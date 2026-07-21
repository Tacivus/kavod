# Causal Trace, Logs, And Observability

> **Status:** Settled for the Kavod MVP diagnostics boundary
> **Scope:** Automatic audit records, user logging, causal correlation, recording outputs, buffering, failure policy, metrics, and optional distributed tracing

## Conclusion

Kavod has one Engine-owned diagnostics facility and one ordered audit stream per Engine run.

```text
automatic Kavod audit records ----\
                                   +--> unified diagnostics pipeline --> configured outputs
user ctx.log.*() records ----------/             |
                                                 +--> bounded memory buffering
                                                 +--> filtering and batching
```

The stream contains distinct automatic-audit and user-log record kinds, but they share correlation, ordering, buffering, and output configuration. Separate physical Event tapes, Command tapes, causal-trace files, and user-log files are not required by the semantic model. A configured writer may expose filtered logical views over the one stream.

The exact implementation remains open. Kavod may use the Rust `log` ecosystem, `tracing`, a small custom recorder, or a combination behind its public diagnostics boundary. No dependency-specific type is part of the settled application API.

Diagnostics exist only to answer:

> What did this Engine instance observe, do, and decide?

Diagnostics are never application state, broker truth, a recovery log, an outbox, a Command-delivery obligation, or permission to resume an earlier Engine instance.

## Terminology And Authority

| Term | Meaning |
|---|---|
| Diagnostics | The Engine-owned facility that accepts automatic audit records and user logs and routes them to configured outputs |
| Audit record | A structured record emitted automatically by Kavod for a kernel, Environment, or Port-boundary action |
| User log | An observational record emitted explicitly through `ctx.log.*()` |
| Audit stream | The ordered stream containing both record classes for one Engine run |
| Metric | An aggregate operational measurement; not an audit record and not ordered with the audit stream |
| Distributed trace | Optional exporter-dependent telemetry using external trace and span identities |

Kavod should not call this facility a journal. In event-sourced, database, and replicated-state-machine systems, a journal normally implies restoration, authoritative history, or redelivery. Those semantics are explicitly absent here.

`AuditRecord`, `audit stream`, or `diagnostics` accurately describe the facility without granting recovery authority. `Trace` is a recording detail level, not the facility's name.

## One Unified Stream

The logical stream contains a tagged union:

```text
DiagnosticRecord
    = AutomaticAudit(AuditRecord)
    | UserLog(UserLogRecord)
```

This provides one correlation and output path while preserving the important distinction between records Kavod emits automatically and text or fields supplied by application code.

Metrics remain aggregates rather than stream records. OpenTelemetry export remains an optional projection from selected diagnostic activity rather than the audit stream's storage or identity model.

## Common Record Envelope

Every diagnostic record carries the applicable subset of:

- Engine run identity.
- Monotonic diagnostic record sequence.
- Event index when associated with an accepted Event turn.
- Turn action sequence when associated with deterministic kernel work.
- Parent action sequence when one recorded action directly caused another.
- Callback identity when emitted during or about a callback.
- Port instance identity when emitted by or about a Port.
- Produced Command identity when associated with one Command.
- Frozen logical time when associated with an Event turn.
- Optional wall time and thread information supplied by diagnostics infrastructure.
- Record kind and level.

Diagnostic record sequence and deterministic kernel order are not the same concept:

- Event index and turn action sequence describe deterministic kernel order.
- Diagnostic record sequence describes the order in which one recorder observed records.
- Concurrent live Port records may race for diagnostic record sequence.
- Neither sequence is a business identifier.

Wall time and thread information may be attached by the diagnostics infrastructure without exposing either capability to Components or Reducers.

## Recording Levels

Automatic audit detail has four levels:

