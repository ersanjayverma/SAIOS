use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;

use crate::console;
use crate::saifs;
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
    let path = resolve_relative_path(args.first().copied().unwrap_or("."));
    let entries = saifs::list(path.as_str()).map_err(|_| "ls failed")?;
    for name in entries {
        console::println!("{}", name);
    }
    Ok(())
}

fn cmd_pwd(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("{}", saifs::pwd());
    Ok(())
}

fn cmd_cd(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let path = resolve_relative_path(args.first().copied().ok_or("cd: missing path")?);
    saifs::cd(path.as_str()).map_err(|_| "cd failed")?;
    ctx.sync_namespace_from_saifs();
    Ok(())
}

fn cmd_mkdir(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let path = resolve_relative_path(args.first().copied().ok_or("mkdir: missing path")?);
    saifs::mkdir(path.as_str()).map_err(|_| "mkdir failed")
}

fn cmd_touch(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let path = resolve_relative_path(args.first().copied().ok_or("touch: missing path")?);
    saifs::touch(path.as_str()).map_err(|_| "touch failed")
}

fn cmd_cat(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.is_empty() {
        if let Some(stdin) = _ctx.env_get("SISH_STDIN") {
            if !stdin.is_empty() {
                console::println!("{}", stdin);
            }
            return Ok(());
        }
        return Err("cat: missing path");
    }

    let path = resolve_relative_path(args.first().copied().ok_or("cat: missing path")?);
    let text = saifs::read_text(path.as_str()).map_err(|_| "cat failed")?;
    if !text.is_empty() {
        console::println!("{}", text);
    }
    Ok(())
}

fn cmd_rm(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let path = resolve_relative_path(args.first().copied().ok_or("rm: missing path")?);
    saifs::remove(path.as_str()).map_err(|_| "rm failed")
}

fn cmd_cp(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let src = resolve_relative_path(args.first().copied().ok_or("cp: missing source")?);
    let dst = resolve_relative_path(args.get(1).copied().ok_or("cp: missing destination")?);

    let data = vfs::read_path(src.as_str()).map_err(|_| "cp: read failed")?;
    vfs::write_path(dst.as_str(), data.as_slice()).map_err(|_| "cp: write failed")?;
    Ok(())
}

fn cmd_mv(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let src = resolve_relative_path(args.first().copied().ok_or("mv: missing source")?);
    let dst = resolve_relative_path(args.get(1).copied().ok_or("mv: missing destination")?);
    vfs::rename(src.as_str(), dst.as_str()).map_err(|_| "mv failed")
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
