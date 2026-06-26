use super::*;
use crate::vfs_contract::VfsContract;

#[repr(C)]
struct Iovec {
    base: u64,
    len: u64,
}

#[repr(C, packed)]
struct LinuxDirent64 {
    d_ino: u64,
    d_off: u64,
    d_reclen: u16,
    d_type: u8,
}

fn enforce_vfs_contract(operation: crate::vfs_contract::VfsOperation, tag: &'static str) {
    if !crate::arch::syscall::kernel_gs_active() {
        return;
    }
    let access = process::with_current_process(|proc| crate::vfs_contract::VfsAccess {
        pid: proc.pid,
        uid: proc.euid,
        gid: proc.egid,
        operation,
    })
    .unwrap_or(crate::vfs_contract::VfsAccess {
        pid: 0,
        uid: 0,
        gid: 0,
        operation,
    });
    crate::vfs_contract::VfsContract::validate_access_or_panic(access, tag);
}

fn current_cwd() -> String {
    process::with_current_process(|proc| proc.cwd.clone()).unwrap_or_else(|| String::from("/"))
}

#[repr(C)]
struct Statfs {
    f_type: i64,
    f_bsize: i64,
    f_blocks: i64,
    f_bfree: i64,
    f_bavail: i64,
    f_files: i64,
    f_ffree: i64,
    f_fsid: [i32; 2],
    f_namelen: i64,
    f_frsize: i64,
    f_flags: i64,
    f_spare: [i64; 4],
}

pub fn sys_read(fd: u64, buf_ptr: u64, len: u64) -> i64 {
    enforce_vfs_contract(crate::vfs_contract::VfsOperation::Read, "sys_read");
    if buf_ptr < 0x1000 || len == 0 {
        return EINVAL;
    }
    let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, len as usize) };

    match VfsContract::read_fd(fd as usize, buf) {
        Ok(n) => n as i64,
        Err(e) => vfs_err(e),
    }
}

pub fn sys_write(fd: u64, buf_ptr: u64, len: u64) -> i64 {
    enforce_vfs_contract(crate::vfs_contract::VfsOperation::Write, "sys_write");
    crate::syscall::trace_write_enter(fd, len, buf_ptr);
    let ret = if buf_ptr < 0x1000 || len == 0 || len > 1024 * 1024 {
        EINVAL
    } else {
        let buf = unsafe { core::slice::from_raw_parts(buf_ptr as *const u8, len as usize) };

        match fd {
            FD_STDIN => -1,
            _ => match VfsContract::write_fd(fd as usize, buf) {
                Ok(n) => n as i64,
                Err(e) => vfs_err(e),
            },
        }
    };
    crate::syscall::trace_write_exit(ret);
    ret
}

pub fn sys_open(path_ptr: u64, flags: u64, mode: u64) -> i64 {
    sys_openat(!0u64, path_ptr, flags, mode)
}

pub fn sys_openat(_dirfd: u64, path_ptr: u64, flags: u64, mode: u64) -> i64 {
    enforce_vfs_contract(crate::vfs_contract::VfsOperation::Open, "sys_openat");
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };

    let oflags = flags as u32;
    let file = match VfsContract::open(&path, oflags, mode as u32) {
        Ok(file) => file,
        Err(e) => return vfs_err(e),
    };
    VfsContract::insert_fd(file)
        .map(|fd| fd as i64)
        .unwrap_or_else(vfs_err)
}

