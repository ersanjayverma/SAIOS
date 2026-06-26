//! SAIOS manual pages.
//! Installed at boot to /usr/share/man/man1/<cmd>.1
//! The `man` shell command reads and renders them.

pub fn install() {
    let dir = "/usr/share/man/man1";
    crate::mkdir_p_pub(dir);
    // Write each page through VFS
    for (name, content) in PAGES {
        let path = alloc::format!("{}/{}.1", dir, name);
        write_page(&path, content);
    }
    crate::println!("[man] {} manual pages installed", PAGES.len());
}

fn write_page(path: &str, content: &str) {
    let _ = crate::vfs_contract::VfsContract::write_file(path, content.as_bytes(), 0o644);
}

/// Render a man page: bold headers (lines starting with uppercase), indent body.
pub fn render(content: &str) {
    for line in content.lines() {
        if line.starts_with(|c: char| c.is_uppercase()) && !line.starts_with(' ') {
            // Header - print in bold via ANSI on serial, plain on VGA
            crate::println!("\x1b[1m{}\x1b[0m", line);
        } else {
            crate::println!("{}", line);
        }
    }
}

// -- Page database ----------------------------------------------------------

static PAGES: &[(&str, &str)] = &[
    ("help", MAN_HELP),
    ("append", MAN_FILESYSTEM_EXTRA),
    ("cd", MAN_FILESYSTEM_EXTRA),
    ("pwd", MAN_FILESYSTEM_EXTRA),
    ("chmod", MAN_ADMIN),
    ("chown", MAN_ADMIN),
    ("diag", MAN_DIAG),
    ("kds", MAN_KDS),
    ("obs", MAN_OBS),
    ("storage", MAN_STORAGE),
    ("sairu", MAN_SAIRU),
    ("set", MAN_SCRIPTING_EXTRA),
    ("todo", MAN_SCRIPTING_EXTRA),
    ("notes", MAN_SCRIPTING_EXTRA),
    ("id", MAN_ADMIN),
    ("whoami", MAN_ADMIN),
    ("users", MAN_ADMIN),
    ("login", MAN_ADMIN),
    ("logout", MAN_ADMIN),
    ("su", MAN_ADMIN),
    ("passwd", MAN_ADMIN),
    ("useradd", MAN_ADMIN),
    ("userdel", MAN_ADMIN),
    ("testsaios", MAN_USERSPACE_TESTS),
    ("cpus", MAN_SYSTEM_EXTRA),
    ("clear", MAN_SYSTEM_EXTRA),
    ("sysinfo", MAN_SYSTEM_EXTRA),
    ("resmon", MAN_SYSTEM_EXTRA),
    ("irqinfo", MAN_SYSTEM_EXTRA),
    ("kbreset", MAN_SYSTEM_EXTRA),
    ("jobs", MAN_SYSTEM_EXTRA),
    ("journal", MAN_SYSTEM_EXTRA),
    ("verify", MAN_SYSTEM_EXTRA),
    ("recover", MAN_STORAGE),
    ("setup", MAN_SERVICES),
    ("bash", MAN_SERVICES),
    ("sh", MAN_SERVICES),
    ("beep", MAN_SERVICES),
    ("gfx", MAN_SERVICES),
    ("reload", MAN_SERVICES),
    ("lsusb", MAN_SERVICES),
    ("config", MAN_SERVICES),
    ("curl", MAN_DEVTOOLS),
    ("wget", MAN_DEVTOOLS),
    ("openssl", MAN_DEVTOOLS),
    ("ssh", MAN_DEVTOOLS),
    ("vi", MAN_DEVTOOLS),
    ("nano", MAN_DEVTOOLS),
    ("apt", MAN_DEVTOOLS),
    ("make", MAN_DEVTOOLS),
    ("build-essential", MAN_DEVTOOLS),
    ("wifi", MAN_NET),
    ("ls", MAN_LS),
    ("cat", MAN_CAT),
    ("write", MAN_WRITE),
    ("mkdir", MAN_MKDIR),
    ("rm", MAN_RM),
    ("cp", MAN_CP),
    ("mv", MAN_MV),
    ("find", MAN_FIND),
    ("grep", MAN_GREP),
    ("hexdump", MAN_HEXDUMP),
    ("wc", MAN_WC),
    ("df", MAN_DF),
    ("echo", MAN_ECHO),
    ("calc", MAN_CALC),
    ("run", MAN_RUN),
    ("env", MAN_ENV),
    ("uname", MAN_UNAME),
    ("cpuinfo", MAN_CPUINFO),
    ("meminfo", MAN_MEMINFO),
    ("lspci", MAN_LSPCI),
    ("uptime", MAN_UPTIME),
    ("color", MAN_COLOR),
    ("net", MAN_NET),
    ("fetch", MAN_FETCH),
    ("ai", MAN_AI),
    ("cc", MAN_CC),
    ("explain", MAN_EXPLAIN),
    ("exec", MAN_EXEC),
    ("ps", MAN_PS),
       ("kill", MAN_KILL),
    ("install", MAN_INSTALL),
    ("update", MAN_UPDATE),
    ("reinstall", MAN_REINSTALL),
    ("history", MAN_HISTORY),
    ("man", MAN_MAN),
    ("reboot", MAN_REBOOT),
    ("halt", MAN_HALT),
];