| Level | Automatic records |
|---|---|
| `Off` | No automatic audit records |
| `Audit` | Run lifecycle, accepted Events, produced Commands, consequential Port-boundary actions, recording gaps, and faults |
| `Debug` | `Audit` plus turn boundaries, callback invocation, and produced Messages |
| `Trace` | `Debug` plus callback completion, canonical-state mutation boundaries, and detailed kernel and Port actions |

User logs use the familiar severities `Error`, `Warn`, `Info`, `Debug`, and `Trace`. User-log filtering is separate from automatic audit detail even though both record classes use the same pipeline and outputs.

`Trace` enables an automatic record for every semantic action visible to Kavod, not every Rust statement or field assignment. This includes every callback boundary, Message and Command production, canonical-state mutation boundary, and configured Port/runtime action. Best-effort failure may still create an explicitly accounted recording gap; required-record failure stops the Engine.

Full tracing can be expensive. Its volume scales approximately with:

```text
accepted Event rate
+ Message production rate
+ Command production rate
+ two records per callback invocation
+ state-mutation boundaries
+ Port operational activity
+ user Trace logs
```

Trace buffering, write rate, dropped-record count, and sink latency must be measured. Full tracing is not assumed safe for every live workload merely because it is configurable.

## Automatic Audit Records

### Audit Level

The `Audit` level includes at least:

- `RunStarted`, including executable/application identity, graph identity, determinism-affecting configuration identity, and diagnostics configuration.
- `EventAccepted`, including complete Event payload, source Port identity, Event index, and acceptance time.
- `CommandProduced`, including complete Command payload, destination Port, root Event index, producing callback, and production order.
- Consequential Port-boundary observations, including worker lifecycle, Command handoff or submission attempts where observable, and infrastructure faults.
- `EngineFault` and terminal runtime failures.
- `RecordingGap` or equivalent loss accounting when a best-effort path loses records and later remains usable.
- `RunStopped`, including terminal status and last completed Event index.

An audit record that says a Command was produced or offered to a Port does not prove that a broker received or executed it. External acknowledgements and reconciliation remain external truth and enter the application as typed Events when application behavior depends on them.

### Debug Level

The `Debug` level adds at least:

- `TurnStarted` and `TurnCompleted`.
- `ReducerInvoked`.
- `ComponentInvoked`.
- `MessageProduced`.

Invocation is recorded before entering the callback. If a callback panics or the process terminates, the absence of its completion record helps locate the active operation.

### Trace Level

The `Trace` level adds at least:

- `ReducerCompleted`.
- `StateModified`.
- `ComponentCompleted`.
- Detailed queue, acceptance, publication, and Port observations selected by configuration.

`StateModified` records that a Reducer completed with mutable access to canonical `AppState`. It does not claim that a particular field or byte changed. The MVP records no old value, new value, field-level diff, state serialization, or state hash.

A Reducer panic may leave partially changed state and therefore has no successful `StateModified` completion record. The fault record and preceding `ReducerInvoked` identify the mutation boundary at which execution failed. Kavod does not roll the state back or resume that Engine.

Component-private state changes are not automatically detected. `ComponentInvoked` and `ComponentCompleted` identify the callback boundary within which private state may have changed.

## Causal Correlation

The MVP does not need an independent distributed-trace identity system for deterministic causality. The following are sufficient:

```text
Engine run ID
+ root Event index
+ turn action sequence
+ parent action sequence
+ callback identity
```

Every produced Command records at least:

- Root Event index.
- Command production order.
- Producing callback identity.
- Immediate parent action when detailed causal recording is enabled.
- Destination Port instance.

At `Audit`, the root Event and producing callback explain the Command's outer cause. At `Debug` and `Trace`, Message and callback records with parent action sequences reconstruct the detailed causal chain.

Callback identity is meaningful within the recorded executable and frozen graph. The MVP does not promise a callback identity stable across different builds. Human callback names are diagnostic labels, not routing keys.

## User Logging

Components, Reducers, and Ports may emit user-authored logs through their Kavod context:

