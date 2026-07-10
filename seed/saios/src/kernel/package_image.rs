use crate::kernel::constants::{
    ET_EXEC, EM_X86_64, EV_CURRENT, PT_LOAD, PT_DYNAMIC, PT_INTERP,
    USER_ELF_LOAD_BASE,
};
use core::sync::atomic::{AtomicBool, Ordering};

use alloc::vec;
use alloc::vec::Vec;

use crate::vfs;

const PROFILE: &str = "saios-base";
const MANIFEST_PATH: &str = "/boot/package.manifest";

const ROOT_DIRS: &[&str] = &[
    "/boot", "/bin", "/etc", "/home", "/proc", "/dev", "/tmp", "/usr", "/lib", "/system",
];

const SHARED_LIB_ENTRIES: &[&str] = &[
    "ld-saios.so",
    "libc.so",
    "libm.so",
    "libshell.so",
    "libui.so",
];

const BIN_ENTRIES: &[&str] = &["snsh", "busybox"];

const EMBEDDED_BUSYBOX: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../busybox"));

static MOUNTED: AtomicBool = AtomicBool::new(false);

const ELF_PT_LOAD: u32 = PT_LOAD;
const ELF_PT_DYNAMIC: u32 = PT_DYNAMIC;
const ELF_PT_INTERP: u32 = PT_INTERP;

#[derive(Copy, Clone, Debug)]
pub struct PackageImageStatus {
    pub mounted: bool,
    pub profile: &'static str,
    pub manifest: &'static str,
    pub roots: usize,
    pub bins: usize,
}

fn ensure_dir(path: &str) -> Result<(), &'static str> {
    match vfs::mkdir(path) {
        Ok(()) => Ok(()),
        Err("already exists") => Ok(()),
        Err(e) => Err(e),
    }
}

fn ensure_file(path: &str) -> Result<(), &'static str> {
    match vfs::touch(path) {
        Ok(()) => Ok(()),
        Err("already exists") => Ok(()),
        Err(e) => Err(e),
    }
}

fn write_manifest() -> Result<(), &'static str> {
    let mut text = alloc::string::String::new();
    text.push_str("profile=");
    text.push_str(PROFILE);
    text.push('\n');

    for d in ROOT_DIRS {
        text.push_str("dir=");
        text.push_str(d);
        text.push('\n');
    }

    for b in BIN_ENTRIES {
        text.push_str("bin=/bin/");
        text.push_str(b);
        text.push('\n');
    }

    for so in SHARED_LIB_ENTRIES {
        text.push_str("lib=/lib/");
        text.push_str(so);
        text.push('\n');
    }

    ensure_file(MANIFEST_PATH)?;
    vfs::write_path(MANIFEST_PATH, text.as_bytes())
}

