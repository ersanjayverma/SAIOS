use super::*;

pub fn sys_socket(dom: u64, typ: u64, proto: u64) -> i64 {
    crate::net::socket::sys_socket(dom, typ, proto)
}

pub fn sys_connect(fd: u64, addr: u64, addrlen: u64) -> i64 {
    crate::net::socket::sys_connect(fd, addr, addrlen)
}

pub fn sys_accept(fd: u64, addr: u64, addrlen: u64) -> i64 {
    crate::net::socket::sys_accept(fd, addr, addrlen)
}

pub fn sys_sendto(fd: u64, buf: u64, len: u64, flags: u64, addr: u64, addrlen: u64) -> i64 {
    crate::net::socket::sys_sendto(fd, buf, len, flags, addr, addrlen)
}

pub fn sys_recvfrom(fd: u64, buf: u64, len: u64, flags: u64, addr: u64, addrlen: u64) -> i64 {
    crate::net::socket::sys_recvfrom(fd, buf, len, flags, addr, addrlen)
}

pub fn sys_sendmsg(_fd: u64, _msg: u64, _flags: u64) -> i64 {
    ENOSYS
}

pub fn sys_recvmsg(_fd: u64, _msg: u64, _flags: u64) -> i64 {
    ENOSYS
}

pub fn sys_shutdown(_fd: u64, _how: u64) -> i64 {
    ENOSYS
}

pub fn sys_bind(fd: u64, addr: u64, addrlen: u64) -> i64 {
    crate::net::socket::sys_bind(fd, addr, addrlen)
}

pub fn sys_listen(fd: u64, backlog: u64) -> i64 {
    crate::net::socket::sys_listen(fd, backlog)
}

pub fn sys_getsockname(_fd: u64, _addr: u64, _addrlen: u64) -> i64 {
    write_unix_socketpair_name(_fd, _addr, _addrlen)
}

pub fn sys_getpeername(_fd: u64, _addr: u64, _addrlen: u64) -> i64 {
    write_unix_socketpair_name(_fd, _addr, _addrlen)
}

pub fn sys_socketpair(dom: u64, typ: u64, proto: u64, sv: u64) -> i64 {
    const SOCK_TYPE_MASK: u64 = 0xF;
    const SOCK_CLOEXEC: u64 = crate::vfs::file::O_CLOEXEC as u64;
    const SUPPORTED_FLAGS: u64 = SOCK_CLOEXEC;

    if dom != crate::net::socket::AF_UNIX {
        return ENOSYS;
    }
    if proto != 0 {
        return EINVAL;
    }
    let socket_type = typ & SOCK_TYPE_MASK;
    let flags = typ & !SOCK_TYPE_MASK;
    if socket_type != crate::net::socket::SOCK_STREAM {
        return ENOSYS;
    }
    if flags & !SUPPORTED_FLAGS != 0 {
        return EINVAL;
    }
    if sv == 0 {
        return EFAULT;
    }
    let (inode0, inode1) = crate::ipc::unix_socket::create_pair();
    let fd_flags = crate::vfs::file::O_RDWR | flags as u32;
    let file0 = crate::vfs::file::OpenFile::new(inode0, fd_flags);
    let file1 = crate::vfs::file::OpenFile::new(inode1, fd_flags);
    let (fd0, fd1) = match crate::vfs_contract::VfsContract::insert_fd_pair(file0, file1) {
        Ok(pair) => pair,
        Err(error) => return error.to_errno(),
    };
    unsafe {
        let ptr = sv as *mut [i32; 2];
        (*ptr) = [fd0 as i32, fd1 as i32];
    }
    0
}

fn write_unix_socketpair_name(fd: u64, addr: u64, addrlen: u64) -> i64 {
    const ENOTSOCK: i64 = -88;
    const UNNAMED_AF_UNIX_LEN: u32 = 2;

    if addr == 0 || addrlen == 0 {
        return EFAULT;
    }
    let file = match crate::vfs_contract::VfsContract::get_fd(fd as usize) {
        Ok(file) => file,
        Err(error) => return error.to_errno(),
    };
    if file.inode.ftype != crate::vfs::FileType::Socket {
        return ENOTSOCK;
    }
    if !crate::ipc::unix_socket::is_socketpair_inode(file.inode.ino) {
        return ENOSYS;
    }
    let provided_len = unsafe { core::ptr::read_volatile(addrlen as *const u32) };
    unsafe {
        core::ptr::write_volatile(addrlen as *mut u32, UNNAMED_AF_UNIX_LEN);
    }
    if provided_len < UNNAMED_AF_UNIX_LEN {
        return EINVAL;
    }
    unsafe {
        core::ptr::write_volatile(addr as *mut u16, crate::net::socket::AF_UNIX as u16);
    }
    0
}

pub fn sys_setsockopt(_fd: u64, _lev: u64, _opt: u64, _val: u64, _len: u64) -> i64 {
    ENOSYS
}

pub fn sys_getsockopt(_fd: u64, _lev: u64, _opt: u64, _val: u64, _len: u64) -> i64 {
    ENOSYS
}
