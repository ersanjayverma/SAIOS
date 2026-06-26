//! SAIOS interactive kernel shell.
//! Reads keystrokes via PS/2 keyboard, dispatches to built-in commands.

pub mod commands;
pub mod config;
pub mod glob;

use crate::diag::watchdog;
use crate::driver::keyboard::{KeyEvent, poll};
use crate::{print, println};
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

/// Kernel-level current working directory — shared between the shell and
/// the `cd` / `pwd` commands.  Initialised to `/` at boot.
pub static CWD: Mutex<String> = Mutex::new(String::new());

/// True when booted from the installed hard disk (GRUB passes `saios.boot=hdd`).
/// Install and update media are delivery paths only; normal, safe, and debug
/// operation belongs to HDD boot.
pub static BOOTED_FROM_HDD: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

pub fn booted_from_hdd() -> bool {
    BOOTED_FROM_HDD.load(core::sync::atomic::Ordering::Relaxed)
}

/// Initialise `CWD` to `/` (called once from `Shell::new`).
fn init_cwd() {
    let mut cwd = CWD.lock();
    if cwd.is_empty() {
        cwd.push('/');
    }
}

pub fn current_cwd() -> String {
    crate::process::with_current_process(|proc| proc.cwd.clone()).unwrap_or_else(|| {
        let cwd = CWD.lock().clone();
        if cwd.is_empty() {
            String::from("/")
        } else {
            cwd
        }
    })
}

pub fn set_current_cwd(path: &str) {
    *CWD.lock() = path.to_string();
    let path = path.to_string();
    let _ = crate::process::with_current_process_mut(|proc| proc.cwd = path);
}

const MAX_HISTORY: usize = 20;

pub struct Shell {
    buf: String,
    /// Cursor position as a byte index into `buf` (input is ASCII in practice).
    cursor: usize,
    /// Number of characters currently drawn for the input line, so a redraw can
    /// blank the tail when the line shrinks.
    shown_len: usize,
    /// Cut/paste buffer (Ctrl+K/U/W cut into it, Ctrl+V/Y paste from it).
    clipboard: String,
    history: Vec<String>,
    hist_idx: Option<usize>,
}

impl Shell {
    pub fn new() -> Self {
        init_cwd();
        Self {
            buf: String::new(),
            cursor: 0,
            shown_len: 0,
            clipboard: String::new(),
            history: Vec::new(),
            hist_idx: None,
        }
    }

    /// Redraw the current input line and place the hardware cursor at `self.cursor`.
    /// Uses `\r` to return to column 0, reprints prompt+buffer, blanks any tail
    /// left from a longer previous line, then reprints up to the cursor so the
    /// hardware cursor lands in the right spot.  (Assumes the line fits one row.)
    fn redraw(&mut self) {
        let prompt = Self::prompt();
        print!("\r{}{}", prompt, self.buf);
        let pad = self.shown_len.saturating_sub(self.buf.len());
        for _ in 0..pad {
            print!(" ");
        }
        print!("\r{}{}", prompt, &self.buf[..self.cursor]);
        self.shown_len = self.buf.len();
    }

    /// Insert a string at the cursor and advance it.
    fn insert_str(&mut self, s: &str) {
        self.buf.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.redraw();
    }

    /// Build the prompt string: `saios:<cwd>$ `
    fn prompt() -> String {
        let cwd = current_cwd();
        alloc::format!("saios:{}$ ", cwd)
    }

