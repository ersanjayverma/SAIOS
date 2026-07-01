use core::cell::Cell;
use bitflags::bitflags;

use hal::arch::x86_64::io::inb;

bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct KeyModifiers: u8 {
        const SHIFT = 0b0000_0001;
        const CTRL  = 0b0000_0010;
        const ALT   = 0b0000_0100;
    }
}

pub enum KeyEvent {
    Character(char),
    Enter,
    Backspace,
    Delete,
    Insert,
    Escape,
    Tab,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ShiftArrowUp,
    ShiftArrowDown,
    ShiftArrowLeft,
    ShiftArrowRight,
    CtrlArrowUp,
    CtrlArrowDown,
    CtrlArrowLeft,
    CtrlArrowRight,
    CtrlShiftArrowUp,
    CtrlShiftArrowDown,
    CtrlShiftArrowLeft,
    CtrlShiftArrowRight,
    FKey(u8),
    CtrlA,
    CtrlC,
    CtrlD,
    CtrlE,
    CtrlK,
    CtrlL,
    CtrlU,
    CtrlW,
}

struct Ps2Driver;

impl Ps2Driver {
    const DATA_PORT: u16 = 0x60;
    const STATUS_PORT: u16 = 0x64;

    pub const fn new() -> Self {
        Self
    }

    pub fn read_scancode(&self) -> Option<u8> {
        let status = inb(Self::STATUS_PORT);
        if (status & 0x01) == 0 {
            return None;
        }
        Some(inb(Self::DATA_PORT))
    }
}

pub struct KeyboardDriver {
    ps2: Ps2Driver,
    extended: Cell<bool>,
    ctrl_down: Cell<bool>,
    shift_down: Cell<bool>,
    caps_lock: Cell<bool>,
}

impl KeyboardDriver {
    pub const fn new() -> Self {
        Self {
            ps2: Ps2Driver::new(),
            extended: Cell::new(false),
            ctrl_down: Cell::new(false),
            shift_down: Cell::new(false),
            caps_lock: Cell::new(false),
        }
    }

    fn decode_char(&self, code: u8) -> Option<char> {
        let shift = self.shift_down.get();
        let upper = shift ^ self.caps_lock.get();

        match code {
            0x02 => Some(if shift { '!' } else { '1' }),
            0x03 => Some(if shift { '@' } else { '2' }),
            0x04 => Some(if shift { '#' } else { '3' }),
            0x05 => Some(if shift { '$' } else { '4' }),
            0x06 => Some(if shift { '%' } else { '5' }),
            0x07 => Some(if shift { '^' } else { '6' }),
            0x08 => Some(if shift { '&' } else { '7' }),
            0x09 => Some(if shift { '*' } else { '8' }),
            0x0A => Some(if shift { '(' } else { '9' }),
            0x0B => Some(if shift { ')' } else { '0' }),
            0x0C => Some(if shift { '_' } else { '-' }),
            0x0D => Some(if shift { '+' } else { '=' }),
            0x10 => Some(if upper { 'Q' } else { 'q' }),
            0x11 => Some(if upper { 'W' } else { 'w' }),
            0x12 => Some(if upper { 'E' } else { 'e' }),
            0x13 => Some(if upper { 'R' } else { 'r' }),
            0x14 => Some(if upper { 'T' } else { 't' }),
            0x15 => Some(if upper { 'Y' } else { 'y' }),
            0x16 => Some(if upper { 'U' } else { 'u' }),
            0x17 => Some(if upper { 'I' } else { 'i' }),
            0x18 => Some(if upper { 'O' } else { 'o' }),
            0x19 => Some(if upper { 'P' } else { 'p' }),
            0x1A => Some(if shift { '{' } else { '[' }),
            0x1B => Some(if shift { '}' } else { ']' }),
            0x1E => Some(if upper { 'A' } else { 'a' }),
            0x1F => Some(if upper { 'S' } else { 's' }),
            0x20 => Some(if upper { 'D' } else { 'd' }),
            0x21 => Some(if upper { 'F' } else { 'f' }),
            0x22 => Some(if upper { 'G' } else { 'g' }),
            0x23 => Some(if upper { 'H' } else { 'h' }),
            0x24 => Some(if upper { 'J' } else { 'j' }),
            0x25 => Some(if upper { 'K' } else { 'k' }),
            0x26 => Some(if upper { 'L' } else { 'l' }),
            0x27 => Some(if shift { ':' } else { ';' }),
            0x28 => Some(if shift { '"' } else { '\'' }),
            0x29 => Some(if shift { '~' } else { '`' }),
            0x2B => Some(if shift { '|' } else { '\\' }),
            0x2C => Some(if upper { 'Z' } else { 'z' }),
            0x2D => Some(if upper { 'X' } else { 'x' }),
            0x2E => Some(if upper { 'C' } else { 'c' }),
            0x2F => Some(if upper { 'V' } else { 'v' }),
            0x30 => Some(if upper { 'B' } else { 'b' }),
            0x31 => Some(if upper { 'N' } else { 'n' }),
            0x32 => Some(if upper { 'M' } else { 'm' }),
            0x33 => Some(if shift { '<' } else { ',' }),
            0x34 => Some(if shift { '>' } else { '.' }),
            0x35 => Some(if shift { '?' } else { '/' }),
            0x39 => Some(' '),
            _ => None,
        }
    }

