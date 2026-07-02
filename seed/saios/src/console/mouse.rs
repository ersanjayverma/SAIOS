use core::cell::Cell;

use hal::arch::x86_64::io::{inb, outb};

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Default)]
pub struct MouseButtons {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
pub enum MouseEvent {
    Move {
        dx: i16,
        dy: i16,
        buttons: MouseButtons,
    },
    Wheel {
        delta: i8,
        buttons: MouseButtons,
    },
}

struct Ps2Mouse;

impl Ps2Mouse {
    const DATA_PORT: u16 = 0x60;
    const STATUS_PORT: u16 = 0x64;
    const COMMAND_PORT: u16 = 0x64;

    const STATUS_OUTPUT_READY: u8 = 0x01;
    const STATUS_AUX_DATA: u8 = 0x20;

    const CMD_ENABLE_AUX: u8 = 0xA8;
    const CMD_READ_CONFIG: u8 = 0x20;
    const CMD_WRITE_CONFIG: u8 = 0x60;
    const CMD_WRITE_MOUSE: u8 = 0xD4;

    const MOUSE_ACK: u8 = 0xFA;

    const MOUSE_SET_DEFAULTS: u8 = 0xF6;
    const MOUSE_ENABLE_DATA_REPORTING: u8 = 0xF4;
    const MOUSE_SET_SAMPLE_RATE: u8 = 0xF3;
    const MOUSE_GET_DEVICE_ID: u8 = 0xF2;

    const CONFIG_ENABLE_IRQ12: u8 = 0x02;
    const CONFIG_DISABLE_AUX_CLOCK: u8 = 0x20;

    const WAIT_ITERS: usize = 100_000;

    fn wait_input_clear() -> bool {
        for _ in 0..Self::WAIT_ITERS {
            if (inb(Self::STATUS_PORT) & 0x02) == 0 {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    fn wait_output_ready() -> bool {
        for _ in 0..Self::WAIT_ITERS {
            if (inb(Self::STATUS_PORT) & Self::STATUS_OUTPUT_READY) != 0 {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    fn controller_write(cmd: u8) {
        if Self::wait_input_clear() {
            outb(Self::COMMAND_PORT, cmd);
        }
    }

    fn controller_read() -> Option<u8> {
        if Self::wait_output_ready() {
            Some(inb(Self::DATA_PORT))
        } else {
            None
        }
    }

    fn mouse_write(data: u8) -> bool {
        if !Self::wait_input_clear() {
            return false;
        }
        outb(Self::COMMAND_PORT, Self::CMD_WRITE_MOUSE);

        if !Self::wait_input_clear() {
            return false;
        }
        outb(Self::DATA_PORT, data);

        matches!(Self::controller_read(), Some(Self::MOUSE_ACK))
    }

    fn set_sample_rate(rate: u8) -> bool {
        Self::mouse_write(Self::MOUSE_SET_SAMPLE_RATE) && Self::mouse_write(rate)
    }

    fn read_device_id() -> Option<u8> {
        if !Self::mouse_write(Self::MOUSE_GET_DEVICE_ID) {
            return None;
        }
        Self::controller_read()
    }

    fn status() -> u8 {
        inb(Self::STATUS_PORT)
    }

    fn read_aux_byte() -> Option<u8> {
        let status = Self::status();
        if (status & Self::STATUS_OUTPUT_READY) == 0 {
            return None;
        }
        if (status & Self::STATUS_AUX_DATA) == 0 {
            return None;
        }
        Some(inb(Self::DATA_PORT))
    }
}

pub struct MouseDriver {
    packet: [u8; 4],
    packet_index: Cell<usize>,
    packet_size: Cell<usize>,
    initialized: Cell<bool>,
}

impl MouseDriver {
    pub const fn new() -> Self {
        Self {
            packet: [0; 4],
            packet_index: Cell::new(0),
            packet_size: Cell::new(3),
            initialized: Cell::new(false),
        }
    }

    fn parse_buttons(flags: u8) -> MouseButtons {
        MouseButtons {
            left: (flags & 0x01) != 0,
            right: (flags & 0x02) != 0,
            middle: (flags & 0x04) != 0,
        }
    }

    fn sign_extend(value: u8, sign: bool) -> i16 {
        if sign {
            i16::from(value) - 256
        } else {
            i16::from(value)
        }
    }

    fn parse_packet(&self) -> Option<MouseEvent> {
        let flags = self.packet[0];

        // Bit 3 is always set in valid packet headers.
        if (flags & 0x08) == 0 {
            return None;
        }

        let buttons = Self::parse_buttons(flags);
        let dx = Self::sign_extend(self.packet[1], (flags & 0x10) != 0);
        let dy = -Self::sign_extend(self.packet[2], (flags & 0x20) != 0);

        if self.packet_size.get() == 4 {
            let delta = self.packet[3] as i8;
            if delta != 0 {
                return Some(MouseEvent::Wheel { delta, buttons });
            }
        }

        if dx != 0 || dy != 0 {
            return Some(MouseEvent::Move { dx, dy, buttons });
        }

        None
    }

    pub fn init(&self) {
        if self.initialized.get() {
            return;
        }

        Ps2Mouse::controller_write(Ps2Mouse::CMD_ENABLE_AUX);

        Ps2Mouse::controller_write(Ps2Mouse::CMD_READ_CONFIG);
        let mut config = Ps2Mouse::controller_read().unwrap_or(0);
        config |= Ps2Mouse::CONFIG_ENABLE_IRQ12;
        config &= !Ps2Mouse::CONFIG_DISABLE_AUX_CLOCK;
        Ps2Mouse::controller_write(Ps2Mouse::CMD_WRITE_CONFIG);
        if Ps2Mouse::wait_input_clear() {
            outb(Ps2Mouse::DATA_PORT, config);
        }

        let _ = Ps2Mouse::mouse_write(Ps2Mouse::MOUSE_SET_DEFAULTS);

        // Intellimouse wheel enable sequence.
        let _ = Ps2Mouse::set_sample_rate(200);
        let _ = Ps2Mouse::set_sample_rate(100);
        let _ = Ps2Mouse::set_sample_rate(80);

        if let Some(device_id) = Ps2Mouse::read_device_id() {
            self.packet_size.set(if device_id == 3 || device_id == 4 { 4 } else { 3 });
        }

        let _ = Ps2Mouse::mouse_write(Ps2Mouse::MOUSE_ENABLE_DATA_REPORTING);
        self.packet_index.set(0);
        self.initialized.set(true);
    }

    pub fn poll_event(&mut self) -> Option<MouseEvent> {
        if !self.initialized.get() {
            return None;
        }

        let byte = Ps2Mouse::read_aux_byte()?;
        let idx = self.packet_index.get();

        if idx == 0 && (byte & 0x08) == 0 {
            return None;
        }

        self.packet[idx] = byte;
        let next = idx + 1;

        if next < self.packet_size.get() {
            self.packet_index.set(next);
            return None;
        }

        self.packet_index.set(0);
        self.parse_packet()
    }
}