    pub fn run(&mut self) -> ! {
        println!();
        println!("Type 'help' for available commands.");
        print!("{}", Self::prompt());
        crate::serial_println!(
            "[shell] prompt ready cwd={} kb_irqs={} pending_scancode={}",
            current_cwd(),
            crate::interrupts::KB_IRQS.load(core::sync::atomic::Ordering::Relaxed),
            crate::interrupts::has_pending_scancode(),
        );

        loop {
            // Enter input-wait mode: we're now at the shell prompt waiting for input.
            // The watchdog will use a longer timeout (60s) while in this mode.
            watchdog::enter_input_wait();

            let _ = crate::interrupts::wait_for_keyboard_input_until(Some(
                crate::shell::commands::boot_ticks().wrapping_add(1),
            ));

            // Blink the text cursor (drawn from thread context — never the IRQ).
            crate::graphics::console::update_cursor();

            // Apply queued mouse cursor update (safe here — not in IRQ context)
            crate::driver::mouse::apply_cursor_update();

            // Mouse wheel scroll
            let delta = crate::driver::mouse::take_scroll_delta();
            if delta != 0 {
                let mut w = crate::vga_buffer::WRITER.lock();
                if delta < 0 {
                    w.scroll_up((-delta) as usize * 3);
                } else {
                    w.scroll_down(delta as usize * 3);
                }
            }

            while let Some(event) = poll() {
                match event {
                    // Scroll-related keys do NOT return to live view
                    KeyEvent::Up if self.buf.is_empty() => {
                        // If input buffer is empty, Up scrolls history display
                        crate::vga_buffer::WRITER.lock().scroll_up(3);
                        continue;
                    }
                    KeyEvent::Down if self.buf.is_empty() => {
                        crate::vga_buffer::WRITER.lock().scroll_down(3);
                        continue;
                    }
                    // All other keypresses snap back to live view
                    _ => {
                        crate::vga_buffer::WRITER.lock().scroll_to_bottom();
                    }
                }
                match event {
                    KeyEvent::Char('\x03') => {
                        // Ctrl+C — abandon the current line.
                        println!("^C");
                        self.buf.clear();
                        self.cursor = 0;
                        self.shown_len = 0;
                        self.hist_idx = None;
                        print!("{}", Self::prompt());
                    }
                    KeyEvent::Char('\x0C') => {
                        // Ctrl+L — clear the screen and redraw the prompt + buffer.
                        crate::vga_buffer::clear();
                        self.shown_len = 0;
                        self.redraw();
                    }
                    // Ctrl+K — cut from the cursor to end of line into the clipboard.
                    KeyEvent::Char('\x0B') => {
                        if self.cursor < self.buf.len() {
                            self.clipboard = self.buf[self.cursor..].to_string();
                            self.buf.truncate(self.cursor);
                            self.redraw();
                        }
                    }
                    // Ctrl+U — cut from start of line to the cursor.
                    KeyEvent::Char('\x15') => {
                        if self.cursor > 0 {
                            self.clipboard = self.buf[..self.cursor].to_string();
                            self.buf = self.buf[self.cursor..].to_string();
                            self.cursor = 0;
                            self.redraw();
                        }
                    }
                    // Ctrl+W — cut the word before the cursor.
                    KeyEvent::Char('\x17') => {
                        let mut s = self.cursor;
                        while s > 0 && self.buf.as_bytes()[s - 1] == b' ' {
                            s -= 1;
                        }
                        while s > 0 && self.buf.as_bytes()[s - 1] != b' ' {
                            s -= 1;
                        }
                        if s < self.cursor {
                            self.clipboard = self.buf[s..self.cursor].to_string();
                            let tail = self.buf[self.cursor..].to_string();
                            self.buf.truncate(s);
                            self.buf.push_str(&tail);
                            self.cursor = s;
                            self.redraw();
                        }
                    }
                    // Ctrl+V or Ctrl+Y — paste the clipboard at the cursor.
                    KeyEvent::Char('\x16') | KeyEvent::Char('\x19') => {
                        if !self.clipboard.is_empty() {
                            let clip = self.clipboard.clone();
                            self.insert_str(&clip);
                        }
                    }
                    KeyEvent::Char(c) if (c >= ' ' && c != '\x7f') => {
                        if self.cursor == self.buf.len() {
                            // Fast path: appending at end — just echo the char
                            // (no full-line redraw, so no prompt reprint flicker).
                            self.buf.push(c);
                            self.cursor = self.buf.len();
                            self.shown_len = self.buf.len();
                            print!("{}", c);
                        } else {
                            self.buf.insert(self.cursor, c);
                            self.cursor += c.len_utf8();
                            self.redraw();
                        }
                    }
                    KeyEvent::Char(_) => {} // ignore other control chars
                    KeyEvent::Backspace => {
                        if self.cursor == self.buf.len() && self.cursor > 0 {
                            // Fast path: erasing at end.
                            self.buf.pop();
                            self.cursor = self.buf.len();
                            self.shown_len = self.buf.len();
                            crate::vga_buffer::backspace();
                        } else if self.cursor > 0 {
                            let mut p = self.cursor - 1;
                            while p > 0 && !self.buf.is_char_boundary(p) {
                                p -= 1;
                            }
                            self.buf.remove(p);
                            self.cursor = p;
                            self.redraw();
                        }
                    }
                    KeyEvent::Delete => {
                        if self.cursor < self.buf.len() {
                            self.buf.remove(self.cursor);
                            self.redraw();
                        }
                    }
                    KeyEvent::Left => {
                        if self.cursor > 0 {
                            let mut p = self.cursor - 1;
                            while p > 0 && !self.buf.is_char_boundary(p) {
                                p -= 1;
                            }
                            self.cursor = p;
                            self.redraw();
                        }
                    }
                    KeyEvent::Right => {
                        if self.cursor < self.buf.len() {
                            let mut p = self.cursor + 1;
                            while p < self.buf.len() && !self.buf.is_char_boundary(p) {
                                p += 1;
                            }
                            self.cursor = p;
                            self.redraw();
                        }
                    }
                    KeyEvent::Home => {
                        self.cursor = 0;
                        self.redraw();
                    }
                    KeyEvent::End => {
                        self.cursor = self.buf.len();
                        self.redraw();
                    }
                    KeyEvent::Enter => {
                        println!();
                        self.cursor = 0;
                        self.shown_len = 0;
                        let line = core::mem::take(&mut self.buf);
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            self.push_history(trimmed.into());
                            // `history` needs shell state; a trailing single `&`
                            // (not `&&`) backgrounds the job; everything else goes
                            // through the pipeline parser (|, >, >>, <, &&, ||).
                            let bg = trimmed.ends_with('&') && !trimmed.ends_with("&&");
                            if trimmed == "history" {
                                self.print_history();
                            } else if bg {
                                let job = trimmed[..trimmed.len() - 1].trim();
                                if !job.is_empty() {
                                    queue_bg_job(job);
                                }
                            } else {
                                run_line(trimmed);
                            }
                        }
                        self.hist_idx = None;
                        print!("{}", Self::prompt());
                    }
                    KeyEvent::Tab => {
                        self.autocomplete();
                    }
                    KeyEvent::Up => {
                        // Up with text in buffer = command history navigation
                        if !self.history.is_empty() && !self.buf.is_empty() {
                            let idx = self
                                .hist_idx
                                .map(|i| i.saturating_sub(1))
                                .unwrap_or(self.history.len() - 1);
                            self.hist_idx = Some(idx);
                            self.replace_line(self.history[idx].clone());
                        }
                        // Up with empty buffer = already handled (scroll) via early continue
                    }
                    KeyEvent::Down => {
                        if let Some(idx) = self.hist_idx {
                            if idx + 1 < self.history.len() {
                                let next = idx + 1;
                                self.hist_idx = Some(next);
                                self.replace_line(self.history[next].clone());
                            } else {
                                self.hist_idx = None;
                                self.replace_line(String::new());
                            }
                        }
                        // Down with empty buffer + no history = scroll (early continue)
                    }
                    // Escape resets stuck modifier state (Ctrl/Alt/Shift) that can
                    // linger after a VM grabs/releases the keyboard (e.g. Ctrl+Alt
                    // release in QEMU swallows key-up events).  Press Escape to
                    // un-stick the keyboard.
                    KeyEvent::Escape => {
                        crate::driver::keyboard::reenable();
                    }
                    // PageUp/PageDown/Insert/Function — not bound; ignore so
                    // they don't insert garbage.
                    _ => {}
                }
            }
        }
    }

    fn replace_line(&mut self, new: String) {
        self.buf = new;
        self.cursor = self.buf.len();
        self.redraw();
    }

    fn push_history(&mut self, line: String) {
        if self.history.last().map(|s| s.as_str()) != Some(&line) {
            if self.history.len() >= MAX_HISTORY {
                self.history.remove(0);
            }
            self.history.push(line);
        }
    }

    fn dispatch(&self, line: &str) {
        // `history` needs shell state; everything else is stateless and shared
        // with the background-job worker via exec_line().
        let cmd = line.split(' ').next().unwrap_or("");
        if cmd == "history" {
            self.print_history();
            return;
        }
        exec_line(line);
    }
}

