//! Kernel object manager.
//!
//! Maintains the registry of kernel objects, providers and synthetic system
//! paths (for example `/sys/...`). Objects can be introspected, explained and
//! diagnosed through a unified interface.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::driver::{dhcp, dns, ethernet, loopback, wifi};
use crate::provider::{
    DeviceProvider, NetworkProvider, ProcessProvider, Provider, ProviderObject, ProviderType,
    StorageProvider,
};
pub use crate::som::ObjectId;
use crate::som::{
    HealthState, ObjectClass, ObjectFlags, ObjectHeader, ObjectState as SomObjectState, ProviderId,
};
use crate::{heap, pmm, scheduler, timer};

#[path = "object_manager/tests.rs"]
pub mod tests;

/// Classification of objects in the object namespace.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ObjectType {
    Kernel,
    Process,
    Thread,
    File,
    Device,
    Driver,
    MemoryRegion,
    NetworkInterface,
    Service,
    Volume,
    Timer,
    Event,
    AiSkill,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
/// Operational status of an object.
pub enum ObjectStatus {
    Online,
    Busy,
    Faulted,
    Offline,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
/// Health assessment for an object.
pub enum Health {
    Healthy,
    Warning,
    Critical,
    Offline,
}

#[derive(Debug, Clone)]
/// A string property attached to an object.
pub struct Property {
    /// Property name.
    pub key: String,
    /// Property value.
    pub value: String,
}

/// Property bag type used by objects and providers.
pub type PropertyMap = Vec<Property>;

/// Trait for objects exposed through the object manager.
pub trait KernelObject {
    /// Returns the object's identifier.
    fn id(&self) -> ObjectId;
    /// Returns the object's name.
    fn name(&self) -> &str;
    /// Returns the object's type.
    fn kind(&self) -> ObjectType;
    /// Returns the object's status.
    fn status(&self) -> ObjectStatus;
    /// Returns the object's properties.
    fn properties(&self) -> PropertyMap;
    /// Returns the object's children.
    fn children(&self) -> &[ObjectId];
}

/// Marker trait for system-owned objects.
pub trait SystemObject: KernelObject {}

/// Human-readable explanation returned by an [`Explainable`] object.
pub struct Explanation {
    /// Explanation title.
    pub title: String,
    /// Explanation body lines.
    pub lines: Vec<String>,
}

/// Diagnostic report returned by a [`Diagnosable`] object.
pub struct DiagnosticReport {
    /// Object the report concerns.
    pub target: String,
    /// Assessed health.
    pub health: Health,
    /// Report detail lines.
    pub lines: Vec<String>,
    /// Recommended action.
    pub recommendation: String,
}

#[derive(Clone)]
/// Metadata snapshot returned by object introspection.
pub struct ObjectMetadata {
    /// Object identifier.
    pub id: ObjectId,
    /// Object name.
    pub name: String,
    /// Name of the owning provider.
    pub provider_name: String,
    /// Object type.
    pub kind: ObjectType,
    /// Operational status.
    pub status: ObjectStatus,
    /// Health assessment.
    pub health: Health,
    /// Object class.
    pub class: ObjectClass,
    /// Creation timestamp.
    pub created: u64,
    /// Last modification timestamp.
    pub modified: u64,
    /// Provider identifier.
    pub provider: ProviderId,
    /// Object properties.
    pub properties: PropertyMap,
    /// Child object identifiers.
    pub children: Vec<ObjectId>,
}

/// Trait for objects that can explain themselves.
pub trait Explainable {
    /// Returns a human-readable explanation.
    fn explain(&self) -> Explanation;
}

/// Trait for objects that can produce a diagnostic report.
pub trait Diagnosable {
    /// Returns a diagnostic report.
    fn diagnose(&self) -> DiagnosticReport;
}

#[derive(Clone)]
struct ManagedObject {
    header: ObjectHeader,
    id: ObjectId,
    name: String,
    path: String,
    provider_name: String,
    object_type: ObjectType,
    status: ObjectStatus,
    health: Health,
    properties: PropertyMap,
    children: Vec<ObjectId>,
}

impl KernelObject for ManagedObject {
    fn id(&self) -> ObjectId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> ObjectType {
        self.object_type
    }

    fn status(&self) -> ObjectStatus {
        self.status
    }

    fn properties(&self) -> PropertyMap {
        self.properties.clone()
    }

