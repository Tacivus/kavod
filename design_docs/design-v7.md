# Kavod Core Design v7

> **Status:** Semantic draft for review
> **Scope:** The deterministic Core shared by live and simulated execution
> **Priority:** The smallest robust design that can be implemented, audited, and tested with confidence

---

## 1. Engineering Framework And Thesis

Kavod is a deterministic application Core. It accepts one ordered Event, invokes one synchronous application transition, records the resulting State and Command evidence, inserts Commands into typed Port inboxes, and only then proceeds to the next Event.

The same frozen Application runs in live and simulation. The Environment changes how Events arrive and how Ports process Commands; it does not change application code or Core turn semantics.

Kavod's engineering approach is informed by NASA's Power of Ten, TigerBeetle's Tiger Style, and SQLite's defensive testing culture. These references are influences, not claims of compliance. The enforceable Kavod rules are:

| Principle | Kavod rule |
|---|---|
| Correctness before convenience | No feature is added without semantics that can be enforced and tested |
| Explicit execution | One Event, one handler invocation, and one turn at a time |
| Finite Core resources | Every Kavod-owned non-stack object has a configured maximum |
| No steady-state Core allocation | Kavod allocates and validates all Core storage before `RunStarted` |
| Bounded local work | Core code uses no recursion or unbounded per-turn loop |
| Checked arithmetic | Counts, lengths, capacities, and identities never wrap or silently saturate |
| Explicit failure | Representable runtime failures establish Fatal |
| Assertions mean bugs | Panic is uncaught and outside Kavod semantics |
| Defensive boundaries | Validate Core capacity and encoding before each irreversible Core action whenever knowable |
| Evidence-driven engineering | Every bound and failure boundary must support direct and fault-injection testing |

The complete Core flow is:

```text
freeze Application and Port Slots
-> allocate bounded Core storage
-> encode initial State and sync RunStarted
-> process Ready
-> accept and sync one Event
-> invoke on_event
-> sync complete Command intent when Commands exist
-> insert Commands into Port inboxes
-> sync TurnCompleted
-> process the next Event, return Stopped, or return Fatal
```

The AuditLog is evidence, not State, recovery authority, or a Command outbox. Kavod does not roll back State, retry Commands, or guarantee exactly-once external effects.

Rust syntax in this document is illustrative. Concrete APIs, traits, derives, macros, storage types, and Environment mechanics remain undecided unless the semantics below require them.

## 2. Execution Model And Determinism

One Engine owns one run and its Application State. Only the Engine's Core may pass that State to application code, and at most one `on_event` invocation is active.

One accepted Event creates one turn. A turn runs synchronously to a synchronized completion boundary or establishes Fatal. Internal application helper calls are ordinary Rust program flow; Kavod does not register, schedule, or audit components, reducers, callbacks, or internal messages.

An accepted Event envelope contains:

- Checked monotonic Event index.
- Authoritative source: the Engine or one Port Slot.
- Frozen logical acceptance time.
- Immutable App Event value.

Event index is the sole accepted-Event order. Logical time is deterministic input visible to the handler; domain time remains ordinary payload data.

Kavod's deterministic claim is:

> For the same executable build, frozen Application, initial State, deterministic configuration, accepted Event envelopes, and application-provided audit encoding behavior, every failure-free completed prefix produces the same handler calls, State transitions, Outcomes, ordered Commands, requested State bytes, and completed-turn frontier.

| Rule | Consequence |
|---|---|
| The Engine owns State | Application transitions cannot race each other |
| One handler runs at a time | Handler program order is semantic order |
| Event index orders Events | Equal or conflicting timestamps never reorder Events |
| A turn must complete before the next Event | State and Command decisions from different turns never overlap |
| Commands are deferred until handler return | Application code cannot perform Port IO |
| Environment mode is hidden | Live and simulation execute the same application decisions |

The accepted live Event sequence is an input to determinism. Kavod does not claim that nominally identical live conditions produce the same sequence.

Application code and application-provided encoders must not make behavior depend on hidden wall-clock reads, unrecorded entropy, IO, environment variables, process-global mutable State, concurrent task ordering, pointer identity, unstable collection iteration, Environment mode, or AuditWriter mode.