    fn modifiers(&self) -> KeyModifiers {
        let mut mods = KeyModifiers::empty();
        if self.shift_down.get() {
            mods |= KeyModifiers::SHIFT;
        }
        if self.ctrl_down.get() {
            mods |= KeyModifiers::CTRL;
        }
        mods
    }

    fn apply_arrow_modifiers(&self, base: KeyEvent) -> KeyEvent {
        let mods = self.modifiers();
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

    pub fn poll_event(&self) -> Option<KeyEvent> {
        let scancode = self.ps2.read_scancode()?;

        if scancode == 0xE0 {
            self.extended.set(true);
            return None;
        }

        let extended = self.extended.replace(false);
        let released = (scancode & 0x80) != 0;
        let code = scancode & 0x7F;

        if !extended {
            if code == 0x1D {
                self.ctrl_down.set(!released);
                return None;
            }

            if code == 0x2A || code == 0x36 {
                self.shift_down.set(!released);
                return None;
            }

            if code == 0x3A {
                if !released {
                    self.caps_lock.set(!self.caps_lock.get());
                }
                return None;
            }

            if released {
                return None;
            }

            if self.ctrl_down.get() {
                return match code {
                    0x1E => Some(KeyEvent::CtrlA),
                    0x2E => Some(KeyEvent::CtrlC),
                    0x20 => Some(KeyEvent::CtrlD),
                    0x12 => Some(KeyEvent::CtrlE),
                    0x25 => Some(KeyEvent::CtrlK),
                    0x26 => Some(KeyEvent::CtrlL),
                    0x16 => Some(KeyEvent::CtrlU),
                    0x11 => Some(KeyEvent::CtrlW),
                    _ => None,
                };
            }

            return match code {
                0x01 => Some(KeyEvent::Escape),
                0x0E => Some(KeyEvent::Backspace),
                0x0F => Some(KeyEvent::Tab),
                0x1C => Some(KeyEvent::Enter),
                0x3B => Some(KeyEvent::FKey(1)),
                0x3C => Some(KeyEvent::FKey(2)),
                0x3D => Some(KeyEvent::FKey(3)),
                0x3E => Some(KeyEvent::FKey(4)),
                0x3F => Some(KeyEvent::FKey(5)),
                0x40 => Some(KeyEvent::FKey(6)),
                0x41 => Some(KeyEvent::FKey(7)),
                0x42 => Some(KeyEvent::FKey(8)),
                0x43 => Some(KeyEvent::FKey(9)),
                0x44 => Some(KeyEvent::FKey(10)),
                0x57 => Some(KeyEvent::FKey(11)),
                0x58 => Some(KeyEvent::FKey(12)),
                _ => self.decode_char(code).map(KeyEvent::Character),
            };
        }

        if released {
            return None;
        }

        match code {
            0x47 => Some(KeyEvent::Home),
            0x48 => Some(self.apply_arrow_modifiers(KeyEvent::ArrowUp)),
            0x49 => Some(KeyEvent::PageUp),
            0x4B => Some(self.apply_arrow_modifiers(KeyEvent::ArrowLeft)),
            0x4D => Some(self.apply_arrow_modifiers(KeyEvent::ArrowRight)),
            0x4F => Some(KeyEvent::End),
            0x50 => Some(self.apply_arrow_modifiers(KeyEvent::ArrowDown)),
            0x51 => Some(KeyEvent::PageDown),
            0x52 => Some(KeyEvent::Insert),
            0x53 => Some(KeyEvent::Delete),
            _ => None,
        }
    }
}
