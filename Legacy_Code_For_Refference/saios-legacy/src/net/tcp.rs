//! OSI Layer 4 — TCP connection state machine.

use super::arp;
use super::ethernet::{ETHERTYPE_ARP, ETHERTYPE_IPV4, EtherFrame, MacAddr};
use super::ip::{Ipv4Packet, PROTO_TCP};
use super::send_packet;
use crate::network_contract::NetworkContract;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

const SYN: u8 = 0x02;
const ACK: u8 = 0x10;
const FIN: u8 = 0x01;
const RST: u8 = 0x04;
const PSH: u8 = 0x08;

#[derive(Debug, Clone, PartialEq)]
pub enum TcpState {
    Closed,
    SynSent,
    Established,
    CloseWait,
    LastAck,
    FinWait1,
    TimeWait,
}

use core::sync::atomic::{AtomicU64, Ordering};
/// Diagnostic counters: in-order bytes accepted, out-of-order segments buffered,
/// duplicate/old segments seen.
pub static RX_INORDER: AtomicU64 = AtomicU64::new(0);
pub static RX_OOO: AtomicU64 = AtomicU64::new(0);
pub static RX_DUP: AtomicU64 = AtomicU64::new(0);
/// Count of ACK segments we transmit (to spot ACK-flooding of the NAT).
pub static TX_ACKS: AtomicU64 = AtomicU64::new(0);
pub fn rx_stats() -> (u64, u64, u64) {
    (
        RX_INORDER.load(Ordering::Relaxed),
        RX_OOO.load(Ordering::Relaxed),
        RX_DUP.load(Ordering::Relaxed),
    )
}
pub fn tx_acks() -> u64 {
    TX_ACKS.load(Ordering::Relaxed)
}

/// True if sequence number `a` is strictly after `b` (mod 2^32).
fn seq_after(a: u32, b: u32) -> bool {
    let d = a.wrapping_sub(b);
    d != 0 && d < 0x8000_0000
}

#[derive(Debug, Clone)]
pub struct TcpSocket {
    pub state: TcpState,
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
    pub src_port: u16,
    pub dst_port: u16,
    pub isn: u32, // initial send sequence number (for SYN retransmit)
    pub seq: u32,
    pub ack: u32,
    pub rx_buf: Vec<u8>,
    pub last_tx_seq: u32,
    pub last_tx: Vec<u8>,
    /// A cumulative ACK is owed but not yet sent.  We coalesce per-segment ACKs
    /// into one ACK per poll cycle: sending an ACK for every segment overflowed
    /// the 32-entry TX ring during bursts, so most ACKs were dropped and the peer
    /// retransmitted endlessly.  One cumulative ACK per batch fixes that.
    pub ack_pending: bool,
    /// Out-of-order segments buffered until the gap before them is filled, keyed
    /// by their starting sequence number.  Without this, a single dropped
    /// segment in a large download discards every later segment (we only accept
    /// strictly in-order data), so the transfer advances one MSS per retransmit
    /// timeout and stalls — the root cause of truncated `apt` downloads.
    pub ooo: BTreeMap<u32, Vec<u8>>,
    /// Initial receive sequence number (server ISN) — lets diagnostics print
    /// byte offsets rather than raw 32-bit seqs.
    pub irs: u32,
    /// Ring of the last received data segments (seq, len) for post-mortem dump
    /// on an unexpected RST.
    pub rx_recent: [(u32, u32); 16],
    pub rx_recent_n: usize,
    /// Last advertised receive window we sent (for RST post-mortem).
    pub last_wnd: u16,
}

impl TcpSocket {
    pub fn new(src_ip: [u8; 4], dst_ip: [u8; 4], src_port: u16, dst_port: u16) -> Self {
        Self {
            state: TcpState::Closed,
            src_ip,
            dst_ip,
            src_port,
            dst_port,
            isn: 0xDEAD_BEEF,
            seq: 0xDEAD_BEEF,
            ack: 0,
            rx_buf: Vec::new(),
            last_tx_seq: 0,
            last_tx: Vec::new(),
            ack_pending: false,
            ooo: BTreeMap::new(),
            irs: 0,
            rx_recent: [(0, 0); 16],
            rx_recent_n: 0,
            last_wnd: 0xFFFF,
        }
    }

