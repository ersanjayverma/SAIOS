//! BSD socket API — simplified implementation without downcast.

use crate::vfs::file::OpenFile;
use crate::vfs::{
    DirEntry, FileType, Inode as VfsInode, InodeOps, Stat, VfsError, VfsResult, alloc_ino,
};
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

pub const AF_INET: u64 = 2;
pub const AF_UNIX: u64 = 1;
pub const SOCK_STREAM: u64 = 1;
pub const SOCK_DGRAM: u64 = 2;

#[repr(C, packed)]
struct SockaddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

// -- Global socket state table ----------------------------------------------

enum SockState {
    Unbound,
    Connected {
        src_port: u16,
        dst_ip: [u8; 4],
        dst_port: u16,
    },
}

struct SockEntry {
    domain: u64,
    stype: u64,
    state: SockState,
}

static SOCKETS: Mutex<BTreeMap<usize, SockEntry>> = Mutex::new(BTreeMap::new());

// -- Socket inode (placeholder for the fd) ---------------------------------

struct SocketInode(u64);

impl InodeOps for SocketInode {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: self.0,
            st_mode: FileType::Socket.mode_bits() | 0o600,
            st_nlink: 1,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let socks = SOCKETS.lock();
        if let Some(s) = socks.get(&(self.0 as usize))
            && let SockState::Connected {
                src_port,
                dst_ip,
                dst_port,
            } = &s.state
        {
            let data = crate::net::tcp::read(*src_port, *dst_ip, *dst_port);
            if data.is_empty() {
                return Err(VfsError::WouldBlock);
            }
            let n = data.len().min(buf.len());
            buf[..n].copy_from_slice(&data[..n]);
            return Ok(n);
        }
        Err(VfsError::WouldBlock)
    }
    fn write(&self, _: u64, buf: &[u8]) -> VfsResult<usize> {
        let socks = SOCKETS.lock();
        if let Some(s) = socks.get(&(self.0 as usize))
            && let SockState::Connected {
                src_port,
                dst_ip,
                dst_port,
            } = &s.state
        {
            crate::net::tcp::write(*src_port, *dst_ip, *dst_port, buf);
            return Ok(buf.len());
        }
        Err(VfsError::Io)
    }
    fn readdir(&self, _: u64) -> VfsResult<Vec<DirEntry>> {
        Err(VfsError::NotADir)
    }
    fn lookup(&self, _: &str) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::NotADir)
    }
    fn create(&self, _: &str, _: FileType, _: u32) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn mkdir(&self, _: &str, _: u32) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn unlink(&self, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn rmdir(&self, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn truncate(&self, _: u64) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn chmod(&self, _: u32) -> VfsResult<()> {
        Ok(())
    }
    fn chown(&self, _: u32, _: u32) -> VfsResult<()> {
        Ok(())
    }
    fn symlink(&self, _: &str, _: &str) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn link(&self, _: &str, _: &Arc<VfsInode>) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn rename(&self, _: &str, _: &Arc<VfsInode>, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
}

/// F-NET-04: Close a socket and emit per-connection KDS event.
/// Called from sys_close when the FD references a socket inode.
pub fn socket_close(id: usize) {
    let entry = SOCKETS.lock().remove(&id);
    if let Some(s) = entry
        && let SockState::Connected {
            src_port,
            dst_ip,
            dst_port,
        } = s.state
    {
        crate::network_contract::NetworkContract::record_socket_close(
            id, src_port, dst_ip, dst_port,
        );
    }
}

// -- syscall API ------------------------------------------------------------

pub fn sys_socket(domain: u64, stype: u64, _proto: u64) -> i64 {
    let id = alloc_ino();
    let socket_type = stype & 0xFF;
    SOCKETS.lock().insert(
        id as usize,
        SockEntry {
            domain,
            stype: socket_type,
            state: SockState::Unbound,
        },
    );
    crate::network_contract::NetworkContract::record_socket_create(
        id as usize,
        domain,
        socket_type,
    );
    let inode = VfsInode::new(id, FileType::Socket, Arc::new(SocketInode(id)));
    let file = OpenFile::new(inode, 0);
    crate::vfs_contract::VfsContract::insert_fd(file)
        .map(|fd| fd as i64)
        .unwrap_or_else(|e| e.to_errno())
}

fn socket_id_for_fd(fd: u64) -> Option<usize> {
    crate::vfs_contract::VfsContract::get_fd(fd as usize)
        .ok()
        .map(|file| file.inode.ino as usize)
}

pub fn sys_bind(fd: u64, addr_ptr: u64, _addrlen: u64) -> i64 {
    if addr_ptr == 0 {
        return -14;
    }
    let sa = unsafe { core::ptr::read_unaligned(addr_ptr as *const SockaddrIn) };
    let port = u16::from_be(sa.sin_port);
    let addr = u32::from_be(sa.sin_addr);
    let ip = addr.to_be_bytes();
    let ip = if ip == [0, 0, 0, 0] {
        crate::network_contract::NetworkContract::ip()
    } else {
        ip
    };
    // Just record the bind — no actual port reservation yet
    if let Some(id) = socket_id_for_fd(fd) {
        crate::network_contract::NetworkContract::record_socket_bind(id, ip, port);
    }
    0
}

pub fn sys_listen(_fd: u64, _backlog: u64) -> i64 {
    0
}

pub fn sys_connect(fd: u64, addr_ptr: u64, _addrlen: u64) -> i64 {
    if addr_ptr == 0 {
        return -14;
    }
    let sa = unsafe { core::ptr::read_unaligned(addr_ptr as *const SockaddrIn) };
    let port = u16::from_be(sa.sin_port);
    let addr = u32::from_be(sa.sin_addr);
    let ip = addr.to_be_bytes();

    if let Some(id) = socket_id_for_fd(fd) {
        let mut socks = SOCKETS.lock();
        if let Some(s) = socks.get_mut(&id)
            && s.stype == SOCK_STREAM
        {
            let src_port = crate::net::tcp::open(ip, port);
            s.state = SockState::Connected {
                src_port,
                dst_ip: ip,
                dst_port: port,
            };
            crate::network_contract::NetworkContract::record_socket_connect(id, src_port, ip, port);
            return 0;
        }
    }
    crate::network_contract::NetworkContract::record_socket_failure(fd as usize, 111, 0);
    -111 // ECONNREFUSED
}

pub fn sys_accept(_fd: u64, _addr: u64, _addrlen: u64) -> i64 {
    -11
} // EAGAIN

pub fn sys_sendto(fd: u64, buf: u64, len: u64, _flags: u64, _addr: u64, _: u64) -> i64 {
    crate::syscall::handlers::sys_write(fd, buf, len)
}

pub fn sys_recvfrom(fd: u64, buf: u64, len: u64, _flags: u64, _addr: u64, _: u64) -> i64 {
    crate::syscall::handlers::sys_read(fd, buf, len)
}
