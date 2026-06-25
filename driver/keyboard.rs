use spin::Mutex;

pub static KEYBOARD: Mutex<Keyboard> = Mutex::new(Keyboard::new());
static KBD_DECODE_LOGS: Mutex<u8> = Mutex::new(0);

pub struct Keyboard {
    shift: bool,
    caps: bool,
    ctrl: bool,
    alt: bool,
    extended: bool, // set after a 0xE0 prefix byte
    pressed: [bool; 256],
}

impl Keyboard {
    const fn new() -> Self {
        Self {
            shift: false,
            caps: false,
            ctrl: false,
            alt: false,
            extended: false,
            pressed: [false; 256],
        }
    }

    /// Clear all modifier state (shift, ctrl, alt, caps, extended).
    /// Called when the keyboard appears stuck — typically after the host
    /// re-grabs the keyboard in a VM, which can leave modifiers stuck "on"
    /// because the VM-grab release (Ctrl+Alt) swallows the key-release
    /// scancodes for the grab keys.
    pub fn reset_modifiers(&mut self) {
        self.shift = false;
        self.caps = false;
        self.ctrl = false;
        self.alt = false;
        self.extended = false;
        self.pressed = [false; 256];
    }

    pub fn scancode_to_char(&mut self, sc: u8) -> Option<KeyEvent> {
        // 0xE0 marks the next byte as an extended key (dedicated arrows, Home,
        // End, Page keys, Insert/Delete, right Ctrl/Alt, keypad Enter, ...).
        if sc == 0xE0 {
            self.extended = true;
            return None;
        }
        let ext = self.extended;
        self.extended = false;

        let released = sc & 0x80 != 0;
        let code = sc & 0x7F;
        let key_index = code as usize + if ext { 128 } else { 0 };

        // Modifier keys (left side bare; right Ctrl/Alt arrive as ext 0x1D/0x38).
        match code {
            0x2A | 0x36 => {
                self.shift = !released;
                self.pressed[key_index] = !released;
                return None;
            }
            // Left Ctrl only.  Right Ctrl (0xE0 0x1D) is VirtualBox's default
            // Host Key — when you tap it to give control back to the host, VBox
            // swallows its key-release, which would leave Ctrl stuck "on" and
            // silently turn every keystroke into a control char (the keyboard
            // appears dead).  So never treat Right Ctrl as a sticky modifier.
            0x1D if !ext => {
                self.ctrl = !released;
                self.pressed[key_index] = !released;
                return None;
            }
            0x1D => {
                return None;
            }
            // Right Alt (0xE0 0x38) is often the QEMU grab key.  QEMU may not
            // send the release scancode for Alt when the grab is released, so
            // we also ignore Right Alt as a modifier — same reasoning as Right
            // Ctrl above.  Only Left Alt (bare 0x38) is tracked.
            0x38 if !ext => {
                self.alt = !released;
                self.pressed[key_index] = !released;
                return None;
            }
            0x38 => {
                return None;
            } // Right Alt: ignore (QEMU grab key)
            0x3A if !ext => {
                if released {
                    self.pressed[key_index] = false;
                    return None;
                }
                if self.pressed[key_index] {
                    return None;
                }
                self.pressed[key_index] = true;
                self.caps = !self.caps;
                return None;
            }
            _ if released => {
                self.pressed[key_index] = false;
                return None;
            }
            _ => {}
        }

        if self.pressed[key_index] {
            return None;
        }
        self.pressed[key_index] = true;

        // Extended (0xE0-prefixed) navigation/editing keys.
        if ext {
            return match code {
                0x48 => Some(KeyEvent::Up),
                0x50 => Some(KeyEvent::Down),
                0x4B => Some(KeyEvent::Left),
                0x4D => Some(KeyEvent::Right),
                0x47 => Some(KeyEvent::Home),
                0x4F => Some(KeyEvent::End),
                0x49 => Some(KeyEvent::PageUp),
                0x51 => Some(KeyEvent::PageDown),
                0x52 => Some(KeyEvent::Insert),
                0x53 => Some(KeyEvent::Delete),
                0x1C => Some(KeyEvent::Enter), // keypad Enter
                _ => None,
            };
        }

        let upper = self.shift ^ self.caps;
        let ch = match code {
            0x01 => return Some(KeyEvent::Escape),
            0x0E => return Some(KeyEvent::Backspace),
            0x0F => return Some(KeyEvent::Tab),
            0x1C => return Some(KeyEvent::Enter),
            // Function keys: F1-F10 = 0x3B..0x44, F11 = 0x57, F12 = 0x58
            0x3B..=0x44 => return Some(KeyEvent::Function(code - 0x3B + 1)),
            0x57 => return Some(KeyEvent::Function(11)),
            0x58 => return Some(KeyEvent::Function(12)),
            // Numeric-keypad navigation (NumLock off) — same as the dedicated keys
            0x47 => return Some(KeyEvent::Home),
            0x48 => return Some(KeyEvent::Up),
            0x49 => return Some(KeyEvent::PageUp),
            0x4B => return Some(KeyEvent::Left),
            0x4D => return Some(KeyEvent::Right),
            0x4F => return Some(KeyEvent::End),
            0x50 => return Some(KeyEvent::Down),
            0x51 => return Some(KeyEvent::PageDown),
            0x52 => return Some(KeyEvent::Insert),
            0x53 => return Some(KeyEvent::Delete),
            0x02 => {
                if self.shift {
                    '!'
                } else {
                    '1'
                }
            }
            0x03 => {
                if self.shift {
                    '@'
                } else {
                    '2'
                }
            }
            0x04 => {
                if self.shift {
                    '#'
                } else {
                    '3'
                }
            }
            0x05 => {
                if self.shift {
                    '$'
                } else {
                    '4'
                }
            }
            0x06 => {
                if self.shift {
                    '%'
                } else {
                    '5'
                }
            }
            0x07 => {
                if self.shift {
                    '^'
                } else {
                    '6'
                }
            }
            0x08 => {
                if self.shift {
                    '&'
                } else {
                    '7'
                }
            }
            0x09 => {
                if self.shift {
                    '*'
                } else {
                    '8'
                }
            }
            0x0A => {
                if self.shift {
                    '('
                } else {
                    '9'
                }
            }
            0x0B => {
                if self.shift {
                    ')'
                } else {
                    '0'
                }
            }
            0x0C => {
                if self.shift {
                    '_'
                } else {
                    '-'
                }
            }
            0x0D => {
                if self.shift {
                    '+'
                } else {
                    '='
                }
            }
            0x10 => {
                if upper {
                    'Q'
                } else {
                    'q'
                }
            }
            0x11 => {
                if upper {
                    'W'
                } else {
                    'w'
                }
            }
            0x12 => {
                if upper {
                    'E'
                } else {
                    'e'
                }
            }
            0x13 => {
                if upper {
                    'R'
                } else {
                    'r'
                }
            }
            0x14 => {
                if upper {
                    'T'
                } else {
                    't'
                }
            }
            0x15 => {
                if upper {
                    'Y'
                } else {
                    'y'
                }
            }
            0x16 => {
                if upper {
                    'U'
                } else {
                    'u'
                }
            }
            0x17 => {
                if upper {
                    'I'
                } else {
                    'i'
                }
            }
            0x18 => {
                if upper {
                    'O'
                } else {
                    'o'
                }
            }
            0x19 => {
                if upper {
                    'P'
                } else {
                    'p'
                }
            }
            0x1A => {
                if self.shift {
                    '{'
                } else {
                    '['
                }
            }
            0x1B => {
                if self.shift {
                    '}'
                } else {
                    ']'
                }
            }
            0x1E => {
                if upper {
                    'A'
                } else {
                    'a'
                }
            }
            0x1F => {
                if upper {
                    'S'
                } else {
                    's'
                }
            }
            0x20 => {
                if upper {
                    'D'
                } else {
                    'd'
                }
            }
            0x21 => {
                if upper {
                    'F'
                } else {
                    'f'
                }
            }
            0x22 => {
                if upper {
                    'G'
                } else {
                    'g'
                }
            }
            0x23 => {
                if upper {
                    'H'
                } else {
                    'h'
                }
            }
            0x24 => {
                if upper {
                    'J'
                } else {
                    'j'
                }
            }
            0x25 => {
                if upper {
                    'K'
                } else {
                    'k'
                }
            }
            0x26 => {
                if upper {
                    'L'
                } else {
                    'l'
                }
            }
            0x27 => {
                if self.shift {
                    ':'
                } else {
                    ';'
                }
            }
            0x28 => {
                if self.shift {
                    '"'
                } else {
                    '\''
                }
            }
            0x29 => {
                if self.shift {
                    '~'
                } else {
                    '`'
                }
            }
            0x2B => {
                if self.shift {
                    '|'
                } else {
                    '\\'
                }
            }
            0x2C => {
                if upper {
                    'Z'
                } else {
                    'z'
                }
            }
            0x2D => {
                if upper {
                    'X'
                } else {
                    'x'
                }
            }
            0x2E => {
                if upper {
                    'C'
                } else {
                    'c'
                }
            }
            0x2F => {
                if upper {
                    'V'
                } else {
                    'v'
                }
            }
            0x30 => {
                if upper {
                    'B'
                } else {
                    'b'
                }
            }
            0x31 => {
                if upper {
                    'N'
                } else {
                    'n'
                }
            }
            0x32 => {
                if upper {
                    'M'
                } else {
                    'm'
                }
            }
            0x33 => {
                if self.shift {
                    '<'
                } else {
                    ','
                }
            }
            0x34 => {
                if self.shift {
                    '>'
                } else {
                    '.'
                }
            }
            0x35 => {
                if self.shift {
                    '?'
                } else {
                    '/'
                }
            }
            0x39 => ' ',
            _ => return None,
        };

        // Ctrl + key → control character (Ctrl+A = 0x01 .. Ctrl+Z = 0x1A), the
        // convention terminals/editors expect (e.g. nano's ^X/^S, shell ^C/^L).
        if self.ctrl {
            let up = ch.to_ascii_uppercase();
            if up.is_ascii_uppercase() {
                return Some(KeyEvent::Char((((up as u8) - b'A') + 1) as char));
            }
            match ch {
                '[' => return Some(KeyEvent::Escape), // ^[ = ESC
                '\\' => return Some(KeyEvent::Char('\x1c')),
                ']' => return Some(KeyEvent::Char('\x1d')),
                _ => {}
            }
        }
        Some(KeyEvent::Char(ch))
    }
}

