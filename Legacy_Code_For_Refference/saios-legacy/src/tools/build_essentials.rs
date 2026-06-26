//! build-essential — cross-compilation toolchain setup for SAIOS.
//!
//! Since SAIOS doesn't yet run GCC natively (that requires a stronger userspace and libc),
//! this module provides:
//!
//!   1. `build-essential` command — shows installation status and build instructions
//!   2. `make` built-in — a minimal make that parses simple Makefiles
//!   3. Cross-compilation guide for building binaries that run on SAIOS
//!
//! # Cross-compilation setup (on the host machine)
//!
//! To build programs that run on SAIOS:
//!
//! ```sh
//! # Install a host C compiler and binutils
//! sudo apt install gcc binutils
//!
//! # Compile a freestanding SAIOS-linked binary once the libc/linker path is ready
//! gcc -nostdlib -static -o hello hello.c
//!
//! # Copy into SAIOS disk image
//! # Then in SAIOS: exec /path/to/hello
//! ```
//!
//! # Native GCC
//! Full native GCC support requires the minimal SAIOS libc, normal GCC-built
//! binaries, BusyBox, binutils, and then a GCC bootstrap.
//! Track progress: see `ROADMAP.md`.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// -- Package list for build-essential -------------------------------------

/// Host package names used as a familiar build-essential checklist.
const BUILD_ESSENTIAL_PACKAGES: &[(&str, &str)] = &[
    ("gcc", "GNU C Compiler"),
    ("g++", "GNU C++ Compiler"),
    ("make", "GNU Make build automation tool"),
    ("libc6-dev", "GNU C Library development headers"),
    ("dpkg-dev", "Debian package development tools"),
    ("binutils", "GNU assembler, linker, and binary utilities"),
    ("cpp", "GNU C Preprocessor"),
    ("libgcc-s1", "GCC support library"),
    (
        "libstdc++-12-dev",
        "GNU Standard C++ Library development files",
    ),
];

/// Run the `build-essential` command.
pub fn run_build_essential() {
    crate::println!("SAIOS Build Essential Toolchain");
    crate::println!("{}", "═".repeat(50));
    crate::println!();
    crate::println!("Status: Cross-compilation mode (native GCC in Phase 6)");
    crate::println!();

    // Check if we have apt and any packages installed
    crate::println!("Packages in build-essential:");
    for (pkg, desc) in BUILD_ESSENTIAL_PACKAGES {
        let installed = crate::tools::apt::INSTALLED.lock().contains(*pkg);
        let status = if installed {
            "✓ installed"
        } else {
            "  not installed"
        };
        crate::println!("  {:30} {}  {}", pkg, status, desc);
    }

    crate::println!();
    crate::println!("To install: apt install build-essential");
    crate::println!();
    crate::println!("Cross-compilation (recommended for now):");
    crate::println!("  On host: musl-gcc -static -o program program.c");
    crate::println!("  Copy binary to SAIOS disk, then: exec /path/to/program");
    crate::println!();
    print_cross_compile_guide();
}

fn print_cross_compile_guide() {
    crate::println!("HOST CROSS-COMPILE GUIDE");
    crate::println!("-----------------------------------------------------");
    crate::println!("# Setup (Ubuntu/Debian host):");
    crate::println!("  sudo apt install musl-tools");
    crate::println!();
    crate::println!("# C program:");
    crate::println!("  musl-gcc -static -O2 -o hello hello.c");
    crate::println!();
    crate::println!("# C++ program:");
    crate::println!("  x86_64-linux-musl-g++ -static -O2 -o hello hello.cpp");
    crate::println!();
    crate::println!("# Rust program (static musl):");
    crate::println!("  rustup target add x86_64-unknown-linux-musl");
    crate::println!("  cargo build --target x86_64-unknown-linux-musl --release");
    crate::println!();
    crate::println!("# Copy to SAIOS (via disk image):");
    crate::println!("  # Mount ext4 partition and copy, OR");
    crate::println!("  # Use wget inside SAIOS to fetch from HTTP server on host");
}

// -- Minimal `make` implementation -----------------------------------------

