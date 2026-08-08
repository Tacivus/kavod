# Kavod Core Design v8

> **Status:** Initial semantic draft
> **Scope:** The deterministic Core shared by live and simulated execution
> **Priority:** The smallest complete design whose rules can be enforced and tested

---

## 1. Engineering Thesis

Kavod is a deterministic application Core. One Engine owns one Application State, accepts one ordered Event, invokes one synchronous transition, hands off its ordered Commands, completes the turn, and only then accepts another Event.

The same frozen Application runs in every Environment. An Environment owns topology, Ports, queues, routing, waiting, logical time, and execution mode behind one Core-facing contract.

The AuditLog is the ordered evidence of Engine execution. Only the Engine submits records; the AuditLog prepares and retains them; the AuditWriter persists them.

Kavod is informed by NASA's Power of Ten, TigerBeetle's Tiger Style, and SQLite's defensive testing culture. These are influences, not claims of compliance. The enforceable rules are:

| Principle | Kavod rule |
|---|---|
| Correctness before convenience | Add no feature without enforceable semantics |
| Single authority | Each fact has one owner and one semantic representation |
| Explicit execution | One Event, handler invocation, and turn at a time |
| Finite resources | Every Kavod-managed container, buffer, count, and identifier has a configured maximum |
| Bounded local work | Kavod-owned active loops are bounded and nonrecursive |
| Checked arithmetic | Counts, lengths, capacities, times, and identities never wrap or silently saturate |
| Explicit failure | A failed required operation establishes one Fatal cause |
| Assertions mean bugs | Invariant violations panic and are outside Engine outcomes |
| Defensive boundaries | Validate knowable failure conditions before irreversible actions |
| Evidence-driven engineering | Every bound and failure boundary supports direct and fault-injection testing |

Resource bounds are semantic. Allocation strategy is an implementation choice.

Rust syntax is illustrative. Concrete APIs and storage remain implementation choices unless required by these semantics.

## 2. Core Model

An Engine owns one run:

- One frozen Application.
- One concrete Application State.
- One Environment with matching Event and Command protocols.
- One AuditLog.
- One bounded turn-local Command batch.
- One checked Event-index domain.

Only the Engine passes State to application code. At most one `on_event` call is active.

One accepted Event creates one turn. The handler runs to normal return before the Engine processes its Outcome or any latched Context failure. A turn reaches `TurnCompleted` or Fatal before another Event is requested.

An accepted Event has one authoritative representation:

```rust
struct EventEnvelope<E> {
    index: EventIndex,
    logical_time: LogicalTime,
    event: E,
}
```

It contains:

- A checked monotonic Event index.
- A frozen nondecreasing logical time.
- An immutable Event value.

`LogicalTime` is a fixed, totally ordered, checked domain. Ready establishes its accepted baseline. Event index is the sole Event order, and equal logical times are valid.

Event index is the zero-based accepted Event ordinal: it equals the accepted Event count before the current Event. Ready has index zero, and the first External Event has index one.

The Engine owns an optional accepted Event frontier containing the latest accepted index and logical time. Successful `RunStarted` or `EventAccepted` submission commits this frontier before handler invocation; each later envelope derives from it.

The determinism contract is:

> Given the same executable build, frozen Application, initial State, deterministic configuration, and ordered Environment and AuditWriter call-result trace, the Engine produces the same handler calls, State transitions, ordered Command intent, audit records, completed-turn frontier, and EngineExit.

Application behavior and audit encoding must not depend on hidden clocks, entropy, IO, environment variables, process-global mutable state, concurrent task order, pointer identity, unstable iteration, or Environment mode.

## 3. Application

Conceptually:

```rust
trait Application {
    type State;
    type Event: AuditEncode;
    type Command: AuditEncode;
    type FatalReason: AuditEncode;

    fn initial_state(&self) -> Self::State;

    fn on_event(
        &self,
        state: &mut Self::State,
        event: &EventEnvelope<CoreEvent<Self::Event>>,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::FatalReason>;
}

enum CoreEvent<E> {
    Ready,
    External(E),
}

enum Outcome<F> {
    Continue,
    Stop,
    Fatal(F),
}
```

The Application, its deterministic configuration, and all Engine capacities are frozen before `Engine::run`.

All run-varying mutable application data resides in State. The frozen Application and accepted Event and Command logical values remain immutable.

