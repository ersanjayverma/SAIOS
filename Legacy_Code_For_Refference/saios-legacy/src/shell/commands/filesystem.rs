use crate::{print, println};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub fn vfs_abs_pub(path: &str) -> String {
    vfs_abs(path)
}

pub fn read_bytes_for_module(path: &str) -> Result<alloc::vec::Vec<u8>, &'static str> {
    vfs_read(path)
}

pub fn write_file_for_module(path: &str, data: &[u8]) -> Result<(), &'static str> {
    vfs_write(path, data)
}

pub fn cat_read_env() -> Result<String, &'static str> {
    let data = vfs_read("/etc/env")?;
    Ok(core::str::from_utf8(&data).unwrap_or("").to_string())
}

pub fn read_env_bytes() -> alloc::vec::Vec<u8> {
    vfs_read("/etc/env").unwrap_or_default()
}

pub fn write_env_bytes(data: &[u8]) -> Result<(), &'static str> {
    vfs_write("/etc/env", data)
}

pub fn read_todo_bytes() -> alloc::vec::Vec<u8> {
    vfs_read("/home/todo.txt").unwrap_or_default()
}

pub fn write_todo_bytes(data: &[u8]) -> Result<(), &'static str> {
    vfs_write("/home/todo.txt", data)
}

fn vfs_abs(path: &str) -> String {
    let p = path.trim();
    if p.starts_with('/') {
        return crate::vfs::path::normalise(p);
    }
    let cwd = crate::shell::current_cwd();
    let joined = if cwd == "/" {
        format!("/{}", p)
    } else {
        format!("{}/{}", cwd, p)
    };
    crate::vfs::path::normalise(&joined)
}

fn vfs_read(path: &str) -> Result<alloc::vec::Vec<u8>, &'static str> {
    crate::vfs_contract::VfsContract::read_file(path).map_err(|_| "read failed")
}

fn vfs_write(path: &str, data: &[u8]) -> Result<(), &'static str> {
    if let Some(target) = symlink_target(path) {
        return vfs_write(&target, data);
    }

    if let Ok(inode) = crate::vfs_contract::VfsContract::resolve(path)
        && inode.ftype == crate::vfs::FileType::RegularFile
    {
        crate::vfs_contract::VfsContract::write_file(path, data, 0o644)
            .map_err(|_| "write failed")?;
        reload_config_if_needed(path);
        return Ok(());
    }

    if let Some(slash) = path.rfind('/') {
        let dir = if slash == 0 { "/" } else { &path[..slash] };
        crate::mkdir_p_pub(dir);
    }
    crate::vfs_contract::VfsContract::write_file(path, data, 0o644).map_err(|_| "write failed")?;
    reload_config_if_needed(path);
    Ok(())
}

fn reload_config_if_needed(path: &str) {
    if crate::config::is_reload_path(path) {
        crate::configuration_contract::ConfigurationContract::reload();
    }
}

fn symlink_target(path: &str) -> Option<String> {
    let (parent, name) = crate::vfs_contract::VfsContract::resolve_parent(path).ok()?;
    let inode = parent.ops.lookup(&name).ok()?;
    if inode.ftype != crate::vfs::FileType::SymLink {
        return None;
    }

    let target = inode.ops.readlink().ok()?;
    if target.starts_with('/') {
        Some(crate::vfs::path::normalise(&target))
    } else {
        let base = match path.rfind('/') {
            Some(0) | None => String::from("/"),
            Some(i) => path[..i].to_string(),
        };
        Some(crate::vfs::path::normalise(&format!("{}/{}", base, target)))
    }
}

fn vfs_unlink(path: &str) -> Result<(), &'static str> {
    crate::vfs_contract::VfsContract::unlink(path).map_err(|_| "unlink failed")
}

fn vfs_ls(path: &str) -> Result<alloc::vec::Vec<String>, &'static str> {
    let entries =
        crate::vfs_contract::VfsContract::read_dir(path).map_err(|_| "not a directory")?;
    Ok(entries.into_iter().map(|e| e.name).collect())
}

