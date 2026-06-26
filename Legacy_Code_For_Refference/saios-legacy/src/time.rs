//! High-resolution time based on the CPU's Time-Stamp Counter (TSC).
//!
//! The PIT tick (~18.2 Hz) is far too coarse for `DateTime`, `Stopwatch`, or
//! sub-second sleeps.  At boot we:
//!   1. Calibrate the TSC frequency against PIT channel 2 (a precise one-shot).
//!   2. Read the CMOS RTC once to anchor wall-clock time to a real Unix epoch.
//!
//! Thereafter all timing reads `rdtsc()` and scales by the measured frequency,
//! so the OS clock advances at exactly the CPU clock rate.

use core::sync::atomic::{AtomicU64, Ordering};

/// Measured TSC frequency in Hz (0 until calibrated → callers fall back to PIT).
static TSC_HZ: AtomicU64 = AtomicU64::new(0);
/// TSC value captured at boot (origin for monotonic uptime).
static BOOT_TSC: AtomicU64 = AtomicU64::new(0);
/// Unix epoch seconds captured from the CMOS RTC at boot.
static BOOT_UNIX_SECS: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn rdtsc() -> u64 {
    crate::arch::read_tsc()
}

/// Calibrate the TSC against PIT channel 2 (gated one-shot), then anchor the
/// wall clock from the CMOS RTC.  Call once, early in boot, after interrupts
/// are set up but it does not depend on the timer IRQ.
pub fn init() {
    let hz = calibrate_tsc();
    TSC_HZ.store(hz, Ordering::SeqCst);
    BOOT_TSC.store(rdtsc(), Ordering::SeqCst);
    BOOT_UNIX_SECS.store(read_cmos_unix_secs(), Ordering::SeqCst);
    crate::serial_println!(
        "[time] TSC {} MHz, boot epoch {}s (CMOS RTC)",
        hz / 1_000_000,
        BOOT_UNIX_SECS.load(Ordering::SeqCst)
    );
}

/// Frequency of the TSC in Hz (0 if calibration failed).
pub fn tsc_hz() -> u64 {
    TSC_HZ.load(Ordering::Relaxed)
}

/// Convert raw TSC tick delta to nanoseconds using calibrated frequency.
#[inline]
pub fn tsc_ticks_to_ns(ticks: u64) -> u64 {
    let hz = TSC_HZ.load(Ordering::Relaxed);
    if hz == 0 {
        return 0;
    }
    ((ticks as u128 * 1_000_000_000u128) / hz as u128) as u64
}

/// Nanoseconds since boot (monotonic), from the TSC.  Falls back to the PIT
/// tick estimate if the TSC was never calibrated.
pub fn uptime_ns() -> u64 {
    let hz = TSC_HZ.load(Ordering::Relaxed);
    if hz == 0 {
        // Fallback: PIT ticks at 100 Hz → 10 ms (10_000_000 ns) per tick.
        return crate::shell::commands::boot_ticks().wrapping_mul(10_000_000);
    }
    let delta = rdtsc().wrapping_sub(BOOT_TSC.load(Ordering::Relaxed));
    // delta * 1e9 / hz, done as u128 to avoid overflow.
    ((delta as u128 * 1_000_000_000u128) / hz as u128) as u64
}

/// Whole seconds since boot (monotonic).
pub fn uptime_secs() -> u64 {
    uptime_ns() / 1_000_000_000
}

/// Current wall-clock time as (seconds, nanoseconds) since the Unix epoch.
pub fn realtime() -> (u64, u64) {
    let up = uptime_ns();
    let secs = BOOT_UNIX_SECS.load(Ordering::Relaxed) + up / 1_000_000_000;
    (secs, up % 1_000_000_000)
}

// -- TSC calibration via PIT channel 2 ---------------------------------------

