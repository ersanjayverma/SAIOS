//! System call dispatcher.
//!
//! Translates integer syscall requests from user-space into kernel operations
//! on the VFS, process manager and timer. Unsupported or unimplemented
//! syscalls return negative error codes compatible with POSIX errno values.

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::mem::size_of;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::console;
use crate::kernel::process;
use crate::pmm;
use crate::timer;
use crate::vfs;
use crate::vmm;

/// Map zeroed anonymous user pages at `start`. Used by brk/mmap growth.
fn map_user_anon_pages(start: u64, pages: usize, owner: &str) -> Result<(), &'static str> {
    if pages == 0 {
        return Ok(());
    }
    let phys = pmm::alloc_pages(pages).ok_or("syscall: no physical memory")?;
    if let Err(e) = vmm::map_owned(
        start,
        phys,
        pages,
        vmm::FLAG_USER | vmm::FLAG_READ | vmm::FLAG_WRITE,
        owner,
    ) {
        let _ = pmm::free_pages_range(phys, pages);
        return Err(e);
    }
    unsafe {
        core::ptr::write_bytes(start as *mut u8, 0, pages * vmm::PAGE_SIZE as usize);
    }
    Ok(())
}

/// Kernel syscall ABI version.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

const ABI_VERSION: AbiVersion = AbiVersion {
    major: 1,
    minor: 3,
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

/// `struct winsize { ws_row, ws_col, ws_xpixel, ws_ypixel }` (all `u16`), the
/// Linux ABI layout expected by `TIOCGWINSZ`.
#[repr(C)]
#[derive(Copy, Clone)]
struct WinSize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}
const WINSIZE_DEFAULT: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
    ws_xpixel: 0,
    ws_ypixel: 0,
};

const SIGCHLD: u64 = 17;

#[allow(dead_code)] const CLONE_VM:       u64 = 0x0000_0100;
#[allow(dead_code)] const CLONE_FS:       u64 = 0x0000_0200;
#[allow(dead_code)] const CLONE_FILES:    u64 = 0x0000_0400;
#[allow(dead_code)] const CLONE_SIGHAND:  u64 = 0x0000_0800;
const CLONE_THREAD:         u64 = 0x0001_0000;
const CLONE_SETTLS:         u64 = 0x0008_0000;
const CLONE_PARENT_SETTID:  u64 = 0x0010_0000;
const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;

// Kept for documentation; individual flags above are tested directly now.
#[allow(dead_code)]
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

const LINUX_ENOSYS: i64 = -38;
const LINUX_EBADF: i64 = -9;
const LINUX_EEXIST: i64 = -17;
const LINUX_ENOTDIR: i64 = -20;
const LINUX_EISDIR: i64 = -21;
const LINUX_EINVAL: i64 = -22;
const LINUX_EFAULT: i64 = -14;
const LINUX_ENOENT: i64 = -2;
const LINUX_ENOEXEC: i64 = -8;

const O_ACCMODE: u64 = 0o3;
const O_WRONLY: u64 = 0o1;
const O_RDWR: u64 = 0o2;
const O_CREAT: u64 = 0o100;
const O_TRUNC: u64 = 0o1000;
const O_APPEND: u64 = 0o2000;
const O_NONBLOCK: u64 = 0o4000;

const AT_FDCWD: i64 = -100;
const AT_EMPTY_PATH: u64 = 0x1000;
const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;

const F_DUPFD: u64 = 0;
const F_GETFD: u64 = 1;
const F_SETFD: u64 = 2;
const F_GETFL: u64 = 3;
const F_SETFL: u64 = 4;
const F_DUPFD_CLOEXEC: u64 = 1030;

const ARCH_SET_FS: u64 = 0x1002;
const ARCH_GET_FS: u64 = 0x1003;
const IA32_FS_BASE: u32 = 0xC000_0100;

const SEEK_SET: u64 = 0;
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;

#[repr(C)]
#[derive(Copy, Clone)]
struct LinuxIovec {
    base: u64,
    len: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct LinuxPollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct LinuxTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct LinuxTimeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct LinuxTms {
    tms_utime: i64,
    tms_stime: i64,
    tms_cutime: i64,
    tms_cstime: i64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct LinuxRlimit {
    rlim_cur: u64,
    rlim_max: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct LinuxUtsname {
    sysname: [u8; 65],
    nodename: [u8; 65],
    release: [u8; 65],
    version: [u8; 65],
    machine: [u8; 65],
    domainname: [u8; 65],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct LinuxStat {
    st_dev: u64,
    st_ino: u64,
    st_nlink: u64,
    st_mode: u32,
    st_uid: u32,
    st_gid: u32,
    __pad0: u32,
    st_rdev: u64,
    st_size: i64,
    st_blksize: i64,
    st_blocks: i64,
    st_atime: i64,
    st_atime_nsec: i64,
    st_mtime: i64,
    st_mtime_nsec: i64,
    st_ctime: i64,
    st_ctime_nsec: i64,
    __reserved: [i64; 3],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct LinuxSysinfo {
    uptime: i64,
    loads: [u64; 3],
    totalram: u64,
    freeram: u64,
    sharedram: u64,
    bufferram: u64,
    totalswap: u64,
    freeswap: u64,
    procs: u16,
    pad: u16,
    totalhigh: u64,
    freehigh: u64,
    mem_unit: u32,
    _f: [u8; 0],
}

fn linux_errno(err: SyscallError) -> i64 {
    match err {
        SyscallError::Unimplemented | SyscallError::InvalidNumber => LINUX_ENOSYS,
        _ => err.code(),
    }
}

static UMASK: AtomicU32 = AtomicU32::new(0o022);

fn linux_ctx(pid: u64) -> SyscallContext {
    SyscallContext { pid }
}

fn active_linux_pid() -> Result<u64, i64> {
    crate::kernel::fault::active_exec_pid().ok_or(LINUX_EINVAL)
}

fn decode_open_options(flags: u64) -> vfs::OpenOptions {
    let access = flags & O_ACCMODE;
    let read = access != O_WRONLY;
    let write = access == O_WRONLY || access == O_RDWR;
    vfs::OpenOptions {
        read,
        write,
        create: (flags & O_CREAT) != 0,
        truncate: (flags & O_TRUNC) != 0,
        append: (flags & O_APPEND) != 0,
    }
}

unsafe fn user_slice<'a>(ptr: u64, len: usize) -> Result<&'a [u8], i64> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr == 0 {
        return Err(LINUX_EFAULT);
    }
    Ok(unsafe { core::slice::from_raw_parts(ptr as *const u8, len) })
}

unsafe fn user_slice_mut<'a>(ptr: u64, len: usize) -> Result<&'a mut [u8], i64> {
    if len == 0 {
        return Ok(unsafe {
            core::slice::from_raw_parts_mut(core::ptr::NonNull::<u8>::dangling().as_ptr(), 0)
        });
    }
    if ptr == 0 {
        return Err(LINUX_EFAULT);
    }
    Ok(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len) })
}

unsafe fn user_ref<'a, T>(ptr: u64) -> Result<&'a T, i64> {
    if ptr == 0 {
        return Err(LINUX_EFAULT);
    }
    Ok(unsafe { &*(ptr as *const T) })
}

fn read_user_cstr(ptr: u64, max_len: usize) -> Result<String, i64> {
    if ptr == 0 {
        return Err(LINUX_EFAULT);
    }
    let mut bytes = Vec::new();
    for idx in 0..max_len {
        let byte = unsafe { *((ptr + idx as u64) as *const u8) };
        if byte == 0 {
            return String::from_utf8(bytes).map_err(|_| LINUX_EINVAL);
        }
        bytes.push(byte);
    }
    Err(LINUX_EINVAL)
}

fn read_user_argv(argv_ptr: u64, max_args: usize) -> Result<Vec<String>, i64> {
    if argv_ptr == 0 {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for idx in 0..max_args {
        let arg_ptr = unsafe { *((argv_ptr as *const u64).add(idx)) };
        if arg_ptr == 0 {
            break;
        }
        out.push(read_user_cstr(arg_ptr, 4096)?);
    }
    Ok(out)
}

fn map_exec_error_to_linux(err: &'static str) -> i64 {
    if err == "exec: program not found" {
        LINUX_ENOENT
    } else if err.contains("PT_INTERP") || err.contains("unsupported ELF") {
        LINUX_ENOEXEC
    } else {
        LINUX_EINVAL
    }
}

pub fn linux_execve_for_pid(pid: u64, path: &str, argv: &[&str]) -> Result<i32, i64> {
    process::exec_in_place(pid, path, argv, &[]).map_err(map_exec_error_to_linux)
}

fn write_user_bytes(ptr: u64, data: &[u8]) -> Result<(), i64> {
    let dst = unsafe { user_slice_mut(ptr, data.len())? };
    dst.copy_from_slice(data);
    Ok(())
}

fn write_user_struct<T: Copy>(ptr: u64, value: &T) -> Result<(), i64> {
    let bytes = unsafe {
        core::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>())
    };
    write_user_bytes(ptr, bytes)
}

fn read_user_struct<T: Copy>(ptr: u64) -> Result<T, i64> {
    let src = unsafe { user_slice(ptr, size_of::<T>())? };
    let mut out = core::mem::MaybeUninit::<T>::uninit();
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), out.as_mut_ptr().cast::<u8>(), size_of::<T>());
        Ok(out.assume_init())
    }
}

fn zero_user_bytes(ptr: u64, len: usize) -> Result<(), i64> {
    let dst = unsafe { user_slice_mut(ptr, len)? };
    dst.fill(0);
    Ok(())
}

fn linux_stat_mode(kind: vfs::FileType) -> u32 {
    match kind {
        vfs::FileType::Directory => 0o040755,
        // No per-file executable bit is tracked yet, so every regular file
        // reports as executable (0o100755, not 0o100644). Without this, ash
        // (and any other exec-search that pre-checks executability via
        // stat/access before calling execve, per standard shell PATH-search
        // convention) sees every candidate -- including a perfectly good
        // `/bin/busybox` -- as non-executable and refuses to even attempt
        // running it, silently, without ever making an execve syscall.
        // That's a strictly worse default than always-executable at this
        // stage: SAIOS has no real permission model to violate yet, and the
        // failure mode for a genuinely non-executable file (a text file,
        // say) is just a normal ENOEXEC from the real exec attempt instead
        // of a client-side pre-check -- not a new hazard.
        vfs::FileType::File => 0o100755,
    }
}

