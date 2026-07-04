//! System call dispatcher.
//!
//! Translates integer syscall requests from user-space into kernel operations
//! on the VFS, process manager and timer. Unsupported or unimplemented
//! syscalls return negative error codes compatible with POSIX errno values.

use core::fmt;

use crate::console;
use crate::kernel::process;
use crate::timer;
use crate::vfs;

/// Kernel syscall ABI version.
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
/// Identifiers for supported system calls.
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
    Spawn = 11,
}

const SUPPORTED: [SyscallNumber; 11] = [
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
    SyscallNumber::Spawn,
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
            SyscallNumber::Spawn => "spawn",
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
            11 => Some(SyscallNumber::Spawn),
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
        } else if name.eq_ignore_ascii_case("spawn") {
            Some(SyscallNumber::Spawn)
        } else {
            None
        }
    }
}

fn selector_to_program(selector: u64) -> Option<&'static str> {
    match selector {
        1 => Some("hello"),
        2 => Some("calc"),
        3 => Some("editor"),
        4 => Some("shell"),
        5 => Some("ls"),
        6 => Some("cat"),
        7 => Some("cp"),
        8 => Some("mv"),
        9 => Some("rm"),
        10 => Some("mkdir"),
        11 => Some("ps"),
        12 => Some("kill"),
        _ => None,
    }
}

/// Map an `open` path selector to a well-known filesystem path.
///
/// The syscall ABI only carries integer arguments (there is no user-space
/// memory model to pass a string pointer), so file paths are addressed by a
/// small fixed selector table, mirroring how [`selector_to_program`] addresses
/// executables.
fn selector_to_path(selector: u64) -> Option<&'static str> {
    match selector {
        1 => Some("/etc/motd"),
        2 => Some("/tmp/scratch"),
        3 => Some("/boot/package.manifest"),
        4 => Some("/home/user/notes.txt"),
        5 => Some("/tmp/syscall.out"),
        _ => None,
    }
}

/// Map a `write` data selector to a fixed payload.
///
/// As with [`selector_to_path`], arbitrary buffers cannot be passed through the
/// integer-only ABI, so writable data is chosen from a small predefined table.
fn selector_to_data(selector: u64) -> Option<&'static [u8]> {
    match selector {
        0 => Some(b""),
        1 => Some(b"hello\n"),
        2 => Some(b"SAIOS syscall write test\n"),
        3 => Some(b"The quick brown fox jumps over the lazy dog\n"),
        _ => None,
    }
}

/// Translate the `mode` argument of the `open` syscall into VFS open options.
///
/// * `0` -> read-only
/// * `1` -> read/write, creating the file if missing
/// * `2` -> append, creating the file if missing
fn open_mode_to_options(mode: u64) -> Option<vfs::OpenOptions> {
    match mode {
        0 => Some(vfs::OpenOptions::read_only()),
        1 => Some(vfs::OpenOptions::read_write_create()),
        2 => Some(vfs::OpenOptions::append_create()),
        _ => None,
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
/// Error returned when a syscall cannot be completed.
pub enum SyscallError {
    /// The requested syscall number is not recognized.
    InvalidNumber,
    /// One or more arguments are invalid.
    InvalidArgument,
    /// The syscall is recognized but not yet implemented.
    Unimplemented,
}

impl SyscallError {
    /// Returns the negative error code returned to user-space.
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
/// Context in which a syscall is executed.
pub struct SyscallContext {
    /// Process identifier of the caller.
    pub pid: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
/// A decoded system call request.
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

/// Dispatches `req` in the context `ctx` and returns the syscall result.
pub fn dispatch(req: SyscallRequest, ctx: SyscallContext) -> Result<u64, SyscallError> {
    match req.number {
        SyscallNumber::GetPid => Ok(ctx.pid),
        SyscallNumber::Sleep => {
            let ms = req.args[0];
            timer::sleep(ms);
            Ok(0)
        }
        SyscallNumber::Exit => {
            let code = req.args[0] as i32;
            process::exit(ctx.pid, code).map_err(|_| SyscallError::InvalidArgument)?;
            Ok(code as u64)
        }
        SyscallNumber::Wait => {
            let pid = req.args[0];
            if pid == 0 {
                return Err(SyscallError::InvalidArgument);
            }
            let code = process::wait(pid).map_err(|_| SyscallError::InvalidArgument)?;
            Ok(code as u64)
        }
        SyscallNumber::Exec => {
            let selector = req.args[0];
            let name = selector_to_program(selector).ok_or(SyscallError::InvalidArgument)?;
            let code = process::exec(name, &[], &[]).map_err(|_| SyscallError::InvalidArgument)?;
            Ok(code as u64)
        }
        SyscallNumber::Spawn => {
            let selector = req.args[0];
            let name = selector_to_program(selector).ok_or(SyscallError::InvalidArgument)?;
            let pid = process::spawn(name, &[], &[]).map_err(|_| SyscallError::InvalidArgument)?;
            Ok(pid)
        }
        SyscallNumber::Open => {
            // args[0] = path selector, args[1] = open mode (0=ro, 1=rw+create, 2=append+create).
            let path = selector_to_path(req.args[0]).ok_or(SyscallError::InvalidArgument)?;
            let options = open_mode_to_options(req.args[1]).ok_or(SyscallError::InvalidArgument)?;
            let fd = vfs::open(path, options).map_err(|_| SyscallError::InvalidArgument)?;
            Ok(fd as u64)
        }
        SyscallNumber::Read => {
            // args[0] = fd, args[1] = max bytes to read (0 defaults to 4096).
            let fd = req.args[0] as vfs::VfsFd;
            let max_len = if req.args[1] == 0 {
                4096
            } else {
                req.args[1] as usize
            };
            let data = vfs::read(fd, max_len).map_err(|_| SyscallError::InvalidArgument)?;
            // Echo the bytes to the console (the integer-only ABI has no user
            // buffer to fill) and report how many bytes were read.
            console::print(core::str::from_utf8(&data).unwrap_or("<binary>"));
            Ok(data.len() as u64)
        }
        SyscallNumber::Write => {
            // args[0] = fd, args[1] = data selector.
            let fd = req.args[0] as vfs::VfsFd;
            let data = selector_to_data(req.args[1]).ok_or(SyscallError::InvalidArgument)?;
            let written = vfs::write(fd, data).map_err(|_| SyscallError::InvalidArgument)?;
            Ok(written as u64)
        }
        SyscallNumber::Close => {
            // args[0] = fd.
            let fd = req.args[0] as vfs::VfsFd;
            vfs::close(fd).map_err(|_| SyscallError::InvalidArgument)?;
            Ok(0)
        }
        SyscallNumber::Fork => {
            // Duplicate the calling process by spawning a fresh instance of the
            // same program image. Returns the child pid. Without per-process
            // address spaces this is a spawn of the caller's program rather than
            // a copy-on-write clone.
            let name = process::jobs()
                .into_iter()
                .find(|r| r.pid == ctx.pid)
                .map(|r| r.name)
                .ok_or(SyscallError::InvalidArgument)?;
            let child = process::spawn(name.as_str(), &[], &[])
                .map_err(|_| SyscallError::InvalidArgument)?;
            Ok(child)
        }
    }
}
