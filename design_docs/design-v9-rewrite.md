# Kavod Core Design

> **Status:** MVP semantic draft
> **Scope:** The deterministic Core shared by live and simulated execution

Rust syntax is illustrative. APIs and storage are implementation choices unless this document gives them semantic meaning. Section-local invariant tables are authoritative; Section 9 collects compact copies for review and test traceability.

Kavod Core is written under `#![forbid(unsafe_code)]`.

## 1. Core Model

One Engine owns one run: a frozen Application, its mutable State, one Environment with matching Event and Command types, one Journal, one bounded turn-local Command batch, and one checked Event-index domain. The Environment owns topology, waiting, Event selection, logical time, lifecycle, routing, and execution mode. Each Port owns its domain and native state. Only the Engine writes the Journal or passes State to application code.

### Core invariants

| ID | Invariant |
|---|---|
| `CORE-AUTHORITY` | Every fact has one owner and one semantic representation. |
| `CORE-TURN` | At most one turn and handler call are active; a turn completes or the run becomes Fatal before another Event is requested. |
| `CORE-ORDER` | Event index is the sole accepted Event order. The start turn is index 0; External Events begin at 1. |
| `CORE-TIME` | One Environment acceptor stamps Core logical time; accepted times never decrease, and equal times are valid. |
| `CORE-BOUNDS` | Every Kavod-managed container, buffer, count, and identifier has an explicit bound and accounting owner. |
| `CORE-ARITHMETIC` | Counts, lengths, capacities, times, and identities use checked arithmetic and never wrap or silently saturate. |
| `CORE-FAILURE` | The first failed required operation observed by the Engine establishes one typed primary Fatal cause. |
| `CORE-PANIC` | Knowable failures returned by user components are typed; Kavod-internal invariant violations intentionally panic, and user-code panics or Rust trait-contract violations are outside Engine outcomes. |
| `CORE-EVIDENCE` | The Journal is ordered evidence of Engine execution and has one writer: the Engine. |

An accepted External Event has this representation:

```rust
struct EventEnvelope<E> {
    index: EventIndex,
    logical_time: LogicalTime,
    event: E,
}
```

`EventIndex` and `LogicalTime` are `Serialize` newtypes with transparent `u64` JSON representations. `LogicalTime` is an opaque nanosecond count with an Environment-owned origin. Port timestamps are ordinary Event payload fields with no Core meaning. A time returned by `start` or `next_event` that precedes the last accepted time is `CoreFailure::TimeRegression`, not a panic.

### Determinism

Within one concrete Environment type, the same executable build, frozen Application, initial State, configuration, complete accepted `(Event, LogicalTime)` and Environment-result trace, and Journal-writer call-result trace produce the same handler calls, State transitions, ordered Command intent, Journal bytes, and typed `EngineExit`.

Across live and simulated Environments, equal Engine configuration and capacities, accepted Event traces, and abstract operation-result traces produce the same handler calls, State transitions, ordered Command intent, Journal protocol sequence, and `NormalizedExit`. An abstract operation-result trace records each Core-facing Environment operation, success or failure classification, and commitment result while discarding mode-specific Error values. A Journal protocol sequence compares record variants and mode-independent fields; concrete Environment Error text and therefore Fatal-message bytes may differ by mode.

```rust
enum NormalizedExit {
    Stopped,
    Fatal(FatalClass),
}

enum FatalClass {
    Application,
    Environment(EnvironmentOperation),
    Journal(JournalFailureKind, RecordKind),
    Core(CoreFailureKind),
}

enum JournalFailureKind {
    Encode,
    RecordBoundExceeded,
    SinkWrite,
    SinkFlush,
}

enum CoreFailureKind {
    TimeRegression,
    TurnBoundExceeded,
    CommandBoundExceeded,
}
```

Normalization retains the Core-defined primary discriminants shown above, including dispatch position and Journal record kind, and discards concrete Errors and `Display` text. Secondary finalization Errors do not change it.

Concurrent live sources may race; their selected order is explicit in the accepted trace. Application behavior and Journal content must not independently depend on hidden clocks, entropy, IO, environment variables, process-global mutation, concurrent order, pointer identity, unstable iteration, or Environment mode. Application serialization must use stable iteration order.

## 2. Application