pub fn sys_close(fd: u64) -> i64 {
    // F-NET-04: If the FD references a socket, emit close KDS event.
    if let Ok(file) = VfsContract::get_fd(fd as usize)
        && file.inode.ftype == crate::vfs::FileType::Socket
    {
        crate::net::socket::socket_close(file.inode.ino as usize);
    }
    VfsContract::close_fd(fd as usize)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_stat(path_ptr: u64, stat_ptr: u64) -> i64 {
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    match VfsContract::resolve(&path) {
        Ok(inode) => match inode.ops.stat() {
            Ok(s) => {
                unsafe { write_user(stat_ptr, s) };
                0
            }
            Err(e) => vfs_err(e),
        },
        Err(e) => vfs_err(e),
    }
}

pub fn sys_lstat(path_ptr: u64, stat_ptr: u64) -> i64 {
    sys_stat(path_ptr, stat_ptr)
}

pub fn sys_fstat(fd: u64, stat_ptr: u64) -> i64 {
    match VfsContract::stat_fd(fd as usize) {
        Ok(s) => {
            unsafe { write_user(stat_ptr, s) };
            0
        }
        Err(e) => vfs_err(e),
    }
}

pub fn sys_newfstatat(_dirfd: u64, path_ptr: u64, stat_ptr: u64, _flags: u64) -> i64 {
    sys_stat(path_ptr, stat_ptr)
}

pub fn sys_truncate(path_ptr: u64, len: u64) -> i64 {
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    VfsContract::truncate(&path, len)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_ftruncate(fd: u64, len: u64) -> i64 {
    VfsContract::truncate_fd(fd as usize, len)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_lseek(fd: u64, offset: u64, whence: u64) -> i64 {
    VfsContract::seek_fd(fd as usize, offset as i64, whence as u32)
        .map(|o| o as i64)
        .unwrap_or_else(vfs_err)
}

pub fn sys_pread64(fd: u64, buf_ptr: u64, len: u64, offset: u64) -> i64 {
    if buf_ptr < 0x1000 {
        return EFAULT;
    }
    let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, len as usize) };
    VfsContract::pread_fd(fd as usize, offset, buf)
        .map(|n| n as i64)
        .unwrap_or_else(vfs_err)
}

pub fn sys_pwrite64(fd: u64, buf_ptr: u64, len: u64, offset: u64) -> i64 {
    if buf_ptr < 0x1000 {
        return EFAULT;
    }
    let buf = unsafe { core::slice::from_raw_parts(buf_ptr as *const u8, len as usize) };
    VfsContract::pwrite_fd(fd as usize, offset, buf)
        .map(|n| n as i64)
        .unwrap_or_else(vfs_err)
}

pub fn sys_readv(fd: u64, iov_ptr: u64, iovcnt: u64) -> i64 {
    if iov_ptr < 0x1000 || iovcnt > 1024 {
        return EINVAL;
    }
    let mut total = 0i64;
    for i in 0..iovcnt as usize {
        let iov = unsafe { core::ptr::read_unaligned((iov_ptr + (i * 16) as u64) as *const Iovec) };
        if iov.len == 0 {
            continue;
        }
        let n = sys_read(fd, iov.base, iov.len);
        if n < 0 {
            return if total > 0 { total } else { n };
        }
        total += n;
        if (n as u64) < iov.len {
            break;
        }
    }
    total
}

pub fn sys_writev(fd: u64, iov_ptr: u64, iovcnt: u64) -> i64 {
    if iov_ptr < 0x1000 || iovcnt > 1024 {
        return EINVAL;
    }
    let mut total = 0i64;
    for i in 0..iovcnt as usize {
        let iov = unsafe { core::ptr::read_unaligned((iov_ptr + (i * 16) as u64) as *const Iovec) };
        if iov.len == 0 {
            continue;
        }
        let n = sys_write(fd, iov.base, iov.len);
        if n < 0 {
            return if total > 0 { total } else { n };
        }
        total += n;
    }
    total
}

pub fn sys_access(path_ptr: u64, _mode: u64) -> i64 {
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    VfsContract::resolve(&path)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_faccessat(_dirfd: u64, path_ptr: u64, mode: u64, _flags: u64) -> i64 {
    sys_access(path_ptr, mode)
}

pub fn sys_pipe(fds_ptr: u64) -> i64 {
    install_pipe_fds(fds_ptr, 0)
}

fn install_pipe_fds(fds_ptr: u64, fd_flags: u32) -> i64 {
    if fds_ptr == 0 {
        return EFAULT;
    }
    let (reader, writer) = match crate::ipc::pipe::try_create_pipe() {
        Ok(pair) => pair,
        Err(error) => return vfs_err(error),
    };
    let rfile = crate::vfs::file::OpenFile::new(reader, crate::vfs::file::O_RDONLY | fd_flags);
    let wfile = crate::vfs::file::OpenFile::new(writer, crate::vfs::file::O_WRONLY | fd_flags);

    match VfsContract::insert_fd_pair(rfile, wfile) {
        Ok((rfd, wfd)) => {
            unsafe {
                core::ptr::write_volatile(fds_ptr as *mut u32, rfd as u32);
                core::ptr::write_volatile((fds_ptr + 4) as *mut u32, wfd as u32);
            }
            0
        }
        Err(e) => vfs_err(e),
    }
}

pub fn sys_pipe2(fds: u64, flags: u64) -> i64 {
    const SUPPORTED_FLAGS: u64 = crate::vfs::file::O_CLOEXEC as u64;
    if flags & !SUPPORTED_FLAGS != 0 {
        return EINVAL;
    }
    install_pipe_fds(fds, flags as u32)
}

pub fn sys_dup(fd: u64) -> i64 {
    VfsContract::dup_fd(fd as usize)
        .map(|fd| fd as i64)
        .unwrap_or_else(vfs_err)
}

pub fn sys_dup2(oldfd: u64, newfd: u64) -> i64 {
    if oldfd == newfd {
        return oldfd as i64;
    }
    VfsContract::dup2_fd(oldfd as usize, newfd as usize)
        .map(|fd| fd as i64)
        .unwrap_or_else(vfs_err)
}

pub fn sys_dup3(oldfd: u64, newfd: u64, _flags: u64) -> i64 {
    sys_dup2(oldfd, newfd)
}

pub fn sys_fcntl(fd: u64, cmd: u64, _arg: u64) -> i64 {
    const F_GETFD: u64 = 1;
    const F_SETFD: u64 = 2;
    const F_GETFL: u64 = 3;
    const F_SETFL: u64 = 4;
    const F_DUPFD: u64 = 0;
    const F_DUPFD_CLOEXEC: u64 = 1030;
    match cmd {
        F_GETFD | F_SETFD | F_GETFL | F_SETFL => 0,
        F_DUPFD | F_DUPFD_CLOEXEC => sys_dup(fd),
        _ => 0,
    }
}

pub fn sys_sync_device() -> i64 {
    if let Some(dev) = crate::block::get() {
        match dev.flush() {
            Ok(()) => 0,
            Err(_) => EIO,
        }
    } else {
        0
    }
}

pub fn sys_fsync(_fd: u64) -> i64 {
    sys_sync_device()
}

pub fn sys_fdatasync(_fd: u64) -> i64 {
    sys_sync_device()
}

pub fn sys_getdents(fd: u64, buf: u64, count: u64) -> i64 {
    sys_getdents64(fd, buf, count)
}

pub fn sys_creat(path: u64, mode: u64) -> i64 {
    sys_open(path, (0o100 | 1 | 0o1000) as u64, mode)
}

pub fn sys_getdents64(fd: u64, buf_ptr: u64, bufsize: u64) -> i64 {
    match VfsContract::readdir_fd(fd as usize) {
        Ok((entries, file)) => {
            let entries = match Ok::<_, crate::vfs::VfsError>(entries) {
                Ok(e) => e,
                Err(e) => return vfs_err(e),
            };

            let mut written = 0usize;
            let mut consumed = 0u64;
            for (i, entry) in entries.iter().enumerate() {
                let name_bytes = entry.name.as_bytes();
                let reclen =
                    (core::mem::size_of::<LinuxDirent64>() + name_bytes.len() + 1).div_ceil(8) * 8;
                if written + reclen > bufsize as usize {
                    break;
                }

                let dtype: u8 = match entry.ftype {
                    FileType::RegularFile => 8,
                    FileType::Directory => 4,
                    FileType::SymLink => 10,
                    FileType::CharDevice => 2,
                    FileType::BlockDevice => 6,
                    FileType::Pipe => 1,
                    FileType::Socket => 12,
                };

                let dirent = LinuxDirent64 {
                    d_ino: entry.inode,
                    d_off: (i + 1) as u64,
                    d_reclen: reclen as u16,
                    d_type: dtype,
                };
                unsafe {
                    let dst = (buf_ptr + written as u64) as *mut u8;
                    core::ptr::copy_nonoverlapping(
                        &dirent as *const _ as *const u8,
                        dst,
                        core::mem::size_of::<LinuxDirent64>(),
                    );
                    let name_dst = dst.add(core::mem::size_of::<LinuxDirent64>());
                    core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), name_dst, name_bytes.len());
                    *name_dst.add(name_bytes.len()) = 0;
                }
                written += reclen;
                consumed = (i + 1) as u64;
            }
            file.offset
                .fetch_add(consumed, core::sync::atomic::Ordering::Relaxed);
            written as i64
        }
        Err(e) => vfs_err(e),
    }
}