/// Live progress for the current long-running task (label, done, total bytes).
pub static PROGRESS: Mutex<Option<(String, u64, u64)>> = Mutex::new(None);
/// True while the background worker is executing a job (suppresses inline bars).
pub static IN_BG: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static LAST_BAR_TICK: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static SMP_TEST_CPU_MASK: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Update the current task's progress.  Long operations (downloads) call this.
pub fn progress_set(label: &str, done: u64, total: u64) {
    *PROGRESS.lock() = Some((String::from(label), done, total));
}
pub fn progress_clear() {
    *PROGRESS.lock() = None;
}

/// Render a one-line progress bar in place (carriage-return updated), at most a
/// few times a second.  Skipped while running as a background job.
pub fn progress_render() {
    use core::sync::atomic::Ordering;
    if IN_BG.load(Ordering::Relaxed) {
        return;
    }
    let now = crate::time::uptime_ns();
    if now.wrapping_sub(LAST_BAR_TICK.load(Ordering::Relaxed)) < 250_000_000 {
        return;
    }
    LAST_BAR_TICK.store(now, Ordering::Relaxed);
    let p = PROGRESS.lock().clone();
    if let Some((label, done, total)) = p {
        let pct = done
            .checked_mul(100)
            .and_then(|n| n.checked_div(total))
            .unwrap_or(0)
            .min(100);
        let fill = (pct / 5) as usize; // 20-cell bar
        let mut bar = String::new();
        for i in 0..20 {
            bar.push(if i < fill { '#' } else { '.' });
        }
        print!(
            "\r{} [{}] {:>3}%  {}/{} KB   ",
            label,
            bar,
            pct,
            done / 1024,
            total / 1024
        );
    }
}

/// Standard executable search path.
const DEFAULT_PATH_DIRS: [&str; 4] = ["/bin", "/usr/bin", "/sbin", "/usr/sbin"];

/// Last uptime (s) the installed-binary autopoll ran.
static BIN_SCAN_AT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Cache of installed binary names, refreshed by the background autopoll.  Used
/// for tab completion and `which`.
pub static INSTALLED_BINS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Scan the PATH directories and cache the names of installed binaries.
pub fn scan_installed_bins() {
    let mut bins: Vec<String> = Vec::new();
    for dir in path_dirs() {
        if let Ok(entries) = crate::vfs_contract::VfsContract::read_dir(dir) {
            for e in entries {
                if e.name != "." && e.name != ".." && !bins.contains(&e.name) {
                    bins.push(e.name);
                }
            }
        }
    }
    *INSTALLED_BINS.lock() = bins;
}