```text
ctx.log.error(...)
ctx.log.warn(...)
ctx.log.info(...)
ctx.log.debug(...)
ctx.log.trace(...)
```

The exact message and structured-field syntax remains open. The capability semantics are settled:

- The logging capability is write-only.
- Calls return no recorder status or sampling decision.
- No `enabled()`, `is_sampled()`, writer handle, flush handle, or exporter handle is exposed.
- Correlation fields are attached automatically from the current context.
- Components and Reducers do not receive wall-clock or thread access through logging.
- Port diagnostics may include wall time, thread information, transport errors, and latency.
- Logging is observational and cannot replace a typed Event, Message, Command, or state transition.

Kavod does not guarantee that arbitrary user formatting is cheap. The API should avoid lazy callbacks whose execution depends on whether a record is enabled, because such callbacks would make logging configuration observable to application code.

Business facts that affect decisions must remain typed protocol or state facts. Examples include risk rejection, reconciliation mismatch, trading permission, disconnect state when application logic reacts to it, and broker acknowledgement. A log may explain such a fact but cannot be its only representation.

## Configuration Ownership

Diagnostics are configured on the Engine run, not on the immutable Application graph.

Configuration has independent dimensions:

| Dimension | Meaning |
|---|---|
| Automatic audit detail | `Off`, `Audit`, `Debug`, or `Trace` |
| User log filter | Minimum enabled user-log severity and optional target filters |
| Output | Retained memory, local disk, human console formatting, or future external export |
| Buffering | Capacity, batching, and flush policy |
| Failure policy | Best effort or required for the selected automatic audit records |

The public configuration should remain opaque and builder-created so that future outputs and required policies can be added without exposing backend-specific types. Exact builder syntax is deferred.

The default MVP path uses bounded in-memory buffering and ordinary batch writes to configured outputs. Exact default capacity, batch size, and flush interval remain implementation choices that must be measured. A retained in-memory output is useful for tests; long-running live use must not assume unbounded retention.

## Recording Modes And Failure Semantics

Useful semantic configurations include:

| Configuration | Meaning |
|---|---|
| Disabled | No automatic audit; user logging may also be disabled by filter |
| Memory | Records are retained or buffered in memory for tests and local inspection |
| Disk best effort | Disk, buffer, or writer failure is visible operationally but execution continues |
| Required recording | The configured automatic record must cross the writer's declared acknowledgement boundary or the Engine stops |
| External export | A future asynchronous output; best effort by default |

The destination and failure policy are conceptually independent even if convenience configuration later combines them.

### Event Acceptance

The Acceptor prepares the candidate Event index, acceptance time, source identity, and root action, then emits `EventAccepted` according to the selected policy before callback dispatch.

- Under best effort, recording is attempted and the Event is committed as accepted even if recording fails.
- Under required recording, the Event is committed as accepted only after the configured writer acknowledges the automatic record.
- If required recording fails, the Event is not dispatched, the fatal latch is set, and the Engine does not continue.
- The acceptance commit remains the semantic linearization point. A record is diagnostic evidence of that operation, not the authority that makes the external Event true.

The same rule applies before publishing produced Commands when required Command audit is configured: required records must be acknowledged before the Command batch crosses the Port boundary. This creates no resend, delivery, or external-effect guarantee.

The writer acknowledgement boundary must be explicit:

- Memory acknowledgement means successful admission to the configured memory recorder or buffer.
- Buffered disk acknowledgement may mean only successful admission to memory and does not imply crash durability.
- A future disk-required mode must state whether acknowledgement means write completion, flush, or data synchronization.

Kavod must not call buffered admission durable. Exact disk-required and fsync semantics remain deferred.

### User Logs And Required Audit

User logs are best effort from callback code even when automatic audit records are required:

- A `ctx.log.*()` call never returns failure and never changes callback control flow.
- Optional user logs must not consume capacity reserved for required automatic records.
- If a shared physical sink becomes unhealthy, the failure is handled by diagnostics supervision and any required automatic record fails at a kernel safe boundary.
- A callback is never interrupted because one user log could not be written.

Required recording intentionally affects Engine liveness. It must not affect successful callback outputs or permit callbacks to observe diagnostics state. Recording configuration is therefore Engine provenance and must be included in run diagnostics.

### Buffer Exhaustion And Sink Failure

- Best-effort exhaustion drops diagnostic records, increments explicit counters, and emits a gap record if the stream later becomes writable.
- Required automatic-record exhaustion is terminal; it is not silently downgraded to best effort.
- User logs are discarded before capacity reserved for required automatic records is consumed.
- OpenTelemetry exporter failure, console failure, and external-export backpressure never block deterministic callbacks.

No recorder can guarantee a complete final record after abrupt process termination, hardware failure, or failure of the recorder itself. Required recording means Kavod does not knowingly continue past a failed required acknowledgement; it does not create impossible evidence.

## In-Memory And Disk Semantics

Memory and disk outputs consume the same logical diagnostic records.

An in-memory output:

- Is intended for tests, short runs, and local inspection.
- Preserves accepted record order.
- Has explicit bounded or caller-owned retention.
- Does not silently overwrite records without loss accounting.

A serializable disk output must eventually:

- Preserve record boundaries and diagnostic record order.
- Identify the Engine run, executable/application, graph, configuration, and record schema.
- Make a truncated or incomplete tail visible.
- Detect ordinary corruption rather than silently reinterpret bytes.
- Preserve unknown-record boundaries if compatible inspection is attempted.

Exact encoding, framing, checksums, segmentation, retention, rotation, compression, and schema evolution are not MVP semantic commitments. The MVP makes no cross-build decoding or replay guarantee. These concerns should be chosen only when a disk implementation is selected.

Retention deletes diagnostic evidence; it does not checkpoint state. Compaction, snapshots, and log truncation must not be described as recovery mechanisms.

## Port Diagnostics

Kavod records Port activity at two levels:

- Environment-owned automatic records for worker lifecycle, queue interaction, Command handoff, unexpected exit, and known runtime failures.
- Port-authored `ctx.log.*()` records for protocol details such as reconnects, wire errors, remote status, submission attempts, and wall-clock latency.

If a reconnect, timeout, rejection, or service state changes application behavior, the application must also represent it as a typed Event. A Port log alone cannot trigger deterministic behavior.

Port records should correlate to a Command identity when they concern one produced Command. Port-local operation sequence and wall time may supplement, but never replace, the Command's kernel identity.

## Metrics

Metrics are aggregate operational signals and are not written into the ordered audit stream by default.

The MVP must make it possible to observe:

- Per-Port Event queue occupancy, capacity, high-water mark, full outcomes, and offer-to-accept lag.
- Acceptor rounds, quantum use, accepted Event counts, and service lag.
- Per-Port Command mailbox occupancy, reservation failures, and queueing lag.
- Turn duration and Message, callback, and Command counts.
- Callback duration by stable callback identity where cardinality is bounded.
- Worker startup, reconnect, failure, unexpected exit, and fatal-latch counts.
- Diagnostics buffer occupancy, batch size, write latency, dropped records, gaps, and sink failures.
- OpenTelemetry or external-export drops when those outputs are configured.

Metric labels must exclude high-cardinality identities such as:

- Engine run ID.
- Event index or turn action sequence.
- Command, order, request, account, or trace identifiers.
- Instrument identifiers unless an application knowingly defines and bounds that metric dimension.
- Error text, file paths, or arbitrary user strings.

High-cardinality correlation belongs in audit records and logs, not metric labels.

## OpenTelemetry And Distributed Context

OpenTelemetry is optional external telemetry, not Kavod causal identity or audit storage.