fn linux_stat_from_vfs(st: &vfs::FileStat) -> LinuxStat {
    let size = st.size as i64;
    let blocks = ((st.size as u64).saturating_add(511) / 512) as i64;
    LinuxStat {
        st_dev: 1,
        st_ino: 1,
        st_nlink: 1,
        st_mode: linux_stat_mode(st.kind),
        st_uid: 0,
        st_gid: 0,
        __pad0: 0,
        st_rdev: 0,
        st_size: size,
        st_blksize: 4096,
        st_blocks: blocks,
        st_atime: 0,
        st_atime_nsec: 0,
        st_mtime: 0,
        st_mtime_nsec: 0,
        st_ctime: 0,
        st_ctime_nsec: 0,
        __reserved: [0; 3],
    }
}

fn dispatch_custom(number: SyscallNumber, args: [u64; 6], pid: u64) -> Result<u64, i64> {
    dispatch(SyscallRequest { number, args }, linux_ctx(pid)).map_err(linux_errno)
}

fn dispatch_wait4(pid: u64, target: u64, options: u64, status_ptr: u64) -> Result<u64, i64> {
    let packed = dispatch_custom(
        SyscallNumber::WaitPid,
        [target, options, 0, 0, 0, 0],
        pid,
    )?;
    let waited_pid = packed >> 32;
    let status = (packed & 0xFFFF_FFFF) as i32;
    if status_ptr != 0 {
        write_user_struct(status_ptr, &status)?;
    }
    Ok(waited_pid)
}

fn join_dir_path(base: &str, child: &str) -> String {
    if child.starts_with('/') {
        return child.to_string();
    }
    if base == "/" {
        let mut out = String::from("/");
        out.push_str(child);
        return out;
    }
    let mut out = base.to_string();
    if !out.ends_with('/') {
        out.push('/');
    }
    out.push_str(child);
    out
}

fn descriptor_path(state: &SyscallState, pid: u64, fd: u64) -> Result<String, SyscallError> {
    let obj_idx = state.obj_for_fd(pid, fd).ok_or(SyscallError::InvalidArgument)?;
    let obj = state
        .objects
        .get(obj_idx)
        .and_then(|o| o.as_ref())
        .ok_or(SyscallError::InvalidArgument)?;
    match &obj.kind {
        DescriptorKind::Vfs { path, .. } | DescriptorKind::Directory(path) => Ok(path.clone()),
        DescriptorKind::Tty => Ok("/dev/tty".to_string()),
        DescriptorKind::PipeRead(_) | DescriptorKind::PipeWrite(_) => {
            Err(SyscallError::InvalidArgument)
        }
    }
}

fn resolve_at_path(pid: u64, dirfd: u64, path_ptr: u64) -> Result<String, i64> {
    let path = read_user_cstr(path_ptr, 4096)?;
    resolve_at_path_str(pid, dirfd, path.as_str())
}

fn resolve_at_path_str(pid: u64, dirfd: u64, path: &str) -> Result<String, i64> {
    if path.starts_with('/') {
        return Ok(path.to_string());
    }
    let dirfd_i64 = dirfd as i64;
    if dirfd_i64 == AT_FDCWD {
        return Ok(join_dir_path(vfs::pwd().as_str(), path));
    }
    let base = with_state_mut(|state| descriptor_path(state, pid, dirfd)).map_err(linux_errno)?;
    match vfs::stat(base.as_str()) {
        Ok(stat) if stat.kind == vfs::FileType::Directory => Ok(join_dir_path(base.as_str(), path)),
        Ok(_) => Err(LINUX_ENOTDIR),
        Err(_) => Err(LINUX_EBADF),
    }
}

fn linux_open_path(pid: u64, path: &str, flags: u64) -> Result<u64, i64> {
    if path == "/dev/tty" || path == "/dev/console" {
        let fd = with_state_mut(|state| {
            let obj = state.alloc_object(DescriptorKind::Tty);
            state.alloc_fd(pid, obj)
        });
        return Ok(fd);
    }

    let opts = decode_open_options(flags);
    if let Ok(stat) = vfs::stat(path)
        && stat.kind == vfs::FileType::Directory
    {
        if opts.write || opts.create || opts.truncate || opts.append {
            return Err(LINUX_EISDIR);
        }
        let fd = with_state_mut(|state| {
            let obj = state.alloc_object(DescriptorKind::Directory(path.to_string()));
            state.alloc_fd(pid, obj)
        });
        return Ok(fd);
    }

    let vfd = vfs::open(path, opts).map_err(|_| LINUX_ENOENT)?;
    let fd = with_state_mut(|state| {
        let obj = state.alloc_object(DescriptorKind::Vfs {
            fd: vfd,
            path: path.to_string(),
        });
        state.alloc_fd(pid, obj)
    });
    Ok(fd)
}

fn linux_open(pid: u64, path_ptr: u64, flags: u64) -> Result<u64, i64> {
    let path = read_user_cstr(path_ptr, 4096)?;
    linux_open_path(pid, path.as_str(), flags)
}

fn linux_close(pid: u64, fd: u64) -> Result<u64, i64> {
    dispatch_custom(SyscallNumber::Close, [fd, 0, 0, 0, 0, 0], pid)
}

/// Bytes from the most recently completed console input line that haven't
/// been handed to a `read(0, ...)` caller yet. See `linux_read`'s fd==0 arm.
static STDIN_PENDING: StaticCell<Vec<u8>> = StaticCell::new(Vec::new());

fn linux_read(pid: u64, fd: u64, buf: u64, len: u64) -> Result<u64, i64> {
    let max_len = len as usize;
    match fd {
        0 => {
            // Read from the interactive console, canonical-tty style: a
            // whole line is captured at once (editing/echo already handled
            // by `console::poll_input` itself), but handed out to the
            // caller in whatever chunk size *it* asks for. ash's own line
            // editor reads stdin a single byte at a time, so a completed
            // line has to survive across many separate `read()` calls --
            // stashed here in `STDIN_PENDING` -- rather than being
            // truncated to the first `max_len` bytes and discarding the
            // rest (which made every read after the first return only its
            // first byte, e.g. "e" of "echo ...", with the remainder of
            // the line silently lost).
            //
            // This used to unconditionally return `Ok(0)` (immediate EOF)
            // -- a stub that made every ring3 shell (ash included) see EOF
            // on its very first read and exit right after printing its
            // prompt, since a shell reading EOF on stdin is completely
            // correct end-of-input behavior. It never had a real chance to
            // be interactive.
            //
            // `console::poll_input` polls PS/2/USB/serial hardware
            // directly (not interrupt-driven), so busy-waiting on it here
            // is safe even though IF is 0 for the whole duration of syscall
            // handling (see `USER_ENTRY_ENABLE_INTERRUPTS` in
            // hal::arch::x86_64::constants) -- it never depends on an IRQ
            // actually being delivered.
            let pending = unsafe { &mut *STDIN_PENDING.get() };
            if pending.is_empty() {
                let mut line = loop {
                    if let Some(line) = console::poll_input() {
                        break line;
                    }
                    core::hint::spin_loop();
                };
                let _ = line.push('\n');
                pending.extend_from_slice(line.as_bytes());
            }
            let take = pending.len().min(max_len);
            write_user_bytes(buf, &pending[..take])?;
            pending.drain(..take);
            Ok(take as u64)
        }
        1 | 2 => Err(LINUX_EBADF),
        _ => {
            let data = with_state_mut(|state| {
                let obj_idx = state.obj_for_fd(pid, fd).ok_or(LINUX_EBADF)?;
                let obj = state
                    .objects
                    .get(obj_idx)
                    .and_then(|o| o.as_ref())
                    .ok_or(LINUX_EBADF)?;
                match obj.kind {
                    DescriptorKind::Vfs { fd: vfd, .. } => {
                        vfs::read(vfd, max_len).map_err(|_| LINUX_EINVAL)
                    }
                    DescriptorKind::Directory(_) => Err(LINUX_EISDIR),
                    DescriptorKind::Tty => Err(LINUX_EBADF),
                    DescriptorKind::PipeRead(pipe_id) => {
                        let pipe = state
                            .pipes
                            .get_mut(pipe_id)
                            .and_then(|p| p.as_mut())
                            .ok_or(LINUX_EBADF)?;
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
                    DescriptorKind::PipeWrite(_) => Err(LINUX_EBADF),
                }
            })?;
            write_user_bytes(buf, data.as_slice())?;
            Ok(data.len() as u64)
        }
    }
}

fn linux_write(pid: u64, fd: u64, buf: u64, len: u64) -> Result<u64, i64> {
    let data = unsafe { user_slice(buf, len as usize)? };
    match fd {
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
                let obj_idx = state.obj_for_fd(pid, fd).ok_or(SyscallError::InvalidArgument)?;
                let obj = state
                    .objects
                    .get(obj_idx)
                    .and_then(|o| o.as_ref())
                    .ok_or(SyscallError::InvalidArgument)?;
                match obj.kind {
                    DescriptorKind::Vfs { fd: vfd, .. } => {
                        vfs::write(vfd, data).map_err(|_| SyscallError::InvalidArgument)
                    }
                    DescriptorKind::Directory(_) => Err(SyscallError::InvalidArgument),
                    DescriptorKind::Tty => {
                        let text = core::str::from_utf8(data).unwrap_or("<binary>");
                        console::print(text);
                        Ok(data.len())
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
            })
            .map_err(linux_errno)?;
            Ok(written as u64)
        }
    }
}

fn linux_pread64(pid: u64, fd: u64, buf: u64, len: u64, off: u64) -> Result<u64, i64> {
    let saved = with_state_mut(|state| {
        let vfd = resolve_vfs_fd(state, pid, fd)?;
        vfs::seek(vfd, vfs::SeekFrom::Current(0)).map_err(|_| SyscallError::InvalidArgument)
    })
    .map_err(linux_errno)?;
    let _ = dispatch_custom(SyscallNumber::Lseek, [fd, off, SEEK_SET, 0, 0, 0], pid)?;
    let out = linux_read(pid, fd, buf, len);
    let _ = dispatch_custom(SyscallNumber::Lseek, [fd, saved as u64, SEEK_SET, 0, 0, 0], pid);
    out
}

fn linux_pwrite64(pid: u64, fd: u64, buf: u64, len: u64, off: u64) -> Result<u64, i64> {
    let saved = with_state_mut(|state| {
        let vfd = resolve_vfs_fd(state, pid, fd)?;
        vfs::seek(vfd, vfs::SeekFrom::Current(0)).map_err(|_| SyscallError::InvalidArgument)
    })
    .map_err(linux_errno)?;
    let _ = dispatch_custom(SyscallNumber::Lseek, [fd, off, SEEK_SET, 0, 0, 0], pid)?;
    let out = linux_write(pid, fd, buf, len);
    let _ = dispatch_custom(SyscallNumber::Lseek, [fd, saved as u64, SEEK_SET, 0, 0, 0], pid);
    out
}