fn write_binary(path: &str, entry: &str) -> Result<(), &'static str> {
    const ELF_HEADER_SIZE: usize = 64;
    const PROGRAM_HEADER_SIZE: usize = 56;
    const CODE_OFFSET: usize = 0x80;
    const IMAGE_SIZE: usize = 0x4000;
    const DATA_PROMPT_OFFSET: usize = 0x1800;
    const DATA_HELP_OFFSET: usize = 0x1820;
    const DATA_UNKNOWN_OFFSET: usize = 0x18c0;
    const DATA_NEWLINE_OFFSET: usize = 0x1900;
    const DATA_DOT_OFFSET: usize = 0x1910;
    const DATA_LINE_OFFSET: usize = 0x2000;
    const DATA_DIRENT_OFFSET: usize = 0x2200;
    const DATA_FILE_OFFSET: usize = 0x2a00;
    const DATA_ARGV_OFFSET: usize = 0x3000;

    let mut elf = vec![0u8; IMAGE_SIZE];

    // e_ident
    elf[0] = 0x7F;
    elf[1] = b'E';
    elf[2] = b'L';
    elf[3] = b'F';
    elf[4] = 2; // ELFCLASS64
    elf[5] = 1; // little-endian
    elf[6] = 1; // current version

    fn put16(buf: &mut [u8], off: usize, value: u16) {
        let b = value.to_le_bytes();
        buf[off..off + 2].copy_from_slice(&b);
    }
    fn put32(buf: &mut [u8], off: usize, value: u32) {
        let b = value.to_le_bytes();
        buf[off..off + 4].copy_from_slice(&b);
    }
    fn put64(buf: &mut [u8], off: usize, value: u64) {
        let b = value.to_le_bytes();
        buf[off..off + 8].copy_from_slice(&b);
    }

    // ELF header fields.
    put16(&mut elf, 16, ET_EXEC);       // e_type
    put16(&mut elf, 18, EM_X86_64);     // e_machine
    put32(&mut elf, 20, EV_CURRENT as u32); // e_version
    put64(&mut elf, 24, USER_ELF_LOAD_BASE + CODE_OFFSET as u64); // e_entry
    put64(&mut elf, 32, ELF_HEADER_SIZE as u64); // e_phoff
    put64(&mut elf, 40, 0); // e_shoff
    put32(&mut elf, 48, 0); // e_flags
    put16(&mut elf, 52, ELF_HEADER_SIZE as u16); // e_ehsize
    put16(&mut elf, 54, PROGRAM_HEADER_SIZE as u16); // e_phentsize
    put16(&mut elf, 56, 1); // e_phnum
    put16(&mut elf, 58, 0); // e_shentsize
    put16(&mut elf, 60, 0); // e_shnum
    put16(&mut elf, 62, 0); // e_shstrndx

    let ph = ELF_HEADER_SIZE;
    put32(&mut elf, ph, PT_LOAD);       // p_type
    let segment_flags = if entry.eq_ignore_ascii_case("snsh") { 0x7 } else { 0x5 };
    put32(&mut elf, ph + 4, segment_flags); // PF_R | PF_W | PF_X for snsh buffer, else PF_R | PF_X
    put64(&mut elf, ph + 8, 0);         // p_offset
    put64(&mut elf, ph + 16, USER_ELF_LOAD_BASE);  // p_vaddr
    put64(&mut elf, ph + 24, USER_ELF_LOAD_BASE);  // p_paddr
    put64(&mut elf, ph + 32, IMAGE_SIZE as u64); // p_filesz
    put64(&mut elf, ph + 40, IMAGE_SIZE as u64); // p_memsz
    put64(&mut elf, ph + 48, 0x1000); // p_align

    let code = if entry.eq_ignore_ascii_case("snsh") {
        let prompt = b"snsh> ";
        let help = b"commands: help pwd cwd cd ls cat uname exit\n";
        let unknown = b"snsh: unknown command\n";
        let newline = b"\n";
        let dot = b".\0";
        let base = USER_ELF_LOAD_BASE;
        let prompt_addr = base + DATA_PROMPT_OFFSET as u64;
        let help_addr = base + DATA_HELP_OFFSET as u64;
        let unknown_addr = base + DATA_UNKNOWN_OFFSET as u64;
        let newline_addr = base + DATA_NEWLINE_OFFSET as u64;
        let dot_addr = base + DATA_DOT_OFFSET as u64;
        let line_addr = base + DATA_LINE_OFFSET as u64;
        let dirent_addr = base + DATA_DIRENT_OFFSET as u64;
        let file_addr = base + DATA_FILE_OFFSET as u64;
        let argv_addr = base + DATA_ARGV_OFFSET as u64;

        elf[DATA_PROMPT_OFFSET..DATA_PROMPT_OFFSET + prompt.len()].copy_from_slice(prompt);
        elf[DATA_HELP_OFFSET..DATA_HELP_OFFSET + help.len()].copy_from_slice(help);
        elf[DATA_UNKNOWN_OFFSET..DATA_UNKNOWN_OFFSET + unknown.len()].copy_from_slice(unknown);
        elf[DATA_NEWLINE_OFFSET..DATA_NEWLINE_OFFSET + newline.len()].copy_from_slice(newline);
        elf[DATA_DOT_OFFSET..DATA_DOT_OFFSET + dot.len()].copy_from_slice(dot);

        build_snsh_code(SnshImageAddrs {
            prompt: prompt_addr,
            prompt_len: prompt.len() as u32,
            help: help_addr,
            help_len: help.len() as u32,
            unknown: unknown_addr,
            unknown_len: unknown.len() as u32,
            newline: newline_addr,
            dot: dot_addr,
            line: line_addr,
            dirent: dirent_addr,
            file: file_addr,
            argv: argv_addr,
        })
    } else {
        // Minimal x86_64 Linux ABI stub:
        //   mov eax, 60   ; __NR_exit
        //   xor edi, edi  ; status = 0
        //   syscall
        //   ud2           ; should not execute if exit succeeds; traps if it does
        vec![
            0xB8, 0x3C, 0x00, 0x00, 0x00,
            0x31, 0xFF,
            0x0F, 0x05,
            0x0F, 0x0B,
        ]
    };
    if CODE_OFFSET + code.len() > DATA_PROMPT_OFFSET {
        return Err("package: generated binary code too large");
    }
    elf[CODE_OFFSET..CODE_OFFSET + code.len()].copy_from_slice(code.as_slice());

    vfs::write_path(path, elf.as_slice())
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn patch_rel32(code: &mut [u8], rel_offset: usize, from_next: usize, target: usize) {
    let rel = (target as isize).saturating_sub(from_next as isize) as i32;
    code[rel_offset..rel_offset + 4].copy_from_slice(&rel.to_le_bytes());
}

fn emit_jmp(code: &mut Vec<u8>) -> usize {
    code.push(0xE9);
    let rel = code.len();
    push_u32(code, 0);
    rel
}