// -- Individual pages -------------------------------------------------------

const MAN_MAN: &str = "\
MAN(1)                    SAIOS Manual                    MAN(1)

NAME
       man - display manual pages

SYNOPSIS
       man <command>

DESCRIPTION
       Display the manual page for a built-in SAIOS command.
       Manual pages are stored in /usr/share/man/man1/.

EXAMPLES
       man ls        Show the ls manual page
       man ai        Show the ai command manual page

SEE ALSO
       help(1)
";

const MAN_FILESYSTEM_EXTRA: &str = "\
FILESYSTEM(1)             SAIOS Manual             FILESYSTEM(1)

NAME
       append, chmod, chown - filesystem mutation helpers

SYNOPSIS
       append <file> <text>
       chmod <mode> <path>
       chown <uid> <path>

DESCRIPTION
       Filesystem helper commands route through VfsContract and the identity
       permission model. append writes text to an existing or new ramfs file;
       chmod and chown update permission metadata where the mounted filesystem
       supports it.

EXAMPLES
       append /home/todo.txt check storage
       chmod 755 /bin/tool
       chown 0 /etc/passwd

SEE ALSO
       cat(1), write(1), ls(1), rm(1)
";

const MAN_SCRIPTING_EXTRA: &str = "\
SCRIPTING(1)              SAIOS Manual              SCRIPTING(1)

NAME
       set, todo, notes - shell state and note commands

SYNOPSIS
       set <key> <value>
       todo <text>
       notes

DESCRIPTION
       These commands expose small shell-owned state helpers. set updates the
       shell environment, todo appends a line to /home/todo.txt, and notes
       prints the current note file.

EXAMPLES
       set PATH /bin:/usr/bin
       todo rerun testsaios
       notes

SEE ALSO
       env(1), run(1), history(1)
";

const MAN_DIAG: &str = "\
DIAG(1)                   SAIOS Manual                   DIAG(1)

NAME
       diag - inspect or toggle kernel diagnostic probes

SYNOPSIS
       diag
       diag sched on|off
       diag proc on|off
       diag freeze [seconds]

DESCRIPTION
       diag reports debug flags, IRQ counters, heartbeat state, and watchdog
       timing. Subcommands toggle scheduler/process tracing or run a bounded
       freeze reproduction loop for watchdog validation.

EXAMPLES
       diag
       diag sched on
       diag freeze 5

SEE ALSO
       kds(1), verify(1), storage(1)
