# SAIOS IpcContract Specification
**Document ID:** DOC-13_IpcContract.txt
**Layer:** Subsystem Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01 and DOC-07

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT_Part2.txt IPC ARCHITECTURE. SAIOS_SSOT.txt IPC-related taxonomy entries.

## OWNERSHIP

IpcContract owns POSIX message queues, POSIX semaphores, System V shared memory/semaphores/message queues, anonymous pipes, named pipes, Unix domain sockets, and SAIOS-native IPEC.

## INVARIANTS

Every IPC object has one owning namespace. IPC creation is counted against the creator's RAF quota. IPC objects are never orphaned: all handles closed means automatic destruction and IPC_OBJECT_DESTROYED. Shared memory frames are owned by the IPC object and obey MemoryContract invariants.

## PIPES

A pipe has read and write file descriptors and a 64KB circular kernel buffer from slab allocator. Writes block when full. Reads block when empty. Empty with all writers closed returns EOF. Write with all readers closed returns EPIPE and delivers SIGPIPE. PIPE_CREATE includes creating_pid and buffer_size. PIPE_STALL fires after 1 second blocked with pid, direction, and duration_ns.

## UNIX DOMAIN SOCKETS

UDS supports SOCK_STREAM and SOCK_DGRAM, accept/connect, SCM_RIGHTS, and SCM_CREDENTIALS. Paths live in VFS namespace, but IpcContract owns socket semantics. UDS_CONNECT includes client_pid, server_pid, and socket_path. UDS_SCM_RIGHTS is CRITICAL and includes sending_pid, receiving_pid, and fd_count.

## POSIX MESSAGE QUEUES

mq_open, mq_send, and mq_receive operate on named queues with priority-ordered delivery. Attributes are maximum message count and maximum message size. RAF tracks queue depth. MQ_DEPTH_EXCEEDED includes queue_name, pid, current_depth, and maximum_depth.

## SAIOS IPEC

IPEC is a native lock-free SPSC ring buffer in shared memory. Producer and consumer each hold a file descriptor. A process creates a named channel; another joins it. The single operation publishes an event record up to 4KB. Consumer polls or waits through eventfd, and multiple IPEC channels can be multiplexed through an epoll analogue. Overflow silently drops and increments overflow counter; no SIGPIPE. IPEC_CREATE includes creating_pid, channel_name, shared_memory_size, and record_limit.

IPEC is modelled on KDS per-CPU rings so application-level structured events can correlate with kernel events when applications choose to emit evidence.

## RESOURCE ACCOUNTING

Every IPC object is a resource. Queue depth, shared-memory size, pipe buffers, and socket buffers are attributed to creating PID and cgroup. Quota denial emits RESOURCE_QUOTA_EXCEEDED.

## COMPLETION CHECK

A developer can implement pipes, UDS, POSIX MQ, and IPEC with correct lifecycle, EOF, SIGPIPE, overflow, quota, and KDS semantics.