```rust
trait Application {
    type State;
    type Event: Serialize;
    type Command: Serialize;
    type FatalReason: Display;

    fn initial_state(&self) -> Self::State;

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

### Application invariants

| ID | Invariant |
|---|---|
| `APP-FROZEN` | The Application, deterministic configuration, and Engine capacities are frozen before `run`. |
| `APP-STATE` | `initial_state` runs once; all run-varying mutable application data resides in State. State mutations are never rolled back. |
| `APP-AUTHORITY` | A handler receives State, its accepted Event data, and Context, but no Environment, Journal, external IO, clock, entropy, or concurrency authority. |
| `APP-EMIT` | `emit` is infallible and transfers one Command. While capacity remains it appends in call order; Commands are immutable after staging. |
| `APP-OVERFLOW` | The first over-bound emit stores nothing and sets an overflow marker; later emits store nothing. After normal return, overflow is Core Fatal, discards the batch, and outranks the returned `Outcome`. |
| `APP-OUTCOME` | `Continue` completes the turn; `Stop` dispatches the batch before graceful shutdown; `Fatal` discards the batch. |
| `APP-FUTURE` | Work for a future turn returns through an External Event. |

Context exposes immutable current index and logical time, including the accepted start time at index 0. `remaining()` is exact and returns zero after overflow. Commands are never silently dropped, coalesced, duplicated, or reordered.

## 3. Ports, Contracts, and Slots

```rust
trait PortContract {
    type Event: Serialize;
    type Command: Serialize;
}

enum Never {}
```

A Contract pairs one Event protocol with one Command protocol. Every Contract is duplex; an absent direction uses Kavod's uninhabited `Never`, whose `Serialize` implementation exhaustively matches an impossible value.

A Slot is one named use of a Contract. Application Event variants are source-qualified by Slot and Command variants are destination-qualified by Slot. Distinct Slots of the same Contract remain distinct variants:

```rust
kavod::ports! {
    pub enum TradingEvent / TradingCommand {
        Primary: MarketData,
        Secondary: MarketData,
        Execution: Execution,
        Timer: Timer,
    }
}
```

`ports!` is syntax sugar for paired enums with matching variant names, associated Contract payload types, and serde's default externally tagged representation. Equivalent hand-written enums are supported and observationally identical. The macro generates no routing, topology, Engine behavior, or Environment behavior. Generated derives use `::serde`; consumers therefore need a direct dependency named `serde`.

### Port invariants

| ID | Invariant |
|---|---|
| `PORT-CONTRACT` | One runtime Port is one mode-specific implementation of one bound Slot and Contract. |
| `PORT-STATE` | A Port exclusively owns its mutable domain, protocol, and native state; wiring and the Environment do not interpret that state. |
| `PORT-SUMS` | Slot-qualified Event and Command sums are closed, exhaustive, and type-correct. |
| `PORT-ROUTING` | Fan-in uses one frozen constructor per inhabited Event direction; fan-out uses one exhaustive destination match; each binding maps its Port Error into the Environment Error sum. |
| `PORT-HANDOFF` | Every Command has one mode-specific handoff point. Processing after handoff belongs to the destination Port. |
| `PORT-KEY` | An externally consequential Command carries an Application-owned stable business key sufficient to recognize repetition or uncertain effect. |

The compiler proves exhaustiveness and payload agreement, not semantic routing correctness or absence of wiring side effects. Correct one-to-one routing and Error mapping are trusted configuration obligations tested per Slot. A `Never` Command arm is discharged by exhaustive matching. Terminal Port state is recovered through user-owned handles captured before binding, never through the Engine.

## 4. Environment

```rust
trait Environment {
    type Event;
    type Command;
    type Error: Display;

    fn start(&mut self) -> Result<LogicalTime, Self::Error>;
    fn next_event(&mut self) -> Result<(Self::Event, LogicalTime), Self::Error>;
    fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error>;
    fn shutdown(self, mode: ShutdownMode) -> Result<(), Self::Error>;
}

