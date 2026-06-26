//! OSI Layer 3 — IPv6 (RFC 8200) with ICMPv6 echo + Neighbor Discovery.
//!
//! Foundational layer: parse/encode the 40-byte IPv6 header, derive a
//! link-local address (fe80::/64 + EUI-64 from the MAC), and answer ICMPv6
//! Echo Requests and Neighbor Solicitations on the local link.  TCP/UDP over
//! IPv6 and SLAAC/routing are tracked follow-ups (the NAT used here is v4-only).

use super::ethernet::{ETHERTYPE_IPV6, EtherFrame, MacAddr};
use crate::network_contract::NetworkContract;
use alloc::vec::Vec;
use spin::Mutex;

const NEXT_HDR_ICMPV6: u8 = 58;
const ICMP6_ECHO_REQUEST: u8 = 128;
const ICMP6_ECHO_REPLY: u8 = 129;
const ICMP6_NEIGHBOR_SOL: u8 = 135;
const ICMP6_NEIGHBOR_ADV: u8 = 136;

/// This host's link-local address (fe80::/64 + EUI-64), set at init.
pub static LINK_LOCAL: Mutex<[u8; 16]> = Mutex::new([0u8; 16]);

/// Derive the link-local address from the NIC MAC (EUI-64) and store it.
pub fn init() {
    let mac = NetworkContract::mac();
    let mut a = [0u8; 16];
    a[0] = 0xFE;
    a[1] = 0x80; // fe80::/64
    a[8] = mac[0] ^ 0x02; // flip U/L bit
    a[9] = mac[1];
    a[10] = mac[2];
    a[11] = 0xFF;
    a[12] = 0xFE; // EUI-64 insert
    a[13] = mac[3];
    a[14] = mac[4];
    a[15] = mac[5];
    *LINK_LOCAL.lock() = a;
    crate::println!(
        "[ipv6] link-local fe80::{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}",
        a[8],
        a[9],
        a[10],
        a[11],
        a[12],
        a[13],
        a[14],
        a[15]
    );
}

fn link_local() -> [u8; 16] {
    *LINK_LOCAL.lock()
}

/// Handle an inbound IPv6 packet (the Ethernet payload).
pub fn handle(eth_src: &MacAddr, pkt: &[u8]) {
    if pkt.len() < 40 {
        return;
    }
    if pkt[0] >> 4 != 6 {
        return;
    } // version must be 6
    let next_hdr = pkt[6];
    let src: [u8; 16] = pkt[8..24].try_into().unwrap();
    let dst: [u8; 16] = pkt[24..40].try_into().unwrap();
    let payload = &pkt[40..];
    if next_hdr == NEXT_HDR_ICMPV6 {
        handle_icmp6(eth_src, src, dst, payload);
    }
}

fn handle_icmp6(eth_src: &MacAddr, src: [u8; 16], dst: [u8; 16], icmp: &[u8]) {
    if icmp.len() < 4 {
        return;
    }
    match icmp[0] {
        ICMP6_ECHO_REQUEST => {
            // Reply: swap src/dst, type 129, recompute checksum.
            let mut body = icmp.to_vec();
            body[0] = ICMP6_ECHO_REPLY;
            body[2] = 0;
            body[3] = 0;
            send(link_local(), src, NEXT_HDR_ICMPV6, &body, eth_src);
        }
        ICMP6_NEIGHBOR_SOL if icmp.len() >= 24 => {
            // Target address is at offset 8..24 of the NS.  Answer only if it's us.
            let target: [u8; 16] = icmp[8..24].try_into().unwrap();
            if target == link_local() {
                let mut na = alloc::vec![0u8; 32];
                na[0] = ICMP6_NEIGHBOR_ADV;
                na[4] = 0x60; // flags: solicited + override
                na[8..24].copy_from_slice(&target);
                // Target link-layer address option (type 2, len 1).
                na[24] = 2;
                na[25] = 1;
                na[26..32].copy_from_slice(&NetworkContract::mac()[..]);
                let _ = dst;
                send(target, src, NEXT_HDR_ICMPV6, &na, eth_src);
            }
        }
        _ => {}
    }
}

/// Build + send an IPv6 packet to `dst` (Ethernet dst = `eth_dst`).
fn send(src: [u8; 16], dst: [u8; 16], next_hdr: u8, payload: &[u8], eth_dst: &MacAddr) {
    let mut pkt = Vec::with_capacity(40 + payload.len());
    pkt.push(0x60); // version 6, traffic class 0
    pkt.extend_from_slice(&[0, 0, 0]); // flow label
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes()); // payload length
    pkt.push(next_hdr);
    pkt.push(64); // hop limit
    pkt.extend_from_slice(&src);
    pkt.extend_from_slice(&dst);
    let mut body = payload.to_vec();
    if next_hdr == NEXT_HDR_ICMPV6 {
        let csum = icmp6_checksum(&src, &dst, &body);
        body[2] = (csum >> 8) as u8;
        body[3] = csum as u8;
    }
    pkt.extend_from_slice(&body);

    let our_mac = NetworkContract::mac();
    let frame = EtherFrame::encode(eth_dst.clone(), MacAddr(our_mac), ETHERTYPE_IPV6, &pkt);
    super::send_packet(frame);
}

/// ICMPv6 checksum over the IPv6 pseudo-header + message (csum field zeroed).
fn sum16(sum: &mut u32, b: &[u8]) {
    let mut i = 0;
    while i + 1 < b.len() {
        *sum += ((b[i] as u32) << 8) | b[i + 1] as u32;
        i += 2;
    }
    if i < b.len() {
        *sum += (b[i] as u32) << 8;
    }
}

fn icmp6_checksum(src: &[u8; 16], dst: &[u8; 16], msg: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    sum16(&mut sum, src);
    sum16(&mut sum, dst);
    sum += msg.len() as u32; // upper-layer length (32-bit, hi=0)
    sum += NEXT_HDR_ICMPV6 as u32; // next header
    // message with checksum field (bytes 2..4) treated as zero
    let mut m = msg.to_vec();
    if m.len() >= 4 {
        m[2] = 0;
        m[3] = 0;
    }
    sum16(&mut sum, &m);
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}
