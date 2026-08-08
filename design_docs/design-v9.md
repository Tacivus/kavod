# Kavod Core Design v10

> **Status:** MVP semantic draft (supersedes v9)
> **Scope:** The deterministic Core shared by live and simulated execution
> **Priority:** The smallest robust design implementable in days, whose rules can be enforced and tested

Rust syntax is illustrative: APIs and storage are implementation choices unless this document gives them semantic meaning. Kavod Core is written under `#![forbid(unsafe_code)]`. Kavod is informed by NASA's Power of Ten, TigerBeetle's Tiger Style, and SQLite's defensive testing culture — influences, not compliance claims.

## 1. Thesis

Kavod is a deterministic application Core. One Engine owns one run: it accepts one ordered Event at a time, invokes one synchronous handler, hands off the handler's ordered Commands, records evidence of each step, and completes the turn before accepting another Event. The same frozen Application runs unchanged in a live Environment (threads, sockets, clocks) and a simulated one (replay, virtual time) because every Environment-facing fact crosses one narrow contract.

Everything else in this document is a consequence of eight axioms:

| # | Axiom | Statement |
|---|---|---|
| A1 | Single authority | Every fact has exactly one owner and one representation. |
| A2 | Serial turns | One Event, one handler call, one Command batch at a time; a turn completes or the run becomes Fatal before the next Event is requested. |
| A3 | One commitment point | Every effectful operation has exactly one commitment point. Failure before it means nothing happened; after it, nothing is retried, revoked, or rolled back. |
| A4 | First failure wins | The first failure the Engine observes is the primary Fatal cause. Everything after is best-effort cleanup whose Errors are secondary and never replace it. |
| A5 | Evidence precedes effect | The Journal records intent before the irreversible action it evidences. |
| A6 | Bounded everything | Every Kavod-managed container, buffer, count, identifier, and active loop has one accounting owner and a bound checked before use. Arithmetic on counts, capacities, times, and identities is checked and never wraps or silently saturates. |
| A7 | Typed inside, rendered at the edge | Failures remain typed values while Kavod owns them. Text and bytes exist only at the serialization boundary. |
| A8 | Panics are Kavod bugs | A failure of a user component is a typed value on the Fatal path. A panic means a Kavod invariant was violated and is outside Engine outcomes. |

Each section below follows one template: purpose, contract, invariants, notes. Section invariants state only what the axioms cannot derive alone — where a commitment point sits, who owns a fact, which bounds exist. Any question this document does not answer explicitly should be answerable from the axioms; if it is not, that is a defect in this document. The document obeys A1 about itself: each fact appears once, in its owner's section; contracts, tables, and notes are normative, derivation remarks are motivation.

### 1.1 Ownership map

| Component | Owns |
|---|---|
| Engine | The run: handler invocation, State handoff, Event indices, the turn protocol, the Command batch, the Journal record schema, Fatal classification. |
| Application | Pure transition logic; all run-varying mutable application data, inside State. |
| Environment | Topology, waiting, Event selection, logical time, routing, lifecycle, execution mode. |
| Port | All of its own mutable domain, protocol, and native state. |
| Journal | The write mechanism only: bounded encoding, one sink, poison state. |

### 1.2 Determinism

Within one concrete Environment type: the same executable build, frozen Application, initial State, configuration, accepted `(Event, LogicalTime)` trace, Environment-result trace, and Journal-sink-result trace produce the same handler calls, State transitions, ordered Command intent, Journal bytes, and typed `EngineExit`.

Across live and simulated Environments: equal Engine configuration and capacities and equal abstract traces — each Core-facing operation, its success-or-failure classification, and its commitment result, with concrete Error values erased — produce the same handler calls, State transitions, ordered Command intent, and Journal record sequence, and exits equal in every Core-owned discriminant: the `FatalCause` variant, `EnvironmentOperation` including dispatch position, `RecordKind`, the `JournalError` variant with its `SinkOperation`, and the `CoreFailure` variant. Serialized content of mode-specific Errors may differ by mode.

