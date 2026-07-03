use crate::console;

use super::registry::CommandInfo;
use super::session::CommandContext;

pub struct ManualEntry {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub summary: &'static str,
    pub synopsis: &'static [&'static str],
    pub description: &'static [&'static str],
    pub examples: &'static [&'static str],
    pub see_also: &'static [&'static str],
}

macro_rules! manual {
    (
        $name:literal,
        aliases: [$($alias:literal),* $(,)?],
        summary: $summary:literal,
        synopsis: [$($synopsis:literal),* $(,)?],
        description: [$($description:literal),* $(,)?],
        examples: [$($example:literal),* $(,)?],
        see_also: [$($see_also:literal),* $(,)?]
    ) => {
        ManualEntry {
            name: $name,
            aliases: &[$($alias),*],
            summary: $summary,
            synopsis: &[$($synopsis),*],
            description: &[$($description),*],
            examples: &[$($example),*],
            see_also: &[$($see_also),*],
        }
    };
}

static MANUALS: &[ManualEntry] = &[
    manual!(
        "man",
        aliases: [],
        summary: "Show detailed manual pages for shell commands",
        synopsis: ["man", "man <command>"],
        description: [
            "Without arguments, prints a command index.",
            "With a command name, shows the manual page for that command.",
            "Alias names resolve to the canonical command page.",
            "If a command has no hand-written page yet, a generated page is shown."
        ],
        examples: ["man", "man mount", "man ls", "man svc"],
        see_also: ["help", "registry"]
    ),
    manual!(
        "help",
        aliases: [],
        summary: "Show the command catalog in a compact table",
        synopsis: ["help"],
        description: [
            "Prints the current namespace and environment count.",
            "Lists every registered shell command in tabular form.",
            "Use man <command> when you need full usage and examples."
        ],
        examples: ["help", "man help"],
        see_also: ["man", "registry"]
    ),
    manual!(
        "registry",
        aliases: [],
        summary: "Show the full registered command registry",
        synopsis: ["registry"],
        description: [
            "Prints the registered command catalog in tabular form.",
            "Useful for checking whether a command is actually registered.",
            "Pairs well with man when diagnosing shell wiring issues."
        ],
        examples: ["registry", "man registry"],
        see_also: ["help", "man"]
    ),
    manual!(
        "clear",
        aliases: [],
        summary: "Clear the interactive console",
        synopsis: ["clear"],
        description: [
            "Clears the framebuffer and serial-backed shell view.",
            "Does not reset shell state, environment variables, or cwd.",
            "Used automatically once when SNSH enters interactive mode."
        ],
        examples: ["clear"],
        see_also: ["console", "display"]
    ),
    manual!(
        "echo",
        aliases: [],
        summary: "Print text or pipeline input",
        synopsis: ["echo <text...>"],
        description: [
            "Prints its arguments to stdout.",
            "If no arguments are provided and pipeline stdin exists, prints stdin.",
            "Useful in scripts, pipelines, and quick shell diagnostics."
        ],
        examples: ["echo hello", "echo $HOSTNAME"],
        see_also: ["grep", "wc"]
    ),
    manual!(
        "grep",
        aliases: [],
        summary: "Filter pipeline input by substring",
        synopsis: ["grep <needle>"],
        description: [
            "Reads text from pipeline stdin and prints matching lines.",
            "Current implementation performs simple substring filtering.",
            "Use with commands that emit line-oriented text."
        ],
        examples: ["events | grep BOOTCHK", "help | grep mount"],
        see_also: ["echo", "wc", "events"]
    ),
    manual!(
        "wc",
        aliases: [],
        summary: "Count lines, words, and bytes from pipeline input",
        synopsis: ["wc"],
        description: [
            "Consumes pipeline stdin and prints line/word/byte counts.",
            "Useful for basic shell scripting and quick text summaries.",
            "Intended to mirror a familiar compatibility workflow."
        ],
        examples: ["events | wc", "cat /etc/profile | wc"],
        see_also: ["grep", "cat"]
    ),
    manual!(
        "version",
        aliases: [],
        summary: "Print the shell/kernel version banner",
        synopsis: ["version"],
        description: [
            "Shows the current SAIOS shell version string.",
            "Useful in logs, demos, and environment checks.",
            "For ABI details, use syscall or crt."
        ],
        examples: ["version", "syscall abi", "crt abi"],
        see_also: ["syscall", "crt"]
    ),
    manual!(
        "exit",
        aliases: [],
        summary: "Exit the current shell session",
        synopsis: ["exit"],
        description: [
            "Stops the interactive shell loop.",
            "Used for ending a session cleanly from the console.",
            "Does not power off the machine or stop the kernel."
        ],
        examples: ["exit", "shutdown"],
        see_also: ["shutdown", "reboot"]
    ),
    manual!(
        "history",
        aliases: [],
        summary: "Show command history",
        synopsis: ["history"],
        description: [
            "Prints the in-memory command history for the current shell session.",
            "Useful for re-running or auditing recent commands.",
            "History is session-scoped and not yet persisted across boots."
        ],
        examples: ["history"],
        see_also: ["help", "source"]
    ),
    manual!(
        "time",
        aliases: [],
        summary: "Show monotonic shell time",
        synopsis: ["time"],
        description: [
            "Prints monotonic timer information for the running system.",
            "Useful for rough timing and uptime correlation.",
            "Pairs well with timeline and ticks."
        ],
        examples: ["time", "ticks", "timeline"],
        see_also: ["ticks", "uptime", "timeline"]
    ),
    manual!(
        "mem",
        aliases: ["memory"],
        summary: "Show physical memory usage",
        synopsis: ["mem", "memory"],
        description: [
            "Prints memory totals and current allocator/PMM state.",
            "Useful for checking whether boot-time changes increased memory use.",
            "Alias: memory."
        ],
        examples: ["mem", "heap"],
        see_also: ["heap", "dashboard"]
    ),
    manual!(
        "cpu",
        aliases: [],
        summary: "Show CPU identity and capability information",
        synopsis: ["cpu"],
        description: [
            "Prints vendor, brand, and selected CPU capability details.",
            "Useful when validating hardware-specific bring-up.",
            "Pairs well with acpi and pci for platform inspection."
        ],
        examples: ["cpu", "acpi info", "pci"],
        see_also: ["acpi", "pci"]
    ),
    manual!(
        "ps",
        aliases: [],
        summary: "List scheduler threads",
        synopsis: ["ps"],
        description: [
            "Prints the active scheduler thread list.",
            "Useful for low-level scheduler visibility during development.",
            "For process-oriented views, see jobs or taskman."
        ],
        examples: ["ps", "threads", "jobs"],
        see_also: ["threads", "jobs", "taskman"]
    ),
    manual!(
        "jobs",
        aliases: [],
        summary: "List managed processes",
        synopsis: ["jobs"],
        description: [
            "Prints higher-level managed process state rather than raw scheduler threads.",
            "Useful for checking spawned programs and shell-launched work.",
            "Pairs well with kill and wait."
        ],
        examples: ["jobs", "kill 2", "wait 2"],
        see_also: ["ps", "kill", "wait"]
    ),
    manual!(
        "kill",
        aliases: [],
        summary: "Terminate a process by pid",
        synopsis: ["kill <pid>"],
        description: [
            "Requests termination for a managed process.",
            "Use jobs or taskman to discover process ids.",
            "Intended for operator control rather than signal semantics."
        ],
        examples: ["jobs", "kill 3"],
        see_also: ["jobs", "wait", "taskman"]
    ),
    manual!(
        "wait",
        aliases: [],
        summary: "Wait for a process to exit",
        synopsis: ["wait <pid>"],
        description: [
            "Blocks until the selected process exits and reports its code.",
            "Useful in scripts or controlled testing sequences.",
            "Use after spawn or exec when you need explicit completion."
        ],
        examples: ["spawn hello", "wait 4"],
        see_also: ["spawn", "exec", "jobs"]
    ),
    manual!(
        "spawn",
        aliases: [],
        summary: "Spawn a program and return immediately",
        synopsis: ["spawn <program> [args...]"],
        description: [
            "Starts a program and prints the new pid.",
            "Use wait or jobs if you need to observe completion.",
            "Good for background test and shell workflow experiments."
        ],
        examples: ["spawn hello", "spawn cc demo.c"],
        see_also: ["exec", "jobs", "wait"]
    ),
    manual!(
        "exec",
        aliases: [],
        summary: "Execute a program and return its exit code",
        synopsis: ["exec <program> [args...]"],
        description: [
            "Runs a program through the process execution path.",
            "Use this for foreground execution where exit status matters.",
            "Works with seeded binaries and shell demo programs."
        ],
        examples: ["exec hello", "exec cc demo.c"],
        see_also: ["spawn", "wait", "jobs"]
    ),
    manual!(
        "syscall",
        aliases: [],
        summary: "Inspect or smoke-test the stable syscall ABI",
        synopsis: ["syscall", "syscall abi"],
        description: [
            "Displays supported syscall ABI information and probing output.",
            "Useful when validating userland/kernel contract stability.",
            "Pairs well with crt for startup contract checks."
        ],
        examples: ["syscall", "syscall abi"],
        see_also: ["crt", "version"]
    ),
    manual!(
        "crt",
        aliases: [],
        summary: "Inspect the C runtime startup contract",
        synopsis: ["crt", "crt abi"],
        description: [
            "Shows the current CRT startup surface and ABI version details.",
            "Useful when validating user-space entry and argument setup.",
            "Pairs naturally with syscall and exec."
        ],
        examples: ["crt", "crt abi", "exec hello"],
        see_also: ["syscall", "exec"]
    ),
    manual!(
        "pkgimg",
        aliases: [],
        summary: "Inspect or remount the package image scaffold",
        synopsis: ["pkgimg", "pkgimg mount"],
        description: [
            "Reports or refreshes the seeded package image tree used for demos.",
            "This is separate from any real FAT32 volume mounted under /mnt.",
            "Useful for understanding why the root filesystem is still tmpfs-backed."
        ],
        examples: ["pkgimg", "df /", "mount"],
        see_also: ["mount", "df", "ls"]
    ),
    manual!(
        "env",
        aliases: [],
        summary: "List shell environment variables",
        synopsis: ["env"],
        description: [
            "Prints the current shell environment key/value pairs.",
            "Used by shell scripts, interpolation, and process startup helpers.",
            "Modify values with setenv and unsetenv."
        ],
        examples: ["env", "setenv HOSTNAME saios"],
        see_also: ["setenv", "unsetenv", "status"]
    ),
    manual!(
        "setenv",
        aliases: [],
        summary: "Set a shell environment variable",
        synopsis: ["setenv <name> <value>"],
        description: [
            "Creates or updates an environment variable in the current shell.",
            "Values are available to interpolation and command execution.",
            "Shell-local for the current session."
        ],
        examples: ["setenv HOSTNAME saios", "setenv MODE debug"],
        see_also: ["env", "unsetenv", "source"]
    ),
    manual!(
        "unsetenv",
        aliases: [],
        summary: "Remove a shell environment variable",
        synopsis: ["unsetenv <name>"],
        description: [
            "Deletes a variable from the current shell environment.",
            "Useful for cleaning up script state or testing defaults.",
            "Safe to call on variables that may or may not exist."
        ],
        examples: ["unsetenv HOSTNAME"],
        see_also: ["env", "setenv"]
    ),
    manual!(
        "alias",
        aliases: ["aliases", "unalias"],
        summary: "Create, list, or remove command aliases",
        synopsis: ["alias", "alias <name> <value>", "unalias <name>", "aliases"],
        description: [
            "Aliases provide shell-level command substitution shortcuts.",
            "Use alias with no arguments to list aliases.",
            "Use unalias to remove one alias by name."
        ],
        examples: ["alias ll ls", "aliases", "unalias ll"],
        see_also: ["help", "source"]
    ),
    manual!(
        "status",
        aliases: [],
        summary: "Show the last shell exit code",
        synopsis: ["status"],
        description: [
            "Prints the last command exit code tracked by the shell session.",
            "Useful when chaining commands or debugging script behavior.",
            "Pairs well with exec and source."
        ],
        examples: ["status", "exec hello", "status"],
        see_also: ["exec", "source"]
    ),
    manual!(
        "source",
        aliases: ["."],
        summary: "Run a script in the current shell context",
        synopsis: ["source <path>", ". <path>"],
        description: [
            "Executes a script file line-by-line inside the current shell session.",
            "Environment changes and aliases persist after the script returns.",
            "Relative paths are resolved from the current working directory."
        ],
        examples: ["source /system/init", ". ./script.sh"],
        see_also: ["setenv", "alias", "cd"]
    ),
    manual!(
        "dashboard",
        aliases: ["dash"],
        summary: "Show one-page system readiness summary",
        synopsis: ["dashboard", "dash"],
        description: [
            "Prints a condensed readiness view of boot state, health, and activity.",
            "Useful for demos and quick status checks after boot.",
            "Prefer taskman, service, or timeline for deeper inspection."
        ],
        examples: ["dashboard", "dash"],
        see_also: ["health", "timeline", "taskman"]
    ),
    manual!(
        "objects",
        aliases: ["obj"],
        summary: "Inspect the object manager namespace",
        synopsis: ["objects", "objects <type>", "obj"],
        description: [
            "Lists object information sourced from the kernel object manager.",
            "Useful for validating provider wiring and runtime visibility.",
            "Use inspect, describe, explain, or diagnose for single-object detail."
        ],
        examples: ["objects", "obj service", "inspect system"],
        see_also: ["inspect", "describe", "providers"]
    ),
    manual!(
        "providers",
        aliases: [],
        summary: "List registered namespace/data providers",
        synopsis: ["providers"],
        description: [
            "Prints the providers backing the object manager and shell views.",
            "Useful when diagnosing missing device, process, or storage entries.",
            "Pairs with objects and inspect."
        ],
        examples: ["providers", "objects", "inspect storage"],
        see_also: ["objects", "inspect"]
    ),
    manual!(
        "devices",
        aliases: ["dev"],
        summary: "List registered devices",
        synopsis: ["devices", "dev"],
        description: [
            "Prints device-manager device records.",
            "Useful for checking console, framebuffer, storage, and other device visibility.",
            "Pairs with drivers and pci."
        ],
        examples: ["devices", "dev", "pci"],
        see_also: ["drivers", "pci", "console"]
    ),
    manual!(
        "drivers",
        aliases: ["drv", "driver"],
        summary: "List or inspect drivers",
        synopsis: ["drivers", "drv", "driver <name>"],
        description: [
            "Shows registered drivers or a specific driver record.",
            "Useful during bring-up when a service depends on driver state.",
            "Use reload to restart a driver implementation where supported."
        ],
        examples: ["drivers", "driver fat32", "reload fat32"],
        see_also: ["devices", "reload", "service"]
    ),
    manual!(
        "service",
        aliases: ["svc", "services", "svcs", "restart"],
        summary: "Manage and inspect kernel services",
        synopsis: ["service", "service list", "service start <name>", "service stop <name>", "service restart <name>", "restart <name>"],
        description: [
            "Controls the Kernel Service Framework lifecycle for registered services.",
            "Use list and health-style subcommands to inspect service state.",
            "Aliases svc, services, svcs, and restart route here."
        ],
        examples: ["service list", "svc start shell", "restart shell"],
        see_also: ["health", "timeline", "drivers"]
    ),
    manual!(
        "reload",
        aliases: [],
        summary: "Reload a driver",
        synopsis: ["reload <driver>"],
        description: [
            "Requests a driver restart path where supported by the runtime.",
            "Useful for iterative development and recovery testing.",
            "Inspect driver state first with drivers or driver."
        ],
        examples: ["reload fat32", "driver fat32"],
        see_also: ["driver", "drivers", "service"]
    ),
    manual!(
        "test",
        aliases: ["verify"],
        summary: "Run test suites or invariant checks",
        synopsis: ["test", "test <suite>", "verify", "verify <area>"],
        description: [
            "Runs internal test and verification helpers from the kernel runtime.",
            "Use verify when you want invariant-oriented output rather than test framing.",
            "Common targets include memory, scheduler, object, service, and saifs."
        ],
        examples: ["test all", "verify console", "verify service"],
        see_also: ["health", "timeline"]
    ),
    manual!(
        "query",
        aliases: [],
        summary: "Run an object query expression",
        synopsis: ["query <expression>"],
        description: [
            "Queries the object subsystem for matching records.",
            "Useful for filtering runtime state without manually traversing views.",
            "Pairs with inspect, objects, and providers."
        ],
        examples: ["query kind=Service", "query health=Warning"],
        see_also: ["objects", "inspect", "providers"]
    ),
    manual!(
        "inspect",
        aliases: ["describe", "diagnose", "explain"],
        summary: "Inspect and explain object state",
        synopsis: ["inspect <path>", "describe <path>", "diagnose <path>", "explain <path>"],
        description: [
            "These commands provide progressively richer views of a single object.",
            "Use inspect for raw object details, describe for higher-level metadata,
and diagnose or explain for reasoning-oriented output.",
            "They are central to the self-describing object model."
        ],
        examples: ["inspect system", "describe devices/pci0", "diagnose services/shell"],
        see_also: ["objects", "query", "health"]
    ),
    manual!(
        "health",
        aliases: [],
        summary: "Show overall system health",
        synopsis: ["health"],
        description: [
            "Prints a health summary synthesized from system objects and services.",
            "Useful as a high-level readiness indicator.",
            "Use diagnose for deeper per-object analysis."
        ],
        examples: ["health", "dashboard"],
        see_also: ["diagnose", "dashboard", "service"]
    ),
    manual!(
        "events",
        aliases: ["ev", "logs"],
        summary: "Show recent kernel events/logs",
        synopsis: ["events", "events <limit>", "logs"],
        description: [
            "Prints recent object-manager or kernel events.",
            "Useful for boot tracing, service debugging, and live activity review.",
            "Aliases ev and logs route to the same handler."
        ],
        examples: ["events", "events 20", "logs | grep BOOTCHK"],
        see_also: ["timeline", "health", "grep"]
    ),
    manual!(
        "mount",
        aliases: ["umount", "df"],
        summary: "Manage mounted filesystems and storage views",
        synopsis: ["mount", "mount list", "mount scan", "mount <volume> <path> [ro]", "umount <path>", "df [path]"],
        description: [
            "Lists storage volumes and mount records, rescans storage, or mounts a volume.",
            "The system root remains tmpfs-backed; real FAT32 media is mounted separately.",
            "Use df to inspect filesystem usage for a mounted path."
        ],
        examples: ["mount", "mount disk0p1 /mnt/disk0p1", "df /mnt/disk0p1", "umount /mnt/disk0p1"],
        see_also: ["ls", "pwd", "diskpart"]
    ),
    manual!(
        "graph",
        aliases: ["gr"],
        summary: "Show dependency or object graph views",
        synopsis: ["graph", "graph services", "gr"],
        description: [
            "Prints graph-oriented views of service or object relationships.",
            "Useful when checking dependency order and runtime topology.",
            "Pairs well with timeline and service."
        ],
        examples: ["graph services", "gr"],
        see_also: ["service", "timeline", "objects"]
    ),
    manual!(
        "timeline",
        aliases: ["tl"],
        summary: "Show boot and service timeline milestones",
        synopsis: ["timeline", "tl"],
        description: [
            "Prints the timeline markers collected during boot and service bring-up.",
            "Useful for understanding startup order and performance drift.",
            "Pairs well with events and dashboard."
        ],
        examples: ["timeline", "tl"],
        see_also: ["events", "dashboard", "service"]
    ),
    manual!(
        "tree",
        aliases: [],
        summary: "Render a SAIFS directory tree",
        synopsis: ["tree [path]"],
        description: [
            "Prints a recursive tree view for a directory path.",
            "Useful for confirming package-image state and mounted FAT32 content.",
            "Works well with ls and pwd."
        ],
        examples: ["tree /", "tree /mnt/disk0p1"],
        see_also: ["ls", "pwd", "mount"]
    ),
    manual!(
        "threads",
        aliases: ["uptime", "ticks", "irq", "heap"],
        summary: "Inspect scheduler, timer, interrupt, or heap runtime state",
        synopsis: ["threads", "uptime", "ticks", "irq", "heap"],
        description: [
            "These commands expose focused runtime internals for scheduler, time,
interrupt, and heap diagnostics.",
            "Use them when bringing up or profiling low-level subsystems.",
            "Pairs well with mem, ps, and timeline."
        ],
        examples: ["threads", "uptime", "ticks", "heap"],
        see_also: ["mem", "ps", "timeline"]
    ),
    manual!(
        "pci",
        aliases: [],
        summary: "List detected PCI devices",
        synopsis: ["pci"],
        description: [
            "Prints the cached PCI enumeration results.",
            "Useful for confirming device discovery used by storage and network paths.",
            "Pairs well with devices and drivers."
        ],
        examples: ["pci", "devices", "drivers"],
        see_also: ["devices", "drivers", "net"]
    ),
    manual!(
        "net",
        aliases: ["dhcp", "ping", "wget"],
        summary: "Inspect and exercise the network stack",
        synopsis: ["net", "dhcp", "ping <host>", "wget <url> [path]"],
        description: [
            "Network commands cover status, DHCP renewal, ICMP echo, and HTTP download helpers.",
            "Use them to validate end-to-end network wiring after boot.",
            "Pairs naturally with events and providers."
        ],
        examples: ["net", "dhcp", "ping 192.168.1.1", "wget http://host/file /tmp/file"],
        see_also: ["providers", "events"]
    ),
    manual!(
        "shutdown",
        aliases: ["reboot"],
        summary: "Power control commands",
        synopsis: ["shutdown", "reboot"],
        description: [
            "shutdown halts the machine; reboot requests an immediate hardware reset path.",
            "Use with care on real hardware.",
            "Best used once diagnostics or demos are complete."
        ],
        examples: ["shutdown", "reboot"],
        see_also: ["service", "health"]
    ),
    manual!(
        "sairu",
        aliases: ["recover", "rcv"],
        summary: "Diagnostics and recovery tooling",
        synopsis: ["sairu ...", "recover", "rcv"],
        description: [
            "SAIRU exposes deeper diagnostic or recovery flows.",
            "recover/rcv trigger automated recovery actions.",
            "Use after observing degraded health, service failures, or test regressions."
        ],
        examples: ["sairu", "recover"],
        see_also: ["health", "diagnose", "service"]
    ),
    manual!(
        "ls",
        aliases: ["pwd", "cd", "mkdir", "touch", "cat", "rm", "cp", "mv"],
        summary: "Compatibility filesystem commands",
        synopsis: [
            "ls [path]",
            "pwd",
            "cd <path>",
            "mkdir <path>",
            "touch <path>",
            "cat <path>",
            "rm <path>",
            "cp <src> <dst>",
            "mv <src> <dst>"
        ],
        description: [
            "These commands provide a familiar POSIX-like surface over SAIFS/VFS operations.",
            "Paths are resolved relative to the current working directory unless absolute.",
            "Mounted FAT32 volumes appear under their mount points, not at /."
        ],
        examples: ["pwd", "ls /mnt", "cd /mnt/disk0p1", "cp /etc/profile /tmp/profile.bak"],
        see_also: ["mount", "tree", "man"]
    ),
    manual!(
        "console",
        aliases: ["display", "fb"],
        summary: "Inspect console and framebuffer state",
        synopsis: ["console", "console clear", "display", "fb"],
        description: [
            "console shows terminal geometry, cursor state, scrollback, and framebuffer attachment.",
            "display/fb show framebuffer geometry and flush-path details.",
            "Useful during early bring-up and rendering regression work."
        ],
        examples: ["console", "console clear", "display", "fb"],
        see_also: ["clear", "events", "timeline"]
    ),
    manual!(
        "acpi",
        aliases: [],
        summary: "Inspect ACPI platform information",
        synopsis: ["acpi info", "acpi proc", "acpi tables", "acpi status", "acpi shutdown"],
        description: [
            "Shows ACPI-discovered platform state including processors and tables.",
            "Useful for hardware validation on real machines.",
            "May also expose shutdown-related control paths."
        ],
        examples: ["acpi info", "acpi tables", "acpi status"],
        see_also: ["cpu", "pci", "shutdown"]
    ),
];

