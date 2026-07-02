use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::som::{EventId, HandleId, ObjectId, OperationId, ProviderId};
use crate::object_manager::{self, Health, ObjectMetadata, ObjectStatus, ObjectType, Property, PropertyMap};
use crate::vfs;

#[path = "saifs/tests.rs"]
pub mod tests;

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

impl SaifsError {
    pub const fn as_str(self) -> &'static str {
        match self {
            SaifsError::NotFound => "not found",
            SaifsError::AlreadyExists => "already exists",
            SaifsError::InvalidPath => "invalid path",
            SaifsError::InvalidHandle => "invalid handle",
            SaifsError::UnsupportedOperation => "unsupported operation",
            SaifsError::AccessDenied => "access denied",
            SaifsError::ProviderUnavailable => "provider unavailable",
            SaifsError::Busy => "resource busy",
            SaifsError::Corrupt => "corrupt",
            SaifsError::Internal => "internal error",
        }
    }
}

impl fmt::Display for SaifsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
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

pub trait NamespaceManager {
    fn lookup(&self, path: &str) -> Result<LookupResult, SaifsError>;
    fn enumerate(&self, path: &str) -> Result<Vec<DirEntry>, SaifsError>;
    fn create(&self, path: &str, kind: CreateKind) -> Result<ObjectId, SaifsError>;
    fn remove(&self, path: &str) -> Result<(), SaifsError>;
}

#[derive(Clone)]
pub struct ResolvedPath {
    pub input_path: String,
    pub absolute_path: String,
    pub provider_path: String,
    pub mount_path: String,
    pub provider: ProviderId,
    pub read_only: bool,
}

pub trait PathResolver {
    fn canonicalize(&self, path: &str) -> Result<String, SaifsError>;
    fn resolve(&self, path: &str) -> Result<ResolvedPath, SaifsError>;
}

#[derive(Debug, Copy, Clone)]
pub struct SaifsProviderRegistry;

#[derive(Debug, Copy, Clone)]
pub struct SaifsMountManager;

#[derive(Debug, Copy, Clone)]
pub struct SaifsNamespaceManager;

#[derive(Debug, Copy, Clone)]
pub struct SaifsPathResolver;

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
    canonicalize_path(path).unwrap_or_else(|_| "/".to_string())
}

fn canonicalize_path(path: &str) -> Result<String, SaifsError> {
    if path.as_bytes().contains(&0) {
        return Err(SaifsError::InvalidPath);
    }

    if path.is_empty() || path == "/" {
        return Ok("/".to_string());
    }

    let source = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };

    let mut parts: Vec<&str> = Vec::new();
    for part in source.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }

        if part == ".." {
            let _ = parts.pop();
            continue;
        }

        parts.push(part);
    }

    if parts.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", parts.join("/")))
    }
}

fn path_in_mount(mount_path: &str, path: &str) -> bool {
    mount_path == "/"
        || path == mount_path
        || (path.starts_with(mount_path) && path.as_bytes().get(mount_path.len()) == Some(&b'/'))
}