fn emit_jcc(code: &mut Vec<u8>, op: u8) -> usize {
    code.extend_from_slice(&[0x0F, op]);
    let rel = code.len();
    push_u32(code, 0);
    rel
}

fn emit_call(code: &mut Vec<u8>) -> usize {
    code.push(0xE8);
    let rel = code.len();
    push_u32(code, 0);
    rel
}

fn emit_write(code: &mut Vec<u8>, fd: u32, buf: u64, len: u32) {
    code.push(0xB8); // mov eax, __NR_write
    push_u32(code, 1);
    code.push(0xBF); // mov edi, fd
    push_u32(code, fd);
    code.extend_from_slice(&[0x48, 0xBE]); // mov rsi, buf
    push_u64(code, buf);
    code.push(0xBA); // mov edx, len
    push_u32(code, len);
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
}

#[derive(Copy, Clone)]
struct SnshImageAddrs {
    prompt: u64,
    prompt_len: u32,
    help: u64,
    help_len: u32,
    unknown: u64,
    unknown_len: u32,
    newline: u64,
    dot: u64,
    line: u64,
    dirent: u64,
    file: u64,
    argv: u64,
}

fn emit_exit(code: &mut Vec<u8>) {
    code.push(0xB8); // mov eax, __NR_exit
    push_u32(code, 60);
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    code.extend_from_slice(&[0x0F, 0x0B]); // ud2
}

fn emit_open_path(code: &mut Vec<u8>, path: u64) {
    code.push(0xB8); // mov eax, __NR_open
    push_u32(code, 2);
    code.extend_from_slice(&[0x48, 0xBF]); // mov rdi, path
    push_u64(code, path);
    code.extend_from_slice(&[0x31, 0xF6]); // xor esi, esi
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
}

fn emit_open_rdi(code: &mut Vec<u8>) {
    code.push(0xB8); // mov eax, __NR_open
    push_u32(code, 2);
    code.extend_from_slice(&[0x31, 0xF6]); // xor esi, esi
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
}

fn emit_close_r14(code: &mut Vec<u8>) {
    code.push(0xB8); // mov eax, __NR_close
    push_u32(code, 3);
    code.extend_from_slice(&[0x44, 0x89, 0xF7]); // mov edi, r14d
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
}

