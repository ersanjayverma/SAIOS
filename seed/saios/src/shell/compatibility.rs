use alloc::boxed::Box;

use crate::console;
use crate::saifs;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::ShellContext;

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
}

fn cmd_ls(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().unwrap_or(".");
    let entries = saifs::list(path).map_err(|_| "ls failed")?;
    for name in entries {
        console::println!("{}", name);
    }
    Ok(())
}

fn cmd_pwd(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    console::println!("{}", saifs::pwd());
    Ok(())
}

fn cmd_cd(ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().ok_or("cd: missing path")?;
    saifs::cd(path).map_err(|_| "cd failed")?;
    ctx.session.current_namespace = saifs::pwd();
    Ok(())
}

fn cmd_mkdir(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().ok_or("mkdir: missing path")?;
    saifs::mkdir(path).map_err(|_| "mkdir failed")
}

fn cmd_touch(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().ok_or("touch: missing path")?;
    saifs::touch(path).map_err(|_| "touch failed")
}

fn cmd_cat(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().ok_or("cat: missing path")?;
    let text = saifs::read_text(path).map_err(|_| "cat failed")?;
    if !text.is_empty() {
        console::println!("{}", text);
    }
    Ok(())
}

fn cmd_rm(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().ok_or("rm: missing path")?;
    saifs::remove(path).map_err(|_| "rm failed")
}