pub fn cd(args: &str) {
    let target = args.trim();
    let new_path = if target.is_empty() || target == "~" {
        String::from("/")
    } else if target.starts_with('/') {
        String::from(target)
    } else {
        let cwd = crate::shell::current_cwd();
        if cwd == "/" {
            format!("/{}", target)
        } else {
            format!("{}/{}", cwd.trim_end_matches('/'), target)
        }
    };

    let norm = crate::vfs::path::normalise(&new_path);

    match crate::vfs_contract::VfsContract::resolve(&norm) {
        Ok(inode) if inode.ftype == crate::vfs::FileType::Directory => {
            crate::shell::set_current_cwd(&norm);
        }
        Ok(_) => println!("cd: {}: Not a directory", norm),
        Err(_) => println!("cd: {}: No such file or directory", norm),
    }
}

pub fn pwd() {
    println!("{}", crate::shell::current_cwd());
}

pub fn ls(args: &str) {
    let mut show_all = false;
    let mut long_fmt = false;
    let mut human_size = false;
    let mut all_paths: Vec<String> = Vec::new();

    for token in args.split_whitespace() {
        if token.starts_with('-') {
            for ch in token.chars().skip(1) {
                match ch {
                    'a' | 'A' => show_all = true,
                    'l' => long_fmt = true,
                    'h' => human_size = true,
                    '1' => {}
                    _ => {}
                }
            }
        } else {
            all_paths.push(token.to_string());
        }
    }

    let raw_paths: Vec<String> = if all_paths.is_empty() {
        vec![crate::shell::current_cwd()]
    } else {
        all_paths
            .into_iter()
            .map(|p| {
                if p.starts_with('/') {
                    p
                } else {
                    let cwd = crate::shell::current_cwd();
                    if cwd == "/" {
                        format!("/{}", p)
                    } else {
                        format!("{}/{}", cwd, p)
                    }
                }
            })
            .collect()
    };
    let multi = raw_paths.len() > 1;
    for (i, this_path) in raw_paths.iter().enumerate() {
        if multi && i > 0 {
            println!();
        }
        if multi {
            println!("{}:", this_path);
        }
        let entries_result = vfs_ls(this_path);
        match entries_result {
            Ok(mut entries) => {
                entries.sort();
                if show_all {
                    entries.insert(0, String::from(".."));
                    entries.insert(0, String::from("."));
                } else {
                    entries.retain(|e| !e.starts_with('.'));
                }

                if entries.is_empty() {
                    println!("(empty)");
                    continue;
                }

                if long_fmt {
                    for name in &entries {
                        let full = if this_path == "/" {
                            format!("/{}", name)
                        } else {
                            format!("{}/{}", this_path.trim_end_matches('/'), name)
                        };
                        let (type_char, mode_str, size) =
                            crate::vfs_contract::VfsContract::resolve(&full)
                                .and_then(|i| i.ops.stat())
                                .map(|s| {
                                    let t = match s.st_mode >> 12 {
                                        0o10 => '-',
                                        0o04 => 'd',
                                        0o12 => 'l',
                                        0o02 => 'c',
                                        0o06 => 'b',
                                        _ => '?',
                                    };
                                    let m = s.st_mode & 0o777;
                                    let ms = format!(
                                        "{}{}{}{}{}{}{}{}{}",
                                        if m & 0o400 != 0 { 'r' } else { '-' },
                                        if m & 0o200 != 0 { 'w' } else { '-' },
                                        if m & 0o100 != 0 { 'x' } else { '-' },
                                        if m & 0o040 != 0 { 'r' } else { '-' },
                                        if m & 0o020 != 0 { 'w' } else { '-' },
                                        if m & 0o010 != 0 { 'x' } else { '-' },
                                        if m & 0o004 != 0 { 'r' } else { '-' },
                                        if m & 0o002 != 0 { 'w' } else { '-' },
                                        if m & 0o001 != 0 { 'x' } else { '-' },
                                    );
                                    (t, ms, s.st_size as u64)
                                })
                                .unwrap_or(('?', String::from("---------"), 0));

                        let size_str = if human_size {
                            if size >= 1024 * 1024 {
                                format!("{:>5}M", size / (1024 * 1024))
                            } else if size >= 1024 {
                                format!("{:>5}K", size / 1024)
                            } else {
                                format!("{:>6}", size)
                            }
                        } else {
                            format!("{:>8}", size)
                        };

                        println!(
                            "{}{} 1 root root {} {}",
                            type_char, mode_str, size_str, name
                        );
                    }
                    println!("total {}", entries.len());
                } else {
                    let col_w = entries.iter().map(|e| e.len()).max().unwrap_or(1) + 2;
                    let cols = (80 / col_w).max(1);
                    for (index, name) in entries.iter().enumerate() {
                        print!("{:<width$}", name, width = col_w);
                        if (index + 1) % cols == 0 {
                            println!();
                        }
                    }
                    if entries.len() % cols != 0 {
                        println!();
                    }
                }
            }
            Err(e) => println!("ls: {}: {}", this_path, e),
        }
    }
}

