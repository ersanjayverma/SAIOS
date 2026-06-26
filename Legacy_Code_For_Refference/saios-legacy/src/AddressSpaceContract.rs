//! Canonical address-space authority.
//!
//! A process owns exactly one address space. Threads may share one only through
//! an explicit address-space handle, never by copying raw PML4 fields.

use alloc::collections::BTreeMap;
use spin::Mutex;

static VMA_RECORDS: Mutex<BTreeMap<(u64, u64), crate::memory_contract::VirtualMemoryArea>> =
    Mutex::new(BTreeMap::new());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressSpaceHandle {
    pub id: u64,
    pub pml4: u64,
    pub owner_pid: u32,
}

pub struct AddressSpaceContract;

impl AddressSpaceContract {
    fn emit_address_space_event(
        reason: &'static str,
        outcome: crate::observability_contract::ObservationOutcome,
        handle: AddressSpaceHandle,
        evidence: [u64; 4],
    ) {
        crate::observability_contract::ObservabilityContract::emit(
            crate::observability_contract::EventRecord {
                event: crate::observability_contract::ObservableEvent::Transition,
                contract: crate::observability_contract::ContractId::AddressSpace,
                tag: crate::observability_contract::ObservationTag::Transition,
                reason,
                outcome,
                resource: crate::observability_contract::ResourceClass::AddressSpace,
                owner: crate::observability_contract::ResourceOwner::AddressSpace(handle.id),
                cpu: Some(crate::process::table::cpu_idx()),
                pid: Some(handle.owner_pid),
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence,
            },
        );
    }

