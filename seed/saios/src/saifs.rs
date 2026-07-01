use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::som::{EventId, HandleId, ObjectId, OperationId, ProviderId};
use crate::object_manager::{self, Health, ObjectMetadata, ObjectStatus, ObjectType, Property, PropertyMap};
use crate::vfs;

pub use crate::som::KernelObject;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SaifsNodeKind {
    File,
    Directory,
    Object,
    Virtual,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CreateKind {
    File,
    Directory,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SaifsError {
    NotFound,
    AlreadyExists,
    InvalidPath,
    InvalidHandle,
    UnsupportedOperation,
    AccessDenied,
    ProviderUnavailable,
    Busy,
    Corrupt,
    Internal,
}

pub struct LookupContext;

#[derive(Clone)]
pub struct LookupResult {
    pub object_id: Option<ObjectId>,
    pub kind: SaifsNodeKind,
}

#[derive(Clone)]
pub struct DirEntry {
    pub name: String,
    pub kind: SaifsNodeKind,
}

pub trait NamespaceProvider {
    fn id(&self) -> ProviderId;
    fn name(&self) -> &str;
    fn lookup(&self, ctx: &LookupContext, path: &str) -> Result<LookupResult, SaifsError>;
    fn enumerate(&self, ctx: &LookupContext, path: &str) -> Result<Vec<DirEntry>, SaifsError>;
    fn create(&self, ctx: &LookupContext, path: &str, kind: CreateKind) -> Result<ObjectId, SaifsError>;
    fn remove(&self, ctx: &LookupContext, path: &str) -> Result<(), SaifsError>;
}

pub trait Handle {
    fn id(&self) -> HandleId;
    fn object_id(&self) -> Option<ObjectId>;
    fn provider_id(&self) -> ProviderId;
    fn read(&self) -> Result<Vec<u8>, SaifsError>;
    fn write(&self, data: &[u8]) -> Result<usize, SaifsError>;
    fn query(&self, key: &str) -> Result<Property, SaifsError>;
    fn properties(&self) -> Result<PropertyMap, SaifsError>;
    fn health(&self) -> Result<Health, SaifsError>;
    fn children(&self) -> Result<Vec<String>, SaifsError>;
}

pub trait OperationDispatcher {
    fn supports(&self, object: ObjectId, op: OperationId) -> bool;
    fn invoke(&self, handle: HandleId, op: OperationId, args: &[u8]) -> Result<Vec<u8>, SaifsError>;
}

#[derive(Clone)]
pub struct MountPoint {
    pub path: String,
    pub provider: ProviderId,
    pub read_only: bool,
}

pub trait ProviderRegistry {
    fn register(&self, provider: &'static dyn NamespaceProvider) -> Result<ProviderId, SaifsError>;
    fn get(&self, provider: ProviderId) -> Option<&'static dyn NamespaceProvider>;
    fn list(&self) -> Vec<ProviderId>;
}

pub trait MountManager {
    fn mount(&self, mount: MountPoint) -> Result<(), SaifsError>;
    fn unmount(&self, path: &str) -> Result<(), SaifsError>;
    fn resolve_provider(&self, path: &str) -> Result<ProviderId, SaifsError>;
    fn mounts(&self) -> Vec<MountPoint>;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EventType {
    ObjectCreated,
    ObjectRemoved,
    PropertyChanged,
    HealthChanged,
    Mounted,
    Unmounted,
    DriverLoaded,
    DeviceAttached,
    MemoryAllocated,
}

#[derive(Clone)]
pub struct Event {
    pub id: EventId,
    pub event_type: EventType,
    pub object: Option<ObjectId>,
    pub payload: String,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SubscriptionId(pub u64);

pub trait EventBus {
    fn publish(&self, event: Event) -> Result<(), SaifsError>;
    fn subscribe(&self) -> Result<SubscriptionId, SaifsError>;
    fn unsubscribe(&self, id: SubscriptionId) -> Result<(), SaifsError>;
}

struct RegisteredProvider {
    id: ProviderId,
    provider: &'static dyn NamespaceProvider,
}

struct SaifsState {
    initialized: bool,
    next_provider_id: ProviderId,
    next_handle_id: HandleId,
    next_event_id: EventId,
    next_subscription_id: SubscriptionId,
    providers: Vec<RegisteredProvider>,
    mounts: Vec<MountPoint>,
    events: Vec<Event>,
    subscriptions: Vec<SubscriptionId>,
}

impl SaifsState {
    fn new() -> Self {
        Self {
            initialized: false,
            next_provider_id: ProviderId(1),
            next_handle_id: HandleId(1),
            next_event_id: EventId(1),
            next_subscription_id: SubscriptionId(1),
            providers: Vec::new(),
            mounts: Vec::new(),
            events: Vec::new(),
            subscriptions: Vec::new(),
        }
    }

    fn alloc_provider_id(&mut self) -> ProviderId {
        let id = self.next_provider_id;
        self.next_provider_id = ProviderId(self.next_provider_id.0.wrapping_add(1));
        id
    }

    fn alloc_handle_id(&mut self) -> HandleId {
        let id = self.next_handle_id;
        self.next_handle_id = HandleId(self.next_handle_id.0.wrapping_add(1));
        id
    }

    fn alloc_event_id(&mut self) -> EventId {
        let id = self.next_event_id;
        self.next_event_id = EventId(self.next_event_id.0.wrapping_add(1));
        id
    }

    fn alloc_subscription_id(&mut self) -> SubscriptionId {
        let id = self.next_subscription_id;
        self.next_subscription_id = SubscriptionId(self.next_subscription_id.0.wrapping_add(1));
        id
    }
}

static STATE: StaticCell<Option<SaifsState>> = StaticCell::new(None);
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

fn with_state<R>(f: impl FnOnce(&mut SaifsState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(SaifsState::new());
            }
            slot.as_mut().expect("saifs state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return "/".to_string();
    }
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    }
}

struct DefaultVfsProvider;

impl NamespaceProvider for DefaultVfsProvider {
    fn id(&self) -> ProviderId {
        ProviderId(1)
    }

    fn name(&self) -> &str {
        "vfs"
    }

    fn lookup(&self, _ctx: &LookupContext, path: &str) -> Result<LookupResult, SaifsError> {
        let path = normalize_path(path);

        if path == "/sys" || path.starts_with("/sys/") {
            if object_manager::metadata(&path).is_some() {
                return Ok(LookupResult {
                    object_id: object_manager::metadata(&path).map(|m| m.id),
                    kind: SaifsNodeKind::Object,
                });
            }

            if object_manager::sys_readdir(&path).is_some() || object_manager::sys_read(&path).is_some() {
                return Ok(LookupResult {
                    object_id: None,
                    kind: SaifsNodeKind::Virtual,
                });
            }
        }

        let node = vfs::open(&path).map_err(map_str_err)?;
        let kind = match node.kind {
            vfs::FileType::File => SaifsNodeKind::File,
            vfs::FileType::Directory => SaifsNodeKind::Directory,
        };

        Ok(LookupResult {
            object_id: object_manager::metadata(&path).map(|m| m.id),
            kind,
        })
    }

    fn enumerate(&self, _ctx: &LookupContext, path: &str) -> Result<Vec<DirEntry>, SaifsError> {
        let mut out = Vec::new();
        for name in vfs::ls(Some(&normalize_path(path))).map_err(map_str_err)? {
            out.push(DirEntry {
                name,
                kind: SaifsNodeKind::Virtual,
            });
        }
        Ok(out)
    }

    fn create(&self, _ctx: &LookupContext, path: &str, kind: CreateKind) -> Result<ObjectId, SaifsError> {
        let path = normalize_path(path);
        match kind {
            CreateKind::File => vfs::touch(&path).map_err(map_str_err)?,
            CreateKind::Directory => vfs::mkdir(&path).map_err(map_str_err)?,
        }

        let obj = object_manager::metadata(&path);
        Ok(obj.map(|m| m.id).unwrap_or(ObjectId(0)))
    }

    fn remove(&self, _ctx: &LookupContext, path: &str) -> Result<(), SaifsError> {
        vfs::rm(&normalize_path(path)).map_err(map_str_err)
    }
}

static DEFAULT_VFS_PROVIDER: DefaultVfsProvider = DefaultVfsProvider;

fn register_provider_internal(state: &mut SaifsState, provider: &'static dyn NamespaceProvider) -> ProviderId {
    if let Some(existing) = state
        .providers
        .iter()
        .find(|entry| entry.provider.name() == provider.name())
    {
        return existing.id;
    }

    let id = state.alloc_provider_id();
    state.providers.push(RegisteredProvider { id, provider });
    id
}

fn resolve_provider_internal(state: &SaifsState, path: &str) -> Option<ProviderId> {
    let path = normalize_path(path);
    let mut best: Option<(usize, ProviderId)> = None;

    for mount in &state.mounts {
        let mount_path = if mount.path.is_empty() { "/" } else { mount.path.as_str() };
        let matched = path == mount_path
            || (mount_path == "/")
            || (path.starts_with(mount_path) && path.as_bytes().get(mount_path.len()) == Some(&b'/'));

        if matched {
            let score = mount_path.len();
            match best {
                None => best = Some((score, mount.provider)),
                Some((best_score, _)) if score >= best_score => {
                    best = Some((score, mount.provider))
                }
                _ => {}
            }
        }
    }

    best.map(|(_, provider)| provider)
}

fn provider_by_id_internal(state: &SaifsState, id: ProviderId) -> Option<&'static dyn NamespaceProvider> {
    state
        .providers
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.provider)
}

fn map_str_err(err: &'static str) -> SaifsError {
    match err {
        "path not found" | "node missing" => SaifsError::NotFound,
        "already exists" => SaifsError::AlreadyExists,
        "invalid name" | "missing name" => SaifsError::InvalidPath,
        "read-only virtual path" => SaifsError::AccessDenied,
        _ => SaifsError::Internal,
    }
}

fn default_read(path: &str) -> Result<Vec<u8>, SaifsError> {
    Ok(vfs::cat(path).map_err(map_str_err)?.into_bytes())
}

fn default_write(path: &str, data: &[u8]) -> Result<usize, SaifsError> {
    vfs::write(path, data).map_err(map_str_err)?;
    Ok(data.len())
}

pub struct SaifsHandle {
    id: HandleId,
    path: String,
    object_id: Option<ObjectId>,
    provider_id: ProviderId,
    kind: SaifsNodeKind,
}

impl SaifsHandle {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn kind(&self) -> SaifsNodeKind {
        self.kind
    }

    pub fn metadata(&self) -> Option<ObjectMetadata> {
        object_manager::metadata(&self.path)
    }

    pub fn status(&self) -> Result<ObjectStatus, SaifsError> {
        if let Some(meta) = object_manager::metadata(&self.path) {
            return Ok(meta.status);
        }
        match self.kind {
            SaifsNodeKind::File | SaifsNodeKind::Directory => Ok(ObjectStatus::Online),
            SaifsNodeKind::Object | SaifsNodeKind::Virtual => Err(SaifsError::UnsupportedOperation),
        }
    }

    pub fn object_type(&self) -> Result<ObjectType, SaifsError> {
        if let Some(meta) = object_manager::metadata(&self.path) {
            return Ok(meta.kind);
        }

        match self.kind {
            SaifsNodeKind::File => Ok(ObjectType::File),
            SaifsNodeKind::Directory => Ok(ObjectType::Volume),
            SaifsNodeKind::Object | SaifsNodeKind::Virtual => Err(SaifsError::UnsupportedOperation),
        }
    }
}

impl Handle for SaifsHandle {
    fn id(&self) -> HandleId {
        self.id
    }

    fn object_id(&self) -> Option<ObjectId> {
        self.object_id
    }

    fn provider_id(&self) -> ProviderId {
        self.provider_id
    }

    fn read(&self) -> Result<Vec<u8>, SaifsError> {
        default_read(&self.path)
    }

    fn write(&self, data: &[u8]) -> Result<usize, SaifsError> {
        default_write(&self.path, data)
    }

    fn query(&self, key: &str) -> Result<Property, SaifsError> {
        for p in self.properties()? {
            if p.key == key {
                return Ok(p);
            }
        }
        Err(SaifsError::NotFound)
    }

    fn properties(&self) -> Result<PropertyMap, SaifsError> {
        if let Some(meta) = object_manager::metadata(&self.path) {
            return Ok(meta.properties);
        }

        match self.kind {
            SaifsNodeKind::File => {
                let data = vfs::cat(&self.path).map_err(map_str_err)?;
                Ok(vec![
                    Property {
                        key: "type".to_string(),
                        value: "file".to_string(),
                    },
                    Property {
                        key: "size".to_string(),
                        value: data.len().to_string(),
                    },
                ])
            }
            SaifsNodeKind::Directory => {
                let children = vfs::ls(Some(&self.path)).map_err(map_str_err)?;
                Ok(vec![
                    Property {
                        key: "type".to_string(),
                        value: "directory".to_string(),
                    },
                    Property {
                        key: "children".to_string(),
                        value: children.len().to_string(),
                    },
                ])
            }
            SaifsNodeKind::Object | SaifsNodeKind::Virtual => Ok(Vec::new()),
        }
    }

    fn health(&self) -> Result<Health, SaifsError> {
        if let Some(meta) = object_manager::metadata(&self.path) {
            return Ok(meta.health);
        }

        match self.kind {
            SaifsNodeKind::File | SaifsNodeKind::Directory => Ok(Health::Healthy),
            SaifsNodeKind::Object | SaifsNodeKind::Virtual => Err(SaifsError::UnsupportedOperation),
        }
    }

    fn children(&self) -> Result<Vec<String>, SaifsError> {
        match self.kind {
            SaifsNodeKind::File => Err(SaifsError::UnsupportedOperation),
            SaifsNodeKind::Directory | SaifsNodeKind::Virtual => {
                vfs::ls(Some(&self.path)).map_err(map_str_err)
            }
            SaifsNodeKind::Object => {
                if let Some(meta) = object_manager::metadata(&self.path) {
                    let mut out = Vec::new();
                    for child_id in meta.children {
                        if let Some((_, name, _)) = object_manager::lookup_by_id(child_id) {
                            out.push(name);
                        }
                    }
                    out.sort();
                    Ok(out)
                } else {
                    vfs::ls(Some(&self.path)).map_err(map_str_err)
                }
            }
        }
    }
}

pub fn init() {
    object_manager::init();
    vfs::init();

    with_state(|state| {
        if state.initialized {
            return;
        }

        let provider = register_provider_internal(state, &DEFAULT_VFS_PROVIDER);
        state.mounts.push(MountPoint {
            path: "/".to_string(),
            provider,
            read_only: false,
        });

        let event_id = state.alloc_event_id();
        state.events.push(Event {
            id: event_id,
            event_type: EventType::Mounted,
            object: None,
            payload: "mounted / -> vfs".to_string(),
        });

        state.initialized = true;
    });
}

pub fn open(path: &str) -> Result<SaifsHandle, SaifsError> {
    init();

    let path = normalize_path(path);
    let provider_id = with_state(|state| resolve_provider_internal(state, &path));
    let provider_id = provider_id.ok_or(SaifsError::ProviderUnavailable)?;

    let lookup = with_state(|state| {
        let provider = provider_by_id_internal(state, provider_id).ok_or(SaifsError::ProviderUnavailable)?;
        provider.lookup(&LookupContext, &path)
    })?;

    let handle_id = with_state(|state| state.alloc_handle_id());

    Ok(SaifsHandle {
        id: handle_id,
        path,
        object_id: lookup.object_id,
        provider_id,
        kind: lookup.kind,
    })
}

pub fn read_text(path: &str) -> Result<String, SaifsError> {
    let handle = open(path)?;
    let bytes = handle.read()?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn list(path: &str) -> Result<Vec<String>, SaifsError> {
    let handle = open(path)?;
    handle.children()
}

pub fn mkdir(path: &str) -> Result<(), SaifsError> {
    init();
    vfs::mkdir(&normalize_path(path)).map_err(map_str_err)
}

pub fn touch(path: &str) -> Result<(), SaifsError> {
    init();
    vfs::touch(&normalize_path(path)).map_err(map_str_err)
}

pub fn remove(path: &str) -> Result<(), SaifsError> {
    init();
    vfs::rm(&normalize_path(path)).map_err(map_str_err)
}

pub fn cd(path: &str) -> Result<(), SaifsError> {
    init();
    vfs::cd(&normalize_path(path)).map_err(map_str_err)
}

pub fn pwd() -> String {
    init();
    vfs::pwd()
}

pub fn explain(path: &str) -> Result<Vec<String>, SaifsError> {
    init();
    object_manager::explain(&normalize_path(path)).map_err(map_str_err)
}

pub fn diagnose(path: &str) -> Result<Vec<String>, SaifsError> {
    init();
    object_manager::diagnose(&normalize_path(path)).map_err(map_str_err)
}

pub fn health(path: &str) -> Result<Health, SaifsError> {
    let handle = open(path)?;
    handle.health()
}

pub fn events(limit: usize) -> Vec<Event> {
    init();
    with_state(|state| {
        let total = state.events.len();
        let start = total.saturating_sub(limit.max(1));
        state.events[start..].to_vec()
    })
}

pub fn publish_event(event_type: EventType, object: Option<ObjectId>, payload: &str) {
    init();
    with_state(|state| {
        let id = state.alloc_event_id();
        state.events.push(Event {
            id,
            event_type,
            object,
            payload: payload.to_string(),
        });

        const MAX_EVENTS: usize = 256;
        if state.events.len() > MAX_EVENTS {
            let drop_count = state.events.len() - MAX_EVENTS;
            state.events.drain(0..drop_count);
        }
    });
}

pub fn register_provider(provider: &'static dyn NamespaceProvider) -> Result<ProviderId, SaifsError> {
    init();
    with_state(|state| Ok(register_provider_internal(state, provider)))
}

pub fn mount(path: &str, provider: ProviderId, read_only: bool) -> Result<(), SaifsError> {
    init();
    let path = normalize_path(path);
    with_state(|state| {
        if provider_by_id_internal(state, provider).is_none() {
            return Err(SaifsError::ProviderUnavailable);
        }

        if state.mounts.iter().any(|m| m.path == path) {
            return Err(SaifsError::AlreadyExists);
        }

        state.mounts.push(MountPoint {
            path: path.clone(),
            provider,
            read_only,
        });

        let id = state.alloc_event_id();
        state.events.push(Event {
            id,
            event_type: EventType::Mounted,
            object: None,
            payload: format!("mounted {}", path),
        });
        Ok(())
    })
}

pub fn mounts() -> Vec<MountPoint> {
    init();
    with_state(|state| state.mounts.clone())
}

pub fn providers() -> Vec<(ProviderId, String)> {
    init();
    with_state(|state| {
        state
            .providers
            .iter()
            .map(|entry| (entry.id, entry.provider.name().to_string()))
            .collect()
    })
}

pub fn subscribe() -> Result<SubscriptionId, SaifsError> {
    init();
    with_state(|state| {
        let id = state.alloc_subscription_id();
        state.subscriptions.push(id);
        Ok(id)
    })
}

pub fn unsubscribe(id: SubscriptionId) -> Result<(), SaifsError> {
    init();
    with_state(|state| {
        let before = state.subscriptions.len();
        state.subscriptions.retain(|s| *s != id);
        if state.subscriptions.len() == before {
            return Err(SaifsError::NotFound);
        }
        Ok(())
    })
}
