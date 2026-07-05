# SAIOS Stub Inventory

Date: 2026-07-06

This file tracks code that is explicitly stubbed, scaffolded, synthetic, no-op by design, or otherwise intentionally incomplete.

## Current Stub Inventory

### 1. Constrained C compiler and print-binary execution
- File: `seed/saios/src/shell/programs.rs`
- Area: `cc` shell program and compiled output path
- Status: Completed for the current supported subset
- Why this is no longer a stub:
  - `cc` now emits `SAIOS_BIN_V1` executables rather than `SAIOS_CC_STUB` marker files.
  - The resulting output is executed through the normal binary dispatch path.
  - Legacy stub files remain readable for backward compatibility, but new compilation output uses the real binary metadata format.
- Evidence summary:
  - The compiler writes `entry=cc_print` binaries with embedded source/message metadata.
  - Execution recognizes and runs those `SAIOS_BIN_V1` outputs directly.
  - Scope is intentionally constrained to print-only `main()` programs with a single string literal.

### 2. Native ext4 write-path scaffolding
- File: `seed/saios/src/driver/storage.rs`
- Area: ext4 create/mkdir/delete/rename and metadata update path
- Status: Core metadata mutation path implemented; allocator/journal internals still need cleanup
- Why this has progressed:
  - Native ext4 `fs_create`, `fs_mkdir`, `fs_delete`, and `fs_rename` now use real on-disk metadata updates instead of returning stage-8 scaffold errors.
  - Empty-file and directory creation now allocate inodes and directory entries.
  - Delete now removes directory entries and frees inode/block bitmap state for the reachable inode blocks.
  - Rename now reinserts and removes directory entries on the native ext4 path.
- Evidence summary:
  - Functions still include `ext4_alloc_block_scaffold`, `ext4_alloc_inode_scaffold`, `ext4_write_inode_basic_scaffold`, and `ext4_journal_intent_scaffold` naming, but the user-visible mutation entry points no longer fail closed on the native ext4 path.
  - Residual follow-up remains around allocator naming, superblock/group counter maintenance, and journaling semantics.

### 3. Synthetic `mmap` and no-op `munmap`
- File: `seed/saios/src/kernel/syscall.rs`
- Area: syscall memory mapping surface
- Status: Placeholder implementation
- Why this is a stub:
  - `mmap` returns a synthetic virtual address instead of creating a real mapping.
  - `munmap` is documented as a no-op in the current build.
- Evidence summary:
  - The syscall comments explicitly describe the implementation as synthetic or no-op.

### 4. Fault policy scaffold
- File: `seed/saios/src/kernel/fault.rs`
- Area: user fault containment and recovery policy
- Status: Scaffold
- Why this is a stub:
  - The module header explicitly calls this readiness scaffolding.
  - It exists to support validation before full containment is implemented.
- Evidence summary:
  - The top-of-file documentation states that it is scaffolding for v0.3 readiness, not the final fault-management design.

### 5. KSF service stop lifecycle
- File: `seed/saios/src/ksf.rs`
- Area: service shutdown behavior
- Status: Mostly no-op
- Why this is a stub:
  - Service stop behavior is intentionally not implemented for most services.
  - The code documents shutdown as a no-op until clean kernel shutdown exists.
- Evidence summary:
  - Multiple service stop paths are documented as no-op because clean kernel shutdown is not implemented.

### 6. Provider lifecycle defaults
- File: `seed/saios/src/provider.rs`
- Area: provider initialization, shutdown, and lookup defaults
- Status: No-op defaults
- Why this is a stub:
  - Several trait-style hooks intentionally do nothing or return `None`.
  - They provide a scaffold API surface rather than a complete behavior contract.
- Evidence summary:
  - `initialize` and `shutdown` default to no-op behavior.
  - `lookup` defaults to `None`.

### 7. Synthetic network download payloads
- File: `seed/saios/src/driver/network.rs`
- Area: generated payloads for downloaded executables
- Status: Synthetic placeholder behavior
- Why this is a stub:
  - Some download paths fabricate placeholder binary content.
  - This is not a real network-delivered executable flow.
- Evidence summary:
  - Executable-like paths can resolve to literal `ELF-STUB` payload content.

### 8. Temporary TSS bootstrap values
- File: `hal/src/arch/x86_64/tss.rs`
- Area: TSS initialization
- Status: Temporary bootstrap implementation
- Why this is a stub:
  - Initial TSS stack and IST values are explicitly temporary.
  - The code leaves runtime stacks unset until later setup is available.
- Evidence summary:
  - The file comments mark the initial values as temporary until memory management is ready.

## Not Counted As Stubs

The following items contain the word `stub` or behave as low-level trampolines, but are not counted as incomplete implementations in this inventory:

- `hal/src/arch/x86_64/seed_support.rs`
  - Contains assembly support stubs and IRQ/user-mode trampolines, but these are real low-level implementations.
- `hal/src/arch/x86_64/idt.rs`
  - Contains interrupt stub symbols, but they are actual IDT entry trampolines rather than placeholder logic.
- `boot/uefi/efi_main/src/*`
  - No explicit, code-backed stub/scaffold implementations were identified in the current scan.

## Notes

- No existing `State.md` or `Status.md` file was present in the workspace at the time of this audit.
- The empty root file `stste.md` was left unchanged because it does not appear to be connected to any code path or existing reporting flow.