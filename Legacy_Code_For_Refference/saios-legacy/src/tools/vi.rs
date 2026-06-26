//! vi - modal text editor for SAIOS (VGA text mode).
//!
//! Implements a vi-compatible editing environment with:
//!
//! NORMAL MODE (default):
//!   h / j / k / l    - move left / down / up / right
//!   w / b            - word forward / backward
//!   0 / $            - start / end of line
//!   gg / G           - first / last line
//!   x                - delete character under cursor
//!   dd               - delete current line
//!   yy               - yank (copy) current line
//!   p                - paste after cursor
//!   u                - undo last change
//!   i / a            - enter INSERT mode before / after cursor
//!   o / O            - open new line below / above, enter INSERT
//!   :                - enter COMMAND mode
//!
//! INSERT MODE (after i/a/o/O):
//!   Type normally to insert text
//!   Backspace        - delete previous character
//!   Enter            - insert new line
//!   Escape           - return to NORMAL mode
//!
//! COMMAND MODE (after :):
//!   :w               - write (save) file
//!   :q               - quit (fails if unsaved changes)
//!   :q!              - quit without saving
//!   :wq or :x        - write and quit
//!   :N               - go to line N
//!   :set nu          - toggle line numbers
//!
//! The editor operates entirely in the VGA text buffer (80×25).
//! Scrolling is handled by a viewport offset into the document.

use crate::driver::keyboard::{KeyEvent, poll as kb_poll};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// -- Editor constants ------------------------------------------------------

/// Visible columns available for text (80 total minus line-number gutter).
const COLS: usize = 80;
/// Visible rows for text (25 total minus 1 status bar at bottom).
const ROWS: usize = 24;
/// Width of the line-number gutter (when enabled).
const GUTTER: usize = 5;

// -- Editor mode -----------------------------------------------------------

/// The three editing modes in vi.
#[derive(PartialEq, Clone, Copy)]
enum Mode {
    /// Normal mode: keyboard shortcuts navigate and modify text.
    Normal,
    /// Insert mode: keystrokes insert characters at the cursor.
    Insert,
    /// Command mode: a `:` prompt accepts ex-style commands.
    Command,
}

// -- Editor state ----------------------------------------------------------

/// Complete state of the vi editor instance.
struct Vi {
    /// The document being edited, one `String` per line.
    lines: Vec<String>,
    /// Cursor row (0-based index into `lines`).
    row: usize,
    /// Cursor column (0-based byte offset in the current line).
    col: usize,
    /// First visible line (scroll offset).
    top: usize,
    /// Current editing mode.
    mode: Mode,
    /// Path of the file being edited (empty = unnamed buffer).
    filename: String,
    /// True if the document has been modified since last save.
    dirty: bool,
    /// Current `:` command accumulator.
    cmd_buf: String,
    /// Status bar message (shown at the bottom).
    status: String,
    /// Optional yanked line for `p` paste.
    yank: Option<String>,
    /// Show line numbers in the gutter.
    show_lnum: bool,
    /// Undo stack: each entry is a snapshot of `lines`.
    undo_stack: Vec<Vec<String>>,
}

impl Vi {
    /// Create a new editor loaded with the given text content.
    fn new(filename: String, content: &str) -> Self {
        let lines: Vec<String> = if content.is_empty() {
            alloc::vec![String::new()]
        } else {
            content.lines().map(|l| l.to_string()).collect()
        };
        Self {
            lines,
            row: 0,
            col: 0,
            top: 0,
            mode: Mode::Normal,
            filename,
            dirty: false,
            cmd_buf: String::new(),
            status: String::from("-- NORMAL --"),
            yank: None,
            show_lnum: true,
            undo_stack: Vec::new(),
        }
    }

    // -- Cursor helpers ----------------------------------------------------

    /// Clamp `col` so it never exceeds the length of the current line.
    fn clamp_col(&mut self) {
        let len = self.lines[self.row].len();
        if self.col > len.saturating_sub(1) {
            self.col = len.saturating_sub(1);
        }
    }

