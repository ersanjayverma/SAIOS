//! Hardware constants for the x86_64 HAL.
//!
//! All numeric literals that encode an x86-64 hardware or firmware contract
//! are defined here so they appear in exactly one place.

// ─── UART / Serial port ──────────────────────────────────────────────────────

pub const COM1: u16 = 0x3F8;
pub const COM2: u16 = 0x2F8;
pub const COM3: u16 = 0x3E8;
pub const COM4: u16 = 0x2E8;

pub const UART_DATA: u16          = 0;
pub const UART_INTERRUPT_ENABLE: u16 = 1;
pub const UART_FIFO_CONTROL: u16  = 2;
pub const UART_LINE_CONTROL: u16  = 3;
pub const UART_MODEM_CONTROL: u16 = 4;
pub const UART_LINE_STATUS: u16   = 5;

pub const UART_LSR_DATA_READY: u8 = 0x01;
pub const UART_LSR_TX_EMPTY: u8   = 0x20;
pub const UART_LCR_8N1: u8        = 0x03;
pub const UART_LCR_DLAB: u8       = 0x80;
pub const UART_FCR_ENABLE: u8     = 0x01;
pub const UART_FCR_CLEAR_RX: u8   = 0x02;
pub const UART_FCR_CLEAR_TX: u8   = 0x04;
pub const UART_FCR_TRIGGER_14: u8 = 0xC0;
pub const UART_MCR_READY: u8      = 0x03;
pub const UART_DIVISOR_LOW_38400: u8  = 0x03;
pub const UART_DIVISOR_HIGH_38400: u8 = 0x00;
pub const UART_INTERRUPTS_DISABLED: u8 = 0x00;

// ─── VGA text mode ───────────────────────────────────────────────────────────

pub const VGA_PHYS_BASE: u64  = 0xB8000;
pub const VGA_CRTC_INDEX: u16 = 0x3D4;
pub const VGA_CRTC_DATA: u16  = 0x3D5;
pub const VGA_CURSOR_HIGH: u8 = 0x0E;
pub const VGA_CURSOR_LOW: u8  = 0x0F;
pub const VGA_WIDTH: usize    = 80;
pub const VGA_HEIGHT: usize   = 25;
/// Default white-on-black character attribute byte.
pub const VGA_ATTR: u8        = 0x0F;
pub const VGA_TAB_WIDTH: usize = 4;

// ─── GDT descriptor values ───────────────────────────────────────────────────

pub const GDT_NULL_DESCRIPTOR: u64           = 0;
pub const GDT_KERNEL_CODE_DESCRIPTOR: u64    = 0x00AF9A000000FFFF;
pub const GDT_KERNEL_DATA_DESCRIPTOR: u64    = 0x00AF92000000FFFF;
pub const GDT_USER_CODE_DESCRIPTOR: u64      = 0x00AFFA000000FFFF;
pub const GDT_USER_DATA_DESCRIPTOR: u64      = 0x00AFF2000000FFFF;
/// Segment type: 64-bit TSS (available), DPL 0.
pub const GDT_TSS_AVAILABLE_TYPE: u64        = 0x89;

// ─── IDT ─────────────────────────────────────────────────────────────────────

pub const IDT_ENTRY_COUNT: usize              = 256;
/// Present + DPL-0 + interrupt-gate (no auto-IF clear).
pub const IDT_INTERRUPT_GATE_ATTRS: u8        = 0x8E;

// ─── x86-64 MSR addresses ────────────────────────────────────────────────────

pub const MSR_IA32_EFER:  u32 = 0xC000_0080;
pub const MSR_IA32_STAR:  u32 = 0xC000_0081;
pub const MSR_IA32_LSTAR: u32 = 0xC000_0082;
pub const MSR_IA32_FMASK: u32 = 0xC000_0084;

// ─── EFER / RFLAGS bits ──────────────────────────────────────────────────────

/// System Call Extensions enable bit in IA32_EFER.
pub const EFER_SCE: u64 = 1 << 0;
/// No-Execute Enable bit in IA32_EFER.
pub const EFER_NXE: u64 = 1 << 11;
/// Interrupt Enable Flag in RFLAGS.
pub const RFLAGS_IF: u64 = 1 << 9;

// ─── Paging ──────────────────────────────────────────────────────────────────

pub const PAGE_SIZE: u64       = 4096;
pub const PAGING_ENTRY_COUNT: usize = 512;
pub const PTE_ADDR_MASK: u64   = 0x000F_FFFF_FFFF_F000;

pub const PTE_PRESENT:   u64 = 1 << 0;
pub const PTE_WRITABLE:  u64 = 1 << 1;
pub const PTE_USER:      u64 = 1 << 2;
pub const PTE_PWT:       u64 = 1 << 3;
pub const PTE_PCD:       u64 = 1 << 4;
pub const PTE_ACCESSED:  u64 = 1 << 5;
pub const PTE_DIRTY:     u64 = 1 << 6;
/// PAT bit for 4 KiB pages (also HUGE for larger pages).
pub const PTE_PAT_HUGE:  u64 = 1 << 7;
pub const PTE_GLOBAL:    u64 = 1 << 8;
pub const PTE_NX:        u64 = 1 << 63;

// ─── User-transition kernel stack ────────────────────────────────────────────

/// Size of the dedicated kernel stack for CPL3→CPL0 transitions (TSS.rsp0).
pub const USER_TRANSITION_STACK_SIZE: usize = 32 * 1024; // 32 KiB
/// Guard page size appended after the transition stack.
pub const USER_TRANSITION_GUARD_SIZE: usize = 4096;

// ─── User-mode entry control ─────────────────────────────────────────────────

/// Whether interrupts are re-enabled on entry to ring-3. `false` during
/// debugging so no timer preemption occurs before the first instruction.
pub const USER_ENTRY_ENABLE_INTERRUPTS: bool = true;

// ─── RFLAGS ──────────────────────────────────────────────────────────────────

/// Bit position of the Interrupt Enable Flag in RFLAGS (not a mask).
pub const RFLAGS_IF_BIT: u64 = 9;
