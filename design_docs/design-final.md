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
| OPEN-1 | §5 | Live Environment construction and wiring: builder, Slot binding, `LiveCtx` final signatures, Fatal-sum composition, `LiveConfig` |
| OPEN-2 | §6 | Simulated Environment construction and wiring: builder, Slot binding, `SimConfig`, Fatal-sum composition |

## 1. Principles

Kavod is a deterministic application Core. One Engine owns one run: it accepts one ordered Event at a time, invokes one synchronous handler, hands off the handler's ordered Commands, records evidence of each step, and completes the turn before accepting another Event. The same frozen Application runs unchanged in a live Environment (threads, sockets, clocks) and a simulated one (replay, virtual time) because every Environment-facing fact crosses one narrow contract.

### 1.1 Axioms

Everything else in this document is a consequence of eight axioms:

| # | Axiom | Statement |
|---|---|---|
| A1 | Single authority | Every fact has exactly one owner and one representation. |
| A2 | Serial turns | One Event, one handler call, one Command batch at a time; a turn completes or the run becomes Fatal before the next Event is requested. |
| A3 | One commitment point | Every effectful operation has exactly one commitment point. Failure before it means nothing happened; after it, nothing is retried, revoked, or rolled back. |
| A4 | First failure wins | The first failure the Engine observes is the primary Fatal cause. Everything after is best-effort cleanup whose Errors are discarded and can never replace it. |
| A5 | Evidence precedes effect | The Journal records intent before the irreversible action it evidences. |
| A6 | Bounded everything | Every Kavod-managed container, buffer, count, identifier, and active loop has one accounting owner and a bound checked before use. Arithmetic on counts, capacities, times, and identities is checked and never wraps or silently saturates. |
| A7 | Typed inside, rendered at the edge | Failures remain typed values while Kavod owns them. Text and bytes exist only at the serialization boundary. |
| A8 | Panics are bugs | A failure of a user component is a typed value on the Fatal path. A panic — in Kavod or user code — is a bug: the process aborts (§1.5) and no Engine outcome represents it. |

### 1.2 Ownership map

| Component | Owns |
|---|---|
| Engine | The run: handler invocation, State handoff, Event indices, the turn protocol, the Command batch, the Journal record schema, Fatal classification. |
| Application | Pure transition logic; all run-varying mutable application data, inside State. |
| Environment | Topology, waiting, Event selection, logical time, routing, lifecycle, execution mode. |
| Port | All of its own mutable domain, protocol, and native state. |
| Journal | The write mechanism only: bounded encoding, one sink, poison state. |

### 1.3 Determinism

Same inputs, same run. Within one concrete Environment type, the same build, frozen Application, initial State, configuration, and trace — every accepted `(Event, Timestamp)`, every Environment result, every Journal-sink result — reproduce the same handler calls, State transitions, ordered Command intent, Journal bytes, and typed `EngineExit`.

Across live and simulated Environments the guarantee is the same with concrete failure values erased: equal abstract traces produce equal handler calls, State transitions, Command intent, and Journal bytes — no failure value is ever serialized, so nothing mode-varying reaches the Journal — and exits equal in every Core-owned discriminant (`FatalCause` variant, `RecordKind`, `JournalError` variant and `SinkOperation`, `CoreFatal` including its payloads). Mode-specific failure content may differ only inside `EngineExit`.

Concurrent live sources may race; the accepted trace records the resolution, and the Core is deterministic conditional on it. Hidden authority is forbidden to Application code and serialization alike: no clocks, entropy, IO, environment variables, process globals, concurrency order, pointer identity, unstable iteration, or Environment-mode dependence.

### 1.4 Failure philosophy

`EngineExit` is the truth; the Journal is evidence. The typed primary cause always reaches `EngineExit`, and it is the run's only failure output: after the first failure the Engine performs best-effort cleanup whose Errors are discarded, writes nothing further to the Journal, and serializes no failure value anywhere — failures leave the Core only as typed values (A7).

Consequences of A3/A4 stated once, so no subsystem section restates them: State mutations, consumed candidates, Port mutations, handed-off prefixes, external effects, and committed records all remain real — Fatal performs no rollback.

### 1.5 Panics

Kavod builds with `panic = "abort"` and relies on unwinding nowhere: no `catch_unwind`, no unwind guards, no panic containment. A panic anywhere — Kavod or user code — terminates the process immediately. `EngineExit` is never produced for a panic; the evidence is the Journal's committed prefix, which flush-per-record keeps current through the last completed commitment (`JRN-COMMIT`).

A library cannot dictate its consumer's panic strategy, so `panic = "abort"` in the final binary's build profile is a trusted build obligation (§10). Kavod's own correctness does not depend on it — the Core never catches — but containment and cleanup this document does not promise must not be assumed under an unwinding build either.

