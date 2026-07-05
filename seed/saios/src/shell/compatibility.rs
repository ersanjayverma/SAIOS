use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::console;
use crate::saifs;
use crate::shell::cat;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;
use crate::vfs;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "ls",
        description: "Compatibility: list namespace entries",
        handler: cmd_ls,
    }));
    registry.register(Box::new(StaticCommand {
        name: "pwd",
        description: "Compatibility: print current namespace",
        handler: cmd_pwd,
    }));
    registry.register(Box::new(StaticCommand {
        name: "cd",
        description: "Compatibility: change namespace",
        handler: cmd_cd,
    }));
    registry.register(Box::new(StaticCommand {
        name: "mkdir",
        description: "Compatibility: create directory object",
        handler: cmd_mkdir,
    }));
    registry.register(Box::new(StaticCommand {
        name: "touch",
        description: "Compatibility: create file object",
        handler: cmd_touch,
    }));
    registry.register(Box::new(StaticCommand {
        name: "cat",
        description: "Compatibility: read object/file contents",
        handler: cmd_cat,
    }));
    registry.register(Box::new(StaticCommand {
        name: "rm",
        description: "Compatibility: remove object/file",
        handler: cmd_rm,
    }));
    registry.register(Box::new(StaticCommand {
        name: "cp",
        description: "Compatibility: copy file",
        handler: cmd_cp,
    }));
    registry.register(Box::new(StaticCommand {
        name: "mv",
        description: "Compatibility: move/rename file",
        handler: cmd_mv,
    }));
}

fn cmd_ls(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let (flags, positionals) = split_flags(args);
    if has_help_flag(&flags) {
        console::println!("usage: ls [-a] [path]");
        console::println!("  -a, --all   show full SAIFS properties per entry");
        return Ok(());
    }

    let mut show_all = false;
    for flag in &flags {
        match *flag {
            "-a" | "--all" => show_all = true,
            _ => return Err("ls: unknown flag"),
        }
    }

    if positionals.len() > 1 {
        return Err("ls: too many paths");
    }

    let path = resolve_relative_path(positionals.first().copied().unwrap_or("."));
    let entries = saifs::list(path.as_str()).map_err(|_| "ls failed")?;

    if show_all {
        console::println!(
            "{:<24}  {:<10}  {:<10}  {:<10}  {:<8}  {:<8}  PROPERTIES",
            "NAME", "KIND", "TYPE", "STATUS", "HEALTH", "SIZE"
        );
        console::println!(
            "{:-<24}  {:-<10}  {:-<10}  {:-<10}  {:-<8}  {:-<8}  {:-<10}",
            "", "", "", "", "", "", ""
        );
    }

    for name in entries {
        if !show_all {
            console::println!("{}", name);
            continue;
        }

        let full_path = join_path(path.as_str(), name.as_str());
        match saifs::open(full_path.as_str()) {
            Ok(handle) => {
                let kind = match handle.kind() {
                    saifs::SaifsNodeKind::File => "file",
                    saifs::SaifsNodeKind::Directory => "dir",
                    saifs::SaifsNodeKind::Object => "object",
                    saifs::SaifsNodeKind::Virtual => "virtual",
                };
                let object_type = handle
                    .object_type()
                    .map(object_type_label)
                    .unwrap_or("-");
                let status = handle.status().map(object_status_label).unwrap_or("-");
                let health = crate::saifs::Handle::health(&handle)
                    .map(health_label)
                    .unwrap_or("-");
                let properties = crate::saifs::Handle::properties(&handle).unwrap_or_default();
                let size = properties
                    .iter()
                    .find(|p| p.key == "size")
                    .map(|p| p.value.as_str())
                    .unwrap_or("-");

                let mut prop_summary = String::new();
                for (idx, prop) in properties.iter().enumerate() {
                    if idx > 0 {
                        prop_summary.push_str("; ");
                    }
                    prop_summary.push_str(prop.key.as_str());
                    prop_summary.push('=');
                    prop_summary.push_str(prop.value.as_str());
                }

                console::println!(
                    "{:<24.24}  {:<10.10}  {:<10.10}  {:<10.10}  {:<8.8}  {:<8.8}  {}",
                    name,
                    kind,
                    object_type,
                    status,
                    health,
                    size,
                    prop_summary
                );
            }
            Err(_) => {
                console::println!(
                    "{:<24.24}  {:<10}  {:<10}  {:<10}  {:<8}  {:<8}  {}",
                    name,
                    "-",
                    "-",
                    "-",
                    "-",
                    "-",
                    "open=failed"
                );
            }
        }
    }
    Ok(())
}

