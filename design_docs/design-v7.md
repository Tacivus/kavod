# Kavod Core Design v7

> **Status:** Semantic draft for review
> **Scope:** The deterministic Core shared by live and simulated execution
> **Priority:** The smallest robust design that can be implemented, audited, and tested with confidence

---

## 1. Engineering Framework And Thesis

Kavod is a deterministic application Core. It accepts one ordered Event, invokes one synchronous application transition, records Event, Command, and turn evidence, inserts Commands into typed Port inboxes, and only then proceeds to the next Event.

The same frozen Application runs in live and simulation. The Environment changes how Events arrive and how Ports process Commands; it does not change application code or Core turn semantics.

Kavod's engineering approach is informed by NASA's Power of Ten, TigerBeetle's Tiger Style, and SQLite's defensive testing culture. These references are influences, not claims of compliance. The enforceable Kavod rules are:

| Principle | Kavod rule |
|---|---|
| Correctness before convenience | No feature is added without semantics that can be enforced and tested |
| Explicit execution | One Event, one handler invocation, and one turn at a time |
| Finite Core resources | Every Core-managed container, item count, byte buffer, and identifier domain has a configured maximum |
| No steady-state Core allocation | Kavod allocates and validates all Core-managed backing storage before `RunStarted` |
| Bounded local work | Core code uses no recursion or unbounded per-turn loop |
| Checked arithmetic | Counts, lengths, capacities, and identities never wrap or silently saturate |
| Explicit failure | Runtime failures that prevent safe continuation submit Fatal reports; only the Engine establishes Fatal |
| Assertions mean bugs | Panic is uncaught and outside Kavod semantics |
| Defensive boundaries | Validate Core capacity and encoding before each irreversible Core action whenever knowable |
| Evidence-driven engineering | Every bound and failure boundary must support direct and fault-injection testing |

The complete Core flow is:

```text
freeze Application and Port Slots
-> allocate bounded Core storage
-> submit Sync(RunStarted)
-> process Ready
-> accept one Event and submit Sync(EventAccepted)
-> invoke on_event
-> preflight Command-inbox capacity when Commands exist
-> submit Sync(CommandsPrepared) when Commands exist
-> insert Commands into Port inboxes
-> submit Sync(TurnCompleted)
-> process the next Event, return Stopped, or return Fatal
```

The AuditLog is evidence, not State, recovery authority, or a Command outbox. Kavod does not roll back State, retry Commands, or guarantee exactly-once external effects.

Rust syntax in this document is illustrative. Concrete APIs, traits, derives, macros, storage types, and Environment mechanics remain undecided unless the semantics below require them.

## 2. Execution Model And Determinism

One Engine owns one run and its Application State. Only the Engine's Core may pass that State to application code, and at most one `on_event` invocation is active.

One accepted Event creates one turn. A turn runs synchronously to completion or the Engine establishes Fatal at a semantic boundary. Internal application helper calls are ordinary Rust program flow; Kavod does not register, schedule, or audit components, reducers, callbacks, or internal messages.

An accepted Event envelope contains:

- Checked monotonic Event index.
- Authoritative source: the Engine or one Port Slot.
- Frozen logical acceptance time.
- Immutable App Event value.

Event index is the sole accepted-Event order. Logical time is deterministic input visible to the handler; domain time remains ordinary payload data.

Kavod's deterministic claim has two layers:

> For the same executable build, frozen Application, initial State, deterministic configuration, and accepted Event envelopes, application execution produces the same handler calls, State transitions, Outcomes, and ordered Command intent.

> Given those same inputs plus the same ordered Core-visible boundary observations and operation results, Core execution produces the same audit records, Command handoffs, completed-turn frontier, and Engine exit. Application-provided audit encoders are part of the deterministic Application behavior assumed by both claims.

| Rule | Consequence |
|---|---|
| The Engine owns State | Application transitions cannot race each other |
| One handler runs at a time | Handler program order is semantic order |
| Event index orders Events | Equal or conflicting timestamps never reorder Events |
| A turn must complete before the next Event | State and Command decisions from different turns never overlap |
| Commands are deferred until handler return | Application code cannot perform Port IO |
| Environment mode is hidden | Live and simulation execute the same application decisions |

The accepted live Event sequence and Core-visible Environment results are inputs to determinism. Kavod contains no hidden source of nondeterminism, but does not claim that nominally identical live conditions produce the same Event sequence, inbox availability, AuditWriter results, Environment failures, or host interrupts.

Application code and application-provided encoders must not make behavior depend on hidden wall-clock reads, unrecorded entropy, IO, environment variables, process-global mutable State, concurrent task ordering, pointer identity, unstable collection iteration, Environment mode, or AuditWriter mode.