User components never signal failure by panicking; failures are typed values (A7, A8). A failing handler returns `Outcome::Fatal`; a failing live Port returns `Err` from `run`, which supervision translates into the latched failure the Engine observes (`LIVE-SUPERVISION`); a failing simulated Port returns `Err` from the failing method. Panic is reserved for invariant violations — bugs.

### 1.6 Bounds accounting

Every Kavod-owned configuration bound — `EngineConfig`'s fields and Environment-owned capacities alike — uses a nonzero type, making zero unrepresentable. Every bound has one accounting owner (A6):

| Owner | Bounds and storage | Exhaustion |
|---|---|---|
| Engine | External turns, per-turn Commands, record bytes | Core Fatal, Journal Fatal, or pre-run `BuildError` |
| Live Environment | Event queue, per-Port Command inboxes, failure latch, time domain, shutdown deadline | Typed Environment Error |
| Simulated Environment | One wakeup arm per Port, equal-time cursor, time domain, `max_steps_per_event` | Typed Environment Error |

The Port and thread count is not a configured bound: it is fixed statically by Slot registration at construction and cannot change during a run. Bounds inside user code — Port domain containers, native buffers, and the transitive memory of Application, Port, or sink values — are outside Kavod's accounting entirely and are trusted obligations (`BOUND-BLOCKING`, §10); the only Kavod-owned per-Port storage is the Command inbox, and the live Environment owns it.

| ID | Invariant |
|---|---|
| `BOUND-LOOPS` | Kavod-owned active loops are bounded and nonrecursive: the run by one start turn plus `max_turns`, dispatch by batch length, each Event acquisition's simulated progression by `max_steps_per_event`, Environment work by its owned budgets, Journal writing by record length. |
| `BOUND-BLOCKING` | Blocking waits are not active loops and imply no elapsed-time bound; work inside user-defined code — handlers, Ports, serializers, writers, callbacks, destructors — is outside Kavod's accounting and trusted to be bounded. |

## 2. Application

The Application is a pure transition function over its State, driven by the Engine. Handlers are user-implemented; Kavod owns the index and time newtypes and `Context`.

### 2.1 Public API

```rust
/// Transparent u64 JSON representations.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct EventIndex(/* u64, private */);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Timestamp(/* u64 nanoseconds, private */);

impl Timestamp {
    /// Constructs a timestamp from a nanosecond count; the origin and meaning
    /// of the count are the stamping Environment's to define (§2.2).
    pub fn from_nanos(nanos: u64) -> Self;
    /// Returns a timestamp advanced by `elapsed`, or `None` if the duration
    /// cannot be represented in nanoseconds or the result would overflow.
    pub fn checked_add(self, elapsed: std::time::Duration) -> Option<Self>;
    pub fn as_nanos(&self) -> u64;
}

pub trait Application {
    type State;
    type Event: Serialize;
    type Command: Serialize;
    type Fatal;

    fn initial_state(&self) -> Self::State;

    fn on_start(
        &self,
        state: &mut Self::State,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Fatal>;

    fn on_event(
        &self,
        state: &mut Self::State,
        event: &Self::Event,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Fatal>;
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
    pub fn logical_time(&self) -> Timestamp;
    /// Exact Commands the batch can still store; zero once the overflow
    /// marker is set.
    pub fn remaining(&self) -> usize;
    /// Infallible; transfers one immutable Command.
    pub fn emit(&mut self, command: C);
}
```

### 2.2 Semantics

`EventIndex` is the accepted-turn number — 0 for the start turn, External Events from 1 — and is the sole accepted-Event order. `Timestamp` nanosecond count originating at unix with an Environment-owned origin and stamping authority (§4); equal times are valid and ordered by index. Port-domain timestamps, such as exchange or receive time, are ordinary Event payload fields with no Core meaning.

During a handler, `Context` is the single authority for the current turn's index and logical time (A1) — including the accepted start time at index 0, so no synthetic "ready" Event exists; the `EventAccepted` record evidences the same facts (§8.2). That staged Commands are never dropped, coalesced, duplicated, or reordered follows from A3; that State mutations survive a later failure follows from A3 as well.

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

`Context` wraps Engine-owned batch storage: one fixed-capacity buffer of `max_commands_per_turn` Commands, allocated once at construction (§8.4, `BuildError::CommandBuffer`) and reused every turn, plus one overflow marker cleared at each handler invocation. The buffer is never grown, so nothing in the turn loop allocates. Whether it is a capacity-reserved `Vec` (a push below reserved capacity does not allocate) or a fixed slab is tier 3.

| Step | `emit` procedure |
|---|---|
| 1 | Overflow marker set → store nothing (`APP-OVERFLOW`). |
| 2 | Buffer full → set the marker, store nothing; the turn result's first check picks it up (§8.4). |
| 3 | Otherwise append in call order (`APP-EMIT`). |

