# SNSH: SAIOS Native Shell (SISH Service)

Status: Implemented (service-based runtime)
Owner: Shell and platform architecture
Last updated: 2026-07-02

## Purpose

SNSH (SISH runtime) is the primary operator and developer interface for SAIOS.

SNSH is object-centric and routes through SIF and SAIFS contracts instead of calling managers directly.

SNSH is started as a kernel service by KSF and runs as a scheduled shell thread, not as a special boot loop.

## Layering

Keyboard IRQ
-> Input Service
-> Console
-> SNSH Session Engine
-> Command Dispatcher
-> Command Registry
-> Query Engine
-> SIF
-> Providers
-> Managers

Key rule:

- SNSH must not call manager internals directly.
- SNSH must not read keyboard drivers directly.

## Module Layout

- Engine: command loop and dispatch
- Dispatcher: registry lookup and command execution
- Parser: line tokenization
- Command: command interface contract
- Registry: dynamic command registration and lookup
- Session: cwd, namespace, environment, history, prompt, and user state
- Prompt: prompt provider contract and implementation
- Service: KSF entry point that spawns shell runtime thread
- Commands: modular command plugins
- Native: object-first commands
- Compatibility: POSIX-like compatibility commands

Code locations:

- [seed/saios/src/shell/engine.rs](../seed/saios/src/shell/engine.rs)
- [seed/saios/src/shell/dispatcher.rs](../seed/saios/src/shell/dispatcher.rs)
- [seed/saios/src/shell/parser.rs](../seed/saios/src/shell/parser.rs)
- [seed/saios/src/shell/command.rs](../seed/saios/src/shell/command.rs)
- [seed/saios/src/shell/registry.rs](../seed/saios/src/shell/registry.rs)
- [seed/saios/src/shell/session.rs](../seed/saios/src/shell/session.rs)
- [seed/saios/src/shell/prompt.rs](../seed/saios/src/shell/prompt.rs)
- [seed/saios/src/shell/service.rs](../seed/saios/src/shell/service.rs)
- [seed/saios/src/shell/commands/](../seed/saios/src/shell/commands/)
- [seed/saios/src/shell/native.rs](../seed/saios/src/shell/native.rs)
- [seed/saios/src/shell/compatibility.rs](../seed/saios/src/shell/compatibility.rs)

## Command Contract

All commands implement a shared contract:

```rust
pub trait Command {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn execute(&self, ctx: &mut CommandContext, args: &[&str]) -> ShellResult;
}
```

Implications:

- No giant command switch required in engine code.
- Commands are self-describing.
- Help output is data-driven from registered commands.

## Command Registry

Registry responsibilities:

- Register command implementations at boot/runtime
- Resolve command by name
- Enumerate registered command metadata

Notes:

- Lookup is case-insensitive.
- Registration order is independent from help output ordering.
- Help output uses sorted command metadata list.

## Shell Session and Context

Session payload includes:

- Running state
- Current working directory
- Current namespace
- Environment variable store
- Command history
- Prompt template
- Current user (optional)

CommandContext includes:

- Session payload
- Command catalog cache

This enables additions without changing command signatures:

- user identity and roles
- permission and capability model
- scripting state
- execution context and tracing

## Prompt Provider

Prompt rendering is provider-based:

```rust
pub trait PromptProvider {
    fn render(&self) -> String;
}
```

Current implementation uses session-backed prompt text, and can switch to user/path-aware prompts without changing engine logic.

## Command Sets

### Native Commands

Primary object-first interface:

- help
- version
- clear
- exit
- objects
- providers
- service
- test
- verify
- query
- inspect
- describe
- health
- diagnose
- explain
- events
- logs
- mount
- threads
- uptime
- ticks
- heap
- pci
- shutdown
- reboot

Service subcommands:

- service list
- service start <name>
- service stop <name>
- service restart <name>
- service health
- service info <name>

Validation commands:

- test
- test memory
- test scheduler
- test console
- test object
- test saifs
- test all
- verify memory
- verify scheduler
- verify console
- verify object
- verify service
- verify saifs
- verify all

### Compatibility Commands

POSIX-like compatibility surface:

- ls
- pwd
- cd
- mkdir
- touch
- cat
- rm

Compatibility commands are intentionally isolated from the native command set implementation.

## Execution Flow

1. KSF starts shell service and spawns shell thread.
2. Engine renders prompt via PromptProvider.
3. Engine receives console input events.
4. Parser tokenizes into command and args.
5. Dispatcher resolves command in registry.
6. Command executes with mutable CommandContext.
7. ShellResult is reported to console.

## Error Model

Shell commands return:

```rust
pub type ShellResult = Result<(), &'static str>;
```

Guideline:

- Return concise operator-facing errors.
- Avoid manager/provider internals leaking into shell UX.

## Integration Rules

- Native discovery operations use object query and inspection APIs.
- Namespace/file operations use SAIFS.
- Shell commands should never call HAL-level APIs except explicit system control commands such as reboot/shutdown.
- Shell runtime should not own boot control flow.

## Command Modules

Built-ins are modular command plugins under `shell/commands/` and each module implements only command behavior.

Current core modules include:

- help
- clear
- version
- objects
- inspect
- health
- shutdown
- reboot

## How To Add a Command

1. Add module under `shell/commands/` (or native/compatibility family as appropriate).
2. Register a StaticCommand entry in that module's register function.
3. Keep command description concise and action-oriented.
4. If it exposes object data, prefer query or inspect contracts over ad hoc traversal.

## Runtime Placement

Boot runtime shape:

Firmware
-> Bootloader
-> Kernel init
-> KSF service startup
-> Shell service start
-> Scheduler/idle runtime

After this handoff, boot code no longer invokes shell loops directly.

## Planned Extensions

- completion subsystem
- pipeline and filter graph execution
- command permissions and policy checks
- structured output modes for machine consumers
- remote shell transport over SIF APIs
