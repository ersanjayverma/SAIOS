//! Canonical memory ownership authority.
//!
//! Every physical page must have one owner, one refcount authority, and one
//! mapping authority. Page-table bits are mirrors of this contract.

use crate::memory::frame::FrameAllocator;
use alloc::collections::BTreeMap;
use spin::Mutex;

static PAGE_RECORDS: Mutex<BTreeMap<u64, PageRecord>> = Mutex::new(BTreeMap::new());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageOwner {
    Free,
    Kernel,
    Process(u32),
    Shared,
    PageTable,
    PageCache,
    IpcObject,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmaType {
    Anonymous = 1,
    FileBacked = 2,
    DeviceBacked = 3,
    Stack = 4,
    Heap = 5,
    Vdso = 6,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumaMemoryPolicy {
    Local = 1,
    Bind = 2,
    Interleave = 3,
    Preferred = 4,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFaultAccess {
    Read = 1,
    Write = 2,
    Execute = 3,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFaultClass {
    Cow = 1,
    AnonymousDemand = 2,
    FileBackedDemand = 3,
    StackGrowth = 4,
    NumaPlacement = 5,
    Unresolvable = 6,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFaultResolution {
    Retried = 1,
    Sigsegv = 2,
    RedRing = 3,
    Deferred = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmaProtection {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualMemoryArea {
    pub base: u64,
    pub size: u64,
    pub protection: VmaProtection,
    pub vma_type: VmaType,
    pub backing_object: u64,
    pub backing_offset: u64,
    pub cow: bool,
    pub numa_policy: NumaMemoryPolicy,
    pub thp_eligible: bool,
    pub kds_telemetry_handle: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFaultReport {
    pub address: u64,
    pub access: PageFaultAccess,
    pub class: PageFaultClass,
    pub resolution: PageFaultResolution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRecord {
    pub phys: u64,
    pub owner: PageOwner,
    pub refcount: u32,
    pub mappings: u32,
    pub flags: u64,
}

pub struct MemoryContract;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryDiagnosticView {
    pub mmap_events: u64,
    pub munmap_events: u64,
    pub mprotect_events: u64,
    pub cow_fault_events: u64,
    pub fault_events: u64,
    pub page_alloc_metrics: u64,
    pub page_free_metrics: u64,
}

impl MemoryContract {
    pub fn diagnostic_view() -> MemoryDiagnosticView {
        crate::kds::flush_aggregates();
        MemoryDiagnosticView {
            mmap_events: crate::kds::count_events(crate::kds::KdsEventType::Mmap),
            munmap_events: crate::kds::count_events(crate::kds::KdsEventType::Munmap),
            mprotect_events: crate::kds::count_events(crate::kds::KdsEventType::Mprotect),
            cow_fault_events: crate::kds::count_events(crate::kds::KdsEventType::CowFault),
            fault_events: crate::kds::count_events(crate::kds::KdsEventType::Fault),
            page_alloc_metrics: crate::kds::count_metrics(crate::kds::KdsMetricId::PageAlloc),
            page_free_metrics: crate::kds::count_metrics(crate::kds::KdsMetricId::PageFree),
        }
    }

    pub fn alloc_kernel_frame(tag: &'static str) -> Option<u64> {
        let chain = accounting_chain_for_owner(PageOwner::Kernel);
        crate::resource_contract::ResourceContract::charge(
            chain,
            crate::resource_contract::ResourceKind::MemoryPages,
            1,
        )
        .ok()?;
        let phys = match crate::memory::FRAME_ALLOCATOR.lock().alloc() {
            Some(phys) => phys,
            None => {
                crate::resource_contract::ResourceContract::release(
                    crate::resource_contract::AccountableEntity::KERNEL,
                    crate::resource_contract::ResourceKind::MemoryPages,
                    1,
                );
                return None;
            }
        };
        crate::OBS_COUNTER!(
            crate::kds::KdsSubsystem::Memory,
            crate::kds::KdsMetricId::PageAlloc,
            1
        );
        Self::record_alloc(phys, 1, PageOwner::Kernel, tag);
        Some(phys)
    }

    pub fn alloc_kernel_frames(pages: usize, tag: &'static str) -> Option<u64> {
        let chain = accounting_chain_for_owner(PageOwner::Kernel);
        crate::resource_contract::ResourceContract::charge(
            chain,
            crate::resource_contract::ResourceKind::MemoryPages,
            pages as u64,
        )
        .ok()?;
        let phys = match crate::memory::FRAME_ALLOCATOR
            .lock()
            .alloc_contiguous(pages)
        {
            Some(phys) => phys,
            None => {
                crate::resource_contract::ResourceContract::release(
                    crate::resource_contract::AccountableEntity::KERNEL,
                    crate::resource_contract::ResourceKind::MemoryPages,
                    pages as u64,
                );
                return None;
            }
        };
        crate::OBS_COUNTER!(
            crate::kds::KdsSubsystem::Memory,
            crate::kds::KdsMetricId::PageAlloc,
            pages as u64
        );
        Self::record_alloc(phys, pages, PageOwner::Kernel, tag);
        Some(phys)
    }

    pub fn alloc_user_frames(pages: usize, tag: &'static str) -> Option<u64> {
        Self::alloc_process_frames(pages, current_owner(), tag)
    }

    pub fn alloc_process_frames(pages: usize, owner: PageOwner, tag: &'static str) -> Option<u64> {
        let chain = accounting_chain_for_owner(owner);
        crate::resource_contract::ResourceContract::charge(
            chain,
            crate::resource_contract::ResourceKind::MemoryPages,
            pages as u64,
        )
        .ok()?;
        let phys = match crate::memory::FRAME_ALLOCATOR
            .lock()
            .alloc_contiguous(pages)
        {
            Some(phys) => phys,
            None => {
                // OOM: attempt to reclaim by killing largest process, then retry once.
                if crate::memory::oom::oom_kill() {
                    match crate::memory::FRAME_ALLOCATOR
                        .lock()
                        .alloc_contiguous(pages)
                    {
                        Some(phys) => phys,
                        None => {
                            crate::resource_contract::ResourceContract::release(
                                accounting_entity_for_owner(owner),
                                crate::resource_contract::ResourceKind::MemoryPages,
                                pages as u64,
                            );
                            return None;
                        }
                    }
                } else {
                    crate::resource_contract::ResourceContract::release(
                        accounting_entity_for_owner(owner),
                        crate::resource_contract::ResourceKind::MemoryPages,
                        pages as u64,
                    );
                    return None;
                }
            }
        };
        crate::OBS_COUNTER!(
            crate::kds::KdsSubsystem::Memory,
            crate::kds::KdsMetricId::PageAlloc,
            pages as u64
        );
        Self::record_alloc(phys, pages, owner, tag);
        Some(phys)
    }

    pub fn free_frame(phys: u64, tag: &'static str) {
        crate::memory::FRAME_ALLOCATOR.lock().free(phys);
        crate::OBS_COUNTER!(
            crate::kds::KdsSubsystem::Memory,
            crate::kds::KdsMetricId::PageFree,
            1
        );
        Self::record_free(phys, 1, tag);
    }

    pub fn free_frames(phys: u64, pages: usize, tag: &'static str) {
        let mut allocator = crate::memory::FRAME_ALLOCATOR.lock();
        for i in 0..pages {
            allocator.free(phys + (i * crate::memory::frame::FRAME_SIZE) as u64);
        }
        crate::OBS_COUNTER!(
            crate::kds::KdsSubsystem::Memory,
            crate::kds::KdsMetricId::PageFree,
            pages as u64
        );
        Self::record_free(phys, pages, tag);
    }

    pub fn fork_cow(
        src: crate::address_space_contract::AddressSpaceHandle,
        dst: crate::address_space_contract::AddressSpaceHandle,
    ) -> Result<(), &'static str> {
        crate::address_space_contract::AddressSpaceContract::validate_handle_or_panic(
            src,
            "fork_cow_src",
        );
        crate::address_space_contract::AddressSpaceContract::validate_handle_or_panic(
            dst,
            "fork_cow_dst",
        );
        let mut allocator = crate::memory::FRAME_ALLOCATOR.lock();
        let result =
            crate::memory::paging::clone_user_space_cow(src.pml4, dst.pml4, &mut allocator);
        if result.is_ok() {
            Self::emit_event(
                crate::kds::KdsEventType::CowFault,
                crate::kds::KdsSeverity::Info,
                [
                    src.pml4,
                    dst.pml4,
                    src.owner_pid as u64,
                    dst.owner_pid as u64,
                ],
            );
        }
        result
    }

    pub fn resolve_cow_fault(
        handle: crate::address_space_contract::AddressSpaceHandle,
        virt: u64,
    ) -> Result<bool, &'static str> {
        crate::address_space_contract::AddressSpaceContract::validate_handle_or_panic(
            handle,
            "resolve_cow_fault",
        );
        let result = crate::memory::paging::resolve_cow_fault_in(handle.pml4, virt);
        if let Ok(true) = result {
            Self::emit_event(
                crate::kds::KdsEventType::CowFault,
                crate::kds::KdsSeverity::Info,
                [
                    virt & !0xFFF,
                    handle.pml4,
                    crate::memory::paging::active_pml4(),
                    1,
                ],
            );
        }
        result
    }

    pub fn resolve_current_cow_fault(pml4: u64, virt: u64) -> Result<bool, &'static str> {
        Self::resolve_cow_fault(
            crate::address_space_contract::AddressSpaceHandle {
                id: pml4,
                pml4,
                owner_pid: crate::process::current_pid().unwrap_or(1),
            },
            virt,
        )
    }

    pub fn validate_page(record: PageRecord) -> Result<(), &'static str> {
        if record.phys & 0xFFF != 0 {
            return Err("memory: page is not frame aligned");
        }
        if record.owner != PageOwner::Free && record.refcount == 0 {
            return Err("memory: owned page has zero refcount");
        }
        Ok(())
    }

    pub fn record_mapping(phys: u64, pages: usize, flags: u64, tag: &'static str) {
        let mut records = PAGE_RECORDS.lock();
        for i in 0..pages {
            let page = phys + (i * crate::memory::frame::FRAME_SIZE) as u64;
            let mut record = records.get(&page).copied().unwrap_or(PageRecord {
                phys: page,
                owner: current_owner(),
                refcount: 1,
                mappings: 0,
                flags: 0,
            });
            if matches!(record.owner, PageOwner::Kernel | PageOwner::Free) {
                record.owner = current_owner();
            }
            if record.refcount == 0 {
                record.refcount = 1;
            }
            record.mappings = record.mappings.saturating_add(1);
            record.flags = flags;
            Self::validate_page_or_panic(record, tag);
            records.insert(page, record);
        }
        drop(records);
        Self::emit_event(
            crate::kds::KdsEventType::Mmap,
            crate::kds::KdsSeverity::Trace,
            [
                phys,
                pages as u64,
                flags,
                crate::memory::paging::active_pml4(),
            ],
        );
    }

    pub fn record_unmapping(phys: u64, pages: usize, tag: &'static str) {
        let mut records = PAGE_RECORDS.lock();
        for i in 0..pages {
            let page = phys + (i * crate::memory::frame::FRAME_SIZE) as u64;
            if let Some(mut record) = records.get(&page).copied() {
                record.mappings = record.mappings.saturating_sub(1);
                if record.mappings == 0 {
                    record.flags = 0;
                }
                Self::validate_page_or_panic(record, tag);
                records.insert(page, record);
            }
        }
        drop(records);
        Self::emit_event(
            crate::kds::KdsEventType::Munmap,
            crate::kds::KdsSeverity::Trace,
            [phys, pages as u64, 0, crate::memory::paging::active_pml4()],
        );
    }

    pub fn update_page_flags(phys: u64, flags: u64, tag: &'static str) {
        let page = phys & !0xFFF;
        let mut records = PAGE_RECORDS.lock();
        if let Some(mut record) = records.get(&page).copied() {
            record.flags = flags;
            Self::validate_page_or_panic(record, tag);
            records.insert(page, record);
        }
        drop(records);
        Self::emit_event(
            crate::kds::KdsEventType::Mprotect,
            crate::kds::KdsSeverity::Trace,
            [page, 1, flags, crate::memory::paging::active_pml4()],
        );
    }

    pub fn classify_page_fault(
        virt: u64,
        error_code: u64,
        handled: bool,
        tag: &'static str,
    ) -> PageFaultReport {
        let access = if error_code & (1 << 4) != 0 {
            PageFaultAccess::Execute
        } else if error_code & (1 << 1) != 0 {
            PageFaultAccess::Write
        } else {
            PageFaultAccess::Read
        };
        let present = error_code & 1 != 0;
        let class = if tag == "cow_fault_resolved" {
            PageFaultClass::Cow
        } else if tag == "stack_growth_fault" {
            PageFaultClass::StackGrowth
        } else if handled && !present {
            PageFaultClass::AnonymousDemand
        } else {
            PageFaultClass::Unresolvable
        };
        let resolution = if handled {
            PageFaultResolution::Retried
        } else if virt >= 0xFFFF_8000_0000_0000 {
            PageFaultResolution::RedRing
        } else {
            PageFaultResolution::Sigsegv
        };
        PageFaultReport {
            address: virt,
            access,
            class,
            resolution,
        }
    }

    pub fn record_fault(pml4: u64, virt: u64, error_code: u64, handled: bool, tag: &'static str) {
        let report = Self::classify_page_fault(virt, error_code, handled, tag);
        Self::emit_event(
            crate::kds::KdsEventType::PageFault,
            if handled {
                crate::kds::KdsSeverity::Info
            } else {
                crate::kds::KdsSeverity::Warn
            },
            [
                virt,
                pml4,
                error_code,
                encode_fault_evidence(report, handled),
            ],
        );
    }

    pub fn record_page_table_frame(phys: u64, tag: &'static str) {
        record_page(
            PageRecord {
                phys: phys & !0xFFF,
                owner: PageOwner::PageTable,
                refcount: 1,
                mappings: 0,
                flags: crate::memory::paging::PTE_PRESENT | crate::memory::paging::PTE_WRITABLE,
            },
            tag,
        );
    }

    pub fn record_released_frame(phys: u64, tag: &'static str) {
        record_page(
            PageRecord {
                phys: phys & !0xFFF,
                owner: PageOwner::Free,
                refcount: 0,
                mappings: 0,
                flags: 0,
            },
            tag,
        );
    }

    pub fn retain_shared_page(phys: u64, flags: u64, tag: &'static str) {
        let page = phys & !0xFFF;
        let mut records = PAGE_RECORDS.lock();
        let mut record = records.get(&page).copied().unwrap_or(PageRecord {
            phys: page,
            owner: PageOwner::Shared,
            refcount: 1,
            mappings: 0,
            flags,
        });
        record.owner = PageOwner::Shared;
        record.refcount = record.refcount.saturating_add(1).max(2);
        record.mappings = record.mappings.saturating_add(1).max(record.refcount);
        record.flags = flags;
        Self::validate_page_or_panic(record, tag);
        records.insert(page, record);
    }

    pub fn shared_page_refcount(phys: u64) -> u32 {
        PAGE_RECORDS
            .lock()
            .get(&(phys & !0xFFF))
            .map(|record| record.refcount.max(1))
            .unwrap_or(1)
    }

    pub fn release_shared_page(phys: u64, tag: &'static str) {
        let page = phys & !0xFFF;
        let mut records = PAGE_RECORDS.lock();
        if let Some(mut record) = records.get(&page).copied() {
            record.refcount = record.refcount.saturating_sub(1).max(1);
            record.mappings = record.mappings.saturating_sub(1);
            if record.refcount <= 1 {
                record.owner = current_owner();
            }
            Self::validate_page_or_panic(record, tag);
            records.insert(page, record);
        }
    }

    pub fn forget_shared_if_unique(phys: u64, flags: u64, tag: &'static str) {
        let page = phys & !0xFFF;
        let mut records = PAGE_RECORDS.lock();
        if let Some(mut record) = records.get(&page).copied()
            && record.refcount <= 1
        {
            record.owner = current_owner();
            record.refcount = 1;
            record.flags = flags;
            Self::validate_page_or_panic(record, tag);
            records.insert(page, record);
        }
    }

    pub fn release_shared_or_free_with_allocator(
        phys: u64,
        allocator: &mut FrameAllocator,
        tag: &'static str,
    ) {
        let page = phys & !0xFFF;
        let mut records = PAGE_RECORDS.lock();
        match records.get(&page).copied() {
            Some(mut record) if record.refcount > 1 => {
                record.refcount -= 1;
                record.mappings = record.mappings.saturating_sub(1);
                record.owner = PageOwner::Shared;
                Self::validate_page_or_panic(record, tag);
                records.insert(page, record);
            }
            _ => {
                allocator.free(page);
                records.insert(
                    page,
                    PageRecord {
                        phys: page,
                        owner: PageOwner::Free,
                        refcount: 0,
                        mappings: 0,
                        flags: 0,
                    },
                );
            }
        }
    }

    pub fn validate_page_or_panic(record: PageRecord, tag: &'static str) {
        if let Err(reason) = Self::validate_page(record) {
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::Memory,
                tag,
                reason,
                crate::observability_contract::ResourceClass::Memory,
                memory_owner(record.owner),
                [
                    record.phys,
                    record.refcount as u64,
                    record.mappings as u64,
                    record.flags,
                ],
            );
            Self::dump_page(record, tag, reason);
            panic!("[memory-contract] {} violation: {}", tag, reason);
        }
    }

    pub fn dump_page(record: PageRecord, tag: &'static str, reason: &'static str) {
        crate::serial_println!(
            "[memory-contract] dump tag={} reason={} phys={:#x} owner={:?} refcount={} mappings={} flags={:#x} cpu={} current_pid={:?} cr3={:#x}",
            tag,
            reason,
            record.phys,
            record.owner,
            record.refcount,
            record.mappings,
            record.flags,
            crate::process::table::cpu_idx(),
            crate::process::current_pid(),
            crate::memory::paging::active_pml4()
        );
    }
}

fn record_page(record: PageRecord, tag: &'static str) {
    MemoryContract::validate_page_or_panic(record, tag);
    PAGE_RECORDS.lock().insert(record.phys, record);
}

impl MemoryContract {
    fn record_alloc(phys: u64, pages: usize, owner: PageOwner, tag: &'static str) {
        for i in 0..pages {
            record_page(
                PageRecord {
                    phys: phys + (i * crate::memory::frame::FRAME_SIZE) as u64,
                    owner,
                    refcount: 1,
                    mappings: 0,
                    flags: 0,
                },
                tag,
            );
        }
        Self::emit_event(
            crate::kds::KdsEventType::PageAlloc,
            crate::kds::KdsSeverity::Info,
            [
                phys,
                pages as u64,
                owner_code(owner),
                crate::memory::paging::active_pml4(),
            ],
        );
    }

    fn record_free(phys: u64, pages: usize, tag: &'static str) {
        let mut released_kernel = 0u64;
        let mut released_process = [(0u32, 0u64); 8];
        {
            let records = PAGE_RECORDS.lock();
            for i in 0..pages {
                let page = phys + (i * crate::memory::frame::FRAME_SIZE) as u64;
                match records.get(&page).map(|record| record.owner) {
                    Some(PageOwner::Process(pid)) => {
                        if let Some(slot) = released_process.iter_mut().find(|slot| slot.0 == pid) {
                            slot.1 = slot.1.saturating_add(1);
                        } else if let Some(slot) =
                            released_process.iter_mut().find(|slot| slot.0 == 0)
                        {
                            *slot = (pid, 1);
                        }
                    }
                    Some(
                        PageOwner::Kernel
                        | PageOwner::PageTable
                        | PageOwner::Shared
                        | PageOwner::PageCache
                        | PageOwner::IpcObject,
                    )
                    | None => {
                        released_kernel = released_kernel.saturating_add(1);
                    }
                    Some(PageOwner::Free) => {}
                }
            }
        }
        if released_kernel != 0 {
            crate::resource_contract::ResourceContract::release(
                crate::resource_contract::AccountableEntity::KERNEL,
                crate::resource_contract::ResourceKind::MemoryPages,
                released_kernel,
            );
        }
        for (pid, released) in released_process {
            if pid != 0 && released != 0 {
                crate::resource_contract::ResourceContract::release(
                    crate::resource_contract::AccountableEntity::process(pid),
                    crate::resource_contract::ResourceKind::MemoryPages,
                    released,
                );
            }
        }
        for i in 0..pages {
            record_page(
                PageRecord {
                    phys: phys + (i * crate::memory::frame::FRAME_SIZE) as u64,
                    owner: PageOwner::Free,
                    refcount: 0,
                    mappings: 0,
                    flags: 0,
                },
                tag,
            );
        }
        Self::emit_event(
            crate::kds::KdsEventType::PageFree,
            crate::kds::KdsSeverity::Info,
            [phys, pages as u64, 0, crate::memory::paging::active_pml4()],
        );
    }

    fn emit_event(
        event_type: crate::kds::KdsEventType,
        severity: crate::kds::KdsSeverity,
        payload: [u64; 4],
    ) {
        crate::observability_contract::ObservabilityContract::emit_as_kds_event(
            crate::observability_contract::EventRecord {
                event: crate::observability_contract::ObservableEvent::Transition,
                contract: crate::observability_contract::ContractId::Memory,
                tag: crate::observability_contract::ObservationTag::Transition,
                reason: memory_event_name(event_type),
                outcome: match event_type {
                    crate::kds::KdsEventType::PageFault => {
                        if payload[3] != 0 {
                            crate::observability_contract::ObservationOutcome::Faulted
                        } else {
                            crate::observability_contract::ObservationOutcome::Failed
                        }
                    }
                    _ => crate::observability_contract::ObservationOutcome::Success,
                },
                resource: crate::observability_contract::ResourceClass::Memory,
                owner: crate::observability_contract::ObservabilityContract::current_pid_owner(),
                cpu: Some(crate::process::table::cpu_idx()),
                pid: crate::process::current_pid(),
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence: payload,
            },
            event_type,
            severity,
        );
    }
}

fn memory_event_name(event_type: crate::kds::KdsEventType) -> &'static str {
    match event_type {
        crate::kds::KdsEventType::PageAlloc => "memory.frame.alloc",
        crate::kds::KdsEventType::PageFree => "memory.frame.free",
        crate::kds::KdsEventType::Mmap => "memory.map",
        crate::kds::KdsEventType::Munmap => "memory.unmap",
        crate::kds::KdsEventType::CowFault => "memory.cow",
        crate::kds::KdsEventType::Mprotect => "memory.protect",
        crate::kds::KdsEventType::PageFault => "memory.fault",
        _ => "memory.state",
    }
}

fn current_owner() -> PageOwner {
    match crate::process::current_pid() {
        Some(pid) => PageOwner::Process(pid),
        None => PageOwner::Kernel,
    }
}

fn memory_owner(owner: PageOwner) -> crate::observability_contract::ResourceOwner {
    match owner {
        PageOwner::Process(pid) => crate::observability_contract::ResourceOwner::Pid(pid),
        PageOwner::Free
        | PageOwner::Kernel
        | PageOwner::Shared
        | PageOwner::PageTable
        | PageOwner::PageCache
        | PageOwner::IpcObject => crate::observability_contract::ResourceOwner::Unknown,
    }
}

fn encode_fault_evidence(report: PageFaultReport, handled: bool) -> u64 {
    handled as u64
        | ((report.access as u64) << 8)
        | ((report.class as u64) << 16)
        | ((report.resolution as u64) << 24)
}

fn accounting_chain_for_owner(owner: PageOwner) -> crate::resource_contract::AttributionChain {
    let accountable = accounting_entity_for_owner(owner);
    crate::resource_contract::AttributionChain {
        accountable,
        acting_pid: crate::process::current_pid(),
        correlation_id:
            crate::observability_contract::ObservabilityContract::current_correlation_id(),
        evidence_event_id: 0,
    }
}

fn accounting_entity_for_owner(owner: PageOwner) -> crate::resource_contract::AccountableEntity {
    match owner {
        PageOwner::Process(pid) => crate::resource_contract::AccountableEntity::process(pid),
        PageOwner::Free
        | PageOwner::Kernel
        | PageOwner::Shared
        | PageOwner::PageTable
        | PageOwner::PageCache
        | PageOwner::IpcObject => crate::resource_contract::AccountableEntity::KERNEL,
    }
}

fn owner_code(owner: PageOwner) -> u64 {
    match owner {
        PageOwner::Free => 0,
        PageOwner::Kernel => 1,
        PageOwner::Process(pid) => ((2u64) << 32) | pid as u64,
        PageOwner::Shared => 3,
        PageOwner::PageTable => 4,
        PageOwner::PageCache => 5,
        PageOwner::IpcObject => 6,
    }
}
