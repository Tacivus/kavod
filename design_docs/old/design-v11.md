# Kavod Core Design

> **Status:** Authoritative. One section is open: Wiring & construction (section 10).
> **Scope:** The deterministic Core shared by live and simulated execution.
> **Priority:** The smallest robust design implementable in days, whose rules can be enforced and tested.

Kavod Core is written under `#![forbid(unsafe_code)]`.

## 0. Reading this document

This document stands alone. It defines Kavod by what Kavod does.

Three forms bind:

1. **API blocks** — item names, type shapes, trait bounds, and variant sets are exact,
   and a doc comment binds the behavior it states, its wording free. Listed derives are
   required; further derives and receiver style are free. A block a section marks
   provisional binds its semantics only, until the Wiring section closes.
2. **Guarantee rows** — every normative rule outside an API block or a binding table is
   a table row with an ID. A rule in none of the three forms does not exist.
3. **Binding tables** — the Environment contract's commitment table and the Run's
   construction, startup, state, edge, and record tables: every row is a guarantee row,
   and each table is exhaustive — work it does not list does not happen. The state and
   edge tables are the run's graph.

Everything else is prose, and prose has exactly three jobs: **define** a term, **derive** a
consequence from the rules, or **justify** a rule so it is not relitigated. A definition
binds vocabulary — the Glossary is its home — and creates no obligation by itself. Test any
sentence by deleting it: if an implementer obligation changes, the sentence was a rule in
the wrong clothes — give it an ID or move it; if nothing changes and it does none of the
three jobs, cut it.

Placement rules, for every future edit:

- **The Run owns interaction.** If a fact can be tested against one component alone, it
  lives in that component's section. If it says when an operation is called, what its
  result means for the run, or what happens next, it lives in the Run.
- **Citations point backward.** Section order is dependency order; a fact that needs a
  forward reference is in the wrong section. Navigation pointers are exempt: the
  open-section notice, the bounds registry, the ownership map, the invariant index, and
  a contract's pointer to its shipped implementations, and trust marks pointing into
  the Obligations table.
- **Cite IDs.** Never section numbers, here or in tests.
- **Implementation sections realize the contract.** A Live or Simulated guarantee
  either names the Environment-contract row it realizes or defines that
  implementation's Port-facing API; a
  fact any conforming Environment implementor would need lives in the contract. Core
  sections build only on the contracts and never name an implementation — the bounds
  registry (navigation) is the one earlier mention of the two shipped Environments.

Every ID is **enforced** — Kavod makes violation impossible or panics — except the IDs in
the Obligations table, which are **trusted**: upheld by a named party, checked by the
stated means. Contract rows bind whoever implements the contract: Kavod enforces them
in the implementations it ships and the Run boundary-checks what it can observe; a
bespoke implementation's conformance is a trusted obligation (Obligations table).

Enforcement has an order: **unrepresentable beats asserted beats tested.** Where ownership
or a token can carry a rule, it must. Always-on constant-time assertions cover where types
run out. Tests cover the rest.

## 1. Glossary

One line per term. These definitions are normative and appear only here.

- **Application** — the user's pure transition logic: two handlers plus an initial State.
- **Handler** — `on_start` or `on_event`; runs once per turn.
- **State** — all run-varying application data, owned by the Application.
- **Event** — one unit of input the Environment delivers to the run.
- **Command** — one unit of intent a handler stages for the Environment to deliver.
- **Turn** — one accepted Event (or the start), one handler call, one batch: the run's
  unit of progress.
- **Batch** — the ordered Commands one turn stages.
- **Candidate** — an Event returned by `next_event`: consumed, not yet accepted.
- **Accepted** — a candidate whose `EventAccepted` record committed; only then does it
  have an index.
- **Contract** — one Event protocol paired with one Command protocol.
- **Slot** — one named use of a Contract.
- **Port** — one Environment's implementation of one bound Slot.
- **Environment** — the Core's one boundary to the outside: waiting, Event selection,
  time, Command routing, lifecycle.
- **Journal** — the bounded JSON Lines writer the run's records pass through.
- **Sink** — the `std::io::Write` value the Journal writes into.
- **Record** — one flat JSON object evidencing one protocol step.
- **Commit** — encode, write, flush; only a successful flush commits.
- **Poisoned** — the Journal's permanent state after a sink failure; it accepts no
  further commits.
- **Error** — a typed value a component reports when an operation fails.
- **Fatal** — the run-level classification of the Error or Core condition that ended
  the run.
- **fail / failure** — plain English for "returned an Error"; no further meaning.
- **Commitment point** — the instant an operation's effect becomes real. Before it,
  nothing happened; after it, nothing is retried, revoked, or rolled back.
- **Latch** — the Environment's store for the first Error its own activity publishes
  (typically a Port). States: empty, pending, reported, closed.
- **Quiescence** — whether all run-scoped activity finished: `Quiesced` (witnessed
  complete) or `Incomplete`.
- **Shutdown signal** — the Environment-delivered notice: no more input is coming;
  finish what you own and return.
- **Trace** — the accepted `(Event, Timestamp)` sequence, every Environment operation
  result, and every sink operation result — Error values (not their presence or
  position) erased — plus the run's Quiescence.
- **Phase, edge, token** — the run's position in its graph, the transitions between
  positions, and the value whose possession proves the position.

## 2. Laws

Everything in this document is a consequence of eight axioms.

| # | Axiom | Statement |
|---|---|---|
| A1 | Single authority | Every fact has exactly one owner and one representation. |
| A2 | Serial turns | One Event, one handler call, one batch at a time; a turn completes, or the run goes Fatal, before the next Event is requested. |
| A3 | One commitment point | Every effectful operation has exactly one commitment point. |
| A4 | First failure wins | The first Error the run observes is the Fatal cause. Everything after is best-effort cleanup whose Errors are discarded. |
| A5 | Intent precedes effect | Where a record announces an action, it commits before the action begins; a completion record witnesses effects already committed. |
| A6 | Bounded everything | Every Kavod-owned container, count, identifier, and active loop has one accounting owner and a bound checked before use. Arithmetic on counts, capacities, times, and identities is checked. |
| A7 | Typed inside, rendered at the edge | Errors stay typed values while Kavod owns them. Text and bytes exist only at the serialization boundary. |
| A8 | Panics are bugs | A failing user component reports a typed Error. A panic — in Kavod or user code — is a bug: the process aborts, and no exit represents it. |

**Failure.** A4's cleanup rule means Fatal performs no rollback: every effect that
reached its commitment point stays real. Consequences of this appear once, at each
effect's owner, and are derivable everywhere else.

**Panics.** Kavod ships with `panic = "abort"` and relies on unwinding nowhere in
shipped code; test code may catch panics under the test profile, which unwinds. After a
panic the evidence is the Journal's committed records, kept current by
flush-per-record commits. The abort profile in the final binary is a trusted obligation
(Obligations table).

