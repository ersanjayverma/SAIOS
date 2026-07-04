# SAIOS v0.2 Final Findings

Date: 2026-07-04
Status: Successful real-hardware progress milestone achieved

## Executive Summary

This cycle resolved the most critical boot instability gap between virtualized and real hardware execution.

The highest-impact result is that kernel startup now survives previously fatal handoff and early paging paths, with deterministic visual diagnostics available even when serial is unavailable.

## What Was Failing

1. Early boot failures occurred after handoff on real hardware even when VBox paths were stable.
2. Silent resets made root-cause isolation difficult when serial output was not available.
3. Early VMM/CR3 assumptions were too strict for firmware-specific mappings and memory behavior.
4. Framebuffer attach path depended on VMM remap behavior that is unsafe in firmware-CR3 fallback mode.

## Core Findings

1. ACPI table parsing could fault when directly dereferencing physical addresses that were not mapped.
2. Validation could stall due to unbounded timer waits on hardware with different timing behavior.
3. Early microcode/MSR probing can fault on real systems even when CPUID indicates MSR support.
4. Early service side effects (USB/PCI/storage/network probing) can block or destabilize boot.
5. Boot-stage observability must include a non-serial channel for hardware triage.
6. CR3 switching inside page-table bootstrap can bypass caller-level fallback and trigger fatal resets.
7. In fallback mode, framebuffer remapping via VMM can fail; direct GOP pointer usage is required.

## Key Stabilization Changes Landed

1. Bootloader framebuffer stage tracing added for pre-EBS, post-EBS, handoff, and pre-jump visibility.
2. Kernel-side framebuffer stage tracing added from entry through memory, VMM, heap, and service phases.
3. VMM bootstrap path hardened for real hardware and decoupled from immediate CR3 activation.
4. Early CR3 switch is disabled by default in fallback mode to prevent bootstrap-time fatal faults.
5. Fallback runtime now proceeds on firmware CR3 with explicit stage markers.
6. Framebuffer attach fallback now uses direct bootloader-provided GOP address instead of VMM remap.
7. ACPI parsing uses mapped physical access patterns rather than raw direct dereference.
8. Validation waits were bounded to avoid infinite stalls.
9. Page-0 reservation and additional early-init hardening reduced nondeterministic faults.

## Evidence of Success

1. Hardware run reached deeper kernel phases than any previous attempt.
2. Visual marker sequence moved beyond prior reset boundary into post-heap/post-timeline region.
3. Repeated release builds of updated kernel paths completed successfully during triage iterations.
4. The latest attach-path fix removed the fallback VMM remap dependency from framebuffer bring-up.

## Residual Risk (Known, Acceptable for v0.2)

1. Boot path currently includes many temporary diagnostics and marker writes.
2. Fallback mode intentionally trades optimal architecture purity for hardware robustness.
3. Some service registration and logging paths remain heavily instrumented.

## v0.3 Plan: Remove Markers and Unwanted Logs

### Goal

Reduce boot-time diagnostic noise while preserving debuggability behind explicit build/runtime flags.

### Scope

1. Remove temporary bootloader and kernel stage marker spam from default boot path.
2. Remove non-essential emergency logs added only for this triage campaign.
3. Keep a minimal, structured, opt-in diagnostics channel for future hardware incidents.

### Work Items

1. Introduce unified debug gate constants for boot and kernel early-init diagnostics.
2. Replace ad-hoc marker calls with a small helper API that compiles out in normal builds.
3. Delete temporary color-band and micro-stage tracing used only for this investigation.
4. Keep only milestone-level logs at subsystem boundaries.
5. Ensure fallback behavior remains functionally identical after instrumentation removal.
6. Re-run real-hardware boot validation after each cleanup step.

### Proposed Execution Order

1. Clean bootloader temporary markers and keep only coarse handoff checkpoints.
2. Clean kernel temporary markers around VMM, heap, timeline, and framebuffer attach.
3. Consolidate serial logs to concise startup milestones.
4. Verify fallback and non-fallback paths produce same functional behavior.
5. Update docs and validation notes with final v0.3 diagnostic policy.

### Acceptance Criteria for v0.3 Cleanup

1. Real hardware boots with no regression relative to current successful state.
2. Default output is concise and does not include triage-only marker noise.
3. Optional debug mode reproduces enough visibility to diagnose early boot failures.
4. Build and validation remain green for bootloader, HAL, and kernel crates.

## Recommended Follow-Up Artifacts

1. Add ADR documenting fallback-mode policy and CR3 activation strategy.
2. Add a short diagnostics policy note defining default vs debug verbosity.
3. Add a validation checklist specific to real-hardware boot parity.