pub fn cat(args: &str) {
    let mut number_lines = false;
    let mut paths: Vec<&str> = Vec::new();
    for tok in args.split_whitespace() {
        if tok.starts_with('-') && tok.len() > 1 && paths.is_empty() {
            for c in tok[1..].chars() {
                if c == 'n' {
                    number_lines = true
                }
            }
        } else {
            paths.push(tok);
        }
    }
    if paths.is_empty() {
        if let Some(s) = crate::shell::take_stdin() {
            if number_lines {
                for (i, line) in s.lines().enumerate() {
                    println!("{:6}\t{}", i + 1, line);
                }
            } else {
                print!("{}", s);
                if !s.ends_with('\n') {
                    println!();
                }
            }
        } else {
            println!("usage: cat [-n] <path> [<path> ...]");
        }
        return;
    }
    for path in paths {
        let p = vfs_abs(path);
        match vfs_read(&p) {
            Ok(data) => {
                if let Ok(s) = core::str::from_utf8(&data) {
                    if number_lines {
                        for (i, line) in s.lines().enumerate() {
                            println!("{:6}\t{}", i + 1, line);
                        }
                    } else {
                        print!("{}", s);
                        if !s.ends_with('\n') {
                            println!();
                        }
                    }
                } else {
                    println!("[binary {} bytes]", data.len());
                }
            }
            Err(e) => println!("cat: {}: {}", p, e),
        }
    }
}

pub fn write_file(args: &str) {
    let mut p = args.splitn(2, ' ');
    let path = p.next().unwrap_or("");
    let text = p.next().unwrap_or("");
    if path.is_empty() {
        println!("usage: write <path> <content>");
        return;
    }
    let abs = vfs_abs(path);
    let mut d = String::from(text);
    d.push('\n');
    match vfs_write(&abs, d.as_bytes()) {
        Ok(()) => println!("wrote {} bytes -> {}", d.len(), abs),
        Err(e) => println!("write: {}", e),
    }
}

pub fn append_file(args: &str) {
    let mut p = args.splitn(2, ' ');
    let path = p.next().unwrap_or("");
    let text = p.next().unwrap_or("");
    if path.is_empty() {
        println!("usage: append <path> <content>");
        return;
    }
    let abs = vfs_abs(path);
    let mut d = String::from(text);
    d.push('\n');
    let existing = vfs_read(&abs).unwrap_or_default();
    let mut buf = existing;
    buf.extend_from_slice(d.as_bytes());
    match vfs_write(&abs, &buf) {
        Ok(()) => println!("appended {} bytes -> {}", d.len(), abs),
        Err(e) => println!("append: {}", e),
    }
}