";

const MAN_KDS: &str = "\
KDS(1)                    SAIOS Manual                    KDS(1)

NAME
       kds - inspect kernel data service streams

SYNOPSIS
       kds [health|events|metrics|traces|objects|state]

DESCRIPTION
       kds prints recent observability data from the storage-independent KDS
       streams used by contracts and runtime diagnostics.

EXAMPLES
       kds
       kds events
       kds metrics

SEE ALSO
       obs(1), diag(1), verify(1)
";

const MAN_OBS: &str = "\
OBS(1)                    SAIOS Manual                    OBS(1)

NAME
       obs - inspect observability contract evidence

SYNOPSIS
       obs last <contract>
       obs trace <correlation_id>
       obs gaps

DESCRIPTION
       Observability commands read contract-backed evidence, traces, and gap
       reports without depending on writable storage. Runtime observability
       validation is part of testsaios.

EXAMPLES
       obs gaps
       obs last scheduler

SEE ALSO
       kds(1), diag(1), verify(1), testsaios(1)
";

const MAN_STORAGE: &str = "\
STORAGE(1)                SAIOS Manual                STORAGE(1)

NAME
       storage, recover - StoragePlatformContract diagnostics and planning

SYNOPSIS
       storage <command>
       recover

DESCRIPTION
       storage is the shell surface for discovery, graph views, install/update
       planning, risk analysis, validation, simulation, recovery, rollback, and
       recommendation. recover is an alias into the recovery planning path.
       Destructive writes require explicit user confirmation through install,
       update, or reinstall commands.

EXAMPLES
       storage graph
       storage analyze
       storage plan install
       storage recommend

SEE ALSO
       install(1), update(1), reinstall(1), sairu(1)
";

const MAN_SAIRU: &str = "\
SAIRU(1)                  SAIOS Manual                  SAIRU(1)

NAME
       sairu - deterministic SAI Runtime command surface

SYNOPSIS
       sairu <request>

DESCRIPTION
       SAIRU routes fixed diagnostic, explanation, storage, hardware, task, and
       override requests through contract-owned tools. It is deterministic in
       early boot and does not require a network AI provider.

EXAMPLES
       sairu diagnose storage
       sairu explain process
       sairu override status

SEE ALSO
       storage(1), diag(1), kds(1)
";

const MAN_ADMIN: &str = "\
ADMIN(1)                  SAIOS Manual                  ADMIN(1)

NAME
       id, whoami, users, login, logout, su, passwd, useradd, userdel, chmod, chown - identity and permission commands

SYNOPSIS
       id
       whoami
       users
       login <user>
       logout
       su <user>
       passwd <user>
       useradd <user>
       userdel <user>

DESCRIPTION
       Administration commands route through IdentityContract and SecurityContract
       surfaces for current identity, account state, session transitions, and
       permission metadata.

EXAMPLES
       whoami
       users
       useradd guest

SEE ALSO
       chmod(1), chown(1)
";

const MAN_USERSPACE_TESTS: &str = "\
USERTESTS(1)              SAIOS Manual              USERTESTS(1)

NAME
       testsaios - run all SAIOS validation matrices

SYNOPSIS
       testsaios

DESCRIPTION
       testsaios is the single shell entry point for runtime validation. It
       runs boot, architecture, storage, user-space, process, memory,
       scheduler, observability, libc, signal, pipe, syscall ABI, and
       capability matrices. Each matrix is bounded and reports PASS, FAIL, or
       TIMEOUT through shell output and KDS TEST_* events.

EXAMPLES
       testsaios

SEE ALSO
       exec(1), ps(1), kds(1)
";

const MAN_SYSTEM_EXTRA: &str = "\
SYSTEM-EXTRA(1)           SAIOS Manual           SYSTEM-EXTRA(1)

NAME
       cpus, sysinfo, resmon, irqinfo, kbreset, jobs, journal, verify - system diagnostics and control helpers