Cross-build, cross-platform, and floating-point equivalence require separate application constraints and testing.

## 3. Frozen Application

An Application supplies:

- One initial concrete AppState.
- One closed AppEvent protocol.
- One synchronous `on_event` handler.
- One application Fatal Reason protocol.
- One ordered static set of Port Slots.
- Deterministic Event, Command, State, and Fatal Reason audit encoders. Event and Command encodings must represent their complete logical values; State encoding has application-defined evidentiary meaning.
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

The handler may mutate complete State, inspect the current Event envelope, stage typed Commands, request final-State audit encoding, and return one Outcome.

The handler receives no Engine, Environment, AuditWriter, Port implementation, external IO, wall-clock, entropy, or concurrency authority.

| Outcome | Meaning |
|---|---|
| Continue | Complete this turn and admit a later Event |
| Stop | Complete this output-free turn and return a successful Engine exit |
| Fatal(reason) | Stop normal Engine execution and return a Fatal Engine exit |

Stop is legal only when the current turn staged no Commands. Stop with any current-turn Command establishes Fatal.

The handler has no generic recoverable error result. Expected domain outcomes use State, Commands, and later Port Events. A detected condition under which continuing is unsafe uses the Application Fatal Reason.

If a Context operation detects a Core failure, the first Fatal cause is retained and no staged output from that handler is processed. Kavod cannot preempt the handler; Fatal processing begins when control returns.

`panic!()` is not an Outcome. Kavod does not catch or translate panics. After panic, Kavod guarantees no Engine exit, final audit synchronization, or process termination. The embedding program and Rust panic mode determine what physically happens.

Application code may internally use functions, modules, state machines, reducers, components, or local queues. Those constructs receive no Kavod ordering, scheduling, bounds, or audit semantics. Work intended for a future turn must return through a Port Event.

The complete Application shape and capacity configuration are frozen before `RunStarted`.

## 4. Port Contracts, Slots, And Inboxes

A Port Contract associates one typed Event protocol with one typed Command protocol. It describes application data, not runtime implementation.

A Port Slot is one logical use of one Port Contract in an Application. Several Slots may use the same Contract while retaining separate identity, source authority, destination authority, capacity, inboxes, binding, and audit identity.

Each Slot has:

- One bounded FIFO Event inbox.
- One bounded FIFO Command inbox.

The Slot, not candidate-supplied metadata, establishes authoritative Event source and Command destination.

A Port may offer only its Contract Event type. Kavod applies the Slot's frozen deterministic injection into the closed AppEvent type before acceptance. Conversion failure establishes Fatal and invokes no handler.

| Inbox rule | Consequence |
|---|---|
| Capacity is fixed before `RunStarted` | Inbox insertion never grows Core storage |
| Insertion is all-or-none | A value is inserted exactly once or not inserted |
| Full insertion establishes Fatal | Kavod never silently drops accepted pressure |
| FIFO is preserved per Slot | Successful insertion order is stable |
| Kavod never overwrites or coalesces | Domain-aware batching must happen before an Event is offered |

Command handoff has one Core meaning:

> A Command is handed off when it is successfully inserted into its destination Slot's Command inbox.

An insertion either succeeds or establishes Fatal. Commands are attempted once in successful staging order. Earlier successful insertions remain real; after the first failed insertion, no later current-turn Command is attempted.

Successful insertion proves neither Port processing nor external effect. It does not prove network transmission, remote receipt, execution, persistence across process failure, or exactly-once behavior.

Externally consequential Commands require application-owned business identity or idempotency information sufficient for reconciliation. Kavod identities do not replace that requirement.

If the process ends before Command-insertion evidence synchronizes, an offline observer may not know whether insertion occurred. That is missing audit evidence, not a third runtime insertion result.

## 5. Bounded Core Storage

> Every Kavod-owned object requiring non-stack storage has a finite configured maximum allocated and validated before `RunStarted`.

This includes, at minimum:

- Port Slot Event and Command inboxes.
- Turn-local Command storage.
- Encoded Event, Command, State, and Fatal Reason storage.
- Pending AuditLog storage.
- Reserved Fatal and `FatalSyncFailed` storage.
- Core counters and identifier domains.

