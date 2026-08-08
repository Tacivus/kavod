# Kavod Core Design v9

> **Status:** MVP semantic draft (supersedes v8)
> **Scope:** The deterministic Core shared by live and simulated execution
> **Priority:** The smallest robust design implementable in days, whose rules can be enforced and tested

---

## 1. Engineering Thesis

Kavod is a deterministic application Core. One Engine owns one Application State, accepts one ordered Event, invokes one synchronous transition, hands off its ordered Commands, completes the turn, and only then accepts another Event.

The same frozen Application runs in every Environment. An Environment owns topology, waiting, Event selection, logical time, and execution mode behind one Core-facing contract.

The Journal is the ordered evidence of Engine execution. Only the Engine writes it.

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

v9 preserves the guarantees of v8 while replacing bespoke mechanisms with standard primitives: the Journal is serde JSON Lines over `std::io::Write`, Event and Command sums are plain enums written by hand or expanded by one declarative sugar macro, and the live Environment uses threads and a bounded channel. Simplicity is taken in mechanism, never in semantics. Robustness remains the first goal.

Resource bounds are semantic. Allocation strategy is an implementation choice.

Rust syntax is illustrative. Concrete APIs and storage remain implementation choices unless required by these semantics.

## 2. Core Model

An Engine owns one run:

- One frozen Application.
- One concrete Application State.
- One Environment with matching Event and Command types.
- One Journal.
- One bounded turn-local Command batch.
- One checked Event-index domain.

Only the Engine passes State to application code. At most one handler call is active.

One accepted start or External Event creates one turn. The handler runs to normal return before the Engine processes its Outcome. A turn completes or the run is Fatal before another Event is requested.

An accepted External Event has one authoritative representation:

```rust
struct EventEnvelope<E> {
    index: EventIndex,
    logical_time: LogicalTime,
    event: E,
}
```

`EventIndex` and `LogicalTime` are `Serialize` newtypes with transparent `u64` JSON representations. `EventIndex` is checked. The start turn has index zero, and the first External Event has index one.

`LogicalTime` is an opaque `u64` count of nanoseconds with an Environment-owned origin. It is owned by the Environment's single Event acceptor: exactly one authority stamps it. Port-domain timestamps, such as exchange or receive time, are ordinary Event payload fields and carry no Core meaning.

Successful `Environment::start` and `Environment::next_event` times must never decrease. A regression is an Environment bug and is Core Fatal. Equal logical times are valid; Event index is the sole Event order.

The Core determinism contract is:

> Within one concrete Environment type, the same executable build, frozen Application, initial State, configuration, complete accepted `(Event, LogicalTime)` and Environment-result trace, and Journal-writer call-result trace produce the same handler calls, State transitions, ordered Command intent, journal bytes, and typed EngineExit.

Across live and simulated Environments, equal accepted Event traces and abstract operation-result traces produce the same handler calls, State transitions, ordered Command intent, Journal protocol sequence, and `NormalizedExit`. Concrete Environment Error text and therefore Fatal-record bytes may differ by mode.

```rust
enum NormalizedExit {
    Stopped,
    Fatal(FatalKind),
}
```

Normalization maps `EngineExit::Stopped` to `Stopped` and maps each primary `FatalCause` variant to its corresponding `FatalKind`. Secondary finalization Errors do not replace the normalized primary classification.

Live Environments may produce nondeterministic traces when concurrent sources race. That nondeterminism is explicit in the accepted Environment trace; the Core remains deterministic conditional on that trace. Application behavior and journal content must not independently depend on hidden clocks, entropy, IO, environment variables, process-global mutable state, concurrent task order, pointer identity, unstable iteration, or Environment mode. Application-owned serialization must likewise avoid unstable iteration order.

## 3. Application

Conceptually:

```rust
trait Application {
    type State;
    type Event: Serialize;
    type Command: Serialize;
    type FatalReason: Display;

    fn initial_state(&self) -> Self::State;

    /// The start turn (index zero). It may stage Commands.
    fn on_start(
        &self,
        state: &mut Self::State,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::FatalReason> {
        Outcome::Continue
    }

    fn on_event(
        &self,
        state: &mut Self::State,
        event: &EventEnvelope<Self::Event>,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::FatalReason>;
}

enum Outcome<F> {
    Continue,
    Stop,
    Fatal(F),
}
```

