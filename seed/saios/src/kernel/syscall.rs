//! System call dispatcher.
//!
//! Translates integer syscall requests from user-space into kernel operations
//! on the VFS, process manager and timer. Unsupported or unimplemented
//! syscalls return negative error codes compatible with POSIX errno values.

use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

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
    minor: 2,
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
    WaitPid = 7,
    Exit = 8,
    Sleep = 9,
    GetPid = 10,
    Lseek = 11,
    Stat = 12,
    Fstat = 13,
    Getdents64 = 14,
    Mmap = 15,
    Munmap = 16,
    Brk = 17,
    Dup = 18,
    Dup2 = 19,
    Pipe = 20,
    Ioctl = 21,
    Poll = 22,
    Select = 23,
    Clone = 24,
    Spawn = 25,
    Kill = 26,
}

const SUPPORTED: [SyscallNumber; 26] = [
    SyscallNumber::Open,
    SyscallNumber::Read,
    SyscallNumber::Write,
    SyscallNumber::Close,
    SyscallNumber::Fork,
    SyscallNumber::Exec,
    SyscallNumber::WaitPid,
    SyscallNumber::Exit,
    SyscallNumber::Sleep,
    SyscallNumber::GetPid,
    SyscallNumber::Lseek,
    SyscallNumber::Stat,
    SyscallNumber::Fstat,
    SyscallNumber::Getdents64,
    SyscallNumber::Mmap,
    SyscallNumber::Munmap,
    SyscallNumber::Brk,
    SyscallNumber::Dup,
    SyscallNumber::Dup2,
    SyscallNumber::Pipe,
    SyscallNumber::Ioctl,
    SyscallNumber::Poll,
    SyscallNumber::Select,
    SyscallNumber::Clone,
    SyscallNumber::Spawn,
    SyscallNumber::Kill,
];

const WAIT_NOHANG: u64 = 0x1;
const WAIT_UNTRACED: u64 = 0x2;
const WAIT_CONTINUED: u64 = 0x8;

const POLLIN: u64 = 0x0001;
const POLLOUT: u64 = 0x0004;
const POLLERR: u64 = 0x0008;
const POLLHUP: u64 = 0x0010;
const POLLNVAL: u64 = 0x0020;

const TCGETS: u64 = 0x5401;
const TIOCGPGRP: u64 = 0x540F;
const TIOCSPGRP: u64 = 0x5410;
const TIOCGWINSZ: u64 = 0x5413;
const FIONREAD: u64 = 0x541B;
const FIONBIO: u64 = 0x5421;

const SIGCHLD: u64 = 17;

const CLONE_VM: u64 = 0x0000_0100;
const CLONE_FS: u64 = 0x0000_0200;
const CLONE_FILES: u64 = 0x0000_0400;
const CLONE_SIGHAND: u64 = 0x0000_0800;
const CLONE_THREAD: u64 = 0x0001_0000;
const CLONE_SETTLS: u64 = 0x0008_0000;
const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;

const UNSUPPORTED_CLONE_FLAGS: u64 = CLONE_VM
    | CLONE_FS
    | CLONE_FILES
    | CLONE_SIGHAND
    | CLONE_THREAD
    | CLONE_SETTLS
    | CLONE_PARENT_SETTID
    | CLONE_CHILD_CLEARTID;

