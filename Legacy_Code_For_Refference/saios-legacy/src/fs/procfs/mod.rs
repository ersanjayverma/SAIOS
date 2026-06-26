//! procfs â€” /proc virtual filesystem (Linux-compatible).

use crate::vfs::{
    self, DirEntry, FileType, Inode as VfsInode, InodeOps, Stat, VfsError, VfsResult, alloc_ino,
};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

struct ProcFile {
    ino: u64,
    name: String,
    content_fn: fn() -> Vec<u8>,
}

impl InodeOps for ProcFile {
    fn stat(&self) -> VfsResult<Stat> {
        let content = (self.content_fn)();
        Ok(Stat {
            st_ino: self.ino,
            st_mode: FileType::RegularFile.mode_bits() | 0o444,
            st_nlink: 1,
            st_size: content.len() as i64,
            st_blksize: 4096,
            ..Default::default()
        })
    }
    fn read(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let data = (self.content_fn)();
        let off = offset as usize;
        if off >= data.len() {
            return Ok(0);
        }
        let n = buf.len().min(data.len() - off);
        buf[..n].copy_from_slice(&data[off..off + n]);
        Ok(n)
    }
    fn write(&self, _o: u64, _b: &[u8]) -> VfsResult<usize> {
        Err(VfsError::PermDenied)
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

struct ProcDir {
    ino: u64,
    children: Vec<(String, Arc<VfsInode>)>,
}

impl InodeOps for ProcDir {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: self.ino,
            st_mode: FileType::Directory.mode_bits() | 0o555,
            st_nlink: 2,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, _: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::IsDir)
    }
    fn write(&self, _: u64, _: &[u8]) -> VfsResult<usize> {
        Err(VfsError::IsDir)
    }
    fn readdir(&self, _: u64) -> VfsResult<Vec<DirEntry>> {
        Ok(self
            .children
            .iter()
            .map(|(name, inode)| DirEntry {
                name: name.clone(),
                inode: inode.ino,
                ftype: inode.ftype,
            })
            .collect())
    }
    fn lookup(&self, name: &str) -> VfsResult<Arc<VfsInode>> {
        self.children
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .ok_or(VfsError::NotFound)
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

struct ProcFsDriver;

impl vfs::FileSystemDriver for ProcFsDriver {
    fn fs_type(&self) -> &'static str {
        "proc"
    }

    fn mount(&self, request: &vfs::MountRequest) -> Result<Arc<VfsInode>, &'static str> {
        match request.source {
            vfs::MountSource::None => Ok(build_root_inode()),
            _ => Err("procfs: mount source not supported"),
        }
    }
}

pub fn register_driver() -> Result<(), &'static str> {
    match vfs::register_filesystem(Arc::new(ProcFsDriver)) {
        Ok(()) | Err(VfsError::AlreadyExists) => Ok(()),
        Err(_) => Err("procfs: failed to register driver"),
    }
}

// -- Per-process /proc/<pid> nodes -------------------------------------------

/// A synthesized file under /proc/<pid> (status / cmdline / stat).
struct ProcPidFile {
    ino: u64,
    pid: u32,
    kind: u8,
}

fn pid_file_content(pid: u32, kind: u8) -> Vec<u8> {
    let name = crate::process::table::TABLE
        .lock()
        .name_of(pid)
        .unwrap_or_default();
    match kind {
        1 => format!("{}\0", name).into_bytes(), // cmdline
        2 => format!("{} ({}) R 1 1 0 0 -1 0 0 0 0 0 0 0\n", pid, name).into_bytes(), // stat
        _ => format!(
            "Name:\t{}\nPid:\t{}\nPPid:\t1\nState:\tR (running)\n\
                      Uid:\t0 0 0 0\nGid:\t0 0 0 0\nThreads:\t1\n",
            name, pid
        )
        .into_bytes(),
    }
}

