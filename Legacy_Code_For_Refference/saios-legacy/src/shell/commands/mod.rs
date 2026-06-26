//! SAIOS built-in shell commands.

mod diagnostics;
mod filesystem;
mod network;
mod package;
mod process;
mod system;
mod users;

use crate::ai;
use crate::shell::config as shell_config;
use crate::version;
use crate::{print, println};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub use diagnostics::diag_str;
pub use filesystem::{
    append_file, cat, cat_read_env, cd, chmod, chown, cp, df, find, grep, hexdump, ls, mkdir, mv,
    pwd, rm, vfs_abs_pub, wc, write_file,
};
pub use network::{fetch, net};
pub use package::{bash_cmd, install, installer_reboot_notice, reinstall, saios_cmd, setup, update};
pub use process::{bootselftest, exec, kill, ps, testsaios};
pub use system::{
    beep, boot_ticks, clear, color, cpuinfo, cpus, disktest, gfx, gziptest, halt, jobs, journal,
    kds, lspci, meminfo, obs, reboot, reload_cmd, resmon, smptest, storage, sysinfo, tick,
    uname, uptime, verify,
};
pub use users::{
    env_cmd, id, login, logout, notes, passwd, set_cmd, su, todo_cmd, useradd, userdel, users,
    whoami,
};

pub fn help() {
    println!(
        "  {} {} - {}",
        version::SAIOS_NAME,
        version::SAIOS_VERSION_TAG,
        version::SAIOS_FULL_NAME
    );
    println!("  -----------------------------------------------------------------");
    system::help_system();
    println!();
    filesystem::help_filesystem();
    println!();
    println!("  Scripting:");
    println!("    echo <text>        print text");
    println!("    calc <expr>        calculator (+−×÷ integers)");
    println!("    run <file>         execute script from ramfs");
    println!("    env                show environment variables");
    println!("    set <key> <val>    set environment variable");
    println!("    history            command history");
    println!();
    network::help_network();
    println!();
    println!("  AI:");
    println!("    ai ask <prompt>    query active AI provider");
    println!("    ai chat            interactive chat session");
    println!("    ai save <f> <p>    save AI response to file");
    println!("    ai use <provider>  ollama | anthropic | openai");
    println!("    ai model <name>    set Ollama model");
    println!("    ai host <ip> <p>   set Ollama host:port");
    println!("    ai key <p> <key>   set API key");
    println!("    ai status          show provider config");
    println!("    reload ai          reload AI state from saios.conf");
    println!();
    println!("  Dev:");
    println!("    sairu <request>    forward a request to the SAIRU Runtime");
    println!("    cc <file>          AI-powered C code analyzer");
    println!("    explain <file>     ask AI to explain a file");
    println!("    todo <text>        append to /home/todo.txt");
    println!("    notes              show /home/todo.txt");
    println!();
    diagnostics::help_diagnostics();
    println!();
    process::help_process();
    println!();
    users::help_users();
    println!();
    println!("  Storage Install:");
    println!("    saios install [device]     analyze, confirm, then install SAIOS");
    println!("    saios update [device]      analyze, confirm, then install over target");
    println!("    saios recover             show recovery operation plan");
    println!("    saios rollback            show rollback operation plan");
    println!("    install [device]          alias for saios install");
    println!("    update [device]           alias for saios update");
    println!("    reinstall [device]        explicit replacement alias");
}

pub fn help_cmd(args: &str) {
    let cmd = args.trim();
    if cmd.is_empty() {
        help();
    } else {
        man_cmd(cmd);
    }
}

pub fn echo(args: &str) {
    println!("{}", args);
}

pub fn calc(args: &str) {
    if args.is_empty() {
        println!("usage: calc <expr>  e.g. calc 3 + 4 * 2");
        return;
    }
    match eval_expr(args.trim()) {
        Some(result) => println!("= {}", result),
        None => println!("calc: could not parse '{}'", args),
    }
}

