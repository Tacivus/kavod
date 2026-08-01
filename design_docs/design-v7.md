# Kavod Core Design v7

> **Status:** Semantic draft for review
> **Scope:** The deterministic Core shared by live and simulated execution
> **Priority:** The smallest robust design that can be implemented, audited, and tested with confidence

---

## 1. Engineering Framework And Thesis

Kavod is a deterministic application Core. It accepts one ordered Event, invokes one synchronous application transition, submits mandatory Event, Command, turn, and Environment-lifecycle evidence, inserts Commands into typed Port inboxes, and only then proceeds to the next Event.

The same frozen Application runs in live and simulation. The Environment changes how Events arrive and how Ports process Commands; it does not change application code or Core turn semantics.

Kavod's engineering approach is informed by NASA's Power of Ten, TigerBeetle's Tiger Style, and SQLite's defensive testing culture. These references are influences, not claims of compliance. The enforceable Kavod rules are:

| Principle | Kavod rule |
|---|---|
| Correctness before convenience | No feature is added without semantics that can be enforced and tested |
| Explicit execution | One Event, one handler invocation, and one turn at a time |
| Finite Core resources | Every Core-managed container, item count, byte buffer, and identifier domain has a configured maximum |
| No steady-state Core allocation | Kavod allocates and validates all Core-managed backing storage before `RunStarted` |
| Bounded local work | Core code uses no recursion and no unbounded per-turn loops |
| Checked arithmetic | Counts, lengths, capacities, and identities never wrap or silently saturate |
| Explicit failure | Runtime failures that prevent safe continuation submit Fatal reports; only the Engine establishes Fatal |
| Assertions mean bugs | Panic is uncaught and outside Kavod semantics |
| Defensive boundaries | Validate Core capacity and encoding before each irreversible Core action whenever knowable |
| Evidence-driven engineering | Every bound and failure boundary must support direct and fault-injection testing |

The high-level normal Core flow is:

```text
freeze Application and Port Slots
-> allocate bounded Core storage
-> start Environment
-> submit Sync(RunStarted)
-> process Ready
-> poll Environment and accept one Event with Sync(EventAccepted)
-> invoke on_event
-> preflight Command-inbox capacity when Commands exist
-> submit Sync(CommandsPrepared) when Commands exist
-> insert Commands into Port inboxes
-> if Continue: submit Sync(TurnCompleted) and poll again
-> if Stop: perform audited aggregate Environment stop, close ordinary audit submission, perform reserved Sync(TurnCompleted with Stop), and process the final Stopped boundary
-> return Stopped or Fatal
```

The AuditLog is evidence, not State, recovery authority, or a Command outbox. Kavod does not roll back State, retry Commands, or guarantee exactly-once external effects.

Rust syntax in this document is illustrative. Concrete APIs, derives, macros, storage types, and unspecified Environment mechanics remain undecided unless the semantics below require them.

## 2. Execution Model And Determinism

One Engine owns one run and its Application State. Only the Engine's Core may pass that State to application code, and at most one `on_event` invocation is active.

One accepted Event creates one turn. Its application transition runs synchronously. Absent an intervening failure, Continue completes the turn synchronously; Stop additionally performs one aggregate Environment stop before terminal turn completion. Internal application helper calls are ordinary Rust program flow; Kavod does not register, schedule, or audit components, reducers, callbacks, or internal messages.

An accepted Event envelope contains:

- Checked monotonic Event index.
- Authoritative source: the Engine or one Port Slot.
- Frozen logical acceptance time.
- Immutable App Event value.

Event index is the sole accepted-Event order. Logical time is deterministic input visible to the handler; domain time remains ordinary payload data.

The Environment owns one opaque nondecreasing logical clock. The Engine reads, validates, and freezes it only for Ready and Port Event acceptance. Equal logical times are valid; Event index remains the sole order.

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
| Stop | Request graceful Environment stop and a successful Engine exit after normal current-turn output processing |
| Fatal(reason) | Report an Application Fatal Reason and end normal execution at the post-handler boundary |