`remaining()` is capacity minus length, or 0 when the marker is set. `Timestamp::checked_add` lets a SimPort schedule a future wakeup from an Environment-supplied time; conversion and addition are checked per A6. Event-index increment and the Engine's validation of timestamp order remain Engine-owned and checked per A6.

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

kavod::ports!(
    pub enum Trading<Event = TradingEvent, Command = TradingCommand> {
        Primary(MarketData),
        Secondary(MarketData),
        Execution(Execution),
        Timer(Timer),
    }
);
```

### 3.2 Semantics

Every Contract is duplex; an absent direction uses `Never`, whose `Serialize` implementation matches an impossible value.

A Slot is one named use of a Contract. The Application uses one closed, source-qualified Event sum and one closed, destination-qualified Command sum whose variants are its Slots; distinct Slots of one Contract are distinct variants. `ports!` is declarative syntax sugar for the two paired enums; hand-written equivalents are supported and observationally identical. The macro generates no routing, topology, Engine behavior, or Environment behavior.

The compiler proves exhaustiveness and payload agreement, not that an arm selects the semantically correct Slot. Trusted, per-Slot-tested obligations (§10): correct one-to-one routing and Error mapping; and for an externally consequential Command, an Application-owned stable business key sufficient to recognize a repeated or uncertain external effect. A `Never` Command arm is discharged by matching the uninhabited value. Terminal Port state is recovered through user-owned handles captured before binding, never through the Engine; a handle is settled only after a quiesced shutdown — after a `ShutdownTimeout` or an Abort, a detached Port thread may still be running behind it (§5.2).

### 3.3 Invariants

| ID | Invariant |
|---|---|
| `PORT-STATE` | A Port exclusively owns its mutable domain, protocol, and native state; wiring and the Environment never interpret it. |
| `PORT-SUMS` | The Slot-qualified Event and Command sums are closed and type-checked against their Contracts. |
| `PORT-ROUTING` | Fan-in is one frozen variant constructor per inhabited Event direction; fan-out is one hand-written exhaustive destination match, each arm mapping its Port Error into the Environment Error sum. The compiler proves exhaustiveness and payload agreement only; that each arm names its semantically correct Slot and Error mapping is a trusted, tested obligation (§10). |
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
    type Error;

    fn start(&mut self) -> Result<Timestamp, Self::Error>;
    fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error>;
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
| `start` | Successful return: start time frozen, run-scoped machinery live. | No new run-scoped activity begins and the Environment is safe to drop; live cleanup detaches rather than joins (`LIVE-START`), so already-spawned threads may briefly outlive the call. | — |
| `next_event` | Returning `(Event, Timestamp)` consumes one candidate. | No candidate was consumed. | The candidate is never retried or revoked; it becomes *accepted* only when `EventAccepted` commits (§8.2). |
| `dispatch` | Mode-specific handoff point (`LIVE-DISPATCH`, `SIM-DISPATCH`); the attempt never waits for future capacity. | This Command was not handed off; the Engine does not retry it. | Port processing failure cannot revoke the handoff. |
| `shutdown` | The call itself: it consumes the Environment. | — | Always returns safe-to-drop. Graceful either quiesces or reports its failure to quiesce as a typed `Err` (`LIVE-SHUTDOWN` bounds the wait); Abort does not wait for quiescence. |

If Engine Fatal finalization begins while a latched Port failure is still unreported, Abort discards it, as it discards every Error of Abort's own cleanup: the primary cause is the only failure the run reports (§1.4).

### 4.3 Invariants

| ID | Invariant |
|---|---|
| `ENV-CALLS` | Only the Engine calls the Environment, serially: `start` exactly once, then `next_event` and `dispatch` interleaved one at a time, then `shutdown` at most once. |
| `ENV-LATCH` | The Environment latches at most its first Port failure. Failure publication is linearized against each operation's commitment: observed before, the operation returns `Err`; observed after, the commitment stands and the latched failure is returned by the next `next_event` or `dispatch` call before that call's own commitment — or by a graceful `shutdown` as its `Err` after its bounded shutdown work, in preference to every Error of that work, `ShutdownTimeout` included. |
| `ENV-TIME` | One Environment authority — the single Event acceptor — stamps `Timestamp` on `start` and every `next_event`; the Engine validates nondecrease (§8.4). |
| `ENV-SHUTDOWN` | `Graceful` stops Event delivery, rejects new Commands, and delivers the shutdown signal to each Port ahead of that Port's queued Commands; already-handed-off residue is the destination Port's to drain or abandon — Port authorship is the disposition, and no Environment knob exists. `Abort` stops Event delivery and new handoffs, initiates no further externally consequential work, and does not wait for Port threads. |
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

A finite Event source does not complete `run`; it offers its application-defined terminal Event and waits for shutdown. The transition out of Running is linearized with Port completion, so completion is unambiguously premature or expected. Port blocking points must observe lifecycle state and cooperate with shutdown. Kavod-owned blocking points wake on the shutdown signal; Port work itself remains unbounded and trusted (`BOUND-BLOCKING`), so graceful shutdown waits at most the configured shutdown deadline — a Port unfinished when it expires is the typed `ShutdownTimeout` failure, its thread detached, never joined, and left to die with the process. After a `ShutdownTimeout` or an Abort the process is condemned: in-process reclamation of a stuck thread is impossible and never attempted; the caller renders the exit and terminates, and the durable evidence is the Journal's committed prefix (§1.5's stance).

A rejected offer (`Full` or `Closed`) is reported to the offering Port, which may recover or return an Error to latch.

### 5.3 Invariants

| ID | Invariant |
|---|---|
| `LIVE-THREADS` | Each bound Port runs in one supervised thread and owns its native client and all domain and protocol state. Everything crossing a Port-thread boundary — values moved in, Commands in, offered Events out, Port Errors out — is `Send + 'static`. |
| `LIVE-EVENTS` | Event fan-in is one configured bounded queue. Mapping into the Application Event sum precedes admission. The offer never waits for future capacity; full or disconnected is reported to the offering Port, which may recover or return an Error to latch. |
| `LIVE-SELECT` | `next_event` waits, without busy-spinning, until the first-failure latch is set or one Event is available, under one Environment-defined linear order between the two. |
| `LIVE-TIME` | The single acceptor stamps from one monotonic clock, making regression structurally impossible in correct operation; monotonic-duration conversion is checked and exhaustion is an Environment Error. |
| `LIVE-DISPATCH` | Each destination Port owns one configured bounded Command inbox; one admission to it is the handoff commitment, linearized against failure publication per `ENV-LATCH`. |
| `LIVE-SUPERVISION` | Port `run(Err)` and unexpected `run` completion while Running (premature closure) each latch a typed failure and wake a blocked `next_event`. |
| `LIVE-LIFECYCLE` | Graceful and Abort signals are Context authority — not Events or Commands — and consume no queue or inbox capacity. |
| `LIVE-START` | A `start` failure after spawning some Port threads signals Abort and detaches them before returning `Err` — no wait, matching Abort's discipline. A Port failing immediately after spawn is not itself a `start` failure: `start` does not wait on or inspect the latch, so `RunStarted` and `on_start` may proceed normally, with the already-latched failure surfacing at the first subsequent `next_event` or `dispatch` call per `ENV-LATCH`. |
| `LIVE-SHUTDOWN` | `shutdown` publishes lifecycle state and closes Engine-facing admission. Graceful waits at most the configured shutdown deadline for every supervised thread to complete, joining finishers and detaching stragglers, continuing past Errors; its `Err` precedence: latched unreported Port failure (`ENV-LATCH`), then `ShutdownTimeout` naming the first unfinished Slot in frozen Slot order, then the first Error from shutdown's own work. Abort detaches every supervised thread and returns without waiting. |