fn build_snsh_code(a: SnshImageAddrs) -> Vec<u8> {
    let mut code = Vec::new();
    let loop_start = code.len();

    emit_write(&mut code, 1, a.prompt, a.prompt_len);

    code.extend_from_slice(&[0x31, 0xC0]); // xor eax, eax (__NR_read)
    code.extend_from_slice(&[0x31, 0xFF]); // xor edi, edi (stdin)
    code.extend_from_slice(&[0x48, 0xBE]); // mov rsi, buffer
    push_u64(&mut code, a.line);
    code.push(0xBA); // mov edx, 128
    push_u32(&mut code, 128);
    code.extend_from_slice(&[0x0F, 0x05]); // syscall

    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    let jle_exit = emit_jcc(&mut code, 0x8E);

    code.extend_from_slice(&[0x48, 0xBB]); // mov rbx, line
    push_u64(&mut code, a.line);
    code.extend_from_slice(&[0xC6, 0x04, 0x03, 0x00]); // mov byte ptr [rbx+rax], 0

    code.extend_from_slice(&[0x48, 0x89, 0xC1]); // mov rcx, rax
    let sanitize_loop = code.len();
    code.extend_from_slice(&[0x48, 0x85, 0xC9]); // test rcx, rcx
    let sanitize_done = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x80, 0x3B, 0x0A]); // cmp byte ptr [rbx], '\n'
    let sanitize_zero_nl = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x80, 0x3B, 0x0D]); // cmp byte ptr [rbx], '\r'
    let sanitize_zero_cr = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x48, 0xFF, 0xC3]); // inc rbx
    code.extend_from_slice(&[0x48, 0xFF, 0xC9]); // dec rcx
    let sanitize_back = emit_jmp(&mut code);
    let sanitize_zero = code.len();
    code.extend_from_slice(&[0xC6, 0x03, 0x00]); // mov byte ptr [rbx], 0
    let sanitize_end = code.len();

    code.extend_from_slice(&[0x48, 0xBB]); // mov rbx, line
    push_u64(&mut code, a.line);

    let trim_loop = code.len();
    code.extend_from_slice(&[0x80, 0x3B, 0x20]); // cmp byte ptr [rbx], ' '
    let trim_done = emit_jcc(&mut code, 0x85);
    code.extend_from_slice(&[0x48, 0xFF, 0xC3]); // inc rbx
    let trim_back = emit_jmp(&mut code);
    let trim_after = code.len();

    code.extend_from_slice(&[0x80, 0x3B, 0x00]); // empty line
    let empty_line = emit_jcc(&mut code, 0x84);

    code.extend_from_slice(&[0x49, 0x89, 0xD8]); // mov r8, rbx (command)
    code.extend_from_slice(&[0x4D, 0x31, 0xC9]); // xor r9, r9 (arg)

    let split_loop = code.len();
    code.extend_from_slice(&[0x80, 0x3B, 0x00]); // cmp byte ptr [rbx], 0
    let split_done_zero = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x80, 0x3B, 0x20]); // cmp byte ptr [rbx], ' '
    let split_space = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x48, 0xFF, 0xC3]); // inc rbx
    let split_back = emit_jmp(&mut code);

    let split_space_start = code.len();
    code.extend_from_slice(&[0xC6, 0x03, 0x00]); // mov byte ptr [rbx], 0
    code.extend_from_slice(&[0x48, 0xFF, 0xC3]); // inc rbx
    let arg_trim_loop = code.len();
    code.extend_from_slice(&[0x80, 0x3B, 0x20]); // cmp byte ptr [rbx], ' '
    let arg_trim_done = emit_jcc(&mut code, 0x85);
    code.extend_from_slice(&[0x48, 0xFF, 0xC3]); // inc rbx
    let arg_trim_back = emit_jmp(&mut code);
    let arg_trim_done_start = code.len();
    code.extend_from_slice(&[0x80, 0x3B, 0x00]); // cmp byte ptr [rbx], 0
    let split_done_no_arg = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x49, 0x89, 0xD9]); // mov r9, rbx
    let split_done_with_arg = emit_jmp(&mut code);

    let split_done = code.len();
    code.extend_from_slice(&[0x4C, 0x89, 0xC3]); // mov rbx, r8

    code.extend_from_slice(&[0x81, 0x3B]); // cmp dword ptr [rbx], 'exit'
    push_u32(&mut code, 0x7469_7865);
    let is_exit = emit_jcc(&mut code, 0x84);

    code.extend_from_slice(&[0x81, 0x3B]); // cmp dword ptr [rbx], 'help'
    push_u32(&mut code, 0x706c_6568);
    let is_help = emit_jcc(&mut code, 0x84);

    code.extend_from_slice(&[0x66, 0x81, 0x3B]); // cmp word ptr [rbx], 'cd'
    push_u16(&mut code, 0x6463);
    let is_cd = emit_jcc(&mut code, 0x84);

    code.extend_from_slice(&[0x66, 0x81, 0x3B]); // cmp word ptr [rbx], 'ls'
    push_u16(&mut code, 0x736c);
    let is_ls = emit_jcc(&mut code, 0x84);

    code.extend_from_slice(&[0x66, 0x81, 0x3B]); // cmp word ptr [rbx], 'pw'
    push_u16(&mut code, 0x7770);
    let not_pwd = emit_jcc(&mut code, 0x85);
    code.extend_from_slice(&[0x80, 0x7B, 0x02, 0x64]); // cmp byte ptr [rbx+2], 'd'
    let is_pwd = emit_jcc(&mut code, 0x84);

    let after_pwd_probe = code.len();
    code.extend_from_slice(&[0x66, 0x81, 0x3B]); // cmp word ptr [rbx], 'cw'
    push_u16(&mut code, 0x7763);
    let not_cwd = emit_jcc(&mut code, 0x85);
    code.extend_from_slice(&[0x80, 0x7B, 0x02, 0x64]); // cmp byte ptr [rbx+2], 'd'
    let is_cwd = emit_jcc(&mut code, 0x84);

    let after_cwd_probe = code.len();
    code.extend_from_slice(&[0x81, 0x3B]); // cmp dword ptr [rbx], 'unam'
    push_u32(&mut code, 0x6d61_6e75);
    let not_uname = emit_jcc(&mut code, 0x85);
    code.extend_from_slice(&[0x80, 0x7B, 0x04, 0x65]); // cmp byte ptr [rbx+4], 'e'
    let is_uname = emit_jcc(&mut code, 0x84);

    let after_uname_probe = code.len();
    code.extend_from_slice(&[0x81, 0x3B]); // cmp dword ptr [rbx], 'cat\0/space'
    push_u32(&mut code, 0x0074_6163);
    let is_cat_exact = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x80, 0x7B, 0x03, 0x20]); // cmp byte ptr [rbx+3], ' '
    let is_cat = emit_jcc(&mut code, 0x84);

    code.extend_from_slice(&[0x48, 0xB8]); // mov rax, argv
    push_u64(&mut code, a.argv);
    code.extend_from_slice(&[0x4C, 0x89, 0x00]); // mov [rax], r8
    code.extend_from_slice(&[0x4D, 0x85, 0xC9]); // test r9, r9
    let external_no_arg = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x4C, 0x89, 0x48, 0x08]); // mov [rax+8], r9
    let external_argv_tail = emit_jmp(&mut code);
    let external_no_arg_start = code.len();
    code.extend_from_slice(&[0x48, 0xC7, 0x40, 0x08]); // mov qword ptr [rax+8], 0
    push_u32(&mut code, 0);
    let external_tail = code.len();
    code.extend_from_slice(&[0x48, 0xC7, 0x40, 0x10]); // mov qword ptr [rax+16], 0
    push_u32(&mut code, 0);
    code.push(0xB8); // mov eax, __NR_execve
    push_u32(&mut code, 59);
    code.extend_from_slice(&[0x4C, 0x89, 0xC7]); // mov rdi, r8
    code.extend_from_slice(&[0x48, 0xBE]); // mov rsi, argv
    push_u64(&mut code, a.argv);
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx
    code.extend_from_slice(&[0x0F, 0x05]); // syscall
    emit_write(&mut code, 1, a.unknown, a.unknown_len);
    let external_done = emit_jmp(&mut code);

    let help_start = code.len();
    emit_write(&mut code, 1, a.help, a.help_len);
    let help_done = emit_jmp(&mut code);

    let pwd_start = code.len();
    code.push(0xB8); // mov eax, __NR_getcwd
    push_u32(&mut code, 79);
    code.extend_from_slice(&[0x48, 0xBF]); // mov rdi, file buffer
    push_u64(&mut code, a.file);
    code.push(0xBE); // mov esi, 512
    push_u32(&mut code, 512);
    code.extend_from_slice(&[0x0F, 0x05]);
    code.extend_from_slice(&[0x48, 0xBE]); // mov rsi, file buffer
    push_u64(&mut code, a.file);
    let pwd_print_call = emit_call(&mut code);
    emit_write(&mut code, 1, a.newline, 1);
    let pwd_done = emit_jmp(&mut code);

    let uname_start = code.len();
    code.push(0xB8); // mov eax, __NR_uname
    push_u32(&mut code, 63);
    code.extend_from_slice(&[0x48, 0xBF]); // mov rdi, file buffer
    push_u64(&mut code, a.file);
    code.extend_from_slice(&[0x0F, 0x05]);
    code.extend_from_slice(&[0x48, 0xBE]); // mov rsi, file buffer (sysname)
    push_u64(&mut code, a.file);
    let uname_print_call = emit_call(&mut code);
    emit_write(&mut code, 1, a.newline, 1);
    let uname_done = emit_jmp(&mut code);

    let cd_start = code.len();
    code.extend_from_slice(&[0x4D, 0x85, 0xC9]); // test r9, r9
    let cd_no_arg = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x4C, 0x89, 0xCF]); // mov rdi, r9
    code.push(0xB8); // mov eax, __NR_chdir
    push_u32(&mut code, 80);
    code.extend_from_slice(&[0x0F, 0x05]);
    let cd_done = emit_jmp(&mut code);

    let ls_start = code.len();
    emit_open_path(&mut code, a.dot);
    code.extend_from_slice(&[0x49, 0x89, 0xC6]); // mov r14, rax
    code.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
    let ls_done_if_bad_open = emit_jcc(&mut code, 0x8C); // jl loop
    code.push(0xB8); // mov eax, __NR_getdents64
    push_u32(&mut code, 78);
    code.extend_from_slice(&[0x44, 0x89, 0xF7]); // mov edi, r14d
    code.extend_from_slice(&[0x48, 0xBE]); // mov rsi, dirent
    push_u64(&mut code, a.dirent);
    code.push(0xBA); // mov edx, 2048
    push_u32(&mut code, 2048);
    code.extend_from_slice(&[0x0F, 0x05]);
    code.extend_from_slice(&[0x49, 0x89, 0xC7]); // mov r15, rax
    emit_close_r14(&mut code);
    code.extend_from_slice(&[0x4D, 0x85, 0xFF]); // test r15, r15
    let ls_done_if_empty = emit_jcc(&mut code, 0x8E); // jle loop
    code.extend_from_slice(&[0x49, 0xBC]); // mov r12, dirent
    push_u64(&mut code, a.dirent);
    code.extend_from_slice(&[0x4D, 0x89, 0xE5]); // mov r13, r12
    code.extend_from_slice(&[0x4D, 0x01, 0xFD]); // add r13, r15
    let ls_loop = code.len();
    code.extend_from_slice(&[0x4D, 0x39, 0xEC]); // cmp r12, r13
    let ls_done = emit_jcc(&mut code, 0x83); // jae
    code.extend_from_slice(&[0x49, 0x8D, 0x74, 0x24, 0x13]); // lea rsi, [r12+19]
    let ls_print_call = emit_call(&mut code);
    emit_write(&mut code, 1, a.newline, 1);
    code.extend_from_slice(&[0x45, 0x0F, 0xB7, 0x74, 0x24, 0x10]); // movzx r14d, word [r12+16]
    code.extend_from_slice(&[0x4D, 0x01, 0xF4]); // add r12, r14
    let ls_back = emit_jmp(&mut code);

    let cat_start = code.len();
    code.extend_from_slice(&[0x4D, 0x85, 0xC9]); // test r9, r9
    let cat_no_arg = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x4C, 0x89, 0xCF]); // mov rdi, r9
    emit_open_rdi(&mut code);
    code.extend_from_slice(&[0x49, 0x89, 0xC6]); // mov r14, rax
    code.extend_from_slice(&[0x48, 0x85, 0xC0]);
    let cat_done_if_bad_open = emit_jcc(&mut code, 0x8C);
    code.extend_from_slice(&[0x31, 0xC0]); // xor eax, eax (__NR_read)
    code.extend_from_slice(&[0x44, 0x89, 0xF7]); // mov edi, r14d
    code.extend_from_slice(&[0x48, 0xBE]); // mov rsi, file
    push_u64(&mut code, a.file);
    code.push(0xBA); // mov edx, 1024
    push_u32(&mut code, 1024);
    code.extend_from_slice(&[0x0F, 0x05]);
    code.extend_from_slice(&[0x49, 0x89, 0xC7]); // mov r15, rax
    emit_close_r14(&mut code);
    code.extend_from_slice(&[0x4D, 0x85, 0xFF]);
    let cat_done_if_empty = emit_jcc(&mut code, 0x8E);
    code.push(0xB8); // mov eax, __NR_write
    push_u32(&mut code, 1);
    code.push(0xBF); // mov edi, 1
    push_u32(&mut code, 1);
    code.extend_from_slice(&[0x48, 0xBE]); // mov rsi, file
    push_u64(&mut code, a.file);
    code.extend_from_slice(&[0x44, 0x89, 0xFA]); // mov edx, r15d
    code.extend_from_slice(&[0x0F, 0x05]);
    emit_write(&mut code, 1, a.newline, 1);
    let cat_done = emit_jmp(&mut code);

    let exit_start = code.len();
    emit_exit(&mut code);

    let print_cstr_start = code.len();
    code.extend_from_slice(&[0x48, 0x89, 0xF3]); // mov rbx, rsi
    code.extend_from_slice(&[0x31, 0xD2]); // xor edx, edx
    let strlen_loop = code.len();
    code.extend_from_slice(&[0x80, 0x3C, 0x13, 0x00]); // cmp byte ptr [rbx+rdx], 0
    let strlen_done = emit_jcc(&mut code, 0x84);
    code.extend_from_slice(&[0x48, 0xFF, 0xC2]); // inc rdx
    let strlen_back = emit_jmp(&mut code);
    let strlen_end = code.len();
    code.push(0xB8); // mov eax, __NR_write
    push_u32(&mut code, 1);
    code.push(0xBF); // mov edi, 1
    push_u32(&mut code, 1);
    code.extend_from_slice(&[0x0F, 0x05]);
    code.push(0xC3); // ret

    patch_rel32(code.as_mut_slice(), jle_exit, jle_exit + 4, exit_start);
    patch_rel32(code.as_mut_slice(), sanitize_done, sanitize_done + 4, sanitize_end);
    patch_rel32(code.as_mut_slice(), sanitize_zero_nl, sanitize_zero_nl + 4, sanitize_zero);
    patch_rel32(code.as_mut_slice(), sanitize_zero_cr, sanitize_zero_cr + 4, sanitize_zero);
    patch_rel32(code.as_mut_slice(), sanitize_back, sanitize_back + 4, sanitize_loop);
    patch_rel32(code.as_mut_slice(), trim_done, trim_done + 4, trim_after);
    patch_rel32(code.as_mut_slice(), trim_back, trim_back + 4, trim_loop);
    patch_rel32(code.as_mut_slice(), empty_line, empty_line + 4, loop_start);
    patch_rel32(code.as_mut_slice(), split_done_zero, split_done_zero + 4, split_done);
    patch_rel32(code.as_mut_slice(), split_space, split_space + 4, split_space_start);
    patch_rel32(code.as_mut_slice(), split_back, split_back + 4, split_loop);
    patch_rel32(code.as_mut_slice(), arg_trim_done, arg_trim_done + 4, arg_trim_done_start);
    patch_rel32(code.as_mut_slice(), arg_trim_back, arg_trim_back + 4, arg_trim_loop);
    patch_rel32(code.as_mut_slice(), split_done_no_arg, split_done_no_arg + 4, split_done);
    patch_rel32(code.as_mut_slice(), split_done_with_arg, split_done_with_arg + 4, split_done);
    patch_rel32(code.as_mut_slice(), is_exit, is_exit + 4, exit_start);
    patch_rel32(code.as_mut_slice(), is_help, is_help + 4, help_start);
    patch_rel32(code.as_mut_slice(), is_cd, is_cd + 4, cd_start);
    patch_rel32(code.as_mut_slice(), is_ls, is_ls + 4, ls_start);
    patch_rel32(code.as_mut_slice(), not_pwd, not_pwd + 4, after_pwd_probe);
    patch_rel32(code.as_mut_slice(), is_pwd, is_pwd + 4, pwd_start);
    patch_rel32(code.as_mut_slice(), not_cwd, not_cwd + 4, after_cwd_probe);
    patch_rel32(code.as_mut_slice(), is_cwd, is_cwd + 4, pwd_start);
    patch_rel32(code.as_mut_slice(), not_uname, not_uname + 4, after_uname_probe);
    patch_rel32(code.as_mut_slice(), is_uname, is_uname + 4, uname_start);
    patch_rel32(code.as_mut_slice(), is_cat_exact, is_cat_exact + 4, cat_start);
    patch_rel32(code.as_mut_slice(), is_cat, is_cat + 4, cat_start);
    patch_rel32(code.as_mut_slice(), external_no_arg, external_no_arg + 4, external_no_arg_start);
    patch_rel32(code.as_mut_slice(), external_argv_tail, external_argv_tail + 4, external_tail);
    patch_rel32(code.as_mut_slice(), external_done, external_done + 4, loop_start);
    patch_rel32(code.as_mut_slice(), help_done, help_done + 4, loop_start);
    patch_rel32(code.as_mut_slice(), pwd_print_call, pwd_print_call + 4, print_cstr_start);
    patch_rel32(code.as_mut_slice(), pwd_done, pwd_done + 4, loop_start);
    patch_rel32(code.as_mut_slice(), uname_print_call, uname_print_call + 4, print_cstr_start);
    patch_rel32(code.as_mut_slice(), uname_done, uname_done + 4, loop_start);
    patch_rel32(code.as_mut_slice(), cd_done, cd_done + 4, loop_start);
    patch_rel32(code.as_mut_slice(), cd_no_arg, cd_no_arg + 4, loop_start);
    patch_rel32(code.as_mut_slice(), ls_done_if_bad_open, ls_done_if_bad_open + 4, loop_start);
    patch_rel32(code.as_mut_slice(), ls_done_if_empty, ls_done_if_empty + 4, loop_start);
    patch_rel32(code.as_mut_slice(), ls_done, ls_done + 4, loop_start);
    patch_rel32(code.as_mut_slice(), ls_print_call, ls_print_call + 4, print_cstr_start);
    patch_rel32(code.as_mut_slice(), ls_back, ls_back + 4, ls_loop);
    patch_rel32(code.as_mut_slice(), cat_no_arg, cat_no_arg + 4, loop_start);
    patch_rel32(code.as_mut_slice(), cat_done_if_bad_open, cat_done_if_bad_open + 4, loop_start);
    patch_rel32(code.as_mut_slice(), cat_done_if_empty, cat_done_if_empty + 4, loop_start);
    patch_rel32(code.as_mut_slice(), cat_done, cat_done + 4, loop_start);
    patch_rel32(code.as_mut_slice(), strlen_done, strlen_done + 4, strlen_end);
    patch_rel32(code.as_mut_slice(), strlen_back, strlen_back + 4, strlen_loop);

    code
}

