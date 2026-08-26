# Kavod Core — Implementation Plan (final)

> **Inputs:** `design_docs/design-v12.md` (authoritative) and `design_docs/test.md`
> (binding test pattern). Everything here implements those two documents; nothing here
> amends them. Where this plan names an ID, the design document's row is the rule.
>
> **Provenance:** this plan supersedes `impl-plan.md` and `impl-plan-2.md`, merging the
> two: the Wiring proposals, probes, and phase structure come from the first; the finer
> chunk decomposition, the trusted-obligation map, and the completion-evidence tracking
> come from the second. Both sources can be deleted once this plan is approved.
>
> **How to use this plan:** approve section 1 first — 1a's proposals unblock the
> §10-gated chunks, 1c's choices become deliberate. Then build the chunks in order.
> Every chunk ends with `cargo test` and `cargo clippy --all-targets -- -D warnings`
> green and at least one named test passing that did not pass before. At any point
> between two chunks the crate compiles, is fully tested, and does exactly what its
> passed tests say.
>
> **Approving 1a also settles the design doc's open Wiring section**: once approved,
> those answers are what v13 of the design document should record. That doc edit is
> separate work, done after (or alongside) the Wiring-area chunks — this plan only
> needs the answers, not the doc edit.

---

## 1. Blockers & decisions register

### 1a. §10 Wiring decisions — proposed answers

The design document lists nine open decisions and fixes the constraints any answer must
satisfy (every Environment/Live/Sim guarantee, the commitment table, `RUN-GRAMMAR`,
`Send + 'static` boundaries, frozen Slot order as the only ordering authority, nonempty
Port set, nonzero bounds, everything frozen before `Engine::run`). Each proposal below
was checked against that list. One shared answer is used wherever the question is shared
between the two Environments.

| # | Decision | Proposed answer | Rationale (one line) |
|---|---|---|---|
| W1 | Builder / registration API | One builder per Environment: `LiveWiring` / `SimWiring`. `wiring.slot(name, port, fan_in, err_map, …caps) -> SlotHandle<C>` registers one Slot; registration order is the frozen Slot order. The zero-slot state is a distinct builder type without a `build` method, converted by the first `slot` call — an empty Port set is unrepresentable (`BOUND-STATIC` at the unrepresentable tier). Live's `slot` also takes that Slot's inbox capacity (`NonZeroUsize`); `build(config, router)` finishes construction with everything frozen before `Engine::run`. | Registration is the only order the Environment can observe (see W7), and typestate makes the nonempty rule compile-time instead of a runtime check. |
| W2 | Where fan-in constructors and the fan-out match live | Both live in user wiring code and are *received* by the builder, keeping the fan-out match hand-written as the decision text describes it: the frozen fan-in constructor is a per-Slot `fn(C::Event) -> Ev` passed to `slot` (in practice the variant constructor, e.g. `TradingEvent::Primary`); the fan-out is one user-written exhaustive `match` inside the `router` closure passed to `build` — `FnMut(Cmd, &mut Outlets<'_>) -> Result<(), HandoffRefused>` — whose arms call `outlets.hand_off(&handle, payload)` with the typed `SlotHandle<C>` from registration. The compiler proves the match exhaustive and the payload types agree (`PORT-ROUTING`); each arm naming the right Slot stays `TRUST-ROUTING`. | Keeps the single exhaustive destination match the contract requires, with payload agreement type-checked through the handles, and needs no macro metadata (the macro stays two enums). |
| W3 | Environment `Error` sums | Kavod ships two generic enums, closed except for one user parameter, their Kavod-owned variants exactly the inventory the decision text fixes: `LiveError<PE> { InboxFull { slot }, SpawnFailed { slot, source: std::io::Error }, TimeExhausted, PrematureClosure { slot }, Port(PE) }` and `SimError<PE> { NothingArmed, StepBudgetExhausted, Port(PE) }` (`slot` = the registered `&'static str` name). `PE` is the wiring author's hand-written sum with one variant per Slot's Port Error; the per-Slot `err_map: fn(P::Error) -> PE` passed at registration is the mapping site (fan-in `Full` goes to the offering Port, never the Engine, exactly as the decision fixes). No closed-inbox variant exists, deliberately: inboxes close only at shutdown initiation, which the serial Run cannot overlap with `dispatch`; a Port that ends early publishes (its Error, or `PrematureClosure`) under the central lock before completing, so `dispatch`'s pending-latch-first check returns that publication — and a Command admitted in the same instant sits undrained while the published Error still reaches the Run at the checkpoint. | The Slot set is application-specific, so the per-Slot fan lives in one user sum; the Kavod-owned variants are exactly the ones the decision lists — no more. |
| W4 | Final `LiveCtx` signatures and construction | Adopt the provisional API block as final, unchanged. Construction (module-private, by the live builder at spawn time): `LiveCtx<C>` holds (1) a boxed offer closure `Box<dyn FnMut(C::Event) -> Result<(), OfferRejected<C::Event>> + Send>` created at registration — it captures the fan-in constructor and the shared central handle, so `LiveCtx` never needs the `Ev` type parameter; (2) an `Arc` of its own typed inbox; (3) an `Arc` of the lifecycle cell. | Erasing the *operation* (not the payload) keeps the public signature exactly as the design document prints it; the second planning session's probe (its P11) independently confirmed the same erasure works as a trait object, so the shape is doubly de-risked. |
| W5 | `LiveConfig` and the clock | `pub struct LiveConfig { pub shutdown_deadline_ms: NonZeroU64, pub fan_in_capacity: NonZeroUsize, pub time_origin: Timestamp }`. Time anchoring: `start` freezes one monotonic reading and the frozen start time is `time_origin`; every later stamp is `time_origin + elapsed-since-that-reading` in checked u64 nanoseconds — exhaustion is `LiveError::TimeExhausted` (`LIVE-TIME`). Per-inbox capacities are per-Slot, so they live on `slot` (W1); the fan-in queue is one shared queue, so its capacity lives here. The clock behind the readings is **public, documented API**: `pub trait MonotonicClock { fn now_nanos(&mut self) -> u64; }` with the shipped `StdClock` (anchored `Instant`) as the default; `build` uses `StdClock`, and a documented `build_with_clock(config, router, clock)` accepts any implementation. The trait's doc comment carries its one contract (readings never decrease). | One monotonic clock realizes `ENV-TIME` structurally; a caller wanting wall-clock anchoring mints `time_origin` with `Timestamp::from_nanos`. The clock seam is public rather than hidden because `VERIFY-LIVE` mandates injected-clock tests that live in `tests/` (per `test.md`), feature gates are ruled out by the layout, and `#[doc(hidden)]` was rejected on principle: hidden-but-`pub` code still ships and still binds — and a user testing a bespoke Live topology needs exactly the same determinism the shipped suite does, so the seam is genuinely useful API, not test residue. |
| W6 | `SimConfig` | `pub struct SimConfig { pub time_origin: Timestamp, pub step_budget: NonZeroU32 }`, passed to `SimWiring::build` — not part of `EngineConfig`. The design fixes only "nonzero"; `NonZeroU32` is chosen over `NonZeroUsize` because a per-call step budget beyond the u32 range is meaningless and the smaller domain documents intent (flippable at approval with no downstream effect). | `EngineConfig` is Core-owned and Environment-agnostic; the bounds registry places origin and step budget with the Simulated Environment. |
| W7 | What fixes the Slot order | Registration order — the builder's `slot` call sequence. Convention (documented on the builder): register in the Slot sum's declaration order; the two agreeing is part of the wiring author's `TRUST-ROUTING` obligation, checked by the per-Slot tests that obligation already carries. Declaration order — the decision text's one-authority candidate — cannot be the *programmatic* authority under W1/W2 as proposed: the macro's whole expansion is two enums and hand-written sums are first-class, so no runtime Slot list exists to read. Recorded for later, not for now: a thin sugar macro that expands a declaration-order Slot list into these same `slot` calls would close the order-drift gap with zero new runtime machinery and zero rework — it layers on top of this builder whenever it becomes worth having. | The builder call sequence is the one order the Environment can actually observe and freeze; the sugar-macro path keeps the doc's preferred candidate reachable without buying a macro-generated router today. |
| W8 | Public re-export policy at `lib.rs` | Flat: every public item re-exported at the crate root (`kavod::Engine`, `kavod::Journal`, `kavod::LivePort`, `kavod::MonotonicClock`, …); the module files themselves stay private (`mod time;`, not `pub mod`). `ports!` reaches the root via `#[macro_export]` and refers to `$crate::PortContract`. The `engine/mod.rs` re-export rule the design already fixes (`CRATE-EXPORTS`) extends to the whole crate. **No `#[doc(hidden)]` items exist anywhere**: everything public is documented, everything undocumented is private. | One path per item, no repeated segments anywhere, the macro's `$crate::` paths stay one segment deep, and the public surface is exactly the documented surface. |
| W9 | Thread naming | Yes: each supervised Port thread is named `kavod-<slot-name>` via `std::thread::Builder::name`, using the `&'static str` name from registration (the same name `LiveError` variants carry). Spawn failure is `LiveError::SpawnFailed { slot, source }`. | The name is free diagnosability in panics, debuggers, and `VERIFY-LIVE` failures, and registration already has the string for error payloads. |

### 1b. Genuine blockers

| # | Quoted text | Why code cannot proceed | Smallest resolution |
|---|---|---|---|
| — | *(none found)* | | |

The table is empty, and here is the defense — reached independently by both planning
passes, which strengthens it: every API block was walked as Rust (the risky interactions
compiled as probes P1–P8, section 2), every "unrepresentable" claim has a named Rust
mechanism (the checklist below), every always-on assertion site is enumerated (the
checklist in section 3), and every `VERIFY-*` bullet has a concrete harness (section 4
maps all eight rows). Every question that survived was either a §10 decision (1a — the
document itself declares them open) or a free implementation choice (1c). The two
candidates examined hardest both dissolved: the `VERIFY-LIVE` injected clock looked like
it conflicted with `test.md`'s "live lifecycle tests live in `tests/`" plus "no feature
gates" — resolved by making the clock public API, a §10-scoped choice (W5); and the
compile-fail fixture's `include!` reconstruction looked fragile until probe P6 built it
across two crates and produced exactly the failure class `VERIFY-GRAMMAR` demands.

### Unrepresentable-mechanism checklist

Every "unrepresentable" claim in the design, with the exact Rust mechanism that carries
it. A claim with no nameable mechanism would be a blocker; none is.

| Claim | Exact mechanism |
|---|---|
| Zero configured capacity | `NonZeroUsize` / `NonZeroU64` / `NonZeroU32` config fields |
| Empty Port set | Builder typestate: the zero-slot state has no `build` method (W1) |
| Mutable topology after construction | Private fields; no registration method exists on a built Environment |
| Handler receives another Kavod capability | Private `Context` construction; the handler signatures expose no other Kavod value |
| Absent direction carries a value | Uninhabited `Never` |
| Open Slot sums or wrong payload type | Concrete enums with projected Contract payloads; exhaustive match |
| Concurrent Engine calls into the Environment | Engine owns `E`; transitions receive one serial `&mut E` |
| Second shutdown | `shutdown(self)` consumes `E` |
| Second grammar over one Journal | The Initial certificate consumes and owns the only Journal |
| Duplicate/fabricated certificate | Private fields and types; no `Clone`/`Copy`/`Default`; consuming transitions |
| Out-of-order transition | Each private phase exposes only its legal consuming methods |
| Caller supplies acceptance index/time/Event | `run_started` has no arguments; `accept_event` takes only the Environment and derives every value |
| Accepted values differ from the record | One transition uses the same locals for payload and successor, updated only after commit |
| Record kind/payload mismatch | `Kind<P>` + `P::KIND`; no caller-supplied kind (F4) |
| Completion outcome disagrees with the answer | `classify` fixes the answer in the phase type; only marker-specific completion methods exist (F5) |
| Skipped checkpoint | Only `checkpoint` yields `Checkpointed<A>` |
| Independent/premature `CommandsDispatched` | Only `dispatch_batch` commits it, after draining every Command successfully |
| `TurnCompleted(Stop)` without a clean report | `close` consumes the Environment and constructs `Closed` only from `{ Quiesced, None }` |
| Commit after Fatal | The Error path consumes the certificate and, with it, the Journal |
| `Stopped` after unclean shutdown | Only `Closed` yields `Stopped`; only a clean report yields `Closed` |
| Port reaches or delegates the completion capability | The terminal guard lives only on the supervision shell's stack frame; neither the Port value nor `LiveCtx` ever holds it |

### 1c. Free choices

Choices the document leaves open, recorded so they are deliberate. None changes a
public API block except F8, which is folded into the W5 proposal; crate-internal
additions are mechanism under the reading rules.

