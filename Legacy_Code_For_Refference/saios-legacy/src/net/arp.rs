//! OSI Layer 2 — ARP (Address Resolution Protocol).
//! Resolves IPv4 addresses to MAC addresses on the local network.

use super::ethernet::{ETHERTYPE_ARP, EtherFrame, MacAddr};
use super::{recv_packet, send_packet};
use crate::network_contract::NetworkContract;
use alloc::vec::Vec;

const ARP_REQUEST: u16 = 1;
const ARP_REPLY: u16 = 2;

/// Our MAC and IP — set during NIC init.
pub fn set_identity(mac: [u8; 6], ip: [u8; 4], driver: &'static str) {
    NetworkContract::set_identity(mac, ip, driver);
}

/// Resolve the next-hop IP for a destination: the destination itself if it's on
/// our subnet, otherwise the default gateway.  Off-subnet hosts (DNS servers,
/// Debian mirrors, …) must be reached via the gateway's MAC — VirtualBox/QEMU
/// NAT only answers ARP for the gateway, not for arbitrary public IPs, so ARPing
/// the destination directly silently failed and every connection timed out.
/// True for the loopback network (127.0.0.0/8) or this host's own address —
/// traffic to either is delivered back to us instead of going out the NIC.
pub fn is_local(ip: [u8; 4]) -> bool {
    NetworkContract::is_local(ip)
}

pub fn next_hop(dst: [u8; 4]) -> [u8; 4] {
    NetworkContract::next_hop(dst)
}

pub fn cache_insert(ip: [u8; 4], mac: [u8; 6]) {
    NetworkContract::cache_arp(ip, mac);
}

pub fn lookup(ip: [u8; 4]) -> Option<[u8; 6]> {
    NetworkContract::lookup_arp(ip)
}

pub fn send_request(target_ip: [u8; 4]) {
    let our_mac = NetworkContract::mac();
    let our_ip = NetworkContract::ip();
    let mut pkt = Vec::with_capacity(28);
    pkt.extend_from_slice(&[0x00, 0x01]); // HTYPE: Ethernet
    pkt.extend_from_slice(&[0x08, 0x00]); // PTYPE: IPv4
    pkt.push(6); // HLEN
    pkt.push(4); // PLEN
    pkt.extend_from_slice(&ARP_REQUEST.to_be_bytes());
    pkt.extend_from_slice(&our_mac);
    pkt.extend_from_slice(&our_ip);
    pkt.extend_from_slice(&[0u8; 6]); // target MAC unknown
    pkt.extend_from_slice(&target_ip);
    let frame = EtherFrame::encode(MacAddr::BROADCAST, MacAddr(our_mac), ETHERTYPE_ARP, &pkt);
    send_packet(frame);
}

/// Process any buffered ARP packets from the RX queue.
pub fn process_queue() {
    while let Some(raw) = recv_packet() {
        if let Some(frame) = EtherFrame::parse(&raw)
            && frame.ethertype == ETHERTYPE_ARP
        {
            handle_arp(frame.payload);
        }
    }
}

/// Ingest one ARP packet payload: learn the sender's MAC and reply if it's a
/// request for us.  Public so blocking protocol loops (e.g. DNS) can cache ARP
/// replies they pull off the RX queue themselves.
pub fn ingest(payload: &[u8]) {
    handle_arp(payload);
}

fn handle_arp(payload: &[u8]) {
    if payload.len() < 28 {
        return;
    }
    let op = u16::from_be_bytes([payload[6], payload[7]]);
    let src_mac: [u8; 6] = payload[8..14].try_into().unwrap_or([0u8; 6]);
    let src_ip: [u8; 4] = payload[14..18].try_into().unwrap_or([0u8; 4]);
    let dst_ip: [u8; 4] = payload[24..28].try_into().unwrap_or([0u8; 4]);

    // Learn sender's MAC regardless of op
    cache_insert(src_ip, src_mac);

    if op == ARP_REQUEST && dst_ip == NetworkContract::ip() {
        send_reply(src_mac, src_ip);
    }
}

fn send_reply(target_mac: [u8; 6], target_ip: [u8; 4]) {
    let our_mac = NetworkContract::mac();
    let our_ip = NetworkContract::ip();
    let mut pkt = Vec::with_capacity(28);
    pkt.extend_from_slice(&[0x00, 0x01]);
    pkt.extend_from_slice(&[0x08, 0x00]);
    pkt.push(6);
    pkt.push(4);
    pkt.extend_from_slice(&ARP_REPLY.to_be_bytes());
    pkt.extend_from_slice(&our_mac);
    pkt.extend_from_slice(&our_ip);
    pkt.extend_from_slice(&target_mac);
    pkt.extend_from_slice(&target_ip);
    let frame = EtherFrame::encode(MacAddr(target_mac), MacAddr(our_mac), ETHERTYPE_ARP, &pkt);
    send_packet(frame);
}