### 5.4 Implementation

One workable mechanism (tier 3 — replaceable wherever §5.3 holds): one bounded MPMC/MPSC channel for Event fan-in; one bounded SPSC inbox per destination Port; a supervisor-owned latch (`Mutex<Option<FailureRecord>>` + `Condvar`, or an equivalent channel) that both the fan-in wait and Port supervision can wake.

| Step | `start` procedure |
|---|---|
| 1 | Freeze Slot order and capacities; create queue, inboxes, latch, lifecycle cell. |
| 2 | Spawn one thread per bound Port in frozen Slot order; each thread runs the Port inside a supervisor shell that publishes its completion (`LIVE-SUPERVISION`). |
| 3 | Any spawn or setup failure: apply `LIVE-START`, return `Err`. |
| 4 | Stamp and freeze the start time from the monotonic clock (`LIVE-TIME`); return it. |

| Step | `next_event` procedure |
|---|---|
| 1 | Wait under `LIVE-SELECT`'s linear order for latch-or-Event. |
| 2 | Latch chosen → return its `Err` (this call commits nothing, `ENV-LATCH`). |
| 3 | Event chosen → dequeue one candidate, stamp `Timestamp` (`LIVE-TIME`, `ENV-TIME`), return it — the dequeue is the consumption commitment (§4.2). |

| Step | `dispatch` procedure |
|---|---|
| 1 | Latch already published → return its `Err` before committing (`ENV-LATCH`). |
| 2 | Route via the wiring's exhaustive destination match (`PORT-ROUTING`). |
| 3 | Try one non-waiting admission to the destination inbox; full or closed → typed `Err`, nothing handed off. |
| 4 | Admission succeeded → `Ok` (`LIVE-DISPATCH`). |

Supervision shell per Port thread: run the Port; map `Err` or completion-while-Running into a typed failure; publish it to the latch (first wins) and wake the select. No separate watcher thread is needed — the shell runs on the Port's own thread and publishes as `run` returns. `shutdown` follows `LIVE-SHUTDOWN`: publish the mode to the lifecycle cell, wake all Port blocking points, close admission; under Graceful, wait on the supervision shells' completion signals with `Condvar::wait_timeout` against the monotonic clock (`JoinHandle` has no timed join), joining completed threads in Slot order and dropping stragglers' handles at the deadline; under Abort, drop all handles immediately.