Concurrent live sources may race; the resolution of the race is explicit in the accepted trace, and the Core is deterministic conditional on it. Application behavior and serialization must not depend on hidden clocks, entropy, IO, environment variables, process-global mutable state, concurrent order, pointer identity, unstable iteration, or Environment mode.

## 2. Application

The Application is a pure transition function over its State, driven by the Engine.

An accepted External Event has one authoritative representation:

```rust
struct EventEnvelope<E> {
    index: EventIndex,
    logical_time: LogicalTime,
    event: E,
}
```

`EventIndex` and `LogicalTime` are `Serialize` newtypes with transparent `u64` JSON representations. `EventIndex` is the accepted-turn number — 0 for the start turn, External Events from 1 — and is the sole accepted-Event order. `LogicalTime` is an opaque nanosecond count with an Environment-owned origin and stamping authority (Section 4); equal times are valid and ordered by index. Port-domain timestamps, such as exchange or receive time, are ordinary Event payload fields with no Core meaning.

```rust
trait Application {
    type State;
    type Event: Serialize;
    type Command: Serialize;
    type FatalReason: Serialize;

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

| ID | Invariant |
|---|---|
| `APP-FROZEN` | The Application, its deterministic configuration, and all Engine capacities are frozen before `run`. |
| `APP-STATE` | `initial_state` runs exactly once; all run-varying mutable application data resides in State. |
| `APP-AUTHORITY` | A handler receives State, its accepted Event, and Context — no Environment, Journal, external IO, clock, entropy, or concurrency authority. |
| `APP-EMIT` | `emit` is infallible and transfers one immutable Command; while capacity remains it appends in call order. |
| `APP-OVERFLOW` | The first over-bound emit stores nothing and sets an overflow marker; later emits store nothing. The marker's consequence is the turn result's first check (Section 6.4). |
| `APP-OUTCOME` | A handler returns exactly one `Outcome`; the effects of its three variants are defined by the turn-result protocol (Section 6.4). |
| `APP-FUTURE` | Work for a future turn returns through an External Event. |

Context exposes the immutable current index and logical time — including the accepted start time at index 0, so no synthetic "ready" Event exists — and `remaining()`, the exact number of Commands the batch can still store (zero once the overflow marker is set). That staged Commands are never dropped, coalesced, duplicated, or reordered follows from A3; that State mutations survive a later failure follows from A3 as well.

## 3. Ports, Contracts, and Slots

A Port Contract pairs one Event protocol with one Command protocol. A runtime Port is one mode-specific implementation of one bound Slot.

```rust
trait PortContract {
    type Event: Serialize;
    type Command: Serialize;
}

