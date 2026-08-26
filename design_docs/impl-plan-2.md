# Kavod Core Implementation Plan

> **Approval gate:** Approve this plan before creating implementation files. Approval may
> accept `W1`-`W9` or leave them pending: C1-C49 can proceed while Wiring is open, but
> every explicitly §10-blocked chunk waits. Every chunk ends with both `cargo test` and
> `cargo clippy
> --all-targets --all-features -- -D warnings` green.
>
> **Test rule:** Every test named below is written from its first commit in the pattern
> required by `test.md`: unit tests live in `#[cfg(test)] mod tests` in the source file;
> cross-file suites live under `tests/`; every nested group is named
> `<subject>_<behavior>`; every test has a doc comment beginning `Invariant:` and cites
> the listed ID, or the listed binding-table/API name when no ID exists.

## 1. Blockers & Decisions Register

### 1.1 Wiring Decisions

These are proposals, not inferred requirements. Approval of all nine closes the only
known design-level implementation gate. `§10-blocked` chunks are identified in the build
plan.

| Decision | Proposed answer | Rationale and fixed constraints satisfied | Blocks |
|---|---|---|---|
| `W1` Builder/registration API | Add a `wiring!` `macro_rules!` macro. Its nonempty Slot list is written once; the macro invokes the exact existing `ports!` expansion and generates a zero-sized namespace such as `TradingWiring`. Associated functions `TradingWiring::sim(config, bindings...)` and `TradingWiring::live(config, bindings...)` accept exactly one typed binding per Slot in declaration order. `SimBinding<C, P>::new(port)` and `LiveBinding<C, P>::new(port, inbox_capacity)` are associated constructors. Construction returns a typed Environment or a typed build error. A companion form accepts hand-written Event and Command sums plus the explicit Slot list. | One source controls generated sums and order; one-or-more macro repetition makes an empty Port set unrepresentable; fixed arguments make omission, duplication, and reorder visible at compile time; associated functions satisfy the house preference. `ports!` itself still expands to exactly two enums. | C50-C82 |
| `W2` Fan-in and fan-out location | `wiring!` generates one frozen Event constructor per inhabited Event direction and one exhaustive `match` over the Command sum. The generated Live/Sim wrappers own these functions. Typed arms hand payloads directly to the matching typed Slot adapter before any erasure. | Preserves payload agreement and exhaustive fan-out at the compiler boundary and routes by discriminant alone (`PORT-SUMS`, `PORT-ROUTING`, `PORT-STATE`). | C53-C57, C60, C65-C66, C72-C76, C78-C82 |
| `W3` Environment Error sums | Generate wiring-local `TradingSimError<...>` and `TradingLiveError<...>`. Both have one variant named for each Slot's Port Error. Sim also has `NothingArmed` and `StepBudgetExhausted`. Live also has `InboxFull { slot }`, `InboxClosed { slot }`, `ThreadSpawn { slot, error }`, `TimeDomainExhausted`, and `PrematureClosure { slot }`. Mapping happens in the generated typed Slot adapter. | Gives every Error one mapping site and preserves direct Slot identity without rendering errors to text (`PORT-ROUTING`, A7). Fan-in `Full` remains `OfferRejected`, never an Engine Error. | C50-C57, C58-C71, C73-C76, C78-C82 |
| `W4` Final `LiveCtx` | Keep the API-block method signatures unchanged. `LiveCtx<C>` owns one exclusive typed inbox receiver plus a private `Send` offer capability whose method accepts/returns `C::Event` while hiding the Application Event sum and frozen constructor inside its concrete implementation. It also owns a read-only lifecycle capability. It is non-cloneable and contains no completion capability or thread handle. | The erased operation, rather than an erased payload, solves the fact that exact `LiveCtx<C>` has no Application-Event type parameter. Payload mapping remains typed inside the generated capability and preserves `LIVE-LIFECYCLE`, `LIVE-EVENTS`, `PORT-SUMS`, and `LIVE-COMPLETION`. | C58-C71 and C75-C76 |
| `W5` `LiveConfig` and clock origin | `LiveConfig { fan_in_capacity: NonZeroUsize, shutdown_timeout_ms: NonZeroU64 }`. Per-inbox capacities live in each `LiveBinding`. At successful `start`, the acceptor freezes a monotonic `Instant` as origin and returns `Timestamp::from_nanos(0)`; later stamps are checked elapsed nanoseconds from that origin. Tests use the same private clock trait with an injected clock. | All configured bounds are nonzero; one acceptor owns monotonic stamps; the shutdown duration is explicitly nonzero milliseconds; no wall clock enters records. | C58, C63-C71, C76, C78-C82 |
| `W6` `SimConfig` placement | `SimConfig { origin: Timestamp, step_budget: NonZeroUsize }`, owned by the Sim Environment and separate from `EngineConfig`. `TradingWiring::sim` consumes it before the Engine is built. | Keeps run bounds with their accounting owners and freezes all configuration before `Engine::run`. | C50-C57, C74, C78-C82 |
| `W7` Slot-order authority | The Slot list in `wiring!` is the sole order authority. For generated sums it is also their declaration order because `wiring!` invokes `ports!`. The same stored ordinal drives start, equal-time selection, dispatch metadata, shutdown, joins, thread names, and Error mapping. Hand-written-sum wiring repeats the list and is covered by per-Slot `TRUST-ROUTING` tests. | Uses declaration order where one source is possible and acknowledges the document's trusted boundary where hand-written sums necessarily duplicate it. | C51-C57, C58-C71, C72-C82 |
| `W8` Public re-exports | Re-export the public time, Application, Environment, Journal, Engine, Port, Live, Sim, config, binding, and build-error items at crate root. Export `ports!` and `wiring!` at crate root. Keep `engine::engine`, `engine::record`, phase/certificate types, bounded storage, latches, clock traits, and typed-erasure adapters private. Directory `mod.rs` files contain declarations and re-exports only. | Gives every public item a path without repeated segments and keeps the grammar boundary private (`CRATE-EXPORTS`, `RUN-ENFORCEMENT`). | C72 and C77 |
| `W9` Thread names | Name supervisor threads `kavod-port-{ordinal}-{slot_name}`. Names are diagnostic only and never enter the Journal. A spawn failure carries the Slot and original `std::io::Error`. | Stable troubleshooting without adding a deterministic output or protocol fact. | C63 and C76 |

### 1.2 Genuine Blockers Found

| Quoted binding text | Why conforming code cannot proceed | Smallest resolution |
|---|---|---|
| None outside the declared open Wiring section. | Every settled API block and named interaction has either an executed compiler probe below or a direct Rust mechanism. Every `VERIFY-*` bullet has a concrete harness and observable assertion. | Approve or amend `W1`-`W9`; no settled design rule needs clarification. |

The empty blocker result is deliberate. In particular, `TryReserveError` propagation can
be tested with a capacity-overflow request; an interior literal newline can be produced by
`serde_json::value::RawValue`; `SimCtx<'_, C>` is accepted in an inherent impl; and the
certificate/Environment ownership split compiles. Those are implementation choices or
executed answers, not blockers.

### 1.3 Free Implementation Choices

| Choice | Recorded answer |
|---|---|
| Fixed storage | Use `Vec<T>` with one successful `try_reserve_exact` at construction and a separate immutable logical limit. Never call a growing operation after construction. Implement `Write` only for the byte specialization. |
| Journal overflow signal | The byte buffer sets a private `bound_hit` flag when `Write::write` returns `Ok(0)` at its logical limit. `Journal` maps a serde failure with that flag to `BoundExceeded`; all other serde failures remain `Encode`. |
| Interior-newline fixture | Enable `serde_json`'s `raw_value` feature through the test dependency and use `RawValue` only in tests. This adds no dependency and produces valid JSON bytes containing insignificant literal whitespace. |
| Record implementation | Use one private `RecordPayload::KIND` associated constant and a zero-sized `Kind<P>` first field. The same constant supplies `JournalFatal.record_kind`; the outcome marker supplies both payload and fatal metadata. |
| Certificate shape | `Certificate<W, P>` owns `Journal<W>` and uses `PhantomData<fn() -> P>`. Phase wrappers carry only data established by their incoming edge. Every transition consumes its source. |
| Engine control flow | Destructure `self`; keep the live Environment in `Option<E>` only at the orchestration boundary where Stop may consume it. Transition methods continue to take `&mut E`, except close, which consumes `E`. |
| Latch ownership | Use one reusable, unsynchronized `ErrorLatch<E>` state machine. Sim owns it directly. Live stores it under the same `Mutex` as lifecycle and completion so publication, Complete, deadline classification, and close can share one critical section. |
| Live queues | Use preallocated `VecDeque` queues under the shared `Mutex`/`Condvar`, with explicit logical capacities. This avoids trying to atomically coordinate unrelated channel implementations. |
| `LiveCtx` Event erasure | Erase the offer operation behind a private `Send` capability, not the Event payload. Its concrete generated implementation owns the typed constructor and shared `AppEvent` queue; its trait surface mentions only `C::Event`, matching exact `LiveCtx<C>`. |
| Live completion count | Do not cache one. Scan the fixed completion set; this removes a derivative invariant and its possible drift. |
| Live deadlines | Use a private `Deadline::{Finite(Instant), Infinite}`; checked addition overflow selects `Infinite`. Every wait asks one budget for remaining time. |
| Sim storage | Store one fixed typed-erasure adapter per Slot after `wiring!` has proved payload types. Lifecycle, arm, and Port stay in the same fixed entry. No entry is added after construction. |
| Compile-fail diagnostics | Assert that each named fixture fails for its marked attack expression and that a control fixture compiles; do not snapshot compiler wording. |
| Module layout | Keep leaf modules in the documented files. For `engine/`, `sim/`, and `live/`, keep `mod.rs` wiring-only and put implementation/tests in subject files such as `context.rs`, `latch.rs`, `environment.rs`, and `supervision.rs`. |

## 2. Probe Results

All eleven executed probes lived in `/tmp/opencode/kavod-probes`, outside the repository. On
Rust 1.96.1, `cargo check --bins` and every listed `cargo run --bin ...` succeeded. The
only warnings were dead-code and private-interface warnings caused by intentionally small
fixtures. The snippets below preserve the material part of each probe for reuse.

### P1: Context lifetime and reusable buffer

- **Status:** Executed.
- **Tested:** `Context<'_, C>` borrowing a reusable command buffer, scoped handler access,
  sticky overflow storage, checked timestamp addition, and reuse on a second turn.
- **Verdict:** Compiles and runs; the handler borrow ends before the buffer is drained or
  reset.

```rust
struct Buffer<C> { values: Vec<C>, limit: usize, overflowed: bool }
struct Context<'a, C> { batch: &'a mut Buffer<C>, index: u64, time: u64 }

impl<C> Context<'_, C> {
    fn emit(&mut self, command: C) {
        if self.batch.overflowed { return; }
        if self.batch.values.len() == self.batch.limit {
            self.batch.overflowed = true;
            return;
        }
        self.batch.values.push(command);
    }
}

fn turn(batch: &mut Buffer<u8>) {
    batch.values.clear();
    batch.overflowed = false;
    {
        let mut ctx = Context { batch, index: 0, time: 1 };
        ctx.emit(7);
        assert_eq!((ctx.index, ctx.time), (0, 1));
    }
    assert_eq!(batch.values.drain(..).collect::<Vec<_>>(), [7]);
}
```

### P2: `Never` and exact `ports!` expansion

- **Status:** Executed.
- **Tested:** Uninhabited serialization, projected associated payloads, repeated Contract
  slots, macro path hygiene, and externally tagged bytes.
- **Verdict:** `match *self {}` and `kavod::ports!` both compile; generated bytes are the
  expected externally tagged object.

```rust
extern crate self as kavod;
use serde::{Serialize, Serializer};

pub trait PortContract { type Event: Serialize; type Command: Serialize; }
pub enum Never {}
impl Serialize for Never {
    fn serialize<S>(&self, _: S) -> Result<S::Ok, S::Error>
    where S: Serializer { match *self {} }
}

#[macro_export]
macro_rules! ports {
    ($vis:vis enum $doc:ident<Event=$event:ident, Command=$command:ident>
     { $($variant:ident($contract:ty)),* $(,)? }) => {
        #[derive(::serde::Serialize)]
        $vis enum $event { $($variant(<$contract as kavod::PortContract>::Event)),* }
        #[derive(::serde::Serialize)]
        $vis enum $command { $($variant(<$contract as kavod::PortContract>::Command)),* }
    };
}
```

### P3: Consuming `Engine::run(self)` and `shutdown(self)`

- **Status:** Executed.
- **Tested:** Destructuring `Engine`, creating State before a fallible start, mutably
  borrowing the Environment, and consuming it exactly once for shutdown.
- **Verdict:** Compiles and runs without cloning or moving State out of an exit path.

```rust
trait App { type State; fn initial_state(&self) -> Self::State; }
trait Env {
    type Error;
    fn start(&mut self) -> Result<(), Self::Error>;
    fn shutdown(self) -> bool;
}
struct Engine<A, E, W> { app: A, env: E, writer: W }
enum Exit<S, EE> { Stopped(S), Fatal(S, EE, bool) }

impl<A: App, E: Env, W> Engine<A, E, W> {
    fn run(self) -> Exit<A::State, E::Error> {
        let Engine { app, mut env, writer } = self;
        let state = app.initial_state();
        let _journal = writer;
        if let Err(error) = env.start() {
            return Exit::Fatal(state, error, true);
        }
        let quiesced = env.shutdown();
        assert!(quiesced);
        Exit::Stopped(state)
    }
}
```

### P4: Live and Sim API lifetime/bound shapes

- **Status:** Executed.
- **Tested:** `LivePort`'s `Send + 'static` bounds, consuming receiver, the complete
  `SimPort` receiver shapes, and `impl<C> SimCtx<'_, C>`.
- **Verdict:** All exact bound and lifetime forms compile.

```rust
trait LivePort<C: PortContract>: Send + 'static
where C::Event: Send + 'static, C::Command: Send + 'static {
    type Error: Send + 'static;
    fn run(self, ctx: LiveCtx<C>) -> Result<(), Self::Error>;
}

impl<C: PortContract> SimCtx<'_, C> {
    fn now(&self) -> Timestamp { Timestamp(self.now.0) }
}

trait SimPort<C: PortContract> {
    type Error;
    fn start(&mut self, ctx: &mut SimCtx<'_, C>) -> Result<(), Self::Error>;
    fn on_command(&mut self, command: C::Command,
        ctx: &mut SimCtx<'_, C>) -> Result<(), Self::Error>;
    fn step(&mut self, ctx: &mut SimCtx<'_, C>)
        -> Result<Option<C::Event>, Self::Error>;
    fn stop(&mut self) -> Result<(), Self::Error>;
}
```

### P5: Certificate owns Journal while transition borrows Environment

- **Status:** Executed.
- **Tested:** Affine certificate consumption, Journal ownership across a dispatch loop,
  mutable Environment borrowing, Command moves, and successor construction.
- **Verdict:** Compiles and runs; no self-referential or overlapping borrow is needed.

```rust
struct Certificate<W, P> {
    journal: Journal<W>,
    _phase: PhantomData<fn() -> P>,
}

impl<W> Certificate<W, TurnOpen> {
    fn dispatch_batch<E, C>(mut self, env: &mut E, commands: Vec<C>)
        -> Result<Certificate<W, EffectsComplete>, Fatal<E::Error>>
    where E: Environment<C> {
        assert!(!commands.is_empty());
        self.journal.commit().map_err(|_| Fatal::Journal)?;
        for (position, command) in commands.into_iter().enumerate() {
            env.dispatch(command)
                .map_err(|error| Fatal::Dispatch { position, error })?;
        }
        self.journal.commit().map_err(|_| Fatal::Journal)?;
        Ok(Certificate {
            journal: self.journal,
            _phase: PhantomData,
        })
    }
}
```

