//! nano — simple modeless text editor for SAIOS.
//!
//! nano is easier than vi: there are no modes. You type and text appears.
//! Control shortcuts handle saving and navigation.
//!
//! SHORTCUTS (displayed in the bottom two rows):
//!   Ctrl+X   — Exit (prompts to save if modified)
//!   Ctrl+S   — Save file
//!   Ctrl+O   — Write Out (save with filename prompt)
//!   Ctrl+K   — Cut current line
//!   Ctrl+U   — Paste (uncut)
//!   Ctrl+G   — Help (this screen)
//!   Ctrl+W   — Search (find text)
//!   Arrow keys — Navigate
//!   Home/End   — Start/end of line (mapped to Ctrl+A / Ctrl+E)
//!
//! Inspired by GNU nano but implemented from scratch for SAIOS.

use crate::driver::keyboard::{KeyEvent, poll as kb_poll};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// How many rows are visible for text (screen height minus header and footer).
const TEXT_ROWS: usize = 22;
/// Screen width in columns.
const COLS: usize = 80;

/// Complete state of the nano editor.
struct Nano {
    /// Document lines, one `String` per line.
    lines: Vec<String>,
    /// Cursor row (0-based index into `lines`).
    row: usize,
    /// Cursor column (0-based byte offset).
    col: usize,
    /// Viewport scroll offset (first visible line index).
    top: usize,
    /// Path being edited.
    filename: String,
    /// True if the buffer has unsaved changes.
    modified: bool,
    /// Status message shown in the header bar.
    status: String,
    /// Clipboard: last line cut with Ctrl+K.
    clipboard: Option<String>,
    /// Search term (Ctrl+W).
    search: String,
    /// Ctrl+X pressed once on a modified buffer — a second exits without saving.
    pending_exit: bool,
}

impl Nano {
    /// Create a new nano instance with the given content.
    fn new(filename: String, content: &str) -> Self {
        let lines: Vec<String> = if content.is_empty() {
            alloc::vec![String::new()]
        } else {
            content.lines().map(|l| l.to_string()).collect()
        };
        let total = lines.len();
        Self {
            lines,
            row: 0,
            col: 0,
            top: 0,
            filename,
            modified: false,
            status: alloc::format!("{} lines", total),
            clipboard: None,
            search: String::new(),
            pending_exit: false,
        }
    }

    /// Clamp cursor column to the length of the current line.
    fn clamp_col(&mut self) {
        let len = self.lines[self.row].len();
        if self.col > len {
            self.col = len;
        }
    }

    /// Scroll viewport to keep cursor in view.
    fn scroll(&mut self) {
        if self.row < self.top {
            self.top = self.row;
        }
        if self.row >= self.top + TEXT_ROWS {
            self.top = self.row + 1 - TEXT_ROWS;
        }
    }

    /// Save the current buffer to `self.filename`.
    fn save(&mut self) -> bool {
        if self.filename.is_empty() {
            self.status = String::from("No filename — use Ctrl+O to write out");
            return false;
        }
        let content: String = self.lines.join("\n");
        match save_vfs(&self.filename, content.as_bytes()) {
            Ok(()) => {
                self.modified = false;
                self.status = format!("Wrote {} lines to {}", self.lines.len(), self.filename);
                true
            }
            Err(e) => {
                self.status = format!("Error: {}", e);
                false
            }
        }
    }

    /// Handle one key press. Returns true when the editor should exit.
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Any key other than a repeated Ctrl+X clears the pending-exit arm.
        let was_pending = self.pending_exit;
        self.pending_exit = false;
        match key {
            // -- Control shortcuts -----------------------------------------
            // Ctrl+X  — exit
            KeyEvent::Char('\x18') => {
                if self.modified && !was_pending {
                    self.pending_exit = true;
                    self.status = String::from(
                        "Unsaved changes! Ctrl+S to save, Ctrl+X again to discard & exit",
                    );
                    return false;
                }
                return true; // unmodified, or second Ctrl+X → exit
            }
            // Ctrl+S  — save
            KeyEvent::Char('\x13') => {
                self.save();
            }
            // Ctrl+K  — cut line
            KeyEvent::Char('\x0B') => {
                self.clipboard = Some(self.lines[self.row].clone());
                if self.lines.len() > 1 {
                    self.lines.remove(self.row);
                    if self.row >= self.lines.len() {
                        self.row = self.lines.len() - 1;
                    }
                } else {
                    self.lines[0] = String::new();
                }
                self.modified = true;
                self.clamp_col();
            }
            // Ctrl+U  — paste (uncut)
            KeyEvent::Char('\x15') => {
                if let Some(ref clip) = self.clipboard.clone() {
                    let pos = (self.row + 1).min(self.lines.len());
                    self.lines.insert(pos, clip.clone());
                    self.row = pos;
                    self.modified = true;
                }
            }
            // Ctrl+W  — search
            KeyEvent::Char('\x17') => {
                // TODO: mini search bar in status line
                self.status = String::from("Search: (type in status bar — not yet implemented)");
            }
            // Ctrl+G  — help
            KeyEvent::Char('\x07') => {
                self.status = String::from("^X Exit  ^S Save  ^K Cut  ^U Paste  ^W Search");
            }

            // -- Navigation ------------------------------------------------
            KeyEvent::Up => {
                if self.row > 0 {
                    self.row -= 1;
                    self.clamp_col();
                }
            }
            KeyEvent::Down => {
                if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.clamp_col();
                }
            }
            KeyEvent::Left => {
                if self.col > 0 {
                    self.col -= 1;
                } else if self.row > 0 {
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                }
            }
            KeyEvent::Right => {
                let len = self.lines[self.row].len();
                if self.col < len {
                    self.col += 1;
                } else if self.row + 1 < self.lines.len() {
                    self.row += 1;
                    self.col = 0;
                }
            }