    pub fn connect(&mut self) {
        self.isn = self.seq;
        // The first SYN is often dropped because ARP hasn't resolved the next-hop
        // MAC yet (send_segment then only fires an ARP request).  resend_syn()
        // retransmits it from the connect wait loop once ARP is warm.
        self.send_segment_seq(self.isn, SYN, &[]);
        let from = tcp_state_code(&self.state);
        self.state = TcpState::SynSent;
        NetworkContract::record_tcp_state(
            self.src_port,
            self.dst_ip,
            self.dst_port,
            from,
            tcp_state_code(&self.state),
            "network.tcp.syn_sent",
        );
        self.seq = self.isn.wrapping_add(1);
    }

    /// Retransmit the SYN (same ISN) if we're still waiting for SYN/ACK.
    pub fn resend_syn(&mut self) {
        if self.state == TcpState::SynSent {
            self.send_segment_seq(self.isn, SYN, &[]);
        }
    }

    pub fn send(&mut self, data: &[u8]) {
        if self.state == TcpState::Established {
            self.last_tx_seq = self.seq;
            self.last_tx.clear();
            self.last_tx.extend_from_slice(data);
            self.send_segment(PSH | ACK, data);
            self.seq = self.seq.wrapping_add(data.len() as u32);
        }
    }

    pub fn resend_last_data(&mut self) {
        if self.state == TcpState::Established && !self.last_tx.is_empty() {
            self.send_segment_seq(self.last_tx_seq, PSH | ACK, &self.last_tx);
        }
    }

    pub fn close(&mut self) {
        self.send_segment(FIN | ACK, &[]);
        let from = tcp_state_code(&self.state);
        self.state = TcpState::FinWait1;
        NetworkContract::record_tcp_state(
            self.src_port,
            self.dst_ip,
            self.dst_port,
            from,
            tcp_state_code(&self.state),
            "network.tcp.close",
        );
    }