fn find_manual(name: &str) -> Option<&'static ManualEntry> {
    MANUALS.iter().find(|entry| {
        entry.name.eq_ignore_ascii_case(name)
            || entry
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

fn find_command<'a>(ctx: &'a CommandContext, name: &str) -> Option<&'a CommandInfo> {
    ctx.command_catalog
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(name))
}

fn print_section(title: &str, lines: &[&str]) {
    if lines.is_empty() {
        return;
    }
    console::println!("{}[0m", "");
    console::println!("{}", title);
    for line in lines {
        console::println!("  {}", line);
    }
}

fn print_see_also(names: &[&str]) {
    if names.is_empty() {
        return;
    }
    let mut line = alloc::string::String::new();
    for (idx, name) in names.iter().enumerate() {
        if idx != 0 {
            line.push_str(", ");
        }
        line.push_str(name);
    }
    console::println!("");
    console::println!("SEE ALSO");
    console::println!("  {}", line);
}

fn print_manual_entry(entry: &ManualEntry) {
    console::println!("NAME");
    console::println!("  {} - {}", entry.name, entry.summary);
    print_section("SYNOPSIS", entry.synopsis);
    print_section("DESCRIPTION", entry.description);
    print_section("EXAMPLES", entry.examples);
    print_see_also(entry.see_also);
}

