# Kavod Core Design v7

> **Status:** Semantic draft for review
> **Scope:** Deterministic application turns, typed Port protocols, ordered Events and Commands, mandatory audit evidence, State audit encoding, fail-stop behavior, and the common contract required to run the same Application in live and simulation
> **Priorities:** Correctness, determinism, explicit authority, bounded Kavod-owned resources, no steady-state Core allocation, and the smallest semantics that can be implemented and tested with confidence

---

## 1. Purpose And Thesis

Kavod is a domain-agnostic deterministic application Core intended to run the same application logic against live and simulated Environments.

The central thesis is:

> A single-writer Engine synchronizes one ordered Event before invoking one synchronous application transition, synchronizes the transition's complete Command intent before attempting effects, and begins no later transition until the current turn's local outcome is synchronized.

Kavod deliberately keeps application architecture outside the Core. An Application may internally use ordinary functions, modules, state machines, reducers, components, or locally drained work queues. Kavod does not register, schedule, order, inspect, or audit those internal concepts.

The Core is responsible only for boundaries it can authoritatively enforce:

- One owner and writer of application State.
- One total order of accepted Events.
- One non-reentrant application handler.
- Typed static Port protocols and logical Port identity.
- Deferred ordered Commands.
- Mandatory audit barriers around execution and handoff.
- Monotonic fail-stop closure.
- Finite Kavod-owned capacities established before the run begins.
- The same Application semantics regardless of Environment mode.

This document defines semantic requirements. Rust syntax is illustrative only. Concrete APIs, traits, derives, macros, registries, queue implementations, storage layouts, and Environment mechanics remain TBD unless a semantic property explicitly requires them.

## 2. Explicit Non-Goals

Kavod Core does not provide or promise:

- Parallel application execution.
- State rollback after an incomplete turn.
- State restoration, replay execution, migration, or continuation of a failed run.
- A durable Command outbox, automatic resend, or automatic retry.
- Exactly-once external effects.
- External-effect rollback or cross-Port atomicity.
- Automatic Port quarantine, replacement, or restart.
- Deterministic live Event arrival.
- Deterministic networks, brokers, operating-system scheduling, or external systems.
- A total process-memory bound for application State, payloads, Ports, models, encoders, or custom audit persistence.
- Domain concepts such as orders, fills, positions, reconciliation, arming, cancellation, or flattening.

The AuditLog is evidence. It is not application State, external truth, recovery authority, or a Command outbox.

## 3. Terminology

| Term | Meaning |
|---|---|
| Application | One frozen application definition, initial State, closed App Event protocol, handler, Fatal Reason protocol, declared Port Slots, audit encoders, and deterministic configuration |
| Engine | The owner and coordinator of one run |
| Core | The Kavod-owned deterministic execution machinery inside the Engine |
| Environment | The interchangeable live or simulated boundary; its concrete design is TBD |
| Port Contract | A reusable application-defined association between one typed Event protocol and one typed Command protocol |
| Port Slot | One statically declared logical occurrence of one Port Contract in one Application |
| Port Event | An immutable fact staged through one Port Slot for possible acceptance |
| Engine Event | A built-in fact; an example is `Ready` |
| App Event | The Application's one closed Event value delivered to its handler |
| Event source | Either the Engine or one authoritative Port Slot |
| Event envelope | One App Event together with source, Event index, and logical acceptance time |
| Accepted Event | An Event envelope whose complete acceptance boundary the Engine observed synchronize successfully |
| State | The Application's one concrete Engine-owned mutable value |
| Handler | The Application's single synchronous `on_event` transition |
| Command | An immutable typed request directed to one declared Port Slot |
| Turn | Processing of one accepted Event through handler execution, optional State encoding, Command preparation and handoff, and synchronized completion |
| Logical time | The accepted Event's frozen acceptance time, visible to its handler |
| Event index | The checked monotonic total order of accepted Events |
| AuditLog | The Kavod-owned ordered semantic record stream and its synchronization barriers |
| AuditWriter | A configured persistence boundary for already framed audit bytes |
| Handoff | One Command crossing Kavod's local boundary according to the bound Port's declared contract |
| Run gate | Monotonic Engine authority that prevents new admitted operations after closure |

## 4. Settled Invariants