### P6: Record kind marker and `JournalFatal` share one source

- **Status:** Executed.
- **Tested:** A zero-sized first field using `RecordPayload::KIND`, exact field order and
  bare tags, and the same associated constant/outcome supplying fatal metadata.
- **Verdict:** Compiles and emits
  `{"record_kind":"TurnCompleted","index":3,"outcome":"Stop"}` exactly.

```rust
trait RecordPayload {
    const KIND: RecordKind;
    fn outcome(&self) -> Option<TurnOutcome> { None }
}
struct Kind<P>(PhantomData<fn() -> P>);
impl<P: RecordPayload> Serialize for Kind<P> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer { serializer.serialize_str(P::KIND.tag()) }
}

#[derive(Serialize)]
struct Completed {
    record_kind: Kind<Completed>,
    index: u64,
    outcome: TurnOutcome,
}
impl RecordPayload for Completed {
    const KIND: RecordKind = RecordKind::TurnCompleted;
    fn outcome(&self) -> Option<TurnOutcome> { Some(self.outcome) }
}

fn fatal<P: RecordPayload>(record: &P) -> JournalFatal {
    JournalFatal { record_kind: P::KIND, outcome: record.outcome() }
}
```

### P7: Bounded encode buffer implementing `Write`

- **Status:** Executed.
- **Tested:** Direct `serde_json::to_writer` encoding, short successful writes into the
  remaining region, and distinguishable zero progress at the bound.
- **Verdict:** A fitting object encodes exactly; an oversized object stops at the fixed
  length and returns an IO-category serde error with `bound_hit = true`.

```rust
struct Bounded { bytes: Vec<u8>, limit: usize, hit_bound: bool }
impl Write for Bounded {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let remaining = self.limit - self.bytes.len();
        if remaining == 0 {
            self.hit_bound = true;
            return Ok(0);
        }
        let count = remaining.min(input.len());
        self.bytes.extend_from_slice(&input[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

let mut bounded = Bounded {
    bytes: Vec::with_capacity(8), limit: 8, hit_bound: false,
};
let error = serde_json::to_writer(
    &mut bounded, &Record { payload: "too long" },
).unwrap_err();
assert!(error.is_io() && bounded.hit_bound && bounded.bytes.len() == 8);
```

### P8: Publication-before-Complete and final close under one lock

- **Status:** Executed.
- **Tested:** Worker publication and completion in one critical section, Condvar waiting,
  and shutdown's final completion scan/latch close/report extraction in one critical
  section.
- **Verdict:** The final observer cannot count Complete while missing the earlier Error.

```rust
#[derive(Default)]
struct State { error: Option<&'static str>, complete: bool, closed: bool }
let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
let shell = Arc::clone(&shared);
let worker = thread::spawn(move || {
    let (lock, wake) = &*shell;
    let mut state = lock.lock().unwrap();
    if !state.closed && state.error.is_none() {
        state.error = Some("port failed");
    }
    state.complete = true;
    wake.notify_all();
});

let (lock, wake) = &*shared;
let mut state = lock.lock().unwrap();
while !state.complete { state = wake.wait(state).unwrap(); }
state.closed = true;
let report = (state.complete, state.error.take());
drop(state);
worker.join().unwrap();
assert_eq!(report, (true, Some("port failed")));
```

### P9: Literal interior-newline JSON fixture

- **Status:** Executed.
- **Tested:** Whether valid serde JSON can contain a literal newline for the required
  `NotAnObject` test rather than an escaped `\\n`.
- **Verdict:** `RawValue` with `serde_json/raw_value` preserves the literal whitespace;
  the bytes start with `{`, end with `}`, and contain `b'\n'`.

```rust
use serde_json::value::RawValue;

let raw = RawValue::from_string(
    "{\n\"record_kind\":\"Injected\",\"index\":0}".to_owned(),
).unwrap();
let encoded = serde_json::to_vec(&raw).unwrap();
assert!(encoded.starts_with(b"{"));
assert!(encoded.ends_with(b"}"));
assert!(encoded.contains(&b'\n'));
```

### P10: One-source `wiring!` wrapping exact `ports!`

- **Status:** Executed.
- **Tested:** A nonempty macro pattern invoking `ports!`, preserving generated Event and
  Command enums, and deriving frozen Slot metadata in declaration order.
- **Verdict:** Nested expansion compiles and the frozen names are
  `["Primary", "Secondary"]`.

```rust
macro_rules! wiring {
    ($vis:vis wiring $name:ident, $vis2:vis enum $doc:ident
     <Event=$event:ident, Command=$command:ident>
     { $first:ident($first_contract:ty)
       $(, $variant:ident($contract:ty))* $(,)? }) => {
        kavod::ports!($vis2 enum $doc<Event=$event, Command=$command> {
            $first($first_contract) $(, $variant($contract))*
        });
        $vis struct $name;
        impl $name {
            const SLOT_NAMES: &'static [&'static str] = &[
                stringify!($first) $(, stringify!($variant))*
            ];
        }
    }
}

wiring!(pub wiring TradingWiring, pub enum Trading
    <Event=TradingEvent, Command=TradingCommand> {
        Primary(Duplex), Secondary(Duplex),
    }
);
assert_eq!(TradingWiring::SLOT_NAMES, ["Primary", "Secondary"]);
```

### P11: Exact `LiveCtx<C>` hiding the Application Event type

- **Status:** Executed.
- **Tested:** A non-cloneable `LiveCtx<C>` whose type mentions only `C`, while a private
  `Send` capability maps `C::Event` into a hidden Application Event sum and can return the
  original typed Event on Full/Closed.
- **Verdict:** Compiles and runs; erasing the operation avoids adding a forbidden public
  generic while payload mapping remains statically typed.

```rust
trait OfferCapability<E>: Send {
    fn offer(&mut self, event: E) -> Result<(), Rejected<E>>;
}
struct MappedOffer<E, Sum> {
    shared: Arc<Mutex<Shared<Sum>>>,
    map: fn(E) -> Sum,
}
impl<E: Send, Sum: Send> OfferCapability<E> for MappedOffer<E, Sum> {
    fn offer(&mut self, event: E) -> Result<(), Rejected<E>> {
        let mut shared = self.shared.lock().unwrap();
        if shared.closed { return Err(Rejected::Closed(event)); }
        if shared.queue.len() == shared.limit {
            return Err(Rejected::Full(event));
        }
        shared.queue.push_back((self.map)(event));
        Ok(())
    }
}
struct LiveCtx<C: Contract> {
    offer: Box<dyn OfferCapability<C::Event>>,
    _contract: PhantomData<fn() -> C>,
}
impl<C: Contract> LiveCtx<C> {
    fn offer(&mut self, event: C::Event) -> Result<(), Rejected<C::Event>> {
        self.offer.offer(event)
    }
}
```

### API Realizability Ledger

| API block | Result | Rust mechanism |
|---|---|---|
| `EventIndex`, `Timestamp`, `Application`, `Outcome`, `Context` | Executed for derives, checked arithmetic, associated types, and lifetime interaction; remaining getters are reasoned-direct. | Private newtypes; derived newtype serialization; checked `u128` to `u64`; scoped `&mut CommandBuffer<C>`. |
| `PortContract`, `Never`, `ports!` | Executed. | Associated types; uninhabited match; `macro_rules!` repetition and projected types. |
| `Environment`, `ShutdownReport`, `Quiescence` | Executed for consuming lifecycle shape in P3; value declarations are reasoned-direct. | Exclusive `&mut self` operations and consuming `shutdown(self)`. |
| `Journal` and its Error enums | Reasoned; its nonstandard interaction was executed in P7/P9. | `W: Write`, `NonZeroUsize`, reusable byte buffer, typed error mapping. No signature-level ambiguity exists. |
| `Engine` and exit/fatal enums | Executed for generic bounds and consuming run shape in P3/P5; enum declarations are reasoned-direct. | Owned `A/E/W`, associated-type equalities, private typestate, generic exit values. |
| `LivePort`, `LiveCtx`, Port-facing enums | Executed for bounds/signatures in P4; concrete `LiveCtx` fields await approved `W4`. | `Send + 'static`, consuming Port, typed capabilities. |
| `SimPort`, `SimCtx`, `SimCtxError` | Executed for bounds/signatures and anonymous impl lifetime in P4. | Scoped mutable capability with per-Slot arm borrow. |

### Unrepresentable Mechanism Checklist

| Claim | Exact mechanism |
|---|---|
| Zero configured capacity | Public/configuration fields use `NonZeroUsize` or `NonZeroU64`. |
| Empty Port set | `wiring!` matches one first Slot plus zero-or-more remaining Slots. |
| Mutable topology after construction | Private fields and no registration mutator on a built Environment. |
| Handler receives another Kavod capability | Private `Context`; exact handler signatures expose no other Kavod value. |
| Absent direction carries a value | Uninhabited `Never`. |
| Open Slot sums or wrong payload type | Concrete enums with projected Contract payloads; exhaustive match. |
| Concurrent Engine calls into Environment | Engine owns `E`; transitions receive one serial `&mut E`. |
| Second shutdown | `shutdown(self)` consumes `E`. |
| Second grammar over one Journal | Initial certificate consumes and owns the only Journal. |
| Duplicate/fabricated certificate | Private fields/types, no `Clone`/`Copy`/`Default`, consuming transitions. |
| Out-of-order transition | Each private phase exposes only legal consuming methods. |
| Initial prospective values used by Context | No getters on `Certificate<Initial>`. |
| Record kind/payload mismatch | `Kind<P>` and `P::KIND`; no caller-supplied kind. |
| Completion outcome disagrees with answer | `classify` returns `TurnOpen<Continue>` or `TurnOpen<Stop>`; only marker-specific methods exist. |
| Caller supplies acceptance index/time/Event | `run_started` has no arguments; `accept_event` accepts only the Environment and derives all values. |
| Accepted values differ from record | One transition uses the same locals for payload and successor, constructed only after commit. |
| Skipped checkpoint | Only `checkpoint` yields `Checkpointed<A>`. |
| Independent/premature `CommandsDispatched` | Only `dispatch_batch` can commit it, after draining every Command successfully. |
| `TurnCompleted(Stop)` without clean report | `close` consumes the Environment and constructs `Closed` only from `{ Quiesced, None }`. |
| Commit after Fatal | Error consumes/drops certificate and therefore its Journal. |
| Event request before Continue completion | Only `complete_continue` yields `BetweenTurns`. |
| Handler before acceptance | Only successful acceptance transitions yield `TurnOpen`. |
| `Stopped` after unclean shutdown | Only `Closed` yields `Stopped`; only a clean report yields `Closed`. |
| Port reaches/delegates completion capability | Guard stays in private supervisor shell, outside Port and `LiveCtx`; non-cloneable Slot capability. |

### Always-On Assertion Checklist

These are the document-named assertion sites. Each uses `assert!`, `assert_eq!`, or an
always-on `expect`, never `debug_assert!`.

| Site | Assertion and owner |
|---|---|
| `Journal::commit` entry | Journal is not poisoned before any encode or sink call (`JRN-POISON`, `ASSERT-INVARIANTS`). |
| Initial transition | Prospective index equals 0 (`RUN-ENFORCEMENT`). |
| `accept_event` after the `u64::MAX` guard | Checked increment succeeds; failure panics as an invariant violation (`RUN-INDEX`). |
| Recordless batch edge | Actual reusable batch is empty (`RUN-ENFORCEMENT`, `ASSERT-INVARIANTS`). |
| `dispatch_batch` entry | Actual reusable batch is nonempty (`RUN-ENFORCEMENT`, `ASSERT-INVARIANTS`). |
| Sim immediately before selected `step` | Selected lifecycle is `Open` (`SIM-LIFECYCLE`, `ASSERT-INVARIANTS`). |

## 3. Chunked Build Plan

For every `Proves` entry, write the test exactly as:

```rust
mod subject_behavior {
    use super::*;

    /// Invariant: ID — the specific observable statement listed below.
    #[test]
    fn observable_behavior() { /* ... */ }
}
```

If a binding table or API name is listed instead of an ID, use that name after
`Invariant:`. A chunk does not claim a `VERIFY-*` row complete until the suite map says
the final contributing chunk has landed.

### Phase A: Foundation Types

#### C1 — Crate safety and `EventIndex`

- **Builds:** Create/update `Cargo.toml`, `src/lib.rs`, and `src/time.rs`. Add only
  `serde` with `derive` and `serde_json`; enable `serde_json/raw_value` through the test
  dependency only; set shipped release/bench profiles to `panic = "abort"`; put
  `#![forbid(unsafe_code)]` at crate root; implement exact `EventIndex` API. `lib.rs`
  declares modules but defers final re-exports to C77.
- **Discharges:** `NO-UNSAFE`, `TRUST-ABORT` build-profile check, EventIndex API,
  start-turn portion of `RUN-INDEX`.
- **Proves:** `event_index_representation::serializes_as_transparent_u64` citing the
  EventIndex API; `event_index_representation::as_u64_returns_inner_ordinal` citing the
  EventIndex API.
- **Rust notes:** A one-field tuple struct with derived `Serialize` is a serde newtype and
  emits the inner number, not an object. Keep its field private.
- **Size:** 65-90 lines.

#### C2 — `Timestamp` construction and checked arithmetic

- **Builds:** Grow `src/time.rs` with the exact `Timestamp` API. This is the planned
  extension of C1; do not alter `EventIndex`.
- **Discharges:** Timestamp API, checked arithmetic portions of A6 and `ENV-TIME`.
- **Proves:** `timestamp_representation::serializes_as_transparent_u64` citing the
  Timestamp API; `timestamp_arithmetic::duration_over_u64_nanos_returns_none` citing A6;
  `timestamp_arithmetic::sum_overflow_returns_none` citing A6;
  `timestamp_arithmetic::equal_timestamp_is_valid` citing `ENV-TIME`.
- **Rust notes:** `Duration::as_nanos` returns `u128`; first use `u64::try_from`, then
  `checked_add`. Never truncate.
- **Size:** 80-110 lines.

#### C3 — Environment contract values and trait

- **Builds:** Create `src/environment.rs` with exact `Environment`, `ShutdownReport`,
  and `Quiescence` API blocks. No latch implementation yet.
- **Discharges:** Environment API and the type-enforced portions of the Commitment points
  table and `ENV-SERIAL`.
- **Proves:** `environment_lifecycle_consumption::shutdown_consumes_environment` citing
  `ENV-SERIAL` (a compile-positive mock that can call shutdown once);
  `shutdown_report_contents::retains_quiescence_and_pending_error` citing the
  ShutdownReport API.
- **Rust notes:** `shutdown(self)` moves the implementor. The trait does not need to be
  object-safe because Engine is generic over `E`.
- **Size:** 75-105 lines.

#### C4 — Fixed-capacity allocation

- **Builds:** Create `src/bounded_buffer.rs` with crate-private `BoundedBuffer<T>` that
  reserves its full logical capacity through `try_reserve_exact` before use. Add length,
  capacity, slice, and bound-query methods only.
- **Discharges:** reusable-storage portions of A6, `BOUND-LOOPS`, `JRN-ENCODE`, and the
  Engine construction table.
- **Proves:** `bounded_buffer_construction::reserves_complete_logical_capacity` citing A6;
  `bounded_buffer_construction::capacity_overflow_returns_try_reserve_error` citing the
  Engine construction table; `bounded_buffer_storage::construction_does_not_insert_values`
  citing A6.
