# SNSH: SAIOS Native Shell

Status: Implemented
Owner: Shell and platform architecture
Last updated: 2026-07-02

## Purpose

SNSH is the primary operator and developer interface for SAIOS.

SNSH is object-centric and routes through SIF and SAIFS contracts instead of calling managers directly.

## Layering

Keyboard
-> Console
-> SNSH Engine
-> Command Registry
-> Query Engine
-> SIF
-> Providers
-> Managers

Key rule:

- SNSH must not call manager internals directly.

## Module Layout

- Engine: command loop and dispatch
- Parser: line tokenization
- Command: command interface contract
- Registry: dynamic command registration and lookup
- Session: shell context and environment/session state
- Native: object-first commands
- Compatibility: POSIX-like compatibility commands

Code locations:

- [seed/saios/src/shell/engine.rs](../seed/saios/src/shell/engine.rs)
- [seed/saios/src/shell/parser.rs](../seed/saios/src/shell/parser.rs)
- [seed/saios/src/shell/command.rs](../seed/saios/src/shell/command.rs)
- [seed/saios/src/shell/registry.rs](../seed/saios/src/shell/registry.rs)
- [seed/saios/src/shell/session.rs](../seed/saios/src/shell/session.rs)
- [seed/saios/src/shell/native.rs](../seed/saios/src/shell/native.rs)
- [seed/saios/src/shell/compatibility.rs](../seed/saios/src/shell/compatibility.rs)

## Command Contract

All commands implement a shared contract:

```rust
pub trait Command {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn execute(&self, ctx: &mut ShellContext, args: &[&str]) -> ShellResult;
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

## Shell Context

Current context payload includes:

- Session state
- Current namespace
- Environment variable store
- Command catalog cache

This enables future additions without changing command signatures:

- user identity and roles
- permission and capability model
- scripting state
- execution context and tracing

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

1. Engine reads input line from console
2. Parser tokenizes into command and args
3. Registry resolves command by name
4. Command executes with mutable ShellContext
5. Command returns ShellResult and engine reports errors

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

## How To Add a Command

1. Add handler in native or compatibility module.
2. Register a StaticCommand entry in that module's register function.
3. Keep command description concise and action-oriented.
4. If it exposes object data, prefer query or inspect contracts over ad hoc traversal.

## Planned Extensions

- completion subsystem
- pipeline and filter graph execution
- command permissions and policy checks
- structured output modes for machine consumers
- remote shell transport over SIF APIs