> **OPEN-1 — Live construction and wiring (needs design).**
> Decisions this section must make:
> - The builder/registration API binding each Slot to one `LivePort` implementation, with per-inbox capacity and the fan-in queue capacity (all `NonZero*`, §1.6).
> - Where the frozen fan-in constructors and the hand-written fan-out match live and how the builder receives them (`PORT-ROUTING`).
> - Composition of the Environment `Error` sum: Kavod-owned variants (queue exhaustion, time-domain exhaustion, premature closure, `ShutdownTimeout` naming the first unfinished Slot) plus one mapped variant per Slot's Port Error.
> - Final `LiveCtx` signatures (freezing §5.1's provisional set), including how a `LiveCtx` is constructed against the chosen channel types.
> - `LiveConfig`'s shutdown deadline field (`NonZeroU64` milliseconds, §1.6), the Graceful wait bound of `LIVE-SHUTDOWN`. Command disposition needs no configuration — it is Port-owned (`ENV-SHUTDOWN`).
> - Thread naming conventions, if any.
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
    pub fn now(&self) -> Timestamp;
    pub fn set_next(&mut self, time: Timestamp) -> Result<(), SimCtxError>;
    pub fn clear_next(&mut self);
}

pub enum SimCtxError {
    /// `set_next` requires `time >= now`; rejection changes nothing.
    TimeBeforeNow { now: Timestamp, requested: Timestamp },
}
```

### 6.2 Semantics

Two consequences worth deriving once, to show the method: `on_command(Err)` is a failure *after* the handoff commitment, so by `ENV-LATCH` the mutations stand, the Error is latched, and the current `dispatch` returns `Ok`. Likewise `step(Err)` cannot roll back the advanced `now`, the cleared arm, or Port mutations — A3 forbids it. Commands and earlier equal-time turns may alter or cancel a later Port's wakeup before it fires; that is what "revocable" means.

Simulated processing is synchronous, so Graceful and Abort coincide and `stop` takes no mode. Kavod ships no built-in replay: a fixed or recorded input trace is presented by a user-written `SimPort` — or a bespoke `Environment` implementation, which `Timestamp::from_nanos` makes possible outside the crate — and the determinism guarantee (§1.3) is the counterfactual such wiring relies on. Port determinism, bounded work, and avoidance of hidden authority are trusted, repeatability-tested obligations (§10).

### 6.3 Invariants

| ID | Invariant |
|---|---|
| `SIM-STATE` | Each simulated Port owns all of its simulated domain state; the Environment has no shared model, no transactions, no rollback, and no concurrency. |
| `SIM-START` | `start` fixes the start time, then calls every Port's `start` in frozen Slot order with `now` equal to it; the first Error fails Environment startup. |
| `SIM-DISPATCH` | `dispatch` synchronously routes to exactly one Port's `on_command`; invocation is the handoff commitment, and `now` does not advance. |
| `SIM-WAKEUP` | Each Port has at most one revocable wakeup arm, modifiable only through its own `SimCtx`: `set_next` requires `time >= now` — violation is the `SimCtxError` rejection, which changes nothing — and is last-call-wins; `clear_next` disarms. An arm is not an Event. |
| `SIM-SELECT` | `next_event` checks the failure latch, then selects the armed Port with the lowest time — equal times by round-robin in frozen Slot order, the cursor advancing past the selected Port after every selected `step`, including one returning `None` — advances `now`, clears the arm, and calls `step`. Only `step(Some)` creates the returned candidate; `step(None)` continues selection; `step(Err)` returns that failure. |
| `SIM-STEPS` | Every `step` call, including one returning `Some`, consumes one unit of the configured `max_steps_per_event`; the budget is fresh for each `next_event` invocation, and `start`, `on_command`, and `stop` calls consume none of it. The check occurs before selecting, advancing time, or clearing an arm for work that would exceed it; exhaustion is an Environment Error under `NextEvent`. |
| `SIM-COMPLETION` | No armed Port is the `SimQuiescent` Environment Error. Normal completion is an application-defined terminal Event whose handler returns `Stop`. |
| `SIM-SHUTDOWN` | `shutdown` calls every Port's `stop` in frozen Slot order, continues past an Error, returning the first subject to `ENV-LATCH` precedence. |

### 6.4 Implementation

Environment state: `now`, per-Port `Option<Timestamp>` arm, the round-robin cursor, the failure latch, and a steps-used counter reset at each `next_event` entry.

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
        -> Result<Self, JournalBuildError>;
    /// Encode into bounded storage, write one line, flush.
    /// Precondition: not poisoned (§7.4).
    pub fn commit<R: Serialize>(&mut self, record: &R) -> Result<(), JournalError>;
    pub fn is_poisoned(&self) -> bool;
}

pub enum JournalBuildError {
    /// `max_record_bytes` leaves no room for the reserved newline byte.
    MaxBytesTooLarge,
    /// The reusable record buffer could not reserve its required storage.
    AllocationFailed,
}

pub enum JournalError {
    Encode(serde_json::Error),
    /// The payload serialized to something other than a JSON object.
    NotAnObject,
    BoundExceeded,
    Sink { operation: SinkOperation, error: std::io::Error },
}

pub enum SinkOperation { Write, Flush }
```

