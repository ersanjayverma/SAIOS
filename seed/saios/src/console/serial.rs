use core::fmt;
use smallvec::SmallVec;
use utf8parse::{Parser, Receiver};

use super::backend::ConsoleBackend;
use super::keyboard::{KeyEvent, KeyModifiers};
use hal::arch::x86_64::io::inb;
use hal::arch::x86_64::sync::StaticCell;

const COM1_DATA: u16 = 0x3F8;
const COM1_LINE_STATUS: u16 = COM1_DATA + 5;

enum EscState {
    None,
    Esc,
    Csi(SmallVec<[u8; 8]>),
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

fn csi_params_eq_dyn(params: &SmallVec<[u8; 8]>, expected: &[u8]) -> bool {
    params.as_slice() == expected
}

fn parse_csi_modifier(params: &SmallVec<[u8; 8]>) -> KeyModifiers {
    let bytes = params.as_slice();
    let mut last_value: u8 = 0;
    let mut current: u8 = 0;
    let mut saw_digit = false;

    for b in bytes {
        if b.is_ascii_digit() {
            saw_digit = true;
            current = current.saturating_mul(10).saturating_add(*b - b'0');
        } else if *b == b';' {
            if saw_digit {
                last_value = current;
            }
            current = 0;
            saw_digit = false;
        }
    }

    if saw_digit {
        last_value = current;
    }

    let mut mods = KeyModifiers::empty();
    match last_value {
        2 => mods |= KeyModifiers::SHIFT,
        5 => mods |= KeyModifiers::CTRL,
        6 => {
            mods |= KeyModifiers::CTRL;
            mods |= KeyModifiers::SHIFT;
        }
        _ => {}
    }
    mods
}

fn apply_arrow_modifiers(base: KeyEvent, mods: KeyModifiers) -> KeyEvent {
    let shift = mods.contains(KeyModifiers::SHIFT);
    let ctrl = mods.contains(KeyModifiers::CTRL);
    match base {
        KeyEvent::ArrowUp if shift && ctrl => KeyEvent::CtrlShiftArrowUp,
        KeyEvent::ArrowDown if shift && ctrl => KeyEvent::CtrlShiftArrowDown,
        KeyEvent::ArrowLeft if shift && ctrl => KeyEvent::CtrlShiftArrowLeft,
        KeyEvent::ArrowRight if shift && ctrl => KeyEvent::CtrlShiftArrowRight,
        KeyEvent::ArrowUp if ctrl => KeyEvent::CtrlArrowUp,
        KeyEvent::ArrowDown if ctrl => KeyEvent::CtrlArrowDown,
        KeyEvent::ArrowLeft if ctrl => KeyEvent::CtrlArrowLeft,
        KeyEvent::ArrowRight if ctrl => KeyEvent::CtrlArrowRight,
        KeyEvent::ArrowUp if shift => KeyEvent::ShiftArrowUp,
        KeyEvent::ArrowDown if shift => KeyEvent::ShiftArrowDown,
        KeyEvent::ArrowLeft if shift => KeyEvent::ShiftArrowLeft,
        KeyEvent::ArrowRight if shift => KeyEvent::ShiftArrowRight,
        _ => base,
    }
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
                *state = EscState::Csi(SmallVec::new());
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
                b'P' => Some(KeyEvent::FKey(1)),
                b'Q' => Some(KeyEvent::FKey(2)),
                b'R' => Some(KeyEvent::FKey(3)),
                b'S' => Some(KeyEvent::FKey(4)),
                _ => None,
            };
        }
        EscState::Csi(params) => {
            if params.len() >= 8 {
                *state = EscState::None;
                return None;
            }

            params.push(byte);

            // Final CSI byte range per ANSI/VT sequences.
            if !(0x40..=0x7E).contains(&byte) {
                return None;
            }

            let mut param_bytes: SmallVec<[u8; 8]> = SmallVec::new();
            let final_byte = params.last().copied().unwrap_or(byte);
            for b in params.iter().take(params.len().saturating_sub(1)) {
                if param_bytes.len() < 8 {
                    param_bytes.push(*b);
                }
            }

            let mods = parse_csi_modifier(&param_bytes);

            let event = match final_byte {
                b'A' => Some(apply_arrow_modifiers(KeyEvent::ArrowUp, mods)),
                b'B' => Some(apply_arrow_modifiers(KeyEvent::ArrowDown, mods)),
                b'C' => Some(apply_arrow_modifiers(KeyEvent::ArrowRight, mods)),
                b'D' => Some(apply_arrow_modifiers(KeyEvent::ArrowLeft, mods)),
                b'H' => Some(KeyEvent::Home),
                b'F' => Some(KeyEvent::End),
                b'~' => {
                    if csi_params_eq_dyn(&param_bytes, b"2") {
                        Some(KeyEvent::Insert)
                    } else if csi_params_eq_dyn(&param_bytes, b"5") {
                        Some(KeyEvent::PageUp)
                    } else if csi_params_eq_dyn(&param_bytes, b"6") {
                        Some(KeyEvent::PageDown)
                    } else if csi_params_eq_dyn(&param_bytes, b"15") {
                        Some(KeyEvent::FKey(5))
                    } else if csi_params_eq_dyn(&param_bytes, b"17") {
                        Some(KeyEvent::FKey(6))
                    } else if csi_params_eq_dyn(&param_bytes, b"18") {
                        Some(KeyEvent::FKey(7))
                    } else if csi_params_eq_dyn(&param_bytes, b"19") {
                        Some(KeyEvent::FKey(8))
                    } else if csi_params_eq_dyn(&param_bytes, b"20") {
                        Some(KeyEvent::FKey(9))
                    } else if csi_params_eq_dyn(&param_bytes, b"21") {
                        Some(KeyEvent::FKey(10))
                    } else if csi_params_eq_dyn(&param_bytes, b"23") {
                        Some(KeyEvent::FKey(11))
                    } else if csi_params_eq_dyn(&param_bytes, b"24") {
                        Some(KeyEvent::FKey(12))
                    } else if csi_params_eq_dyn(&param_bytes, b"1;2") {
                        Some(apply_arrow_modifiers(KeyEvent::ArrowUp, KeyModifiers::SHIFT))
                    } else if csi_params_eq_dyn(&param_bytes, b"1;5") {
                        Some(apply_arrow_modifiers(KeyEvent::ArrowUp, KeyModifiers::CTRL))
                    } else if csi_params_eq_dyn(&param_bytes, b"1;6") {
                        Some(apply_arrow_modifiers(KeyEvent::ArrowUp, KeyModifiers::CTRL | KeyModifiers::SHIFT))
                    } else if csi_params_eq_dyn(&param_bytes, b"3") {
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
        match c {
            '\n' => {
                hal::arch::x86_64::console::_print(format_args!("\r\n"));
            }
            _ => {
                hal::arch::x86_64::console::_print(format_args!("{}", c));
            }
        }
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
        match c {
            '\n' => {
                // Keep serial terminals aligned by using CRLF line endings.
                hal::arch::x86_64::console::_print(format_args!("\r\n"));
            }
            _ => {
                hal::arch::x86_64::console::_print(format_args!("{}", c));
            }
        }
    }

    fn clear(&mut self) {
        // ANSI clear screen + home cursor (works in most serial terminals).
        Self::write_escape(format_args!("\x1b[2J\x1b[H"));
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        // ANSI cursor is 1-based.
        Self::write_escape(format_args!("\x1b[{};{}H", y + 1, x + 1));
    }

    fn scroll_up(&mut self, rows: usize) -> bool {
        let rows = core::cmp::max(1, rows);
        Self::write_escape(format_args!("\x1b[{}S", rows));
        true
    }
}