SYNOPSIS
       cpus
       sysinfo
       resmon
       journal
       verify observability

DESCRIPTION
       These commands expose CPU, process, interrupt, journal, resource, and
       verification views for kernel contracts and boot diagnostics.

EXAMPLES
       sysinfo
       journal
       verify observability

SEE ALSO
       uname(1), cpuinfo(1), meminfo(1), diag(1)
";

const MAN_SERVICES: &str = "\
SERVICES(1)               SAIOS Manual               SERVICES(1)

NAME
       setup, bash, sh, beep, gfx, reload, lsusb, config - service and compatibility helpers

SYNOPSIS
       setup <package>
       bash
       sh
       reload <target>
       config <key>

DESCRIPTION
       Service commands manage local compatibility helpers, reloadable runtime
       configuration, simple device checks, graphics/audio tests, and USB HID
       enumeration.

EXAMPLES
       setup bash
       bash --version
       reload ai

SEE ALSO
       config(1), exec(1)
";

const MAN_DEVTOOLS: &str = "\
DEVTOOLS(1)               SAIOS Manual               DEVTOOLS(1)

NAME
       curl, wget, openssl, ssh, vi, nano, apt, make, build-essential - compatibility tool stubs

SYNOPSIS
       <tool> [args]

DESCRIPTION
       Development tool commands provide compatibility-oriented shell surfaces
       for networking, editing, package/build workflows, and crypto/remote access
       tooling as those subsystems mature.

EXAMPLES
       curl http://example.com/
       apt help
       make

SEE ALSO
       fetch(1), cc(1), explain(1)
";

const MAN_HELP: &str = "\
HELP(1)                   SAIOS Manual                   HELP(1)

NAME
       help - list all available commands

SYNOPSIS
       help

DESCRIPTION
       Displays a categorised summary of all built-in SAIOS shell
       commands with brief descriptions.

SEE ALSO
       man(1)
";

const MAN_LS: &str = "\
LS(1)                     SAIOS Manual                     LS(1)

NAME
       ls - list directory contents

SYNOPSIS
       ls [path]

DESCRIPTION
       List the files and directories at path (default: /).
       Entries are space-separated on a single line.

EXAMPLES
       ls               List root directory
       ls /etc          List /etc directory
       ls /bin          List binaries

SEE ALSO
       find(1), df(1)
";

const MAN_CAT: &str = "\
CAT(1)                    SAIOS Manual                    CAT(1)

NAME
       cat - print file contents

SYNOPSIS
       cat <file>

DESCRIPTION
       Print the contents of a file to standard output.
       Binary files are displayed as [binary N bytes].

EXAMPLES
       cat /etc/hostname
       cat /etc/os-release

SEE ALSO
       hexdump(1), grep(1)
";

const MAN_WRITE: &str = "\
WRITE(1)                  SAIOS Manual                  WRITE(1)

NAME
       write - write text to a file

SYNOPSIS
       write <path> <content>

DESCRIPTION
       Create or overwrite a file with the given text.
       A newline is appended automatically.
       Use append(1) to add to an existing file.

EXAMPLES
       write /tmp/hello.c 'int main(){return 0;}'
       write /etc/hostname mybox

SEE ALSO
       append(1), cat(1)
";

const MAN_MKDIR: &str = "\
MKDIR(1)                  SAIOS Manual                  MKDIR(1)

NAME
       mkdir - create a directory

SYNOPSIS
       mkdir <path>

DESCRIPTION
       Create the directory at path. Parent directories must exist.

EXAMPLES
       mkdir /home/sanjay
       mkdir /var/log/saios

SEE ALSO
       ls(1), rm(1)
";

const MAN_RM: &str = "\
RM(1)                     SAIOS Manual                     RM(1)

NAME
       rm - remove a file or empty directory

SYNOPSIS
       rm <path>

