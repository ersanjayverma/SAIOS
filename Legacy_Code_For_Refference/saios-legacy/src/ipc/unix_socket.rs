//! Unix domain socket (AF_UNIX) — socketpair implementation.
//!
//! F-IPC-01: Provides bidirectional local IPC via `socketpair(AF_UNIX, SOCK_STREAM, 0)`.
//! Each end can read what the other writes (two crossed pipes internally).

use alloc::collections::BTreeSet;
use alloc::sync::Arc;
use spin::Mutex;

use crate::ipc::pipe::PipeBuffer;
use crate::vfs::{
    self, DirEntry, FileType, Inode as VfsInode, InodeOps, Stat, VfsError, VfsResult,
};

/// One end of a Unix domain socket pair.
struct UnixSockEnd {
    /// Read from this buffer (peer writes here).
    rx: Arc<Mutex<PipeBuffer>>,
    /// Write to this buffer (peer reads from here).
    tx: Arc<Mutex<PipeBuffer>>,
}

struct UnixSockOps(Mutex<UnixSockEnd>);

lazy_static::lazy_static! {
    static ref SOCKETPAIR_INODES: Mutex<BTreeSet<u64>> = Mutex::new(BTreeSet::new());
}

pub fn is_socketpair_inode(ino: u64) -> bool {
    SOCKETPAIR_INODES.lock().contains(&ino)
}

impl InodeOps for UnixSockOps {
    fn read(&self, _offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let end = self.0.lock();
        let mut rx = end.rx.lock();
        let n = rx.read(buf);
        if n == 0 && buf.is_empty() {
            return Ok(0);
        }
        Ok(n)
    }

    fn write(&self, _offset: u64, data: &[u8]) -> VfsResult<usize> {
        let end = self.0.lock();
        let mut tx = end.tx.lock();
        let n = tx.write(data);
        Ok(n)
    }

    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: 0,
            st_mode: FileType::Socket.mode_bits() | 0o600,
            st_nlink: 1,
            ..Default::default()
        })
    }

    fn readdir(&self, _: u64) -> VfsResult<alloc::vec::Vec<DirEntry>> {
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
        Ok(())
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

/// Create a Unix domain socket pair. Returns two inodes (one per end).
/// Each end reads what the other writes.
pub fn create_pair() -> (Arc<VfsInode>, Arc<VfsInode>) {
    let buf_a = Arc::new(Mutex::new(PipeBuffer::new()));
    let buf_b = Arc::new(Mutex::new(PipeBuffer::new()));

    let end_0 = UnixSockEnd {
        rx: buf_a.clone(),
        tx: buf_b.clone(),
    };
    let end_1 = UnixSockEnd {
        rx: buf_b,
        tx: buf_a,
    };

    let ino0 = vfs::alloc_ino();
    let ino1 = vfs::alloc_ino();
    {
        let mut inodes = SOCKETPAIR_INODES.lock();
        inodes.insert(ino0);
        inodes.insert(ino1);
    }

    let inode0 = VfsInode::new(
        ino0,
        FileType::Socket,
        Arc::new(UnixSockOps(Mutex::new(end_0))),
    );
    let inode1 = VfsInode::new(
        ino1,
        FileType::Socket,
        Arc::new(UnixSockOps(Mutex::new(end_1))),
    );

    (inode0, inode1)
}