fn provider_path_from_mount(mount_path: &str, absolute_path: &str) -> String {
    if mount_path == "/" {
        return absolute_path.to_string();
    }

    if absolute_path == mount_path {
        return "/".to_string();
    }

    let tail = &absolute_path[mount_path.len()..];
    if tail.starts_with('/') {
        tail.to_string()
    } else {
        format!("/{}", tail)
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

        let node = vfs::open_node(&path).map_err(map_str_err)?;
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

fn resolve_mount_internal(state: &SaifsState, path: &str) -> Option<MountPoint> {
    let mut best: Option<MountPoint> = None;
    let mut best_score = 0usize;

    for mount in &state.mounts {
        let mount_path = if mount.path.is_empty() { "/" } else { mount.path.as_str() };
        if path_in_mount(mount_path, path) {
            let score = mount_path.len();
            if best.is_none() || score >= best_score {
                best = Some(mount.clone());
                best_score = score;
            }
        }
    }

    best
}

fn resolve_provider_internal(state: &SaifsState, path: &str) -> Option<ProviderId> {
    resolve_mount_internal(state, path).map(|m| m.provider)
}

fn resolve_path_internal(state: &SaifsState, path: &str) -> Result<ResolvedPath, SaifsError> {
    let absolute_path = canonicalize_path(path)?;
    let mount = resolve_mount_internal(state, &absolute_path).ok_or(SaifsError::ProviderUnavailable)?;

    Ok(ResolvedPath {
        input_path: path.to_string(),
        absolute_path: absolute_path.clone(),
        provider_path: provider_path_from_mount(&mount.path, &absolute_path),
        mount_path: mount.path,
        provider: mount.provider,
        read_only: mount.read_only,
    })
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
        "not a file" | "not a directory" | "parent is not a directory" => SaifsError::UnsupportedOperation,
        "cannot remove root" | "directory not empty" => SaifsError::Busy,
        "invalid inode" => SaifsError::Corrupt,
        "read-only virtual path" => SaifsError::AccessDenied,
        _ => SaifsError::Internal,
    }
}

fn default_read(path: &str) -> Result<Vec<u8>, SaifsError> {
    Ok(vfs::cat(path).map_err(map_str_err)?.into_bytes())
}

fn default_write(path: &str, data: &[u8]) -> Result<usize, SaifsError> {
    vfs::write_path(path, data).map_err(map_str_err)?;
    Ok(data.len())
}

pub struct SaifsHandle {
    id: HandleId,
    path: String,
    provider_path: String,
    object_id: Option<ObjectId>,
    provider_id: ProviderId,
    kind: SaifsNodeKind,
}

impl SaifsHandle {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn provider_path(&self) -> &str {
        &self.provider_path
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
                let children = self.children()?;
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
                with_state(|state| {
                    let provider = provider_by_id_internal(state, self.provider_id)
                        .ok_or(SaifsError::ProviderUnavailable)?;
                    let entries = provider.enumerate(&LookupContext, &self.provider_path)?;
                    let mut out: Vec<String> = entries.into_iter().map(|e| e.name).collect();
                    out.sort();
                    Ok(out)
                })
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
                    with_state(|state| {
                        let provider = provider_by_id_internal(state, self.provider_id)
                            .ok_or(SaifsError::ProviderUnavailable)?;
                        let entries = provider.enumerate(&LookupContext, &self.provider_path)?;
                        let mut out: Vec<String> = entries.into_iter().map(|e| e.name).collect();
                        out.sort();
                        Ok(out)
                    })
                }
            }
        }
    }
}

impl ProviderRegistry for SaifsProviderRegistry {
    fn register(&self, provider: &'static dyn NamespaceProvider) -> Result<ProviderId, SaifsError> {
        init();
        with_state(|state| Ok(register_provider_internal(state, provider)))
    }

    fn get(&self, provider: ProviderId) -> Option<&'static dyn NamespaceProvider> {
        init();
        with_state(|state| provider_by_id_internal(state, provider))
    }

    fn list(&self) -> Vec<ProviderId> {
        init();
        with_state(|state| state.providers.iter().map(|entry| entry.id).collect())
    }
}

impl MountManager for SaifsMountManager {
    fn mount(&self, mut mount: MountPoint) -> Result<(), SaifsError> {
        init();
        mount.path = canonicalize_path(&mount.path)?;

        with_state(|state| {
            if provider_by_id_internal(state, mount.provider).is_none() {
                return Err(SaifsError::ProviderUnavailable);
            }

            if state.mounts.iter().any(|m| m.path == mount.path) {
                return Err(SaifsError::AlreadyExists);
            }

            state.mounts.push(mount.clone());

            let id = state.alloc_event_id();
            state.events.push(Event {
                id,
                event_type: EventType::Mounted,
                object: None,
                payload: format!("mounted {}", mount.path),
            });

            Ok(())
        })
    }

    fn unmount(&self, path: &str) -> Result<(), SaifsError> {
        init();
        let path = canonicalize_path(path)?;
        if path == "/" {
            return Err(SaifsError::Busy);
        }

        with_state(|state| {
            let before = state.mounts.len();
            state.mounts.retain(|m| m.path != path);
            if state.mounts.len() == before {
                return Err(SaifsError::NotFound);
            }

            let id = state.alloc_event_id();
            state.events.push(Event {
                id,
                event_type: EventType::Unmounted,
                object: None,
                payload: format!("unmounted {}", path),
            });

            Ok(())
        })
    }

    fn resolve_provider(&self, path: &str) -> Result<ProviderId, SaifsError> {
        init();
        let path = canonicalize_path(path)?;
        with_state(|state| resolve_provider_internal(state, &path).ok_or(SaifsError::ProviderUnavailable))
    }

    fn mounts(&self) -> Vec<MountPoint> {
        init();
        with_state(|state| state.mounts.clone())
    }
}

impl PathResolver for SaifsPathResolver {
    fn canonicalize(&self, path: &str) -> Result<String, SaifsError> {
        canonicalize_path(path)
    }

    fn resolve(&self, path: &str) -> Result<ResolvedPath, SaifsError> {
        init();
        with_state(|state| resolve_path_internal(state, path))
    }
}

impl NamespaceManager for SaifsNamespaceManager {
    fn lookup(&self, path: &str) -> Result<LookupResult, SaifsError> {
        init();
        let resolved = path_resolver().resolve(path)?;

        with_state(|state| {
            let provider = provider_by_id_internal(state, resolved.provider)
                .ok_or(SaifsError::ProviderUnavailable)?;
            provider.lookup(&LookupContext, &resolved.provider_path)
        })
    }