enum ShutdownMode {
    Graceful,
    Abort,
}
```

The Application and Environment Event and Command types must match.

### Environment invariants

| ID | Invariant |
|---|---|
| `ENV-CALLS` | Only the Engine calls the Environment: `start` exactly once, serially interleaved `next_event` and `dispatch`, then consuming `shutdown` at most once. |
| `ENV-START` | `start(Err)` leaves no run-scoped activity and the Environment safe to drop. |
| `ENV-CANDIDATE` | `next_event` owns waiting, source selection, and stamping. A returned Event is consumed once and never retried or revoked; it becomes accepted only when `EventAccepted` flushes. |
| `ENV-DISPATCH` | `dispatch` attempts one handoff without waiting for future capacity. `Ok` commits; `Err` guarantees no handoff; the Engine never retries. |
| `ENV-SHUTDOWN` | Shutdown always quiesces and returns only when safe to drop, even on `Err`; an Error reports a previously retained failure or failure of the requested disposition, never failure to quiesce. |
| `ENV-LATCH` | The Environment retains only its first Port failure. A pre-commit observation fails the active operation; a post-commit failure cannot revoke it and is reported at the next fallible Environment boundary before that boundary's next Event or handoff commitment. |
| `ENV-INTEGRITY` | Successfully returned Events and handed-off Commands are never silently dropped, overwritten, coalesced, duplicated, retried, or revoked. |
| `ENV-BOUNDS` | Every operation preserves Environment-owned queue, channel, Port, thread, wakeup, time, and shutdown-work bounds. |
| `ENV-SEPARATION` | The Environment orchestrates Ports but owns no Port domain state and never invokes an Application handler. |

### Commitments and failure

| Operation | Commitment | Failure rule |
|---|---|---|
| `start` | Successful return freezes the start time and establishes run-scoped activity. | `Err` self-cleans and becomes `Environment(Start)` Fatal. |
| `next_event` | Return of `(Event, LogicalTime)` consumes one candidate. | A retained failure observed first returns `Err`; a failure observed after return is deferred. |
| `dispatch` | Mode-specific handoff point. | Pre-handoff failure returns `Err`; post-handoff failure leaves the current return `Ok` and is latched. |
| `shutdown(Graceful)` | Consuming call quiesces while applying configured graceful disposition. | `Err` may be the primary `Environment(ShutdownGraceful)` Fatal. |
| `shutdown(Abort)` | Consuming cleanup stops delivery and handoff without initiating further consequential work. | `Err` is secondary during Fatal finalization. |

Every concrete Environment defines and tests one linear order between first-failure publication and each operation's commitment, including startup, handoff, Event selection, and the transition out of Running. Graceful shutdown stops Event delivery, rejects new Commands, and resolves the configured disposition of handed-off Commands. Abort stops Event delivery and new handoffs. A retained post-handoff Port failure is returned by the next `dispatch`, `next_event`, or graceful `shutdown` before that operation makes another Event or handoff commitment. If Engine Fatal begins first, an unreported retained Port failure is discarded rather than promoted to a secondary Error; only an Error produced by Abort cleanup becomes `shutdown_error`.

### 4.1 Live Environment

```rust
trait LivePort<C: PortContract>: Send + 'static {
    type Error: Display + Send + 'static;
    fn run(self, ctx: LiveCtx<C>) -> Result<(), Self::Error>;
}
```

`LiveCtx` semantically provides Command reception, nonblocking inspection of pending Commands, typed Event offering through the Slot's frozen fan-in constructor, and direct lifecycle observation.

| ID | Live invariant |
|---|---|
| `LIVE-THREADS` | Each bound Port runs in one supervised thread and owns its native, protocol, and domain state. |
| `LIVE-LIFECYCLE` | Graceful and Abort signals are Context authority, not Events or Commands, and consume no queue or inbox capacity. |
| `LIVE-EVENTS` | Fan-in uses one bounded queue. Mapping precedes admission; offer is nonblocking with respect to future capacity, and full/disconnected is reported to the Port. |
| `LIVE-SELECT` | `next_event` waits without busy-spinning for either the first failure or an Event. One Environment-defined linear order resolves their race. |
| `LIVE-TIME` | The single acceptor stamps from one monotonic clock; checked conversion exhaustion is an Environment Error. |
| `LIVE-DISPATCH` | Each destination has one bounded inbox. Failure publication and nonblocking inbox admission are linearized: failure first commits nothing; admission first commits handoff and defers the failure. |
| `LIVE-SUPERVISION` | Port `Err`, Port panic, or unexpected completion while Running latches a typed failure and wakes `next_event`. |
| `LIVE-START` | `start` linearizes immediate Port failure against successful startup; on failure it signals and joins every thread already spawned before returning `Err`. |
| `LIVE-SHUTDOWN` | Shutdown closes Engine-facing admission, signals all Ports, and joins all threads; it continues after Error and retains the first. |

A full or disconnected Event queue never silently loses an Event. A Port may handle the reported offer failure; if it cannot progress, it returns an Error for the Environment to latch. A failure linearized before Event commitment preempts a queued candidate; one linearized afterward cannot revoke the returned candidate and appears at the next fallible boundary.

`run(Err)` latches the mapped Port Error. A Port panic is contained at the supervised boundary as typed `PortPanicked`. Unexpected successful completion while Running is premature closure; a finite source instead offers its application-defined terminal Event and waits for shutdown.

The transition out of Running is linearized with Port completion: completion before the transition is premature; completion after it is expected. Port blocking points must observe lifecycle state and cooperate with shutdown. Kavod promises no wall-clock shutdown deadline. Event and Command types, Port Errors, and values moved into Port threads are `Send + 'static`.

### 4.2 Simulated Environment

```rust
trait SimPort<C: PortContract> {
    type Error: Display;
    fn start(&mut self, ctx: &mut SimCtx<'_, C>) -> Result<(), Self::Error>;
    fn on_command(
        &mut self,
        command: C::Command,
        ctx: &mut SimCtx<'_, C>,
    ) -> Result<(), Self::Error>;
    fn step(&mut self, ctx: &mut SimCtx<'_, C>)
        -> Result<Option<C::Event>, Self::Error>;
    fn stop(&mut self) -> Result<(), Self::Error>;
}

impl<C: PortContract> SimCtx<'_, C> {
    fn now(&self) -> LogicalTime;
    fn set_next(&mut self, time: LogicalTime) -> Result<(), SimCtxError>;
    fn clear_next(&mut self);
}
```