impl InodeOps for ProcPidFile {
    fn stat(&self) -> VfsResult<Stat> {
        let c = pid_file_content(self.pid, self.kind);
        Ok(Stat {
            st_ino: self.ino,
            st_mode: FileType::RegularFile.mode_bits() | 0o444,
            st_nlink: 1,
            st_size: c.len() as i64,
            st_blksize: 4096,
            ..Default::default()
        })
    }
    fn read(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let data = pid_file_content(self.pid, self.kind);
        let off = offset as usize;
        if off >= data.len() {
            return Ok(0);
        }
        let n = buf.len().min(data.len() - off);
        buf[..n].copy_from_slice(&data[off..off + n]);
        Ok(n)
    }
    fn write(&self, _: u64, _: &[u8]) -> VfsResult<usize> {
        Err(VfsError::PermDenied)
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

/// /proc/<pid> directory.
struct ProcPidDir {
    ino: u64,
    pid: u32,
}

impl ProcPidDir {
    fn file(&self, name: &str) -> Option<Arc<VfsInode>> {
        let kind = match name {
            "status" => 0,
            "cmdline" => 1,
            "stat" => 2,
            _ => return None,
        };
        let ino = alloc_ino();
        Some(VfsInode::new(
            ino,
            FileType::RegularFile,
            Arc::new(ProcPidFile {
                ino,
                pid: self.pid,
                kind,
            }),
        ))
    }
}

impl InodeOps for ProcPidDir {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: self.ino,
            st_mode: FileType::Directory.mode_bits() | 0o555,
            st_nlink: 2,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, _: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::IsDir)
    }
    fn write(&self, _: u64, _: &[u8]) -> VfsResult<usize> {
        Err(VfsError::IsDir)
    }
    fn readdir(&self, _: u64) -> VfsResult<Vec<DirEntry>> {
        Ok(["status", "cmdline", "stat"]
            .iter()
            .map(|n| DirEntry {
                name: String::from(*n),
                inode: alloc_ino(),
                ftype: FileType::RegularFile,
            })
            .collect())
    }
    fn lookup(&self, name: &str) -> VfsResult<Arc<VfsInode>> {
        self.file(name).ok_or(VfsError::NotFound)
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

/// The /proc root: static children PLUS a live /proc/<pid> entry per process.
struct ProcRootDir {
    ino: u64,
    children: Vec<(String, Arc<VfsInode>)>,
}

impl InodeOps for ProcRootDir {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: self.ino,
            st_mode: FileType::Directory.mode_bits() | 0o555,
            st_nlink: 2,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, _: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::IsDir)
    }
    fn write(&self, _: u64, _: &[u8]) -> VfsResult<usize> {
        Err(VfsError::IsDir)
    }
    fn readdir(&self, _: u64) -> VfsResult<Vec<DirEntry>> {
        let mut out: Vec<DirEntry> = self
            .children
            .iter()
            .map(|(name, inode)| DirEntry {
                name: name.clone(),
                inode: inode.ino,
                ftype: inode.ftype,
            })
            .collect();
        for pid in crate::process::table::TABLE.lock().pids() {
            out.push(DirEntry {
                name: format!("{}", pid),
                inode: pid as u64,
                ftype: FileType::Directory,
            });
        }
        Ok(out)
    }
    fn lookup(&self, name: &str) -> VfsResult<Arc<VfsInode>> {
        if let Some((_, v)) = self.children.iter().find(|(n, _)| n == name) {
            return Ok(v.clone());
        }
        if let Ok(pid) = name.parse::<u32>()
            && crate::process::table::TABLE.lock().name_of(pid).is_some()
        {
            let ino = alloc_ino();
            return Ok(VfsInode::new(
                ino,
                FileType::Directory,
                Arc::new(ProcPidDir { ino, pid }),
            ));
        }
        Err(VfsError::NotFound)
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

fn proc_file(name: &str, f: fn() -> Vec<u8>) -> (String, Arc<VfsInode>) {
    let ino = alloc_ino();
    let ops = Arc::new(ProcFile {
        ino,
        name: String::from(name),
        content_fn: f,
    });
    (
        String::from(name),
        VfsInode::new(ino, FileType::RegularFile, ops),
    )
}

// â”€â”€ Content generators â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn version() -> Vec<u8> {
    b"Linux version 6.1.0-saios (sanjay@saios) (gcc 12.2.0) #1 SMP SAIOS\n".to_vec()
}

fn cpuinfo() -> Vec<u8> {
    let (total, free, _) = crate::memory::frame_stats();
    b"processor\t: 0\nvendor_id\t: GenuineIntel\ncpu family\t: 6\n\
            model name\t: SAIOS Virtual CPU\ncpu MHz\t\t: 2000.000\ncache size\t: 4096 KB\n\
            physical id\t: 0\nsiblings\t: 1\ncore id\t\t: 0\ncpu cores\t: 1\n\
            flags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca \
            cmov pat pse36 clflush mmx fxsr sse sse2 syscall nx lm\nbogomips\t: 4000.00\n\
            clflush size\t: 64\ncache_alignment\t: 64\naddress sizes\t: 39 bits physical, 48 bits virtual\n\n"
                .to_vec()
}

fn meminfo() -> Vec<u8> {
    let (total, free, used) = crate::memory::frame_stats();
    let total_kb = total * 4;
    let free_kb = free * 4;
    let used_kb = used * 4;
    format!(
        "MemTotal:       {:8} kB\nMemFree:        {:8} kB\nMemAvailable:   {:8} kB\n\
         Buffers:               0 kB\nCached:                0 kB\n\
         SwapTotal:             0 kB\nSwapFree:              0 kB\n",
        total_kb, free_kb, free_kb
    )
    .into_bytes()
}

fn uptime() -> Vec<u8> {
    let ticks = crate::shell::commands::boot_ticks();
    let secs = ticks / 18; // ~18 Hz PIT
    format!("{}.00 {}.00\n", secs, secs).into_bytes()
}

fn mounts() -> Vec<u8> {
    let mut out = alloc::vec::Vec::new();
    for (path, fstype) in crate::vfs::list_mounts() {
        let line = format!("{} {} {} rw,relatime 0 0\n", fstype, path, fstype);
        out.extend_from_slice(line.as_bytes());
    }
    out
}

fn cmdline() -> Vec<u8> {
    b"SAIOS_KERNEL\n".to_vec()
}
fn hostname() -> Vec<u8> {
    b"saios\n".to_vec()
}
fn osrelease() -> Vec<u8> {
    b"6.1.0-saios\n".to_vec()
}
fn ostype() -> Vec<u8> {
    b"Linux\n".to_vec()
}

fn self_status() -> Vec<u8> {
    let pid = crate::process::current_pid().unwrap_or(1);
    format!(
        "Name:\tsh\nPid:\t{}\nPPid:\t1\nUid:\t0 0 0 0\nGid:\t0 0 0 0\n\
         VmRSS:\t4096 kB\nVmSize:\t65536 kB\nThreads:\t1\n",
        pid
    )
    .into_bytes()
}

fn build_root_inode() -> Arc<VfsInode> {
    let ino = alloc_ino();
    let sys_ino = alloc_ino();

    // /proc/sys/kernel
    let sys_kernel = Arc::new(ProcDir {
        ino: sys_ino,
        children: alloc::vec![
            proc_file("hostname", hostname),
            proc_file("osrelease", osrelease),
            proc_file("ostype", ostype),
        ],
    });
    let sys_kernel_inode = VfsInode::new(sys_ino, FileType::Directory, sys_kernel);

    let sys_ino2 = alloc_ino();
    let sys_dir = Arc::new(ProcDir {
        ino: sys_ino2,
        children: alloc::vec![(String::from("kernel"), sys_kernel_inode),],
    });
    let sys_inode = VfsInode::new(sys_ino2, FileType::Directory, sys_dir);

    // /proc/self
    let self_ino = alloc_ino();
    let self_dir = Arc::new(ProcDir {
        ino: self_ino,
        children: alloc::vec![proc_file("status", self_status)],
    });
    let self_inode = VfsInode::new(self_ino, FileType::Directory, self_dir);

    let root = Arc::new(ProcRootDir {
        ino,
        children: alloc::vec![
            proc_file("version", version),
            proc_file("cpuinfo", cpuinfo),
            proc_file("meminfo", meminfo),
            proc_file("uptime", uptime),
            proc_file("mounts", mounts),
            proc_file("cmdline", cmdline),
            (String::from("sys"), sys_inode),
            (String::from("self"), self_inode),
        ],
    });
    VfsInode::new(ino, FileType::Directory, root)
}

pub fn mount(mountpoint: &str) {
    let _ = crate::vfs_contract::VfsContract::mount_fs(
        "proc",
        &vfs::MountRequest::new(mountpoint, vfs::MountSource::None),
    );
}
