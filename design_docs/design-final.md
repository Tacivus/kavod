# Kavod Core Design — Final

> **Status:** Implementation-ready draft (supersedes v10)
> **Scope:** The deterministic Core shared by live and simulated execution
> **Priority:** The smallest robust design implementable in days, whose rules can be enforced and tested

Kavod Core is written under `#![forbid(unsafe_code)]`. Kavod is informed by NASA's Power of Ten, TigerBeetle's Tiger Style, and SQLite's defensive testing culture — influences, not compliance claims.

**Normativity.** This document has three tiers:

1. **Public API** — signatures in `Public API` subsections are normative: item names, type shapes, trait bounds, and variant sets are exact. Doc comments, formatting, and method receiver conventions are free.
2. **Semantics and invariants** — `Semantics` and `Invariants` subsections are normative, including every commitment-point and procedure table.
3. **Implementation** — `Implementation` subsections are normative about *ordering* and *observable procedure*; the mechanism chosen to realize them (data structures, synchronization primitives, encodings of internal state) is free wherever tiers 1–2 do not constrain it.

**Template.** Every subsystem section has the same shape: a one-paragraph purpose, `Public API`, `Semantics`, `Invariants`, `Implementation`. Invariants state only what the axioms cannot derive alone — where a commitment point sits, who owns a fact, which bounds exist. Implementation subsections state ordering and mechanism only; they cite invariant IDs and never restate them. Any question this document does not answer explicitly should be answerable from the axioms; if it is not, that is a defect in this document. The document obeys A1 about itself: each fact appears once, in its owner's section.

**Open sections.** Blocks marked `OPEN-n` are the only parts of this document not ready for implementation. Each is self-contained: it lists the decisions the section must make and the constraints already fixed elsewhere. Index:

| ID | Location | Subject |
|---|---|---|
| OPEN-1 | §5 | Live Environment construction and wiring: builder, Slot binding, `LiveCtx` final signatures, Error-sum composition, graceful disposition configuration |
| OPEN-2 | §6 | Simulated Environment construction and wiring: builder, Slot binding, `SimConfig`, Error-sum composition, fixed-input replay wiring |

## 1. Principles

Kavod is a deterministic application Core. One Engine owns one run: it accepts one ordered Event at a time, invokes one synchronous handler, hands off the handler's ordered Commands, records evidence of each step, and completes the turn before accepting another Event. The same frozen Application runs unchanged in a live Environment (threads, sockets, clocks) and a simulated one (replay, virtual time) because every Environment-facing fact crosses one narrow contract.

### 1.1 Axioms

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

### 1.2 Ownership map

| Component | Owns |
|---|---|
| Engine | The run: handler invocation, State handoff, Event indices, the turn protocol, the Command batch, the Journal record schema, Fatal classification. |
| Application | Pure transition logic; all run-varying mutable application data, inside State. |
| Environment | Topology, waiting, Event selection, logical time, routing, lifecycle, execution mode. |
| Port | All of its own mutable domain, protocol, and native state. |
| Journal | The write mechanism only: bounded encoding, one sink, poison state. |

### 1.3 Determinism

Within one concrete Environment type: the same executable build, frozen Application, initial State, configuration, accepted `(Event, LogicalTime)` trace, Environment-result trace, and Journal-sink-result trace produce the same handler calls, State transitions, ordered Command intent, Journal bytes, and typed `EngineExit`.

Across live and simulated Environments: equal Engine configuration and capacities and equal abstract traces — each Core-facing operation, its success-or-failure classification, and its commitment result, with concrete Error values erased — produce the same handler calls, State transitions, ordered Command intent, and Journal record sequence, and exits equal in every Core-owned discriminant: the `FatalCause` variant, `EnvironmentOperation` including dispatch position, `RecordKind`, the `JournalError` variant with its `SinkOperation`, and the `CoreFailure` variant. Serialized content of mode-specific Errors may differ by mode.

Concurrent live sources may race; the resolution of the race is explicit in the accepted trace, and the Core is deterministic conditional on it. Application behavior and serialization must not depend on hidden clocks, entropy, IO, environment variables, process-global mutable state, concurrent order, pointer identity, unstable iteration, or Environment mode.

### 1.4 Failure philosophy

`EngineExit` is the truth; the Journal is evidence. The typed primary cause always reaches `EngineExit`; the `Fatal` record is best-effort forensics.

Consequences of A3/A4 stated once, so no subsystem section restates them: State mutations, consumed candidates, Port mutations, handed-off prefixes, external effects, and committed records all remain real — Fatal performs no rollback — and a Journal poisoned by the primary failure receives no `Fatal` attempt at all, leaving that typed failure as the primary cause.

### 1.5 Panic boundaries

| ID | Source | Treatment |
|---|---|---|
| `PANIC-INTERNAL` | Kavod reaches a state its prior validation made unreachable. | Immediate invariant panic; not an Engine outcome (A8). |
| `PANIC-ENGINE` | User code — handler, simulated Port, serializer, writer, callback, destructor — panics on the Engine thread. | The Engine semantic model ends; resuming after a catch is unsupported. |
| `PANIC-GUARD` | Engine-thread unwinding while the Engine still owns a started Environment. | An Engine-owned guard invokes Abort cleanup; once a consuming `shutdown` has begun, that method owns its own unwind safety. |
| `PANIC-PORT` | A supervised live Port thread panics. | Contained at the Port boundary as typed `PortPanicked`; the ordinary Fatal path follows (`LIVE-SUPERVISION`). |
| `PANIC-ABORT` | The build uses `panic = "abort"`. | Guards and Port containment never run; the panic guarantees assume unwinding. |

