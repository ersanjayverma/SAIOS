use core::fmt;
use arrayvec::ArrayVec;
use utf8parse::{Parser, Receiver};

use super::backend::ConsoleBackend;
use super::keyboard::KeyEvent;
use hal::arch::x86_64::io::inb;
use hal::arch::x86_64::sync::StaticCell;

const COM1_DATA: u16 = 0x3F8;
const COM1_LINE_STATUS: u16 = COM1_DATA + 5;

enum EscState {
    None,
    Esc,
    Csi(ArrayVec<u8, 8>),
    Ss3,
}

struct Utf8Receiver {
    codepoint: Option<char>,
    invalid: bool,
}

impl Utf8Receiver {
    const fn new() -> Self {
        Self {
            codepoint: None,
            invalid: false,
        }
    }

    fn reset(&mut self) {
        self.codepoint = None;
        self.invalid = false;
    }
}

impl Receiver for Utf8Receiver {
    fn codepoint(&mut self, c: char) {
        self.codepoint = Some(c);
    }

    fn invalid_sequence(&mut self) {
        self.invalid = true;
    }
}

struct Utf8Decoder {
    parser: Option<Parser>,
    receiver: Utf8Receiver,
}

impl Utf8Decoder {
    const fn new() -> Self {
        Self {
            parser: None,
            receiver: Utf8Receiver::new(),
        }
    }

    fn feed(&mut self, byte: u8) -> Option<char> {
        if self.parser.is_none() {
            self.parser = Some(Parser::new());
        }

        self.receiver.reset();
        if let Some(parser) = self.parser.as_mut() {
            parser.advance(&mut self.receiver, byte);
        }

        if self.receiver.invalid {
            return Some('\u{FFFD}');
        }

        self.receiver.codepoint
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

fn csi_params_eq_dyn(params: &ArrayVec<u8, 8>, expected: &[u8]) -> bool {
    params.as_slice() == expected
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
                *state = EscState::Csi(ArrayVec::new());
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
        EscState::Csi(params) => {
            if params.is_full() {
                *state = EscState::None;
                return None;
            }

            if params.try_push(byte).is_err() {
                *state = EscState::None;
                return None;
            }

            // Final CSI byte range per ANSI/VT sequences.
            if !(0x40..=0x7E).contains(&byte) {
                return None;
            }

            let mut param_bytes: ArrayVec<u8, 8> = ArrayVec::new();
            let final_byte = params.last().copied().unwrap_or(byte);
            for b in params.iter().take(params.len().saturating_sub(1)) {
                let _ = param_bytes.try_push(*b);
            }

            let event = match final_byte {
                b'A' => Some(KeyEvent::ArrowUp),
                b'B' => Some(KeyEvent::ArrowDown),
                b'C' => Some(KeyEvent::ArrowRight),
                b'D' => Some(KeyEvent::ArrowLeft),
                b'H' => Some(KeyEvent::Home),
                b'F' => Some(KeyEvent::End),
                b'~' => {
                    if csi_params_eq_dyn(&param_bytes, b"3") {
                        Some(KeyEvent::Delete)
                    } else if csi_params_eq_dyn(&param_bytes, b"1") || csi_params_eq_dyn(&param_bytes, b"7") {
                        Some(KeyEvent::Home)
                    } else if csi_params_eq_dyn(&param_bytes, b"4") || csi_params_eq_dyn(&param_bytes, b"8") {
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
