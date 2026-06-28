use efi_main::SaiosBootInfo;

pub type Pid = u32;
pub type Tid = u32;

#[derive(Copy, Clone)]
pub enum Exception {
    InvalidOpcode,
    GeneralProtection,
    PageFault,
    Panic,
    Fatal,
    Unknown(u32),
}

impl Exception {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidOpcode => "#UD Invalid Opcode",
            Self::GeneralProtection => "#GP General Protection",
            Self::PageFault => "#PF Page Fault",
            Self::Panic => "Kernel Panic",
            Self::Fatal => "Fatal Error",
            Self::Unknown(_) => "Unknown Exception",
        }
    }
}

pub struct RRodContext {
    pub reason: &'static str,
    pub exception: Exception,
    pub cpu: u32,
    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub cr2: u64,
    pub error_code: u64,
    pub file: &'static str,
    pub line: u32,
    pub process: Option<Pid>,
    pub thread: Option<Tid>,
}

static mut LAST_BOOT_INFO: *const SaiosBootInfo = core::ptr::null();

pub fn set_boot_info(boot_info: *const SaiosBootInfo) {
    unsafe {
        LAST_BOOT_INFO = boot_info;
    }
}

pub fn boot_info() -> Option<&'static SaiosBootInfo> {
    let ptr = unsafe { LAST_BOOT_INFO };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}
