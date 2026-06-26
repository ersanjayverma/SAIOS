//! SAIOS network stack — implements OSI layers 2–4 from scratch.
//!
//! Layer 2: Ethernet  (ethernet.rs)
//! Layer 2: ARP       (arp.rs)
//! Layer 3: IPv4      (ip.rs)
//! Layer 4: UDP       (udp.rs)
//! Layer 4: TCP       (tcp.rs)
//! Layer 5–7: DNS, HTTP (dns.rs, http.rs)
//! NIC driver: VirtIO-Net (virtio.rs)

pub mod arp;
pub mod dns;
pub mod ethernet;
pub mod filter;
pub mod http;
pub mod ip;
pub mod ipv6;
pub mod socket;
pub mod tcp;
pub mod tls;
pub mod udp;
pub mod virtio;

use crate::network_contract::NetworkContract;
use alloc::vec::Vec;

pub fn send_packet(frame: Vec<u8>) {
    // Loopback: an IPv4 frame addressed to 127.0.0.0/8 or our own IP is delivered
    // straight back into the receive queue instead of going out the NIC, so
    // 127.0.0.1 (and self-connections) work with no hardware.  Ethernet layout:
    // dst(6) src(6) ethertype(2) | IPv4 header (dst IP at IP+16 = frame+30).
    if frame.len() >= 34 && frame[12] == 0x08 && frame[13] == 0x00 {
        let dst = [frame[30], frame[31], frame[32], frame[33]];
        if arp::is_local(dst) {
            NetworkContract::enqueue_rx(frame, "loopback");
            return;
        }
    }
    NetworkContract::enqueue_tx(frame);
}

/// Ensure the wired link is up before a network operation.  VirtualBox's e1000
/// raises STATUS.LU only ~5 s (wall-clock) after CTRL.SLU, so the very first
/// network command after boot must wait for it.  Cheap (returns immediately)
/// once the link is already up.  MUST be called from task context (uses hlt).
pub fn ensure_link() {
    if crate::driver::net::e1000::present() {
        // bring_link_up is marked as unsafe because it uses hlt() in a loop
        // but it's safe to call from task context where interrupts are enabled
        let _ = crate::driver::net::e1000::bring_link_up();
    }
}

pub fn recv_packet() -> Option<Vec<u8>> {
    NetworkContract::recv_rx()
}

/// Pump the active wired NIC once: flush queued TX frames out and harvest any
/// received frames into RX_QUEUE.  Blocking protocol loops (DNS/TCP/HTTP/TLS)
/// MUST call this instead of `virtio::poll_rx()` directly — otherwise, when a
/// hardware NIC (e1000/rtl8139) is the active link, their packets are never
/// transmitted and replies are never received (the symptom: IP is assigned but
/// nothing connects).  Mirrors the NIC selection in `poll()`.
pub fn pump() {
    if crate::driver::net::hw_nic_active() {
        crate::driver::net::flush_tx();
        crate::driver::net::poll_rx();
    } else {
        virtio::flush_tx();
        virtio::poll_rx();
    }
}

/// During a long blocking network wait (e.g. an AI request), call this once per
/// loop iteration: it animates a "thinking" dot (throttled) and returns `true`
/// if the user pressed Ctrl+C, so the caller can cancel.  There is no timeout —
/// the caller loops until the operation completes or this returns true.
pub fn wait_spin() -> bool {
    use core::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = crate::shell::commands::boot_ticks();
    let mut emitted_progress = false;
    if now.wrapping_sub(LAST.load(Ordering::Relaxed)) >= 40 {
        // ~0.4 s
        LAST.store(now, Ordering::Relaxed);
        crate::print!(".");
        emitted_progress = true;
    }
    let canceled = matches!(
        crate::driver::keyboard::poll(),
        Some(crate::driver::keyboard::KeyEvent::Char('\x03'))
    );
    if emitted_progress || canceled {
        NetworkContract::record_wait_progress("network.wait.progress", now, canceled);
    }
    canceled
}

/// Drive the network stack — called from the main shell loop.
///
/// Only one wired driver is active at a time:
///   hardware NIC (e1000/rtl8139) takes priority over VirtIO-Net.
/// Wi-Fi is always polled independently.
pub fn poll() {
    if crate::driver::net::hw_nic_active() {
        // Hardware NIC (e1000 or rtl8139) is driving the wired link
        crate::driver::net::poll_rx();
        crate::driver::net::flush_tx();
    } else {
        // Fall back to VirtIO-Net (QEMU user-mode or VirtIO adapter)
        virtio::poll_rx();
        virtio::flush_tx();
    }
    // Wi-Fi always polled regardless of wired state
    crate::driver::wifi::poll();
    arp::process_queue();
}
