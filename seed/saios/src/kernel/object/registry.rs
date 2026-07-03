use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::event::{ObjectEvent, ObjectEventRecord};
use super::id::ObjectId;
use super::state::ObjectState;
use super::types::ObjectType;

#[derive(Clone, Debug)]
pub struct ObjectRecord {
    pub id: ObjectId,
    pub object_type: ObjectType,
    pub name: String,
    pub state: ObjectState,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct KomStats {
    pub total: usize,
    pub kernels: usize,
    pub processes: usize,
    pub threads: usize,
    pub drivers: usize,
    pub devices: usize,
    pub mounts: usize,
}

pub struct Registry {
    next_id: u64,
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
            next_id: 1,
            objects: Vec::new(),
            events: Vec::new(),
        }
    }

    fn alloc_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub fn register(
        &mut self,
        object_type: ObjectType,
        name: &str,
        state: ObjectState,
    ) -> Result<ObjectId, &'static str> {
        if name.is_empty() {
            return Err("kom: name cannot be empty");
        }
        if self.objects.iter().any(|o| o.name == name) {
            return Err("kom: object name already exists");
        }

        let id = self.alloc_id();
        self.objects.push(ObjectRecord {
            id,
            object_type,
            name: name.to_string(),
            state,
        });
        self.push_event(id, ObjectEvent::Created, name);
        if matches!(state, ObjectState::Running) {
            self.push_event(id, ObjectEvent::Started, name);
        }
        Ok(id)
    }

    pub fn unregister(&mut self, id: ObjectId) -> bool {
        if let Some(idx) = self.objects.iter().position(|o| o.id == id) {
            let removed = self.objects.remove(idx);
            self.push_event(removed.id, ObjectEvent::Destroyed, removed.name.as_str());
            return true;
        }
        false
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
                ObjectType::Process => s.processes = s.processes.saturating_add(1),
                ObjectType::Thread => s.threads = s.threads.saturating_add(1),
                ObjectType::Driver => s.drivers = s.drivers.saturating_add(1),
                ObjectType::Device => s.devices = s.devices.saturating_add(1),
                ObjectType::Mount => s.mounts = s.mounts.saturating_add(1),
            }
        }
        s
    }

    pub fn events(&self, limit: usize) -> Vec<ObjectEventRecord> {
        let take = core::cmp::min(limit, self.events.len());
        self.events[self.events.len().saturating_sub(take)..].to_vec()
    }

    fn push_event(&mut self, id: ObjectId, event: ObjectEvent, name: &str) {
        self.events.push(ObjectEventRecord {
            id,
            event,
            name: name.to_string(),
        });
        if self.events.len() > 256 {
            let _ = self.events.remove(0);
        }
    }
}