DESCRIPTION
       Remove the file or empty directory at path.
       Non-empty directories cannot be removed.

EXAMPLES
       rm /tmp/test.txt
       rm /home/old_dir

SEE ALSO
       mkdir(1), mv(1)
";

const MAN_CP: &str = "\
CP(1)                     SAIOS Manual                     CP(1)

NAME
       cp - copy a file

SYNOPSIS
       cp <source> <destination>

DESCRIPTION
       Copy a file from source to destination.
       Directories are not copied recursively.

EXAMPLES
       cp /etc/hostname /tmp/hostname.bak

SEE ALSO
       mv(1), write(1)
";

const MAN_MV: &str = "\
MV(1)                     SAIOS Manual                     MV(1)

NAME
       mv - move or rename a file

SYNOPSIS
       mv <source> <destination>

DESCRIPTION
       Move or rename a file. Equivalent to cp followed by rm.

EXAMPLES
       mv /tmp/draft.txt /home/final.txt

SEE ALSO
       cp(1), rm(1)
";

const MAN_FIND: &str = "\
FIND(1)                   SAIOS Manual                   FIND(1)

NAME
       find - search for files by name

SYNOPSIS
       find <path> <pattern>

DESCRIPTION
       Recursively search path for entries whose name contains pattern.

EXAMPLES
       find / .conf         Find all .conf files
       find /home sanjay    Find files named sanjay

SEE ALSO
       grep(1), ls(1)
";

const MAN_GREP: &str = "\
GREP(1)                   SAIOS Manual                   GREP(1)

NAME
       grep - search text in a file

SYNOPSIS
       grep <pattern> <file>

DESCRIPTION
       Print lines from file that contain pattern.
       Line numbers are shown. Reports total matching lines.

EXAMPLES
       grep root /etc/passwd
       grep error /var/log/saios.log

SEE ALSO
       cat(1), find(1), wc(1)
";

const MAN_HEXDUMP: &str = "\
HEXDUMP(1)                SAIOS Manual                HEXDUMP(1)

NAME
       hexdump - hex and ASCII dump of a file

SYNOPSIS
       hexdump <file>

DESCRIPTION
       Display file contents as a hex dump with ASCII representation.
       Output is formatted as:  OFFSET  HEX×16  |ASCII|

EXAMPLES
       hexdump /boot/saios.elf
       hexdump /etc/hostname

SEE ALSO
       cat(1)
";

const MAN_WC: &str = "\
WC(1)                     SAIOS Manual                     WC(1)

NAME
       wc - word, line, and byte count

SYNOPSIS
       wc <file>

DESCRIPTION
       Count lines, words, and bytes in a file and print the totals.

EXAMPLES
       wc /etc/passwd
       wc /var/log/messages

SEE ALSO
       cat(1), grep(1)
";

const MAN_DF: &str = "\
DF(1)                     SAIOS Manual                     DF(1)

NAME
       df - report filesystem usage

SYNOPSIS
       df

DESCRIPTION
       Display the number of files, directories, and bytes used in
       the in-memory ramfs filesystem. Data is lost on reboot.

SEE ALSO
       meminfo(1)
";

const MAN_ECHO: &str = "\
ECHO(1)                   SAIOS Manual                   ECHO(1)

NAME
       echo - display a line of text

SYNOPSIS
       echo <text>

DESCRIPTION
       Write text to standard output followed by a newline.

EXAMPLES
       echo Hello, SAIOS!
       echo $PATH

SEE ALSO
       write(1)
";

const MAN_CALC: &str = "\
CALC(1)                   SAIOS Manual                   CALC(1)

NAME
       calc - integer expression evaluator

SYNOPSIS
       calc <expr>

DESCRIPTION
       Evaluate a simple integer arithmetic expression.
       Supported operators: + - * / % **

