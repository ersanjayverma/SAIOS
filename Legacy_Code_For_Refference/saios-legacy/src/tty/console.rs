//! Console driver - keyboard input and framebuffer output
//!
//! Reads keystrokes from PS/2 keyboard and writes to VGA framebuffer.
//! Single authoritative owner of keyboard input buffer.

use crate::vfs::VfsResult;

// Initialize console
pub fn init() {
    // Console initialization handled in main.rs via driver::keyboard::init()
}

// Read from console (keyboard input) - authoritative implementation
pub fn read(buf: &mut [u8], _offset: u64) -> VfsResult<usize> {
    {
        let mut state = crate::tty::get_tty_state();
        if !state.input_buffer.is_empty() {
            let n = buf.len().min(state.input_buffer.len());
            for (dst, src) in buf.iter_mut().zip(state.input_buffer.drain(..n)) {
                *dst = src;
            }
            return Ok(n);
        }
    }

    // Try to get a character from keyboard
    let mut n = 0;
    while n < buf.len() {
        // Poll keyboard - this is a simple polled implementation
        // In a full implementation, this would wait for keyboard interrupt
        if let Some(sc) = crate::interrupts::next_scancode()
            && let Some(ev) = crate::driver::keyboard::KEYBOARD
                .lock()
                .scancode_to_char(sc)
        {
            use crate::driver::keyboard::KeyEvent;
            match ev {
                KeyEvent::Char(c) => {
                    let mut enc = [0u8; 4];
                    let s = c.encode_utf8(&mut enc);
                    let bytes_to_copy = s.len().min(buf.len() - n);
                    buf[n..n + bytes_to_copy].copy_from_slice(&enc[..bytes_to_copy]);
                    n += bytes_to_copy;
                    break; // Read at least one character
                }
                KeyEvent::Enter => {
                    buf[n] = b'\n';
                    n += 1;
                    break; // Read enter key
                }
                _ => continue,
            }
        }
        // No more input available
        break;
    }

    Ok(n)
}

// Write to console (framebuffer output) - authoritative implementation
pub fn write(buf: &[u8], _offset: u64) -> VfsResult<usize> {
    // Use the global VGA writer
    {
        let mut writer = crate::vga_buffer::WRITER.lock();
        for &c in buf {
            match c {
                b'\n' => {
                    // Newline - move to next line
                    writer.new_line();
                }
                _ => {
                    // Regular character - write to current position
                    writer.write_byte(c);
                }
            }
        }
    }

    Ok(buf.len())
}

// Add a character to the keyboard buffer (for keyboard interrupt handler)
pub fn add_input_char(c: char) {
    let mut state = crate::tty::get_tty_state();

    // Encode character to UTF-8
    let mut enc = [0u8; 4];
    let s = c.encode_utf8(&mut enc);
    state.input_buffer.extend_from_slice(s.as_bytes());
}

// Check if keyboard buffer has data
pub fn has_input() -> bool {
    let state = crate::tty::get_tty_state();
    !state.input_buffer.is_empty()
}

// Clear the keyboard buffer
pub fn clear_input() {
    let mut state = crate::tty::get_tty_state();
    state.input_buffer.clear();
}