/// Look for `cmd` as an installed binary on PATH and run it.  Returns true if a
/// matching executable was found and launched.
fn try_run_installed(cmd: &str, args: &str) -> bool {
    let argv = match parse_command_words(args) {
        Ok(words) => words,
        Err(e) => {
            println!("{}: {}", cmd, e);
            LAST_STATUS.store(1, AOrd::Relaxed);
            return true;
        }
    };
    if cmd.contains('/') {
        // explicit path
        if run_binary_path(cmd, &argv) {
            return true;
        }
    }
    for dir in path_dirs() {
        let path = alloc::format!("{}/{}", dir, cmd);
        if let Ok(inode) = crate::vfs_contract::VfsContract::resolve(&path)
            && inode.ftype == crate::vfs::FileType::RegularFile
        {
            return run_binary_path(&path, &argv);
        }
    }
    false
}

fn run_binary_path(path: &str, argv: &[String]) -> bool {
    if file_uses_shell(path) {
        let mut shell_args = String::from(path);
        if !argv.is_empty() {
            shell_args.push(' ');
            shell_args.push_str(&join_shell_words(argv));
        }
        commands::shell_builtin(&shell_args);
        return true;
    }

    let mut full_argv = Vec::with_capacity(argv.len() + 1);
    full_argv.push(String::from(path));
    full_argv.extend_from_slice(argv);
    let envp = build_exec_env();

    match crate::process::spawn_with_args_env(path, &full_argv, &envp) {
        Ok(pid) => {
            // Wait for the child process to exit via waitpid
            let _ = crate::process::waitpid(pid, 0);
            true
        }
        Err(e) => {
            println!("{}: {}", path, e);
            LAST_STATUS.store(1, AOrd::Relaxed);
            true
        }
    }
}

fn path_dirs() -> Vec<&'static str> {
    let env_path =
        shell_env_value("PATH").unwrap_or_else(|| String::from("/bin:/usr/bin:/sbin:/usr/sbin"));
    let mut dirs = Vec::new();
    for dir in env_path.split(':') {
        match dir {
            "/bin" => dirs.push("/bin"),
            "/usr/bin" => dirs.push("/usr/bin"),
            "/sbin" => dirs.push("/sbin"),
            "/usr/sbin" => dirs.push("/usr/sbin"),
            _ => {}
        }
    }
    if dirs.is_empty() {
        dirs.extend(DEFAULT_PATH_DIRS);
    }
    dirs
}

fn shell_env_value(key: &str) -> Option<String> {
    let data = commands::cat_read_env().ok()?;
    for line in data.lines() {
        let (name, value) = line.split_once('=')?;
        if name.trim() == key {
            return Some(String::from(value.trim()));
        }
    }
    None
}

fn build_exec_env() -> Vec<String> {
    let user = crate::user::get_current_user().unwrap_or_else(|| crate::user::User {
        uid: 0,
        gid: 0,
        username: String::from("root"),
        home: String::from("/users/root"),
        shell: String::from("/bin/sh"),
    });
    let cwd = current_cwd();
    let path =
        shell_env_value("PATH").unwrap_or_else(|| String::from("/bin:/usr/bin:/sbin:/usr/sbin"));

    let mut env = vec![
        alloc::format!("PATH={}", path),
        alloc::format!("HOME={}", user.home),
        alloc::format!("USER={}", user.username),
        alloc::format!("PWD={}", cwd),
    ];

    if let Ok(text) = commands::cat_read_env() {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with("PATH=")
                || trimmed.starts_with("HOME=")
                || trimmed.starts_with("USER=")
                || trimmed.starts_with("PWD=")
            {
                continue;
            }
            env.push(String::from(trimmed));
        }
    }
    env
}