#[derive(Debug)]
pub enum KeyEvent {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Function(u8),
}

/// Initialize the 8042 PS/2 controller and enable keyboard scanning.
pub fn init() {
    unsafe {
        crate::serial_println!("[kbd] PS/2 keyboard init begin");
        // Disable both PS/2 ports during init
        wait_write();
        crate::arch::port_write_u8(0x64, 0xAD); // disable port 1
        wait_write();
        crate::arch::port_write_u8(0x64, 0xA7); // disable port 2

        // Flush any bytes sitting in the output buffer
        let mut flushed = 0u8;
        for _ in 0..16u8 {
            let status = crate::arch::port_read_u8(0x64);
            if status & 0x01 == 0 {
                break;
            }
            let _ = crate::arch::port_read_u8(0x60);
            flushed = flushed.saturating_add(1);
        }

        // Read controller config byte
        wait_write();
        crate::arch::port_write_u8(0x64, 0x20);
        let config_ready = wait_read();
        let mut config = crate::arch::port_read_u8(0x60);
        let initial_config = config;

        // Enable IRQ1 (keyboard), disable IRQ12 (mouse), enable translation
        config |= 0x01; // IRQ1 enable
        config &= !0x02; // IRQ12 disable
        config |= 0x40; // scancode translation (set 2 → set 1)

        // Write config back
        wait_write();
        crate::arch::port_write_u8(0x64, 0x60);
        wait_write();
        crate::arch::port_write_u8(0x60, config);

        // Re-enable port 1 (keyboard)
        wait_write();
        crate::arch::port_write_u8(0x64, 0xAE);

        // Send "enable scanning" command (0xF4) to keyboard
        wait_write();
        crate::arch::port_write_u8(0x60, 0xF4);

        // Read ACK (0xFA)
        let ack_ready = wait_read();
        let ack = if ack_ready {
            crate::arch::port_read_u8(0x60)
        } else {
            0
        };
        crate::serial_println!(
            "[kbd] PS/2 keyboard {} config={:#04x}->{:#04x} irq1={} translation={} flushed={} ack={:#04x} config_ready={} ack_ready={} pending_scancode={}",
            if ack == 0xFA { "ready" } else { "init-warning" },
            initial_config,
            config,
            config & 0x01 != 0,
            config & 0x40 != 0,
            flushed,
            ack,
            config_ready,
            ack_ready,
            crate::interrupts::has_pending_scancode(),
        );
    }
}

