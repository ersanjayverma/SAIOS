mod command;
mod commands;
mod compatibility;
mod dispatcher;
mod engine;
mod parser;
mod prompt;
pub mod programs;
mod registry;
mod service;
mod session;
mod native;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::console;
use hal::arch::x86_64::sync::StaticCell;
use crate::kernel::package_image;
use crate::kernel::object as kom;
use crate::object_manager;
use crate::saifs;

#[derive(Default)]
struct CompletionSnapshot {
    commands: Vec<String>,
    aliases: Vec<String>,
}

static COMPLETION_LOCK: AtomicBool = AtomicBool::new(false);
static COMPLETION: StaticCell<Option<CompletionSnapshot>> = StaticCell::new(None);

fn lock_completion() {
    while COMPLETION_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock_completion() {
    COMPLETION_LOCK.store(false, Ordering::Release);
}

pub(crate) fn update_completion_snapshot(commands: Vec<String>, aliases: Vec<String>) {
    lock_completion();
    // SAFETY: guarded by spin lock.
    unsafe {
        *COMPLETION.get() = Some(CompletionSnapshot { commands, aliases });
    }
    unlock_completion();
}

fn complete_command_token(token: &str) -> Option<String> {
    lock_completion();
    // SAFETY: guarded by spin lock.
    let snapshot = unsafe { &*COMPLETION.get() };
    let mut matches: Vec<String> = Vec::new();
    if let Some(snapshot) = snapshot.as_ref() {
        for name in &snapshot.commands {
            if name.starts_with(token) {
                matches.push(name.clone());
            }
        }
        for name in &snapshot.aliases {
            if name.starts_with(token) {
                matches.push(name.clone());
            }
        }
    }
    unlock_completion();

    matches.sort();
    matches.dedup();
    if matches.len() == 1 {
        return matches.into_iter().next();
    }
    None
}

fn complete_path_token(token: &str) -> Option<String> {
    let (dir, prefix) = if let Some((left, right)) = token.rsplit_once('/') {
        let d = if left.is_empty() { "/" } else { left };
        (d.to_string(), right.to_string())
    } else {
        (".".to_string(), token.to_string())
    };

    let entries = saifs::list(dir.as_str()).ok()?;
    let mut matches: Vec<String> = entries
        .into_iter()
        .filter(|name| name.starts_with(prefix.as_str()))
        .collect();
    matches.sort();
    if matches.len() != 1 {
        return None;
    }

    let completed = matches.pop()?;
    if token.contains('/') {
        if dir == "/" {
            Some(alloc::format!("/{}", completed))
        } else {
            Some(alloc::format!("{}/{}", dir, completed))
        }
    } else {
        Some(completed)
    }
}

pub fn complete_for_console(line: &str, cursor: usize) -> Option<String> {
    let cursor = core::cmp::min(cursor, line.len());
    let head = &line[..cursor];

    let token_start = head
        .rfind(|c: char| c.is_whitespace())
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let token = &head[token_start..];

    if token.is_empty() {
        return None;
    }

    let is_first_token = head[..token_start].trim().is_empty();
    let replacement = if is_first_token {
        complete_command_token(token)
    } else {
        complete_path_token(token)
    }?;

    let mut out = String::new();
    out.push_str(&line[..token_start]);
    out.push_str(replacement.as_str());
    if is_first_token {
        out.push(' ');
    }
    out.push_str(&line[cursor..]);
    Some(out)
}

fn ensure_init_script() {
    let _ = saifs::mkdir("/system");
    let _ = saifs::touch("/system/init");
    let script = b"# SAIOS init script\nsetenv HOSTNAME saios\nalias ll ls\n";
    let _ = crate::vfs::write("/system/init", script);
}

pub fn init() {
    console::clear();
    console::println!("SAIOS v1.0");
    console::println!("UEFI Boot");
    console::println!("Initializing subsystems...");
    console::println!("UTF framebuffer: Cafe Ω α あ ┌─┐ █");
    console::newline();
    object_manager::init();
    saifs::init();
    let _ = package_image::mount_default();
    ensure_init_script();
    kom::init();
}

pub fn run() -> ! {
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}

pub fn start_service() -> Result<(), &'static str> {
    service::start()
}