fn parse_command_words(input: &str) -> Result<Vec<String>, &'static str> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' && !in_single {
            if let Some(next) = chars.next() {
                current.push(next);
            }
            continue;
        }

        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    words.push(core::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if in_single || in_double {
        return Err("unterminated quote");
    }

    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

fn join_shell_words(words: &[String]) -> String {
    let mut out = String::new();
    for (idx, word) in words.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

fn file_uses_shell(path: &str) -> bool {
    let Ok(data) = crate::vfs_contract::VfsContract::read_file(path) else {
        return false;
    };
    data.len() >= 2 && data[0] == b'#' && data[1] == b'!'
}

/// Background job queue — commands suffixed with `&` are pushed here and run by
/// the `bgworker` kernel thread, so the shell prompt stays responsive.
pub static BG_QUEUE: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Queue a command line to run in the background.
pub fn queue_bg_job(line: &str) {
    BG_QUEUE.lock().push_back(String::from(line));
    println!("[bg] queued: {}", line);
}

/// Background-job worker kernel thread: drains BG_QUEUE and runs each command.
/// Preemption + this thread mean a long `apt update &` no longer blocks input.
pub extern "C" fn bg_worker_thread() {
    loop {
        let job = BG_QUEUE.lock().pop_front();
        if let Some(line) = job {
            println!("\n[bg] start: {}", line);
            IN_BG.store(true, core::sync::atomic::Ordering::Relaxed);
            run_line(&line);
            IN_BG.store(false, core::sync::atomic::Ordering::Relaxed);
            println!("\n[bg] done: {}", line);
        } else {
            // Drive the network stack from the bg thread so the shell input
            // loop is never blocked by RX polling or TX flushing.
            crate::net::poll();
            // Autopoll: refresh the installed-binary cache about every 5 s so
            // newly apt-installed programs become runnable/tab-completable.
            let now = crate::time::uptime_secs();
            let last = BIN_SCAN_AT.load(core::sync::atomic::Ordering::Relaxed);
            if now.wrapping_sub(last) >= 5 {
                BIN_SCAN_AT.store(now, core::sync::atomic::Ordering::Relaxed);
                scan_installed_bins();
            }
            crate::process::scheduler::yield_now();
            crate::arch::halt();
        }
    }
}

/// Service the keyboard while a long-running command is executing, so the
/// terminal stays visibly alive: printable keys echo and Enter shows a newline.
/// Called from long loops (e.g. the HTTP download in `apt`).  Runs in the
/// command's own thread, so console output here is thread-context safe.
/// Keystrokes are consumed as live feedback (the running command isn't reading
/// stdin); the prompt returns fresh when the command finishes.
pub fn service_input_echo() {
    while let Some(event) = poll() {
        match event {
            KeyEvent::Char(c) if c >= ' ' && c != '\x7f' => print!("{}", c),
            KeyEvent::Enter => println!(),
            KeyEvent::Backspace => crate::vga_buffer::backspace(),
            _ => {}
        }
        // After processing each keystroke, leave input-wait mode so the watchdog
        // can detect actual stalls in the long-running command.
        watchdog::leave_input_wait();
    }
    // If we're in this function, we've finished draining the input buffer.
    // Re-enter input-wait mode since we may continue waiting for more input.
    watchdog::enter_input_wait();
}

/// SMP smoke-test worker: a non-pinned compute thread that logs which CPU it is
/// running on, does a burst of busy work, yields, and exits.  Spawned by the
/// `smptest` command — application processors pull these off the run queue, so
/// the `[smptest] ... cpu N` serial lines show threads executing across cores.
pub extern "C" fn smp_worker_thread() {
    for _ in 0..6 {
        let cpu = crate::process::table::cpu_idx();
        SMP_TEST_CPU_MASK.fetch_or(1u64 << cpu, core::sync::atomic::Ordering::Relaxed);
        crate::serial_println!(
            "[smptest] worker on cpu{} apic_id={}",
            cpu,
            crate::smp::lapic_id()
        );
        let mut x = 0u64;
        for _ in 0..30_000_000u64 {
            x = x.wrapping_add(1);
            core::hint::spin_loop();
        }
        core::hint::black_box(x);
        crate::process::scheduler::yield_now();
    }
    crate::serial_println!(
        "[smptest] worker exiting cpu{} apic_id={}",
        crate::process::table::cpu_idx(),
        crate::smp::lapic_id()
    );
}

// -- Shell pipeline: operators |  >  >>  <  &&  ||  and $((arith)) -------------

use core::sync::atomic::{AtomicI32, Ordering as AOrd};

/// Exit status of the last command (0 = success).  Set by `exec_line`.
pub static LAST_STATUS: AtomicI32 = AtomicI32::new(0);
/// Standard input for the current command (set by a pipe `|` or `< file`).
pub static STDIN_BUF: Mutex<Option<String>> = Mutex::new(None);

/// A command that reads stdin (cat/grep/wc with no file arg) takes it here.
pub fn take_stdin() -> Option<String> {
    STDIN_BUF.lock().take()
}

/// Parse and run a full command line, honouring &&, ||, pipes, redirection and
/// arithmetic expansion.  This is the entry point the shell + bg worker use.
pub fn run_line(line: &str) {
    let line = expand_arith(line);
    // Split into &&/|| segments (left-to-right, single | stays inside segments).
    let mut seg = String::new();
    let mut connector = Conn::Always;
    let run_seg = |s: &str, conn: Conn| {
        let s = s.trim();
        if s.is_empty() {
            return;
        }
        let last = LAST_STATUS.load(AOrd::Relaxed);
        let should = match conn {
            Conn::Always => true,
            Conn::AndIf => last == 0,
            Conn::OrIf => last != 0,
        };
        if should {
            run_pipeline(s);
        }
    };
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '&' if chars.peek().copied() == Some('&') => {
                chars.next();
                run_seg(&seg, connector);
                connector = Conn::AndIf;
                seg.clear();
            }
            '|' if chars.peek().copied() == Some('|') => {
                chars.next();
                run_seg(&seg, connector);
                connector = Conn::OrIf;
                seg.clear();
            }
            _ => seg.push(ch),
        }
    }
    run_seg(&seg, connector);
}

#[derive(Clone, Copy)]
enum Conn {
    Always,
    AndIf,
    OrIf,
}

