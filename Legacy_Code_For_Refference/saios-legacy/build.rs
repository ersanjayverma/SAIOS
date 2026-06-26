use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::vec::Vec;

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let target = std::env::var("TARGET").unwrap_or_default();

    // Object paths are built with the host's native path separator (Path::join)
    // and emitted to the linker as-is.  The previous code hard-coded `\\`, which
    // produced paths like `/out\boot.o` on Linux and broke `rust-lld`.
    let obj = |name: &str| {
        Path::new(&out_dir)
            .join(name)
            .to_string_lossy()
            .into_owned()
    };

    if target == "x86_64-unknown-none" {
        for (src, name) in [
            ("src/boot.s", "boot.o"),
            ("src/arch/x86_64/syscall/entry.s", "syscall_entry.o"),
            ("src/arch/x86_64/process/context_switch.s", "switch.o"),
            ("src/arch/x86_64/smp/trampoline.s", "smp_trampoline.o"),
        ] {
            let o = obj(name);
            assemble(src, &o);
            println!("cargo:rustc-link-arg={}", o);
        }

        println!("cargo:rustc-link-arg=-Tlinker.ld");
    }
    println!("cargo:rerun-if-changed=src/boot.s");
    println!("cargo:rerun-if-changed=src/arch/x86_64/syscall/entry.s");
    println!("cargo:rerun-if-changed=src/arch/x86_64/process/context_switch.s");
    println!("cargo:rerun-if-changed=src/arch/x86_64/smp/trampoline.s");
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/boot_mode.rs");
    println!("cargo:rerun-if-changed=iso/boot/grub/grub.cfg");
    println!("cargo:rerun-if-changed=secure_boot/build_signed_iso.sh");

    validate_media_grub_boot_modes();

    embed_grub(&out_dir);
    embed_usertest(&out_dir);
    embed_ring3loop(&out_dir);
    embed_ring3halt(&out_dir);
    embed_testpie(&out_dir);
    embed_validation(&out_dir);
    embed_fork_abi_test(&out_dir);
    embed_execve_driver(&out_dir);
    embed_execve_child(&out_dir);
    embed_fault_test(&out_dir);
    embed_gp_test(&out_dir);
    embed_ud_test(&out_dir);
    embed_div0_test(&out_dir);
    embed_pf_test(&out_dir);
    embed_memperm_test(&out_dir);
    embed_thread_test(&out_dir);
    embed_futex_test(&out_dir);
    embed_signal_test(&out_dir);
    embed_wait_reap_test(&out_dir);
    embed_pipe_semantics_test(&out_dir);
    embed_syscall_abi_test(&out_dir);
    embed_capability_test(&out_dir);
    embed_libc_plan_test(&out_dir);
    embed_saios_shell(&out_dir);
}

fn validate_media_grub_boot_modes() {
    let boot_mode_src = std::fs::read_to_string("src/boot_mode.rs")
        .expect("build.rs: failed to read src/boot_mode.rs");
    let install = rust_string_const(&boot_mode_src, "BOOT_MODE_INSTALL");
    let update = rust_string_const(&boot_mode_src, "BOOT_MODE_UPDATE");
    for path in ["iso/boot/grub/grub.cfg", "secure_boot/build_signed_iso.sh"] {
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|_| panic!("build.rs: failed to read {}", path));
        require_grub_mode(path, &source, "install", install);
        require_grub_mode(path, &source, "update", update);
    }
}

fn rust_string_const<'a>(source: &'a str, name: &str) -> &'a str {
    let declaration = format!("pub const {}: &str = ", name);
    let start = source
        .find(&declaration)
        .unwrap_or_else(|| panic!("build.rs: missing boot mode constant {}", name))
        + declaration.len();
    let rest = &source[start..];
    let quoted = rest
        .strip_prefix('"')
        .unwrap_or_else(|| panic!("build.rs: malformed boot mode constant {}", name));
    let end = quoted
        .find('"')
        .unwrap_or_else(|| panic!("build.rs: malformed boot mode constant {}", name));
    &quoted[..end]
}