    pub fn recv(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.rx_buf)
    }

    pub fn handle_segment(&mut self, flags: u8, seq: u32, ack: u32, payload: &[u8]) {
        // A reset tears the connection down immediately.  Without this, when the
        // NAT/peer RSTs (e.g. an unreachable Ollama backend after slirp's
        // optimistic SYN-ACK), we ignored it and blocked the full stall timeout
        // before reporting "no response".
        if flags & RST != 0 {
            // Post-mortem: does the server's RST ack match our SND.NXT?  If their
            // ack != our self.seq, the server saw different send-side data than we
            // think we sent (a TX seq/ACK bug).  rx_off = bytes we've accepted.
            let rx_off = self.ack.wrapping_sub(self.irs).wrapping_sub(1);
            let snd_off = self.seq.wrapping_sub(self.isn);
            let their_ack_off = ack.wrapping_sub(self.isn);
            crate::serial_println!(
                "[tcp] RST: rx_bytes={} rcv_nxt={} their_seq={} their_ack_off={} our_snd_off={} ooo={} last_wnd={}",
                rx_off,
                self.ack,
                seq,
                their_ack_off,
                snd_off,
                self.ooo.len(),
                self.last_wnd
            );
            if their_ack_off != snd_off {
                crate::serial_println!(
                    "[tcp]   !! their RST acks snd_off={} but our SND.NXT off={} (send-side seq mismatch)",
                    their_ack_off,
                    snd_off
                );
            }
            // Dump the last few received segments to confirm contiguity (a
            // protocol bug would show a gap/overlap here; corruption a random
            // offset).
            let n = self.rx_recent_n;
            let start = n.saturating_sub(4);
            for i in start..n {
                let (off, len) = self.rx_recent[i % 16];
                crate::serial_println!("[tcp]   rx[{}] off={} len={}", i, off, len);
            }
            let from = tcp_state_code(&self.state);
            self.state = TcpState::Closed;
            NetworkContract::record_tcp_failure(
                self.src_port,
                self.dst_ip,
                self.dst_port,
                RST as u64,
            );
            NetworkContract::record_tcp_state(
                self.src_port,
                self.dst_ip,
                self.dst_port,
                from,
                tcp_state_code(&self.state),
                "network.tcp.reset",
            );
            return;
        }
        match self.state {
            TcpState::SynSent if flags & (SYN | ACK) == (SYN | ACK) => {
                self.irs = seq; // server's initial receive sequence number
                self.ack = seq.wrapping_add(1);
                self.send_segment(ACK, &[]);
                let from = tcp_state_code(&self.state);
                self.state = TcpState::Established;
                NetworkContract::record_tcp_state(
                    self.src_port,
                    self.dst_ip,
                    self.dst_port,
                    from,
                    tcp_state_code(&self.state),
                    "network.tcp.established",
                );
            }
            TcpState::Established => {
                // Accept IN-ORDER data only (seq == our cumulative rcv_nxt, kept
                // in self.ack).  For a duplicate (already-received) or
                // out-of-order (gap) segment, do NOT append and do NOT advance
                // the ACK — just re-ACK the current rcv_nxt so the peer
                // retransmits the byte we actually need.  The old code advanced
                // the ACK to seq+len for ANY segment, corrupting the stream and
                // stalling large downloads partway through (partial body).
                if !payload.is_empty() {
                    // Record (offset-from-IRS, len) for the RST post-mortem ring.
                    self.rx_recent[self.rx_recent_n % 16] =
                        (seq.wrapping_sub(self.irs), payload.len() as u32);
                    self.rx_recent_n += 1;
                    if seq == self.ack {
                        RX_INORDER.fetch_add(payload.len() as u64, Ordering::Relaxed);
                        // In-order: append, advance rcv_nxt, then stitch in any
                        // buffered out-of-order segments that are now contiguous.
                        self.rx_buf.extend_from_slice(payload);
                        self.ack = self.ack.wrapping_add(payload.len() as u32);
                        while let Some(&s) = self.ooo.keys().next() {
                            if s == self.ack {
                                let data = self.ooo.remove(&s).unwrap();
                                self.ack = self.ack.wrapping_add(data.len() as u32);
                                self.rx_buf.extend_from_slice(&data);
                            } else if seq_after(self.ack, s) {
                                self.ooo.remove(&s); // wholly old/overlapping — discard
                            } else {
                                break; // still a gap before `s`
                            }
                        }
                    } else if seq_after(seq, self.ack) {
                        // Future segment (a gap precedes it): buffer it so the
                        // single retransmit of the missing bytes unblocks the
                        // whole run.  Bounded by the advertised window (~64 KiB)
                        // plus a hard cap.  Dup-ACK to prompt fast retransmit.
                        if seq.wrapping_sub(self.ack) < 1_048_576
                            && self.ooo.len() < 1024
                            && !self.ooo.contains_key(&seq)
                        {
                            RX_OOO.fetch_add(1, Ordering::Relaxed);
                            self.ooo.insert(seq, payload.to_vec());
                        }
                    } else {
                        RX_DUP.fetch_add(1, Ordering::Relaxed);
                    }
                    // Owe a cumulative ACK; it is coalesced and sent once per
                    // poll cycle (see flush_acks) to avoid TX-ring overflow.
                    self.ack_pending = true;
                }
                // Honour FIN only once it is in order (all preceding data
                // received), so we don't close on an out-of-order FIN.
                if flags & FIN != 0 {
                    if seq.wrapping_add(payload.len() as u32) == self.ack {
                        crate::serial_println!("[tcp] FIN in-order: rcv_nxt={}", self.ack);
                        self.ack = self.ack.wrapping_add(1); // FIN consumes one seq
                        self.send_segment(ACK, &[]);
                        let from = tcp_state_code(&self.state);
                        self.state = TcpState::CloseWait;
                        NetworkContract::record_tcp_state(
                            self.src_port,
                            self.dst_ip,
                            self.dst_port,
                            from,
                            tcp_state_code(&self.state),
                            "network.tcp.close_wait",
                        );
                        self.send_segment(FIN | ACK, &[]);
                        let from = tcp_state_code(&self.state);
                        self.state = TcpState::LastAck;
                        NetworkContract::record_tcp_state(
                            self.src_port,
                            self.dst_ip,
                            self.dst_port,
                            from,
                            tcp_state_code(&self.state),
                            "network.tcp.last_ack",
                        );
                    } else {
                        crate::serial_println!(
                            "[tcp] FIN out-of-order (gap): fin_seq={} rcv_nxt={} ooo={}",
                            seq.wrapping_add(payload.len() as u32),
                            self.ack,
                            self.ooo.len()
                        );
                    }
                }
            }
            TcpState::LastAck if flags & ACK != 0 => {
                let from = tcp_state_code(&self.state);
                self.state = TcpState::Closed;
                NetworkContract::record_tcp_state(
                    self.src_port,
                    self.dst_ip,
                    self.dst_port,
                    from,
                    tcp_state_code(&self.state),
                    "network.tcp.closed",
                );
            }
            TcpState::FinWait1 if flags & (FIN | ACK) == (FIN | ACK) => {
                self.ack = seq.wrapping_add(1);
                self.send_segment(ACK, &[]);
                let from = tcp_state_code(&self.state);
                self.state = TcpState::TimeWait;
                NetworkContract::record_tcp_state(
                    self.src_port,
                    self.dst_ip,
                    self.dst_port,
                    from,
                    tcp_state_code(&self.state),
                    "network.tcp.time_wait",
                );
            }
            _ => {}
        }
    }

    /// Receive window to advertise.  A dynamic window (capacity − unconsumed
    /// bytes) was tested and proven NOT to be the cause of the mid-transfer RST
    /// (the peer reset regardless), while it throttled throughput and let the
    /// reset cut us off sooner.  We therefore advertise the full 64 KiB: our
    /// consumer drains rx_buf every loop, so the buffer is effectively always
    /// free, which makes a constant full window correct as well as fastest.
    fn advertised_window(&self) -> u16 {
        65535
    }

    fn send_segment(&self, flags: u8, payload: &[u8]) {
        self.send_segment_seq(self.seq, flags, payload);
    }

    fn send_segment_seq(&self, seq: u32, flags: u8, payload: &[u8]) {
        let tcp_pkt = encode_tcp(
            self.src_ip,
            self.dst_ip,
            self.src_port,
            self.dst_port,
            seq,
            self.ack,
            flags,
            self.advertised_window(),
            payload,
        );
        let ip_pkt = Ipv4Packet::encode(self.src_ip, self.dst_ip, PROTO_TCP, &tcp_pkt);
        // Off-subnet destinations are reached via the gateway's MAC.
        let next = arp::next_hop(self.dst_ip);
        if let Some(dst_mac) = arp::lookup(next) {
            let our_mac = NetworkContract::mac();
            let frame =
                EtherFrame::encode(MacAddr(dst_mac), MacAddr(our_mac), ETHERTYPE_IPV4, &ip_pkt);
            send_packet(frame);
        } else {
            arp::send_request(next);
        }
    }
}