/// Run one `|`-separated pipeline with redirection on its ends.
fn run_pipeline(segment: &str) {
    // Glob-expand the entire segment first.  We split on `|` only
    // for the *pipeline*; globs inside a single command work as
    // expected (`cat /tmp/*.log | grep foo` expands the glob on the
    // left side of the pipe).  Expansion preserves redirection
    // tokens: `<`, `>`, `>>` are not glob metachars so they pass
    // through unchanged, and the post-expansion token list is
    // re-split on `|` afterwards.
    let expanded = glob_expand_segment(segment);
    let stages: Vec<String> = expanded.split('|').map(|s| s.trim().to_string()).collect();
    let mut piped: Option<String> = None;

    for (idx, raw) in stages.iter().enumerate() {
        let last = idx == stages.len() - 1;
        // Strip redirections from this stage's command text.
        let (cmd, infile, outfile, append) = parse_redirs(raw);

        // stdin: file redirect wins, else the previous stage's piped output.
        if let Some(f) = infile {
            *STDIN_BUF.lock() = read_file_string(&f);
        } else if let Some(p) = piped.take() {
            *STDIN_BUF.lock() = Some(p);
        }

        // Capture this stage's stdout if it pipes onward or redirects to a file.
        let capturing = !last || outfile.is_some();
        let prev = if capturing {
            Some(crate::vga_buffer::capture_begin())
        } else {
            None
        };
        exec_line(&cmd);
        let out = if let Some(p) = prev {
            crate::vga_buffer::capture_end(p)
        } else {
            String::new()
        };

        if let Some(path) = outfile {
            write_file_string(&path, &out, append);
        } else if !last {
            piped = Some(out);
        }
        // clear any unconsumed stdin
        *STDIN_BUF.lock() = None;
    }
}

/// Extract `< in`, `> out`, `>> out` from a command, returning the cleaned
/// command plus the filenames.
fn parse_redirs(s: &str) -> (String, Option<String>, Option<String>, bool) {
    let toks: Vec<&str> = s.split_whitespace().collect();
    let mut cmd = String::new();
    let (mut infile, mut outfile, mut append) = (None, None, false);
    let mut i = 0;
    while i < toks.len() {
        match toks[i] {
            ">>" => {
                append = true;
                outfile = toks.get(i + 1).map(|s| String::from(*s));
                i += 2;
            }
            ">" => {
                append = false;
                outfile = toks.get(i + 1).map(|s| String::from(*s));
                i += 2;
            }
            "<" => {
                infile = toks.get(i + 1).map(|s| String::from(*s));
                i += 2;
            }
            t => {
                if !cmd.is_empty() {
                    cmd.push(' ');
                }
                cmd.push_str(t);
                i += 1;
            }
        }
    }
    (cmd, infile, outfile, append)
}