fn linux_writev(pid: u64, fd: u64, iov_ptr: u64, iovcnt: u64) -> Result<u64, i64> {
    let mut total = 0u64;
    for idx in 0..iovcnt as usize {
        let iov = unsafe { *user_ref::<LinuxIovec>(iov_ptr + (idx * size_of::<LinuxIovec>()) as u64)? };
        total = total.saturating_add(linux_write(pid, fd, iov.base, iov.len)?);
    }
    Ok(total)
}

fn linux_readv(pid: u64, fd: u64, iov_ptr: u64, iovcnt: u64) -> Result<u64, i64> {
    let mut total = 0u64;
    for idx in 0..iovcnt as usize {
        let iov = unsafe { *user_ref::<LinuxIovec>(iov_ptr + (idx * size_of::<LinuxIovec>()) as u64)? };
        let got = linux_read(pid, fd, iov.base, iov.len)?;
        total = total.saturating_add(got);
        if got < iov.len {
            break;
        }
    }
    Ok(total)
}

fn linux_pipe(pid: u64, pipefd_ptr: u64) -> Result<u64, i64> {
    let packed = dispatch_custom(SyscallNumber::Pipe, [0, 0, 0, 0, 0, 0], pid)?;
    let pair = [(packed & 0xFFFF_FFFF) as i32, (packed >> 32) as i32];
    write_user_struct(pipefd_ptr, &pair)?;
    Ok(0)
}

fn linux_select(
    pid: u64,
    nfds: u64,
    readfds_ptr: u64,
    writefds_ptr: u64,
    exceptfds_ptr: u64,
    timeout_ptr: u64,
) -> Result<u64, i64> {
    fn read_fdset(ptr: u64, nfds: u64) -> Result<Vec<u64>, i64> {
        let words = (nfds as usize).saturating_add(63) / 64;
        let mut out = vec![0u64; words];
        if ptr == 0 || words == 0 {
            return Ok(out);
        }
        let bytes = unsafe { user_slice(ptr, words * size_of::<u64>())? };
        for (idx, chunk) in bytes.chunks_exact(size_of::<u64>()).enumerate() {
            let mut word = [0u8; size_of::<u64>()];
            word.copy_from_slice(chunk);
            out[idx] = u64::from_le_bytes(word);
        }
        Ok(out)
    }

    fn write_fdset(ptr: u64, words: &[u64]) -> Result<(), i64> {
        if ptr == 0 || words.is_empty() {
            return Ok(());
        }
        let mut bytes = Vec::with_capacity(words.len() * size_of::<u64>());
        for word in words {
            bytes.extend_from_slice(word.to_le_bytes().as_slice());
        }
        write_user_bytes(ptr, bytes.as_slice())
    }

    fn evaluate(
        pid: u64,
        read_words: &[u64],
        write_words: &[u64],
        except_words: &[u64],
        nfds: u64,
    ) -> Result<(Vec<u64>, Vec<u64>, Vec<u64>, u64), i64> {
        let mut read_out = vec![0u64; read_words.len()];
        let mut write_out = vec![0u64; write_words.len()];
        let mut except_out = vec![0u64; except_words.len()];
        let mut ready = 0u64;

        for fd in 0..nfds {
            let idx = (fd / 64) as usize;
            let bit = 1u64 << (fd % 64);
            let mut counted = false;

            if idx < read_words.len() && (read_words[idx] & bit) != 0 {
                let mask = with_state_mut(|state| poll_fd_mask(state, pid, fd, POLLIN))
                    .map_err(linux_errno)?;
                if (mask & POLLIN) != 0 {
                    read_out[idx] |= bit;
                    counted = true;
                }
            }

            if idx < write_words.len() && (write_words[idx] & bit) != 0 {
                let mask = with_state_mut(|state| poll_fd_mask(state, pid, fd, POLLOUT))
                    .map_err(linux_errno)?;
                if (mask & POLLOUT) != 0 {
                    write_out[idx] |= bit;
                    counted = true;
                }
            }

            if idx < except_words.len() && (except_words[idx] & bit) != 0 {
                let mask = with_state_mut(|state| poll_fd_mask(state, pid, fd, POLLERR))
                    .map_err(linux_errno)?;
                if (mask & (POLLERR | POLLHUP | POLLNVAL)) != 0 {
                    except_out[idx] |= bit;
                    counted = true;
                }
            }

            if counted {
                ready = ready.saturating_add(1);
            }
        }

        Ok((read_out, write_out, except_out, ready))
    }

    let timeout_ms = if timeout_ptr == 0 {
        0
    } else {
        let tv = unsafe { *user_ref::<LinuxTimeval>(timeout_ptr)? };
        (tv.tv_sec.max(0) as u64)
            .saturating_mul(1000)
            .saturating_add((tv.tv_usec.max(0) as u64) / 1000)
    };
    let read_words = read_fdset(readfds_ptr, nfds)?;
    let write_words = read_fdset(writefds_ptr, nfds)?;
    let except_words = read_fdset(exceptfds_ptr, nfds)?;
    let (mut read_out, mut write_out, mut except_out, mut ready) =
        evaluate(pid, &read_words, &write_words, &except_words, nfds)?;
    if ready == 0 && timeout_ms > 0 {
        timer::sleep(timeout_ms);
        (read_out, write_out, except_out, ready) =
            evaluate(pid, &read_words, &write_words, &except_words, nfds)?;
    }
    write_fdset(readfds_ptr, read_out.as_slice())?;
    write_fdset(writefds_ptr, write_out.as_slice())?;
    write_fdset(exceptfds_ptr, except_out.as_slice())?;
    Ok(ready)
}

fn linux_truncate_path(path: &str, len: usize) -> Result<u64, i64> {
    let stat = vfs::stat(path).map_err(|_| LINUX_ENOENT)?;
    if stat.kind != vfs::FileType::File {
        return Err(LINUX_EISDIR);
    }
    let mut bytes = vfs::read_path(path).map_err(|_| LINUX_EINVAL)?;
    bytes.resize(len, 0);
    vfs::write_path(path, bytes.as_slice()).map_err(|_| LINUX_EINVAL)?;
    Ok(0)
}

fn linux_truncate(path_ptr: u64, len: u64) -> Result<u64, i64> {
    let path = read_user_cstr(path_ptr, 4096)?;
    linux_truncate_path(path.as_str(), len as usize)
}

fn linux_ftruncate(pid: u64, fd: u64, len: u64) -> Result<u64, i64> {
    let path = with_state_mut(|state| descriptor_path(state, pid, fd)).map_err(linux_errno)?;
    linux_truncate_path(path.as_str(), len as usize)
}

fn linux_fchdir(pid: u64, fd: u64) -> Result<u64, i64> {
    let path = with_state_mut(|state| descriptor_path(state, pid, fd)).map_err(linux_errno)?;
    let stat = vfs::stat(path.as_str()).map_err(|_| LINUX_EBADF)?;
    if stat.kind != vfs::FileType::Directory {
        return Err(LINUX_ENOTDIR);
    }
    vfs::cd(path.as_str()).map(|_| 0).map_err(|_| LINUX_EINVAL)
}

fn linux_link_paths(old_path: &str, new_path: &str) -> Result<u64, i64> {
    if vfs::stat(new_path).is_ok() {
        return Err(LINUX_EEXIST);
    }
    let stat = vfs::stat(old_path).map_err(|_| LINUX_ENOENT)?;
    if stat.kind != vfs::FileType::File {
        return Err(LINUX_EISDIR);
    }
    let bytes = vfs::read_path(old_path).map_err(|_| LINUX_EINVAL)?;
    vfs::write_path(new_path, bytes.as_slice()).map_err(|_| LINUX_EINVAL)?;
    Ok(0)
}

fn linux_link(old_ptr: u64, new_ptr: u64) -> Result<u64, i64> {
    let old_path = read_user_cstr(old_ptr, 4096)?;
    let new_path = read_user_cstr(new_ptr, 4096)?;
    linux_link_paths(old_path.as_str(), new_path.as_str())
}

fn linux_getdents64(pid: u64, fd: u64, dirp: u64, count: u64) -> Result<u64, i64> {
    let (path, cursor) = with_state_mut(|state| {
        let obj_idx = state.obj_for_fd(pid, fd).ok_or(SyscallError::InvalidArgument)?;
        let obj = state
            .objects
            .get(obj_idx)
            .and_then(|o| o.as_ref())
            .ok_or(SyscallError::InvalidArgument)?;
        match &obj.kind {
            DescriptorKind::Directory(path) => Ok::<(String, usize), SyscallError>((path.clone(), obj.cursor)),
            _ => Err(SyscallError::InvalidArgument),
        }
    })
    .map_err(linux_errno)?;

    let entries = vfs::readdir(path.as_str()).map_err(|_| LINUX_ENOTDIR)?;
    let max_bytes = count as usize;
    let mut out = Vec::new();
    let mut consumed = 0usize;

    for (idx, name) in entries.iter().enumerate().skip(cursor) {
        let child_path = join_dir_path(path.as_str(), name.as_str());
        let dtype = match vfs::stat(child_path.as_str()) {
            Ok(stat) if stat.kind == vfs::FileType::Directory => DT_DIR,
            _ => DT_REG,
        };
        let reclen = ((19 + name.len() + 1) + 7) & !7;
        if reclen > max_bytes {
            return Err(LINUX_EINVAL);
        }
        if out.len().saturating_add(reclen) > max_bytes {
            break;
        }
        out.extend_from_slice(((idx + 1) as u64).to_le_bytes().as_slice());
        out.extend_from_slice(((idx + 1) as i64).to_le_bytes().as_slice());
        out.extend_from_slice((reclen as u16).to_le_bytes().as_slice());
        out.push(dtype);
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        while out.len() % 8 != 0 {
            out.push(0);
        }
        consumed = consumed.saturating_add(1);
    }

    write_user_bytes(dirp, out.as_slice())?;
    with_state_mut(|state| {
        let obj_idx = state.obj_for_fd(pid, fd).ok_or(SyscallError::InvalidArgument)?;
        let obj = state
            .objects
            .get_mut(obj_idx)
            .and_then(|o| o.as_mut())
            .ok_or(SyscallError::InvalidArgument)?;
        obj.cursor = obj.cursor.saturating_add(consumed);
        Ok::<(), SyscallError>(())
    })
    .map_err(linux_errno)?;
    Ok(out.len() as u64)
}

fn linux_openat(pid: u64, dirfd: u64, path_ptr: u64, flags: u64) -> Result<u64, i64> {
    let path = resolve_at_path(pid, dirfd, path_ptr)?;
    linux_open_path(pid, path.as_str(), flags)
}