### 7.2 Semantics

Terminology: the *sink* is the `W: std::io::Write` value the Journal writes into — a file, an in-memory `Vec<u8>`, a socket; the Journal neither knows nor cares which. *Poisoned* is the Journal's permanent post-sink-failure state: after one failed write or flush, the sink's tail is unknowable (`JRN-COMMIT`), so appending more bytes could only corrupt evidence, and the Journal refuses. `is_poisoned` lets a direct Journal consumer check `commit`'s precondition (§7.4); the Engine never needs it — after any commit failure the run is Fatal and the Engine never writes again (`FAIL-FINALIZE`).

Encode requirements on all payloads: `Serialize` implementations are deterministic, side-effect-free, bounded, and nonpanicking (trusted obligations, §10); map iteration order is stable; map keys not representable as JSON strings are `Encode` failures. Non-finite floating-point serialization follows `serde_json` behavior. Lossy serialization is evidence only of the fields it emits. `JRN-FORMAT` requires every record to serialize as a JSON object; a payload that does not is rejected as `NotAnObject`, checked immediately after a successful encode and before the newline is appended, with the same write-nothing, poison-nothing guarantee as `Encode` and `BoundExceeded`.

`max_record_bytes` plus the reserved newline byte must not overflow `usize`; `JournalBuildError::MaxBytesTooLarge` rejects the one construction where it would (`max_record_bytes == usize::MAX`), per A6's ban on silent saturation. Reserving the resulting reusable buffer can also fail, which is reported as `JournalBuildError::AllocationFailed` before constructing a Journal.

Memory sinks (`Vec<u8>` via a user-owned shared handle) make tests and fault injection direct; `std::io::sink()` discards evidence but still pays encoding. Because JSONL bytes alone cannot identify the committed prefix after a sink failure, replay requires a cleanly completed Journal or an externally trusted committed boundary.

### 7.3 Invariants

| ID | Invariant |
|---|---|
| `JRN-FORMAT` | One record is one serde JSON object plus one newline; line order is the sequence. `max_record_bytes` bounds the encoded object and excludes the newline. |
| `JRN-ENCODE` | Encoding completes in the reusable bounded buffer before any byte of that record reaches the sink. `Encode`, `NotAnObject`, and `BoundExceeded` therefore write nothing and do not poison. |
| `JRN-COMMIT` | Only a successful flush commits a record. After a sink failure, bytes past the last committed record are an uncertain suffix and are not records, even if they form complete lines. |
| `JRN-POISON` | Any sink failure — a write or flush Error, zero progress (`Ok(0)` becomes `WriteZero`), or `Interrupted`, which is not retried — permanently poisons the Journal; no later explicit `write` or `flush` occurs through it. |
| `JRN-SINK` | `W: std::io::Write` is the whole persistence abstraction. A sink is fresh for one run or positioned immediately after a newline. Persistence beyond successful `flush`, including power-loss durability, is outside the contract; writer destructor behavior is too. |

### 7.4 Implementation

The reusable bounded byte buffer implements `std::io::Write`, so `serde_json::to_writer` writes directly into it. A zero-progress write while encoding becomes `WriteZero` through `Write::write_all` and is reported as `BoundExceeded`; other serializer errors remain `Encode`. Appending the newline rejects a record that left no reserved byte, also reporting `BoundExceeded` before any sink interaction (`JRN-ENCODE`). serde_json surfaces map keys not representable as JSON strings as `Encode` errors.

`new` computes `max_record_bytes.checked_add(1)` to size the buffer for the object plus the reserved newline byte; overflow — only at `max_record_bytes == usize::MAX` — is `JournalBuildError::MaxBytesTooLarge` rather than a saturated size (A6). A failed buffer reservation is `JournalBuildError::AllocationFailed`.