fn tcp_state_code(state: &TcpState) -> u64 {
    match state {
        TcpState::Closed => 0,
        TcpState::SynSent => 1,
        TcpState::Established => 2,
        TcpState::CloseWait => 3,
        TcpState::LastAck => 4,
        TcpState::FinWait1 => 5,
        TcpState::TimeWait => 6,
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_tcp(
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
) -> Vec<u8> {
    let mut seg = Vec::with_capacity(20 + payload.len());
    seg.extend_from_slice(&src_port.to_be_bytes());
    seg.extend_from_slice(&dst_port.to_be_bytes());
    seg.extend_from_slice(&seq.to_be_bytes());
    seg.extend_from_slice(&ack.to_be_bytes());
    seg.push(0x50); // data offset = 5 (20 bytes)
    seg.push(flags);
    seg.extend_from_slice(&window.to_be_bytes()); // advertised receive window
    seg.extend_from_slice(&[0x00, 0x00]); // checksum placeholder (bytes 16-17)
    seg.extend_from_slice(&[0x00, 0x00]); // urgent pointer
    seg.extend_from_slice(payload);

    // TCP checksum over the pseudo-header + segment.  A zero/absent checksum is
    // INVALID for TCP — peers (and VirtualBox NAT) drop such segments, so the
    // connection never establishes.  Must be computed for the link to work.
    let csum = tcp_checksum(src_ip, dst_ip, &seg);
    seg[16] = (csum >> 8) as u8;
    seg[17] = (csum & 0xFF) as u8;
    seg
}

/// TCP checksum: ones-complement sum over the IPv4 pseudo-header followed by
/// the TCP segment (with its checksum field zeroed).
fn tcp_checksum(src: [u8; 4], dst: [u8; 4], seg: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    // Pseudo-header: src(4) dst(4) zero(1) proto(1) tcp_len(2)
    sum += ((src[0] as u32) << 8) | src[1] as u32;
    sum += ((src[2] as u32) << 8) | src[3] as u32;
    sum += ((dst[0] as u32) << 8) | dst[1] as u32;
    sum += ((dst[2] as u32) << 8) | dst[3] as u32;
    sum += PROTO_TCP as u32; // zero byte + protocol
    sum += seg.len() as u32; // TCP length
    // Segment, 16-bit words (checksum field is already 0)
    let mut i = 0;
    while i + 1 < seg.len() {
        sum += ((seg[i] as u32) << 8) | seg[i + 1] as u32;
        i += 2;
    }
    if i < seg.len() {
        sum += (seg[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Drive incoming TCP: drain the RX queue, cache ARP, and deliver TCP segments
/// to their sockets.  Without this, `handle_segment` is never called and
/// `tcp::read` always returns empty — so HTTP/TLS receive nothing.  Blocking
/// callers (http/tls) must call this after `net::pump()`.
pub fn poll() {
    let frames: Vec<Vec<u8>> = NetworkContract::drain_rx();
    for raw in &frames {
        let Some(frame) = EtherFrame::parse(raw) else {
            continue;
        };
        match frame.ethertype {
            ETHERTYPE_ARP => arp::ingest(frame.payload),
            ETHERTYPE_IPV4 => {
                let Some(ip) = Ipv4Packet::parse(frame.payload) else {
                    continue;
                };
                if ip.protocol != PROTO_TCP {
                    continue;
                }
                let seg = ip.payload;
                if seg.len() < 20 {
                    continue;
                }
                // Verify the TCP checksum and DROP corrupt segments — the peer
                // will retransmit.  A valid segment's ones-complement sum is 0.
                // Skip when the on-wire checksum field is 0: virtual NICs / NAT
                // paths often deliver packets with checksum offload (field left
                // 0 / pre-validated), and dropping those breaks the link
                // entirely.  (The real fix for the gzip corruption was trimming
                // Ethernet padding in ip.rs; this is defence-in-depth.)
                let recv_csum = u16::from_be_bytes([seg[16], seg[17]]);
                if recv_csum != 0 && seg.len() <= 1480 && tcp_checksum(ip.src, ip.dst, seg) != 0 {
                    RX_DUP.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                let rem_port = u16::from_be_bytes([seg[0], seg[1]]); // sender's port
                let our_port = u16::from_be_bytes([seg[2], seg[3]]);
                let seq = u32::from_be_bytes([seg[4], seg[5], seg[6], seg[7]]);
                let ack = u32::from_be_bytes([seg[8], seg[9], seg[10], seg[11]]);
                let data_off = ((seg[12] >> 4) as usize) * 4;
                let flags = seg[13];
                let payload: &[u8] = if seg.len() > data_off {
                    &seg[data_off..]
                } else {
                    &[]
                };
                // Socket key = (our src_port, remote ip, remote port).
                let key = (our_port, ip.src, rem_port);
                if let Some(sock) = SOCKETS.lock().get_mut(&key) {
                    sock.handle_segment(flags, seq, ack, payload);
                }
            }
            super::ethernet::ETHERTYPE_IPV6 => {
                super::ipv6::handle(&frame.src, frame.payload);
            }
            _ => {}
        }
    }
    // Send one coalesced cumulative ACK per socket that received data this batch.
    for sock in SOCKETS.lock().values_mut() {
        if sock.state != TcpState::Established {
            continue;
        }
        let wnd = sock.advertised_window();
        // Send an ACK for newly received data, OR a pure window-update if the
        // window has reopened from a constrained value (prevents a zero-window
        // deadlock: the sender would otherwise wait for a window update we'd
        // never send absent new data).
        let reopened = wnd > sock.last_wnd && sock.last_wnd < 8192;
        if sock.ack_pending || reopened {
            sock.last_wnd = wnd;
            sock.send_segment(ACK, &[]);
            sock.ack_pending = false;
            let n = TX_ACKS.fetch_add(1, Ordering::Relaxed);
            if n.is_multiple_of(512) {
                crate::serial_println!(
                    "[tcp] ACK#{} rcv_nxt={} wnd={} rx_used={}",
                    n,
                    sock.ack,
                    wnd,
                    sock.rx_buf.len()
                );
            }
        }
    }
}

/// Global socket table keyed by (src_port, dst_ip, dst_port)
type SocketKey = (u16, [u8; 4], u16);

static SOCKETS: Mutex<BTreeMap<SocketKey, TcpSocket>> = Mutex::new(BTreeMap::new());

/// Monotonic ephemeral-port allocator.  A fixed (deterministic) src_port made
/// every connection to a given dst_port reuse the same 4-tuple; on a quick
/// reconnect (e.g. resumable Range downloads) that tuple is still in TIME_WAIT
/// on the NAT/peer, so the new connection got a garbage/empty response.  Cycling
/// the source port gives each connection a fresh tuple.
static NEXT_PORT: AtomicU64 = AtomicU64::new(49152);

pub fn open(dst_ip: [u8; 4], dst_port: u16) -> u16 {
    let our_ip = NetworkContract::ip();
    let mut sockets = SOCKETS.lock();
    // Pick the next ephemeral port whose 4-tuple is not already live in the
    // socket table.  A still-open (or not-yet-removed) tuple to the same
    // destination would otherwise alias an in-flight connection; scan forward
    // until we find a free one (bounded so a full table can't spin forever).
    let mut src_port = 49152;
    for _ in 0..16000 {
        let n = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
        src_port = 49152 + (n % 16000) as u16;
        if !sockets.contains_key(&(src_port, dst_ip, dst_port)) {
            break;
        }
    }
    let mut sock = TcpSocket::new(our_ip, dst_ip, src_port, dst_port);
    sock.connect();
    sockets.insert((src_port, dst_ip, dst_port), sock);
    src_port
}

pub fn write(src_port: u16, dst_ip: [u8; 4], dst_port: u16, data: &[u8]) {
    if let Some(sock) = SOCKETS.lock().get_mut(&(src_port, dst_ip, dst_port)) {
        sock.send(data);
    }
}

pub fn resend_last(src_port: u16, dst_ip: [u8; 4], dst_port: u16) {
    if let Some(sock) = SOCKETS.lock().get_mut(&(src_port, dst_ip, dst_port)) {
        sock.resend_last_data();
    }
}

pub fn read(src_port: u16, dst_ip: [u8; 4], dst_port: u16) -> Vec<u8> {
    SOCKETS
        .lock()
        .get_mut(&(src_port, dst_ip, dst_port))
        .map(|s| s.recv())
        .unwrap_or_default()
}

/// True once the 3-way handshake has completed.
pub fn is_established(src_port: u16, dst_ip: [u8; 4], dst_port: u16) -> bool {
    SOCKETS
        .lock()
        .get(&(src_port, dst_ip, dst_port))
        .map(|s| s.state == TcpState::Established)
        .unwrap_or(false)
}

/// True once the peer has closed the connection (FIN seen) — for HTTP responses
/// sent with `Connection: close`, this marks the end of the body.
pub fn is_closed(src_port: u16, dst_ip: [u8; 4], dst_port: u16) -> bool {
    SOCKETS
        .lock()
        .get(&(src_port, dst_ip, dst_port))
        .map(|s| {
            matches!(
                s.state,
                TcpState::CloseWait | TcpState::LastAck | TcpState::Closed | TcpState::TimeWait
            )
        })
        .unwrap_or(true) // missing socket ⇒ treat as closed
}

/// Retransmit the SYN if the socket is still in SynSent (first SYN was dropped
/// before ARP resolved the next hop).
pub fn resend_syn(src_port: u16, dst_ip: [u8; 4], dst_port: u16) {
    if let Some(sock) = SOCKETS.lock().get_mut(&(src_port, dst_ip, dst_port)) {
        sock.resend_syn();
    }
}

/// Tear down and forget a socket once a request is complete, so the deterministic
/// (src_port,dst_ip,dst_port) key is free for the next request to the same host.
pub fn close_and_remove(src_port: u16, dst_ip: [u8; 4], dst_port: u16) {
    let mut t = SOCKETS.lock();
    if let Some(sock) = t.get_mut(&(src_port, dst_ip, dst_port))
        && sock.state == TcpState::Established
    {
        sock.close();
    }
    t.remove(&(src_port, dst_ip, dst_port));
}