enum Never {}
```

Every Contract is duplex; an absent direction uses Kavod's uninhabited `Never`, whose `Serialize` implementation matches an impossible value.

A Slot is one named use of a Contract. The Application uses one closed, source-qualified Event sum and one closed, destination-qualified Command sum whose variants are its Slots; distinct Slots of one Contract are distinct variants:

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

`ports!` is declarative syntax sugar for the two paired enums with matching variant names, Contract payload types, and serde's default externally tagged representation. Hand-written equivalents are supported and observationally identical. The macro generates no routing, topology, Engine behavior, or Environment behavior. Generated derives use `::serde`, so consumers need a direct dependency named `serde`.

| ID | Invariant |
|---|---|
| `PORT-STATE` | A Port exclusively owns its mutable domain, protocol, and native state; wiring and the Environment never interpret it. |
| `PORT-SUMS` | The Slot-qualified Event and Command sums are closed and type-checked against their Contracts. |
| `PORT-ROUTING` | Fan-in is one frozen variant constructor per inhabited Event direction; fan-out is one hand-written exhaustive destination match; each binding maps its Port Error into the Environment Error sum. |
| `PORT-HANDOFF` | Every Command has one mode-specific handoff commitment point (Section 4); processing after handoff belongs to the destination Port. |

The compiler proves exhaustiveness and payload agreement, not that an arm selects the semantically correct Slot. Trusted, per-Slot-tested obligations: correct one-to-one routing and Error mapping; and for an externally consequential Command, an Application-owned stable business key sufficient to recognize a repeated or uncertain external effect. A `Never` Command arm is discharged by matching the uninhabited value. Terminal Port state is recovered through user-owned handles captured before binding, never through the Engine.

## 4. Environment

The Environment is the Core's single boundary to the outside: it owns waiting, Event selection, time stamping, Command routing, and lifecycle.

```rust
trait Environment {
    type Event;
    type Command;
    type Error: Serialize;

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

`Engine<A, E>` requires equal Application and Environment Event and Command types.

Commitment points (A3 applies on both sides of each):

| Operation | Commitment point | `Err` before commitment | After commitment |
|---|---|---|---|
| `start` | Successful return: start time frozen, run-scoped machinery live. | No run-scoped activity remains; the Environment is safe to drop. | — |
| `next_event` | Returning `(Event, LogicalTime)` consumes one candidate. | No candidate was consumed. | The candidate is never retried or revoked; it becomes *accepted* only when `EventAccepted` commits (Section 6). |
| `dispatch` | Mode-specific handoff point (`LIVE-DISPATCH`, `SIM-DISPATCH`); the attempt never waits for future capacity. | This Command was not handed off; the Engine does not retry it. | Port processing failure cannot revoke the handoff. |
| `shutdown` | The call itself: it consumes the Environment. | — | Always quiesces and returns safe-to-drop, even on `Err`; an Error never reports failure to quiesce. |

| ID | Invariant |
|---|---|
| `ENV-CALLS` | Only the Engine calls the Environment, serially: `start` exactly once, then `next_event` and `dispatch` interleaved one at a time, then `shutdown` at most once. |
| `ENV-LATCH` | The Environment latches at most its first Port failure. Failure publication is linearized against each operation's commitment: observed before, the operation returns `Err`; observed after, the commitment stands and the latched failure is returned by the next `next_event` or `dispatch` call before that call's own commitment — or by a graceful `shutdown` as its `Err` after quiescing, in preference to Errors from shutdown's own work. |
| `ENV-TIME` | One Environment authority — the single Event acceptor — stamps `LogicalTime` on `start` and every `next_event`; the Engine validates nondecrease (Section 6.3). |
| `ENV-SHUTDOWN` | `Graceful` stops Event delivery, rejects new Commands, and resolves the configured disposition of already-handed-off Commands. `Abort` stops Event delivery and new handoffs and initiates no further externally consequential work. |
| `ENV-SEPARATION` | The Environment orchestrates Ports but owns no Port domain state and never invokes an Application handler. |
| `ENV-BOUNDS` | Every operation preserves the Environment's own configured bounds: queues, channels, Port and thread counts, wakeup storage, time domain, shutdown work. |

If Engine Fatal finalization begins while a latched Port failure is still unreported, Abort discards it: only an Error produced by Abort's own cleanup becomes `shutdown_error` (Section 7).

### 4.1 Live Environment

```rust
trait LivePort<C: PortContract>: Send + 'static {
    type Error: Send + 'static;
    fn run(self, ctx: LiveCtx<C>) -> Result<(), Self::Error>;
}
```

`LiveCtx` is an implementation choice; semantically it provides Command reception, nonblocking inspection of pending Commands, typed Event offering through the Slot's frozen fan-in constructor, and direct observation of lifecycle signaling.

| ID | Invariant |
|---|---|
| `LIVE-THREADS` | Each bound Port runs in one supervised thread and owns its native client and all domain and protocol state. Everything crossing a Port-thread boundary — values moved in, Commands in, offered Events out, Port Errors out — is `Send + 'static`. |
| `LIVE-EVENTS` | Event fan-in is one configured bounded queue. Mapping into the Application Event sum precedes admission. The offer never waits for future capacity; full or disconnected is reported to the offering Port, which may recover or return an Error to latch. |
| `LIVE-SELECT` | `next_event` waits, without busy-spinning, until the first-failure latch is set or one Event is available, under one Environment-defined linear order between the two. |
| `LIVE-TIME` | The single acceptor stamps from one monotonic clock, making regression structurally impossible in correct operation; monotonic-duration conversion is checked and exhaustion is an Environment Error. |
| `LIVE-DISPATCH` | Each destination Port owns one configured bounded Command inbox; one admission to it is the handoff commitment, linearized against failure publication per `ENV-LATCH`. |
| `LIVE-SUPERVISION` | Port `run(Err)`, a Port thread panic (contained at the boundary as typed `PortPanicked`), and unexpected `run` completion while Running (premature closure) each latch a typed failure and wake a blocked `next_event`. |
| `LIVE-LIFECYCLE` | Graceful and Abort signals are Context authority — not Events or Commands — and consume no queue or inbox capacity. |
| `LIVE-START` | A `start` failure after spawning some Port threads signals and joins them before returning `Err`. |
| `LIVE-SHUTDOWN` | `shutdown` publishes lifecycle state, closes Engine-facing admission, and joins every supervised thread; it continues past an Error, returning the first subject to `ENV-LATCH` precedence. |

A finite Event source does not complete `run`; it offers its application-defined terminal Event and waits for shutdown. The transition out of Running is linearized with Port completion, so completion is unambiguously premature or expected. Port blocking points must observe lifecycle state and cooperate with shutdown; Kavod promises no wall-clock shutdown deadline (`BOUND-BLOCKING`).

### 4.2 Simulated Environment

```rust
trait SimPort<C: PortContract> {
    type Error;
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

| ID | Invariant |
|---|---|
| `SIM-STATE` | Each simulated Port owns all of its simulated domain state; the Environment has no shared model, no transactions, no rollback, and no concurrency. |
| `SIM-START` | `start` fixes the start time, then calls every Port's `start` in frozen Slot order with `now` equal to it; the first Error fails Environment startup. |
| `SIM-DISPATCH` | `dispatch` synchronously routes to exactly one Port's `on_command`; invocation is the handoff commitment, and `now` does not advance. |
| `SIM-WAKEUP` | Each Port has at most one revocable wakeup arm, modifiable only through its own `SimCtx`: `set_next` requires `time >= now` — violation is the `SimCtxError` rejection, which changes nothing — and is last-call-wins; `clear_next` disarms. An arm is not an Event. |
| `SIM-SELECT` | `next_event` checks the failure latch, then selects the armed Port with the lowest time — equal times by round-robin in frozen Slot order, the cursor advancing past the selected Port after every selected `step`, including one returning `None` — advances `now`, clears the arm, and calls `step`. Only `step(Some)` creates the returned candidate; `step(None)` continues selection; `step(Err)` returns that failure. |
| `SIM-STEPS` | Every `step` call, including one returning `Some`, consumes one unit of the configured `max_steps_per_event`; the budget is fresh for each `next_event` invocation, and `start`, `on_command`, and `stop` calls consume none of it. The check occurs before selecting, advancing time, or clearing an arm for work that would exceed it; exhaustion is an Environment Error under `NextEvent`. |
| `SIM-COMPLETION` | No armed Port is the `SimQuiescent` Environment Error, and every Environment Error is Fatal. Normal completion is an application-defined terminal Event whose handler returns `Stop`; fixed-input replay wiring therefore accepts a constructor for that Event. |
| `SIM-SHUTDOWN` | `shutdown` calls every Port's `stop` in frozen Slot order, continues past an Error, returning the first subject to `ENV-LATCH` precedence. Simulated processing is synchronous, so Graceful and Abort coincide and `stop` takes no mode. |

Two consequences worth deriving once, to show the method: `on_command(Err)` is a failure *after* the handoff commitment, so by `ENV-LATCH` the mutations stand, the Error is latched, and the current `dispatch` returns `Ok`. Likewise `step(Err)` cannot roll back the advanced `now`, the cleared arm, or Port mutations — A3 forbids it. Commands and earlier equal-time turns may alter or cancel a later Port's wakeup before it fires; that is what "revocable" means. Port determinism, bounded work, and avoidance of hidden authority are trusted, repeatability-tested obligations.

## 5. Journal

The Journal is a policy-free bounded JSON Lines writer. It knows nothing about the Engine, records, or turns; the record schema is Engine-owned (Section 6). It is human-readable forensic evidence, not a crash-proof write-ahead log.

```rust
struct Journal<W: std::io::Write> {
    writer: W,
    // One reusable bounded encode buffer and a poison marker.
}

impl<W: std::io::Write> Journal<W> {
    /// Encode into bounded storage, write one line, flush.
    fn commit<R: Serialize>(&mut self, record: &R) -> Result<(), JournalError>;
}

enum JournalError {
    Encode(serde_json::Error),
    BoundExceeded,
    Sink { operation: SinkOperation, error: std::io::Error },
}

enum SinkOperation { Write, Flush }
```

| ID | Invariant |
|---|---|
| `JRN-FORMAT` | One record is one serde JSON object plus one newline; line order is the sequence. `max_record_bytes` bounds the encoded object and excludes the newline. |
| `JRN-ENCODE` | Encoding completes in the reusable bounded buffer — the encoder rejects excess bytes before extending it — before any byte of that record reaches the sink. `Encode` and `BoundExceeded` therefore write nothing and do not poison. |
| `JRN-COMMIT` | Only a successful flush commits a record. After a sink failure, bytes past the last committed record are an uncertain suffix and are not records, even if they form complete lines. |
| `JRN-POISON` | Any sink failure — a write or flush Error, zero progress (`Ok(0)` becomes `WriteZero`), or `Interrupted`, which is not retried — permanently poisons the Journal; no later explicit `write` or `flush` occurs through it. |
| `JRN-SINK` | `W: std::io::Write` is the whole persistence abstraction. A sink is fresh for one run or positioned immediately after a newline. Persistence beyond successful `flush`, including power-loss durability, is outside the contract; writer destructor behavior is too. |

The Journal writes directly from its complete record buffer, with no second buffering layer, and its partial-write loop is bounded by the record length. Encode requirements on all payloads: `Serialize` implementations are deterministic, side-effect-free, bounded, and nonpanicking (trusted obligations); map iteration order is stable; the bounded encoder rejects non-finite floats and map keys not representable as JSON strings as `Encode` failures. Lossy serialization is evidence only of the fields it emits.

Memory sinks (`Vec<u8>` via a user-owned shared handle) make tests and fault injection direct; `std::io::sink()` discards evidence but still pays encoding. A bounded reader rejects a line after `max_record_bytes + 1` bytes without allocating it; because JSONL bytes alone cannot identify the committed prefix after a sink failure, replay requires a cleanly completed Journal or an externally trusted committed boundary.

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

### 6.1 Record protocol

The Engine owns the record schema and is the Journal's only caller. `RecordKind` is the closed set of record names below. Records use serde's default externally tagged representation. `schema_version` identifies the record schema of one build; `RunStarted` and `Fatal` are the only possible first committed records, so every nonempty Journal begins with a versioned record.

| Record | Fields | Committed | Evidences |
|---|---|---|---|
| `RunStarted` | `schema_version`, `logical_time` | Before `on_start`. | Acceptance of the start turn (index 0) at the start time. |
| `EventAccepted` | `index`, `logical_time`, `event` | Before `on_event`. | Acceptance of one External Event. |
| `CommandsPrepared` | `index`, ordered `commands` | Before the first handoff of a nonempty batch. | The complete Command intent of the turn. |
| `CommandsDispatched` | `index` | After the last handoff of a nonempty batch. | Every prepared Command was handed off. |
| `StopRequested` | `index` | After a `Stop` outcome, before graceful shutdown. | The Application requested shutdown. |
| `TurnCompleted` | `index`, `outcome` (`Continue`/`Stop`) | End of every non-Fatal turn. | The turn's outcome. |
| `Fatal` | `schema_version`, optional `index`, `cause` | During Fatal finalization, best effort (Section 7). | The primary cause. |

Both `CommandsPrepared` and `CommandsDispatched` are omitted for an empty batch. These commit points are A5 in action: no handler runs before its acceptance record commits, no handoff precedes `CommandsPrepared`, and no next Event is acquired before `TurnCompleted(Continue)` commits.

### 6.2 Construction and startup

Construction reserves the complete Command batch and Journal encode buffer, and verifies the largest fallback `Fatal` record (Section 7) fits `max_record_bytes`. Failure is `ConstructionError` — before State creation, invoking no Application or Environment method — never a runtime Fatal.

| Step | Action | On failure |
|---|---|---|
| 1 | Create initial State exactly once. | A panic is outside Engine outcomes (A8). |
| 2 | `Environment::start`. | `Environment(Start)` Fatal; `start` already cleaned up, so finalization skips Abort. |
| 3 | Commit `RunStarted`. | Journal Fatal. |
| 4 | Index 0 becomes current; invoke `on_start`; process the turn result. | — |

### 6.3 External Event

| Step | Action | On failure |
|---|---|---|
| 1 | Check accepted External Event count `< max_turns`. | Core Fatal `TurnBoundExceeded`; `next_event` is not called. |
| 2 | `Environment::next_event`. | `Environment(NextEvent)` Fatal. |
| 3 | Validate candidate time against the last accepted time. | Core Fatal `TimeRegression`; the candidate stays consumed, no handler runs. |
| 4 | Assign the next checked `EventIndex`; build the envelope. | Overflow is unreachable (`BOUND-INDEX`) and would be an invariant panic. |
| 5 | Commit `EventAccepted`. | Journal Fatal; the candidate stays consumed but never becomes current. |
| 6 | The index becomes current; invoke `on_event` exactly once; process the turn result. | — |

`max_turns` counts accepted External Events, excluding the start turn. It exists to bound Event/Command feedback loops, including one advancing the index forever at a single time.

### 6.4 Turn result

Processed in this order after normal handler return (`on_start` uses the same protocol at index 0):

| Order | Condition or action | Effect |
|---|---|---|
| 1 | Overflow marker set. | Core Fatal `CommandBoundExceeded`; discard the whole batch regardless of the returned `Outcome`. |
| 2 | `Outcome::Fatal(reason)`. | Application Fatal; discard the batch. |
| 3 | Nonempty batch: commit `CommandsPrepared`. | Journal failure dispatches nothing. |
| 4 | Dispatch each Command once, in order. | `Err` at position `k` is `Environment(Dispatch { position: k })`: the prefix `[0, k)` stands, the Command at `k` was not handed off, the suffix is discarded. |
| 5 | Nonempty batch: commit `CommandsDispatched`. | Journal failure leaves every handoff real. |
| 6a | `Continue`: commit `TurnCompleted(Continue)`. | Only success permits the next Event acquisition. |
| 6b | `Stop`: commit `StopRequested`. | Journal failure precedes shutdown. |
| 7b | `shutdown(Graceful)` (consumes the Environment). | `Err` is primary `Environment(ShutdownGraceful)`; no second shutdown call is possible. |
| 8b | Commit `TurnCompleted(Stop)`. | Journal Fatal; the Environment is already consumed. |
| 9b | Return `EngineExit::Stopped { state }`. | — |

`CommandsPrepared` plus the typed dispatch position identifies the exact successful prefix even if the `Fatal` record is never written.

## 7. Failure

`EngineExit` is the truth; the Journal is evidence. The typed primary cause always reaches `EngineExit`; the `Fatal` record is best-effort forensics.

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

struct JournalFailure {
    record: RecordKind,
    error: JournalError,
}

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
        journal_error: Option<JournalError>,
    },
}
```

| ID | Invariant |
|---|---|
| `FAIL-FINALIZE` | Fatal finalization runs exactly once, in order: stop normal execution → if the Environment was started and not consumed, `shutdown(Abort)` → if the Journal is unpoisoned, attempt the `Fatal` record → return `EngineExit::Fatal`. No handler, dispatch, Event acquisition, or graceful action begins after Fatal (A2, A4). |
| `FAIL-SECONDARY` | An Abort Error becomes `shutdown_error`; a `Fatal`-record Error becomes `journal_error`, which therefore always concerns the `Fatal` record and needs no `RecordKind`. Neither replaces the primary cause (A4). |
| `FAIL-INDEX` | The `Fatal` record's `index` is `Some(i)` exactly when `i` is the current accepted turn established by a committed `RunStarted` or `EventAccepted`, and `None` before start acceptance. A consumed candidate whose acceptance record failed never becomes current. |
| `FAIL-RECORD` | The `Fatal` record is `Fatal { schema_version, index, cause }` with the cause serialized structurally. If encoding fails (`Encode` or `BoundExceeded` — no sink bytes were written), the Engine falls back once to the same record with the cause reduced to its variant name, which construction proved fits `max_record_bytes`; a fallback `Encode` failure is therefore a Kavod invariant panic (A8). At most one sink write-and-flush is attempted, and `journal_error` is the first Error observed during the attempt, even if the fallback subsequently commits. |

Consequences of A3/A4 stated once: State mutations, consumed candidates, Port mutations, handed-off prefixes, external effects, and committed records all remain real — Fatal performs no rollback — and a Journal poisoned by the primary failure receives no `Fatal` attempt at all, leaving that typed failure as the primary cause.

### 7.1 Serialization of failures

Every failure type that can appear in the `Fatal` record is `Serialize`: `Application::FatalReason` and `Environment::Error` by trait bound, `CoreFailure` and `JournalFailure` as Kavod-owned data. Kavod serializes the foreign Errors it owns — `std::io::Error`, `serde_json::Error` — through owned structured mirrors capturing the error kind, optional OS code, and rendered text; that text is exactly the mode-varying content the determinism contract already erases. Kavod requires `Display` nowhere.

For user Error types, `Serialize` is strictly more general than `Display`: any `Display`-only type serializes in one line via `Serializer::collect_str`, and Kavod provides a `DisplayText<T: Display>` adapter plus an `IoErrorRecord` mirror (`From<&std::io::Error>`) so wrapping is trivial. Serialized failure payloads carry the same trusted obligations as all payloads (Section 5).

### 7.2 Panic boundaries

| ID | Source | Treatment |
|---|---|---|
| `PANIC-INTERNAL` | Kavod reaches a state its prior validation made unreachable. | Immediate invariant panic; not an Engine outcome (A8). |
| `PANIC-ENGINE` | User code — handler, simulated Port, serializer, writer, callback, destructor — panics on the Engine thread. | The Engine semantic model ends; resuming after a catch is unsupported. |
| `PANIC-GUARD` | Engine-thread unwinding while the Engine still owns a started Environment. | An Engine-owned guard invokes Abort cleanup; once a consuming `shutdown` has begun, that method owns its own unwind safety. |
| `PANIC-PORT` | A supervised live Port thread panics. | Contained at the Port boundary as typed `PortPanicked`; the ordinary Fatal path follows (`LIVE-SUPERVISION`). |
| `PANIC-ABORT` | The build uses `panic = "abort"`. | Guards and Port containment never run; the panic guarantees assume unwinding. |

User-defined handlers, Ports, serializers, writers, callbacks, and destructors are trusted to be bounded and nonpanicking.

## 8. Bounds and Configuration

```rust
struct EngineConfig {
    max_turns: NonZeroU64,
    max_commands_per_turn: NonZeroUsize,
    max_record_bytes: NonZeroUsize,
}

enum ConstructionError {
    CommandStorage(TryReserveError),
    JournalRecordStorage(TryReserveError),
    /// The largest fallback Fatal record cannot fit max_record_bytes.
    RecordBoundTooSmall,
}
```

Every Kavod-owned configuration bound — `EngineConfig`'s fields and Environment-owned capacities alike — uses a nonzero type, making zero unrepresentable. Every bound has one accounting owner (A6):

| Owner | Bounds and storage | Exhaustion |
|---|---|---|
| Engine | External turns, per-turn Commands, record bytes | Core Fatal, Journal Fatal, or pre-run `ConstructionError` |
| Live Environment | Port and thread counts, Event queue, Command inboxes, failure latch, time domain, shutdown work | Typed Environment Error |
| Simulated Environment | Port count, one wakeup arm per Port, equal-time cursor, time domain, `max_steps_per_event` | Typed Environment Error |
| Port | Domain containers, native buffers, identifiers, counters, local loops, local shutdown work | Typed Port Error, mapped into the Environment Error sum |
| Value owner | Transitive memory owned by Application, Port, Environment, or sink values | Owner-defined bounded failure |

| ID | Invariant |
|---|---|
| `BOUND-SIZING` | `max_record_bytes` must accommodate the largest batch the Application can stage under `max_commands_per_turn`; this sizing is a trusted configuration obligation, not a construction proof. |
| `BOUND-LOOPS` | Kavod-owned active loops are bounded and nonrecursive: the run by one start turn plus `max_turns`, dispatch by batch length, each Event acquisition's simulated progression by `max_steps_per_event`, Environment work by its owned budgets, Journal writing by record length. |
| `BOUND-BLOCKING` | Blocking waits are not active loops and imply no elapsed-time bound; work inside user-defined code — handlers, Ports, serializers, writers, callbacks, destructors — is outside Kavod's accounting and trusted to be bounded. |
| `BOUND-INDEX` | `max_turns` may equal `u64::MAX`; the pre-acquisition turn check makes `EventIndex` overflow unreachable, so overflow is an invariant panic, not an Engine outcome. |

## 9. Verification

Three rules generate the test plan from this document:

1. **Every invariant ID has at least one test that cites it**, exercised at ordinary and boundary values — excepting `PANIC-ENGINE` and `PANIC-ABORT`, which define unsupported behavior rather than testable guarantees.
2. **Every commitment point is tested on both sides**: failure injected immediately before must leave nothing committed; failure injected immediately after must leave the commitment real and un-rolled-back.
3. **Every bound is tested below, at, and above its limit**, and every fallible boundary — Journal encode, bound, write, zero-progress, `Interrupted`, flush; every Environment operation; every Port callback — gets direct fault injection.

Obligations not derivable from a single invariant:

- `ports!` expansion equals the hand-written sums in Rust variants and serialized bytes.
- Per-Slot routing selects the semantically correct destination (the compiler cannot check this).
- Repeated equal traces: within one Environment type, equal Journal bytes and typed `EngineExit`s; across modes, equal record sequences and Core-owned exit discriminants (Section 1.2).
- Exact Journal record sequences for startup, `Continue`, `Stop`, and every Fatal boundary in Section 6's tables, including the `Fatal` fallback encoding path.
- Construction failure occurs before State creation and invokes no Application or Environment method.
- Live race linearizations: Event availability against first failure; Port completion against the transition out of Running.

## Appendix A. Deltas from v9

| v9 | v10 |
|---|---|
| Engineering thesis as influence table plus scattered principles | Eight axioms as the explicit derivation base; section invariants restricted to non-derivable facts |
| `Display` bounds on `FatalReason`, Environment and Port Errors; bounded Fatal message buffer, `max_fatal_message_bytes`, UTF-8 truncation, escaping-fits-envelope construction proof, `fmt::Error` fallback | `Serialize` bounds; structured `Fatal { cause }` record; one-tier fallback to the cause's variant name; `RecordBoundTooSmall` construction check; no `Display` requirement anywhere |
| Journal owns the `Record` enum and Engine record types | Journal is a policy-free bounded JSONL writer with generic `commit<R: Serialize>`; the Engine owns the record schema |
| `FatalKind`, `NormalizedExit`, `FatalClass`, `JournalFailureKind`, `CoreFailureKind` enums | Derived: the `Fatal` record's fallback uses the `FatalCause` variant name; cross-mode equality is stated over Core-owned discriminants in prose |
| `JournalFailure` variants embedding record kind per shape | `JournalFailure { record: RecordKind, error: JournalError }`: the Journal reports mechanism, the Engine adds record context |
| Invariant catalog (Section 9) duplicating section tables; verification matrix restating invariants | Three generative verification rules plus only non-derivable obligations |