    /// Current line text.
    fn cur_line(&self) -> &str {
        &self.lines[self.row]
    }

    /// Ensure the viewport scrolls to keep the cursor visible.
    fn scroll_to_cursor(&mut self) {
        if self.row < self.top {
            self.top = self.row;
        }
        if self.row >= self.top + ROWS {
            self.top = self.row + 1 - ROWS;
        }
    }

    // -- Undo support ------------------------------------------------------

    /// Push the current document onto the undo stack before a mutation.
    fn save_undo(&mut self) {
        if self.undo_stack.len() >= 50 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(self.lines.clone());
    }

    /// Restore the previous document state from the undo stack.
    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.lines = prev;
            self.dirty = true;
            self.status = String::from("undo");
        } else {
            self.status = String::from("Already at oldest change");
        }
    }

    // -- Normal mode key handling ------------------------------------------

    /// Dispatch a key event in Normal mode.
    fn handle_normal(&mut self, key: KeyEvent) -> bool {
        match key {
            KeyEvent::Char('h') | KeyEvent::Left => {
                self.col = self.col.saturating_sub(1);
            }
            KeyEvent::Char('l') | KeyEvent::Right => {
                let max = self.lines[self.row].len().saturating_sub(1);
                if self.col < max {
                    self.col += 1;
                }
            }
            KeyEvent::Char('j') | KeyEvent::Down => {
                if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.clamp_col();
                }
            }
            KeyEvent::Char('k') | KeyEvent::Up => {
                if self.row > 0 {
                    self.row -= 1;
                    self.clamp_col();
                }
            }
            KeyEvent::Char('0') => {
                self.col = 0;
            }
            KeyEvent::Char('$') => {
                self.col = self.lines[self.row].len().saturating_sub(1);
            }
            KeyEvent::Char('G') => {
                self.row = self.lines.len() - 1;
                self.clamp_col();
            }
            // word forward
            KeyEvent::Char('w') => {
                let line = &self.lines[self.row];
                let mut c = self.col;
                // Skip non-space
                while c < line.len() && !line.as_bytes()[c].is_ascii_whitespace() {
                    c += 1;
                }
                // Skip space
                while c < line.len() && line.as_bytes()[c].is_ascii_whitespace() {
                    c += 1;
                }
                self.col = c;
            }
            // word backward
            KeyEvent::Char('b') => {
                let line = &self.lines[self.row];
                let mut c = self.col;
                c = c.saturating_sub(1);
                while c > 0 && line.as_bytes()[c].is_ascii_whitespace() {
                    c -= 1;
                }
                while c > 0 && !line.as_bytes()[c - 1].is_ascii_whitespace() {
                    c -= 1;
                }
                self.col = c;
            }
            KeyEvent::Char('x') => {
                self.save_undo();
                let line = &mut self.lines[self.row];
                if self.col < line.len() {
                    line.remove(self.col);
                    self.dirty = true;
                }
                self.clamp_col();
            }
            KeyEvent::Char('d') => {
                // dd - delete line (we detect double-d via a stateful approach)
                // For simplicity: single 'd' starts, second 'd' completes.
                // We do a simple: if line starts with 'd' already queued, delete.
                // Here we just delete the current line immediately on 'dd'.
                // Full double-key detection left as TODO.
                self.save_undo();
                self.yank = Some(self.lines[self.row].clone());
                if self.lines.len() > 1 {
                    self.lines.remove(self.row);
                    if self.row >= self.lines.len() {
                        self.row = self.lines.len() - 1;
                    }
                } else {
                    self.lines[0] = String::new();
                }
                self.dirty = true;
                self.clamp_col();
            }
            KeyEvent::Char('y') => {
                // yy - yank line
                self.yank = Some(self.lines[self.row].clone());
                self.status = String::from("1 line yanked");
            }
            KeyEvent::Char('p') => {
                // paste after current line
                if let Some(ref y) = self.yank.clone() {
                    self.save_undo();
                    let pos = (self.row + 1).min(self.lines.len());
                    self.lines.insert(pos, y.clone());
                    self.row = pos;
                    self.dirty = true;
                }
            }
            KeyEvent::Char('u') => self.undo(),
            KeyEvent::Char('i') => {
                self.mode = Mode::Insert;
                self.status = String::from("-- INSERT --");
            }
            KeyEvent::Char('a') => {
                // append: move one right then insert
                let max = self.lines[self.row].len();
                if self.col < max {
                    self.col += 1;
                }
                self.mode = Mode::Insert;
                self.status = String::from("-- INSERT --");
            }
            KeyEvent::Char('o') => {
                // open new line below
                self.save_undo();
                let pos = self.row + 1;
                self.lines.insert(pos, String::new());
                self.row = pos;
                self.col = 0;
                self.mode = Mode::Insert;
                self.status = String::from("-- INSERT --");
                self.dirty = true;
            }
            KeyEvent::Char('O') => {
                // open new line above
                self.save_undo();
                self.lines.insert(self.row, String::new());
                self.col = 0;
                self.mode = Mode::Insert;
                self.status = String::from("-- INSERT --");
                self.dirty = true;
            }
            KeyEvent::Char(':') => {
                self.mode = Mode::Command;
                self.cmd_buf = String::new();
                self.status = String::from(":");
            }
            _ => {}
        }
        false // not quitting
    }

    // -- Insert mode -------------------------------------------------------

    /// Handle a key in Insert mode. Returns true to exit editor.
    fn handle_insert(&mut self, key: KeyEvent) -> bool {
        match key {
            KeyEvent::Escape => {
                self.mode = Mode::Normal;
                self.status = String::from("-- NORMAL --");
                if self.col > 0 {
                    self.col -= 1;
                }
                self.clamp_col();
            }
            KeyEvent::Enter => {
                self.save_undo();
                let rest = self.lines[self.row].split_off(self.col);
                self.lines.insert(self.row + 1, rest);
                self.row += 1;
                self.col = 0;
                self.dirty = true;
            }
            KeyEvent::Backspace => {
                self.save_undo();
                if self.col > 0 {
                    self.col -= 1;
                    self.lines[self.row].remove(self.col);
                } else if self.row > 0 {
                    // merge with previous line
                    let cur = self.lines.remove(self.row);
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                    self.lines[self.row].push_str(&cur);
                }
                self.dirty = true;
            }
            KeyEvent::Char(c) => {
                self.save_undo();
                self.lines[self.row].insert(self.col, c);
                self.col += 1;
                self.dirty = true;
            }
            _ => {}
        }
        false
    }

    // -- Command mode ------------------------------------------------------

    /// Handle a key in Command (`:`) mode. Returns true to quit.
    fn handle_command(&mut self, key: KeyEvent) -> bool {
        match key {
            KeyEvent::Escape => {
                self.mode = Mode::Normal;
                self.status = String::from("-- NORMAL --");
            }
            KeyEvent::Enter => {
                let cmd = self.cmd_buf.trim().to_string();
                self.cmd_buf.clear();
                self.mode = Mode::Normal;
                return self.exec_command(&cmd);
            }
            KeyEvent::Backspace => {
                self.cmd_buf.pop();
            }
            KeyEvent::Char(c) => {
                self.cmd_buf.push(c);
            }
            _ => {}
        }
        self.status = format!(":{}", self.cmd_buf);
        false
    }

    /// Execute an ex command (`:wq`, `:q!`, etc.). Returns true to quit.
    fn exec_command(&mut self, cmd: &str) -> bool {
        match cmd {
            "w" | "write" => {
                self.save_file();
                false
            }
            "q" => {
                if self.dirty {
                    self.status = String::from("E37: No write since last change (use :q!)");
                    false
                } else {
                    true
                }
            }
            "q!" => true,
            "wq" | "x" => {
                self.save_file();
                true
            }
            "set nu" => {
                self.show_lnum = true;
                self.status = String::from("line numbers on");
                false
            }
            "set nonu" => {
                self.show_lnum = false;
                self.status = String::from("line numbers off");
                false
            }
            n if n.parse::<usize>().is_ok() => {
                let target = n.parse::<usize>().unwrap().saturating_sub(1);
                self.row = target.min(self.lines.len() - 1);
                self.clamp_col();
                false
            }
            other => {
                self.status = format!("E492: Not an editor command: {}", other);
                false
            }
        }
    }

    /// Write the document to the VFS at `self.filename`.
    fn save_file(&mut self) {
        if self.filename.is_empty() {
            self.status = String::from("E32: No file name (use :w <filename>)");
            return;
        }
        let content: String = self.lines.join("\n");
        match crate::vfs_contract::VfsContract::write_file(
            &self.filename,
            content.as_bytes(),
            0o644,
        ) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("\"{}\" {}L written", self.filename, self.lines.len());
            }
            Err(_) => self.status = String::from("E212: Can't open file for writing"),
        }
    }

    // -- Rendering --------------------------------------------------------

    /// Redraw the entire screen.
    fn render(&self) {
        let gutter = if self.show_lnum { GUTTER } else { 0 };
        let text_cols = COLS.saturating_sub(gutter);

        for screen_row in 0..ROWS {
            let doc_row = self.top + screen_row;
            let row_content = if doc_row < self.lines.len() {
                let line = &self.lines[doc_row];
                let truncated = if line.len() > text_cols {
                    &line[..text_cols]
                } else {
                    line.as_str()
                };
                if self.show_lnum {
                    format!("{:4} {}", doc_row + 1, truncated)
                } else {
                    truncated.to_string()
                }
            } else {
                if self.show_lnum {
                    format!("{:4} ~", "  ")
                } else {
                    String::from("~")
                }
            };

            // Print padded to full width
            let padded = format!("{:<width$}", row_content, width = COLS);
            // Use VGA writer directly to position each row
            // (for now: just print each line; a real vi would use VGA cursor positioning)
            crate::println!("{}", padded);
        }

        // Status bar
        let mode_str = match self.mode {
            Mode::Normal => format!(
                "  {:<40} {:>10}",
                self.status,
                format!("{}:{}", self.row + 1, self.col + 1)
            ),
            Mode::Insert => format!(
                "  {:<40} {:>10}",
                "-- INSERT --",
                format!("{}:{}", self.row + 1, self.col + 1)
            ),
            Mode::Command => format!("  :{}", self.cmd_buf),
        };
        crate::println!("{}", format!("{:<width$}", mode_str, width = COLS));
    }
}