Continue and Stop process State mutation and ordinary current-turn Command output identically. Continue then completes the turn and may admit a later Event. Stop admits no later Event and completes only through the aggregate Environment stop protocol; Environment, audit, or terminal synchronization failure instead produces Fatal.

The handler has no generic recoverable error result. Expected domain outcomes use State, Commands, and later Port Events. A detected condition under which continuing is unsafe uses the Application Fatal Reason.

If a Context operation detects a Core failure, it reports Fatal. Kavod cannot preempt the handler; the Engine checks the Fatal inbox when control returns and processes no staged output if Fatal is established.

`panic!()` is not an Outcome. Kavod does not catch or translate panics. After panic, Kavod guarantees no Engine exit, final audit synchronization, or process termination. The embedding program and Rust panic mode determine what physically happens.

Application code may internally use functions, modules, state machines, reducers, components, or local queues. Those constructs receive no Kavod ordering, scheduling, bounds, or audit semantics. Work intended for a future turn must return through a Port Event.

The complete Application shape and capacity configuration are frozen before `Environment::start`.

## 4. Port Contracts, Slots, And Inboxes

A Port Contract associates one typed Event protocol with one typed Command protocol. It describes application data, not runtime behavior or lifecycle.

A Port Slot is one logical use of one Port Contract in an Application. Several Slots may use the same Contract while retaining separate identity, source authority, destination authority, capacity, inboxes, binding, and audit identity.

A Port binding is the frozen association between one Slot and one mode-specific runtime Port implementation. MVP permits one implementation per Slot.

Each Slot has:

- One bounded FIFO Event inbox.
- One bounded FIFO Command inbox.

The Core is the sole producer for each Command inbox, and the bound Port is its sole consumer. Live consumption may be concurrent; simulation consumption occurs during Environment polling. The Core accounts for its complete current-turn batch; concurrent live consumption can only increase available Command capacity.

The Slot, not candidate-supplied metadata, establishes authoritative Event source and Command destination.

A Port may offer only its Contract Event type. Kavod applies the Slot's frozen deterministic injection into the closed AppEvent type before acceptance. Conversion failure reports Fatal and invokes no handler.

Each bound Port participates in Environment lifecycle. `Environment::stop` controls aggregate graceful shutdown and succeeds only after every binding completes its private shutdown contract and will make no later use of run-scoped Event, Command, ordinary audit, or Fatal-reporting interfaces. Each binding privately decides how to handle handed-off Commands. Success does not prove Command processing, external effect, reconciliation, or exactly-once behavior.

Port Event staging and its terminal closure are linearized against each other. An offer that commits first is staged and later abandoned if it remains unaccepted; one that loses the closure race returns `RunClosed` and does not report inbox pressure.

After Fatal establishment, `Environment::abort` follows Section 9.2. Abort is infrastructure control, not an application Event or Command.

| Inbox rule | Consequence |
|---|---|
| Capacity is fixed before `RunStarted` | Inbox insertion never grows Core storage |
| Insertion is all-or-none | A value is inserted exactly once or not inserted |
| Full Event insertion while Event staging is open reports Fatal | Kavod never silently drops offered pressure during normal admission |
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
- Bounded Environment-lifecycle evidence and run-notifier storage.
- The bounded Fatal inbox and reserved terminal-record storage.
- Core counters and identifier domains.

After `RunStarted`, Core code does not request heap allocation or grow Core-managed storage. Fatal handling and Engine exit use the same preallocated storage.

All capacity arithmetic is checked before allocation and use. Construction validates that worst-case turn records, inbox entries, framing, and terminal reserve are mutually compatible. Identifiers never wrap, silently saturate, or reuse a prior value within their declared scope.

Runtime exhaustion reports Fatal before partial insertion of one inbox item or audit record, overwrite, or identifier assignment.