| ID | Simulated invariant |
|---|---|
| `SIM-STATE` | Each Port owns all simulated domain state; the Environment provides no shared model, transaction, or rollback. |
| `SIM-START` | `start` fixes time, then starts Ports in frozen Slot order with `now` equal to that time; the first Error fails Environment startup. |
| `SIM-DISPATCH` | Dispatch synchronously routes to exactly one `on_command`; invocation begins handoff, and logical time does not advance. |
| `SIM-POSTFAIL` | `on_command(Err)` leaves handoff and mutations committed, latches the Error, and makes the current `dispatch` return `Ok`. |
| `SIM-WAKEUP` | Each Port has at most one Slot-local, revocable arm; `set_next` requires `time >= now`, is last-call-wins, and `clear_next` disarms. |
| `SIM-EVENT` | `next_event` checks the failure latch before selection. A wakeup is not an Event; selection advances `now` and clears the arm before `step`, and only `step(Some)` creates the returned candidate. |
| `SIM-ORDER` | Lowest time wins; equal times use deterministic round-robin in frozen Slot order, advancing after every selected `step`. |
| `SIM-STEPS` | Every `step`, including `Some`, consumes one `max_steps_per_event` unit; the bound is checked before selecting, advancing time, or clearing an arm for excess work. |
| `SIM-SHUTDOWN` | Shutdown calls every `stop` in frozen Slot order, continues after Error, and retains the first shutdown Error; Graceful and Abort dispositions coincide. |
| `SIM-COMPLETION` | No armed Port yields `SimQuiescent` Error. Normal completion is an application-defined terminal Event whose handler returns `Stop`. |

Commands and earlier equal-time turns may alter or cancel a later wakeup. `SimCtx` modifies only its Port's arm, and rejected `set_next` changes nothing. A `step(Err)` preserves advanced time, the cleared arm, and all Port mutations; there is no rollback. A `step(None)` checks the failure latch again, then continues selection under the same budget. Step exhaustion is an Environment Error under `EnvironmentOperation::NextEvent`.

Simulation has no concurrency. Port determinism, bounded work, and avoidance of hidden authority are trusted and tested obligations. Fixed-input replay wiring supplies a constructor for its terminal application Event.

## 5. Journal

The Journal is human-readable forensic evidence, not a crash-proof write-ahead log. It is JSON Lines over `std::io::Write`: one serde JSON object and one newline per record. Line order is the sequence. `schema_version` identifies the record schema of one build.

### Record protocol

| Record | Fields | Meaning |
|---|---|---|
| `RunStarted` | `schema_version`, `logical_time` | Accepts the start turn at index 0. |
| `EventAccepted` | `index`, `logical_time`, `event` | Accepts one External Event and precedes its handler. |
| `CommandsPrepared` | `index`, complete ordered `commands` | Records intent before the first handoff. Omitted for an empty batch. |
| `CommandsDispatched` | `index` | Confirms all prepared Commands were handed off. Omitted for an empty batch. |
| `StopRequested` | `index` | Records `Outcome::Stop` before graceful shutdown. |
| `TurnCompleted` | `index`, `outcome: Continue | Stop` | Completes one non-Fatal turn. |
| `Fatal` | `schema_version`, optional `index`, `kind`, `message` | Best-effort evidence of the primary Fatal cause. |

`RecordKind` is exactly the rows above. Record enums use serde's default externally tagged representation. `FatalKind` is `Application`, `Environment`, `Journal`, or `Core`. `RunStarted` and `Fatal` are the only possible first committed records, so every committed record sequence begins with a versioned record.

### Journal invariants

| ID | Invariant |
|---|---|
| `JRN-FORMAT` | Each record is encoded completely, then written with one newline and flushed. `max_record_bytes` bounds the JSON object and excludes the newline. |
| `JRN-ENCODE` | Encoding and bound checks complete in reusable bounded storage before any byte of that record reaches the sink. |
| `JRN-WRITE` | The partial-write loop permits at most one successful progress call per output byte plus one terminal call; it does not retry `Interrupted`; `Ok(0)` while bytes remain becomes `WriteZero`. |
| `JRN-COMMIT` | Only successful flush commits a record. Bytes after the last committed record are an uncertain suffix and are not records, even if they form complete JSONL lines. |
| `JRN-POISON` | A sink write or flush Error permanently poisons the Journal; no later explicit `write` or `flush`, including Fatal, may occur. |
| `JRN-EVIDENCE` | `EventAccepted` commits before handler invocation and `CommandsPrepared` before the first handoff. |
| `JRN-PAYLOAD` | Events and Commands use their Application-defined `Serialize` representations; primary Errors remain typed in `EngineExit` and only bounded `Display` text enters the Journal. |
| `JRN-FATAL` | Fatal text is rendered when the primary cause is established, deterministically truncated at a UTF-8 boundary, and buffered before abort cleanup. The Fatal record is attempted at most once after cleanup and only if unpoisoned. |
| `JRN-SINK` | A sink is fresh for one run or positioned after a newline. Persistence beyond successful `flush`, including power-loss durability, is outside the contract. |

