# SAIOS SAIRU and Intelligence Architecture Specification
**Document ID:** DOC-17_SAIRU_Intelligence_Architecture.txt
**Layer:** Intelligence Layer
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01, DOC-10, and DOC-16

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt SAIRU - THE INTELLIGENCE INTERFACE; SAIRU ENGINES; FLIGHT RECORDER ARCHITECTURE read path; DIAGNOSTIC, PREDICTIVE, and OPTIMIZATION INTELLIGENCE; CORRELATION ENGINE; KGS; CAUSAL CHAIN; CONFIDENCE SCORING; EVENT RELATIONSHIP MODEL; SGQL; RAF attribution. SAIOS_SSOT_Part2.txt ACCOUNTING CONSTITUTION attribution chain requirement.

## SAIRU DEFINITION

SAIRU explains, diagnoses, predicts, guides, and orchestrates approved workflows. SAIRU is not a chatbot, log viewer, override authority, post-mortem-only agent, or owner of execution, storage, scheduling, security, or memory. SAIRU consumes evidence; it does not own canonical kernel state.

## AUTHORITY BOUNDARY

SAIRU may observe KDS evidence, explain system state, diagnose failures, predict trends, recommend actions, and orchestrate approved workflows through contract APIs. SAIRU may not bypass contracts, modify kernel state directly, ignore validation gates, ignore safety constraints, or violate subsystem ownership.

Phase One requires SAIRU to function with no AI model installed. All seven engines operate deterministically on KDS evidence. AI models assist later but are never required for SAIOS understanding.

## SEVEN ENGINES

Context Engine reconstructs system state from KDS event history by timestamp and optional scope. Tool Engine exposes contract APIs as named tools with parameter and return schemas, subject to Policy Engine approval. Skill Engine stores named diagnostic and recovery sequences. Task Engine executes multi-step approved workflows and logs every step to KDS; tasks are pauseable, resumable, and cancellable. Knowledge Engine performs KDS queries, KGS queries through SGQL, causal search, trend analysis, and anomaly detection. Planning Engine assembles skills into plans with prerequisites, expected outcomes, rollback actions, and human approval. Policy Engine validates ownership, capability, safety pre-checks, and reversibility; failure rejects the action with a reason.

## CORRELATION ENGINE

Ingestion consumes raw KDS events from per-CPU rings in timestamp order, deduplicates by event_id, and normalises payloads. Correlation matches events against a rule library. Analysis assembles correlated pairs into causal chains and inserts them into KGS.

A rule contains antecedent event pattern, consequent event pattern, maximum temporal window, minimum confidence threshold, and causal relationship type.

## KGS MODEL

Node types: ProcessNode, DeviceNode, FileNode, NetworkConnectionNode, MemoryRegionNode, SubsystemNode, UserNode, and TimeIntervalNode. Each records stable ID, source event IDs, timestamps, ownership metadata, and type-specific attributes such as PID, device_id, inode, socket tuple, virtual range, subsystem name, UID, or time bounds.

Edge types: CAUSED, ENABLED, BLOCKED_BY, DEPENDS_ON, PRODUCED, CONSUMED, and CO_OCCURRED_WITH. Each edge has source, target, confidence, relationship type, evidence event IDs, and temporal window.

## CAUSAL CHAIN AND CONFIDENCE

Causal chain construction runs backward BFS from the trigger event following CAUSED, ENABLED, and DEPENDS_ON edges. Depth limit is 20. Chains below confidence 0.1 are pruned. Output is an ordered list of event_id, description, confidence tuples.

Confidence uses 16-bit fixed-point from 0.0 to 1.0 with resolution about 0.0000152. Base confidence comes from the rule. Adjustments include temporal proximity with 1-second half-life, rolling Bayesian co-occurrence frequency, and contradiction penalty for mutually exclusive hypotheses.

## BUILT-IN RULES

OOM-kill precursor: OOM_PRESSURE sustained plus MEMORY_ACCOUNT_PERIOD growth leads to OOM_KILL within configured window. Scheduler stall from IRQ storm: IRQ_STORM precedes SCHED_STALL. Process crash from memory corruption: MCE_USER_FRAME or PAGE_FAULT unresolvable precedes PROCESS_TERMINATE. Network congestion from storage pressure: PAGE_CACHE_WRITEBACK or storage delay correlates with TCP_RETRANSMIT/NET_CONGESTION. Driver fault from hardware error: MCE, IOMMU_FAULT, or device telemetry degradation precedes DRIVER_ERROR.

## SGQL

SGQL is Cypher-inspired and returns JSON. Queries that access restricted telemetry fields require SecurityContract capability checks. Example forms:

MATCH (p:Process)-[:CAUSED]->(e:Event {type: 'OOM_KILL'}) RETURN p, e
MATCH path = (d:Device)-[:CAUSED*1..5]->(e:Event {severity: 'CRITICAL'}) RETURN path
MATCH (p:Process)-[:CONSUMED]->(r:Resource {type: 'CPU'}) WHERE r.period = '5m' RETURN p, r

Custom runtime rules are validated for circular causation and minimum evidence, and receive lower base confidence than built-in rules.

## DIS, PIS, AND OIS

DIS triggers on Red Ring, CRITICAL KDS event, ProgressContract threshold breach, or user request. Output includes primary_event, causal_chain, confidence_score, affected_entities, explanation, recommended_actions, and prevention_recommendation.

PIS models: OOM prediction with projected exhaustion time; disk exhaustion in 24-hour window; TSC divergence on multi-socket; driver health degradation; memory fragmentation. Each model defines input signals, threshold, emitted predictive event, confidence, and payload.

OIS advisory recommendations: NUMA affinity, scheduler class, memory policy, IRQ affinity, and huge page use. Recommendations are never applied automatically.

## ATTRIBUTION REQUIREMENT

SAIRU answers resource questions with causal narratives backed by KDS events and RAF attribution chains. Every report references the event IDs justifying the conclusion.

## FLIGHT RECORDER READ PATH

SAIRU can query FR data older than live rings with the same SGQL interface and time-range filters. The read path requires no scheduler, VFS, or syscall and uses the independent path established at boot.

## AI MODEL INTEGRATION

Phase 6 AI Gateway enforces CAP_SAIOS_INTELLIGENCE and CAP_SAIOS_POLICY. AI output is labelled AI-assisted and is never confused with deterministic KDS-derived conclusions. SAIOS is AI-model agnostic; the OS is the intelligence source.

## COMPLETION CHECK

A developer can implement CE ingestion, KGS nodes and edges, causal BFS, fixed-point confidence scoring, SGQL parser interface, and five PIS models without consulting another document.