fn require_grub_mode(path: &str, source: &str, label: &str, mode: &str) {
    let needle = format!("saios.mode={}", mode);
    if !source.contains(&needle) {
        panic!(
            "build.rs: {} must emit {} mode through canonical boot mode value {:?}",
            path, label, mode
        );
    }
}

/// Phase 5.0a: build the static userspace test program and embed its ELF bytes
/// (OUT_DIR/usertest_elf.rs).  Assembled + statically linked (no libc) with the
/// host `as`/`ld` (native on Linux, via `wsl` on Windows).  If the tools are
/// missing, the embedded array is empty and the `usertest` command says so.
///
/// Uses the SAIOS user-space linker script (userspace/user.ld) which places all
/// code in the PML4[1] window (512 GiB .. 1 TiB) at canonical addresses that
/// won't trigger #GP(0) under 4-level paging.  See user.ld for the layout.
fn embed_usertest(out_dir: &str) {
    println!("cargo:rerun-if-changed=userspace/hello.s");
    println!("cargo:rerun-if-changed=userspace/user.ld");
    let obj = Path::new(out_dir)
        .join("usertest.o")
        .to_string_lossy()
        .into_owned();
    let elf = Path::new(out_dir)
        .join("usertest.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("usertest_elf.rs");

    assemble("userspace/hello.s", &obj);

    // Link with the SAIOS user-space linker script.  This produces an ET_EXEC
    // binary at canonical user addresses in PML4[1], avoiding the kernel identity
    // map in PML4[0] and the non-canonical hole above 0x0000_7FFF_FFFF_FFFF.
    let linker_script = "userspace/user.ld";
    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "ld",
                "-static",
                "-nostdlib",
                "-T",
                linker_script,
                "-e",
                "_start",
                "-o",
                &to_wsl_path(&elf),
                &to_wsl_path(&obj),
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("ld")
            .args([
                "-static",
                "-nostdlib",
                "-T",
                linker_script,
                "-e",
                "_start",
                "-o",
                &elf,
                &obj,
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!("build.rs: usertest ELF not built (need as/ld) — `usertest` will be a no-op");
    } else {
        eprintln!("build.rs: usertest ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "USERTEST_ELF", &bytes);
}

/// Phase 5.0b debugger: build and embed the smallest possible user program.
///
/// This program is deliberately weaker than `usertest`: it never calls into the
/// kernel after entry. A successful run hangs forever in a two-byte `jmp .`
/// loop, which proves that `iretq` entered CPL3 and fetched at least one user
/// instruction without involving the syscall path, libc, relocations, globals,
/// stack traffic, or device I/O.
fn embed_ring3loop(out_dir: &str) {
    // Re-run the build script whenever the probe source changes.
    println!("cargo:rerun-if-changed=userspace/ring3_loop.s");
    println!("cargo:rerun-if-changed=userspace/user.ld");

    // Keep build products under OUT_DIR so the repository stays clean.
    let obj = Path::new(out_dir)
        .join("ring3_loop.o")
        .to_string_lossy()
        .into_owned();
    let elf = Path::new(out_dir)
        .join("ring3_loop.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("ring3_loop_elf.rs");

    // Assemble without libc or compiler-generated runtime code.
    assemble("userspace/ring3_loop.s", &obj);

    // Link with the SAIOS user-space linker script for canonical addresses.
    let linker_script = "userspace/user.ld";
    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "ld",
                "-static",
                "-nostdlib",
                "-T",
                linker_script,
                "-e",
                "_start",
                "-o",
                &to_wsl_path(&elf),
                &to_wsl_path(&obj),
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("ld")
            .args([
                "-static",
                "-nostdlib",
                "-T",
                linker_script,
                "-e",
                "_start",
                "-o",
                &elf,
                &obj,
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    // If host binutils are unavailable, embed an empty array and let the shell
    // command fail cleanly instead of breaking the kernel build.
    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!("build.rs: ring3 loop ELF not built (need as/ld) - `ring3loop` will be a no-op");
    } else {
        eprintln!("build.rs: ring3 loop ELF = {} bytes", bytes.len());
    }

    // Emit Rust source consumed by shell/commands.rs through include!().
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "RING3_LOOP_ELF", &bytes);
}

/// Ring 3 execution proof — HLT probe.
///
/// This 1-instruction binary contains only `hlt`, a privileged instruction
/// that generates #GP in Ring 3.  If the #GP handler reports CPL=3, that
/// PROVES the CPU entered user mode and executed at least one user instruction.
/// No syscalls, no I/O, no stack access, no globals, no relocations needed.
fn embed_ring3halt(out_dir: &str) {
    println!("cargo:rerun-if-changed=userspace/ring3_halt.s");
    println!("cargo:rerun-if-changed=userspace/user.ld");

    let obj = Path::new(out_dir)
        .join("ring3_halt.o")
        .to_string_lossy()
        .into_owned();
    let elf = Path::new(out_dir)
        .join("ring3_halt.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("ring3_halt_elf.rs");

    assemble("userspace/ring3_halt.s", &obj);

    // Link with the SAIOS user-space linker script for canonical addresses.
    let linker_script = "userspace/user.ld";
    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "ld",
                "-static",
                "-nostdlib",
                "-T",
                linker_script,
                "-e",
                "_start",
                "-o",
                &to_wsl_path(&elf),
                &to_wsl_path(&obj),
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("ld")
            .args([
                "-static",
                "-nostdlib",
                "-T",
                linker_script,
                "-e",
                "_start",
                "-o",
                &elf,
                &obj,
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!("build.rs: ring3 halt ELF not built (need as/ld) - `ring3halt` will be a no-op");
    } else {
        eprintln!("build.rs: ring3 halt ELF = {} bytes", bytes.len());
    }

    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "RING3_HALT_ELF", &bytes);
}
/// Build and embed the PIE (Position-Independent Executable) test binary.
///
/// This produces an ET_DYN (PIE) ELF from test_pie.c.  PIE binaries start at
/// virtual address 0 and are shifted by the kernel loader's load_bias
/// (USER_TEXT_BASE = 0x0000_0080_0000_0000, PML4[1]) at load time, so the
/// linker script placement doesn't matter — only relocations matter.
///
/// The resulting binary is type DYN (not EXEC), which tells the SAIOS loader
/// to apply load_bias and R_X86_64_RELATIVE relocations before jumping to
/// the entry point.
///
/// Build flags rationale:
///   -fPIE         Generate position-independent code (RIP-relative addressing).
///   -pie          Produce a PIE executable (ET_DYN).  NOT -shared (shared library
///                 semantics differ) and NOT -static -pie (produces ET_EXEC on
///                 some GCC versions, which defeats PIE load_bias).
///   -nostdlib     SAIOS has no libc — no startup files, no libgcc.
///   -Wl,--no-dynamic-linker  Remove PT_INTERP (no ld-linux.so).
fn embed_testpie(out_dir: &str) {
    println!("cargo:rerun-if-changed=test_pie");
    let out_rs = Path::new(out_dir).join("test_pie_elf.rs");

    let bytes = std::fs::read("test_pie").unwrap_or_default();
    if bytes.is_empty() {
        eprintln!("build.rs: checked-in test_pie ELF missing — PIE test will fail");
    } else {
        eprintln!("build.rs: checked-in test_pie ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "TEST_PIE_ELF", &bytes);
}

/// Embed GRUB i386-pc boot.img + core.img for the optional BIOS-GRUB installer.
///
/// `src/install/grub_embed.rs` is a COMMITTED, curated blob that also contains
/// the self-contained UEFI image (`GRUB_EFI_IMG`).  We therefore only (re)write
/// it when it is empty/absent, and never clobber populated content — so the
/// curated EFI image survives.  Build tools run natively on Linux and via `wsl`
/// on Windows; we never run privileged package installs from a build script.
fn embed_grub(_out_dir: &str) {
    // Portable path (no hard-coded `\\`): src/install/grub_embed.rs.
    let out_rs = Path::new("src").join("install").join("grub_embed.rs");

    // Guard: never overwrite an already-populated file (keeps the curated EFI +
    // BIOS images that are committed to the repo).
    if let Ok(existing) = std::fs::read_to_string(&out_rs) {
        let boot_empty = existing.contains("GRUB_BOOT_IMG: &[u8] = &[];");
        let core_empty = existing.contains("GRUB_CORE_IMG: &[u8] = &[];");
        if !boot_empty
            && !core_empty
            && existing.contains("GRUB_BOOT_IMG")
            && existing.contains("GRUB_CORE_IMG")
        {
            return; // populated — keep it
        }
    }

    let write_empty = |reason: &str| {
        eprintln!("build.rs: {reason} — embedding empty GRUB images (BIOS install will warn)");
        let mut f = std::fs::File::create(&out_rs).unwrap();
        write_byte_array(&mut f, "GRUB_BOOT_IMG", &[]);
        write_byte_array(&mut f, "GRUB_CORE_IMG", &[]);
    };

    // Need the host GRUB i386-pc support files.  We do NOT auto-install them
    // (a build script must not run privileged package installs); just warn.
    if !host_test("test -f /usr/lib/grub/i386-pc/boot.img") {
        write_empty("grub-pc-bin not found (install: apt install grub-pc-bin xorriso mtools)");
        return;
    }

    // Build a fresh core.img.
    let core_path = "/tmp/saios_core.img";
    let mk = run_tool(&[
        "grub-mkimage",
        "--directory",
        "/usr/lib/grub/i386-pc",
        "--prefix",
        "(hd0,msdos1)/boot/grub",
        "--output",
        core_path,
        "--format",
        "i386-pc",
        "biosdisk",
        "part_msdos",
        "ext2",
        "multiboot2",
        "normal",
        "echo",
        "sleep",
        "ls",
        "cat",
        "all_video",
        "video",
        "vbe",
        "vga",
        "gfxterm",
    ]);
    let core_ok = matches!(&mk, Some(o) if o.status.success());
    if !core_ok {
        if let Some(o) = &mk {
            eprintln!(
                "build.rs: grub-mkimage failed:\n{}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        write_empty("grub-mkimage unavailable");
        return;
    }

    let boot_bytes = read_host_file("/usr/lib/grub/i386-pc/boot.img");
    let core_bytes = read_host_file(core_path);
    if boot_bytes.is_empty() || core_bytes.is_empty() {
        write_empty("could not read GRUB images");
        return;
    }
    eprintln!(
        "build.rs: GRUB boot.img={} B  core.img={} KiB — embedded OK",
        boot_bytes.len(),
        core_bytes.len() / 1024
    );
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "GRUB_BOOT_IMG", &boot_bytes);
    write_byte_array(&mut f, "GRUB_CORE_IMG", &core_bytes);
}

/// Build and embed the comprehensive user-space validation suite.
///
/// This produces an ET_DYN (PIE) ELF from usertest.c — a 16-phase test that
/// progressively validates ring 3 execution, stack, memory, heap, page boundaries,
/// ELF relocations, syscalls, scheduler, IPC, filesystem, and more.
///
/// Build flags: same as test_pie (PIE, no libc, no dynamic linker).
fn embed_validation(out_dir: &str) {
    println!("cargo:rerun-if-changed=usertest.c");

    let elf = Path::new(out_dir)
        .join("validation.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("validation_elf.rs");

    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "gcc",
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &to_wsl_path(&elf),
                "usertest.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("gcc")
            .args([
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &elf,
                "usertest.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!(
            "build.rs: validation suite ELF not built (need gcc) — `validate` will be a no-op"
        );
    } else {
        eprintln!("build.rs: validation suite ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "VALIDATION_ELF", &bytes);
}

fn embed_execve_driver(out_dir: &str) {
    println!("cargo:rerun-if-changed=userspace/execve_driver.c");

    let elf = Path::new(out_dir)
        .join("execve_driver.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("execve_driver_elf.rs");

    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "gcc",
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &to_wsl_path(&elf),
                "userspace/execve_driver.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("gcc")
            .args([
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &elf,
                "userspace/execve_driver.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!(
            "build.rs: execve driver ELF not built (need gcc) — `execvetest` will be a no-op"
        );
    } else {
        eprintln!("build.rs: execve driver ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "EXECVE_DRIVER_ELF", &bytes);
}

fn embed_fork_abi_test(out_dir: &str) {
    println!("cargo:rerun-if-changed=userspace/fork_abi_test.c");

    let elf = Path::new(out_dir)
        .join("fork_abi_test.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("fork_abi_test_elf.rs");

    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "gcc",
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &to_wsl_path(&elf),
                "userspace/fork_abi_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("gcc")
            .args([
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &elf,
                "userspace/fork_abi_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!(
            "build.rs: fork ABI test ELF not built (need gcc) — `forkabitest` will be a no-op"
        );
    } else {
        eprintln!("build.rs: fork ABI test ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "FORK_ABI_TEST_ELF", &bytes);
}

fn embed_execve_child(out_dir: &str) {
    println!("cargo:rerun-if-changed=userspace/execve_child.c");

    let elf = Path::new(out_dir)
        .join("execve_child.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("execve_child_elf.rs");

    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "gcc",
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &to_wsl_path(&elf),
                "userspace/execve_child.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("gcc")
            .args([
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &elf,
                "userspace/execve_child.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!("build.rs: execve child ELF not built (need gcc) — `execvetest` will be a no-op");
    } else {
        eprintln!("build.rs: execve child ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "EXECVE_CHILD_ELF", &bytes);
}

fn embed_fault_test(out_dir: &str) {
    println!("cargo:rerun-if-changed=userspace/fault_test.c");

    let elf = Path::new(out_dir)
        .join("fault_test.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("fault_test_elf.rs");

    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "gcc",
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &to_wsl_path(&elf),
                "userspace/fault_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("gcc")
            .args([
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &elf,
                "userspace/fault_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!("build.rs: fault test ELF not built (need gcc) — `faulttest` will be a no-op");
    } else {
        eprintln!("build.rs: fault test ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "FAULT_TEST_ELF", &bytes);
}

/// Build and embed the GP (General Protection) test binary.
fn embed_gp_test(out_dir: &str) {
    println!("cargo:rerun-if-changed=userspace/gp_test.c");

    let elf = Path::new(out_dir)
        .join("gp_test.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("gp_test_elf.rs");

    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "gcc",
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &to_wsl_path(&elf),
                "userspace/gp_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("gcc")
            .args([
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &elf,
                "userspace/gp_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!("build.rs: gp_test ELF not built (need gcc) — `gp_test` will be a no-op");
    } else {
        eprintln!("build.rs: gp_test ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "GP_TEST_ELF", &bytes);
}

/// Build and embed the UD (Invalid Opcode) test binary.
fn embed_ud_test(out_dir: &str) {
    println!("cargo:rerun-if-changed=userspace/ud_test.c");

    let elf = Path::new(out_dir)
        .join("ud_test.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("ud_test_elf.rs");

    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "gcc",
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &to_wsl_path(&elf),
                "userspace/ud_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("gcc")
            .args([
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &elf,
                "userspace/ud_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!("build.rs: ud_test ELF not built (need gcc) — `ud_test` will be a no-op");
    } else {
        eprintln!("build.rs: ud_test ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "UD_TEST_ELF", &bytes);
}

/// Build and embed the DIV0 (Divide by Zero) test binary.
fn embed_div0_test(out_dir: &str) {
    println!("cargo:rerun-if-changed=userspace/div0_test.c");

    let elf = Path::new(out_dir)
        .join("div0_test.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("div0_test_elf.rs");

    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "gcc",
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &to_wsl_path(&elf),
                "userspace/div0_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("gcc")
            .args([
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &elf,
                "userspace/div0_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!("build.rs: div0_test ELF not built (need gcc) — `div0_test` will be a no-op");
    } else {
        eprintln!("build.rs: div0_test ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "DIV0_TEST_ELF", &bytes);
}

/// Build and embed the PF (Page Fault) test binary.
fn embed_pf_test(out_dir: &str) {
    println!("cargo:rerun-if-changed=userspace/pf_test.c");

    let elf = Path::new(out_dir)
        .join("pf_test.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("pf_test_elf.rs");

    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "gcc",
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &to_wsl_path(&elf),
                "userspace/pf_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("gcc")
            .args([
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &elf,
                "userspace/pf_test.c",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!("build.rs: pf_test ELF not built (need gcc) — `pf_test` will be a no-op");
    } else {
        eprintln!("build.rs: pf_test ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "PF_TEST_ELF", &bytes);
}

fn embed_memperm_test(out_dir: &str) {
    build_pie_probe(
        out_dir,
        "userspace/memperm_test.c",
        "memperm_test.elf",
        "memperm_test_elf.rs",
        "MEMPERM_TEST_ELF",
        "mempermtest",
    );
}

fn embed_thread_test(out_dir: &str) {
    build_pie_probe(
        out_dir,
        "userspace/thread_test.c",
        "thread_test.elf",
        "thread_test_elf.rs",
        "THREAD_TEST_ELF",
        "threadtest",
    );
}

fn embed_futex_test(out_dir: &str) {
    build_pie_probe(
        out_dir,
        "userspace/futex_test.c",
        "futex_test.elf",
        "futex_test_elf.rs",
        "FUTEX_TEST_ELF",
        "futextest",
    );
}

fn embed_signal_test(out_dir: &str) {
    build_pie_probe(
        out_dir,
        "userspace/signal_test.c",
        "signal_test.elf",
        "signal_test_elf.rs",
        "SIGNAL_TEST_ELF",
        "signaltest",
    );
}

fn embed_wait_reap_test(out_dir: &str) {
    build_pie_probe(
        out_dir,
        "userspace/wait_reap_test.c",
        "wait_reap_test.elf",
        "wait_reap_test_elf.rs",
        "WAIT_REAP_TEST_ELF",
        "waitreaptest",
    );
}

fn embed_pipe_semantics_test(out_dir: &str) {
    build_pie_probe(
        out_dir,
        "userspace/pipe_semantics_test.c",
        "pipe_semantics_test.elf",
        "pipe_semantics_test_elf.rs",
        "PIPE_SEMANTICS_TEST_ELF",
        "pipesemtest",
    );
}

fn embed_syscall_abi_test(out_dir: &str) {
    build_pie_probe(
        out_dir,
        "userspace/syscall_abi_test.c",
        "syscall_abi_test.elf",
        "syscall_abi_test_elf.rs",
        "SYSCALL_ABI_TEST_ELF",
        "syscallabitest",
    );
}

fn embed_capability_test(out_dir: &str) {
    build_pie_probe(
        out_dir,
        "userspace/capability_test.c",
        "capability_test.elf",
        "capability_test_elf.rs",
        "CAPABILITY_TEST_ELF",
        "capabilitytest",
    );
}

fn embed_saios_shell(out_dir: &str) {
    build_pie_probe(
        out_dir,
        "userspace/shell.c",
        "saios_shell.elf",
        "saios_shell_elf.rs",
        "SAIOS_SHELL_ELF",
        "shell",
    );
}

fn build_pie_probe(
    out_dir: &str,
    source: &str,
    elf_name: &str,
    rs_name: &str,
    static_name: &str,
    command_name: &str,
) {
    println!("cargo:rerun-if-changed={}", source);

    let elf = Path::new(out_dir)
        .join(elf_name)
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join(rs_name);

    let linked = if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args([
                "gcc",
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &to_wsl_path(&elf),
                source,
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("gcc")
            .args([
                "-fPIE",
                "-pie",
                "-nostdlib",
                "-Wl,--entry=_start",
                "-Wl,--gc-sections",
                "-Wl,-z,now",
                "-Wl,--no-dynamic-linker",
                "-o",
                &elf,
                source,
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!(
            "build.rs: {} ELF not built (need gcc) - `{}` will be a no-op",
            elf_name, command_name
        );
    } else {
        eprintln!("build.rs: {} = {} bytes", elf_name, bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, static_name, &bytes);
}

fn embed_libc_plan_test(out_dir: &str) {
    let libc_sources = [
        "userspace/libc/src/errno.c",
        "userspace/libc/src/syscall.c",
        "userspace/libc/src/unistd.c",
        "userspace/libc/src/string.c",
        "userspace/libc/src/stdio.c",
        "userspace/libc/src/stdlib.c",
        "userspace/libc/src/crt0.S",
        "userspace/libc/include/errno.h",
        "userspace/libc/include/fcntl.h",
        "userspace/libc/include/saios/syscall.h",
        "userspace/libc/include/signal.h",
        "userspace/libc/include/stdarg.h",
        "userspace/libc/include/stddef.h",
        "userspace/libc/include/stdint.h",
        "userspace/libc/include/stdio.h",
        "userspace/libc/include/stdlib.h",
        "userspace/libc/include/string.h",
        "userspace/libc/include/sys/mman.h",
        "userspace/libc/include/sys/types.h",
        "userspace/libc/include/sys/wait.h",
        "userspace/libc/include/unistd.h",
        "userspace/libc_plan_test.c",
        "userspace/user.ld",
    ];
    for source in libc_sources {
        println!("cargo:rerun-if-changed={}", source);
    }

    let cflags = [
        "-ffreestanding",
        "-fno-stack-protector",
        "-fno-builtin",
        "-fno-pic",
        "-mcmodel=large",
        "-mno-red-zone",
        "-nostdinc",
        "-Iuserspace/libc/include",
        "-Wall",
        "-Wextra",
    ];
    let objects = [
        ("userspace/libc/src/crt0.S", "libc_plan_crt0.o"),
        ("userspace/libc/src/errno.c", "libc_plan_errno.o"),
        ("userspace/libc/src/syscall.c", "libc_plan_syscall.o"),
        ("userspace/libc/src/unistd.c", "libc_plan_unistd.o"),
        ("userspace/libc/src/string.c", "libc_plan_string.o"),
        ("userspace/libc/src/stdio.c", "libc_plan_stdio.o"),
        ("userspace/libc/src/stdlib.c", "libc_plan_stdlib.o"),
        ("userspace/libc_plan_test.c", "libc_plan_test.o"),
    ];

    let mut built_objects = Vec::new();
    let mut compiled = true;
    for (source, obj_name) in objects {
        let obj = Path::new(out_dir)
            .join(obj_name)
            .to_string_lossy()
            .into_owned();
        let mut args: Vec<String> = cflags.iter().map(|s| s.to_string()).collect();
        args.extend(["-c".to_string(), source.to_string(), "-o".to_string()]);
        args.push(if cfg!(target_os = "windows") {
            to_wsl_path(&obj)
        } else {
            obj.clone()
        });
        let ok = if cfg!(target_os = "windows") {
            Command::new("wsl")
                .arg("gcc")
                .args(args.iter().map(String::as_str))
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        } else {
            Command::new("gcc")
                .args(args.iter().map(String::as_str))
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        compiled &= ok;
        built_objects.push(obj);
    }

    let elf = Path::new(out_dir)
        .join("libc_plan_test.elf")
        .to_string_lossy()
        .into_owned();
    let out_rs = Path::new(out_dir).join("libc_plan_test_elf.rs");
    let linked = if compiled {
        let mut args = vec![
            "ld".to_string(),
            "-static".to_string(),
            "-nostdlib".to_string(),
            "-T".to_string(),
            "userspace/user.ld".to_string(),
            "-e".to_string(),
            "_start".to_string(),
            "-o".to_string(),
            if cfg!(target_os = "windows") {
                to_wsl_path(&elf)
            } else {
                elf.clone()
            },
        ];
        args.extend(built_objects.iter().map(|obj| {
            if cfg!(target_os = "windows") {
                to_wsl_path(obj)
            } else {
                obj.clone()
            }
        }));
        if cfg!(target_os = "windows") {
            Command::new("wsl")
                .args(args.iter().map(String::as_str))
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        } else {
            Command::new("ld")
                .args(args[1..].iter().map(String::as_str))
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
    } else {
        false
    };

    let bytes = if linked {
        std::fs::read(&elf).unwrap_or_default()
    } else {
        Vec::new()
    };
    if bytes.is_empty() {
        eprintln!(
            "build.rs: libc plan test ELF not built (need gcc/ld) - `libchello` will be a no-op"
        );
    } else {
        eprintln!("build.rs: libc plan test ELF = {} bytes", bytes.len());
    }
    let mut f = std::fs::File::create(&out_rs).unwrap();
    write_byte_array(&mut f, "LIBC_PLAN_TEST_ELF", &bytes);
}

/// Run a host build tool: natively on Linux, via `wsl` on Windows.
fn run_tool(args: &[&str]) -> Option<std::process::Output> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("wsl");
        c.args(args);
        c
    } else {
        let mut c = Command::new(args[0]);
        c.args(&args[1..]);
        c
    };
    cmd.output().ok()
}

/// Shell test (`bash -c <cmd>`), native on Linux / via `wsl` on Windows.
fn host_test(cmd: &str) -> bool {
    let mut c = if cfg!(target_os = "windows") {
        let mut c = Command::new("wsl");
        c.args(["bash", "-c", cmd]);
        c
    } else {
        let mut c = Command::new("bash");
        c.args(["-c", cmd]);
        c
    };
    c.status().map(|s| s.success()).unwrap_or(false)
}

/// Read a host file: native std::fs on Linux, `wsl cat` on Windows.
fn read_host_file(path: &str) -> Vec<u8> {
    if cfg!(target_os = "windows") {
        Command::new("wsl")
            .args(["cat", path])
            .output()
            .map(|o| if o.status.success() { o.stdout } else { vec![] })
            .unwrap_or_default()
    } else {
        std::fs::read(path).unwrap_or_default()
    }
}

fn write_byte_array(f: &mut std::fs::File, name: &str, data: &[u8]) {
    write!(f, "pub static {}: &[u8] = &[", name).unwrap();
    for (i, b) in data.iter().enumerate() {
        if i % 16 == 0 {
            write!(f, "\n    ").unwrap();
        }
        write!(f, "0x{:02x},", b).unwrap();
    }
    writeln!(f, "\n];").unwrap();
}

fn assemble(src: &str, out: &str) {
    let status = if cfg!(target_os = "windows") {
        let wsl_src = to_wsl_path(src);
        let wsl_out = to_wsl_path(out);
        Command::new("wsl")
            .args(["as", "--64", "--no-pad-sections", &wsl_src, "-o", &wsl_out])
            .status()
            .expect("wsl as failed")
    } else {
        Command::new("as")
            .args(["--64", src, "-o", out])
            .status()
            .expect("as failed")
    };
    assert!(status.success(), "Assembly of {} failed", src);
}

fn to_wsl_path(win_path: &str) -> String {
    let abs = if std::path::Path::new(win_path).is_absolute() {
        win_path.to_string()
    } else {
        std::env::current_dir()
            .unwrap()
            .join(win_path)
            .to_string_lossy()
            .into_owned()
    };
    let n = abs.replace('\\', "/");
    if n.len() >= 3 && n.as_bytes()[1] == b':' {
        format!(
            "/mnt/{}{}",
            n.chars().next().unwrap().to_ascii_lowercase(),
            &n[2..]
        )
    } else {
        n
    }
}