The bounded encoder rejects excess bytes before extending storage. Encode or bound failure writes no bytes for that record and is Journal Fatal, but does not poison the sink. Sink `Interrupted`, zero progress, write Error, or flush Error is a sink failure and poisons it. The Journal writes directly from its complete record buffer without a second buffering layer. Writer destructor behavior is outside the contract.

Application `Serialize` and Application/Environment `Display` implementations are deterministic, side-effect-free, bounded, and nonpanicking trusted obligations. Lossy serialization is only evidence of fields it emits. Kavod's bounded JSON encoder rejects non-finite floats and map keys not representable as JSON strings instead of accepting serde_json's lossy float representation; either condition is an Encode failure. Map order must be stable.

Fatal text uses a separate buffer bounded by `max_fatal_message_bytes`. Construction proves that the maximally escaped message and largest Fatal envelope fit `max_record_bytes`; both buffers reserve full capacity before run activity. `fmt::Error` uses a fixed static fallback. Thus Fatal encoding is infallible after construction. Failure writing or flushing Fatal is secondary `journal_error` and never replaces the primary cause.

JSONL bytes alone cannot identify the committed prefix after sink failure. Replay or forensic consumption therefore requires a cleanly completed Journal or an externally trusted committed boundary; a bounded reader rejects a line beyond `max_record_bytes + 1` bytes before allocating the complete line.

## 6. Execution

```rust
impl<A, E, W> Engine<A, E, W>
where
    A: Application,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: std::io::Write,
{
    fn new(app: A, env: E, writer: W, config: EngineConfig)
        -> Result<Self, ConstructionError>;
    fn run(self) -> EngineExit<A::State, A::FatalReason, E::Error>;
}
```

### Execution invariants

| ID | Invariant |
|---|---|
| `EXEC-CONSTRUCT` | Construction reserves all Engine storage and validates layout and Fatal sizing before State creation or any Application or Environment call. Failure is `ConstructionError`, not Fatal. |
| `EXEC-START` | Runtime creates State once, calls `start`, commits `RunStarted`, then invokes `on_start` at index 0. |
| `EXEC-EVENT` | The Engine checks the turn bound, acquires one candidate, validates time, assigns a checked index, commits `EventAccepted`, then invokes `on_event` once. |
| `EXEC-HANDLER` | No handler runs before its acceptance record commits; no next Event is acquired before `TurnCompleted(Continue)` commits. |
| `EXEC-PRECEDENCE` | After normal handler return: overflow, Application Fatal, batch preparation and dispatch, then `Continue` or `Stop`. |
| `EXEC-DISPATCH` | A nonempty batch is recorded completely, then dispatched once in order. Error at position `k` preserves `[0,k)`, commits no Command at `k`, and discards the suffix. |
| `EXEC-CONTINUE` | After successful dispatch, `Continue` commits `TurnCompleted(Continue)` before another acquisition. |
| `EXEC-STOP` | After successful dispatch, `Stop` commits `StopRequested`, consumes graceful shutdown, commits `TurnCompleted(Stop)`, then returns `Stopped`. |

### Construction and startup

All Engine and Environment configuration bounds use nonzero types. Construction reserves the complete Command batch, Journal record buffer, and Fatal-message buffer and proves Fatal-envelope sizing. Checked layout or allocation failure invokes no Application or Environment method.

Runtime startup is ordered:

| Step | Action | Failure |
|---|---|---|
| 1 | Create initial State exactly once. | A panic is outside Engine outcomes. |
| 2 | Call `Environment::start`. | `Environment(Start)` Fatal; `start` already cleaned up, so no Abort call. |
| 3 | Write and flush `RunStarted`. | Journal Fatal; Abort the started Environment. |
| 4 | Establish current accepted index 0 and invoke `on_start`. | Process the ordinary turn result. |

Before `RunStarted` commits there is no current accepted index. No External Event is requested before the start turn completes.

### External Event

| Step | Action | Failure effect |
|---|---|---|
| 1 | Check accepted External Event count below `max_turns`. | `TurnBoundExceeded`; do not call `next_event`. |
| 2 | Call `next_event`. | `Environment(NextEvent)` Fatal. |
| 3 | Check candidate time against the previous accepted time. | `TimeRegression`; candidate remains consumed and no handler runs. |
| 4 | Assign next checked `EventIndex` and construct the envelope. | Index overflow is an internal invariant panic. |
| 5 | Write and flush `EventAccepted`. | Journal Fatal; candidate remains consumed but is not current. |
| 6 | Establish the index as current and invoke `on_event` once. | Process the ordinary turn result. |