/// Measure the TSC frequency by timing a precise PIT channel-2 one-shot.
/// Returns Hz, or 0 if the result looks implausible.
fn calibrate_tsc() -> u64 {
    // PIT input clock is 1_193_182 Hz.  Program channel 2 for a ~50 ms one-shot.
    const PIT_HZ: u64 = 1_193_182;
    const MS: u64 = 50;
    let count: u16 = ((PIT_HZ * MS) / 1000) as u16; // 59659

    unsafe {
        crate::arch::prepare_pit_channel2_oneshot(count);

        let start = rdtsc();
        // Channel-2 output state is bit5 of port 0x61; it goes high at terminal
        // count.  Spin until it does (bounded so a broken PIT can't hang boot).
        let mut guard = 0u64;
        while !crate::arch::pit_channel2_terminal_count() {
            guard += 1;
            if guard > 100_000_000 {
                return 0;
            }
            core::hint::spin_loop();
        }
        let end = rdtsc();

        let delta = end.wrapping_sub(start);
        // delta TSC ticks elapsed over MS milliseconds → Hz.
        let hz = delta * 1000 / MS;
        // Sanity: between 100 MHz and 100 GHz.
        if !(100_000_000..=100_000_000_000).contains(&hz) {
            0
        } else {
            hz
        }
    }
}

// -- CMOS RTC → Unix epoch seconds -------------------------------------------

fn cmos_read(reg: u8) -> u8 {
    unsafe { crate::arch::read_cmos_register(reg) }
}

fn cmos_updating() -> bool {
    cmos_read(0x0A) & 0x80 != 0
}

/// Read the CMOS real-time clock and convert to Unix epoch seconds (UTC).
fn read_cmos_unix_secs() -> u64 {
    // Wait out any in-progress update, then read; retry until two reads agree.
    let mut last = read_cmos_raw();
    for _ in 0..10 {
        let cur = read_cmos_raw();
        if cur == last {
            break;
        }
        last = cur;
    }
    let (sec, min, hour, day, mon, year) = last;

    let status_b = cmos_read(0x0B);
    let bcd = status_b & 0x04 == 0; // bit2 clear ⇒ BCD encoding
    let conv = |v: u8| {
        if bcd {
            ((v >> 4) * 10 + (v & 0x0F)) as u64
        } else {
            v as u64
        }
    };

    let sec = conv(sec);
    let min = conv(min);
    let hour = conv(hour);
    let day = conv(day);
    let mon = conv(mon);
    let mut year = conv(year);
    year += 2000; // CMOS year is two digits; assume 21st century.

    days_to_unix(year, mon, day) * 86400 + hour * 3600 + min * 60 + sec
}

fn read_cmos_raw() -> (u8, u8, u8, u8, u8, u8) {
    while cmos_updating() {
        core::hint::spin_loop();
    }
    (
        cmos_read(0x00),
        cmos_read(0x02),
        cmos_read(0x04),
        cmos_read(0x07),
        cmos_read(0x08),
        cmos_read(0x09),
    )
}

/// Convert Unix epoch seconds (UTC) to civil (year, month, day, hour, min, sec).
/// Uses Howard Hinnant's days→civil algorithm.
pub fn civil_from_epoch(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = secs / 86400;
    let rem = secs % 86400;
    let (hour, min, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, min, sec)
}

/// Civil (UTC) date/time → Unix epoch seconds.
pub fn epoch_from_civil(year: u64, mon: u64, day: u64, h: u64, mi: u64, s: u64) -> u64 {
    days_to_unix(year, mon, day) * 86400 + h * 3600 + mi * 60 + s
}

/// Days from 1970-01-01 to year-mon-day (proleptic Gregorian).
fn days_to_unix(year: u64, mon: u64, day: u64) -> u64 {
    let is_leap = |y: u64| (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400);
    let mdays = [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut days = 0u64;
    let mut y = 1970;
    while y < year {
        days += if is_leap(y) { 366 } else { 365 };
        y += 1;
    }
    let mut m = 1;
    while m < mon {
        days += mdays[(m - 1) as usize];
        if m == 2 && is_leap(year) {
            days += 1;
        }
        m += 1;
    }
    days + day.saturating_sub(1)
}
