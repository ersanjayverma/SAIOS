use hal::arch::x86_64::io::inb;

pub enum KeyEvent {
    Character(char),
    Enter,
    Backspace,
    Escape,
    Tab,
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

struct ScancodeDecoder;

impl ScancodeDecoder {
    pub const fn new() -> Self {
        Self
    }

    pub fn decode(&self, scancode: u8) -> Option<KeyEvent> {
        if (scancode & 0x80) != 0 {
            return None;
        }

        match scancode {
            0x01 => Some(KeyEvent::Escape),
            0x0E => Some(KeyEvent::Backspace),
            0x0F => Some(KeyEvent::Tab),
            0x1C => Some(KeyEvent::Enter),
            0x02 => Some(KeyEvent::Character('1')),
            0x03 => Some(KeyEvent::Character('2')),
            0x04 => Some(KeyEvent::Character('3')),
            0x05 => Some(KeyEvent::Character('4')),
            0x06 => Some(KeyEvent::Character('5')),
            0x07 => Some(KeyEvent::Character('6')),
            0x08 => Some(KeyEvent::Character('7')),
            0x09 => Some(KeyEvent::Character('8')),
            0x0A => Some(KeyEvent::Character('9')),
            0x0B => Some(KeyEvent::Character('0')),
            0x0C => Some(KeyEvent::Character('-')),
            0x0D => Some(KeyEvent::Character('=')),
            0x10 => Some(KeyEvent::Character('q')),
            0x11 => Some(KeyEvent::Character('w')),
            0x12 => Some(KeyEvent::Character('e')),
            0x13 => Some(KeyEvent::Character('r')),
            0x14 => Some(KeyEvent::Character('t')),
            0x15 => Some(KeyEvent::Character('y')),
            0x16 => Some(KeyEvent::Character('u')),
            0x17 => Some(KeyEvent::Character('i')),
            0x18 => Some(KeyEvent::Character('o')),
            0x19 => Some(KeyEvent::Character('p')),
            0x1A => Some(KeyEvent::Character('[')),
            0x1B => Some(KeyEvent::Character(']')),
            0x1E => Some(KeyEvent::Character('a')),
            0x1F => Some(KeyEvent::Character('s')),
            0x20 => Some(KeyEvent::Character('d')),
            0x21 => Some(KeyEvent::Character('f')),
            0x22 => Some(KeyEvent::Character('g')),
            0x23 => Some(KeyEvent::Character('h')),
            0x24 => Some(KeyEvent::Character('j')),
            0x25 => Some(KeyEvent::Character('k')),
            0x26 => Some(KeyEvent::Character('l')),
            0x27 => Some(KeyEvent::Character(';')),
            0x28 => Some(KeyEvent::Character('\'')),
            0x29 => Some(KeyEvent::Character('`')),
            0x2B => Some(KeyEvent::Character('\\')),
            0x2C => Some(KeyEvent::Character('z')),
            0x2D => Some(KeyEvent::Character('x')),
            0x2E => Some(KeyEvent::Character('c')),
            0x2F => Some(KeyEvent::Character('v')),
            0x30 => Some(KeyEvent::Character('b')),
            0x31 => Some(KeyEvent::Character('n')),
            0x32 => Some(KeyEvent::Character('m')),
            0x33 => Some(KeyEvent::Character(',')),
            0x34 => Some(KeyEvent::Character('.')),
            0x35 => Some(KeyEvent::Character('/')),
            0x39 => Some(KeyEvent::Character(' ')),
            _ => None,
        }
    }
}

pub struct KeyboardDriver {
    ps2: Ps2Driver,
    decoder: ScancodeDecoder,
}

impl KeyboardDriver {
    pub const fn new() -> Self {
        Self {
            ps2: Ps2Driver::new(),
            decoder: ScancodeDecoder::new(),
        }
    }

    pub fn poll_event(&self) -> Option<KeyEvent> {
        let scancode = self.ps2.read_scancode()?;
        self.decoder.decode(scancode)
    }
}
