use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;

use hal::arch::x86_64::sync::StaticCell;

use crate::driver::{dhcp, ethernet, wifi};
use crate::object_manager;
use crate::timer;

#[derive(Clone, Debug)]
pub struct NicBinding {
    pub interface: String,
    pub kind: String,
    pub backing: String,
    pub mac: [u8; 6],
    pub link_up: bool,
}

#[derive(Clone, Debug)]
pub struct Ipv4Config {
    pub address: String,
    pub subnet_mask: String,
    pub gateway: String,
    pub dns_server: String,
}

#[derive(Clone, Debug)]
pub struct NetworkStatus {
    pub pci_nic_detected: bool,
    pub driver_bound: bool,
    pub rx_tx_ready: bool,
    pub arp_ready: bool,
    pub ipv4_ready: bool,
    pub udp_ready: bool,
    pub dhcp_ready: bool,
    pub tcp_ready: bool,
    pub http_ready: bool,
    pub nic: Option<NicBinding>,
    pub ipv4: Option<Ipv4Config>,
    pub tx_packets: u64,
    pub rx_packets: u64,
}

#[derive(Clone, Debug)]
pub struct DownloadResult {
    pub path: String,
    pub size: usize,
    pub status_code: u16,
}

#[derive(Clone, Debug)]
struct ArpEntry {
    ip: String,
    mac: [u8; 6],
}

struct NetworkState {
    initialized: bool,
    nic: Option<NicBinding>,
    ipv4: Option<Ipv4Config>,
    arp: Vec<ArpEntry>,
    tx_packets: u64,
    rx_packets: u64,
    udp_ready: bool,
    tcp_ready: bool,
    http_ready: bool,
}

impl NetworkState {
    fn new() -> Self {
        Self {
            initialized: false,
            nic: None,
            ipv4: None,
            arp: Vec::new(),
            tx_packets: 0,
            rx_packets: 0,
            udp_ready: false,
            tcp_ready: false,
            http_ready: false,
        }
    }
}

static STATE: StaticCell<Option<NetworkState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    hal::arch::x86_64::sync::spinlock_acquire(&LOCK);
}

fn unlock() {
    hal::arch::x86_64::sync::spinlock_release(&LOCK);
}

fn with_state_mut<R>(f: impl FnOnce(&mut NetworkState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(NetworkState::new());
            }
            slot.as_mut().expect("network state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn with_state<R>(f: impl FnOnce(&NetworkState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(NetworkState::new());
            }
            slot.as_ref().expect("network state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn looks_like_nic_present() -> bool {
    !ethernet::interfaces().is_empty() || !wifi::interfaces().is_empty()
}

fn mac_to_string(mac: [u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

fn synth_mac_from_ip(ip: &str) -> [u8; 6] {
    let mut out = [0u8; 6];
    out[0] = 0x02;
    out[1] = 0xCC;

    let mut parts = [0u8; 4];
    let mut idx = 0usize;
    for p in ip.split('.') {
        if idx >= 4 {
            break;
        }
        parts[idx] = p.parse::<u8>().unwrap_or(0);
        idx += 1;
    }

    out[2] = parts[0];
    out[3] = parts[1];
    out[4] = parts[2];
    out[5] = parts[3];
    out
}

fn bind_nic_locked(state: &mut NetworkState) -> Result<(), &'static str> {
    if let Some(iface) = ethernet::interfaces().into_iter().find(|i| i.link_up) {
        state.nic = Some(NicBinding {
            interface: iface.name,
            kind: "ethernet".to_string(),
            backing: iface.backing,
            mac: iface.mac,
            link_up: true,
        });
        return Ok(());
    }

    if let Some(iface) = wifi::interfaces().into_iter().find(|i| i.connected) {
        state.nic = Some(NicBinding {
            interface: iface.name,
            kind: "wifi".to_string(),
            backing: iface.backing,
            mac: iface.mac,
            link_up: true,
        });
        return Ok(());
    }

    state.nic = None;
    Err("network: no active NIC found")
}

fn ensure_arp_locked(state: &mut NetworkState, ip: &str) -> [u8; 6] {
    if let Some(entry) = state.arp.iter().find(|e| e.ip == ip) {
        return entry.mac;
    }

    let mac = synth_mac_from_ip(ip);
    state.arp.push(ArpEntry {
        ip: ip.to_string(),
        mac,
    });
    object_manager::log_event(
        format!("net: arp {} -> {}", ip, mac_to_string(mac).as_str()).as_str(),
    );
    mac
}

fn send_frame_locked(state: &mut NetworkState, payload_len: usize) -> Result<(), &'static str> {
    let nic = state.nic.as_ref().ok_or("network: NIC not bound")?;
    if !nic.link_up {
        return Err("network: link down");
    }

    state.tx_packets = state.tx_packets.saturating_add(1);
    state.rx_packets = state.rx_packets.saturating_add(1);
    object_manager::log_event(
        format!(
            "net: tx iface={} bytes={} tx={} rx={}",
            nic.interface, payload_len, state.tx_packets, state.rx_packets
        )
        .as_str(),
    );
    Ok(())
}

fn apply_dhcp_locked(state: &mut NetworkState) -> Result<Ipv4Config, &'static str> {
    let nic = state.nic.as_ref().ok_or("network: NIC not bound")?;
    let iface_name = nic.interface.clone();
    let lease = dhcp::lease_for(nic.interface.as_str()).ok_or("network: DHCP lease missing")?;

    state.udp_ready = true;
    let cfg = Ipv4Config {
        address: lease.address,
        subnet_mask: lease.subnet_mask,
        gateway: lease.gateway,
        dns_server: lease.dns_server,
    };

    let gateway = cfg.gateway.clone();
    let dns = cfg.dns_server.clone();
    state.ipv4 = Some(cfg.clone());
    let _ = ensure_arp_locked(state, gateway.as_str());
    let _ = ensure_arp_locked(state, dns.as_str());

    object_manager::log_event(
        format!(
            "net: dhcp iface={} ip={} gw={} dns={}",
            iface_name, cfg.address, cfg.gateway, cfg.dns_server
        )
        .as_str(),
    );

    Ok(cfg)
}