- **Rust notes:** Logical capacity is separate from `Vec::capacity`; success proves at
  least the requested storage exists, but all later writes enforce the logical value.
- **Size:** 90-120 lines.

#### C5 — Fixed-capacity values, clear, and drain

- **Builds:** Grow `src/bounded_buffer.rs` with non-growing push, clear, and by-value drain
  operations. This is an explicit extension of C4 and becomes the final Command-buffer
  storage API.
- **Discharges:** storage mechanics for `APP-EMIT`, `APP-OVERFLOW`, `BOUND-LOOPS`.
- **Proves:** `bounded_buffer_capacity::accepts_exact_capacity_without_growth` citing A6;
  `bounded_buffer_capacity::rejects_next_value_without_growth` citing A6;
  `bounded_buffer_reuse::clear_and_drain_retain_allocation` citing `APP-OVERFLOW`.
- **Rust notes:** Use `Vec::drain(..)` or `mem::take` only if it retains the original
  allocation; test capacity before and after. Never use `push` without a preceding bound
  check.
- **Size:** 85-115 lines.

#### C6 — Bounded byte writer

- **Builds:** Grow `src/bounded_buffer.rs` with `std::io::Write` for
  `BoundedBuffer<u8>` and a sticky `bound_hit` query/reset. This is the final planned
  production touch to the buffer.
- **Discharges:** bounded encode-region mechanism of `JRN-ENCODE`, `BOUND-LOOPS`.
- **Proves:** `bounded_writer_progress::short_write_stops_at_remaining_capacity` citing
  `JRN-ENCODE`; `bounded_writer_progress::full_buffer_returns_zero_and_marks_bound`
  citing `JRN-ENCODE`; `bounded_writer_reuse::clear_resets_bound_marker` citing
  `JRN-ENCODE`.
- **Rust notes:** `Write::write` may accept a prefix. Returning `Ok(0)` at the logical
  bound lets `serde_json` stop; the sticky marker distinguishes that expected bound from
  other serializer errors.
- **Size:** 80-110 lines.

#### C7 — Application API and Context observations

- **Builds:** Create `src/application.rs` with exact `Application`, `Outcome`, and
  `Context` types, private Context construction, clear overflow marker storage, and final
  `index`, `logical_time`, and `remaining` behavior (including zero when marked).
  Context borrows the reusable buffer from C5.
- **Discharges:** Application API, `APP-CONTEXT`, `APP-FUTURE`.
- **Proves:** `context_turn_observation::reports_supplied_index_and_time` citing
  `APP-CONTEXT`; `context_capacity_observation::reports_exact_unused_slots` citing
  `APP-CONTEXT`; `context_capability_surface::handler_receives_only_context` citing
  `APP-CONTEXT` (compile-positive signature test).
- **Rust notes:** `Context<'a, C>` is a temporary mutable capability. Keep its constructor
  and buffer access crate-private; only observation and `emit` become public.
- **Size:** 105-135 lines.

#### C8 — Context emission, overflow, and turn reset

- **Builds:** Grow `src/application.rs` with final `emit`, sticky overflow transitions,
  and the crate-private fresh-turn reset path. No later chunk changes Context semantics;
  C42-C44 only add cross-file State tests.
- **Discharges:** `APP-EMIT`, `APP-OVERFLOW`; unit-test portion of `VERIFY-CONTEXT`.
- **Proves:** `context_command_order::emissions_append_in_call_order_through_exact_capacity`
  citing `VERIFY-CONTEXT` and `APP-EMIT`;
  `context_overflow_handling::first_over_capacity_emit_stores_nothing_and_sets_marker`
  citing `VERIFY-CONTEXT` and `APP-OVERFLOW`;
  `context_overflow_handling::emissions_after_overflow_store_nothing` citing
  `VERIFY-CONTEXT` and `APP-OVERFLOW`;
  `context_handler_reset::fresh_handler_starts_empty_with_clear_marker` citing
  `VERIFY-CONTEXT` and `APP-OVERFLOW`.
- **Rust notes:** `emit` always takes ownership of `C`; dropping rejected Commands is what
  makes it infallible. Ensure the first overflow does not evict an existing Command.
- **Size:** 100-135 lines.

### Phase B: Journal

#### C9 — Journal construction and errors

- **Builds:** Create `src/journal.rs` with the exact structs/enums and `Journal::new`,
  `is_poisoned`; compute `max_record_bytes.checked_add(1)` and reserve the whole region.
- **Discharges:** Journal API construction, `BOUND-NONZERO`, construction portion of
  `JRN-ENCODE`.
- **Proves:** `journal_construction::checked_region_overflow_returns_max_bytes_too_large`
  citing `JRN-ENCODE`; `journal_construction::reservation_failure_is_preserved` citing
  `JRN-ENCODE`; `journal_construction::fresh_journal_is_not_poisoned` citing `JRN-POISON`.
- **Rust notes:** Preserve `TryReserveError` by value. The maximum-capacity test should
  trigger capacity overflow, not attempt a real giant allocation.
- **Size:** 105-135 lines.

#### C10 — Raw bounded encoding helper

- **Builds:** Grow `src/journal.rs` with a private final `encode_raw` helper: clear the
  reusable buffer, call `serde_json::to_writer`, map `bound_hit` to `BoundExceeded`, and
  map every other serde failure to `Encode`. Do not define `commit` until C13.
- **Discharges:** encode-before-sink and encode-error portions of `JRN-ENCODE`.
- **Proves:** `journal_encoding::oversized_value_returns_bound_exceeded_without_sink_calls`
  citing `JRN-ENCODE`; `journal_encoding::serializer_error_returns_encode_without_sink_calls`
  citing `JRN-ENCODE`; `journal_encoding::encode_failures_do_not_poison` citing
  `JRN-ENCODE`.
- **Rust notes:** A counting writer fixture belongs in the outer unit-test module because
  later Journal groups share it. Encoding may fill `max + 1`; classification comes next.
- **Size:** 105-140 lines.

#### C11 — Complete-line encoding helper

- **Builds:** Grow `src/journal.rs` with a private final `encode_line` helper that calls
  C10, classifies start/end braces and literal newline, then appends exactly one newline.
  Use the P9 `RawValue` fixture. `commit` remains absent.
- **Discharges:** `JRN-FORMAT`, remaining classification rules of `JRN-ENCODE`; Journal
  portion of `VERIFY-JOURNAL`.
- **Proves:** `journal_object_validation::scalar_returns_not_an_object_without_sink_calls`
  citing `JRN-ENCODE`; `journal_object_validation::interior_newline_returns_not_an_object_without_sink_calls`
  citing `VERIFY-JOURNAL` and `JRN-ENCODE`;
  `journal_object_validation::max_plus_one_non_object_precedes_bound_classification`
  citing `JRN-ENCODE`; `journal_newline_reservation::max_plus_one_object_returns_bound_exceeded`
  citing `JRN-ENCODE`; `journal_newline_reservation::newline_is_stored_beyond_payload_bound`
  citing `JRN-FORMAT`.
- **Rust notes:** Classification order is observable: non-object first, then newline room.
  Ordinary string newlines are escaped and are not this test case.
- **Size:** 115-145 lines.

#### C12 — Bounded sink write helper

- **Builds:** Grow `src/journal.rs` with a private final `write_line` helper containing the
  bounded write loop. Retry only a short successful write; classify `Err` including
  `Interrupted`, `Ok(0)`, and over-report; poison before returning the typed write error.
  Source-local tests call the helper; public `commit` remains absent until C13.
- **Discharges:** write side of `JRN-POISON`, `BOUND-LOOPS`, sink-call portion of
  `JRN-SINK`.
- **Proves:** `journal_sink_writes::short_successful_writes_complete_one_line` citing
  `JRN-POISON`; `journal_sink_writes::interrupted_is_not_retried` citing `JRN-POISON`;
  `journal_sink_writes::zero_progress_maps_to_write_zero` citing `JRN-POISON`;
  `journal_sink_writes::overreported_count_maps_to_invalid_data` citing `JRN-POISON`.
- **Rust notes:** Increment offset only after validating `count <= remaining`. The loop's
  progress measure is bytes remaining, which proves the active-loop bound.
- **Size:** 110-145 lines.

#### C13 — Flush commitment and poison precondition

- **Builds:** Add complete `Journal::commit` in `src/journal.rs` by composing C11 and C12:
  assert not poisoned, encode/classify the complete line, write it, then flush; poison on
  flush failure and commit only on success. This is the first implementation of `commit`
  and the final behavior touch to Journal.
- **Discharges:** `JRN-COMMIT`, remaining `JRN-POISON`, `JRN-SINK`, Journal assertion site.
- **Proves:** `journal_commitment::successful_flush_commits_exact_line` citing
  `JRN-COMMIT`; `journal_commitment::flush_error_reports_flush_and_poisons` citing
  `JRN-POISON`; `journal_poison_precondition::later_commit_panics_without_encode_or_sink_call`
  citing `JRN-POISON` and `ASSERT-INVARIANTS`.
- **Rust notes:** Use always-on `assert!`, not `debug_assert!`. Bytes written before a
  failed flush remain an uncertain suffix; tests inspect calls, not pretend rollback.
- **Size:** 95-130 lines.

#### C14 — Journal fault matrix

- **Builds:** Add only tests to `src/journal.rs`, using the existing shared unit-test sink,
  to cover sink call/result traces and committed boundaries comprehensively.
- **Discharges:** direct Journal coverage of `JRN-FORMAT`, `JRN-ENCODE`, `JRN-COMMIT`,
  `JRN-POISON`, `JRN-SINK`, `TRUST-SINK` memory-sink check.
- **Proves:** `journal_commit_boundaries::only_successful_flush_advances_committed_boundary`
  citing `JRN-COMMIT`; `journal_sink_failure_matrix::every_sink_failure_poisons_once`
  citing `JRN-POISON`; `journal_sink_bytes::sink_receives_exact_bytes_in_call_order`
  citing `JRN-SINK` and `TRUST-SINK`.
- **Rust notes:** This is permanent unit coverage, not duplicate Engine fault coverage;
  Engine later verifies classification into `JournalFatal`.
- **Size:** 80-120 test lines.

### Phase C: Port and `ports!`

#### C15 — Port contract and `Never`

- **Builds:** Create `src/port.rs` with exact `PortContract` and `Never`; hand-write
  `Serialize` as an exhaustive uninhabited match.
- **Discharges:** Port API, absent-direction definition, type-level portion of
  `PORT-STATE`.
- **Proves:** `never_serialization::never_satisfies_serialize_bound` citing the Never API;
  `never_exhaustiveness::absent_direction_arm_is_dischargeable` citing the Never API.
- **Rust notes:** There is no runtime `Never` value to serialize. Tests prove trait
  satisfaction and exhaustive matching, not an impossible call.
- **Size:** 45-70 lines.

#### C16 — Exact `ports!` expansion

- **Builds:** Grow `src/port.rs` with exported `macro_rules! ports` whose expansion is
  exactly the two derived enums. Grow `src/lib.rs` only as required for crate-root macro
  export; final item re-exports remain C76.
- **Discharges:** generated-sum portion of `PORT-SUMS`; exact `ports!` API block.
- **Proves:** `ports_macro_payloads::generated_variants_use_contract_associated_types`
  citing `PORT-SUMS`; `ports_macro_slots::repeated_contract_slots_remain_distinct_variants`
  citing `PORT-SUMS`.
- **Rust notes:** Capture generated names as `ident`; capture Contracts as `ty`; use
  `::serde::Serialize` and `kavod::PortContract` paths exactly. Do not synthesize names.
- **Size:** 75-105 lines.

#### C17 — Generated bytes and hand-written equivalence

- **Builds:** Add tests only to `src/port.rs`; no production change.
- **Discharges:** serialized-form and hand-written-equivalence portions of `PORT-SUMS`.
- **Proves:** `ports_macro_serialization::event_uses_external_slot_tag` citing
  `PORT-SUMS`; `ports_macro_serialization::command_uses_external_slot_tag` citing
  `PORT-SUMS`; `ports_macro_equivalence::generated_and_handwritten_sums_emit_equal_bytes`
  citing `PORT-SUMS`.
- **Rust notes:** Compare exact bytes, not parsed `Value`, because enum representation is
  part of the supported equivalence.
- **Size:** 70-100 test lines.

#### C18 — Downstream macro fixture

- **Builds:** Add cross-file fixture `tests/ports_macro.rs` that invokes `kavod::ports!`
  as a consumer with direct `serde` dependency and exhaustively matches both sums.
- **Discharges:** macro path/hygiene portion of `PORT-SUMS`; validates the documented
  consumer dependency requirement.
- **Proves:** `ports_macro_downstream::consumer_invocation_compiles_and_serializes`
  citing `PORT-SUMS`; `ports_macro_downstream::fanout_match_is_exhaustive` citing
  `PORT-ROUTING`.
- **Rust notes:** This is cross-file because it verifies the exported macro boundary. An
  exhaustive match is compile evidence; semantic destination correctness waits for C73-C75.
- **Size:** 55-85 lines.

### Phase D: Certificate Grammar and Engine

#### C19 — Public run outcome taxonomy

- **Builds:** Create wiring-only `src/engine/mod.rs`, plus `src/engine/engine.rs` and
  `src/engine/record.rs`. Implement exact `EngineConfig`, `BuildError`, `EngineExit`,
  `FatalCause`, `EnvironmentFatal`, `EnvironmentOperation`, and `CoreError` in
  `engine.rs`; implement exact `RecordKind`, `TurnOutcome`, and `JournalFatal` in
  `record.rs`; re-export public items from `mod.rs`. Do not add Engine behavior yet.
- **Discharges:** exact Run value APIs, `CRATE-EXPORTS` within the engine module.
- **Proves:** `environment_operation_metadata::dispatch_retains_observation_position`
  citing the EnvironmentOperation API; `journal_fatal_metadata::outcome_is_present_only_for_turn_completed`
  citing the JournalFatal API; `core_error_payloads::time_regression_retains_both_timestamps`
  citing the CoreError API.
- **Rust notes:** Additional derives are permitted, but add only those tests need. Avoid
  requiring user Error types to implement formatting or equality.
- **Size:** 115-145 lines.

#### C20 — Shared record marker and `RunStarted`

- **Builds:** Grow `src/engine/record.rs` with private `RecordPayload`, `Kind<P>`, and the
  `RunStarted` payload. Its fields appear in exact wire order. Add one private generic
  commit helper that maps failure from `P::KIND` and `P::outcome()`.
- **Discharges:** `RunStarted` Records-table row; marker portion of `RUN-GRAMMAR`,
  `RUN-RECORDS`, `RUN-ENFORCEMENT`.
- **Proves:** `run_started_record_bytes::fields_and_order_are_exact` citing
  `RUN-RECORDS`; `record_kind_source::fatal_kind_matches_payload_kind` citing
  `RUN-GRAMMAR`.
- **Rust notes:** Struct field declaration order controls serde map order. `Kind<P>` is
  zero-sized but serializes as the bare tag string.
- **Size:** 105-140 lines.

#### C21 — `EventAccepted` payload

- **Builds:** Grow `src/engine/record.rs` with a borrowed/generic `EventAccepted` payload;
  use the existing marker and commit helper unchanged.
- **Discharges:** `EventAccepted` Records-table row, corresponding part of `RUN-RECORDS`.
- **Proves:** `event_accepted_record_bytes::new_index_time_and_event_are_byte_exact`
  citing `RUN-RECORDS`; `event_accepted_record_shape::top_level_has_only_required_fields`
  citing `RUN-RECORDS`.