impl SyscallNumber {
    pub fn as_str(self) -> &'static str {
        match self {
            SyscallNumber::Open => "open",
            SyscallNumber::Read => "read",
            SyscallNumber::Write => "write",
            SyscallNumber::Close => "close",
            SyscallNumber::Fork => "fork",
            SyscallNumber::Exec => "exec",
            SyscallNumber::WaitPid => "waitpid",
            SyscallNumber::Exit => "exit",
            SyscallNumber::Sleep => "sleep",
            SyscallNumber::GetPid => "getpid",
            SyscallNumber::Lseek => "lseek",
            SyscallNumber::Stat => "stat",
            SyscallNumber::Fstat => "fstat",
            SyscallNumber::Getdents64 => "getdents64",
            SyscallNumber::Mmap => "mmap",
            SyscallNumber::Munmap => "munmap",
            SyscallNumber::Brk => "brk",
            SyscallNumber::Dup => "dup",
            SyscallNumber::Dup2 => "dup2",
            SyscallNumber::Pipe => "pipe",
            SyscallNumber::Ioctl => "ioctl",
            SyscallNumber::Poll => "poll",
            SyscallNumber::Select => "select",
            SyscallNumber::Clone => "clone",
            SyscallNumber::Spawn => "spawn",
            SyscallNumber::Kill => "kill",
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
            7 => Some(SyscallNumber::WaitPid),
            8 => Some(SyscallNumber::Exit),
            9 => Some(SyscallNumber::Sleep),
            10 => Some(SyscallNumber::GetPid),
            11 => Some(SyscallNumber::Lseek),
            12 => Some(SyscallNumber::Stat),
            13 => Some(SyscallNumber::Fstat),
            14 => Some(SyscallNumber::Getdents64),
            15 => Some(SyscallNumber::Mmap),
            16 => Some(SyscallNumber::Munmap),
            17 => Some(SyscallNumber::Brk),
            18 => Some(SyscallNumber::Dup),
            19 => Some(SyscallNumber::Dup2),
            20 => Some(SyscallNumber::Pipe),
            21 => Some(SyscallNumber::Ioctl),
            22 => Some(SyscallNumber::Poll),
            23 => Some(SyscallNumber::Select),
            24 => Some(SyscallNumber::Clone),
            25 => Some(SyscallNumber::Spawn),
            26 => Some(SyscallNumber::Kill),
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
        } else if name.eq_ignore_ascii_case("wait") || name.eq_ignore_ascii_case("waitpid") {
            Some(SyscallNumber::WaitPid)
        } else if name.eq_ignore_ascii_case("exit") {
            Some(SyscallNumber::Exit)
        } else if name.eq_ignore_ascii_case("sleep") {
            Some(SyscallNumber::Sleep)
        } else if name.eq_ignore_ascii_case("getpid") {
            Some(SyscallNumber::GetPid)
        } else if name.eq_ignore_ascii_case("lseek") {
            Some(SyscallNumber::Lseek)
        } else if name.eq_ignore_ascii_case("stat") {
            Some(SyscallNumber::Stat)
        } else if name.eq_ignore_ascii_case("fstat") {
            Some(SyscallNumber::Fstat)
        } else if name.eq_ignore_ascii_case("getdents64") {
            Some(SyscallNumber::Getdents64)
        } else if name.eq_ignore_ascii_case("mmap") {
            Some(SyscallNumber::Mmap)
        } else if name.eq_ignore_ascii_case("munmap") {
            Some(SyscallNumber::Munmap)
        } else if name.eq_ignore_ascii_case("brk") {
            Some(SyscallNumber::Brk)
        } else if name.eq_ignore_ascii_case("dup") {
            Some(SyscallNumber::Dup)
        } else if name.eq_ignore_ascii_case("dup2") {
            Some(SyscallNumber::Dup2)
        } else if name.eq_ignore_ascii_case("pipe") {
            Some(SyscallNumber::Pipe)
        } else if name.eq_ignore_ascii_case("ioctl") {
            Some(SyscallNumber::Ioctl)
        } else if name.eq_ignore_ascii_case("poll") {
            Some(SyscallNumber::Poll)
        } else if name.eq_ignore_ascii_case("select") {
            Some(SyscallNumber::Select)
        } else if name.eq_ignore_ascii_case("clone") {
            Some(SyscallNumber::Clone)
        } else if name.eq_ignore_ascii_case("spawn") {
            Some(SyscallNumber::Spawn)
        } else if name.eq_ignore_ascii_case("kill") {
            Some(SyscallNumber::Kill)
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

#[derive(Copy, Clone, Debug)]
enum DescriptorKind {
    Vfs(vfs::VfsFd),
    PipeRead(usize),
    PipeWrite(usize),
}

#[derive(Copy, Clone, Debug)]
struct DescriptorObject {
    kind: DescriptorKind,
    refs: u32,
    nonblocking: bool,
}

#[derive(Clone, Debug)]
struct PipeBuffer {
    data: Vec<u8>,
    read_pos: usize,
}

#[derive(Clone, Debug)]
struct ProcessFdTable {
    pid: u64,
    slots: Vec<Option<usize>>,
}

#[derive(Clone, Debug)]
struct SyscallState {
    procs: Vec<ProcessFdTable>,
    objects: Vec<Option<DescriptorObject>>,
    pipes: Vec<Option<PipeBuffer>>,
    brk: u64,
    next_mmap: u64,
}

impl SyscallState {
    fn new() -> Self {
        Self {
            procs: Vec::new(),
            objects: Vec::new(),
            pipes: Vec::new(),
            brk: 0x0100_0000,
            next_mmap: 0x1000_0000,
        }
    }

    fn proc_mut(&mut self, pid: u64) -> &mut ProcessFdTable {
        if let Some(idx) = self.procs.iter().position(|p| p.pid == pid) {
            return &mut self.procs[idx];
        }
        let mut slots = Vec::new();
        slots.resize(3, None); // reserve 0,1,2
        self.procs.push(ProcessFdTable { pid, slots });
        let idx = self.procs.len() - 1;
        &mut self.procs[idx]
    }

    fn alloc_object(&mut self, kind: DescriptorKind) -> usize {
        if let Some((idx, slot)) = self
            .objects
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.is_none())
        {
            *slot = Some(DescriptorObject {
                kind,
                refs: 1,
                nonblocking: false,
            });
            return idx;
        }
        self.objects.push(Some(DescriptorObject {
            kind,
            refs: 1,
            nonblocking: false,
        }));
        self.objects.len() - 1
    }

    fn alloc_fd(&mut self, pid: u64, obj: usize) -> u64 {
        let table = self.proc_mut(pid);
        for i in 3..table.slots.len() {
            if table.slots[i].is_none() {
                table.slots[i] = Some(obj);
                return i as u64;
            }
        }
        table.slots.push(Some(obj));
        (table.slots.len() - 1) as u64
    }

    fn obj_for_fd(&self, pid: u64, fd: u64) -> Option<usize> {
        let proc = self.procs.iter().find(|p| p.pid == pid)?;
        let idx = usize::try_from(fd).ok()?;
        proc.slots.get(idx).and_then(|s| *s)
    }

    fn close_fd(&mut self, pid: u64, fd: u64) -> Result<(), SyscallError> {
        if fd <= 2 {
            return Ok(());
        }
        let idx = usize::try_from(fd).map_err(|_| SyscallError::InvalidArgument)?;
        let proc = self.proc_mut(pid);
        let obj_idx = proc
            .slots
            .get_mut(idx)
            .and_then(|s| s.take())
            .ok_or(SyscallError::InvalidArgument)?;

        if let Some(obj) = self.objects.get_mut(obj_idx).and_then(|o| o.as_mut()) {
            if obj.refs > 1 {
                obj.refs -= 1;
                return Ok(());
            }
            let final_obj = *obj;
            self.objects[obj_idx] = None;
            if let DescriptorKind::Vfs(vfd) = final_obj.kind {
                let _ = vfs::close(vfd);
            }
        }
        Ok(())
    }
}

static STATE: StaticCell<Option<SyscallState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn with_state_mut<R>(f: impl FnOnce(&mut SyscallState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(SyscallState::new());
            }
            slot.as_mut().expect("syscall state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn resolve_vfs_fd(state: &SyscallState, pid: u64, fd: u64) -> Result<vfs::VfsFd, SyscallError> {
    if fd <= 2 {
        return Err(SyscallError::InvalidArgument);
    }
    let obj_idx = state
        .obj_for_fd(pid, fd)
        .ok_or(SyscallError::InvalidArgument)?;
    let obj = state
        .objects
        .get(obj_idx)
        .and_then(|o| *o)
        .ok_or(SyscallError::InvalidArgument)?;
    match obj.kind {
        DescriptorKind::Vfs(vfd) => Ok(vfd),
        _ => Err(SyscallError::InvalidArgument),
    }
}

fn seek_whence_to_from(whence: u64, off: i64) -> Option<vfs::SeekFrom> {
    match whence {
        0 => {
            if off < 0 {
                None
            } else {
                Some(vfs::SeekFrom::Start(off as usize))
            }
        }
        1 => Some(vfs::SeekFrom::Current(off as isize)),
        2 => Some(vfs::SeekFrom::End(off as isize)),
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
    /// No child process matches the requested wait target.
    NoChild,
    /// Operation would block and caller requested non-blocking behavior.
    WouldBlock,
    /// Inappropriate ioctl for this descriptor.
    NotTty,
    /// The syscall is recognized but not yet implemented.
    Unimplemented,
}

impl SyscallError {
    /// Returns the negative error code returned to user-space.
    pub fn code(self) -> i64 {
        match self {
            SyscallError::InvalidNumber => -38,
            SyscallError::InvalidArgument => -22,
            SyscallError::NoChild => -10,
            SyscallError::WouldBlock => -11,
            SyscallError::NotTty => -25,
            SyscallError::Unimplemented => -78,
        }
    }
}

impl fmt::Display for SyscallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyscallError::InvalidNumber => f.write_str("invalid syscall number"),
            SyscallError::InvalidArgument => f.write_str("invalid syscall argument"),
            SyscallError::NoChild => f.write_str("no child process"),
            SyscallError::WouldBlock => f.write_str("operation would block"),
            SyscallError::NotTty => f.write_str("inappropriate ioctl for device"),
            SyscallError::Unimplemented => f.write_str("syscall not implemented"),
        }
    }
}