fn cmd_pwd(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let (flags, positionals) = split_flags(_args);
    if has_help_flag(&flags) {
        console::println!("usage: pwd");
        return Ok(());
    }
    if !flags.is_empty() {
        return Err("pwd: unknown flag");
    }
    if !positionals.is_empty() {
        return Err("pwd: unexpected argument");
    }

    console::println!("{}", saifs::pwd());
    Ok(())
}

fn cmd_cd(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let (flags, positionals) = split_flags(args);
    if has_help_flag(&flags) {
        console::println!("usage: cd <path>");
        return Ok(());
    }
    if !flags.is_empty() {
        return Err("cd: unknown flag");
    }
    if positionals.len() != 1 {
        return Err("cd: missing path");
    }

    let path = resolve_relative_path(positionals[0]);
    saifs::cd(path.as_str()).map_err(|_| "cd failed")?;
    ctx.sync_namespace_from_saifs();
    Ok(())
}

fn cmd_mkdir(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let (flags, positionals) = split_flags(args);
    if has_help_flag(&flags) {
        console::println!("usage: mkdir <path>");
        return Ok(());
    }
    if !flags.is_empty() {
        return Err("mkdir: unknown flag");
    }
    if positionals.len() != 1 {
        return Err("mkdir: missing path");
    }

    let path = resolve_relative_path(positionals[0]);
    saifs::mkdir(path.as_str()).map_err(|_| "mkdir failed")
}

fn cmd_touch(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let (flags, positionals) = split_flags(args);
    if has_help_flag(&flags) {
        console::println!("usage: touch <path>");
        return Ok(());
    }
    if !flags.is_empty() {
        return Err("touch: unknown flag");
    }
    if positionals.len() != 1 {
        return Err("touch: missing path");
    }

    let path = resolve_relative_path(positionals[0]);
    saifs::touch(path.as_str()).map_err(|_| "touch failed")
}

fn cmd_cat(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let (flags, positionals) = split_flags(args);
    if has_help_flag(&flags) {
        console::println!("usage: cat <path>");
        console::println!("       cat              # reads from stdin when piped");
        return Ok(());
    }
    if !flags.is_empty() {
        return Err("cat: unknown flag");
    }

    if positionals.is_empty() {
        if let Some(stdin) = _ctx.env_get("SISH_STDIN") {
            if !stdin.is_empty() {
                console::println!("{}", stdin);
            }
            return Ok(());
        }
        return Err("cat: missing path");
    }

    if positionals.len() != 1 {
        return Err("cat: too many paths");
    }

    let path = resolve_relative_path(positionals[0]);
    let rendered = cat::read_rendered(path.as_str()).map_err(cat::map_read_error)?;
    if !rendered.is_empty() {
        console::println!("{}", rendered);
    }
    Ok(())
}

fn cmd_rm(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let (flags, positionals) = split_flags(args);
    if has_help_flag(&flags) {
        console::println!("usage: rm <path>");
        return Ok(());
    }
    if !flags.is_empty() {
        return Err("rm: unknown flag");
    }
    if positionals.len() != 1 {
        return Err("rm: missing path");
    }

    let path = resolve_relative_path(positionals[0]);
    saifs::remove(path.as_str()).map_err(|_| "rm failed")
}