- **Rust notes:** Borrow the Event during serialization so successful commit can return
  the owned Event to the handler without requiring `Clone`.
- **Size:** 65-95 lines.

#### C22 — Command payloads

- **Builds:** Grow `src/engine/record.rs` with `CommandsPrepared` borrowing the ordered
  batch and `CommandsDispatched`.
- **Discharges:** both Command Records-table rows; corresponding `RUN-RECORDS` wire shapes.
- **Proves:** `commands_prepared_record_bytes::commands_remain_in_batch_order` citing
  `RUN-RECORDS`; `commands_dispatched_record_bytes::contains_no_extra_fields` citing
  `RUN-RECORDS`.
- **Rust notes:** Serialize a slice/shared view before moving Commands. Do not clone the
  batch or payloads.
- **Size:** 70-100 lines.

#### C23 — Stop and completion payloads

- **Builds:** Grow `src/engine/record.rs` with `StopRequested` and `TurnCompleted`; bind
  outcome to a private marker implementation that also supplies `JournalFatal.outcome`.
- **Discharges:** final two Records-table rows; payload portion of `RUN-RECORDS` and
  outcome portion of `RUN-GRAMMAR`.
- **Proves:** `stop_requested_record_bytes::contains_only_kind_and_index` citing
  `RUN-RECORDS`; `turn_completed_record_bytes::continue_is_a_bare_tag` citing
  `RUN-RECORDS`; `turn_completed_record_bytes::stop_is_a_bare_tag` citing
  `RUN-RECORDS`; `turn_completed_kind_source::fatal_metadata_uses_same_outcome_value`
  citing `RUN-ENFORCEMENT`.
- **Rust notes:** A private trait implemented by Continue/Stop marker types avoids a
  runtime outcome parameter on completion transitions.
- **Size:** 95-130 lines.

#### C24 — Initial certificate and `run_started`

- **Builds:** Grow `src/engine/record.rs` with private phase markers, affine
  `Certificate<W, P>`, its sole minting function, and consuming `run_started`. Add no
  accepted-phase getters to `Initial`; assert prospective index 0.
- **Discharges:** startup rows 3-4, Initial phase and edge, induction base of
  `RUN-GRAMMAR`, `RUN-ENFORCEMENT`, `RUN-INDEX`.
- **Proves:** `initial_transition::successful_commit_yields_turn_open_at_zero` citing
  `RUN-GRAMMAR` and `RUN-INDEX`; `initial_transition::record_uses_certificate_time`
  citing the startup table; `initial_invariant::nonzero_prospective_index_panics`
  citing `RUN-ENFORCEMENT`.
- **Rust notes:** `PhantomData<fn() -> P>` carries typestate without inheriting phase
  `Send`/`Sync`. The test module can construct an invalid certificate because it shares
  module privacy; production callers cannot.
- **Size:** 110-145 lines.

#### C25 — Answer classification

- **Builds:** Grow `src/engine/record.rs` with private non-cloneable `Continue` and `Stop`
  markers, `TurnOpen<A>`, and `ClassifiedTurn`. `classify` consumes the unclassified
  phase and the non-Fatal answer once.
- **Discharges:** classification portion of `RUN-GRAMMAR`, runtime answer point of
  `RUN-ENFORCEMENT`.
- **Proves:** `turn_classification::continue_returns_continue_refinement` citing
  `RUN-ENFORCEMENT`; `turn_classification::stop_returns_stop_refinement` citing
  `RUN-ENFORCEMENT`.
- **Rust notes:** Match once and move the certificate into one enum variant. No later
  transition accepts `Outcome`, preventing disagreement by API shape.
- **Size:** 80-110 lines.

#### C26 — Empty-batch edge

- **Builds:** Grow `src/engine/record.rs` with `TurnOpen<A>::no_commands`, accepted-phase
  index/time getters, and `EffectsComplete<A>`. Assert the actual reusable buffer empty;
  commit nothing.
- **Discharges:** recordless TurnOpen-to-EffectsComplete edge, empty-batch assertion site,
  applicable `RUN-GRAMMAR` and `RUN-ENFORCEMENT`.
- **Proves:** `empty_batch_edge::empty_buffer_yields_effects_complete_without_sink_call`
  citing the Edges table; `empty_batch_invariant::nonempty_buffer_panics` citing
  `RUN-ENFORCEMENT` and `ASSERT-INVARIANTS`.
- **Rust notes:** Pass `&CommandBuffer<C>` only as the runtime assertion witness; it does
  not become part of the successor.
- **Size:** 75-105 lines.

#### C27 — Atomic prepared/dispatch transition

- **Builds:** Grow `src/engine/record.rs` with the final `dispatch_batch`: assert nonempty,
  commit `CommandsPrepared` from a shared view, drain each Command in order through
  `Environment::dispatch`, and commit `CommandsDispatched` only after all handoffs.
- **Discharges:** Prepared phase, both command edges, `RUN-GRAMMAR`, `BOUND-LOOPS`,
  dispatch Commitment-points row.
- **Proves:** `dispatch_batch_invariant::empty_buffer_panics_before_recording` citing
  `RUN-ENFORCEMENT`; `dispatch_batch_order::prepared_commits_before_first_handoff`
  citing the Records table; `dispatch_batch_order::dispatched_commits_after_last_handoff`
  citing `RUN-GRAMMAR`; `dispatch_batch_failure::error_reports_position_and_keeps_prefix`
  citing the Prepared phase.
- **Rust notes:** The shared borrow for serialization must end before `drain`. `enumerate`
  supplies the exact zero-based failure position; dropping the drain discards the suffix.
- **Size:** 120-150 lines; split the failure test into C42 if formatting exceeds 150, but
  do not split the production transition.

#### C28 — Checkpoint transition

- **Builds:** Grow `src/engine/record.rs` with `EffectsComplete<A>::checkpoint`,
  `Checkpointed<A>`, and internal Error mapping. It calls `take_error` exactly once and
  commits nothing.
- **Discharges:** EffectsComplete phase, checkpoint edge, `RUN-CHECKPOINT`, checkpoint
  portion of `RUN-GRAMMAR`.
- **Proves:** `checkpoint_transition::none_yields_checkpointed_without_record` citing
  `RUN-CHECKPOINT`; `checkpoint_transition::pending_error_reports_checkpoint_and_consumes_certificate`
  citing `RUN-CHECKPOINT`.
- **Rust notes:** The Error branch must not return a certificate. This is how ownership
  prevents any completion record after a pending Error.
- **Size:** 75-105 lines.

#### C29 — Continue completion

- **Builds:** Grow `src/engine/record.rs` with the sole method on
  `Checkpointed<Continue>`, committing `TurnCompleted(Continue)` and returning
  `BetweenTurns`.
- **Discharges:** Continue completion edge, corresponding Records row, marker-enforced
  part of `RUN-GRAMMAR`.
- **Proves:** `continue_completion::commits_continue_and_yields_between_turns` citing
  `RUN-GRAMMAR`; `continue_completion::journal_failure_reports_continue_outcome` citing
  `RUN-ENFORCEMENT`.
- **Rust notes:** The method takes no outcome argument. The phase marker chooses both bytes
  and fatal metadata.
- **Size:** 65-90 lines.

#### C30 — Stop request

- **Builds:** Grow `src/engine/record.rs` with the sole method on `Checkpointed<Stop>`,
  committing `StopRequested` and returning `StopPending`.
- **Discharges:** StopRequested edge and Records-table position, marker-enforced
  `RUN-GRAMMAR`.
- **Proves:** `stop_request::commits_before_shutdown_and_yields_stop_pending` citing the
  Records table; `stop_request::journal_failure_prevents_shutdown_phase` citing
  `RUN-GRAMMAR`.
- **Rust notes:** This transition does not borrow the Environment; that makes intent
  commit structurally precede shutdown.
- **Size:** 60-85 lines.

#### C31 — Stop close and clean-report witness

- **Builds:** Grow `src/engine/record.rs` with `StopPending::close`, consuming `E`, storing
  report quiescence before inspecting error, enforcing Error-over-Incomplete precedence,
  and committing `TurnCompleted(Stop)` only after `{ Quiesced, None }`.
- **Discharges:** StopPending and Closed phases, Stop completion edge, shutdown
  Commitment-points row, Stop-path parts of `RUN-GRAMMAR` and `RUN-FINALIZE`.
- **Proves:** `stop_close::error_report_is_environment_shutdown_fatal_even_when_incomplete`
  citing `RUN-FINALIZE`; `stop_close::incomplete_without_error_is_core_fatal` citing the
  StopPending phase; `stop_close::clean_report_commits_stop_completion` citing
  `RUN-GRAMMAR`; `stop_close::commit_failure_retains_quiesced` citing `RUN-FINALIZE`.
- **Rust notes:** Return a private close result enum carrying either `Closed` or the
  already-retained quiescence/fatal data. The Environment no longer exists on any branch.
- **Size:** 120-150 lines.

#### C32 — Event index/time validation helpers

- **Builds:** Grow `src/engine/record.rs` with private final helpers used by acceptance:
  reject `u64::MAX` before interaction, checked-increment with invariant panic after the
  guard, and reject decreasing timestamps while retaining previous/offered values.
- **Discharges:** runtime guard portions of `RUN-INDEX`, `ENV-TIME`, `RUN-ENFORCEMENT`.
- **Proves:** `event_index_guard::maximum_index_returns_index_exhausted` citing
  `RUN-INDEX`; `event_index_invariant::overflow_after_guard_panics` citing `RUN-INDEX`;
  `event_time_validation::decreasing_stamp_returns_both_values` citing `ENV-TIME`.
- **Rust notes:** Helpers return typed Core errors, except the post-guard impossible
  overflow, which uses always-on `expect` as the named invariant assertion.
- **Size:** 75-105 lines.

#### C33 — Event acceptance transition

- **Builds:** Grow `src/engine/record.rs` with final `BetweenTurns::accept_event`. Check
  domain, call `next_event`, validate time, commit using the same Event/index/time locals,
  then return `TurnOpen` and owned Event.
- **Discharges:** BetweenTurns phase, EventAccepted edge, `RUN-INDEX`, acceptance part of
  `RUN-GRAMMAR`, `ENV-TIME`, next-event Commitment-points row.
- **Proves:** `event_acceptance::maximum_index_skips_next_event` citing `RUN-INDEX`;
  `event_acceptance::decreasing_time_consumes_candidate_without_commit` citing the
  EventAccepted edge; `event_acceptance::successful_record_and_successor_share_values`
  citing `RUN-GRAMMAR`; `event_acceptance::next_event_error_reports_observation_operation`
  citing the BetweenTurns phase.
- **Rust notes:** Serialize `&event`; move `event` only after commit succeeds. No caller
  supplies any acceptance fact.
- **Size:** 110-145 lines.

#### C34 — Engine construction

- **Builds:** Grow `src/engine/engine.rs` with private `Engine` fields and exact
  `Engine::new`. Reserve Command storage first, then construct Journal; invoke no
  Application or Environment method.
- **Discharges:** Engine construction table, exact `Engine::new` API, `BOUND-NONZERO`.
- **Proves:** `engine_construction::reserves_command_buffer_before_journal` citing the
  Engine construction table; `engine_construction::failure_invokes_no_application_or_environment_method`
  citing the Engine construction table; `engine_construction::allocation_failure_maps_to_command_buffer`
  citing the BuildError API.
- **Rust notes:** Test call absence with counters in inert values. A private constructor
  seam may inject a failing Journal build only in this source-file test module.
- **Size:** 100-135 lines.

#### C35 — One-turn handler execution

- **Builds:** Grow `src/engine/engine.rs` with a private final helper that clears the
  buffer, builds Context, invokes exactly one appropriate handler, ends the Context
  borrow, checks overflow before Outcome, discards Fatal batches, and calls `classify` for
  Continue/Stop.
- **Discharges:** TurnOpen phase, A2, A4, `APP-STATE`, Core-over-Application precedence.
- **Proves:** `turn_handler_selection::index_zero_calls_on_start_once` citing the Phases
  table; `turn_handler_selection::external_index_calls_on_event_once` citing the Phases
  table; `turn_overflow_precedence::overflow_outranks_application_fatal` citing
  `APP-OVERFLOW` and A4; `turn_application_fatal::state_mutation_and_fatal_payload_stand`
  citing `APP-STATE`.
- **Rust notes:** Put Context in its own lexical block so its mutable borrow ends before
  inspecting/draining the buffer. State remains outside all Result branches.
- **Size:** 115-150 lines.

#### C36 — Fatal finalization helper

- **Builds:** Grow `src/engine/engine.rs` with one private finalization helper for the
  three Environment states: unconsumed after successful start, consumed by Stop, or
  start-Err. It never changes the fixed cause and calls shutdown at most once.
- **Discharges:** `RUN-FINALIZE`, A4, finalization portions of `ENV-SERIAL`.
- **Proves:** `fatal_finalization::started_environment_is_shutdown_once` citing
  `RUN-FINALIZE`; `fatal_finalization::shutdown_error_never_replaces_fixed_cause` citing
  A4 and `RUN-FINALIZE`; `fatal_finalization::start_error_skips_shutdown_and_is_quiesced`
  citing `ENV-START` and `RUN-FINALIZE`; `fatal_finalization::consumed_environment_uses_retained_quiescence`
  citing `RUN-FINALIZE`.
- **Rust notes:** An `Option<E>` at this orchestration boundary makes availability
  explicit: `take()` before consuming shutdown; `None` proves Stop already consumed it.
- **Size:** 105-140 lines.

#### C37 — Complete serial `Engine::run`

- **Builds:** Finish exact `Engine::run(self)` in `src/engine/engine.rs` by composing C24-
  C36 in one nonrecursive loop. Create State exactly once first, start Environment, mint
  certificate, run each turn, request the next Event only from `BetweenTurns`, and route
  every error to C36. No earlier helper is replaced.
- **Discharges:** startup table, all Phases/Edges tables as orchestration, `RUN-SERIAL`,
  runtime operation sequence of `RUN-GRAMMAR`, `BOUND-LOOPS`, exact Engine API.
- **Proves:** `engine_startup_order::state_precedes_start_and_run_started_precedes_handler`
  citing the startup table; `engine_serial_loop::continue_completion_precedes_next_event`
  citing `RUN-SERIAL` and `RUN-GRAMMAR`; `engine_stop_exit::only_closed_returns_stopped`
  citing `RUN-GRAMMAR`.
- **Rust notes:** Prefer a private `TurnResult` enum over one giant nested match. The loop
  owns one `BetweenTurns` certificate at its back edge, which makes serial progression
  visible to the borrow checker.
- **Size:** 120-150 lines.

### Phase E: Permanent Engine Test Infrastructure and Suites

#### C38 — Scripted Environment harness

- **Builds:** Create wiring-only `tests/support/mod.rs` and
  `tests/support/scripted_environment.rs`. The permanent bespoke Environment scripts
  every operation result, records every call and handoff, and exposes shutdown count and
  contractual-effect counters. Add `tests/harness_contract.rs` as its first consumer.
- **Discharges:** infrastructure for `VERIFY-CONFORMANCE`, `VERIFY-FAULTS`,
  `VERIFY-CONTEXT`; observable subset of `TRUST-ENV` for this fixture.
- **Proves:** `scripted_environment_trace::records_each_operation_and_result_in_order`
  citing the Trace definition; `scripted_environment_effects::failed_dispatch_records_no_handoff`
  citing the dispatch Commitment-points row.
- **Rust notes:** Scripts own values and consume each step exactly once. Keep Error values
  typed in the fixture; add an erased projection only when comparing traces in C77.
- **Size:** 115-150 lines.

