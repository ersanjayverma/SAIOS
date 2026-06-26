use crate::{print, println};
use alloc::string::String;

pub fn setup(args: &str) {
    match args.trim() {
        "bash" => match crate::bash_setup::install_bash() {
            Ok(()) => {}
            Err(e) => println!("setup bash: {}", e),
        },
        "musl" => {
            println!("musl libc setup:");
            println!("  Cross-compile musl on host:");
            println!("    wget https://musl.libc.org/releases/musl-1.2.4.tar.gz");
            println!("    ./configure --prefix=/usr/local/musl");
            println!("    make && make install");
            println!("  Copy libs to SAIOS disk: /lib/x86_64-linux-musl/libc.so");
        }
        "info" | "" => {
            println!("SAIOS setup utility");
            println!("Commands:");
            println!("  setup bash     Install GNU bash 5.2.15");
            println!("  setup musl     Instructions for musl libc");
        }
        other => println!("setup: unknown package '{}'. Try: setup bash", other),
    }
}

pub fn bash_cmd(args: &str) {
    if args.trim() == "--version" || args.trim() == "-version" {
        crate::bash_setup::version_info();
        return;
    }
    if crate::bash_setup::is_installed() {
        super::process::exec(crate::bash_setup::BASH_PATH);
    } else {
        println!("bash not installed. Run: setup bash");
        println!("Or copy a static bash binary to /bin/bash and run: exec /bin/bash");
    }
}

pub fn install(args: &str) {
    let dev = if args.is_empty() {
        "/dev/vda"
    } else {
        args.trim()
    };
    if !crate::block::present() {
        println!("install: no block device found.");
        println!("  In VirtualBox: Machine → Settings → Storage → add a new VDI hard disk.");
        println!("  In QEMU: add -drive file=saios_hdd.img,format=raw");
        return;
    }

    println!("===== SAIOS: Install =====");
    println!("Step 0/4: System overview");
    super::system::sysinfo();
    println!();

    println!("Step 1/4: Analysis Complete");
    let snapshot = crate::saios::storage_platform::decision_snapshot();
    let analysis = &snapshot.target;
    let plan = &snapshot.plan;
    if let Some(reason) = plan.refusal_reason {
        println!("install: no executable target - {}", reason);
        println!("  Disk left untouched. Run: storage graph; storage analyze; sairu diagnose storage");
        return;
    }
    print_destructive_plan(
        "Installation",
        dev,
        analysis.classification,
        plan.risk,
        &plan.operations,
        "Backup first. Continue only if this disk is the intended SAIOS target.",
    );
    println!();

    println!("Step 2/4: User confirmation");
    if !confirm_phrase("install", "INSTALL SAIOS", plan.operation_id, plan.risk, 1) {
        println!("install: cancelled - disk left untouched.");
        return;
    }

    println!("Step 3/4: Formatting and writing {} ...", dev);
    match crate::install::run_approved(dev) {
        Ok(()) => {
            println!("Step 4/4: Install complete.");
            installer_reboot_notice();
        }
        Err(e) => println!("install failed: {}", e),
    }
}

pub fn update(args: &str) {
    let dev = if args.is_empty() {
        "/dev/vda"
    } else {
        args.trim()
    };
    if !crate::block::present() {
        println!("update: no target disk found.");
        println!("  Attach the target disk, then run: update");
        return;
    }

    println!("===== SAIOS: Update =====");
    let snapshot = crate::saios::storage_platform::decision_snapshot();
    let plan = crate::saios::storage_platform::plan_update();
    if let Some(reason) = plan.refusal_reason {
        println!("update: no executable target - {}", reason);
        println!("  Disk left untouched. Run: storage graph; storage analyze; sairu diagnose storage");
        return;
    }
    print_destructive_plan(
        "Update",
        dev,
        snapshot.target.classification,
        plan.risk,
        &plan.operations,
        "Backup first. Compatibility concerns are advisory; confirmation authorizes execution.",
    );
    println!();
    if !confirm_phrase("update", "UPDATE SAIOS", plan.operation_id, plan.risk, 3) {
        println!("update: cancelled - disk left untouched.");
        return;
    }

    match crate::install::update(dev) {
        Ok(()) => {
            println!("Update complete.");
            println!("Type 'reboot' when ready, then boot from the installed disk.");
        }
        Err(e) => {
            println!("update failed: {}", e);
            println!("Recommended next commands:");
            println!("  storage graph");
            println!("  storage plan update");
            println!("  storage recommend");
            println!("  storage analyze");
            println!("  sairu diagnose storage");
        }
    }
}

pub fn reinstall(args: &str) {
    let dev = if args.is_empty() {
        "/dev/vda"
    } else {
        args.trim()
    };
    if !crate::block::present() {
        println!("reinstall: no target disk found.");
        println!("  Attach the target disk, then run: reinstall");
        return;
    }

    println!("===== SAIOS: Reinstall =====");
    let snapshot = crate::saios::storage_platform::decision_snapshot();
    match crate::saios::storage_platform::reinstall_gate() {
        Ok(plan) => {
            print_destructive_plan(
                "Reinstall",
                dev,
                snapshot.target.classification,
                plan.risk,
                &plan.operations,
                "Backup first. Existing install evidence is not required; confirmation authorizes replacement.",
            );
            println!();
            if !confirm_phrase("reinstall", "REINSTALL SAIOS", plan.operation_id, plan.risk, 2) {
                println!("reinstall: cancelled - disk left untouched.");
                return;
            }
        }
        Err(e) => {
            println!("reinstall: no executable target - {}", e);
            println!("  Disk left untouched. Run: storage graph; storage analyze; sairu diagnose storage");
            return;
        }
    }

    match crate::install::run_reinstall_approved(dev) {
        Ok(()) => {
            println!("Reinstall complete.");
            installer_reboot_notice();
        }
        Err(e) => println!("reinstall failed: {}", e),
    }
}