Terminal reserve includes bytes and audit-sequence values for `TurnCompleted` with Stop, the maximum Fatal record, and `FatalSyncFailed` where the Stop-to-Fatal path requires them together. The maximum Fatal record includes any complete rejected audit record retained by an Audit Fatal report. Fatal uses a fixed bounded fallback record if detailed report encoding fails. FatalSyncFailed records a fixed Core failure classification; the concrete AuditWriter error is returned separately in EngineExit.

Core stores a bounded number of typed values in preallocated inline storage sized for their concrete Rust types. AppState and Event, Command, and Fatal Reason values may contain pointers or handles to transitive allocations. Core may temporarily own or move those values, but their transitive allocations remain application- or Port-managed and outside this guarantee. Application code, encoders, Ports, the Environment, custom AuditWriters, and their allocation behavior also remain outside it.

Core code uses no recursion. Every Core-owned loop within a turn has a configured bound. Environment polling is nonblocking and bounded. A run continues until Stopped or Fatal, including Fatal caused by finite identifier exhaustion; application code, Ports, Environment stop, encoders, and AuditWriters may still block or fail to return.

## 6. AuditLog

The AuditLog is one globally ordered stream of mandatory evidence. It is not State, recovery authority, or a Command outbox.

Core and Port infrastructure submit complete bounded records to one bounded multi-producer queue. Application code has no AuditLog access. This queue-backed path is ordinary audit submission; reserved terminal records bypass it. Successful queue insertion establishes audit order; concurrent submissions may appear in either order. Ordinary submission and ordinary closure are linearized against each other: a submission that commits first belongs to the accepted prefix, while one that loses the closure race returns `RunClosed` and is outside the run AuditLog.

One AuditWorker processes accepted records in order and owns sequence assignment, framing, integrity protection, pending storage, and the AuditWriter. All AuditLog storage and terminal reserve are allocated before `RunStarted`.

Every record type has one fixed synchronization policy, conceptually:

```rust
enum AuditRecord<T> {
    Sync(T),
    NoSync(T),
}
```

`Sync(T)` appends `T` and then synchronizes the complete pending prefix through it. `NoSync(T)` appends `T` without synchronizing. The worker performs no automatic, periodic, or capacity-triggered synchronization.

For example, RunStarted, EventAccepted, CommandsPrepared, and TurnCompleted synchronize; CommandAccepted, EnvironmentStopStarted, and EnvironmentStopped do not. Successful completion of the reserved Sync(TurnCompleted with Stop) action synchronizes the accepted stop evidence before it. Fatal uses the reserved terminal path. New record types follow the same rules.

Submission never blocks. Before Fatal is established, a producer that observes a full or unexpectedly disconnected open queue submits an Audit Fatal report. When ordinary submission of a complete record fails, that report retains the complete rejected record; the record is evidence of the failed submission, not an accepted ordinary record or a retry. Encoding, framing, pending-capacity, writer, or synchronization failure follows the same reporting rule. These reports follow the single rule in Section 9; they do not establish Fatal. Intentional ordinary closure instead returns `RunClosed` and creates no Fatal report. Final Fatal synchronization failure follows Section 9's finalization rule instead of reporting another Fatal. Records are never silently dropped, overwritten, or sampled, and producers never retry failed submissions.

An AuditWorker that reports failure retains its pending state and terminal reserve for Engine-directed Fatal finalization.

Nonterminal turn processing does not wait for the AuditWorker. Synchronization does not authorize Core execution, and abrupt process destruction may lose an unsynchronized suffix.

Pending records are retained until Kavod observes synchronization success. The AuditWriter must permit the same prefix to be submitted again without duplicating or reordering logical records. A writer may physically persist bytes even when synchronization reports failure; failure means only that Kavod did not observe success.

Fatal reports use the bounded inbox in Section 9 rather than the ordinary audit queue. During Stop, ordinary submission remains open through aggregate Environment stop so its start and completion records join the accepted prefix. Stop then closes ordinary submission and uses the terminal reserve for Sync(TurnCompleted with Stop). Fatal closes ordinary submission when established, calls `Environment::abort`, and uses the terminal reserve for the single Sync(Fatal) containing the frozen Fatal reports. In either case, the worker processes the accepted prefix, makes the terminal synchronization attempt, and joins before EngineExit.