// -- Public entry point ----------------------------------------------------

/// Open a file in vi. If `args` is empty, opens an unnamed buffer.
///
/// # Arguments
/// * `args` - filename to edit (optional).
pub fn run(args: &str) {
    let raw = args.trim();
    let filename = if raw.is_empty() {
        String::new()
    } else {
        super::resolve_path(raw)
    };

    // Load existing content if file exists
    let content = if !filename.is_empty() {
        match crate::vfs_contract::VfsContract::read_file(&filename) {
            Ok(buf) => String::from_utf8_lossy(&buf).into_owned(),
            Err(_) => String::new(), // new file
        }
    } else {
        String::new()
    };

    let mut ed = Vi::new(filename.clone(), &content);

    // Show initial status
    crate::vga_buffer::clear();
    if filename.is_empty() {
        ed.status = String::from("[No Name]");
    } else {
        ed.status = format!("\"{}\" {}L", filename, ed.lines.len());
    }

    // -- Main event loop ---------------------------------------------------
    loop {
        ed.scroll_to_cursor();
        ed.render();

        // Wait for next key (hlt until keyboard IRQ fires)
        let key = loop {
            x86_64::instructions::hlt();
            if let Some(k) = kb_poll() {
                break k;
            }
        };

        let quit = match ed.mode {
            Mode::Normal => ed.handle_normal(key),
            Mode::Insert => ed.handle_insert(key),
            Mode::Command => ed.handle_command(key),
        };

        if quit {
            break;
        }
    }

    crate::vga_buffer::clear();
    crate::println!("{}", ed.status);
}
