use crate::address_space_contract::AddressSpaceHandle;
use crate::vfs::file::FdTable;
use crate::vfs::mount_namespace::MountNamespace;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;

pub const FD_STDIN: u64 = 0;
pub const FD_STDOUT: u64 = 1;
pub const FD_STDERR: u64 = 2;

#[derive(Debug, Clone)]
pub struct TlsInfo {
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

impl TlsInfo {
    pub fn new(vaddr: u64, filesz: u64, memsz: u64, align: u64) -> Self {
        Self {
            vaddr,
            filesz,
            memsz,
            align,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InterpreterInfo {
    pub path: String,
    pub base: u64,
    pub entry: u64,
}

impl InterpreterInfo {
    pub fn new(path: String, base: u64, entry: u64) -> Self {
        Self { path, base, entry }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FsBase {
    pub fs_base: u64,
    pub gs_base: u64,
}

pub const USER_TEXT_BASE: u64 = 0x0000_0080_0000_0000;
pub const USER_BRK_BASE: u64 = 0x0000_0090_0000_0000;
pub const USER_MMAP_BASE: u64 = 0x0000_00A0_0000_0000;
pub const USER_STACK_TOP: u64 = 0x0000_00FF_FFFF_F000;
pub const USER_TOP: u64 = 0x0000_7FFF_FFFF_FFFF;
pub const USER_STACK_SIZE: usize = 64 * 1024 * 1024;
pub const KERNEL_STACK_SIZE: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    New,
    Ready,
    Running,
    Blocked,
    Zombie,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulingPolicy {
    pub allowed_cpus: u64,
    pub preferred_cpu: Option<usize>,
    pub numa_node: Option<usize>,
}

impl SchedulingPolicy {
    pub const fn unrestricted() -> Self {
        Self {
            allowed_cpus: !0,
            preferred_cpu: None,
            numa_node: None,
        }
    }
}

pub struct Process {
    pub pid: u32,
    pub parent_pid: u32,
    pub(super) state: ProcessState,
    pub name: String,
    pub cwd: String,
    pub namespace_view: crate::vfs::namespace::NamespaceView,
    pub mount_namespace: Arc<MountNamespace>,
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub suid: u32,
    pub sgid: u32,
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub program_entry: u64,
    pub fork_rax: u64,
    pub fork_rdi: u64,
    pub fork_rsi: u64,
    pub fork_rdx: u64,
    pub fork_r8: u64,
    pub fork_r9: u64,
    pub fork_r10: u64,
    pub fork_rbx: u64,
    pub fork_rbp: u64,
    pub fork_r12: u64,
    pub fork_r13: u64,
    pub fork_r14: u64,
    pub fork_r15: u64,
    pub kernel_rsp: u64,
    on_cpu: bool,
    cpu: Option<usize>,
    pub boot_cpu_affine: bool,
    pub scheduling: SchedulingPolicy,
    pub brk: u64,
    pub mmap_base: u64,
    pub address_space: AddressSpaceHandle,
    pub pml4_phys: u64,
    pub owns_address_space: bool,
    pub stack_base: u64,
    pub stack_size: u64,
    pub tls_info: Option<TlsInfo>,
    pub interpreter: Option<InterpreterInfo>,
    pub is_windows_process: bool,
    pub peb_addr: u64,
    pub teb_addr: u64,
    pub fs_base: FsBase,
    pub kernel_stack: Box<[u8; KERNEL_STACK_SIZE]>,
    pub fd_table: FdTable,
    pub signals: crate::process::signal::SigState,
    pub session_id: u32,
    pub pgid: u32,
    pub exit_code: i64,
    pub clear_child_tid: u64,
    /// Monotonic timestamp (ns) when process last entered Ready state.
    /// Used by starvation aging: if Ready for >10s, placement_score is boosted.
    pub ready_since_ns: u64,
    /// Cumulative CPU time in nanoseconds (TSC-based, per-CPU accounting).
    /// F-SCHED-11: tracks actual execution time for times()/getrusage().
    pub cpu_ns: u64,
    /// Priority inheritance boost. Non-zero when this process holds a mutex
    /// contested by a higher-priority waiter. F-SCHED-09.
    pub pi_boost: u8,
}

impl Process {
    pub fn new(pid: u32, name: String) -> Self {
        use crate::vfs::{
            DirEntry, FileType, Inode, InodeOps, Stat, VfsError, VfsResult, alloc_ino,
            file::OpenFile,
        };
        use alloc::vec::Vec;

        let mut fd_table = FdTable::new();

        struct StdinOps;
        impl InodeOps for StdinOps {
            fn stat(&self) -> VfsResult<Stat> {
                Ok(Stat {
                    st_ino: 1,
                    st_mode: FileType::CharDevice.mode_bits() | 0o600,
                    st_nlink: 1,
                    ..Default::default()
                })
            }
            fn read(&self, _: u64, buf: &mut [u8]) -> VfsResult<usize> {
                crate::tty::console::read(buf, 0)
            }
            fn write(&self, _: u64, _: &[u8]) -> VfsResult<usize> {
                Err(VfsError::BadFd)
            }
            fn readdir(&self, _: u64) -> VfsResult<Vec<DirEntry>> {
                Err(VfsError::NotADir)
            }
            fn lookup(&self, _: &str) -> VfsResult<Arc<Inode>> {
                Err(VfsError::NotADir)
            }
            fn create(&self, _: &str, _: FileType, _: u32) -> VfsResult<Arc<Inode>> {
                Err(VfsError::PermDenied)
            }
            fn mkdir(&self, _: &str, _: u32) -> VfsResult<Arc<Inode>> {
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
            fn symlink(&self, _: &str, _: &str) -> VfsResult<Arc<Inode>> {
                Err(VfsError::PermDenied)
            }
            fn link(&self, _: &str, _: &Arc<Inode>) -> VfsResult<()> {
                Err(VfsError::PermDenied)
            }
            fn rename(&self, _: &str, _: &Arc<Inode>, _: &str) -> VfsResult<()> {
                Err(VfsError::PermDenied)
            }
        }

        struct StdoutOps;
        impl InodeOps for StdoutOps {
            fn stat(&self) -> VfsResult<Stat> {
                Ok(Stat {
                    st_ino: 2,
                    st_mode: FileType::CharDevice.mode_bits() | 0o600,
                    st_nlink: 1,
                    ..Default::default()
                })
            }
            fn read(&self, _: u64, _: &mut [u8]) -> VfsResult<usize> {
                Err(VfsError::BadFd)
            }
            fn write(&self, _: u64, buf: &[u8]) -> VfsResult<usize> {
                crate::tty::console::write(buf, 0)
            }
            fn readdir(&self, _: u64) -> VfsResult<Vec<DirEntry>> {
                Err(VfsError::NotADir)
            }
            fn lookup(&self, _: &str) -> VfsResult<Arc<Inode>> {
                Err(VfsError::NotADir)
            }
            fn create(&self, _: &str, _: FileType, _: u32) -> VfsResult<Arc<Inode>> {
                Err(VfsError::PermDenied)
            }
            fn mkdir(&self, _: &str, _: u32) -> VfsResult<Arc<Inode>> {
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
            fn symlink(&self, _: &str, _: &str) -> VfsResult<Arc<Inode>> {
                Err(VfsError::PermDenied)
            }
            fn link(&self, _: &str, _: &Arc<Inode>) -> VfsResult<()> {
                Err(VfsError::PermDenied)
            }
            fn rename(&self, _: &str, _: &Arc<Inode>, _: &str) -> VfsResult<()> {
                Err(VfsError::PermDenied)
            }
        }

        let stdin_inode = Inode::new(alloc_ino(), FileType::CharDevice, Arc::new(StdinOps));
        let stdout_inode = Inode::new(alloc_ino(), FileType::CharDevice, Arc::new(StdoutOps));
        let stderr_inode = Inode::new(alloc_ino(), FileType::CharDevice, Arc::new(StdoutOps));

        fd_table.insert_at(0, OpenFile::new(stdin_inode, 0));
        fd_table.insert_at(1, OpenFile::new(stdout_inode, 1));
        fd_table.insert_at(2, OpenFile::new(stderr_inode, 1));

        Self::from_fd_table(pid, name, fd_table)
    }

    pub fn new_kernel(pid: u32, name: String) -> Self {
        Self::from_fd_table(pid, name, FdTable::new())
    }

    fn from_fd_table(pid: u32, name: String, fd_table: FdTable) -> Self {
        Process {
            pid,
            parent_pid: 1,
            state: ProcessState::New,
            name,
            cwd: String::from("/"),
            namespace_view: crate::vfs::namespace::NamespaceView::Native,
            mount_namespace: crate::vfs::kernel_mount_namespace(),
            uid: 0,
            gid: 0,
            euid: 0,
            egid: 0,
            suid: 0,
            sgid: 0,
            rip: 0,
            rsp: 0,
            rflags: 0x246,
            program_entry: 0,
            fork_rax: 0,
            fork_rdi: 0,
            fork_rsi: 0,
            fork_rdx: 0,
            fork_r8: 0,
            fork_r9: 0,
            fork_r10: 0,
            fork_rbx: 0,
            fork_rbp: 0,
            fork_r12: 0,
            fork_r13: 0,
            fork_r14: 0,
            fork_r15: 0,
            kernel_rsp: 0,
            on_cpu: false,
            cpu: None,
            boot_cpu_affine: false,
            scheduling: SchedulingPolicy::unrestricted(),
            brk: USER_BRK_BASE,
            mmap_base: USER_MMAP_BASE,
            address_space: AddressSpaceHandle {
                id: 0,
                pml4: 0,
                owner_pid: pid,
            },
            pml4_phys: 0,
            owns_address_space: false,
            stack_base: 0,
            stack_size: 0,
            tls_info: None,
            interpreter: None,
            is_windows_process: false,
            peb_addr: 0,
            teb_addr: 0,
            fs_base: FsBase {
                fs_base: 0,
                gs_base: 0,
            },
            kernel_stack: Box::new([0u8; KERNEL_STACK_SIZE]),
            fd_table,
            signals: crate::process::signal::SigState::new(),
            session_id: 1, // Default session ID (PID 1 is session leader)
            pgid: 1,       // Default process group ID
            exit_code: 0,
            clear_child_tid: 0,
            ready_since_ns: 0,
            cpu_ns: 0,
            pi_boost: 0,
        }
    }

    pub fn kernel_stack_top(&self) -> u64 {
        self.kernel_stack.as_ptr() as u64 + KERNEL_STACK_SIZE as u64
    }

    pub fn state(&self) -> &ProcessState {
        &self.state
    }

    pub(crate) fn set_contract_state(&mut self, state: ProcessState) {
        self.state = state;
    }

    pub(crate) fn set_contract_cpu_owner(&mut self, cpu: Option<usize>, on_cpu: bool) {
        self.cpu = cpu;
        self.on_cpu = on_cpu;
    }

    pub fn is_on_cpu(&self) -> bool {
        self.on_cpu
    }

    pub fn cpu_owner(&self) -> Option<usize> {
        self.cpu
    }

    pub fn ensure_address_space(&mut self) -> Result<u64, &'static str> {
        if self.address_space.pml4 == 0 {
            let handle =
                crate::address_space_contract::AddressSpaceContract::create_for_process(self.pid)?;
            self.install_address_space(handle);
            self.owns_address_space = true;
        }
        crate::address_space_contract::AddressSpaceContract::validate_handle_or_panic(
            self.address_space,
            "ensure_address_space",
        );
        Ok(self.address_space.pml4)
    }

    pub fn install_address_space(&mut self, handle: AddressSpaceHandle) {
        self.address_space = handle;
        self.pml4_phys = handle.pml4;
    }

    pub fn clear_address_space(&mut self) {
        self.address_space = AddressSpaceHandle {
            id: 0,
            pml4: 0,
            owner_pid: self.pid,
        };
        self.pml4_phys = 0;
    }

    pub fn address_space_handle(&self) -> Option<AddressSpaceHandle> {
        if self.address_space.pml4 == 0 {
            None
        } else {
            Some(self.address_space)
        }
    }

    pub fn address_space_pml4(&self) -> u64 {
        self.address_space.pml4
    }

    /// Destroy this process's address space and free all associated frames.
    /// Must only be called when the process is no longer running and CR3 has
    /// been switched away from this PML4.
    pub fn destroy_address_space(&mut self) {
        let Some(handle) = self.address_space_handle() else {
            return;
        };
        if !self.owns_address_space {
            return;
        }
        let pml4 = handle.pml4;
        self.clear_address_space();
        self.owns_address_space = false;
        let _ = crate::address_space_contract::AddressSpaceContract::destroy_for_process(
            AddressSpaceHandle {
                id: pml4,
                pml4,
                owner_pid: self.pid,
            },
        );
    }
}