    pub fn map_user_frames(virt: u64, phys: u64, pages: usize) -> Result<(), &'static str> {
        Self::map_user_frames_in(active_handle(), virt, phys, pages)
    }

    pub fn create_for_process(owner_pid: u32) -> Result<AddressSpaceHandle, &'static str> {
        let mut allocator = crate::memory::FRAME_ALLOCATOR.lock();
        let pml4 = crate::memory::paging::new_address_space(&mut allocator)?;
        let handle = AddressSpaceHandle {
            id: pml4,
            pml4,
            owner_pid,
        };
        Self::validate_handle_or_panic(handle, "create_address_space");
        Self::emit_address_space_event(
            "as.create",
            crate::observability_contract::ObservationOutcome::Success,
            handle,
            [handle.id, handle.pml4, owner_pid as u64, 0],
        );
        Ok(handle)
    }

    pub fn destroy_for_process(handle: AddressSpaceHandle) -> Result<(), &'static str> {
        Self::validate_handle_or_panic(handle, "destroy_address_space");
        let result = crate::memory::paging::destroy_address_space(handle.pml4);
        Self::emit_address_space_event(
            if result.is_ok() {
                "as.destroy"
            } else {
                "as.failure"
            },
            if result.is_ok() {
                crate::observability_contract::ObservationOutcome::Success
            } else {
                crate::observability_contract::ObservationOutcome::Failed
            },
            handle,
            [
                handle.id,
                handle.pml4,
                handle.owner_pid as u64,
                result.is_err() as u64,
            ],
        );
        result
    }

    pub fn map_user_frames_with_flags_in(
        handle: AddressSpaceHandle,
        virt: u64,
        phys: u64,
        pages: usize,
        flags: u64,
    ) -> Result<(), &'static str> {
        Self::validate_handle_or_panic(handle, "map_user_frames");
        let mapping_entity = crate::resource_contract::AccountableEntity::process(handle.owner_pid);
        crate::resource_contract::ResourceContract::charge(
            crate::resource_contract::AttributionChain {
                accountable: mapping_entity,
                acting_pid: Some(handle.owner_pid),
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence_event_id: 0,
            },
            crate::resource_contract::ResourceKind::VirtualMappings,
            pages as u64,
        )?;
        let mut allocator = crate::memory::FRAME_ALLOCATOR.lock();
        for (mapped_count, i) in (0..pages).enumerate() {
            let page_virt = virt + (i * 0x1000) as u64;
            let page_phys = phys + (i * 0x1000) as u64;
            if let Err(reason) = crate::memory::paging::map_in(
                handle.pml4,
                page_virt,
                page_phys,
                flags,
                &mut allocator,
            ) {
                for j in 0..mapped_count {
                    let undo_virt = virt + (j * 0x1000) as u64;
                    if let Some(undo_phys) =
                        crate::memory::paging::translate_in(handle.pml4, undo_virt)
                    {
                        crate::memory_contract::MemoryContract::record_unmapping(
                            undo_phys & !0xFFF,
                            1,
                            "map_user_frames_rollback",
                        );
                    }
                    crate::memory::paging::unmap_in(handle.pml4, undo_virt);
                }
                crate::resource_contract::ResourceContract::release(
                    mapping_entity,
                    crate::resource_contract::ResourceKind::VirtualMappings,
                    pages as u64,
                );
                Self::emit_address_space_event(
                    "as.failure",
                    crate::observability_contract::ObservationOutcome::Failed,
                    handle,
                    [virt, phys, pages as u64, mapped_count as u64],
                );
                return Err(reason);
            }
            crate::memory_contract::MemoryContract::record_mapping(
                page_phys,
                1,
                flags,
                "map_user_frames",
            );
        }
        emit_memory_event(
            crate::kds::KdsEventType::Mmap,
            crate::kds::KdsSeverity::Info,
            [virt, (pages * 0x1000) as u64, pages as u64, phys],
        );
        Self::emit_address_space_event(
            "as.mutate",
            crate::observability_contract::ObservationOutcome::Success,
            handle,
            [virt, phys, pages as u64, flags],
        );
        crate::observability_contract::ObservabilityContract::kds_metric_for(
            crate::kds::KdsSubsystem::Memory,
            crate::kds::KdsMetricId::MmapBytes,
            (pages * 0x1000) as u64,
            handle.owner_pid,
            handle.owner_pid,
            [virt, phys],
        );
        Self::record_vma(handle, virt, (pages * 0x1000) as u64, flags);
        Ok(())
    }

    pub fn map_user_frames_in(
        handle: AddressSpaceHandle,
        virt: u64,
        phys: u64,
        pages: usize,
    ) -> Result<(), &'static str> {
        Self::map_user_frames_with_flags_in(
            handle,
            virt,
            phys,
            pages,
            crate::memory::paging::USER_FLAGS,
        )
    }

    pub fn unmap_user_range(addr: u64, pages: usize) {
        Self::unmap_user_range_in(active_handle(), addr, pages);
    }

    pub fn unmap_user_range_in(handle: AddressSpaceHandle, addr: u64, pages: usize) {
        Self::validate_handle_or_panic(handle, "unmap_user_range");
        let mut unmapped_pages = 0usize;
        let mut first_phys = 0u64;
        for i in 0..pages {
            let virt = addr + (i * 0x1000) as u64;
            if let Some(phys) = crate::memory::paging::translate_in(handle.pml4, virt) {
                if first_phys == 0 {
                    first_phys = phys & !0xFFF;
                }
                crate::memory_contract::MemoryContract::record_unmapping(
                    phys & !0xFFF,
                    1,
                    "unmap_user_range",
                );
                crate::memory::paging::unmap_in(handle.pml4, virt);
                crate::memory_contract::MemoryContract::free_frame(phys & !0xFFF, "munmap");
                unmapped_pages += 1;
            }
        }
        if unmapped_pages > 0 {
            crate::resource_contract::ResourceContract::release(
                crate::resource_contract::AccountableEntity::process(handle.owner_pid),
                crate::resource_contract::ResourceKind::VirtualMappings,
                unmapped_pages as u64,
            );
            emit_memory_event(
                crate::kds::KdsEventType::Munmap,
                crate::kds::KdsSeverity::Info,
                [
                    addr,
                    (unmapped_pages * 0x1000) as u64,
                    unmapped_pages as u64,
                    first_phys,
                ],
            );
            crate::observability_contract::ObservabilityContract::kds_metric_for(
                crate::kds::KdsSubsystem::Memory,
                crate::kds::KdsMetricId::MunmapBytes,
                (unmapped_pages * 0x1000) as u64,
                handle.owner_pid,
                handle.owner_pid,
                [addr, first_phys],
            );
            Self::emit_address_space_event(
                "as.mutate",
                crate::observability_contract::ObservationOutcome::Success,
                handle,
                [addr, first_phys, unmapped_pages as u64, 0],
            );
            Self::remove_vma_range(handle, addr, (unmapped_pages * 0x1000) as u64);
        }
    }

    pub fn protect_user_range(addr: u64, pages: usize, flags: u64) -> Result<(), &'static str> {
        for i in 0..pages {
            let virt = addr + (i * 0x1000) as u64;
            crate::memory::paging::update_user_page_flags(virt, flags)?;
        }
        emit_memory_event(
            crate::kds::KdsEventType::Mprotect,
            crate::kds::KdsSeverity::Info,
            [addr, (pages * 0x1000) as u64, pages as u64, flags],
        );
        Self::emit_address_space_event(
            "as.mutate",
            crate::observability_contract::ObservationOutcome::Success,
            active_handle(),
            [addr, pages as u64, flags, 0],
        );
        Self::protect_vma_range(active_handle(), addr, (pages * 0x1000) as u64, flags);
        Ok(())
    }

    pub fn vma_count_for_handle(handle: AddressSpaceHandle) -> usize {
        VMA_RECORDS
            .lock()
            .iter()
            .filter(|((address_space_id, _), _)| *address_space_id == handle.id)
            .count()
    }

    fn record_vma(handle: AddressSpaceHandle, base: u64, size: u64, flags: u64) {
        if size == 0 {
            return;
        }
        let vma = crate::memory_contract::VirtualMemoryArea {
            base,
            size,
            protection: protection_from_pte_flags(flags),
            vma_type: infer_vma_type(base, size),
            backing_object: 0,
            backing_offset: 0,
            cow: flags & crate::memory::paging::PTE_COW != 0,
            numa_policy: crate::memory_contract::NumaMemoryPolicy::Local,
            thp_eligible: false,
            kds_telemetry_handle: handle.id ^ base,
        };
        VMA_RECORDS.lock().insert((handle.id, base), vma);
    }

    fn remove_vma_range(handle: AddressSpaceHandle, base: u64, size: u64) {
        let end = base.saturating_add(size);
        VMA_RECORDS
            .lock()
            .retain(|(address_space_id, vma_base), vma| {
                *address_space_id != handle.id
                    || vma.base.saturating_add(vma.size) <= base
                    || *vma_base >= end
            });
    }

    fn protect_vma_range(handle: AddressSpaceHandle, base: u64, size: u64, flags: u64) {
        let end = base.saturating_add(size);
        let protection = protection_from_pte_flags(flags);
        for ((address_space_id, _), vma) in VMA_RECORDS.lock().iter_mut() {
            if *address_space_id == handle.id
                && vma.base < end
                && vma.base.saturating_add(vma.size) > base
            {
                vma.protection = protection;
            }
        }
    }

    pub fn validate_handle(handle: AddressSpaceHandle) -> Result<(), &'static str> {
        if handle.id == 0 || handle.owner_pid == 0 {
            return Err("address-space: handle has no owner");
        }
        if handle.pml4 == 0 || handle.pml4 & 0xFFF != 0 {
            return Err("address-space: invalid PML4");
        }
        Ok(())
    }

    pub fn validate_handle_or_panic(handle: AddressSpaceHandle, tag: &'static str) {
        if let Err(reason) = Self::validate_handle(handle) {
            Self::emit_address_space_event(
                "as.failure",
                crate::observability_contract::ObservationOutcome::Failed,
                handle,
                [handle.id, handle.pml4, handle.owner_pid as u64, 0],
            );
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::AddressSpace,
                tag,
                reason,
                crate::observability_contract::ResourceClass::AddressSpace,
                crate::observability_contract::ResourceOwner::AddressSpace(handle.id),
                [handle.id, handle.pml4, handle.owner_pid as u64, 0],
            );
            Self::dump_handle(handle, tag, reason);
            panic!("[address-space-contract] {} violation: {}", tag, reason);
        }
    }

    pub fn dump_handle(handle: AddressSpaceHandle, tag: &'static str, reason: &'static str) {
        crate::serial_println!(
            "[address-space-contract] dump tag={} reason={} id={} owner_pid={} pml4={:#x} cpu={} current_pid={:?} active_cr3={:#x}",
            tag,
            reason,
            handle.id,
            handle.owner_pid,
            handle.pml4,
            crate::process::table::cpu_idx(),
            crate::process::current_pid(),
            crate::memory::paging::active_pml4()
        );
    }
}