fn read_file_string(path: &str) -> Option<String> {
    let p = commands::vfs_abs_pub(path);
    let buf = crate::vfs_contract::VfsContract::read_file(&p).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn write_file_string(path: &str, data: &str, append: bool) {
    let p = commands::vfs_abs_pub(path);
    let bytes = data.as_bytes();
    if append {
        let _ = crate::vfs_contract::VfsContract::append_file(&p, bytes, 0o644);
        return;
    }
    let _ = crate::vfs_contract::VfsContract::write_file(&p, bytes, 0o644);
}

/// Expand `$(( expr ))` integer arithmetic in a command line.
fn expand_arith(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("$((") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        if let Some(end) = after.find("))") {
            let expr = &after[..end];
            out.push_str(&alloc::format!("{}", eval_arith(expr)));
            rest = &after[end + 2..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

/// Evaluate an integer expression with + - * / % and parentheses.
pub fn eval_arith(expr: &str) -> i64 {
    let toks = tokenize_expr(expr);
    let mut p = 0usize;
    parse_expr(&toks, &mut p)
}
fn tokenize_expr(s: &str) -> Vec<String> {
    let mut t = Vec::new();
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            if !num.is_empty() {
                t.push(num.clone());
                num.clear();
            }
            if "+-*/%()".contains(c) {
                let mut s = String::new();
                s.push(c);
                t.push(s);
            }
        }
    }
    if !num.is_empty() {
        t.push(num);
    }
    t
}
fn parse_expr(t: &[String], p: &mut usize) -> i64 {
    let mut v = parse_term(t, p);
    while *p < t.len() && (t[*p] == "+" || t[*p] == "-") {
        let op = t[*p].clone();
        *p += 1;
        let r = parse_term(t, p);
        v = if op == "+" { v + r } else { v - r };
    }
    v
}
fn parse_term(t: &[String], p: &mut usize) -> i64 {
    let mut v = parse_factor(t, p);
    while *p < t.len() && (t[*p] == "*" || t[*p] == "/" || t[*p] == "%") {
        let op = t[*p].clone();
        *p += 1;
        let r = parse_factor(t, p);
        v = match op.as_str() {
            "*" => v * r,
            "/" => {
                if r != 0 {
                    v / r
                } else {
                    0
                }
            }
            _ => {
                if r != 0 {
                    v % r
                } else {
                    0
                }
            }
        };
    }
    v
}
fn parse_factor(t: &[String], p: &mut usize) -> i64 {
    if *p >= t.len() {
        return 0;
    }
    if t[*p] == "(" {
        *p += 1;
        let v = parse_expr(t, p);
        if *p < t.len() && t[*p] == ")" {
            *p += 1;
        }
        return v;
    }
    if t[*p] == "-" {
        *p += 1;
        return -parse_factor(t, p);
    }
    let v = t[*p].parse::<i64>().unwrap_or(0);
    *p += 1;
    v
}

/// Run one command line with no shell state.  Shared by the interactive shell
/// (foreground) and the background-job worker thread (`cmd &`).
pub fn exec_line(line: &str) {
    let words = match parse_command_words(line) {
        Ok(words) => words,
        Err(e) => {
            println!("shell: {}", e);
            LAST_STATUS.store(1, AOrd::Relaxed);
            return;
        }
    };
    let cmd = words.first().map(|s| s.as_str()).unwrap_or("");
    let args = if words.len() > 1 {
        join_shell_words(&words[1..])
    } else {
        String::new()
    };

    LAST_STATUS.store(0, AOrd::Relaxed); // success unless a command says otherwise

    if let Some(shell_args) = implicit_shell_args(cmd, &args) {
        commands::shell_builtin(&shell_args);
        return;
    }

    match cmd {
        // System
        "irqinfo" => {
            use core::sync::atomic::Ordering;
            let kb = crate::interrupts::KB_IRQS.load(Ordering::Relaxed);
            let mouse = crate::interrupts::MOUSE_IRQS.load(Ordering::Relaxed);
            let timer = crate::interrupts::TIMER_IRQS.load(Ordering::Relaxed);
            println!("IRQ counters: kb={} mouse={} timer={}", kb, mouse, timer);
            println!("(run irqinfo again after input freeze to see if they increment)");
        }
        "kbreset" => {
            crate::driver::keyboard::reenable();
            println!("Keyboard reset: drained buffer, re-enabled scanning, cleared modifiers.");
        }
        "help" => commands::help_cmd(&args),
        "uname" => commands::uname(),
        "cpuinfo" => commands::cpuinfo(),
        "cpus" => commands::cpus(),
        "jobs" => commands::jobs(),
        "testsaios" => commands::testsaios(),
        "sysinfo" => commands::sysinfo(),
        "resmon" => commands::resmon(),
        "diag" => commands::diag_str(&args),
        "kds" => commands::kds(&args),
        "obs" => commands::obs(&args),
        "storage" => commands::storage(&args),
        "verify" => commands::verify(&args),
        "meminfo" => commands::meminfo(),
        "uptime" => commands::uptime(),
        "lspci" => commands::lspci(),
        "clear" => commands::clear(),
        "journal" | "dmesg" => commands::journal(&args),
        "color" => commands::color(&args),
        "reboot" => commands::reboot(),
        "halt" => commands::halt(),
        // Files
        "cd" => commands::cd(&args),
        "pwd" => commands::pwd(),
        "ls" => commands::ls(&args),
        "cat" => commands::cat(&args),
        "write" => commands::write_file(&args),
        "append" => commands::append_file(&args),
        "cp" => commands::cp(&args),
        "mv" => commands::mv(&args),
        "rm" => commands::rm(&args),
        "mkdir" => commands::mkdir(&args),
        "find" => commands::find(&args),
        "grep" => commands::grep(&args),
        "hexdump" => commands::hexdump(&args),
        "wc" => commands::wc(&args),
        "df" => commands::df(),
        // Scripting
        "echo" => commands::echo(&args),
        "calc" => commands::calc(&args),
        "run" => commands::run(&args),
        "env" => commands::env_cmd(&args),
        "set" => commands::set_cmd(&args),
        "todo" => commands::todo_cmd(&args),
        "notes" => commands::notes(),
        "history" => println!("history: interactive shell only"),
        // Network
        "net" => commands::net(&args),
        "fetch" => commands::fetch(&args),
        // AI
        "ai" => commands::ai(&args),
        "sairu" => commands::sairu(&args),
        // Dev
        "cc" => commands::cc(&args),
        "explain" => commands::explain(&args),
        "exec" => commands::exec(&args),
        "ps" => commands::ps(),
        "kill" => commands::kill(&args),
        "id" => commands::id(),
        "whoami" => commands::whoami(),
        "users" => commands::users(),
        "login" => commands::login(&args),
        "logout" => commands::logout(),
        "su" => commands::su(&args),
        "passwd" => commands::passwd(&args),
        "useradd" => commands::useradd(&args),
        "userdel" => commands::userdel(&args),
        "chmod" => commands::chmod(&args),
        "chown" => commands::chown(&args),
        "install" => commands::install(&args),
        "update" => commands::update(&args),
        "recover" => commands::storage("recover"),
        "reinstall" => commands::reinstall(&args),
        "saios" => commands::saios_cmd(&args),
        "man" => commands::man_cmd(&args),
        "setup" => commands::setup(&args),
        "bash" => commands::bash_cmd(&args),
        "wifi" => crate::driver::wifi::cmd_wifi(&args),
        "beep" => commands::beep(&args),
        "gfx" => commands::gfx(&args),
        "reload" => commands::reload_cmd(&args),
        "lsusb" => crate::driver::usb_hid::init(), // re-enumerate
        // Built-in tools
        "curl" => crate::tools::curl::run(&args),
        "wget" => crate::tools::wget::run(&args),
        "openssl" => crate::tools::openssl::run(&args),
        "ssh" => crate::tools::ssh::run(&args),
        "vi" => crate::tools::vi::run(&args),
        "nano" => crate::tools::nano::run(&args),
        "sh" | "/bin/sh" | "/bin/bash" | "/usr/bin/bash" => commands::shell_builtin(&args),
        "apt" => crate::tools::apt::run(&args),
        "apt-get" => crate::tools::apt::run(&args),
        "make" => crate::tools::build_essentials::run_make(&args),
        "build-essential" | "build-essentials" => {
            crate::tools::build_essentials::run_build_essential()
        }
        "config" => commands::config_cmd(&args),
        "" => {} // empty stage (e.g. trailing pipe) — no-op
        _ => {
            // Not a builtin — look for an installed binary on PATH and run it.
            if !try_run_installed(cmd, &args) {
                explain_unknown_command(cmd, &args);
                LAST_STATUS.store(1, AOrd::Relaxed);
            }
        }
    }
}

fn explain_unknown_command(cmd: &str, args: &str) {
    match cmd {
        "storag" | "stroage" => suggest_command(cmd, "storage", args),
        "sair" | "sariu" => suggest_command(cmd, "sairu", args),
        "rebot" | "reboto" => suggest_command(cmd, "reboot", args),
        "analyse" => {
            println!("unknown command: '{}'", cmd);
            println!("Did you mean: storage analyze");
        }
        "diagnose" | "daignose" => {
            println!("unknown command: '{}'", cmd);
            println!("Possible commands:");
            println!(
                "  sairu diagnose {}",
                if args.is_empty() { "storage" } else { args }
            );
            println!("  storage diagnose");
        }
        _ => {
            println!("unknown command: '{}' — type 'help'", cmd);
            println!("Try: saios help, sairu diagnose storage, storage analyze, storage recommend");
        }
    }
}

fn suggest_command(input: &str, command: &str, args: &str) {
    println!("unknown command: '{}'", input);
    if args.is_empty() {
        println!("Did you mean: {}", command);
    } else {
        println!("Did you mean: {} {}", command, args);
    }
}

fn implicit_shell_args(cmd: &str, args: &str) -> Option<String> {
    let rest = cmd
        .strip_prefix("sh")
        .or_else(|| cmd.strip_prefix("bash"))?;
    if !(rest.starts_with("./") || rest.starts_with("../") || rest.starts_with('/')) {
        return None;
    }

    let mut shell_args = String::from(rest);
    if !args.is_empty() {
        shell_args.push(' ');
        shell_args.push_str(args);
    }
    Some(shell_args)
}

impl Shell {
    fn print_history(&self) {
        for (i, line) in self.history.iter().enumerate() {
            println!("  {:3}  {}", i + 1, line);
        }
    }

    fn autocomplete(&mut self) {
        let completions = [
            "help", "clear", "echo", "uname", "meminfo", "net", "ai", "sairu", "ls", "cat",
            "write", "mkdir", "rm", "df", "history", "reboot", "halt", "jobs", "cpus", "sysinfo",
            "resmon", "openssl", "ssh", "wget", "curl", "apt", "man", "fetch", "explain", "color",
            "uptime", "id", "whoami", "users", "grep", "find",
        ];
        // Builtins + autopolled installed binaries.
        let bins = INSTALLED_BINS.lock().clone();
        let matches: Vec<String> = completions
            .iter()
            .map(|s| String::from(*s))
            .chain(bins)
            .filter(|c| c.starts_with(self.buf.as_str()))
            .collect();
        if matches.len() == 1 {
            // Apply the completion to the buffer (not just the screen).
            let suffix = matches[0][self.buf.len()..].to_string();
            self.buf.push_str(&suffix);
            self.cursor = self.buf.len();
            self.redraw();
        } else if matches.len() > 1 {
            println!();
            for m in &matches {
                print!("{}  ", m);
            }
            println!();
            self.shown_len = 0;
            self.redraw();
        }
    }
}

/// Walk a single pipeline segment token-by-token, expanding any
/// globs.  Quoted tokens (single *or* double quotes) pass through
/// unchanged.  Pipe characters inside quotes are preserved.
///
/// We have to do a *character-class aware* walk here because
/// `str::split_whitespace()` would mis-split
/// `echo "a | b" | wc`.  The state machine is small: 4 states
/// (Space | Token), 2 quote modes (none | single | double).
fn glob_expand_segment(seg: &str) -> String {
    let mut out = String::new();
    let mut token = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let flush = |tok: String, out: &mut String| {
        if tok.is_empty() {
            return;
        }
        let expanded = glob::expand_token(&tok);
        for (j, e) in expanded.iter().enumerate() {
            if j > 0 {
                out.push(' ');
            }
            out.push_str(e);
        }
    };
    let mut chars = seg.chars().peekable();
    while let Some(c) = chars.next() {
        // Handle backslash escape (outside single quotes).
        if c == '\\' && !in_single {
            token.push(c);
            if let Some(next) = chars.next() {
                token.push(next);
            }
            continue;
        }
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                token.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                token.push(c);
            }
            ' ' | '\t' if !in_single && !in_double => {
                flush(token.clone(), &mut out);
                token.clear();
                out.push(' ');
            }
            _ => token.push(c),
        }
    }
    flush(token, &mut out);
    out
}