| # | Choice | What was chosen and why |
|---|---|---|
| F1 | Crate-private constructors on Kavod-owned public types | `EventIndex`, `Timestamp`-adjacent internals, and `Context` get `pub(crate)` constructors/accessors (`Context::new`, `Context::overflowed`, an `EventIndex` minting fn). API blocks bind the public shape; the Run must be able to mint what it owns. |
| F2 | Extra derives | Add `Debug` wherever derivable (error enums, `EngineExit`, reports); add `Clone, Copy` to `TurnOutcome` (one value legally serves both the `TurnCompleted` payload and `JournalFatal.outcome`, per the enforcement prose); add `Debug, Clone, Copy, PartialEq, Eq` to `Never`. All free under "further derives are free"; nothing prohibits them (`RUN-GRAMMAR` prohibits only the certificate's `Clone`/`Copy`/`Default`). `JournalError` cannot be `PartialEq` (`std::io::Error` isn't); tests match on variants. |
| F3 | Encode buffer accepts partial writes | `BoundedBuffer<u8>`'s `Write::write` accepts up to remaining capacity and returns `Ok(n)`; only at zero remaining does it return `ErrorKind::WriteZero`. This makes "the encode region is exactly `max_record_bytes + 1` bytes" literally true — an encode completes iff it fits — so `JRN-ENCODE`'s size-boundary classifications (`NotAnObject` at exactly the region size, `BoundExceeded` for the object that leaves no newline room) hold by construction. Probe P4 proved `serde_json` drives the writer with retried partial writes. |
| F4 | Record kind markers are `Kind<Self>` | Each payload struct's first field is `Kind<Self>` (a ZST), and the payload itself implements `RecordPayload { const KIND: RecordKind }`. One shared hand-written `Serialize` on `Kind<P>` emits `P::KIND.tag()`; the module-private commit helper is generic over `P: RecordPayload + Serialize` and builds `JournalFatal` from the same `P::KIND` — tag, payload, and fatal kind have one source and a mismatch is unconstructible even in-module, as the enforcement section requires. Probe P3. |
| F5 | Phase refinement via a default type parameter | `TurnOpen<A = Unclassified>` with private marker types `Continue`/`Stop` (in a private module, so they never collide with `Outcome`'s variants); `classify` returns the `ClassifiedTurn` enum exactly as the enforcement table renders it. Probe P1. |
| F6 | Shared latch core in `latch.rs` | Both Environments need `ENV-LATCH`'s four-state machine (empty → pending → reported; → closed; first-wins; post-close discard). One crate-internal `Latch<E>` value type carries the state rules plus the observation-precedence helpers its owners share (pending beats a local failure; close into a report exactly once), unit-tested once; Sim uses it directly, Live wraps it under its central lock. The latch carries no synchronization of its own — the owner's lock acquisition is the linearization decision, which is why the same state machine serves both Environments. |
| F7 | Test-double placement: source-local plus `tests/support` — **no shipped test code** | Unit-level fixtures (memory sink, scripted-failure sink, scripted Environments, scripted clock) live inside the `#[cfg(test)] mod tests` of the file whose tests need them — code under `#[cfg(test)]` is never compiled into the shipped library at all. Cross-file suites get their own permanent doubles in `tests/support/` (wiring-only `mod.rs`, one subject per file). The rejected alternative was a `#[doc(hidden)] pub mod testkit`: it would let unit and integration tests share one fixture, but hidden-`pub` code ships, binds, and is API in everything but documentation. The accepted cost is a few tiny duplicated fixtures (a failing writer appears in `journal.rs` tests and again in `engine/record.rs` tests; a scripted clock in `live/env.rs` tests and again in `tests/support`) — each copy is small, and both copies are pinned by tests citing the same IDs, so drift fails loudly rather than silently. |
| F8 | Injected clock is public API | Folded into W5: `MonotonicClock` and `build_with_clock` are documented public items; the shipped `StdClock` anchors one `Instant`. Deadline arithmetic is u64 nanoseconds with `saturating_add` — "saturates at the latest representable monotonic instant" becomes `u64::MAX` on the clock's own axis. Probe P5. |
| F9 | Dev-dependencies | `trybuild` for the compile-fail suite (fetched when C35 starts) and `serde_json`'s `raw_value` feature (dev-only) to construct the interior-newline non-object `VERIFY-JOURNAL` demands — JSON permits a raw newline between tokens, so `RawValue` produces it without violating `TRUST-SERIALIZE`. Probe P4. |
| F10 | `record.rs` import discipline | From its first line, `engine/record.rs` imports only through `crate::journal::…`, `crate::time::…`, `crate::environment::…`, `crate::bounded_buffer::…`, `crate::application::…` — the exact path set the compile-fail fixture crate mirrors with `pub mod journal { pub use kavod::Journal; }`-style stubs, so `include!` reconstruction keeps working for the crate's whole life. Probe P6 proved the pattern across two crates. |
| F11 | Sim command delivery | The sim router's `hand_off` boxes the typed payload as `Box<dyn Any>` and the Slot's erased runtime downcasts it back, guarded by the typed `SlotHandle<C>` minted at registration; the downcast carries an always-on `expect` (a mismatch is a Kavod wiring bug — A8). Cost: sim payloads need `'static` (they are `Serialize` data; Live already demands `Send + 'static`). Chosen over per-Slot extractor closures, which would abandon the single compiler-checked exhaustive match `PORT-ROUTING` mandates. |
| F12 | Engine finalization shape | `RUN-FINALIZE`'s three arms become one private associated fn keyed by what the control flow already knows: transitions that borrow the Environment leave it with the Engine (arm: call `shutdown`, keep quiescence, discard the report's Error); `close` consumed it and its failure value carries the retained quiescence (arm: use it); the `start`-`Err` exit never reaches finalization's shutdown (arm: `Quiesced`). Probe P1's `finalize` shows the shape. |
| F13 | Existing `Cargo.toml` profile lines stay | `[profile.dev] panic = "abort"` and the release twin match `TRUST-ABORT` for binaries built here; Cargo ignores the `panic` setting for the `test` profile, so tests keep the unwinding the design's test-profile language relies on. The CI check `TRUST-ABORT` names is a deployment task, noted in C57. |
| F14 | `EventIndex`/`Timestamp` serialization via derive | `#[derive(Serialize)]` on a one-field newtype serializes as the bare inner value — exactly "transparent u64 JSON values". Hand-written impls would add surface for drift. Probes P3/P1 exercised the resulting bytes. |
| F15 | Scripted test Environments are ordinary trait impls | `VERIFY-CONFORMANCE`/`VERIFY-FAULTS` scripted Environments implement the public `Environment` trait from a step list and assert the call order against the graph as they go — they are the permanent "Environment-contract test doubles" phase, not scaffolding. |

---

## 2. Probe results

Eight probes, all **executed** (compiled and run) across the two planning sessions that
produced the source plans — P1–P7 in the first, P8 in the second (which ran three
further probes; one is preserved here as P8, and the other two corroborate W4 and W7's
sugar-macro note without adding mechanism this plan uses). The scratch directories the
probes ran in are ephemeral; the code preserved below is the surviving artifact and is
deliberately complete enough to re-run or copy from. No probe failed, so no probe
became a blocker.

| Probe | What it tested | Verdict |
|---|---|---|
| P1 | The certificate typestate end to end: `Certificate<W, P>` owning the Journal, `PhantomData<fn() -> P>`, consuming transitions that borrow the Environment, `classify` into answer-typed refinements, one marker-generic effects helper shared by both refinement arms, `run(self)` destructuring, the loop re-binding the certificate each turn, `close(env)` moving the Environment on the Stop path while Fatal paths keep it for finalization | Compiles and runs as designed; the borrow checker accepts the whole shape with no `Rc`, no cloning, no restructuring |
| P2 | `ports!` as `macro_rules!` with the document's exact invocation grammar (`<Event = …, Command = …>`), `::serde` derive paths, a Contract reused by two Slots, `Never` with `match *self {}` `Serialize`, externally tagged bytes, hand-written equivalence | Works; generated and hand-written sums are byte-identical; `Never` arm discharges by matching |
| P3 | The kind marker: `Kind<Self>` ZST first field, shared hand-written `Serialize` from `RecordPayload::KIND`, the lifetime+generic borrowed payload (`EventAccepted`), `TurnOutcome` as bare tag, the document's golden first line byte-exact | Compiles including the self-referential `Kind<Self>` under generics and lifetimes; output matches `{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}` exactly |
| P4 | The bounded encode buffer as `std::io::Write` under `serde_json::to_writer`: partial writes retried (write_all semantics), zero-progress `WriteZero` classified back through `serde_json::Error::io_error_kind()`, completion at exactly the region size, `RawValue` interior newline as valid JSON | All hold; the `JRN-ENCODE` classification triad (`Encode`/`NotAnObject`/`BoundExceeded`) is implementable exactly as written |
| P5 | The Live concurrency skeleton: `Mutex`+`Condvar` start/cancel gate (no shell runs while pending, all observe the signal), first-wins latch under a lock, u64-nanosecond deadline arithmetic saturating at `u64::MAX` with non-underflowing remaining-time | Works; this is the seed of `LIVE-START`/`LIVE-SHUTDOWN`'s one-lock realization |
| P6 | The `VERIFY-GRAMMAR` fixture: a second crate `include!`s the real `engine/record.rs`, mirrors its `crate::` imports with re-export stubs, and attacks from inside the reconstructed engine module | Legal use compiles; `cert.clone()` fails with `E0599: no method named clone` — the grammar restriction — and not a privacy error. (The probe also demonstrated the failure mode the fixture exists to avoid: the same attack placed *outside* the module dies on `E0603` privacy first.) |
| P7 | `Context<'a, C>` over the Engine's reusable buffer: construction clears the buffer, the marker lives in the Context, the Engine reads the marker after the handler and reuses the allocation next turn | Works; borrow ends when the Context drops, no lifetime friction |
| P8 | Publication-before-Complete and the shutdown final observation as one critical section: a worker publishes its Error and marks Complete inside one lock hold; the final observer scans completion, closes the latch, and extracts the report inside one lock hold | The final observer cannot count Complete while missing the earlier Error — the one-lock discipline decides every race, by construction |

### P1 — certificate typestate, transitions, and the Engine loop (seed of `engine/record.rs` + `engine/engine.rs`)

```rust
use std::marker::PhantomData;

// Stand-ins mirror the design doc's API blocks: a Journal that commits one
// line, and the Environment trait (u64 for Timestamp, (bool, Option<E>) for
// the ShutdownReport). The real chunks use the real types.
pub struct Journal<W: std::io::Write> { w: W }
impl<W: std::io::Write> Journal<W> {
    fn commit(&mut self, line: &[u8]) -> Result<(), ()> {
        self.w.write_all(line).and_then(|_| self.w.flush()).map_err(|_| ())
    }
}
pub trait Environment {
    type Event; type Command; type Error;
    fn start(&mut self) -> Result<u64, Self::Error>;
    fn next_event(&mut self) -> Result<(Self::Event, u64), Self::Error>;
    fn dispatch(&mut self, c: Self::Command) -> Result<(), Self::Error>;
    fn take_error(&mut self) -> Option<Self::Error>;
    fn shutdown(self) -> (bool, Option<Self::Error>);
}
pub struct CommandBuffer<C> { items: Vec<C> }

// Phases; `A` is the answer marker fixed by `classify`.
pub struct Unclassified;
pub struct ContinueA;
pub struct StopA;
pub struct Initial;
pub struct TurnOpen<A = Unclassified>(PhantomData<fn() -> A>);
pub struct EffectsComplete<A>(PhantomData<fn() -> A>);
pub struct Checkpointed<A>(PhantomData<fn() -> A>);
pub struct BetweenTurns;
pub struct StopPending;
pub struct Closed;

pub struct Certificate<W: std::io::Write, P> {
    journal: Journal<W>,
    index: u64,
    last_time: u64,
    _phase: PhantomData<fn() -> P>,
}

pub enum ClassifiedTurn<W: std::io::Write> {
    Continue(Certificate<W, TurnOpen<ContinueA>>),
    Stop(Certificate<W, TurnOpen<StopA>>),
}
pub enum Answer { Continue, Stop }

// Fatal stand-in: cause plus the quiescence `close` retained, if it ran.
pub struct Fatal { pub cause: &'static str, pub retained_quiescence: Option<bool> }

impl<W: std::io::Write, P> Certificate<W, P> {
    fn advance<Q>(self) -> Certificate<W, Q> {
        Certificate { journal: self.journal, index: self.index, last_time: self.last_time, _phase: PhantomData }
    }
}

impl<W: std::io::Write> Certificate<W, Initial> {
    pub fn mint(journal: Journal<W>, start_time: u64) -> Self {
        let cert = Certificate { journal, index: 0, last_time: start_time, _phase: PhantomData };
        assert_eq!(cert.index, 0); // RUN-ENFORCEMENT: induction base, always-on
        cert
    }
    pub fn run_started(mut self) -> Result<Certificate<W, TurnOpen>, Fatal> {
        // Reads prospective index/time from itself; no caller supplies either.
        let line = format!("{{\"record_kind\":\"RunStarted\",\"index\":{},\"schema_version\":1,\"logical_time\":{}}}\n",
            self.index, self.last_time);
        self.journal.commit(line.as_bytes())
            .map_err(|_| Fatal { cause: "Journal(RunStarted)", retained_quiescence: None })?;
        Ok(self.advance())
    }
}

impl<W: std::io::Write> Certificate<W, TurnOpen> {
    pub fn classify(self, answer: Answer) -> ClassifiedTurn<W> {
        match answer {
            Answer::Continue => ClassifiedTurn::Continue(self.advance()),
            Answer::Stop => ClassifiedTurn::Stop(self.advance()),
        }
    }
}

impl<W: std::io::Write, A> Certificate<W, TurnOpen<A>> {
    pub fn no_commands<C>(self, buf: &CommandBuffer<C>) -> Certificate<W, EffectsComplete<A>> {
        assert!(buf.items.is_empty()); // ASSERT-INVARIANTS: recordless edge bypasses nothing
        self.advance()
    }
    pub fn dispatch_batch<C, E>(mut self, env: &mut E, buf: &mut CommandBuffer<C>)
        -> Result<Certificate<W, EffectsComplete<A>>, Fatal>
    where E: Environment<Command = C> {
        assert!(!buf.items.is_empty()); // ASSERT-INVARIANTS
        self.journal.commit(b"{\"record_kind\":\"CommandsPrepared\"}\n")
            .map_err(|_| Fatal { cause: "Journal(CommandsPrepared)", retained_quiescence: None })?;
        for (position, c) in buf.items.drain(..).enumerate() {
            if env.dispatch(c).is_err() {
                let _ = position; // the real thing carries Dispatch { position }
                return Err(Fatal { cause: "Environment(Dispatch)", retained_quiescence: None });
            }
        }
        self.journal.commit(b"{\"record_kind\":\"CommandsDispatched\"}\n")
            .map_err(|_| Fatal { cause: "Journal(CommandsDispatched)", retained_quiescence: None })?;
        Ok(self.advance())
    }
}

impl<W: std::io::Write, A> Certificate<W, EffectsComplete<A>> {
    pub fn checkpoint<E: Environment>(self, env: &mut E) -> Result<Certificate<W, Checkpointed<A>>, Fatal> {
        match env.take_error() {
            Some(_) => Err(Fatal { cause: "Environment(Checkpoint)", retained_quiescence: None }),
            None => Ok(self.advance()),
        }
    }
}

impl<W: std::io::Write> Certificate<W, Checkpointed<ContinueA>> {
    pub fn complete_continue(mut self) -> Result<Certificate<W, BetweenTurns>, Fatal> {
        self.journal.commit(b"{\"record_kind\":\"TurnCompleted\",\"outcome\":\"Continue\"}\n")
            .map_err(|_| Fatal { cause: "Journal(TurnCompleted)", retained_quiescence: None })?;
        Ok(self.advance())
    }
}

impl<W: std::io::Write> Certificate<W, Checkpointed<StopA>> {
    pub fn request_stop(mut self) -> Result<Certificate<W, StopPending>, Fatal> {
        self.journal.commit(b"{\"record_kind\":\"StopRequested\"}\n")
            .map_err(|_| Fatal { cause: "Journal(StopRequested)", retained_quiescence: None })?;
        Ok(self.advance())
    }
}

impl<W: std::io::Write> Certificate<W, StopPending> {
    pub fn close<E: Environment>(mut self, env: E) -> Result<Certificate<W, Closed>, Fatal> {
        let (quiesced, error) = env.shutdown(); // consumes the Environment
        let retained = Some(quiesced);          // retained before inspecting the Error
        if error.is_some() {
            return Err(Fatal { cause: "Environment(Shutdown)", retained_quiescence: retained });
        }
        if !quiesced {
            return Err(Fatal { cause: "Core(ShutdownIncomplete)", retained_quiescence: retained });
        }
        self.journal.commit(b"{\"record_kind\":\"TurnCompleted\",\"outcome\":\"Stop\"}\n")
            .map_err(|_| Fatal { cause: "Journal(TurnCompleted)", retained_quiescence: retained })?;
        Ok(self.advance())
    }
}

impl<W: std::io::Write> Certificate<W, BetweenTurns> {
    pub fn accept_event<E: Environment>(mut self, env: &mut E)
        -> Result<(Certificate<W, TurnOpen>, E::Event), Fatal> {
        if self.index == u64::MAX {
            return Err(Fatal { cause: "Core(IndexExhausted)", retained_quiescence: None });
        }
        let (event, time) = env.next_event()
            .map_err(|_| Fatal { cause: "Environment(NextEvent)", retained_quiescence: None })?;
        if time < self.last_time {
            return Err(Fatal { cause: "Core(TimeRegression)", retained_quiescence: None });
        }
        let next_index = self.index.checked_add(1).expect("RUN-INDEX: overflow past domain check");
        self.journal.commit(b"{\"record_kind\":\"EventAccepted\"}\n")
            .map_err(|_| Fatal { cause: "Journal(EventAccepted)", retained_quiescence: None })?;
        self.index = next_index;
        self.last_time = time;
        Ok((self.advance(), event))
    }
}

// --- the Engine loop shape --------------------------------------------------

pub struct Engine<A, E, W> { app: A, env: E, writer: W }
pub enum Exit<S, EE> {
    Stopped { state: S },
    Fatal { state: S, cause: &'static str, quiesced: bool, _e: PhantomData<EE> },
}
pub trait App {
    type State; type Event; type Command;
    fn initial_state(&self) -> Self::State;
    fn on_start(&self, state: &mut Self::State, buf: &mut CommandBuffer<Self::Command>) -> Answer;
    fn on_event(&self, state: &mut Self::State, ev: &Self::Event, buf: &mut CommandBuffer<Self::Command>) -> Answer;
}

// The marker-generic effects helper both classify arms share: one body.
fn effects<W: std::io::Write, A, E, C>(
    cert: Certificate<W, TurnOpen<A>>, env: &mut E, buf: &mut CommandBuffer<C>,
) -> Result<Certificate<W, Checkpointed<A>>, Fatal>
where E: Environment<Command = C> {
    let cert = if buf.items.is_empty() { cert.no_commands(buf) } else { cert.dispatch_batch(env, buf)? };
    cert.checkpoint(env)
}

impl<A, E, W> Engine<A, E, W>
where
    A: App,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: std::io::Write,
{
    pub fn run(self) -> Exit<A::State, E::Error> {
        let Engine { app, mut env, writer } = self; // run(self) destructuring
        let mut buf = CommandBuffer { items: Vec::new() };
        let mut state = app.initial_state(); // before any fallible step
        let start_time = match env.start() {
            Ok(t) => t,
            Err(_) => return Exit::Fatal { state, cause: "Environment(Start)", quiesced: true, _e: PhantomData },
        };
        let cert = Certificate::mint(Journal { w: writer }, start_time);
        let mut cert: Certificate<W, TurnOpen> = match cert.run_started() {
            Ok(c) => c,
            Err(f) => return Self::finalize(state, f, Some(env)),
        };
        let mut pending_event: Option<A::Event> = None;
        loop {
            buf.items.clear();
            let answer = match pending_event.take() {
                None => app.on_start(&mut state, &mut buf),
                Some(ev) => app.on_event(&mut state, &ev, &mut buf),
            };
            match cert.classify(answer) {
                ClassifiedTurn::Continue(c) => match effects(c, &mut env, &mut buf) {
                    Ok(cp) => match cp.complete_continue() {
                        Ok(between) => match between.accept_event(&mut env) {
                            Ok((next, ev)) => { pending_event = Some(ev); cert = next; }
                            Err(f) => return Self::finalize(state, f, Some(env)),
                        },
                        Err(f) => return Self::finalize(state, f, Some(env)),
                    },
                    Err(f) => return Self::finalize(state, f, Some(env)),
                },
                ClassifiedTurn::Stop(c) => match effects(c, &mut env, &mut buf) {
                    Ok(cp) => match cp.request_stop() {
                        Ok(stop_pending) => match stop_pending.close(env) {
                            // env moved out; the Stop arm never touches it again
                            Ok(_closed) => return Exit::Stopped { state },
                            Err(f) => return Self::finalize(state, f, None),
                        },
                        Err(f) => return Self::finalize(state, f, Some(env)),
                    },
                    Err(f) => return Self::finalize(state, f, Some(env)),
                },
            }
        }
    }

    // RUN-FINALIZE's three arms, keyed by retained quiescence and Environment
    // ownership (F12).
    fn finalize(state: A::State, f: Fatal, env: Option<E>) -> Exit<A::State, E::Error> {
        let quiesced = match (f.retained_quiescence, env) {
            (Some(q), None) => q,                  // StopPending ran: use retained report
            (None, Some(env)) => env.shutdown().0, // unconsumed: finalizing shutdown, Error discarded
            (None, None) => true,                  // start-Err path: ENV-START gives Quiesced
            (Some(_), Some(_)) => unreachable!(),
        };
        Exit::Fatal { state, cause: f.cause, quiesced, _e: PhantomData }
    }
}
```

