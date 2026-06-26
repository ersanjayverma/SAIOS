//! OSI Layer 4 — UDP datagram encoding/decoding.

use alloc::vec::Vec;

#[derive(Debug)]
pub struct UdpPacket<'a> {
    pub src_port: u16,
    pub dst_port: u16,
    pub payload: &'a [u8],
}

impl<'a> UdpPacket<'a> {
    pub fn parse(raw: &'a [u8]) -> Option<Self> {
        if raw.len() < 8 {
            return None;
        }
        Some(Self {
            src_port: u16::from_be_bytes([raw[0], raw[1]]),
            dst_port: u16::from_be_bytes([raw[2], raw[3]]),
            payload: &raw[8..],
        })
    }

    pub fn encode(src_port: u16, dst_port: u16, payload: &[u8]) -> Vec<u8> {
        let len = (8 + payload.len()) as u16;
        let mut pkt = Vec::with_capacity(8 + payload.len());
        pkt.extend_from_slice(&src_port.to_be_bytes());
        pkt.extend_from_slice(&dst_port.to_be_bytes());
        pkt.extend_from_slice(&len.to_be_bytes());
        pkt.extend_from_slice(&[0x00, 0x00]); // checksum optional for UDP
        pkt.extend_from_slice(payload);
        pkt
    }
}
