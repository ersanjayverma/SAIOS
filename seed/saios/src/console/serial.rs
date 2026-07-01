use core::fmt;

use super::backend::ConsoleBackend;
use super::keyboard::KeyEvent;
use hal::arch::x86_64::io::inb;
use hal::arch::x86_64::sync::StaticCell;

const COM1_DATA: u16 = 0x3F8;
const COM1_LINE_STATUS: u16 = COM1_DATA + 5;

enum EscState {
    None,
    Esc,
    Csi {
        buf: [u8; 8],
        len: usize,
    },
    Ss3,
}

struct Utf8Decoder {
    buf: [u8; 4],
    len: usize,
    needed: usize,
}

impl Utf8Decoder {
    const fn new() -> Self {
        Self {
            buf: [0; 4],
            len: 0,
            needed: 0,
        }
    }

    fn reset(&mut self) {
        self.len = 0;
        self.needed = 0;
    }

    fn finish_or_replacement(&mut self) -> char {
        let out = core::str::from_utf8(&self.buf[..self.needed])
            .ok()
            .and_then(|s| s.chars().next())
            .unwrap_or('\u{FFFD}');
        self.reset();
        out
    }

    fn feed(&mut self, byte: u8) -> Option<char> {
        if self.len == 0 {
            if byte < 0x80 {
                return Some(byte as char);
            }

            let needed = if (byte & 0xE0) == 0xC0 {
                2
            } else if (byte & 0xF0) == 0xE0 {
                3
            } else if (byte & 0xF8) == 0xF0 {
                4
            } else {
                return Some('\u{FFFD}');
            };

            self.buf[0] = byte;
            self.len = 1;
            self.needed = needed;
            return None;
        }

        if (byte & 0xC0) != 0x80 {
            self.reset();
            return Some('\u{FFFD}');
        }

        self.buf[self.len] = byte;
        self.len += 1;

        if self.len == self.needed {
            return Some(self.finish_or_replacement());
        }

        None
    }
}

static UTF8_DECODER: StaticCell<Utf8Decoder> = StaticCell::new(Utf8Decoder::new());
static ESC_STATE: StaticCell<EscState> = StaticCell::new(EscState::None);

fn poll_byte() -> Option<u8> {
    if (inb(COM1_LINE_STATUS) & 0x01) == 0 {
        return None;
    }
    Some(inb(COM1_DATA))
}

fn csi_params_eq(buf: &[u8; 8], len: usize, expected: &[u8]) -> bool {
    len == expected.len() && &buf[..len] == expected
}

pub fn poll_input_event() -> Option<KeyEvent> {
    let byte = poll_byte()?;
    // SAFETY: single-core early kernel context.
    let state = unsafe { &mut *ESC_STATE.get() };

    match state {
        EscState::None => {
            if byte == 0x1B {
                *state = EscState::Esc;
                return None;
            }
        }
        EscState::Esc => {
            if byte == b'[' {
                *state = EscState::Csi {
                    buf: [0; 8],
                    len: 0,
                };
                return None;
            }

            if byte == b'O' {
                *state = EscState::Ss3;
                return None;
            }

            *state = EscState::None;
            return Some(KeyEvent::Escape);
        }
        EscState::Ss3 => {
            *state = EscState::None;
            return match byte {
                b'H' => Some(KeyEvent::Home),
                b'F' => Some(KeyEvent::End),
                b'A' => Some(KeyEvent::ArrowUp),
                b'B' => Some(KeyEvent::ArrowDown),
                b'C' => Some(KeyEvent::ArrowRight),
                b'D' => Some(KeyEvent::ArrowLeft),
                _ => None,
            };
        }
        EscState::Csi { buf, len } => {
            if *len >= buf.len() {
                *state = EscState::None;
                return None;
            }

            buf[*len] = byte;
            *len += 1;

            // Final CSI byte range per ANSI/VT sequences.
            if !(0x40..=0x7E).contains(&byte) {
                return None;
            }

            let final_byte = byte;
            let param_len = *len - 1;

            let event = match final_byte {
                b'A' => Some(KeyEvent::ArrowUp),
                b'B' => Some(KeyEvent::ArrowDown),
                b'C' => Some(KeyEvent::ArrowRight),
                b'D' => Some(KeyEvent::ArrowLeft),
                b'H' => Some(KeyEvent::Home),
                b'F' => Some(KeyEvent::End),
                b'~' => {
                    if csi_params_eq(buf, param_len, b"3") {
                        Some(KeyEvent::Delete)
                    } else if csi_params_eq(buf, param_len, b"1") || csi_params_eq(buf, param_len, b"7") {
                        Some(KeyEvent::Home)
                    } else if csi_params_eq(buf, param_len, b"4") || csi_params_eq(buf, param_len, b"8") {
                        Some(KeyEvent::End)
                    } else {
                        None
                    }
                }
                _ => None,
            };

            *state = EscState::None;
            return event;
        }
    }

    // SAFETY: single-core early kernel context.
    let ch = unsafe { (*UTF8_DECODER.get()).feed(byte) }?;

    match ch {
        '\r' | '\n' => Some(KeyEvent::Enter),
        '\u{08}' | '\u{7f}' => Some(KeyEvent::Backspace),
        '\t' => Some(KeyEvent::Tab),
        '\u{01}' => Some(KeyEvent::CtrlA),
        '\u{03}' => Some(KeyEvent::CtrlC),
        '\u{04}' => Some(KeyEvent::CtrlD),
        '\u{05}' => Some(KeyEvent::CtrlE),
        '\u{0b}' => Some(KeyEvent::CtrlK),
        '\u{0c}' => Some(KeyEvent::CtrlL),
        '\u{15}' => Some(KeyEvent::CtrlU),
        '\u{17}' => Some(KeyEvent::CtrlW),
        c if !c.is_control() => Some(KeyEvent::Character(c)),
        _ => None,
    }
}

pub struct SerialConsole;

impl SerialConsole {
    pub const fn new() -> Self {
        Self
    }

    pub fn init() {
        hal::arch::x86_64::console::init_serial();
    }

    #[inline(always)]
    pub fn emergency_put_char(c: char) {
        hal::arch::x86_64::console::_print(format_args!("{}", c));
    }

    pub fn emergency_write_str(s: &str) {
        for c in s.chars() {
            Self::emergency_put_char(c);
        }
    }

    fn write_escape(args: fmt::Arguments) {
        hal::arch::x86_64::console::_print(args);
    }
}

impl ConsoleBackend for SerialConsole {
    fn put_char(&mut self, c: char) {
        hal::arch::x86_64::console::_print(format_args!("{}", c));
    }

    fn clear(&mut self) {
        // ANSI clear screen + home cursor (works in most serial terminals).
        Self::write_escape(format_args!("\x1b[2J\x1b[H"));
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        // ANSI cursor is 1-based.
        Self::write_escape(format_args!("\x1b[{};{}H", y + 1, x + 1));
    }
}