The probe's test (`stop_path_compiles_and_runs`) drives a Stop-at-start Application
through a null Environment to `Exit::Stopped` with the mutated state — the loop shape,
the moves, and the borrows all clear the compiler untouched.

### P2 — `ports!` as `macro_rules!` (seed of `port.rs`)

```rust
use serde::Serialize;

pub trait PortContract { type Event: Serialize; type Command: Serialize; }

pub enum Never {}
impl Serialize for Never {
    fn serialize<S: serde::Serializer>(&self, _s: S) -> Result<S::Ok, S::Error> {
        match *self {}
    }
}

#[macro_export]
macro_rules! ports {
    (
        $vis:vis enum $name:ident<Event = $event:ident, Command = $command:ident> {
            $( $variant:ident($contract:ty) ),+ $(,)?
        }
    ) => {
        #[derive(::serde::Serialize)]
        $vis enum $event {
            $( $variant(<$contract as $crate::PortContract>::Event) ),+
        }
        #[derive(::serde::Serialize)]
        $vis enum $command {
            $( $variant(<$contract as $crate::PortContract>::Command) ),+
        }
    };
}
```

Probe tests confirmed: the document's exact invocation shape parses (the `Trading`
name is consumed and discarded — no item of that name is generated); a Contract bound
at two Slots yields two distinct variants; serialized bytes are serde's externally
tagged form (`{"Secondary":{"px":42}}`); a hand-written sum is byte-identical; a
`Never`-typed Command arm is discharged with `match never {}`.

### P3 — kind marker with one source (seed of `engine/record.rs` payloads)

```rust
use serde::{Serialize, Serializer};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind { RunStarted, EventAccepted, CommandsPrepared, CommandsDispatched, StopRequested, TurnCompleted }
impl RecordKind {
    pub const fn tag(self) -> &'static str {
        match self {
            RecordKind::RunStarted => "RunStarted",
            RecordKind::EventAccepted => "EventAccepted",
            RecordKind::CommandsPrepared => "CommandsPrepared",
            RecordKind::CommandsDispatched => "CommandsDispatched",
            RecordKind::StopRequested => "StopRequested",
            RecordKind::TurnCompleted => "TurnCompleted",
        }
    }
}

pub trait RecordPayload { const KIND: RecordKind; }

/// Kind-typed zero-sized first field; `fn() -> P` keeps auto-traits clean.
pub struct Kind<P>(PhantomData<fn() -> P>);
impl<P> Kind<P> { pub const fn new() -> Self { Kind(PhantomData) } }
impl<P: RecordPayload> Serialize for Kind<P> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(P::KIND.tag())
    }
}

#[derive(Serialize)]
pub struct RunStartedRecord {
    pub record_kind: Kind<Self>,
    pub index: u64,          // EventIndex in the real thing
    pub schema_version: u32,
    pub logical_time: u64,   // Timestamp in the real thing
}
impl RecordPayload for RunStartedRecord { const KIND: RecordKind = RecordKind::RunStarted; }

#[derive(Serialize)]
pub struct EventAcceptedRecord<'a, Ev> {
    pub record_kind: Kind<Self>,
    pub index: u64,
    pub logical_time: u64,
    pub event: &'a Ev,
}
impl<'a, Ev> RecordPayload for EventAcceptedRecord<'a, Ev> { const KIND: RecordKind = RecordKind::EventAccepted; }
```