pub fn installer_reboot_notice() -> ! {
    println!("Install finished. System needs to reboot.");
    println!("Rebooting automatically in 10 seconds...");

    let deadline_ns = crate::time::uptime_ns().wrapping_add(10_000_000_000);
    let mut last_remaining = 11u64;
    loop {
        let now = crate::time::uptime_ns();
        let remaining_ns = deadline_ns.saturating_sub(now);
        let remaining = remaining_ns.saturating_add(999_999_999) / 1_000_000_000;
        if remaining == 0 {
            break;
        }
        if remaining != last_remaining {
            println!(
                "Rebooting in {} second{}...",
                remaining,
                if remaining == 1 { "" } else { "s" }
            );
            last_remaining = remaining;
        }
        crate::arch::enable_interrupts();
        crate::arch::halt();
    }

    super::system::reboot()
}

fn print_destructive_plan(
    title: &str,
    dev: &str,
    current_state: &str,
    risk: crate::saios::storage_platform::PlatformRisk,
    operations: &[&'static str],
    recommendation: &str,
) {
    println!("SAIOS {} Plan", title);
    println!("  Target          : {}", dev);
    println!("  Current State   : {}", current_state);
    println!("  Risk            : {}", risk.label());
    println!("  Data Loss       : YES");
    println!(
        "  Recovery        : {}",
        if risk >= crate::saios::storage_platform::PlatformRisk::High {
            "LOW"
        } else {
            "MEDIUM"
        }
    );
    println!("  Recommendation  : {}", recommendation);
    println!("  Impact:");
    for operation in operations {
        println!("    - {}", operation);
    }
    println!("  The final decision belongs to you.");
}

fn confirm_phrase(
    label: &str,
    phrase: &str,
    operation_id: u64,
    risk: crate::saios::storage_platform::PlatformRisk,
    operation_code: u64,
) -> bool {
    crate::diag::watchdog::enter_input_wait();

    let (uid, _gid, euid, _egid) = crate::user::get_current_credentials();
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Override,
        crate::kds::KdsEventType::OverrideRequest,
        crate::kds::KdsSeverity::Warn,
        [operation_id, uid as u64, risk as u64, operation_code],
    );

    print!("  Type {} to continue: ", phrase);
    let mut typed = String::new();
    let deadline_ns = crate::time::uptime_ns().wrapping_add(30_000_000_000);
    loop {
        while let Some(ev) = crate::driver::keyboard::poll() {
            use crate::driver::keyboard::KeyEvent;
            match ev {
                KeyEvent::Enter => {
                    println!();
                    crate::diag::watchdog::leave_input_wait();
                    if typed.as_str() == phrase {
                        crate::serial_println!(
                            "[{}] user confirmation received operation_id={} uid={} euid={} risk={} phrase='{}'",
                            label,
                            operation_id,
                            uid,
                            euid,
                            risk.label(),
                            phrase
                        );
                        crate::observability_contract::ObservabilityContract::kds_event(
                            crate::kds::KdsSubsystem::Override,
                            crate::kds::KdsEventType::OverrideApproved,
                            crate::kds::KdsSeverity::Warn,
                            [operation_id, uid as u64, risk as u64, operation_code],
                        );
                        return true;
                    }
                    crate::serial_println!(
                        "[{}] confirmation mismatch operation_id={} uid={} typed='{}'",
                        label,
                        operation_id,
                        uid,
                        typed.as_str()
                    );
                    crate::observability_contract::ObservabilityContract::kds_event(
                        crate::kds::KdsSubsystem::Override,
                        crate::kds::KdsEventType::OverrideAborted,
                        crate::kds::KdsSeverity::Warn,
                        [operation_id, uid as u64, risk as u64, operation_code],
                    );
                    return false;
                }
                KeyEvent::Backspace => {
                    if typed.pop().is_some() {
                        print!("\x08 \x08");
                    }
                }
                KeyEvent::Char(c) => {
                    if typed.len() < 64 {
                        typed.push(c);
                        print!("{}", c);
                    }
                }
                _ => {}
            }
        }

        if crate::time::uptime_ns() >= deadline_ns {
            crate::serial_println!(
                "[{}] confirmation timeout operation_id={} uid={}",
                label,
                operation_id,
                uid
            );
            println!("(timeout - cancelled)");
            crate::diag::watchdog::leave_input_wait();
            crate::observability_contract::ObservabilityContract::kds_event(
                crate::kds::KdsSubsystem::Override,
                crate::kds::KdsEventType::OverrideAborted,
                crate::kds::KdsSeverity::Warn,
                [operation_id, uid as u64, risk as u64, operation_code],
            );
            return false;
        }

        let _ = crate::interrupts::wait_for_keyboard_input_until(Some(
            crate::shell::commands::boot_ticks().wrapping_add(1),
        ));
    }
}

pub fn saios_cmd(args: &str) {
    let trimmed = args.trim();
    let (cmd, rest) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
    match cmd {
        "install" => install(rest.trim()),
        "update" => update(rest.trim()),
        "reinstall" => reinstall(rest.trim()),
        "recover" => super::system::storage("recover"),
        "rollback" => super::system::storage("rollback"),
        "" | "help" => {
            println!("saios <mode>");
            println!("  saios install [device]");
            println!("  saios update [device]");
            println!("  saios reinstall [device]");
            println!("  saios recover");
            println!("  saios rollback");
        }
        _ => println!("saios: unknown mode '{}'. Try: saios help", cmd),
    }
}

pub fn help_package() {
    println!("  Dev:");
    println!("    todo <text>        append to /home/todo.txt");
    println!("    notes              show /home/todo.txt");
}