Cross-build, cross-platform, and floating-point equivalence require separate application constraints and testing.

## 3. Frozen Application

An Application supplies:

- One initial concrete AppState.
- One closed AppEvent protocol.
- One synchronous `on_event` handler.
- One application Fatal Reason protocol.
- One ordered static set of Port Slots.
- Deterministic Event, Command, and Fatal Reason audit encoders. Event and Command encodings must represent their complete logical values.
- Finite Core capacity configuration.
- Immutable deterministic configuration.

Conceptually:

```rust
fn on_event(
    state: &mut AppState,
    event: &AppEvent,
    ctx: &mut Context,
) -> Outcome<AppFatalReason>;

enum Outcome<F> {
    Continue,
    Stop,
    Fatal(F),
}
```

The handler may mutate complete State, inspect the current Event envelope, stage typed Commands, and return one Outcome.

The handler receives no Engine, Environment, AuditWriter, Port implementation, external IO, wall-clock, entropy, or concurrency authority.

| Outcome | Meaning |
|---|---|
| Continue | Complete this turn and admit a later Event |
| Stop | Complete this turn and return a successful Engine exit |
| Fatal(reason) | Report an Application Fatal Reason and end normal execution at the post-handler boundary |

Continue and Stop process the current turn identically. Continue admits a later Event; Stop does not.

The handler has no generic recoverable error result. Expected domain outcomes use State, Commands, and later Port Events. A detected condition under which continuing is unsafe uses the Application Fatal Reason.

If a Context operation detects a Core failure, it reports Fatal. Kavod cannot preempt the handler; the Engine checks the Fatal inbox when control returns and processes no staged output if Fatal is established.

`panic!()` is not an Outcome. Kavod does not catch or translate panics. After panic, Kavod guarantees no Engine exit, final audit synchronization, or process termination. The embedding program and Rust panic mode determine what physically happens.

Application code may internally use functions, modules, state machines, reducers, components, or local queues. Those constructs receive no Kavod ordering, scheduling, bounds, or audit semantics. Work intended for a future turn must return through a Port Event.

The complete Application shape and capacity configuration are frozen before `RunStarted`.

## 4. Port Contracts, Slots, And Inboxes

A Port Contract associates one typed Event protocol with one typed Command protocol. It describes application data, not runtime implementation.

A Port Slot is one logical use of one Port Contract in an Application. Several Slots may use the same Contract while retaining separate identity, source authority, destination authority, capacity, inboxes, binding, and audit identity.

Each Slot has:

- One bounded FIFO Event inbox.
- One bounded FIFO Command inbox.

The Core is the sole producer for each Command inbox, and the bound Port is its sole consumer. The Core accounts for its complete current-turn batch; concurrent Port consumption can only increase available Command capacity.

The Slot, not candidate-supplied metadata, establishes authoritative Event source and Command destination.

A Port may offer only its Contract Event type. Kavod applies the Slot's frozen deterministic injection into the closed AppEvent type before acceptance. Conversion failure reports Fatal and invokes no handler.

| Inbox rule | Consequence |
|---|---|
| Capacity is fixed before `RunStarted` | Inbox insertion never grows Core storage |
| Insertion is all-or-none | A value is inserted exactly once or not inserted |
| Full Event insertion reports Fatal | Kavod never silently drops offered pressure |
| Insufficient Command capacity reports Fatal during preflight | No current-turn Command is inserted for a predictably full destination |
| FIFO is preserved per Slot | Successful insertion order is stable |
| Kavod never overwrites or coalesces | Domain-aware batching must happen before an Event is offered |

Command handoff has one Core meaning:

> A Command is handed off when it is successfully inserted into its destination Slot's Command inbox.

Commands are attempted once in successful staging order. A full result after successful capacity preflight is an invariant violation and panics. Any other Command-inbox insertion failure reports Fatal. Earlier successful insertions remain real; after the first failed insertion, no later current-turn Command is attempted.

Successful insertion proves neither Port processing nor external effect. It does not prove network transmission, remote receipt, execution, persistence across process failure, or exactly-once behavior.

Externally consequential Commands require application-owned business identity or idempotency information sufficient for reconciliation. Kavod identities do not replace that requirement.

If the process ends before Command-insertion evidence synchronizes, an offline observer may not know whether insertion occurred. That is missing audit evidence, not a third runtime insertion result.

## 5. Bounded Core Storage

> Every backing allocation and growable container managed by Core has a finite configured maximum allocated and validated before `RunStarted`.

This includes, at minimum:

- Port Slot Event and Command inboxes.
- Turn-local Command storage.
- Encoded Event, Command, and Fatal Reason storage.
- Audit ingress, pending encoded storage, and worker control storage.
- The bounded Fatal inbox and reserved terminal-record storage.
- Core counters and identifier domains.

After `RunStarted`, Core code does not request heap allocation or grow Core-managed storage. Fatal handling and Engine exit use the same preallocated storage.

All capacity arithmetic is checked before allocation and use. Construction validates that worst-case turn records, inbox entries, framing, and terminal reserve are mutually compatible. Identifiers never wrap, silently saturate, or reuse a prior value within one run.

Runtime exhaustion reports Fatal before partial insertion of one inbox item or audit record, overwrite, or identifier assignment.

Terminal reserve includes bytes and audit-sequence values for `TurnCompleted` with Stop, the maximum Fatal record, and `FatalSyncFailed` where the Stop-to-Fatal path requires them together. Fatal uses a fixed bounded fallback record if detailed report encoding fails. FatalSyncFailed records a fixed Core failure classification; the concrete AuditWriter error is returned separately in EngineExit.

Core stores a bounded number of typed values in preallocated inline storage sized for their concrete Rust types. AppState and Event, Command, and Fatal Reason values may contain pointers or handles to transitive allocations. Core may temporarily own or move those values, but their transitive allocations remain application- or Port-managed and outside this guarantee. Application code, encoders, Ports, the Environment, custom AuditWriters, and their allocation behavior also remain outside it.

Core code uses no recursion. Every Core-owned loop within a turn has a configured bound. A run continues until Stop, Fatal, or finite identifier exhaustion; application code, Ports, encoders, and AuditWriters may still block or fail to return.

## 6. AuditLog

The AuditLog is one globally ordered stream of mandatory evidence. It is not State, recovery authority, or a Command outbox.

Core and Port infrastructure submit complete bounded records to one bounded multi-producer queue. Application code has no AuditLog access. Successful queue insertion establishes audit order; concurrent submissions may appear in either order.

One AuditWorker processes accepted records in order and owns sequence assignment, framing, integrity protection, pending storage, and the AuditWriter. All AuditLog storage and terminal reserve are allocated before `RunStarted`.

Every record type has one fixed synchronization policy, conceptually:

```rust
enum AuditRecord<T> {
    Sync(T),
    NoSync(T),
}
```

`Sync(T)` appends `T` and then synchronizes the complete pending prefix through it. `NoSync(T)` appends `T` without synchronizing. The worker performs no automatic, periodic, or capacity-triggered synchronization.

For example, RunStarted, EventAccepted, CommandsPrepared, and TurnCompleted synchronize; CommandAccepted and Port observations do not. Fatal uses the reserved terminal path. New record types follow the same rules.

Submission never blocks. Before Fatal finalization, a producer that observes a full or disconnected queue submits an Audit Fatal report. Encoding, framing, pending-capacity, writer, or synchronization failure does the same. These reports follow the single rule in Section 9; they do not establish Fatal. Final Fatal synchronization failure follows Section 9's finalization rule instead of reporting another Fatal. Records are never silently dropped, overwritten, or sampled, and producers never retry failed submissions.

An AuditWorker that reports failure retains its pending state and terminal reserve for Engine-directed Fatal finalization.

Normal execution does not wait for the AuditWorker. Synchronization does not authorize Core execution, and abrupt process destruction may lose an unsynchronized suffix.

Pending records are retained until Kavod observes synchronization success. The AuditWriter must permit the same prefix to be submitted again without duplicating or reordering logical records. A writer may physically persist bytes even when synchronization reports failure; failure means only that Kavod did not observe success.

Fatal reports use the bounded inbox in Section 9 rather than the ordinary audit queue. Stop and Fatal close ordinary audit submission and use the terminal reserve for Sync(TurnCompleted with Stop) or the single Sync(Fatal) containing the frozen Fatal reports. The worker finishes the accepted prefix and terminal synchronization and joins before EngineExit.

If final Fatal synchronization fails, the worker appends FatalSyncFailed without another writer call and returns the bounded pending AuditBuffer and synchronization error. The buffer is not a complete journal or retry instruction; some or all of its records may already exist in the writer.

## 7. Startup And Ready

Engine construction validates the frozen Application, Port Slots, bindings, encoders, capacities, and all required Core allocations. Failure during construction is a construction error, not an EngineExit. Exact construction-error representation is undecided.

The startup sequence is:

```text
submit Sync(RunStarted)
-> process Ready as Event index zero
-> permit Port Event acceptance
```

Ready is the only built-in Engine Event. It is structurally first but otherwise uses the ordinary Event and turn protocol. Ready may produce Commands.