            // -- Editing ---------------------------------------------------
            KeyEvent::Enter => {
                let rest = self.lines[self.row].split_off(self.col);
                self.lines.insert(self.row + 1, rest);
                self.row += 1;
                self.col = 0;
                self.modified = true;
            }
            // Backspace, however it arrives (dedicated key, ^H 0x08, or DEL 0x7f).
            KeyEvent::Backspace | KeyEvent::Char('\x08') | KeyEvent::Char('\x7f') => {
                if self.col > 0 {
                    self.col -= 1;
                    self.lines[self.row].remove(self.col);
                } else if self.row > 0 {
                    let cur = self.lines.remove(self.row);
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                    self.lines[self.row].push_str(&cur);
                }
                self.modified = true;
            }
            KeyEvent::Delete => {
                // Forward delete: remove the char at the cursor, or join the
                // next line when at end of line.
                let len = self.lines[self.row].len();
                if self.col < len {
                    self.lines[self.row].remove(self.col);
                    self.modified = true;
                } else if self.row + 1 < self.lines.len() {
                    let next = self.lines.remove(self.row + 1);
                    self.lines[self.row].push_str(&next);
                    self.modified = true;
                }
            }
            KeyEvent::Char(c) if c >= ' ' && c != '\x7f' => {
                self.lines[self.row].insert(self.col, c);
                self.col += 1;
                self.modified = true;
            }
            _ => {}
        }
        false
    }

    /// Render the complete screen.
    fn render(&self) {
        crate::vga_buffer::clear();

        // -- Header bar ----------------------------------------------------
        let title = format!(
            " GNU nano 7.2  {}  {}",
            if self.filename.is_empty() {
                "New Buffer"
            } else {
                &self.filename
            },
            if self.modified { "Modified" } else { "" }
        );
        crate::println!("{:<width$}", title, width = COLS);
        crate::println!("{}", "-".repeat(COLS));

        // -- Document text -------------------------------------------------
        for screen_row in 0..TEXT_ROWS {
            let doc_row = self.top + screen_row;
            let line = if doc_row < self.lines.len() {
                let l = &self.lines[doc_row];
                if l.len() > COLS {
                    &l[..COLS]
                } else {
                    l.as_str()
                }
            } else {
                ""
            };
            crate::println!("{:<width$}", line, width = COLS);
        }

        // -- Status bar ----------------------------------------------------
        crate::println!("{}", "-".repeat(COLS));
        let loc = format!("Ln {}, Col {}", self.row + 1, self.col + 1);
        crate::println!("{:<40}{:>40}", self.status, loc);

        // -- Shortcut bar --------------------------------------------------
        crate::println!("^X Exit  ^S Save  ^K Cut Line  ^U Paste  ^W Search  ^G Help");

        // Place the visible cursor at the editing position.  The text area
        // starts at screen row 2 (after the title + separator); the column is
        // the cursor's byte offset (ASCII), clamped to the screen width.
        let scr_row = (self.row - self.top) + 2;
        let scr_col = self.col.min(COLS - 1);
        crate::vga_buffer::move_cursor(scr_col, scr_row);
    }
}

/// Save bytes to a VFS path (overwrite if exists).
fn save_vfs(path: &str, data: &[u8]) -> Result<(), &'static str> {
    crate::vfs_contract::VfsContract::write_file(path, data, 0o644).map_err(|_| "write failed")
}

/// Open nano. If args is empty, opens a new unnamed buffer.
///
/// # Arguments
/// * `args` — filename to edit (optional).
pub fn run(args: &str) {
    // Resolve the filename against the current working directory.  nano was
    // using the raw (relative) arg with vfs::resolve, which only matches
    // absolute paths — so `nano file.txt` from any non-root dir opened a blank
    // buffer and then saved to the wrong place.
    let raw = args.trim();
    let filename = if raw.is_empty() {
        String::new()
    } else {
        super::resolve_path(raw)
    };

    // Load existing file content, or start empty
    let content = if !filename.is_empty() {
        match crate::vfs_contract::VfsContract::read_file(&filename) {
            Ok(buf) => String::from_utf8_lossy(&buf).into_owned(),
            Err(_) => String::new(),
        }
    } else {
        String::new()
    };

    let mut ed = Nano::new(filename, &content);

    crate::vga_buffer::clear();

    // -- Main event loop ---------------------------------------------------
    loop {
        ed.scroll();
        ed.render();

        let key = loop {
            x86_64::instructions::hlt();
            if let Some(k) = kb_poll() {
                break k;
            }
        };

        if ed.handle_key(key) {
            break;
        }
    }

    crate::vga_buffer::clear();
    crate::println!("{}", ed.status);
}
