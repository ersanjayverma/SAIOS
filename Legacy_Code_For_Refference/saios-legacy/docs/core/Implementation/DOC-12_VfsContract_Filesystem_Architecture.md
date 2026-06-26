# SAIOS VfsContract and Filesystem Architecture Specification
**Document ID:** DOC-12_VfsContract_Filesystem_Architecture.txt
**Layer:** Subsystem Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01 and DOC-06

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt VFSCONTRACT; filesystem intelligence sections. SAIOS_SSOT_Part2.txt FILESYSTEM ARCHITECTURE; NUMA-AWARE PAGE CACHE.

## OWNERSHIP

VfsContract owns namespace selection, path resolution, permission policy, mount operations, inode dispatch, filesystem registration wrappers, page-cache integration, journaling observation, proc, and sys pseudo-filesystems.

## INVARIANTS

Every permission check is performed by VfsContract, never by the filesystem implementation. Path resolution never crosses namespace boundaries without authorisation. Mount operations are atomic; partial mounts are never visible.

## FAILURE MODES

Bypassed permission check triggers Red Ring high plus audit event. Namespace crossing without authorisation returns EPERM and emits audit event. Filesystem returning the wrong inode returns ESTALE and emits FS_ERROR. Partial mount after crash rolls back and is never visible. Symlink cycle returns ELOOP after 40 follows. Path component beyond NAME_MAX returns ENAMETOOLONG.

## GENERIC FILESYSTEM INTERFACE

Every filesystem registers mount, unmount, lookup, inode read, inode write, directory enumeration, create, delete, getattr, setattr, and fsync. VfsContract wraps every operation with capability checks, namespace checks, KDS event emission, and resource accounting. Filesystem implementations contain no KDS or capability logic.

## PAGE CACHE

A page cache entry maps inode plus offset to one 4KB physical frame. Page-cache frames are owned by VFS in the MemoryContract frame model. MAP_PRIVATE mappings use COW. Page states are Clean, Dirty, and Writeback. Writeback runs periodically and when dirty pages exceed 20 percent. PAGE_CACHE_WRITEBACK includes inode, page_count, and reason. PAGE_CACHE_EVICT includes inode, page_count, and dirty flag.

NUMA-aware page cache uses first-touch placement. Shared multi-node files use NUMA_INTERLEAVE. Scheduler-driven migration may move page-cache frames and emits NUMA_PAGE_CACHE_MIGRATED.

## JOURNALING OBSERVATION

VfsContract provides a generic wrapper over filesystem-specific journals. JOURNAL_COMMIT includes filesystem, transaction_id, and commit_latency_ns. JOURNAL_CHECKPOINT includes filesystem, transaction_id, and space_reclaimed. JOURNAL_ERROR is CRITICAL and includes filesystem, error_code, transaction_id if available, and affected_device.

## FILESYSTEM INTELLIGENCE SIGNALS

ext4 exposes journal state, fsck precursors, and fragmentation score. XFS exposes log buffers, inode cluster tracking, and metadata contention. Btrfs exposes COW fragmentation, balance status, and subvolume IO attribution. tmpfs exposes memory pressure and eviction tracking. overlayfs exposes container layer IO attribution and merge cost. NFS and CIFS expose network-induced latency attribution and stale mount detection.

## KDS EVENTS

FS_OPEN: pid, path, flags, latency_ns. FS_WRITE: pid, inode, bytes, latency_ns, dirty_page_count. FS_MOUNT: filesystem_type, device, mount_point, options_hash. FS_ERROR: filesystem, error_type, inode if applicable, operation. JOURNAL_COMMIT, JOURNAL_CHECKPOINT, JOURNAL_ERROR, PAGE_CACHE_WRITEBACK, and PAGE_CACHE_EVICT are mandatory where applicable.

## PROC AND SYS

/proc includes /proc/self with pid, cmdline, maps, status, fd; /proc/cpuinfo; /proc/meminfo; and /proc/version. /sys exposes device enumeration. Both activate at Gate 11.

## COMPLETION CHECK

A developer can register a RAM filesystem with the required functions and receive VFS-controlled permissions, namespace checks, accounting, and KDS events automatically.