The Application, its deterministic configuration, and all Engine capacities are frozen before `Engine::run`.

All run-varying mutable application data resides in State. The frozen Application and accepted Event and Command logical values remain immutable.

The handler may mutate complete State and stage Commands through Context. It receives no Environment, Journal, external IO, clock, entropy, or concurrency authority.

`Context::emit` is infallible and appends immutable Commands to the bounded current-turn batch in call order. The batch stores at most `max_commands_per_turn` Commands. The first emit beyond the bound stores nothing and sets an overflow marker; later emits also store nothing. After the handler returns, a set marker is Core Fatal and discards the batch, taking precedence over the returned Outcome. Commands are never silently dropped, coalesced, or reordered.

Conceptually, `Context::emit(&mut self, command: Command)` transfers ownership of one Command. Context also exposes the immutable current Event index and LogicalTime, so `on_start` can observe the accepted start time without a synthetic `CoreEvent::Ready` wrapper.

Absent a higher-precedence Core, Environment, or Journal failure, `Continue` completes the turn and `Stop` dispatches the current batch before graceful shutdown. `Fatal` discards the batch and ends normal execution.

Internal application structure has ordinary Rust semantics. Work for a future turn returns through an External Event.

## 4. Ports, Contracts, And Slots

A Port Contract pairs one Event protocol with one Command protocol. Every Contract is duplex; a direction with no messages uses the uninhabited `Never` type:

```rust
trait PortContract {
    type Event: Serialize;
    type Command: Serialize;
}

/// Kavod-provided uninhabited type for an absent direction.
enum Never {}

struct MarketData;

impl PortContract for MarketData {
    type Event = MarketDataEvent;
    type Command = Never; // Event-only Port.
}
```

Kavod implements `Serialize` for `Never` by exhaustive matching. No `Never` value can be constructed.

A Slot is one named use of a Contract. Distinct Slots of one Contract are distinct variants. An Application uses one closed, source-qualified Event sum and one closed, destination-qualified Command sum whose variants are its Slots:

```rust
kavod::ports! {
    pub enum TradingEvent / TradingCommand {
        Primary:   MarketData,
        Secondary: MarketData,
        Execution: Execution,
        Timer:     Timer,
    }
}
```

`ports!` is a declarative `macro_rules!` macro. It expands to exactly the two paired sums, with matching variant names and serde's default externally tagged `Serialize` representation:

```rust
#[derive(::serde::Serialize)]
pub enum TradingEvent {
    Primary(<MarketData as PortContract>::Event),
    Secondary(<MarketData as PortContract>::Event),
    Execution(<Execution as PortContract>::Event),
    Timer(<Timer as PortContract>::Event),
}

#[derive(::serde::Serialize)]
pub enum TradingCommand {
    Primary(<MarketData as PortContract>::Command),
    Secondary(<MarketData as PortContract>::Command),
    Execution(<Execution as PortContract>::Command),
    Timer(<Timer as PortContract>::Command),
}
```

The macro is pure syntax sugar with no semantic authority. Hand-writing enums with the same variants and serde representation is fully supported and observationally identical. The macro generates no routing, no topology type, and nothing that participates in the Engine or Environment contracts. Macro consumers already require serde for their protocol payloads and must have a direct dependency named `serde`; this keeps generated derives hygienic without custom derive machinery. The MVP Engine may initially use hand-written sums; the macro is implemented after the Engine runs end-to-end.

Wiring remains explicit and compiler-checked:

- Fan-in uses a variant constructor per inhabited Event direction at wiring time. An enum variant constructor is already a function: `TradingEvent::Primary` maps one `MarketDataEvent` into one `TradingEvent`.
- Fan-out is one hand-written `match` in the Environment's dispatch path, with exhaustiveness enforced by the compiler. An arm with a `Never` payload is discharged by matching the uninhabited value: `TradingCommand::Primary(never) => match never {}`.

Adding a Slot is one line in `ports!`, one fan-in wiring line when its Event direction is inhabited, and one exhaustive dispatch arm.

