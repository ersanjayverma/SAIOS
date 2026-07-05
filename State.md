# SAIOS Stub Inventory

Date: 2026-07-06

This file tracks code that is explicitly stubbed, scaffolded, synthetic, no-op by design, or otherwise intentionally incomplete.

## Current Stub Inventory

### 1. Stub C compiler and compiled-stub execution
- File: `seed/saios/src/shell/programs.rs`
- Area: `cc` shell program and compiled output path
- Status: Incomplete by design
- Why this is a stub:
  - The code explicitly describes `cc` as a stub compiler.
  - It emits marker content rather than producing a real compiled binary.
  - Real compilation and dynamic linking are explicitly deferred.
- Evidence summary:
  - The file documents that the `cc` command is currently a stub.
  - It writes outputs tagged with `SAIOS_CC_STUB` and routes execution through a compiled-stub path.

### 2. Native ext4 write-path scaffolding
- File: `seed/saios/src/driver/storage.rs`
- Area: ext4 create/mkdir/delete/rename and metadata update path
- Status: Scaffolded but incomplete
- Why this is a stub:
  - Several internal helpers are named as scaffolds.
  - User-visible mutating operations return explicit "scaffolded but incomplete" errors.
  - Diagnostics report this path as `mode=stub`.
- Evidence summary:
  - Functions include `ext4_alloc_block_scaffold`, `ext4_alloc_inode_scaffold`, `ext4_write_inode_basic_scaffold`, and `ext4_journal_intent_scaffold`.
  - Mutating operations reject requests with stage-8 scaffold/incomplete messages.

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