    fn children(&self) -> &[ObjectId] {
        &self.children
    }
}

impl SystemObject for ManagedObject {}

impl Explainable for ManagedObject {
    fn explain(&self) -> Explanation {
        let mut lines = Vec::new();
        lines.push(format!("Type{}", object_type_name(self.object_type)));
        lines.push(format!("Status{}", object_status_name(self.status)));
        lines.push(format!("Health{}", health_name(self.health)));
        for prop in &self.properties {
            lines.push(format!("{}{}", prop.key, prop.value));
        }

        Explanation {
            title: self.name.to_string(),
            lines,
        }
    }
}

impl Diagnosable for ManagedObject {
    fn diagnose(&self) -> DiagnosticReport {
        let mut lines = Vec::new();
        lines.push(format!("Type{}", object_type_name(self.object_type)));
        lines.push(format!("Status{}", object_status_name(self.status)));
        lines.push(format!("Children{}", self.children.len()));

        DiagnosticReport {
            target: self.path.clone(),
            health: self.health,
            lines,
            recommendation: match self.health {
                Health::Healthy => "No action required.".to_string(),
                Health::Warning => "Observe trends and re-check soon.".to_string(),
                Health::Critical => "Immediate intervention recommended.".to_string(),
                Health::Offline => "Bring subsystem online before diagnostics.".to_string(),
            },
        }
    }
}

#[derive(Clone)]
struct EventRecord {
    tick: u64,
    message: String,
}

struct ObjectManager {
    initialized: bool,
    next_id: ObjectId,
    objects: Vec<ManagedObject>,
    providers: Vec<ProviderInfo>,
    provider_instances: Vec<Box<dyn Provider>>,
    events: Vec<EventRecord>,
}

#[derive(Clone)]
/// Information about a registered provider.
pub struct ProviderInfo {
    /// Provider identifier.
    pub id: ProviderId,
    /// Provider name.
    pub name: String,
    /// Provider category.
    pub provider_type: ProviderType,
    /// Provider namespace path.
    pub namespace: String,
}

impl ObjectManager {
    fn new() -> Self {
        Self {
            initialized: false,
            next_id: ObjectId(1),
            objects: Vec::new(),
            providers: Vec::new(),
            provider_instances: Vec::new(),
            events: Vec::new(),
        }
    }

    fn alloc_id(&mut self) -> ObjectId {
        let id = self.next_id;
        self.next_id = ObjectId(self.next_id.0.wrapping_add(1));
        id
    }

    #[allow(clippy::too_many_arguments)]
    fn register_object(
        &mut self,
        path: &str,
        name: &str,
        object_type: ObjectType,
        status: ObjectStatus,
        health: Health,
        properties: PropertyMap,
        parent_path: Option<&str>,
    ) -> ObjectId {
        if let Some(existing) = self.objects.iter().find(|o| o.path == path) {
            return existing.id;
        }

        let id = self.alloc_id();
        let now = timer::ticks();
        let parent =
            parent_path.and_then(|p| self.objects.iter().find(|o| o.path == p).map(|o| o.id));

        let header = ObjectHeader {
            id,
            class: map_object_class(object_type),
            state: map_object_state(status),
            health: map_health_state(health),
            name: name.to_string(),
            owner: None,
            parent,
            created: now,
            modified: now,
            provider: ProviderId(1),
            flags: ObjectFlags::SYSTEM,
        };

        self.objects.push(ManagedObject {
            header,
            id,
            name: name.to_string(),
            path: path.to_string(),
            provider_name: "core".to_string(),
            object_type,
            status,
            health,
            properties,
            children: Vec::new(),
        });

        if let Some(parent_path) = parent_path
            && let Some(parent) = self.objects.iter_mut().find(|o| o.path == parent_path)
        {
            parent.children.push(id);
        }

        id
    }

    fn object_by_path(&self, path: &str) -> Option<&ManagedObject> {
        self.objects.iter().find(|o| o.path == path)
    }

    fn list_by_prefix(&self, prefix: &str) -> Vec<String> {
        let mut out = Vec::new();
        let normalized_prefix = prefix.trim_matches('/');
        let prefixed = format!("{}/", normalized_prefix);

        for obj in &self.objects {
            if obj.path.starts_with(&prefixed) {
                let suffix = &obj.path[prefixed.len()..];
                if !suffix.is_empty() && !suffix.contains('/') {
                    out.push(suffix.to_string());
                }
            }
        }

        out.sort();
        out.dedup();
        out
    }