If final Fatal synchronization fails, the worker appends FatalSyncFailed without another writer call and returns the bounded pending AuditBuffer and synchronization error described in Section 9.2. The buffer is not a complete journal or retry instruction.

## 7. Startup And Ready

Engine construction validates the frozen Application, Port Slots, bindings, mode-specific implementations, encoders, capacities, and all required Core allocations. Failure during construction is a construction error, not an EngineExit. Exact construction-error representation is undecided.

The startup sequence is:

```text
Environment::start
-> submit Sync(RunStarted)
-> process Ready as Event index zero
-> if Continue: permit Port Event acceptance
-> if Stop: enter the ordinary Stop path
```

`Environment::start` is transactional. Failure leaves no run-scoped activity or interfaces live and returns a pre-run startup error without EngineExit. Successful return is the single publication point for run-scoped interfaces and transfers the active Environment to the Engine. It establishes runtime machinery and initializes SimPorts, not domain readiness.

Ready is the only built-in Engine Event. It is structurally first, receives `Environment::now()`, and otherwise uses the ordinary accepted-envelope evidence and canonical turn protocol. Ready may produce Commands.

No Port Event is accepted and no SimPort cursor is stepped before the Ready turn completes. Live Events may already wait in bounded Slot inboxes. Ready Commands reach SimPorts before the first cursor selection and cannot recursively invoke the handler.

Ready means that Kavod can begin execution. It does not mean connected, authenticated, subscribed, reconciled, armed, or safe to trade.

No Fatal boundary separates successful Environment start from the RunStarted attempt. RunStarted encoding or submission failure reports Fatal and Ready is not invoked. After RunStarted reaches its success or failure boundary, the Engine processes reports committed since start before admitting Ready. Later AuditWorker failure follows the same reporting path.

## 8. Event And Turn Processing

### 8.1 Event Staging And Acceptance

A Port stages an immutable typed Event into its Slot's Event inbox. Staging preserves per-Slot FIFO but does not assign Event index, logical time, or acceptance.

`Environment::next_event` is one nonblocking bounded poll returning `Option<NextEvent>`. A candidate identifies one Slot inbox head; selection neither removes the Event nor establishes source, time, or acceptance. Absent polling failure, `None` asserts that no Event candidate was selectable when the poll completed. A polling failure submits a Fatal report and returns `None`.

The Engine processes Fatal reports before and after each poll. If the post-poll boundary admits a candidate, one selected Event follows this path:

```text
read and validate Environment::now()
-> remove one Event from its Slot inbox
-> inject it into AppEvent
-> assign Event index and logical time
-> submit Sync(EventAccepted)
-> invoke on_event once
```

Event index is checked and never reused. Source Slot comes from the inbox, not the payload. Successful EventAccepted submission establishes acceptance.

Invalid candidate, clock regression, conversion, encoding, capacity, or EventAccepted submission failure reports Fatal and invokes no handler. An accepted Event is never retried.

The post-poll boundary either establishes Fatal and abandons the candidate or admits the complete Event action above. No boundary occurs between successful acceptance and handler invocation; a later report is processed after the handler returns.

After `None`, the Engine waits only while the race-safe run notifier confirms no runnable work. A staged Event remains signaled until selected; any published simulation cursor and every Fatal report remain signaled until processed. Signals may coalesce or be spurious and carry no Event ordering, source, logical time, or Fatal authority.

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
-> if Continue:
     submit Sync(TurnCompleted with Continue)
     resume Environment polling
-> if Stop:
     close Port Event staging and Core Command production
     abandon staged but unaccepted Events
     submit NoSync(EnvironmentStopStarted)
     call Environment::stop
     on failure: report Fatal
     process the post-stop Fatal boundary
     submit NoSync(EnvironmentStopped)
     close ordinary audit submission
     perform reserved Sync(TurnCompleted with Stop), including append and terminal synchronization
     process the final Stopped boundary