| Step | `commit` procedure |
|---|---|
| 1 | Poisoned → precondition violation: an invariant panic (A8). The Engine never calls a poisoned Journal (`FAIL-FINALIZE`). |
| 2 | Clear the buffer; encode directly through its `Write` implementation. Its zero-progress `WriteZero` rejection → `BoundExceeded`; other serde failures → `Encode`. Nothing was written, nothing poisons (`JRN-ENCODE`). |
| 3 | Encoded bytes must start with `{` and end with `}` — otherwise `NotAnObject`. Nothing was written, nothing poisons (`JRN-ENCODE`). |
| 4 | Append the newline (excluded from the bound, `JRN-FORMAT`). |
| 5 | Write the buffer with a hand-rolled loop bounded by record length — not `write_all`, because `Interrupted` is not retried (`JRN-POISON`); `Ok(0)` becomes `WriteZero`. First failure → poison, `Sink { operation: Write, .. }`. |
| 6 | Flush. Failure → poison, `Sink { operation: Flush, .. }`. Success commits (`JRN-COMMIT`). |

The Journal writes directly from its complete record buffer, with no second buffering layer. Reading Journals back is not Kavod's concern: the output is plain JSON Lines, and consumers read it with whatever tooling they like.

## 8. Engine

The Engine owns the run: it drives startup, the turn protocol, the record protocol, and Fatal classification, and it is the Journal's only caller.

### 8.1 Public API

```rust
pub struct EngineConfig {
    pub max_turns: NonZeroUsize,
    pub max_commands_per_turn: NonZeroUsize,
    pub max_record_bytes: NonZeroUsize,
}

pub enum BuildError {
    CommandBuffer(TryReserveError),
    Journal(JournalBuildError),
}

impl<A, E, W> Engine<A, E, W>
where
    A: Application,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: std::io::Write,
{
    pub fn new(config: EngineConfig, app: A, env: E, writer: W)
        -> Result<Self, BuildError>;
    pub fn run(self) -> EngineExit<A::State, A::Fatal, E::Error>;
}

pub enum RecordKind {
    RunStarted,
    EventAccepted,
    CommandsPrepared,
    CommandsDispatched,
    StopRequested,
    TurnCompleted,
}

pub enum EngineExit<S, AF, EE> {
    Stopped { state: S },
    Fatal { state: S, cause: FatalCause<AF, EE> },
}

pub enum FatalCause<AF, EE> {
    Application(AF),
    Environment(EnvironmentFatal<EE>),
    Journal(JournalFatal),
    Core(CoreFatal),
}

pub enum EnvironmentOperation {
    Start,
    NextEvent,
    /// Where in the dispatch loop the Error was observed — not necessarily
    /// this Command's own routing/admission; an unrelated already-latched
    /// failure can surface here too, per `ENV-LATCH`.
    Dispatch { position: usize },
    ShutdownGraceful,
}

pub struct EnvironmentFatal<EE> {
    pub error: EE,
    pub operation: EnvironmentOperation,
}

pub struct JournalFatal {
    pub record_kind: RecordKind,
    pub error: JournalError,
}

pub enum CoreFatal {
    TimeRegression { previous: Timestamp, offered: Timestamp },
    TurnBoundExceeded,
    CommandBoundExceeded,
}
```

### 8.2 Semantics: record protocol

`RecordKind` is the closed set of record names below. Records use serde's default externally tagged representation. `RunStarted` is the only possible first committed record, so every nonempty Journal begins with a versioned record.

| Record | Fields | Committed | Evidences |
|---|---|---|---|
| `RunStarted` | `schema_version`, `logical_time` | Before `on_start`. | Acceptance of the start turn (index 0) at the start time. |
| `EventAccepted` | `index`, `logical_time`, `event` | Before `on_event`. | Acceptance of one External Event. |
| `CommandsPrepared` | `index`, ordered `commands` | Before the first handoff of a nonempty batch. | The complete Command intent of the turn. |
| `CommandsDispatched` | `index` | After the last handoff of a nonempty batch. | Every prepared Command was handed off. |
| `StopRequested` | `index` | After a `Stop` outcome, before graceful shutdown. | The Application requested shutdown. |
| `TurnCompleted` | `index`, `outcome` (`Continue`/`Stop`) | End of every non-Fatal turn. | The turn's outcome. |

Both `CommandsPrepared` and `CommandsDispatched` are omitted for an empty batch. These commit points are A5 in action: no handler runs before its acceptance record commits, no handoff precedes `CommandsPrepared`, and no next Event is acquired before `TurnCompleted(Continue)` commits. A run's exit is never journaled — a fatal turn's Journal simply ends at its last committed record, and `CommandsPrepared` plus the typed dispatch position in `EngineExit` identifies the exact successful prefix.

The concrete Rust types of the records are Engine-internal (tier 3); their serialized form per this table is normative.

### 8.3 Invariants

| ID | Invariant |
|---|---|
| `FAIL-FINALIZE` | Fatal finalization runs exactly once, in order: stop normal execution → if the Environment was started and not consumed, `shutdown(Abort)`, discarding its `Err` (§1.4) → return `EngineExit::Fatal { state, cause }`. After the primary failure the Engine never writes to the Journal again; no handler, dispatch, Event acquisition, or graceful action begins after Fatal (A2, A4). |
| `BOUND-SIZING` | `max_record_bytes` must accommodate the largest batch the Application can stage under `max_commands_per_turn`; this sizing is a trusted configuration obligation, not a construction proof. |
| `BOUND-INDEX` | `max_turns` may equal `u64::MAX`; the pre-acquisition turn check makes `EventIndex` overflow unreachable, so overflow is an invariant panic, not an Engine outcome. |

