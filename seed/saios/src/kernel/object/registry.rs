use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::event::{ObjectEvent, ObjectEventRecord};
use super::id::ObjectId;
use super::state::ObjectState;
use super::types::ObjectType;

const DEFAULT_NAMESPACE: u16 = 1;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ObjectCapabilities(pub u64);

impl ObjectCapabilities {
    pub const NONE: Self = Self(0);
    pub const READABLE: Self = Self(1 << 0);
    pub const WRITABLE: Self = Self(1 << 1);
    pub const EXECUTABLE: Self = Self(1 << 2);
    pub const MOUNTABLE: Self = Self(1 << 3);
    pub const SCHEDULABLE: Self = Self(1 << 4);
    pub const DRAWABLE: Self = Self(1 << 5);
    pub const INTERRUPT_SOURCE: Self = Self(1 << 6);

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Clone, Debug)]
pub struct ObjectProperty {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct ObjectRecord {
    pub id: ObjectId,
    pub object_type: ObjectType,
    pub name: String,
    pub state: ObjectState,
    pub flags: u64,
    pub parent: Option<ObjectId>,
    pub children: Vec<ObjectId>,
    pub owner: Option<ObjectId>,
    pub reference_count: u32,
    pub created_tick: u64,
    pub last_modified_tick: u64,
    pub capabilities: ObjectCapabilities,
    pub properties: Vec<ObjectProperty>,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct KomStats {
    pub total: usize,
    pub kernels: usize,
    pub services: usize,
    pub processes: usize,
    pub threads: usize,
    pub drivers: usize,
    pub devices: usize,
    pub timers: usize,
    pub events: usize,
    pub surfaces: usize,
    pub windows: usize,
    pub files: usize,
    pub directories: usize,
    pub volumes: usize,
    pub filesystems: usize,
    pub mounts: usize,
    pub sockets: usize,
    pub pipes: usize,
}

pub struct Registry {
    next_sequence: u32,
    objects: Vec<ObjectRecord>,
    events: Vec<ObjectEventRecord>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            next_sequence: 1,
            objects: Vec::new(),
            events: Vec::new(),
        }
    }

    fn alloc_id(&mut self, object_type: ObjectType) -> ObjectId {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        ObjectId::from_parts(object_type.code(), DEFAULT_NAMESPACE, sequence)
    }

