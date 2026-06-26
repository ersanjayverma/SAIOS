//! Network contract owner surface.
//!
//! The protocol stack and NIC drivers remain implementation modules; this
//! contract owns shared packet queues, network identity snapshots, and the
//! canonical network evidence shape.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::configuration_contract::ConfigurationContract;
use crate::observability_contract::{
    ContractId, EventRecord, ObservabilityContract, ObservableEvent, ObservationOutcome,
    ObservationTag, ResourceClass, ResourceOwner,
};

static TX_QUEUE: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
static RX_QUEUE: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
static ARP_TABLE: Mutex<BTreeMap<[u8; 4], [u8; 6]>> = Mutex::new(BTreeMap::new());
static IDENTITY: Mutex<NetworkIdentity> = Mutex::new(NetworkIdentity {
    mac: [0; 6],
    ip: [0; 4],
    gateway: ConfigurationContract::DEFAULT_NETWORK_GATEWAY,
    netmask: ConfigurationContract::DEFAULT_NETWORK_NETMASK,
    dns: ConfigurationContract::DEFAULT_NETWORK_DNS,
});

static TX_ENQUEUED: AtomicU64 = AtomicU64::new(0);
static RX_ENQUEUED: AtomicU64 = AtomicU64::new(0);
static RX_DEQUEUED: AtomicU64 = AtomicU64::new(0);
static TX_DRAINED: AtomicU64 = AtomicU64::new(0);
static ARP_UPDATES: AtomicU64 = AtomicU64::new(0);
static SOCKET_EVENTS: AtomicU64 = AtomicU64::new(0);
static TCP_TRANSITIONS: AtomicU64 = AtomicU64::new(0);
static WAIT_PROGRESS: AtomicU64 = AtomicU64::new(0);

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkProtocol {
    Tcp = 1,
    Udp = 2,
    Icmp = 3,
    Sctp = 4,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkSocketKind {
    Stream = 1,
    Datagram = 2,
    Raw = 3,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkNamespaceKind {
    Root = 1,
    Container = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkNamespaceId {
    pub kind: NetworkNamespaceKind,
    pub id: u64,
}

impl NetworkNamespaceId {
    pub const ROOT: Self = Self {
        kind: NetworkNamespaceKind::Root,
        id: 0,
    };
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteTableKind {
    StaticDefaultRoute = 1,
    LongestPrefixTrie = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrafficControlPolicy {
    None = 1,
    Fifo = 2,
    FairQueue = 3,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetTcpState {
    Closed = 1,
    Listen = 2,
    SynSent = 3,
    SynReceived = 4,
    Established = 5,
    FinWait1 = 6,
    FinWait2 = 7,
    CloseWait = 8,
    Closing = 9,
    LastAck = 10,
    TimeWait = 11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketBufferPolicyView {
    pub send_max_bytes: usize,
    pub recv_max_bytes: usize,
    pub pressure_threshold_percent: u8,
    pub raf_byte_accounting: bool,
    pub slab_backed_buffers: bool,
    pub tcp_blocks_on_full_send: bool,
    pub udp_eagain_on_full_send: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkCapabilityView {
    pub socket_api: bool,
    pub socket_buffer_policy_metadata: bool,
    pub socket_buffer_pressure_events: bool,
    pub raf_network_byte_accounting: bool,
    pub raf_network_packet_accounting: bool,
    pub ipv4: bool,
    pub ipv6_basic_module: bool,
    pub ipv6_full_routing: bool,
    pub arp_table: bool,
    pub ndp_table: bool,
    pub route_default_gateway: bool,
    pub route_longest_prefix_trie: bool,
    pub tcp_basic_state_machine: bool,
    pub tcp_full_state_machine: bool,
    pub tcp_congestion_control: bool,
    pub udp_datagrams: bool,
    pub icmp: bool,
    pub sctp: bool,
    pub network_namespaces: bool,
    pub traffic_control: bool,
    pub xdp_bpf_verifier: bool,
    pub socket_create_close_events: bool,
    pub tcp_state_change_events: bool,
    pub dns_query_events: bool,
    pub interface_up_down_events: bool,
}

#[derive(Clone, Copy)]
pub struct NetworkIdentity {
    pub mac: [u8; 6],
    pub ip: [u8; 4],
    pub gateway: [u8; 4],
    pub netmask: [u8; 4],
    pub dns: [u8; 4],
}

#[derive(Clone, Copy)]
pub struct NetworkStatusView {
    pub identity: NetworkIdentity,
    pub driver: &'static str,
    pub tx_depth: usize,
    pub rx_depth: usize,
    pub tx_enqueued: u64,
    pub rx_enqueued: u64,
    pub rx_dequeued: u64,
    pub tx_drained: u64,
    pub arp_entries: usize,
    pub socket_events: u64,
    pub tcp_transitions: u64,
    pub wait_progress: u64,
}

pub struct NetworkContract;

impl NetworkContract {
    pub fn capability_view() -> NetworkCapabilityView {
        NetworkCapabilityView {
            socket_api: true,
            socket_buffer_policy_metadata: true,
            socket_buffer_pressure_events: false,
            raf_network_byte_accounting: true,
            raf_network_packet_accounting: true,
            ipv4: true,
            ipv6_basic_module: true,
            ipv6_full_routing: false,
            arp_table: true,
            ndp_table: false,
            route_default_gateway: true,
            route_longest_prefix_trie: false,
            tcp_basic_state_machine: true,
            tcp_full_state_machine: false,
            tcp_congestion_control: false,
            udp_datagrams: true,
            icmp: false,
            sctp: false,
            network_namespaces: false,
            traffic_control: false,
            xdp_bpf_verifier: false,
            socket_create_close_events: true,
            tcp_state_change_events: true,
            dns_query_events: false,
            interface_up_down_events: false,
        }
    }

    pub fn socket_buffer_policy() -> SocketBufferPolicyView {
        SocketBufferPolicyView {
            send_max_bytes: ConfigurationContract::DEFAULT_SOCKET_SEND_BUFFER_BYTES,
            recv_max_bytes: ConfigurationContract::DEFAULT_SOCKET_RECV_BUFFER_BYTES,
            pressure_threshold_percent: ConfigurationContract::SOCKET_BUFFER_PRESSURE_PERCENT,
            raf_byte_accounting: true,
            slab_backed_buffers: false,
            tcp_blocks_on_full_send: false,
            udp_eagain_on_full_send: false,
        }
    }

    pub fn root_namespace() -> NetworkNamespaceId {
        NetworkNamespaceId::ROOT
    }

    pub fn default_ip() -> [u8; 4] {
        ConfigurationContract::DEFAULT_NETWORK_IPV4
    }

    pub fn set_identity(mac: [u8; 6], ip: [u8; 4], driver: &'static str) {
        {
            let mut identity = IDENTITY.lock();
            identity.mac = mac;
            identity.ip = ip;
        }
        Self::emit_event(
            "network.identity.update",
            ObservationOutcome::Success,
            ResourceOwner::Driver(driver),
            [Self::ipv4_word(ip), Self::mac_word(mac), 0, 0],
        );
    }

    pub fn identity() -> NetworkIdentity {
        *IDENTITY.lock()
    }

    pub fn ip() -> [u8; 4] {
        IDENTITY.lock().ip
    }

    pub fn mac() -> [u8; 6] {
        IDENTITY.lock().mac
    }

    pub fn gateway() -> [u8; 4] {
        IDENTITY.lock().gateway
    }

    pub fn set_route(gateway: [u8; 4], netmask: [u8; 4]) {
        {
            let mut identity = IDENTITY.lock();
            identity.gateway = gateway;
            identity.netmask = netmask;
        }
        Self::emit_event(
            "network.route.update",
            ObservationOutcome::Success,
            ObservabilityContract::current_pid_owner(),
            [Self::ipv4_word(gateway), Self::ipv4_word(netmask), 0, 0],
        );
    }

    pub fn netmask() -> [u8; 4] {
        IDENTITY.lock().netmask
    }

    pub fn dns_server() -> [u8; 4] {
        IDENTITY.lock().dns
    }

    pub fn set_dns_server(dns: [u8; 4]) {
        IDENTITY.lock().dns = dns;
        Self::emit_event(
            "network.dns.update",
            ObservationOutcome::Success,
            ObservabilityContract::current_pid_owner(),
            [Self::ipv4_word(dns), 0, 0, 0],
        );
    }

    pub fn is_local(ip: [u8; 4]) -> bool {
        ip[0] == 127 || ip == Self::ip()
    }

    pub fn next_hop(dst: [u8; 4]) -> [u8; 4] {
        if Self::is_local(dst) {
            return dst;
        }
        let identity = Self::identity();
        let same_subnet = (0..4)
            .all(|i| (dst[i] & identity.netmask[i]) == (identity.ip[i] & identity.netmask[i]));
        let hop = if same_subnet { dst } else { identity.gateway };
        Self::emit_event(
            "network.route.resolve",
            ObservationOutcome::Success,
            ObservabilityContract::current_pid_owner(),
            [
                Self::ipv4_word(dst),
                Self::ipv4_word(hop),
                same_subnet as u64,
                0,
            ],
        );
        hop
    }

    pub fn record_nic_activation(driver: &'static str, driver_id: u64, fallback: bool) {
        Self::emit_event(
            "network.nic.activate",
            ObservationOutcome::Success,
            ResourceOwner::Driver(driver),
            [driver_id, fallback as u64, Self::ipv4_word(Self::ip()), 0],
        );
    }

    pub fn record_socket_create(socket_id: usize, domain: u64, stype: u64) {
        SOCKET_EVENTS.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            "network.socket.create",
            ObservationOutcome::Success,
            ObservabilityContract::current_pid_owner(),
            [
                socket_id as u64,
                domain,
                stype,
                SOCKET_EVENTS.load(Ordering::Relaxed),
            ],
        );
    }

    pub fn record_socket_bind(socket_id: usize, ip: [u8; 4], port: u16) {
        SOCKET_EVENTS.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            "network.socket.bind",
            ObservationOutcome::Success,
            ObservabilityContract::current_pid_owner(),
            [
                socket_id as u64,
                Self::ipv4_word(ip),
                port as u64,
                SOCKET_EVENTS.load(Ordering::Relaxed),
            ],
        );
    }

    pub fn record_socket_connect(socket_id: usize, src_port: u16, dst_ip: [u8; 4], dst_port: u16) {
        SOCKET_EVENTS.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            "network.socket.connect",
            ObservationOutcome::Success,
            ObservabilityContract::current_pid_owner(),
            [
                socket_id as u64,
                src_port as u64,
                Self::ipv4_word(dst_ip),
                dst_port as u64,
            ],
        );
    }

    pub fn record_socket_close(socket_id: usize, src_port: u16, dst_ip: [u8; 4], dst_port: u16) {
        SOCKET_EVENTS.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            "network.socket.close",
            ObservationOutcome::Success,
            ObservabilityContract::current_pid_owner(),
            [
                socket_id as u64,
                src_port as u64,
                Self::ipv4_word(dst_ip),
                dst_port as u64,
            ],
        );
    }

    pub fn record_socket_failure(socket_id: usize, code: u64, detail: u64) {
        SOCKET_EVENTS.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            "network.socket.failure",
            ObservationOutcome::Failed,
            ObservabilityContract::current_pid_owner(),
            [
                socket_id as u64,
                code,
                detail,
                SOCKET_EVENTS.load(Ordering::Relaxed),
            ],
        );
    }

    pub fn record_tcp_state(
        src_port: u16,
        dst_ip: [u8; 4],
        dst_port: u16,
        from: u64,
        to: u64,
        reason: &'static str,
    ) {
        TCP_TRANSITIONS.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            reason,
            ObservationOutcome::Success,
            ObservabilityContract::current_pid_owner(),
            [
                src_port as u64,
                Self::ipv4_word(dst_ip),
                dst_port as u64,
                (from << 32) | to,
            ],
        );
    }

    pub fn record_tcp_failure(src_port: u16, dst_ip: [u8; 4], dst_port: u16, code: u64) {
        TCP_TRANSITIONS.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            "network.tcp.failure",
            ObservationOutcome::Failed,
            ObservabilityContract::current_pid_owner(),
            [
                src_port as u64,
                Self::ipv4_word(dst_ip),
                dst_port as u64,
                code,
            ],
        );
    }

    pub fn record_wait_progress(reason: &'static str, tick: u64, canceled: bool) {
        WAIT_PROGRESS.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            reason,
            if canceled {
                ObservationOutcome::Denied
            } else {
                ObservationOutcome::Retried
            },
            ObservabilityContract::current_pid_owner(),
            [
                tick,
                canceled as u64,
                WAIT_PROGRESS.load(Ordering::Relaxed),
                0,
            ],
        );
    }

    pub fn cache_arp(ip: [u8; 4], mac: [u8; 6]) {
        ARP_TABLE.lock().insert(ip, mac);
        ARP_UPDATES.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            "network.arp.update",
            ObservationOutcome::Success,
            ResourceOwner::Unknown,
            [Self::ipv4_word(ip), Self::mac_word(mac), 0, 0],
        );
    }

    pub fn lookup_arp(ip: [u8; 4]) -> Option<[u8; 6]> {
        if Self::is_local(ip) {
            return Some(Self::mac());
        }
        ARP_TABLE.lock().get(&ip).copied()
    }

    pub fn enqueue_tx(frame: Vec<u8>) {
        let len = frame.len() as u64;
        let chain = crate::resource_contract::AttributionChain::current();
        if crate::resource_contract::ResourceContract::charge(
            chain,
            crate::resource_contract::ResourceKind::NetworkBytes,
            len,
        )
        .is_err()
        {
            return;
        }
        if crate::resource_contract::ResourceContract::charge(
            chain,
            crate::resource_contract::ResourceKind::NetworkPackets,
            1,
        )
        .is_err()
        {
            crate::resource_contract::ResourceContract::release(
                chain.accountable,
                crate::resource_contract::ResourceKind::NetworkBytes,
                len,
            );
            return;
        }
        let depth = {
            let mut queue = TX_QUEUE.lock();
            queue.push(frame);
            queue.len()
        };
        TX_ENQUEUED.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            "network.packet.tx",
            ObservationOutcome::Success,
            ObservabilityContract::current_pid_owner(),
            [len, depth as u64, TX_ENQUEUED.load(Ordering::Relaxed), 0],
        );
    }

    pub fn enqueue_rx(frame: Vec<u8>, source: &'static str) {
        let len = frame.len() as u64;
        let chain = crate::resource_contract::AttributionChain {
            accountable: crate::resource_contract::AccountableEntity::KERNEL,
            acting_pid: crate::process::current_pid(),
            correlation_id:
                crate::observability_contract::ObservabilityContract::current_correlation_id(),
            evidence_event_id: 0,
        };
        if crate::resource_contract::ResourceContract::charge(
            chain,
            crate::resource_contract::ResourceKind::NetworkBytes,
            len,
        )
        .is_err()
        {
            return;
        }
        if crate::resource_contract::ResourceContract::charge(
            chain,
            crate::resource_contract::ResourceKind::NetworkPackets,
            1,
        )
        .is_err()
        {
            crate::resource_contract::ResourceContract::release(
                chain.accountable,
                crate::resource_contract::ResourceKind::NetworkBytes,
                len,
            );
            return;
        }
        let depth = {
            let mut queue = RX_QUEUE.lock();
            queue.push(frame);
            queue.len()
        };
        RX_ENQUEUED.fetch_add(1, Ordering::Relaxed);
        Self::emit_event(
            "network.packet.rx",
            ObservationOutcome::Success,
            ResourceOwner::Driver(source),
            [len, depth as u64, RX_ENQUEUED.load(Ordering::Relaxed), 0],
        );
    }

    pub fn recv_rx() -> Option<Vec<u8>> {
        let frame = RX_QUEUE.lock().pop();
        if let Some(ref frame) = frame {
            RX_DEQUEUED.fetch_add(1, Ordering::Relaxed);
            Self::emit_event(
                "network.packet.dequeue",
                ObservationOutcome::Success,
                ObservabilityContract::current_pid_owner(),
                [frame.len() as u64, RX_QUEUE.lock().len() as u64, 0, 0],
            );
        }
        frame
    }

    pub fn drain_rx() -> Vec<Vec<u8>> {
        let frames = {
            let mut queue = RX_QUEUE.lock();
            core::mem::take(&mut *queue)
        };
        if !frames.is_empty() {
            RX_DEQUEUED.fetch_add(frames.len() as u64, Ordering::Relaxed);
            Self::emit_event(
                "network.packet.rx.drain",
                ObservationOutcome::Success,
                ObservabilityContract::current_pid_owner(),
                [
                    frames.len() as u64,
                    RX_DEQUEUED.load(Ordering::Relaxed),
                    0,
                    0,
                ],
            );
        }
        frames
    }

    pub fn drain_tx() -> Vec<Vec<u8>> {
        let frames = {
            let mut queue = TX_QUEUE.lock();
            core::mem::take(&mut *queue)
        };
        if !frames.is_empty() {
            TX_DRAINED.fetch_add(frames.len() as u64, Ordering::Relaxed);
            Self::emit_event(
                "network.packet.tx.drain",
                ObservationOutcome::Success,
                ResourceOwner::Driver(Self::driver_name()),
                [
                    frames.len() as u64,
                    TX_DRAINED.load(Ordering::Relaxed),
                    0,
                    0,
                ],
            );
        }
        frames
    }

    pub fn status_view() -> NetworkStatusView {
        NetworkStatusView {
            identity: Self::identity(),
            driver: Self::driver_name(),
            tx_depth: TX_QUEUE.lock().len(),
            rx_depth: RX_QUEUE.lock().len(),
            tx_enqueued: TX_ENQUEUED.load(Ordering::Relaxed),
            rx_enqueued: RX_ENQUEUED.load(Ordering::Relaxed),
            rx_dequeued: RX_DEQUEUED.load(Ordering::Relaxed),
            tx_drained: TX_DRAINED.load(Ordering::Relaxed),
            arp_entries: ARP_TABLE.lock().len(),
            socket_events: SOCKET_EVENTS.load(Ordering::Relaxed),
            tcp_transitions: TCP_TRANSITIONS.load(Ordering::Relaxed),
            wait_progress: WAIT_PROGRESS.load(Ordering::Relaxed),
        }
    }

    pub fn driver_name() -> &'static str {
        match crate::driver::net::active_driver_id() {
            1 => "Intel e1000 (hardware NIC)",
            2 => "Realtek RTL8139 (hardware NIC)",
            3 => "VirtIO-Net",
            _ => "none",
        }
    }

    fn emit_event(
        reason: &'static str,
        outcome: ObservationOutcome,
        owner: ResourceOwner,
        evidence: [u64; 4],
    ) {
        ObservabilityContract::emit(EventRecord {
            event: ObservableEvent::ResourceDelta,
            contract: ContractId::Network,
            tag: ObservationTag::ResourceDelta,
            reason,
            outcome,
            resource: ResourceClass::Device,
            owner,
            cpu: Some(crate::process::table::cpu_idx()),
            pid: crate::process::current_pid(),
            correlation_id: ObservabilityContract::current_correlation_id(),
            evidence,
        });
    }

    fn ipv4_word(ip: [u8; 4]) -> u64 {
        u32::from_be_bytes(ip) as u64
    }

    fn mac_word(mac: [u8; 6]) -> u64 {
        ((mac[0] as u64) << 40)
            | ((mac[1] as u64) << 32)
            | ((mac[2] as u64) << 24)
            | ((mac[3] as u64) << 16)
            | ((mac[4] as u64) << 8)
            | mac[5] as u64
    }
}
