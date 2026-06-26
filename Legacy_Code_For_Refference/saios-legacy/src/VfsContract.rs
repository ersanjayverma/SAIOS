//! Canonical VFS authority.
//!
//! All filesystem operations should enter through VFS for namespace selection,
//! path resolution, permission checks, and inode operation dispatch.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use crate::block::BlockDevice;
use crate::vfs::file::OpenFile;
use crate::vfs::{DirEntry, FileType, Inode, VfsError};

const PAGE_CACHE_PAGE_SIZE: usize = 4096;
const PAGE_CACHE_DIRTY_THRESHOLD_PERCENT: u8 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsOperation {
    Resolve,
    Open,
    Read,
    Write,
    Create,
    Unlink,
    Rename,
    Chmod,
    Chown,
    Mount,
    Unmount,
    Namespace,
    Exec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VfsAccess {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
    pub operation: VfsOperation,
}

pub struct VfsExecImage {
    pub file_type: FileType,
    pub size: i64,
    pub data: Vec<u8>,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsFilesystemKind {
    Ext4 = 1,
    Tmpfs = 2,
    Procfs = 3,
    Devfs = 4,
    Xfs = 5,
    Btrfs = 6,
    Overlayfs = 7,
    Nfs = 8,
    Cifs = 9,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageCacheState {
    Clean = 1,
    Dirty = 2,
    Writeback = 3,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageCacheWritebackReason {
    Periodic = 1,
    Pressure = 2,
    Fsync = 3,
    Close = 4,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalObservationKind {
    Commit = 1,
    Checkpoint = 2,
    Error = 3,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesystemIntelligenceDomain {
    Ext4JournalHealth = 1,
    Ext4Fragmentation = 2,
    XfsMetadataContention = 3,
    BtrfsCowFragmentation = 4,
    TmpfsMemoryPressure = 5,
    OverlayLayerCost = 6,
    NetworkFilesystemLatency = 7,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageCachePolicyView {
    pub page_size: usize,
    pub dirty_threshold_percent: u8,
    pub states: [PageCacheState; 3],
    pub vfs_frame_owner: bool,
    pub writeback_daemon: bool,
    pub numa_placement_policy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VfsCapabilityView {
    pub namespace_selection: bool,
    pub path_resolution: bool,
    pub permission_policy: bool,
    pub mount_operations: bool,
    pub inode_dispatch: bool,
    pub file_open_events: bool,
    pub file_read_events: bool,
    pub file_write_events: bool,
    pub mount_events: bool,
    pub storage_byte_accounting: bool,
    pub mac_file_write_hook: bool,
    pub generic_filesystem_registry: bool,
    pub fsync_wrapper: bool,
    pub atomic_mount_rollback: bool,
    pub namespace_escape_enforcement: bool,
    pub page_cache: bool,
    pub page_cache_writeback_events: bool,
    pub page_cache_evict_events: bool,
    pub journal_observation: bool,
    pub ext4: bool,
    pub tmpfs: bool,
    pub procfs: bool,
    pub devfs: bool,
    pub xfs: bool,
    pub btrfs: bool,
    pub overlayfs: bool,
    pub nfs: bool,
    pub cifs: bool,
    pub filesystem_intelligence: bool,
}

pub struct VfsContract;

impl VfsContract {
    pub fn capability_view() -> VfsCapabilityView {
        VfsCapabilityView {
            namespace_selection: true,
            path_resolution: true,
            permission_policy: true,
            mount_operations: true,
            inode_dispatch: true,
            file_open_events: true,
            file_read_events: true,
            file_write_events: true,
            mount_events: true,
            storage_byte_accounting: true,
            mac_file_write_hook: true,
            generic_filesystem_registry: true,
            fsync_wrapper: false,
            atomic_mount_rollback: false,
            namespace_escape_enforcement: false,
            page_cache: false,
            page_cache_writeback_events: false,
            page_cache_evict_events: false,
            journal_observation: false,
            ext4: true,
            tmpfs: true,
            procfs: true,
            devfs: true,
            xfs: false,
            btrfs: false,
            overlayfs: false,
            nfs: false,
            cifs: false,
            filesystem_intelligence: false,
        }
    }

    pub fn page_cache_policy_view() -> PageCachePolicyView {
        PageCachePolicyView {
            page_size: PAGE_CACHE_PAGE_SIZE,
            dirty_threshold_percent: PAGE_CACHE_DIRTY_THRESHOLD_PERCENT,
            states: [
                PageCacheState::Clean,
                PageCacheState::Dirty,
                PageCacheState::Writeback,
            ],
            vfs_frame_owner: false,
            writeback_daemon: false,
            numa_placement_policy: false,
        }
    }

    fn emit_vfs_event(
        event_type: crate::kds::KdsEventType,
        operation: VfsOperation,
        outcome: crate::observability_contract::ObservationOutcome,
        reason: &'static str,
        evidence: [u64; 4],
    ) {
        let reason = if reason.is_empty() {
            vfs_operation_event_name(operation)
        } else {
            reason
        };
        let pid = crate::process::current_pid();
        crate::observability_contract::ObservabilityContract::emit_as_kds_event(
            crate::observability_contract::EventRecord {
                event: crate::observability_contract::ObservableEvent::Transition,
                contract: crate::observability_contract::ContractId::Vfs,
                tag: crate::observability_contract::ObservationTag::Transition,
                reason,
                outcome,
                resource: crate::observability_contract::ResourceClass::Vfs,
                owner: crate::observability_contract::ObservabilityContract::current_pid_owner(),
                cpu: Some(crate::process::table::cpu_idx()),
                pid,
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence: [operation as u64, evidence[0], evidence[1], evidence[2]],
            },
            event_type,
            match outcome {
                crate::observability_contract::ObservationOutcome::Success => {
                    crate::kds::KdsSeverity::Trace
                }
                crate::observability_contract::ObservationOutcome::Denied => {
                    crate::kds::KdsSeverity::Warn
                }
                _ => crate::kds::KdsSeverity::Info,
            },
        );
    }

    fn validate_current_access(operation: VfsOperation, tag: &'static str) {
        if !crate::arch::syscall::kernel_gs_active() {
            return;
        }
        let access = crate::process::with_current_process(|proc| VfsAccess {
            pid: proc.pid,
            uid: proc.euid,
            gid: proc.egid,
            operation,
        })
        .unwrap_or(VfsAccess {
            pid: 0,
            uid: 0,
            gid: 0,
            operation,
        });
        Self::validate_access_or_panic(access, tag);
    }

    fn check_inode_permission(
        inode: &Arc<Inode>,
        operation: crate::user::PermissionOperation,
    ) -> crate::vfs::VfsResult<()> {
        let stat = inode.ops.stat()?;
        if crate::user::check_permission(&stat, operation) {
            Ok(())
        } else {
            Self::emit_vfs_event(
                crate::kds::KdsEventType::CompatibilityFailure,
                VfsOperation::Open,
                crate::observability_contract::ObservationOutcome::Denied,
                "vfs.permission",
                [
                    inode.ino,
                    operation as u64,
                    stat.st_mode as u64,
                    stat.st_uid as u64,
                ],
            );
            Err(VfsError::PermDenied)
        }
    }

    fn check_inode_owner_or_root(
        inode: &Arc<Inode>,
        operation: VfsOperation,
        reason: &'static str,
    ) -> crate::vfs::VfsResult<()> {
        let stat = inode.ops.stat()?;
        let (_, _, euid, _) = crate::user::get_current_credentials();
        if euid == 0 || euid == stat.st_uid {
            return Ok(());
        }
        Self::emit_vfs_event(
            crate::kds::KdsEventType::CompatibilityFailure,
            operation,
            crate::observability_contract::ObservationOutcome::Denied,
            reason,
            [inode.ino, euid as u64, stat.st_uid as u64, stat.st_mode as u64],
        );
        Err(VfsError::PermDenied)
    }

    fn check_current_root(
        operation: VfsOperation,
        reason: &'static str,
    ) -> crate::vfs::VfsResult<()> {
        let (_, _, euid, _) = crate::user::get_current_credentials();
        if euid == 0 {
            return Ok(());
        }
        Self::emit_vfs_event(
            crate::kds::KdsEventType::CompatibilityFailure,
            operation,
            crate::observability_contract::ObservationOutcome::Denied,
            reason,
            [euid as u64, 0, 0, 0],
        );
        Err(VfsError::PermDenied)
    }

    pub fn validate_access(access: VfsAccess) -> Result<(), &'static str> {
        if access.pid == 0 {
            return Err("vfs: access has no process owner");
        }
        Ok(())
    }

    pub fn validate_access_or_panic(access: VfsAccess, tag: &'static str) {
        if let Err(reason) = Self::validate_access(access) {
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::Vfs,
                tag,
                reason,
                crate::observability_contract::ResourceClass::Vfs,
                crate::observability_contract::ResourceOwner::Pid(access.pid),
                [
                    access.pid as u64,
                    access.uid as u64,
                    access.gid as u64,
                    access.operation as u64,
                ],
            );
            Self::dump_access(access, tag, reason);
            panic!("[vfs-contract] {} violation: {}", tag, reason);
        }
    }

    pub fn dump_access(access: VfsAccess, tag: &'static str, reason: &'static str) {
        crate::serial_println!(
            "[vfs-contract] dump tag={} reason={} pid={} uid={} gid={} op={:?} cpu={} current_pid={:?}",
            tag,
            reason,
            access.pid,
            access.uid,
            access.gid,
            access.operation,
            crate::process::table::cpu_idx(),
            crate::process::current_pid()
        );
    }

    pub fn resolve(path: &str) -> crate::vfs::VfsResult<Arc<Inode>> {
        Self::validate_current_access(VfsOperation::Resolve, "resolve");
        let inode = crate::vfs::resolve(path)?;
        Self::emit_vfs_event(
            crate::kds::KdsEventType::State,
            VfsOperation::Resolve,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [inode.ino, inode.ftype as u64, 0, 0],
        );
        Ok(inode)
    }

    pub fn resolve_parent(path: &str) -> crate::vfs::VfsResult<(Arc<Inode>, String)> {
        Self::validate_current_access(VfsOperation::Resolve, "resolve_parent");
        crate::vfs::resolve_parent(path)
    }

    pub fn namespace_view() -> crate::vfs::namespace::NamespaceView {
        Self::validate_current_access(VfsOperation::Namespace, "namespace_view");
        crate::vfs::namespace::current_view()
    }

    pub fn translate_path(path: &str) -> String {
        Self::validate_current_access(VfsOperation::Namespace, "translate_path");
        crate::vfs::namespace::translate_path_for_view(path, Self::namespace_view())
    }

    pub fn mount_root(path: &str) -> Option<Arc<Inode>> {
        Self::validate_current_access(VfsOperation::Namespace, "mount_root");
        crate::vfs::get_mount_root(path)
    }

    pub fn mount_list() -> Vec<(String, String)> {
        Self::validate_current_access(VfsOperation::Namespace, "mount_list");
        crate::vfs::list_mounts()
    }

    pub fn open(path: &str, flags: u32, mode: u32) -> crate::vfs::VfsResult<Arc<OpenFile>> {
        Self::validate_current_access(VfsOperation::Open, "open");
        let create = flags & crate::vfs::file::O_CREAT != 0;
        let trunc = flags & crate::vfs::file::O_TRUNC != 0;

        let inode = match crate::vfs::resolve(path) {
            Ok(inode) => {
                match flags & 0b11 {
                    crate::vfs::file::O_WRONLY => {
                        Self::check_inode_permission(
                            &inode,
                            crate::user::PermissionOperation::Write,
                        )?;
                    }
                    crate::vfs::file::O_RDWR => {
                        Self::check_inode_permission(
                            &inode,
                            crate::user::PermissionOperation::Read,
                        )?;
                        Self::check_inode_permission(
                            &inode,
                            crate::user::PermissionOperation::Write,
                        )?;
                    }
                    _ => {
                        Self::check_inode_permission(
                            &inode,
                            crate::user::PermissionOperation::Read,
                        )?;
                    }
                }
                if trunc {
                    Self::check_inode_permission(&inode, crate::user::PermissionOperation::Write)?;
                    inode.ops.truncate(0)?;
                }
                inode
            }
            Err(VfsError::NotFound) if create => {
                let (parent, name) = crate::vfs::resolve_parent(path)?;
                Self::check_inode_permission(&parent, crate::user::PermissionOperation::Write)?;
                parent.ops.create(&name, FileType::RegularFile, mode)?
            }
            Err(error) => return Err(error),
        };
        Self::emit_vfs_event(
            crate::kds::KdsEventType::FileOpen,
            VfsOperation::Open,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [inode.ino, flags as u64, mode as u64, create as u64],
        );
        Ok(OpenFile::new(inode, flags))
    }

    pub fn mount_fs(fs_type: &str, request: &crate::vfs::MountRequest) -> Result<(), &'static str> {
        Self::validate_current_access(VfsOperation::Mount, "mount_fs");
        crate::vfs::mount_fs(fs_type, request)?;
        Self::emit_vfs_event(
            crate::kds::KdsEventType::Mount,
            VfsOperation::Mount,
            crate::observability_contract::ObservationOutcome::Success,
            "vfs.mount",
            [stable_hash(fs_type), stable_hash(request.target), 0, 0],
        );
        Ok(())
    }

    pub fn unmount(path: &str) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Unmount, "unmount");
        crate::vfs::unmount(path)?;
        Self::emit_vfs_event(
            crate::kds::KdsEventType::Unmount,
            VfsOperation::Unmount,
            crate::observability_contract::ObservationOutcome::Success,
            "vfs.unmount",
            [stable_hash(path), 0, 0, 0],
        );
        Ok(())
    }

    pub fn insert_fd(file: Arc<OpenFile>) -> crate::vfs::VfsResult<usize> {
        Self::validate_current_access(VfsOperation::Open, "insert_fd");
        crate::process::with_current_process_mut(|proc| proc.fd_table.insert(file))
            .unwrap_or(Err(VfsError::BadFd))
    }

    pub fn insert_fd_pair(
        first: Arc<OpenFile>,
        second: Arc<OpenFile>,
    ) -> crate::vfs::VfsResult<(usize, usize)> {
        Self::validate_current_access(VfsOperation::Open, "insert_fd_pair");
        crate::process::with_current_process_mut(|proc| {
            Self::insert_fd_pair_for_process(proc, first, second)
        })
        .unwrap_or(Err(VfsError::BadFd))
    }

    pub fn insert_fd_pair_for_process(
        proc: &mut crate::process::Process,
        first: Arc<OpenFile>,
        second: Arc<OpenFile>,
    ) -> crate::vfs::VfsResult<(usize, usize)> {
        let first_fd = proc.fd_table.insert(first)?;
        let second_fd = match proc.fd_table.insert(second) {
            Ok(fd) => fd,
            Err(error) => {
                let _ = proc.fd_table.close(first_fd);
                return Err(error);
            }
        };
        Ok((first_fd, second_fd))
    }

    pub fn close_on_exec_for_process(proc: &mut crate::process::Process) {
        proc.fd_table.close_on_exec();
    }

    pub fn get_fd(fd: usize) -> crate::vfs::VfsResult<Arc<OpenFile>> {
        Self::validate_current_access(VfsOperation::Open, "get_fd");
        crate::process::with_current_process(|proc| proc.fd_table.get(fd))
            .unwrap_or(Err(VfsError::BadFd))
    }

    pub fn close_fd(fd: usize) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Unlink, "close_fd");
        crate::process::with_current_process_mut(|proc| proc.fd_table.close(fd))
            .unwrap_or(Err(VfsError::BadFd))
    }

    pub fn dup_fd(fd: usize) -> crate::vfs::VfsResult<usize> {
        Self::validate_current_access(VfsOperation::Open, "dup_fd");
        let file = Self::get_fd(fd)?;
        Self::insert_fd(file)
    }

    pub fn dup2_fd(oldfd: usize, newfd: usize) -> crate::vfs::VfsResult<usize> {
        Self::validate_current_access(VfsOperation::Open, "dup2_fd");
        let file = Self::get_fd(oldfd)?;
        crate::process::with_current_process_mut(|proc| {
            proc.fd_table.insert_at(newfd, file);
            Ok(newfd)
        })
        .unwrap_or(Err(VfsError::BadFd))
    }

    pub fn read_fd(fd: usize, buf: &mut [u8]) -> crate::vfs::VfsResult<usize> {
        Self::validate_current_access(VfsOperation::Read, "read_fd");
        let file = Self::get_fd(fd)?;
        let read = file.read(buf)?;
        Self::emit_vfs_event(
            crate::kds::KdsEventType::FileRead,
            VfsOperation::Read,
            crate::observability_contract::ObservationOutcome::Success,
            "vfs.fd.read",
            [fd as u64, file.inode.ino, read as u64, buf.len() as u64],
        );
        Ok(read)
    }

    pub fn write_fd(fd: usize, buf: &[u8]) -> crate::vfs::VfsResult<usize> {
        Self::validate_current_access(VfsOperation::Write, "write_fd");
        let file = Self::get_fd(fd)?;
        let written = file.write(buf)?;
        Self::emit_vfs_event(
            crate::kds::KdsEventType::FileWrite,
            VfsOperation::Write,
            crate::observability_contract::ObservationOutcome::Success,
            "vfs.fd.write",
            [fd as u64, file.inode.ino, written as u64, buf.len() as u64],
        );
        Ok(written)
    }

    pub fn seek_fd(fd: usize, offset: i64, whence: u32) -> crate::vfs::VfsResult<u64> {
        Self::validate_current_access(VfsOperation::Read, "seek_fd");
        Self::get_fd(fd)?.seek(offset, whence)
    }

    pub fn stat_fd(fd: usize) -> crate::vfs::VfsResult<crate::vfs::Stat> {
        Self::validate_current_access(VfsOperation::Read, "stat_fd");
        Self::get_fd(fd)?.inode.ops.stat()
    }

    pub fn truncate_fd(fd: usize, len: u64) -> crate::vfs::VfsResult<()> {
        let file = Self::get_fd(fd)?;
        Self::truncate_inode(&file.inode, len)
    }

    pub fn chmod_fd(fd: usize, mode: u32) -> crate::vfs::VfsResult<()> {
        let file = Self::get_fd(fd)?;
        Self::check_inode_owner_or_root(&file.inode, VfsOperation::Chmod, "vfs.chmod_fd")?;
        file.inode.ops.chmod(mode)
    }

    pub fn pread_fd(fd: usize, offset: u64, buf: &mut [u8]) -> crate::vfs::VfsResult<usize> {
        let file = Self::get_fd(fd)?;
        Self::read_inode(&file.inode, offset, buf)
    }

    pub fn pwrite_fd(fd: usize, offset: u64, buf: &[u8]) -> crate::vfs::VfsResult<usize> {
        let file = Self::get_fd(fd)?;
        Self::write_inode(&file.inode, offset, buf)
    }

    pub fn readdir_fd(fd: usize) -> crate::vfs::VfsResult<(Vec<DirEntry>, Arc<OpenFile>)> {
        Self::validate_current_access(VfsOperation::Read, "readdir_fd");
        let file = Self::get_fd(fd)?;
        let offset = file.offset.load(Ordering::Relaxed);
        let entries = file.inode.ops.readdir(offset)?;
        Ok((entries, file))
    }

    pub fn mkdir(path: &str, mode: u32) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Create, "mkdir");
        let (parent, name) = crate::vfs::resolve_parent(path)?;
        Self::check_inode_permission(&parent, crate::user::PermissionOperation::Write)?;
        parent.ops.mkdir(&name, mode).map(|_| ())
    }

    pub fn unlink(path: &str) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Unlink, "unlink");
        let (parent, name) = crate::vfs::resolve_parent(path)?;
        Self::check_inode_permission(&parent, crate::user::PermissionOperation::Write)?;
        parent.ops.unlink(&name)
    }

    pub fn rmdir(path: &str) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Unlink, "rmdir");
        let (parent, name) = crate::vfs::resolve_parent(path)?;
        Self::check_inode_permission(&parent, crate::user::PermissionOperation::Write)?;
        parent.ops.rmdir(&name)
    }

    pub fn rename(old_path: &str, new_path: &str) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Rename, "rename");
        let (old_parent, old_name) = crate::vfs::resolve_parent(old_path)?;
        let (new_parent, new_name) = crate::vfs::resolve_parent(new_path)?;
        Self::check_inode_permission(&old_parent, crate::user::PermissionOperation::Write)?;
        Self::check_inode_permission(&new_parent, crate::user::PermissionOperation::Write)?;
        old_parent.ops.rename(&old_name, &new_parent, &new_name)
    }

    pub fn chmod(path: &str, mode: u32) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Chmod, "chmod");
        let inode = crate::vfs::resolve(path)?;
        Self::check_inode_owner_or_root(&inode, VfsOperation::Chmod, "vfs.chmod")?;
        inode.ops.chmod(mode)
    }

    pub fn chown(path: &str, uid: u32, gid: u32) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Chown, "chown");
        let inode = crate::vfs::resolve(path)?;
        Self::check_current_root(VfsOperation::Chown, "vfs.chown")?;
        inode.ops.chown(uid, gid)
    }

    pub fn truncate(path: &str, len: u64) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Write, "truncate");
        let inode = crate::vfs::resolve(path)?;
        Self::check_inode_permission(&inode, crate::user::PermissionOperation::Write)?;
        inode.ops.truncate(len)
    }

    pub fn truncate_inode(inode: &Arc<Inode>, len: u64) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Write, "truncate_inode");
        Self::check_inode_permission(inode, crate::user::PermissionOperation::Write)?;
        inode.ops.truncate(len)
    }

    pub fn read_inode(
        inode: &Arc<Inode>,
        offset: u64,
        buf: &mut [u8],
    ) -> crate::vfs::VfsResult<usize> {
        Self::validate_current_access(VfsOperation::Read, "read_inode");
        Self::check_inode_permission(inode, crate::user::PermissionOperation::Read)?;
        let read = inode.ops.read(offset, buf)?;
        Self::emit_vfs_event(
            crate::kds::KdsEventType::FileRead,
            VfsOperation::Read,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [inode.ino, offset, read as u64, buf.len() as u64],
        );
        Ok(read)
    }

    pub fn write_inode(
        inode: &Arc<Inode>,
        offset: u64,
        buf: &[u8],
    ) -> crate::vfs::VfsResult<usize> {
        Self::validate_current_access(VfsOperation::Write, "write_inode");
        Self::check_inode_permission(inode, crate::user::PermissionOperation::Write)?;
        let written = Self::accounted_inode_write(inode, offset, buf)?;
        Self::emit_vfs_event(
            crate::kds::KdsEventType::FileWrite,
            VfsOperation::Write,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [inode.ino, offset, written as u64, buf.len() as u64],
        );
        Ok(written)
    }

    pub fn read_file(path: &str) -> crate::vfs::VfsResult<Vec<u8>> {
        let inode = Self::resolve(path)?;
        let stat = inode.ops.stat()?;
        Self::check_inode_permission(&inode, crate::user::PermissionOperation::Read)?;
        if stat.st_size < 0 {
            return Err(VfsError::InvalidArg);
        }
        let mut data = alloc::vec![0u8; stat.st_size as usize];
        let read = inode.ops.read(0, &mut data)?;
        data.truncate(read);
        Self::emit_vfs_event(
            crate::kds::KdsEventType::FileRead,
            VfsOperation::Read,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [inode.ino, 0, read as u64, stat.st_size as u64],
        );
        Ok(data)
    }

    pub fn write_file(path: &str, data: &[u8], mode: u32) -> crate::vfs::VfsResult<()> {
        match crate::vfs::resolve(path) {
            Ok(inode) => {
                Self::check_inode_permission(&inode, crate::user::PermissionOperation::Write)?;
                inode.ops.truncate(0)?;
                let written = Self::accounted_inode_write(&inode, 0, data)?;
                Self::emit_vfs_event(
                    crate::kds::KdsEventType::FileWrite,
                    VfsOperation::Write,
                    crate::observability_contract::ObservationOutcome::Success,
                    "",
                    [inode.ino, 0, written as u64, data.len() as u64],
                );
                Ok(())
            }
            Err(VfsError::NotFound) => {
                let (parent, name) = crate::vfs::resolve_parent(path)?;
                Self::check_inode_permission(&parent, crate::user::PermissionOperation::Write)?;
                let inode = parent.ops.create(&name, FileType::RegularFile, mode)?;
                let written = Self::accounted_inode_write(&inode, 0, data)?;
                Self::emit_vfs_event(
                    crate::kds::KdsEventType::FileWrite,
                    VfsOperation::Write,
                    crate::observability_contract::ObservationOutcome::Success,
                    "",
                    [inode.ino, 0, written as u64, data.len() as u64],
                );
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub fn append_file(path: &str, data: &[u8], mode: u32) -> crate::vfs::VfsResult<()> {
        match crate::vfs::resolve(path) {
            Ok(inode) => {
                Self::check_inode_permission(&inode, crate::user::PermissionOperation::Write)?;
                let offset = inode.ops.stat()?.st_size.max(0) as u64;
                let written = Self::accounted_inode_write(&inode, offset, data)?;
                Self::emit_vfs_event(
                    crate::kds::KdsEventType::FileWrite,
                    VfsOperation::Write,
                    crate::observability_contract::ObservationOutcome::Success,
                    "",
                    [inode.ino, offset, written as u64, data.len() as u64],
                );
                Ok(())
            }
            Err(VfsError::NotFound) => Self::write_file(path, data, mode),
            Err(error) => Err(error),
        }
    }

    fn accounted_inode_write(
        inode: &Arc<Inode>,
        offset: u64,
        data: &[u8],
    ) -> crate::vfs::VfsResult<usize> {
        crate::security_contract::SecurityContract::check_mac(
            crate::security_contract::SecurityOperation::FileWrite,
            crate::security_contract::SecurityLabel::current_subject(),
            crate::security_contract::SecurityLabel::public_object(),
        )
        .map_err(|_| VfsError::PermDenied)?;
        let amount = data.len() as u64;
        let chain = crate::resource_contract::AttributionChain::current();
        if crate::resource_contract::ResourceContract::charge(
            chain,
            crate::resource_contract::ResourceKind::StorageBytes,
            amount,
        )
        .is_err()
        {
            return Err(VfsError::NoSpace);
        }
        match inode.ops.write(offset, data) {
            Ok(written) => {
                let unused = amount.saturating_sub(written as u64);
                if unused != 0 {
                    crate::resource_contract::ResourceContract::release(
                        chain.accountable,
                        crate::resource_contract::ResourceKind::StorageBytes,
                        unused,
                    );
                }
                Ok(written)
            }
            Err(error) => {
                crate::resource_contract::ResourceContract::release(
                    chain.accountable,
                    crate::resource_contract::ResourceKind::StorageBytes,
                    amount,
                );
                Err(error)
            }
        }
    }

    pub fn read_dir(path: &str) -> crate::vfs::VfsResult<Vec<DirEntry>> {
        let inode = Self::resolve(path)?;
        Self::check_inode_permission(&inode, crate::user::PermissionOperation::Read)?;
        inode.ops.readdir(0)
    }

    pub fn symlink(path: &str, target: &str) -> crate::vfs::VfsResult<Arc<Inode>> {
        Self::validate_current_access(VfsOperation::Create, "symlink");
        let (parent, name) = crate::vfs::resolve_parent(path)?;
        Self::check_inode_permission(&parent, crate::user::PermissionOperation::Write)?;
        parent.ops.symlink(&name, target)
    }

    pub fn link(path: &str, target: &Arc<Inode>) -> crate::vfs::VfsResult<()> {
        Self::validate_current_access(VfsOperation::Create, "link");
        let (parent, name) = crate::vfs::resolve_parent(path)?;
        Self::check_inode_permission(&parent, crate::user::PermissionOperation::Write)?;
        parent.ops.link(&name, target)
    }

    pub fn exec(path: &str) -> crate::vfs::VfsResult<Arc<Inode>> {
        Self::validate_current_access(VfsOperation::Exec, "exec");
        crate::vfs::resolve(path)
    }

    pub fn exec_image(path: &str) -> crate::vfs::VfsResult<VfsExecImage> {
        Self::validate_current_access(VfsOperation::Exec, "exec_image");
        let inode = crate::vfs::resolve(path)?;
        let stat = inode.ops.stat()?;
        if !crate::user::check_permission(&stat, crate::user::PermissionOperation::Execute) {
            return Err(VfsError::PermDenied);
        }
        if stat.st_size < 0 {
            return Err(VfsError::InvalidArg);
        }
        let mut data = alloc::vec![0u8; stat.st_size as usize];
        inode.ops.read(0, &mut data)?;
        Self::emit_vfs_event(
            crate::kds::KdsEventType::Execve,
            VfsOperation::Exec,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [inode.ino, stat.st_size as u64, inode.ftype as u64, 0],
        );
        Ok(VfsExecImage {
            file_type: inode.ftype,
            size: stat.st_size,
            data,
        })
    }

    pub fn mount_install_rootfs(dev: Arc<dyn BlockDevice>) -> Result<Arc<Inode>, &'static str> {
        let fs = crate::fs::ext4::Ext4Fs::mount(dev)?;
        let root = crate::fs::ext4::Ext4Fs::root_inode(fs)
            .map_err(|_| "vfs-contract: install root lookup failed")?;
        Self::emit_vfs_event(
            crate::kds::KdsEventType::Mount,
            VfsOperation::Mount,
            crate::observability_contract::ObservationOutcome::Success,
            "vfs.mount",
            [root.ino, root.ftype as u64, 0, 0],
        );
        Ok(root)
    }

    pub fn ensure_install_dir(root: &Arc<Inode>, path: &str) -> Result<(), &'static str> {
        let _ = Self::resolve_or_create_install_dir(root.clone(), path)?;
        Ok(())
    }

    pub fn write_install_file(
        root: &Arc<Inode>,
        path: &str,
        data: &[u8],
        mode: u32,
    ) -> Result<(), &'static str> {
        let (parent_path, name) = match path.rsplit_once('/') {
            Some(("", name)) => ("/", name),
            Some((parent, name)) => (parent, name),
            None => return Err("vfs-contract: install path invalid"),
        };
        let parent = Self::resolve_or_create_install_dir(root.clone(), parent_path)?;
        let inode = match parent.ops.lookup(name) {
            Ok(existing) => existing,
            Err(VfsError::NotFound) | Err(VfsError::NoEntry) => parent
                .ops
                .create(name, FileType::RegularFile, mode)
                .map_err(|_| "vfs-contract: install create failed")?,
            Err(_) => return Err("vfs-contract: install file lookup failed"),
        };

        inode
            .ops
            .truncate(0)
            .map_err(|_| "vfs-contract: install truncate failed")?;
        inode
            .ops
            .write(0, data)
            .map_err(|_| "vfs-contract: install write failed")?;
        Ok(())
    }

    fn resolve_or_create_install_dir(
        mut current: Arc<Inode>,
        path: &str,
    ) -> Result<Arc<Inode>, &'static str> {
        if path.is_empty() || path == "/" {
            return Ok(current);
        }

        for part in path
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
        {
            current = match current.ops.lookup(part) {
                Ok(next) => next,
                Err(VfsError::NotFound) | Err(VfsError::NoEntry) => current
                    .ops
                    .mkdir(part, 0o755)
                    .map_err(|_| "vfs-contract: install mkdir failed")?,
                Err(_) => return Err("vfs-contract: install lookup failed"),
            };
        }

        Ok(current)
    }
}

fn vfs_operation_event_name(operation: VfsOperation) -> &'static str {
    match operation {
        VfsOperation::Resolve => "vfs.resolve",
        VfsOperation::Open => "vfs.open",
        VfsOperation::Read => "vfs.read",
        VfsOperation::Write => "vfs.write",
        VfsOperation::Mount => "vfs.mount",
        VfsOperation::Unmount => "vfs.unmount",
        VfsOperation::Namespace => "vfs.namespace",
        VfsOperation::Exec => "vfs.exec",
        _ => "vfs.permission",
    }
}

fn stable_hash(name: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