1. One Engine thread owns and mutates application State.
2. At most the one synchronous `on_event() handler invocation is active.
3. Accepted Events are ordered exclusively by checked monotonic Event index.
4. `Ready` is structurally Event index zero, and no Port Event is accepted before the Ready turn completes.
5. The complete Event envelope synchronizes before handler admission.
6. One accepted Event receives at most one handler admission and is never automatically retried.
7. Handler program order is application semantic order.
8. The handler returns Continue, Stop, or one application-defined Fatal Reason.
9. Stop is legal only when the current turn produced no Commands.
10. Commands cannot cross a Port boundary during handler execution.
11. Complete ordered Command intent synchronizes before the first current-turn handoff.
12. Commands receive at most one handoff attempt in successful staging order.
13. `TurnCompleted` synchronizes only after every current-turn handoff succeeds.
14. No later handler begins before the preceding `TurnCompleted` synchronization succeeds.
15. `ctx.sync_state()` requests one encoding of final handler State and performs no immediate IO.
16. Requested State evidence becomes authoritative only with successful turn completion.
17. Initial State encoding synchronizes before Ready acceptance.
18. Every fatal cause enters one common fatal path and permanently poisons any incomplete State.
19. Authoritative host stop is Fatal. Cooperative host shutdown is an ordinary Engine Event.
20. Once the run gate closes, no new gated operation begins. One already admitted operation may reach its defined boundary.
21. A failed run is never resumed. A later execution is a new run with new State and identity.
22. Application shape, Port Slots, bindings, ordering configuration, capacities, and audit configuration are frozen before `RunStarted`.
23. Application code cannot observe Environment mode or AuditWriter mode.
24. After `RunStarted` synchronization, Kavod-owned Core code performs no heap allocation or capacity growth.
25. Every Kavod-owned collection, buffer, record, and identifier domain has a finite validated bound.
26. Kavod identity never substitutes for application-owned business identity or idempotency policy.

## 5. Application Model

### 5.1 One State And One Handler

An Application supplies one concrete State value and one synchronous handler. Conceptually:

```rust
fn on_event(
    state: &mut AppState,
    event: &AppEvent,
    ctx: &mut Context,
) -> Outcome<AppFatalReason>;
```

State contains every behavior-affecting mutable application value. Kavod defines no additional application State classes.

The handler may:

- Read and mutate complete State.
- Read the immutable App Event.
- Read Event index, logical time, and authoritative Event source.
- Stage typed Commands to declared Port Slots.
- Request final-State audit encoding for the current turn.
- Return Continue, Stop, or an application Fatal Reason.

The handler receives no Engine, Environment, AuditWriter, Port implementation, external IO, wall-clock, entropy, or concurrency authority.

Ordinary Rust code is not sandboxed. Determinism still depends on application discipline, review, testing, and controlled dependencies.

### 5.2 Outcomes

Conceptually:

```rust
enum Outcome<F> {
    Continue,
    Stop,
    Fatal(F),
}
```

**Continue** requests ordinary turn completion and later Event processing.

**Stop** requests graceful application termination after the output-free current turn completes. Stop plus any current-turn Command is an application contract violation and Fatal.

**Fatal(reason)** reports that the Application detected a condition under which continuing is unsafe. It follows the common fatal path. Current-turn Commands and State-sync requests are suppressed.

The Fatal Reason is one Application-associated concrete protocol with application-defined audit encoding. Exact representation is TBD.

Panic is not an application outcome. An unwind crossing the handler boundary is Fatal. No unwind, audit, cleanup, or outcome path is guaranteed for aborting panic, allocator abort, stack overflow, process destruction, or equivalent failures.

The handler has no recoverable generic error result. Expected domain outcomes use State, Commands, and later Port Events. A detected condition requiring fail-stop uses Fatal.

### 5.3 Internal Architecture

The handler may delegate to arbitrary application-defined functions and abstractions. Kavod assigns no meaning beyond ordinary Rust program order.

Application-created queues or dispatch systems receive no Kavod scheduling, causality, bounds, or audit guarantee. Work intended for a future turn crosses a Port boundary and later returns as an accepted Event.

## 6. Port Contracts, Slots, And App Events

### 6.1 Contracts And Slots

A Port Contract associates one typed Event protocol with one typed Command protocol. It declares a data relationship, not runtime behavior.

An Application declares a finite ordered set of logical Port Slots. Each Slot uses one Port Contract. Several Slots may use the same Contract while retaining distinct:

- Logical identity.
- Event source authority.
- Command destination authority.
- Staging queue and capacity.
- Position in the input-source order.
- Environment binding.
- Audit identity.

Slot identity comes from the frozen Application shape and Kavod-issued staging authority. A candidate never self-asserts its authoritative source.

### 6.2 One Closed App Event

Every accepted Port Event is transformed through its Slot's frozen Application injection into one closed App Event value.

The semantic requirements are:

- A Port stages only its Contract Event type.
- The owning Slot remains the authoritative source.
- The resulting App Event is immutable during delivery.
- Identical payload types from different Slots remain distinguishable through source, App Event representation, or both.
- Audit evidence records authoritative source independently of payload interpretation.
- Conversion is total and deterministic under the Application contract.
- Conversion failure or panic is Fatal and invokes no handler.

Exact Rust representation is TBD.

### 6.3 Frozen Declarative Shape

Application construction independently defines:

- Initial State.
- Closed App Event protocol.
- One handler and one Fatal Reason protocol.
- Built-in Engine Event injections.
- Ordered Port Slots and their Contracts.
- Port Event to App Event injections.
- Audit encoders and bounds.
- Deterministic configuration.

Every declared Slot must have exactly one compatible Environment binding before `run()`. Exact construction and binding mechanics are TBD.

## 7. Allocation And Bounds

Before `RunStarted`, Kavod allocates and validates every Core-owned resource needed for Ready, ordinary turns, fatal handling, and terminal reporting.

After `RunStarted`, Kavod Core must not invoke heap allocation or grow existing storage. Exact storage structures are TBD.

Finite bounds include at least:

- Event entries per Port Slot queue.
- Commands per turn.
- Encoded App Event, Command, Fatal Reason, and State-audit bytes.
- Audit bytes per record, turn, and run.
- Audit working storage and terminal reserve.
- Primary and secondary failure evidence.
- Every Kavod identifier domain.
- Any additional Kavod-owned Environment storage once that design is settled.

All capacity arithmetic is checked. Runtime exhaustion is Fatal before the requiring operation partially commits.

The first fatal failure is retained as primary. Later failures use one bounded prefix plus an explicit omitted-additional-failures indicator.

This allocation guarantee applies only to Kavod-owned Core code. Application code, payloads, Ports, models, encoders, and custom AuditWriters remain outside it.

Kavod does not claim a total bound on transitive memory owned by arbitrary State or payload object graphs.

## 8. Startup And Ready

Structural validation occurs before `run()` and may return a build error. Once `run()` begins, every unsuccessful return follows the common Fatal outcome path.

The semantic startup order is:

```text
allocate and validate all Kavod-owned run storage
-> establish the required static Port boundaries
-> encode initial State into bounded audit storage
-> append RunStarted and InitialStateEncoded
-> synchronize the complete startup prefix
-> accept Ready as Event index 0
-> process Ready through the ordinary turn protocol
-> permit ordinary Port Event acceptance
```

Exact Environment preparation and release mechanics are TBD.

Ready means only that the frozen Application, Core storage, audit boundary, and required static Port boundaries can begin ordinary execution. It does not mean connected, authenticated, subscribed, reconciled, armed, or safe to trade.

Ready is structurally first but otherwise follows ordinary Event, Command, State-sync, audit, handoff, and Fatal semantics. Ready may produce Commands.

A Ready-caused response may become a later candidate but cannot recursively enter the handler or precede Ready completion.

Failure after `run()` begins but before startup synchronization is Fatal during Starting. Fatal evidence is attempted when possible, but `RunStarted` is not fabricated.

## 9. Event Staging, Selection, And Acceptance

### 9.1 Per-Slot Staging

Each Port Slot has one bounded FIFO for immutable Event candidates.

Successful staging establishes source Slot and preserves successful FIFO staging order within that Slot. It does not assign Event index or logical time and does not promise later acceptance.

Staging must serialize with run-gate closure. Staging admitted before closure may complete, after which its candidate is abandoned unless already accepted. Closure that wins first rejects staging under the existing Fatal cause and performs no queue mutation.

Kavod never silently overwrites, rewrites, reprioritizes, coalesces, or replaces a successfully staged candidate.

A Port may perform domain-aware batching or coalescing before offering an Event. Offering to a full Kavod queue is Fatal. Exact staging mechanics are TBD.

### 9.2 Selection

One Engine sequencer assigns the accepted total order. The exact policy for selecting among nonempty Port Slot queues and pending Engine Events is TBD.

The policy must be frozen before `RunStarted`, recorded in run provenance, and precisely testable once selected.

Live producer races before acceptance are not deterministic. The resulting accepted sequence is authoritative.

### 9.3 Acceptance

For one selected candidate, the Engine:

1. Identifies one candidate without establishing acceptance.
2. Obtains acceptance admission from the run gate.
3. Establishes authoritative Event source and takes the candidate from its queue.
4. Constructs the immutable App Event.
5. Validates identifier, encoding, audit, and capacity bounds.
6. Freezes logical acceptance time and assigns the next Event index.
7. Appends the complete Event envelope to the AuditLog.
8. Synchronizes the acceptance boundary.
9. Treats the Event as accepted only after observing synchronization success.
10. Requests separate handler admission.

No handler receives an Event before successful acceptance synchronization.

Conversion, encoding, capacity, append, or synchronization failure is Fatal and invokes no handler. The selected candidate is not retried.

If closure denies acceptance admission, the candidate remains staged until terminal abandonment.

If acceptance synchronization succeeds but closure prevents handler admission, the Event remains accepted but unprocessed.

The handler may observe Event index, logical time, and source. Event index, not time, establishes accepted order. Domain time remains ordinary payload data.

The exact production of live and simulated logical time is TBD.

## 10. Turn And Command Protocol

### 10.1 Handler Execution

After acceptance, the Engine obtains handler admission and invokes `on_event` once.

The handler mutates State in place and stages Commands into bounded turn-local storage. Each successful Command staging call establishes destination Slot, payload type, immutable payload, and the next checked production ordinal.

Command staging performs no Port IO. A staging or encoding failure is Fatal and is not exposed as a recoverable application result.

Normal handler return certifies that current State and staged outputs form the Application's complete transition. Kavod cannot validate domain coherence.

### 10.2 Output-Free Completion

For Continue or legal Stop with no Commands:

```text
handler returns normally
-> encode final State if requested
-> append optional StateEncoded and TurnCompleted
-> synchronize completion
-> either admit the next Event or begin graceful application closure
```

### 10.3 Command-Producing Completion

For Continue with Commands:

```text
handler returns normally
-> encode final State if requested
-> validate all remaining turn and terminal capacity
-> append complete ordered Command intent
-> synchronize TurnPrepared
-> attempt each Command handoff once in production order
-> append handoff evidence
-> append optional StateEncoded and TurnCompleted
-> synchronize completion
-> admit the next Event
```

`TurnPrepared` contains the root Event index and each Command's destination Slot, ordinal, protocol identity, and complete application-defined audit encoding.

Preparation failure causes no current-turn handoff.

### 10.4 One-Pass Handoff

Each Command handoff requires fresh run-gate admission.

If admission is denied, no transfer begins and the existing Fatal cause remains authoritative.

An admitted handoff must produce truthful local certainty:

- Definitely handed off.
- Definitely not handed off.
- Indeterminate.

The exact local handoff boundary is TBD and must be defined by the eventual Environment and binding design.

The first admitted handoff that is not definitely successful is Fatal. Earlier successful handoffs remain real, and every later Command is unattempted.

No failed or ambiguous Command is retried, resent, or automatically returned as an App Event.

Local handoff does not prove worker processing, network transmission, remote receipt, external execution, exactly-once effect, persistence across process failure, or cross-Port external ordering.

Externally consequential Commands must contain application-owned business identity or idempotency information sufficient for reconciliation. Kavod identity does not satisfy this requirement.

### 10.5 Incomplete Turns

Panic, application Fatal, contract violation, concurrent Fatal closure, bound failure, preparation failure, handoff failure, or completion-sync failure makes the current turn incomplete.

An incomplete turn:

- Produces no authoritative `TurnCompleted` frontier.
- Exposes no current State to a later Event.
- Suppresses every not-yet-handed-off current-turn Command.
- Establishes no authoritative State checkpoint.
- Permanently poisons State.
- Enters the common Fatal path.

If `StateEncoded` was appended before completion synchronization failed, it may remain as trailing non-authoritative evidence. Its presence does not establish a completed checkpoint.

There is no rollback.

## 11. State Audit Encoding

### 11.1 Application-Defined Bytes

The Application supplies an audit encoder for State. Exact interface and format are TBD.

The encoder may produce JSON, Arrow IPC, custom binary data, a digest, a projection, or another bounded representation.

Kavod guarantees only that:

- The encoder is invoked at the defined boundary with immutable State access.
- Its output is captured within a configured maximum.
- If the Engine observes successful synchronization of the corresponding audit boundary, that boundary includes the exact emitted bytes.

Kavod does not claim that the bytes are complete, reversible, canonical, truthful, stable across builds, or sufficient to reconstruct State.

Because encoding affects mandatory execution, the Application contract requires deterministic encoding behavior. Kavod cannot enforce arbitrary encoder purity.

### 11.2 Initial State

Initial State encoding is mandatory and synchronizes with startup provenance before Ready acceptance.

Encoding failure, panic, overflow, append failure, or synchronization failure is Fatal during Starting.

### 11.3 Requested State

`ctx.sync_state()` latches one idempotent turn-local request. It returns no persistence result and performs no immediate encoding or IO.

The request captures final State after normal handler return, including mutations after the call.

For a Command-producing turn, Kavod encodes final State before handoff into reserved storage. This avoids a preventable encoding failure after Commands cross their boundaries. The frozen bytes are appended only after every handoff succeeds.

For an output-free turn, encoding occurs after normal handler return.

`StateEncoded` is appended with `TurnCompleted`. It becomes authoritative completed-turn evidence only if the Engine observes completion synchronization success. A trailing `StateEncoded` record after failed or unobserved synchronization is non-authoritative evidence from an incomplete turn.

Application Fatal, panic, contract violation, concurrent Fatal closure, and failed handoff produce no State checkpoint. Bytes encoded before interruption are discarded unless they had already been appended as part of a later failed completion attempt.

One State encoding must fit one configured preallocated bound. Chunking and delta encoding are not defined here.

State evidence grants no restoration or continuation authority.

## 12. AuditLog And AuditWriter

### 12.1 Authority

Kavod owns:

- Semantic audit evidence.
- Record order and sequence.
- Framing and complete-record detection.
- Integrity protection.
- Capacity and terminal reserve.
- Synchronization barriers.
- Valid-prefix interpretation.

The AuditWriter persists already framed bytes under one immutable declared synchronization contract. Its exact interface is TBD.

Memory, file, and acknowledged remote persistence may provide different contracts. Application code cannot observe which is selected.

A custom AuditWriter is part of the trusted computing base. Kavod cannot prove that arbitrary writer code does not reorder, discard, corrupt, or lie about persistence.

### 12.2 Required Evidence

The AuditLog must represent at least:

- Run start and initial State encoding.
- Every accepted Event envelope.
- Complete prepared Command intent.
- Every attempted handoff's available certainty.
- Requested State encoding for completed checkpoints.
- Turn completion.
- Graceful termination or Fatal failure and available frontiers.

Exact record names, fields, framing, and physical grouping are TBD.

### 12.3 Synchronization Boundaries

For a Command-producing turn:

| Boundary | Meaning after Engine-observed synchronization |
|---|---|
| Input acceptance | Handler admission may be requested |
| Turn preparation | Complete Command intent exists; handoff may be requested |
| Turn completion | Every handoff succeeded, optional State evidence is included, and the next Event may be admitted |

An output-free turn uses input acceptance and turn completion.

No optimization may weaken these admission barriers or truth claims.

Appending or writing is not synchronization. Synchronization exists only when the Engine observes successful completion under the configured AuditWriter contract.

A complete trailing record may persist even if the Engine never observed synchronization success. Record presence alone therefore does not always prove an Engine-observed frontier. Later causally dependent ordinary evidence may prove the prerequisite observation.

### 12.4 Audit Failure

Audit encoding, capacity, append, integrity, or synchronization failure after `run()` begins is Fatal.

Failure before turn preparation causes no current-turn handoff. Failure synchronizing turn completion may occur after successful handoffs; those handoffs are not undone and State is poisoned.

If the AuditWriter itself fails, Fatal terminal evidence is best effort. A returned Fatal outcome reports compromised audit status from bounded in-memory evidence where available. No outcome is guaranteed if the writer operation never returns.

## 13. Closure And Terminal Outcomes

### 13.1 Run Gate

The run gate transitions from Open to Closed and never reopens.

> Closure prevents the next gated operation from beginning. One operation admitted before closure may reach its defined boundary. Its result cannot authorize another operation without fresh admission.

Gated operations include staging insertion, acceptance synchronization, handler invocation, requested State encoding, turn-preparation synchronization, each Command handoff, turn-completion synchronization, and ordinary activity release after Ready.

Exact concurrency mechanics are TBD.

### 13.2 Graceful Completion

There are two graceful triggers:

- Application Stop after an output-free turn completes.
- Simulation completion selected between completed turns under the configured policy, whose remaining semantics are TBD.

A graceful trigger returns a graceful outcome only if required terminal audit and cleanup succeed. Otherwise the run returns Fatal with the graceful trigger retained as context.

After a graceful trigger closes the gate, cleanup runs before final graceful evidence is appended and synchronized. Only Engine-observed successful final synchronization authorizes a Graceful outcome.

Cleanup failure establishes Fatal before final graceful evidence is created. If final graceful audit fails, AuditFailure establishes Fatal and no Graceful outcome is returned.

### 13.3 Host Authority

The host has two controls:

- Cooperative shutdown request, delivered as an ordinary Engine Event through the accepted Event pipeline.
- Authoritative host stop, which is always Fatal and immediately closes the run gate.

Authoritative host stop invokes no application business policy. It may interrupt an active or partially handed-off turn. State is poisoned, not-yet-handed-off Commands are suppressed, available handoff evidence is retained, and the common Fatal path follows.

The exact host API and repeated cooperative-request policy are TBD.

### 13.4 Common Fatal Path

The first established Fatal failure is primary. Later failures fill a bounded prefix and set an omitted-additional-failures indicator when necessary.

Fatal causes include:

- Application Fatal Reason.
- Authoritative host stop.
- Application panic or contract violation.
- Internal Kavod invariant violation.
- Port or Environment technical failure.
- Queue overflow.
- Command staging, preparation, or handoff failure.
- Audit encoding or State encoding failure.
- Bound or identifier exhaustion.
- Audit failure.
- Cleanup failure after a graceful trigger.
- Any failure that makes ordering, ownership, or boundary certainty untrustworthy.

After Fatal establishment:

- No new gated operation begins.
- An admitted operation may reach only its defined boundary.
- No application State from the failed run is ever reused.
- Uncommitted Commands and State-sync requests are suppressed.
- No completion evidence is fabricated.
- Fatal audit and cleanup are attempted.
- No replacement Engine resumes the run.

Exact cleanup mechanics are TBD. Cleanup begins no application work and hands off no new application Command.

When a Fatal cause is known before cleanup and the AuditLog remains usable, the Engine attempts to append it before cleanup begins. If cleanup establishes the first Fatal cause, the Engine records it when observed and does not restart cleanup. After the cleanup attempt returns, the Engine attempts one final `RunFailed` snapshot containing the primary cause, cleanup result, known frontiers, audit status, and bounded secondary evidence. Fatal terminal synchronization is best effort because the AuditWriter or process may itself be failing.

If application, Port, audit, or cleanup code never returns, Kavod cannot promise a terminal outcome. External process destruction provides no cleanup or final-audit guarantee.

### 13.5 Outcome Classes

Once `run()` begins, there are only:

- Graceful, caused by Application Stop or Simulation Completion.
- Fatal, carrying the primary cause, phase, known frontiers, audit status, cleanup status, and bounded secondary evidence.

Build errors before `run()` are not Engine outcomes.

Missing handoff evidence is unknown. It is never silently classified as not handed off.

## 14. Environment Boundary

The Environment design is intentionally TBD.

Only these shared semantic requirements are settled:

- The same frozen Application and handler run in live and simulation.
- The same Port Contracts and Port Slots are used.
- Application code cannot observe Environment mode.
- Events and Commands cross only declared Slots.
- Environment activity cannot recursively invoke the handler.
- Environment technical failure is Fatal.
- Environment cleanup participates in the common terminal path.

Live and simulation share Application semantics, not physical runtime behavior. No additional Environment API, topology, scheduling, model, time, handoff, startup, or cleanup guarantee should be inferred from this document.

## 15. Determinism Boundary

Kavod's deterministic claim is:

> Given the same executable build, frozen Application shape, initial State, deterministic configuration, accepted Event-envelope sequence with identical Event indices, sources, logical times, and App Events, application-provided audit encoding behavior, and the same technical interruption trace where relevant, the Engine invokes the handler in the same order and produces the same State transitions, outcomes, ordered Commands, requested State encodings, and completed-turn frontier.

The accepted Event sequence is an input to this claim. Kavod does not claim that nominally identical live conditions produce the same sequence.

Application and application-provided encoding behavior must not depend on wall-clock reads outside logical time, unrecorded entropy, IO, environment variables, task races, process-global mutable State, Port implementation State, pointer identity, unstable iteration, Environment mode, or AuditWriter mode.

Kavod does not guarantee cross-platform numeric equivalence, floating-point equivalence, or cross-build compatibility unless separately constrained and tested.

State encoding strengthens audit comparison but not State authority. Kavod guarantees invocation timing and retained bytes, not that those bytes completely represent behavior-affecting State.

## 16. Identity Discipline

The semantic identities are:

- Run identity.
- Checked monotonic Event index.
- Stable logical Port Slot identity.
- Checked turn-local Command ordinal.
- Checked audit-record sequence.
- Application-owned business identities inside domain payloads.

Additional Environment-private identities are TBD.

Kavod identifiers never wrap, silently saturate, or reuse within their scope. Runtime exhaustion is Fatal before the requiring operation commits.

Kavod technical identity never replaces application business identity.

## 17. Required Conformance Cases

The implementation must test at least:

1. Identical deterministic inputs produce identical handler outcomes, Commands, requested State bytes, and completed frontiers.
2. Ready is Event index zero, may produce Commands, and completes before any Port Event acceptance.
3. Input-acceptance synchronization failure invokes no handler.
4. An accepted Event may remain unprocessed if Fatal closure wins before handler admission.
5. Stop with any current-turn Command is Fatal and hands off none of those Commands.
6. Application Fatal and panic suppress current-turn Commands and State evidence.
7. No Command crosses before turn-preparation synchronization.
8. Commands receive at most one handoff attempt in production order.
9. The first failed or indeterminate admitted handoff suppresses every later attempt.
10. Turn-completion synchronization failure after successful handoffs is Fatal and does not undo them.
11. Repeated `ctx.sync_state()` calls encode final State at most once.
12. State evidence becomes an authoritative checkpoint only with Engine-observed successful turn completion.
13. State encoding failure occurs before current-turn handoff and is Fatal.
14. Queue overflow is Fatal and silently overwrites no candidate.
15. Authoritative host stop is Fatal at every run phase and preserves available partial-turn evidence.
16. Closure racing every gated operation admits no later operation.
17. The first Fatal cause remains primary and excess secondary failures are explicitly omitted.
18. Fatal and cleanup paths require no Kavod Core allocation after `RunStarted`.
19. Every fixed-capacity Core resource fails without overwrite or growth one beyond its bound.
20. Restart after failure creates a new run and never reuses poisoned State or automatically resends Commands.

## 18. Open Design Work

The following remain unsettled and must not be inferred:

- Exact Rust APIs and type relationships.
- Construction, derive, macro, registry, and manifest design.
- Application audit-encoding interfaces and schema identities.
- AuditWriter interface, binary framing, and storage layout.
- Fixed-capacity storage implementation.
- Environment interfaces and runtime architecture.
- Selection among nonempty Port queues and pending Engine Events.
- Logical-time production.
- Port handoff linearization for each eventual binding kind.
- Simulation scheduling, equal-time order, and completion policy.
- Host API and cooperative-request policy.
- Cleanup, cancellation, joining, and process-boundary mechanics.
- Optional logs, metrics, and tracing.

These topics should be added only after their semantics are understood well enough to state concrete invariants and conformance cases.