    pub fn register(
        &mut self,
        object_type: ObjectType,
        name: &str,
        state: ObjectState,
    ) -> Result<ObjectId, &'static str> {
        self.register_with_meta(object_type, name, state, None, None, ObjectCapabilities::NONE, 0)
    }

    pub fn register_with_meta(
        &mut self,
        object_type: ObjectType,
        name: &str,
        state: ObjectState,
        parent: Option<ObjectId>,
        owner: Option<ObjectId>,
        capabilities: ObjectCapabilities,
        flags: u64,
    ) -> Result<ObjectId, &'static str> {
        if name.is_empty() {
            return Err("kom: name cannot be empty");
        }
        if self.objects.iter().any(|o| o.name == name) {
            return Err("kom: object name already exists");
        }

        if let Some(parent_id) = parent
            && !self.objects.iter().any(|o| o.id == parent_id)
        {
            return Err("kom: parent object not found");
        }

        let id = self.alloc_id(object_type);
        let now = crate::timer::ticks();
        self.objects.push(ObjectRecord {
            id,
            object_type,
            name: name.to_string(),
            state,
            flags,
            parent,
            children: Vec::new(),
            owner,
            reference_count: 1,
            created_tick: now,
            last_modified_tick: now,
            capabilities,
            properties: Vec::new(),
        });

        if let Some(parent_id) = parent
            && let Some(parent_obj) = self.objects.iter_mut().find(|o| o.id == parent_id)
            && !parent_obj.children.contains(&id)
        {
            parent_obj.children.push(id);
            parent_obj.last_modified_tick = now;
        }

        self.push_event(id, ObjectEvent::Created, name, Some(state), "register");
        Ok(id)
    }

    pub fn unregister(&mut self, id: ObjectId) -> bool {
        if let Some(idx) = self.objects.iter().position(|o| o.id == id) {
            let removed = self.objects.remove(idx);
            let now = crate::timer::ticks();
            for obj in &mut self.objects {
                obj.children.retain(|child| *child != removed.id);
                if obj.parent == Some(removed.id) {
                    obj.parent = None;
                }
                obj.last_modified_tick = now;
            }
            self.push_event(
                removed.id,
                ObjectEvent::Destroyed,
                removed.name.as_str(),
                Some(ObjectState::Destroyed),
                "unregister",
            );
            return true;
        }
        false
    }

    pub fn transition(&mut self, id: ObjectId, state: ObjectState) -> Result<(), &'static str> {
        let idx = self
            .objects
            .iter()
            .position(|o| o.id == id)
            .ok_or("kom: object not found")?;
        let current = self.objects[idx].state;
        if !current.can_transition(state) {
            return Err("kom: invalid lifecycle transition");
        }
        self.objects[idx].state = state;
        self.objects[idx].last_modified_tick = crate::timer::ticks();
        let name = self.objects[idx].name.clone();
        self.push_event(id, ObjectEvent::StateChanged, name.as_str(), Some(state), "transition");
        Ok(())
    }

    pub fn set_owner(&mut self, id: ObjectId, owner: Option<ObjectId>) -> Result<(), &'static str> {
        let idx = self
            .objects
            .iter()
            .position(|o| o.id == id)
            .ok_or("kom: object not found")?;
        if let Some(owner_id) = owner
            && !self.objects.iter().any(|o| o.id == owner_id)
        {
            return Err("kom: owner object not found");
        }
        self.objects[idx].owner = owner;
        self.objects[idx].last_modified_tick = crate::timer::ticks();
        let name = self.objects[idx].name.clone();
        self.push_event(id, ObjectEvent::OwnerChanged, name.as_str(), self.objects.get(idx).map(|o| o.state), "owner");
        Ok(())
    }

    pub fn set_parent(&mut self, id: ObjectId, parent: Option<ObjectId>) -> Result<(), &'static str> {
        let idx = self
            .objects
            .iter()
            .position(|o| o.id == id)
            .ok_or("kom: object not found")?;
        if let Some(parent_id) = parent
            && !self.objects.iter().any(|o| o.id == parent_id)
        {
            return Err("kom: parent object not found");
        }

        let old_parent = self.objects[idx].parent;
        if old_parent == parent {
            return Ok(());
        }

        if let Some(old_parent_id) = old_parent
            && let Some(parent_obj) = self.objects.iter_mut().find(|o| o.id == old_parent_id)
        {
            parent_obj.children.retain(|child| *child != id);
            parent_obj.last_modified_tick = crate::timer::ticks();
        }

        self.objects[idx].parent = parent;
        self.objects[idx].last_modified_tick = crate::timer::ticks();

        if let Some(parent_id) = parent
            && let Some(parent_obj) = self.objects.iter_mut().find(|o| o.id == parent_id)
            && !parent_obj.children.contains(&id)
        {
            parent_obj.children.push(id);
            parent_obj.last_modified_tick = crate::timer::ticks();
        }

        let name = self.objects[idx].name.clone();
        self.push_event(id, ObjectEvent::ParentChanged, name.as_str(), self.objects.get(idx).map(|o| o.state), "parent");
        Ok(())
    }

    pub fn acquire(&mut self, id: ObjectId) -> Result<u32, &'static str> {
        let idx = self
            .objects
            .iter()
            .position(|o| o.id == id)
            .ok_or("kom: object not found")?;
        self.objects[idx].reference_count = self.objects[idx].reference_count.saturating_add(1);
        self.objects[idx].last_modified_tick = crate::timer::ticks();
        let rc = self.objects[idx].reference_count;
        let name = self.objects[idx].name.clone();
        self.push_event(id, ObjectEvent::Acquired, name.as_str(), self.objects.get(idx).map(|o| o.state), "acquire");
        Ok(rc)
    }

    pub fn release(&mut self, id: ObjectId) -> Result<u32, &'static str> {
        let idx = self
            .objects
            .iter()
            .position(|o| o.id == id)
            .ok_or("kom: object not found")?;

        if self.objects[idx].reference_count == 0 {
            return Err("kom: reference underflow");
        }

        self.objects[idx].reference_count -= 1;
        self.objects[idx].last_modified_tick = crate::timer::ticks();
        let rc = self.objects[idx].reference_count;
        let name = self.objects[idx].name.clone();
        self.push_event(id, ObjectEvent::Released, name.as_str(), self.objects.get(idx).map(|o| o.state), "release");
        Ok(rc)
    }

    pub fn set_property(&mut self, id: ObjectId, key: &str, value: &str) -> Result<(), &'static str> {
        if key.is_empty() {
            return Err("kom: property key cannot be empty");
        }
        let idx = self
            .objects
            .iter()
            .position(|o| o.id == id)
            .ok_or("kom: object not found")?;

        if let Some(prop) = self.objects[idx].properties.iter_mut().find(|p| p.key == key) {
            prop.value = value.to_string();
        } else {
            self.objects[idx].properties.push(ObjectProperty {
                key: key.to_string(),
                value: value.to_string(),
            });
        }
        self.objects[idx].last_modified_tick = crate::timer::ticks();
        Ok(())
    }

    pub fn clone_object(&mut self, id: ObjectId, new_name: &str) -> Result<ObjectId, &'static str> {
        let source = self
            .objects
            .iter()
            .find(|o| o.id == id)
            .cloned()
            .ok_or("kom: object not found")?;
        let new_id = self.register_with_meta(
            source.object_type,
            new_name,
            ObjectState::Created,
            source.parent,
            source.owner,
            source.capabilities,
            source.flags,
        )?;
        for prop in source.properties {
            self.set_property(new_id, prop.key.as_str(), prop.value.as_str())?;
        }
        Ok(new_id)
    }

    pub fn find(&self, id: ObjectId) -> Option<ObjectRecord> {
        self.objects.iter().find(|o| o.id == id).cloned()
    }

    pub fn find_by_name(&self, name: &str) -> Vec<ObjectRecord> {
        self.objects
            .iter()
            .filter(|o| o.name == name)
            .cloned()
            .collect()
    }

    pub fn find_by_type(&self, object_type: ObjectType) -> Vec<ObjectRecord> {
        self.objects
            .iter()
            .filter(|o| o.object_type == object_type)
            .cloned()
            .collect()
    }

    pub fn enumerate(&self) -> Vec<ObjectRecord> {
        self.objects.clone()
    }

    pub fn count(&self) -> usize {
        self.objects.len()
    }

    pub fn stats(&self) -> KomStats {
        let mut s = KomStats::default();
        for o in &self.objects {
            s.total = s.total.saturating_add(1);
            match o.object_type {
                ObjectType::Kernel => s.kernels = s.kernels.saturating_add(1),
                ObjectType::Service => s.services = s.services.saturating_add(1),
                ObjectType::Process => s.processes = s.processes.saturating_add(1),
                ObjectType::Thread => s.threads = s.threads.saturating_add(1),
                ObjectType::Driver => s.drivers = s.drivers.saturating_add(1),
                ObjectType::Device => s.devices = s.devices.saturating_add(1),
                ObjectType::Timer => s.timers = s.timers.saturating_add(1),
                ObjectType::Event => s.events = s.events.saturating_add(1),
                ObjectType::Surface => s.surfaces = s.surfaces.saturating_add(1),
                ObjectType::Window => s.windows = s.windows.saturating_add(1),
                ObjectType::File => s.files = s.files.saturating_add(1),
                ObjectType::Directory => s.directories = s.directories.saturating_add(1),
                ObjectType::Volume => s.volumes = s.volumes.saturating_add(1),
                ObjectType::Filesystem => s.filesystems = s.filesystems.saturating_add(1),
                ObjectType::Mount => s.mounts = s.mounts.saturating_add(1),
                ObjectType::Socket => s.sockets = s.sockets.saturating_add(1),
                ObjectType::Pipe => s.pipes = s.pipes.saturating_add(1),
            }
        }
        s
    }

    pub fn events(&self, limit: usize) -> Vec<ObjectEventRecord> {
        let take = core::cmp::min(limit, self.events.len());
        self.events[self.events.len().saturating_sub(take)..].to_vec()
    }

    fn push_event(
        &mut self,
        id: ObjectId,
        event: ObjectEvent,
        name: &str,
        state: Option<ObjectState>,
        detail: &str,
    ) {
        self.events.push(ObjectEventRecord {
            id,
            event,
            name: name.to_string(),
            state,
            detail: detail.to_string(),
        });
        if self.events.len() > 256 {
            let _ = self.events.remove(0);
        }
    }
}