### 1.6 Bounds accounting

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
| `BOUND-LOOPS` | Kavod-owned active loops are bounded and nonrecursive: the run by one start turn plus `max_turns`, dispatch by batch length, each Event acquisition's simulated progression by `max_steps_per_event`, Environment work by its owned budgets, Journal writing by record length. |
| `BOUND-BLOCKING` | Blocking waits are not active loops and imply no elapsed-time bound; work inside user-defined code — handlers, Ports, serializers, writers, callbacks, destructors — is outside Kavod's accounting and trusted to be bounded. |

## 2. Application

The Application is a pure transition function over its State, driven by the Engine. Handlers are user-implemented; Kavod owns the envelope, the index/time newtypes, and `Context`.

### 2.1 Public API

```rust
/// Transparent u64 JSON representations.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct EventIndex(/* u64, private */);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct LogicalTime(/* u64 nanoseconds, private */);

pub struct EventEnvelope<E> {
    pub index: EventIndex,
    pub logical_time: LogicalTime,
    pub event: E,
}

pub trait Application {
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

pub enum Outcome<F> {
    Continue,
    Stop,
    Fatal(F),
}

impl<'a, C> Context<'a, C> {
    /// The current accepted turn.
    pub fn index(&self) -> EventIndex;
    /// The current turn's accepted logical time (the start time at index 0).
    pub fn logical_time(&self) -> LogicalTime;
    /// Exact Commands the batch can still store; zero once the overflow
    /// marker is set.
    pub fn remaining(&self) -> usize;
    /// Infallible; transfers one immutable Command.
    pub fn emit(&mut self, command: C);
}
```

### 2.2 Semantics

An accepted External Event has one authoritative representation, the `EventEnvelope`. `EventIndex` is the accepted-turn number — 0 for the start turn, External Events from 1 — and is the sole accepted-Event order. `LogicalTime` is an opaque nanosecond count with an Environment-owned origin and stamping authority (§4); equal times are valid and ordered by index. Port-domain timestamps, such as exchange or receive time, are ordinary Event payload fields with no Core meaning.

Context exposes the immutable current index and logical time — including the accepted start time at index 0, so no synthetic "ready" Event exists. That staged Commands are never dropped, coalesced, duplicated, or reordered follows from A3; that State mutations survive a later failure follows from A3 as well.

### 2.3 Invariants

| ID | Invariant |
|---|---|
| `APP-FROZEN` | The Application, its deterministic configuration, and all Engine capacities are frozen before `run`. |
| `APP-STATE` | `initial_state` runs exactly once; all run-varying mutable application data resides in State. |
| `APP-AUTHORITY` | A handler receives State, its accepted Event, and Context — no Environment, Journal, external IO, clock, entropy, or concurrency authority. |
| `APP-EMIT` | `emit` is infallible and transfers one immutable Command; while capacity remains it appends in call order. |
| `APP-OVERFLOW` | The first over-bound emit stores nothing and sets an overflow marker; later emits store nothing. The marker's consequence is the turn result's first check (§8.4). |
| `APP-OUTCOME` | A handler returns exactly one `Outcome`; the effects of its three variants are defined by the turn-result protocol (§8.4). |
| `APP-FUTURE` | Work for a future turn returns through an External Event. |

### 2.4 Implementation

`Context` wraps Engine-owned batch storage: one `Vec<C>` whose full capacity is reserved at construction (§8.4, `ConstructionError::CommandStorage`) and reused every turn, plus one overflow flag cleared at each handler invocation.

| Step | `emit` procedure |
|---|---|
| 1 | Overflow flag set → return, storing nothing (`APP-OVERFLOW`). |
| 2 | `len == max_commands_per_turn` → set the overflow flag, store nothing. |
| 3 | Otherwise push; capacity was prereserved, so no allocation occurs mid-turn. |

`remaining()` is `max_commands_per_turn - len`, or 0 when the overflow flag is set. The newtypes' arithmetic (index increment, time comparison) lives in the Engine and is checked per A6.

## 3. Ports, Contracts, and Slots

A Port Contract pairs one Event protocol with one Command protocol. A runtime Port is one mode-specific implementation of one bound Slot.

### 3.1 Public API

```rust
pub trait PortContract {
    type Event: Serialize;
    type Command: Serialize;
}

/// Kavod-owned uninhabited type for absent directions.
pub enum Never {}

kavod::ports! {
    pub enum TradingEvent / TradingCommand {
        Primary:   MarketData,
        Secondary: MarketData,
        Execution: Execution,
        Timer:     Timer,
    }
}
```

### 3.2 Semantics

Every Contract is duplex; an absent direction uses `Never`, whose `Serialize` implementation matches an impossible value.

A Slot is one named use of a Contract. The Application uses one closed, source-qualified Event sum and one closed, destination-qualified Command sum whose variants are its Slots; distinct Slots of one Contract are distinct variants. `ports!` is declarative syntax sugar for the two paired enums; hand-written equivalents are supported and observationally identical. The macro generates no routing, topology, Engine behavior, or Environment behavior.

The compiler proves exhaustiveness and payload agreement, not that an arm selects the semantically correct Slot. Trusted, per-Slot-tested obligations (§10): correct one-to-one routing and Error mapping; and for an externally consequential Command, an Application-owned stable business key sufficient to recognize a repeated or uncertain external effect. A `Never` Command arm is discharged by matching the uninhabited value. Terminal Port state is recovered through user-owned handles captured before binding, never through the Engine.

### 3.3 Invariants

| ID | Invariant |
|---|---|
| `PORT-STATE` | A Port exclusively owns its mutable domain, protocol, and native state; wiring and the Environment never interpret it. |
| `PORT-SUMS` | The Slot-qualified Event and Command sums are closed and type-checked against their Contracts. |
| `PORT-ROUTING` | Fan-in is one frozen variant constructor per inhabited Event direction; fan-out is one hand-written exhaustive destination match; each binding maps its Port Error into the Environment Error sum. |
| `PORT-HANDOFF` | Every Command has one mode-specific handoff commitment point (§4); processing after handoff belongs to the destination Port. |