**Assertions.** Kavod checks its own invariants with always-on, constant-time assertions
that panic on violation (A8). Their reach follows the enforcement order in section 0.

**Guarantees**

| ID | Guarantee |
|---|---|
| `BOUND-LOOPS` | Every Kavod-owned active loop is bounded and nonrecursive: the run by the index domain, dispatch by batch length, Environment work by its owned budgets, Journal writing by record length. A blocking wait is not an active loop and implies no elapsed-time bound; work inside user code is trusted to be bounded (`BOUND-BLOCKING`). |

**Bounds registry** (navigation; each bound's rules live with its owner):

| Bound | Owner |
|---|---|
| Command batch capacity (`max_commands_per_turn`) | The Run |
| Record bytes (`max_record_bytes`) | Journal |
| Index domain (`u64`) | The Run |
| Event queue, per-Port Command inboxes, shutdown deadline, time domain | Live Environment |
| Wakeup arms (one per Port), step budget per acquisition, time domain | Simulated Environment |

Slot registration at construction fixes the Port set, the thread count, and the Slot
order: static, not configured. Every configured capacity uses a nonzero type, so zero
is unrepresentable.

**Ownership map** (navigation):

| Component | Owns |
|---|---|
| Application | Pure transition logic; all run-varying data, inside State. |
| Port | All of its own domain, protocol, and native state. |
| Environment | Topology, waiting, Event selection, time stamping, routing, lifecycle. |
| Journal | The write mechanism: bounded encoding, one sink, poison. |
| The Run | The graph, the records, the token (index and time), Fatal classification. |

## 3. Application contract

The Application is a pure transition function over its State. Handlers are
user-implemented; Kavod owns the index and time types and `Context`.

### API

```rust
/// Both serialize as transparent u64 JSON values.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct EventIndex(/* u64, private */);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Timestamp(/* u64 nanoseconds, private */);

impl EventIndex {
    /// The accepted turn's ordinal: 0 for the start turn, External Events from 1.
    pub fn as_u64(self) -> u64;
}

impl Timestamp {
    /// Builds a timestamp from a nanosecond count. The count's origin and
    /// meaning belong to the stamping Environment.
    pub fn from_nanos(nanos: u64) -> Self;
    /// The timestamp advanced by `elapsed`, or `None` if the duration or the
    /// sum overflows u64 nanoseconds (A6).
    pub fn checked_add(self, elapsed: std::time::Duration) -> Option<Self>;
    pub fn as_nanos(self) -> u64;
}

pub trait Application {
    type State;
    type Event: Serialize;
    type Command: Serialize;
    type Error;

    fn initial_state(&self) -> Self::State;

    fn on_start(
        &self,
        state: &mut Self::State,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Error>;

    fn on_event(
        &self,
        state: &mut Self::State,
        event: &Self::Event,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Error>;
}

pub enum Outcome<E> {
    Continue,
    Stop,
    Fatal(E),
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

### Guarantees

| ID | Guarantee |
|---|---|
| `APP-CONTEXT` | During a handler, `Context` reports its turn's index and logical time, and is the only capability Kavod supplies a handler (A1). |
| `APP-EMIT` | `emit` is infallible and transfers one immutable Command; while capacity remains it appends in call order. |
| `APP-OVERFLOW` | The first over-bound `emit` stores nothing and sets an overflow marker; every later `emit` stores nothing. A fresh handler invocation starts with the marker clear. |
| `APP-FUTURE` | Work for a future turn returns through an External Event; `Context` offers no other channel. |

### Mechanism

`Context` wraps one fixed-capacity Command buffer of `max_commands_per_turn` entries,
allocated once at construction and reused every turn, plus the overflow marker. The
buffer never grows, so the turn loop does not allocate. `remaining()` is capacity minus
length, or zero when the marker is set.

| Step | `emit` |
|---|---|
| 1 | Marker set → store nothing (`APP-OVERFLOW`). |
| 2 | Buffer full → set the marker, store nothing. |
| 3 | Otherwise append in call order (`APP-EMIT`). |

### Notes

*Derive:* a handler cannot observe or influence anything outside State, its Event, and
Context — the signatures admit nothing else. Determinism then rests on State and the
payload types carrying no hidden authority, which is the Application author's trusted
obligation (Obligations table).

*Derive:* `Timestamp::checked_add` lets Port code compute a later time from an
Environment-supplied one; `from_nanos` lets an Environment implementation outside the
crate mint the timestamps it stamps.

*Define:* Port-domain timestamps — exchange time, receive time — are ordinary Event
payload fields with no Core meaning. Equal logical times are valid; index order breaks
ties.

## 4. Port contract

A Contract pairs one Event protocol with one Command protocol. A Port is one
mode-specific implementation of one bound Slot.

### API

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

### Guarantees

| ID | Guarantee |
|---|---|
| `PORT-STATE` | A Port exclusively owns its mutable domain, protocol, and native state; wiring and the Environment relay its values without interpreting them. Processing after a Command's handoff belongs to the destination Port. |
| `PORT-SUMS` | The Slot-qualified Event and Command sums are closed and type-checked against their Contracts; distinct Slots of one Contract are distinct variants. |
| `PORT-ROUTING` | Fan-in is one frozen variant constructor per inhabited Event direction; fan-out is one hand-written exhaustive destination match, each arm mapping its Port Error into the Environment Error sum. The compiler proves exhaustiveness and payload agreement; each arm naming its semantically correct Slot and mapping is a trusted obligation (Obligations table). |

### Mechanism

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

That is the whole expansion: two enums, serde's default externally tagged
representation. Hand-written equivalents are supported and observationally identical,
and may add derives freely. Generated derives use `::serde` paths, so consumers need a
direct dependency named `serde`. `Never`'s `Serialize` implementation is `match *self {}`,
and a `Never` arm is discharged by matching the uninhabited value.

### Notes

*Define:* every Contract is duplex; an absent direction uses `Never`.

*Define:* the finite-source pattern — a source that runs out of input offers one
application-defined terminal Event and awaits the shutdown signal; the terminal Event's
handler answers `Stop`. Ending a run is Application logic, expressed in the Event
protocol like everything else.

*Justify:* the expansion above is exhaustive, so the two enums are inspectable by eye
and replaceable by hand — the macro is sugar, never authority.

## 5. Environment contract

The Environment is the Core's one boundary to the outside. This section is the
complete contract: an implementation satisfying every row here, under `ENV-SERIAL`'s
call pattern, is a conforming Environment. The Live and Simulated sections are the two
implementations Kavod ships; each adds only its own Port-facing API.

### API

```rust
pub trait Environment {
    type Event;
    type Command;
    type Error;

    fn start(&mut self) -> Result<Timestamp, Self::Error>;
    fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error>;
    fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error>;
    /// Takes the first currently latched Error without waiting for one.
    fn take_error(&mut self) -> Option<Self::Error>;
    /// Publishes the shutdown signal, closes admission, and applies the
    /// Environment's bounded quiescence policy.
    fn shutdown(self) -> Quiescence;
}

pub enum Quiescence {
    Quiesced,
    Incomplete,
}
```

### Commitment points

A3 applies on both sides of each row. The table binds outcomes, not instants: where a
commitment sits inside an implementation is that implementation's business — each names
its own — and the returned value is the caller's only witness of it.

| Operation | Commitment point | `Err` means | Success means |
|---|---|---|---|
| `start` | Activation: run-scoped activity becomes live, after the start time is frozen. | `ENV-START` holds; effects already made stand (A4's cleanup rule). | Run-scoped activity is live; the frozen start time is returned. |
| `next_event` | Consumption of one candidate; the call may wait for one — the only operation that waits for input. | No candidate was consumed. | Exactly one candidate is consumed, for good — never retried, revoked, or re-offered. |
| `dispatch` | Handoff of this one Command; the attempt never waits for future capacity. | This Command was not handed off. | Handoff stands; the destination Port owns all further processing (`PORT-STATE`). |
| `take_error` | One atomic snapshot of the latch. | — | `Some(error)` reports the pending first Error and marks the latch reported forever; `None` proves only that nothing was pending at the snapshot. The call never waits. |
| `shutdown` | The call itself: it consumes the Environment. | — | `Quiesced` witnesses that every unit of run-scoped activity completed; `Incomplete` means at least one unit was still unfinished when the Environment's bounded shutdown policy ended. |

### Guarantees

| ID | Guarantee |
|---|---|
| `ENV-SERIAL` | The contract assumes one serial caller: `start` exactly once, first; then `next_event`, `dispatch`, and `take_error` one at a time; `shutdown` at most once, consuming the Environment. After any operation returns `Err`, the only later call is `shutdown`. Implementations need no synchronization against concurrent contract calls. |
| `ENV-START` | When `start` returns `Err`, the Environment is quiesced and safe to drop, and no Port is left mid-lifecycle: every Port either never began or had its lifecycle ended before the return. |
| `ENV-LATCH` | The latch holds at most the first Error the Environment's own activity publishes. States: empty → pending (first publication) → reported (`take_error` `Some`, or an operation returning it as its `Err`) or closed (shutdown began; a pending Error is discarded). Publication is linearized against `next_event` and `dispatch` commitment and `take_error`'s snapshot: a pending Error observed before an operation's own commitment is taken, marked reported, and returned as that operation's `Err`; otherwise the operation's commitment stands. After reported or closed, every later publication is discarded. |
| `ENV-TIME` | One Environment authority — the single Event acceptor — stamps `Timestamp` on `start` and every `next_event`, and owns the count's origin and meaning. Stamped times never decrease across the run; equal stamps are valid. |
| `ENV-SHUTDOWN` | `shutdown` stops Event delivery, rejects new Commands, closes the latch, and raises the shutdown signal so each Port can observe it before processing any further queued Command. Already-handed-off residue is the destination Port's to drain or abandon (`PORT-STATE`). The Environment itself initiates no further externally consequential work, and applies its own bounded quiescence policy. |
| `ENV-SEPARATION` | The Environment orchestrates Ports and only that: Port domain state belongs to Ports (`PORT-STATE`), and handler invocation belongs to the Run. |
| `ENV-BOUNDS` | Every operation preserves the Environment's own bounds (Laws registry). |

### Notes

*Define:* the shutdown signal carries exactly its glossary meaning — no more input is
coming; finish what you own and return. Disposition of queued work is Port authorship.

*Derive:* `shutdown` reports through `Quiescence` alone, so an Error arising during
shutdown work stays inside the Environment — A4's cleanup rule, applied at the boundary.

## 6. Journal

The Journal is a policy-free bounded JSON Lines writer: it encodes one value, writes
one line, flushes. The record schema belongs to the Run. The Journal's output is
human-readable forensic evidence, guaranteed exactly through its last committed record.

### API

```rust
pub struct Journal<W: std::io::Write> {
    /* writer, one reusable bounded encode buffer, poison marker */
}

impl<W: std::io::Write> Journal<W> {
    /// Reserves the encode buffer up front.
    pub fn new(writer: W, max_record_bytes: NonZeroUsize)
        -> Result<Self, JournalBuildError>;
    /// Encode into bounded storage, write one line, flush.
    /// Precondition: not poisoned.
    pub fn commit<R: Serialize>(&mut self, record: &R) -> Result<(), JournalError>;
    pub fn is_poisoned(&self) -> bool;
}

pub enum JournalBuildError {
    /// `max_record_bytes` leaves no room for the reserved newline byte.
    MaxBytesTooLarge,
    /// The reusable record buffer could not reserve its storage.
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

### Guarantees

| ID | Guarantee |
|---|---|
| `JRN-FORMAT` | One record is one serde JSON object plus one newline; line order is the sequence. `max_record_bytes` bounds the encoded object; the newline is stored beyond it. |
| `JRN-ENCODE` | Encoding completes in the reusable bounded buffer before any byte of that record reaches the sink. `Encode`, `NotAnObject`, and `BoundExceeded` therefore write nothing and poison nothing. |
| `JRN-COMMIT` | Only a successful flush commits a record. After a sink failure, bytes past the last committed record are an uncertain suffix, even if they form complete lines. |
| `JRN-POISON` | Any sink failure permanently poisons the Journal: a write or flush Error, zero progress (`Ok(0)` becomes `WriteZero`), `Interrupted` (never retried), or a sink claiming more bytes written than it was given (`InvalidData`). A poisoned Journal performs no further sink operation; `commit` on it is a precondition violation and panics (A8). |
| `JRN-SINK` | `W: std::io::Write` is the whole persistence abstraction. A sink is fresh for one run or positioned immediately after a newline. The contract ends at successful flush; durability beyond it, and writer destructor behavior, belong to the sink's owner. |

### Mechanism

The reusable bounded buffer implements `std::io::Write`, so `serde_json::to_writer`
encodes directly into it.

| Step | `commit` |
|---|---|
| 1 | Poisoned → invariant panic (A8). |
| 2 | Clear the buffer; encode into it. The buffer's zero-progress `WriteZero` → `BoundExceeded`; other serde failures → `Encode`. Nothing written, nothing poisoned (`JRN-ENCODE`). |
| 3 | Encoded bytes must start with `{` and end with `}` — otherwise `NotAnObject`. Nothing written, nothing poisoned. |
| 4 | Append the newline; a record that left no room for it → `BoundExceeded`. |
| 5 | Write the buffer with a loop bounded by record length that retries only short successful writes: `Err` (including `Interrupted`), `Ok(0)` as `WriteZero`, and an over-reported count as `InvalidData` each poison and return `Sink { operation: Write, .. }` (`JRN-POISON`). |
| 6 | Flush. Failure → poison, `Sink { operation: Flush, .. }`. Success commits (`JRN-COMMIT`). |

`new` computes `max_record_bytes.checked_add(1)` to size the buffer for the object plus
the newline; overflow — only at `usize::MAX` — is `MaxBytesTooLarge` (A6). A failed
reservation is `AllocationFailed`, and it reports no underlying detail — a deliberate
asymmetry kept for the construction path's simplicity.

### Notes

*Derive:* encode requirements fall on payload authors as trusted obligations
(Obligations table): deterministic, side-effect-free, bounded, nonpanicking `Serialize`
with stable map order. Map keys that cannot be JSON strings surface as `Encode`.
Non-finite floats follow `serde_json`. Lossy serialization is evidence only of the
fields it emits.

*Derive:* a named-field struct payload serializes as a JSON object — newtype, tuple,
and unit structs do not — so a caller committing only named-field structs can treat
`NotAnObject` as unreachable; the variant serves direct Journal consumers with
arbitrary payloads.

*Derive:* memory sinks (a shared `Vec<u8>` handle) make tests and fault injection
direct. Because JSONL bytes alone cannot mark the committed boundary after a sink
failure, replay needs a cleanly completed Journal or an externally trusted boundary.

## 7. The Run

The Run composes the contracts: one Engine drives one Application against one
Environment, evidencing every step through one Journal. Its shape is a graph. States
carry the work; edges carry the records; a transition *is* a commit — the next phase is
unreachable until the edge's record commits. This is A5 and A3 closed over
the whole run, and the Engine enforces it at compile time (`RUN-GRAMMAR`).

### API

```rust
pub struct EngineConfig {
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
    pub fn run(self) -> EngineExit<A::State, A::Error, E::Error>;
}

pub enum RecordKind {
    RunStarted,
    EventAccepted,
    CommandsPrepared,
    CommandsDispatched,
    StopRequested,
    TurnCompleted,
}

pub enum EngineExit<S, AE, EE> {
    Stopped { state: S },
    Fatal {
        state: S,
        cause: FatalCause<AE, EE>,
        quiescence: Quiescence,
    },
}

pub enum FatalCause<AE, EE> {
    Application(AE),
    Environment(EnvironmentFatal<EE>),
    Journal(JournalFatal),
    Core(CoreError),
}

pub struct EnvironmentFatal<EE> {
    pub error: EE,
    pub operation: EnvironmentOperation,
}

pub enum EnvironmentOperation {
    Start,
    NextEvent,
    /// Where in the dispatch loop the Error was observed — possibly an
    /// unrelated already-latched Error, per ENV-LATCH.
    Dispatch { position: usize },
    /// The per-turn latch snapshot (RUN-CHECKPOINT) returned a pending Error.
    Checkpoint,
}

pub struct JournalFatal {
    pub record_kind: RecordKind,
    pub error: JournalError,
}

pub enum CoreError {
    TimeRegression { previous: Timestamp, offered: Timestamp },
    IndexExhausted,
    CommandBoundExceeded,
    ShutdownIncomplete,
}
```

### Construction and startup

`Engine::new` runs before State creation and invokes no Application or Environment
method; failure is `BuildError`, and no run happened.

| Step | `Engine::new` | On failure |
|---|---|---|
| 1 | Reserve the complete Command batch for `max_commands_per_turn` with `try_reserve`. | `CommandBuffer` |
| 2 | Build the Journal from `max_record_bytes`. | `Journal` |

| Step | `run` startup | On failure |
|---|---|---|
| 1 | Create initial State, exactly once, before any fallible step — so every exit carries State. | A panic is a bug (A8). |
| 2 | `Environment::start`. | `Environment(Start)` Fatal with `Quiescence::Quiesced` — `ENV-START` already holds, so finalization skips `shutdown`. |
| 3 | Mint the token at `Initial`, consuming the Journal. | — |
| 4 | Take the `RunStarted` edge with the start time; the start turn proceeds per the graph at index 0. | Journal Fatal. |

### The graph

Non-normative sketch; the two tables below are the guarantee.

```
Initial ──RunStarted──▶ TurnOpen ──CommandsPrepared──▶ Prepared
                          │   ▲                           │
             (empty batch)│   │EventAccepted     CommandsDispatched
                          ▼   │                           ▼
                        EffectsComplete ◀─────────────────┘
                          │         │
     TurnCompleted(Continue)        StopRequested
                          │         │
                          ▼         ▼
                BetweenTurns      StopPending ──TurnCompleted(Stop)──▶ Closed

any failure: drop the token ──▶ RUN-FINALIZE
```

**States** — work in the listed order; each failure row names its `FatalCause`.

| State | Work, in order |
|---|---|
| `Initial` | None; startup takes the only edge out. |
| `TurnOpen` | Invoke the handler once with `Context` over the batch buffer — `on_start` at index 0, `on_event` otherwise, one turn protocol (A2). Then: overflow marker set → discard the batch → `Core(CommandBoundExceeded)`, beating every `Outcome` (A4: the overflow came first; a returned `Fatal` payload is discarded with the batch). `Outcome::Fatal(error)` → discard the batch → `Application(error)`. Otherwise remember the answer and leave: empty batch by the recordless edge, nonempty by `CommandsPrepared`. |
| `Prepared` | Dispatch each Command once, in order. `Err` at position k → `Environment(Dispatch { position: k })`: the prefix `[0, k)` stands handed off, the Command at k was not handed off, the suffix is discarded. |
| `EffectsComplete` | The checkpoint (`RUN-CHECKPOINT`): `take_error`. `Some(error)` → `Environment(Checkpoint)`. `None` mints the checkpoint witness; the remembered answer picks the edge. |
| `BetweenTurns` | The index-domain check (`RUN-INDEX`): at the `u64` boundary → `Core(IndexExhausted)`, `next_event` uncalled. Then `next_event`; `Err` → `Environment(NextEvent)`. The candidate feeds the `EventAccepted` edge. |
| `StopPending` | `shutdown` — it consumes the Environment. `Incomplete` → `Core(ShutdownIncomplete)`, finalization reusing that result. `Quiesced` mints the quiescence witness. |
| `Closed` | Return `EngineExit::Stopped { state }`. |

**Edges** — each commits its record or fails as `Journal(JournalFatal)` carrying that
record's kind; `EventAccepted` alone can also fail as `Core(TimeRegression)`. On any
failure the token is dropped and `RUN-FINALIZE` runs.

| From | Record | Requires | To |
|---|---|---|---|
| `Initial` | `RunStarted` | the frozen start time | `TurnOpen` (index 0) |
| `TurnOpen` | — | empty batch | `EffectsComplete` |
| `TurnOpen` | `CommandsPrepared` | nonempty batch | `Prepared` |
| `Prepared` | `CommandsDispatched` | every Command handed off | `EffectsComplete` |
| `EffectsComplete` | `TurnCompleted(Continue)` | checkpoint witness; answer was `Continue` | `BetweenTurns` |
| `EffectsComplete` | `StopRequested` | checkpoint witness; answer was `Stop` | `StopPending` |
| `BetweenTurns` | `EventAccepted` | a candidate; `ENV-TIME`'s nondecrease, checked inside the transition — violation is `Core(TimeRegression)` and the candidate stays consumed | `TurnOpen` |
| `StopPending` | `TurnCompleted(Stop)` | quiescence witness | `Closed` |

### Records

`RUN-RECORDS` fixes the wire shape; the table fixes each record's fields. Example:
`{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}`.

| Record | Fields after `record_kind`, `index` | Committed | Evidences |
|---|---|---|---|
| `RunStarted` | `schema_version`, `logical_time` | Before `on_start`. | Acceptance of the start turn, index 0, at the start time. |
| `EventAccepted` | `logical_time`, `event` | Before `on_event`. | Acceptance of one External Event. |
| `CommandsPrepared` | ordered `commands` | Before the first handoff of a nonempty batch. | The turn's complete Command intent. |
| `CommandsDispatched` | — | After the last handoff of a nonempty batch. | Every prepared Command was handed off. |
| `StopRequested` | — | After a `Stop` answer, before `shutdown`. | The Application requested shutdown. |
| `TurnCompleted` | `outcome` (`Continue`/`Stop`) | End of every non-Fatal turn. | The turn's outcome. |

`EngineExit` is the run's only outcome channel: a fatal run's Journal simply ends at its
last committed record, and `CommandsPrepared` plus the typed `Dispatch { position }`
identify the exact handed-off prefix. Records carry indices, times, Events, Commands,
and outcomes and nothing else (`RUN-RECORDS`), so Journal bytes are
Environment-independent (`DET-ENV`). The concrete Rust record types are mechanism;
`RUN-RECORDS` and the table bind the serialized form.

### Guarantees

| ID | Guarantee |
|---|---|
| `RUN-SERIAL` | The Engine owns the Environment and the Journal by value and is their only caller, delivering `ENV-SERIAL` by construction: one serial loop (A2), calls in the order the graph directs, and a consuming `shutdown` that makes a second lifecycle call unrepresentable. |
| `RUN-GRAMMAR` | Records are committed only through the graph's transitions, and the graph is enforced at compile time: possession of the token in phase S proves the run is non-Fatal, the Journal holds exactly the records of the token's path to S, and the token's index and last accepted time are the run's. An out-of-order record, a record whose kind disagrees with its payload, a wrong `TurnCompleted` outcome, a wrong index, a skipped checkpoint, or a `TurnCompleted(Stop)` without witnessed quiescence fails to compile. |
| `RUN-RECORDS` | A record is one flat JSON object: `record_kind` first — a bare tag string naming the kind — then `index`, then its row's fields in table order; those fields and only those. `schema_version` is 1. `RunStarted` is the only possible first record, so every nonempty Journal begins with a versioned record. |
| `RUN-INDEX` | The token's index is the accepted count: 0 for the start turn, advancing exactly when `EventAccepted` commits. The bound is the index domain itself, checked before `next_event`; at the boundary the run ends `Core(IndexExhausted)` with no candidate consumed. Overflow past that check is an invariant panic. |
| `RUN-CHECKPOINT` | Every turn observes the latch exactly once — after its last handoff (immediately after the handler when the batch is empty) and before its completion record. A pending Error there is `Environment(Checkpoint)` Fatal. On the Stop path no latch-observing operation follows the checkpoint; on the Continue path a later publication stays pending for the next observing operation (`ENV-LATCH`). |
| `RUN-FINALIZE` | Fatal finalization runs exactly once: fix the first-observed cause (A4); fix quiescence — Environment started and unconsumed → call `shutdown` and take its result; consumed by the Stop path → reuse that call's result; `start` returned `Err` → `Quiesced` (`ENV-START`); return `EngineExit::Fatal { state, cause, quiescence }`. |
| `DET-RUN` | Within one Environment type: the same build, Application, initial State, configuration, and trace reproduce the same handler calls, State transitions, Command intent, and Journal bytes, and exits equal in every Core-owned discriminant — exactly equal when the concrete Error values erased from the trace also match. |
| `DET-ENV` | Across Environment types: equal traces produce equal handler calls, State transitions, Command intent, and Journal bytes, and exits equal in every Core-owned discriminant — `FatalCause` variant, `EnvironmentOperation`, `RecordKind`, `JournalError` variant and `SinkOperation`, `CoreError` including payloads. Only Error values inside the exit may differ; they are erased from the trace. |

### Enforcement

The mechanism behind `RUN-GRAMMAR`. All of it is module-private inside the engine;
`RecordKind` and `JournalFatal` are defined here and re-exported publicly.

The token:

```rust
pub(super) struct Recorder<W: std::io::Write, S> {
    journal: Journal<W>,   // the run's one Journal, consumed at minting
    index: EventIndex,     // the accepted count (RUN-INDEX)
    last_time: Timestamp,  // the last accepted logical time
    _phase: PhantomData<fn() -> S>,
}
```

No `Clone`, `Copy`, or `Default`. Minting consumes the run's Journal, so a second
grammar over it is unconstructible; dropping the token destroys the Journal. The
`fn() -> S` marker keeps `Send`/`Sync` independent of the phase. `index()` and
`logical_time()` are the getters `Context` construction reads.

Transitions — each consumes the token, builds its payload from the token's own index,
commits, and returns the next phase only on success:

| Token | Method | Record | Returns |
|---|---|---|---|
| `Initial` | `run_started(start_time)` | `RunStarted` | `TurnOpen` at index 0 |
| `BetweenTurns` | `accept_event(time, &event)` — derives the next index and enforces `ENV-TIME`'s nondecrease (`time >= last_time`) | `EventAccepted` | `TurnOpen`; `Err` is `Regression { previous, offered }` or `Journal(JournalFatal)` |
| `TurnOpen` | `no_commands()` — infallible, no commit | — | `EffectsComplete` |
| `TurnOpen` | `prepare_commands(&[C])` — asserts nonempty | `CommandsPrepared` | `Prepared` |
| `Prepared` | `commands_dispatched()` | `CommandsDispatched` | `EffectsComplete` |
| `EffectsComplete` | `complete_continue(checkpoint witness)` | `TurnCompleted(Continue)` | `BetweenTurns` |
| `EffectsComplete` | `request_stop(checkpoint witness)` | `StopRequested` | `StopPending` |
| `StopPending` | `complete_stop(quiescence witness)` | `TurnCompleted(Stop)` | `Closed` |

One payload struct per record, each deriving `Serialize`, first field
`record_kind: Self::KIND` from a shared `RecordPayload` trait — the serialized tag and
a `JournalFatal`'s kind have one source and cannot diverge. `TurnOutcome` is chosen by
the transition, never its caller.

The witnesses are two affine, module-private types with exactly one constructor each:
the checkpoint witness is minted by the helper that wraps `take_error` (a `Some` becomes
the `Environment(Checkpoint)` path instead); the quiescence witness is minted by the
helper that wraps `shutdown` (an `Incomplete` becomes the `ShutdownIncomplete` path
instead). Forgetting the checkpoint, or completing a stop before quiescence, leaves the
caller with nothing to pass.

The proof's boundary, honestly:

- **Affinity, not linearity.** Dropping a token and committing nothing type-checks —
  that is the Fatal path by design — so a record *omitted* where the graph requires one
  is caught by golden-Journal tests, never the compiler.
- **Payload content** beyond kind, outcome, index, and time is assertion and test
  territory. Residual always-on asserts: `prepare_commands` rejects an empty slice; a
  freshly minted Recorder sits at the start index; the recordless edge asserts the
  batch it bypasses is empty — a nonempty batch there is a bug, not a silent drop.
- **Unforgeable means module-private.** The token, phases, transitions, witnesses, and
  the Journal field hold their guarantees exactly as long as they stay behind their
  modules.
- **The wire format** (`RUN-RECORDS`) is pinned by byte-exact golden tests.

### Notes

*Derive — the certificate's corollaries.* After any Fatal, no commit is expressible: the
token is gone and the Journal was destroyed with it. The next Event is acquirable only
after `TurnCompleted(Continue)` commits: no other edge yields `BetweenTurns`. No handler
runs before its acceptance record: only `RunStarted` and `EventAccepted` yield
`TurnOpen`. `Stopped` implies `Quiesced`: `Closed` is reachable only through the
quiescence witness.

*Derive:* the empty batch takes a recordless edge because it brackets no effect —
nothing was prepared, nothing handed off. A5 fixes every other record's position:
acceptance and intent records precede their effects; `CommandsDispatched` and
`TurnCompleted` witness completed ones.

*Derive:* `CommandsDispatched` can be a run's final record — the checkpoint that
follows it observed a pending Error.

*Derive:* an Application that wants stop-specific Port behavior emits it as Commands
before answering `Stop`; the shutdown signal carries only its glossary meaning.

*Derive:* a candidate consumed by `next_event` becomes accepted only when
`EventAccepted` commits; a candidate lost to `TimeRegression` or a failed commit never
had an index — indices exist only inside the token.

*Derive:* an Environment may resolve races among concurrent sources however it likes;
the accepted trace records the resolution, and `DET-RUN` holds conditional on it.

*Justify:* the index domain is the run bound because it is the one non-arbitrary bound
available: it makes `RUN-INDEX`'s overflow argument a domain fact rather than a
configuration promise, and a harness that wants a tighter bound composes one in user
code — a counting Port, or an Environment wrapper.

## 8. Live Environment

The live Environment runs each bound Port on its own supervised thread and bridges
concurrent reality into the serial Environment contract. The Core's boundary is that
contract; this section ships one implementation of it — every guarantee below realizes
a named contract row or defines the live Port-facing API.

### API

Semantics here are normative; the exact `LiveCtx` signatures are provisional until the
open Wiring section settles construction.

```rust
pub trait LivePort<C: PortContract>: Send + 'static {
    type Error: Send + 'static;
    fn run(self, ctx: LiveCtx<C>) -> Result<(), Self::Error>;
}

impl<C: PortContract> LiveCtx<C> {
    /// Block until one Command arrives or the shutdown signal is raised.
    pub fn recv(&mut self) -> PortInput<C::Command>;
    /// Nonblocking: pending Commands first, then the signal.
    pub fn try_recv(&mut self) -> Option<PortInput<C::Command>>;
    /// Offer one Event through the Slot's frozen fan-in constructor.
    /// Never waits for future capacity.
    pub fn offer(&mut self, event: C::Event) -> Result<(), OfferRejected>;
    /// Direct observation of lifecycle signaling.
    pub fn lifecycle(&self) -> Lifecycle;
}

pub enum PortInput<Cmd> { Command(Cmd), Shutdown }
pub enum OfferRejected { Full, Closed }
pub enum Lifecycle { Running, Shutdown }
```

### Guarantees

| ID | Guarantee |
|---|---|
| `LIVE-THREADS` | Each bound Port runs in one supervised thread and owns its native client and all domain and protocol state. Everything crossing a Port-thread boundary — values moved in, Commands in, offered Events out, Port Errors out — is `Send + 'static`. |
| `LIVE-EVENTS` | Event fan-in is one bounded queue. Mapping into the Application Event sum precedes admission. `offer` never waits; `Full` or `Closed` is reported to the offering Port, which may recover or return an Error to latch. |
| `LIVE-SELECT` | `next_event` waits, without busy-spinning, until the latch is pending or one Event is available; the choice between them follows `ENV-LATCH`'s linearization. |
| `LIVE-TIME` | The single acceptor stamps from one monotonic clock, realizing `ENV-TIME`'s nondecrease structurally; duration conversion is checked (A6) and exhaustion is a typed Environment Error. |
| `LIVE-DISPATCH` | Each destination Port owns one bounded Command inbox; one non-waiting admission to it is where `dispatch`'s handoff commits (commitment table), linearized against Error publication per `ENV-LATCH`. |
| `LIVE-SUPERVISION` | `run(Err)` and `run` completing while `Running` (premature closure) each publish a typed Error to the latch (`ENV-LATCH` publication) and wake a blocked `next_event`. The transition out of `Running` is linearized with Port completion, so every completion is unambiguously premature or expected. Completion after that transition, and every later Error, are shutdown work and stay unpublished (A4). |
| `LIVE-LIFECYCLE` | The shutdown signal is `LiveCtx` authority — it consumes no queue or inbox capacity. Once raised, `recv` reports it ahead of that Port's queued Commands — `ENV-SHUTDOWN`'s observability in its strongest form; `try_recv` yields queued Commands first and the signal after them, which is the draining path; `lifecycle` reads it directly. |
| `LIVE-START` | Every spawned supervisor shell waits at one start/cancel gate and cannot invoke `LivePort::run` while the gate is pending. Setup failure publishes cancel, wakes and joins every shell, and returns `Err` with no Port code ever run — realizing `ENV-START`. After all fallible setup and start-time stamping succeed, publishing start is the commitment; no fallible startup work follows it. A Port failure after publication is a runtime failure, surfacing per `ENV-LATCH`. |
| `LIVE-SHUTDOWN` | `shutdown` realizes `ENV-SHUTDOWN`: it publishes the signal, closes Engine-facing admission and the latch (`ENV-LATCH` closed), and wakes every Kavod-owned blocking point. It waits at most the shutdown deadline — this Environment's configured bound on shutdown waiting — joining finishers, detaching stragglers at the deadline, and discarding their Errors (A4). It returns `Quiesced` exactly when every supervised thread was joined. |

### Mechanism

One workable mechanism, replaceable wherever the guarantees hold: a bounded channel for
fan-in; one bounded SPSC inbox per destination Port; a supervisor-owned latch
(`Mutex` + `Condvar`, or an equivalent channel) that fan-in waiting and supervision
both wake; one start/cancel gate shared by the supervisor shells; a lifecycle cell the
`LiveCtx` blocking points check first.

| Step | `start` | 
|---|---|
| 1 | Freeze Slot order and capacities; create queue, inboxes, latch, lifecycle cell, completion tracking, and the pending gate. |
| 2 | Spawn one thread per bound Port in frozen Slot order; each shell waits at the gate. |
| 3 | Complete every remaining fallible setup step; stamp and freeze the start time (`LIVE-TIME`). |
| 4 | Any failure so far: publish cancel, wake and join every shell, return `Err` (`LIVE-START`). |
| 5 | Publish start — the commitment — and wake every shell; each invokes `LivePort::run` and publishes its completion under `LIVE-SUPERVISION`. |
| 6 | Return the frozen start time. |

`next_event`: wait under `LIVE-SELECT`; a pending latch Error is taken, marked
reported, and returned (nothing consumed); otherwise stamp from the acceptor's
clock — a failed conversion is a typed Error, nothing consumed — then dequeue one
candidate and return it with that stamp; the dequeue is the consumption instant, and
nothing fallible follows it. `dispatch`: a pending latch
Error returns first (`ENV-LATCH`); otherwise route by the fan-out match
(`PORT-ROUTING`) and try one non-waiting admission — full or closed is a typed `Err`
with nothing handed off. `take_error`: one atomic snapshot per its commitment row.
`shutdown`: per `LIVE-SHUTDOWN`, with a timed wait against the deadline from the
monotonic clock, joining completed threads in Slot order.

The supervision shell runs on the Port's own thread: wait at the gate; cancel returns
without invoking the Port; start invokes it and maps `Err` or premature completion into
a typed Error published first-wins to the latch, waking the select.

### Notes

*Derive:* `Quiesced` here is a full witness — every supervised thread was joined, so
every Port finished entirely, destructors included. That is what settles user-owned
handles captured before binding: an exit of `Stopped`, or `Fatal` with `Quiesced`,
means terminal Port state is readable through them.

*Derive:* after `Incomplete`, a detached thread may still be running and in-process
reclamation is impossible; the caller renders the exit and terminates promptly, and a
supervisor above the process reclaims it (Obligations table). The evidence is
the Journal's committed records.

*Justify:* Port work itself is unbounded and trusted (`BOUND-BLOCKING`), which is why
`shutdown` bounds its *wait*, and why Port blocking points observing the lifecycle is a
trusted obligation rather than a Kavod guarantee.

*Justify:* `try_recv` yields Commands before the signal so a draining Port can finish
queued work; the signal is never hidden — `lifecycle` and `recv` report it immediately —
so `ENV-SHUTDOWN`'s observability holds on every path.

## 9. Simulated Environment

The simulated Environment executes the same contract single-threaded under virtual
time; Ports advance only when stepped. This section ships the second implementation of
the Environment contract — every guarantee below realizes a named contract row or
defines the sim Port-facing API.

### API

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

### Guarantees

| ID | Guarantee |
|---|---|
| `SIM-STATE` | Each simulated Port owns all of its simulated domain state; the Environment holds no shared model and runs no concurrency. |
| `SIM-START` | `start` fixes the start time from the configured origin and sets `now` to it; immediately before the first Port `start` invocation is the startup commitment (with no Ports, successful return is). It calls every Port's `start` in frozen Slot order. On the first `Err` it calls `stop` on every Port whose `start` succeeded, in frozen Slot order, discarding their Errors, then fails startup — the failing Port's `Err` ends its lifecycle and no further call, `stop` included, reaches it; effects already made stay real (A3), and the return satisfies `ENV-START`. |
| `SIM-DISPATCH` | `dispatch` synchronously routes to exactly one Port's `on_command`; the invocation is where `dispatch`'s handoff commits (commitment table), and `now` does not advance. |
| `SIM-WAKEUP` | Each Port has at most one revocable wakeup arm, modifiable only through its own `SimCtx`: `set_next` requires `time >= now` — rejection changes nothing — and is last-call-wins; `clear_next` disarms. An arm is not an Event. |
| `SIM-SELECT` | `next_event` checks the latch (`ENV-LATCH`), then selects the armed Port with the lowest time — equal times by round-robin in frozen Slot order, the cursor advancing past the selected Port after every selected `step`, including one returning `None` — advances `now`, clears the arm, and calls `step`. Only `step(Some)` creates the returned candidate; `step(None)` continues selection; `step(Err)` returns that Error. |
| `SIM-STEPS` | Every `step` call consumes one unit of the configured step budget, fresh for each `next_event` invocation; `start`, `on_command`, and `stop` consume none. The budget is checked before selecting, advancing time, or clearing an arm; exhaustion is a typed Environment Error. |
| `SIM-COMPLETION` | `next_event` with no armed Port is a typed Environment Error: the run has nothing left to wait for. A run ends normally through the finite-source pattern (Ports Notes). |
| `SIM-SHUTDOWN` | `shutdown` realizes `ENV-SHUTDOWN`: it closes the latch (`ENV-LATCH` closed), calls every Port's `stop` in frozen Slot order, discarding their Errors (A4), and returns `Quiesced`. |

### Mechanism

Environment state: `now`, one `Option<Timestamp>` arm per Port, the round-robin cursor,
the latch, and a steps-used counter reset at each `next_event` entry. `dispatch`:
pending latch Error first (`ENV-LATCH`); otherwise route by the fan-out match and
invoke `on_command` — an `Err` from it lands in the latch and the `dispatch` still
returns `Ok`, because invocation already committed. `take_error`: one snapshot per its
commitment row; it is how an `Err` from a turn's final `on_command` reaches the run.

### Notes

*Derive, showing the method:* `on_command(Err)` is a failure after the handoff
commitment, so the Port's mutations stand, the Error latches, and the current
`dispatch` returns `Ok` — A3, applied through `ENV-LATCH`. Likewise `step(Err)` cannot
roll back the advanced `now`, the cleared arm, or Port mutations. Commands and earlier
equal-time turns may alter or cancel a later Port's arm before it fires; that is what
revocable means.

*Derive:* processing is synchronous, so quiescence is structural: `shutdown` always
returns `Quiesced`.

*Derive:* replay is user wiring: a fixed or recorded trace presented by a user-written
`SimPort`, or a bespoke `Environment` built on `Timestamp::from_nanos`, with `DET-RUN`
as the counterfactual it relies on. Sim-Port determinism and bounded `step` work are
trusted obligations (Obligations table).

## 10. Wiring & construction — OPEN

The one part of this document not ready for implementation. Decisions this section must
make, for both Environments and ideally with one shared answer where the question is
shared:

- The builder/registration API binding each Slot to one Port implementation in frozen
  Slot order — live: `LivePort` plus per-inbox and fan-in queue capacities; sim:
  `SimPort`.
- Where the frozen fan-in constructors and the hand-written fan-out match live, and how
  the builders receive them (`PORT-ROUTING`).
- Composition of each Environment's `Error` sum: Kavod-owned variants (live: queue
  exhaustion, time-domain exhaustion, premature closure; sim: nothing-armed, step-budget
  exhaustion, time-domain exhaustion) plus one mapped variant per Slot's Port Error.
- Final `LiveCtx` signatures, and how one is constructed against the chosen channels.
- `LiveConfig`: the shutdown deadline (nonzero milliseconds) and how the live time
  origin is anchored.
- `SimConfig`: the time origin and step budget (nonzero), and where it lives relative
  to `EngineConfig`.
- What fixes the Slot order: registration order, or the Slot sum's declaration order.
- The crate's public re-export policy at `lib.rs`.
- Thread naming conventions, if any.

Constraints already fixed: every guarantee in the Environment, Live, and Simulated
sections; the commitment table; `Send + 'static` boundaries; frozen Slot order as the
only ordering authority; nonzero configured bounds (A6); everything frozen before
`Engine::run`.

## 11. Crate layout

One crate, `kavod`, no feature gates — both Environments are std-only. Dependencies:
`serde` (with `derive`) and `serde_json`. `ports!` is `macro_rules!`, so no proc-macro
crate exists. This section is mechanism except the public item names, which the API
blocks own.

```
kavod/src/
  lib.rs             #![forbid(unsafe_code)]; public re-exports (policy: Wiring, open)
  time.rs            EventIndex, Timestamp
  application.rs     Application, Outcome, Context
  port.rs            PortContract, Never, ports!
  environment.rs     Environment, Quiescence
  journal.rs         Journal, JournalError
  bounded_buffer.rs  crate-internal fixed-capacity storage backing the Command
                     batch and the Journal's encode buffer
  engine/
    mod.rs           wiring only: module declarations plus public re-exports
    engine.rs        Engine, EngineConfig, EngineExit, FatalCause, CoreError
    record.rs        record payloads, the token, transitions, witnesses
                     (private; RecordKind and JournalFatal re-exported)
  live/              LivePort, LiveCtx, live Environment      (planned: Wiring)
  sim/               SimPort, SimCtx, simulated Environment   (planned: Wiring)
```

Every public item is reachable at a path without repeated segments — the engine
module's `mod.rs` re-exports its children's public items rather than exposing the
child modules.

## 12. Obligations & verification

Kavod enforces every ID outside this section. The rows below are **trusted**: upheld by
the named party and checked by the stated means. This table is the complete boundary —
an obligation absent from it is enforced, not assumed.

| ID / obligation | Upholder | Verified by |
|---|---|---|
| Handlers and State carry no hidden authority — clocks, entropy, IO, globals, concurrency order, Environment dependence — and all run-varying data lives in State | Application author | Simulated repeatability: same trace twice → identical Journal bytes and exit (`DET-RUN`) |
| Simulated Ports are deterministic, do bounded `step` work, and carry no hidden authority | Sim Port author | Repeatability tests |
| A bespoke Environment — one Kavod does not ship — upholds every Environment-contract row | Environment author | The conformance trace suite run against it |
| `BOUND-BLOCKING`: user code — handlers, Ports, serializers, writers, callbacks, destructors — is bounded and reports Errors instead of panicking | Their authors | Review; A8 defines the blast radius when violated |
| The final binary builds with `panic = "abort"` | Build/deployment configuration | Build profile review |
| One-to-one Slot routing and Error mapping (`PORT-ROUTING`) | Wiring author | Per-Slot tests |
| A stable business key on every externally consequential Command, sufficient to recognize a repeated or uncertain external effect | Application author | Per-Slot tests |
| `Serialize` impls are deterministic, side-effect-free, bounded, nonpanicking, with stable map order | Payload authors | Golden-Journal tests |
| Live Port blocking points observe the lifecycle and cooperate with shutdown | Live Port author | Shutdown tests under load |
| The process terminates promptly after an `Incomplete` exit, under a supervisor that reclaims it | Caller / deployment | Operational review |
| `BOUND-SIZING`: `max_record_bytes` fits the largest batch stageable under `max_commands_per_turn` | Deployment configuration | Config review |
| Transitive memory bounds of owned values | Value owner | Owner-defined |

Kavod-side verification conventions:

- One conformance trace suite runs against both Environments and compares every
  Core-owned discriminant (`DET-ENV`); run against a bespoke Environment, the same
  suite is its certification (Obligations table).
- Golden-Journal tests pin every record byte-exactly.
- Fault injection exercises every edge: scripted sinks for Journal failures, scripted
  Environments for each operation's `Err`, checking the resulting `FatalCause`.
- A compile-fail suite proves illegal transition sequences, a skipped checkpoint, and a
  premature `TurnCompleted(Stop)` do not compile (`RUN-GRAMMAR`).
- Live lifecycle tests prove: no `LivePort::run` begins before gate activation; failed
  startup cancels and joins every shell; `Quiesced` joins every supervised thread;
  deadline expiry returns `Incomplete` while detaching only unfinished threads.
- Environment conformance tests prove both sides of `ENV-LATCH`'s linearization,
  permanent first-Error reporting, final-Command simulated Error observation, and that
  `Stopped` follows only `Quiesced`.

## Appendix A. Invariant index

Navigation only.

| ID | Section |
|---|---|
| `APP-CONTEXT`, `APP-EMIT`, `APP-OVERFLOW`, `APP-FUTURE` | Application contract |
| `PORT-STATE`, `PORT-SUMS`, `PORT-ROUTING` | Port contract |
| `ENV-SERIAL`, `ENV-START`, `ENV-LATCH`, `ENV-TIME`, `ENV-SHUTDOWN`, `ENV-SEPARATION`, `ENV-BOUNDS` | Environment contract |
| `JRN-FORMAT`, `JRN-ENCODE`, `JRN-COMMIT`, `JRN-POISON`, `JRN-SINK` | Journal |
| `RUN-SERIAL`, `RUN-GRAMMAR`, `RUN-RECORDS`, `RUN-INDEX`, `RUN-CHECKPOINT`, `RUN-FINALIZE`, `DET-RUN`, `DET-ENV` | The Run |
| `LIVE-THREADS`, `LIVE-EVENTS`, `LIVE-SELECT`, `LIVE-TIME`, `LIVE-DISPATCH`, `LIVE-SUPERVISION`, `LIVE-LIFECYCLE`, `LIVE-START`, `LIVE-SHUTDOWN` | Live Environment |
| `SIM-STATE`, `SIM-START`, `SIM-DISPATCH`, `SIM-WAKEUP`, `SIM-SELECT`, `SIM-STEPS`, `SIM-COMPLETION`, `SIM-SHUTDOWN` | Simulated Environment |
| `BOUND-LOOPS` | Laws |
| `BOUND-BLOCKING`, `BOUND-SIZING` (trusted) | Obligations & verification |