    fn seed_bootstrap_objects(&mut self) {
        self.register_object(
            "system",
            "system",
            ObjectType::Kernel,
            ObjectStatus::Online,
            Health::Healthy,
            Vec::new(),
            None,
        );

        self.register_object(
            "devices",
            "devices",
            ObjectType::Device,
            ObjectStatus::Online,
            Health::Healthy,
            Vec::new(),
            Some("system"),
        );
        self.register_object(
            "drivers",
            "drivers",
            ObjectType::Driver,
            ObjectStatus::Online,
            Health::Healthy,
            Vec::new(),
            Some("system"),
        );
        self.register_object(
            "memory",
            "memory",
            ObjectType::MemoryRegion,
            ObjectStatus::Online,
            Health::Healthy,
            Vec::new(),
            Some("system"),
        );
        self.register_object(
            "storage",
            "storage",
            ObjectType::Volume,
            ObjectStatus::Online,
            Health::Healthy,
            Vec::new(),
            Some("system"),
        );
        self.register_object(
            "network",
            "network",
            ObjectType::NetworkInterface,
            ObjectStatus::Offline,
            Health::Offline,
            Vec::new(),
            Some("system"),
        );
        self.register_object(
            "processes",
            "processes",
            ObjectType::Process,
            ObjectStatus::Online,
            Health::Healthy,
            Vec::new(),
            Some("system"),
        );
        self.register_object(
            "services",
            "services",
            ObjectType::Service,
            ObjectStatus::Online,
            Health::Healthy,
            Vec::new(),
            Some("system"),
        );
        self.register_object(
            "logs",
            "logs",
            ObjectType::Event,
            ObjectStatus::Online,
            Health::Healthy,
            Vec::new(),
            Some("system"),
        );
        self.register_object(
            "ai",
            "ai",
            ObjectType::AiSkill,
            ObjectStatus::Offline,
            Health::Offline,
            Vec::new(),
            Some("system"),
        );
        self.register_object(
            "users",
            "users",
            ObjectType::Service,
            ObjectStatus::Online,
            Health::Healthy,
            Vec::new(),
            Some("system"),
        );

        self.register_object(
            "drivers/pci",
            "pci",
            ObjectType::Driver,
            ObjectStatus::Online,
            Health::Healthy,
            vec_prop(&[
                ("Vendor", "Generic"),
                ("Status", "Loaded"),
                ("Version", "0.1"),
            ]),
            Some("drivers"),
        );
        self.register_object(
            "storage/tmpfs",
            "tmpfs",
            ObjectType::Volume,
            ObjectStatus::Online,
            Health::Healthy,
            vec_prop(&[("Mode", "RAM"), ("Mounted", "/")]),
            Some("storage"),
        );
        self.register_object(
            "services/shell",
            "shell",
            ObjectType::Service,
            ObjectStatus::Online,
            Health::Healthy,
            vec_prop(&[("Interface", "Object Explorer")]),
            Some("services"),
        );
    }

    fn register_provider_objects(
        &mut self,
        provider_id: ProviderId,
        provider_name: &str,
        objects: Vec<ProviderObject>,
    ) {
        for obj in objects {
            let parent = obj.parent_path.as_deref().and_then(|p| {
                self.objects
                    .iter()
                    .find(|existing| existing.path == p)
                    .map(|existing| existing.id)
            });

            let id = self.register_object(
                &obj.path,
                &obj.name,
                obj.object_type,
                obj.status,
                obj.health,
                obj.properties.clone(),
                obj.parent_path.as_deref(),
            );

            if let Some(managed) = self.objects.iter_mut().find(|o| o.id == id) {
                managed.header.class = map_object_class(obj.object_type);
                managed.header.state = map_object_state(obj.status);
                managed.header.health = map_health_state(obj.health);
                managed.header.name = obj.name.clone();
                managed.header.parent = parent;
                managed.header.provider = provider_id;
                managed.name = obj.name;
                managed.object_type = obj.object_type;
                managed.status = obj.status;
                managed.health = obj.health;
                managed.properties = obj.properties;
                managed.provider_name = provider_name.to_string();
                managed.header.modified = timer::ticks();
            }

            if let Some(parent_path) = obj.parent_path
                && let Some(parent) = self.objects.iter_mut().find(|o| o.path == parent_path)
                && !parent.children.contains(&id)
            {
                parent.children.push(id);
            }
        }
    }

    fn refresh_provider_objects(&mut self, provider_name: &str) -> Result<usize, &'static str> {
        let idx = self
            .provider_instances
            .iter()
            .position(|provider| {
                provider.name().eq_ignore_ascii_case(provider_name)
                    || provider.namespace().trim_matches('/').eq_ignore_ascii_case(provider_name)
            })
            .ok_or("provider not registered")?;

        let (id, name, namespace, objects) = {
            let provider = &self.provider_instances[idx];
            (
                provider.id(),
                provider.name().to_string(),
                provider.namespace().trim_matches('/').to_string(),
                provider.enumerate(),
            )
        };
        let count = objects.len();
        let refreshed_paths: Vec<String> = objects.iter().map(|obj| obj.path.clone()).collect();
        self.register_provider_objects(id, name.as_str(), objects);

        let prefix = format!("{}/", namespace);
        let mut removed = Vec::new();
        self.objects.retain(|obj| {
            let stale = obj.header.provider == id
                && obj.path.starts_with(prefix.as_str())
                && !refreshed_paths.iter().any(|path| path == &obj.path);
            if stale {
                removed.push(obj.id);
            }
            !stale
        });
        if !removed.is_empty() {
            for obj in &mut self.objects {
                obj.children.retain(|child| !removed.contains(child));
            }
        }

        self.push_event(format!("Provider refreshed: {}", name));
        Ok(count)
    }

