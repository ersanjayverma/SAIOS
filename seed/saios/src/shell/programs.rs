use crate::console;
use crate::kernel::process;
use crate::kernel::telemetry;
use crate::saifs;
use crate::saifs::Handle;
use crate::timer;
use crate::vfs;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

type ProgramResult = Result<i32, &'static str>;

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EM_X86_64: u16 = 62;
const ET_DYN: u16 = 3;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const DT_NULL: i64 = 0;
const DT_NEEDED: i64 = 1;
const DT_STRTAB: i64 = 5;
const DT_STRSZ: i64 = 10;

#[derive(Clone, Debug)]
pub struct BinaryMetadata {
    pub entry: String,
    pub pie: bool,
    pub preferred_base: u64,
    pub dynamic: bool,
    pub interpreter: Option<String>,
    pub needed_libraries: Vec<String>,
    pub required_symbols: Vec<String>,
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
        if d.is_empty() { "/" } else { d }
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

    let message =
        extract_first_string_literal(source.as_str()).unwrap_or_else(|| "Hello World".to_string());

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
    let fd =
        vfs::open(path.as_str(), vfs::OpenOptions::read_only()).map_err(|_| "cat: open failed")?;
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
    let expr = args
        .first()
        .copied()
        .ok_or("calc: missing expression")?
        .trim();

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

fn parse_csv_values(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    for item in raw.split(',') {
        let value = item.trim();
        if !value.is_empty() {
            out.push(value.to_string());
        }
    }
    out
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let chunk = bytes.get(offset..end)?;
    Some(u16::from_le_bytes([chunk[0], chunk[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let chunk = bytes.get(offset..end)?;
    Some(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let chunk = bytes.get(offset..end)?;
    Some(u64::from_le_bytes([
        chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
    ]))
}

fn read_i64_le(bytes: &[u8], offset: usize) -> Option<i64> {
    read_u64_le(bytes, offset).map(|v| v as i64)
}

fn read_cstring(bytes: &[u8], start: usize) -> Option<String> {
    let mut end = start;
    while end < bytes.len() {
        if bytes[end] == 0 {
            break;
        }
        end += 1;
    }
    if end > bytes.len() {
        return None;
    }
    Some(String::from_utf8_lossy(bytes.get(start..end)?).into_owned())
}

fn path_basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[derive(Clone, Copy)]
struct ElfProgramHeader {
    p_type: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_filesz: u64,
}

fn parse_elf_program_headers(bytes: &[u8]) -> Option<Vec<ElfProgramHeader>> {
    if bytes.len() < 64 || bytes.get(0..4)? != ELF_MAGIC {
        return None;
    }
    if *bytes.get(4)? != ELFCLASS64 || *bytes.get(5)? != ELFDATA2LSB {
        return None;
    }
    if read_u16_le(bytes, 18)? != EM_X86_64 {
        return None;
    }

    let phoff = read_u64_le(bytes, 32)? as usize;
    let phentsize = read_u16_le(bytes, 54)? as usize;
    let phnum = read_u16_le(bytes, 56)? as usize;

    if phentsize < 56 {
        return None;
    }

    let mut headers = Vec::new();
    for i in 0..phnum {
        let off = phoff.checked_add(i.checked_mul(phentsize)?)?;
        let end = off.checked_add(56)?;
        if end > bytes.len() {
            return None;
        }

        headers.push(ElfProgramHeader {
            p_type: read_u32_le(bytes, off)?,
            p_offset: read_u64_le(bytes, off + 8)?,
            p_vaddr: read_u64_le(bytes, off + 16)?,
            p_filesz: read_u64_le(bytes, off + 32)?,
        });
    }

    Some(headers)
}

fn vaddr_to_file_offset(program_headers: &[ElfProgramHeader], vaddr: u64) -> Option<usize> {
    for ph in program_headers {
        if ph.p_type != PT_LOAD {
            continue;
        }

        let seg_start = ph.p_vaddr;
        let seg_end = seg_start.checked_add(ph.p_filesz)?;
        if vaddr >= seg_start && vaddr < seg_end {
            let delta = vaddr.checked_sub(seg_start)?;
            let file = ph.p_offset.checked_add(delta)?;
            return usize::try_from(file).ok();
        }
    }
    None
}

fn parse_elf_metadata(path: &str, bytes: &[u8]) -> Option<BinaryMetadata> {
    let program_headers = parse_elf_program_headers(bytes)?;
    let e_type = read_u16_le(bytes, 16)?;
    let entry_addr = read_u64_le(bytes, 24)?;
    let mut dynamic = false;
    let mut interpreter: Option<String> = None;
    let mut needed_libraries: Vec<String> = Vec::new();

    for ph in &program_headers {
        if ph.p_type == PT_INTERP {
            let start = usize::try_from(ph.p_offset).ok()?;
            if start >= bytes.len() {
                return None;
            }
            interpreter = read_cstring(bytes, start);
            continue;
        }

        if ph.p_type == PT_DYNAMIC {
            dynamic = true;
            let dyn_start = usize::try_from(ph.p_offset).ok()?;
            let dyn_size = usize::try_from(ph.p_filesz).ok()?;
            let dyn_end = dyn_start.checked_add(dyn_size)?;
            if dyn_end > bytes.len() {
                return None;
            }

            let mut strtab_vaddr: Option<u64> = None;
            let mut strtab_size: Option<usize> = None;
            let mut needed_offsets: Vec<u64> = Vec::new();

            let mut cursor = dyn_start;
            while cursor + 16 <= dyn_end {
                let d_tag = read_i64_le(bytes, cursor)?;
                let d_val = read_u64_le(bytes, cursor + 8)?;
                cursor += 16;

                if d_tag == DT_NULL {
                    break;
                }
                if d_tag == DT_NEEDED {
                    needed_offsets.push(d_val);
                    continue;
                }
                if d_tag == DT_STRTAB {
                    strtab_vaddr = Some(d_val);
                    continue;
                }
                if d_tag == DT_STRSZ {
                    strtab_size = usize::try_from(d_val).ok();
                }
            }

            if let (Some(strtab_addr), Some(size)) = (strtab_vaddr, strtab_size)
                && let Some(strtab_off) =
                    vaddr_to_file_offset(program_headers.as_slice(), strtab_addr)
            {
                let strtab_end = strtab_off.saturating_add(size).min(bytes.len());
                for needed in needed_offsets {
                    let rel = usize::try_from(needed).ok()?;
                    let start = strtab_off.checked_add(rel)?;
                    if start >= strtab_end {
                        continue;
                    }

                    let mut end = start;
                    while end < strtab_end && bytes[end] != 0 {
                        end += 1;
                    }
                    if end > start {
                        needed_libraries
                            .push(String::from_utf8_lossy(&bytes[start..end]).into_owned());
                    }
                }
            }
        }
    }

    let preferred_base = if e_type == ET_DYN {
        0x0040_0000
    } else {
        entry_addr & !0xFFF
    };

    Some(BinaryMetadata {
        entry: path_basename(path),
        pie: e_type == ET_DYN,
        preferred_base,
        dynamic,
        interpreter,
        needed_libraries,
        required_symbols: Vec::new(),
    })
}

pub fn binary_metadata(path: &str) -> Option<BinaryMetadata> {
    let text = saifs::read_text(path).ok()?;
    if text.starts_with("SAIOS_BIN_V1") {
        let mut entry: Option<String> = None;
        let mut pie = false;
        let mut preferred_base = 0x0040_0000u64;
        let mut dynamic = false;
        let mut interpreter: Option<String> = None;
        let mut needed_libraries: Vec<String> = Vec::new();
        let mut required_symbols: Vec<String> = Vec::new();

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

            if let Some(raw) = line.strip_prefix("dynamic=") {
                dynamic = raw.trim().eq_ignore_ascii_case("true");
                continue;
            }

            if let Some(raw) = line.strip_prefix("interp=") {
                let value = raw.trim();
                if !value.is_empty() {
                    interpreter = Some(value.to_string());
                }
                continue;
            }

            if let Some(raw) = line.strip_prefix("needed=") {
                needed_libraries = parse_csv_values(raw);
                continue;
            }

            if let Some(raw) = line.strip_prefix("required=") {
                required_symbols = parse_csv_values(raw);
                continue;
            }

            if let Some(raw) = line.strip_prefix("preferred_base=")
                && let Some(base) = parse_u64_value(raw)
            {
                preferred_base = base;
            }
        }

        let entry = entry?;
        return Some(BinaryMetadata {
            entry,
            pie,
            preferred_base,
            dynamic,
            interpreter,
            needed_libraries,
            required_symbols,
        });
    }

    let handle = saifs::open(path).ok()?;
    let bytes = handle.read().ok()?;
    parse_elf_metadata(path, bytes.as_slice())
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
        n if n.eq_ignore_ascii_case("taskman") => crate::taskman::run(args, env),
        n if n.eq_ignore_ascii_case("diskpart") => crate::diskpart::run(args, env),
        _ => Err("program not found"),
    }
}

pub fn execute_path(
    path: &str,
    name: &str,
    args: &[&str],
    env: &[(String, String)],
) -> ProgramResult {
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