Probe tests confirmed: `serde_json::to_string(&RunStartedRecord { … })` equals the
document's example line byte-for-byte; the lifetime+generic borrowed payload compiles
(serde's derive adds the `Ev: Serialize` bound itself) and serializes with its tag
first; `TurnOutcome` with `Serialize` emits the bare tag `"Stop"`, and with `Copy`
added (F2) one value legally serves both the payload and `JournalFatal.outcome`.

### P4 — bounded encode buffer under `serde_json` (seed of `bounded_buffer.rs`)

```rust
use std::io::{ErrorKind, Write};

pub struct BoundedBuf { buf: Vec<u8>, cap: usize }
impl BoundedBuf {
    pub fn new(cap: usize) -> Self {
        let mut buf = Vec::new();
        buf.try_reserve_exact(cap).unwrap(); // JournalBuildError::AllocationFailed in the real thing
        BoundedBuf { buf, cap }
    }
}
impl Write for BoundedBuf {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let remaining = self.cap - self.buf.len();
        if remaining == 0 { return Err(ErrorKind::WriteZero.into()); } // zero-progress rejection
        let n = remaining.min(data.len());                            // partial accept (F3)
        self.buf.extend_from_slice(&data[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
```

Probe tests confirmed, each load-bearing for `JRN-ENCODE`:
- an oversized record surfaces as `serde_json::Error` with `io_error_kind() == Some(WriteZero)` — the `BoundExceeded` classification hook exists and fires;
- an encode of exactly the region size completes (partial writes make the region boundary exact);
- `serde_json` retries partial writes (a one-byte-per-call writer still yields the full correct encoding — `write_all` semantics), so F3 loses no bytes;
- `RawValue::from_string("{\"a\":\n1}")` is accepted (valid JSON) and passes through with the raw interior newline — the `NotAnObject` test vector `VERIFY-JOURNAL` requires is constructible;
- a non-object top level (`42`) is classifiable by the first/last-byte + no-newline rule.

### P5 — gate, latch, deadline (seed of the Live concurrency core)

```rust
use std::sync::{Condvar, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate { Pending, Start, Cancel }

pub struct Shared {
    pub gate: Mutex<Gate>,
    pub cv: Condvar,
    pub latch: Mutex<Option<&'static str>>, // first publication wins
}

pub fn wait_at_gate(sh: &Shared) -> Gate {
    let mut g = sh.gate.lock().unwrap();
    while *g == Gate::Pending { g = sh.cv.wait(g).unwrap(); }
    *g
}

pub fn deadline(raise_nanos: u64, configured_nanos: u64) -> u64 {
    raise_nanos.saturating_add(configured_nanos) // saturates at the latest representable instant
}
pub fn remaining(deadline_nanos: u64, now_nanos: u64) -> std::time::Duration {
    std::time::Duration::from_nanos(deadline_nanos.saturating_sub(now_nanos))
}
```

Probe tests confirmed: three spawned shells make no progress while the gate is
`Pending`, all run after `Start` with `notify_all`, and the latch keeps exactly the
first publication; deadline math saturates at `u64::MAX` and remaining time never
underflows.

### P6 — the `include!` compile-fail fixture (seed of the `VERIFY-GRAMMAR` crate)

Target side: `engine/record.rs` is an ordinary module. Fixture side (a separate test
crate depending on `kavod`):

```rust
// fixture/src/lib.rs — mirrors exactly the paths record.rs imports via `crate::` (F10)
pub mod journal { pub use kavod::Journal; /* … */ }
pub mod engine {
    pub mod record {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/engine/record.rs"));
    }
    // Attack position: inside the reconstructed engine module, where
    // pub(super) items are visible — privacy is out of the way, so the
    // failure that remains is the grammar restriction itself.
}
```

Executed across two crates: the legal transition sequence compiles from the attack
position; `cert.clone()` fails with `E0599` (no `Clone`), not `E0603` (privacy) — the
exact separation `VERIFY-GRAMMAR` demands. The probe also showed the trap: the same
attack at crate root fails on privacy first, which is why every trybuild case must sit
inside the reconstructed module.

### P7 — `Context` over the reusable buffer (seed of `application.rs`)

```rust
pub struct Context<'a, C> {
    buffer: &'a mut Vec<C>, // BoundedBuffer<C> in the real thing
    capacity: usize,
    overflowed: bool,
    index: u64,          // EventIndex in the real thing
    logical_time: u64,   // Timestamp in the real thing
}
impl<'a, C> Context<'a, C> {
    pub(crate) fn new(buffer: &'a mut Vec<C>, capacity: usize, index: u64, logical_time: u64) -> Self {
        buffer.clear(); // fresh handler invocation: empty buffer, clear marker (APP-OVERFLOW)
        Context { buffer, capacity, overflowed: false, index, logical_time }
    }
    pub fn remaining(&self) -> usize {
        if self.overflowed { 0 } else { self.capacity - self.buffer.len() }
    }
    pub fn emit(&mut self, command: C) {
        if self.overflowed { return; }
        if self.buffer.len() == self.capacity { self.overflowed = true; return; }
        self.buffer.push(command);
    }
    pub(crate) fn overflowed(&self) -> bool { self.overflowed }
}
```

Probe test confirmed: the Engine constructs a Context per handler call (construction
clears the buffer), reads the marker after the handler, the borrow ends at drop, and
the same allocation serves the next turn.

### P8 — publication-before-Complete and final close under one lock (seed of the Live shutdown observation)

```rust
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

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

Executed: because the worker publishes and completes inside one lock hold, and the
final observer scans and closes inside one lock hold, the observer can never count
`Complete` while missing the Error published before it. This is the discipline C48 and
C51 implement.

---

## 3. Chunked build plan

### Ground rules (restated once, applying to every chunk)

- **Forward-only.** Every chunk's code is final. Later chunks *extend* files (a new
  item, a new test module, a new `mod`+`pub use` line in `lib.rs`); every such touch to
  an earlier chunk's file is named in the later chunk's **Builds** line. Nothing is ever
  rewritten or deleted.
- **Done-when.** A chunk is done when `cargo test` and `cargo clippy` are green and the
  chunk's **Proves** tests pass by name. Tests follow `design_docs/test.md` exactly:
  unit tests in `#[cfg(test)] mod tests` in the file they test, one nested module per
  subject-and-behavior named `<subject>_<behavior>`, every test doc-commented with the
  invariant it verifies cited by ID (by name where no ID exists), test names stating the
  observable behavior. Cross-file suites live in `tests/`. Concretely, every listed test
  is written in this shape from its first commit:

  ```rust
  mod subject_behavior {
      use super::*;

      /// Invariant: ID — the specific observable statement the test pins.
      #[test]
      fn observable_behavior() { /* … */ }
  }
  ```

- **Standing lib.rs touch.** Each chunk that creates a source file adds its `mod` line
  and root `pub use` lines (W8) to `lib.rs`; this recurring one-line touch is not
  repeated in each **Builds** entry.
- **§10-gated** marks a chunk that starts only after the 1a proposals are approved (or
  amended). Everything unmarked can proceed immediately, in order.

### Always-on assertion checklist (`ASSERT-INVARIANTS`)

Every asserted invariant with its owning guarantee and named site; each lands in the
chunk shown. All are always-on (`assert!`/`assert_eq!`/`expect`, never
`debug_assert!`) and constant-time.

| Site | Assertion | Owner | Chunk |
|---|---|---|---|
| `Journal::commit` | not poisoned — a poisoned commit is a precondition violation and panics (A8) | `JRN-POISON` | C8 |
| `Certificate::mint` | `Initial` certificate stores prospective index 0 (induction base) | `RUN-ENFORCEMENT` | C18 |
| `no_commands` | the bypassed buffer is empty | `RUN-ENFORCEMENT` | C19 |
| `dispatch_batch` | the buffer is nonempty | `RUN-ENFORCEMENT` | C20 |
| `accept_event` | index increment past the domain check cannot overflow (`checked_add(1).expect`) | `RUN-INDEX` | C22 |
| Sim delivery | the `SlotHandle`-guarded downcast succeeds (`expect`) | `PORT-ROUTING` realization (F11) | C38 |
| Sim selection | the selected Port is `Open` | `SIM-SELECT` / `SIM-LIFECYCLE` | C39 |
| Live wake tokens | at most one wake token per bound Slot outstanding | `LIVE-COMPLETION` (A6 derivative) | C46 |
| Live completion entry | a completing shell's entry was `Outstanding` (exactly-once transition) | `LIVE-COMPLETION` | C48 |

Further always-on assertions are welcome under the house preference wherever they stay
constant-time and name their owning guarantee in the panic message.

### Phase overview

The document's section order is dependency order and doubles as the Rust learning ramp:

| Phase | Chunks | Theme | New Rust ground |
|---|---|---|---|
| 0 Foundations | C1–C3 | crate skeleton, time types, bounded storage | newtypes, checked arithmetic, `try_reserve`, implementing `std::io::Write` |
| 1 Journal | C4–C9 | the bounded JSONL writer | generic structs, error enums, `serde_json` at the boundary, `#[should_panic]` |
| 2 Application | C10 | `Context`, `Outcome`, the trait | lifetimes on structs, `pub(crate)` capability surface |
| 3 Port | C11–C12 | `PortContract`, `Never`, `ports!` | `macro_rules!`, uninhabited types, manual `Serialize` |
| 4 Environment + latch | C13–C14 | the contract trait, the shared latch core | associated types, consuming `self`, state-machine enums |
| 5 Record grammar | C15–C23 | records, certificate, transitions | typestate with `PhantomData`, affine consumption, module privacy |
| 6 Engine | C24–C28 | construction and the driver loop | destructuring `self`, moves out of branches, generic helpers |
| 7 Core suites | C29–C35 | harness, golden, faults, conformance, compile-fail | integration-test layout, trybuild, `include!` fixture |
| 8 Sim | C36–C43 | the single-threaded Environment | trait objects, `Box<dyn Any>`, builder typestate |
| 9 Live | C44–C56 | threads, gate, latch, shutdown | `Arc`, `Mutex`/`Condvar`, `JoinHandle`, `Drop` guards, injected clocks |
| 10 Close | C57 | export audit, docs, CI notes | rustdoc, public-API hygiene |

---

### Phase 0 — Foundations

**C1 — crate skeleton and the time types**
- **Builds:** fresh `src/` (`lib.rs` with `#![forbid(unsafe_code)]`, `time.rs`);
  `Cargo.toml` keeps its deps and profile lines (F13), gains `[dev-dependencies]
  serde_json = { version = "1", features = ["raw_value"] }` (F9). `EventIndex` and
  `Timestamp` exactly per the API block, `Serialize` by newtype derive (F14), plus
  `pub(crate)` minting fns (F1).
- **Discharges:** `NO-UNSAFE`; the `EventIndex`/`Timestamp` API block; A6 for time
  arithmetic (`checked_add` with checked `Duration` conversion).
- **Proves:** `mod timestamp_arithmetic`: `overflowing_sum_returns_none`,
  `oversized_duration_returns_none` (A6), `equal_timestamp_is_valid` (`ENV-TIME`);
  `mod index_and_time_wire`: `both_serialize_as_transparent_u64` (by name: the API
  block's doc comment).
- **Rust notes:** newtype pattern; derives on private fields. Tricky part:
  `Duration::as_nanos()` is `u128` — convert with `u64::try_from` first, then
  `checked_add`; never truncate.
- **Size:** ~120 lines.

**C2 — bounded storage, part 1**
- **Builds:** `bounded_buffer.rs`: `BoundedBuffer<T>` — `try_reserve_exact`
  construction returning `Result<Self, TryReserveError>`, `try_push` (returns the value
  back on refusal), `len`/`capacity`/`is_empty`/`as_slice`/`clear`/`drain`. Fixed
  logical capacity forever; no growth path exists.
- **Discharges:** A6 for the two containers this type will back (Command batch, encode
  region); the crate-layout row naming `bounded_buffer.rs`.
- **Proves:** `mod bounded_buffer_capacity`: `push_beyond_capacity_is_refused_without_growth`
  (A6), `construction_failure_reports_the_reservation_error` (by name:
  `BoundedBuffer` construction, A6); `mod bounded_buffer_reuse`:
  `clear_and_drain_retain_capacity` (A6).
- **Rust notes:** `Vec::try_reserve_exact`; logical capacity is separate from
  `Vec::capacity` — success proves at least the requested storage exists, and every
  later write enforces the logical value. `drain(..)` returns owned items — the batch
  handoff loop will consume Commands by value through exactly this.
- **Size:** ~130 lines.

**C3 — bounded storage as a sink target**
- **Builds:** grows `bounded_buffer.rs`: `impl std::io::Write for BoundedBuffer<u8>`
  with partial accepts and zero-progress `WriteZero` (F3, probe P4 verbatim).
- **Discharges:** the encode-region behavior `JRN-ENCODE` presupposes.
- **Proves:** `mod encode_buffer_write`: `full_buffer_returns_write_zero`
  (`JRN-ENCODE`), `partial_writes_accumulate_without_loss` (`JRN-ENCODE`),
  `serde_json_encode_completes_at_exact_region_size` (`JRN-ENCODE`).
- **Rust notes:** the `io::Write` contract (`Ok(n)` may be short; `write_all` loops);
  `ErrorKind::WriteZero.into()`.
- **Size:** ~90 lines.

### Phase 1 — Journal

The Journal lands as four final private pieces composed by `commit` — each helper is
final code from its first commit, testable alone through the file's own test module,
never a placeholder.

**C4 — Journal types and construction**
- **Builds:** `journal.rs`: `JournalBuildError`, `JournalError`, `SinkOperation`,
  `Journal<W>` struct (writer + `BoundedBuffer<u8>` encode region + poison flag),
  `Journal::new` (checked `max_record_bytes + 1`, reservation), `is_poisoned`.
- **Discharges:** the Journal API block's construction half; `JRN-ENCODE`'s
  `checked_add` rule; `BOUND-NONZERO` for `max_record_bytes` (`NonZeroUsize`).
- **Proves:** `mod journal_construction`: `region_size_overflow_is_max_bytes_too_large`
  (`JRN-ENCODE`), `failed_reservation_is_allocation_failed` (by name:
  `JournalBuildError`), `fresh_journal_is_not_poisoned` (`JRN-POISON`).
- **Rust notes:** generic struct over `W: std::io::Write`; `NonZeroUsize::get`;
  preserve `TryReserveError` by value. The maximum-capacity test triggers capacity
  overflow, never a real giant allocation.
- **Size:** ~110 lines.

**C5 — bounded encoding**
- **Builds:** grows `journal.rs`: private final `fn encode_raw<R: Serialize>` — clear
  the region, `serde_json::to_writer` into it, map the buffer's `WriteZero` (via
  `serde_json::Error::io_error_kind()`) to `BoundExceeded` and every other serde
  failure to `Encode`. Touches no sink. A minimal deliberately-failing `Serialize`
  fixture and a counting-writer fixture land in the file's test module (F7).
- **Discharges:** the encode-before-sink and encode-error halves of `JRN-ENCODE`.
- **Proves:** `mod journal_encoding`: `oversized_record_is_bound_exceeded_without_sink_calls`
  (`JRN-ENCODE`), `serializer_failure_is_encode_without_sink_calls` (`JRN-ENCODE`),
  `encode_failures_do_not_poison` (`JRN-ENCODE`).
- **Rust notes:** `serde_json::Error::io_error_kind()`; a failing `Serialize` impl is
  ~5 lines. Encoding may legally fill `max + 1`; classification comes next chunk.
- **Size:** ~120 lines.

**C6 — classification and the newline**
- **Builds:** grows `journal.rs`: private final `fn encode_line` — call C5, classify
  (`{` first, `}` last, no interior newline → else `NotAnObject`), append the newline
  (no room → `BoundExceeded`). Uses the `RawValue` interior-newline vector (F9).
- **Discharges:** `JRN-FORMAT` (encoding half); `JRN-ENCODE` complete.
- **Proves:** `mod journal_object_validation`:
  `object_plus_newline_is_the_encoded_line` (`JRN-FORMAT`),
  `interior_newline_is_not_an_object` (`JRN-ENCODE`; `RawValue` vector),
  `non_object_top_level_is_rejected` (`JRN-ENCODE`);
  `mod journal_newline_reservation`:
  `object_of_region_size_has_no_newline_room` (`JRN-ENCODE`),
  `encode_at_exactly_max_bytes_completes` (`JRN-ENCODE`).
- **Rust notes:** classification order is observable — non-object first, then newline
  room; ordinary string newlines are escaped by serde and are *not* this test case.
  Tricky part: the exact boundary cases at `max_record_bytes` and
  `max_record_bytes + 1` — write them straight from the guarantee row's sentences.
- **Size:** ~120 lines.

**C7 — the sink write loop**
- **Builds:** grows `journal.rs`: private final `fn write_line` — the bounded write
  loop retrying only short successful writes; classify `Err` (including
  `Interrupted`), `Ok(0)`, and over-reported counts; poison before returning the typed
  write error. The scripted sink fixture (per-call scripted write/flush results + call
  log) lands in the file's test module (F7).
- **Discharges:** the write side of `JRN-POISON`; `BOUND-LOOPS` for the write loop;
  the sink-call side of `JRN-SINK`.
- **Proves:** `mod journal_sink_writes`:
  `short_successful_writes_are_retried_to_completion` (`JRN-POISON`),
  `interrupted_write_poisons_without_retry` (`JRN-POISON`),
  `zero_progress_maps_to_write_zero` (`JRN-POISON`),
  `over_reported_count_maps_to_invalid_data` (`JRN-POISON`).
- **Rust notes:** the loop's bound is progress against record length; increment the
  offset only after validating `count <= remaining`. `Interrupted` is *not* retried
  here — the design's Justify note explains why; don't "fix" it.
- **Size:** ~130 lines.

**C8 — commit and the poison precondition**
- **Builds:** grows `journal.rs`: `commit` composing C6 and C7 — assert not poisoned
  (always-on), encode/classify, write, flush; poison on flush failure; commit only on
  successful flush. This is the first `commit` and the final behavior touch to the
  Journal.
- **Discharges:** `JRN-COMMIT`; `JRN-POISON` complete; `JRN-SINK` (the Journal's
  side); the Journal assertion site.
- **Proves:** `mod journal_commit`: `successful_flush_commits_exactly_the_line`
  (`JRN-COMMIT`), `flush_failure_is_sink_flush_and_uncommitted` (`JRN-COMMIT`),
  `sink_error_poisons_permanently` (`JRN-POISON`);
  `mod journal_poisoning`: `commit_on_poisoned_journal_panics` (`JRN-POISON`, A8;
  `#[should_panic(expected = …)]`).
- **Rust notes:** bytes written before a failed flush remain an uncertain suffix —
  tests inspect the call log, never pretend rollback.
- **Size:** ~100 lines.

**C9 — Journal fault matrix (tests-only)**
- **Builds:** grows `journal.rs` tests (explicit touch; no production change): the
  comprehensive sink call/result traces and committed-boundary checks over the C7
  scripted sink.
- **Discharges:** direct unit coverage of `JRN-FORMAT`, `JRN-COMMIT`, `JRN-POISON`,
  `JRN-SINK`; `TRUST-SINK`'s memory-sink check.
- **Proves:** `mod journal_commit_boundaries`:
  `only_successful_flush_advances_the_committed_boundary` (`JRN-COMMIT`);
  `mod journal_sink_matrix`: `every_sink_failure_poisons_exactly_once` (`JRN-POISON`),
  `the_sink_receives_exact_bytes_in_call_order` (`JRN-SINK`, `TRUST-SINK`).
- **Rust notes:** this is permanent unit coverage, not duplicate Engine fault coverage
  — the Engine's suites later verify classification into `JournalFatal`.
- **Size:** ~100 test lines.

### Phase 2 — Application

**C10 — `application.rs`**
- **Builds:** `Outcome`, the `Application` trait, `Context<'a, C>` over
  `&'a mut BoundedBuffer<C>` with the overflow marker in the Context, `pub(crate)`
  construction/readback (P7 shape; F1).
- **Discharges:** the Application API block; `APP-CONTEXT`, `APP-EMIT`,
  `APP-OVERFLOW`, `APP-FUTURE` (signature-level); `VERIFY-CONTEXT`'s in-file half.
- **Proves:** `mod context_emit`: `commands_append_in_call_order_through_exact_capacity`
  (`APP-EMIT`), `remaining_reports_exact_free_capacity` (`APP-CONTEXT`);
  `mod context_overflow`: `first_over_bound_emit_stores_nothing_and_sets_the_marker`
  (`APP-OVERFLOW`), `every_later_emit_stores_nothing` (`APP-OVERFLOW`),
  `remaining_is_zero_once_the_marker_is_set` (`APP-OVERFLOW`);
  `mod context_reuse`: `fresh_invocation_starts_empty_with_a_clear_marker`
  (`APP-OVERFLOW`).
- **Rust notes:** a lifetime parameter on a struct; `emit` takes ownership of `C` and
  drops rejected Commands — that is what makes it infallible by signature. Tricky
  part: none — P7 is the whole shape.
- **Size:** ~140 lines.

### Phase 3 — Port

**C11 — `port.rs`**
- **Builds:** `PortContract`, `Never` + manual `Serialize` (+F2 derives), `ports!`
  with `#[macro_export]` and `$crate::PortContract` paths (P2 verbatim, adjusted to
  the crate root).
- **Discharges:** the Port API block; `PORT-SUMS`' and `PORT-ROUTING`'s compile-time
  substance (their trusted residue stays `TRUST-ROUTING`).
- **Proves:** `mod ports_macro_expansion`: `generated_sums_are_externally_tagged`
  (by name: the Mechanism's wire form), `hand_written_equivalent_is_byte_identical`
  (`PORT-SUMS`), `contract_bound_at_two_slots_yields_two_variants` (`PORT-SUMS`);
  `mod never_direction`: `never_command_arm_is_discharged_by_match` (by name: `Never`).
- **Rust notes:** `macro_rules!` fragment specifiers (`$vis:vis`, `$ty`), `$crate`
  hygiene, why the consumer needs a direct `serde` dependency. Tricky part: keep the
  macro exactly two enums — resist adding metadata (W7 depends on that restraint).
- **Size:** ~130 lines.

**C12 — the downstream macro fixture**
- **Builds:** `tests/ports_macro.rs` — invokes `kavod::ports!` as a real consumer
  (integration tests are a separate crate, so this is the exported-macro boundary) and
  exhaustively matches both generated sums.
- **Discharges:** the macro path/hygiene portion of `PORT-SUMS`; the documented
  consumer-dependency requirement.
- **Proves:** `mod ports_macro_downstream`: `consumer_invocation_compiles_and_serializes`
  (`PORT-SUMS`), `the_fanout_match_is_exhaustive` (`PORT-ROUTING`).
- **Rust notes:** an exhaustive match is compile evidence; semantic destination
  correctness waits for the wiring chunks.
- **Size:** ~70 lines.

### Phase 4 — Environment contract and the latch core

**C13 — `environment.rs`**
- **Builds:** the `Environment` trait, `ShutdownReport`, `Quiescence`, doc comments
  binding the behavior rows they quote.
- **Discharges:** the Environment API block; the type-enforced portions of the
  commitment table and `ENV-SERIAL` (consuming `shutdown`).
- **Proves:** `mod environment_contract_shape`:
  `a_scripted_implementation_drives_all_five_operations` (by name: the Environment API
  block) — a minimal in-file impl exercising the call pattern `ENV-SERIAL` names,
  including consuming `shutdown`.
- **Rust notes:** associated types; a consuming receiver in a trait (`fn shutdown(self)`
  needs `Self: Sized` — it has it; trait objects are not a design goal because the
  Engine is generic over `E`).
- **Size:** ~80 lines.

**C14 — `latch.rs` (crate-internal)**
- **Builds:** `Latch<E>`: empty/pending/reported/closed, `publish` (first-wins,
  discard after close), `take` (snapshot; pending → reported forever), `close`
  (returns the pending Error once; closed forever), `is_pending`, plus the
  observation-precedence helpers (pending beats a local failure; close into a report
  exactly once) both owners share (F6).
- **Discharges:** `ENV-LATCH`'s state rules and local-failure precedence (its
  *ordering* rules are each Environment's, later).
- **Proves:** `mod latch_first_wins`: `first_publication_is_kept_and_later_discarded`
  (`ENV-LATCH`), `take_marks_reported_forever` (`ENV-LATCH`); `mod latch_close`:
  `close_returns_the_pending_error_exactly_once` (`ENV-LATCH`),
  `publication_after_close_is_discarded` (`ENV-LATCH`);
  `mod latch_precedence`: `a_pending_error_wins_and_discards_the_local_error`
  (`ENV-LATCH`, A4), `a_local_error_wins_when_the_latch_is_empty` (`ENV-LATCH`).
- **Rust notes:** an enum-with-payload state machine and `std::mem::replace` to move
  the Error out. The latch carries no lock — its owner's lock acquisition is the
  linearization decision, which is why one state machine serves both Environments.
  Tricky part: `take` on `reported` must return `None` — reported is forever.
- **Size:** ~140 lines.

### Phase 5 — Record grammar (`engine/`)

From C15 on, `engine/record.rs` obeys the F10 import discipline so the C35 fixture can
mirror it. `engine/mod.rs` stays wiring-only: `mod` declarations plus the re-exports
`CRATE-EXPORTS` names.

**C15 — engine skeleton and the public exit types**
- **Builds:** `engine/mod.rs`; `engine/record.rs` part 1: `RecordKind` + `tag()`,
  `TurnOutcome` (+`Clone, Copy`, F2), `JournalFatal`; `engine/engine.rs` part 1:
  `EngineConfig`, `BuildError`, `EngineExit`, `FatalCause`, `EnvironmentFatal`,
  `EnvironmentOperation`, `CoreError` — declarations and doc comments only.
- **Discharges:** the Run API block's type inventory; `RUN-RECORDS`' bare-tag rules
  for `record_kind` and `outcome`.
- **Proves:** `mod record_kind_wire`: `turn_outcome_serializes_as_a_bare_tag`
  (`RUN-RECORDS`), `kind_tags_match_their_variant_names` (`RUN-RECORDS`);
  `mod journal_fatal_metadata`: `outcome_is_present_only_for_turn_completed` (by
  name: `JournalFatal`).
- **Rust notes:** three-generic enums (`FatalCause<AE, EE>`); nothing tricky — this is
  deliberately a breather chunk before typestate.
- **Size:** ~140 lines, mostly declarations.

**C16 — the record marker and the first two payloads**
- **Builds:** grows `engine/record.rs`: `RecordPayload`, `Kind<P>` with the shared
  hand-written `Serialize` (P3), `RunStartedRecord`, and the lifetime+generic
  `EventAcceptedRecord<'a, Ev>` — fields exactly in the Records table's order.
- **Discharges:** the `RunStarted` and `EventAccepted` Records rows; the enforcement
  rule that a kind/payload mismatch is unconstructible even in-module (F4).
- **Proves:** `mod record_payload_wire`:
  `run_started_matches_the_documented_example_line` (`RUN-RECORDS`),
  `event_accepted_serializes_index_time_and_borrowed_event` (`RUN-RECORDS`),
  `payload_tag_and_kind_share_one_source` (`RUN-GRAMMAR`).
- **Rust notes:** serde derive on lifetime+generic structs (bounds are added for you);
  `Kind<Self>`; struct field declaration order controls serde map order. Borrowing the
  Event during serialization is what later lets commit return the owned Event without
  `Clone`. Tricky part: none after P3 — copy its shape.
- **Size:** ~110 lines.

**C17 — the remaining payloads**
- **Builds:** grows `engine/record.rs`: `CommandsPreparedRecord<'a, C>` (borrowing the
  batch as a slice), `CommandsDispatchedRecord`, `StopRequestedRecord`,
  `TurnCompletedRecord` — fields exactly in the Records table's order.
- **Discharges:** the four remaining Records rows; `RUN-RECORDS`' field inventory
  complete.
- **Proves:** `mod record_payload_wire` (grown):
  `commands_prepared_keeps_batch_order_in_bytes` (`RUN-RECORDS`),
  `every_payload_leads_with_its_kind_in_table_order` (`RUN-RECORDS`),
  `turn_completed_outcome_is_a_bare_tag_for_both_answers` (`RUN-RECORDS`).
- **Rust notes:** serialize a shared view of the batch before any Command moves; do
  not clone the batch or payloads.
- **Size:** ~110 lines.

**C18 — the certificate, minting, and `RunStarted`**
- **Builds:** grows `engine/record.rs`: `Certificate<W, P>` (P1 shape, real types:
  `EventIndex`, `Timestamp`, `Journal<W>`), phase markers `Initial` … `Closed` with the
  answer markers in a private module (F5), `mint` (consumes the Journal and the frozen
  start time; asserts the induction base), the private generic commit helper
  (`JournalError` + `P::KIND` [+ outcome for `TurnCompleted`] → `JournalFatal`), and
  `run_started()`. A minimal failing-writer fixture lands in the file's test module
  (F7 — the accepted small duplication with C7's scripted sink).
- **Discharges:** the certificate block; `RUN-INDEX`'s prospective-zero rule; the
  `Initial → TurnOpen` edge row; `RUN-RECORDS`' "only possible first record".
- **Proves:** `mod certificate_minting`:
  `minting_asserts_the_prospective_index_base` (`RUN-ENFORCEMENT`),
  `run_started_commits_the_versioned_first_record` (`RUN-RECORDS`);
  `mod certificate_fatal_path`:
  `commit_failure_names_run_started_and_destroys_the_journal` (`RUN-GRAMMAR`).
- **Rust notes:** `PhantomData<fn() -> P>` (why `fn() ->` keeps `Send`/`Sync`
  independent of the phase); consuming `self` methods; why omitting `Clone`/`Copy`/
  `Default` *is* the enforcement. Tricky part: the phase-advance helper (P1's
  `advance`) keeps every transition's body one honest line of bookkeeping.
- **Size:** ~150 lines.

**C19 — `classify` and the recordless edge**
- **Builds:** grows `engine/record.rs`: `ClassifiedTurn`, `classify(answer)`,
  `no_commands(&BoundedBuffer<C>)` with its emptiness assert.
- **Discharges:** the `classify` and `no_commands` transition rows; the empty-batch
  edge row; `RUN-ENFORCEMENT`'s "after that call no transition accepts an answer".
- **Proves:** `mod turn_classification`: `classify_fixes_the_answer_in_the_phase_type`
  (`RUN-ENFORCEMENT`), `the_empty_batch_edge_commits_nothing` (by name: the Edges
  table's recordless row), `no_commands_panics_on_a_nonempty_buffer`
  (`ASSERT-INVARIANTS`; `#[should_panic]`).
- **Rust notes:** enum variants carrying differently-typed certificates; a runtime
  match fixing a compile-time marker. The buffer reference is only the assertion
  witness — it does not become part of the successor. Tricky part: conceptual only —
  the marker types never exist at runtime.
- **Size:** ~100 lines.

**C20 — `dispatch_batch`**
- **Builds:** grows `engine/record.rs`: `dispatch_batch(env, &mut BoundedBuffer<C>)` —
  nonempty assert, `CommandsPrepared` from a shared view, per-Command drain-by-value
  handoff in order, `Err` at k → `{ position, error }` with the suffix discarded,
  `CommandsDispatched` after the last handoff. The in-file recording/scripted
  Environment test helper (an `Environment` impl with a call log and a scripted
  failure position) is born here (F7).
- **Discharges:** the `Prepared` phase row, both its edge rows, the `dispatch_batch`
  transition row (A5 bracketing).
- **Proves:** `mod batch_dispatch`:
  `prepared_then_each_handoff_in_order_then_dispatched` (A5; the Edges table),
  `error_at_position_k_keeps_the_prefix_and_discards_the_suffix` (the Phases table's
  `Prepared` row), `prepared_commit_failure_precedes_any_handoff` (`RUN-GRAMMAR`),
  `dispatched_commit_failure_follows_every_handoff` (the Edges table),
  `an_empty_buffer_is_an_invariant_panic` (`ASSERT-INVARIANTS`).
- **Rust notes:** the one-transition fusion (the design explains why prepare/dispatch
  cannot be two calls — read that Derive before coding); `enumerate` supplies the
  zero-based failure position; dropping the drain discards the suffix. Tricky part:
  commit borrows the buffer immutably, the drain needs it mutably — sequence them;
  the borrow checker will hold you to it.
- **Size:** ~150 lines.

**C21 — checkpoint and the completion records**
- **Builds:** grows `engine/record.rs`: `checkpoint(env)` (`take_error` snapshot,
  exactly once, commits nothing), `complete_continue()`, `request_stop()`.
- **Discharges:** `RUN-CHECKPOINT`'s transition conduct; the `EffectsComplete`,
  `Checkpointed` phase rows; the `TurnCompleted(Continue)` and `StopRequested` edge
  rows.
- **Proves:** `mod turn_checkpoint`: `the_snapshot_is_taken_exactly_once`
  (`RUN-CHECKPOINT`), `a_pending_error_is_checkpoint_fatal_and_consumes_the_certificate`
  (`RUN-CHECKPOINT`);
  `mod turn_completion`: `continue_commits_turn_completed_continue` (the Edges table),
  `stop_commits_stop_requested` (the Edges table),
  `the_committed_outcome_is_the_phase_marker_not_a_caller_value` (`RUN-ENFORCEMENT`).
- **Rust notes:** methods that exist only on one marker instantiation
  (`impl Certificate<W, Checkpointed<ContinueMark>>`) — this is where typestate pays.
  The checkpoint Error branch returns no certificate: that ownership fact is what
  prevents any completion record after a pending Error. `request_stop` does not
  borrow the Environment — intent commit structurally precedes shutdown.
- **Size:** ~130 lines.

**C22 — `accept_event`**
- **Builds:** grows `engine/record.rs`: `accept_event(env)` — domain check before
  `next_event`, regression check before commit, `EventAccepted` with the derived index
  and returned time, certificate updated only on committed success, the accepted Event
  returned for the handler.
- **Discharges:** `RUN-INDEX` (domain check + overflow panic), the `BetweenTurns`
  phase row, the `EventAccepted` edge row, the Run's `ENV-TIME` boundary check.
- **Proves:** `mod event_acceptance`:
  `the_domain_check_precedes_next_event` (`RUN-INDEX`; the scripted env proves no call),
  `a_decreasing_stamp_is_time_regression_with_the_candidate_consumed` (the Edges
  table's `EventAccepted` row), `an_equal_stamp_is_accepted` (`ENV-TIME`),
  `acceptance_advances_index_and_time_only_on_commit` (`RUN-GRAMMAR`),
  `event_accepted_bytes_carry_the_new_index_and_time` (`RUN-RECORDS`).
- **Rust notes:** in-module tests may construct a certificate at `u64::MAX - 1`
  directly (private fields; `RUN-ENFORCEMENT` says in-module conduct is test-enforced —
  that is this). Serialize `&event`; move `event` only after commit succeeds. Tricky
  part: the update-on-commit ordering — index/time are written to the certificate only
  after `commit` returns `Ok`.
- **Size:** ~150 lines.

**C23 — `close`**
- **Builds:** grows `engine/record.rs`: `close(env)` — consuming `shutdown`, retain
  quiescence before inspecting the Error, the three outcomes, `TurnCompleted(Stop)`,
  `Closed`.
- **Discharges:** the `StopPending` phase row, the `TurnCompleted(Stop)` edge row,
  `RUN-FINALIZE`'s retained-quiescence hooks.
- **Proves:** `mod stop_closing`: `a_clean_report_commits_turn_completed_stop` (the
  Edges table), `a_report_error_outranks_incomplete` (the `StopPending` row),
  `incomplete_without_error_is_shutdown_incomplete` (the `StopPending` row),
  `commit_failure_after_a_clean_report_retains_quiesced` (`RUN-FINALIZE`).
- **Rust notes:** taking `env` by value in a method whose siblings borrow it — the
  caller decides which world it is in (P1 proved the loop handles this). The
  Environment no longer exists on any branch out of this method.
- **Size:** ~140 lines.

### Phase 6 — Engine

The driver lands as two final private helpers plus the composing loop — each helper is
final code with its own tests, so no chunk exceeds one sitting and nothing is a
placeholder.

**C24 — `Engine::new`**
- **Builds:** grows `engine/engine.rs`: the `Engine` struct fields, `new` per the
  construction table (batch `try_reserve`, Journal build), no Application or
  Environment method invoked.
- **Discharges:** the construction table; `BOUND-NONZERO` for both config bounds.
- **Proves:** `mod engine_construction`:
  `batch_reservation_failure_is_command_buffer` (the construction table),
  `journal_build_failure_is_journal` (the construction table),
  `construction_invokes_no_application_or_environment_method` (the construction
  table; recording doubles).
- **Rust notes:** the three-way generic bound block — write it once on the `impl`, as
  the API block does (house rule: no re-declared generics).
- **Size:** ~100 lines.

**C25 — the turn helper**
- **Builds:** grows `engine/engine.rs`: a private final associated fn that clears the
  batch, builds a `Context` (index/time from the certificate), invokes exactly one
  handler (`on_start` at index zero, `on_event` after), ends the Context borrow,
  checks overflow before the returned `Outcome`, discards the batch on `Fatal(e)`, and
  calls `classify` for Continue/Stop.
- **Discharges:** the `TurnOpen` phase row (overflow beats `Outcome`; `Fatal` discards
  the batch); A2; A4's Core-over-Application precedence; `APP-STATE`; `APP-CONTEXT`
  (index/time sourced from the certificate).
- **Proves:** `mod turn_handler_selection`: `index_zero_calls_on_start_once` (the
  Phases table), `a_later_index_calls_on_event_once` (the Phases table);
  `mod turn_overflow_precedence`: `overflow_outranks_the_returned_outcome`
  (`APP-OVERFLOW`, A4); `mod turn_application_fatal`:
  `state_mutation_and_the_fatal_payload_both_stand` (`APP-STATE`).
- **Rust notes:** put the Context in its own lexical block so its mutable borrow ends
  before inspecting/draining the buffer; State stays outside all `Result` branches.
- **Size:** ~130 lines.

**C26 — the finalization helper**
- **Builds:** grows `engine/engine.rs`: `finalize` per F12 — one private associated fn
  for the three Environment states (unconsumed after successful start; consumed by
  Stop with retained quiescence; the `start`-`Err` path). It never changes the fixed
  cause and calls `shutdown` at most once.
- **Discharges:** `RUN-FINALIZE`'s three arms; A4 (the shutdown report's Error never
  replaces the fixed cause); the finalization portion of `ENV-SERIAL`.
- **Proves:** `mod fatal_finalization`:
  `a_started_environment_is_shutdown_exactly_once` (`RUN-FINALIZE`),
  `the_shutdown_error_never_replaces_the_fixed_cause` (A4, `RUN-FINALIZE`),
  `a_start_error_skips_shutdown_and_is_quiesced` (`ENV-START`, `RUN-FINALIZE`),
  `a_consumed_environment_uses_the_retained_quiescence` (`RUN-FINALIZE`).
- **Rust notes:** `Option<E>` at this one orchestration boundary makes availability
  explicit — `None` *proves* Stop already consumed it. The `(Some(_), Some(_))` arm is
  `unreachable!` and says so.
- **Size:** ~110 lines.

**C27 — `Engine::run`, composed**
- **Builds:** grows `engine/engine.rs`: `run(self)` composing C25/C26 in one
  nonrecursive loop — the startup table (State first, `start`, mint, `RunStarted`),
  then per turn: the turn helper → the marker-generic `effects` helper (P1) →
  completion → `accept_event` at the loop's back edge, every error routed to
  `finalize`. No earlier helper is replaced.
- **Discharges:** the startup table; `RUN-SERIAL`; the Phases/Edges tables as
  orchestration; `BOUND-LOOPS` for the driver.
- **Proves:** `mod run_startup`: `state_is_created_before_any_fallible_step` (the
  startup table), `a_start_error_exits_fatal_quiesced_without_shutdown` (the startup
  table; `ENV-START`); `mod run_stop_path`:
  `stop_at_start_produces_the_three_record_journal` (`RUN-GRAMMAR`, `RUN-RECORDS`),
  `stopped_carries_the_final_state` (by name: `EngineExit`),
  `the_call_sequence_matches_env_serial` (`ENV-SERIAL`; recording env).
- **Rust notes:** P1's loop verbatim with real types; where each `match` arm moves
  what. Tricky part: resist "simplifying" the nested matches until it compiles — then
  extract further helpers only if they still borrow cleanly.
- **Size:** ~130 lines (the helpers already carry the logic).

**C28 — the turn loop's remaining behavior (tests-only)**
- **Builds:** grows `engine/engine.rs` tests (explicit touch; no code change): the
  Continue path with Events and Commands, over-emit, handler `Fatal`, State-on-Fatal.
- **Discharges:** the `TurnOpen` phase row in full; `APP-STATE`; the intent-vacuum
  derivation's observable half; `VERIFY-CONTEXT`'s Fatal-path half (unit level).
- **Proves:** `mod run_turn_loop`: `continue_turns_accept_events_in_sequence` (A2),
  `overflow_beats_the_returned_outcome_and_discards_the_batch` (the `TurnOpen` row),
  `a_handler_fatal_discards_the_batch_and_carries_the_error` (the `TurnOpen` row, A4),
  `state_mutations_stand_on_every_fatal_exit` (`APP-STATE`),
  `an_over_emitting_turn_leaves_no_command_record` (by name: the intent-vacuum
  derivation under `Core(CommandBoundExceeded)`).
- **Rust notes:** scripted envs with multi-step scripts; asserting on captured sink
  bytes.
- **Size:** ~140 lines of tests.

### Phase 7 — Core suites (`tests/`)

**C29 — the integration harness (`tests/support`)**
- **Builds:** wiring-only `tests/support/mod.rs` plus one subject per file (F7):
  `scripted_env.rs` (the trace-driven `ScriptedEnv` builder — per-call scripted
  results asserting graph-conformant call order, with call/handoff/shutdown counters),
  `scripted_sink.rs` (scripted write/flush results, captured bytes, call log — the
  `tests/`-side twin of C7's unit fixture), `recording_app.rs` (a recording
  `Application` fixture), and golden-line helpers. `tests/harness_contract.rs` is the
  first consumer.
- **Discharges:** the permanent infrastructure for `VERIFY-CONFORMANCE`,
  `VERIFY-FAULTS`, `VERIFY-JOURNAL`, and `VERIFY-CONTEXT`'s Engine half; the
  observable subset of `TRUST-ENV` for this fixture.
- **Proves:** `mod scripted_environment_trace`:
  `records_each_operation_and_result_in_order` (by name: the Trace definition),
  `a_failed_dispatch_records_no_handoff` (by name: the dispatch commitment row);
  `mod scripted_sink_trace`: `stores_exactly_the_reported_prefix` (`TRUST-SINK`).
- **Rust notes:** `tests/support` is a module of each test crate, not a crate; scripts
  own values and consume each step exactly once; `Rc<RefCell<_>>` is fine here —
  Engine tests are serial (Live suites use thread-safe probes of their own).
- **Size:** ~150 lines.

**C30 — the golden suite, part 1**
- **Builds:** `tests/golden_journal.rs` with the full-run sequences over C29.
- **Discharges:** `VERIFY-JOURNAL`'s sequence half; `DET-ENV`'s premise that Journal
  bytes are Environment-independent given the trace (byte-fixed vectors);
  `TRUST-SERIALIZE`'s fixture check.
- **Proves:** `mod golden_sequences`:
  `a_stop_run_writes_exactly_its_records` (`VERIFY-JOURNAL`),
  `a_command_run_writes_exactly_its_records` (`VERIFY-JOURNAL`),
  `an_event_run_writes_exactly_its_records` (`VERIFY-JOURNAL`).
- **Rust notes:** byte-literal goldens beat string-building helpers for evidence —
  assert every comma, field order, bare tag, schema version, and trailing newline.
- **Size:** ~140 lines.

**C31 — the golden suite, part 2**
- **Builds:** grows `tests/golden_journal.rs` (explicit touch): the per-answer
  outcome-record pinning at `classify`'s single call site, the interior-newline
  rejection through a full Engine run, `CommandsDispatched` as a legal final record,
  and the table-driven empty/nonempty × Continue/Stop sequence matrix.
- **Discharges:** `VERIFY-JOURNAL` complete; `RUN-ENFORCEMENT`'s classify pinning.
- **Proves:** `mod classify_call_site`:
  `each_non_fatal_answer_yields_its_required_outcome_records` (`VERIFY-JOURNAL`,
  `RUN-ENFORCEMENT`); `mod encoding_rejection`:
  `an_interior_newline_payload_is_rejected_with_nothing_written` (`JRN-ENCODE` via
  `VERIFY-JOURNAL`); `mod fatal_tails`: `commands_dispatched_can_be_the_final_record`
  (`RUN-CHECKPOINT`); `mod graph_sequences`:
  `every_empty_and_command_turn_shape_has_its_required_sequence` (`VERIFY-JOURNAL`).
- **Rust notes:** one table of expected complete byte strings is easier to audit than
  incrementally parsed records.
- **Size:** ~140 lines.

**C32 — the fault suite, part 1: Journal faults**
- **Builds:** `tests/faults.rs`: scripted-sink failure at each record kind, checking
  `JournalFatal { record_kind, outcome }` and the exit; `start`-`Err` proving no
  shutdown call; the Stop-path commit failure retaining `Quiesced`.
- **Discharges:** `VERIFY-FAULTS`' Journal rows; `RUN-FINALIZE`'s Journal arms.
- **Proves:** `mod journal_fault_matrix`:
  `each_record_kind_maps_to_its_journal_fatal` (`VERIFY-FAULTS`),
  `only_turn_completed_carries_an_outcome` (by name: `JournalFatal`),
  `a_stop_commit_failure_retains_quiesced` (`RUN-FINALIZE`);
  `mod startup_faults`: `a_start_error_performs_no_shutdown` (`VERIFY-FAULTS`).
- **Rust notes:** a table entry names the record kind, the sink operation, the
  expected last committed bytes, the expected handoff count, and the expected
  `JournalFatal.outcome`.
- **Size:** ~150 lines.

**C33 — the fault suite, part 2: Environment and Application faults**
- **Builds:** grows `tests/faults.rs` (explicit touch): each operation `Err`
  (`NextEvent`, `Dispatch { position }`, `Checkpoint`, `Shutdown`), the decreasing
  timestamp, shutdown reports `Some(error)` and `{ Incomplete, None }`, the
  over-emitting Application, and the cross-product of every post-`start` operation
  `Err` with a `Some(error)` report.
- **Discharges:** `VERIFY-FAULTS` complete; A4's first-failure precedence as tested
  behavior; `VERIFY-CONTEXT`'s State-on-Fatal half complete.
- **Proves:** `mod environment_fault_matrix`:
  `each_operation_error_maps_to_its_cause_and_quiescence` (`VERIFY-FAULTS`),
  `the_operation_error_outranks_the_report_error` (A4, `RUN-FINALIZE`),
  `a_decreasing_stamp_is_time_regression` (`VERIFY-FAULTS`);
  `mod application_fault_matrix`:
  `an_over_emitting_application_is_command_bound_exceeded` (`VERIFY-FAULTS`),
  `state_mutations_survive_each_post_handler_fatal_exit` (`VERIFY-CONTEXT`,
  `APP-STATE`).
- **Rust notes:** generate the cross-product from data but keep each assertion
  explicit: original cause, shutdown count one, returned quiescence, discarded report
  Error.
- **Size:** ~150 lines.

**C34 — conformance, within-type**
- **Builds:** `tests/conformance.rs`: the scripted-trace catalog (success runs, each
  failure shape), each trace run twice against fresh scripted envs and sinks,
  comparing everything in `DET-RUN`'s list (handler calls via the recording
  Application, State transitions, Command intent, Journal bytes, `DET-RUN`-equal
  exits).
- **Discharges:** `DET-RUN`; `VERIFY-CONFORMANCE`'s within-type half (the scripted env
  asserts every call against the graph, including each handoff); the `TRUST-PURE`
  verification recipe exists as reusable machinery.
- **Proves:** `mod conformance_within_type`:
  `the_same_trace_reproduces_identical_journal_bytes` (`DET-RUN`),
  `the_same_trace_reproduces_det_run_equal_exits` (`DET-RUN`),
  `every_environment_call_is_graph_conformant` (`VERIFY-CONFORMANCE`).
- **Rust notes:** recreate Application, State, Environment, writer, and config for
  each run — reusing a mutated fixture would not test determinism.
- **Size:** ~150 lines.

**C35 — the compile-fail fixture (`VERIFY-GRAMMAR`)**
- **Builds:** `tests/compile_fail.rs` (trybuild runner; `trybuild` joins
  `[dev-dependencies]`, F9) plus the fixture crate `tests/grammar_fixture/` per P6:
  mirror stubs for F10's import paths, the reconstructed `mod engine { mod record {
  include!(…) } }`, one `.rs` case per attack — illegal transition sequences, a skipped
  checkpoint, a premature `TurnCompleted(Stop)`, committing `CommandsDispatched`
  outside the transition, an outcome disagreeing with the fixed answer,
  `Clone`/`Copy`/`Default` on the certificate — each with its `.stderr` pinned so the
  failure is the grammar restriction, plus one `legal.rs` pass-case proving the
  fixture itself compiles.
- **Discharges:** `VERIFY-GRAMMAR`.
- **Proves:** `mod grammar_compile_fail`: `illegal_transitions_do_not_compile`
  (`VERIFY-GRAMMAR`), `certificate_duplication_does_not_compile` (`VERIFY-GRAMMAR`),
  `the_fixture_reconstruction_itself_compiles` (`VERIFY-GRAMMAR`; the control case).
- **Rust notes:** trybuild's expected-stderr workflow (wildcards where rustc prose is
  unstable); `include!` + `env!("CARGO_MANIFEST_DIR")` paths; test `Copy` by
  attempting reuse after a move. Tricky part: every attack file must sit inside the
  reconstructed module — P6 showed the privacy trap, and the compiling control case is
  what proves later failures are grammar, not broken imports.
- **Size:** ~150 lines across the runner and cases.

### Phase 8 — Simulated Environment

The Sim comes before Live (single-threaded first — the same contract without the
concurrency). C36 is free; C37–C43 are **§10-gated** (they realize W1–W3, W6, W7).

**C36 — `SimPort`, `SimCtx`, and the lifecycle type**
- **Builds:** `sim/mod.rs` (wiring-only) and `sim/port.rs`: the `SimPort` trait,
  `SimCtx<'a, C>` (borrowing `now` and that Port's arm cell; `PhantomData<fn() -> C>`),
  `SimCtxError`; crate-internal `PortLifecycle { NotStarted, Open, Ended }`.