fn print_generated_manual(info: &CommandInfo) {
    console::println!("NAME");
    console::println!("  {} - {}", info.name, info.description);
    console::println!("");
    console::println!("SYNOPSIS");
    console::println!("  {} [args...]", info.name);
    console::println!("");
    console::println!("DESCRIPTION");
    console::println!("  {}", info.description);
    console::println!("  This page is generated from the command registry.");
    console::println!("  Add a dedicated entry in shell/man.rs for richer usage text.");
}

pub fn print_index(ctx: &CommandContext) {
    let name_width = ctx
        .command_catalog
        .iter()
        .map(|item| item.name.len())
        .max()
        .unwrap_or(7)
        .max(7);

    console::println!("MANUAL TOPICS");
    console::println!(
        "{:<width$}  DESCRIPTION",
        "COMMAND",
        width = name_width
    );
    console::println!(
        "{:-<width$}  {:-<11}",
        "",
        "",
        width = name_width
    );
    for item in &ctx.command_catalog {
        console::println!(
            "{:<width$}  {}",
            item.name,
            item.description,
            width = name_width
        );
    }
}

pub fn print_command(ctx: &CommandContext, name: &str) -> Result<(), &'static str> {
    if let Some(entry) = find_manual(name) {
        print_manual_entry(entry);
        return Ok(());
    }

    if let Some(info) = find_command(ctx, name) {
        print_generated_manual(info);
        return Ok(());
    }

    Err("man: command not found")
}