### 3.4 Implementation

`ports!` is a `macro_rules!` macro. Its complete expansion for the example above:

```rust
#[derive(::serde::Serialize)]
pub enum TradingEvent {
    Primary(<MarketData as ::kavod::PortContract>::Event),
    Secondary(<MarketData as ::kavod::PortContract>::Event),
    Execution(<Execution as ::kavod::PortContract>::Event),
    Timer(<Timer as ::kavod::PortContract>::Event),
}

#[derive(::serde::Serialize)]
pub enum TradingCommand {
    Primary(<MarketData as ::kavod::PortContract>::Command),
    Secondary(<MarketData as ::kavod::PortContract>::Command),
    Execution(<Execution as ::kavod::PortContract>::Command),
    Timer(<Timer as ::kavod::PortContract>::Command),
}
```

Nothing else is generated. Serde's default externally tagged representation applies. Generated derives use `::serde` paths, so consumers need a direct dependency named `serde`; hand-written equivalents may add further derives freely. `Never`'s `Serialize` implementation is `match *self {}`.

## 4. Environment

The Environment is the Core's single boundary to the outside: it owns waiting, Event selection, time stamping, Command routing, and lifecycle. This section is the contract; §5 and §6 are its two implementations.

### 4.1 Public API

```rust
pub trait Environment {
    type Event;
    type Command;
    type Error: Serialize;

    fn start(&mut self) -> Result<LogicalTime, Self::Error>;
    fn next_event(&mut self) -> Result<(Self::Event, LogicalTime), Self::Error>;
    fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error>;
    fn shutdown(self, mode: ShutdownMode) -> Result<(), Self::Error>;
}

pub enum ShutdownMode {
    Graceful,
    Abort,
}
```

### 4.2 Semantics

Commitment points (A3 applies on both sides of each):

| Operation | Commitment point | `Err` before commitment | After commitment |
|---|---|---|---|
| `start` | Successful return: start time frozen, run-scoped machinery live. | No run-scoped activity remains; the Environment is safe to drop. | — |
| `next_event` | Returning `(Event, LogicalTime)` consumes one candidate. | No candidate was consumed. | The candidate is never retried or revoked; it becomes *accepted* only when `EventAccepted` commits (§8.2). |
| `dispatch` | Mode-specific handoff point (`LIVE-DISPATCH`, `SIM-DISPATCH`); the attempt never waits for future capacity. | This Command was not handed off; the Engine does not retry it. | Port processing failure cannot revoke the handoff. |
| `shutdown` | The call itself: it consumes the Environment. | — | Always quiesces and returns safe-to-drop, even on `Err`; an Error never reports failure to quiesce. |

If Engine Fatal finalization begins while a latched Port failure is still unreported, Abort discards it: only an Error produced by Abort's own cleanup becomes `shutdown_error` (§8.4).

### 4.3 Invariants

| ID | Invariant |
|---|---|
| `ENV-CALLS` | Only the Engine calls the Environment, serially: `start` exactly once, then `next_event` and `dispatch` interleaved one at a time, then `shutdown` at most once. |
| `ENV-LATCH` | The Environment latches at most its first Port failure. Failure publication is linearized against each operation's commitment: observed before, the operation returns `Err`; observed after, the commitment stands and the latched failure is returned by the next `next_event` or `dispatch` call before that call's own commitment — or by a graceful `shutdown` as its `Err` after quiescing, in preference to Errors from shutdown's own work. |
| `ENV-TIME` | One Environment authority — the single Event acceptor — stamps `LogicalTime` on `start` and every `next_event`; the Engine validates nondecrease (§8.4). |
| `ENV-SHUTDOWN` | `Graceful` stops Event delivery, rejects new Commands, and resolves the configured disposition of already-handed-off Commands. `Abort` stops Event delivery and new handoffs and initiates no further externally consequential work. |
| `ENV-SEPARATION` | The Environment orchestrates Ports but owns no Port domain state and never invokes an Application handler. |
| `ENV-BOUNDS` | Every operation preserves the Environment's own configured bounds: queues, channels, Port and thread counts, wakeup storage, time domain, shutdown work. |

### 4.4 Implementation

Nothing to implement at this level. `ENV-CALLS` is enforced structurally, not defensively: the Engine owns the Environment by value, its run loop is serial (A2), and `shutdown` consumes `self`, making a second lifecycle call unrepresentable.

## 5. Live Environment

The live Environment runs each bound Port on its own supervised thread and bridges concurrent reality into the serial contract of §4.

### 5.1 Public API

```rust
pub trait LivePort<C: PortContract>: Send + 'static {
    type Error: Send + 'static;
    fn run(self, ctx: LiveCtx<C>) -> Result<(), Self::Error>;
}
```

`LiveCtx` semantics are normative; exact signatures are provisional pending OPEN-1:

```rust
impl<C: PortContract> LiveCtx<C> {
    /// Block until one Command arrives or a lifecycle signal is raised.
    pub fn recv(&mut self) -> PortInput<C::Command>;
    /// Nonblocking inspection of pending Commands / signals.
    pub fn try_recv(&mut self) -> Option<PortInput<C::Command>>;
    /// Offer one Event through the Slot's frozen fan-in constructor.
    /// Never waits for future capacity.
    pub fn offer(&mut self, event: C::Event) -> Result<(), OfferRejected>;
    /// Direct observation of lifecycle signaling.
    pub fn lifecycle(&self) -> Lifecycle;
}

pub enum PortInput<Cmd> { Command(Cmd), Graceful, Abort }
pub enum OfferRejected { Full, Closed }
pub enum Lifecycle { Running, Graceful, Abort }
```

