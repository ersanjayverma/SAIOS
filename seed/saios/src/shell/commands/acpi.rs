/// ACPI System Command
/// Displays ACPI system information, processor details, and power management options

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::console;
use crate::kernel;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "acpi",
        description: "Display ACPI system information and manage power states",
        handler: cmd_acpi,
    }));
}

fn cmd_acpi(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.is_empty() {
        display_acpi_info();
    } else {
        match args[0] {
            "info" => display_acpi_info(),
            "proc" | "processors" => display_acpi_processors(),
            "tables" => display_acpi_tables(),
            "status" => display_acpi_status(),
            "shutdown" => {
                if let Some(acpi_mgr) = kernel::acpi::get_manager() {
                    match acpi_mgr.shutdown() {
                        Ok(()) => console::println!("System shutdown initiated"),
                        Err(e) => console::println!("Shutdown failed: {}", e),
                    }
                } else {
                    console::println!("ACPI not initialized");
                }
            }
            "help" => print_acpi_help(),
            _ => {
                console::println!("Unknown ACPI subcommand: {}", args[0]);
                console::println!("Try 'acpi help' for usage");
            }
        }
    }
    Ok(())
}

fn print_acpi_help() {
    console::println!("ACPI Commands:");
    console::println!("  acpi            - Show ACPI info");
    console::println!("  acpi info       - Show ACPI system information");
    console::println!("  acpi proc       - Show ACPI processors");
    console::println!("  acpi tables     - Show discovered ACPI tables");
    console::println!("  acpi status     - Show ACPI subsystem status");
    console::println!("  acpi shutdown   - Shutdown system");
    console::println!("  acpi help       - Show this help");
}

fn display_acpi_info() {
    if let Some(acpi_mgr) = kernel::acpi::get_manager() {
        console::println!("ACPI System Information");
        console::println!("=======================");

        match acpi_mgr.oem_info() {
            Ok((oem_id, revision)) => {
                console::println!("ACPI Version:     {}", revision);
                console::println!("OEM ID:           {}", oem_id);
            }
            Err(e) => {
                console::println!("OEM Info Error:   {}", e);
            }
        }

        console::println!("Status:           {}", if acpi_mgr.is_enabled() { "Enabled" } else { "Disabled" });
        console::println!("Processors:       {}", acpi_mgr.processor_count());
        console::println!("Local APIC Addr:  {:#x}", acpi_mgr.local_apic_address());
        console::println!();
        console::println!("Discovered Tables:");
        console::println!("  DSDT, SSDT     - Differentiated/Secondary System Description Tables");
        console::println!("  FADT           - Fixed ACPI Description Table");
        console::println!("  MADT           - Multiple APIC Description Table");
    } else {
        console::println!("ACPI not initialized");
    }
}

fn display_acpi_processors() {
    if let Some(acpi_mgr) = kernel::acpi::get_manager() {
        console::println!("ACPI Processors");
        console::println!("===============");

        let processors = acpi_mgr.processors();
        if processors.is_empty() {
            console::println!("No processors found");
            return;
        }

        console::println!("{:<4} {:<8} {:<8} {:<8}", "Idx", "ACPI ID", "APIC ID", "Flags");
        console::println!("{}", "----".repeat(8));

        for (i, proc) in processors.iter().enumerate() {
            console::println!("{:<4} {:<8} {:<8} {:<#08x}",
                i,
                proc.acpi_processor_id,
                proc.apic_id,
                proc.flags
            );
        }

        console::println!();
        console::println!("Total: {} processors", processors.len());
    } else {
        console::println!("ACPI not initialized");
    }
}

fn display_acpi_tables() {
    console::println!("Discovered ACPI Tables");
    console::println!("======================");

    if let Some(acpi_mgr) = kernel::acpi::get_manager() {
        console::println!("DSDT (Differentiated System Description Table)");
        console::println!("  - System configuration and device objects");
        console::println!();

        console::println!("SSDT (Secondary System Description Table)");
        console::println!("  - Additional device definitions and power states");
        console::println!();

        console::println!("FADT (Fixed ACPI Description Table)");
        console::println!("  - Power management and system fixed features");
        console::println!("  - Local APIC address: {:#x}", acpi_mgr.local_apic_address());
        console::println!();

        console::println!("MADT (Multiple APIC Description Table)");
        console::println!("  - APIC/x2APIC and interrupt source configuration");
        console::println!("  - Processors: {}", acpi_mgr.processor_count());
        console::println!();

        console::println!("Note: Full AML interpreter not yet implemented");
        console::println!("      Power control methods require future development");
    } else {
        console::println!("ACPI not initialized");
    }
}

fn display_acpi_status() {
    console::println!("ACPI Subsystem Status");
    console::println!("====================");

    if let Some(acpi_mgr) = kernel::acpi::get_manager() {
        console::println!("State:              {}", if acpi_mgr.is_enabled() { "Enabled" } else { "Disabled");
        console::println!("Version:            {}", acpi_mgr.revision());
        console::println!("Processors:         {}", acpi_mgr.processor_count());
        console::println!();

        console::println!("Capabilities:");
        console::println!("  [x] Table parsing");
        console::println!("  [x] RSDP/RSDT/XSDT discovery");
        console::println!("  [x] MADT processor enumeration");
        console::println!("  [x] FADT information");
        console::println!("  [ ] AML interpreter");
        console::println!("  [ ] Power state transitions");
        console::println!("  [ ] Device enumeration via DSDT");
        console::println!();

        let proc_count = acpi_mgr.processor_count();
        if proc_count > 0 {
            console::println!("System Processors:");
            for proc in acpi_mgr.processors() {
                console::println!("  APIC {}: ACPI ID {} (flags={:#x})",
                    proc.apic_id,
                    proc.acpi_processor_id,
                    proc.flags
                );
            }
        } else {
            console::println!("No processors enumerated");
        }
    } else {
        console::println!("ACPI not initialized");
        console::println!();
        console::println!("Possible causes:");
        console::println!("  - System firmware does not support ACPI");
        console::println!("  - RSDP not found in configuration tables");
        console::println!("  - Boot environment did not provide ACPI information");
    }
}
