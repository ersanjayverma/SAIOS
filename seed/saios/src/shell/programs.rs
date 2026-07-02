use crate::console;
use crate::kernel::process;
use crate::kernel::telemetry;
use crate::saifs;
use crate::saifs::Handle;
use crate::timer;
use crate::vfs;
use alloc::format;
use alloc::string::{String, ToString};

type ProgramResult = Result<i32, &'static str>;

#[derive(Clone, Debug)]
pub struct BinaryMetadata {
    pub entry: String,
    pub pie: bool,
    pub preferred_base: u64,
}

fn hello_program(args: &[&str], env: &[(String, String)]) -> i32 {
    console::println!("Hello from user space!");
    if !args.is_empty() {
        console::println!("args: {}", args.join(" "));
    }
    console::println!("env vars: {}", env.len());
    0
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

fn parse_cc_output(args: &[&str]) -> Option<String> {
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == "-o" {
            return args.get(i + 1).map(|s| (*s).to_string());
        }
        i += 1;
    }
    None
}

fn infer_output_from_source(src: &str) -> String {
    let base = src.rsplit('/').next().unwrap_or(src);
    let stem = if let Some(stripped) = base.strip_suffix(".c") {
        stripped
    } else {
        base
    };

    let dir = if let Some((d, _)) = src.rsplit_once('/') {
        if d.is_empty() {
            "/"
        } else {
            d
        }
    } else {
        "."
    };

    if dir == "/" {
        format!("/{}", stem)
    } else if dir == "." {
        stem.to_string()
    } else {
        format!("{}/{}", dir, stem)
    }
}

fn extract_first_string_literal(source: &str) -> Option<String> {
    let bytes = source.as_bytes();
    let mut start = None;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'"' {
            if let Some(s) = start {
                if i > s + 1 {
                    let lit = &source[s + 1..i];
                    return Some(lit.replace("\\n", "\n"));
                }
                start = None;
            } else {
                start = Some(i);
            }
        }
    }
    None
}

fn cc_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let src_arg = args.first().copied().ok_or("cc: missing source file")?;
    let src = resolve_relative_path(src_arg);
    let source = saifs::read_text(src.as_str()).map_err(|_| "cc: source read failed")?;

    let out = if let Some(o) = parse_cc_output(args) {
        resolve_relative_path(o.as_str())
    } else {
        resolve_relative_path(infer_output_from_source(src.as_str()).as_str())
    };

    let message = extract_first_string_literal(source.as_str())
        .unwrap_or_else(|| "Hello World".to_string());

    let payload = format!(
        "SAIOS_CC_STUB\nsource={}\nmessage={}\n",
        src,
        message.replace('\n', "\\n")
    );

    let _ = saifs::touch(out.as_str());
    let out_handle = saifs::open(out.as_str()).map_err(|_| "cc: output open failed")?;
    let _ = out_handle
        .write(payload.as_bytes())
        .map_err(|_| "cc: output write failed")?;

    console::println!("cc: compiled {} -> {}", src, out);
    Ok(0)
}

fn compiled_stub_message(path: &str) -> Option<String> {
    let text = saifs::read_text(path).ok()?;
    if !text.starts_with("SAIOS_CC_STUB") {
        return None;
    }

    for line in text.lines() {
        if let Some(msg) = line.strip_prefix("message=") {
            return Some(msg.replace("\\n", "\n"));
        }
    }

    Some("Hello World".to_string())
}

fn execute_compiled_stub(path: &str, args: &[&str]) -> ProgramResult {
    let Some(msg) = compiled_stub_message(path) else {
        return Err("program not found");
    };

    console::println!("{}", msg);
    if !args.is_empty() {
        console::println!("args: {}", args.join(" "));
    }
    Ok(0)
}

fn ls_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let path = resolve_relative_path(args.first().copied().unwrap_or("."));
    let entries = vfs::ls(Some(path.as_str())).map_err(|_| "ls: failed")?;
    for e in entries {
        console::println!("{}", e);
    }
    Ok(0)
}

fn cat_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let path = resolve_relative_path(args.first().copied().ok_or("cat: missing path")?);
    let fd = vfs::open(path.as_str(), vfs::OpenOptions::read_only()).map_err(|_| "cat: open failed")?;
    let read_result = vfs::read(fd, usize::MAX);
    let close_result = vfs::close(fd);
    let data = read_result.map_err(|_| "cat: read failed")?;
    close_result.map_err(|_| "cat: close failed")?;
    let text = String::from_utf8_lossy(&data).into_owned();
    if !text.is_empty() {
        console::println!("{}", text);
    }
    Ok(0)
}

fn mkdir_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let path = resolve_relative_path(args.first().copied().ok_or("mkdir: missing path")?);
    vfs::mkdir(path.as_str()).map_err(|_| "mkdir: failed")?;
    Ok(0)
}

fn rm_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let path = resolve_relative_path(args.first().copied().ok_or("rm: missing path")?);
    vfs::unlink(path.as_str()).map_err(|_| "rm: failed")?;
    Ok(0)
}