fn linux_mkdirat(pid: u64, dirfd: u64, path_ptr: u64) -> Result<u64, i64> {
    let path = resolve_at_path(pid, dirfd, path_ptr)?;
    vfs::mkdir(path.as_str()).map(|_| 0).map_err(|_| LINUX_EINVAL)
}

fn linux_mknodat(pid: u64, dirfd: u64, path_ptr: u64, mode: u64) -> Result<u64, i64> {
    let path = resolve_at_path(pid, dirfd, path_ptr)?;
    let file_type = (mode as u32) & S_IFMT;
    if file_type == S_IFDIR {
        return vfs::mkdir(path.as_str()).map(|_| 0).map_err(|_| LINUX_EINVAL);
    }
    linux_open_path(pid, path.as_str(), O_CREAT | O_WRONLY)
}

fn linux_newfstatat(pid: u64, dirfd: u64, path_ptr: u64, stat_ptr: u64, flags: u64) -> Result<u64, i64> {
    if path_ptr == 0 {
        return Err(LINUX_EFAULT);
    }
    let path = read_user_cstr(path_ptr, 4096)?;
    let resolved = if path.is_empty() && (flags & AT_EMPTY_PATH) != 0 {
        with_state_mut(|state| descriptor_path(state, pid, dirfd)).map_err(linux_errno)?
    } else {
        resolve_at_path_str(pid, dirfd, path.as_str())?
    };
    let st = vfs::stat(resolved.as_str()).map_err(|_| LINUX_ENOENT)?;
    let linux_st = linux_stat_from_vfs(&st);
    write_user_struct(stat_ptr, &linux_st)?;
    Ok(0)
}

fn linux_unlinkat(pid: u64, dirfd: u64, path_ptr: u64) -> Result<u64, i64> {
    let path = resolve_at_path(pid, dirfd, path_ptr)?;
    vfs::unlink(path.as_str()).map(|_| 0).map_err(|_| LINUX_EINVAL)
}

fn linux_renameat(pid: u64, olddirfd: u64, old_ptr: u64, newdirfd: u64, new_ptr: u64) -> Result<u64, i64> {
    let old_path = resolve_at_path(pid, olddirfd, old_ptr)?;
    let new_path = resolve_at_path(pid, newdirfd, new_ptr)?;
    vfs::rename(old_path.as_str(), new_path.as_str())
        .map(|_| 0)
        .map_err(|_| LINUX_EINVAL)
}

fn linux_linkat(pid: u64, olddirfd: u64, old_ptr: u64, newdirfd: u64, new_ptr: u64) -> Result<u64, i64> {
    let old_path = resolve_at_path(pid, olddirfd, old_ptr)?;
    let new_path = resolve_at_path(pid, newdirfd, new_ptr)?;
    linux_link_paths(old_path.as_str(), new_path.as_str())
}

fn linux_faccessat(pid: u64, dirfd: u64, path_ptr: u64) -> Result<u64, i64> {
    let path = resolve_at_path(pid, dirfd, path_ptr)?;
    vfs::stat(path.as_str()).map(|_| 0).map_err(|_| LINUX_ENOENT)
}

fn linux_dup3(pid: u64, oldfd: u64, newfd: u64) -> Result<u64, i64> {
    if oldfd == newfd {
        return Err(LINUX_EINVAL);
    }
    dispatch_custom(SyscallNumber::Dup2, [oldfd, newfd, 0, 0, 0, 0], pid)
}

fn linux_pipe2(pid: u64, pipefd_ptr: u64, flags: u64) -> Result<u64, i64> {
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

        let mut read_obj = DescriptorObject::new(DescriptorKind::PipeRead(pipe_id));
        let mut write_obj = DescriptorObject::new(DescriptorKind::PipeWrite(pipe_id));
        let nonblocking = (flags & O_NONBLOCK) != 0;
        read_obj.nonblocking = nonblocking;
        write_obj.nonblocking = nonblocking;

        let read_slot = state.alloc_object(read_obj.kind);
        let write_slot = state.alloc_object(write_obj.kind);
        if let Some(obj) = state.objects.get_mut(read_slot).and_then(|o| o.as_mut()) {
            obj.nonblocking = nonblocking;
        }
        if let Some(obj) = state.objects.get_mut(write_slot).and_then(|o| o.as_mut()) {
            obj.nonblocking = nonblocking;
        }
        let rfd = state.alloc_fd(pid, read_slot);
        let wfd = state.alloc_fd(pid, write_slot);
        Ok::<u64, SyscallError>((wfd << 32) | (rfd & 0xFFFF_FFFF))
    })
    .map_err(linux_errno)?;
    let pair = [(packed & 0xFFFF_FFFF) as i32, (packed >> 32) as i32];
    write_user_struct(pipefd_ptr, &pair)?;
    Ok(0)
}

fn linux_poll(pid: u64, fds_ptr: u64, nfds: u64, timeout_ms: u64) -> Result<u64, i64> {
    let mut ready = 0u64;
    for idx in 0..nfds as usize {
        let slot_ptr = fds_ptr + (idx * size_of::<LinuxPollFd>()) as u64;
        let mut pfd = unsafe { *user_ref::<LinuxPollFd>(slot_ptr)? };
        let revents = dispatch_custom(
            SyscallNumber::Poll,
            [pfd.fd as u64, pfd.events as u64, timeout_ms, 0, 0, 0],
            pid,
        )?;
        pfd.revents = revents as i16;
        if pfd.revents != 0 {
            ready = ready.saturating_add(1);
        }
        write_user_struct(slot_ptr, &pfd)?;
    }
    Ok(ready)
}

fn linux_nanosleep(req_ptr: u64, rem_ptr: u64) -> Result<u64, i64> {
    let req = unsafe { *user_ref::<LinuxTimespec>(req_ptr)? };
    let ms = (req.tv_sec.max(0) as u64)
        .saturating_mul(1000)
        .saturating_add((req.tv_nsec.max(0) as u64) / 1_000_000);
    timer::sleep(ms);
    if rem_ptr != 0 {
        let rem = LinuxTimespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        write_user_struct(rem_ptr, &rem)?;
    }
    Ok(0)
}

fn linux_uname(buf_ptr: u64) -> Result<u64, i64> {
    fn fill(dst: &mut [u8; 65], src: &[u8]) {
        let take = src.len().min(64);
        dst[..take].copy_from_slice(&src[..take]);
        dst[take] = 0;
    }

    let mut uts = LinuxUtsname {
        sysname: [0; 65],
        nodename: [0; 65],
        release: [0; 65],
        version: [0; 65],
        machine: [0; 65],
        domainname: [0; 65],
    };
    fill(&mut uts.sysname, crate::version::PRODUCT_NAME.as_bytes());
    fill(&mut uts.nodename, b"saios");
    fill(&mut uts.release, crate::version::UTS_RELEASE.as_bytes());
    fill(&mut uts.version, crate::version::UTS_VERSION.as_bytes());
    fill(&mut uts.machine, b"x86_64");
    fill(&mut uts.domainname, b"localdomain");
    write_user_struct(buf_ptr, &uts)?;
    Ok(0)
}

fn linux_gettimeofday(tv_ptr: u64, tz_ptr: u64) -> Result<u64, i64> {
    let uptime = timer::uptime();
    if tv_ptr != 0 {
        let tv = LinuxTimeval {
            tv_sec: uptime.as_secs() as i64,
            tv_usec: uptime.subsec_micros() as i64,
        };
        write_user_struct(tv_ptr, &tv)?;
    }
    if tz_ptr != 0 {
        zero_user_bytes(tz_ptr, 8)?;
    }
    Ok(0)
}

fn linux_clock_gettime(ts_ptr: u64) -> Result<u64, i64> {
    let uptime = timer::uptime();
    let ts = LinuxTimespec {
        tv_sec: uptime.as_secs() as i64,
        tv_nsec: uptime.subsec_nanos() as i64,
    };
    write_user_struct(ts_ptr, &ts)?;
    Ok(0)
}

fn linux_times(buf_ptr: u64) -> Result<u64, i64> {
    let ticks = timer::ticks() as i64;
    if buf_ptr != 0 {
        let tms = LinuxTms {
            tms_utime: ticks,
            tms_stime: 0,
            tms_cutime: 0,
            tms_cstime: 0,
        };
        write_user_struct(buf_ptr, &tms)?;
    }
    Ok(timer::ticks())
}

fn linux_getcwd(buf_ptr: u64, size: u64) -> Result<u64, i64> {
    let cwd = vfs::pwd();
    let bytes = cwd.as_bytes();
    if size == 0 || bytes.len() + 1 > size as usize {
        return Err(LINUX_EINVAL);
    }
    write_user_bytes(buf_ptr, bytes)?;
    write_user_bytes(buf_ptr + bytes.len() as u64, &[0])?;
    Ok(buf_ptr)
}

fn linux_path_stat(path_ptr: u64, stat_ptr: u64) -> Result<u64, i64> {
    let path = read_user_cstr(path_ptr, 4096)?;
    let resolved = process::stat_redirect_path(path.as_str());
    let st = vfs::stat(resolved.as_str()).map_err(|_| LINUX_ENOENT)?;
    let linux_st = linux_stat_from_vfs(&st);
    write_user_struct(stat_ptr, &linux_st)?;
    Ok(0)
}

fn linux_fstat(pid: u64, fd: u64, stat_ptr: u64) -> Result<u64, i64> {
    let st = if fd <= 2 {
        LinuxStat {
            st_dev: 1,
            st_ino: fd,
            st_nlink: 1,
            st_mode: 0o020666,
            st_uid: 0,
            st_gid: 0,
            __pad0: 0,
            st_rdev: 0,
            st_size: 0,
            st_blksize: 4096,
            st_blocks: 0,
            st_atime: 0,
            st_atime_nsec: 0,
            st_mtime: 0,
            st_mtime_nsec: 0,
            st_ctime: 0,
            st_ctime_nsec: 0,
            __reserved: [0; 3],
        }
    } else {
        let size = dispatch_custom(SyscallNumber::Fstat, [fd, 0, 0, 0, 0, 0], pid)? as usize;
        linux_stat_from_vfs(&vfs::FileStat {
            kind: vfs::FileType::File,
            size,
        })
    };
    write_user_struct(stat_ptr, &st)?;
    Ok(0)
}

fn linux_access(path_ptr: u64) -> Result<u64, i64> {
    let path = read_user_cstr(path_ptr, 4096)?;
    let resolved = process::stat_redirect_path(path.as_str());
    vfs::stat(resolved.as_str()).map(|_| 0).map_err(|_| LINUX_ENOENT)
}

