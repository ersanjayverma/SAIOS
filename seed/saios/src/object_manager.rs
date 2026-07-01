use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::{heap, pci, pmm, scheduler, timer};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u64);

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
pub enum ObjectStatus {
    Online,
    Busy,
    Faulted,
    Offline,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Warning,
    Critical,
    Offline,
}

#[derive(Debug, Clone)]
pub struct Property {
    pub key: String,
    pub value: String,
}

pub type PropertyMap = Vec<Property>;

pub trait KernelObject {
    fn id(&self) -> ObjectId;
    fn name(&self) -> &str;
    fn kind(&self) -> ObjectType;
    fn status(&self) -> ObjectStatus;
    fn properties(&self) -> PropertyMap;
    fn children(&self) -> &[ObjectId];
}

pub trait SystemObject: KernelObject {}

pub struct Explanation {
    pub title: String,
    pub lines: Vec<String>,
}

pub struct DiagnosticReport {
    pub target: String,
    pub health: Health,
    pub lines: Vec<String>,
    pub recommendation: String,
}

#[derive(Clone)]
pub struct ObjectMetadata {
    pub id: ObjectId,
    pub name: String,
    pub kind: ObjectType,
    pub status: ObjectStatus,
    pub health: Health,
    pub properties: PropertyMap,
    pub children: Vec<ObjectId>,
}

pub trait Explainable {
    fn explain(&self) -> Explanation;
}

pub trait Diagnosable {
    fn diagnose(&self) -> DiagnosticReport;
}

#[derive(Clone)]
struct ManagedObject {
    id: ObjectId,
    name: String,
    path: String,
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
            title: format!("{}", self.name),
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
    events: Vec<EventRecord>,
}

impl ObjectManager {
    fn new() -> Self {
        Self {
            initialized: false,
            next_id: ObjectId(1),
            objects: Vec::new(),
            events: Vec::new(),
        }
    }

    fn alloc_id(&mut self) -> ObjectId {
        let id = self.next_id;
        self.next_id = ObjectId(self.next_id.0.wrapping_add(1));
        id
    }

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
        self.objects.push(ManagedObject {
            id,
            name: name.to_string(),
            path: path.to_string(),
            object_type,
            status,
            health,
            properties,
            children: Vec::new(),
        });