- **Discharges:** the Sim API block; `SIM-WAKEUP` (the arm's own rules).
- **Proves:** `mod sim_ctx_wakeup`: `set_next_before_now_is_rejected_unchanged`
  (`SIM-WAKEUP`), `later_set_next_replaces_the_arm` (`SIM-WAKEUP`),
  `clear_next_disarms` (`SIM-WAKEUP`), `now_is_readable_during_port_code`
  (by name: `SimCtx::now`).
- **Rust notes:** a mutable borrow threaded into a context struct — the same P7 move.
  The Ctx borrows only its own Slot's arm: a Port cannot reach another Slot's arm by
  construction.
- **Size:** ~120 lines.

**C37 — sim wiring and startup** *(§10-gated: W1, W3, W6, W7)*
- **Builds:** `sim/error.rs` (`SimError<PE>`), `sim/wiring.rs` (`SimConfig`,
  `SimWiring` two-state builder, `SlotHandle<C>`, the erased Slot runtime holding the
  boxed Port + fan-in constructor + `err_map`), `sim/env.rs`: the env struct and
  `Environment::start` per `SIM-START` (fix origin, start each `NotStarted` Port in
  frozen order, prefix-`stop` cleanup on first `Err`). The trace-recording `SimPort`
  test double is born here (in-file; promoted to `tests/support` in C42).
- **Discharges:** `SIM-START`, `SIM-LIFECYCLE`'s startup rows, `BOUND-STATIC`
  (typestate nonempty + frozen order), the env's `ENV-START` realization.
- **Proves:** `mod sim_startup`: `ports_start_in_frozen_slot_order` (`SIM-START`),
  `failure_at_slot_k_stops_exactly_the_open_prefix_in_order` (`SIM-START`),
  `the_failing_and_unstarted_ports_receive_no_stop` (`SIM-START`),
  `startup_returns_the_original_error` (`SIM-START`).
- **Rust notes:** `Box<dyn Trait>` for the erased runtime; why the erased trait can
  cover `start`/`step`/`stop` untyped while `on_command` cannot (F11 explains the
  asymmetry). Table-drive every failure position; move the failing entry to `Ended`
  before its prefix cleanup so even a `stop` Error cannot permit a second lifecycle
  call. Tricky part: the builder typestate — two small types, one conversion.
- **Size:** ~160 lines — the split signal: land `error.rs` + `wiring.rs` compiling
  first if a sitting runs short, but the chunk's tests need `start`.

**C38 — sim dispatch and `take_error`** *(§10-gated)*
- **Builds:** grows `sim/env.rs`: `dispatch` (pending-latch check first, then the
  router's exhaustive match into `hand_off`, the `on_command` invocation as the
  handoff commitment, `Err` published and `Ok` returned), `take_error` over the C14
  latch.
- **Discharges:** `SIM-DISPATCH`; the sim's `ENV-ERRORS` naming (commitment at
  invocation); F11's delivery assert.