#### C39 — Scripted memory/fault sink harness

- **Builds:** Add `tests/support/scripted_sink.rs` and re-export it from the wiring-only
  support module. It scripts short writes, write Error, flush Error, `Ok(0)`, over-report,
  captures bytes, and records each sink result through a shared handle.
- **Discharges:** permanent infrastructure for `VERIFY-JOURNAL`, `VERIFY-FAULTS`,
  memory-sink portion of `TRUST-SINK`.
- **Proves:** `scripted_sink_trace::records_every_write_and_flush_result` citing the Trace
  definition; `scripted_sink_storage::stores_exactly_the_reported_prefix` citing
  `TRUST-SINK`.
- **Rust notes:** `Rc<RefCell<_>>` is acceptable because Engine tests are serial and the
  sink remains exclusively owned by Journal; Live tests use separate thread-safe probes.
- **Size:** 100-135 lines.

#### C40 — Acceptance and outcome golden sequences

- **Builds:** Create `tests/golden_journal.rs` using C38-C39. Cover start/Stop and one
  Continue/Event/Stop run with exact byte literals and call traces.
- **Discharges:** acceptance/outcome portions of `RUN-RECORDS`, `RUN-GRAMMAR`,
  `VERIFY-JOURNAL`, `TRUST-SERIALIZE` fixture check.
- **Proves:** `run_records_wire_format::run_started_bytes_are_exact` citing
  `VERIFY-JOURNAL` and `RUN-RECORDS`; `run_records_wire_format::event_accepted_bytes_are_exact`
  citing `VERIFY-JOURNAL` and `RUN-RECORDS`; `run_answer_classification::continue_answer_commits_continue_outcome`
  citing `RUN-ENFORCEMENT`; `run_answer_classification::stop_answer_commits_stop_request_then_stop_outcome`
  citing `RUN-ENFORCEMENT`.
- **Rust notes:** Assert raw bytes including every comma, field order, bare tags, schema
  version, and trailing newline. Also assert handler calls occur only after acceptance.
- **Size:** 110-145 lines.

#### C41 — Empty and command-batch golden sequences

- **Builds:** Grow `tests/golden_journal.rs` with table-driven empty/nonempty ×
  Continue/Stop paths, exact batch bytes, and handoff observations. This is an explicit
  test-only extension of C40.
- **Discharges:** all normal graph record sequences, Records table, normal-path portion of
  `VERIFY-JOURNAL`, `TRUST-SERIALIZE` for fixture payloads.
- **Proves:** `run_graph_record_sequence::empty_continue_turn_has_required_sequence`
  citing `VERIFY-JOURNAL`; `run_graph_record_sequence::command_continue_turn_has_required_sequence`
  citing `VERIFY-JOURNAL`; `run_graph_record_sequence::empty_stop_turn_has_required_sequence`
  citing `VERIFY-JOURNAL`; `run_graph_record_sequence::command_stop_turn_has_required_sequence`
  citing `VERIFY-JOURNAL`; `command_effect_order::prepared_precedes_first_handoff_and_dispatched_follows_last`
  citing the Records table.
- **Rust notes:** One table of expected complete byte strings is easier to audit than
  incrementally parsing records. Record Environment call positions alongside sink calls.
- **Size:** 110-145 lines.

#### C42 — Context State and Core fault paths

- **Builds:** Create `tests/fault_injection.rs`. Add table-driven Applications for Fatal,
  over-emission, and mutation; scripts for decreasing time and index helper exposure
  through source-local tests only.
- **Discharges:** Core-condition portion of `VERIFY-FAULTS`; `APP-STATE`; State portion of
  `VERIFY-CONTEXT`; TimeRegression and CommandBoundExceeded Phase rows.
- **Proves:** `application_state_preservation::fatal_answer_preserves_handler_state_mutations`
  citing `VERIFY-CONTEXT` and `APP-STATE`;
  `application_state_preservation::overflow_preserves_state_and_outranks_fatal_answer`
  citing `VERIFY-CONTEXT`, `APP-OVERFLOW`, and A4;
  `run_core_failures::decreasing_timestamp_consumes_candidate_without_accepting_it`
  citing `VERIFY-FAULTS`; `run_core_failures::over_emission_dispatches_nothing` citing
  `VERIFY-FAULTS`.
- **Rust notes:** Compare both exit and full call trace. The candidate remains present only
  in the scripted trace; no acceptance record or handler call may appear.
- **Size:** 115-150 lines.

#### C43 — Environment operation and shutdown-report faults

- **Builds:** Grow `tests/fault_injection.rs` with start, next-event, every dispatch
  position, checkpoint, shutdown Error, and Incomplete scripts. Include every
  post-`start` operation Error crossed with shutdown `{ Quiesced, Some }` and
  `{ Incomplete, Some }`.
- **Discharges:** Environment-operation and report portions of `VERIFY-FAULTS`, A4,
  `RUN-CHECKPOINT`, `RUN-FINALIZE`.
- **Proves:** `environment_operation_failures::start_error_is_quiesced_and_never_calls_shutdown`
  citing `VERIFY-FAULTS` and `RUN-FINALIZE`;
  `environment_operation_failures::dispatch_error_reports_position_and_preserves_prefix`
  citing `VERIFY-FAULTS`; `environment_operation_failures::checkpoint_error_is_reported_at_checkpoint`
  citing `RUN-CHECKPOINT`; `environment_operation_failures::shutdown_error_outranks_incomplete`
  citing A4 and `RUN-FINALIZE`; `fatal_finalization::operation_error_remains_cause_when_shutdown_reports_error`
  citing `VERIFY-FAULTS` and `RUN-FINALIZE`.
- **Rust notes:** Generate the cross-product from data but keep each assertion explicit:
  original cause, shutdown count one, returned quiescence, and discarded report Error.
- **Size:** 120-150 lines.

#### C44 — Record-commit faults and final Context coverage

- **Builds:** Finish `tests/fault_injection.rs` with write/flush failure at every recorded
  edge and State mutation checks for every post-handler Fatal point. Check retained
  Quiesced after failed `TurnCompleted(Stop)`.
- **Discharges:** remaining `VERIFY-FAULTS`; remaining `VERIFY-CONTEXT`; fatal-ending
  sequences needed by `VERIFY-JOURNAL`; `JRN-COMMIT`, `RUN-FINALIZE`.
- **Proves:** `journal_record_failures::each_edge_reports_matching_record_kind_and_outcome`
  citing `VERIFY-FAULTS`; `journal_record_failures::commands_prepared_failure_hands_off_nothing`
  citing A5; `journal_record_failures::commands_dispatched_failure_follows_all_handoffs`
  citing A5; `journal_record_failures::event_accepted_failure_leaves_candidate_unhandled`
  citing `RUN-GRAMMAR`; `fatal_finalization::turn_completed_stop_failure_retains_quiesced`
  citing `RUN-FINALIZE`; `application_state_preservation::mutations_survive_each_post_handler_fatal_exit`
  citing `VERIFY-CONTEXT` and `APP-STATE`.
- **Rust notes:** A table entry identifies the record kind, sink operation, expected last
  committed bytes, expected handoff count, and expected `JournalFatal.outcome`.
- **Size:** 120-150 lines.

#### C45 — Compile-fail fixture crate and control

- **Builds:** Create `tests/grammar_compile_fail.rs` and
  `tests/fixtures/grammar/{Cargo.toml,src/lib.rs,cases/control.rs}`. The fixture reconstructs
  the Engine module position, re-exports required crate items, and `include!`s production
  `engine.rs`/`record.rs` plus one selected case. Use a dedicated target directory.
- **Discharges:** permanent infrastructure mandated for `VERIFY-GRAMMAR`.
- **Proves:** `grammar_fixture_control::valid_transition_sequence_compiles` citing
  `VERIFY-GRAMMAR`; `grammar_fixture_visibility::control_reaches_engine_visibility_position`
  citing `RUN-ENFORCEMENT`.
- **Rust notes:** The runner invokes `cargo check` noninteractively. A successful control
  proves failures in later cases are grammar restrictions rather than broken fixture
  imports or privacy.
- **Size:** 110-145 lines.

#### C46 — Compile-fail transition attacks

- **Builds:** Add fixture cases and runner groups for illegal order, duplicate start,
  skipped checkpoint, premature Stop completion, and independent
  `CommandsDispatched`. No production files change.
- **Discharges:** transition/witness portion of `VERIFY-GRAMMAR`, `RUN-GRAMMAR`,
  `RUN-ENFORCEMENT`.
- **Proves:** `grammar_transition_order::run_started_cannot_be_committed_twice` citing
  `VERIFY-GRAMMAR`; `grammar_transition_order::event_cannot_be_accepted_before_continue_completion`
  citing `VERIFY-GRAMMAR`; `grammar_checkpoint_requirement::checkpoint_cannot_be_skipped`
  citing `VERIFY-GRAMMAR`; `grammar_stop_witness::stop_completion_requires_clean_shutdown`
  citing `VERIFY-GRAMMAR`; `grammar_dispatch_witness::commands_dispatched_cannot_be_committed_independently`
  citing `VERIFY-GRAMMAR`.
- **Rust notes:** Put a stable marker comment beside each attack expression; the runner
  checks failure and case identity, not rustc's unstable prose.
- **Size:** 100-140 lines.

#### C47 — Compile-fail outcome and affinity attacks

- **Builds:** Finish fixture cases for Continue-as-Stop, Stop-as-Continue, and attempts to
  use `Clone`, `Copy`, or `Default` on the certificate.
- **Discharges:** remaining `VERIFY-GRAMMAR`; compile-time scope of `RUN-GRAMMAR` and
  `RUN-ENFORCEMENT`.
- **Proves:** `grammar_answer_refinement::continue_cannot_commit_stop_outcome` citing
  `VERIFY-GRAMMAR`; `grammar_answer_refinement::stop_cannot_commit_continue_outcome`
  citing `VERIFY-GRAMMAR`; `certificate_trait_absence::certificate_cannot_be_cloned`
  citing `VERIFY-GRAMMAR`; `certificate_trait_absence::certificate_cannot_be_copied`
  citing `VERIFY-GRAMMAR`; `certificate_trait_absence::certificate_has_no_default`
  citing `VERIFY-GRAMMAR`.
- **Rust notes:** Copy is tested by attempting reuse after move rather than asking a
  reflection API. Keep each attack independent so one expected error cannot mask another.
- **Size:** 85-125 lines.

### Phase F: Shared Error Latch

#### C48 — First-error latch state machine

- **Builds:** Grow `src/environment.rs` with crate-private `ErrorLatch<E>` and explicit
  Empty/Pending/Reported/Closed behavior for publish, take, and close. It contains no
  synchronization; its owner supplies the critical section.
- **Discharges:** sequential state-machine portion of `ENV-LATCH`.
- **Proves:** `error_latch_first_publication::first_error_becomes_pending_and_later_errors_are_discarded`
  citing `ENV-LATCH`; `error_latch_reporting::take_reports_pending_once_and_reported_is_permanent`
  citing `ENV-LATCH`; `error_latch_close::pending_error_leaves_through_close` citing
  `ENV-LATCH`; `error_latch_close::post_close_publication_is_discarded` citing
  `ENV-LATCH`.
- **Rust notes:** Model states explicitly instead of two booleans. `Option::take` moves the
  typed Error out without cloning.
- **Size:** 100-135 lines.

#### C49 — Latch observation precedence helpers

- **Builds:** Grow `src/environment.rs` with small crate-private operations used inside an
  owner's observation critical section: prefer pending over a local pre-commit failure,
  mark returned pending as reported, and close into a report exactly once. This is the
  final shared latch touch; concurrency remains with Live.
- **Discharges:** local-failure precedence and final-observation portions of `ENV-LATCH`.
- **Proves:** `error_latch_failure_precedence::pending_error_wins_and_discards_local_error`
  citing `ENV-LATCH` and A4; `error_latch_failure_precedence::local_error_wins_when_latch_is_empty`
  citing `ENV-LATCH`; `error_latch_final_observation::close_returns_one_consistent_state`
  citing `ENV-LATCH`.
- **Rust notes:** The helper does not choose race order. Its caller's lock acquisition is
  the linearization decision, which is why the same state machine works in Sim and Live.
- **Size:** 75-105 lines.

### Phase G: Simulated Environment

All C50-C57 chunks are **§10-blocked**. They begin only after `W1`-`W7` are approved.
They use a permanent crate-private `from_parts` construction seam exercised by an
in-crate typed topology; C72-C73 expose that same seam to generated wiring without
replacing it.

#### C50 — Sim config, lifecycle, and `SimCtx`

- **Builds:** Create wiring-only `src/sim/mod.rs`; create `src/sim/config.rs`,
  `src/sim/context.rs`, and `src/sim/slot.rs`. Implement approved `SimConfig`, exact
  `SimCtx`/`SimCtxError` API, private lifecycle enum, and one fixed arm cell per Slot.
- **Discharges:** `BOUND-NONZERO`, storage portion of `BOUND-STATIC`, `SIM-WAKEUP`,
  `SIM-TIME` Context API.
- **Proves:** `sim_context_time::now_reports_environment_time` citing `SIM-TIME`;
  `sim_context_wakeup::before_now_is_rejected_without_change` citing `SIM-WAKEUP`;
  `sim_context_wakeup::later_set_replaces_existing_arm` citing `SIM-WAKEUP`;
  `sim_context_wakeup::clear_disarms_the_slot` citing `SIM-WAKEUP`.
- **Rust notes:** `SimCtx<'a, C>` borrows only that Slot's arm and shared current time for
  one callback. The Port cannot reach another Slot's arm.
- **Size:** 115-145 lines.

#### C51 — Fixed Sim storage and one-Slot startup helper

- **Builds:** Create `src/sim/environment.rs` with fixed entries, `now`, cursor, latch,
  and budget config; add permanent `from_parts`. Add a private final `start_slot` helper
  that moves one NotStarted lifecycle to Open before invoking its Port. Do not implement
  `Environment::start` until C52 can include complete failure cleanup.
- **Discharges:** `SIM-STATE`, local lifecycle portion of `SIM-START`, `SIM-LIFECYCLE`,
  storage portion of `SIM-TIME`, `BOUND-STATIC`.
- **Proves:** `sim_storage_bounds::one_lifecycle_and_arm_exist_per_frozen_slot` citing
  `ENV-BOUNDS` and `BOUND-STATIC`; `sim_startup_slot::lifecycle_is_open_before_port_code_runs`
  citing `SIM-LIFECYCLE`; `sim_startup_slot::successful_callback_leaves_slot_open`
  citing `SIM-LIFECYCLE`; `sim_storage_time::configured_origin_is_stored_unchanged`
  citing `SIM-TIME`.
- **Rust notes:** Build the complete fixed Vec before any Port method. Index-based scoped
  borrows keep `now`, arm, and one Port independently borrowable without unsafe code.
- **Size:** 120-150 lines.

#### C52 — Complete Sim startup and failure cleanup

- **Builds:** Add complete `Environment::start` in `src/sim/environment.rs`: set `now` to
  origin, call C51 in frozen order, commit startup after all Ok, or mark the failing
  lifecycle Ended, stop exactly the earlier Open prefix in order, discard cleanup Errors,
  and leave suffix NotStarted. This is the first implementation of Sim `start`.
- **Discharges:** all of `SIM-START`, startup `SIM-LIFECYCLE`, `SIM-TIME`, `ENV-START`,
  start Commitment-points row, A4.
