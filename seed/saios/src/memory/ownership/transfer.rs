use core::cell::UnsafeCell;

use crate::memory::constants::MAX_OWNERSHIP_RECORDS;
use crate::memory::errors::{MemoryError, MemoryResult};
use crate::memory::ownership::debug;
use crate::memory::ownership::owner::Owner;
use crate::memory::types::PhysicalFrame;

struct GlobalOwnership(UnsafeCell<OwnershipTable>);

unsafe impl Sync for GlobalOwnership {}

static OWNERSHIP: GlobalOwnership = GlobalOwnership(UnsafeCell::new(OwnershipTable::new()));

pub fn init() -> MemoryResult<()> {
    table().init()
}

pub fn claim_frame(frame: PhysicalFrame, owner: Owner) -> MemoryResult<()> {
    table().claim(frame, owner)
}

pub fn transfer_frame(frame: PhysicalFrame, from: Owner, to: Owner) -> MemoryResult<()> {
    table().transfer(frame, from, to)
}

pub fn release_frame(frame: PhysicalFrame, owner: Owner) -> MemoryResult<()> {
    table().release(frame, owner)
}

pub fn owner_of(frame: PhysicalFrame) -> Option<Owner> {
    table().owner_of(frame)
}

fn table() -> &'static mut OwnershipTable {
    unsafe { &mut *OWNERSHIP.0.get() }
}

#[derive(Debug, Copy, Clone)]
struct OwnershipRecord {
    active: bool,
    frame: PhysicalFrame,
    owner: Owner,
}

impl OwnershipRecord {
    const fn empty() -> Self {
        Self {
            active: false,
            frame: PhysicalFrame::new(0),
            owner: Owner::Pmm,
        }
    }
}

struct OwnershipTable {
    initialized: bool,
    records: [OwnershipRecord; MAX_OWNERSHIP_RECORDS],
}

impl OwnershipTable {
    const fn new() -> Self {
        Self {
            initialized: false,
            records: [OwnershipRecord::empty(); MAX_OWNERSHIP_RECORDS],
        }
    }

    fn init(&mut self) -> MemoryResult<()> {
        if self.initialized {
            return Err(MemoryError::AlreadyInitialized);
        }

        self.initialized = true;
        Ok(())
    }

    fn slot_for(&self, frame: PhysicalFrame) -> Option<usize> {
        self.records
            .iter()
            .position(|entry| entry.active && entry.frame == frame)
    }

    fn free_slot(&self) -> Option<usize> {
        self.records.iter().position(|entry| !entry.active)
    }

    fn claim(&mut self, frame: PhysicalFrame, owner: Owner) -> MemoryResult<()> {
        if let Some(current) = self.owner_of(frame) {
            debug::debug_violation(frame, Some(current), owner);
            return Err(MemoryError::OwnershipConflict);
        }

        let slot = self.free_slot().ok_or(MemoryError::OutOfFrames)?;
        self.records[slot] = OwnershipRecord {
            active: true,
            frame,
            owner,
        };
        Ok(())
    }

    fn transfer(&mut self, frame: PhysicalFrame, from: Owner, to: Owner) -> MemoryResult<()> {
        let slot = self.slot_for(frame).ok_or(MemoryError::OwnershipMissing)?;
        if self.records[slot].owner != from {
            debug::debug_violation(frame, Some(self.records[slot].owner), to);
            return Err(MemoryError::OwnershipConflict);
        }

        self.records[slot].owner = to;
        Ok(())
    }

    fn release(&mut self, frame: PhysicalFrame, owner: Owner) -> MemoryResult<()> {
        let slot = self.slot_for(frame).ok_or(MemoryError::OwnershipMissing)?;
        if self.records[slot].owner != owner {
            debug::debug_violation(frame, Some(self.records[slot].owner), owner);
            return Err(MemoryError::OwnershipConflict);
        }

        self.records[slot] = OwnershipRecord::empty();
        Ok(())
    }

    fn owner_of(&self, frame: PhysicalFrame) -> Option<Owner> {
        self.slot_for(frame).map(|slot| self.records[slot].owner)
    }
}