### 5.2 Semantics

A finite Event source does not complete `run`; it offers its application-defined terminal Event and waits for shutdown. The transition out of Running is linearized with Port completion, so completion is unambiguously premature or expected. Port blocking points must observe lifecycle state and cooperate with shutdown; Kavod promises no wall-clock shutdown deadline (`BOUND-BLOCKING`).

A rejected offer (`Full` or `Closed`) is reported to the offering Port, which may recover or return an Error to latch.

### 5.3 Invariants

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

### 5.4 Implementation

One workable mechanism (tier 3 — replaceable wherever §5.3 holds): one bounded MPMC/MPSC channel for Event fan-in; one bounded SPSC inbox per destination Port; a supervisor-owned latch (`Mutex<Option<FailureRecord>>` + `Condvar`, or an equivalent channel) that both the fan-in wait and Port supervision can wake.

| Step | `start` procedure |
|---|---|
| 1 | Freeze Slot order and capacities; create queue, inboxes, latch, lifecycle cell. |
| 2 | Spawn one thread per bound Port in frozen Slot order; each thread runs the Port inside a panic-catching supervisor shell (`LIVE-SUPERVISION`). |
| 3 | Any spawn or setup failure: apply `LIVE-START`, return `Err`. |
| 4 | Stamp and freeze the start time from the monotonic clock (`LIVE-TIME`); return it. |

| Step | `next_event` procedure |
|---|---|
| 1 | Wait under `LIVE-SELECT`'s linear order for latch-or-Event. |
| 2 | Latch chosen → return its `Err` (this call commits nothing, `ENV-LATCH`). |
| 3 | Event chosen → dequeue one candidate, stamp `LogicalTime` (`LIVE-TIME`, `ENV-TIME`), return it — the dequeue is the consumption commitment (§4.2). |

| Step | `dispatch` procedure |
|---|---|
| 1 | Latch already published → return its `Err` before committing (`ENV-LATCH`). |
| 2 | Route via the wiring's exhaustive destination match (`PORT-ROUTING`). |
| 3 | Try one non-waiting admission to the destination inbox; full or closed → typed `Err`, nothing handed off. |
| 4 | Admission succeeded → `Ok` (`LIVE-DISPATCH`). |

Supervision shell per Port thread: run the Port; map `Err`, caught panic (`PortPanicked`), or completion-while-Running into a typed failure; publish it to the latch (first wins) and wake the select. `shutdown` follows `LIVE-SHUTDOWN`: publish the mode to the lifecycle cell, wake all Port blocking points, close admission, join threads in Slot order, collect the first Error under `ENV-LATCH` precedence.

