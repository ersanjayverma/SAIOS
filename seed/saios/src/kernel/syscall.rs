use core::fmt;

use crate::timer;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

const ABI_VERSION: AbiVersion = AbiVersion {
    major: 1,
    minor: 0,
    patch: 0,
};

#[repr(u16)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SyscallNumber {
    Open = 1,
    Read = 2,
    Write = 3,
    Close = 4,
    Fork = 5,
    Exec = 6,
    Wait = 7,
    Exit = 8,
    Sleep = 9,
    GetPid = 10,
}

const SUPPORTED: [SyscallNumber; 10] = [
    SyscallNumber::Open,
    SyscallNumber::Read,
    SyscallNumber::Write,
    SyscallNumber::Close,
    SyscallNumber::Fork,
    SyscallNumber::Exec,
    SyscallNumber::Wait,
    SyscallNumber::Exit,
    SyscallNumber::Sleep,
    SyscallNumber::GetPid,
];

impl SyscallNumber {
    pub fn as_str(self) -> &'static str {
        match self {
            SyscallNumber::Open => "open",
            SyscallNumber::Read => "read",
            SyscallNumber::Write => "write",
            SyscallNumber::Close => "close",
            SyscallNumber::Fork => "fork",
            SyscallNumber::Exec => "exec",
            SyscallNumber::Wait => "wait",
            SyscallNumber::Exit => "exit",
            SyscallNumber::Sleep => "sleep",
            SyscallNumber::GetPid => "getpid",
        }
    }

    pub fn from_raw(raw: u64) -> Option<Self> {
        match raw {
            1 => Some(SyscallNumber::Open),
            2 => Some(SyscallNumber::Read),
            3 => Some(SyscallNumber::Write),
            4 => Some(SyscallNumber::Close),
            5 => Some(SyscallNumber::Fork),
            6 => Some(SyscallNumber::Exec),
            7 => Some(SyscallNumber::Wait),
            8 => Some(SyscallNumber::Exit),
            9 => Some(SyscallNumber::Sleep),
            10 => Some(SyscallNumber::GetPid),
            _ => None,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("open") {
            Some(SyscallNumber::Open)
        } else if name.eq_ignore_ascii_case("read") {
            Some(SyscallNumber::Read)
        } else if name.eq_ignore_ascii_case("write") {
            Some(SyscallNumber::Write)
        } else if name.eq_ignore_ascii_case("close") {
            Some(SyscallNumber::Close)
        } else if name.eq_ignore_ascii_case("fork") {
            Some(SyscallNumber::Fork)
        } else if name.eq_ignore_ascii_case("exec") {
            Some(SyscallNumber::Exec)
        } else if name.eq_ignore_ascii_case("wait") {
            Some(SyscallNumber::Wait)
        } else if name.eq_ignore_ascii_case("exit") {
            Some(SyscallNumber::Exit)
        } else if name.eq_ignore_ascii_case("sleep") {
            Some(SyscallNumber::Sleep)
        } else if name.eq_ignore_ascii_case("getpid") {
            Some(SyscallNumber::GetPid)
        } else {
            None
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SyscallError {
    InvalidNumber,
    InvalidArgument,
    Unimplemented,
}

impl SyscallError {
    pub fn code(self) -> i64 {
        match self {
            SyscallError::InvalidNumber => -38,
            SyscallError::InvalidArgument => -22,
            SyscallError::Unimplemented => -78,
        }
    }
}

impl fmt::Display for SyscallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyscallError::InvalidNumber => f.write_str("invalid syscall number"),
            SyscallError::InvalidArgument => f.write_str("invalid syscall argument"),
            SyscallError::Unimplemented => f.write_str("syscall not implemented"),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SyscallContext {
    pub pid: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SyscallRequest {
    pub number: SyscallNumber,
    pub args: [u64; 6],
}

pub fn abi_version() -> AbiVersion {
    ABI_VERSION
}

pub fn supported() -> &'static [SyscallNumber] {
    &SUPPORTED
}

pub fn dispatch(req: SyscallRequest, ctx: SyscallContext) -> Result<u64, SyscallError> {
    match req.number {
        SyscallNumber::GetPid => Ok(ctx.pid),
        SyscallNumber::Sleep => {
            let ms = req.args[0];
            timer::sleep(ms);
            Ok(0)
        }
        SyscallNumber::Exit => Ok(req.args[0]),
        SyscallNumber::Open
        | SyscallNumber::Read
        | SyscallNumber::Write
        | SyscallNumber::Close
        | SyscallNumber::Fork
        | SyscallNumber::Exec
        | SyscallNumber::Wait => Err(SyscallError::Unimplemented),
    }
}