fn linux_chdir(path_ptr: u64) -> Result<u64, i64> {
    let path = read_user_cstr(path_ptr, 4096)?;
    vfs::cd(path.as_str()).map(|_| 0).map_err(|_| LINUX_ENOENT)
}

fn linux_rename(old_ptr: u64, new_ptr: u64) -> Result<u64, i64> {
    let from = read_user_cstr(old_ptr, 4096)?;
    let to = read_user_cstr(new_ptr, 4096)?;
    vfs::rename(from.as_str(), to.as_str())
        .map(|_| 0)
        .map_err(|_| LINUX_EINVAL)
}

fn linux_mkdir(path_ptr: u64) -> Result<u64, i64> {
    let path = read_user_cstr(path_ptr, 4096)?;
    vfs::mkdir(path.as_str()).map(|_| 0).map_err(|_| LINUX_EINVAL)
}

fn linux_unlink(path_ptr: u64) -> Result<u64, i64> {
    let path = read_user_cstr(path_ptr, 4096)?;
    vfs::unlink(path.as_str()).map(|_| 0).map_err(|_| LINUX_EINVAL)
}

fn linux_creat(pid: u64, path_ptr: u64) -> Result<u64, i64> {
    linux_open(pid, path_ptr, O_WRONLY | O_CREAT | O_TRUNC)
}

fn linux_setpgid(pid: u64, target: u64, pgid: u64) -> Result<u64, i64> {
    let target_pid = if target == 0 { pid } else { target };
    let target_pgid = if pgid == 0 { target_pid } else { pgid };
    process::set_process_group(target_pid, target_pgid)
        .map(|_| 0)
        .map_err(|_| LINUX_EINVAL)
}

fn linux_getppid(pid: u64) -> u64 {
    process::record(pid)
        .and_then(|r| r.parent_pid)
        .unwrap_or(1)
}

fn linux_getpgrp(pid: u64) -> Result<u64, i64> {
    process::process_group(pid).ok_or(LINUX_EINVAL)
}

fn linux_setsid(pid: u64) -> Result<u64, i64> {
    process::create_session(pid).map_err(|_| LINUX_EINVAL)
}

fn linux_getpgid(target: u64, pid: u64) -> Result<u64, i64> {
    let q = if target == 0 { pid } else { target };
    process::process_group(q).ok_or(LINUX_EINVAL)
}

fn linux_getsid(target: u64, pid: u64) -> Result<u64, i64> {
    let q = if target == 0 { pid } else { target };
    process::session_id(q).ok_or(LINUX_EINVAL)
}

fn linux_set_tid_address(pid: u64, clear_child_tid: u64) -> Result<u64, i64> {
    with_state_mut(|state| {
        let proc = state.proc_mut(pid);
        proc.clear_child_tid = clear_child_tid;
    });
    Ok(pid)
}

fn linux_set_robust_list(pid: u64, head: u64, len: u64) -> Result<u64, i64> {
    if len == 0 {
        return Err(LINUX_EINVAL);
    }
    with_state_mut(|state| {
        let proc = state.proc_mut(pid);
        proc.robust_list_head = head;
        proc.robust_list_len = len;
    });
    Ok(0)
}

fn linux_rseq(pid: u64, area: u64, len: u64, flags: u64, sig: u64) -> Result<u64, i64> {
    if flags != 0 {
        return Err(LINUX_EINVAL);
    }
    if len == 0 {
        return Err(LINUX_EINVAL);
    }
    with_state_mut(|state| {
        let proc = state.proc_mut(pid);
        proc.rseq_area = area;
        proc.rseq_len = len;
        proc.rseq_sig = sig;
    });
    Ok(0)
}

fn linux_fcntl(pid: u64, fd: u64, cmd: u64, arg: u64) -> Result<u64, i64> {
    match cmd {
        F_GETFD => Ok(0),
        F_SETFD => Ok(0),
        F_GETFL => Ok(0),
        F_SETFL => {
            with_state_mut(|state| {
                let obj_idx = state.obj_for_fd(pid, fd).ok_or(SyscallError::InvalidArgument)?;
                let obj = state
                    .objects
                    .get_mut(obj_idx)
                    .and_then(|o| o.as_mut())
                    .ok_or(SyscallError::InvalidArgument)?;
                obj.nonblocking = (arg & O_NONBLOCK) != 0;
                Ok::<u64, SyscallError>(0)
            })
            .map_err(linux_errno)
        }
        F_DUPFD | F_DUPFD_CLOEXEC => {
            let min_fd = arg.max(3) as usize;
            with_state_mut(|state| {
                let obj_idx = state.obj_for_fd(pid, fd).ok_or(SyscallError::InvalidArgument)?;
                let proc = state.proc_mut(pid);
                if proc.slots.len() <= min_fd {
                    proc.slots.resize(min_fd + 1, None);
                }
                let mut slot = min_fd;
                while slot < proc.slots.len() && proc.slots[slot].is_some() {
                    slot += 1;
                }
                if slot == proc.slots.len() {
                    proc.slots.push(None);
                }
                proc.slots[slot] = Some(obj_idx);
                let obj = state
                    .objects
                    .get_mut(obj_idx)
                    .and_then(|o| o.as_mut())
                    .ok_or(SyscallError::InvalidArgument)?;
                obj.refs = obj.refs.saturating_add(1);
                Ok::<u64, SyscallError>(slot as u64)
            })
            .map_err(linux_errno)
        }
        _ => Err(LINUX_ENOSYS),
    }
}

fn linux_getrlimit(ptr: u64) -> Result<u64, i64> {
    let lim = LinuxRlimit {
        rlim_cur: u64::MAX,
        rlim_max: u64::MAX,
    };
    write_user_struct(ptr, &lim)?;
    Ok(0)
}

fn linux_getres_ids(ptr1: u64, ptr2: u64, ptr3: u64) -> Result<u64, i64> {
    if ptr1 != 0 {
        write_user_struct(ptr1, &0u32)?;
    }
    if ptr2 != 0 {
        write_user_struct(ptr2, &0u32)?;
    }
    if ptr3 != 0 {
        write_user_struct(ptr3, &0u32)?;
    }
    Ok(0)
}

fn linux_sysinfo(ptr: u64) -> Result<u64, i64> {
    let info = LinuxSysinfo {
        uptime: timer::uptime().as_secs() as i64,
        loads: [0; 3],
        totalram: 0,
        freeram: 0,
        sharedram: 0,
        bufferram: 0,
        totalswap: 0,
        freeswap: 0,
        procs: process::jobs().len() as u16,
        pad: 0,
        totalhigh: 0,
        freehigh: 0,
        mem_unit: 1,
        _f: [],
    };
    write_user_struct(ptr, &info)?;
    Ok(0)
}

fn linux_sched_getaffinity(mask_ptr: u64, len: u64) -> Result<u64, i64> {
    if len < 8 {
        return Err(LINUX_EINVAL);
    }
    write_user_struct(mask_ptr, &1u64)?;
    Ok(8)
}

fn linux_arch_prctl(code: u64, addr: u64) -> Result<u64, i64> {
    match code {
        ARCH_SET_FS => {
            hal::arch::x86_64::msr::wrmsr(IA32_FS_BASE, addr);
            Ok(0)
        }
        ARCH_GET_FS => {
            let value = hal::arch::x86_64::msr::rdmsr(IA32_FS_BASE);
            write_user_struct(addr, &value)?;
            Ok(0)
        }
        _ => Err(LINUX_ENOSYS),
    }
}

fn linux_time(ptr: u64) -> Result<u64, i64> {
    let secs = timer::uptime().as_secs() as u64;
    if ptr != 0 {
        write_user_struct(ptr, &(secs as i64))?;
    }
    Ok(secs)
}

fn linux_clock_getres(ptr: u64) -> Result<u64, i64> {
    let ts = LinuxTimespec {
        tv_sec: 0,
        tv_nsec: 10_000_000,
    };
    write_user_struct(ptr, &ts)?;
    Ok(0)
}

fn linux_clock_nanosleep(req_ptr: u64, rem_ptr: u64) -> Result<u64, i64> {
    linux_nanosleep(req_ptr, rem_ptr)
}

fn linux_exit_now(pid: u64, code: u64) -> ! {
    crate::console::println!("[syscall] exit_now pid={} code={}", pid, code);
    let _ = process::exit_quiet(pid, code as i32);
    // Wake any thread blocked in waitpid for this pid.
    crate::scheduler::unblock_waiters_for_pid(pid);
    // Switch back to the kernel's main address space before reading
    // fault-recovery globals: an isolated exec_root may have stale/corrupted
    // kernel identity-map entries (e.g. from VMM segment-load side effects)
    // that would make reads of hal_user_fault_return_rip return garbage.
    {
        use crate::vmm;
        use hal::arch::x86_64::paging;
        const ADDR_MASK: u64 = crate::vmm::ADDR_MASK;
        let kernel_cr3 = vmm::stats().cr3 & ADDR_MASK;
        let current_cr3 = paging::read_cr3() & ADDR_MASK;
        if kernel_cr3 != 0 && kernel_cr3 != current_cr3 {
            unsafe { paging::write_cr3(kernel_cr3) };
        }
    }
    hal::arch::x86_64::seed_support::resume_from_user_fault()
}


