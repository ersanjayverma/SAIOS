//! x86_64 Global Descriptor Table and TSS setup.
//!
//! Segment layout is dictated by SYSRETQ semantics:
//!   SYSRETQ CS = STAR[63:48]+16 | 3
//!   SYSRETQ SS = STAR[63:48]+8  | 3
//!
//! So user-data must immediately precede user-code in the table.
//!
//!   0x00  null
//!   0x08  kernel code   (ring 0, 64-bit)   ← SYSCALL CS
//!   0x10  kernel data   (ring 0)            ← SYSCALL SS = kernel CS + 8
//!   0x18  user data     (ring 3)            ← SYSRETQ SS = STAR[63:48]+8
//!   0x20  user code     (ring 3, 64-bit)    ← SYSRETQ CS = STAR[63:48]+16
//!   0x28  TSS low       (system, 16 bytes)
//!   0x30  TSS high

use crate::process::table::{MAX_CPUS, cpu_idx};
use lazy_static::lazy_static;
use x86_64::VirtAddr;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;

unsafe extern "C" {
    static stack_top: u8;
}

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

// Selectors (with RPL bits)
pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_SS: u16 = 0x10;
pub const USER_DS: u16 = 0x18 | 3;
pub const USER_CS: u16 = 0x20 | 3;

lazy_static! {
    static ref TSS: [TaskStateSegment; MAX_CPUS] = core::array::from_fn(|cpu| {
        let mut tss = TaskStateSegment::new();
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACKS: [u8; STACK_SIZE * MAX_CPUS] = [0; STACK_SIZE * MAX_CPUS];
            let stack_start = VirtAddr::new(core::ptr::addr_of!(STACKS) as u64);
            stack_start + ((cpu + 1) * STACK_SIZE) as u64
        };
        tss
    });
    pub static ref GDT: [CpuGdt; MAX_CPUS] = core::array::from_fn(|cpu| {
        let mut gdt = GlobalDescriptorTable::new();
        let kcode = gdt.add_entry(Descriptor::kernel_code_segment());
        let kdata = gdt.add_entry(Descriptor::kernel_data_segment());
        let udata = gdt.add_entry(Descriptor::user_data_segment());
        let ucode = gdt.add_entry(Descriptor::user_code_segment());
        let tss = gdt.add_entry(Descriptor::tss_segment(&TSS[cpu]));
        CpuGdt {
            gdt,
            selectors: Selectors {
                kcode,
                kdata,
                udata,
                ucode,
                tss,
            },
        }
    });
}

pub struct CpuGdt {
    pub gdt: GlobalDescriptorTable,
    pub selectors: Selectors,
}

pub struct Selectors {
    pub kcode: SegmentSelector,
    pub kdata: SegmentSelector,
    pub udata: SegmentSelector,
    pub ucode: SegmentSelector,
    pub tss: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};
    use x86_64::instructions::tables::load_tss;

    let gdt = &GDT[0];
    gdt.gdt.load();
    unsafe {
        CS::set_reg(gdt.selectors.kcode);
        SS::set_reg(gdt.selectors.kdata);
        DS::set_reg(gdt.selectors.kdata);
        ES::set_reg(gdt.selectors.kdata);
        load_tss(gdt.selectors.tss);
    }
    set_kernel_stack(unsafe { &stack_top as *const u8 as u64 });
}

/// Load the shared GDT and kernel segments on an application processor.
/// APs load their own TSS selector so ring-3 interrupts and exceptions use the
/// process kernel stack selected for that CPU by the scheduler.
pub fn load_on_ap() {
    use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};
    use x86_64::instructions::tables::load_tss;
    let gdt = &GDT[cpu_idx()];
    gdt.gdt.load();
    unsafe {
        CS::set_reg(gdt.selectors.kcode);
        SS::set_reg(gdt.selectors.kdata);
        DS::set_reg(gdt.selectors.kdata);
        ES::set_reg(gdt.selectors.kdata);
        load_tss(gdt.selectors.tss);
    }
}

/// Update TSS.RSP0 — the kernel stack used when entering ring 0 via interrupt.
/// Called on every context switch.
pub fn set_kernel_stack(rsp0: u64) {
    unsafe {
        // We reach into the lazy_static TSS via raw pointer to mutate this CPU's RSP0.
        let tss_ptr = &TSS[cpu_idx()] as *const TaskStateSegment as *mut TaskStateSegment;
        (*tss_ptr).privilege_stack_table[0] = VirtAddr::new(rsp0);
    }
}

pub fn current_rsp0() -> u64 {
    TSS[cpu_idx()].privilege_stack_table[0].as_u64()
}

pub fn tss_rsp0_valid() -> bool {
    TSS[cpu_idx()].privilege_stack_table[0].as_u64() != 0
}

/// Returns true if the BSP TSS double-fault stack is configured (proves TSS loaded).
pub fn tss_ist0_valid() -> bool {
    TSS[cpu_idx()].interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize].as_u64() != 0
}