fn cmd_cp(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let (flags, positionals) = split_flags(args);
    if has_help_flag(&flags) {
        console::println!("usage: cp <source> <destination>");
        return Ok(());
    }
    if !flags.is_empty() {
        return Err("cp: unknown flag");
    }
    if positionals.len() < 2 {
        return Err("cp: missing destination");
    }
    if positionals.len() > 2 {
        return Err("cp: too many arguments");
    }

    let src = resolve_relative_path(positionals[0]);
    let dst = resolve_relative_path(positionals[1]);

    let data = vfs::read_path(src.as_str()).map_err(|_| "cp: read failed")?;
    vfs::write_path(dst.as_str(), data.as_slice()).map_err(|_| "cp: write failed")?;
    Ok(())
}

fn cmd_mv(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let (flags, positionals) = split_flags(args);
    if has_help_flag(&flags) {
        console::println!("usage: mv <source> <destination>");
        return Ok(());
    }
    if !flags.is_empty() {
        return Err("mv: unknown flag");
    }
    if positionals.len() < 2 {
        return Err("mv: missing destination");
    }
    if positionals.len() > 2 {
        return Err("mv: too many arguments");
    }

    let src = resolve_relative_path(positionals[0]);
    let dst = resolve_relative_path(positionals[1]);
    vfs::rename(src.as_str(), dst.as_str()).map_err(|_| "mv failed")
}

fn has_help_flag(flags: &[&str]) -> bool {
    flags
        .iter()
        .any(|f| *f == "-h" || *f == "--help")
}

fn split_flags<'a>(args: &'a [&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
    let mut flags = Vec::new();
    let mut positionals = Vec::new();
    let mut positional_mode = false;

    for arg in args {
        if positional_mode {
            positionals.push(*arg);
            continue;
        }

        if *arg == "--" {
            positional_mode = true;
            continue;
        }

        if arg.starts_with('-') {
            flags.push(*arg);
        } else {
            positionals.push(*arg);
        }
    }

    (flags, positionals)
}

fn join_path(base: &str, name: &str) -> String {
    if name.starts_with('/') {
        return name.to_string();
    }
    if base == "/" {
        return format!("/{}", name);
    }
    format!("{}/{}", base, name)
}

fn object_type_label(value: crate::object_manager::ObjectType) -> &'static str {
    match value {
        crate::object_manager::ObjectType::Kernel => "kernel",
        crate::object_manager::ObjectType::AiSkill => "ai-skill",
        crate::object_manager::ObjectType::File => "file",
        crate::object_manager::ObjectType::Process => "process",
        crate::object_manager::ObjectType::Thread => "thread",
        crate::object_manager::ObjectType::Driver => "driver",
        crate::object_manager::ObjectType::Device => "device",
        crate::object_manager::ObjectType::Service => "service",
        crate::object_manager::ObjectType::Volume => "volume",
        crate::object_manager::ObjectType::MemoryRegion => "memory",
        crate::object_manager::ObjectType::NetworkInterface => "network-if",
        crate::object_manager::ObjectType::Timer => "timer",
        crate::object_manager::ObjectType::Event => "event",
    }
}

fn object_status_label(value: crate::object_manager::ObjectStatus) -> &'static str {
    match value {
        crate::object_manager::ObjectStatus::Online => "online",
        crate::object_manager::ObjectStatus::Offline => "offline",
        crate::object_manager::ObjectStatus::Faulted => "faulted",
        crate::object_manager::ObjectStatus::Busy => "busy",
    }
}

fn health_label(value: crate::object_manager::Health) -> &'static str {
    match value {
        crate::object_manager::Health::Healthy => "healthy",
        crate::object_manager::Health::Warning => "warning",
        crate::object_manager::Health::Critical => "critical",
        crate::object_manager::Health::Offline => "offline",
    }
}

fn resolve_relative_path(path: &str) -> String {
    if path.starts_with('/') {
        return path.to_string();
    }

    let cwd = saifs::pwd();
    if cwd == "/" {
        format!("/{}", path)
    } else {
        format!("{}/{}", cwd, path)
    }
}
