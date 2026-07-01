# ADR-0016: Shell Serviceization and Session Runtime Architecture

- Status: Accepted
- Date: 2026-07-02
- Complements: ADR-0014, ADR-0015

## Context

SAIOS boot flow previously entered a direct shell loop after kernel bring-up.

That model works for early bring-up but makes shell a boot-special path instead of a service participant under kernel architecture rules.

SAIOS 0.2 architecture requires services to be lifecycle-managed by KSF and executed as scheduled tasks.

## Decision

Adopt shell serviceization under KSF with session-based runtime architecture.

- Shell runtime is started by KSF ShellService start().
- Shell runtime executes in a scheduler-managed thread.
- Boot and seed code do not directly invoke shell loops.
- Shell input path is event-driven through Keyboard IRQ -> Input/Console path.
- Shell command execution uses registry lookup and dispatcher execution over CommandContext.

## Architectural Shape

Firmware
-> Bootloader
-> Kernel init
-> KSF bootstrap
-> Console + Input + Shell services running
-> Shell service thread
-> Scheduler + idle runtime

## Shell Runtime Contracts

- Session owns runtime state: cwd, namespace, environment, history, prompt, user.
- PromptProvider renders prompt text independent of engine loop.
- CommandContext is the single command execution context payload.
- CommandRegistry resolves commands and avoids duplicate registrations.
- CommandDispatcher performs parse -> lookup -> execute.

## Consequences

Positive:

- Shell no longer violates service-layer boundaries.
- Boot flow is cleaner and architecture-compliant.
- Multi-session shell support is unblocked.
- Transition to user-mode shell process is simplified.

Trade-offs:

- Slightly higher shell subsystem complexity.
- Requires explicit service dependency management for shell startup.

## Compliance Requirements

A shell/runtime implementation is compliant when:

- Shell is started via KSF service graph.
- Shell loop is not called directly by boot path.
- Commands execute through dispatcher and context contracts.
- Session state is owned per shell session.

## Related Documents

- [docs/SNSH.md](../SNSH.md)
- [docs/KSF.md](../KSF.md)
- [docs/KernelArchitecture.md](../KernelArchitecture.md)
- [docs/SAIOS-0.2-Foundation.md](../SAIOS-0.2-Foundation.md)