#[unsafe(no_mangle)]
pub extern "C" fn saios_linux_syscall(
    nr: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) -> i64 {
    let pid = match active_linux_pid() {
        Ok(pid) => pid,
        Err(code) => return code,
    };

    let out = match nr {
        0 => linux_read(pid, a0, a1, a2),
        1 => linux_write(pid, a0, a1, a2),
        2 => linux_open(pid, a0, a1),
        3 => linux_close(pid, a0),
        4 => linux_path_stat(a0, a1),
        5 => linux_fstat(pid, a0, a1),
        6 => linux_path_stat(a0, a1),
        7 => linux_poll(pid, a0, a1, a2),
        8 => dispatch_custom(SyscallNumber::Lseek, [a0, a1, a2, 0, 0, 0], pid),
        9 => dispatch_custom(SyscallNumber::Mmap, [a1, 0, 0, 0, 0, 0], pid),
        10 => Ok(0),
        11 => dispatch_custom(SyscallNumber::Munmap, [a0, a1, 0, 0, 0, 0], pid),
        12 => dispatch_custom(SyscallNumber::Brk, [a0, 0, 0, 0, 0, 0], pid),
        13 => Ok(0),
        14 => Ok(0),
        15 => Err(LINUX_ENOSYS),
        16 => dispatch_custom(SyscallNumber::Ioctl, [a0, a1, a2, 0, 0, 0], pid),
        17 => linux_pread64(pid, a0, a1, a2, a3),
        18 => linux_pwrite64(pid, a0, a1, a2, a3),
        19 => linux_readv(pid, a0, a1, a2),
        20 => linux_writev(pid, a0, a1, a2),
        21 => linux_access(a0),
        22 => linux_pipe(pid, a0),
        23 => linux_select(pid, a0, a1, a2, a3, a4),
        24 => {
            crate::scheduler::yield_now();
            Ok(0)
        }
        25 => Err(LINUX_ENOSYS),
        26 => Ok(0),
        27 => Err(LINUX_ENOSYS),
        28 => Ok(0),
        29 => Err(LINUX_ENOSYS),
        30 => Err(LINUX_ENOSYS),
        31 => Err(LINUX_ENOSYS),
        32 => dispatch_custom(SyscallNumber::Dup, [a0, 0, 0, 0, 0, 0], pid),
        33 => dispatch_custom(SyscallNumber::Dup2, [a0, a1, 0, 0, 0, 0], pid),
        34 => Err(LINUX_ENOSYS),
        35 => linux_nanosleep(a0, a1),
        36 => Err(LINUX_ENOSYS),
        37 => Err(LINUX_ENOSYS),
        38 => Err(LINUX_ENOSYS),
        39 => dispatch_custom(SyscallNumber::GetPid, [0, 0, 0, 0, 0, 0], pid),
        40 => Err(LINUX_ENOSYS),
        41 => Err(LINUX_ENOSYS),
        42 => Err(LINUX_ENOSYS),
        43 => Err(LINUX_ENOSYS),
        44 => Err(LINUX_ENOSYS),
        45 => Err(LINUX_ENOSYS),
        46 => Err(LINUX_ENOSYS),
        47 => Err(LINUX_ENOSYS),
        48 => Err(LINUX_ENOSYS),
        49 => Err(LINUX_ENOSYS),
        50 => Err(LINUX_ENOSYS),
        51 => Err(LINUX_ENOSYS),
        52 => Err(LINUX_ENOSYS),
        53 => Err(LINUX_ENOSYS),
        54 => Err(LINUX_ENOSYS),
        55 => Err(LINUX_ENOSYS),
        56 => dispatch_custom(SyscallNumber::Clone, [a0, a1, a2, a3, a4, a5], pid),
        57 => dispatch_custom(SyscallNumber::Fork, [0, 0, 0, 0, 0, 0], pid),
        58 => dispatch_custom(SyscallNumber::Fork, [0, 0, 0, 0, 0, 0], pid),
        59 => {
            let path = match read_user_cstr(a0, 4096) {
                Ok(path) => path,
                Err(code) => return code,
            };
            let argv = match read_user_argv(a1, 32) {
                Ok(argv) => argv,
                Err(code) => return code,
            };
            let arg_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
            // execve replaces the calling process: on success the process must
            // never return to ring-3.  Exit the child immediately with the
            // child program's own exit code so the parent sees a clean exit.
            match process::exec_in_place(pid, path.as_str(), arg_refs.as_slice(), &[]) {
                Ok(exit_code) => linux_exit_now(pid, exit_code as u64),
                Err(e) => Err(map_exec_error_to_linux(e)),
            }
        }
        60 => linux_exit_now(pid, a0),
        61 => dispatch_wait4(pid, a0, a2, a1),
        62 => dispatch_custom(SyscallNumber::Kill, [a0, a1, 0, 0, 0, 0], pid),
        63 => linux_uname(a0),
        64 => Err(LINUX_ENOSYS),
        65 => Err(LINUX_ENOSYS),
        66 => Err(LINUX_ENOSYS),
        67 => Err(LINUX_ENOSYS),
        68 => Err(LINUX_ENOSYS),
        69 => Err(LINUX_ENOSYS),
        70 => Err(LINUX_ENOSYS),
        71 => Err(LINUX_ENOSYS),
        72 => linux_fcntl(pid, a0, a1, a2),
        73 => Ok(0),
        74 => Ok(0),
        75 => Ok(0),
        76 => linux_truncate(a0, a1),
        77 => linux_ftruncate(pid, a0, a1),
        78 => linux_getdents64(pid, a0, a1, a2),
        79 => linux_getcwd(a0, a1),
        80 => linux_chdir(a0),
        81 => linux_fchdir(pid, a0),
        82 => linux_rename(a0, a1),
        83 => linux_mkdir(a0),
        84 => linux_unlink(a0),
        85 => linux_creat(pid, a0),
        86 => linux_link(a0, a1),
        87 => linux_unlink(a0),
        88 => Err(LINUX_ENOSYS),
        89 => Err(LINUX_ENOSYS),
        90 => Ok(0),
        91 => Ok(0),
        92 => Ok(0),
        93 => Ok(0),
        94 => Ok(0),
        95 => Ok(UMASK.swap((a0 as u32) & 0o777, Ordering::SeqCst) as u64),
        96 => linux_gettimeofday(a0, a1),
        97 => linux_getrlimit(a1),
        98 => {
            if a1 != 0 {
                match zero_user_bytes(a1, 144) {
                    Ok(()) => {}
                    Err(code) => return code,
                }
            }
            Ok(0)
        }
        99 => linux_sysinfo(a0),
        100 => linux_times(a0),
        101 => Err(LINUX_ENOSYS),
        102 => Ok(0),
        103 => Err(LINUX_ENOSYS),
        104 => Ok(0),
        105 => Ok(0),
        106 => Ok(0),
        107 => Ok(0),
        108 => Ok(0),
        109 => linux_setpgid(pid, a0, a1),
        110 => Ok(linux_getppid(pid)),
        111 => linux_getpgrp(pid),
        112 => linux_setsid(pid),
        113 => Ok(0),
        114 => Ok(0),
        115 => Ok(0),
        116 => Ok(0),
        117 => Ok(0),
        118 => linux_getres_ids(a0, a1, a2),
        119 => Ok(0),
        120 => linux_getres_ids(a0, a1, a2),
        121 => linux_getpgid(a0, pid),
        122 => Ok(0),
        123 => Ok(0),
        124 => linux_getsid(a0, pid),
        125 => Ok(0),
        126 => Ok(0),
        127 => {
            if a0 != 0 {
                match zero_user_bytes(a0, a1 as usize) {
                    Ok(()) => {}
                    Err(code) => return code,
                }
            }
            Ok(0)
        }
        128 => Err(LINUX_ENOSYS),
        129 => Err(LINUX_ENOSYS),
        130 => Err(LINUX_ENOSYS),
        131 => Ok(0),
        132 => Ok(0),
        133 => Err(LINUX_ENOSYS),
        134 => Err(LINUX_ENOSYS),
        135 => Ok(0),
        136 => Err(LINUX_ENOSYS),
        137 => {
            if a1 != 0 {
                match zero_user_bytes(a1, 128) {
                    Ok(()) => {}
                    Err(code) => return code,
                }
            }
            Ok(0)
        }
        138 => {
            if a1 != 0 {
                match zero_user_bytes(a1, 128) {
                    Ok(()) => {}
                    Err(code) => return code,
                }
            }
            Ok(0)
        }
        139 => Err(LINUX_ENOSYS),
        140 => Ok(0),
        141 => Ok(0),
        142 => Ok(0),
        143 => {
            if a1 != 0 {
                match write_user_struct(a1, &0i32) {
                    Ok(()) => {}
                    Err(code) => return code,
                }
            }
            Ok(0)
        }
        144 => Ok(0),
        145 => Ok(0),
        146 => Ok(0),
        147 => Ok(0),
        148 => linux_clock_getres(a1),
        149 => Ok(0),
        150 => Ok(0),
        151 => Ok(0),
        152 => Ok(0),
        153 => Err(LINUX_ENOSYS),
        154 => Err(LINUX_ENOSYS),
        155 => Err(LINUX_ENOSYS),
        156 => Err(LINUX_ENOSYS),
        157 => Ok(0),
        158 => linux_arch_prctl(a0, a1),
        159 => Err(LINUX_ENOSYS),
        160 => Ok(0),
        161 => Err(LINUX_ENOSYS),
        162 => Ok(0),
        163 => Err(LINUX_ENOSYS),
        164 => Err(LINUX_ENOSYS),
        165 => Err(LINUX_ENOSYS),
        166 => {
            let path = match read_user_cstr(a0, 4096) {
                Ok(path) => path,
                Err(code) => return code,
            };
            vfs::umount(path.as_str()).map(|_| 0).map_err(|_| LINUX_EINVAL)
        }
        167 => Err(LINUX_ENOSYS),
        168 => Err(LINUX_ENOSYS),
        169 => Err(LINUX_ENOSYS),
        170 => Ok(0),
        171 => Ok(0),
        172 => Err(LINUX_ENOSYS),
        173 => Err(LINUX_ENOSYS),
        174 => Err(LINUX_ENOSYS),
        175 => Err(LINUX_ENOSYS),
        176 => Err(LINUX_ENOSYS),
        177 => Err(LINUX_ENOSYS),
        178 => Err(LINUX_ENOSYS),
        179 => Err(LINUX_ENOSYS),
        180 => Err(LINUX_ENOSYS),
        181 => Err(LINUX_ENOSYS),
        182 => Err(LINUX_ENOSYS),
        183 => Err(LINUX_ENOSYS),
        184 => Err(LINUX_ENOSYS),
        185 => Err(LINUX_ENOSYS),
        186 => dispatch_custom(SyscallNumber::GetPid, [0, 0, 0, 0, 0, 0], pid),
        187 => Ok(0),
        188 => Err(LINUX_ENOSYS),
        189 => Err(LINUX_ENOSYS),
        190 => Err(LINUX_ENOSYS),
        191 => Err(LINUX_ENOSYS),
        192 => Err(LINUX_ENOSYS),
        193 => Err(LINUX_ENOSYS),
        194 => Err(LINUX_ENOSYS),
        195 => Err(LINUX_ENOSYS),
        196 => Err(LINUX_ENOSYS),
        197 => Err(LINUX_ENOSYS),
        198 => Err(LINUX_ENOSYS),
        199 => Err(LINUX_ENOSYS),
        200 => dispatch_custom(SyscallNumber::Kill, [a0, a1, 0, 0, 0, 0], pid),
        201 => linux_time(a0),
        202 => Ok(0),
        203 => Ok(0),
        204 => linux_sched_getaffinity(a2, a1),
        205 => Ok(0),
        206 => Err(LINUX_ENOSYS),
        207 => Err(LINUX_ENOSYS),
        208 => Err(LINUX_ENOSYS),
        209 => Err(LINUX_ENOSYS),
        210 => Err(LINUX_ENOSYS),
        211 => Ok(0),
        212 => Err(LINUX_ENOSYS),
        213 => Err(LINUX_ENOSYS),
        214 => Err(LINUX_ENOSYS),
        215 => Err(LINUX_ENOSYS),
        216 => Ok(0),
        217 => linux_getdents64(pid, a0, a1, a2),
        218 => linux_set_tid_address(pid, a0),
        219 => Ok(0),
        220 => Err(LINUX_ENOSYS),
        221 => Ok(0),
        222 => Err(LINUX_ENOSYS),
        223 => Err(LINUX_ENOSYS),
        224 => Err(LINUX_ENOSYS),
        225 => Err(LINUX_ENOSYS),
        226 => Err(LINUX_ENOSYS),
        227 => Err(LINUX_ENOSYS),
        228 => linux_clock_gettime(a1),
        229 => linux_clock_getres(a1),
        230 => linux_clock_nanosleep(a3, a4),
        231 => linux_exit_now(pid, a0),
        232 => Err(LINUX_ENOSYS),
        233 => Err(LINUX_ENOSYS),
        234 => dispatch_custom(SyscallNumber::Kill, [a1, a2, 0, 0, 0, 0], pid),
        235 => Ok(0),
        236 => Err(LINUX_ENOSYS),
        237 => Err(LINUX_ENOSYS),
        238 => Err(LINUX_ENOSYS),
        239 => Err(LINUX_ENOSYS),
        240 => Err(LINUX_ENOSYS),
        241 => Err(LINUX_ENOSYS),
        242 => Err(LINUX_ENOSYS),
        243 => Err(LINUX_ENOSYS),
        244 => Err(LINUX_ENOSYS),
        245 => Err(LINUX_ENOSYS),
        246 => Err(LINUX_ENOSYS),
        247 => Err(LINUX_ENOSYS),
        248 => Err(LINUX_ENOSYS),
        249 => Err(LINUX_ENOSYS),
        257 => linux_openat(pid, a0, a1, a2),
        258 => linux_mkdirat(pid, a0, a1),
        259 => linux_mknodat(pid, a0, a1, a2),
        262 => linux_newfstatat(pid, a0, a1, a2, a3),
        263 => linux_unlinkat(pid, a0, a1),
        264 => linux_renameat(pid, a0, a1, a2, a3),
        265 => linux_linkat(pid, a0, a1, a2, a3),
        269 => linux_faccessat(pid, a0, a1),
        270 => linux_select(pid, a0, a1, a2, a3, a5),
        271 => {
            let timeout_ms = if a2 == 0 {
                0
            } else {
                let ts = match unsafe { user_ref::<LinuxTimespec>(a2) } {
                    Ok(ts) => *ts,
                    Err(code) => return code,
                };
                (ts.tv_sec.max(0) as u64)
                    .saturating_mul(1000)
                    .saturating_add((ts.tv_nsec.max(0) as u64) / 1_000_000)
            };
            linux_poll(pid, a0, a1, timeout_ms)
        }
        272 => Ok(0),
        273 => linux_set_robust_list(pid, a0, a1),
        274 => {
            if a1 != 0 {
                match write_user_struct(a1, &0u64) {
                    Ok(()) => {}
                    Err(code) => return code,
                }
            }
            if a2 != 0 {
                match write_user_struct(a2, &0usize) {
                    Ok(()) => {}
                    Err(code) => return code,
                }
            }
            Ok(0)
        }
        292 => linux_dup3(pid, a0, a1),
        293 => linux_pipe2(pid, a0, a1),
        334 => linux_rseq(pid, a0, a1, a2, a3),
        _ => Err(LINUX_ENOSYS),
    };

    match out {
        Ok(value) => value as i64,
        Err(code) => code,
    }
}

