//! OSI Layer 3 — IPv4 packet encoding/decoding.

use alloc::vec::Vec;
use core::sync::atomic::AtomicU32;

/// Monotonic IPv4 identification field source (incremented per emitted packet).
static IP_ID: AtomicU32 = AtomicU32::new(1);

pub const PROTO_ICMP: u8 = 1;
pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;

#[derive(Debug)]
pub struct Ipv4Packet<'a> {
    pub src: [u8; 4],
    pub dst: [u8; 4],
    pub protocol: u8,
    pub ttl: u8,
    pub payload: &'a [u8],
}

impl<'a> Ipv4Packet<'a> {
    pub fn parse(raw: &'a [u8]) -> Option<Self> {
        if raw.len() < 20 {
            return None;
        }
        let version_ihl = raw[0];
        if (version_ihl >> 4) != 4 {
            return None;
        }
        let ihl = ((version_ihl & 0x0F) * 4) as usize;
        if raw.len() < ihl {
            return None;
        }
        // Trim to the IP total-length field: Ethernet pads frames to a 60-byte
        // minimum, and those padding bytes must NOT be treated as payload — doing
        // so appends garbage to a TCP stream (corrupting e.g. a downloaded gzip
        // on its small final segment) and breaks checksum verification.
        let total_len = u16::from_be_bytes([raw[2], raw[3]]) as usize;
        let end = if total_len >= ihl && total_len <= raw.len() {
            total_len
        } else {
            raw.len()
        };
        Some(Self {
            ttl: raw[8],
            protocol: raw[9],
            src: raw[12..16].try_into().ok()?,
            dst: raw[16..20].try_into().ok()?,
            payload: &raw[ihl..end],
        })
    }

    pub fn encode(src: [u8; 4], dst: [u8; 4], protocol: u8, payload: &[u8]) -> Vec<u8> {
        let total_len = (20 + payload.len()) as u16;
        let mut pkt = Vec::with_capacity(20 + payload.len());
        pkt.push(0x45); // version=4, IHL=5
        pkt.push(0x00); // DSCP/ECN
        pkt.extend_from_slice(&total_len.to_be_bytes());
        // Incrementing IP identification, like a real stack (Linux bumps it per
        // packet).  A constant ID can confuse NAT/middlebox connection tracking.
        let id = IP_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed) as u16;
        pkt.extend_from_slice(&id.to_be_bytes());
        pkt.extend_from_slice(&[0x40, 0x00]); // flags: DF, frag offset 0
        pkt.push(64); // TTL
        pkt.push(protocol);
        pkt.extend_from_slice(&[0x00, 0x00]); // checksum placeholder
        pkt.extend_from_slice(&src);
        pkt.extend_from_slice(&dst);
        pkt.extend_from_slice(payload);

        let csum = checksum(&pkt[..20]);
        pkt[10] = (csum >> 8) as u8;
        pkt[11] = csum as u8;
        pkt
    }
}

pub fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}
