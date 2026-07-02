use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use super::event::ObjectEventRecord;
use super::handle::ObjectHandle;
use super::id::ObjectId;
use super::registry::{KomStats, ObjectRecord, Registry};
use super::state::ObjectState;
use super::types::ObjectType;

static REGISTRY: StaticCell<Option<Registry>> = StaticCell::new(None);
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

fn with_registry_mut<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *REGISTRY.get() };
    if slot.is_none() {
        *slot = Some(Registry::new());
    }
    let out = f(slot.as_mut().expect("kom: registry must exist"));
    unlock();
    out
}

fn with_registry<R>(f: impl FnOnce(&Registry) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *REGISTRY.get() };
    if slot.is_none() {
        *slot = Some(Registry::new());
    }
    let out = f(slot.as_ref().expect("kom: registry must exist"));
    unlock();
    out
}

pub fn init() {
    with_registry_mut(|r| {
        if r.count() != 0 {
            return;
        }

        let _ = r.register(ObjectType::Kernel, "saios", ObjectState::Running);
        let _ = r.register(ObjectType::Process, "snsh", ObjectState::Running);
        let _ = r.register(ObjectType::Mount, "/", ObjectState::Running);
    });
}

pub fn register(
    object_type: ObjectType,
    name: &str,
    state: ObjectState,
) -> Result<ObjectHandle, &'static str> {
    with_registry_mut(|r| r.register(object_type, name, state).map(ObjectHandle::new))
}

pub fn unregister(handle: ObjectHandle) -> bool {
    with_registry_mut(|r| r.unregister(handle.id()))
}

pub fn find(id: ObjectId) -> Option<ObjectRecord> {
    with_registry(|r| r.find(id))
}

pub fn find_by_name(name: &str) -> Vec<ObjectRecord> {
    with_registry(|r| r.find_by_name(name))
}

pub fn find_by_type(object_type: ObjectType) -> Vec<ObjectRecord> {
    with_registry(|r| r.find_by_type(object_type))
}

pub fn enumerate() -> Vec<ObjectRecord> {
    with_registry(|r| r.enumerate())
}

pub fn count() -> usize {
    with_registry(|r| r.count())
}

pub fn stats() -> KomStats {
    with_registry(|r| r.stats())
}

pub fn events(limit: usize) -> Vec<ObjectEventRecord> {
    with_registry(|r| r.events(limit))
}

pub fn inspect(id: ObjectId) -> Option<Vec<String>> {
    find(id).map(|record| {
        let mut out = Vec::new();
        out.push(alloc::format!("Id: {}", record.id.0));
        out.push(alloc::format!("Type: {}", record.object_type.as_str()));
        out.push(alloc::format!("Name: {}", record.name));
        out.push(alloc::format!("State: {:?}", record.state));
        out
    })
}