- **Proves:** `mod sim_dispatch`: `handoff_commits_at_the_on_command_invocation`
  (`SIM-DISPATCH`), `an_on_command_error_latches_and_dispatch_returns_ok`
  (`SIM-DISPATCH`), `a_pending_error_returns_first_with_no_invocation`
  (`SIM-DISPATCH`, `ENV-LATCH`), `a_final_command_error_reaches_the_run_via_take_error`
  (by name: the sim Mechanism's `take_error` note).
- **Rust notes:** `Box<dyn Any>` downcast with the always-on `expect` (checklist row).
  Invocation is the commitment point, not the Port's successful return.
- **Size:** ~120 lines.

**C39 — sim selection helpers** *(§10-gated)*
- **Builds:** grows `sim/env.rs` with two private final helpers: a read-only scan
  returning the lowest-time armed Slot (equal times resolved from the persistent
  cursor, wrapping once in frozen order; no mutation), and a one-selection step
  (advance `now`, clear the arm, assert the selected Port is `Open`, call `step`,
  advance the cursor after every selected call, return a private
  `Selected::{Event, Continue}` for `Some`/`None`, `Ended` on `Err`).
- **Discharges:** the scan and tie-order portions of `SIM-SELECT`; `SIM-TIME`'s
  advance rule; the selected-`Open` assertion site; `BOUND-LOOPS` for the scan.
- **Proves:** `mod sim_selection_scan`: `the_lowest_time_wins_and_ties_follow_the_cursor`
  (`SIM-SELECT`), `the_scan_wraps_once_in_frozen_order` (`SIM-SELECT`, `BOUND-LOOPS`);
  `mod sim_selection_step`: `now_advances_and_the_arm_clears_before_step`
  (`SIM-SELECT`, `SIM-TIME`), `a_selected_step_none_moves_the_cursor_and_continues`
  (`SIM-SELECT`), `a_step_error_ends_the_port_with_subordinate_effects_standing`
  (`SIM-LIFECYCLE`), `a_selected_closed_port_is_an_invariant_panic`
  (`ASSERT-INVARIANTS`; `#[should_panic]`).
- **Rust notes:** the wrapping cursor scan (`(cursor + i) % len` over the frozen
  order); separating the read-only scan from the mutating step keeps ordering
  testable without callback borrows. The cursor updates regardless of
  `Some`/`None`/`Err` after a selected call.
- **Size:** ~130 lines.

**C40 — sim `next_event`** *(§10-gated)*
- **Builds:** grows `sim/env.rs`: `next_event` — check order per `SIM-SELECT` (latch,
  nothing-armed, budget — all before any selection effect), a fresh per-call step
  counter spending one unit per selected step, the C39 helpers in a bounded loop,
  subordinate effects standing on every `Err`.
- **Discharges:** `SIM-SELECT` complete; `SIM-STEPS`; `SIM-COMPLETION`; the sim's
  `ENV-TIME` realization and consumption-instant naming.
- **Proves:** `mod sim_next_event`: `budget_exhaustion_precedes_any_selection_effect`
  (`SIM-STEPS`), `the_exact_configured_budget_is_permitted` (`SIM-STEPS`),
  `the_budget_is_fresh_for_each_call` (`SIM-STEPS`),
  `nothing_armed_is_the_completion_error` (`SIM-COMPLETION`),
  `stamps_never_decrease_and_equal_stamps_are_valid` (`SIM-TIME`),
  `an_error_leaves_completed_selections_standing` (`SIM-SELECT`).
- **Rust notes:** check the budget *before* selecting/advancing/clearing — the
  guarantee row's order is the code's order; checked counter arithmetic even where the
  NonZero bound makes overflow remote.
- **Size:** ~120 lines.

**C41 — sim shutdown** *(§10-gated)*
- **Builds:** grows `sim/env.rs`: `shutdown(self)` per `SIM-SHUTDOWN` — close
  admission, `stop` each `Open` Port once in frozen order (moving it `Ended` before
  the call), publish every `Err` first-wins, final observation closes the latch into
  the report, `Quiesced` always.
- **Discharges:** `SIM-SHUTDOWN`; the sim's `ENV-SHUTDOWN` realization.
- **Proves:** `mod sim_shutdown`: `stop_runs_once_per_open_port_in_frozen_order`
  (`SIM-SHUTDOWN`), `a_stop_error_does_not_prevent_remaining_stops` (`SIM-SHUTDOWN`),
  `the_first_stop_error_reaches_the_report` (`SIM-SHUTDOWN`, `ENV-LATCH`),
  `an_all_ok_shutdown_reports_quiesced_none` (`SIM-SHUTDOWN`).
- **Rust notes:** one ordered loop that continues after every `stop` Error; since all
  callbacks have returned and every started entry is `Ended`, quiescence is
  structural.
- **Size:** ~120 lines.

**C42 — `tests/sim_lifecycle.rs` (`VERIFY-SIM`)** *(§10-gated)*
- **Builds:** the full `VERIFY-SIM` matrix with per-Port call traces (the recording
  `SimPort` moves to `tests/support` — explicit touch): startup failure at every Slot
  position; `on_command`/`step` `Err` followed by shutdown; `stop` `Ok`/`Err` at every
  position; `Ended` receives no later method; the wakeup/selection/budget/bounds
  items; storage-growth checks; per-Slot routing (`TRUST-ROUTING` for the suite's own
  wiring).
- **Discharges:** `VERIFY-SIM`; the sim rows of `ENV-BOUNDS`.
- **Proves (representative):** `mod sim_lifecycle_matrix`:
  `an_ended_port_receives_no_later_method` (`SIM-LIFECYCLE`),
  `startup_failure_at_every_position_cleans_exactly_the_prefix` (`VERIFY-SIM`);
  `mod sim_bounds`: `one_arm_per_port_never_grows` (`ENV-BOUNDS`),
  `exact_budget_boundaries_permit_the_configured_calls` (`SIM-STEPS`);
  `mod sim_routing`: `each_slot_receives_only_its_commands` (`TRUST-ROUTING`).
- **Size:** ~150 lines (grow in sittings by matrix row if needed; each sitting ends
  green).

**C43 — sim conformance and the finite-source example** *(§10-gated)*
- **Builds:** grows `tests/conformance.rs` (explicit touch): Engine-over-Sim
  end-to-end runs, twice-run `DET-RUN` comparison, and the byte-equal single-Port
  replay the design's replay Derive describes (its three preconditions scripted); a
  finite-source example Port (terminal Event → `Stop`) as a permanent fixture.
- **Discharges:** `DET-RUN` over the shipped Sim; `VERIFY-CONFORMANCE`'s sim leg;
  `TRUST-SIM-PORT`'s fixture check; the finite-source pattern demonstrated.
- **Proves:** `mod conformance_sim`: `a_sim_run_repeats_byte_identically` (`DET-RUN`),
  `a_single_port_replay_reproduces_the_recorded_run` (by name: the replay Derive's
  three preconditions), `a_finite_source_run_ends_stopped` (by name: finite-source
  pattern).
- **Size:** ~130 lines.

### Phase 9 — Live Environment

C44–C46 and C48 are free; C47 and C49–C56 are **§10-gated** (W1–W5, W7, W9).

**C44 — the clock**
- **Builds:** `live/mod.rs` (wiring-only) and `live/clock.rs`: the public documented
  `MonotonicClock` trait (W5/F8) with its contract in the doc comment, the public
  `StdClock` (anchored `Instant`, origin + checked elapsed→u64 conversion), and
  crate-internal deadline helpers (`saturating_add` / non-underflowing remaining,
  P5).
- **Discharges:** `LIVE-TIME`'s arithmetic half; `LIVE-SHUTDOWN`'s saturation
  sentence.
- **Proves:** `mod clock_stamps`: `production_stamps_never_decrease` (`LIVE-TIME`),
  `conversion_exhaustion_is_a_typed_error_value` (`LIVE-TIME`); `mod clock_deadline`:
  `deadline_addition_saturates_at_the_domain_maximum` (`LIVE-SHUTDOWN`, A6),
  `remaining_time_never_underflows` (`LIVE-SHUTDOWN`).
- **Rust notes:** `Instant` vs the u64-nanosecond axis — everything Kavod compares
  lives on the u64 axis; `Instant` exists only inside `StdClock`. The trait is public
  API: document the nondecrease contract and keep the surface to the one method so
  there is nothing else to promise.
- **Size:** ~110 lines.

**C45 — lifecycle cell and the start/cancel gate**
- **Builds:** `live/sync.rs`: the lifecycle cell (`Running`/`Shutdown` behind a lock,
  readable via `Lifecycle`), the start/cancel gate (P5's `Mutex`+`Condvar` shape).
- **Discharges:** the gate mechanics `LIVE-START` presupposes; `Lifecycle` reading for
  `LIVE-LIFECYCLE`.
- **Proves:** `mod start_gate`: `no_shell_proceeds_while_the_gate_is_pending`
  (`LIVE-START`), `cancel_wakes_every_waiting_shell` (`LIVE-START`),
  `start_wakes_every_waiting_shell` (`LIVE-START`).
- **Rust notes:** first threaded chunk — `std::thread::spawn`, `Arc`, the
  condition-variable wait loop (always re-check the predicate; `Condvar::wait` may
  wake spuriously). Read P5 before starting.
- **Size:** ~130 lines.

**C46 — the central select monitor**
- **Builds:** `live/central.rs`: `Central<Ev, E>` under one `Mutex` + `Condvar` — the
  bounded fan-in `VecDeque`, the C14 `Latch<E>`, the lifecycle mirror the selector
  checks, the fixed completion-entry array with its one-token-per-Slot wake bound —
  plus `offer` admission (map first, bounded, never waits, `Full`/`Closed` return the
  Event), publication (publish + notify), and the wait predicate (`latch pending or
  event available`).
- **Discharges:** `LIVE-EVENTS` (admission half), the wait mechanics of
  `LIVE-SELECT`, the one-lock discipline the Live Justify note sketches (P8).
- **Proves:** `mod fan_in_admission`:
  `offer_succeeds_through_exact_capacity_then_full_returns_the_event`
  (`LIVE-EVENTS`), `dequeue_order_is_admission_order` (`LIVE-EVENTS`),
  `offer_after_close_returns_closed_with_the_event` (`LIVE-EVENTS`);
  `mod select_wait`: `an_event_or_a_publication_wakes_the_wait` (`LIVE-SELECT`).
- **Rust notes:** one lock owning several facts (the design's Justify note is the
  spec); `Condvar::notify_all` vs `notify_one` — wake broadly, filter by predicate.
  Preallocate every bounded container before activation.
- **Size:** ~150 lines.

**C47 — inboxes and `LiveCtx`** *(§10-gated: W4)*
- **Builds:** `live/inbox.rs` (per-Port bounded `Mutex`+`Condvar` inbox with a
  shutdown flag: blocking `recv` reporting the signal ahead of queued Commands,
  `try_recv` draining Commands then `Shutdown`, non-waiting admission) and
  `live/ctx.rs`: `PortInput`, `OfferRejected`, `Lifecycle`, `LiveCtx<C>` per W4.
- **Discharges:** `LIVE-LIFECYCLE`; `LIVE-DISPATCH`'s inbox half; the `LiveCtx` API
  block (now final).
- **Proves:** `mod live_ctx_signal`:
  `recv_reports_a_raised_signal_ahead_of_queued_commands` (`LIVE-LIFECYCLE`),
  `try_recv_drains_commands_before_reporting_shutdown` (`LIVE-LIFECYCLE`),
  `try_recv_none_means_no_command_and_no_signal` (`LIVE-LIFECYCLE`);
  `mod inbox_admission`: `admission_never_waits_and_full_is_refusal` (`LIVE-DISPATCH`).
- **Rust notes:** two condition sources under one inbox lock (queue nonempty, signal
  raised); the boxed offer closure erasing `Ev` (W4). Tricky part: `recv` must wake on
  a signal raised while it sleeps — the raise path must notify every inbox.
- **Size:** ~150 lines.

**C48 — supervision shell, completion state, terminal guard**
- **Builds:** `live/supervise.rs`: the shell fn a spawned thread runs (gate wait →
  cancel return or `LivePort::run` → result classification per `LIVE-SUPERVISION`,
  publication under the central lock) and the non-cloneable terminal guard living on
  the shell's frame — its `Drop` publishes a pre-signal unwind's premature-closure
  Error first, then flips that Slot's completion entry exactly once (assert:
  was `Outstanding`) and sends one nonblocking wake.
- **Discharges:** `LIVE-SUPERVISION`; `LIVE-COMPLETION` (capability, exactly-once,
  publication-precedes-`Complete` — P8's discipline); `LIVE-THREADS`' boundary bounds.
- **Proves:** `mod supervision_completion`:
  `each_terminal_path_completes_the_entry_exactly_once` (`LIVE-COMPLETION`),
  `every_required_publication_precedes_complete` (`LIVE-SUPERVISION`),
  `a_premature_ok_publishes_and_wakes_the_select` (`LIVE-SUPERVISION`),
  `a_pre_signal_unwind_publishes_premature_closure` (`LIVE-SUPERVISION`; test-profile
  unwind), `a_post_signal_ok_stays_unpublished` (`LIVE-SUPERVISION`).
- **Rust notes:** a `Drop` guard as the completion capability (why the Port value and
  `LiveCtx` never see it — it lives only on the shell's stack frame);
  `catch_unwind` is *not* used — the guard's `Drop` runs during test-profile unwind by
  itself. Tricky part: publish-then-complete order under the one lock.
- **Size:** ~150 lines.

**C49 — live wiring and `start`** *(§10-gated: W1–W5, W9)*
- **Builds:** `live/error.rs` (`LiveError<PE>`), `live/wiring.rs` (`LiveConfig`,
  `LiveWiring` two-state builder, `slot` creating the typed inbox + `SlotHandle<C>` +
  spawn closure, `build(config, router)` and the documented
  `build_with_clock(config, router, clock)` — W5), `live/env.rs`:
  `Environment::start` per the Mechanism's six steps (create shared state, spawn named
  shells in frozen order at the gate, finish fallible setup, stamp and freeze the
  start time, cancel-join-`Err` on any failure, signal start as the commitment).
- **Discharges:** `LIVE-START`; `LIVE-THREADS` (spawn topology); `BOUND-STATIC` for
  live; the live `ENV-ERRORS` activation naming; W9's thread names.
- **Proves:** `mod live_startup`: `no_port_code_runs_before_gate_activation`
  (`LIVE-START`), `failed_setup_cancels_joins_every_shell_and_errs` (`LIVE-START`),
  `spawn_failure_maps_to_its_slot_name` (by name: `LiveError::SpawnFailed`),
  `the_frozen_start_time_is_returned_after_activation` (`LIVE-START`).
- **Rust notes:** `thread::Builder::name(…).spawn` returns `io::Result<JoinHandle>`;
  moving a `FnOnce` spawn closure per Slot; keeping `JoinHandle`s in frozen order.
  On failure, preserve the original setup Error; discard gate-shell cleanup results
  after joining.
- **Size:** ~160 lines (split signal: `error.rs` + `wiring.rs` types can land
  compiling first; the tests need `start`).

**C50 — live `next_event`, `dispatch`, `take_error`** *(§10-gated)*
- **Builds:** grows `live/env.rs`: `next_event` (wait per C46's predicate; pending
  latch taken first; stamp after the wait and before the dequeue; the dequeue is
  consumption; nothing fallible after it), `dispatch` (pending latch first; router
  match; one non-waiting admission; a full inbox → typed `Err`, nothing handed off —
  no closed-inbox case exists, per the W3 note), `take_error` (one snapshot). A small
  scripted-clock fixture lands in the file's test module (F7).
- **Discharges:** `LIVE-SELECT`, `LIVE-DISPATCH`, `LIVE-TIME` (stamping-in-place);
  the live `ENV-ERRORS` consumption naming.
- **Proves:** `mod live_acceptance`: `the_stamp_is_taken_after_the_wait_before_the_dequeue`
  (`LIVE-SELECT`; scripted clock), `time_exhaustion_leaves_the_event_queued`
  (`LIVE-SELECT`), `a_waking_event_is_stamped_no_earlier_than_its_admission`
  (`LIVE-SELECT`; scripted clock); `mod live_dispatch`:
  `a_full_inbox_is_a_typed_error_with_no_handoff_or_growth` (`LIVE-DISPATCH`),
  `a_pending_error_returns_before_any_routing` (`ENV-LATCH`).
- **Rust notes:** don't hold the central lock across the router call; re-take it for
  the admission commit (the inbox has its own lock — mind the lock order, central
  never nested inside an inbox). Hold the queue lock across stamp and dequeue so the
  selected front stays stable.
- **Size:** ~150 lines.

**C51 — live shutdown** *(§10-gated)*
- **Builds:** grows `live/env.rs`: `shutdown(self)` — one initiating critical section
  (raise the signal, end `Running`, close fan-in, notify every blocking point, fix the
  saturated deadline), the completion wait (scan the fixed set, consume at most one
  token per entry, every wait bounded by the one deadline), the final synchronized
  observation (decide `Quiesced`/`Incomplete`, close the latch into the report — P8's
  one-critical-section discipline), then join-all in frozen order or detach-all.
- **Discharges:** `LIVE-SHUTDOWN`; `LIVE-COMPLETION`'s accounting reads; the live
  `ENV-SHUTDOWN` realization; `TRUST-SHUTDOWN`'s shipped conduct.
- **Proves:** `mod live_shutdown`: `one_deadline_fixed_at_initiation_governs_every_wait`
  (`LIVE-SHUTDOWN`; scripted clock), `quiesced_joins_every_supervised_thread`
  (`LIVE-SHUTDOWN`), `expiry_detaches_unjoined_threads_and_reports_incomplete`
  (`LIVE-SHUTDOWN`), `the_latch_stays_open_through_the_window` (`ENV-SHUTDOWN`),
  `a_completion_during_the_wait_ends_the_wait_promptly` (`LIVE-SHUTDOWN`).
- **Rust notes:** `Condvar::wait_timeout` in a loop against the remaining-time helper;
  do not cache a completion count — scan the fixed set (a cached count is a derivative
  invariant that can drift). The final observation is one critical section — every
  race is decided inside it, by design, which is what makes the race tests
  deterministic. Tricky part: never restart the deadline; C44's helpers make that
  structural.
- **Size:** ~160 lines (split signal: initiation + wait first, final observation +
  join/detach second — each half independently testable).

**C52 — `VERIFY-LIVE`, part 1: lifecycle, supervision, completion** *(§10-gated)*
- **Builds:** `tests/live_lifecycle.rs` + `tests/support` growth (explicit touch): the
  scripted Live Port doubles — `CuePort` (blocks on a cue channel; releases on
  command), `ErrPort`, `UnwindPort`, `PrematureOkPort` — plus the `tests/`-side
  scripted `MonotonicClock` (public trait, W5) and barrier helpers; the suite's
  gate/supervision/completion items.
- **Discharges:** `VERIFY-LIVE`'s first third; `TRUST-LIFECYCLE` and `TRUST-DRAIN`'s
  fixture checks.
- **Proves (representative):** `mod live_gate`:
  `no_run_begins_before_gate_activation` (`VERIFY-LIVE`),
  `failed_startup_cancels_and_joins_every_shell` (`VERIFY-LIVE`);
  `mod live_completion`: `normal_err_and_unwind_each_complete_exactly_once`
  (`VERIFY-LIVE`), `a_completion_before_shutdown_remains_visible_at_the_final_observation`
  (`VERIFY-LIVE`), `port_code_cannot_reach_the_terminal_guard` (`VERIFY-LIVE`; API
  surface demonstration).
- **Rust notes:** no test depends on sleep duration — barriers and the scripted clock
  control every boundary; never assert which side an overlap chooses — assert the
  returned result and the resulting state agree.
- **Size:** ~150 lines.

**C53 — `VERIFY-LIVE`, part 2: events, select, dispatch, bounds** *(§10-gated)*
- **Builds:** grows `tests/live_lifecycle.rs` (explicit touch): the
  `LIVE-EVENTS`/`LIVE-SELECT`/`LIVE-DISPATCH`/`ENV-BOUNDS` items under the scripted
  clock — capacity boundaries, admission order, stamp-vs-admission ordering,
  exhaustion-leaves-queued, exactly-once admission, no storage growth.
- **Discharges:** `VERIFY-LIVE`'s middle third; live `ENV-BOUNDS`; `TRUST-INBOX`'s
  capacity-exactness check.
- **Proves (representative):** `mod live_bounds`:
  `fan_in_and_inbox_occupancy_never_exceed_capacity` (`ENV-BOUNDS`),
  `completion_and_wakeup_storage_never_grows_past_one_per_slot` (`ENV-BOUNDS`);
  `mod live_select_suite`: `a_blocked_next_event_wakes_on_publication`
  (`VERIFY-LIVE`).
- **Size:** ~140 lines.

**C54 — `VERIFY-LIVE`, part 3: shutdown, deadline, races** *(§10-gated)*
- **Builds:** grows `tests/live_lifecycle.rs` (explicit touch): the shutdown third —
  signal-ahead-of-commands, window observability, `run(Ok)`-after-signal unpublished,
  `run(Err)`-before-close reported, saturation, no-join-while-outstanding, the two
  final-observation race classifications, `{ Incomplete, None }`,
  `{ Incomplete, Some }`, post-close discard, detach, per-Slot routing
  (`TRUST-ROUTING` for the suite's own wiring).
- **Discharges:** `VERIFY-LIVE` complete.
- **Proves (representative):** `mod live_shutdown_suite`:
  `a_port_blocked_in_recv_observes_shutdown_within_the_window` (`VERIFY-LIVE`),
  `error_plus_expiry_reports_incomplete_with_the_first_publication` (`VERIFY-LIVE`),
  `a_post_close_publication_is_discarded` (`VERIFY-LIVE`),
  `races_at_the_final_observation_are_classified_by_it` (`VERIFY-LIVE`; cue ports
  arrange each side), `a_joined_panicked_thread_is_quiesced_not_succeeded`
  (`VERIFY-LIVE`).
- **Size:** ~150 lines.

**C55 — `tests/latch.rs` (`VERIFY-LATCH`, both Environments)** *(§10-gated)*
- **Builds:** `tests/latch.rs`: the ordering-constraint suite run against both shipped
  Environments — before-call/after-return placement, overlapping publications accepted
  either way with result/state agreement, pending-beats-own-Error with the secondary
  discarded, the blocked `next_event` wake, permanence, final-Command sim observation,
  open-through-shutdown, racing-the-close, post-close discard, and the stop-path
  integration rows (`{Quiesced, None}` alone reaches `Stopped`; `Some(error)` →
  `Environment(Shutdown)` even with `Incomplete`; `{Incomplete, None}` →
  `Core(ShutdownIncomplete)`).
- **Discharges:** `VERIFY-LATCH`; with it, `ENV-LATCH` and `ENV-SHUTDOWN` gain their
  named suite. (Run against a bespoke Environment later, this same suite is
  `TRUST-ENV`'s certification — that reuse is why it lives in `tests/` with public-API
  doubles only.)
- **Proves (representative):** `mod latch_ordering`:
  `a_pending_error_wins_over_the_operations_own_failure` (`ENV-LATCH`),
  `a_blocked_next_event_returns_the_error_that_wakes_it` (`ENV-LATCH`);
  `mod latch_stop_path`: `only_a_clean_report_reaches_stopped` (`VERIFY-LATCH`),
  `a_report_error_is_environment_shutdown_even_with_incomplete` (`VERIFY-LATCH`).
- **Size:** ~150 lines.

**C56 — cross-type conformance (`DET-ENV`)** *(§10-gated)*
- **Builds:** grows `tests/conformance.rs` (explicit touch): equal-trace runs across
  Live and Sim over the expressible overlap, comparing every Core-owned discriminant
  and payload in `DET-ENV`'s list; Journal bytes equal through the last committed
  record; Environment-specific failure shapes explicitly named and excluded.
- **Discharges:** `DET-ENV`; `VERIFY-CONFORMANCE` complete; A9 now carries its full
  test weight.
- **Proves:** `mod conformance_cross_type`:
  `equal_traces_produce_equal_core_owned_outputs` (`DET-ENV`),
  `equal_traces_produce_equal_journal_bytes` (`DET-ENV`),
  `environment_specific_failure_shapes_are_not_compared` (`DET-ENV`).
- **Rust notes:** driving Live deterministically means cue ports delivering a scripted
  Event sequence — the trace, not the clock, is what must match. Compare
  `JournalError` variants/operations and every `CoreError` payload; never Error
  payloads across types.
- **Size:** ~130 lines.

### Phase 10 — Close

**C57 — export audit, docs, CI notes**
- **Builds:** grows `lib.rs` (explicit touch): the final W8 re-export audit (including
  the audit that no `#[doc(hidden)]` item exists); crate-level rustdoc with the
  finite-source example; a README-level pointer if wanted. Confirms the `TRUST-ABORT`
  deployment note (profiles already in `Cargo.toml`, F13; the CI build-profile check
  is deployment work, recorded here as a TODO for CI setup).
- **Discharges:** `CRATE-EXPORTS`.
- **Proves:** `mod crate_exports` (in `tests/exports.rs`):
  `every_public_item_is_reachable_without_repeated_segments` (`CRATE-EXPORTS`) — one
  `use`-list that fails to compile if a path regresses.
- **Size:** ~80 lines.

---

## 4. Suite build-out map

Every harness piece is built exactly once, at the chunk shown, and is permanent test
infrastructure (never scaffolding). Where a unit-level fixture and a `tests/support`
twin both exist (F7), both are listed; each is small and both are pinned by tests
citing the same IDs, so drift fails loudly.

| Harness piece | Built in | Used by |
|---|---|---|
| Source-local Journal fixtures (counting writer, failing `Serialize`, scripted sink) | C5, C7 | C5–C9 |
| Source-local failing-writer fixture in `engine/record.rs` | C18 | C18–C23 fatal paths |
| Source-local recording/scripted Environment (unit level) | C20 | C20–C28 |
| `tests/support`: `ScriptedEnv`, scripted sink, recording `Application`, golden helpers | C29 | C29–C34, C55, C56 |
| Compile-fail fixture crate + trybuild | C35 | C35 |
| Trace-recording `SimPort` double | C37 (promoted to `tests/support` in C42) | C37–C43, C55 |
| Source-local scripted clock (unit level, in `live/env.rs`) | C50 | C50–C51 |
| `tests/support` scripted `MonotonicClock` + barrier helpers | C52 | C52–C56 |
| Cue-controlled Live Port doubles (`CuePort`, `ErrPort`, `UnwindPort`, `PrematureOkPort`) | C52 | C52–C56 |
| Finite-source example Port | C43 | C43, C56, docs (C57) |

| `VERIFY-*` row | Discharged across | Complete at | Notes |
|---|---|---|---|
| `VERIFY-CONTEXT` | C10 (emit/overflow/reuse, in-file) + C28, C33 (State stands on every Fatal path, via Engine runs) | C33 | the split follows the row's own two halves |
| `VERIFY-JOURNAL` | C30, C31 (+ byte goldens seeded unit-level in C16–C18) | C31 | golden lines are byte literals |
| `VERIFY-FAULTS` | C32, C33 | C33 | includes the cross-product and the start-`Err`-no-shutdown proof |
| `VERIFY-CONFORMANCE` | C34 (within-type, scripted) + C43 (sim) + C56 (cross-type) | C56 | the suite is also `TRUST-ENV`'s bespoke certification vehicle |
| `VERIFY-GRAMMAR` | C35 | C35 | fed by F10's import discipline, started in C16 |
| `VERIFY-SIM` | C42 (unit groundwork C36–C41) | C42 | per-Port call traces |
| `VERIFY-LIVE` | C52, C53, C54 (unit groundwork C44–C51) | C54 | scripted clock + cue ports keep it deterministic |
| `VERIFY-LATCH` | C55 (state machine grounded in C14) | C55 | runs against both shipped Environments |

### Trusted obligations landed by the suites

The `TRUST-*` rows are obligations on people, not on Kavod code — but where a shipped
fixture or suite can *check* one for the code this plan builds, the check is planned
here, and where it cannot, that is said rather than faked.

| Obligation | Planned check |
|---|---|
| `TRUST-PURE` | C34/C43's twice-run machinery — checks the fixture Applications, and is the reusable recipe for user code |
| `TRUST-SIM-PORT` | C42's per-Port call traces + C43's repeatability, for the suite's own Sim Ports |
| `TRUST-ENV` | C34's graph-conformant scripted env is the certification vehicle; C55's suite doubles as the bespoke-Environment latch certification |
| `TRUST-ROUTING` | per-Slot routing tests on the suites' own wiring in C42 and C54 — the recipe every wiring author repeats |
| `TRUST-SERIALIZE` | C30–C31 exact bytes + C34 repetition, for the fixture payloads |
| `TRUST-SINK` | C9's memory-sink exactness; deployment freshness, exclusivity, and durability remain review |
| `TRUST-INBOX` | C53's capacity-exactness; deployment sizing remains config review |
| `TRUST-LIFECYCLE` | C52/C54 block a Port in `recv` and prove shutdown observation |
| `TRUST-DRAIN` | C52's `try_recv` drain-before-return fixture conduct |
| `TRUST-SHUTDOWN` | C51's shipped conduct (join-or-detach exactly as specified) |
| `TRUST-ABORT` | C1's profile lines (F13) + the C57 CI note; tests intentionally keep unwinding for the Live guard coverage |
| `TRUST-BLOCKING`, `TRUST-SPAWN`, `TRUST-MEMORY` | code/owner review — no shipped test can support them; not converted into fake tests |
| `TRUST-KEY`, `TRUST-SIZING` | application-specific per-Slot obligations — the wiring author's, with the C42/C54 tests as the pattern |
| `TRUST-EXIT` | operational review |

---

## 5. Risk list

The five places most likely to fight back, each with the symptom, the fallback, and
the probe that de-risks it.

1. **The certificate/Engine borrow seams.** *Symptom:* `E0382`/`E0505` walls when the
   loop re-binds the certificate, or when `close(env)` moves the Environment out of a
   loop other arms still borrow. *Fallback:* P1's exact shape — nested `match` arms
   with early `return`s, the marker-generic `effects` helper, `finalize` keyed on
   `Option<E>` — is proven; if a refactor fights, revert to P1's structure rather than
   reaching for `Rc`/`Option::take` tricks. *De-risked by:* P1 (executed, green).

2. **The `include!` fixture drifting from `record.rs`.** *Symptom:* C35's fixture stops
   compiling after a `record.rs` edit (an import outside the mirrored path set), or an
   attack case starts failing on privacy instead of the grammar. *Fallback:* F10's
   import discipline plus the fixture's `legal.rs` control case, which turns drift into
   a loud, attributable failure; the mirror stubs are one line per path. *De-risked
   by:* P6 (executed across two crates; both failure classes observed and separated).

3. **The Live shutdown lattice.** *Symptom:* a hanging `shutdown` test, or flaky
   races around the final observation. *Fallback:* the design's own Justify note is a
   complete one-lock realization — lifecycle, latch, and completion under one `Mutex`,
   one final critical section deciding every race — implement it verbatim before
   attempting anything cleverer; determinism in tests comes from cue ports and the
   scripted clock, never sleeps; never assert which side a race chooses, only that the
   returned result and the resulting state agree. *De-risked by:* P5 (gate, first-wins
   latch, saturating deadline) and P8 (publication-before-Complete and the final close
   as single critical sections), plus the C51 split signal keeping each half testable
   alone.

4. **`serde_json` boundary behavior.** *Symptom:* `BoundExceeded` misclassified as
   `Encode` (or vice versa), or golden bytes shifting. *Fallback:* the classification
   contract is pinned as unit tests from C3/C5 on, so any dependency bump that changes
   behavior fails loudly and locally; `Cargo.lock` pins the build, which `DET-RUN`
   already makes part of the determinism premise. *De-risked by:* P4 (executed:
   `io_error_kind()`, partial-write retry, `RawValue` newline, exact-capacity
   completion).

5. **`ports!` invocation-grammar edge cases.** *Symptom:* a wiring shape the macro
   won't parse (unexpected visibility, trailing comma, path-typed contracts).
   *Fallback:* hand-written sums are first-class by design — same names, variants,
   bytes — so no user is ever blocked on the macro; extend the matcher only with a
   failing test in hand. *De-risked by:* P2 (executed with the document's exact
   invocation, a reused Contract, and a `Never` direction) and C12's downstream
   consumer fixture.

---

*End of plan. Nothing in `src/` has been written. Next step: review section 1 —
approving 1a unblocks the §10-gated chunks (C37–C43, C47, C49–C56); everything else
can start immediately with C1. Once approved, the 1a answers are what design v13's
Wiring section should record (separate work), and `impl-plan.md`, `impl-plan-2.md`,
and `impl-plan-prompt.md` can be deleted — this document supersedes them.*