> **OPEN-1 — Live construction and wiring (needs design).**
> Decisions this section must make:
> - The builder/registration API binding each Slot to one `LivePort` implementation, with per-inbox capacity and the fan-in queue capacity (all `NonZero*`, §1.6).
> - Where the frozen fan-in constructors and the hand-written fan-out match live and how the builder receives them (`PORT-ROUTING`).
> - Composition of the Environment `Error` sum: Kavod-owned variants (queue exhaustion, time-domain exhaustion, `PortPanicked`, premature closure) plus one mapped variant per Slot's Port Error.
> - Final `LiveCtx` signatures (freezing §5.1's provisional set), including how a `LiveCtx` is constructed against the chosen channel types.
> - The configuration surface for `ENV-SHUTDOWN`'s "configured disposition of already-handed-off Commands" under Graceful.
> - Thread naming/panic-hook conventions, if any.
> Constraints already fixed: every invariant in §5.3, the commitment table in §4.2, `Send + 'static` boundaries, frozen Slot order as the only ordering authority, and A6's nonzero bounds. The builder must freeze everything before `Engine::run` (`APP-FROZEN`).

## 6. Simulated Environment

The simulated Environment executes the same contract single-threaded under virtual time; Ports advance only when stepped.

### 6.1 Public API

```rust
pub trait SimPort<C: PortContract> {
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
    pub fn now(&self) -> LogicalTime;
    pub fn set_next(&mut self, time: LogicalTime) -> Result<(), SimCtxError>;
    pub fn clear_next(&mut self);
}

pub enum SimCtxError {
    /// `set_next` requires `time >= now`; rejection changes nothing.
    TimeBeforeNow { now: LogicalTime, requested: LogicalTime },
}
```

### 6.2 Semantics

Two consequences worth deriving once, to show the method: `on_command(Err)` is a failure *after* the handoff commitment, so by `ENV-LATCH` the mutations stand, the Error is latched, and the current `dispatch` returns `Ok`. Likewise `step(Err)` cannot roll back the advanced `now`, the cleared arm, or Port mutations — A3 forbids it. Commands and earlier equal-time turns may alter or cancel a later Port's wakeup before it fires; that is what "revocable" means.

Simulated processing is synchronous, so Graceful and Abort coincide and `stop` takes no mode. Port determinism, bounded work, and avoidance of hidden authority are trusted, repeatability-tested obligations (§10).

### 6.3 Invariants

| ID | Invariant |
|---|---|
| `SIM-STATE` | Each simulated Port owns all of its simulated domain state; the Environment has no shared model, no transactions, no rollback, and no concurrency. |
| `SIM-START` | `start` fixes the start time, then calls every Port's `start` in frozen Slot order with `now` equal to it; the first Error fails Environment startup. |
| `SIM-DISPATCH` | `dispatch` synchronously routes to exactly one Port's `on_command`; invocation is the handoff commitment, and `now` does not advance. |
| `SIM-WAKEUP` | Each Port has at most one revocable wakeup arm, modifiable only through its own `SimCtx`: `set_next` requires `time >= now` — violation is the `SimCtxError` rejection, which changes nothing — and is last-call-wins; `clear_next` disarms. An arm is not an Event. |
| `SIM-SELECT` | `next_event` checks the failure latch, then selects the armed Port with the lowest time — equal times by round-robin in frozen Slot order, the cursor advancing past the selected Port after every selected `step`, including one returning `None` — advances `now`, clears the arm, and calls `step`. Only `step(Some)` creates the returned candidate; `step(None)` continues selection; `step(Err)` returns that failure. |
| `SIM-STEPS` | Every `step` call, including one returning `Some`, consumes one unit of the configured `max_steps_per_event`; the budget is fresh for each `next_event` invocation, and `start`, `on_command`, and `stop` calls consume none of it. The check occurs before selecting, advancing time, or clearing an arm for work that would exceed it; exhaustion is an Environment Error under `NextEvent`. |
| `SIM-COMPLETION` | No armed Port is the `SimQuiescent` Environment Error, and every Environment Error is Fatal. Normal completion is an application-defined terminal Event whose handler returns `Stop`; fixed-input replay wiring therefore accepts a constructor for that Event. |
| `SIM-SHUTDOWN` | `shutdown` calls every Port's `stop` in frozen Slot order, continues past an Error, returning the first subject to `ENV-LATCH` precedence. |

### 6.4 Implementation

Environment state: `now`, per-Port `Option<LogicalTime>` arm, the round-robin cursor, the failure latch, and a steps-used counter reset at each `next_event` entry.

| Step | `start` procedure |
|---|---|
| 1 | Fix the start time (configured origin); set `now` to it. |
| 2 | Call each Port's `start` in frozen Slot order with a `SimCtx` for that Port. |
| 3 | First `Err` → map via the Slot's Error mapping (`PORT-ROUTING`) and fail startup (§4.2 `start` row). |

| Step | `dispatch` procedure |
|---|---|
| 1 | Latch set → return its `Err` before committing (`ENV-LATCH`). |
| 2 | Route via the exhaustive destination match to one Port; invoke `on_command` (`SIM-DISPATCH` — the invocation commits). |
| 3 | `Err` → latch it, return `Ok` (§6.2). `Ok` → return `Ok`. |

| Step | `next_event` selection loop |
|---|---|
| 1 | Latch set → return its `Err`. |
| 2 | No armed Port → `Err(SimQuiescent)` (`SIM-COMPLETION`). |
| 3 | Steps used `== max_steps_per_event` → budget-exhaustion `Err` (`SIM-STEPS` — checked before any selection work). |
| 4 | Select lowest-armed-time Port, ties by cursor round-robin in frozen Slot order (`SIM-SELECT`). |
| 5 | Advance `now` to the arm time; clear the arm; count one step; call `step`; advance the cursor past the selected Port. |
| 6 | `Ok(Some(event))` → map via the Slot's frozen fan-in constructor, return `(event, now)`. `Ok(None)` → go to 1. `Err` → map and return it. |

`shutdown` follows `SIM-SHUTDOWN` directly: iterate `stop` in Slot order, remember the first Error, prefer a latched unreported failure per `ENV-LATCH`.

> **OPEN-2 — Simulated construction and wiring (needs design).**
> Decisions this section must make:
> - The builder/registration API binding each Slot to one `SimPort` implementation in frozen Slot order.
> - `SimConfig`: the time origin and `max_steps_per_event` (`NonZero*`, §1.6), and where it lives relative to `EngineConfig`.
> - Composition of the Environment `Error` sum: Kavod-owned variants (`SimQuiescent`, step-budget exhaustion, time-domain exhaustion) plus one mapped variant per Slot's Port Error.
> - Fixed-input replay wiring: how a recorded/fixed Event trace is presented (a provided `SimPort`?), and the terminal-Event constructor required by `SIM-COMPLETION`.
> - Where the fan-in constructors and fan-out match are supplied (same question as OPEN-1; ideally one shared wiring answer).
> Constraints already fixed: every invariant in §6.3, the commitment table in §4.2, single-threaded synchronous execution, frozen Slot order, A6's nonzero bounds, `APP-FROZEN`.

## 7. Journal

The Journal is a policy-free bounded JSON Lines writer. It knows nothing about the Engine, records, or turns; the record schema is Engine-owned (§8). It is human-readable forensic evidence, not a crash-proof write-ahead log.

### 7.1 Public API

```rust
pub struct Journal<W: std::io::Write> {
    /* writer, one reusable bounded encode buffer, poison marker */
}

impl<W: std::io::Write> Journal<W> {
    /// Reserves the encode buffer up front.
    pub fn new(writer: W, max_record_bytes: NonZeroUsize)
        -> Result<Self, std::collections::TryReserveError>;
    /// Encode into bounded storage, write one line, flush.
    /// Precondition: not poisoned (§7.4).
    pub fn commit<R: Serialize>(&mut self, record: &R) -> Result<(), JournalError>;
    pub fn is_poisoned(&self) -> bool;
}

pub enum JournalError {
    Encode(serde_json::Error),
    BoundExceeded,
    Sink { operation: SinkOperation, error: std::io::Error },
}

pub enum SinkOperation { Write, Flush }

/// Forensics/test utility: bounded line reader for Journal output.
pub struct JournalReader<R: std::io::BufRead> { /* reader, bound */ }

impl<R: std::io::BufRead> JournalReader<R> {
    pub fn new(reader: R, max_record_bytes: NonZeroUsize) -> Self;
    /// One complete line without its newline; `None` at clean EOF.
    pub fn next_line(&mut self) -> Result<Option<Vec<u8>>, ReadError>;
}

pub enum ReadError {
    /// Rejected after max_record_bytes + 1 bytes without allocating the line.
    LineTooLong,
    /// Trailing bytes with no newline: an uncertain suffix, not a record.
    MissingTrailingNewline,
    Io(std::io::Error),
}
```

### 7.2 Semantics

Encode requirements on all payloads: `Serialize` implementations are deterministic, side-effect-free, bounded, and nonpanicking (trusted obligations, §10); map iteration order is stable; the bounded encoder rejects non-finite floats and map keys not representable as JSON strings as `Encode` failures. Lossy serialization is evidence only of the fields it emits.

Memory sinks (`Vec<u8>` via a user-owned shared handle) make tests and fault injection direct; `std::io::sink()` discards evidence but still pays encoding. Because JSONL bytes alone cannot identify the committed prefix after a sink failure, replay requires a cleanly completed Journal or an externally trusted committed boundary.

### 7.3 Invariants

| ID | Invariant |
|---|---|
| `JRN-FORMAT` | One record is one serde JSON object plus one newline; line order is the sequence. `max_record_bytes` bounds the encoded object and excludes the newline. |
| `JRN-ENCODE` | Encoding completes in the reusable bounded buffer — the encoder rejects excess bytes before extending it — before any byte of that record reaches the sink. `Encode` and `BoundExceeded` therefore write nothing and do not poison. |
| `JRN-COMMIT` | Only a successful flush commits a record. After a sink failure, bytes past the last committed record are an uncertain suffix and are not records, even if they form complete lines. |
| `JRN-POISON` | Any sink failure — a write or flush Error, zero progress (`Ok(0)` becomes `WriteZero`), or `Interrupted`, which is not retried — permanently poisons the Journal; no later explicit `write` or `flush` occurs through it. |
| `JRN-SINK` | `W: std::io::Write` is the whole persistence abstraction. A sink is fresh for one run or positioned immediately after a newline. Persistence beyond successful `flush`, including power-loss durability, is outside the contract; writer destructor behavior is too. |

### 7.4 Implementation

The bounded encoder is the reusable buffer behind an internal `std::io::Write` adapter that rejects any write which would exceed `max_record_bytes` *before* extending the buffer (`JRN-ENCODE`); `serde_json::to_writer` targets that adapter. serde_json itself surfaces non-finite floats and non-string map keys as errors, satisfying §7.2's rejection requirement as `Encode`.

| Step | `commit` procedure |
|---|---|
| 1 | Poisoned → precondition violation. The Engine never calls a poisoned Journal (§8.4); a standalone caller doing so is an invariant panic (`PANIC-INTERNAL`). |
| 2 | Clear the buffer; encode the record through the bounded adapter. Adapter rejection → `BoundExceeded`; serde failure → `Encode`. Nothing was written, nothing poisons (`JRN-ENCODE`). |
| 3 | Append the newline (excluded from the bound, `JRN-FORMAT`). |
| 4 | Write the buffer with a hand-rolled loop bounded by record length — not `write_all`, because `Interrupted` is not retried (`JRN-POISON`); `Ok(0)` becomes `WriteZero`. First failure → poison, `Sink { operation: Write, .. }`. |
| 5 | Flush. Failure → poison, `Sink { operation: Flush, .. }`. Success commits (`JRN-COMMIT`). |

The Journal writes directly from its complete record buffer, with no second buffering layer. `JournalReader::next_line` reads at most `max_record_bytes + 1` bytes per line before rejecting with `LineTooLong`, never allocating the oversized line.

## 8. Engine

The Engine owns the run: it drives startup, the turn protocol, the record protocol, and Fatal classification, and it is the Journal's only caller.

### 8.1 Public API

```rust
/// Identifies the record schema of one build.
pub const SCHEMA_VERSION: u32 = 1;

pub struct EngineConfig {
    pub max_turns: NonZeroU64,
    pub max_commands_per_turn: NonZeroUsize,
    pub max_record_bytes: NonZeroUsize,
}

pub enum ConstructionError {
    CommandStorage(TryReserveError),
    JournalRecordStorage(TryReserveError),
    /// The largest fallback Fatal record cannot fit max_record_bytes.
    RecordBoundTooSmall,
}

impl<A, E, W> Engine<A, E, W>
where
    A: Application,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: std::io::Write,
{
    pub fn new(app: A, env: E, writer: W, config: EngineConfig)
        -> Result<Self, ConstructionError>;
    pub fn run(self) -> EngineExit<A::State, A::FatalReason, E::Error>;
}

#[derive(Serialize)]
pub enum RecordKind {
    RunStarted,
    EventAccepted,
    CommandsPrepared,
    CommandsDispatched,
    StopRequested,
    TurnCompleted,
    Fatal,
}

pub enum EngineExit<S, AF, EE> {
    Stopped { state: S },
    Fatal {
        state: S,
        cause: FatalCause<AF, EE>,
        shutdown_error: Option<EE>,
        journal_error: Option<JournalError>,
    },
}

pub enum FatalCause<AF, EE> {
    Application(AF),
    Environment { error: EE, operation: EnvironmentOperation },
    Journal(JournalFailure),
    Core(CoreFailure),
}

pub enum EnvironmentOperation {
    Start,
    NextEvent,
    Dispatch { position: usize },
    ShutdownGraceful,
}

pub struct JournalFailure {
    pub record: RecordKind,
    pub error: JournalError,
}

pub enum CoreFailure {
    TimeRegression { previous: LogicalTime, offered: LogicalTime },
    TurnBoundExceeded,
    CommandBoundExceeded,
}
```

Failure-serialization adapters (usable by Applications and wirings for their own Error types):

```rust
/// Serializes any Display-only type in one line via collect_str.
pub struct DisplayText<T: std::fmt::Display>(pub T);

/// Owned structured mirror of std::io::Error: kind, optional OS code,
/// rendered text.
pub struct IoErrorRecord { /* ... */ }
impl From<&std::io::Error> for IoErrorRecord { /* ... */ }
```

### 8.2 Semantics: record protocol

`RecordKind` is the closed set of record names below. Records use serde's default externally tagged representation. `RunStarted` and `Fatal` are the only possible first committed records, so every nonempty Journal begins with a versioned record.

| Record | Fields | Committed | Evidences |
|---|---|---|---|
| `RunStarted` | `schema_version`, `logical_time` | Before `on_start`. | Acceptance of the start turn (index 0) at the start time. |
| `EventAccepted` | `index`, `logical_time`, `event` | Before `on_event`. | Acceptance of one External Event. |
| `CommandsPrepared` | `index`, ordered `commands` | Before the first handoff of a nonempty batch. | The complete Command intent of the turn. |
| `CommandsDispatched` | `index` | After the last handoff of a nonempty batch. | Every prepared Command was handed off. |
| `StopRequested` | `index` | After a `Stop` outcome, before graceful shutdown. | The Application requested shutdown. |
| `TurnCompleted` | `index`, `outcome` (`Continue`/`Stop`) | End of every non-Fatal turn. | The turn's outcome. |
| `Fatal` | `schema_version`, optional `index`, `cause` | During Fatal finalization, best effort. | The primary cause. |

Both `CommandsPrepared` and `CommandsDispatched` are omitted for an empty batch. These commit points are A5 in action: no handler runs before its acceptance record commits, no handoff precedes `CommandsPrepared`, and no next Event is acquired before `TurnCompleted(Continue)` commits. `CommandsPrepared` plus the typed dispatch position identifies the exact successful prefix even if the `Fatal` record is never written.

The concrete Rust types of the records are Engine-internal (tier 3); their serialized form per this table is normative. Every failure type that can appear in the `Fatal` record is `Serialize`: `Application::FatalReason` and `Environment::Error` by trait bound, `CoreFailure` and `JournalFailure` as Kavod-owned data. Kavod serializes the foreign Errors it owns — `std::io::Error`, `serde_json::Error` — through owned structured mirrors capturing the error kind, optional OS code, and rendered text; that text is exactly the mode-varying content the determinism contract already erases. Kavod requires `Display` nowhere; for user Error types, `Serialize` is strictly more general than `Display` via `DisplayText`. Serialized failure payloads carry the same trusted obligations as all payloads (§7.2).

### 8.3 Invariants

| ID | Invariant |
|---|---|
| `FAIL-FINALIZE` | Fatal finalization runs exactly once, in order: stop normal execution → if the Environment was started and not consumed, `shutdown(Abort)` → if the Journal is unpoisoned, attempt the `Fatal` record → return `EngineExit::Fatal`. No handler, dispatch, Event acquisition, or graceful action begins after Fatal (A2, A4). |
| `FAIL-SECONDARY` | An Abort Error becomes `shutdown_error`; a `Fatal`-record Error becomes `journal_error`, which therefore always concerns the `Fatal` record and needs no `RecordKind`. Neither replaces the primary cause (A4). |
| `FAIL-INDEX` | The `Fatal` record's `index` is `Some(i)` exactly when `i` is the current accepted turn established by a committed `RunStarted` or `EventAccepted`, and `None` before start acceptance. A consumed candidate whose acceptance record failed never becomes current. |
| `FAIL-RECORD` | The `Fatal` record is `Fatal { schema_version, index, cause }` with the cause serialized structurally. If encoding fails (`Encode` or `BoundExceeded` — no sink bytes were written), the Engine falls back once to the same record with the cause reduced to its variant name, which construction proved fits `max_record_bytes`; a fallback `Encode` failure is therefore a Kavod invariant panic (A8). At most one sink write-and-flush is attempted, and `journal_error` is the first Error observed during the attempt, even if the fallback subsequently commits. |
| `BOUND-SIZING` | `max_record_bytes` must accommodate the largest batch the Application can stage under `max_commands_per_turn`; this sizing is a trusted configuration obligation, not a construction proof. |
| `BOUND-INDEX` | `max_turns` may equal `u64::MAX`; the pre-acquisition turn check makes `EventIndex` overflow unreachable, so overflow is an invariant panic, not an Engine outcome. |

### 8.4 Implementation

**Construction** (`Engine::new`) — before State creation, invoking no Application or Environment method; failure is `ConstructionError`, never a runtime Fatal:

| Step | Action | On failure |
|---|---|---|
| 1 | `try_reserve` the complete Command batch for `max_commands_per_turn`. | `CommandStorage`. |
| 2 | Construct the Journal, reserving its encode buffer for `max_record_bytes`. | `JournalRecordStorage`. |
| 3 | Encode the maximal fallback `Fatal` record — `index: Some(u64::MAX)`, the longest `FatalCause` variant name — and check it fits `max_record_bytes` (backs `FAIL-RECORD`'s panic claim). | `RecordBoundTooSmall`. |

**Startup:**

| Step | Action | On failure |
|---|---|---|
| 1 | Create initial State exactly once (`APP-STATE`). | A panic is outside Engine outcomes (A8). |
| 2 | `Environment::start`. | `Environment(Start)` Fatal; `start` already cleaned up (§4.2), so finalization skips Abort. |
| 3 | Commit `RunStarted`. | Journal Fatal. |
| 4 | Index 0 becomes current; invoke `on_start`; process the turn result. | — |

**External Event acquisition** (repeated while turns end in `Continue`):

| Step | Action | On failure |
|---|---|---|
| 1 | Check accepted External Event count `< max_turns`. | Core Fatal `TurnBoundExceeded`; `next_event` is not called. |
| 2 | `Environment::next_event`. | `Environment(NextEvent)` Fatal. |
| 3 | Validate candidate time `>=` the last accepted time (`ENV-TIME`). | Core Fatal `TimeRegression`; the candidate stays consumed, no handler runs. |
| 4 | Assign the next checked `EventIndex`; build the envelope. | Overflow is unreachable (`BOUND-INDEX`) and would be an invariant panic. |
| 5 | Commit `EventAccepted`. | Journal Fatal; the candidate stays consumed but never becomes current (`FAIL-INDEX`). |
| 6 | The index becomes current; invoke `on_event` exactly once; process the turn result. | — |

`max_turns` counts accepted External Events, excluding the start turn. It exists to bound Event/Command feedback loops, including one advancing the index forever at a single time.

**Turn result** — processed in this order after normal handler return (`on_start` uses the same protocol at index 0):

| Order | Condition or action | Effect |
|---|---|---|
| 1 | Overflow marker set (`APP-OVERFLOW`). | Core Fatal `CommandBoundExceeded`; discard the whole batch regardless of the returned `Outcome`. |
| 2 | `Outcome::Fatal(reason)`. | Application Fatal; discard the batch. |
| 3 | Nonempty batch: commit `CommandsPrepared`. | Journal failure dispatches nothing. |
| 4 | Dispatch each Command once, in order. | `Err` at position `k` is `Environment(Dispatch { position: k })`: the prefix `[0, k)` stands, the Command at `k` was not handed off, the suffix is discarded. |
| 5 | Nonempty batch: commit `CommandsDispatched`. | Journal failure leaves every handoff real. |
| 6a | `Continue`: commit `TurnCompleted(Continue)`. | Only success permits the next Event acquisition. |
| 6b | `Stop`: commit `StopRequested`. | Journal failure precedes shutdown. |
| 7b | `shutdown(Graceful)` (consumes the Environment). | `Err` is primary `Environment(ShutdownGraceful)`; no second shutdown call is possible. |
| 8b | Commit `TurnCompleted(Stop)`. | Journal Fatal; the Environment is already consumed, so finalization skips Abort. |
| 9b | Return `EngineExit::Stopped { state }`. | — |

**Fatal finalization** (the procedure behind `FAIL-FINALIZE`):

| Step | Action | Notes |
|---|---|---|
| 1 | Stop normal execution; fix the primary cause (A4). | |
| 2 | Environment started and not consumed → `shutdown(Abort)`; its `Err` → `shutdown_error` (`FAIL-SECONDARY`). | Skipped after a `start` failure or any consuming shutdown. |
| 3 | Journal unpoisoned → attempt the `Fatal` record per the procedure below; first observed Error → `journal_error`. | Skipped entirely if the primary failure poisoned the Journal (§1.4). |
| 4 | Return `EngineExit::Fatal { state, cause, shutdown_error, journal_error }`. | State always exists here — it is created before any fallible run step. |

**`Fatal` record attempt** (`FAIL-RECORD` mechanics): encode the full structural record; on `Encode`/`BoundExceeded` (nothing reached the sink), encode the fallback with the cause reduced to its variant name — a fallback encode failure is `PANIC-INTERNAL`, construction step 3 proved it fits. Then make at most one sink write-and-flush attempt with whichever record encoded. `journal_error` is the first Error observed across the whole attempt, even if the fallback subsequently commits.

## 9. Crate layout

One crate, `kavod`, no feature gates — both Environments are std-only. Dependencies: `serde` (with `derive`), `serde_json`. `ports!` is `macro_rules!`, so no proc-macro crate exists. This section is tier 3 except the public item names, which §§2–8 own.

```
kavod/src/
  lib.rs         #![forbid(unsafe_code)]; public re-exports
  time.rs        EventIndex, LogicalTime
  application.rs Application, Outcome, Context, EventEnvelope
  port.rs        PortContract, Never, ports!
  environment.rs Environment, ShutdownMode
  live/          LivePort, LiveCtx, live Environment (OPEN-1)
  sim/           SimPort, SimCtx, simulated Environment (OPEN-2)
  journal.rs     Journal, JournalError, JournalReader
  engine.rs      Engine, EngineConfig, records, EngineExit, FatalCause
  serialize.rs   DisplayText, IoErrorRecord, foreign-Error mirrors
```

## 10. Obligations and verification

Kavod enforces its invariants; everything below is trusted — upheld by a named party and checked by the stated means, not by the Engine at runtime. This table is the complete boundary; an obligation not listed here is not trusted, it is enforced.

| Obligation | Upholder | Verified by |
|---|---|---|
| Handlers avoid hidden authority: clocks, entropy, IO, globals, concurrency, mode (§1.3) | Application author | Simulated repeatability tests: same trace twice → identical Journal bytes and exit |
| Handlers, serializers, writers, callbacks, destructors are bounded and nonpanicking | Their authors | Review; §1.5 defines the blast radius when violated |
| One-to-one Slot routing and Error mapping (`PORT-ROUTING`) | Wiring author | Per-Slot tests |
| Stable business key on externally consequential Commands (§3.2) | Application author | Per-Slot tests recognizing repeated or uncertain external effects |
| `Serialize` impls deterministic, side-effect-free, bounded, nonpanicking; stable map order (§7.2) | Payload authors | Golden-Journal tests |
| Simulated Port determinism and bounded `step` work (§6.2) | Sim Port author | Repeatability tests |
| Live Port blocking points observe lifecycle and cooperate with shutdown (`BOUND-BLOCKING`) | Live Port author | Shutdown tests under load |
| `BOUND-SIZING`: `max_record_bytes` fits the largest stageable batch | Deployment configuration | Config review; construction proves only the fallback `Fatal` record |
| Transitive memory bounds of owned values (§1.6) | Value owner | Owner-defined |

Kavod-side verification conventions: every invariant ID maps to at least one test named for it; Journal and sink failures are exercised through memory sinks and failing writers (§7.2); and the determinism contract (§1.3) is checked by running one conformance trace suite against both Environments and comparing every Core-owned discriminant.