- **Proves:** `sim_startup_success::all_ports_start_in_order_and_origin_is_returned`
  citing `SIM-START` and `SIM-TIME`; `sim_startup_failure::failure_at_each_slot_stops_only_open_prefix`
  citing `VERIFY-SIM` and `SIM-START`; `sim_startup_failure::prefix_is_stopped_once_in_frozen_order`
  citing `VERIFY-SIM`; `sim_startup_failure::failing_and_later_ports_receive_no_stop`
  citing `VERIFY-SIM`; `sim_startup_failure::cleanup_errors_do_not_replace_original`
  citing A4.
- **Rust notes:** Table-drive every Slot position. Transition each prefix entry to Ended
  before calling `stop`, so even a stop Error cannot permit a second lifecycle call.
- **Size:** 110-145 lines.

#### C53 — Sim dispatch and post-handoff Error

- **Builds:** Grow `src/sim/environment.rs` with final dispatch: observe pending latch
  first, use the frozen exhaustive router, set lifecycle Ended before reporting
  `on_command` Err, publish it, and return Ok because invocation committed handoff.
- **Discharges:** `SIM-DISPATCH`, `SIM-LIFECYCLE`, dispatch Commitment-points row,
  `ENV-ERRORS`, `ENV-LATCH`.
- **Proves:** `sim_dispatch_precedence::pending_error_prevents_port_invocation` citing
  `ENV-LATCH`; `sim_dispatch_handoff::matching_port_receives_command_once_without_time_advance`
  citing `SIM-DISPATCH`; `sim_dispatch_handoff::port_error_is_latched_after_ok_dispatch`
  citing `SIM-DISPATCH` and `ENV-ERRORS`; `sim_command_lifecycle::on_command_error_ends_port_before_later_work`
  citing `SIM-LIFECYCLE`.
- **Rust notes:** Invocation is the commitment point, not successful return from Port
  code. Map the typed Error before putting it in the common latch.
- **Size:** 105-140 lines.

#### C54 — Sim bounded selection scan

- **Builds:** Grow `src/sim/environment.rs` with one private final read-only scan that
  returns the lowest-time armed Slot, resolving equal times by scanning from a supplied
  cursor and wrapping in frozen order. It performs no time, arm, cursor, or Port mutation.
- **Discharges:** bounded scan and tie-order portions of `SIM-SELECT`, `BOUND-LOOPS`.
- **Proves:** `sim_selection_scan::lowest_time_arm_is_selected` citing `SIM-SELECT`;
  `sim_selection_scan::equal_times_choose_first_from_supplied_cursor` citing
  `SIM-SELECT`; `sim_selection_scan::scan_wraps_once_in_frozen_order` citing
  `SIM-SELECT` and `BOUND-LOOPS`.
- **Rust notes:** A read-only helper separates ordering from callback borrows. One scan is
  bounded by fixed Slot count and never recurses.
- **Size:** 115-145 lines.

#### C55 — Sim cursor and `step(None)` iteration

- **Builds:** Grow `src/sim/environment.rs` with a private final one-selection helper that
  calls C54, advances `now`, clears the arm, asserts lifecycle Open, invokes `step`, moves
  lifecycle to Ended on Err, advances the persistent cursor, and returns typed
  `Selected::{Event, Continue}` for Some/None. Do not create an unbounded loop.
- **Discharges:** one-step portions of `SIM-SELECT`, `SIM-TIME`, `SIM-LIFECYCLE`,
  next-event Commitment-points row, selected-Open assertion site.
- **Proves:** `sim_event_selection::cursor_advances_after_each_selected_step` citing
  `SIM-SELECT`; `sim_event_selection::now_advances_and_arm_clears_before_step` citing
  `SIM-SELECT` and `SIM-TIME`; `sim_event_selection::step_none_advances_cursor_and_returns_continue`
  citing `SIM-SELECT`; `sim_step_lifecycle::step_error_ends_port_after_subordinate_effects`
  citing `SIM-LIFECYCLE`; `sim_selection_invariant::selected_port_must_be_open` citing
  `ASSERT-INVARIANTS`.
- **Rust notes:** Cursor update occurs regardless of Some/None/Err after a selected call.
  Unit tests invoke the helper repeatedly; production looping arrives only with its bound.
- **Size:** 90-125 lines.

#### C56 — Step budget, completion, and operation Error ordering

- **Builds:** Grow `src/sim/environment.rs` with the final `next_event` loop and fresh
  per-call counter in this check order: pending latch, armed-set completion, budget, then
  the C54-C55 selection helper. Spend one unit per selected step. Add final `take_error`.
- **Discharges:** `SIM-STEPS`, `SIM-COMPLETION`, remaining `SIM-SELECT`, shipped
  `ENV-BOUNDS`, `ENV-LATCH` operation precedence.
- **Proves:** `sim_step_budget::exact_configured_count_is_permitted` citing `VERIFY-SIM`
  and `SIM-STEPS`; `sim_step_budget::budget_is_fresh_for_each_next_event` citing
  `SIM-STEPS`; `sim_step_budget::exhaustion_changes_no_time_arm_port_or_storage` citing
  `VERIFY-SIM`; `sim_source_completion::no_arm_at_entry_or_after_none_returns_completion_error`
  citing `SIM-COMPLETION`; `sim_storage_bounds::arms_and_budget_storage_never_grow`
  citing `ENV-BOUNDS`.
- **Rust notes:** Check before selecting, advancing, clearing, or incrementing. Use checked
  counter arithmetic even though the NonZero bound and pre-check make overflow remote.
- **Size:** 115-150 lines.

#### C57 — Sim shutdown and lifecycle matrix

- **Builds:** Finish `Environment` for Sim in `src/sim/environment.rs`: close admission,
  move every Open lifecycle to Ended before one ordered stop call, publish every mapped
  Error while latch stays open, close after all calls, always report Quiesced. Add the
  remaining per-Slot lifecycle matrix tests.
- **Discharges:** `SIM-SHUTDOWN`, remaining `SIM-LIFECYCLE`, Sim realization of
  `ENV-SHUTDOWN`, complete `VERIFY-SIM`.
- **Proves:** `sim_shutdown_lifecycle::stops_exactly_open_ports_once_in_frozen_order`
  citing `VERIFY-SIM`; `sim_shutdown_lifecycle::all_ok_returns_quiesced_without_error`
  citing `VERIFY-SIM`; `sim_shutdown_lifecycle::stop_error_at_each_slot_is_published_before_close`
  citing `VERIFY-SIM`; `sim_shutdown_lifecycle::first_stop_error_wins_without_skipping_later_ports`
  citing `VERIFY-SIM` and `ENV-LATCH`; `sim_lifecycle_exclusion::ended_port_receives_no_later_method`
  citing `VERIFY-SIM` and `SIM-LIFECYCLE`.
- **Rust notes:** A single ordered loop continues after every stop Error. Since all
  callbacks have returned and all started entries are Ended, quiescence is structural.
- **Size:** 120-150 lines.

### Phase H: Live Environment

All C58-C71 chunks are **§10-blocked**. They use the approved permanent `from_parts`
construction seam; C74-C75 expose it through generated typed wiring. No test depends on
sleep duration: barriers and the injected clock control every boundary.

#### C58 — Live config and fixed shared state

- **Builds:** Create wiring-only `src/live/mod.rs`; create `src/live/config.rs`,
  `src/live/shared.rs`, and `src/live/clock.rs`. Implement approved `LiveConfig`, private
  clock/deadline traits, fixed fan-in/inboxes, lifecycle, latch, completion entries, gate,
  and handles. Reserve every bounded container before activation.
- **Discharges:** storage portions of `LIVE-THREADS`, `LIVE-COMPLETION`, `ENV-BOUNDS`,
  `BOUND-STATIC`, `BOUND-NONZERO`.
- **Proves:** `live_storage_bounds::frozen_slots_have_one_inbox_completion_and_handle_entry`
  citing `VERIFY-LIVE` and `LIVE-COMPLETION`; `live_storage_bounds::fan_in_and_inboxes_start_empty_at_fixed_capacity`
  citing `ENV-BOUNDS`; `live_completion_state::every_entry_starts_outstanding` citing
  `LIVE-COMPLETION`.
- **Rust notes:** Put lifecycle, fan-in, latch, and completion under one `Mutex`; use one
  `Condvar` for predicate changes. Handles remain outside the lock but in frozen order.
- **Size:** 120-150 lines.

#### C59 — `LiveCtx` Command and lifecycle observations

- **Builds:** Create `src/live/context.rs` with final `LiveCtx`, `PortInput`,
  `OfferRejected`, and `Lifecycle` API shapes except `offer` implementation. Add the
  private offer-capability trait from `W4`; implement `recv`, `try_recv`, and `lifecycle`
  against one typed inbox and read-only shared lifecycle capability.
- **Discharges:** `LIVE-LIFECYCLE`, `LiveCtx` API portions approved by `W4`,
  `ENV-SHUTDOWN` observability.
- **Proves:** `live_context_receive::recv_reports_shutdown_ahead_of_queued_command` citing
  `VERIFY-LIVE` and `LIVE-LIFECYCLE`; `live_context_receive::try_recv_drains_commands_then_reports_shutdown_forever`
  citing `VERIFY-LIVE`, `LIVE-LIFECYCLE`, and `TRUST-DRAIN`;
  `live_context_receive::none_requires_no_command_and_running_lifecycle` citing
  `VERIFY-LIVE`; `live_context_lifecycle::lifecycle_reports_signal_directly` citing
  `TRUST-LIFECYCLE`.
- **Rust notes:** `recv` waits in a predicate loop while Running and empty; blocking wait
  is not an active loop. `try_recv` checks queue before signal by design.
- **Size:** 115-145 lines.

#### C60 — Live Event offering

- **Builds:** Grow `src/live/context.rs` with final `offer` delegation. Its concrete
  generated capability takes `C::Event`; under the shared lock it first rejects Closed or
  Full while the typed Event is still owned, otherwise maps through the frozen constructor
  and immediately admits the hidden Application Event sum before releasing the lock.
- **Discharges:** `LIVE-EVENTS`, fan-in bound of `ENV-BOUNDS`, fan-in portion of
  `PORT-ROUTING`.
- **Proves:** `live_event_offer::exact_capacity_succeeds_then_full_returns_same_event`
  citing `VERIFY-LIVE` and `LIVE-EVENTS`; `live_event_offer::shutdown_closed_returns_same_event`
  citing `VERIFY-LIVE`; `live_event_offer::occupancy_never_exceeds_configured_capacity`
  citing `ENV-BOUNDS`; `live_event_offer::successful_mapping_uses_bound_slot_constructor`
  citing `PORT-ROUTING`.
- **Rust notes:** The capacity decision is not admission. Mapping occurs after that check
  but immediately before the guaranteed insertion in the same critical section, so it
  still precedes admission and rejection can return the original Event without cloning.
- **Size:** 100-135 lines.

#### C61 — Completion capability and terminal guard

- **Builds:** Create `src/live/supervision.rs` with one private non-cloneable per-Slot
  completion capability and terminal guard. The guard classifies normal/Err/unwind state,
  publishes required Error before changing exactly its own entry to Complete, and wakes
  waiters once.
- **Discharges:** structural `LIVE-COMPLETION`, publication ordering in
  `LIVE-SUPERVISION`, `ENV-LATCH`.
- **Proves:** `completion_capability_ownership::each_shell_capability_changes_only_its_slot_once`
  citing `VERIFY-LIVE` and `LIVE-COMPLETION`;
  `completion_capability_ownership::live_context_and_port_cannot_receive_terminal_guard`
  citing `VERIFY-LIVE`; `supervision_publication_order::error_is_pending_before_entry_becomes_complete`
  citing `LIVE-SUPERVISION`; `supervision_completion::normal_and_error_return_complete_exactly_once`
  citing `VERIFY-LIVE`.
- **Rust notes:** Keep the guard as a local on the shell stack. Its nonpanicking `Drop`
  covers test-profile unwind; normal paths set classification data then let one Drop run.
- **Size:** 120-150 lines.

#### C62 — Start/cancel gate and supervisor shell

- **Builds:** Grow `src/live/supervision.rs` with final gate state and shell function:
  wait while Pending, return through guard on Cancel without invoking Port, or invoke
  `LivePort::run` on Start. Add permanent shell-spawn helper that retains handles in order.
- **Discharges:** gate and shell portions of `LIVE-START`, `LIVE-THREADS`,
  `LIVE-SUPERVISION`.
- **Proves:** `live_start_gate::port_run_does_not_begin_before_activation` citing
  `VERIFY-LIVE` and `LIVE-START`; `live_start_gate::cancel_completes_without_running_port`
  citing `LIVE-START`; `live_threads::each_bound_port_uses_one_supervised_thread` citing
  `LIVE-THREADS`.
- **Rust notes:** Spawned shells may exist before start commitment but cannot run user
  code. Move Port and `LiveCtx` into the shell; only gate state is shared.
- **Size:** 110-145 lines.

#### C63 — Live `start` success and failure

- **Builds:** Create `src/live/environment.rs` with permanent `from_parts` and full
  `start`: spawn in frozen order, perform all setup, freeze origin/start time, cancel/wake/
  join the spawned prefix on any failure, or signal Start as commitment with no later
  fallible setup. Apply approved thread names.
- **Discharges:** `LIVE-START`, `ENV-START`, start Commitment-points row, startup portion
  of `LIVE-TIME`, `W9`.
- **Proves:** `live_startup_success::start_signal_is_last_fallible_setup_boundary` citing
  `LIVE-START`; `live_startup_failure::each_failure_position_cancels_and_joins_all_spawned_shells`
  citing `VERIFY-LIVE`; `live_startup_failure::no_port_code_runs_before_failed_start_returns`
  citing `ENV-START`; `live_thread_names::name_contains_frozen_ordinal_and_slot` citing
  the approved thread-naming convention.
- **Rust notes:** Retain every handle immediately after spawn. On failure, preserve the
  original setup Error and discard impossible gate-shell cleanup results after joining.
- **Size:** 120-150 lines.

#### C64 — Live wait arbitration and wakeups

- **Builds:** Grow `src/live/environment.rs` with a final private wait helper for
  `next_event`: under the shared lock, return/take a pending Error or wait without busy
  spin until latch pending or fan-in nonempty. Do not dequeue here.
- **Discharges:** waiting and publication-order portions of `LIVE-SELECT`, `ENV-LATCH`.
- **Proves:** `live_selection_wait::empty_running_environment_blocks_without_spinning`
  citing `LIVE-SELECT`; `live_selection_wait::pending_error_returns_before_event_consumption`
  citing `ENV-LATCH`; `live_supervision_wakeup::premature_completion_wakes_blocked_selection_with_error`
  citing `VERIFY-LIVE` and `LIVE-SUPERVISION`.
- **Rust notes:** `Condvar::wait` may wake spuriously; recheck predicates under the same
  lock. The lock acquisition chooses the allowed order for overlapping publication.
- **Size:** 90-125 lines.

#### C65 — Live clock and dequeue commitment

- **Builds:** Grow `src/live/environment.rs` with final `next_event`: after C64 says an
  Event is available, stamp from the injected monotonic clock immediately before dequeue;
  checked conversion failure returns before dequeue; dequeue and return have no later
  fallible work.
- **Discharges:** `LIVE-SELECT`, `LIVE-TIME`, next-event Commitment-points row,
  `ENV-ERRORS`, event order portion of `ENV-BOUNDS`.
- **Proves:** `live_event_selection::admitted_events_are_dequeued_in_admission_order`
  citing `VERIFY-LIVE`; `live_event_selection::event_waking_wait_is_stamped_no_earlier_than_admission`
  citing `VERIFY-LIVE` and `LIVE-TIME`; `live_event_selection::time_exhaustion_leaves_event_queued`
  citing `VERIFY-LIVE`; `selection_commitment_order::successful_stamp_precedes_dequeue_with_no_fallible_tail`
  citing `LIVE-SELECT` and `ENV-ERRORS`.
