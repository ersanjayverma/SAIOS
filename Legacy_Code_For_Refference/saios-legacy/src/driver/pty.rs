//! PTY (pseudo-terminal) — master/slave pairs for terminal emulation.
//!
//! The master side is held by the terminal emulator (shell).
//! The slave side is given to the child process as its controlling terminal.
//!
//! Data flow:
//!   write(master) → read(slave)   [master sends input to child]
//!   write(slave)  → read(master)  [child's output goes to master / screen]

use crate::vfs::file::OpenFile;
use crate::vfs::{
    DirEntry, FileType, Inode as VfsInode, InodeOps, Stat, VfsError, VfsResult, alloc_ino,
};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

static NEXT_PTY_NUM: AtomicU32 = AtomicU32::new(0);

const PTY_BUF: usize = 4096;

// -- Shared pipe between master and slave -----------------------------------

pub(crate) struct PtyBuf {
    master_to_slave: VecDeque<u8>, // master writes, slave reads
    slave_to_master: VecDeque<u8>, // slave writes, master reads
    cols: u16,
    rows: u16,
}

impl PtyBuf {
    fn new() -> Self {
        Self {
            master_to_slave: VecDeque::new(),
            slave_to_master: VecDeque::new(),
            cols: 80,
            rows: 25,
        }
    }
}

// -- Master -----------------------------------------------------------------

struct PtyMaster {
    ino: u64,
    num: u32,
    buf: Arc<Mutex<PtyBuf>>,
}

impl InodeOps for PtyMaster {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: self.ino,
            st_mode: FileType::CharDevice.mode_bits() | 0o620,
            st_nlink: 1,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, buf: &mut [u8]) -> VfsResult<usize> {
        // Read child output (slave→master)
        loop {
            let mut b = self.buf.lock();
            if !b.slave_to_master.is_empty() {
                let n = buf.len().min(b.slave_to_master.len());
                for byte in &mut buf[..n] {
                    *byte = b.slave_to_master.pop_front().unwrap();
                }
                return Ok(n);
            }
            drop(b);
            x86_64::instructions::hlt();
        }
    }
    fn write(&self, _: u64, buf: &[u8]) -> VfsResult<usize> {
        // Send to child (master→slave)
        let mut b = self.buf.lock();
        let generate_signals = crate::tty::io::is_signal_generation_enabled();
        // Apply line discipline: echo back, handle Ctrl+C etc.
        for &byte in buf {
            if generate_signals {
                match byte {
                    3 => {
                        let _ = crate::tty::io::signal_fg_pgid(crate::ipc::signal::SIGINT);
                        continue;
                    }
                    26 => {
                        let _ = crate::tty::io::signal_fg_pgid(crate::ipc::signal::SIGTSTP);
                        continue;
                    }
                    _ => {}
                }
            }
            if b.master_to_slave.len() < PTY_BUF {
                b.master_to_slave.push_back(byte);
            }
            // Echo back
            if b.slave_to_master.len() < PTY_BUF {
                b.slave_to_master.push_back(byte);
            }
        }
        Ok(buf.len())
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

// -- Slave ------------------------------------------------------------------

struct PtySlave {
    ino: u64,
    num: u32,
    buf: Arc<Mutex<PtyBuf>>,
}

impl InodeOps for PtySlave {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: self.ino,
            st_mode: FileType::CharDevice.mode_bits() | 0o620,
            st_nlink: 1,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, buf: &mut [u8]) -> VfsResult<usize> {
        loop {
            let mut b = self.buf.lock();
            if !b.master_to_slave.is_empty() {
                let n = buf.len().min(b.master_to_slave.len());
                for byte in &mut buf[..n] {
                    *byte = b.master_to_slave.pop_front().unwrap();
                }
                return Ok(n);
            }
            drop(b);
            x86_64::instructions::hlt();
        }
    }
    fn write(&self, _: u64, buf: &[u8]) -> VfsResult<usize> {
        let mut b = self.buf.lock();
        for &byte in buf {
            if b.slave_to_master.len() < PTY_BUF {
                b.slave_to_master.push_back(byte);
            }
        }
        Ok(buf.len())
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

// -- Public API -------------------------------------------------------------

/// Open a new PTY pair. Returns (master_fd, slave_fd) inserted into the process's fd table.
pub fn openpty(proc: &mut crate::process::Process) -> VfsResult<(usize, usize)> {
    let num = NEXT_PTY_NUM.fetch_add(1, Ordering::Relaxed);
    let buf = Arc::new(Mutex::new(PtyBuf::new()));

    let master_ino = alloc_ino();
    let slave_ino = alloc_ino();

    let master_node = VfsInode::new(
        master_ino,
        FileType::CharDevice,
        Arc::new(PtyMaster {
            ino: master_ino,
            num,
            buf: buf.clone(),
        }),
    );
    let slave_node = VfsInode::new(
        slave_ino,
        FileType::CharDevice,
        Arc::new(PtySlave {
            ino: slave_ino,
            num,
            buf,
        }),
    );

    // Register slave in /dev/pts/N
    let pts_path = alloc::format!("/dev/pts/{}", num);
    let _ = crate::vfs_contract::VfsContract::link(&pts_path, &slave_node);

    let (master_fd, slave_fd) = crate::vfs_contract::VfsContract::insert_fd_pair_for_process(
        proc,
        OpenFile::new(master_node, 0o2),
        OpenFile::new(slave_node, 0o2),
    )?;

    crate::println!(
        "[pty] /dev/pts/{} opened (master={}, slave={})",
        num,
        master_fd,
        slave_fd
    );
    Ok((master_fd, slave_fd))
}

/// Get/set window size for a PTY fd.
pub fn tiocgwinsz(buf: Arc<Mutex<PtyBuf>>) -> (u16, u16) {
    let b = buf.lock();
    (b.rows, b.cols)
}

pub fn tiocswinsz(buf: Arc<Mutex<PtyBuf>>, rows: u16, cols: u16) {
    let mut b = buf.lock();
    b.rows = rows;
    b.cols = cols;
}
