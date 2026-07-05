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

        let _ = r.register(ObjectType::Kernel, "saios", ObjectState::Ready);
        let _ = r.register(ObjectType::Process, "sish", ObjectState::Ready);
        let _ = r.register(ObjectType::Mount, "/", ObjectState::Ready);
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

pub fn transition(handle: ObjectHandle, state: ObjectState) -> Result<(), &'static str> {
    with_registry_mut(|r| r.transition(handle.id(), state))
}

pub fn acquire(handle: ObjectHandle) -> Result<u32, &'static str> {
    with_registry_mut(|r| r.acquire(handle.id()))
}

pub fn release(handle: ObjectHandle) -> Result<u32, &'static str> {
    with_registry_mut(|r| r.release(handle.id()))
}

pub fn set_parent(handle: ObjectHandle, parent: Option<ObjectHandle>) -> Result<(), &'static str> {
    with_registry_mut(|r| r.set_parent(handle.id(), parent.map(|p| p.id())))
}

pub fn set_owner(handle: ObjectHandle, owner: Option<ObjectHandle>) -> Result<(), &'static str> {
    with_registry_mut(|r| r.set_owner(handle.id(), owner.map(|o| o.id())))
}

pub fn set_property(handle: ObjectHandle, key: &str, value: &str) -> Result<(), &'static str> {
    with_registry_mut(|r| r.set_property(handle.id(), key, value))
}

pub fn clone_object(handle: ObjectHandle, new_name: &str) -> Result<ObjectHandle, &'static str> {
    with_registry_mut(|r| r.clone_object(handle.id(), new_name).map(ObjectHandle::new))
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
        out.push(alloc::format!(
            "Label: {}",
            record.id.label(record.object_type)
        ));
        out.push(alloc::format!("Type: {}", record.object_type.as_str()));
        out.push(alloc::format!("Name: {}", record.name));
        out.push(alloc::format!("State: {}", record.state.as_str()));
        out.push(alloc::format!("Flags: 0x{:X}", record.flags));
        out.push(alloc::format!(
            "Parent: {}",
            record.parent.map(|p| p.0).unwrap_or(0)
        ));
        out.push(alloc::format!(
            "Owner: {}",
            record.owner.map(|p| p.0).unwrap_or(0)
        ));
        out.push(alloc::format!("Children: {}", record.children.len()));
        out.push(alloc::format!("RefCount: {}", record.reference_count));
        out.push(alloc::format!("CreatedTick: {}", record.created_tick));
        out.push(alloc::format!(
            "ModifiedTick: {}",
            record.last_modified_tick
        ));
        out
    })
}