fn resolve_program_argument(arg: u64) -> Result<String, SyscallError> {
    if arg == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    read_user_cstr(arg, 4096).map_err(|_| SyscallError::InvalidArgument)
}

fn resolve_path_argument(arg: u64) -> Result<String, SyscallError> {
    if arg == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    read_user_cstr(arg, 4096).map_err(|_| SyscallError::InvalidArgument)
}

fn user_slice_ro(ptr: u64, len: usize) -> Result<&'static [u8], SyscallError> {
    unsafe { user_slice(ptr, len).map_err(|_| SyscallError::InvalidArgument) }
}

fn user_slice_rw(ptr: u64, len: usize) -> Result<&'static mut [u8], SyscallError> {
    unsafe { user_slice_mut(ptr, len).map_err(|_| SyscallError::InvalidArgument) }
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

#[derive(Clone, Debug)]
enum DescriptorKind {
    Vfs { fd: vfs::VfsFd, path: String },
    Directory(String),
    Tty,
    PipeRead(usize),
    PipeWrite(usize),
}

#[derive(Clone, Debug)]
struct DescriptorObject {
    kind: DescriptorKind,
    refs: u32,
    nonblocking: bool,
    cursor: usize,
}

impl DescriptorObject {
    fn new(kind: DescriptorKind) -> Self {
        Self {
            kind,
            refs: 1,
            nonblocking: false,
            cursor: 0,
        }
    }
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
    clear_child_tid: u64,
    robust_list_head: u64,
    robust_list_len: u64,
    rseq_area: u64,
    rseq_len: u64,
    rseq_sig: u64,
}

#[derive(Clone, Debug)]
struct SyscallState {
    procs: Vec<ProcessFdTable>,
    objects: Vec<Option<DescriptorObject>>,
    pipes: Vec<Option<PipeBuffer>>,
    brk: u64,
    brk_mapped_end: u64,
    next_mmap: u64,
}

impl SyscallState {
    fn new() -> Self {
        Self {
            procs: Vec::new(),
            objects: Vec::new(),
            pipes: Vec::new(),
            brk: 0x0100_0000,
            brk_mapped_end: 0x0100_0000,
            next_mmap: 0x1000_0000,
        }
    }

    fn proc_mut(&mut self, pid: u64) -> &mut ProcessFdTable {
        if let Some(idx) = self.procs.iter().position(|p| p.pid == pid) {
            return &mut self.procs[idx];
        }
        let mut slots = Vec::new();
        slots.resize(3, None); // reserve 0,1,2
        self.procs.push(ProcessFdTable {
            pid,
            slots,
            clear_child_tid: 0,
            robust_list_head: 0,
            robust_list_len: 0,
            rseq_area: 0,
            rseq_len: 0,
            rseq_sig: 0,
        });
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
            *slot = Some(DescriptorObject::new(kind));
            return idx;
        }
        self.objects.push(Some(DescriptorObject::new(kind)));
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
            let final_obj = obj.clone();
            self.objects[obj_idx] = None;
            if let DescriptorKind::Vfs { fd, .. } = final_obj.kind {
                let _ = vfs::close(fd);
            }
        }
        Ok(())
    }
}

static STATE: StaticCell<Option<SyscallState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    hal::arch::x86_64::sync::spinlock_acquire(&LOCK);
}

fn unlock() {
    hal::arch::x86_64::sync::spinlock_release(&LOCK);
}

