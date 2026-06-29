use core::sync::atomic::{AtomicBool, Ordering};

static SERIAL_INITIALIZED: AtomicBool = AtomicBool::new(false);
/// Optimistic: assume the port is present until proven otherwise.
/// The timeout mechanism in write_byte already handles absent hardware.
static SERIAL_PRESENT: AtomicBool = AtomicBool::new(true);

// COM1 serial port I/O ports
const COM1_BASE: u16 = 0x3F8;
const COM1_DATA: u16 = COM1_BASE;          // 0x3F8 - Data register (R/W)
const COM1_IER: u16 = COM1_BASE + 1;       // 0x3F9 - Interrupt Enable Register
#[allow(dead_code)]
const COM1_IIR: u16 = COM1_BASE + 2;       // 0x3FA - Interrupt Identification Register (R)
const COM1_FIFO: u16 = COM1_BASE + 2;      // 0x3FA - FIFO Control Register (W)
const COM1_LCR: u16 = COM1_BASE + 3;       // 0x3FB - Line Control Register
const COM1_MCR: u16 = COM1_BASE + 4;       // 0x3FC - Modem Control Register
const COM1_LSR: u16 = COM1_BASE + 5;       // 0x3FD - Line Status Register
#[allow(dead_code)]
const COM1_MSR: u16 = COM1_BASE + 6;       // 0x3FE - Modem Status Register

// Line Status Register bits
const LSR_DR: u8 = 0x01;    // Data Ready (byte available to read)
#[allow(dead_code)]
const LSR_OE: u8 = 0x02;    // Overrun Error
#[allow(dead_code)]
const LSR_PE: u8 = 0x04;    // Parity Error
#[allow(dead_code)]
const LSR_FE: u8 = 0x08;    // Framing Error
#[allow(dead_code)]
const LSR_BI: u8 = 0x10;    // Break Interrupt
const LSR_THRE: u8 = 0x20;  // Transmit Holding Register Empty
const LSR_TEMT: u8 = 0x40;  // Transmitter Empty
#[allow(dead_code)]
const LSR_ERR: u8 = 0x80;   // Impending Error (FIFO mode)

// Interrupt Enable Register bits
#[allow(dead_code)]
const IER_RDA: u8 = 0x01;   // Received Data Available interrupt
#[allow(dead_code)]
const IER_THRE: u8 = 0x02;  // Transmit Holding Register Empty interrupt
#[allow(dead_code)]
const IER_LSR: u8 = 0x04;   // Line Status Register interrupt
#[allow(dead_code)]
const IER_MSR: u8 = 0x08;   // Modem Status Register interrupt

// Modem Control Register bits
const MCR_DTR: u8 = 0x01;   // Data Terminal Ready
const MCR_RTS: u8 = 0x02;   // Request To Send
#[allow(dead_code)]
const MCR_OUT1: u8 = 0x04;  // Aux Output 1
const MCR_OUT2: u8 = 0x08;  // Aux Output 2 (needed for IRQs on PC)
const MCR_LOOP: u8 = 0x10;  // Loopback mode

// Timeout configuration
const WRITE_TIMEOUT: u32 = 100_000;  // Max iterations to wait for THRE
const READ_TIMEOUT: u32 = 100_000;   // Max iterations to wait for data
const FLUSH_TIMEOUT: u32 = 200_000;  // Max iterations to wait for TEMT

// Standard PC serial clock: 115200 Hz
// Divisor = clock / (16 * baud)
// 115200: divisor 1
// 57600:  divisor 2
// 38400:  divisor 3
// 19200:  divisor 6
// 9600:   divisor 12
const BAUD_DIVISOR: u16 = 3;  // 38400 baud (matches common VM/default configs)

#[inline]
fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[inline]
fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

// ── Probe ────────────────────────────────────────────────────────────

/// Probe whether a serial port exists at COM1.
///
/// Writes to the scratch register (offset 7) and reads back. On real
/// hardware the scratch register is a simple read/write byte; if the
/// value round-trips the port is almost certainly present.
pub fn probe() -> bool {
    let scratch = COM1_BASE + 7;
    let saved = inb(scratch);
    outb(scratch, 0xA5);
    let readback = inb(scratch);
    outb(scratch, saved);
    readback == 0xA5
}