pub fn cp(args: &str) {
    let mut p = args.splitn(2, ' ');
    let src = p.next().unwrap_or("").trim();
    let dst = p.next().unwrap_or("").trim();
    if src.is_empty() || dst.is_empty() {
        println!("usage: cp <src> <dst>");
        return;
    }
    let (asrc, adst) = (vfs_abs(src), vfs_abs(dst));
    match vfs_read(&asrc) {
        Ok(data) => match vfs_write(&adst, &data) {
            Ok(()) => println!("copied {} -> {} ({} bytes)", asrc, adst, data.len()),
            Err(e) => println!("cp: {}", e),
        },
        Err(e) => println!("cp: {}: {}", asrc, e),
    }
}

pub fn mv(args: &str) {
    let mut p = args.splitn(2, ' ');
    let src = p.next().unwrap_or("").trim();
    let dst = p.next().unwrap_or("").trim();
    if src.is_empty() || dst.is_empty() {
        println!("usage: mv <src> <dst>");
        return;
    }
    let (asrc, adst) = (vfs_abs(src), vfs_abs(dst));
    match vfs_read(&asrc) {
        Ok(data) => {
            if let Err(e) = vfs_write(&adst, &data) {
                println!("mv: {}", e);
                return;
            }
            let _ = vfs_unlink(&asrc);
            println!("moved {} -> {}", asrc, adst);
        }
        Err(e) => println!("mv: {}: {}", asrc, e),
    }
}

pub fn rm(args: &str) {
    let mut recursive = false;
    let mut force = false;
    let mut targets: Vec<&str> = Vec::new();
    for tok in args.split_whitespace() {
        if tok.starts_with('-') && tok.len() > 1 {
            for c in tok[1..].chars() {
                match c {
                    'r' | 'R' => recursive = true,
                    'f' => force = true,
                    _ => {}
                }
            }
        } else {
            targets.push(tok);
        }
    }
    if targets.is_empty() {
        println!("usage: rm [-rf] <path>...");
        return;
    }
    for t in targets {
        let abs = vfs_abs(t);
        match rm_path(&abs, recursive) {
            Ok(()) => {}
            Err(e) => {
                if !force {
                    println!("rm: {}: {}", abs, e);
                }
            }
        }
    }
}

fn rm_path(abs: &str, recursive: bool) -> Result<(), &'static str> {
    let inode =
        crate::vfs_contract::VfsContract::resolve(abs).map_err(|_| "no such file or directory")?;
    if inode.ftype == crate::vfs::FileType::Directory {
        if !recursive {
            return Err("is a directory (use -r)");
        }
        let entries =
            crate::vfs_contract::VfsContract::read_dir(abs).map_err(|_| "cannot read directory")?;
        for e in entries {
            if e.name == "." || e.name == ".." {
                continue;
            }
            let child = alloc::format!("{}/{}", abs.trim_end_matches('/'), e.name);
            if let Err(err) = rm_path(&child, true) {
                println!("rm: {}: {}", child, err);
                return Err("directory not empty");
            }
        }
        crate::vfs_contract::VfsContract::rmdir(abs).map_err(|_| "rmdir failed")
    } else {
        vfs_unlink(abs)
    }
}

pub fn mkdir(args: &str) {
    if args.is_empty() {
        println!("usage: mkdir <path>");
        return;
    }
    let abs = vfs_abs(args);
    crate::mkdir_p_pub(&abs);
    println!("created {}", abs);
}

pub fn find(args: &str) {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    if tokens.is_empty() {
        println!("usage: find <path> [-name <pattern>] [-type f|d]");
        println!("       find <path> <name>  (legacy positional)");
        return;
    }

    let path = tokens[0];
    let mut name_pattern: Option<&str> = None;
    let mut type_filter: Option<char> = None; // 'f' or 'd'

    // Parse options
    let mut i = 1;
    while i < tokens.len() {
        match tokens[i] {
            "-name" => {
                i += 1;
                if i < tokens.len() {
                    name_pattern = Some(tokens[i]);
                } else {
                    println!("find: -name requires an argument");
                    return;
                }
            }
            "-type" => {
                i += 1;
                if i < tokens.len() && (tokens[i] == "f" || tokens[i] == "d") {
                    type_filter = Some(tokens[i].as_bytes()[0] as char);
                } else {
                    println!("find: -type requires 'f' or 'd'");
                    return;
                }
            }
            _ => {
                // Legacy positional: find <path> <pattern>
                if name_pattern.is_none() && !tokens[i].starts_with('-') {
                    name_pattern = Some(tokens[i]);
                }
            }
        }
        i += 1;
    }

    // Default: if no pattern specified, match everything
    let pattern = name_pattern.unwrap_or("");

    let mut results: Vec<String> = Vec::new();
    find_recursive_filtered(path, pattern, type_filter, &mut results, 0);
    if results.is_empty() {
        if !pattern.is_empty() {
            println!("no matches for '{}' under {}", pattern, path);
        }
    } else {
        for r in &results {
            println!("{}", r);
        }
        println!("--- {} match(es)", results.len());
    }
}