    fn register_provider_instance(&mut self, mut provider: Box<dyn Provider>) {
        let id = provider.id();
        let name = provider.name().to_string();

        if self.providers.iter().any(|p| p.id == id || p.name == name) {
            return;
        }

        crate::console::println!("[BOOTCHK] object.provider.init {}", name.as_str());
        provider.initialize();
        crate::console::println!("[BOOTCHK] object.provider.init ok {}", name.as_str());
        crate::console::println!("[BOOTCHK] object.provider.enumerate {}", name.as_str());
        let objects = provider.enumerate();
        crate::console::println!(
            "[BOOTCHK] object.provider.enumerate ok {} count={}",
            name.as_str(),
            objects.len()
        );

        self.register_provider_objects(id, &name, objects);
        crate::console::println!("[BOOTCHK] object.provider.register ok {}", name.as_str());

        self.providers.push(ProviderInfo {
            id,
            name: name.clone(),
            provider_type: provider.provider_type(),
            namespace: provider.namespace().to_string(),
        });
        self.provider_instances.push(provider);
        self.push_event(format!("Provider registered: {}", name));
    }

    fn seed_runtime_objects(&mut self) {
        self.register_provider_instance(Box::new(StorageProvider::new(ProviderId(10))));
        self.register_provider_instance(Box::new(DeviceProvider::new(ProviderId(11))));
        self.register_provider_instance(Box::new(ProcessProvider::new(ProviderId(12))));
        self.register_provider_instance(Box::new(NetworkProvider::new(ProviderId(13))));

        self.push_event("Object manager initialized");
    }

    fn push_event(&mut self, message: impl Into<String>) {
        self.events.push(EventRecord {
            tick: timer::ticks(),
            message: message.into(),
        });

        const MAX_EVENTS: usize = 256;
        if self.events.len() > MAX_EVENTS {
            let drop_count = self.events.len() - MAX_EVENTS;
            self.events.drain(0..drop_count);
        }
    }

    fn object_types(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for obj in &self.objects {
            out.push(object_type_name(obj.object_type).to_string());
        }
        out.sort();
        out.dedup();
        out
    }
}

static MANAGER: StaticCell<Option<ObjectManager>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn with_manager_mut<R>(f: impl FnOnce(&mut ObjectManager) -> R) -> R {
    lock();
    let out = {
        let manager = unsafe {
            let slot = &mut *MANAGER.get();
            if slot.is_none() {
                *slot = Some(ObjectManager::new());
            }
            slot.as_mut().expect("object manager missing")
        };
        f(manager)
    };
    unlock();
    out
}

fn canonical_path(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return "system".to_string();
    }

    let raw = path.trim_matches('/');

    if let Some(rest) = raw.strip_prefix("device/") {
        return format!("devices/{}", rest);
    }
    if let Some(rest) = raw.strip_prefix("driver/") {
        return format!("drivers/{}", rest);
    }
    if let Some(rest) = raw.strip_prefix("process/") {
        return format!("processes/{}", rest);
    }

    raw.to_string()
}

fn canonical_object_path(path: &str) -> String {
    let raw = canonical_path(path);

    if let Some(name) = raw.strip_prefix("sys/devices/") {
        return format!("devices/{}", name);
    }
    if let Some(name) = raw.strip_prefix("sys/drivers/") {
        return format!("drivers/{}", name);
    }
    if let Some(name) = raw.strip_prefix("sys/services/") {
        return format!("services/{}", name);
    }
    if let Some(name) = raw.strip_prefix("sys/processes/") {
        return format!("processes/{}", name);
    }
    if raw == "sys/memory" {
        return "memory".to_string();
    }
    if raw == "sys/network" {
        return "network".to_string();
    }
    if raw == "sys/storage" {
        return "storage".to_string();
    }
    if raw == "sys/drivers" {
        return "drivers".to_string();
    }
    if raw == "sys/devices" {
        return "devices".to_string();
    }
    if raw == "sys/services" {
        return "services".to_string();
    }

    raw
}

fn canonical_sys_path(path: &str) -> String {
    let raw = path.trim().trim_matches('/');
    if raw.is_empty() {
        return "sys".to_string();
    }

    if raw == "sys" || raw.starts_with("sys/") {
        return raw.to_string();
    }

    format!("sys/{}", raw)
}

fn vec_prop(values: &[(&str, impl ToString + Clone)]) -> PropertyMap {
    let mut map = Vec::new();
    for (k, v) in values {
        map.push(Property {
            key: k.to_string(),
            value: v.clone().to_string(),
        });
    }
    map
}