No Port Event is accepted before the Ready turn completes. An Event caused by a Ready Command waits in its Slot inbox and cannot recursively invoke the handler.

Ready means that Kavod can begin execution. It does not mean connected, authenticated, subscribed, reconciled, armed, or safe to trade.

RunStarted encoding or submission failure reports Fatal and Ready is not invoked. After RunStarted reaches its success or failure boundary, the Engine processes Fatal reports before admitting Ready. Later AuditWorker failure follows the same reporting path.

## 8. Event And Turn Processing

### 8.1 Event Staging And Acceptance

A Port stages an immutable typed Event into its Slot's Event inbox. Staging preserves per-Slot FIFO but does not assign Event index, logical time, or acceptance.

One selected Event follows this path:

```text
remove one Event from its Slot inbox
-> inject it into AppEvent
-> assign Event index and logical time
-> submit Sync(EventAccepted)
-> invoke on_event once
```

Event index is checked and never reused. Source Slot comes from the inbox, not the payload. Successful EventAccepted submission establishes acceptance.

Conversion, encoding, capacity, or EventAccepted submission failure reports Fatal and invokes no handler. An accepted Event is never retried.

Before removing the Event, the Engine processes Fatal reports at a semantic boundary. An empty boundary admits the complete Event action above. No boundary occurs between successful acceptance and handler invocation; a later report is processed after the handler returns.

The policy for selecting among nonempty Slot inboxes and the production of logical time belong to the Environment design and remain undecided.

### 8.2 Canonical Turn

After successful EventAccepted submission, the Engine invokes `on_event` once without waiting for audit synchronization.

Command staging during the handler writes immutable Commands and their complete bounded audit encodings into turn-local Core storage. It performs no Port insertion or IO. Each successfully staged Command receives the next checked turn-local ordinal. Encoding or staging failure reports Fatal and no handler output is processed after return.

After normal handler return:

```text
inspect Outcome
-> if Fatal(reason): report Fatal
-> check the Fatal inbox; if nonempty, establish Fatal and begin finalization
-> if Commands exist:
     count required capacity per destination Slot
     verify every destination Command inbox has sufficient capacity
     submit Sync(CommandsPrepared) with complete ordered intent
     insert each Command into its destination inbox in ordinal order
     submit NoSync(CommandAccepted) after each success
-> submit Sync(TurnCompleted with Continue or Stop)
-> if Continue: admit the next Event
-> if Stop: begin terminal finalization
```

This flow elides the Section 9 boundaries before Command preparation, each Command insertion and evidence action, TurnCompleted, the next Event, and Stop terminal synchronization.

Command-capacity preflight or CommandsPrepared submission failure reports Fatal and inserts no current-turn Command.

Because the Core is the sole producer, successful capacity preflight guarantees that its complete batch fits; concurrent Port consumption can only create more space. A full result during subsequent insertion is an invariant violation and panics. Any other Command-inbox insertion or CommandAccepted submission failure reports Fatal. A successfully inserted Command remains handed off even if its CommandAccepted submission fails; later Commands are not attempted.

TurnCompleted is submitted only after every current-turn Command insertion and CommandAccepted submission succeeds. Continue may admit a later Event after successful TurnCompleted submission without waiting for audit synchronization.

TurnCompleted with Stop is the terminal turn record for Stop. Stop then waits for AuditWorker terminal finalization.

Stop proves only that the Application completed its current turn and requested exit. Processing of Commands accepted in that or earlier turns is an Application protocol obligation. Kavod does not inspect Command inbox emptiness or infer external quiescence.

| Failure point | Result |
|---|---|
| Before EventAccepted submission | Handler is not invoked |
| During handler through explicit Fatal or checked Core failure | Staged Commands are not inserted |
| During Command-capacity preflight | No current-turn Command is inserted |
| Before CommandsPrepared submission | No current-turn Command is inserted |
| During one Command insertion or CommandAccepted submission | Successfully inserted Commands remain handed off; later Commands are skipped |
| During TurnCompleted submission | Inserted Commands remain accepted; turn completion is not established |

Every reportable failure in this table submits a report and follows Section 9. A synchronous Core failure ends the current action at its defined failure boundary; no later normal Core action begins before the next semantic boundary. Invariant failures panic under Section 1.

## 9. Fatal And Engine Exit

Fatal is the permanent failure disposition of a started run. There is one Fatal disposition, produced from one or more failure reports. EngineExit distinguishes Stopped from Fatal and returns State in either case; Fatal also returns the frozen reports and final audit status.

### 9.1 Reporting And Establishment