    fn enumerate(&self, path: &str) -> Result<Vec<DirEntry>, SaifsError> {
        init();
        let resolved = path_resolver().resolve(path)?;

        with_state(|state| {
            let provider = provider_by_id_internal(state, resolved.provider)
                .ok_or(SaifsError::ProviderUnavailable)?;
            provider.enumerate(&LookupContext, &resolved.provider_path)
        })
    }

    fn create(&self, path: &str, kind: CreateKind) -> Result<ObjectId, SaifsError> {
        init();
        let resolved = path_resolver().resolve(path)?;
        if resolved.read_only {
            return Err(SaifsError::AccessDenied);
        }

        with_state(|state| {
            let provider = provider_by_id_internal(state, resolved.provider)
                .ok_or(SaifsError::ProviderUnavailable)?;
            provider.create(&LookupContext, &resolved.provider_path, kind)
        })
    }

    fn remove(&self, path: &str) -> Result<(), SaifsError> {
        init();
        let resolved = path_resolver().resolve(path)?;
        if resolved.read_only {
            return Err(SaifsError::AccessDenied);
        }

        with_state(|state| {
            let provider = provider_by_id_internal(state, resolved.provider)
                .ok_or(SaifsError::ProviderUnavailable)?;
            provider.remove(&LookupContext, &resolved.provider_path)
        })
    }
}

pub const fn provider_registry() -> SaifsProviderRegistry {
    SaifsProviderRegistry
}

pub const fn mount_manager() -> SaifsMountManager {
    SaifsMountManager
}

pub const fn namespace_manager() -> SaifsNamespaceManager {
    SaifsNamespaceManager
}

pub const fn path_resolver() -> SaifsPathResolver {
    SaifsPathResolver
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

pub fn is_initialized() -> bool {
    with_state(|state| state.initialized)
}

pub fn open(path: &str) -> Result<SaifsHandle, SaifsError> {
    init();

    let resolved = path_resolver().resolve(path)?;
    let lookup = namespace_manager().lookup(&resolved.absolute_path)?;

    let handle_id = with_state(|state| state.alloc_handle_id());

    Ok(SaifsHandle {
        id: handle_id,
        path: resolved.absolute_path,
        provider_path: resolved.provider_path,
        object_id: lookup.object_id,
        provider_id: resolved.provider,
        kind: lookup.kind,
    })
}

pub fn read_text(path: &str) -> Result<String, SaifsError> {
    let handle = open(path)?;
    let bytes = handle.read()?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn list(path: &str) -> Result<Vec<String>, SaifsError> {
    let mut out: Vec<String> = namespace_manager()
        .enumerate(path)?
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    out.sort();
    Ok(out)
}

pub fn mkdir(path: &str) -> Result<(), SaifsError> {
    namespace_manager().create(path, CreateKind::Directory).map(|_| ())
}

pub fn touch(path: &str) -> Result<(), SaifsError> {
    namespace_manager().create(path, CreateKind::File).map(|_| ())
}

pub fn remove(path: &str) -> Result<(), SaifsError> {
    namespace_manager().remove(path)
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
    provider_registry().register(provider)
}

pub fn mount(path: &str, provider: ProviderId, read_only: bool) -> Result<(), SaifsError> {
    mount_manager().mount(MountPoint {
        path: path.to_string(),
        provider,
        read_only,
    })
}

pub fn unmount(path: &str) -> Result<(), SaifsError> {
    mount_manager().unmount(path)
}

pub fn mounts() -> Vec<MountPoint> {
    mount_manager().mounts()
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

pub fn verify() -> crate::kernel::testing::report::VerifyReport {
    init();

    with_state(|state| {
        let mut checks = Vec::new();

        checks.push(if state.initialized {
            crate::kernel::testing::report::VerifyCheck::pass("Initialization", "saifs initialized")
        } else {
            crate::kernel::testing::report::VerifyCheck::fail("Initialization", "saifs not initialized")
        });

        checks.push(if state.mounts.iter().any(|m| m.path == "/") {
            crate::kernel::testing::report::VerifyCheck::pass("Root mount", "root mount exists")
        } else {
            crate::kernel::testing::report::VerifyCheck::fail("Root mount", "missing root mount")
        });

        checks.push(if !state.providers.is_empty() {
            crate::kernel::testing::report::VerifyCheck::pass("Provider registry", "provider(s) registered")
        } else {
            crate::kernel::testing::report::VerifyCheck::fail("Provider registry", "no providers registered")
        });

        crate::kernel::testing::report::VerifyReport {
            target: "saifs",
            checks,
        }
    })
}
