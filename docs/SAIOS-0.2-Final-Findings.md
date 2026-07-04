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