EXAMPLES
       calc 3 + 4           = 7
       calc 1024 * 1024     = 1048576
       calc 2 ** 10         = 1024
       calc 100 / 7         = 14

SEE ALSO
       echo(1)
";

const MAN_RUN: &str = "\
RUN(1)                    SAIOS Manual                    RUN(1)

NAME
       run - execute a script from the filesystem

SYNOPSIS
       run <script-path>

DESCRIPTION
       Read a text file from the filesystem and execute each line as a
       SAIOS shell command. Lines beginning with # are comments.

EXAMPLES
       write /home/setup.sh 'echo Starting...'
       run /home/setup.sh

SEE ALSO
       exec(1), write(1)
";

const MAN_ENV: &str = "\
ENV(1)                    SAIOS Manual                    ENV(1)

NAME
       env - display or set environment variables

SYNOPSIS
       env
       set <key> <value>

DESCRIPTION
       env   - display all variables from /etc/env
       set   - append key=value to /etc/env

EXAMPLES
       set PATH /bin:/usr/bin
       set EDITOR vi
       env

SEE ALSO
       run(1)
";

const MAN_UNAME: &str = "\
UNAME(1)                  SAIOS Manual                  UNAME(1)

NAME
       uname - print system information

SYNOPSIS
       uname

DESCRIPTION
       Display the SAIOS version, architecture, kernel mode, and
       bootloader information.

SEE ALSO
       cpuinfo(1), meminfo(1)
";

const MAN_CPUINFO: &str = "\
CPUINFO(1)                SAIOS Manual                CPUINFO(1)

NAME
       cpuinfo - display CPU information

SYNOPSIS
       cpuinfo

DESCRIPTION
       Use CPUID to query and display: vendor string, brand name,
       family/model/stepping, and supported feature flags (SSE, AVX,
       VMX, etc.).

SEE ALSO
       uname(1), meminfo(1), lspci(1)
";

const MAN_MEMINFO: &str = "\
MEMINFO(1)                SAIOS Manual                MEMINFO(1)

NAME
       meminfo - display memory information

SYNOPSIS
       meminfo

DESCRIPTION
       Show physical frame allocator statistics (total/free/used frames)
       and the Multiboot2 memory map regions reported by GRUB.

SEE ALSO
       cpuinfo(1), df(1)
";

const MAN_LSPCI: &str = "\
LSPCI(1)                  SAIOS Manual                  LSPCI(1)

NAME
       lspci - list PCI devices

SYNOPSIS
       lspci

DESCRIPTION
       Scan PCI buses 0-3 and display all detected devices with their
       bus/device/function address, vendor:device IDs, and class name.

SEE ALSO
       uname(1)
";

const MAN_UPTIME: &str = "\
UPTIME(1)                 SAIOS Manual                 UPTIME(1)

NAME
       uptime - show ticks since boot

SYNOPSIS
       uptime

DESCRIPTION
       Display the number of timer ticks since kernel boot.
       The PIT fires at approximately 18 Hz on real hardware.

SEE ALSO
       uname(1)
";

const MAN_COLOR: &str = "\
COLOR(1)                  SAIOS Manual                  COLOR(1)

NAME
       color - change the terminal foreground color

SYNOPSIS
       color <scheme>

DESCRIPTION
       Set the VGA text-mode foreground color. Available schemes:
       green  cyan  white  yellow  red  blue  pink

EXAMPLES
       color cyan          Switch to cyan text
       color white         Switch to white text

SEE ALSO
       clear(1)
";

const MAN_NET: &str = "\
NET(1)                    SAIOS Manual                    NET(1)

NAME
       net - network management

SYNOPSIS
       net status
       net dns <hostname>
       net ping <ip>

DESCRIPTION
       status - Show IP address, MAC address, and DNS server
       dns    - Resolve a hostname via UDP DNS (port 53)
       ping   - Send an ARP probe to an IP address

EXAMPLES
       net status
       net dns api.anthropic.com
       net ping 10.0.2.2