- **Rust notes:** Hold the queue lock across stamp and dequeue to keep the selected front
  stable. Injected clock returns typed conversion failure before mutation.
- **Size:** 105-140 lines.

#### C66 — Live Command dispatch

- **Builds:** Grow `src/live/environment.rs` with final dispatch using the generated
  exhaustive router: under the shared observation discipline prefer pending latch, then
  attempt one nonblocking admission to the typed destination inbox.
- **Discharges:** `LIVE-DISPATCH`, dispatch Commitment-points row, `ENV-LATCH`, inbox part
  of `ENV-BOUNDS`, `PORT-ROUTING` pending generated evidence.
- **Proves:** `live_dispatch_precedence::pending_error_prevents_inbox_admission` citing
  `ENV-LATCH`; `live_command_dispatch::exact_capacity_hands_each_command_off_once` citing
  `VERIFY-LIVE` and `LIVE-DISPATCH`; `live_command_dispatch::full_inbox_returns_typed_error_without_growth_or_handoff`
  citing `VERIFY-LIVE`; `live_command_dispatch::closed_inbox_returns_typed_error_without_handoff`
  citing the dispatch Commitment-points row.
- **Rust notes:** The router's match arm owns the Command and its typed sender. A successful
  queue insertion is the handoff; no Port processing result can revise it.
- **Size:** 105-140 lines.

#### C67 — Shutdown initiating instant and deadline

- **Builds:** Grow `src/live/environment.rs` with a final private initiation operation:
  in one critical section raise lifecycle signal, close fan-in, wake all blocking points,
  leave latch open, and create one absolute finite/infinite deadline from the injected
  clock.
- **Discharges:** initiating half of `LIVE-SHUTDOWN`, `ENV-SHUTDOWN`, `LIVE-LIFECYCLE`,
  deadline arithmetic in `ENV-BOUNDS`.
- **Proves:** `live_shutdown_initiation::signal_and_fan_in_close_are_one_observation`
  citing `VERIFY-LIVE` and `LIVE-SHUTDOWN`; `live_shutdown_initiation::latch_remains_open`
  citing `ENV-LATCH`; `live_shutdown_deadline::addition_overflow_saturates_to_infinite`
  citing `VERIFY-LIVE` and A6; `live_shutdown_wakeup::every_kavod_blocking_point_is_woken`
  citing `LIVE-SHUTDOWN`.
- **Rust notes:** Return a private deadline budget token from initiation. No later code may
  construct or restart a deadline.
- **Size:** 100-135 lines.

#### C68 — Completion waiting and final observation

- **Builds:** Grow `src/live/environment.rs` with a final wait helper that repeatedly scans
  the authoritative fixed set and waits only while an entry is Outstanding and budget
  remains. Then one final critical section scans completion and closes the latch into a
  `ShutdownReport`.
- **Discharges:** wait/final-observation portions of `LIVE-SHUTDOWN`, `LIVE-COMPLETION`,
  `ENV-LATCH`, `BOUND-LOOPS`.
- **Proves:** `live_shutdown_wait::completion_before_shutdown_remains_visible` citing
  `VERIFY-LIVE`; `live_shutdown_wait::all_waits_consume_one_absolute_deadline` citing
  `VERIFY-LIVE`; `live_shutdown_wait::completion_during_wait_wakes_promptly` citing
  `VERIFY-LIVE`; `live_shutdown_final_observation::completion_racing_expiry_has_one_consistent_classification`
  citing `VERIFY-LIVE`; `live_shutdown_final_observation::publication_racing_close_has_one_consistent_report`
  citing `ENV-LATCH`.
- **Rust notes:** Do not cache completion count. The final scan and latch close occur while
  holding the same lock used by guard publication/Complete.
- **Size:** 115-150 lines.

#### C69 — Live consuming shutdown, joins, and detach

- **Builds:** Finish `Environment::shutdown` in `src/live/environment.rs` by composing C67-
  C68. If final report is Quiesced, extract and join every handle in frozen order; if
  Incomplete, drop every unjoined handle without waiting. Add final `take_error`.
- **Discharges:** remaining `LIVE-SHUTDOWN`, `ENV-SHUTDOWN`, `ENV-SERIAL` consuming
  lifecycle, `BOUND-LOOPS`.
- **Proves:** `live_shutdown_joining::no_join_begins_while_any_entry_is_outstanding`
  citing `VERIFY-LIVE`; `live_shutdown_joining::quiesced_joins_every_supervised_thread`
  citing `VERIFY-LIVE`; `live_shutdown_detach::expiry_returns_incomplete_none_and_drops_handles`
  citing `VERIFY-LIVE`; `live_shutdown_detach::expiry_with_error_returns_first_publication`
  citing `VERIFY-LIVE` and `ENV-LATCH`.
- **Rust notes:** Joining after every entry is Complete is intentionally not deadline
  bounded. A panicked test-profile thread can be joined and still counts Quiesced.
- **Size:** 105-140 lines.

#### C70 — Supervision and final-close race suite

- **Builds:** Create `tests/live_lifecycle.rs` and
  `tests/support/live_control.rs` with barriers, thread-state handles, and injected clock.
  Cover normal return, typed Err, pre-signal unwind, post-signal Ok/Err, close races, and
  post-close publication.
- **Discharges:** supervision/race portions of `VERIFY-LIVE`, `VERIFY-LATCH`,
  `LIVE-SUPERVISION`, `LIVE-COMPLETION`.
- **Proves:** `live_supervision_completion::normal_error_and_unwind_complete_exactly_once`
  citing `VERIFY-LIVE`; `live_supervision_publication::required_error_precedes_complete`
  citing `VERIFY-LIVE`; `live_supervision_publication::ok_after_signal_is_unpublished`
  citing `LIVE-SUPERVISION`; `live_supervision_publication::error_before_close_enters_report`
  citing `VERIFY-LIVE`; `live_shutdown_races::post_close_publication_is_discarded`
  citing `ENV-LATCH`; `live_shutdown_joining::joined_panicked_thread_is_quiesced_not_succeeded`
  citing `VERIFY-LIVE`.
- **Rust notes:** Use barriers around lock acquisition and final-observation release. Never
  assert which side an overlap chooses; assert returned result and resulting state agree.
- **Size:** 120-150 lines.

#### C71 — Live capacity, signal, and timing suite completion

- **Builds:** Grow `tests/live_lifecycle.rs` with remaining end-to-end capacity, blocked
  recv, drain, selection, dispatch, fixed storage, deadline, and per-Slot checks. No
  production change.
- **Discharges:** complete `VERIFY-LIVE`; `TRUST-LIFECYCLE`, `TRUST-DRAIN`, and
  `TRUST-INBOX` fixture checks; shipped Live `ENV-BOUNDS`.
- **Proves:** `live_context_lifecycle::blocked_recv_observes_shutdown_within_window`
  citing `VERIFY-LIVE` and `TRUST-LIFECYCLE`; `live_context_lifecycle::draining_port_consumes_commands_before_return`
  citing `TRUST-DRAIN`; `live_capacity_bounds::fan_in_inbox_completion_and_wakeup_storage_never_grow`
  citing `VERIFY-LIVE` and `ENV-BOUNDS`; `live_shutdown_deadline::late_wakeups_do_not_restart_deadline`
  citing `VERIFY-LIVE`; `live_routing_slots::each_slot_receives_only_its_commands`
  citing `TRUST-ROUTING`; `live_selection_commitment::exhaustion_and_success_preserve_dequeue_rules`
  citing `VERIFY-LIVE`.
- **Rust notes:** Reuse shared control fixtures; keep each nested module cohesive even
  though this file now contains many groups. Split the file physically only by suite, not
  by arbitrary line count.
- **Size:** 120-150 lines.

### Phase I: Wiring and Public Construction

All C72-C77 chunks are **§10-blocked** and implement the approved `W1`-`W9` answers.
This phase is last by design. It exposes permanent construction seams already exercised
internally; it does not replace Sim or Live mechanics.

#### C72 — One-source `wiring!` Slot declaration

- **Builds:** Create `src/wiring.rs` with the first half of exported `wiring!`: require one
  first Slot, invoke exact `ports!` for generated-sum form, generate Slot identity and
  frozen metadata in declaration order, and support the explicit hand-written-sum form.
  Grow `src/lib.rs` only for macro export.
- **Discharges:** construction source of `BOUND-STATIC`, sum/order portions of
  `PORT-SUMS`, approved `W1`, `W7`.
- **Proves:** `wiring_slot_set::empty_declaration_has_no_matching_macro_form` citing
  `BOUND-STATIC` (compile-fail fixture); `wiring_slot_order::metadata_matches_sum_declaration_order`
  citing `BOUND-STATIC`; `wiring_generated_sums::wrapper_preserves_exact_ports_expansion`
  citing `PORT-SUMS`.
- **Rust notes:** Use the P10 pattern. Generated Slot metadata may derive Copy because it
  is identity, not mutable topology. Do not modify `ports!` itself.
- **Size:** 105-140 lines.

#### C73 — Generated Sim Error sum and exhaustive router

- **Builds:** Grow `src/wiring.rs` macro expansion with the per-Slot Sim Error variants,
  built-in Sim variants, typed Port adapters, Event constructors, and one exhaustive
  Command match. Keep constructor emission for C74.
- **Discharges:** Sim portions of `PORT-ROUTING`, `PORT-SUMS`, `TRUST-ROUTING`, approved
  `W2`, `W3`.
- **Proves:** `sim_wiring_events::each_constructor_targets_its_named_slot` citing
  `TRUST-ROUTING`; `sim_wiring_commands::each_variant_invokes_exactly_one_matching_adapter`
  citing `PORT-ROUTING`; `sim_wiring_errors::each_port_error_maps_to_one_named_variant`
  citing `TRUST-ROUTING`; `sim_wiring_payloads::router_never_inspects_payload_contents`
  citing `PORT-STATE`.
- **Rust notes:** Keep payload typed through the match arm. Erase only after the compiler
  has paired that payload with its Contract adapter.
- **Size:** 115-150 lines.

#### C74 — Generated Sim construction

- **Builds:** Finish Sim expansion in `src/wiring.rs`: generate the namespace's associated
  `sim` constructor, exact one-binding-per-Slot arguments, map approved `SimConfig`, and
  expose the existing Sim `from_parts` seam with the minimum visibility needed by the
  macro. Add public construction tests.
- **Discharges:** Sim construction part of `BOUND-STATIC`, `BOUND-NONZERO`, `PORT-ROUTING`,
  approved `W1`, `W6`, `W7`.
- **Proves:** `sim_wiring_construction::associated_constructor_requires_every_slot_once`
  citing `PORT-SUMS`; `sim_wiring_construction::constructed_order_drives_start_select_and_stop`
  citing `BOUND-STATIC`; `sim_wiring_configuration::origin_and_nonzero_budget_are_frozen`
  citing `BOUND-NONZERO` and `SIM-TIME`.
- **Rust notes:** Fixed positional arguments avoid a typestate-builder explosion while
  still acting as registration in declaration order. `SimBinding<C, P>` disambiguates a
  Port type implementing multiple Contracts.
- **Size:** 100-135 lines.

#### C75 — Generated Live Error sum and exhaustive router

- **Builds:** Grow `src/wiring.rs` macro expansion with per-Slot Live Error variants,
  approved built-in variants, typed `LiveCtx` factories/Event constructors, and one
  exhaustive Command-to-inbox match.
- **Discharges:** Live portions of `PORT-ROUTING`, `PORT-SUMS`, `TRUST-ROUTING`, approved
  `W2`-`W4`.
- **Proves:** `live_wiring_events::each_context_uses_its_named_fan_in_constructor` citing
  `TRUST-ROUTING`; `live_wiring_commands::each_variant_targets_exactly_one_matching_inbox`
  citing `PORT-ROUTING`; `live_wiring_errors::port_spawn_inbox_time_and_closure_errors_keep_typed_variants`
  citing A7 and `PORT-ROUTING`; `live_wiring_payloads::router_reads_only_discriminant`
  citing `PORT-STATE`.
- **Rust notes:** Generated shell closures capture typed Port/receiver/error mapper. The
  Environment sees only frozen adapters and never deserializes or formats payloads.
- **Size:** 120-150 lines.

#### C76 — Generated Live construction and thread identity

- **Builds:** Finish Live expansion in `src/wiring.rs`: generate associated `live`
  constructor taking `LiveConfig` and one `LiveBinding<C, P>` per Slot, pass frozen inbox
  capacities and names to existing `from_parts`, and expose only required support seams.
- **Discharges:** Live construction part of `BOUND-STATIC`, `BOUND-NONZERO`,
  `PORT-ROUTING`, `TRUST-INBOX`, approved `W1`, `W5`, `W7`, `W9`.
- **Proves:** `live_wiring_construction::associated_constructor_requires_every_slot_once`
  citing `PORT-SUMS`; `live_wiring_configuration::fan_in_timeout_and_inboxes_are_nonzero_and_frozen`
  citing `BOUND-NONZERO`; `live_wiring_order::one_order_drives_spawn_route_shutdown_and_join`
  citing `BOUND-STATIC`; `live_wiring_thread_names::ordinal_and_slot_name_follow_frozen_order`
  citing the approved thread-naming convention.
- **Rust notes:** Config and bindings are consumed. No registration method remains on the
  resulting Environment.
- **Size:** 105-140 lines.

#### C77 — Crate-root public surface

- **Builds:** Make the final planned touches to `src/lib.rs`, `src/engine/mod.rs`,
  `src/live/mod.rs`, and `src/sim/mod.rs`. Re-export all approved public items once at
  nonrepeating paths; keep each directory `mod.rs` wiring-only. Update compile-fail
  fixture include paths only if final module paths require it.
- **Discharges:** `CRATE-EXPORTS`, approved `W8`; completes public API reachability.
- **Proves:** `public_item_paths::documented_items_are_reachable_without_repeated_segments`
  citing `CRATE-EXPORTS`; `public_item_paths::private_engine_children_are_not_public_paths`
  citing `CRATE-EXPORTS`; `public_macro_paths::ports_and_wiring_are_available_at_crate_root`
  citing the `ports!` and approved `wiring!` APIs.
- **Rust notes:** Re-export children, not child modules. Do not add compatibility aliases;
  this is the first final public surface.
- **Size:** 70-105 lines.

### Phase J: Cross-Environment Contract Suites

These chunks are **§10-blocked** because they instantiate the public generated Live and
Sim Environments. They add tests and harness code only.

#### C78 — Canonical trace model and graph oracle

- **Builds:** Add `tests/support/trace.rs`, `tests/support/projections.rs`, and create
  `tests/conformance_trace.rs`. Model every Environment and sink call/result, handler,
  State transition, Command handoff, and shutdown report; erase Error values but preserve
  presence/position. Add a graph oracle over scripted cases.
- **Discharges:** infrastructure for `VERIFY-CONFORMANCE`, `DET-RUN`, `DET-ENV`, observable
  `TRUST-ENV` certification.
- **Proves:** `environment_graph_conformance::scripted_calls_follow_graph_order` citing
  `VERIFY-CONFORMANCE`; `environment_graph_conformance::each_prepared_command_is_handed_off_once_in_order`
  citing `VERIFY-CONFORMANCE`; `environment_graph_conformance::each_effects_complete_turn_has_one_checkpoint`
  citing `RUN-CHECKPOINT`; `bespoke_environment_certification::scripted_bespoke_environment_passes_observable_contract`
  citing `TRUST-ENV` and `VERIFY-CONFORMANCE`.