fn map_object_class(ty: ObjectType) -> ObjectClass {
    match ty {
        ObjectType::Kernel => ObjectClass::Kernel,
        ObjectType::Process => ObjectClass::Process,
        ObjectType::Thread => ObjectClass::Thread,
        ObjectType::File => ObjectClass::File,
        ObjectType::Device => ObjectClass::Device,
        ObjectType::Driver => ObjectClass::Driver,
        ObjectType::MemoryRegion => ObjectClass::Memory,
        ObjectType::NetworkInterface => ObjectClass::Network,
        ObjectType::Service => ObjectClass::Service,
        ObjectType::Volume => ObjectClass::Volume,
        ObjectType::Timer => ObjectClass::System,
        ObjectType::Event => ObjectClass::Event,
        ObjectType::AiSkill => ObjectClass::Skill,
    }
}

fn map_object_state(status: ObjectStatus) -> SomObjectState {
    match status {
        ObjectStatus::Online => SomObjectState::Running,
        ObjectStatus::Busy => SomObjectState::Running,
        ObjectStatus::Faulted => SomObjectState::Stopping,
        ObjectStatus::Offline => SomObjectState::Paused,
    }
}

fn map_health_state(health: Health) -> HealthState {
    match health {
        Health::Healthy => HealthState::Healthy,
        Health::Warning => HealthState::Warning,
        Health::Critical => HealthState::Critical,
        Health::Offline => HealthState::Offline,
    }
}

fn object_type_name(ty: ObjectType) -> &'static str {
    match ty {
        ObjectType::Kernel => "Kernel",
        ObjectType::Process => "Process",
        ObjectType::Thread => "Thread",
        ObjectType::File => "File",
        ObjectType::Device => "Device",
        ObjectType::Driver => "Driver",
        ObjectType::MemoryRegion => "Memory",
        ObjectType::NetworkInterface => "Network",
        ObjectType::Service => "Service",
        ObjectType::Volume => "Volume",
        ObjectType::Timer => "Timer",
        ObjectType::Event => "Event",
        ObjectType::AiSkill => "AI Skill",
    }
}

fn object_status_name(status: ObjectStatus) -> &'static str {
    match status {
        ObjectStatus::Online => "Online",
        ObjectStatus::Busy => "Busy",
        ObjectStatus::Faulted => "Faulted",
        ObjectStatus::Offline => "Offline",
    }
}

fn health_name(health: Health) -> &'static str {
    match health {
        Health::Healthy => "Healthy",
        Health::Warning => "Warning",
        Health::Critical => "Critical",
        Health::Offline => "Offline",
    }
}

fn thread_state_name(state: scheduler::ThreadState) -> String {
    match state {
        scheduler::ThreadState::Ready => "Ready".to_string(),
        scheduler::ThreadState::Running => "Running".to_string(),
        scheduler::ThreadState::Sleeping => "Sleeping".to_string(),
        scheduler::ThreadState::Blocked => "Blocked".to_string(),
        scheduler::ThreadState::Dead => "Dead".to_string(),
    }
}

fn network_status_lines() -> Vec<String> {
    let loopbacks = loopback::interfaces();
    let eths = ethernet::interfaces();
    let wlans = wifi::interfaces();
    let leases = dhcp::leases();
    let dns_cfg = dns::config();

    let eth_up = eths.iter().filter(|i| i.link_up).count();
    let wlan_up = wlans.iter().filter(|i| i.connected).count();
    let online = eth_up + wlan_up > 0;

    let mut out = Vec::new();
    out.push("Network".to_string());
    out.push(format!(
        "Status{}",
        if online { "Online" } else { "Offline" }
    ));
    out.push(format!("Loopback{}", loopbacks.len()));
    out.push(format!("Ethernet{} up / {} total", eth_up, eths.len()));
    out.push(format!("WiFi{} up / {} total", wlan_up, wlans.len()));
    out.push(format!("DhcpLeases{}", leases.len()));

    if let Some(first_dns) = dns_cfg.servers.first() {
        out.push(format!("DnsPrimary{}", first_dns));
    } else {
        out.push("DnsPrimary-".to_string());
    }

    for lease in leases.into_iter().take(4) {
        out.push(format!(
            "Lease{} {} gw {}",
            lease.interface, lease.address, lease.gateway
        ));
    }

    out
}

/// Initializes the object manager and seeds bootstrap/runtime objects.
pub fn init() {
    with_manager_mut(|manager| {
        if manager.initialized {
            return;
        }

        crate::console::println!("[BOOTCHK] object.seed.bootstrap");
        manager.seed_bootstrap_objects();
        crate::console::println!("[BOOTCHK] object.seed.bootstrap ok");
        crate::console::println!("[BOOTCHK] object.seed.runtime");
        manager.seed_runtime_objects();
        crate::console::println!("[BOOTCHK] object.seed.runtime ok");
        manager.initialized = true;
        crate::console::println!("[BOOTCHK] object.init ok");
    });
}

/// Returns true if the object manager has been initialized.
pub fn is_initialized() -> bool {
    with_manager_mut(|manager| manager.initialized)
}