`max_turns` counts accepted External Events and excludes the start turn.

### Turn result

| Order | Condition or action | Result |
|---|---|---|
| 1 | Context overflow marker is set. | `CommandBoundExceeded`; discard complete batch regardless of returned `Outcome`. |
| 2 | `Outcome::Fatal(reason)`. | Application Fatal; discard complete batch. |
| 3 | Nonempty batch: commit `CommandsPrepared`. | Journal failure dispatches nothing. |
| 4 | Dispatch each Command once in order. | Error at `k` is `Environment(Dispatch { position: k })`; preserve prefix and discard suffix. |
| 5 | Nonempty batch: commit `CommandsDispatched`. | Journal failure leaves every handoff real. |
| 6a | `Continue`: commit `TurnCompleted(Continue)`. | Only success permits another Event acquisition. |
| 6b | `Stop`: commit `StopRequested`. | Only success permits graceful shutdown. |
| 7b | Consume `shutdown(Graceful)`. | Error is primary `Environment(ShutdownGraceful)`; no second shutdown occurs. |
| 8b | Commit `TurnCompleted(Stop)`. | Failure is Journal Fatal after Environment shutdown. |
| 9b | Return `EngineExit::Stopped { state }`. | Graceful completion. |

`on_start` uses this same result protocol at index 0. `CommandsPrepared` plus a typed dispatch position identifies the exact successful prefix even if Fatal recording fails.

## 7. Fatal and Panic

```rust
enum FatalCause<AF, EE> {
    Application(AF),
    Environment { error: EE, operation: EnvironmentOperation },
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
    Encode { record: RecordKind, error: serde_json::Error },
    RecordBoundExceeded { record: RecordKind },
    Sink {
        record: RecordKind,
        operation: JournalOperation,
        error: std::io::Error,
    },
}

enum JournalOperation { Write, Flush }

enum CoreFailure {
    TimeRegression { previous: LogicalTime, offered: LogicalTime },
    TurnBoundExceeded,
    CommandBoundExceeded,
}

enum EngineExit<S, AF, EE> {
    Stopped { state: S },
    Fatal {
        state: S,
        cause: FatalCause<AF, EE>,
        shutdown_error: Option<EE>,
        journal_error: Option<JournalFailure>,
    },
}
```

### Fatal invariants

| ID | Invariant |
|---|---|
| `FAIL-PRIMARY` | The first failure observed by the Engine is the typed primary cause and is never replaced. |
| `FAIL-TYPED` | Application, Environment, serializer, and sink Errors remain typed while the Engine owns them; Journal text uses only bounded `Display`. |
| `FAIL-STOP` | Fatal immediately stops normal execution: no later handler, dispatch, Event acquisition, or graceful action begins. |
| `FAIL-FINALIZE` | After buffering Fatal text, an unconsumed started Environment receives consuming Abort cleanup; then an unpoisoned Journal receives at most one Fatal-record attempt. |
| `FAIL-SECONDARY` | Abort Error becomes `shutdown_error`; Fatal-record write/flush Error becomes `journal_error`; neither changes the primary cause. |
| `FAIL-INDEX` | Fatal index is `Some(i)` exactly when `i` is the current accepted turn established by committed `RunStarted` or `EventAccepted`; it is `None` before start acceptance. A failed candidate record leaves the previous current index unchanged. |
| `FAIL-EFFECTS` | State mutations, consumed candidates, Port mutations, handed-off prefixes, external effects, and committed Journal records remain real; Fatal performs no rollback. |

Fatal finalization is ordered:

```text
establish the primary cause and format its bounded message
-> stop normal execution
-> if the Environment started and was not consumed: shutdown(Abort)
-> if the Journal is unpoisoned: attempt Fatal once
-> return current State, typed primary cause, and secondary Errors
```

A Journal already poisoned by the primary failure receives no Fatal attempt; that typed Journal failure remains the primary cause. `FatalKind` maps directly from the four `FatalCause` variants.

### Failure classification

| Trigger | Classification | Retained commitment |
|---|---|---|
| Handler returns `Fatal` | Application | State mutations; no staged Commands |
| Context overflow | Core: `CommandBoundExceeded` | State mutations; no staged Commands |
| External turn limit reached | Core: `TurnBoundExceeded` | No candidate acquired |
| Candidate time regresses | Core: `TimeRegression` | Candidate consumed, not accepted |
| Normal Environment call returns `Err` | Environment with operation | Any earlier commitment only |
| Journal encode or bound failure | Journal with record | No bytes for failed record |
| Journal sink failure | Journal with record and write/flush operation | Uncertain suffix; Journal poisoned |
| Graceful shutdown returns `Err` | Environment: `ShutdownGraceful` | Environment consumed and quiesced |
| Abort or Fatal-record operation fails | Secondary | Primary unchanged |