fn encode_wait_status(code: i32) -> u64 {
    if code < 0 {
        ((-code) as u64) & 0x7F
    } else {
        ((code as u64) & 0xFF) << 8
    }
}

fn poll_fd_mask(
    state: &mut SyscallState,
    pid: u64,
    fd: u64,
    events: u64,
) -> Result<u64, SyscallError> {
    if fd <= 2 {
        let requested = if events == 0 {
            POLLIN | POLLOUT
        } else {
            events
        };
        return Ok(requested & (POLLIN | POLLOUT));
    }

    let obj_idx = state
        .obj_for_fd(pid, fd)
        .ok_or(SyscallError::InvalidArgument)?;
    let obj = state
        .objects
        .get(obj_idx)
        .and_then(|o| *o)
        .ok_or(SyscallError::InvalidArgument)?;

    let requested = if events == 0 {
        POLLIN | POLLOUT
    } else {
        events
    };
    let mut revents = 0u64;
    match obj.kind {
        DescriptorKind::Vfs(_) => {
            revents |= requested & (POLLIN | POLLOUT);
        }
        DescriptorKind::PipeRead(pipe_id) => {
            let pipe = state
                .pipes
                .get(pipe_id)
                .and_then(|p| p.as_ref())
                .ok_or(SyscallError::InvalidArgument)?;
            if pipe.read_pos < pipe.data.len() {
                revents |= POLLIN;
            }
        }
        DescriptorKind::PipeWrite(pipe_id) => {
            if state.pipes.get(pipe_id).and_then(|p| p.as_ref()).is_none() {
                revents |= POLLERR | POLLHUP;
            } else {
                revents |= POLLOUT;
            }
        }
    }
    Ok(revents & (requested | POLLERR | POLLHUP | POLLNVAL))
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
        SyscallNumber::WaitPid => {
            let target = req.args[0] as i64;
            let options = req.args[1];
            if target == 0 || target < -1 {
                return Err(SyscallError::InvalidArgument);
            }
            if options & !(WAIT_NOHANG | WAIT_UNTRACED | WAIT_CONTINUED) != 0 {
                return Err(SyscallError::InvalidArgument);
            }

            let has_child = if target == -1 {
                process::has_waitable_child(ctx.pid, None)
            } else {
                process::has_waitable_child(ctx.pid, Some(target as u64))
            };
            if !has_child {
                return Err(SyscallError::NoChild);
            }

            if target == -1 {
                if let Some((pid, code)) = process::first_exited_child(ctx.pid) {
                    let status = encode_wait_status(code);
                    let _ = process::reap_child(ctx.pid, pid);
                    return Ok((pid << 32) | (status & 0xFFFF_FFFF));
                }
                if (options & WAIT_NOHANG) != 0 {
                    return Ok(0);
                }
                return Err(SyscallError::WouldBlock);
            }

            let pid = target as u64;
            let rec = process::child_record(ctx.pid, pid).ok_or(SyscallError::NoChild)?;
            if rec.state != process::ProcessState::Exited {
                if (options & WAIT_NOHANG) != 0 {
                    return Ok(0);
                }
                return Err(SyscallError::WouldBlock);
            }

            let code =
                process::reap_child(ctx.pid, pid).map_err(|_| SyscallError::InvalidArgument)?;
            let status = encode_wait_status(code);
            Ok((pid << 32) | (status & 0xFFFF_FFFF))
        }
        SyscallNumber::Exec => {
            let selector = req.args[0];
            let name = selector_to_program(selector).ok_or(SyscallError::InvalidArgument)?;
            let code = process::exec_from(Some(ctx.pid), name, &[], &[])
                .map_err(|_| SyscallError::InvalidArgument)?;
            Ok(code as u64)
        }
        SyscallNumber::Spawn => {
            let selector = req.args[0];
            let name = selector_to_program(selector).ok_or(SyscallError::InvalidArgument)?;
            let pid = process::spawn_from(Some(ctx.pid), name, &[], &[])
                .map_err(|_| SyscallError::InvalidArgument)?;
            Ok(pid)
        }
        SyscallNumber::Open => {
            // args[0] = path selector, args[1] = open mode (0=ro, 1=rw+create, 2=append+create).
            let path = selector_to_path(req.args[0]).ok_or(SyscallError::InvalidArgument)?;
            let options = open_mode_to_options(req.args[1]).ok_or(SyscallError::InvalidArgument)?;
            let vfd = vfs::open(path, options).map_err(|_| SyscallError::InvalidArgument)?;
            let fd = with_state_mut(|state| {
                let obj = state.alloc_object(DescriptorKind::Vfs(vfd));
                state.alloc_fd(ctx.pid, obj)
            });
            Ok(fd)
        }
        SyscallNumber::Read => {
            // args[0] = fd, args[1] = max bytes to read (0 defaults to 4096).
            let fd = req.args[0];
            let max_len = if req.args[1] == 0 {
                4096
            } else {
                req.args[1] as usize
            };
            let data = with_state_mut(|state| {
                let obj_idx = state
                    .obj_for_fd(ctx.pid, fd)
                    .ok_or(SyscallError::InvalidArgument)?;
                let obj = state
                    .objects
                    .get(obj_idx)
                    .and_then(|o| *o)
                    .ok_or(SyscallError::InvalidArgument)?;
                match obj.kind {
                    DescriptorKind::Vfs(vfd) => {
                        vfs::read(vfd, max_len).map_err(|_| SyscallError::InvalidArgument)
                    }
                    DescriptorKind::PipeRead(pipe_id) => {
                        let pipe = state
                            .pipes
                            .get_mut(pipe_id)
                            .and_then(|p| p.as_mut())
                            .ok_or(SyscallError::InvalidArgument)?;
                        let available = pipe.data.len().saturating_sub(pipe.read_pos);
                        let take = available.min(max_len);
                        let start = pipe.read_pos;
                        let end = start + take;
                        let out = pipe.data[start..end].to_vec();
                        pipe.read_pos = end;
                        if pipe.read_pos >= pipe.data.len() {
                            pipe.data.clear();
                            pipe.read_pos = 0;
                        }
                        Ok(out)
                    }
                    DescriptorKind::PipeWrite(_) => Err(SyscallError::InvalidArgument),
                }
            })?;
            // Echo the bytes to the console (the integer-only ABI has no user
            // buffer to fill) and report how many bytes were read.
            console::print(core::str::from_utf8(&data).unwrap_or("<binary>"));
            Ok(data.len() as u64)
        }
        SyscallNumber::Write => {
            // args[0] = fd, args[1] = data selector.
            let fd = req.args[0];
            let data = selector_to_data(req.args[1]).ok_or(SyscallError::InvalidArgument)?;
            match fd {
                // Conventional stdio descriptors for process/runtime output channels.
                1 => {
                    let text = core::str::from_utf8(data).unwrap_or("<binary>");
                    console::print(text);
                    Ok(data.len() as u64)
                }
                2 => {
                    let text = core::str::from_utf8(data).unwrap_or("<binary>");
                    console::stderr_write_str(text);
                    Ok(data.len() as u64)
                }
                _ => {
                    let written = with_state_mut(|state| {
                        let obj_idx = state
                            .obj_for_fd(ctx.pid, fd)
                            .ok_or(SyscallError::InvalidArgument)?;
                        let obj = state
                            .objects
                            .get(obj_idx)
                            .and_then(|o| *o)
                            .ok_or(SyscallError::InvalidArgument)?;
                        match obj.kind {
                            DescriptorKind::Vfs(vfd) => {
                                vfs::write(vfd, data).map_err(|_| SyscallError::InvalidArgument)
                            }
                            DescriptorKind::PipeWrite(pipe_id) => {
                                let pipe = state
                                    .pipes
                                    .get_mut(pipe_id)
                                    .and_then(|p| p.as_mut())
                                    .ok_or(SyscallError::InvalidArgument)?;
                                pipe.data.extend_from_slice(data);
                                Ok(data.len())
                            }
                            DescriptorKind::PipeRead(_) => Err(SyscallError::InvalidArgument),
                        }
                    })?;
                    Ok(written as u64)
                }
            }
        }
        SyscallNumber::Close => {
            // args[0] = fd.
            let fd = req.args[0];
            with_state_mut(|state| state.close_fd(ctx.pid, fd))?;
            Ok(0)
        }
        SyscallNumber::Lseek => {
            // args[0] = fd, args[1] = offset (signed i64 bits), args[2] = whence (0/1/2).
            let off = req.args[1] as i64;
            let from =
                seek_whence_to_from(req.args[2], off).ok_or(SyscallError::InvalidArgument)?;
            let pos = with_state_mut(|state| {
                let vfd = resolve_vfs_fd(state, ctx.pid, req.args[0])?;
                vfs::seek(vfd, from).map_err(|_| SyscallError::InvalidArgument)
            })?;
            Ok(pos as u64)
        }
        SyscallNumber::Stat => {
            // args[0] = path selector.
            let path = selector_to_path(req.args[0]).ok_or(SyscallError::InvalidArgument)?;
            let st = vfs::stat(path).map_err(|_| SyscallError::InvalidArgument)?;
            Ok(st.size as u64)
        }
        SyscallNumber::Fstat => {
            // args[0] = fd. Returns file size in bytes.
            let size = with_state_mut(|state| {
                let vfd = resolve_vfs_fd(state, ctx.pid, req.args[0])?;
                let cur = vfs::seek(vfd, vfs::SeekFrom::Current(0))
                    .map_err(|_| SyscallError::InvalidArgument)?;
                let end = vfs::seek(vfd, vfs::SeekFrom::End(0))
                    .map_err(|_| SyscallError::InvalidArgument)?;
                let _ = vfs::seek(vfd, vfs::SeekFrom::Start(cur));
                Ok::<usize, SyscallError>(end)
            })?;
            Ok(size as u64)
        }
        SyscallNumber::Getdents64 => {
            // args[0] = path selector.
            let path = selector_to_path(req.args[0]).ok_or(SyscallError::InvalidArgument)?;
            let entries = vfs::readdir(path).map_err(|_| SyscallError::InvalidArgument)?;
            for name in &entries {
                console::println!("{}", name);
            }
            Ok(entries.len() as u64)
        }
        SyscallNumber::Mmap => {
            // args[0] = len bytes (0 defaults to one page). Returns synthetic VA.
            let mut len = req.args[0];
            if len == 0 {
                len = 4096;
            }
            let len_aligned = (len + 4095) & !4095;
            let addr = with_state_mut(|state| {
                let base = state.next_mmap;
                state.next_mmap = state.next_mmap.saturating_add(len_aligned);
                base
            });
            Ok(addr)
        }
        SyscallNumber::Munmap => {
            // args[0] = addr, args[1] = len. No-op in this build.
            let _addr = req.args[0];
            let _len = req.args[1];
            Ok(0)
        }
        SyscallNumber::Brk => {
            // args[0] = new break (0 = query current).
            let brk = with_state_mut(|state| {
                if req.args[0] != 0 {
                    state.brk = req.args[0];
                }
                state.brk
            });
            Ok(brk)
        }
        SyscallNumber::Dup => {
            // args[0] = oldfd
            let newfd = with_state_mut(|state| {
                let obj_idx = state
                    .obj_for_fd(ctx.pid, req.args[0])
                    .ok_or(SyscallError::InvalidArgument)?;
                let obj = state
                    .objects
                    .get_mut(obj_idx)
                    .and_then(|o| o.as_mut())
                    .ok_or(SyscallError::InvalidArgument)?;
                obj.refs = obj.refs.saturating_add(1);
                Ok::<u64, SyscallError>(state.alloc_fd(ctx.pid, obj_idx))
            })?;
            Ok(newfd)
        }
        SyscallNumber::Dup2 => {
            // args[0] = oldfd, args[1] = newfd
            let target = req.args[1];
            if target <= 2 {
                return Err(SyscallError::InvalidArgument);
            }
            let out = with_state_mut(|state| {
                let obj_idx = state
                    .obj_for_fd(ctx.pid, req.args[0])
                    .ok_or(SyscallError::InvalidArgument)?;
                // close newfd first if open
                let _ = state.close_fd(ctx.pid, target);
                let slot = usize::try_from(target).map_err(|_| SyscallError::InvalidArgument)?;
                let proc = state.proc_mut(ctx.pid);
                if proc.slots.len() <= slot {
                    proc.slots.resize(slot + 1, None);
                }
                proc.slots[slot] = Some(obj_idx);
                let obj = state
                    .objects
                    .get_mut(obj_idx)
                    .and_then(|o| o.as_mut())
                    .ok_or(SyscallError::InvalidArgument)?;
                obj.refs = obj.refs.saturating_add(1);
                Ok::<u64, SyscallError>(target)
            })?;
            Ok(out)
        }
        SyscallNumber::Pipe => {
            // Returns packed pair: low32=read_fd high32=write_fd.
            let packed = with_state_mut(|state| {
                let pipe_id = if let Some((idx, slot)) = state
                    .pipes
                    .iter_mut()
                    .enumerate()
                    .find(|(_, p)| p.is_none())
                {
                    *slot = Some(PipeBuffer {
                        data: Vec::new(),
                        read_pos: 0,
                    });
                    idx
                } else {
                    state.pipes.push(Some(PipeBuffer {
                        data: Vec::new(),
                        read_pos: 0,
                    }));
                    state.pipes.len() - 1
                };

                let read_obj = state.alloc_object(DescriptorKind::PipeRead(pipe_id));
                let write_obj = state.alloc_object(DescriptorKind::PipeWrite(pipe_id));
                let rfd = state.alloc_fd(ctx.pid, read_obj);
                let wfd = state.alloc_fd(ctx.pid, write_obj);
                Ok::<u64, SyscallError>((wfd << 32) | (rfd & 0xFFFF_FFFF))
            })?;
            Ok(packed)
        }
        SyscallNumber::Ioctl => {
            // args[0]=fd args[1]=request args[2]=value
            let fd = req.args[0];
            let request = req.args[1];
            let value = req.args[2];

            if fd <= 2 {
                return match request {
                    TCGETS => Ok(0),
                    TIOCGPGRP => Ok(process::foreground_process_group().unwrap_or(ctx.pid)),
                    TIOCSPGRP => {
                        process::set_foreground_process_group(value)
                            .map_err(|_| SyscallError::InvalidArgument)?;
                        Ok(0)
                    }
                    TIOCGWINSZ => Ok((24u64 << 16) | 80u64),
                    FIONREAD => Ok(0),
                    FIONBIO => Ok(0),
                    _ => Err(SyscallError::NotTty),
                };
            }

            with_state_mut(|state| {
                let obj_idx = state
                    .obj_for_fd(ctx.pid, fd)
                    .ok_or(SyscallError::InvalidArgument)?;
                let obj = state
                    .objects
                    .get_mut(obj_idx)
                    .and_then(|o| o.as_mut())
                    .ok_or(SyscallError::InvalidArgument)?;

                match request {
                    FIONBIO => {
                        obj.nonblocking = value != 0;
                        Ok(0)
                    }
                    FIONREAD => match obj.kind {
                        DescriptorKind::PipeRead(pipe_id) => {
                            let pipe = state
                                .pipes
                                .get(pipe_id)
                                .and_then(|p| p.as_ref())
                                .ok_or(SyscallError::InvalidArgument)?;
                            Ok(pipe.data.len().saturating_sub(pipe.read_pos) as u64)
                        }
                        _ => Ok(0),
                    },
                    TIOCGPGRP => Ok(process::foreground_process_group().unwrap_or(ctx.pid)),
                    TIOCSPGRP => {
                        process::set_foreground_process_group(value)
                            .map_err(|_| SyscallError::InvalidArgument)?;
                        Ok(0)
                    }
                    TCGETS | TIOCGWINSZ => Err(SyscallError::NotTty),
                    _ => Err(SyscallError::NotTty),
                }
            })
        }
        SyscallNumber::Poll => {
            // args[0]=fd args[1]=events args[2]=timeout_ms
            let fd = req.args[0];
            let events = req.args[1];
            let timeout_ms = req.args[2];
            let mut revents = with_state_mut(|state| poll_fd_mask(state, ctx.pid, fd, events))?;
            if revents == 0 && timeout_ms > 0 {
                timer::sleep(timeout_ms);
                revents = with_state_mut(|state| poll_fd_mask(state, ctx.pid, fd, events))?;
            }
            Ok(revents)
        }
        SyscallNumber::Select => {
            // args[0]=read fd, args[1]=write fd, args[2]=except fd, args[3]=timeout_ms.
            let r = SyscallRequest {
                number: SyscallNumber::Poll,
                args: [req.args[0], POLLIN, req.args[3], 0, 0, 0],
            };
            let w = SyscallRequest {
                number: SyscallNumber::Poll,
                args: [req.args[1], POLLOUT, req.args[3], 0, 0, 0],
            };
            let e = SyscallRequest {
                number: SyscallNumber::Poll,
                args: [req.args[2], POLLERR, req.args[3], 0, 0, 0],
            };
            let rdy_r = dispatch(r, ctx)?;
            let rdy_w = dispatch(w, ctx)?;
            let rdy_e = dispatch(e, ctx)?;
            Ok((if rdy_r != 0 { 1 } else { 0 })
                | (if rdy_w != 0 { 2 } else { 0 })
                | (if rdy_e != 0 { 4 } else { 0 }))
        }
        SyscallNumber::Clone => {
            // args[0]=clone flags (low byte is child-exit signal)
            let flags = req.args[0];
            let exit_signal = flags & 0xFF;
            if exit_signal != 0 && exit_signal != SIGCHLD {
                return Err(SyscallError::InvalidArgument);
            }
            if (flags & UNSUPPORTED_CLONE_FLAGS) != 0 {
                return Err(SyscallError::Unimplemented);
            }
            let child =
                process::fork_from(ctx.pid, flags).map_err(|_| SyscallError::InvalidArgument)?;
            if (flags & CLONE_THREAD) == 0 {
                let _ = process::set_process_group(child, child);
            }
            Ok(child)
        }
        SyscallNumber::Fork => {
            let child =
                process::fork_from(ctx.pid, SIGCHLD).map_err(|_| SyscallError::InvalidArgument)?;
            Ok(child)
        }
        SyscallNumber::Kill => {
            // args[0]=pid args[1]=signal
            let target = req.args[0] as i64;
            let signo = req.args[1];
            if signo > 64 {
                return Err(SyscallError::InvalidArgument);
            }

            if signo == 0 {
                if target == 0 {
                    let pgid =
                        process::process_group(ctx.pid).ok_or(SyscallError::InvalidArgument)?;
                    return if process::jobs().into_iter().any(|r| r.process_group == pgid) {
                        Ok(0)
                    } else {
                        Err(SyscallError::InvalidArgument)
                    };
                }
                return if process::record(target as u64).is_some() {
                    Ok(0)
                } else {
                    Err(SyscallError::InvalidArgument)
                };
            }

            if target > 0 {
                process::send_signal(target as u64, signo as u8)
                    .map_err(|_| SyscallError::InvalidArgument)?;
                return Ok(0);
            }

            if target == 0 {
                let pgid = process::process_group(ctx.pid).ok_or(SyscallError::InvalidArgument)?;
                let _ = process::signal_process_group(pgid, signo as u8)
                    .map_err(|_| SyscallError::InvalidArgument)?;
                return Ok(0);
            }

            if target == -1 {
                return Err(SyscallError::Unimplemented);
            }

            let pgid = (-target) as u64;
            let _ = process::signal_process_group(pgid, signo as u8)
                .map_err(|_| SyscallError::InvalidArgument)?;
            Ok(0)
        }
    }
}