fn write_shared_library(path: &str, soname: &str, exports: &str) -> Result<(), &'static str> {
    let mut text = alloc::string::String::new();
    text.push_str("SAIOS_SO_V1\n");
    text.push_str("soname=");
    text.push_str(soname);
    text.push('\n');
    text.push_str("exports=");
    text.push_str(exports);
    text.push('\n');
    vfs::write_path(path, text.as_bytes())
}

fn seed_binaries() -> Result<(), &'static str> {
    for b in BIN_ENTRIES {
        if *b == "busybox" {
            continue;
        }
        let path = alloc::format!("/bin/{}", b);
        ensure_file(path.as_str())?;
        write_binary(path.as_str(), b)?;
    }

    Ok(())
}

fn seed_shared_libraries() -> Result<(), &'static str> {
    for so in SHARED_LIB_ENTRIES {
        let path = alloc::format!("/lib/{}", so);
        ensure_file(path.as_str())?;

        if *so == "ld-saios.so" {
            write_shared_library(path.as_str(), so, "dl_open,dl_sym,dl_close")?;
        } else if *so == "libc.so" {
            write_shared_library(path.as_str(), so, "malloc,free,printf,puts,exit")?;
        } else if *so == "libm.so" {
            write_shared_library(path.as_str(), so, "sin,cos,tan,sqrt")?;
        } else if *so == "libshell.so" {
            write_shared_library(path.as_str(), so, "shell_init,shell_run,shell_prompt")?;
        } else if *so == "libui.so" {
            write_shared_library(path.as_str(), so, "ui_init,ui_draw,text_edit")?;
        }
    }

    Ok(())
}

