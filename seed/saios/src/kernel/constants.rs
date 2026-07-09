//! Kernel-wide constants.
//!
//! Architecture-specific hardware constants (MSRs, VGA ports, paging flags,
//! user-transition stack sizes) are defined once in `hal::arch::x86_64::constants`
//! and re-exported here so kernel code has a single import path.

// Re-export hardware constants from HAL

pub use hal::arch::x86_64::constants::{
    PAGE_SIZE, PTE_ADDR_MASK,
    PTE_PRESENT, PTE_WRITABLE, PTE_USER, PTE_PWT, PTE_PCD,
    PTE_ACCESSED, PTE_DIRTY, PTE_PAT_HUGE, PTE_GLOBAL, PTE_NX,
    MSR_IA32_EFER, MSR_IA32_STAR, MSR_IA32_LSTAR, MSR_IA32_FMASK,
    EFER_SCE, EFER_NXE, RFLAGS_IF,
    VGA_PHYS_BASE, VGA_WIDTH, VGA_HEIGHT, VGA_TAB_WIDTH, VGA_ATTR,
    VGA_CRTC_INDEX, VGA_CRTC_DATA, VGA_CURSOR_HIGH, VGA_CURSOR_LOW,
    USER_TRANSITION_STACK_SIZE, USER_TRANSITION_GUARD_SIZE,
    USER_ENTRY_ENABLE_INTERRUPTS,
};

pub const KERNEL_PHYS_BASE: u64 = 0x0010_0000;
pub const PMM_MIN_ALLOC_PHYS: u64 = 0x0000_1000;
pub const EARLY_TABLE_MIN_PHYS: u64 = 0x0010_0000;
pub const EARLY_TABLE_MAX_PHYS: u64 = 0x4000_0000;
pub const EARLY_TABLE_FALLBACK_MIN_PHYS: u64 = 0x0000_1000;
pub const FALLBACK_IDENTITY_HEAP_MAX_PHYS: u64 = 0x0400_0000;
pub const CR3_MIN_SWITCH_PHYS: u64 = 0x0010_0000;
pub const KERNEL_VIRT_BASE: u64 = 0xFFFF_FFFF_8000_0000;
pub const KERNEL_IMAGE_MIRROR_BASE: u64 = 0xFFFF_FFFF_8000_0000;
pub const HUGE_PAGE_SIZE_2M: u64 = 2 * 1024 * 1024;
pub const HUGE_PAGE_SIZE_1G: u64 = 1024 * 1024 * 1024;
pub const HEAP_INITIAL_BYTES: usize = 32 * 1024 * 1024;
pub const HEAP_FALLBACK_INITIAL_BYTES: usize = 2 * 1024 * 1024;
pub const HEAP_MAX_BYTES: usize = 1024 * 1024 * 1024;
pub const HEAP_FALLBACK_MAX_BYTES: usize = 32 * 1024 * 1024;
pub const HEAP_IDENTITY_MAX_PHYS: u64 = EARLY_TABLE_MAX_PHYS;
pub const KERNEL_THREAD_STACK_SIZE: usize = 64 * 1024;
/// Per-process kernel stack for ring3→ring0 syscall/interrupt transitions.
pub const USER_PROCESS_KERNEL_STACK_SIZE: usize = 64 * 1024;
pub const USER_ELF_LOAD_BASE: u64 = 0x0040_0000;
pub const USER_STACK_BASE: u64 = 0x0000_4000_0000_0000;
pub const USER_STACK_PAGES: usize = 64;
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const ELFCLASS64: u8 = 2;
pub const ELFDATA2LSB: u8 = 1;
pub const EV_CURRENT: u8 = 1;
pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;
pub const EM_X86_64: u16 = 62;
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PF_X: u32 = 0x1;
pub const PF_W: u32 = 0x2;
pub const PF_R: u32 = 0x4;
pub const DT_NULL: i64 = 0;
pub const DT_NEEDED: i64 = 1;
pub const DT_STRTAB: i64 = 5;
pub const DT_STRSZ: i64 = 10;
pub const DT_RELA: i64 = 7;
pub const DT_RELASZ: i64 = 8;
pub const DT_RELAENT: i64 = 9;
pub const DT_RELACOUNT: i64 = 0x6fff_fff9;
pub const R_X86_64_RELATIVE: u32 = 8;
pub const AT_NULL: u64 = 0;
pub const AT_PHDR: u64 = 3;
pub const AT_PHENT: u64 = 4;
pub const AT_PHNUM: u64 = 5;
pub const AT_PAGESZ: u64 = 6;
pub const AT_ENTRY: u64 = 9;
pub const AT_RANDOM: u64 = 25;
pub const AT_EXECFN: u64 = 31;
pub const PF_ERR_USER: usize = 1 << 2;