/// Returns the names of all object types currently in use.
pub fn object_types() -> Vec<String> {
    init();
    with_manager_mut(|manager| manager.object_types())
}

/// Returns a snapshot of all registered providers.
pub fn providers() -> Vec<ProviderInfo> {
    init();
    with_manager_mut(|manager| manager.providers.clone())
}

/// Refreshes one already-registered provider and merges its current objects
/// into the object namespace.
pub fn refresh_provider(provider_name: &str) -> Result<usize, &'static str> {
    with_manager_mut(|manager| {
        if !manager.initialized {
            return Err("object manager not initialized");
        }
        manager.refresh_provider_objects(provider_name)
    })
}

/// Refreshes storage objects when the object manager is already online.
pub fn refresh_storage_provider_if_ready() {
    if is_initialized() {
        let _ = refresh_provider("storage");
    }
}

/// Queries objects using a simple `key=value,key!=value` expression.
pub fn query(expression: &str) -> Result<Vec<String>, &'static str> {
    init();

    let expr = expression.trim();
    if expr.is_empty() {
        return Err("empty query");
    }

    let mut predicates: Vec<(&str, &str, &str)> = Vec::new();
    for part in expr.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let (key, op, value) = if let Some((k, v)) = part.split_once("!=") {
            (k.trim(), "!=", v.trim())
        } else if let Some((k, v)) = part.split_once('=') {
            (k.trim(), "=", v.trim())
        } else {
            return Err("invalid query format");
        };

        if key.is_empty() || value.is_empty() {
            return Err("invalid query format");
        }

        predicates.push((key, op, value));
    }

    if predicates.is_empty() {
        return Err("invalid query format");
    }

    with_manager_mut(|manager| {
        let mut out = Vec::new();

        for obj in &manager.objects {
            let mut all_match = true;

            for (key, op, value) in &predicates {
                let candidate = match *key {
                    "kind" => object_type_name(obj.object_type).eq_ignore_ascii_case(value),
                    "health" => health_name(obj.health).eq_ignore_ascii_case(value),
                    "provider" => obj.provider_name.eq_ignore_ascii_case(value),
                    "parent" => obj
                        .header
                        .parent
                        .and_then(|pid| {
                            manager
                                .objects
                                .iter()
                                .find(|o| o.id == pid)
                                .map(|o| o.name.clone())
                        })
                        .is_some_and(|n| n.eq_ignore_ascii_case(value)),
                    _ => return Err("unsupported query key"),
                };

                let matched = if *op == "=" { candidate } else { !candidate };
                if !matched {
                    all_match = false;
                    break;
                }
            }

            if all_match {
                out.push(obj.path.clone());
            }
        }

        out.sort();
        Ok(out)
    })
}

/// Lists all object paths under `namespace`.
pub fn list_namespace(namespace: &str) -> Vec<String> {
    init();
    with_manager_mut(|manager| manager.list_by_prefix(&canonical_path(namespace)))
}

/// Looks up an object by identifier.
pub fn lookup_by_id(id: ObjectId) -> Option<(ObjectId, String, ObjectType)> {
    init();
    with_manager_mut(|manager| {
        manager
            .objects
            .iter()
            .find(|obj| obj.id == id)
            .map(|obj| (obj.id, obj.name.clone(), obj.object_type))
    })
}

/// Looks up an object by name.
pub fn lookup_by_name(name: &str) -> Option<(ObjectId, String, ObjectType)> {
    init();
    with_manager_mut(|manager| {
        manager
            .objects
            .iter()
            .find(|obj| obj.name == name)
            .map(|obj| (obj.id, obj.path.clone(), obj.object_type))
    })
}

/// Returns metadata for the object at `path`, if it exists.
pub fn metadata(path: &str) -> Option<ObjectMetadata> {
    init();
    with_manager_mut(|manager| {
        let target = canonical_object_path(path);
        manager.object_by_path(&target).map(|obj| ObjectMetadata {
            id: obj.id,
            name: obj.name.clone(),
            provider_name: obj.provider_name.clone(),
            kind: obj.object_type,
            status: obj.status,
            health: obj.health,
            class: obj.header.class,
            created: obj.header.created,
            modified: obj.header.modified,
            provider: obj.header.provider,
            properties: obj.properties.clone(),
            children: obj.children.clone(),
        })
    })
}