Command handoff is successful dispatch into the Environment. Subsequent processing belongs to the Environment. An externally consequential Command carries an Application-owned stable business key sufficient to recognize a repeated or uncertain external effect. Correct key scope and uniqueness are Application obligations.

## 5. Environment

Conceptually:

```rust
trait Environment {
    type Event;
    type Command;
    type Error: Display;

    /// Erect run-scoped machinery and freeze the start turn's LogicalTime.
    fn start(&mut self) -> Result<LogicalTime, Self::Error>;

    /// Wait for and return the next authoritative Event and LogicalTime.
    fn next_event(
        &mut self,
    ) -> Result<(Self::Event, LogicalTime), Self::Error>;

    /// Attempt one Command handoff.
    fn dispatch(
        &mut self,
        command: Self::Command,
    ) -> Result<(), Self::Error>;

    /// Consume the Environment and release all run-scoped machinery.
    fn shutdown(self, mode: ShutdownMode) -> Result<(), Self::Error>;
}

enum ShutdownMode {
    Graceful,
    Abort,
}
```

`Engine<A, E>` requires equal Application and Environment Event and Command types.

After successful Engine construction, the Engine is the only caller and guarantees this call order: `start` exactly once; then `next_event` and `dispatch` interleaved, one call at a time; then `shutdown` at most once. There is no Environment lifecycle state machine.

- `start` returning `Err` leaves no run-scoped activity live. The Environment cleans up after itself and is safe to drop.
- `next_event` owns waiting, source selection, and time stamping. It waits until it returns one authoritative `(Event, LogicalTime)` pair or one Error. A returned Event is a committed candidate: it is consumed exactly once and is never retried or revoked. It becomes an accepted Event only after `EventAccepted` is successfully flushed.
- `dispatch` attempts one Command handoff without waiting for future capacity. `Ok` commits the handoff. `Err` guarantees that this Command was not handed off; the Engine does not retry it.
- `shutdown(Graceful)` stops Event delivery, rejects new Commands, and resolves the configured graceful disposition of handed-off Commands. `shutdown(Abort)` stops Event delivery and new Command handoff without initiating further externally consequential work.
- `shutdown` consumes the Environment, quiesces all run-scoped activity, and returns when it is safe to drop even when it returns `Err`. Its Error reports failure to achieve the requested disposition, not failure to quiesce.
- Any Error from a normal fallible Environment operation is Fatal to the run. An Abort cleanup Error is retained as a secondary finalization Error and never replaces an existing primary cause.

Every Environment operation preserves the Environment's own configured bounds, including channel capacities, queue depths, thread counts, and shutdown work. Successfully returned Events and handed-off Commands are never silently overwritten, coalesced, or duplicated. Neither Environment mode may invoke the Application handler.

### 5.1 Live Environment

A live Environment uses plain threads and one bounded `std::sync::mpsc::sync_channel` for Event fan-in. Each source adapter is a thread that owns its native client and offers mapped Events to the channel without silently waiting for future capacity. Queue-full, disconnect, and adapter failures become Environment Errors. The single channel acceptor stamps `LogicalTime` from one monotonic clock, making time regression structurally impossible in correct operation. Monotonic-duration conversion is checked; exhaustion is an Environment Error.

Each Command destination adapter owns one configured bounded Command inbox. `dispatch` is the hand-written destination match and performs one nonblocking inbox admission. Successful admission is the Command handoff commitment; queue-full or disconnect returns Error before admission. Native IO occurs later inside the owning adapter and does not alter that commitment.

`shutdown` signals adapters, closes Engine-facing channels, and joins a configured maximum number of adapters. Adapter implementations must make every blocking point cancellation-aware and must cooperate with shutdown. Blocking duration is not an active-loop bound and Kavod promises no wall-clock termination deadline. Live Event and Command types must be `Send + 'static`.

### 5.2 Simulated Environment

A simulated Environment owns a model and a deterministic scheduled queue of `(LogicalTime, sequence, Event)`. Its user-supplied Command handler is conceptually equivalent to:

```rust
FnMut(&Model, Command, &mut SimTransaction<Event, Model>) -> Result<(), Error>
```