The handler may mutate complete State and stage Commands through Context. It receives no Environment, Port, AuditLog, AuditWriter, external IO, clock, entropy, or concurrency authority.

Context appends immutable Commands to the bounded current-turn batch in call order. It performs no Environment operation. Its first failure latches; later staging calls have no effect. After the handler returns, a latched Context failure takes precedence over the returned Outcome and discards the batch.

`Continue` completes the turn. `Stop` processes the current batch and requests graceful Environment shutdown. `Fatal` discards the batch and ends normal execution.

Internal application structure has ordinary Rust semantics. Work for a future turn returns through an External Event.

## 4. Port Protocols

A Port Contract associates one Event protocol with one Command protocol:

```rust
trait PortContract {
    type Event;
    type Command;
}
```

An application uses closed, source-qualified Event and destination-qualified Command sums. Distinct uses of one Contract remain distinct variants.

Conceptually:

```rust
enum TradingEvent {
    Primary(MarketDataEvent),
    Secondary(MarketDataEvent),
    Execution(ExecutionEvent),
    Timer(TimerEvent),
}

enum TradingCommand {
    Primary(MarketDataCommand),
    Secondary(MarketDataCommand),
    Execution(ExecutionCommand),
    Timer(TimerCommand),
}
```

These variants establish Event source and Command destination. A protocol macro may generate the sums and Environment-side routing support. No topology type participates in the Engine contract.

Command handoff is successful insertion into a destination inbox. Subsequent processing belongs to the Port. Externally consequential Commands carry application-owned reconciliation identity.

## 5. Environment

Conceptually:

```rust
trait Environment {
    type Event: AuditEncode;
    type Command: AuditEncode;
    type Error: AuditEncode;

    fn start(&mut self) -> Result<LogicalTime, Self::Error>;

    fn next_event(
        &mut self,
    ) -> Result<(Self::Event, LogicalTime), Self::Error>;

    fn command_batch(
        &mut self,
        commands: CommandBatch<Self::Command>,
    ) -> Result<(), Self::Error>;

    fn stop(&mut self) -> Result<(), Self::Error>;

    fn abort(&mut self);
}
```

`Engine<A, E>` requires equal Application and Environment Event and Command types.

The legal lifecycle is:

```text
Unstarted --start Err--------------------------> StartFailed
Unstarted --start Ok---------------------------> Running
Running   --next_event Ok / command_batch Ok---> Running
Running   --stop Ok----------------------------> Stopped
Running   --fallible operation Err-------------> Failed
Running or Failed --abort----------------------> Aborted
```

The Engine invokes one Environment method at a time. `start`, `stop`, and `abort` are each attempted at most once. `StartFailed`, `Stopped`, and `Aborted` are safe to drop. After `Failed`, only `abort` is valid. Other call sequences are invariant bugs.

Every successful operation result, each Command handoff, and the first runtime Environment failure has one commitment point in one Environment order. A committed result is irrevocable. A failure committed before the active operation's result makes that operation return the Error; a failure committed afterward is returned by the next fallible Environment operation. Thus a successfully returned Event is not revoked by a later Environment failure and reaches its handler after successful Engine acceptance, while a failure that wins first preempts undelivered Events.

While Running, the Environment retains its first runtime failure, closes future Event delivery and Command handoff, and wakes a blocked `next_event`. Later failures do not replace it.

`start` is transactional. `Ok(time)` publishes Running and freezes Ready's logical time. `Err` leaves no run-scoped activity live.

`next_event` owns waiting, selection, source authority, and time capture. It waits until it returns one authoritative `(Event, LogicalTime)` result or one Error. The pair commits atomically. Successful `start` and `next_event` times form a nondecreasing sequence. The Engine validates the time, assigns the next Event index, and constructs the EventEnvelope.

`command_batch` consumes a nonempty batch and attempts Commands at most once in order, stopping at the first failure without waiting for future capacity or processing. Each successful handoff commits before the next attempt. `Ok` commits with the final handoff and means the complete batch reached its destination inboxes. Its Error identifies the exact proper prefix handed off before the failure; the failed Command and suffix are discarded without handoff. A previously committed failure produces an empty prefix.

`stop` closes Event delivery and Command handoff, resolves private graceful shutdown, and quiesces all Environment activity and failure production. A pending failure makes it return `Err`; `Ok` establishes Stopped. The Environment owns shutdown disposition of handed-off Commands.