/// Point brk/mmap growth at the given page-aligned address instead of the
/// fixed default. The default (a raw guessed low address) can land inside a
/// still-huge kernel-identity-mapped 2 MiB PDE; growing brk there forces
/// `vmm::map_owned` to demote and *clear* that whole 2 MiB window (see
/// `vmm.rs`'s huge-PDE split), destroying whatever kernel mapping was really
/// there. Callers should pass an address just past the process's own highest
/// mapped ELF segment, which is guaranteed to be inside a range this same
/// process already safely owns (or an already-demoted, already-user-owned
/// page table), so growth never touches unrelated kernel memory.
pub fn set_initial_brk(addr: u64) {
    with_state_mut(|state| {
        state.brk = addr;
        state.brk_mapped_end = addr;
    });
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
        .and_then(|o| o.as_ref())
        .ok_or(SyscallError::InvalidArgument)?;
    match obj.kind {
        DescriptorKind::Vfs { fd, .. } => Ok(fd),
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
        .and_then(|o| o.as_ref())
        .ok_or(SyscallError::InvalidArgument)?;

    let requested = if events == 0 {
        POLLIN | POLLOUT
    } else {
        events
    };
    let mut revents = 0u64;
    match obj.kind {
        DescriptorKind::Vfs { .. } | DescriptorKind::Directory(_) | DescriptorKind::Tty => {
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
            process::exit_quiet(ctx.pid, code).map_err(|_| SyscallError::InvalidArgument)?;
            Ok(0)
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
                // Block until any child exits.
                // We don't know which pid will exit first; use 0 as sentinel.
                crate::scheduler::block_current_waiting_for_pid(0);
                // Re-attempt after unblock.
                if let Some((pid, code)) = process::first_exited_child(ctx.pid) {
                    let status = encode_wait_status(code);
                    let _ = process::reap_child(ctx.pid, pid);
                    return Ok((pid << 32) | (status & 0xFFFF_FFFF));
                }
                return Err(SyscallError::WouldBlock);
            }

            let pid = target as u64;
            let rec = process::child_record(ctx.pid, pid).ok_or(SyscallError::NoChild)?;
            if rec.state != process::ProcessState::Exited {
                if (options & WAIT_NOHANG) != 0 {
                    return Ok(0);
                }
                // Block until this specific child exits.
                drop(rec);
                crate::scheduler::block_current_waiting_for_pid(pid);
                // Re-check after unblock.
                let rec = process::child_record(ctx.pid, pid).ok_or(SyscallError::NoChild)?;
                if rec.state != process::ProcessState::Exited {
                    return Err(SyscallError::WouldBlock);
                }
            }

            let code =
                process::reap_child(ctx.pid, pid).map_err(|_| SyscallError::InvalidArgument)?;
            let status = encode_wait_status(code);
            Ok((pid << 32) | (status & 0xFFFF_FFFF))
        }
        SyscallNumber::Exec => {
            // args[0] = user C-string program pointer.
            // args[1] = optional user argv pointer (NULL-terminated char**).
            let name = resolve_program_argument(req.args[0])?;
            let argv_owned = if req.args[1] != 0 {
                read_user_argv(req.args[1], 32).map_err(|_| SyscallError::InvalidArgument)?
            } else {
                Vec::new()
            };
            let argv_refs: Vec<&str> = argv_owned.iter().map(|s| s.as_str()).collect();
            let code = process::exec_in_place(ctx.pid, name.as_str(), argv_refs.as_slice(), &[])
                .map_err(|_| SyscallError::InvalidArgument)?;
            Ok(code as u64)
        }
        SyscallNumber::Spawn => {
            // args[0] = user C-string program pointer.
            // args[1] = optional user argv pointer (NULL-terminated char**).
            let name = resolve_program_argument(req.args[0])?;
            let argv_owned = if req.args[1] != 0 {
                read_user_argv(req.args[1], 32).map_err(|_| SyscallError::InvalidArgument)?
            } else {
                Vec::new()
            };
            let argv_refs: Vec<&str> = argv_owned.iter().map(|s| s.as_str()).collect();
            let pid = process::spawn_from(Some(ctx.pid), name.as_str(), argv_refs.as_slice(), &[])
                .map_err(|_| SyscallError::InvalidArgument)?;
            Ok(pid)
        }
        SyscallNumber::Open => {
            // args[0] = user C-string path pointer,
            // args[1] = open mode (0=ro, 1=rw+create, 2=append+create).
            let path = resolve_path_argument(req.args[0])?;
            let options = open_mode_to_options(req.args[1]).ok_or(SyscallError::InvalidArgument)?;
            let vfd = vfs::open(path.as_str(), options).map_err(|_| SyscallError::InvalidArgument)?;
            let fd = with_state_mut(|state| {
                let obj = state.alloc_object(DescriptorKind::Vfs {
                    fd: vfd,
                    path,
                });
                state.alloc_fd(ctx.pid, obj)
            });
            Ok(fd)
        }
        SyscallNumber::Read => {
            // args[0] = fd, args[1] = user buffer ptr, args[2] = max bytes.
            let fd = req.args[0];
            let max_len = req.args[2] as usize;
            if max_len == 0 {
                return Ok(0);
            }
            let data = with_state_mut(|state| {
                let obj_idx = state
                    .obj_for_fd(ctx.pid, fd)
                    .ok_or(SyscallError::InvalidArgument)?;
                let obj = state
                    .objects
                    .get(obj_idx)
                    .and_then(|o| o.as_ref())
                    .ok_or(SyscallError::InvalidArgument)?;
                match obj.kind {
                    DescriptorKind::Vfs { fd: vfd, .. } => {
                        vfs::read(vfd, max_len).map_err(|_| SyscallError::InvalidArgument)
                    }
                    DescriptorKind::Directory(_) => Err(SyscallError::InvalidArgument),
                    DescriptorKind::Tty => Err(SyscallError::InvalidArgument),
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

            let dst = user_slice_rw(req.args[1], data.len())?;
            dst.copy_from_slice(data.as_slice());
            Ok(data.len() as u64)
        }
        SyscallNumber::Write => {
            // args[0] = fd, args[1] = user buffer ptr, args[2] = len.
            let fd = req.args[0];
            let len = req.args[2] as usize;
            if len == 0 {
                return Ok(0);
            }
            let src = user_slice_ro(req.args[1], len)?;
            let owned_data = src.to_vec();
            let data = owned_data.as_slice();
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
                            .and_then(|o| o.as_ref())
                            .ok_or(SyscallError::InvalidArgument)?;
                        match obj.kind {
                            DescriptorKind::Vfs { fd: vfd, .. } => {
                                vfs::write(vfd, data).map_err(|_| SyscallError::InvalidArgument)
                            }
                            DescriptorKind::Directory(_) => Err(SyscallError::InvalidArgument),
                            DescriptorKind::Tty => {
                                let text = core::str::from_utf8(data).unwrap_or("<binary>");
                                console::print(text);
                                Ok(data.len())
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
            // args[0] = user C-string path pointer.
            // Optional pointer mode: args[1] = user stat buffer.
            let path = resolve_path_argument(req.args[0])?;
            let st = vfs::stat(path.as_str()).map_err(|_| SyscallError::InvalidArgument)?;
            if req.args[1] != 0 {
                let linux = linux_stat_from_vfs(&st);
                let bytes = unsafe {
                    core::slice::from_raw_parts(
                        (&linux as *const LinuxStat).cast::<u8>(),
                        size_of::<LinuxStat>(),
                    )
                };
                let dst = user_slice_rw(req.args[1], bytes.len())?;
                dst.copy_from_slice(bytes);
            }
            Ok(st.size as u64)
        }
        SyscallNumber::Fstat => {
            // args[0] = fd. Returns file size in bytes.
            // Optional pointer mode: args[1] = user stat buffer.
            let (size, path) = with_state_mut(|state| {
                let path = descriptor_path(state, ctx.pid, req.args[0])?;
                let vfd = resolve_vfs_fd(state, ctx.pid, req.args[0])?;
                let cur = vfs::seek(vfd, vfs::SeekFrom::Current(0))
                    .map_err(|_| SyscallError::InvalidArgument)?;
                let end = vfs::seek(vfd, vfs::SeekFrom::End(0))
                    .map_err(|_| SyscallError::InvalidArgument)?;
                let _ = vfs::seek(vfd, vfs::SeekFrom::Start(cur));
                Ok::<(usize, String), SyscallError>((end, path))
            })?;

            if req.args[1] != 0 {
                let st = vfs::stat(path.as_str()).map_err(|_| SyscallError::InvalidArgument)?;
                let linux = linux_stat_from_vfs(&st);
                let bytes = unsafe {
                    core::slice::from_raw_parts(
                        (&linux as *const LinuxStat).cast::<u8>(),
                        size_of::<LinuxStat>(),
                    )
                };
                let dst = user_slice_rw(req.args[1], bytes.len())?;
                dst.copy_from_slice(bytes);
            }

            Ok(size as u64)
        }
        SyscallNumber::Getdents64 => {
            // args[0] = directory fd, args[1] = user output buffer, args[2] = max bytes.
            if req.args[1] == 0 || req.args[2] == 0 {
                return Err(SyscallError::InvalidArgument);
            }
            linux_getdents64(ctx.pid, req.args[0], req.args[1], req.args[2])
                .map_err(|_| SyscallError::InvalidArgument)
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
            let pages = (len_aligned / vmm::PAGE_SIZE) as usize;
            map_user_anon_pages(addr, pages, "linux-mmap")
                .map_err(|_| SyscallError::InvalidArgument)?;
            Ok(addr)
        }
        SyscallNumber::Munmap => {
            // args[0] = addr, args[1] = len. No-op in this build.
            let _addr = req.args[0];
            let _len = req.args[1];
            Ok(0)
        }
        SyscallNumber::Brk => {
            // args[0] = new break (0 = query current). Growing the break maps
            // fresh user pages; on allocation failure the old break is
            // returned unchanged (Linux brk contract).
            let brk = with_state_mut(|state| {
                let requested = req.args[0];
                if requested != 0 {
                    if requested > state.brk_mapped_end {
                        let start = state.brk_mapped_end;
                        let end = (requested + (vmm::PAGE_SIZE - 1)) & !(vmm::PAGE_SIZE - 1);
                        let pages = ((end - start) / vmm::PAGE_SIZE) as usize;
                        if map_user_anon_pages(start, pages, "linux-brk").is_err() {
                            return state.brk;
                        }
                        state.brk_mapped_end = end;
                    }
                    state.brk = requested;
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
                    TIOCGPGRP => {
                        let pgid = process::foreground_process_group().unwrap_or(ctx.pid) as u32;
                        write_user_struct(value, &pgid).map_err(|_| SyscallError::InvalidArgument)?;
                        Ok(0)
                    }
                    TIOCSPGRP => {
                        let pgid: u32 =
                            read_user_struct(value).map_err(|_| SyscallError::InvalidArgument)?;
                        process::set_foreground_process_group(pgid as u64)
                            .map_err(|_| SyscallError::InvalidArgument)?;
                        Ok(0)
                    }
                    TIOCGWINSZ => {
                        write_user_struct(value, &WINSIZE_DEFAULT)
                            .map_err(|_| SyscallError::InvalidArgument)?;
                        Ok(0)
                    }
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
                    TCGETS => Ok(0),
                    TIOCGPGRP => {
                        let pgid = process::foreground_process_group().unwrap_or(ctx.pid) as u32;
                        write_user_struct(value, &pgid).map_err(|_| SyscallError::InvalidArgument)?;
                        Ok(0)
                    }
                    TIOCSPGRP => {
                        let pgid: u32 =
                            read_user_struct(value).map_err(|_| SyscallError::InvalidArgument)?;
                        process::set_foreground_process_group(pgid as u64)
                            .map_err(|_| SyscallError::InvalidArgument)?;
                        Ok(0)
                    }
                    TIOCGWINSZ => {
                        write_user_struct(value, &WINSIZE_DEFAULT)
                            .map_err(|_| SyscallError::InvalidArgument)?;
                        Ok(0)
                    }
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

            if (flags & CLONE_THREAD) != 0 {
                // Thread-level clone: just create a process record, no fork semantics.
                if (flags & (CLONE_SETTLS | CLONE_PARENT_SETTID | CLONE_CHILD_CLEARTID)) != 0 {
                    return Err(SyscallError::Unimplemented);
                }
                let child =
                    process::fork_from(ctx.pid, flags).map_err(|_| SyscallError::InvalidArgument)?;
                return Ok(child);
            }

            // Process-level clone / vfork.
            // CLONE_VM, CLONE_VFORK, CLONE_FS, CLONE_FILES, CLONE_SIGHAND are
            // all effectively true in our shared-address-space model — accept
            // and ignore them.  Reject only flags that need kernel TID pointer
            // operations we do not implement.
            if (flags & (CLONE_SETTLS | CLONE_PARENT_SETTID | CLONE_CHILD_CLEARTID)) != 0 {
                return Err(SyscallError::Unimplemented);
            }

            let child_pid =
                process::fork_from(ctx.pid, flags).map_err(|_| SyscallError::InvalidArgument)?;
            let _ = process::set_process_group(child_pid, child_pid);
            let parent_frame = hal::arch::x86_64::syscall::capture_user_syscall_frame();
            let mut child_frame = parent_frame;
            child_frame.rax = 0; // fork returns 0 in child
            let parent_tid = crate::scheduler::current_thread_id().unwrap_or(0);
            crate::scheduler::spawn_user_child_thread(child_pid, parent_tid, child_frame);
            // Block parent until child exits (vfork-style cooperative execution).
            crate::scheduler::block_current_waiting_for_pid(child_pid);
            // Parent resumes here after child has exited.
            Ok(child_pid)
        }
        SyscallNumber::Fork => {
            let child_pid =
                process::fork_from(ctx.pid, SIGCHLD).map_err(|_| SyscallError::InvalidArgument)?;
            let parent_frame = hal::arch::x86_64::syscall::capture_user_syscall_frame();
            let mut child_frame = parent_frame;
            child_frame.rax = 0; // fork returns 0 in child
            let parent_tid = crate::scheduler::current_thread_id().unwrap_or(0);
            crate::scheduler::spawn_user_child_thread(child_pid, parent_tid, child_frame);
            // Block parent until child exits.
            crate::scheduler::block_current_waiting_for_pid(child_pid);
            Ok(child_pid)
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