SEE ALSO
       fetch(1), ai(1)
";

const MAN_FETCH: &str = "\
FETCH(1)                  SAIOS Manual                  FETCH(1)

NAME
       fetch - perform an HTTP GET request

SYNOPSIS
       fetch http://<host>[/path]

DESCRIPTION
       Send an HTTP/1.1 GET request to the given URL and print the
       response headers and first 40 lines of the body.
       HTTPS is not yet supported (use a plain HTTP endpoint).

EXAMPLES
       fetch http://example.com/
       fetch http://10.0.2.2:8080/api/status

SEE ALSO
       net(1), ai(1)
";

const MAN_AI: &str = "\
AI(1)                     SAIOS Manual                     AI(1)

NAME
       ai - artificial intelligence interface

SYNOPSIS
       ai ask <prompt>
       ai chat
       ai save <file> <prompt>
       ai use <provider>
       ai model <name>
       ai host <ip> <port>
       ai key <provider> <api-key>
       ai status

DESCRIPTION
       Interact with AI language models from the SAIOS shell.

PROVIDERS
       ollama     Free, local - requires Ollama running on host (10.0.2.2:11434)
       anthropic  Claude - requires API key
       openai     GPT-4o - requires API key

COMMANDS
       ask     Send a one-shot prompt and print the response
       chat    Interactive chat session
       save    Send a prompt and save the response to a file
       use     Switch active provider
       model   Set the Ollama model (llama3, mistral, phi3, etc.)
       host    Set Ollama host IP and port
       key     Set cloud provider API key (stored in session only)
       status  Display current provider configuration

EXAMPLES
       ai ask \"explain what a page fault is\"
       ai use ollama
       ai model mistral
       ai save /home/notes.txt \"summarise ext4 filesystem design\"

SEE ALSO
       fetch(1), net(1), cc(1)
";

const MAN_CC: &str = "\
CC(1)                     SAIOS Manual                     CC(1)

NAME
       cc - AI-powered C code analyser

SYNOPSIS
       cc <file.c>

DESCRIPTION
       Read a C source file from the SAIOS filesystem, send it to the
       active AI provider, and display analysis including: error
       detection, behaviour explanation, and expected output.

       Write C code first with write(1), then analyse with cc.

EXAMPLES
       write /tmp/hello.c 'int main(){return 42;}'
       cc /tmp/hello.c

SEE ALSO
       ai(1), explain(1), write(1)
";

const MAN_EXPLAIN: &str = "\
EXPLAIN(1)                SAIOS Manual                EXPLAIN(1)

NAME
       explain - ask AI to explain a file

SYNOPSIS
       explain <file>

DESCRIPTION
       Read any file from the filesystem and send its contents to the
       active AI provider with a request for a concise explanation.

EXAMPLES
       explain /etc/saios.conf
       explain /boot/grub/grub.cfg

SEE ALSO
       ai(1), cc(1), cat(1)
";

const MAN_EXEC: &str = "\
EXEC(1)                   SAIOS Manual                   EXEC(1)

NAME
       exec - execute an ELF binary

SYNOPSIS
       exec <path>

DESCRIPTION
       Load and execute an ELF64 static binary from the SAIOS VFS.
       The binary runs in ring-3 (user mode) with access to the
       PARTIAL Linux-flavored syscall layer. The shell-oriented return path
       remains a known lifecycle limitation.

       Binaries must be placed in the VFS first (e.g. via the installer
       or a pre-loaded disk image).

EXAMPLES
       exec /bin/busybox
       exec /usr/bin/python3

SEE ALSO
       ps(1), install(1)
";

const MAN_PS: &str = "\
PS(1)                     SAIOS Manual                     PS(1)

NAME
       ps - list running processes

SYNOPSIS
       ps

DESCRIPTION
       Display all processes currently known to ProcessContract: PID, state,
       current CPU if running, and process name.