- **Rust notes:** Projection types list every Core-owned discriminant/payload explicitly;
  this prevents a new enum payload from silently escaping comparison.
- **Size:** 120-150 lines.

#### C79 — Within-Environment repeatability

- **Builds:** Grow `tests/conformance_trace.rs` with a fresh-run factory and run every
  expressible scripted trace twice for Live and twice for Sim. Compare handler calls,
  State transitions, Command intent, bytes through last commit, and complete Core
  projections; compare whole exits when erased Errors correspond.
- **Discharges:** `DET-RUN`, within-type portion of `VERIFY-CONFORMANCE`, `TRUST-PURE`,
  `TRUST-SIM-PORT`, fixture portion of `TRUST-SERIALIZE`.
- **Proves:** `run_trace_repeatability::every_live_trace_repeats_identical_core_outputs`
  citing `DET-RUN` and `VERIFY-CONFORMANCE`; `run_trace_repeatability::every_sim_trace_repeats_identical_core_outputs`
  citing `DET-RUN`, `VERIFY-CONFORMANCE`, and `TRUST-SIM-PORT`;
  `run_trace_repeatability::corresponding_error_values_produce_equal_full_exits` citing
  `DET-RUN`.
- **Rust notes:** Recreate Application, State, Environment, writer, clock script, and
  config for each run; reusing a mutated fixture would not test determinism.
- **Size:** 105-140 lines.

#### C80 — Cross-Environment equivalence

- **Builds:** Finish `tests/conformance_trace.rs` with equal accepted traces expressed by
  generated Live and Sim test wiring. Compare every Core-owned field named by `DET-ENV`;
  explicitly identify Environment-specific failure cases excluded from comparison.
- **Discharges:** `DET-ENV`, remaining `VERIFY-CONFORMANCE`.
- **Proves:** `environment_cross_type_equivalence::equal_traces_have_equal_handlers_state_intent_bytes_and_core_exit`
  citing `DET-ENV` and `VERIFY-CONFORMANCE`; `environment_cross_type_equivalence::projection_includes_every_named_core_payload`
  citing `DET-ENV`; `environment_cross_type_equivalence::environment_specific_failure_shapes_are_not_compared`
  citing `DET-ENV`.
- **Rust notes:** Do not compare Error payloads across types. Do compare `JournalError`
  variants/operations and all `CoreError` payload values.
- **Size:** 100-135 lines.

#### C81 — Latch ordering and failure precedence conformance

- **Builds:** Create `tests/environment_latch.rs` with a shared scenario interface for
  Live, Sim where expressible, and a controllable bespoke Environment. Use barriers for
  strict before, strict after, and overlap around next-event, dispatch, take-error, and
  close; script local pre-commit failures.
- **Discharges:** operation-order, first-wins, blocked-select, and local-failure portions of
  `VERIFY-LATCH`, `ENV-LATCH`, observable `TRUST-ENV` latch certification.
- **Proves:** `environment_latch_ordering::publication_before_call_is_reported_by_that_call`
  citing `VERIFY-LATCH`; `environment_latch_ordering::publication_after_return_remains_pending`
  citing `VERIFY-LATCH`; `environment_latch_ordering::overlap_accepts_only_two_consistent_placements`
  citing `VERIFY-LATCH`; `environment_latch_failure_precedence::pending_error_beats_local_failure_and_discards_secondary`
  citing `ENV-LATCH` and A4; `environment_latch_failure_precedence::overlap_with_local_failure_exercises_both_orders`
  citing `VERIFY-LATCH`; `environment_latch_blocked_selection::publication_wakes_and_reports_error`
  citing `VERIFY-LATCH`.
- **Rust notes:** For overlap, derive which order occurred from the returned value and then
  assert the resulting latch state. Never require one scheduler outcome.
- **Size:** 120-150 lines.

#### C82 — Latch shutdown window and Stop integration

- **Builds:** Finish `tests/environment_latch.rs` with permanent reporting, final simulated
  Command Error checkpoint, latch-open shutdown window, typed Error before close,
  close-race consistency, post-close discard, and all Stop report shapes.
- **Discharges:** remaining `VERIFY-LATCH`; Stop integration of `RUN-CHECKPOINT`,
  `RUN-FINALIZE`, `ENV-LATCH`.
- **Proves:** `environment_latch_first_error::first_error_is_reported_permanently`
  citing `VERIFY-LATCH`; `environment_latch_final_command::sim_command_error_is_observed_at_checkpoint`
  citing `VERIFY-LATCH` and `RUN-CHECKPOINT`; `environment_latch_shutdown_window::latch_stays_open_until_final_observation`
  citing `VERIFY-LATCH`; `environment_latch_shutdown_window::close_race_and_post_close_publication_are_consistent`
  citing `VERIFY-LATCH`; `run_stop_report::only_quiesced_none_reaches_stopped` citing
  `VERIFY-LATCH`; `run_stop_report::some_error_is_environment_shutdown_even_when_incomplete`
  citing `RUN-FINALIZE`; `run_stop_report::incomplete_none_is_shutdown_incomplete`
  citing `RUN-FINALIZE`.
- **Rust notes:** Stop-path assertions include both typed exit and exact final Journal
  bytes; a report Error outranks Incomplete, while clean Incomplete is a Core cause.
- **Size:** 120-150 lines.

## 4. Suite Build-Out Map

### Permanent Harness Inventory

| Harness | First built | Reused by | Ownership rule |
|---|---|---|---|
| Source-local Context fixture | C7-C8 | C35, C42-C44 | Remains in `src/application.rs` outer `tests` module; only shared Context helpers sit outside nested groups. |
| Source-local Journal counting/fault sink | C10 | C11-C14 | Remains in `src/journal.rs`; tests private encode/poison state without exposing it publicly. |
| Scripted bespoke Environment | C38 | C40-C44, C78-C82 | `tests/support/scripted_environment.rs`; typed scripts, full call/effect trace, no hidden retries. |
| Scripted memory/fault sink | C39 | C40-C44, C78-C80 | `tests/support/scripted_sink.rs`; exact bytes and every write/flush result. |
| Golden runner | C40 | C41-C44 | `tests/golden_journal.rs`; exact byte strings plus operation ordering, never parsed-only assertions. |
| Compile-fail fixture crate | C45 | C46-C47 | `tests/fixtures/grammar`; `include!` reconstructs Engine visibility and a compiling control validates the fixture. |
| Sim per-Slot call recorder | C50 | C51-C57, C73-C74, C79-C82 | Source-local to Sim tests until imported through public wiring; records lifecycle before/after each callback. |
| Injected monotonic clock/deadline | C58 | C63-C71, C79, C81-C82 | Private clock trait; tests advance/fail/saturate time explicitly and never sleep. |
| Live barriers/thread probes | C70 | C71, C79-C82 | `tests/support/live_control.rs`; barriers expose publication, Complete, expiry, close, join, and detach boundaries. |
| Canonical trace/projection | C78 | C79-C80 | `tests/support/trace.rs` and `projections.rs`; Error values erased, presence/position retained, all Core-owned fields explicit. |

### Required Verification Rows

| Verification row | Chunks that build it | Required harness | Completion evidence |
|---|---|---|---|
| `VERIFY-CONTEXT` | C8, C35, C42-C44 | Source-local Context fixture; scripted Environment and sink for Engine Fatal paths. | C8 proves exact capacity, order, sticky overflow, later discard, and fresh reset. C42 proves Fatal and overflow State survival/precedence. C43-C44 table-drive State survival across every post-handler Environment and Journal Fatal path. Complete at C44. |
| `VERIFY-JOURNAL` | C11, C20-C23, C40-C44 | `RawValue` newline fixture, memory sink, scripted Environment, golden runner. | C11 proves literal interior-newline rejection with no sink calls. C20-C23 pin each payload shape locally. C40-C41 pin every normal graph sequence and byte, including answer classification. C44 pins fatal truncation at every record edge and matching fatal kind/outcome. Complete at C44. |
| `VERIFY-FAULTS` | C10-C14, C38-C39, C42-C44 | Scripted sink and Environment, mutating/over-emitting Applications. | C42 covers over-emission and decreasing time. C43 covers every Environment operation Error, both shutdown-report Fatal shapes, start-Err no-shutdown, and post-start Error × shutdown Error. C44 covers every record edge and retained Quiesced after Stop completion commit failure. Complete at C44. |
| `VERIFY-GRAMMAR` | C24-C33, C45-C47 | Include-based fixture crate at Engine visibility position. | C46 proves illegal order, skipped checkpoint, premature Stop, and independent dispatch witness fail. C47 proves wrong outcomes and absence of Clone/Copy/Default. Control compiles in C45. Complete at C47. |
| `VERIFY-SIM` | C50-C57, with public routing confirmation C73-C74 | Source-local indexed Ports, per-call trace, fixed arm/storage inspection. | C50 covers arm mutation. C51-C52 cover success and every startup failure position/prefix. C53 covers on-command Err and handoff. C54-C56 cover step Err, selection, ties, cursor, None, exact budget, no-mutation exhaustion, fixed storage, and no-arm at entry/mid-loop. C57 covers every stop result/position, first-wins, continued stopping, lifecycle exclusion, and Quiesced reports. Public per-Slot routing is confirmed C73-C74. Core suite complete at C57; wiring evidence complete at C74. |
| `VERIFY-LIVE` | C58-C71, with public routing confirmation C75-C76 | Fixed storage probes, injected clock/deadline, barriers, controllable Ports, join/drop handles. | C58 proves one fixed entry/inbox/wakeup per Slot. C59-C60 prove recv/try_recv/lifecycle/offer semantics and capacities. C61-C63 prove guard ownership, publication-before-Complete, gate activation, and failed-start cancel/join. C64-C66 prove wake/select/time/dequeue and dispatch. C67-C69 prove one initiating instant/deadline, saturation, final observation, no early join, Quiesced join, Incomplete detach, and report shapes. C70 covers normal/Err/unwind and boundary races/post-close. C71 closes load, drain, blocking, capacity, and timing cases. Public per-Slot routing/thread identity is confirmed C75-C76. Core suite complete at C71; wiring evidence complete at C76. |
| `VERIFY-CONFORMANCE` | C38-C39, C77-C80 | Canonical trace oracle, scripted Environment/sink, generated Live/Sim wiring, projection comparators. | C78 checks every operation against the graph, every handoff, checkpoint, and fatal finalization, and certifies the bespoke fixture's observable contract. C79 repeats every trace twice within each shipped type and compares all `DET-RUN` outputs. C80 compares expressible overlap and every `DET-ENV` Core projection across types, excluding only named type-specific failure shapes. Complete at C80. |
| `VERIFY-LATCH` | C48-C49, C53, C56-C57, C61, C64, C67-C70, C81-C82 | Shared latch state machine, Sim final-command case, Live barriers/injected clock, bespoke controllable Environment. | C48-C49 prove sequential first-wins/report/close and local precedence. Sim/Live chunks prove their concrete observation points. C81 covers before/after/overlap for operations, both overlap orders with local failure, pending precedence, permanent first reporting, and blocked select wake. C82 covers final Command checkpoint, graceful window, typed pre-close Error, close race, post-close discard, and all Stop report integrations. Complete at C82. |

### Trusted Checks Landed by the Suites

| Obligation | Planned check |
|---|---|
| `TRUST-PURE` | C79 recreates and repeats every scripted run, comparing bytes and `DET-RUN` exits. This checks the fixture Application, not arbitrary user code. |
| `TRUST-SIM-PORT` | C57 selection/lifecycle traces and C79 repeatability check the shipped suite's Sim Ports. |
| `TRUST-ENV` | C78 and C81-C82 certify the bespoke fixture's observable graph/latch behavior; bounds and hidden effects remain review items as the obligation states. |
| `TRUST-ABORT` | C1 profile setting plus CI/profile review; runtime tests intentionally unwind for Live guard coverage only. |
| `TRUST-ROUTING` | C73-C76 exercise every Slot's Event constructor, Command destination, and Error variant. |
| `TRUST-SERIALIZE` | C40-C44 exact bytes and C79 repetition check the fixture payload serializers. |
| `TRUST-LIFECYCLE` | C59 and C71 block a Port in `recv` and prove shutdown observation under load. |
| `TRUST-DRAIN` | C59 and C71 use `try_recv` to drain queued Commands before return. |
| `TRUST-SINK` | C10-C14 and C39-C44 check memory-sink exact storage/fault behavior; deployment freshness, exclusivity, and durability remain review obligations. |
| `TRUST-INBOX` | C66/C71/C76 verify configured capacity exactly; deployment sizing remains config review. |

The remaining trusted rows are not converted into tests they cannot support:
`TRUST-BLOCKING`, `TRUST-SPAWN`, `TRUST-SHUTDOWN`, and `TRUST-MEMORY` remain code/owner
review; `TRUST-KEY` and `TRUST-SIZING` require application-specific per-Slot tests;
`TRUST-EXIT` remains operational review.

## 5. Risk List

| Risk | Symptom during implementation | Fallback realization | De-risking probe |
|---|---|---|---|
| Typestate ownership across handler, Journal, and consuming shutdown | Borrow-checker errors such as a second mutable Environment borrow, use after moving a certificate, or inability to return State on one branch. | Shorten Context/record borrows with lexical blocks; keep Journal solely in the certificate; use private result enums and `Option<E>` only in Engine orchestration. Do not clone data or weaken consuming receivers. | P1, P3, and P5 compiled the three interacting ownership shapes. |
| `wiring!` expansion with heterogeneous Ports and errors | Inference failures, private-type leakage, or a generated fan-out arm that loses payload type before reaching its destination. | Generate concrete wrapper/error/adapter types per invocation and fixed associated constructors; keep each payload typed through its exhaustive match arm, erasing only the operation afterward. Split C73/C75 by Event and Command expansion if either exceeds 150 lines. | P2 proves projected sums; P10 proves one-source nested expansion/order metadata; P11 proves exact `LiveCtx<C>` can hide the Application Event sum without erasing its input. |
| Bounded Journal classification and uncertain suffix | Oversize records surface as `Encode`, newline uses payload capacity, `Interrupted` retries, or a sink receives bytes before encoding/classification completes. | Keep one dedicated byte buffer with sticky `bound_hit`; classify complete bytes before newline; use an explicit offset loop and poison at the first sink failure. Never use `write_all`, which hides over-report handling and retry policy. | P7 proves bounded serde `Write`; P9 proves the literal-newline fixture; P6 proves byte/order metadata source. |
| Live publication/completion/deadline lattice | Flaky tests, shutdown reports Complete without the Port Error, deadline restarts after wakeups, or joins begin while one entry is Outstanding. | Keep lifecycle, latch, and completion under one mutex; publish then mark Complete under that lock; derive no cached count; carry one absolute deadline token; use barriers/injected time instead of sleeps. | P8 executes publication-before-Complete plus final close in one critical section; P4 confirms Port API bounds do not obstruct the shell. |
| Compile-fail fixture attacks privacy instead of grammar | Cases fail with inaccessible/private-item errors, so `VERIFY-GRAMMAR` gives false confidence. | Include production Engine files and each attack inside the reconstructed module at the same visibility position; require the control case to compile before running attacks; check the marked attack expression rather than exact diagnostics. | P5 proves the intended private transition shape compiles when code is at the owning module boundary. |

Approval of the overall plan is the prerequisite to C1. If `W1`-`W9` remain pending,
C1-C49 can still proceed and stop green at that boundary. If any proposal changes,
renumbering is unnecessary: update the affected blocked chunks before C50, then continue
only when every chunk still extends final code forward without replacement.
