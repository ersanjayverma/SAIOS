# SAIOS NetContract Specification
**Document ID:** DOC-14_NetContract.txt
**Layer:** Subsystem Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01 and DOC-11

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT_Part2.txt NETWORKING STACK; SAIOS_SSOT.txt network taxonomy and NETWORK ACCOUNTING.

## OWNERSHIP

NetContract owns socket API, protocol dispatch for TCP/UDP/ICMP/SCTP, IP routing table, ARP and NDP tables, network namespaces, socket buffer management, traffic control, XDP hook point, and network observability.

## SOCKET BUFFERS

Each socket has send and receive buffers with configurable maximum size, allocated from slab allocator and tracked by RAF per PID. TCP writers block on full send buffer. UDP with MSG_DONTWAIT returns EAGAIN. SOCKET_BUFFER_PRESSURE emits at 80 percent of maximum with pid, socket_id, buffer_kind, used_bytes, maximum_bytes.

## PROTOCOL STACK

IPv4 and IPv6 implement fragmentation/reassembly, TTL/hop limit, ICMP/ICMPv6, and longest-prefix-match routing trie. ROUTE_CHANGE emits namespace, route, operation, and pid.

TCP implements CLOSED, LISTEN, SYN_SENT, SYN_RECEIVED, ESTABLISHED, FIN_WAIT_1, FIN_WAIT_2, CLOSE_WAIT, CLOSING, LAST_ACK, and TIME_WAIT. Nagle is enabled by default and disabled by TCP_NODELAY. CUBIC is default congestion control; BBR is available. SACK, timestamps, and window scaling are supported. TCP_STATE_CHANGE emits tuple, old_state, new_state, reason.

UDP is stateless. SCTP is stream-oriented for telephony and signalling use cases.

## XDP

XDP programs attach at driver level before the kernel stack. BPF verifier validation is required before load. XDP_PROGRAM_LOADED includes interface_name, program_hash, and loading_pid. Loading requires CAP_NET_ADMIN enforced by SecurityContract.

## NETWORK OBSERVABILITY

TCP_RETRANSMIT: socket_tuple, retransmit_count. TCP_RESET: socket_tuple, direction. SOCKET_CREATE: pid, type, protocol, namespace. SOCKET_CLOSE: pid, tuple, bytes_sent, bytes_received, duration_ns. DNS_QUERY: pid, query_name, query_type. INTERFACE_UP: interface_name, speed. INTERFACE_DOWN: interface_name, reason. NET_CONNECT, NET_ERROR, NET_CONGESTION, and NETWORK_ACCOUNT_PERIOD retain DOC-02 payloads.

Correlation rules for DOC-17 consume these events: retransmit storms correlate with NET_CONGESTION; DNS failures correlate with application connection errors. NetContract emits evidence only.

## NETWORK ACCOUNTING

Socket-level accounting records per-PID bytes sent and received per socket. Interface-level accounting records bytes, packets, errors, and drops. Reconciliation identifies unattributed ARP, ICMP, and kernel-generated traffic. NETWORK_ACCOUNT_PERIOD emits per active socket per period.

## NETWORK NAMESPACES

Each network namespace has independent interfaces, routing table, ARP/NDP tables, and socket namespace. Interface migration between namespaces requires CAP_NET_ADMIN and emits SECURITY_NETWORK_POLICY_DENY on denial.

## COMPLETION CHECK

A developer can implement TCP state machine, socket buffering, XDP load path, network accounting, and event emission with correlation-ready evidence.