SEE ALSO
       exec(1)
";

const MAN_KILL: &str = "\
KILL(1)                   SAIOS Manual                   KILL(1)

NAME
       kill - send a signal to a process

SYNOPSIS
       kill [-<signal>] <pid>

DESCRIPTION
       Send a signal to an existing process by PID. If no signal is provided,
       SIGTERM is used. Named signals accept the common HUP, INT, QUIT, KILL,
       TERM, STOP, CONT, USR1, and USR2 forms.

EXAMPLES
       kill 42
       kill -TERM 42
       kill -KILL 42

SEE ALSO
       ps(1), testsaios(1)
";

const MAN_INSTALL: &str = "\
INSTALL(1)                SAIOS Manual                INSTALL(1)

NAME
       install - install SAIOS to a hard disk

SYNOPSIS
       install [device]

DESCRIPTION
       Install the running SAIOS kernel to a block device (default:
       /dev/vda). Writes:

         MBR         GRUB boot.img (sector 0)
         Sectors 1-2047  GRUB core.img (with ext2 + multiboot2 modules)
         Sector 2048+    ext4 partition containing:
                           /boot/grub/grub.cfg
                           /boot/saios.elf (current kernel)

       After installation: power off, remove installer media, and boot from disk.

       CAUTION: This ERASES the target device.

EXAMPLES
       install              Install to /dev/vda (default)
       install /dev/vdb     Install to second disk

SEE ALSO
       reinstall(1), update(1), lspci(1)
";

const MAN_UPDATE: &str = "\
UPDATE(1)                 SAIOS Manual                 UPDATE(1)

NAME
       update - update an existing SAIOS installation

SYNOPSIS
       update [device]

DESCRIPTION
       Route update intent for an installed SAIOS disk through the
       StoragePlatformContract update gates. The command requires an existing
       installed SAIOS target, snapshot and rollback evidence, and SPC approval
       before writes are attempted.

       The default device is /dev/vda.

EXAMPLES
       update              Update the default installed disk
       update /dev/vdb     Update a second attached disk

SEE ALSO
       install(1), reinstall(1), storage(1), sairu(1)
";

const MAN_REINSTALL: &str = "\
REINSTALL(1)              SAIOS Manual              REINSTALL(1)

NAME
       reinstall - explicitly replace a target disk with SAIOS

SYNOPSIS
       reinstall [device]

DESCRIPTION
       Replace the selected target disk with SAIOS. Existing SAIOS evidence is
       not required. The advisor reports risk and impact, then the user confirms
       with REINSTALL SAIOS before writing. The default device is /dev/vda.

EXAMPLES
       reinstall           Replace the default target disk
       reinstall /dev/vdb  Replace /dev/vdb with SAIOS

SEE ALSO
       install(1), update(1), storage(1), sairu(1)
";

const MAN_HISTORY: &str = "\
HISTORY(1)                SAIOS Manual                HISTORY(1)

NAME
       history - display command history

SYNOPSIS
       history

DESCRIPTION
       Display the last 20 commands entered in the shell.
       Use the Up/Down arrow keys to navigate history interactively.

SEE ALSO
       help(1)
";

const MAN_REBOOT: &str = "\
REBOOT(1)                 SAIOS Manual                 REBOOT(1)

NAME
       reboot - restart the system

SYNOPSIS
       reboot

DESCRIPTION
       Immediately restart the system by sending the PS/2 controller
       reset command. All unsaved data in the ramfs will be lost.

SEE ALSO
       halt(1), install(1)
";

const MAN_HALT: &str = "\
HALT(1)                   SAIOS Manual                   HALT(1)

NAME
       halt - stop the system

SYNOPSIS
       halt

DESCRIPTION
       Halt the CPU. The system enters an infinite HLT loop.
       All unsaved data in the ramfs will be lost.

SEE ALSO
       reboot(1)
";