> Reporters latch; boundaries decide; admitted work finishes.

All Fatal sources, including Engine code, Context operations, Ports, the Environment, the AuditWorker, and the host, submit bounded reports to one run-scoped Fatal inbox. Reporting never establishes Fatal, starts finalization, or preempts an admitted Core action. Successful insertion establishes report order. The first report is the primary cause. Later reports are retained as secondary causes up to the configured capacity. A full-inbox report commits only `secondary_truncated` and follows the same closure ordering; it does not block or create another Fatal.

Only the Engine establishes Fatal. Before beginning each Core action, it processes the Fatal inbox at a semantic boundary. The boundary atomically does exactly one of two things:

- If a report has committed, close the inbox, freeze its contents, and establish Fatal. The next action does not begin.
- If no report has committed, admit exactly one following Core action. Admission of the final Stopped action atomically closes the inbox as Stopped.

A report committed after admission is processed at the next boundary. The admitted action reaches its defined success or failure boundary before another action is admitted. If Engine code detects a failure, it submits a report and begins no later normal action before that boundary. An idle Engine processes committed reports before waiting again or admitting an Event; how it is made runnable is an Environment mechanism with no Fatal authority.

These boundaries admit:

- RunStarted submission.
- Event acceptance and, if acceptance succeeds, one handler invocation through normal return, including processing its Outcome and submitting any Application Fatal report.
- Current-turn Command preparation through CommandsPrepared submission.
- One Command insertion and, if insertion succeeds, its CommandAccepted submission.
- TurnCompleted submission.
- Stop terminal synchronization before the final Stopped boundary.
- Final Stopped commitment and EngineExit.

After normal handler return, `Outcome::Fatal` submits its Application report before the post-handler boundary. Reports already committed retain their earlier order. No staged output is processed when that boundary establishes Fatal.

At a boundary that establishes Fatal, a concurrent report either commits before closure or observes that Fatal is already established. The primary cause determines the Engine outcome; secondary causes are diagnostic only.

Before returning Stopped, the Engine processes one final semantic boundary. A concurrent report either commits first and causes Fatal or loses to final Stopped admission and observes that the run has Stopped.

Once Fatal is established:

- No later normal Core action begins.
- Commands not yet admitted for handoff are discarded.
- Accepted Events, handed-off Commands, submitted audit records, and State mutations remain real.
- Kavod performs no rollback, retry, or continuation.

Fatal returns the State present when the Engine regains control. It may contain mutations from an incomplete turn and is diagnostic only.

### 9.2 Finalization

Fatal finalization is:

```text
close ordinary audit submission
-> finish the accepted audit prefix
-> append one reserved Sync(Fatal) containing the frozen reports
-> make one final synchronization attempt
-> join the AuditWorker
-> return EngineExit::Fatal
```

Reserved storage and fallback encoding make construction of the terminal Fatal record infallible. A violation of that guarantee is an invariant failure and panics.

Writer or synchronization failure during Fatal finalization follows the terminal failure path below and submits no report. If final synchronization succeeds, `audit` is `Synced`. If it fails, the worker appends `FatalSyncFailed` to the pending in-memory buffer without another writer call and returns:

```rust
FatalAudit::Unsynchronized {
    pending: AuditBuffer,
    sync_error,
}
```

The pending buffer contains records whose synchronization Kavod has not observed, including the terminal Fatal record and `FatalSyncFailed`. It is not necessarily the complete run AuditLog, and some or all of it may already exist in the writer.

Finalization failure never replaces the primary cause or begins another Fatal sequence. Failure while preparing Stop submits a Fatal report and is processed at the next semantic boundary. Stopped is established only after TurnCompleted with Stop synchronizes successfully and all reportable Stop work completes. The AuditWorker remains available for Fatal finalization until that boundary, then terminates and joins before EngineExit.

An authoritative host interrupt reports Fatal(HostInterrupt). Graceful external shutdown requests and simulation-ending requests must arrive through a declared Port as ordinary Events; the Application decides whether to return Stop.

Panic, process destruction, or an operation that never returns may prevent finalization and therefore provide no EngineExit.

## 10. Environment Boundary

The Environment design is intentionally undecided.

The settled common contract is small:

- Live and simulation run the same frozen Application.
- Both use the same Port Contracts, Slots, Event inboxes, Command inboxes, and Core turn protocol.
- Application code cannot observe Environment mode.
- Environment activity cannot recursively invoke `on_event`.
- Environment failures reported to Kavod enter the Fatal inbox.

Input selection, logical-time production, Port processing, runtime topology, scheduling, and all other Environment mechanics require separate design work. No additional Environment guarantee is implied here.