/// Wait until the PS/2 controller is ready to receive a command/data byte.
unsafe fn wait_write() -> bool {
    unsafe {
        for _ in 0..100_000u32 {
            if crate::arch::port_read_u8(0x64) & 0x02 == 0 {
                return true;
            }
            crate::arch::nop();
        }
        false
    }
}

/// Re-enable keyboard scanning and clear stuck state.
///
/// Call this when the keyboard appears unresponsive — typically after the host
/// VM grabs/releases the keyboard (Ctrl+Alt in QEMU, Host Key in VirtualBox),
/// which can leave scancodes in the 8042 buffer without an IRQ edge and leave
/// modifier keys stuck "on" because the grab-release swallows key-up events.
///
/// This drains the hardware buffer, re-sends the "enable scanning" (0xF4)
/// command to the keyboard, and clears all modifier state in the software
/// scancode-to-character state machine.
pub fn reenable() {
    // 1. Drain any bytes stuck in the 8042 output buffer (mouse or keyboard).
    drain_hw_buffer();

    // 2. Clear the software scancode queue — any partial or stale scancodes
    //    (e.g. a lone 0xE0 prefix from a broken grab transition) would corrupt
    //    the next key event.
    crate::interrupts::clear_scancode_queue();

    // 3. Re-send "enable scanning" (0xF4) to the keyboard.  If a previous
    //    grab transition caused QEMU to send "disable scanning" (0xF5), the
    //    keyboard sits silently until this command resets it.
    // F-KBD-04: Use standard direct-to-keyboard write (wait for input buffer
    // empty, then write 0xF4 to port 0x60) instead of non-standard 0xD2
    // controller command which many 8042 implementations ignore.
    crate::arch::without_interrupts(|| unsafe {
        // Wait for the controller input buffer to be ready.
        wait_write();
        // Send "enable scanning" directly to keyboard via data port.
        crate::arch::port_write_u8(0x60, 0xF4);

        // Drain the ACK (0xFA) that the keyboard sends back.
        // Don't block if it never arrives.
        for _ in 0..50_000u32 {
            if crate::arch::port_read_u8(0x64) & 0x01 != 0 {
                let _ = crate::arch::port_read_u8(0x60);
                break;
            }
        }
    });

    // 4. Drain again — the ACK byte is now in the buffer.
    drain_hw_buffer();

    // 5. Clear all modifier state (stuck Ctrl/Alt/Shift/extended after grab release).
    // F-KBD-03: Hold KEYBOARD lock with interrupts disabled to prevent deadlock
    // if keyboard IRQ handler ever accesses KEYBOARD in the future.
    crate::arch::without_interrupts(|| {
        KEYBOARD.lock().reset_modifiers();
        *KBD_DECODE_LOGS.lock() = 0;
    });
}

