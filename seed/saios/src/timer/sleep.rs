use core::time::Duration;

pub fn sleep_ns(ns: u64) {
    let start = super::manager().monotonic_ns();
    loop {
        let now = super::manager().monotonic_ns();
        if now.wrapping_sub(start) >= ns {
            break;
        }
        core::hint::spin_loop();
    }
}

pub fn sleep_ms(ms: u64) {
    sleep_ns(ms.saturating_mul(1_000_000));
}

pub fn sleep(duration: Duration) {
    sleep_ns(duration.as_nanos().min(u64::MAX as u128) as u64);
}