The handler receives the authoritative model immutably. `SimTransaction` may stage one replacement model and future Events, but cannot mutate authoritative state. Each staging operation immediately validates and reserves bounded queue capacity, checked insertion-sequence availability, and `scheduled_time >= current_time` inside provisional storage. Therefore no fallible validation remains after the callback returns `Ok`.

On `Ok`, the Environment commits the replacement model and all staged Events atomically using previously reserved storage. On `Err`, it discards the complete transaction without changing the authoritative model or queue and without consuming authoritative insertion sequences. Callback-captured mutable state must not affect future behavior; all observable simulation state belongs to the Environment-owned model. Violation is an Environment bug.

`next_event` removes the entry with the lowest `(LogicalTime, sequence)` key and advances logical time. The queue ordering wrapper compares only this key and imposes no `Ord` bound on Event. Equal-time entries are ordered by a checked insertion sequence with an Environment-configured maximum. Sequence exhaustion is detected before queue mutation and is an Environment Error. The simulated Environment uses no concurrency. Its callback's determinism, bounded work, and avoidance of hidden authority are trusted Environment obligations subject to repeatability tests.

Normal simulation completion is an application-defined External Event whose handler returns `Stop`. An Environment Error is always Fatal.

## 6. Journal

The Journal is ordered, human-readable evidence of Engine execution: forensics, not a crash-proof write-ahead log. It is JSON Lines. Each record is one `serde_json` object on one line. The line number is the sequence and the newline is the frame. There is no bespoke encoding, framing, sequence domain, pending store, or synchronization policy.

Conceptually:

```rust
struct Journal<W: std::io::Write> {
    writer: W,
    // One reusable bounded record buffer, one bounded Fatal-message buffer,
    // and a poison marker.
}

#[derive(Serialize)]
enum CompletedOutcome {
    Continue,
    Stop,
}

enum RecordKind {
    RunStarted,
    EventAccepted,
    CommandsPrepared,
    CommandsDispatched,
    StopRequested,
    TurnCompleted,
    Fatal,
}

#[derive(Serialize)]
enum FatalKind {
    Application,
    Environment,
    Journal,
    Core,
}

#[derive(Serialize)]
enum Record<'a, E: Serialize, C: Serialize> {
    RunStarted {
        logical_time: LogicalTime,
    },
    EventAccepted {
        index: EventIndex,
        logical_time: LogicalTime,
        event: &'a E,
    },
    CommandsPrepared {
        index: EventIndex,
        commands: &'a [C],
    },
    CommandsDispatched {
        index: EventIndex,
    },
    StopRequested {
        index: EventIndex,
    },
    TurnCompleted {
        index: EventIndex,
        outcome: CompletedOutcome,
    },
    Fatal {
        index: Option<EventIndex>,
        kind: FatalKind,
        message: &'a str,
    },
}
```

`W: std::io::Write` is the persistence abstraction. A File, `Vec<u8>`, network stream, or any other byte sink may be used. Memory sinks make Journal tests and fault injection direct.

Writing a record is:

```text
encode through a max_record_bytes-limited writer into the reusable buffer
-> write the complete encoded object and newline to W
-> flush W
```

`max_record_bytes` bounds the encoded JSON object and excludes the one terminating newline byte. The bounded encoder rejects bytes beyond the bound before extending the buffer. Encoding failure or bound exhaustion occurs before any bytes for that record reach `W`; it is Journal Fatal.

The Journal writes directly from the complete record buffer, without a second `BufWriter`. Its own partial-write loop permits at most one successful progress call per output byte plus one terminal call. It does not retry `Interrupted`. `Ok(0)` while bytes remain becomes `io::ErrorKind::WriteZero`. Either condition is a sink write failure. A write or flush Error poisons the Journal permanently. Bytes after the last successfully flushed record are an uncertain trailing suffix and are not Journal records, even if they happen to contain a complete line. The Journal makes no further explicit `Write::write` or `Write::flush` call, including a Fatal write, through a poisoned writer; behavior of the user-owned writer's destructor is outside this contract.

`flush` after every record is the complete Journal persistence guarantee: durability beyond the operating system's write interface, including power-loss durability, is outside the MVP contract. A sink is fresh for one run or is initially positioned immediately after a newline. Journal sequence is run-relative.