fn eval_expr(s: &str) -> Option<i64> {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    if tokens.len() == 1 {
        return tokens[0].parse().ok();
    }
    if tokens.len() == 3 {
        let a: i64 = tokens[0].parse().ok()?;
        let b: i64 = tokens[2].parse().ok()?;
        return Some(match tokens[1] {
            "+" | "plus" => a + b,
            "-" | "minus" => a - b,
            "*" | "x" => a * b,
            "/" | "div" => {
                if b != 0 {
                    a / b
                } else {
                    return None;
                }
            }
            "%" | "mod" => {
                if b != 0 {
                    a % b
                } else {
                    return None;
                }
            }
            "**" => {
                let mut r = 1i64;
                for _ in 0..b {
                    r *= a;
                }
                r
            }
            _ => return None,
        });
    }
    None
}

pub fn run(args: &str) {
    if args.is_empty() {
        println!("usage: run <script-path>");
        return;
    }
    match filesystem::read_bytes_for_module(&filesystem::vfs_abs_pub(args)) {
        Ok(data) => {
            let text = match core::str::from_utf8(&data) {
                Ok(s) => s.to_string(),
                Err(_) => {
                    println!("run: not valid UTF-8");
                    return;
                }
            };
            println!("Running {}...", args);
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                println!("saios> {}", line);
                dispatch_line(line);
            }
        }
        Err(e) => println!("run: {}: {}", args, e),
    }
}

pub fn dispatch_line(line: &str) {
    let mut parts = line.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let args = parts.next().unwrap_or("").trim();
    match cmd {
        "cd" => cd(args),
        "pwd" => pwd(),
        "echo" => echo(args),
        "uname" => uname(),
        "sysinfo" => sysinfo(),
        "meminfo" => meminfo(),
        "kds" => kds(args),
        "obs" => obs(args),
        "storage" => storage(args),
        "verify" => verify(args),
        "cpus" => cpus(),
        "ps" => ps(),
        "env" => env_cmd(args),
        "id" => id(),
        "whoami" => whoami(),
        "journal" => journal(args),
        "write" => write_file(args),
        "append" => append_file(args),
        "mkdir" => mkdir(args),
        "rm" => rm(args),
        "cp" => cp(args),
        "mv" => mv(args),
        "cat" => cat(args),
        "ls" => ls(args),
        "find" => find(args),
        "grep" => grep(args),
        "hexdump" => hexdump(args),
        "wc" => wc(args),
        "df" => df(),
        "clear" => clear(),
        "ai" => ai(args),
        "sairu" => sairu(args),
        "fetch" => fetch(args),
        "diag" => diag_str(args),
        "net" => net(args),
        "calc" => calc(args),
        "color" => color(args),
        "todo" => todo_cmd(args),
        "notes" => notes(),
        "testsaios" => testsaios(),
        _ => println!("run: unknown command '{}'", cmd),
    }
}

pub fn shell_builtin(args: &str) {
    let args = args.trim();
    if args == "-c" {
        return;
    }
    if let Some(rest) = args.strip_prefix("-c ") {
        let cmd = rest.trim().trim_matches('"').trim_matches('\'');
        if !cmd.is_empty() {
            crate::shell::run_line(cmd);
        }
        return;
    }
    if args.is_empty() {
        println!("SAIOS shell (sh/bash built-in) - you are already at the shell.");
        println!("Use: sh -c \"<command>\"  or  sh <script-file>");
        return;
    }
    let raw_path = args.split_whitespace().next().unwrap_or("");
    let path = crate::shell::glob::expand_token(raw_path)
        .into_iter()
        .next()
        .unwrap_or_else(|| raw_path.to_string());
    match filesystem::read_bytes_for_module(&filesystem::vfs_abs_pub(&path)) {
        Ok(data) => {
            let text = alloc::string::String::from_utf8_lossy(&data);
            for line in text.lines() {
                let l = line.trim();
                if l.is_empty() || l.starts_with('#') {
                    continue;
                }
                crate::shell::run_line(l);
            }
        }
        Err(_) => println!("sh: {}: No such file or directory", raw_path),
    }
}

