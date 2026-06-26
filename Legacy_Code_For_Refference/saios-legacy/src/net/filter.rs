//! Basic packet filter (firewall) for inbound/outbound traffic.
//!
//! F-NET-01: Constitutional requirement for network packet filtering.
//! Provides a simple allow/deny rule table evaluated on every IP packet.
//!
//! Default policy: ALLOW ALL (permissive until rules are added).

use alloc::vec::Vec;
use spin::Mutex;

/// Filter action for a matched rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterAction {
    Allow,
    Deny,
    Log, // allow but emit KDS event
}

/// A single packet filter rule.
#[derive(Debug, Clone)]
pub struct FilterRule {
    pub src_ip: Option<[u8; 4]>,
    pub dst_ip: Option<[u8; 4]>,
    pub src_port: Option<u16>,
    pub dst_port: Option<u16>,
    pub protocol: Option<u8>, // 6=TCP, 17=UDP, 1=ICMP
    pub action: FilterAction,
}

/// Direction of packet flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
}

static INBOUND_RULES: Mutex<Vec<FilterRule>> = Mutex::new(Vec::new());
static OUTBOUND_RULES: Mutex<Vec<FilterRule>> = Mutex::new(Vec::new());
static PACKETS_ALLOWED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static PACKETS_DENIED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Add a filter rule for the given direction.
pub fn add_rule(direction: Direction, rule: FilterRule) {
    match direction {
        Direction::Inbound => INBOUND_RULES.lock().push(rule),
        Direction::Outbound => OUTBOUND_RULES.lock().push(rule),
    }
}

/// Evaluate a packet against the filter rules. Returns true if allowed.
/// Called from the IP receive/transmit path.
pub fn filter_packet(
    direction: Direction,
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    protocol: u8,
    src_port: u16,
    dst_port: u16,
) -> bool {
    let rules = match direction {
        Direction::Inbound => INBOUND_RULES.lock(),
        Direction::Outbound => OUTBOUND_RULES.lock(),
    };

    for rule in rules.iter() {
        if !matches_rule(rule, src_ip, dst_ip, protocol, src_port, dst_port) {
            continue;
        }
        match rule.action {
            FilterAction::Deny => {
                PACKETS_DENIED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                crate::kds::kds_event(
                    crate::kds::KdsSubsystem::Network,
                    crate::kds::KdsEventType::State,
                    crate::kds::KdsSeverity::Warn,
                    [
                        u32::from_be_bytes(src_ip) as u64,
                        u32::from_be_bytes(dst_ip) as u64,
                        ((protocol as u64) << 32) | (src_port as u64),
                        dst_port as u64,
                    ],
                );
                return false;
            }
            FilterAction::Log => {
                crate::kds::kds_event(
                    crate::kds::KdsSubsystem::Network,
                    crate::kds::KdsEventType::State,
                    crate::kds::KdsSeverity::Info,
                    [
                        u32::from_be_bytes(src_ip) as u64,
                        u32::from_be_bytes(dst_ip) as u64,
                        ((protocol as u64) << 32) | (src_port as u64),
                        dst_port as u64,
                    ],
                );
                PACKETS_ALLOWED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                return true;
            }
            FilterAction::Allow => {
                PACKETS_ALLOWED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                return true;
            }
        }
    }

    // Default policy: allow if no rule matches.
    PACKETS_ALLOWED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    true
}

fn matches_rule(
    rule: &FilterRule,
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    protocol: u8,
    src_port: u16,
    dst_port: u16,
) -> bool {
    if let Some(r) = rule.src_ip
        && r != src_ip
    {
        return false;
    }
    if let Some(r) = rule.dst_ip
        && r != dst_ip
    {
        return false;
    }
    if let Some(r) = rule.protocol
        && r != protocol
    {
        return false;
    }
    if let Some(r) = rule.src_port
        && r != src_port
    {
        return false;
    }
    if let Some(r) = rule.dst_port
        && r != dst_port
    {
        return false;
    }
    true
}

/// Statistics for diagnostics.
pub fn stats() -> (u64, u64) {
    (
        PACKETS_ALLOWED.load(core::sync::atomic::Ordering::Relaxed),
        PACKETS_DENIED.load(core::sync::atomic::Ordering::Relaxed),
    )
}

/// Number of active rules.
pub fn rule_count() -> (usize, usize) {
    (INBOUND_RULES.lock().len(), OUTBOUND_RULES.lock().len())
}
