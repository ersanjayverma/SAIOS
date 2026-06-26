//! Anonymous pipes — unidirectional byte stream between two file descriptors.

use crate::vfs::{
    DirEntry, FileType, Inode as VfsInode, InodeOps, Stat, VfsError, VfsResult, alloc_ino,
};
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

const PIPE_BUF: usize = crate::ipc_contract::IpcContract::ANONYMOUS_PIPE_BUFFER_SIZE as usize;

pub struct PipeBuffer {
    buf: VecDeque<u8>,
    readers: usize,
    writers: usize,
}

impl PipeBuffer {
    /// Create a bare PipeBuffer (for unix_socket bidirectional use).
    pub fn new() -> Self {
        Self {
            buf: VecDeque::with_capacity(4096),
            readers: 1,
            writers: 1,
        }
    }

    fn new_arc() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self::new()))
    }

    /// Read up to buf.len() bytes from the pipe. Returns bytes read.
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.buf.len());
        for byte in buf.iter_mut().take(n) {
            *byte = self.buf.pop_front().unwrap_or(0);
        }
        n
    }

    /// Write data into the pipe buffer. Returns bytes written.
    pub fn write(&mut self, data: &[u8]) -> usize {
        let space = 65536usize.saturating_sub(self.buf.len());
        let n = data.len().min(space);
        for &byte in &data[..n] {
            self.buf.push_back(byte);
        }
        n
    }
}

// -- Read end ---------------------------------------------------------------

pub struct PipeReader(Arc<Mutex<PipeBuffer>>);

impl Drop for PipeReader {
    fn drop(&mut self) {
        let mut pipe = self.0.lock();
        pipe.readers = pipe.readers.saturating_sub(1);
    }
}

impl InodeOps for PipeReader {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: alloc_ino(),
            st_mode: FileType::Pipe.mode_bits() | 0o600,
            st_nlink: 1,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, buf: &mut [u8]) -> VfsResult<usize> {
        loop {
            let mut pipe = self.0.lock();
            if !pipe.buf.is_empty() {
                let n = buf.len().min(pipe.buf.len());
                for b in &mut buf[..n] {
                    *b = pipe.buf.pop_front().unwrap();
                }
                return Ok(n);
            }
            if pipe.writers == 0 {
                return Ok(0);
            } // EOF
            drop(pipe);
            x86_64::instructions::hlt(); // wait for writer
        }
    }
    fn write(&self, _: u64, _: &[u8]) -> VfsResult<usize> {
        Err(VfsError::BadFd)
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

// -- Write end --------------------------------------------------------------

pub struct PipeWriter(Arc<Mutex<PipeBuffer>>);

impl Drop for PipeWriter {
    fn drop(&mut self) {
        let mut pipe = self.0.lock();
        pipe.writers = pipe.writers.saturating_sub(1);
    }
}

impl InodeOps for PipeWriter {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: alloc_ino(),
            st_mode: FileType::Pipe.mode_bits() | 0o600,
            st_nlink: 1,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, _: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::BadFd)
    }
    fn write(&self, _: u64, buf: &[u8]) -> VfsResult<usize> {
        let mut pipe = self.0.lock();
        if pipe.readers == 0 {
            drop(pipe);
            crate::ipc::signal::raise_signal(crate::ipc::signal::SIGPIPE);
            return Err(VfsError::BrokenPipe);
        }
        if pipe.buf.len() + buf.len() > PIPE_BUF {
            return Err(VfsError::WouldBlock);
        }
        pipe.buf.extend(buf.iter().copied());
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

/// Create a (read_inode, write_inode) pipe pair.
pub fn create_pipe() -> (Arc<VfsInode>, Arc<VfsInode>) {
    try_create_pipe().unwrap_or_else(|_| create_pipe_unaccounted())
}

pub fn try_create_pipe() -> VfsResult<(Arc<VfsInode>, Arc<VfsInode>)> {
    let _grant =
        crate::ipc_contract::IpcContract::create_anonymous_pipe().map_err(|_| VfsError::NoSpace)?;

    Ok(create_pipe_unaccounted())
}

fn create_pipe_unaccounted() -> (Arc<VfsInode>, Arc<VfsInode>) {
    let buf = PipeBuffer::new_arc();
    let reader = VfsInode::new(
        alloc_ino(),
        FileType::Pipe,
        Arc::new(PipeReader(buf.clone())),
    );
    let writer = VfsInode::new(alloc_ino(), FileType::Pipe, Arc::new(PipeWriter(buf)));
    crate::kds::kds_event(
        crate::kds::KdsSubsystem::Ipc,
        crate::kds::KdsEventType::IpcPipeCreate,
        crate::kds::KdsSeverity::Trace,
        [crate::process::current_pid().unwrap_or(0) as u64, 0, 0, 0],
    );
    (reader, writer)
}
