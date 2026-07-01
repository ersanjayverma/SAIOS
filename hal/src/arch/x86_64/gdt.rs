use crate::arch::x86_64::sync::StaticCell;
use crate::arch::x86_64::tss::{self, TaskStateSegment};
use core::{arch::asm, mem::size_of};

/// Segment selectors.
pub const KERNEL_CODE: SegmentSelector = SegmentSelector::new(1, 0);
pub const KERNEL_DATA: SegmentSelector = SegmentSelector::new(2, 0);

pub const USER_CODE: SegmentSelector = SegmentSelector::new(3, 3);
pub const USER_DATA: SegmentSelector = SegmentSelector::new(4, 3);

pub const TSS_SELECTOR: SegmentSelector = SegmentSelector::new(5, 0);

/// Global GDT storage.
static GDT: StaticCell<GlobalDescriptorTable> = StaticCell::new(GlobalDescriptorTable::new());

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct SystemDescriptor {
    low: u64,
    high: u64,
}

#[repr(transparent)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SegmentSelector(pub u16);

impl SegmentSelector {
    pub const fn new(index: u16, rpl: u16) -> Self {
        Self((index << 3) | (rpl & 0b11))
    }
}

struct GlobalDescriptorTable {
    entries: [u64; 7],
}

impl GlobalDescriptorTable {
    pub const fn new() -> Self {
        Self { entries: [0; 7] }
    }

    fn pointer(&self) -> GdtPointer {
        GdtPointer {
            limit: (size_of::<[u64; 7]>() - 1) as u16,
            base: self.entries.as_ptr() as u64,
        }
    }

    fn install_segments(&mut self) {
        // Long mode ignores base/limit for code/data segments.
        self.entries[0] = 0;

        // Kernel code
        self.entries[1] = 0x00AF9A000000FFFF;

        // Kernel data
        self.entries[2] = 0x00AF92000000FFFF;

        // User code
        self.entries[3] = 0x00AFFA000000FFFF;

        // User data
        self.entries[4] = 0x00AFF2000000FFFF;
    }

    fn install_tss(&mut self, tss: *const TaskStateSegment) {
        let base = tss as u64;
        let limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u32;

        let desc = SystemDescriptor::tss(base, limit);

        self.entries[5] = desc.low;
        self.entries[6] = desc.high;
    }

    unsafe fn load(&'static self) {
        let ptr = self.pointer();

        unsafe {
            asm!(
                "lgdt [{}]",
                in(reg) &ptr,
                options(readonly, nostack, preserves_flags)
            );
        }
    }
}

impl SystemDescriptor {
    fn tss(base: u64, limit: u32) -> Self {
        let low = ((limit as u64) & 0xFFFF)
            | ((base & 0xFFFF) << 16)
            | (((base >> 16) & 0xFF) << 32)
            | (0x89u64 << 40)
            | ((((limit as u64) >> 16) & 0xF) << 48)
            | (((base >> 24) & 0xFF) << 56);

        let high = base >> 32;

        Self { low, high }
    }
}

unsafe fn reload_segments() {
    unsafe {
        asm!(
            "mov ax, {data}",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ss, ax",
            data = const KERNEL_DATA.0,
            options(nostack, preserves_flags),
        );
    }

    // Reload CS with a far return.
    unsafe {
        asm!(
            "push {cs}",
            "lea rax, [rip + 2f]",
            "push rax",
            "retfq",
            "2:",
            cs = const KERNEL_CODE.0 as u64,
            out("rax") _,
        );
    }
}

unsafe fn load_tss() {
    unsafe {
        asm!(
            "ltr ax",
            in("ax") TSS_SELECTOR.0,
            options(nostack, preserves_flags),
        );
    }
}

/// Initialize and load the Global Descriptor Table.
pub fn init() {
    tss::init();

    unsafe {
        let gdt = GDT.get();

        (*gdt).install_segments();
        (*gdt).install_tss(tss::instance());

        (*gdt).load();

        reload_segments();
        load_tss();
    }
}