fn find_recursive_filtered(
    path: &str,
    pattern: &str,
    type_filter: Option<char>,
    out: &mut Vec<String>,
    depth: u32,
) {
    const MAX_DEPTH: u32 = 32;
    if depth > MAX_DEPTH {
        return;
    }
    if let Ok(entries) = vfs_ls(path) {
        for e in entries {
            if e == "." || e == ".." {
                continue;
            }
            let full = if path == "/" {
                format!("/{}", e)
            } else {
                format!("{}/{}", path, e)
            };

            // Check type filter
            let is_dir = crate::vfs_contract::VfsContract::resolve(&full)
                .map(|inode| inode.ftype == crate::vfs::FileType::Directory)
                .unwrap_or(false);

            let type_ok = match type_filter {
                Some('d') => is_dir,
                Some('f') => !is_dir,
                _ => true,
            };

            // Check name pattern (substring match, or glob * prefix/suffix)
            let name_ok = if pattern.is_empty() {
                true
            } else if let Some(mid) = pattern.strip_prefix('*').and_then(|s| s.strip_suffix('*')) {
                e.contains(mid)
            } else if let Some(suffix) = pattern.strip_prefix('*') {
                e.ends_with(suffix)
            } else if let Some(prefix) = pattern.strip_suffix('*') {
                e.starts_with(prefix)
            } else {
                e.contains(pattern)
            };

            if type_ok && name_ok {
                out.push(full.clone());
            }

            if is_dir {
                find_recursive_filtered(&full, pattern, type_filter, out, depth + 1);
            }
        }
    }
}

pub fn grep(args: &str) {
    let mut ignore_case = false;
    let mut show_line_numbers = false;
    let mut invert = false;
    let mut positional: Vec<&str> = Vec::new();

    for tok in args.split_whitespace() {
        if tok.starts_with('-') && tok.len() > 1 && positional.is_empty() {
            for c in tok[1..].chars() {
                match c {
                    'i' => ignore_case = true,
                    'n' => show_line_numbers = true,
                    'v' => invert = true,
                    _ => {
                        println!("grep: unknown option '-{}'", c);
                        return;
                    }
                }
            }
        } else {
            positional.push(tok);
        }
    }

    let pattern = match positional.first() {
        Some(p) => *p,
        None => {
            println!("usage: grep [-inv] <pattern> [file]");
            return;
        }
    };

    let text: String = if positional.len() > 1 {
        let path = positional[1];
        match vfs_read(&vfs_abs(path)) {
            Ok(data) => String::from_utf8_lossy(&data).into_owned(),
            Err(e) => {
                println!("grep: {}: {}", path, e);
                return;
            }
        }
    } else {
        match crate::shell::take_stdin() {
            Some(s) => s,
            None => {
                println!("usage: grep [-inv] <pattern> <file>");
                return;
            }
        }
    };

    let pat_lower = if ignore_case {
        pattern.to_lowercase()
    } else {
        String::new()
    };

    let mut match_count = 0usize;
    for (idx, line) in text.lines().enumerate() {
        let matches = if ignore_case {
            line.to_lowercase().contains(&pat_lower)
        } else {
            line.contains(pattern)
        };
        if matches != invert {
            if show_line_numbers {
                println!("{}:{}", idx + 1, line);
            } else {
                println!("{}", line);
            }
            match_count += 1;
        }
    }
    if match_count == 0 && !invert {
        // silent — standard grep returns exit code 1 but prints nothing
    }
}