### Panic boundaries

| ID | Source | Treatment |
|---|---|---|
| `PANIC-INTERNAL` | Kavod reaches a state made unreachable by prior validation, checked arithmetic, or lifecycle rules. | Immediate invariant panic; not an Engine outcome. |
| `PANIC-ENGINE` | Handler, simulated Port, serializer, formatter, writer, callback, destructor, or other user code panics on the Engine thread. | Engine semantic model ends; resuming the run after catch is unsupported. |
| `PANIC-GUARD` | Engine-thread unwinding after startup while the Engine still owns the Environment. | An Engine-owned guard invokes Abort cleanup. After consuming shutdown begins, that method owns its unwind safety and no second cleanup call is possible. |
| `PANIC-PORT` | A supervised live Port thread panics. | The boundary contains it as typed `PortPanicked`, wakes the Engine, and follows the Fatal path. |
| `PANIC-ABORT` | Build uses `panic = "abort"`. | Cleanup guards and Port-panic containment do not run; panic guarantees assume unwinding. |

User-defined handlers, Ports, serializers, formatters, writers, callbacks, and destructors are trusted to be bounded and nonpanicking.

## 8. Bounds and Configuration

```rust
struct EngineConfig {
    max_turns: NonZeroU64,
    max_commands_per_turn: NonZeroUsize,
    max_record_bytes: NonZeroUsize,
    max_fatal_message_bytes: NonZeroUsize,
}

enum ConstructionError {
    CommandStorage(TryReserveError),
    JournalRecordStorage(TryReserveError),
    FatalMessageStorage(TryReserveError),
    FatalMessageTooLarge,
}
```

| Owner | Bounds and storage | Exhaustion |
|---|---|---|
| Engine | External turns, per-turn Commands, Journal record bytes, Fatal-message bytes | Core Fatal, Journal Fatal, or pre-run `ConstructionError` as defined above |
| Live Environment | Port/thread count, Event queue, Command inboxes, failure storage, logical-time domain, shutdown work | Typed Environment Error |
| Simulated Environment | Port count, one wakeup per Port, equal-time cursor, logical-time domain, steps per Event | Typed Environment Error |
| Port | Domain containers, native buffers, identifiers, counters, local loops and shutdown work | Typed Port Error mapped into Environment Error |
| Value owner | Transitive memory owned by Application, Port, Environment, or sink values | Owner-defined bounded failure |

### Bound invariants

| ID | Invariant |
|---|---|
| `BOUND-DEFINITION` | Every concrete bound documents its unit, accounting owner, check point, and exhaustion Error. Zero is unrepresentable for configuration bounds. |
| `BOUND-BEFORE` | Capacity, layout, count, time, and identity checks occur before corrupting or partially admitting the next item. |
| `BOUND-STORAGE` | Every Kavod-managed container and buffer is bounded; allocation strategy is otherwise an implementation choice. |
| `BOUND-SIZING` | Configuration must size `max_record_bytes` for the largest complete batch the Application may stage under `max_commands_per_turn`; this is a trusted sizing obligation, not a construction proof. |
| `BOUND-LOOPS` | Kavod-owned active loops are bounded and nonrecursive: run by `max_turns`, dispatch by batch length, simulation by `max_steps_per_event`, and Environment work by owned budgets. |
| `BOUND-BLOCKING` | Blocking waits are not active loops and imply no elapsed-time bound or progress guarantee. |
| `BOUND-DELEGATED` | Work inside handlers, Ports, serializers, formatters, destructors, callbacks, and `Write` implementations is outside Kavod active-loop accounting. |
| `BOUND-INDEX` | `max_turns` may equal `u64::MAX`; the pre-acquisition check makes EventIndex overflow unreachable and therefore an invariant panic, not an Engine outcome. |

The run executes at most one start turn plus `max_turns` External turns. All arithmetic is checked before use.

## 9. Invariant Catalog and Verification

This catalog is a compact review index. Section-local tables contain the authoritative wording.