fn cp_program(args: &[&str], _env: &[(String, String)]) -> ProgramResult {
    let src = resolve_relative_path(args.first().copied().ok_or("cp: missing source")?);
    let dst = resolve_relative_path(args.get(1).copied().ok_or("cp: missing destination")?);

    let src_fd = vfs::open(src.as_str(), vfs::OpenOptions::read_only())
        .map_err(|_| "cp: source open failed")?;
    let read_result = vfs::read(src_fd, usize::MAX);
    let src_close_result = vfs::close(src_fd);
    let data = read_result.map_err(|_| "cp: source read failed")?;
    src_close_result.map_err(|_| "cp: source close failed")?;

    let dst_fd = vfs::open(dst.as_str(), vfs::OpenOptions::write_only_create())
        .map_err(|_| "cp: destination open failed")?;
    let write_result = vfs::write(dst_fd, data.as_slice());
    let dst_close_result = vfs::close(dst_fd);
    let _ = write_result.map_err(|_| "cp: destination write failed")?;
    dst_close_result.map_err(|_| "cp: destination close failed")?;
    Ok(0)
}

fn mv_program(args: &[&str], env: &[(String, String)]) -> ProgramResult {
    let src = resolve_relative_path(args.first().copied().ok_or("mv: missing source")?);
    let dst = resolve_relative_path(args.get(1).copied().ok_or("mv: missing destination")?);
    if vfs::rename(src.as_str(), dst.as_str()).is_ok() {
        return Ok(0);
    }

    cp_program(args, env)?;
    vfs::unlink(src.as_str()).map_err(|_| "mv: remove source failed")?;
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

fn shell_program(_args: &[&str], _env: &[(String, String)]) -> i32 {
    console::println!("shell binary: interactive mode is provided by SISH service");
    0
}

fn editor_program(args: &[&str], _env: &[(String, String)]) -> i32 {
    let target = args.first().copied().unwrap_or("untitled.txt");
    console::println!("editor binary: interactive editor not wired yet");
    console::println!("target: {}", target);
    0
}

fn parse_u64_value(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16).ok();
    }
    trimmed.parse::<u64>().ok()
}

pub fn binary_metadata(path: &str) -> Option<BinaryMetadata> {
    let text = saifs::read_text(path).ok()?;
    if text.starts_with("SAIOS_BIN_V1") {
        let mut entry: Option<String> = None;
        let mut pie = false;
        let mut preferred_base = 0x0040_0000u64;

        for line in text.lines() {
            if let Some(raw_entry) = line.strip_prefix("entry=") {
                let trimmed = raw_entry.trim();
                if !trimmed.is_empty() {
                    entry = Some(trimmed.to_string());
                }
                continue;
            }

            if let Some(raw) = line.strip_prefix("type=") {
                if raw.trim().eq_ignore_ascii_case("pie") {
                    pie = true;
                }
                continue;
            }

            if let Some(raw) = line.strip_prefix("preferred_base=") {
                if let Some(base) = parse_u64_value(raw) {
                    preferred_base = base;
                }
            }
        }

        let entry = entry?;
        return Some(BinaryMetadata {
            entry,
            pie,
            preferred_base,
        });
    }
    None
}

fn execute_entry(entry: &str, args: &[&str], env: &[(String, String)]) -> ProgramResult {
    match entry {
        n if n.eq_ignore_ascii_case("hello") => Ok(hello_program(args, env)),
        n if n.eq_ignore_ascii_case("calc") => calc_program(args, env),
        n if n.eq_ignore_ascii_case("editor") => Ok(editor_program(args, env)),
        n if n.eq_ignore_ascii_case("shell") => Ok(shell_program(args, env)),
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
        n if n.eq_ignore_ascii_case("cc") => cc_program(args, env),
        n if n.eq_ignore_ascii_case("stress") => Ok(stress_program(args, env)),
        _ => Err("program not found"),
    }
}

pub fn execute_path(path: &str, name: &str, args: &[&str], env: &[(String, String)]) -> ProgramResult {
    if compiled_stub_message(path).is_some() {
        return execute_compiled_stub(path, args);
    }

    let entry = binary_metadata(path)
        .map(|m| m.entry)
        .unwrap_or_else(|| name.to_string());
    execute_entry(entry.as_str(), args, env)
}

pub fn spawn(name: &str, args: &[&str], env: &[(String, String)]) -> ProgramResult {
    execute_entry(name, args, env)
}

pub fn exit(code: i32) -> ProgramResult {
    Ok(code)
}

pub fn wait(code: i32) -> ProgramResult {
    Ok(code)
}

pub fn exec(name: &str, args: &[&str], env: &[(String, String)]) -> ProgramResult {
    execute_entry(name, args, env)
}

pub fn supports_binary(path: &str) -> bool {
    binary_metadata(path).is_some() || compiled_stub_message(path).is_some()
}

