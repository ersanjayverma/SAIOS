use crate::console;
use crate::kernel::process;
use crate::kernel::telemetry;
use crate::saifs;
use crate::saifs::Handle;
use crate::timer;
use alloc::format;
use alloc::string::{String, ToString};

type ProgramResult = Result<i32, &'static str>;

fn hello_program(args: &[&str], env: &[(String, String)]) -> i32 {
    console::println!("Hello from user space!");
    if !args.is_empty() {
        console::println!("args: {}", args.join(" "));
    }
    console::println!("env vars: {}", env.len());
    0
}

fn true_program(_args: &[&str], _env: &[(String, String)]) -> i32 {
    0
}

fn false_program(_args: &[&str], _env: &[(String, String)]) -> i32 {
    1
}

fn argc_program(args: &[&str], _env: &[(String, String)]) -> i32 {
    console::println!("argc={}", args.len());
    args.len() as i32
}

fn env_program(_args: &[&str], env: &[(String, String)]) -> i32 {
    for (k, v) in env {
        console::println!("{}={}", k, v);
    }
    0
}

fn fail_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let code = args
        .first()
        .and_then(|raw| raw.parse::<i32>().ok())
        .unwrap_or(1);
    Ok(code)
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

fn ls_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let path = resolve_relative_path(args.first().copied().unwrap_or("."));
    let entries = saifs::list(path.as_str()).map_err(|_| "ls: failed")?;
    for e in entries {
        console::println!("{}", e);
    }
    Ok(0)
}

fn cat_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let path = resolve_relative_path(args.first().copied().ok_or("cat: missing path")?);
    let text = saifs::read_text(path.as_str()).map_err(|_| "cat: failed")?;
    if !text.is_empty() {
        console::println!("{}", text);
    }
    Ok(0)
}

fn mkdir_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let path = resolve_relative_path(args.first().copied().ok_or("mkdir: missing path")?);
    saifs::mkdir(path.as_str()).map_err(|_| "mkdir: failed")?;
    Ok(0)
}

fn rm_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let path = resolve_relative_path(args.first().copied().ok_or("rm: missing path")?);
    saifs::remove(path.as_str()).map_err(|_| "rm: failed")?;
    Ok(0)
}

fn cp_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let src = resolve_relative_path(args.first().copied().ok_or("cp: missing source")?);
    let dst = resolve_relative_path(args.get(1).copied().ok_or("cp: missing destination")?);

    let src_handle = saifs::open(src.as_str()).map_err(|_| "cp: source open failed")?;
    let data = src_handle.read().map_err(|_| "cp: source read failed")?;

    let _ = saifs::touch(dst.as_str());
    let dst_handle = saifs::open(dst.as_str()).map_err(|_| "cp: destination open failed")?;
    let _ = dst_handle.write(data.as_slice()).map_err(|_| "cp: destination write failed")?;
    Ok(0)
}

fn mv_program(args: &[&str], env: &[(String, String)]) -> ProgramResult {
    cp_program(args, env)?;
    let src = resolve_relative_path(args.first().copied().ok_or("mv: missing source")?);
    saifs::remove(src.as_str()).map_err(|_| "mv: remove source failed")?;
    Ok(0)
}

fn ps_program(_args: &[&str], _env: &[(String, String)]) -> i32 {
    console::println!("PID   STATE     NAME");
    let mut records = process::jobs();
    records.sort_by_key(|p| p.pid);
    for p in records {
        console::println!("{}    {:?}    {}", p.pid, p.state, p.name);
    }
    0
}

fn kill_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let pid = args
        .first()
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or("kill: missing pid")?;
    process::kill(pid)?;
    Ok(0)
}

fn top_program(_args: &[&str], _env: &[(String, String)]) -> i32 {
    let t = telemetry::snapshot();
    let jobs = process::jobs();
    console::println!("CPU logical: {}", t.cpu_logical);
    console::println!("RAM MB: {}", t.ram_mb);
    console::println!("Heap KB: {} / {}", t.heap_used_kb, t.heap_total_kb);
    console::println!("Threads: {}", t.scheduler_threads);
    console::println!("Processes: {}", jobs.len());
    0
}

