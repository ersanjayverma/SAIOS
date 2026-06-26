//! DNS resolver — sends UDP queries to a configured DNS server.

use super::arp;
use super::ethernet::{ETHERTYPE_ARP, ETHERTYPE_IPV4, EtherFrame, MacAddr};
use super::ip::{Ipv4Packet, PROTO_UDP};
use super::send_packet;
use super::udp::UdpPacket;
use crate::network_contract::NetworkContract;
use alloc::vec::Vec;

pub fn resolve_blocking(hostname: &str) -> Option<[u8; 4]> {
    // Localhost shortcuts
    if hostname == "localhost" {
        return Some([127, 0, 0, 1]);
    }

    // If it already looks like an IP, parse it
    if let Some(ip) = parse_ipv4(hostname) {
        return Some(ip);
    }

    // Make sure the link is up (first net op after boot waits for it).
    super::ensure_link();

    let dns_server = NetworkContract::dns_server();

    // Wall-clock-bounded wait (the timer IRQ advances boot_ticks while we spin):
    // a fixed iteration count elapses in microseconds — far less than the real
    // DNS round-trip through NAT to the internet — so a query always "timed out"
    // before the reply arrived.  ~18 Hz PIT ⇒ 150 ticks ≈ 8 s.  Each iteration
    // pumps the active NIC, caches ARP replies, and (once the next-hop MAC is
    // known) sends/re-sends the query.
    let next = arp::next_hop(dns_server); // gateway for off-subnet DNS servers
    let t0 = crate::shell::commands::boot_ticks();
    let mut last_q: u64 = 0;
    while crate::shell::commands::boot_ticks().wrapping_sub(t0) < 800 {
        // ~8 s @100Hz
        let now = crate::shell::commands::boot_ticks().wrapping_sub(t0);
        if arp::lookup(next).is_some() {
            // (Re)send the query when first able, then every ~0.5 s in case
            // the datagram was dropped.
            if last_q == 0 || now.wrapping_sub(last_q) >= 50 {
                send_query(hostname, 1 /* A record */);
                last_q = now.max(1);
            }
        } else {
            arp::send_request(next);
        }
        super::pump();
        if let Some(ip) = drain_rx() {
            return Some(ip);
        }
        x86_64::instructions::nop();
    }
    None
}

/// Drain the RX queue: cache ARP replies (so the server MAC resolves) and
/// return an answer IP if a DNS response (UDP src port 53) arrived.
fn drain_rx() -> Option<[u8; 4]> {
    let frames: Vec<Vec<u8>> = { NetworkContract::drain_rx() };
    let mut found = None;
    for raw in &frames {
        let Some(frame) = EtherFrame::parse(raw) else {
            continue;
        };
        match frame.ethertype {
            ETHERTYPE_ARP => arp::ingest(frame.payload),
            ETHERTYPE_IPV4 => {
                if let Some(ip) = Ipv4Packet::parse(frame.payload)
                    && ip.protocol == PROTO_UDP
                    && let Some(udp) = UdpPacket::parse(ip.payload)
                    && udp.src_port == 53
                    && udp.payload.len() > 12
                    && let Some(a) = parse_dns_response(udp.payload)
                {
                    found = Some(a);
                }
            }
            _ => {}
        }
        if found.is_some() {
            break;
        }
    }
    found
}

fn send_query(name: &str, qtype: u16) {
    let mut payload: Vec<u8> = Vec::new();
    let tx_id: u16 = 0x1337;
    payload.extend_from_slice(&tx_id.to_be_bytes());
    payload.extend_from_slice(&[0x01, 0x00]); // flags: recursion desired
    payload.extend_from_slice(&[0x00, 0x01]); // QDCOUNT=1
    payload.extend_from_slice(&[0x00, 0x00]); // ANCOUNT
    payload.extend_from_slice(&[0x00, 0x00]); // NSCOUNT
    payload.extend_from_slice(&[0x00, 0x00]); // ARCOUNT

    for label in name.split('.') {
        payload.push(label.len() as u8);
        payload.extend_from_slice(label.as_bytes());
    }
    payload.push(0x00); // root label
    payload.extend_from_slice(&qtype.to_be_bytes());
    payload.extend_from_slice(&[0x00, 0x01]); // QCLASS=IN

    let dns_server = NetworkContract::dns_server();
    let udp = UdpPacket::encode(54321, 53, &payload);
    let ip = Ipv4Packet::encode(NetworkContract::ip(), dns_server, PROTO_UDP, &udp);

    let next = arp::next_hop(dns_server);
    if let Some(gw_mac) = arp::lookup(next) {
        let frame = EtherFrame::encode(
            MacAddr(gw_mac),
            MacAddr(NetworkContract::mac()),
            ETHERTYPE_IPV4,
            &ip,
        );
        send_packet(frame);
    } else {
        arp::send_request(next);
    }
}

fn parse_dns_response(payload: &[u8]) -> Option<[u8; 4]> {
    if payload.len() < 12 {
        return None;
    }
    let qdcount = u16::from_be_bytes([payload[4], payload[5]]);
    let ancount = u16::from_be_bytes([payload[6], payload[7]]);
    if ancount == 0 {
        return None;
    }

    let mut i = 12;
    // Skip ALL questions (qdcount, usually 1).
    for _ in 0..qdcount {
        while i < payload.len() {
            let len = payload[i] as usize;
            if len & 0xC0 == 0xC0 {
                i += 2;
                break;
            } // compressed name pointer
            if len == 0 {
                i += 1;
                break;
            } // root label terminator
            i += len + 1;
        }
        i += 4; // QTYPE + QCLASS
    }

    // Iterate ALL answer records.  A hostname like deb.debian.org returns a
    // CNAME chain first (rtype=5); the A record (rtype=1) comes later — the old
    // parser only looked at the first answer and gave up on the CNAME.
    for _ in 0..ancount {
        if i + 10 > payload.len() {
            return None;
        }
        // Skip NAME (compressed pointer = 2 bytes, else a label sequence).
        if payload[i] & 0xC0 == 0xC0 {
            i += 2;
        } else {
            while i < payload.len() && payload[i] != 0 {
                i += payload[i] as usize + 1;
            }
            i += 1;
        }
        if i + 10 > payload.len() {
            return None;
        }
        let rtype = u16::from_be_bytes([payload[i], payload[i + 1]]);
        i += 2;
        i += 2; // CLASS
        i += 4; // TTL
        let rdlen = u16::from_be_bytes([payload[i], payload[i + 1]]) as usize;
        i += 2;
        if i + rdlen > payload.len() {
            return None;
        }
        if rtype == 1 && rdlen == 4 {
            return Some([payload[i], payload[i + 1], payload[i + 2], payload[i + 3]]);
        }
        i += rdlen; // CNAME / AAAA / etc. — skip and keep looking
    }
    None
}

fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut out = [0u8; 4];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p.parse().ok()?;
    }
    Some(out)
}