After `RunStarted`, Core code does not allocate heap storage or grow existing storage. Fatal handling and Engine exit use the same preallocated storage.

All capacity arithmetic is checked before allocation and use. Construction validates that worst-case turn records, inbox entries, framing, and terminal reserve are mutually compatible. Identifiers never wrap, silently saturate, or reuse a prior value within one run.

Exhaustion establishes Fatal before partial insertion of one inbox item or audit record, overwrite, or identifier assignment.

Terminal reserve includes bytes and audit-sequence values for Fatal and FatalSyncFailed. Fatal uses a fixed bounded fallback record if detailed cause encoding fails. FatalSyncFailed records a fixed Core failure classification; the concrete AuditWriter error is returned separately in EngineExit.

This guarantee applies only to Kavod-owned Core storage. AppState, transitive payload object graphs, application encoders, Ports, the Environment, and custom AuditWriters remain outside it.

Core code uses no recursion. Every Core-owned loop within a turn has a configured bound. A run continues until Stop, Fatal, or finite identifier exhaustion; application code, Ports, encoders, and AuditWriters may still block or fail to return.

## 6. Audit And State Evidence

### 6.1 AuditLog And AuditWriter

The AuditLog is one ordered queue of complete framed records retained by the Engine until synchronization succeeds.

Kavod owns record order, framing, sequence numbers, integrity protection, pending capacity, and synchronization triggers. A configured AuditWriter receives the queued framed bytes and implements one declared synchronization contract.

A successful synchronization:

```text
covers the complete pending AuditLog queue through the trigger record
-> advances the Engine-observed synchronized frontier
-> clears the pending queue
```

Appending a record is not synchronization. A writer may physically persist bytes even when synchronization returns failure. Failure means only that Kavod did not observe success.

A custom AuditWriter is trusted infrastructure. Application code cannot observe writer mode or synchronization status.

The AuditWriter contract must permit Kavod to synchronize a retained pending queue again after a failed synchronization without duplicating or reordering logical records. Exact storage mechanics remain an AuditWriter concern.

### 6.2 Records And Sync Triggers

The semantic record set is:

| Record | Contents | Triggers synchronization |
|---|---|---|
| RunStarted | Run identity, frozen provenance, and initial State bytes | Yes |
| EventAccepted | Complete Event envelope | Yes |
| CommandsPrepared | Complete ordered current-turn Command intent | Yes |
| CommandAccepted | One successful Command-inbox insertion | No |
| StateEncoded | Requested final-State bytes | No |
| TurnCompleted | Event index and Continue or Stop outcome | Yes |
| Fatal | First Fatal cause and available current-turn progress | Yes, final attempt |
| FatalSyncFailed | Error from failed final Fatal synchronization | No; returned in memory |

Exact binary encoding is undecided. Every record has a checked monotonic audit sequence and enough framing to recover record boundaries from one contiguous byte stream.

The exhaustive synchronization policy is:

| Trigger | Engine-observed success authorizes |
|---|---|
| RunStarted | Ready acceptance |
| EventAccepted | `on_event` invocation |
| CommandsPrepared | First current-turn Command insertion |
| TurnCompleted | Next Event or successful Stop exit |
| Fatal | Nothing; this is the one final synchronization attempt |

Everything appended since the previous successful synchronization is included in the next trigger's synchronization.

Authorization belongs to the specific synchronization attempt for its trigger. If that attempt fails, a later successful Fatal synchronization may preserve the bytes but never retroactively accepts the Event, authorizes Command insertion, completes the turn, or establishes a State checkpoint.

### 6.3 State Evidence

The Application supplies an audit encoder for State. Kavod does not define its format. It may be JSON, Arrow IPC, custom binary data, a digest, a projection, or another bounded representation.

Kavod guarantees only that the encoder is invoked at the defined boundary and that successful TurnCompleted synchronization includes the exact bytes appended as StateEncoded. If the turn becomes Fatal before StateEncoded is appended, pre-encoded bytes are discarded. Kavod does not claim retained State bytes are complete, reversible, canonical, truthful, stable across builds, or sufficient to reconstruct State.

Initial State encoding is mandatory and included in RunStarted.

`ctx.sync_state()` requests one final-State encoding for the current turn:

- It is idempotent within the turn.
- It performs no immediate encoding or IO.
- It captures State after normal handler return, including mutations after the call.
- Encoding occurs before any current-turn Command insertion.
- StateEncoded is appended only after every current-turn Command insertion succeeds.
- StateEncoded becomes authoritative checkpoint evidence only with successful TurnCompleted synchronization.

State encoding failure or bound exhaustion establishes Fatal before Command insertion.

State evidence grants no replay, restoration, or continuation authority.

### 6.4 Unsynchronized Fatal Evidence

If final Fatal synchronization fails, Kavod appends FatalSyncFailed locally and returns the entire pending AuditLog queue without another AuditWriter call.

The returned AuditBuffer is one bounded contiguous sequence of the exact complete framed bytes pending after the last Engine-observed successful synchronization. It contains:

```text
records already pending
-> Fatal
-> FatalSyncFailed
```

FatalSyncFailed was appended after the failed synchronization attempt and was not offered in another attempt.

The AuditBuffer is not the complete run journal and is not a retry instruction. Some or all records offered during the failed synchronization may already exist in the writer.

The concrete AuditBuffer type is undecided. Its backing storage is allocated before RunStarted and moved out of the consumed Engine without new Core allocation.

## 7. Startup And Ready

Engine construction validates the frozen Application, Port Slots, bindings, encoders, capacities, and all required Core allocations. Failure during construction is a construction error, not an EngineExit. Exact construction-error representation is undecided.

The startup sequence is:

```text
encode initial State
-> append RunStarted
-> synchronize the complete pending AuditLog
-> process Ready as Event index zero
-> permit Port Event acceptance
```

Ready is the only built-in Engine Event. It is structurally first but otherwise uses the ordinary Event and turn protocol. Ready may produce Commands.

No Port Event is accepted before the Ready turn completes. An Event caused by a Ready Command waits in its Slot inbox and cannot recursively invoke the handler.

Ready means that Kavod can begin execution. It does not mean connected, authenticated, subscribed, reconciled, armed, or safe to trade.

After construction succeeds, initial State encoding or RunStarted synchronization failure establishes Fatal. Ready is not invoked.

## 8. Event And Turn Processing

### 8.1 Event Staging And Acceptance

A Port stages an immutable typed Event into its Slot's Event inbox. Staging preserves per-Slot FIFO but does not assign Event index, logical time, or acceptance.

One selected Event follows this path:

```text
remove one Event from its Slot inbox
-> inject it into AppEvent
-> assign Event index and logical time
-> append EventAccepted
-> synchronize the complete pending AuditLog
-> invoke on_event once
```

Event index is checked and never reused. Source Slot comes from the inbox, not the payload.

Conversion, encoding, capacity, or EventAccepted synchronization failure establishes Fatal and invokes no handler. An accepted Event is never retried.

The policy for selecting among nonempty Slot inboxes and the production of logical time belong to the Environment design and remain undecided.

### 8.2 Canonical Turn

After EventAccepted synchronization, the Engine invokes `on_event` once.

Command staging during the handler writes immutable Commands and their complete bounded audit encodings into turn-local Core storage. It performs no Port insertion or IO. Each successfully staged Command receives the next checked turn-local ordinal. Encoding or staging failure establishes Fatal and no handler output is processed after return.

After normal handler return:

```text
inspect Outcome
-> if Fatal(reason): establish Fatal
-> if Stop with Commands: establish Fatal
-> encode final State if requested
-> reserve all remaining turn audit bytes and audit-sequence values
-> if Commands exist:
     append CommandsPrepared with complete ordered intent
     synchronize the complete pending AuditLog
     insert each Command into its destination inbox in ordinal order
     append CommandAccepted after each success
-> append StateEncoded if requested
-> append TurnCompleted with Continue or Stop
-> synchronize the complete pending AuditLog
-> Continue to the next Event or return Stopped with AppState
```

CommandsPrepared synchronization failure inserts no current-turn Command.

Command-inbox insertion failure establishes Fatal. Earlier insertions remain accepted and their CommandAccepted records remain pending for Fatal synchronization. Later Commands are not attempted.