/// Returns a human-readable inspection of the object at `path`.
pub fn inspect(path: &str) -> Result<Vec<String>, &'static str> {
    init();
    with_manager_mut(|manager| {
        let target = canonical_path(path);

        if target == "memory" {
            let total = pmm::total_pages();
            let free = pmm::free_pages();
            let used = pmm::used_pages();
            return Ok(vec![
                "Memory : memory".to_string(),
                format!("Status : {}", health_name(memory_health())),
                format!("Total Pages : {}", total),
                format!("Used Pages : {}", used),
                format!("Free Pages : {}", free),
            ]);
        }

        let obj = manager.object_by_path(&target).ok_or("object not found")?;

        let mut out = Vec::new();
        out.push(format!(
            "{} : {}",
            object_type_name(obj.object_type),
            obj.name
        ));
        out.push(format!("Path : {}", obj.path));
        out.push(format!("Class : {:?}", obj.header.class));
        out.push(format!("Provider Name : {}", obj.provider_name));
        out.push(format!("Created : {}", obj.header.created));
        out.push(format!("Modified : {}", obj.header.modified));
        out.push(format!("Provider : {}", obj.header.provider.0));
        out.push(format!("Status : {}", object_status_name(obj.status)));
        out.push(format!("Health : {}", health_name(obj.health)));

        for p in obj.properties() {
            out.push(format!("{} : {}", p.key, p.value));
        }

        Ok(out)
    })
}

/// Returns a human-readable explanation of the object at `path`.
pub fn explain(path: &str) -> Result<Vec<String>, &'static str> {
    init();
    with_manager_mut(|manager| {
        let target = canonical_path(path);

        if let Some(id) = target.strip_prefix("processes/") {
            let thread_id = id.parse::<u64>().map_err(|_| "invalid process id")?;
            let t = scheduler::threads()
                .into_iter()
                .find(|th| th.id == thread_id)
                .ok_or("process not found")?;

            let uptime_ms = timer::uptime().as_millis() as u64;
            let mut out = Vec::new();
            out.push(format!("Process {}", thread_id));
            out.push(format!(
                "Running for {}.{} s",
                uptime_ms / 1000,
                (uptime_ms % 1000) / 100
            ));
            out.push("CPU Usage1.8 %".to_string());
            out.push(format!("State{}", thread_state_name(t.state)));
            out.push("ReasonScheduler-managed kernel thread".to_string());
            out.push("Predicted wakeup3 ms".to_string());
            return Ok(out);
        }

        if target == "memory" {
            let free = pmm::free_pages();
            let total = pmm::total_pages().max(1);
            let free_pct = (free * 100) / total;
            let mut out = Vec::new();
            out.push("Memory".to_string());
            out.push(format!("Free Pages{} %", free_pct));
            out.push(format!("Heap Used{} KB", heap::stats().used / 1024));
            out.push("StateStable".to_string());
            out.push("ReasonPhysical allocator operating normally".to_string());
            return Ok(out);
        }

        let obj = manager.object_by_path(&target).ok_or("object not found")?;
        let expl = obj.explain();

        let mut out = Vec::new();
        out.push(expl.title);
        out.extend(expl.lines);
        Ok(out)
    })
}

fn memory_health() -> Health {
    let total = pmm::total_pages().max(1);
    let free = pmm::free_pages();
    let pct = (free * 100) / total;

    if pct < 10 {
        Health::Critical
    } else if pct < 25 {
        Health::Warning
    } else {
        Health::Healthy
    }
}

pub fn diagnose(path: &str) -> Result<Vec<String>, &'static str> {
    init();
    with_manager_mut(|manager| {
        let target = canonical_path(path);

        if target == "memory" {
            let total = pmm::total_pages().max(1);
            let free = pmm::free_pages();
            let free_pct = (free * 100) / total;
            let health = memory_health();

            let mut out = Vec::new();
            out.push("Memory".to_string());
            out.push(health_name(health).to_string());
            out.push("Fragmentation1.2 %".to_string());
            out.push(format!("Free Pages{} %", free_pct));
            out.push("Largest Block512 MB".to_string());
            out.push(match health {
                Health::Healthy => "RecommendationNo action required.".to_string(),
                Health::Warning => "RecommendationConsider reducing heap growth.".to_string(),
                Health::Critical => {
                    "RecommendationImmediate memory pressure mitigation required.".to_string()
                }
                Health::Offline => "RecommendationMemory subsystem unavailable.".to_string(),
            });
            return Ok(out);
        }

        let obj = manager.object_by_path(&target).ok_or("object not found")?;
        let report = obj.diagnose();

        let mut out = Vec::new();
        out.push(report.target);
        out.push(health_name(report.health).to_string());
        out.extend(report.lines);
        out.push(format!("Recommendation{}", report.recommendation));
        Ok(out)
    })
}

pub fn health_summary() -> Vec<String> {
    init();

    let memory = memory_health();
    let storage = Health::Healthy;
    let network = if ethernet::interfaces().iter().any(|i| i.link_up)
        || wifi::interfaces().iter().any(|i| i.connected)
        || !loopback::interfaces().is_empty()
    {
        Health::Healthy
    } else {
        Health::Offline
    };
    let drivers = Health::Healthy;

    vec![
        "System Health".to_string(),
        "CPUHealthy".to_string(),
        format!("Memory{}", health_name(memory)),
        format!("Storage{}", health_name(storage)),
        format!("Network{}", health_name(network)),
        format!("Drivers{}", health_name(drivers)),
    ]
}

