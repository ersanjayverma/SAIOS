# Boot Paging & CR3 Investigation

Status: Engineering investigation

## Why this exists

SAIOS has reached deeper kernel phases on real hardware, but early paging and address-space transitions have historically been a major source of opaque resets. This note records the current failure model and the engineering rules that follow from it.

The repository's v0.2 findings identify several related hazards: strict early VMM/CR3 assumptions, CR3 switching during page-table bootstrap, framebuffer remapping while operating under firmware-CR3 fallback, and raw access to physical ACPI addresses. See `docs/SAIOS-0.2-Final-Findings.md` for the recorded findings.

## Observed failure pattern

The important distinction is between **building page tables** and **activating page tables**.

A page-table root can be constructed successfully while switching CR3 to that root still causes an immediate failure. After CR3 changes, every instruction fetch, stack access, global/static access, interrupt path, and device mapping must be valid in the newly active address space.

That makes an early CR3 switch a much stronger operation than simply preparing mappings.

## Current stabilization model

The v0.2 findings document the following stabilization decisions:

1. VMM bootstrap was decoupled from immediate CR3 activation.
2. Early CR3 switching is disabled by default in fallback mode.
3. Fallback execution can continue on the firmware CR3 with explicit stage markers.
4. Framebuffer fallback uses the bootloader-provided GOP address instead of requiring a VMM remap.
5. ACPI parsing uses mapped physical-access patterns rather than raw physical dereferences.
6. Validation waits are bounded so hardware timing cannot turn initialization into an infinite stall.

These are not merely boot hacks: they establish a safer sequencing rule for future memory-management work.

## Engineering rule

> **Map first. Validate second. Activate last.**

Before activating a new CR3, SAIOS should be able to demonstrate that the new address space contains the mappings required by the currently executing context.

At minimum, validation should cover:

- current instruction address
- current stack range
- kernel text and read-only data
- writable kernel data
- heap/allocator metadata used immediately after the switch
- interrupt/exception entry paths
- required boot/runtime framebuffer mappings
- any physical-memory window used by early hardware discovery

## Diagnostic contract

A paging failure should answer five questions without requiring a debugger attached to the machine:

1. Which CR3 was active?
2. What virtual address faulted?
3. What operation caused the fault (read/write/instruction)?
4. Which page-table level stopped the translation?
5. Which boot stage performed the access?

The existing SAIOS direction already treats non-serial visual diagnostics as important for real-hardware triage. The next step is to make page-table diagnostics structured and reusable rather than scattering one-off markers through the boot path.

## Next technical step

The strongest follow-up is a small page-table validation routine that can walk the active hierarchy for a supplied virtual address and report the first missing or incompatible entry before a CR3 transition.

That routine should be usable in:

- boot-time validation
- process address-space creation
- ELF user-space setup
- page-fault diagnostics
- regression tests for memory isolation

The goal is not to avoid CR3 isolation. The goal is to make CR3 isolation **provable before activation**.

## Source of truth

This document intentionally summarizes the repository's existing v0.2 findings rather than presenting old console logs as current measurements. New fault addresses, page-table entries, and hardware results should be added only when reproduced against the current tree.