- OTel traces may be sampled or dropped.
- Exporter queues and backpressure are outside deterministic application behavior.
- OTel trace and span IDs are unrelated to Event index and turn action sequence.
- OTel IDs may be attached to diagnostic records for correlation but never used for routing, ordering, business identity, or replay comparison.

If distributed context later crosses a Port boundary, it travels as Environment- or Port-managed sidecar metadata rather than inside the typed business payload. Components and Reducers do not read it. Invalid or absent context cannot alter Event acceptance or Command meaning.

If diagnostic replay is later implemented, it should create new OTel traces and link them to original traces when useful. It should not reuse old trace IDs as though replay were the original live execution.

## Diagnostic Replay

Replay, state hashes, and divergence tooling are not MVP diagnostics guarantees.

The audit stream deliberately preserves accepted Events and produced Commands so a later diagnostic tool may use them. If added, that tool must:

- Cold-start a new Engine instance.
- Treat recorded Events only as explicit diagnostic inputs to that isolated run.
- Use passive Ports and produce no live external effects.
- Compare produced Commands and report the first observed mismatch.
- State honestly that without state hashes or detailed causal records, the first Command mismatch may be later than the first internal divergence.
- Create new diagnostic and distributed-trace identities linked to, but distinct from, the original run.

No replay result authorizes live Command delivery or restoration of the original Engine.

## Comparable Patterns

