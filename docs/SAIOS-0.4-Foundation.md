# SAIOS 0.4 Foundation

Status: Planned milestone
Owner: Native filesystem and user-space compatibility
Last updated: 2026-07-05

## Mission

Boot a Linux filesystem and interact with it correctly.

SAIOS 0.4 intentionally narrows scope to Unix-like correctness. This milestone does not prioritize networking, GUI, AI runtime, containers, package manager, USB, audio, security framework, or virtualization.

## Scope and Non-Goals

In scope:

- Native ext4 read-only correctness as the primary root filesystem path.
- Proper VFS contracts as the only I/O path for shell, runtime, and kernel services.
- ELF64 loader improvements sufficient for realistic static user binaries.
- Linux ABI syscall surface sufficient for BusyBox-style workflows.
- Process runtime semantics required for sessions, jobs, and shell correctness.
- Boot handoff to PID1 init and init script driven startup.
- Native Linux-style root layout and mount behavior.
- Storage stack completion for block, partition, and filesystem plumbing.

Explicitly postponed to v0.5+:

- Networking
- GUI/Desktop
- AI Runtime (SAIRU)
- Containers
- Package manager
- USB
- Audio
- Security framework
- Virtualization

## Milestone 1: Native Ext4 Read-Only

Priority: Highest

Architecture target:

- Block Device
- Partition
- Ext4 Superblock
- Group Descriptor
- Inode Cache
- Extent Reader
- Directory Reader
- File Reader

Deliverables:

- Read ext4 superblock and validate feature compatibility.
- Read and validate group descriptor table.
- Read inode 2 (root inode) reliably across valid ext4 variants.
- Parse and walk extents for regular files and directories.
- Traverse directories including sparse/indexed layouts.
- Read regular file data through extent mappings.
- Resolve symlinks.
- Support long filenames.

Acceptance checks:

- Can list root directory from a Linux ext4 rootfs image.
- Can resolve /etc, /usr, /bin, /var and nested paths.
- Can read text and binary files without UTF-8 assumptions.
- Can read symlink targets and follow links during path resolution.

Expected operator result:

ls /

bin
boot
dev
etc
home
lib
mnt
opt
proc
root
run
srv
sys
tmp
usr
var

## Milestone 2: Proper VFS Core

Target model:

- inode
- dentry
- superblock
- mount
- file
- operations
- cache

Rule:

All path-based access must flow through VFS.

Flow:

Shell -> VFS -> Filesystem driver -> Block driver

Deliverables:

- Consolidate filesystem access behind VFS object model.
- Uniform lookup, open, read, write, seek, readdir, stat semantics.
- Mount table with source, target, fs type, flags.
- Path resolution correctness for absolute, relative, dot, dotdot, and symlink-aware traversal.

Acceptance checks:

- Shell file commands use VFS only.
- Kernel/runtime subsystems stop bypassing VFS paths.
- Mount and namespace behavior are deterministic under repeated operations.

## Milestone 3: ELF Loader

Current state is demo-capable; v0.4 requires execution-capable.

Deliverables:

- ELF64 header and program-header validation.
- PT_LOAD mapping with correct virtual ranges.
- Segment permission enforcement (R/W/X).
- BSS zero-initialization semantics.
- Minimal process image contract for static binaries.
- TLS deferred to later phase.

Acceptance checks:

- execve style execution path can load and run static ELF64 binaries.
- Loader rejects malformed binaries with deterministic errors.

## Milestone 4: Linux ABI Surface

Target syscall set for v0.4:

- open
- close
- read
- write
- lseek
- stat
- fstat
- getdents64
- mmap
- munmap
- brk
- dup
- dup2
- pipe
- ioctl
- poll
- select
- fork
- clone
- waitpid

Deliverables:

- Stable syscall table and ABI versioning policy.
- Correct error returns and argument validation.
- Descriptor lifecycle semantics consistent with process boundaries.

Acceptance checks:

- BusyBox-oriented command set can execute core file/process operations.
- Syscall tests validate both success and failure behavior.

## Milestone 5: Process Runtime

Current model:

- Process
- Thread
- Scheduler

Required additions:

- Signals
- Sessions
- Process groups
- TTY integration
- Controlling terminal semantics
- Exit status propagation
- Zombie reaping

Acceptance checks:

- Foreground/background shell jobs behave correctly.
- wait and exit status semantics match expected Unix behavior.

## Milestone 6: Init Flow

Target boot chain:

UEFI -> Kernel -> PID1 (/sbin/init) -> mount -> /etc/init.rc -> spawn shell

Deliverables:

- Replace boot shell as default entrypoint with PID1.
- Init-script driven mount and service bootstrap order.
- Deterministic emergency shell fallback when init fails.

Acceptance checks:

- System reaches shell through init path, not ad-hoc shell bootstrap.
- Init failures are observable and recoverable.

## Milestone 7: Dynamic Linker (Deferred Inside v0.4)

Policy:

- Static binaries first.
- Dynamic linking after core correctness milestones are stable.

Later deliverables:

- ld-linux style interpreter handoff
- ELF relocations
- Shared-library resolution

## Milestone 8: Userland Compatibility

Goal:

Run common Linux userland binaries without recompilation where static compatibility permits.

Target set:

- BusyBox
- toybox
- bash
- coreutils
- vi
- nano

Acceptance checks:

- Core interactive and file workflows execute against mounted Linux rootfs.

## Milestone 9: Native Package Layout

Required real layout at root:

/

bin
boot
dev
etc
home
lib
mnt
proc
root
run
srv
sys
tmp
usr
var

Rule:

Directories and contents are provided by mounted filesystems, not synthetic placeholders.

## Milestone 10: Storage Completion

Required in v0.4:

- AHCI
- GPT
- MBR
- Ext4 RO
- TmpFS
- RamFS
- ProcFS
- DevFS

Deferred:

- Ext4 RW
- FAT32 RW
- NTFS RO

## Definition of Done for v0.4

SAIOS 0.4 is complete when all commands below work from a Linux root filesystem with expected behavior:

- mount /dev/sda2 /
- ls /
- cd /etc
- cat passwd
- cat hostname
- /bin/busybox
- /bin/sh
- echo hello > /tmp/test
- cat /tmp/test
- ps
- uname -a

## Validation Strategy

Required validation layers:

- Unit and parser tests for ext4 metadata, extents, directory entries, and symlink resolution.
- Integration tests for mount, path traversal, open/read/write/readdir/stat.
- Process and ABI tests for descriptor semantics, fork/wait paths, and exit handling.
- Boot-to-init test proving PID1 startup and shell handoff through init scripts.
- Hardware and VM matrix checks for storage controller and ext4 rootfs mount behavior.

## Execution Order and Dependency Gates

Execution order:

1. Ext4 RO correctness
2. VFS normalization
3. ELF loader hardening
4. Linux ABI expansion
5. Process runtime semantics
6. PID1 init chain
7. Userland compatibility
8. Storage and filesystem completion gates

Hard gates:

- Do not advance ABI or process semantics until ext4 and VFS correctness are stable.
- Do not claim userland compatibility until init path is default and deterministic.
- Do not start deferred v0.5 features during v0.4 freeze.

## Why This Defines the 0.4 Milestone

When these gates are complete, SAIOS transitions from a kernel with demos to a usable OS baseline capable of booting and interacting with standard Linux userspace. This creates the architectural foundation required for higher-level capabilities in later milestones.