fn active_handle() -> AddressSpaceHandle {
    let pml4 = crate::memory::paging::active_pml4();
    let handle = AddressSpaceHandle {
        id: pml4,
        pml4,
        owner_pid: crate::process::current_pid().unwrap_or(1),
    };
    AddressSpaceContract::emit_address_space_event(
        "as.activate",
        crate::observability_contract::ObservationOutcome::Success,
        handle,
        [handle.id, handle.pml4, handle.owner_pid as u64, 0],
    );
    handle
}

fn emit_memory_event(
    event_type: crate::kds::KdsEventType,
    severity: crate::kds::KdsSeverity,
    payload: [u64; 4],
) {
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Memory,
        event_type,
        severity,
        payload,
    );
}

fn protection_from_pte_flags(flags: u64) -> crate::memory_contract::VmaProtection {
    crate::memory_contract::VmaProtection {
        read: flags & crate::memory::paging::PTE_USER != 0,
        write: flags & crate::memory::paging::PTE_WRITABLE != 0,
        execute: flags & crate::memory::paging::PTE_NO_EXEC == 0,
    }
}

fn infer_vma_type(base: u64, size: u64) -> crate::memory_contract::VmaType {
    let end = base.saturating_add(size);
    if end <= crate::process::USER_STACK_TOP
        && end
            > crate::process::USER_STACK_TOP.saturating_sub(crate::process::USER_STACK_SIZE as u64)
    {
        crate::memory_contract::VmaType::Stack
    } else if (crate::process::USER_BRK_BASE..crate::process::USER_MMAP_BASE).contains(&base) {
        crate::memory_contract::VmaType::Heap
    } else {
        crate::memory_contract::VmaType::Anonymous
    }
}