pub fn sys_getcwd(buf_ptr: u64, size: u64) -> i64 {
    if buf_ptr < 0x1000 {
        return EFAULT;
    }
    let cwd = current_cwd();
    let bytes = cwd.as_bytes();
    if bytes.len() + 1 > size as usize {
        return ERANGE;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, bytes.len());
        *(buf_ptr as *mut u8).add(bytes.len()) = 0;
    }
    buf_ptr as i64
}

pub fn sys_chdir(path_ptr: u64) -> i64 {
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    match VfsContract::resolve(&path) {
        Ok(inode) => {
            if inode.ftype != FileType::Directory {
                return ENOTDIR;
            }
            let _ = process::with_current_process_mut(|p| p.cwd = path.clone());
            0
        }
        Err(e) => vfs_err(e),
    }
}

pub fn sys_fchdir(fd: u64) -> i64 {
    match VfsContract::get_fd(fd as usize).map_err(vfs_err) {
        Ok(f) if f.inode.ftype == FileType::Directory => 0,
        Ok(_) => ENOTDIR,
        Err(e) => e,
    }
}

pub fn sys_mkdir(path_ptr: u64, mode: u64) -> i64 {
    sys_mkdirat(!0u64, path_ptr, mode)
}

pub fn sys_mkdirat(_dirfd: u64, path_ptr: u64, mode: u64) -> i64 {
    enforce_vfs_contract(crate::vfs_contract::VfsOperation::Create, "sys_mkdirat");
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    VfsContract::mkdir(&path, mode as u32)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_mknodat(_dirfd: u64, _path: u64, _mode: u64, _dev: u64) -> i64 {
    ENOSYS
}

pub fn sys_rmdir(path_ptr: u64) -> i64 {
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    VfsContract::rmdir(&path).map(|_| 0).unwrap_or_else(vfs_err)
}

pub fn sys_unlink(path_ptr: u64) -> i64 {
    sys_unlinkat(!0u64, path_ptr, 0)
}

pub fn sys_unlinkat(_dirfd: u64, path_ptr: u64, _flags: u64) -> i64 {
    enforce_vfs_contract(crate::vfs_contract::VfsOperation::Unlink, "sys_unlinkat");
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    VfsContract::unlink(&path)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_rename(old_ptr: u64, new_ptr: u64) -> i64 {
    enforce_vfs_contract(crate::vfs_contract::VfsOperation::Rename, "sys_rename");
    let old = unsafe {
        match read_user_str(old_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    let new = unsafe {
        match read_user_str(new_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    VfsContract::rename(&old, &new)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_renameat(_old_dir: u64, old_ptr: u64, _new_dir: u64, new_ptr: u64) -> i64 {
    sys_rename(old_ptr, new_ptr)
}

pub fn sys_renameat2(_old_dir: u64, old: u64, _new_dir: u64, new: u64, _flags: u64) -> i64 {
    sys_rename(old, new)
}

pub fn sys_link(old_ptr: u64, new_ptr: u64) -> i64 {
    let old = unsafe {
        match read_user_str(old_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    let new = unsafe {
        match read_user_str(new_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    let target = match VfsContract::resolve(&old) {
        Ok(i) => i,
        Err(e) => return vfs_err(e),
    };
    VfsContract::link(&new, &target)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_symlink(target_ptr: u64, link_ptr: u64) -> i64 {
    let target = unsafe {
        match read_user_str(target_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    let link = unsafe {
        match read_user_str(link_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    VfsContract::symlink(&link, &target)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_readlink(path_ptr: u64, buf_ptr: u64, bufsiz: u64) -> i64 {
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    let inode = match VfsContract::resolve(&path) {
        Ok(i) => i,
        Err(e) => return vfs_err(e),
    };
    match inode.ops.readlink() {
        Ok(target) => {
            let bytes = target.as_bytes();
            let n = bytes.len().min(bufsiz as usize);
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr as *mut u8, n);
            }
            n as i64
        }
        Err(e) => vfs_err(e),
    }
}

pub fn sys_chmod(path_ptr: u64, mode: u64) -> i64 {
    enforce_vfs_contract(crate::vfs_contract::VfsOperation::Chmod, "sys_chmod");
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    VfsContract::chmod(&path, mode as u32)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_fchmod(fd: u64, mode: u64) -> i64 {
    VfsContract::chmod_fd(fd as usize, mode as u32)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_chown(path_ptr: u64, uid: u64, gid: u64) -> i64 {
    enforce_vfs_contract(crate::vfs_contract::VfsOperation::Chown, "sys_chown");
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => return EFAULT,
        }
    };
    VfsContract::chown(&path, uid as u32, gid as u32)
        .map(|_| 0)
        .unwrap_or_else(vfs_err)
}

pub fn sys_fchown(_fd: u64, _uid: u64, _gid: u64) -> i64 {
    ENOSYS
}

pub fn sys_lchown(path: u64, uid: u64, gid: u64) -> i64 {
    sys_chown(path, uid, gid)
}

pub fn sys_fchownat(_: u64, p: u64, u: u64, g: u64, _: u64) -> i64 {
    sys_chown(p, u, g)
}

pub fn sys_fchmodat(_: u64, p: u64, m: u64, _: u64) -> i64 {
    sys_chmod(p, m)
}

pub fn sys_umask(_mask: u64) -> i64 {
    0o22
}

pub fn sys_statfs(_path_ptr: u64, buf_ptr: u64) -> i64 {
    if buf_ptr < 0x1000 {
        return EFAULT;
    }
    let fs = Statfs {
        f_type: 0xEF53,
        f_bsize: 4096,
        f_blocks: 100000,
        f_bfree: 50000,
        f_bavail: 50000,
        f_files: 100000,
        f_ffree: 90000,
        f_fsid: [0; 2],
        f_namelen: 255,
        f_frsize: 4096,
        f_flags: 0,
        f_spare: [0; 4],
    };
    unsafe {
        core::ptr::write_volatile(buf_ptr as *mut Statfs, fs);
    }
    0
}

pub fn sys_fstatfs(_fd: u64, buf_ptr: u64) -> i64 {
    sys_statfs(0, buf_ptr)
}

pub fn sys_close_range(first: u64, last: u64, _flags: u64) -> i64 {
    for fd in first..=last {
        let _ = VfsContract::close_fd(fd as usize);
    }
    0
}
