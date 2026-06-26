//! Unified Ethernet NIC dispatcher.
//!
//! Probes drivers in order of preference:
//!   e1000   → Intel PRO/1000 (VirtualBox default "Intel PRO/1000 MT Desktop")
//!   rtl8139 → Realtek 8139
//!   (VirtIO-Net is handled separately in net::virtio and used as fallback)
//!
//! Only ONE driver is active at a time — whichever probe() returns true first.
//! This prevents the MAC/IP conflict that occurred when e1000 and VirtIO-Net
//! both initialised simultaneously on the same NIC.

pub mod e1000;
pub mod rtl8139;

use core::sync::atomic::{AtomicU8, Ordering};

/// Which NIC driver is active.
/// 0 = none, 1 = e1000, 2 = rtl8139, 3 = virtio (handled externally)
pub static ACTIVE: AtomicU8 = AtomicU8::new(0);

pub fn active_driver_id() -> u8 {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn init() {
    crate::driver_contract::DriverContract::record_register(1, 0);
    if e1000::probe() {
        ACTIVE.store(1, Ordering::Relaxed);
        crate::driver_contract::DriverContract::transition_or_panic(
            crate::driver_contract::DriverState::New,
            crate::driver_contract::DriverState::Initialized,
            "net.e1000.init",
        );
        crate::driver_contract::DriverContract::record_resource(1, 1, 0);
        crate::network_contract::NetworkContract::record_nic_activation("e1000", 1, false);
        crate::serial_println!("[net] Intel e1000 Gigabit Ethernet active");
        return;
    }
    if rtl8139::probe() {
        ACTIVE.store(2, Ordering::Relaxed);
        crate::driver_contract::DriverContract::transition_or_panic(
            crate::driver_contract::DriverState::New,
            crate::driver_contract::DriverState::Initialized,
            "net.rtl8139.init",
        );
        crate::driver_contract::DriverContract::record_resource(2, 2, 0);
        crate::network_contract::NetworkContract::record_nic_activation("rtl8139", 2, false);
        crate::serial_println!("[net] Realtek RTL8139 Fast Ethernet active");
        return;
    }
    // No dedicated NIC found — VirtIO-Net will be used (initialised in net::virtio)
    ACTIVE.store(3, Ordering::Relaxed);
    crate::driver_contract::DriverContract::record_resource(3, 3, 0);
    crate::network_contract::NetworkContract::record_nic_activation("virtio-net", 3, true);
}

/// Returns true if a hardware NIC (e1000 / rtl8139) is active.
/// When false, the caller should use VirtIO-Net.
pub fn hw_nic_active() -> bool {
    ACTIVE.load(Ordering::Relaxed) < 3
}

/// Poll the active hardware NIC for received frames.
pub fn poll_rx() {
    match ACTIVE.load(Ordering::Relaxed) {
        1 => e1000::poll_rx(),
        2 => rtl8139::poll_rx(),
        _ => {} // VirtIO-Net handles its own polling
    }
}

/// Flush pending TX frames through the active hardware NIC.
pub fn flush_tx() {
    match ACTIVE.load(Ordering::Relaxed) {
        1 => e1000::flush_tx(),
        2 => rtl8139::flush_tx(),
        _ => {}
    }
}