/// Run a quick internal loopback test to verify the UART is functional.
///
/// Puts the UART into loopback mode, sends a test byte, reads it back,
/// then restores normal mode. Returns `true` if the byte round-trips.
pub fn loopback_test() -> bool {
    // Save current MCR state
    let saved_mcr = inb(COM1_MCR);

    // Enter loopback mode (bit 4)
    outb(COM1_MCR, MCR_LOOP | MCR_DTR | MCR_RTS | MCR_OUT2);

    // Disable interrupts during test
    let saved_ier = inb(COM1_IER);
    outb(COM1_IER, 0x00);

    // Send test byte
    let test_byte: u8 = 0x5A;
    outb(COM1_DATA, test_byte);

    // Wait for data to be available (loopback returns immediately)
    let mut timeout = READ_TIMEOUT;
    let mut received: u8 = 0;
    while timeout > 0 {
        let status = inb(COM1_LSR);
        if status & LSR_DR != 0 {
            received = inb(COM1_DATA);
            break;
        }
        timeout -= 1;
    }

    // Restore state
    outb(COM1_IER, saved_ier);
    outb(COM1_MCR, saved_mcr);

    timeout > 0 && received == test_byte
}

// ── Initialization ────────────────────────────────────────────────────

/// Initialize the serial port.
///
/// Configures 38400 baud, 8N1, FIFO enabled with 14-byte trigger.
/// Always configures the hardware — probe/loopback are informational
/// only. If the port is genuinely absent, the timeout mechanism in
/// write_byte prevents hangs.
pub fn init() {
    if SERIAL_INITIALIZED.swap(true, Ordering::Relaxed) {
        return; // Already initialized
    }

    // Disable all interrupts
    outb(COM1_IER, 0x00);

    // Enable DLAB to set baud rate divisor
    outb(COM1_LCR, 0x80);

    // Set divisor for configured baud rate
    let divisor_lo = (BAUD_DIVISOR & 0xFF) as u8;
    let divisor_hi = ((BAUD_DIVISOR >> 8) & 0xFF) as u8;
    outb(COM1_DATA, divisor_lo);
    outb(COM1_IER, divisor_hi);

    // Enable FIFOs, clear both FIFOs, 14-byte RX trigger
    outb(COM1_FIFO, 0xC7);

    // 8 data bits, no parity, 1 stop bit, clear DLAB
    outb(COM1_LCR, 0x03);

    // Assert DTR, RTS, and OUT2 (required for IRQs on PC)
    outb(COM1_MCR, MCR_DTR | MCR_RTS | MCR_OUT2);

    // Drain any stale data from the RX buffer
    while (inb(COM1_LSR) & LSR_DR) != 0 {
        let _ = inb(COM1_DATA);
    }

    // Informational: check if hardware looks present
    if !probe() || !loopback_test() {
        SERIAL_PRESENT.store(false, Ordering::Relaxed);
    }
}

// ── Write path ────────────────────────────────────────────────────────

/// Write a single byte to the serial port with timeout.
///
/// Waits for THRE (Transmit Holding Register Empty). If the timeout
/// expires the byte is silently dropped — the kernel never hangs on
/// a stuck or absent serial port.
pub fn write_byte(byte: u8) {
    let mut timeout = WRITE_TIMEOUT;
    while timeout > 0 {
        let status = inb(COM1_LSR);
        if status & LSR_THRE != 0 {
            break;
        }
        timeout -= 1;
    }

    if timeout > 0 {
        outb(COM1_DATA, byte);
    }
}

/// Flush all pending transmit data.
///
/// Waits for TEMT (Transmitter Empty) — both the holding register and
/// the shift register must be empty. Uses a separate, longer timeout.
pub fn flush() {
    let mut timeout = FLUSH_TIMEOUT;
    while timeout > 0 {
        let status = inb(COM1_LSR);
        if status & LSR_TEMT != 0 {
            break;
        }
        timeout -= 1;
    }
    // If timeout occurs, continue anyway — don't hang
}