pub fn hexdump(args: &str) {
    if args.is_empty() {
        println!("usage: hexdump <file>");
        return;
    }
    match vfs_read(&vfs_abs(args)) {
        Ok(data) => {
            for (i, chunk) in data.chunks(16).enumerate() {
                print!("{:04x}  ", i * 16);
                for (j, b) in chunk.iter().enumerate() {
                    print!("{:02x} ", b);
                    if j == 7 {
                        print!(" ");
                    }
                }
                for j in chunk.len()..16 {
                    print!("   ");
                    if j == 7 {
                        print!(" ");
                    }
                }
                print!(" |");
                for b in chunk {
                    let c = if *b >= 0x20 && *b < 0x7F {
                        *b as char
                    } else {
                        '.'
                    };
                    print!("{}", c);
                }
                println!("|");
            }
            println!("--- {} bytes", data.len());
        }
        Err(e) => println!("hexdump: {}: {}", args, e),
    }
}

pub fn wc(args: &str) {
    let (text, label): (String, &str) = if args.is_empty() {
        match crate::shell::take_stdin() {
            Some(s) => (s, ""),
            None => {
                println!("usage: wc <file>");
                return;
            }
        }
    } else {
        match vfs_read(&vfs_abs(args)) {
            Ok(data) => (String::from_utf8_lossy(&data).into_owned(), args),
            Err(e) => {
                println!("wc: {}: {}", args, e);
                return;
            }
        }
    };
    let lines = text.lines().count();
    let words = text.split_whitespace().count();
    let bytes = text.len();
    println!(
        "{:6} lines  {:6} words  {:6} bytes  {}",
        lines, words, bytes, label
    );
}

pub fn df() {
    let (total, free, _) = crate::memory::frame_stats();
    println!("tmpfs  /    in-memory (lost on reboot)");
    println!(
        "RAM:   {} MiB free / {} MiB total",
        free * 4 / 1024,
        total * 4 / 1024
    );
    for (mp, fstype) in crate::vfs::list_mounts() {
        println!("  {:12} {}", fstype, mp);
    }
}

pub fn chmod(args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() != 2 {
        println!("usage: chmod <mode> <file>");
        println!("  numeric: chmod 755 file.txt");
        println!("  symbolic: chmod u+x file.txt");
        return;
    }

    let mode_str = parts[0];
    let path = parts[1];

    // Try numeric (octal) mode first
    if let Ok(m) = u32::from_str_radix(mode_str, 8) {
        match crate::vfs_contract::VfsContract::chmod(path, m) {
            Ok(()) => {}
            Err(e) => println!("chmod: cannot access '{}': {}", path, e.to_errno()),
        }
        return;
    }

    // Symbolic mode: [ugoa]*[+-=][rwx]+
    let abs = vfs_abs(path);
    let current_mode = match crate::vfs_contract::VfsContract::resolve(&abs) {
        Ok(inode) => match inode.ops.stat() {
            Ok(st) => st.st_mode,
            Err(e) => {
                println!("chmod: cannot stat '{}': {}", path, e.to_errno());
                return;
            }
        },
        Err(e) => {
            println!("chmod: cannot access '{}': {}", path, e.to_errno());
            return;
        }
    };

    match parse_symbolic_mode(mode_str, current_mode) {
        Some(new_mode) => match crate::vfs_contract::VfsContract::chmod(&abs, new_mode) {
            Ok(()) => {}
            Err(e) => println!("chmod: cannot access '{}': {}", path, e.to_errno()),
        },
        None => {
            println!("chmod: invalid mode '{}'", mode_str);
        }
    }
}