/// A rule parsed from a Makefile.
#[derive(Debug, Clone)]
struct MakeRule {
    /// Target name (e.g. "all", "clean", "%.o").
    target: String,
    /// Prerequisite targets/files.
    deps: Vec<String>,
    /// Commands to run (each starting with a tab).
    commands: Vec<String>,
}

/// Run a minimal `make` in the current working directory.
///
/// # Arguments
/// * `args` — arguments after `make` (e.g. empty for default target,
///   or a target name like `clean`).
pub fn run_make(args: &str) {
    let cwd = crate::process::with_current_process(|p| p.cwd.clone())
        .unwrap_or_else(|| String::from("/"));

    let makefile_path = format!("{}/Makefile", cwd.trim_end_matches('/'));
    let alt_path = format!("{}/makefile", cwd.trim_end_matches('/'));

    // Try Makefile then makefile
    let content = if let Ok(buf) = crate::vfs_contract::VfsContract::read_file(&makefile_path) {
        String::from_utf8_lossy(&buf).into_owned()
    } else if let Ok(buf) = crate::vfs_contract::VfsContract::read_file(&alt_path) {
        String::from_utf8_lossy(&buf).into_owned()
    } else {
        crate::println!("make: *** No targets specified and no Makefile found.  Stop.");
        return;
    };

    let rules = parse_makefile(&content);
    if rules.is_empty() {
        crate::println!("make: *** No rules defined in Makefile.");
        return;
    }

    // Default target: first rule, or the one named in args
    let target = if args.trim().is_empty() {
        rules[0].target.clone()
    } else {
        args.trim().to_string()
    };

    execute_target(&target, &rules, &mut alloc::collections::BTreeSet::new());
}

/// Parse a Makefile into a list of rules.
///
/// Supports:
///   - `target: dep1 dep2 ...`  rule headers
///   - `\t command`             recipe lines
///   - `# comment`             comments (ignored)
///   - `VAR = value`            variable assignments (stored but not expanded yet)
fn parse_makefile(content: &str) -> Vec<MakeRule> {
    let mut rules: Vec<MakeRule> = Vec::new();
    let mut vars: alloc::collections::BTreeMap<String, String> =
        alloc::collections::BTreeMap::new();
    let mut cur: Option<usize> = None; // index of current rule being built

    for line in content.lines() {
        // Skip comments and empty lines
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }

        // Recipe line (starts with a tab)
        if let Some(command) = line.strip_prefix('\t') {
            if let Some(idx) = cur {
                rules[idx].commands.push(command.to_string());
            }
            continue;
        }

        // Variable assignment
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_string();
            let val = line[eq + 1..].trim().to_string();
            vars.insert(key, val);
            cur = None;
            continue;
        }

        // Rule header: target: deps...
        if let Some(colon) = line.find(':') {
            let target_str = line[..colon].trim().to_string();
            let deps_str = line[colon + 1..].trim();
            let deps: Vec<String> = deps_str
                .split_whitespace()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            rules.push(MakeRule {
                target: target_str,
                deps,
                commands: Vec::new(),
            });
            cur = Some(rules.len() - 1);
        }
    }
    rules
}

/// Execute a Makefile target, recursing into dependencies first.
///
/// Uses a `visited` set to detect cycles and avoid re-running targets.
fn execute_target(
    target: &str,
    rules: &[MakeRule],
    visited: &mut alloc::collections::BTreeSet<String>,
) {
    if visited.contains(target) {
        return;
    }
    visited.insert(target.to_string());

    // Find the rule
    let rule = match rules.iter().find(|r| r.target == target) {
        Some(r) => r,
        None => {
            // Target may be a file — check VFS
            if crate::vfs_contract::VfsContract::resolve(target).is_ok() {
                return;
            }
            crate::println!("make: *** No rule to make target '{}'.  Stop.", target);
            return;
        }
    };

    // Build dependencies first
    let deps = rule.deps.clone();
    for dep in &deps {
        execute_target(dep, rules, visited);
    }

    // Run commands
    for cmd in &rule.commands.clone() {
        let display = cmd.trim();
        if !display.starts_with('@') {
            crate::println!("{}", display);
        }
        // Execute via shell command dispatcher
        crate::shell::commands::dispatch_line(display.trim_start_matches('@'));
    }
}