```

Section 9 defines the elided boundaries. EnvironmentStopStarted and `Environment::stop` form one admitted action: if evidence submission succeeds, the call follows without another boundary. EnvironmentStopped proves only that Kavod observed aggregate success after the post-stop boundary admitted its submission.

Command-capacity preflight or CommandsPrepared submission failure reports Fatal and inserts no current-turn Command.

Because the Core is the sole producer, successful capacity preflight guarantees that its complete batch fits; concurrent Port consumption can only create more space. A full result during subsequent insertion is an invariant violation and panics. Any other Command-inbox insertion or CommandAccepted submission failure reports Fatal. A successfully inserted Command remains handed off even if its CommandAccepted submission fails; later Commands are not attempted.

TurnCompleted is submitted only after every current-turn Command insertion and CommandAccepted submission succeeds. Continue may poll for a later Event after successful TurnCompleted submission without waiting for audit synchronization. Stop additionally requires successful aggregate Environment stop and successful stop evidence before its reserved TurnCompleted submission.

TurnCompleted with Stop is the terminal turn record for Stop. Its append proves that successful aggregate stop and its evidence preceded it, but its presence alone proves neither that Kavod observed terminal synchronization success nor that the final Stopped disposition won. Only returned `EngineExit::Stopped` proves observed successful Stop synchronization. A Fatal report committed during the combined terminal action is processed afterward, so TurnCompleted with Stop may be followed by Fatal.

Stopped proves that the Application completed its current turn, requested exit, Kavod observed successful aggregate Environment stop, and the terminal Stop record synchronized successfully. It does not prove that handed-off Commands were processed or caused external effects. Each binding's private stop contract owns those obligations beyond ending its use of run-scoped interfaces.

| Failure point | Result |
|---|---|
| Before EventAccepted submission | Handler is not invoked |
| During handler through explicit Fatal or checked Core failure | Staged Commands are not inserted |
| During Command-capacity preflight | No current-turn Command is inserted |
| Before CommandsPrepared submission | No current-turn Command is inserted |
| During one Command insertion or CommandAccepted submission | Successfully inserted Commands remain handed off; later Commands are skipped |
| During aggregate Environment stop or its audit evidence | Completed handoffs and Environment actions remain real; failure reports Fatal |
| During TurnCompleted with Continue submission | Handed-off Commands remain real; turn completion is not established |
| During TurnCompleted with Stop submission or synchronization | Handed-off Commands and successful Environment stop remain real; Fatal may follow an appended Stop record |

Every reportable failure in this table submits a report and follows Section 9. A synchronous Core failure ends the current action at its defined failure boundary; no later normal Core action begins before the next semantic boundary. Invariant failures panic under Section 1.

## 9. Fatal And Engine Exit

Fatal is the permanent failure disposition of a started run. There is one Fatal disposition, produced from one or more failure reports. EngineExit distinguishes Stopped from Fatal and returns State in either case; Fatal also returns the frozen reports and final audit status. `Outcome::Stop` only requests the graceful terminal protocol; Stopped is established after successful aggregate Environment stop, terminal synchronization, and the final boundary.

### 9.1 Reporting And Establishment

> Reporters latch; boundaries decide; admitted work finishes.

All Fatal sources, including Engine code, Context operations, Ports, the Environment, the AuditWorker, and the host, submit bounded reports to one run-scoped Fatal inbox. Reporting never establishes Fatal, starts finalization, or preempts an admitted action. Successful insertion establishes report order. The first report is the primary cause. Later reports are retained as secondary causes up to the configured capacity. A full-inbox report commits only `secondary_truncated` and follows the same closure ordering; it does not block or create another Fatal.

Only the Engine establishes Fatal. RunStarted is the sole exception: its attempt immediately follows successful Environment start, and the first Fatal boundary follows that attempt. The Engine then processes the Fatal inbox before each admitted action. The boundary atomically does exactly one of two things:

- If a report has committed, atomically close the Fatal inbox, ordinary audit submission, Port Event staging, and Core Command production; freeze the reports; and establish Fatal. Closing an already closed terminal interface succeeds without another effect. The next action does not begin.
- If no report has committed, admit exactly one following Core action. Admission of the final Stopped action atomically closes the inbox as Stopped.

A report committed after admission is processed at the next boundary. The admitted action reaches its defined success or failure boundary before another action is admitted. If Engine code detects a failure, it submits a report and begins no later normal action before that boundary. The Engine processes reports before and after each Environment poll and before waiting. The run notifier only makes it runnable and has no ordering or Fatal authority.

These boundaries admit:

- One nonblocking Environment poll.
- Event acceptance and, if acceptance succeeds, one handler invocation through normal return, including processing its Outcome and submitting any Application Fatal report.
- Current-turn Command preparation through CommandsPrepared submission.
- One Command insertion and, if insertion succeeds, its CommandAccepted submission.
- TurnCompleted with Continue submission.
- Stop Port Event-staging and Command-production closure.
- EnvironmentStopStarted submission and, if it succeeds, one aggregate Environment stop call.
- EnvironmentStopped submission after successful stop and the post-stop boundary.
- Stop ordinary-audit closure.
- Reserved TurnCompleted with Stop submission and terminal synchronization.
- Final Stopped commitment.

After normal handler return, `Outcome::Fatal` submits its Application report before the post-handler boundary. Reports already committed retain their earlier order. No staged output is processed when that boundary establishes Fatal.

At a boundary that establishes Fatal, a concurrent report either commits before closure or observes that Fatal is already established. The first report determines the primary cause returned in `EngineExit::Fatal`; secondary causes are diagnostic only.

Before returning Stopped, after aggregate Environment stop and terminal Stop synchronization succeed, the Engine processes one final semantic boundary. A concurrent report either commits first and causes Fatal or loses to final Stopped admission and observes that the run has Stopped.

Once Fatal is established:

- No later normal Core action begins.
- No later graceful Environment action begins.
- Commands not yet admitted for handoff are discarded.
- Events staged but not accepted are abandoned.
- Accepted Events, handed-off Commands, submitted audit records, and State mutations remain real.
- Kavod performs no rollback, Command retry, or normal continuation.

Fatal returns the State present when the Engine regains control. It may contain mutations from an incomplete turn and is diagnostic only.

### 9.2 Finalization

Fatal finalization is:

```text
ordinary audit submission, Port Event staging, and Core Command production are already closed
-> call Environment::abort
-> finish the accepted audit prefix
-> append one reserved Sync(Fatal) containing the frozen reports
-> make one final synchronization attempt
-> join the AuditWorker
-> return EngineExit::Fatal
```

`Environment::abort` is one bounded, nonblocking, best-effort call valid after partial or successful graceful stop. It signals live bindings, abandons simulated execution, invokes no Port callback, and is never awaited or retried by Core. Internal failure and later Port cleanup neither replace the primary cause nor begin another Fatal sequence. Ordinary records that commit before the audit cutoff remain in the accepted prefix; submissions after the cutoff receive `RunClosed`.

Reserved storage and fallback encoding make construction of the terminal Fatal record infallible. A violation of that guarantee is an invariant failure and panics.

Writer or synchronization failure during Fatal finalization follows the terminal failure path below and submits no report. If final synchronization succeeds, `audit` is `Synced`. If it fails, the worker appends `FatalSyncFailed` to the pending in-memory buffer without another writer call and returns:

```rust
FatalAudit::Unsynchronized {
    pending: AuditBuffer,
    sync_error,
}
```

The pending buffer contains records whose synchronization Kavod has not observed, including the terminal Fatal record and `FatalSyncFailed`. It is not necessarily the complete run AuditLog. Some or all records preceding `FatalSyncFailed` may already exist in the writer; `FatalSyncFailed` exists only in the returned buffer.

Finalization failure never replaces the primary cause or begins another Fatal sequence. Stop-to-Fatal transitions follow Section 9.1's admitted-action rule; accepted Stop evidence remains real. A failure before the Stop record is appended produces an accepted ordinary prefix followed by Fatal. The Stopped requirements are those in Sections 8.2 and 9.1. The AuditWorker remains available for Fatal finalization until the final Stopped boundary, then terminates and joins before EngineExit.

An authoritative host interrupt reports `Fatal(HostInterrupt)`. Graceful external shutdown requests and simulation-ending requests must arrive through a declared Port as ordinary Events; the Application decides whether to return Stop.

Panic, process destruction, an AuditWriter operation that never returns, or `Environment::stop` that never returns may prevent finalization and therefore provide no EngineExit. Once Fatal is established, Kavod does not wait for Port cleanup.

## 10. Environment Boundary

The Environment changes how Ports execute, not the Application or Core protocol. Its common interface is conceptually:

```rust
trait Environment {
    type StartError;
    type StopError;