fn parse_symbolic_mode(spec: &str, current: u32) -> Option<u32> {
    let mut mode = current;
    for clause in spec.split(',') {
        // Parse who: u, g, o, a (default: a)
        let mut who_u = false;
        let mut who_g = false;
        let mut who_o = false;
        let mut i = 0;
        let bytes = clause.as_bytes();

        while i < bytes.len() && matches!(bytes[i], b'u' | b'g' | b'o' | b'a') {
            match bytes[i] {
                b'u' => who_u = true,
                b'g' => who_g = true,
                b'o' => who_o = true,
                b'a' => {
                    who_u = true;
                    who_g = true;
                    who_o = true;
                }
                _ => {}
            }
            i += 1;
        }
        // If no who specified, default to 'a'
        if !who_u && !who_g && !who_o {
            who_u = true;
            who_g = true;
            who_o = true;
        }

        if i >= bytes.len() {
            return None;
        }
        let op = bytes[i] as char;
        if op != '+' && op != '-' && op != '=' {
            return None;
        }
        i += 1;

        // Parse perms: r, w, x
        let mut perm_bits: u32 = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'r' => perm_bits |= 4,
                b'w' => perm_bits |= 2,
                b'x' => perm_bits |= 1,
                _ => return None,
            }
            i += 1;
        }

        // Apply to selected who fields
        let mut mask: u32 = 0;
        let mut bits: u32 = 0;
        if who_u {
            mask |= 0o700;
            bits |= perm_bits << 6;
        }
        if who_g {
            mask |= 0o070;
            bits |= perm_bits << 3;
        }
        if who_o {
            mask |= 0o007;
            bits |= perm_bits;
        }

        match op {
            '+' => mode |= bits,
            '-' => mode &= !bits,
            '=' => mode = (mode & !mask) | bits,
            _ => return None,
        }
    }
    Some(mode)
}

pub fn chown(args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() != 2 {
        println!("usage: chown <owner>[:<group>] <file>");
        println!("  e.g. chown alice file.txt");
        println!("       chown alice:users file.txt");
        return;
    }

    let owner_spec = parts[0];
    let path = parts[1];

    let (owner_name, group_name) = if let Some(colon_pos) = owner_spec.find(':') {
        let owner = &owner_spec[..colon_pos];
        let group = &owner_spec[colon_pos + 1..];
        (owner, if group.is_empty() { None } else { Some(group) })
    } else {
        (owner_spec, None)
    };

    let uid = match crate::user::get_user_by_name(owner_name) {
        Some(user) => user.uid,
        None => {
            println!("chown: invalid user '{}'", owner_name);
            return;
        }
    };

    let gid = if let Some(group_name) = group_name {
        match crate::user::get_group_by_name(group_name) {
            Some(group) => group.gid,
            None => {
                println!("chown: invalid group '{}'", group_name);
                return;
            }
        }
    } else {
        match crate::user::get_user_by_name(owner_name) {
            Some(user) => user.gid,
            None => {
                println!("chown: invalid user '{}'", owner_name);
                return;
            }
        }
    };

    match crate::vfs_contract::VfsContract::chown(path, uid, gid) {
        Ok(()) => {}
        Err(e) => println!("chown: cannot access '{}': {}", path, e.to_errno()),
    }
}

pub fn help_filesystem() {
    println!("  Files:");
    println!("    ls [-la] [path]        list directory (-l long, -a all)");
    println!("    cat [-n] <file>        print file (-n line numbers)");
    println!("    write <f> <text>       write text to file");
    println!("    append <f> <text>      append text to file");
    println!("    cp <src> <dst>         copy file");
    println!("    mv <src> <dst>         rename/move file");
    println!("    rm [-rf] <path>        delete file/dir (-r recursive, -f force)");
    println!("    mkdir <path>           create directory (recursive)");
    println!("    find <path> [-name p] [-type f|d]  search files");
    println!("    grep [-inv] <pat> <f>  search text (-i ignore case, -n numbers, -v invert)");
    println!("    chmod <mode> <file>    change permissions (octal or symbolic u+x)");
    println!("    hexdump <file>         hex + ASCII dump");
    println!("    wc <file>              line/word/byte count");
    println!("    df                     filesystem usage");
}