fn uname_program(_args: &[&str], _env: &[(String, String)]) -> i32 {
    let vendor_bytes = hal::arch::x86_64::cpuid::vendor();
    let mut vendor = String::from_utf8_lossy(&vendor_bytes).to_string();
    vendor.retain(|c| c != '\0');
    let v = crate::kernel::syscall::abi_version();
    console::println!("SAIOS v0.10");
    console::println!("arch=x86_64 cpu_vendor={}", vendor.trim());
    console::println!("syscall_abi={}.{}.{}", v.major, v.minor, v.patch);
    0
}

fn calc_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let expr = args.first().copied().ok_or("calc: missing expression")?.trim();

    let mut op_pos = None;
    let mut op = '\0';
    for (idx, ch) in expr.char_indices() {
        if idx == 0 {
            continue;
        }
        if matches!(ch, '+' | '-' | '*' | '/') {
            op_pos = Some(idx);
            op = ch;
            break;
        }
    }

    let idx = op_pos.ok_or("calc: expression format is <a><op><b>")?;
    let left = expr[..idx]
        .trim()
        .parse::<i64>()
        .map_err(|_| "calc: invalid left operand")?;
    let right = expr[idx + 1..]
        .trim()
        .parse::<i64>()
        .map_err(|_| "calc: invalid right operand")?;

    let out = match op {
        '+' => left.saturating_add(right),
        '-' => left.saturating_sub(right),
        '*' => left.saturating_mul(right),
        '/' => {
            if right == 0 {
                return Err("calc: division by zero");
            }
            left / right
        }
        _ => return Err("calc: unsupported operator"),
    };

    console::println!("{}", out);
    Ok(0)
}

fn stress_program(_args: &[&str], _env: &[(String, String)]) -> i32 {
    let start = timer::uptime().as_millis() as u64;
    let mut acc = 0u64;
    for i in 0..750_000u64 {
        acc = acc.wrapping_add(i.rotate_left((i & 7) as u32));
    }
    let end = timer::uptime().as_millis() as u64;
    let elapsed = end.saturating_sub(start);
    let _ = acc;
    console::println!("Completed in {} ms", elapsed);
    0
}

pub fn execute(name: &str, args: &[&str], env: &[(String, String)]) -> ProgramResult {
    match name {
        n if n.eq_ignore_ascii_case("hello") => Ok(hello_program(args, env)),
        n if n.eq_ignore_ascii_case("true") => Ok(true_program(args, env)),
        n if n.eq_ignore_ascii_case("false") => Ok(false_program(args, env)),
        n if n.eq_ignore_ascii_case("argc") => Ok(argc_program(args, env)),
        n if n.eq_ignore_ascii_case("env") => Ok(env_program(args, env)),
        n if n.eq_ignore_ascii_case("fail") => fail_program(args, env),
        n if n.eq_ignore_ascii_case("ls") => ls_program(args, env),
        n if n.eq_ignore_ascii_case("cat") => cat_program(args, env),
        n if n.eq_ignore_ascii_case("mkdir") => mkdir_program(args, env),
        n if n.eq_ignore_ascii_case("rm") => rm_program(args, env),
        n if n.eq_ignore_ascii_case("cp") => cp_program(args, env),
        n if n.eq_ignore_ascii_case("mv") => mv_program(args, env),
        n if n.eq_ignore_ascii_case("ps") => Ok(ps_program(args, env)),
        n if n.eq_ignore_ascii_case("kill") => kill_program(args, env),
        n if n.eq_ignore_ascii_case("top") => Ok(top_program(args, env)),
        n if n.eq_ignore_ascii_case("uname") => Ok(uname_program(args, env)),
        n if n.eq_ignore_ascii_case("calc") => calc_program(args, env),
        n if n.eq_ignore_ascii_case("stress") => Ok(stress_program(args, env)),
        _ => Err("program not found"),
    }
}

