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
| Explicit failure | Representable runtime failures establish Fatal |
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

One accepted Event creates one turn. A turn runs synchronously to completion or establishes Fatal. Internal application helper calls are ordinary Rust program flow; Kavod does not register, schedule, or audit components, reducers, callbacks, or internal messages.

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

The Core is the sole producer for each Command inbox, and the bound Port is its sole consumer. The Core accounts for its complete current-turn batch; concurrent Port consumption can only increase available Command capacity.

The Slot, not candidate-supplied metadata, establishes authoritative Event source and Command destination.

A Port may offer only its Contract Event type. Kavod applies the Slot's frozen deterministic injection into the closed AppEvent type before acceptance. Conversion failure establishes Fatal and invokes no handler.

| Inbox rule | Consequence |
|---|---|
| Capacity is fixed before `RunStarted` | Inbox insertion never grows Core storage |
| Insertion is all-or-none | A value is inserted exactly once or not inserted |
| Full Event insertion establishes Fatal | Kavod never silently drops offered pressure |
| Insufficient Command capacity establishes Fatal during preflight | No current-turn Command is inserted for a predictably full destination |
| FIFO is preserved per Slot | Successful insertion order is stable |
| Kavod never overwrites or coalesces | Domain-aware batching must happen before an Event is offered |

Command handoff has one Core meaning:

> A Command is handed off when it is successfully inserted into its destination Slot's Command inbox.

Commands are attempted once in successful staging order. Successful capacity preflight makes a full result impossible; any representable Command-inbox insertion failure establishes Fatal. Earlier successful insertions remain real; after the first failed insertion, no later current-turn Command is attempted.

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
- Reserved Fatal and `FatalSyncFailed` storage.
- Core counters and identifier domains.

After `RunStarted`, Core code does not request heap allocation or grow Core-managed storage. Fatal handling and Engine exit use the same preallocated storage.

All capacity arithmetic is checked before allocation and use. Construction validates that worst-case turn records, inbox entries, framing, and terminal reserve are mutually compatible. Identifiers never wrap, silently saturate, or reuse a prior value within one run.

Exhaustion establishes Fatal before partial insertion of one inbox item or audit record, overwrite, or identifier assignment.

Terminal reserve includes bytes and audit-sequence values for Fatal and FatalSyncFailed. Fatal uses a fixed bounded fallback record if detailed cause encoding fails. FatalSyncFailed records a fixed Core failure classification; the concrete AuditWriter error is returned separately in EngineExit.

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

Submission never blocks. A full or disconnected queue establishes Fatal. Encoding, framing, pending-capacity, writer, or synchronization failure also establishes Fatal. Records are never silently dropped, overwritten, or sampled, and producers never retry failed submissions.

Normal execution does not wait for the AuditWorker. Synchronization does not authorize Core execution, and abrupt process destruction may lose an unsynchronized suffix.

Pending records are retained until Kavod observes synchronization success. The AuditWriter must permit the same prefix to be submitted again without duplicating or reordering logical records. A writer may physically persist bytes even when synchronization reports failure; failure means only that Kavod did not observe success.

A first-wins Fatal latch and terminal audit reserve exist outside ordinary queue capacity. Stop and Fatal close ordinary submission, use the reserve to submit Sync(TurnCompleted with Stop) or Sync(Fatal), wait for the worker to finish the accepted prefix and terminal synchronization, and join the worker before returning EngineExit.

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

RunStarted encoding or submission failure establishes Fatal and Ready is not invoked. Later AuditWorker failure follows the ordinary asynchronous Fatal path.

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

Conversion, encoding, capacity, or EventAccepted submission failure establishes Fatal and invokes no handler. An accepted Event is never retried.

The policy for selecting among nonempty Slot inboxes and the production of logical time belong to the Environment design and remain undecided.

### 8.2 Canonical Turn

After successful EventAccepted submission, the Engine invokes `on_event` once without waiting for audit synchronization.

Command staging during the handler writes immutable Commands and their complete bounded audit encodings into turn-local Core storage. It performs no Port insertion or IO. Each successfully staged Command receives the next checked turn-local ordinal. Encoding or staging failure establishes Fatal and no handler output is processed after return.

After normal handler return:

```text
inspect Outcome
-> if Fatal(reason): establish Fatal
-> if Stop with Commands: establish Fatal
-> if Commands exist:
     count required capacity per destination Slot
     verify every destination Command inbox has sufficient capacity
     submit Sync(CommandsPrepared) with complete ordered intent
     insert each Command into its destination inbox in ordinal order
     submit NoSync(CommandAccepted) after each success
-> if Continue: submit Sync(TurnCompleted with Continue) and admit the next Event
-> if Stop: begin terminal finalization with Sync(TurnCompleted with Stop)
```

Command-capacity preflight or CommandsPrepared submission failure establishes Fatal and inserts no current-turn Command.

Because the Core is the sole producer, successful capacity preflight guarantees that its complete batch fits; concurrent Port consumption can only create more space. A full result during subsequent insertion is an invariant violation and panics. Any other Command-inbox insertion or CommandAccepted submission failure establishes Fatal. Earlier insertions remain accepted; later Commands are not attempted.

TurnCompleted is submitted only after every current-turn Command insertion and CommandAccepted submission succeeds. Continue may admit a later Event after successful TurnCompleted submission without waiting for audit synchronization.

TurnCompleted with Stop is the final successful-run record. Stop then waits for AuditWorker terminal finalization.

Stop only proves that the Application completed an output-free turn and requested exit. Processing of Commands accepted in earlier turns is an Application protocol obligation. Kavod does not inspect Command inbox emptiness or infer external quiescence.

| Failure point | Result |
|---|---|
| Before EventAccepted submission | Handler is not invoked |
| During handler through explicit Fatal or checked Core failure | Staged Commands are not inserted |
| During Command-capacity preflight | No current-turn Command is inserted |
| Before CommandsPrepared submission | No current-turn Command is inserted |
| During Command insertion or CommandAccepted submission | Earlier insertions remain accepted; later Commands are skipped |
| During TurnCompleted submission | Inserted Commands remain accepted; turn completion is not established |

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

- Stopped returns State after the AuditWorker synchronizes TurnCompleted with Stop and terminates.
- Fatal returns the first Fatal cause observed by Kavod and the State owned when execution stopped.
- The Fatal cause retains its concrete application or Core error value where available; its exact sum type is undecided.
- State is returned directly without a wrapper or validity classification.
- Synced means Kavod observed final Fatal synchronization success.
- Unsynchronized returns the pending framed AuditBuffer and final sync error.

### 9.2 Fatal Sequence

Once Fatal is observed, normal Engine execution ends. Kavod begins no subsequent Event acceptance, handler invocation, Command preparation, Command insertion, or TurnCompleted operation.

Kavod cannot preempt an executing `on_event`. If Fatal is reported while application code runs, Fatal processing begins when control returns to Kavod. That handler's staged Commands are not processed.

The Fatal sequence is:

```text
retain the first Fatal cause
-> close ordinary audit submission
-> suppress Commands not already inserted into Port inboxes
-> signal reserved Sync(Fatal) to the AuditWorker
-> wait for the final synchronization attempt
-> join the AuditWorker
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

If ordinary writer synchronization failure establishes Fatal, the AuditWorker retains the pending prefix and uses the one final Fatal synchronization attempt described above.

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