    fn start(&mut self) -> Result<(), Self::StartError>;
    fn next_event(&mut self) -> Option<NextEvent>;
    fn now(&self) -> LogicalTime;
    fn stop(&mut self) -> Result<(), Self::StopError>;
    fn abort(&mut self);
}

struct NextEvent {
    slot: SlotId,
}
```

`start` is the transactional pre-run operation in Section 7. `next_event` and Port Event acceptance follow Section 8.1. Ready reads `now` under Section 7; a Port Event reads it only after its candidate wins the post-poll boundary.

`stop` is the single aggregate graceful Environment call in Section 8.2. The Environment controls Port ordering and private shutdown work. Failure has a bounded representation that the Engine reports through the Fatal inbox. `abort` is called only after Fatal establishment and follows Section 9.2.

Live and simulation use the same Port Contracts, Slots, Event inboxes, Command inboxes, and Core turn protocol. Application code cannot observe Environment mode, and Environment activity cannot recursively invoke `on_event`.

### 10.1 Live Ports

A LivePort owns its execution and receives one run-scoped LivePortSession. The session supplies Command and terminal-control input plus typed Event and failure-report output. Live workers may execute concurrently, but only Slot inbox staging can make an Event eligible for Engine selection. Exact thread, task, process, polling, and session APIs remain implementation details.

### 10.2 Simulated Ports

A SimPort is a synchronous deterministic state machine with four conceptual callbacks:

```rust
start(ctx)
on_command(ctx, command)
step(ctx) -> Option<Event>
stop(stop_ctx)
```

Each callback runs to completion on the simulation thread and may inspect `ctx.now()`. Contexts cannot be retained. Each SimPort binding has one Environment-held optional cursor slot modified only through its context:

```rust
ctx.set_next(time)
ctx.clear_next()
```

`set_next` replaces the cursor and rejects time before `ctx.now()`; equal time is valid. Rejection during `start(ctx)` makes `Environment::start` fail transactionally; during a started run it reports Fatal. Before `step`, the Environment advances virtual time to and clears the selected cursor. The callback must publish a replacement if more work remains. `Some(Event)` is inserted into the Slot's Event inbox and returned as a candidate; `None` denotes private progress only. The restricted stop context cannot publish cursors or Events.

Before cursor selection, one simulation poll delivers all Commands handed off by completed turns in deterministic order. It then selects deterministically and invokes at most one `step`. Ready Commands are delivered before the first selection. The exact deterministic Command and equal-time selection policies remain Environment configuration, not Application behavior.

The Environment counts `on_command` and `step` callbacks at each virtual time with checked arithmetic and resets only after time strictly advances. It reports Fatal before invoking a callback that would exceed the configured bound and begins no later callback in that poll. Any published cursor keeps the run notifier signaled.

### 10.3 Common Limits

Simulation ordering must not depend on pointer identity, unstable iteration, hidden concurrency, or unrecorded external state. Before start, the Environment validates one optional cursor slot per frozen SimPort binding and the configured same-time bound.