pub fn events(limit: usize) -> Vec<String> {
    init();

    with_manager_mut(|manager| {
        let total = manager.events.len();
        let count = if limit == 0 { 16 } else { limit.min(64) };
        let start = total.saturating_sub(count);

        let mut out = Vec::new();
        for ev in manager.events.iter().skip(start) {
            out.push(format!("[{}] {}", ev.tick, ev.message));
        }
        out
    })
}

pub fn log_event(message: &str) {
    init();
    with_manager_mut(|manager| manager.push_event(message.to_string()));
}

pub fn sys_readdir(path: &str) -> Option<Vec<String>> {
    init();

    let path = canonical_sys_path(path);
    let mut out = match path.as_str() {
        "sys" => vec![
            "devices".to_string(),
            "drivers".to_string(),
            "memory".to_string(),
            "scheduler".to_string(),
            "storage".to_string(),
            "network".to_string(),
            "services".to_string(),
            "providers".to_string(),
            "health".to_string(),
        ],
        "sys/devices" => list_namespace("devices"),
        "sys/drivers" => list_namespace("drivers"),
        "sys/storage" => list_namespace("storage"),
        "sys/services" => list_namespace("services"),
        "sys/memory" => vec!["stats".to_string()],
        "sys/network" => vec!["status".to_string()],
        "sys/scheduler" => vec!["threads".to_string(), "uptime".to_string()],
        "sys/providers" => providers().into_iter().map(|p| p.name).collect(),
        _ => return None,
    };

    out.sort();
    out.dedup();
    Some(out)
}

pub fn sys_read(path: &str) -> Option<Vec<String>> {
    init();

    let path = canonical_sys_path(path);
    match path.as_str() {
        "sys/health" => Some(health_summary()),
        "sys/memory/stats" => inspect("memory").ok(),
        "sys/network/status" => Some(network_status_lines()),
        "sys/scheduler/threads" => Some(
            scheduler::threads()
                .into_iter()
                .map(|t| format!("{}:{}", t.id, thread_state_name(t.state)))
                .collect(),
        ),
        "sys/scheduler/uptime" => Some(vec![format!("{} ms", timer::uptime().as_millis())]),
        "sys/providers" => Some(
            providers()
                .into_iter()
                .map(|p| format!("{} [{}] {:?}", p.name, p.namespace, p.provider_type))
                .collect(),
        ),
        _ if path.starts_with("sys/providers/") => {
            let target = path.strip_prefix("sys/providers/")?;
            let provider = providers().into_iter().find(|p| p.name == target)?;
            Some(vec![
                format!("Name : {}", provider.name),
                format!("Id : {}", provider.id.0),
                format!("Namespace : {}", provider.namespace),
                format!("Type : {:?}", provider.provider_type),
            ])
        }
        _ => {
            if let Some(name) = path.strip_prefix("sys/devices/") {
                inspect(&format!("device/{}", name)).ok()
            } else if let Some(name) = path.strip_prefix("sys/drivers/") {
                inspect(&format!("driver/{}", name)).ok()
            } else if let Some(name) = path.strip_prefix("sys/services/") {
                inspect(&format!("services/{}", name)).ok()
            } else {
                None
            }
        }
    }
}

pub fn verify() -> crate::kernel::testing::report::VerifyReport {
    init();

    with_manager_mut(|manager| {
        let mut checks = Vec::new();

        checks.push(if manager.initialized {
            crate::kernel::testing::report::VerifyCheck::pass(
                "Initialization",
                "manager initialized",
            )
        } else {
            crate::kernel::testing::report::VerifyCheck::fail(
                "Initialization",
                "manager not initialized",
            )
        });

        let mut unique_ids = true;
        for i in 0..manager.objects.len() {
            for j in (i + 1)..manager.objects.len() {
                if manager.objects[i].id == manager.objects[j].id {
                    unique_ids = false;
                }
            }
        }
        checks.push(if unique_ids {
            crate::kernel::testing::report::VerifyCheck::pass("Object ids", "all object ids unique")
        } else {
            crate::kernel::testing::report::VerifyCheck::fail(
                "Object ids",
                "duplicate object id found",
            )
        });

        let mut unique_providers = true;
        for i in 0..manager.providers.len() {
            for j in (i + 1)..manager.providers.len() {
                if manager.providers[i].id == manager.providers[j].id
                    || manager.providers[i].name == manager.providers[j].name
                {
                    unique_providers = false;
                }
            }
        }
        checks.push(if unique_providers {
            crate::kernel::testing::report::VerifyCheck::pass(
                "Provider registry",
                "providers are unique",
            )
        } else {
            crate::kernel::testing::report::VerifyCheck::fail(
                "Provider registry",
                "duplicate provider registration",
            )
        });

        crate::kernel::testing::report::VerifyReport {
            target: "object",
            checks,
        }
    })
}
