//! OSI Layer 2 — Ethernet frame encoding/decoding.

use alloc::vec::Vec;

pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;
pub const ETHERTYPE_IPV6: u16 = 0x86DD;

#[derive(Debug, Clone)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const BROADCAST: MacAddr = MacAddr([0xFF; 6]);
    pub const ZERO: MacAddr = MacAddr([0x00; 6]);
}

#[derive(Debug)]
pub struct EtherFrame<'a> {
    pub dst: MacAddr,
    pub src: MacAddr,
    pub ethertype: u16,
    pub payload: &'a [u8],
}

impl<'a> EtherFrame<'a> {
    pub fn parse(raw: &'a [u8]) -> Option<Self> {
        if raw.len() < 14 {
            return None;
        }
        Some(Self {
            dst: MacAddr(raw[0..6].try_into().ok()?),
            src: MacAddr(raw[6..12].try_into().ok()?),
            ethertype: u16::from_be_bytes([raw[12], raw[13]]),
            payload: &raw[14..],
        })
    }

    pub fn encode(dst: MacAddr, src: MacAddr, ethertype: u16, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(14 + payload.len());
        frame.extend_from_slice(&dst.0);
        frame.extend_from_slice(&src.0);
        frame.push((ethertype >> 8) as u8);
        frame.push(ethertype as u8);
        frame.extend_from_slice(payload);
        frame
    }
}