`abort` closes Engine-facing interfaces and initiates private termination. It invokes no application code and returns when the Environment is safe to drop; any continuing cleanup is Environment-owned and cannot use Engine-facing interfaces.

Every Environment operation preserves its configured bounds. Runtime resource exhaustion and asynchronous Port failure are Environment errors. Successfully offered Events and handed-off Commands are never silently overwritten, coalesced, or duplicated.

A live Environment may use concurrency. A simulated Environment produces deterministic results from frozen configuration and prior call history; normal simulation completion is an application-defined External Event. Neither Environment mode may invoke the application handler.

## 6. Audit

`AuditEncode` owns deterministic encoding of logical values. The closed AuditRecord protocol owns record schema and fixed synchronization policy. The AuditLog exclusively owns checked sequence assignment, framing, bounded pending storage, policy application, writer state, and one AuditWriter.

Conceptually:

```rust
trait AuditEncode {
    fn encode(
        &self,
        output: &mut AuditBuffer,
    ) -> Result<(), AuditEncodeError>;
}

trait AuditWriter {
    type Error;

    fn append(
        &mut self,
        record: &EncodedAuditRecord,
    ) -> Result<(), Self::Error>;

    fn sync(&mut self) -> Result<(), Self::Error>;
}

struct AuditLog<W: AuditWriter> {
    writer: W,
    pending: AuditBuffer,
    // Checked sequence, writer state, and terminal reserve.
}

impl<W: AuditWriter> AuditLog<W> {
    fn submit(
        &mut self,
        record: AuditRecord<'_>,
    ) -> Result<(), AuditFailureKind>;

    fn finalize_fatal(
        &mut self,
        cause: &impl AuditEncode,
    ) -> FatalAudit<W::Error>;
}
```

`AuditRecord` is the closed Engine-owned semantic protocol listed in the table below. It is a concrete enum, implements `AuditEncode`, and composes the `AuditEncode` implementations of its application and Environment payloads.

`AuditWriter::append(Ok)` accepts one complete record after all previously accepted records. `AuditWriter::sync(Ok)` synchronizes the complete accepted prefix. Persistence is unknown after either operation returns `Err`.

Only the Engine calls the AuditLog. `submit` accepts every nonterminal AuditRecord variant, selects its fixed policy, then transactionally assigns its sequence, encodes and frames it, and inserts it into pending storage. Ordinary preparation failure commits no sequence or bytes and establishes Audit Fatal before any writer call. Successful preparation consumes one unreused sequence value and places the complete record in pending storage before `AuditWriter::append`. Immediate submission then synchronizes the complete pending prefix. Successfully synchronized records are removed from pending storage.

The AuditLog reserves one sequence value and sufficient pending bytes for the maximum framed terminal Fatal record. Ordinary records cannot consume this reserve.

The closed AuditRecord protocol is:

| Record | Synchronization |
|---|---|
| `RunStarted`, including the complete Ready EventEnvelope | Immediate |
| `EventAccepted`, including the complete EventEnvelope | Immediate |
| `CommandsPrepared`, including complete ordered intent | Immediate |
| `CommandBatchAccepted` | With the next synchronized record |
| `StopRequested` | Immediate |
| `TurnCompleted` | Immediate |
| `Fatal`, including the primary cause | Immediate final attempt |

Immediate ordinary submission returns only after synchronization succeeds. The Fatal variant is constructed only by `finalize_fatal`.

An ordinary append or synchronization failure establishes Audit Fatal. The attempted record and all records since the last observed synchronization remain pending. The AuditLog makes no further writer call after a writer failure.

The terminal Fatal record contains a fixed-bounded descriptor of its cause and falls back to a fixed Core descriptor when detailed cause encoding fails or exceeds its reserve. When the writer has failed, the AuditLog adds this record to its terminal reserve without calling the writer.

Fatal audit status is:

```rust
enum FatalAudit<E> {
    Synced,
    Failed {
        pending: AuditBuffer,
        failure: E,
    },
}
```

`pending` is the exact encoded suffix whose synchronization Kavod did not observe.

Fatal finalization creates exactly one Fatal record. Failure to append or synchronize it changes only `FatalAudit` to `Failed` and never re-enters finalization.

## 7. Execution

### 7.1 Construction And Startup