Before the first Command insertion, Kavod has reserved every required CommandAccepted, optional StateEncoded, and TurnCompleted record. After insertion begins, those local appends have no expected capacity or identifier failure path.

TurnCompleted is appended only after every current-turn Command insertion succeeds. No later Event begins before the Engine observes TurnCompleted synchronization success.

TurnCompleted with Stop is the final successful-run record. No additional successful terminal record or synchronization is performed.

Stop only proves that the Application completed an output-free turn and requested exit. Processing of Commands accepted in earlier turns is an Application protocol obligation. Kavod does not inspect Command inbox emptiness or infer external quiescence.

| Failure point | Result |
|---|---|
| Before EventAccepted sync | Handler is not invoked |
| During handler through explicit Fatal or checked Core failure | Staged Commands are not inserted |
| Before CommandsPrepared sync | No current-turn Command is inserted |
| During Command insertion | Earlier insertions remain accepted; later Commands are skipped |
| During TurnCompleted sync | Inserted Commands remain accepted; successful turn completion is not established |

Every failure in this table establishes Fatal and follows Section 9.

## 9. Fatal And Engine Exit

### 9.1 Exit Value

The Engine caller receives one value that distinguishes successful Stop from Fatal and returns AppState in either case.

Conceptually:

```rust
enum EngineExit<S, C, E> {
    Stopped {
        state: S,
    },
    Fatal {
        cause: C,
        state: S,
        audit: FatalAudit<E>,
    },
}

enum FatalAudit<E> {
    Synced,
    Unsynchronized {
        records: AuditBuffer,
        sync_error: E,
    },
}
```

Exact Rust representation is undecided. The semantics are not:

- Stopped returns State after synchronized TurnCompleted with Stop.
- Fatal returns the first Fatal cause observed by Kavod and the State owned when execution stopped.
- The Fatal cause retains its concrete application or Core error value where available; its exact sum type is undecided.
- State is returned directly without a wrapper or validity classification.
- Synced means Kavod observed final Fatal synchronization success.
- Unsynchronized returns the pending framed AuditBuffer and final sync error.

### 9.2 Fatal Sequence

Once Fatal is observed, normal Engine execution ends. Kavod begins no subsequent Event acceptance, handler invocation, State encoding, Command preparation, Command insertion, or TurnCompleted operation.

Kavod cannot preempt an executing `on_event`. If Fatal is reported while application code runs, Fatal processing begins when control returns to Kavod. That handler's staged Commands and State-sync request are not processed.

The Fatal sequence is:

```text
retain the first Fatal cause
-> suppress Commands not already inserted into Port inboxes
-> append Fatal to the pending AuditLog
-> call AuditWriter synchronization exactly once
```

If synchronization succeeds:

```text
return EngineExit::Fatal {
    cause,
    state,
    audit: Synced,
}
```

If synchronization fails:

```text
append FatalSyncFailed to the pending AuditLog
-> make no further AuditWriter call
-> move out the pending AuditBuffer
-> return EngineExit::Fatal {
       cause,
       state,
       audit: Unsynchronized {
           records,
           sync_error,
       },
   }
```

Fatal and FatalSyncFailed byte capacity and audit-sequence values are reserved before RunStarted and cannot be consumed by ordinary records.

If an ordinary required synchronization failure establishes Fatal, Kavod retains that pending queue, appends Fatal, and uses the one final Fatal synchronization attempt described above.

An authoritative host interrupt observed by Kavod establishes Fatal(HostInterrupt). Graceful external shutdown requests and simulation-ending requests must arrive through a declared Port as ordinary Events; the Application decides whether to return Stop.

Process destruction or an operation that never returns may prevent Kavod from beginning or completing the Fatal sequence. In that case no EngineExit is guaranteed.

## 10. Environment Boundary

The Environment design is intentionally undecided.

The settled common contract is small:

- Live and simulation run the same frozen Application.
- Both use the same Port Contracts, Slots, Event inboxes, Command inboxes, and Core turn protocol.
- Application code cannot observe Environment mode.
- Environment activity cannot recursively invoke `on_event`.
- Environment failures reported to Kavod establish Fatal.

Input selection, logical-time production, Port processing, runtime topology, scheduling, and all other Environment mechanics require separate design work. No additional Environment guarantee is implied here.
