# SNSH: SAIOS Native Shell (SISH Service)

Status: Implemented (service-based runtime)
Owner: Shell and platform architecture
Last updated: 2026-08-25

## Default shell

**SNSH is the canonical default interactive shell for SAIOS.**

The kernel shell service exports `DEFAULT_SHELL = "snsh"`, records the interactive shell process under that identity, and launches the SNSH session after `/system/init` completes. This keeps boot initialization separate from the operator shell.

The shell service is implemented in [`seed/saios/src/shell/service.rs`](../seed/saios/src/shell/service.rs).

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

- help, registry
- echo, grep, wc
- version
- clear
- exit
- history
- time
- mem, memory
- cpu
- ps, jobs
- kill, wait
- spawn, exec
- env, setenv, unsetenv, status
- alias, unalias, aliases
- source, .
- syscall
- crt
- pkgimg
- dashboard, dash
- objects, obj
- providers
- devices, dev
- drivers, drv, driver
- service, svc, services, svcs, restart
- reload
- test, verify
- query
- inspect, describe
- health, diagnose, explain
- events, ev, logs
- graph, gr
- timeline, tl
- tree
- mount, umount
- threads
- uptime, ticks, irq
- heap
- pci
- sairu
- recover, rcv
- shutdown, reboot

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

## Process Execution Contract

`exec` is the primary process execution entry point.

Capabilities:

- Program name + positional argument forwarding
- Temporary inline environment overlay (`KEY=VALUE` prefix on `exec`)
- Shell-level environment store (`setenv`, `unsetenv`, `env`)
- Exit code capture in session state
- Last exit status query via `status`

`spawn` starts a process and returns PID through shell output.

The compatibility `shell` binary is **not** the default interactive shell. Interactive sessions belong to the SNSH service and are recorded as process name `snsh`.

## SISH Language Features

The SNSH parser supports:

- statements separated by `;`
- pipelines using `|`
- stdout/stdin redirection
- environment expansion
- inline environment overlays
- aliases
- sourced scripts
- tab completion

## Execution Flow

1. KSF starts the shell service and spawns the shell thread.
2. `/system/init` is started as PID 1 and sourced for system initialization.
3. PID 1 is completed.
4. The process manager creates the canonical shell process with name `snsh`.
5. SNSH renders the prompt and owns the interactive console session.
6. The parser and dispatcher execute native commands or fall back to `/bin/<name>` / explicit paths.
7. Exit status is retained in the session for diagnostics.

## Integration Rules

- Native discovery operations use object query and inspection APIs.
- Namespace/file operations use SAIFS.
- Shell commands should never call HAL-level APIs except explicit system control commands such as reboot/shutdown.
- Shell runtime should not own boot control flow.

## Runtime Placement

```text
Firmware
  -> Bootloader
  -> Kernel init
  -> KSF service startup
  -> SNSH shell service
  -> /system/init
  -> snsh interactive session
  -> Scheduler / idle runtime
```

After the `/system/init` handoff, boot code no longer owns the interactive shell loop.