Construction validates Engine configuration, bounds, arithmetic, and required storage. Construction failure returns `ConstructionError` before `Engine::run`. `Engine::run` creates initial State exactly once before startup.

Runtime startup is:

```text
Environment::start
-> on Error: Fatal with initial State
-> validate Ready time and reserve Event index zero
-> on validation failure: Core Fatal
-> construct the Ready EventEnvelope
-> submit RunStarted with Ready envelope
-> commit the accepted Event frontier
-> invoke on_event with Ready
-> process the ordinary turn result
```

Successful `RunStarted` submission is Ready's acceptance frontier. No External Event is requested before the Ready turn completes.

### 7.2 External Event

```text
verify that the next Event index exists
-> on index exhaustion: Core Fatal
-> Environment::next_event
-> on Error: Fatal
-> validate logical time and assign the reserved Event index
-> on validation failure: Core Fatal
-> construct the External EventEnvelope
-> submit EventAccepted
-> commit the accepted Event frontier
-> invoke on_event exactly once
-> process the ordinary turn result
```

Failure before successful `EventAccepted` submission invokes no handler. An Event returned by the Environment is consumed once and never retried.

### 7.3 Turn Result

After normal handler return:

```text
latched Context failure: Fatal
-> Outcome::Fatal: Fatal
-> when Commands exist:
     submit CommandsPrepared
     call Environment::command_batch
     on Error: Fatal
     submit CommandBatchAccepted
-> Outcome::Continue:
     submit TurnCompleted(Continue)
-> Outcome::Stop:
     submit StopRequested
     call Environment::stop
     on Error: Fatal
     submit TurnCompleted(Stop)
     return Stopped
```

`TurnCompleted` is the completed-turn frontier. Another Event is requested only after successful `TurnCompleted(Continue)` submission.

### 7.4 Fatal

The first failure observed by the Engine is the primary cause:

```rust
enum FatalCause<AF, EE> {
    Application(AF),
    Environment(EE),
    Audit(AuditFailureKind),
    Core(CoreFailure),
}

enum EngineExit<S, AF, EE, AFE> {
    Stopped {
        state: S,
    },
    Fatal {
        state: S,
        cause: FatalCause<AF, EE>,
        audit: FatalAudit<AFE>,
    },
}
```

`AuditFailureKind` is a fixed Core classification. A concrete AuditWriter error is owned by `FatalAudit::Failed`.

Fatal finalization is:

```text
stop normal execution
-> after start succeeded, call abort once unless stop returned Ok
-> finalize Fatal through AuditLog
-> return EngineExit::Fatal with State, cause, and FatalAudit
```

Finalization failure does not replace the primary cause. No normal or graceful action begins after Fatal. EngineExit returns the current State; State mutations, consumed Events, inserted Command prefixes, and accepted audit records remain real.

## 8. Bounds And Failure Boundary

Engine configuration bounds at least:

- Commands per turn.
- Encoded record bytes.
- Pending audit bytes.
- Event-index and AuditLog-sequence domains.
- Core error representations.

Each Environment separately bounds its queues, work units, clocks, counters, errors, and mode-specific resources.

Every configured bound has one accounting owner and is validated before run-scoped activity. Encoded and pending byte bounds include framing overhead. All capacity and identity arithmetic is checked before use. Exhaustion is detected by its accounting owner before corrupting one item, record, or identity. Every Kavod-owned active loop has a configured iteration bound; blocking waits are not active loops.

Bounds apply to the resource named by configuration. Values with transitive owned memory remain governed by their owning Application, Environment, or AuditWriter.

Engine control flow advances only when the active application, Environment, or AuditWriter call returns. Panic and process termination end the Engine semantic model.

## 9. Verification Obligations

Tests must establish:

- One handler invocation per synchronized accepted Event.
- No overlapping turns or State access.
- Checked Event order and nondecreasing logical time.
- Deferred, ordered, bounded Command staging.
- Exact successful Command prefix on batch failure.
- No rollback or retry after irreversible actions.
- Correct startup, Continue, Stop, and Fatal frontiers.
- Audit ordering, synchronization, pending retention, and failed finalization.
- Every configured resource and identifier boundary.
- Equal Application behavior under equal timed Event and Environment-result traces across live and simulated Environments.
- Failure behavior on both sides of every operation-result commitment point.