### 8.4 Implementation

**Construction** (`Engine::new`) — before State creation, invoking no Application or Environment method; failure is `BuildError`, never a runtime Fatal:

| Step | Action | On failure |
|---|---|---|
| 1 | `try_reserve` the complete Command batch for `max_commands_per_turn`. | `CommandBuffer`. |
| 2 | Construct the Journal from `max_record_bytes`. | `Journal`. |

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
| 4 | Assign the next checked `EventIndex`. | Overflow is unreachable (`BOUND-INDEX`) and would be an invariant panic. |
| 5 | Commit `EventAccepted`. | Journal Fatal; the candidate stays consumed but never becomes current. |
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
| 6a | `Continue`: commit `TurnCompleted(Continue)`. | Only success permits the next Event acquisition; Journal failure is Fatal, and the Environment is still live and unconsumed, so finalization runs Abort. |
| 6b | `Stop`: commit `StopRequested`. | Journal failure precedes shutdown. |
| 7b | `shutdown(Graceful)` (consumes the Environment). | `Err` is primary `Environment(ShutdownGraceful)`; no second shutdown call is possible. |
| 8b | Commit `TurnCompleted(Stop)`. | Journal Fatal; the Environment is already consumed, so finalization skips Abort. |
| 9b | Return `EngineExit::Stopped { state }`. | — |

**Fatal finalization** (the procedure behind `FAIL-FINALIZE`):

| Step | Action | Notes |
|---|---|---|
| 1 | Stop normal execution; fix the primary cause (A4). | |
| 2 | Environment started and not consumed → `shutdown(Abort)`, discarding its `Err` (§1.4; terminal Port and Environment state flows through user-owned handles, §3.2). | Skipped after a `start` failure or any consuming shutdown. |
| 3 | Return `EngineExit::Fatal { state, cause }`. | State always exists here — it is created before any fallible run step. The Journal is not touched. |

## 9. Crate layout

One crate, `kavod`, no feature gates — both Environments are std-only. Dependencies: `serde` (with `derive`), `serde_json`. `ports!` is `macro_rules!`, so no proc-macro crate exists. This section is tier 3 except the public item names, which §§2–8 own.

```
kavod/src/
  lib.rs         #![forbid(unsafe_code)]; public re-exports
  time.rs        EventIndex, Timestamp
  application.rs Application, Outcome, Context
  port.rs        PortContract, Never, ports!
  environment.rs Environment, ShutdownMode
  live/          LivePort, LiveCtx, live Environment (OPEN-1)
  sim/           SimPort, SimCtx, simulated Environment (OPEN-2)
  journal.rs     Journal, JournalError
  engine.rs      Engine, EngineConfig, records, EngineExit, FatalCause
```

## 10. Obligations and verification

Kavod enforces its invariants; everything below is trusted — upheld by a named party and checked by the stated means, not by the Engine at runtime. This table is the complete boundary; an obligation not listed here is not trusted, it is enforced.

| Obligation | Upholder | Verified by |
|---|---|---|
| Handlers avoid hidden authority: clocks, entropy, IO, globals, concurrency, mode (§1.3) | Application author | Simulated repeatability tests: same trace twice → identical Journal bytes and exit |
| Handlers, serializers, writers, callbacks, destructors are bounded and nonpanicking | Their authors | Review; §1.5 defines the blast radius when violated |
| Final binary built with `panic = "abort"` (§1.5) | Build/deployment configuration | Build profile review |
| One-to-one Slot routing and Error mapping (`PORT-ROUTING`) | Wiring author | Per-Slot tests |
| Stable business key on externally consequential Commands (§3.2) | Application author | Per-Slot tests recognizing repeated or uncertain external effects |
| `Serialize` impls deterministic, side-effect-free, bounded, nonpanicking; stable map order (§7.2) | Payload authors | Golden-Journal tests |
| Simulated Port determinism and bounded `step` work (§6.2) | Sim Port author | Repeatability tests |
| Live Port blocking points observe lifecycle and cooperate with shutdown (`BOUND-BLOCKING`) | Live Port author | Shutdown tests under load |
| `BOUND-SIZING`: `max_record_bytes` fits the largest stageable batch | Deployment configuration | Config review; construction proves nothing about record sizes |
| Transitive memory bounds of owned values (§1.6) | Value owner | Owner-defined |

Kavod-side verification conventions: Journal and sink failures are exercised through memory sinks and failing writers (§7.2); and the determinism contract (§1.3) is checked by running one conformance trace suite against both Environments and comparing every Core-owned discriminant.