pub fn cc(args: &str) {
    if args.is_empty() {
        println!("usage: cc <file.c>");
        println!("  First write code: write /tmp/prog.c int main(){{return 0;}}");
        return;
    }
    match filesystem::read_bytes_for_module(&filesystem::vfs_abs_pub(args)) {
        Ok(data) => {
            let src = match core::str::from_utf8(&data) {
                Ok(s) => s,
                Err(_) => {
                    println!("cc: not valid UTF-8");
                    return;
                }
            };
            let prompt = format!(
                "You are GCC running inside SAIOS (x86_64 bare-metal OS). \
                 Analyze this C code: identify errors, explain what it does, \
                 show what the output would be, and suggest improvements.\n\n\
                 ```c\n{}\n```",
                src
            );
            print!("Compiling via AI... ");
            match ai::complete(&prompt) {
                Some(r) => println!("\n\n{}\n", r),
                None => println!("\n[AI] No response"),
            }
        }
        Err(e) => println!("cc: {}: {}", args, e),
    }
}

pub fn explain(args: &str) {
    if args.is_empty() {
        println!("usage: explain <file>");
        return;
    }
    match filesystem::read_bytes_for_module(&filesystem::vfs_abs_pub(args)) {
        Ok(data) => {
            let text = core::str::from_utf8(&data).unwrap_or("[binary]");
            let prompt = format!(
                "Explain the following file contents concisely. \
                 File: {}\n\n{}",
                args, text
            );
            print!("Asking AI... ");
            match ai::complete(&prompt) {
                Some(r) => println!("\n\n{}\n", r),
                None => println!("\n[AI] No response"),
            }
        }
        Err(e) => println!("explain: {}: {}", args, e),
    }
}

pub fn man_cmd(args: &str) {
    let cmd = args.trim();
    if cmd.is_empty() {
        println!("usage: man <command>");
        println!("Available: try  man ls  or  man ai");
        return;
    }
    let path = alloc::format!("/usr/share/man/man1/{}.1", cmd);
    match crate::vfs_contract::VfsContract::read_file(&path) {
        Ok(buf) => {
            if let Ok(text) = core::str::from_utf8(&buf) {
                crate::manpages::render(text);
            }
        }
        Err(_) => println!("man: no manual entry for '{}'", cmd),
    }
}

pub fn config_cmd(args: &str) {
    let mut parts = args.splitn(2, ' ');
    let sub = parts.next().unwrap_or("");

    match sub {
        "show" => {
            crate::configuration_contract::ConfigurationContract::show();
        }
        "save" => {
            if crate::configuration_contract::ConfigurationContract::save() {
                println!("[config] save success");
            } else {
                println!("[config] save failed: no config loaded");
            }
        }
        "reload" => {
            reload_cmd("config");
        }
        _ => {
            println!("usage: config <show|save|reload>");
        }
    }
}

pub fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let p: Vec<&str> = s.split('.').collect();
    if p.len() != 4 {
        return None;
    }
    Some([
        p[0].parse().ok()?,
        p[1].parse().ok()?,
        p[2].parse().ok()?,
        p[3].parse().ok()?,
    ])
}

