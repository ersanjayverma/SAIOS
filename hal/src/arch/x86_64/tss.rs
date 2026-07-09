use crate::arch::x86_64::sync::StaticCell;
#[repr(C)]
pub struct TaskStateSegment {
    pub reserved1: u32,
    pub rsp: [u64; 3],
    pub reserved2: u64,
    pub ist: [u64; 7],
    pub reserved3: u64,
    pub reserved4: u16,
    pub io_map_base: u16,
}
static TSS: StaticCell<TaskStateSegment> = StaticCell::new(TaskStateSegment::new());
impl TaskStateSegment {
    pub const fn new() -> Self {
        Self {
            reserved1: 0,
            rsp: [0; 3],
            reserved2: 0,
            ist: [0; 7],
            reserved3: 0,
            reserved4: 0,
            io_map_base: core::mem::size_of::<TaskStateSegment>() as u16,
        }
    }
}

impl Default for TaskStateSegment {
    fn default() -> Self {
        Self::new()
    }
}

pub fn instance() -> *const TaskStateSegment {
    TSS.get() as *const TaskStateSegment
}

pub fn instance_mut() -> *mut TaskStateSegment {
    TSS.get()
}
pub fn init() {
    unsafe {
        let tss = &mut *TSS.get();
        tss.io_map_base = core::mem::size_of::<TaskStateSegment>() as u16;

        // Temporary values until memory management is ready.
        tss.rsp[0] = 0;
        tss.rsp[1] = 0;
        tss.rsp[2] = 0;

        tss.ist = [0; 7];
    }
}
pub fn set_kernel_stack(rsp0: u64) {
    unsafe {
        let tss = &mut *TSS.get();
        tss.rsp[0] = rsp0;
    }
}

pub fn set_ist(index: usize, stack: u64) {
    assert!(index < 7);

    unsafe {
        let tss = &mut *TSS.get();
        tss.ist[index] = stack;
    }
}
pub fn set_rsp0(stack: u64) {
    unsafe {
        let tss = &mut *TSS.get();
        tss.rsp[0] = stack;
    }
}

pub fn rsp0() -> u64 {
    unsafe { (*TSS.get()).rsp[0] }
}

pub fn ist(index: usize) -> u64 {
    unsafe { (*TSS.get()).ist[index] }
}