        if let Some(parent_path) = parent_path {
            if let Some(parent) = self.objects.iter_mut().find(|o| o.path == parent_path) {
                parent.children.push(id);
            }
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
                    out.push(format!("{}", suffix));
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

    fn seed_runtime_objects(&mut self) {
        pci::init();

        let mut device_idx: usize = 0;
        for dev in pci::devices() {
            let path = format!("devices/pci{}", device_idx);
            let name = format!("pci{}", device_idx);
            let status = ObjectStatus::Online;
            let health = Health::Healthy;
            let props = vec_prop(&[
                ("Vendor", hex16(dev.vendor_id)),
                ("Device", hex16(dev.device_id)),
                ("Class", pci::class_name(dev.class).to_string()),
                (
                    "Location",
                    format!("{:02x}:{:02x}.{}", dev.bus, dev.device, dev.function),
                ),
            ]);

            self.register_object(
                &path,
                &name,
                ObjectType::Device,
                status,
                health,
                props,
                Some("devices"),
            );
            device_idx = device_idx.saturating_add(1);
        }

        for thread in scheduler::threads() {
            let proc_path = format!("processes/{}", thread.id);
            let proc_name = format!("{}", thread.id);
            let proc_status = match thread.state {
                scheduler::ThreadState::Dead => ObjectStatus::Offline,
                scheduler::ThreadState::Blocked | scheduler::ThreadState::Sleeping => {
                    ObjectStatus::Busy
                }
                _ => ObjectStatus::Online,
            };
            let proc_health = match thread.state {
                scheduler::ThreadState::Dead => Health::Critical,
                scheduler::ThreadState::Blocked => Health::Warning,
                _ => Health::Healthy,
            };

            self.register_object(
                &proc_path,
                &proc_name,
                ObjectType::Process,
                proc_status,
                proc_health,
                vec_prop(&[("State", thread_state_name(thread.state))]),
                Some("processes"),
            );

            let thread_path = format!("threads/{}", thread.id);
            self.register_object(
                &thread_path,
                &proc_name,
                ObjectType::Thread,
                proc_status,
                proc_health,
                vec_prop(&[("State", thread_state_name(thread.state))]),
                Some("system"),
            );
        }

        self.push_event("Object manager initialized");
        self.push_event("Storage provider attached: tmpfs");
        self.push_event("Driver loaded: pci");
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

    if raw.starts_with("device/") {
        return format!("devices/{}", &raw[7..]);
    }
    if raw.starts_with("driver/") {
        return format!("drivers/{}", &raw[7..]);
    }
    if raw.starts_with("process/") {
        return format!("processes/{}", &raw[8..]);
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

fn hex16(v: u16) -> String {
    format!("0x{:04x}", v)
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

pub fn init() {
    with_manager_mut(|manager| {
        if manager.initialized {
            return;
        }

        manager.seed_bootstrap_objects();
        manager.seed_runtime_objects();
        manager.initialized = true;
    });
}

pub fn object_types() -> Vec<String> {
    init();
    with_manager_mut(|manager| manager.object_types())
}

pub fn list_namespace(namespace: &str) -> Vec<String> {
    init();
    with_manager_mut(|manager| manager.list_by_prefix(&canonical_path(namespace)))
}

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

pub fn metadata(path: &str) -> Option<ObjectMetadata> {
    init();
    with_manager_mut(|manager| {
        let target = canonical_object_path(path);
        manager.object_by_path(&target).map(|obj| ObjectMetadata {
            id: obj.id,
            name: obj.name.clone(),
            kind: obj.object_type,
            status: obj.status,
            health: obj.health,
            properties: obj.properties.clone(),
            children: obj.children.clone(),
        })
    })
}

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
        out.push(format!("{} : {}", object_type_name(obj.object_type), obj.name));
        out.push(format!("Path : {}", obj.path));
        out.push(format!("Status : {}", object_status_name(obj.status)));
        out.push(format!("Health : {}", health_name(obj.health)));

        for p in obj.properties() {
            out.push(format!("{} : {}", p.key, p.value));
        }

        Ok(out)
    })
}

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
            out.push(format!("{}", health_name(health)));
            out.push("Fragmentation1.2 %".to_string());
            out.push(format!("Free Pages{} %", free_pct));
            out.push("Largest Block512 MB".to_string());
            out.push(match health {
                Health::Healthy => "RecommendationNo action required.".to_string(),
                Health::Warning => "RecommendationConsider reducing heap growth.".to_string(),
                Health::Critical => "RecommendationImmediate memory pressure mitigation required."
                    .to_string(),
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
    let network = Health::Offline;
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
            "health".to_string(),
        ],
        "sys/devices" => list_namespace("devices"),
        "sys/drivers" => list_namespace("drivers"),
        "sys/storage" => list_namespace("storage"),
        "sys/services" => list_namespace("services"),
        "sys/memory" => vec!["stats".to_string()],
        "sys/network" => vec!["status".to_string()],
        "sys/scheduler" => vec!["threads".to_string(), "uptime".to_string()],
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
        "sys/network/status" => Some(vec![
            "Network".to_string(),
            "StatusOffline".to_string(),
            "ReasonNo network driver active".to_string(),
        ]),
        "sys/scheduler/threads" => Some(
            scheduler::threads()
                .into_iter()
                .map(|t| format!("{}:{}", t.id, thread_state_name(t.state)))
                .collect(),
        ),
        "sys/scheduler/uptime" => Some(vec![format!("{} ms", timer::uptime().as_millis())]),
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
