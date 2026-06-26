# SAIOS SecurityContract Specification
**Document ID:** DOC-15_SecurityContract.txt
**Layer:** Subsystem Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01; security is non-negotiable

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt SECURITYCONTRACT and security taxonomy. SAIOS_SSOT_Part2.txt SECURITY MODEL; CONTAINER SUPPORT; POSIX COMPLIANCE reference; COMPAT invariants.

## SECURITY PRINCIPLES

Security is non-negotiable. Observability never weakens security boundaries. Intelligence data access is capability-controlled. AI models consume intelligence and never influence kernel policy directly. Least privilege applies to all kernel services including intelligence. Telemetry data is classified. AI Gateway authenticates and authorises every query. Sensitive FR fields are encrypted at rest. Security events are CRITICAL where specified and never silently dropped. Diagnostic recommendations never weaken security posture.

## CAPABILITY MODEL

Each process has Permitted, Inheritable, and Effective sets. Privileged operation checks Effective. Missing capability returns EPERM and emits SECURITY_SYSCALL_DENIED.

CAP_SAIOS_INTELLIGENCE grants SAIRU query and SGQL access. CAP_SAIOS_TELEMETRY grants restricted telemetry fields. CAP_SAIOS_ORCHESTRATE grants submission of approved SAIRU tasks through contract APIs. CAP_SAIOS_POLICY grants Policy Engine rule modification. None bypass SecurityContract.

## MANDATORY ACCESS CONTROL

SecurityContract provides an LSM-compatible hook framework. SMAP labels contain type, sensitivity 0-255, and category set. Access is permitted only when subject label dominates object label under lattice ordering. Policy is loaded from a boot text file. SECURITY_MAC_DENIED is CRITICAL and includes subject_label, object_label, operation, and policy_rule.

## NAMESPACE ISOLATION

A process may not name, observe, signal, or communicate with resources outside its namespace without specific capability grants. Denial emits SECURITY_NAMESPACE_ESCAPE at CRITICAL severity and returns EPERM. Three such events from one PID within 10 seconds become evidence for DOC-17 correlation.

Namespace types: PID, network, mount, UTS, IPC, user, and cgroup. User namespace creation requires root or explicit policy approval.

## AUDIT TRAIL

All security events are persisted to FR before acknowledgement. A dedicated VFS security audit log is written synchronously as JSON Lines containing KDS event ID, timestamp, event type, subject PID and executable, object identifier, operation, and outcome.

## SECURITY EVENTS

SECURITY_SYSCALL_DENIED: pid, syscall_number, policy_id, action. SECURITY_PRIVILEGE_ESCALATION: pid, old_credentials, new_credentials, operation. SECURITY_NAMESPACE_ESCAPE: pid, namespace_type, target, action. SECURITY_INTEGRITY_VIOLATION: subject, object, violation_type. SECURITY_AUDIT_EXEC: pid, executable, credentials, outcome. SECURITY_NETWORK_POLICY_DENY: pid, namespace, destination, policy_id. SECURITY_MAC_DENIED: subject_label, object_label, operation, policy_rule. CONTAINER_CREATE: container_id, root_pid, namespace_set. CONTAINER_DESTROY: container_id, exit_status.

## CONTAINER SUPPORT

Native containers use PID, network, mount, UTS, IPC, user, and cgroup namespaces. OCI image format is native. Namespace creation is capability-gated. CONTAINER_CREATE and CONTAINER_DESTROY are mandatory.

## COMPATIBILITY INVARIANTS

Compatibility shims never bypass SecurityContract. Linux and POSIX-shim operations have the same security constraints as native operations. Compatibility shims emit the same KDS events as native operations.

## COMPLETION CHECK

A developer can implement MAC hooks, capability checks, audit writer, namespace escape denial, container event emission, and SAIOS-specific capability enforcement.