| Pattern | Verified lesson | Kavod decision |
|---|---|---|
| [LMAX architecture](https://martinfowler.com/articles/lmax.html) | Ordered input recording and deterministic execution support diagnosis; LMAX also uses its journal for recovery | Borrow ordered diagnostic capture, reject recovery authority |
| [Aeron Archive](https://github.com/aeron-io/aeron/wiki/Aeron-Archive) | Stream recording has explicit positions, durability configuration, segmentation, replay, and optional checksums | Borrow explicit recording mechanics only when disk storage is implemented |
| [Aeron Cluster](https://github.com/aeron-io/aeron/wiki/Cluster-Tutorial) | Deterministic services plus a replicated log and snapshots support recovery | Borrow determinism cautions, reject replicated recovery and snapshots |
| [Event Sourcing](https://martinfowler.com/eaaDev/EventSourcing.html) | Event logs are commonly authoritative, but may instead exist only for audit and special processing | Adopt only the audit interpretation |
| [NautilusTrader live operation](https://nautilustrader.io/docs/latest/concepts/live/) | Live reconciliation treats venue reports as external reality | Preserve cold-start reconciliation and external truth |
| [OpenTelemetry](https://opentelemetry.io/docs/specs/otel/trace/sdk/) | Sampling, bounded queues, dropped spans, exporter failure, and links are normal trace behavior | Keep OTel optional and non-authoritative |

These patterns justify separation and observability mechanics. They do not justify importing event-store restoration, snapshots, replicated failover, outbox delivery, or exactly-once claims.

## Settled Rules

1. Kavod has one Engine-owned diagnostics facility and one logical audit stream per run.
2. Automatic audit records and user logs are distinct record kinds in the same pipeline.
3. Diagnostics configuration belongs to the Engine run, not the Application graph.
4. The exact Rust logging or tracing dependency remains open.
5. Components, Reducers, and Ports log through `ctx.log.error/warn/info/debug/trace` semantics.
6. Context logging is write-only and reveals no enablement, sampling, buffering, or writer state.
7. Automatic audit detail and user-log severity are configured separately.
8. `Trace` enables an automatic record for every semantic action visible to Kavod; configured failure policy determines whether a missing acknowledgement creates an accounted gap or stops the Engine.
9. `StateModified` records only a successful Reducer mutation boundary, never old or new state values.
10. Event index and turn action sequence define deterministic kernel order; diagnostic record sequence defines recorder observation order.
11. Every Command retains its root Event, production order, producer callback, and destination Port.
12. Event and Command payloads are recorded at the Audit level; Message records begin at Debug.
13. Business facts remain typed Events, Messages, Commands, or state.
14. The default diagnostics path uses bounded memory buffering and batched output writes.
15. Output, buffering, detail, and failure policy are independently configurable.
16. Best-effort diagnostics failure does not stop execution.
17. Required automatic-record failure is terminal and never silently downgraded.
18. Required writer acknowledgement semantics must be explicit and must not misuse the word durable.
19. User logs remain best effort and cannot consume capacity reserved for required automatic audit.
20. Port operational logs do not prove external receipt or effect.
21. Metrics exclude high-cardinality run, causal, trace, and business identifiers.
22. OpenTelemetry identity, sampling, and export are outside deterministic semantics.
23. Diagnostic records never restore state, resume an Engine, or require Command delivery.

## Rejected Alternatives

- **Journal terminology:** implies recovery or authoritative history that Kavod explicitly rejects.
- **Separate Event, Command, and causal-trace files as a semantic requirement:** duplicates correlation and configuration; filtered views over one stream are sufficient.
- **OpenTelemetry as the audit record:** sampling and exporter loss make it unsuitable.
- **Process-global logger as the Kavod context API:** weakens per-Engine isolation and leaks implementation choice.
- **Direct logger or exporter handles in contexts:** exposes availability and backend behavior to deterministic code.
- **Logging enablement queries:** permit business behavior to depend on telemetry configuration.
- **Old/new state capture in the MVP:** imposes cloning, serialization, cost, and schema commitments without a current need.
- **Random sampling of automatic causal records:** produces misleading partial traces.
- **Treating Port submission logs as broker truth:** only external responses and reconciliation establish external reality.
- **Required user-authored logs:** arbitrary application verbosity must not consume required audit capacity or become a safety dependency.

## Explicit Non-Goals

The MVP diagnostics design does not provide:

- State restoration, Engine resumption, snapshots, or recovery.
- Durable Command outbox, resend, or delivery guarantees.
- A source of truth for broker or venue state.
- Cross-build record compatibility.
- Stable state serialization or state hashes.
- Full diagnostic replay or divergence tooling.
- A standardized disk encoding or retention policy.
- Guaranteed preservation after process, hardware, or recorder failure.
- Proof that diagnostics have zero physical performance effect.

Instrumentation consumes CPU and memory and may change live latency or which external Event becomes visible first. The enforceable invariant is narrower: callbacks cannot observe diagnostics configuration or failure and cannot branch on it. Once an Event is accepted, diagnostics do not reorder callbacks or alter successful callback outputs. A required-record failure may terminate the Engine at a safe boundary and truncate the remaining deterministic execution prefix.

## Dependencies And Required Reconciliation

This report refines the determinism and live-runtime reports where they previously described successful journal append as the unconditional Event-acceptance linearization point:

- The acceptance commit is the semantic linearization point.
- Best-effort recording failure does not prevent that commit.
- Required recording acknowledgement gates the commit and callback dispatch.
- Recording configuration may affect liveness under failure but never grants recovery authority.

The state report's observability dependency is resolved for the MVP: record only the `StateModified` boundary and defer state values, hashes, serialization, and replay comparison.

## Open Questions

No unresolved question blocks the MVP diagnostics semantics.

The following implementation and future-feature decisions remain deliberately open:

- Whether the backend uses `log`, `tracing`, a custom recorder, or adapters among them.
- Exact `ctx.log.*()` message and structured-field syntax.
- Exact Engine diagnostics builder syntax.
- Default memory capacity, batch size, and flush interval.
- Concrete record queue, writer-thread, and capacity-reservation implementation.
- Event, Message, and Command disk encoding.
- Disk framing, checksums, segmentation, retention, and compression.
- Exact disk-required acknowledgement and synchronization policies.
- External export and OpenTelemetry integration.
- Cross-build schema evolution.
- Diagnostic replay, state hashing, and divergence reports.