pub fn ai(args: &str) {
    let mut parts = args.splitn(2, ' ');
    let sub = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();

    match sub {
        "status" => {
            for line in ai::status() {
                println!("{}", line);
            }
        }
        "use" => {
            let provider_str = match rest.to_lowercase().as_str() {
                "ollama" => {
                    ai::CONFIG.lock().provider = ai::Provider::Ollama;
                    "ollama"
                }
                "anthropic" => {
                    ai::CONFIG.lock().provider = ai::Provider::Anthropic;
                    "anthropic"
                }
                "openai" => {
                    ai::CONFIG.lock().provider = ai::Provider::OpenAI;
                    "openai"
                }
                "together" => {
                    ai::CONFIG.lock().provider = ai::Provider::Together;
                    "together"
                }
                _ => {
                    println!(
                        "unknown provider '{}' - try: ollama | anthropic | openai | together",
                        rest
                    );
                    return;
                }
            };
            println!(
                "Switched to {}",
                provider_str.to_uppercase().replace("ai", "AI")
            );
            crate::configuration_contract::ConfigurationContract::set_provider(provider_str);
        }
        "key" => {
            let mut kp = rest.splitn(2, ' ');
            let prov = kp.next().unwrap_or("");
            let key = kp.next().unwrap_or("").trim();
            if key.is_empty() {
                println!("usage: ai key <anthropic|openai|together> <key>");
                return;
            }
            let leaked: &'static str = String::from(key).leak();
            match prov {
                "anthropic" => {
                    ai::CONFIG.lock().anthropic_key = Some(leaked);
                    println!("Anthropic key set.");
                    crate::configuration_contract::ConfigurationContract::set_anthropic_key(leaked);
                }
                "openai" => {
                    ai::CONFIG.lock().openai_key = Some(leaked);
                    println!("OpenAI key set.");
                    crate::configuration_contract::ConfigurationContract::set_openai_key(leaked);
                }
                "together" => {
                    ai::CONFIG.lock().together_key = Some(leaked);
                    println!("Together key set.");
                    crate::configuration_contract::ConfigurationContract::set_together_key(leaked);
                }
                _ => println!("unknown provider: {}", prov),
            }
        }
        "model" => {
            if rest.is_empty() {
                println!("usage: ai model <name>");
                return;
            }
            let leaked: &'static str = String::from(rest).leak();
            let mut cfg = ai::CONFIG.lock();
            match cfg.provider {
                ai::Provider::Together => {
                    cfg.together_model = Some(leaked);
                    crate::configuration_contract::ConfigurationContract::set_together_model(rest);
                }
                _ => {
                    cfg.ollama_model = Some(leaked);
                    crate::configuration_contract::ConfigurationContract::set_ollama_model(rest);
                }
            }
            println!("Model set to: {}", rest);
            crate::configuration_contract::ConfigurationContract::save();
        }
        "host" => {
            let mut hp = rest.split(' ');
            let h = hp.next().unwrap_or("");
            let p = hp.next().unwrap_or("11434");
            if let (Some(ip), Ok(port)) = (parse_ipv4(h), p.parse::<u16>()) {
                let mut cfg = ai::CONFIG.lock();
                cfg.ollama_host = ip;
                cfg.ollama_port = port;
                println!("Ollama -> {}:{}", h, port);
                crate::configuration_contract::ConfigurationContract::set_ollama_host(h);
                crate::configuration_contract::ConfigurationContract::set_ollama_port(port);
                crate::configuration_contract::ConfigurationContract::save();
            } else {
                println!("usage: ai host <ip> <port>");
            }
        }
        "setup" => ai_setup(),
        "save-config" => {
            crate::configuration_contract::ConfigurationContract::save();
            println!("Saved current AI + apt config (keys in plaintext) to /etc/saios.conf");
        }
        "models" => {
            println!("Together AI models (alias → API id):");
            for (alias, api) in ai::together::MODELS {
                println!("  {:<24} {}", alias, api);
            }
            println!("Select with: ai use together; ai model <alias>");
        }
        "memory" => match rest {
            "clear" => {
                ai::memory::clear();
                println!("AI memory cleared.");
            }
            _ => {
                let m = ai::memory::load();
                if m.is_empty() {
                    println!("(AI memory is empty)");
                } else {
                    println!("{}", m);
                }
            }
        },
        "ask" => {
            if rest.is_empty() {
                println!("usage: ai ask <prompt>");
                return;
            }
            {
                let cfg = ai::CONFIG.lock();
                let missing = match cfg.provider {
                    ai::Provider::Together => cfg.together_key.is_none(),
                    ai::Provider::OpenAI => cfg.openai_key.is_none(),
                    ai::Provider::Anthropic => cfg.anthropic_key.is_none(),
                    ai::Provider::Ollama => false,
                };
                if missing {
                    let p = cfg.provider.clone();
                    drop(cfg);
                    println!(
                        "[AI] no API key for {:?} - set it: ai key {} <key>",
                        p,
                        alloc::format!("{:?}", p).to_lowercase()
                    );
                    return;
                }
            }
            print!("Thinking... ");
            ai::memory::log("ask", rest);
            match ai::complete(rest) {
                Some(resp) => {
                    ai::memory::log("answer", &resp);
                    println!("\n\n{}\n", resp);
                }
                None => println!("\n[AI] No response - run `ai status` to check config"),
            }
        }
        "save" => {
            let mut sp = rest.splitn(2, ' ');
            let file = sp.next().unwrap_or("").trim();
            let prompt = sp.next().unwrap_or("").trim();
            if file.is_empty() || prompt.is_empty() {
                println!("usage: ai save <file> <prompt>");
                return;
            }
            print!("Querying AI... ");
            match ai::complete(prompt) {
                Some(resp) => match filesystem::write_file_for_module(
                    &filesystem::vfs_abs_pub(file),
                    resp.as_bytes(),
                ) {
                    Ok(()) => println!("saved {} bytes -> {}", resp.len(), file),
                    Err(e) => println!("save failed: {}", e),
                },
                None => println!("\n[AI] No response."),
            }
        }
        "chat" => {
            println!("AI Chat - type your message, 'exit' to quit.");
            println!("Provider: {:?}", ai::CONFIG.lock().provider);
            println!("---------------------------------------------");
            println!("(use 'ai ask <message>' for single queries)");
            println!("(interactive chat requires terminal echo support)");
        }
        _ => {
            println!(
                "usage: ai <ask|chat|save|save-config|setup|use|model|models|host|key|status|memory>"
            );
            println!("  providers: ollama | anthropic | openai | together");
            println!("  models:    ai models   (list Together AI model catalog)");
            println!("  setup:     ai setup   (prompts for provider, then key if needed)");
            println!("  runtime:   sairu <request>   (SAIRU Runtime diagnostics)");
        }
    }
}