| Group | IDs | Summary |
|---|---|---|
| Core | `CORE-AUTHORITY`, `CORE-TURN`, `CORE-ORDER`, `CORE-TIME` | Single ownership, serial turns, checked Event order, one time authority |
| Core safety | `CORE-BOUNDS`, `CORE-ARITHMETIC`, `CORE-FAILURE`, `CORE-PANIC`, `CORE-EVIDENCE` | Finite resources, checked math, typed failure, internal panic, ordered evidence |
| Application | `APP-FROZEN`, `APP-STATE`, `APP-AUTHORITY`, `APP-EMIT`, `APP-OVERFLOW`, `APP-OUTCOME`, `APP-FUTURE` | Frozen configuration, State ownership, restricted authority, bounded staging, outcomes |
| Ports | `PORT-CONTRACT`, `PORT-STATE`, `PORT-SUMS`, `PORT-ROUTING`, `PORT-HANDOFF`, `PORT-KEY` | Typed Slots, Port-owned state, exhaustive wiring, mode-specific commitment, stable keys |
| Environment | `ENV-CALLS`, `ENV-START`, `ENV-CANDIDATE`, `ENV-DISPATCH`, `ENV-SHUTDOWN`, `ENV-LATCH`, `ENV-INTEGRITY`, `ENV-BOUNDS`, `ENV-SEPARATION` | Lifecycle, candidate and handoff commitments, first failure, quiescence, bounds |
| Live | `LIVE-THREADS`, `LIVE-LIFECYCLE`, `LIVE-EVENTS`, `LIVE-SELECT`, `LIVE-TIME`, `LIVE-DISPATCH`, `LIVE-SUPERVISION`, `LIVE-START`, `LIVE-SHUTDOWN` | Supervised threads, bounded queues, race ordering, handoff, cleanup |
| Simulated | `SIM-STATE`, `SIM-START`, `SIM-DISPATCH`, `SIM-POSTFAIL`, `SIM-WAKEUP`, `SIM-EVENT`, `SIM-ORDER`, `SIM-STEPS`, `SIM-SHUTDOWN`, `SIM-COMPLETION` | Port-owned model, synchronous handoff, wakeups, scheduling, budgets, completion |
| Journal | `JRN-FORMAT`, `JRN-ENCODE`, `JRN-WRITE`, `JRN-COMMIT`, `JRN-POISON`, `JRN-EVIDENCE`, `JRN-PAYLOAD`, `JRN-FATAL`, `JRN-SINK` | JSONL, bounded encoding, flush commitment, poison, evidence ordering, Fatal attempt |
| Execution | `EXEC-CONSTRUCT`, `EXEC-START`, `EXEC-EVENT`, `EXEC-HANDLER`, `EXEC-PRECEDENCE`, `EXEC-DISPATCH`, `EXEC-CONTINUE`, `EXEC-STOP` | Construction, startup, acceptance, handler and dispatch ordering, completion |
| Fatal | `FAIL-PRIMARY`, `FAIL-TYPED`, `FAIL-STOP`, `FAIL-FINALIZE`, `FAIL-SECONDARY`, `FAIL-INDEX`, `FAIL-EFFECTS` | Primary cause, finalization, secondary Errors, accepted index, retained effects |
| Panic | `PANIC-INTERNAL`, `PANIC-ENGINE`, `PANIC-GUARD`, `PANIC-PORT`, `PANIC-ABORT` | Internal assertions, user panic boundaries, unwind cleanup, Port containment |
| Bounds | `BOUND-DEFINITION`, `BOUND-BEFORE`, `BOUND-STORAGE`, `BOUND-SIZING`, `BOUND-LOOPS`, `BOUND-BLOCKING`, `BOUND-DELEGATED`, `BOUND-INDEX` | Ownership, prechecks, finite storage/work, blocking and delegated work |

### Verification matrix

Tests must cover each invariant at ordinary and boundary values, plus both sides of every commitment point.

| Area | Required verification |
|---|---|
| Core and Application | One handler per accepted Event; no overlapping State access; index and nondecreasing-time checks; all start outcomes; ordered staging; first and repeated overflow; overflow precedence and retained State. |
| Routing | Exhaustive fan-in, fan-out, and Error mapping per Slot; semantically correct destination; `ports!` variants and bytes equal hand-written sums. |
| Environment common | Exact call order; no retry or rollback; first-failure retention; pre/post-commit behavior; shutdown quiescence on success and Error. |
| Live | Inbox-admission handoff; queue full/disconnect/prior failure commit nothing; Event/failure race order; non-spinning wakeup; Port Error, panic, and premature closure; partial-start and shutdown joining. |
| Simulated | One synchronous destination; post-handoff Error deferral; wakeup replacement, cancellation, isolation, and time check; `Some` commitment; equal-time fairness; every-step accounting; quiescence; ordered stop. |
| Journal | Exact startup, Continue, Stop, and Fatal sequences; valid JSONL; encoding and size failures; partial, zero-progress, interrupted, and flush failures; uncertain suffix; poison forbids later calls; deterministic Fatal truncation and at-most-once finalization. |
| Execution and Fatal | Failure at every record boundary; exact dispatched prefix; no normal action after Fatal; accepted-index rules; primary and secondary typed Errors; retained State and commitments. |
| Bounds and determinism | Every bound below, at, and above its limit; construction failure before State or component calls; repeated same-mode traces produce equal bytes and typed exits; equal cross-mode traces produce equal protocol and `NormalizedExit`. |