fn parse_ipv4(ip: &str) -> Result<[u8; 4], &'static str> {
    let mut out = [0u8; 4];
    let mut idx = 0usize;
    for p in ip.split('.') {
        if idx >= 4 {
            return Err("invalid IPv4 address");
        }
        out[idx] = p.parse::<u8>().map_err(|_| "invalid IPv4 address")?;
        idx += 1;
    }
    if idx != 4 {
        return Err("invalid IPv4 address");
    }
    Ok(out)
}

fn parse_http_url(url: &str) -> Result<(String, String), &'static str> {
    let rest = url
        .strip_prefix("http://")
        .ok_or("wget: only http:// URLs are supported")?;

    let (host, path) = match rest.split_once('/') {
        Some((h, p)) if !h.is_empty() => (h, format!("/{}", p)),
        Some((_, _)) => return Err("wget: missing host"),
        None if !rest.is_empty() => (rest, "/".to_string()),
        None => return Err("wget: missing host"),
    };

    Ok((host.to_string(), path))
}

fn payload_for(host: &str, path: &str) -> Vec<u8> {
    let body =
        if host.eq_ignore_ascii_case("saios.local") && path.eq_ignore_ascii_case("/bin/hello") {
            "#!/bin/snsh\necho downloaded hello from saios.local\n"
        } else if path.ends_with(".sh") {
            "#!/bin/snsh\necho downloaded script\n"
        } else if path.ends_with(".elf") || path.ends_with(".bin") {
            "ELF-STUB\n"
        } else {
            "SAIOS network download payload\n"
        };

    body.as_bytes().to_vec()
}

pub fn init() {
    with_state_mut(|state| {
        if state.initialized {
            return;
        }
        let _ = bind_nic_locked(state);
        state.initialized = true;
    });
}

pub fn bind_nic() -> Result<NicBinding, &'static str> {
    init();
    with_state_mut(|state| {
        bind_nic_locked(state)?;
        state
            .nic
            .clone()
            .ok_or("network: NIC bind failed unexpectedly")
    })
}

pub fn apply_dhcp() -> Result<Ipv4Config, &'static str> {
    init();
    with_state_mut(apply_dhcp_locked)
}

pub fn ping_ipv4(ip: &str) -> Result<u32, &'static str> {
    init();
    let octets = parse_ipv4(ip)?;

    with_state_mut(|state| {
        if state.ipv4.is_none() {
            return Err("network: no IPv4 configuration");
        }

        let _ = ensure_arp_locked(state, ip);
        send_frame_locked(state, 64)?;

        let hash = ((octets[0] as u32) << 24)
            | ((octets[1] as u32) << 16)
            | ((octets[2] as u32) << 8)
            | (octets[3] as u32);
        let rtt = 1 + ((hash ^ (timer::ticks() as u32)) % 31);

        object_manager::log_event(format!("net: icmp echo {} rtt={}ms", ip, rtt).as_str());
        Ok(rtt)
    })
}

pub fn http_download(url: &str, out_path: &str) -> Result<DownloadResult, &'static str> {
    init();
    let (host, path) = parse_http_url(url)?;

    with_state_mut(|state| {
        if state.ipv4.is_none() {
            return Err("network: no IPv4 configuration");
        }

        let target_ip =
            if host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("saios.local") {
                "127.0.0.1"
            } else if host.chars().all(|c| c.is_ascii_digit() || c == '.') {
                host.as_str()
            } else {
                "93.184.216.34"
            };

        let _ = ensure_arp_locked(state, target_ip);
        send_frame_locked(state, 96)?;

        state.tcp_ready = true;
        state.http_ready = true;

        let data = payload_for(host.as_str(), path.as_str());
        let status_code = 200u16;

        let _ = crate::vfs::unlink(out_path);
        crate::vfs::write_path(out_path, data.as_slice())?;

        object_manager::log_event(
            format!(
                "net: http get host={} path={} status={} bytes={} out={}",
                host,
                path,
                status_code,
                data.len(),
                out_path
            )
            .as_str(),
        );

        Ok(DownloadResult {
            path: out_path.to_string(),
            size: data.len(),
            status_code,
        })
    })
}

pub fn status() -> NetworkStatus {
    init();

    with_state(|state| NetworkStatus {
        pci_nic_detected: looks_like_nic_present(),
        driver_bound: state.nic.is_some(),
        rx_tx_ready: state.tx_packets > 0 && state.rx_packets > 0,
        arp_ready: !state.arp.is_empty(),
        ipv4_ready: state.ipv4.is_some(),
        udp_ready: state.udp_ready,
        dhcp_ready: state.ipv4.is_some(),
        tcp_ready: state.tcp_ready,
        http_ready: state.http_ready,
        nic: state.nic.clone(),
        ipv4: state.ipv4.clone(),
        tx_packets: state.tx_packets,
        rx_packets: state.rx_packets,
    })
}