Records evidencing handler invocation and Command handoff are flushed before those actions: `EventAccepted` before the handler is invoked, and `CommandsPrepared`, containing the complete application-defined serialized intent, before the first dispatch. Environment Event acquisition commits earlier and may be followed by validation or Journal failure without structural evidence of the candidate Event.

Application Events and Commands appear through their application-defined `Serialize` representations. Kavod does not claim that a custom or lossy serializer captures fields it omits. Application and Environment primary failures remain typed in `EngineExit` and are rendered through `Display` only at the Journal boundary. Deterministic, side-effect-free `Serialize` and `Display` implementations are trusted Application and Environment obligations.

Fatal text is formatted into a separate buffer that is also bounded by `max_record_bytes`, then encoded into the record buffer. Both buffers reserve their complete configured capacity before run-scoped activity begins.

The Fatal record is attempted at most once, only while the Journal is unpoisoned. If the Journal has already failed, encoding fails, the bound is exceeded, or its write fails, the primary cause still reaches `EngineExit`; finalization is never re-entered.

An Application whose Event and Command types also implement `serde::de::DeserializeOwned` may use a separately owned Journal-reading schema to construct a replay script for the simulated Environment. A reader must reject a line after `max_record_bytes + 1` bytes without first allocating the complete line. Replay is enabled by the JSONL format but is not required for the MVP.

## 7. Execution

### 7.1 Construction And Startup

Engine and Environment bounds use nonzero types, making zero invalid at construction. The Engine reserves the complete Command batch, record buffer, and Fatal-message buffer before it creates State or starts the Environment. Checked layout or allocation failure returns `ConstructionError`; it is not a runtime Fatal and invokes no Application or Environment method.

Runtime startup is:

```text
create initial State exactly once
-> Environment::start
   on Error: Fatal(Environment::Start); Environment is safe to drop
-> write RunStarted with the accepted start LogicalTime
   on Error: Fatal
-> invoke on_start for turn index zero
-> process the ordinary turn result
```

Successful `RunStarted` flush accepts the start turn and establishes the current accepted index as zero. Before that point, there is no current accepted index. No External Event is requested before the start turn completes.

### 7.2 External Event

```text
verify accepted External Event count < max_turns
   on exhaustion: Core Fatal
-> Environment::next_event
   on Error: Fatal(Environment::NextEvent)
-> validate LogicalTime is not less than the previous accepted time
   on regression: Core Fatal
-> checked assignment of the next EventIndex
-> construct the EventEnvelope
-> write EventAccepted
-> invoke on_event exactly once
-> process the ordinary turn result
```

Failure before a successful `EventAccepted` flush invokes no handler. An Event returned by the Environment is a committed candidate and is consumed once even if time validation or Journal writing subsequently fails. Successful `EventAccepted` flush accepts the Event and establishes its index as the current accepted turn.

`max_turns` bounds accepted External Events, excluding the start turn. Exhaustion is Fatal: it prevents an unbounded Event/Command feedback loop, including a loop that advances EventIndex indefinitely at one LogicalTime.

### 7.3 Turn Result

After normal handler return:

```text
Context overflow marker set: Core Fatal; discard batch
-> Outcome::Fatal: Fatal(Application); discard batch
-> when Commands exist:
     write CommandsPrepared with complete ordered intent
     dispatch each Command once in order
       on Error at zero-based position k: Fatal(Environment::Dispatch(k));
         discard undispatched suffix
     write CommandsDispatched
-> Outcome::Continue:
     write TurnCompleted(Continue)
     begin next-Event acquisition; its bound check may fail before next_event
-> Outcome::Stop:
     write StopRequested
     Environment::shutdown(Graceful)
       on Error: Fatal(Environment::ShutdownGraceful)
     write TurnCompleted(Stop)
     return EngineExit::Stopped
```

The `CommandsPrepared` record plus the typed `EngineExit` cause identifies the exact successful prefix `[0, k)` after a partial batch failure. A dispatch Error means the Command at zero-based position `k` was not handed off. The Journal alone need not contain the position if Fatal recording fails.

Another Event is requested only after `TurnCompleted(Continue)` is flushed.

### 7.4 Fatal

The first failure observed by the Engine is the primary cause:

```rust
enum FatalCause<AF, EE> {
    Application(AF),
    Environment {
        error: EE,
        operation: EnvironmentOperation,
    },
    Journal(JournalFailure),
    Core(CoreFailure),
}

enum EnvironmentOperation {
    Start,
    NextEvent,
    Dispatch { position: usize },
    ShutdownGraceful,
}

enum JournalFailure {
    Encode {
        record: RecordKind,
        error: serde_json::Error,
    },
    RecordBoundExceeded {
        record: RecordKind,
    },
    Sink {
        record: RecordKind,
        operation: JournalOperation,
        error: std::io::Error,
    },
}

enum JournalOperation {
    Write,
    Flush,
}

enum CoreFailure {
    TimeRegression {
        previous: LogicalTime,
        offered: LogicalTime,
    },
    TurnBoundExceeded,
    CommandBoundExceeded,
}

enum EngineExit<S, AF, EE> {
    Stopped {
        state: S,
    },
    Fatal {
        state: S,
        cause: FatalCause<AF, EE>,
        shutdown_error: Option<EE>,
        journal_error: Option<JournalFailure>,
    },
}
```

`RecordKind` is the closed set of Record variants. `FatalKind` maps directly from the four `FatalCause` variants. Application, Environment, serializer, and Journal sink Errors remain typed in `EngineExit` for as long as the Engine owns them. The Fatal JSONL record uses that closed classification and a bounded `Display` rendering.

The Fatal record's `index` is `Some(i)` exactly when a successful `RunStarted` or `EventAccepted` flush has established current accepted turn `i`; it is `None` before the start turn is accepted. A consumed candidate Event whose `EventAccepted` write fails does not become current.

Fatal finalization is:

```text
stop normal execution
-> if the Environment started and has not shut down:
     Environment::shutdown(Abort)
     retain any Error as shutdown_error without replacing the primary cause
-> if the Journal is unpoisoned:
     render and write the Fatal record, best effort, at most once
     retain any Error as journal_error without replacing the primary cause
-> return EngineExit::Fatal with State, typed primary cause,
     shutdown_error, and journal_error
```

Finalization failure never replaces the primary cause. After Fatal, no handler, dispatch, or graceful action begins; only mandatory abort cleanup and the Fatal-record attempt may run. `EngineExit` returns the current State; State mutations, consumed candidate Events, dispatched Command prefixes, and successfully flushed Journal records remain real.

## 8. Bounds And Configuration

Conceptually:

```rust
struct EngineConfig {
    /// Accepted External Events per run. Exhaustion is Core Fatal.
    max_turns: NonZeroU64,

    /// Commands stored in one turn batch. Exhaustion is Core Fatal.
    max_commands_per_turn: NonZeroUsize,

    /// Encoded JSON-object bytes, excluding newline. Exhaustion is Fatal.
    max_record_bytes: NonZeroUsize,
}

enum ConstructionError {
    CommandStorage(TryReserveError),
    JournalRecordStorage(TryReserveError),
    FatalMessageStorage(TryReserveError),
}
```

Bounds use nonzero types so that zero is unrepresentable. Checked storage layout and reservation occur before run-scoped activity and may return `ConstructionError`.

Each bound has one accounting owner and is checked before corrupting one item, record, or identity:

- The Engine owns `max_turns`, `max_commands_per_turn`, and `max_record_bytes`.
- Each Environment owns its channel capacity, scheduled-queue depth, adapter count, logical-time domain, insertion-sequence domain, counters, and shutdown-work bounds. Every concrete bound documents its unit, check point, accounting owner, and exhaustion Error.
- Values with transitive owned memory remain governed by their owning Application, Environment, or Journal sink.

All Kavod-managed containers and buffers are bounded. The Command batch never stores beyond `max_commands_per_turn`. The Journal encoding buffer never stores beyond `max_record_bytes`. Live and simulated queues never store beyond their Environment capacities.

Kavod-owned active loops are bounded and nonrecursive. The run loop performs at most one start turn plus `max_turns` External turns; the dispatch loop is bounded by the current batch length; Environment polling, queue, and shutdown loops are bounded by Environment-owned iteration or work budgets. Blocking waits are not active loops and have no implied elapsed-time bound.