fn seed_busybox_from_kernel_image() -> Result<(), &'static str> {
    verify_busybox_static_elf(EMBEDDED_BUSYBOX)?;
    ensure_file("/bin/busybox")?;
    vfs::write_path("/bin/busybox", EMBEDDED_BUSYBOX)
}

fn read_u16_le(data: &[u8], off: usize) -> Option<u16> {
    let bytes = data.get(off..off + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(data: &[u8], off: usize) -> Option<u32> {
    let bytes = data.get(off..off + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64_le(data: &[u8], off: usize) -> Option<u64> {
    let bytes = data.get(off..off + 8)?;
    Some(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn verify_busybox_static_elf(image: &[u8]) -> Result<(), &'static str> {
    if image.len() < 64 {
        return Err("busybox: invalid ELF image");
    }
    if image[0] != 0x7F || image[1] != b'E' || image[2] != b'L' || image[3] != b'F' {
        return Err("busybox: not an ELF binary");
    }
    if image[4] != 2 || image[5] != 1 {
        return Err("busybox: unsupported ELF class or endianness");
    }

    let phoff = read_u64_le(image, 32).ok_or("busybox: malformed ELF header")? as usize;
    let phentsize = read_u16_le(image, 54).ok_or("busybox: malformed ELF header")? as usize;
    let phnum = read_u16_le(image, 56).ok_or("busybox: malformed ELF header")? as usize;

    if phentsize < 56 || phnum == 0 {
        return Err("busybox: malformed program headers");
    }

    let table_bytes = phentsize
        .checked_mul(phnum)
        .ok_or("busybox: malformed program headers")?;
    let ph_end = phoff
        .checked_add(table_bytes)
        .ok_or("busybox: malformed program headers")?;
    if ph_end > image.len() {
        return Err("busybox: truncated program headers");
    }

    let mut has_load_segment = false;
    for idx in 0..phnum {
        let off = phoff + idx * phentsize;
        let p_type = read_u32_le(image, off).ok_or("busybox: malformed program header")?;
        let p_filesz = read_u64_le(image, off + 32).ok_or("busybox: malformed program header")?;

        if p_type == ELF_PT_LOAD {
            has_load_segment = true;
        }
        if p_type == ELF_PT_INTERP && p_filesz > 0 {
            return Err("busybox: dynamically linked (PT_INTERP present)");
        }
        if p_type == ELF_PT_DYNAMIC && p_filesz > 0 {
            return Err("busybox: dynamically linked (PT_DYNAMIC present)");
        }
    }

    if !has_load_segment {
        return Err("busybox: no loadable segment");
    }

    Ok(())
}

fn busybox_source_candidates() -> Vec<alloc::string::String> {
    let mut out = Vec::new();
    for volume in crate::driver::storage::volumes_cached() {
        if volume.name == "tmpfs" {
            continue;
        }
        let Some(mountpoint) = volume.mounted_at else {
            continue;
        };
        let root = mountpoint.trim_end_matches('/');
        out.push(alloc::format!("{}/busybox", root));
        out.push(alloc::format!("{}/bin/busybox", root));
        out.push(alloc::format!("{}/usr/bin/busybox", root));
    }
    out
}

pub fn install_busybox_from_storage_to_tmpfs() -> Result<bool, &'static str> {
    for candidate in busybox_source_candidates() {
        if crate::driver::storage::mounted_volume_for_path_cached(candidate.as_str()).is_none() {
            continue;
        }

        let Ok(image) = vfs::read_path(candidate.as_str()) else {
            continue;
        };

        verify_busybox_static_elf(image.as_slice())?;
        ensure_file("/bin/busybox")?;
        vfs::write_path("/bin/busybox", image.as_slice())?;
        return Ok(true);
    }

    Ok(false)
}

fn mount_default_inner() -> Result<(), &'static str> {
    for d in ROOT_DIRS {
        ensure_dir(d)?;
    }

    seed_shared_libraries()?;
    seed_binaries()?;
    seed_busybox_from_kernel_image()?;

    ensure_file("/etc/profile")?;
    ensure_file("/etc/hostname")?;
    let _ = vfs::write_path("/etc/hostname", b"saios\n");

    write_manifest()?;
    MOUNTED.store(true, Ordering::Release);
    Ok(())
}

pub fn mount_default() -> Result<(), &'static str> {
    let previous_logging = vfs::set_event_logging_enabled(false);
    let result = mount_default_inner();
    let _ = vfs::set_event_logging_enabled(previous_logging);
    result
}

pub fn status() -> PackageImageStatus {
    PackageImageStatus {
        mounted: MOUNTED.load(Ordering::Acquire),
        profile: PROFILE,
        manifest: MANIFEST_PATH,
        roots: ROOT_DIRS.len(),
        bins: BIN_ENTRIES.len(),
    }
}