fn read_line(prompt: &str) -> String {
    use crate::driver::keyboard::{KeyEvent, poll};
    print!("{}", prompt);
    let mut buf = String::new();
    loop {
        let _ = crate::interrupts::wait_for_keyboard_input_until(None);
        if let Some(ev) = poll() {
            match ev {
                KeyEvent::Enter => {
                    println!();
                    break;
                }
                KeyEvent::Backspace => {
                    if buf.pop().is_some() {
                        crate::vga_buffer::backspace();
                    }
                }
                KeyEvent::Char(c) if c >= ' ' && c != '\x7f' => {
                    buf.push(c);
                    print!("{}", c);
                }
                _ => {}
            }
        }
    }
    buf.trim().to_string()
}

fn ai_setup() {
    let prov = read_line("AI provider [ollama | anthropic | openai | together]: ").to_lowercase();
    match prov.as_str() {
        "ollama" => {
            ai::CONFIG.lock().provider = ai::Provider::Ollama;
            let hp = read_line("Ollama host:port [10.0.2.2:11434]: ");
            let hp = if hp.is_empty() {
                String::from("10.0.2.2:11434")
            } else {
                hp
            };
            let mut it = hp.rsplitn(2, ':');
            let port = it.next().unwrap_or("11434").parse::<u16>().unwrap_or(11434);
            let host = it.next().unwrap_or("10.0.2.2");
            if let Some(ip) = parse_ipv4(host) {
                let mut c = ai::CONFIG.lock();
                c.ollama_host = ip;
                c.ollama_port = port;
            }
            let m = read_line("Model [leave blank to keep current]: ");
            if !m.is_empty() {
                ai::CONFIG.lock().ollama_model = Some(m.leak());
            }
            println!("Ollama configured.");
        }
        "anthropic" | "openai" | "together" => {
            let key = read_line("API key: ");
            if key.is_empty() {
                println!("No key entered - aborting setup.");
                return;
            }
            let leaked: &'static str = key.leak();
            {
                let mut c = ai::CONFIG.lock();
                match prov.as_str() {
                    "anthropic" => {
                        c.provider = ai::Provider::Anthropic;
                        c.anthropic_key = Some(leaked);
                    }
                    "openai" => {
                        c.provider = ai::Provider::OpenAI;
                        c.openai_key = Some(leaked);
                    }
                    _ => {
                        c.provider = ai::Provider::Together;
                        c.together_key = Some(leaked);
                    }
                }
            }
            if prov == "together" {
                let m = read_line("Model [openai/gpt-oss-120b]: ");
                if !m.is_empty() {
                    ai::CONFIG.lock().together_model = Some(m.leak());
                }
            }
            println!("{} configured.", prov);
        }
        _ => {
            println!("unknown provider '{}'", prov);
            return;
        }
    }
    crate::config::sync_from_ai();
    println!("Saved to /etc/saios.conf.");
}

pub fn sairu(request: &str) {
    crate::sairu::handle_request(request);
}