/// Write a string to the serial port.
///
/// - `\n` (LF) is expanded to `\r\n` (CR+LF) for proper terminal display.
/// - `\r` (CR) is passed through as-is.
/// - `\r\n` sequences are NOT doubled — a CR immediately before an LF
///   suppresses the extra CR that LF would normally emit.
/// - `\t` (tab) is expanded to 4 spaces.
pub fn write_str(s: &str) {
    let mut prev_was_cr = false;

    for byte in s.bytes() {
        match byte {
            b'\n' => {
                if !prev_was_cr {
                    // Standalone LF → emit CR+LF
                    write_byte(b'\r');
                }
                // prev_was_cr: CR already emitted, just emit LF
                write_byte(b'\n');
                prev_was_cr = false;
            }
            b'\r' => {
                write_byte(b'\r');
                prev_was_cr = true;
            }
            b'\t' => {
                write_byte(b' ');
                write_byte(b' ');
                write_byte(b' ');
                write_byte(b' ');
                prev_was_cr = false;
            }
            _ => {
                write_byte(byte);
                prev_was_cr = false;
            }
        }
    }
}

/// Write formatted output to the serial port.
pub fn write_fmt(args: core::fmt::Arguments<'_>) {
    use core::fmt::Write;
    let _ = write!(SerialWriter, "{}", args);
}

/// Write a debug string (no formatting) to the serial port.
pub fn write_debug_str(s: &str) {
    write_str(s);
}

/// Write an error string to the serial port.
pub fn write_error_str(s: &str) {
    write_str("[ERROR] ");
    write_str(s);
    write_str("\n");
}

/// Write an info string to the serial port.
pub fn write_info_str(s: &str) {
    write_str("[INFO] ");
    write_str(s);
    write_str("\n");
}

/// Write a debug message with trailing newline.
pub fn write_debug_fmt(args: core::fmt::Arguments<'_>) {
    write_fmt(args);
    write_str("\n");
}

// ── Read path ─────────────────────────────────────────────────────────

/// Check whether a byte is available to read from the serial port.
#[inline]
pub fn is_data_ready() -> bool {
    (inb(COM1_LSR) & LSR_DR) != 0
}

/// Read a single byte from the serial port with timeout.
///
/// Returns `Some(byte)` if data was available before the timeout,
/// or `None` if the timeout expired.
pub fn read_byte() -> Option<u8> {
    let mut timeout = READ_TIMEOUT;
    while timeout > 0 {
        let status = inb(COM1_LSR);
        if status & LSR_DR != 0 {
            let byte = inb(COM1_DATA);
            return Some(byte);
        }
        timeout -= 1;
    }
    None
}

/// Read a byte without waiting. Returns `None` immediately if no data
/// is available.
#[inline]
pub fn try_read_byte() -> Option<u8> {
    if (inb(COM1_LSR) & LSR_DR) != 0 {
        Some(inb(COM1_DATA))
    } else {
        None
    }
}

/// Read available bytes into the provided buffer.
///
/// Returns the number of bytes actually read. Does not block — reads
/// whatever is immediately available up to `buf.len()`.
pub fn read_bytes(buf: &mut [u8]) -> usize {
    let mut count = 0;
    for slot in buf.iter_mut() {
        match try_read_byte() {
            Some(b) => {
                *slot = b;
                count += 1;
            }
            None => break,
        }
    }
    count
}

// ── Interrupt control ─────────────────────────────────────────────────

/// Enable specific serial interrupts.
///
/// `mask` should be a combination of `IER_*` constants. Typically
/// `IER_RDA` for receive interrupts.
pub fn enable_interrupts(mask: u8) {
    // Writing to IER requires DLAB=0
    let lcr = inb(COM1_LCR);
    outb(COM1_LCR, lcr & !0x80); // Ensure DLAB is clear
    outb(COM1_IER, mask);
}

/// Disable all serial interrupts.
pub fn disable_interrupts() {
    let lcr = inb(COM1_LCR);
    outb(COM1_LCR, lcr & !0x80);
    outb(COM1_IER, 0x00);
}

// ── Status queries ────────────────────────────────────────────────────

/// Check if the serial port has been initialized.
pub fn is_initialized() -> bool {
    SERIAL_INITIALIZED.load(Ordering::Relaxed)
}

/// Check if a serial port was detected and is functional.
pub fn is_present() -> bool {
    SERIAL_PRESENT.load(Ordering::Relaxed)
}

/// Check if the serial port is ready to accept a byte for transmission.
pub fn is_ready() -> bool {
    (inb(COM1_LSR) & LSR_THRE) != 0
}

/// Read the raw Line Status Register. Useful for error checking after
/// a read that may have encountered parity/framing/overrun errors.
pub fn line_status() -> u8 {
    inb(COM1_LSR)
}

// ── fmt::Write impl ───────────────────────────────────────────────────

struct SerialWriter;

impl core::fmt::Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_str(s);
        Ok(())
    }
}