/// Wait until the PS/2 output buffer has a byte ready.
unsafe fn wait_read() -> bool {
    unsafe {
        for _ in 0..100_000u32 {
            if crate::arch::port_read_u8(0x64) & 0x01 != 0 {
                return true;
            }
            crate::arch::nop();
        }
        false
    }
}

/// Drain any bytes sitting in the 8042 output buffer straight into the scancode
/// queue.  The keyboard IRQ is edge-triggered: if a byte arrives while IRQ1 is
/// masked (e.g. interrupts briefly disabled during a long command), the edge is
/// lost and the controller will not assert IRQ1 again until that byte is read.
/// Draining here, every poll, makes keyboard input self-heal from a lost edge.
fn drain_hw_buffer() {
    // Thread context: disable interrupts so the keyboard IRQ can't read the same
    // port mid-loop (the shared drain assumes interrupts are already off).
    crate::arch::without_interrupts(crate::interrupts::drain_ps2_keyboard);
}

/// Called by the shell poll loop — returns the next key event if one is queued.
/// F-KBD-03: KEYBOARD lock acquired with interrupts disabled to prevent
/// deadlock if keyboard IRQ handler ever accesses KEYBOARD.
pub fn poll() -> Option<KeyEvent> {
    drain_hw_buffer();
    crate::arch::without_interrupts(|| {
        let mut keyboard = KEYBOARD.lock();
        while let Some(sc) = crate::interrupts::next_scancode() {
            let event = keyboard.scancode_to_char(sc);
            {
                let mut logs = KBD_DECODE_LOGS.lock();
                if *logs < 32 {
                    crate::serial_println!("[kbd-pipe] scancode={:#x} event={:?}", sc, event);
                    *logs = logs.saturating_add(1);
                }
            }
            if let Some(event) = event {
                return Some(event);
            }
        }
        None
    })
}