All capacity and identity arithmetic is checked before use. `max_turns` may equal `u64::MAX`; exhaustion is checked before attempting an index beyond the configured maximum, so EventIndex overflow is unreachable and remains an invariant assertion rather than an Engine outcome. Environment time conversion, insertion sequences, and other identities have the same checked-arithmetic obligation under their owning bounds.

Engine control flow advances only when the active Application, Environment, or Journal-writer call returns. Work delegated to user-defined handlers, serializers, formatters, destructors, Environment callbacks, and `Write` implementations is outside Kavod's active-loop bound; these implementations must be bounded and nonpanicking. Blocking is not a progress guarantee.

Panic and process termination end the Engine semantic model. The Environment remains safe to drop after `start`; an Engine-owned cleanup guard invokes abort cleanup during unwinding. Catching an Engine panic and attempting to resume the run is unsupported.

## 9. Verification Obligations

Tests must establish:

- One handler invocation per flushed `EventAccepted` record.
- No overlapping turns or State access.
- Checked Event order, nondecreasing LogicalTime, and Core Fatal on regression.
- Deferred, ordered, bounded Command staging.
- Context overflow is Core Fatal, stores no out-of-bound Command, discards the batch, and takes precedence over the returned Outcome.
- Live dispatch commits exactly at bounded destination-inbox admission; queue-full and disconnect commit nothing.
- Simulated dispatch exposes authoritative model state immutably, commits its complete provisional transaction atomically, and leaves model, queue, and insertion sequence unchanged on every Error.
- Dispatch stops at the first Error; `CommandsPrepared` and the typed Fatal dispatch position identify the exact successful prefix.
- No retry or rollback after an Event or Command commitment.
- Exact Journal sequences for startup, Continue, Stop, and every Fatal boundary.
- Journal encode, size, write, zero-progress, interrupted-write, and flush failures; sink failure poisons the Journal and permits no later explicit sink call; Fatal-record writing is best effort and never re-entered; primary and finalization Errors reach `EngineExit` typed.
- Graceful and Abort shutdown dispositions, graceful shutdown as a possible primary Fatal, and Abort failure as a typed secondary Error.
- Construction storage failure occurs before State creation and invokes no Application or Environment method.
- `max_turns`, `max_commands_per_turn`, `max_record_bytes`, and every Environment bound at, below, and above their boundaries.
- Repeated equal abstract traces produce equal handler calls, State transitions, ordered Command intent, Journal protocol sequences, and `NormalizedExit` across live and simulated Environments. Repeated traces within one Environment type additionally produce equal Journal bytes and typed EngineExit values.
- Failure behavior on both sides of every irreversible operation.
- `ports!` expansion has the same Rust variants and serialized bytes as the corresponding hand-written Event and Command sums.

## Appendix A. Deltas From v8

| v8 mechanism | v9 replacement |
|---|---|
| `AuditEncode` and derive macro, `AuditBuffer`, framing, Audit sequence domain | serde `Serialize` and JSON Lines; newline is the frame and line order is the sequence |
| `AuditWriter`, pending store, terminal reserve, `FatalAudit`, synchronization policy table | `Journal<W: std::io::Write>`, flush per record, poison after sink failure, at most one best-effort Fatal record |
| Port protocol macro with generated Environment routing | `PortContract` remains descriptive; `macro_rules!` `ports!` expands only the paired sums; routing remains hand-written |
| `CoreEvent<E>` and `Ready` variant | Optional `on_start` hook for the index-zero start turn |
| Environment lifecycle state machine and separate `stop`/`abort` methods | Engine-enforced call order and one consuming `shutdown(mode)` with typed graceful or Abort disposition |
| `command_batch` with prefix result | Per-Command `dispatch`; exact prefix identified by `CommandsPrepared` plus typed failure position |
| Context latched staging Error | Infallible `emit` with bounded storage and one overflow marker |
| Event-index exhaustion Fatal and reserve-before-fetch | `max_turns` makes EventIndex overflow unreachable; checked assertion remains |
| Audit-specific construction and pending-byte bounds | Engine-owned turn, Command, and Journal-line bounds plus Environment-owned queue and work bounds |
