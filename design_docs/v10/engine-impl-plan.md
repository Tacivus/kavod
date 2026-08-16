# Engine Implementation & Test Plan

> **Status:** Working plan for §8 of `design-final.md` (testing strategy revised after adversarial review, 2026-08-11)
> **Scope:** `Engine::run`, its record protocol, the engine's runtime assertion discipline, and the scripted test harness that proves both. Sim/live Environments come later and reuse this harness's doubles for the §10 conformance suite.
> **Strategy:** Four independent angles, so a bug must evade all of them simultaneously:
> 1. **Targeted cases** — small unit/integration tests, one per fault point and commitment point, asserting their exact expected observables inline (§5 matrix A–G, I–K).
> 2. **Universal properties** — a postcondition bundle every run passes automatically, plus exhaustive fault-enumeration sweeps checked against a spec-derived model (§4 bundle, §5 group H).
> 3. **Engine-side assertions** — always-on `assert!`/`expect` at every state the design proves unreachable, so a violated invariant aborts loudly instead of corrupting silently (§6).
> 4. **External instruments** — mutation testing, Miri, and an allocation tripwire, which audit the *suite* and the claims the tests can't express (§7).
>
> The Engine is tested exclusively against fully scripted fakes — scripted Application, scripted Environment, commit-granular scripted sink — sharing one chronological trace. Every failure the design's tables name is injectable at its exact call site, and a spec-derived model predicts the outcome of every single-fault run.

> **2026-08-15 shutdown amendment (design-final.md) — not yet integrated below.**
> The shutdown contract changed: `Environment::shutdown(self, timeout: Duration) -> bool`
> — no `ShutdownMode`, no `Err` channel; `true` = witnessed quiescence. `EngineConfig`
> gains `shutdown_timeout: NonZeroU64` (ms), `EngineExit::Fatal` gains `quiesced: bool`,
> `CoreFatal` gains `ShutdownTimeout`, and `EnvironmentOperation::ShutdownGraceful` is
> deleted. Ripples to fold in when implementation resumes: §3's pseudocode rows 7b/8b and
> `fatal_exit` (capture the bool from `shutdown`, reuse the Stop path's, `false` when never
> started); matrix rows E1/E4/E5 (shutdown-`false` is now `Core(ShutdownTimeout)` and E5's
> "no Abort" reads "skips `shutdown`, `quiesced: true`"); `Fault::ShutdownGraceful` becomes
> a shutdown-returns-`false` scripting knob; ScriptedEnv's shutdown script and
> `EnvShutdown` trace event drop the mode and record the received `timeout`; the model
> oracle predicts `quiesced`; group F asserts the bool per fatal path. The Journal/record
> protocol is untouched.

Notation used throughout: `RS` = RunStarted, `EA(i)` = EventAccepted at index i, `CP(i)` = CommandsPrepared, `CD(i)` = CommandsDispatched, `SR(i)` = StopRequested, `TC(i,o)` = TurnCompleted with outcome o. `J=` committed journal sequence, `Env=` environment call log, `H=` handler call log.

## 1. Conformance fixes found by review

Land these with (or before) the run() work — each is a divergence from normative text.

1. **`Engine::new` constructs the Journal before the command buffer** (`engine.rs:31-33`). §8.4's Construction table is normative about ordering: step 1 = `try_reserve` the batch → `CommandBuffer`, step 2 = Journal → `Journal`. Observable when both fail: the stub reports `Journal`, the design requires `CommandBuffer`. Swap, and pin with a both-fail test.
2. **`lib.rs` is missing `#![forbid(unsafe_code)]`** (§1 preamble, §9). Add it. While there: §9 sketches "public re-exports" — re-export the §§2–8 public surface flat (`Application`, `Outcome`, `Context`, `Engine`, `EngineConfig`, `EngineExit`, `FatalCause`, `CoreFatal`, `BuildError`, `EnvironmentFatal`, `EnvironmentOperation`, `JournalFatal`, `RecordKind`, `Environment`, `ShutdownMode`, `Journal`, `JournalError`, `JournalBuildError`, `SinkOperation`, `EventIndex`, `Timestamp`); `Never`/`PortContract` are already done.
3. **`Timestamp` doc overclaims** ("nanoseconds since Unix epoch"); §2.2 says opaque count with an Environment-owned origin. Fix the doc.
4. **`EnvironmentOperation::Dispatch` is missing §8.1's doc comment** (position = where the Error was observed in the dispatch loop, not necessarily this Command's own failure). Copy it verbatim — it is part of the normative signature block.

Verified clean: no failure type derives `Serialize` (design direction: Fatal is never journaled); record types below are separate serializable types that never embed failure values, keeping that structural. Extra `Debug`/`PartialEq` derives and receiver-convention differences are tier-3 free.

## 2. Production additions

### 2.1 `BoundedBuffer::drain`

`Environment::dispatch` takes `Command` by value; nothing today moves values out of the buffer. Add:

```rust
pub(crate) fn drain(&mut self) -> std::vec::Drain<'_, T>   // self.values.drain(..)
```

`Vec::Drain` gives the required semantics for free: owned values in insertion order; on early drop (the engine `return`s at a failed dispatch position) the un-yielded suffix is dropped exactly once, length returns to 0, logical capacity and allocation are untouched, buffer reusable next turn, zero allocation. Tests: full-drain order, DropProbe suffix-dropped-once on early drop, `as_ptr` stable across drain + reuse, empty drain.

The one ordering obligation at the call site: `CommandsPrepared` serializes `cmd_buf.as_slice()` and **commits before** `drain()` is created (A5). The borrows are sequential, so code order enforces it.

### 2.2 Record types (engine.rs, private)

Concrete Rust shape is tier-3; the serialized form is normative (§8.2). Serde's default externally tagged representation on one enum matches the record-name table exactly, and borrowed payloads avoid clones:

```rust
pub(crate) const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
enum Record<'a, E, C> {
    RunStarted { schema_version: u32, logical_time: Timestamp },
    EventAccepted { index: EventIndex, logical_time: Timestamp, event: &'a E },
    CommandsPrepared { index: EventIndex, commands: &'a [C] },
    CommandsDispatched { index: EventIndex },
    StopRequested { index: EventIndex },
    TurnCompleted { index: EventIndex, outcome: TurnOutcome },
}

#[derive(Serialize, Clone, Copy)]
enum TurnOutcome { Continue, Stop }
```

The enum is declared **without** inline `Serialize` bounds — `#[derive(Serialize)]` places `E: Serialize, C: Serialize` on the generated impl, so the type itself stays mentionable without bounds. Two construction consequences the listing in §3 accounts for: variants that never mention `E` or `C` (`RunStarted`, `CommandsDispatched`, `StopRequested`, `TurnCompleted`) leave the generics uninferable when passed to a generic commit helper, so every construction site uses the `Record::<A::Event, A::Command>::…` turbofish (lifetime omitted, which path syntax allows); and in `process_turn_result` the impl's bounds are discharged through `A: Application` (`A::Event: Serialize`, `A::Command: Serialize`).

Field order follows the §8.2 table; the golden test (§8 below) freezes the exact bytes, e.g. `{"RunStarted":{"schema_version":1,"logical_time":100}}`. A commit helper keeps every site honest about `JournalFatal` pairing:

```rust
fn commit_record<W: io::Write, R: Serialize>(journal: &mut Journal<W>, record: &R, kind: RecordKind)
    -> Result<(), JournalFatal>;
// journal.commit(record), with Err(error) → JournalFatal { record_kind: kind, error }
```

One structural consequence worth recording: because `Record` is an externally tagged enum, its serialization is *always* a JSON object regardless of payload behavior — a payload `Serialize` can fail (`Encode`) or overflow (`BoundExceeded`) but cannot make the outer record a non-object. **`JournalError::NotAnObject` is unreachable through the Engine.** The model treats it as such; the variant stays live for direct Journal consumers and is the Journal suite's job.

## 3. `run()` — structure, pseudocode, analysis

Two structural decisions carry the design; everything after them is ordered transcription of §8.4's tables.

**Free functions over disjoint locals.** `run(self)` destructures into locals (`journal`, `app`, `env`, `cmd_buf`, `max_turns`) on its first line, and every helper is a free function taking explicit `&mut` parameters — never a method on `Engine`. Two required borrow pairs (`Context` over `cmd_buf` during a handler; a live `Drain` of `cmd_buf` across `env.dispatch`) only check as disjoint locals and would jam forever behind `&mut self`.

**Environment lifecycle as `Option<E>`, wrapped only after successful `start`.** A `start` Err returns Fatal while `env` is still a plain local — plain drop, no Abort (§4.2 start row) — so "started ∧ unconsumed" is exactly `Some`, graceful shutdown `take()`s, and both `FAIL-FINALIZE` skip conditions are scope facts rather than flags.

### 3.1 Signatures

```rust
enum TurnFlow<AF, EE> { Continue, Stop, Fatal(FatalCause<AF, EE>) }

/// FAIL-FINALIZE steps 2–3: shutdown(Abort) iff `Some`, its Err discarded (§1.4).
/// Step 1 (fixing the primary cause) holds at every call site by construction:
/// each failure returns here immediately, so no later Error can precede it.
fn fatal_exit<S, AF, E: Environment>(state: S, cause: FatalCause<AF, E::Error>, env: Option<E>)
    -> EngineExit<S, AF, E::Error>;

/// §8.4 turn result, orders 1–9b; on_start and on_event both land here (A2).
fn process_turn_result<A, E, W>(
    index: EventIndex,
    outcome: Outcome<A::Fatal>,
    overflowed: bool,
    cmd_buf: &mut BoundedBuffer<A::Command>,
    journal: &mut Journal<W>,
    env: &mut Option<E>,
) -> TurnFlow<A::Fatal, E::Error>
where
    A: Application,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: std::io::Write;
```

### 3.2 Pseudocode

Step numbers are §8.4's own; annotations say only what the design tables don't — which local, which helper, which exit mapping.

```text
run(self):
  destructure self → journal, app, env, cmd_buf, max_turns    // all later borrows hit disjoint locals
  state = app.initial_state()                                 // Startup 1: before any fallible step,
                                                              //   so every exit path has state
  start_time = env.start()                                    // Startup 2
    Err → return Fatal{Environment(Start)} directly           //   env still plain local → dropped, no Abort
  env = Some(env)                                             // from here: started ∧ unconsumed ⇔ Some
  commit RunStarted{schema_version, logical_time:start_time}  // Startup 3; Err → fatal_exit (Abort runs)
  turn(index 0, start_time, on_start)                         // Startup 4, via the shared protocol:
    Continue → fall through  |  Stop → Stopped{state}  |  Fatal(cause) → fatal_exit(state, cause, env)

  accepted: usize = 0;  last_time = start_time
  loop:                                                       // Acquisition, §8.4
    1. accepted >= max_turns.get()
         → fatal_exit(Core(TurnBoundExceeded))                //   before next_event is called
    2. (event, time) = env.next_event()
         Err → fatal_exit(Environment(NextEvent))
    3. time < last_time                                       //   vs last ACCEPTED time; equal is valid
         → fatal_exit(Core(TimeRegression{previous:last_time, offered:time}))
                                                              //   candidate stays consumed, no handler
    4. index = index.checked_next()                           //   expect(): BOUND-INDEX invariant panic,
                                                              //   unreachable behind step 1 (see 3.3)
    5. commit EventAccepted{index, time, &event}              //   Err → fatal_exit; consumed, never current
       accepted += 1 (checked);  last_time = time             //   only after the commit succeeds
    6. turn(index, time, on_event(&event))                    //   same three-way match as the start turn

turn(index, time, handler):                                   // inlined at both call sites, or a closure —
  ctx = Context::new(index, time, &mut cmd_buf)               //   tier-3; the protocol call is what's fixed
  outcome = handler(&mut state, &mut ctx)
  overflowed = ctx.is_overflowed()                            // ctx's LAST use — ends its cmd_buf borrow
  process_turn_result(index, outcome, overflowed, &mut cmd_buf, &mut journal, &mut env)

process_turn_result(index, outcome, overflowed, cmd_buf, journal, env):
  1.  overflowed → clear batch; Fatal(Core(CommandBoundExceeded))   // beats any Outcome, incl. Fatal(f)
  2.  outcome is Fatal(f) → clear batch; Fatal(Application(f))
      staged = !cmd_buf.is_empty()                                  // captured BEFORE drain empties it
      if staged:
  3.     commit CommandsPrepared{index, cmd_buf.as_slice()}         // Err → Journal Fatal, zero dispatches
  4.     for (position, cmd) in cmd_buf.drain().enumerate():
             env.as_mut().dispatch(cmd)                             // Err → Fatal(Dispatch{position});
                                                                    //   return drops the Drain → suffix
                                                                    //   discarded exactly once (A3)
  5.     commit CommandsDispatched{index}                           // Err → Journal Fatal, handoffs stand
  6a. Continue → commit TurnCompleted(Continue)                     // Err → Journal Fatal (env Some →
        → TurnFlow::Continue                                        //   caller's fatal_exit aborts)
  6b. Stop → commit StopRequested{index}                            // Err → Journal Fatal,
                                                                    //   graceful never attempted
  7b.   env.take().shutdown(Graceful)                               // Err → Fatal(ShutdownGraceful);
                                                                    //   env now None, 2nd shutdown
                                                                    //   unrepresentable
  8b.   commit TurnCompleted(Stop)                                  // Err → Journal Fatal, Abort
                                                                    //   structurally skipped (None)
  9b.   TurnFlow::Stop                                              // caller returns Stopped{state}
```

Turn counting is `usize` against `max_turns.get()`, with `EventIndex` carrying the u64 separately: after the i-th accepted External Event, `index == i ≤ max_turns ≤ usize::MAX ≤ u64::MAX` — that inequality chain *is* the BOUND-INDEX unreachability argument, and no usize↔u64 conversion exists to get wrong. Batch-drop timing is tier-3: orders 1/2 (and optionally the order-3 failure arm) `clear()` eagerly for a uniform "discard" reading; a failed dispatch's suffix drops via the `Drain` guard; anything left drops with `run()`'s locals. Tests assert dropped-exactly-once-by-exit (bundle check U4), never timing.

### 3.3 Borrow and panic analysis

The load-bearing borrow facts — several design rules are *enforced* by these, not just permitted:

| # | Fact | Why it matters |
|---|---|---|
| B1 | Destructuring makes `journal`/`app`/`env`/`cmd_buf` disjoint locals | Every simultaneous-borrow pair below is between separate bindings; this is why no helper may become an `Engine` method |
| B2 | In the start-Err arm, `env` is still a plain `E` dropped at scope end | The no-Abort rule for start failures is unrepresentable to violate — no flag to forget |
| B3 | `ctx`'s `&mut cmd_buf` ends at `ctx.is_overflowed()`, its last use | NLL ends borrows of non-`Drop` types at last use; `process_turn_result` may then reborrow. A later `Drop` impl on `Context` breaks this — flagged in 3.4 |
| B4 | `TurnFlow` has no lifetime parameter | The returned value borrows nothing, so the caller's match arms may move `state` and `env` into exit constructors; a borrowed variant would stop every fatal arm compiling |
| B5 | `env.as_mut()…next_event()` is a scrutinee-position borrow of an owned `Result` | The borrow dies before the arms run, so the Err arm may move `env` into `fatal_exit` — legal only because `Environment` methods return owned data |
| B6 | The CP record's `as_slice()` borrow (shared) ends at its commit call, before `drain()` (mut) | A5's serialize-before-handoff is compiler-enforced at this site: holding the CP record across the drain cannot borrow-check |
| B7 | The dispatch loop holds `&mut cmd_buf` (Drain) and `&mut env` together | Legal as two free-function parameters; the early `return` drops the live `Drain`, dropping the suffix exactly once and leaving the buffer empty with capacity intact |
| B8 | 7b's `env.take()` moves `E` out through `&mut Option<E>` | An 8b commit failure reaches a caller whose `env` is `None`; `fatal_exit` skips Abort structurally |

Invariant panics (A8; each provably unreachable in a correct Engine): `checked_next` (behind acquisition step 1, inequality above); the checked `accepted` increment (runs only under `accepted < max_turns.get()`); `env.as_mut()` in dispatch and acquisition (`None` arises only at 7b, whose every continuation returns before another dispatch or acquisition); `env.take()` at 7b (first and only consumption point on any path). §6 turns this list into the engine's assertion discipline.

Conformance is 1:1 by construction — the pseudocode is numbered by §8.4's own steps. The non-obvious mappings: state precedes every fallible step, so `EngineExit` always carries it (finalization step 3); order 1 sits before the `outcome` match, so overflow beats `Outcome::Fatal` (test C5); 6a's commit gates the loop, realizing "only success permits the next acquisition"; no commit site is reachable after any `FatalCause` is constructed, realizing "never writes to the Journal again"; `ENV-CALLS` is structural per §4.4 (start once pre-wrap, serial loop, shutdown ≤ once via B2/B8).

### 3.4 What inspection cannot prove

The above is by-inspection only. These residual claims need `cargo check`, so the implementation phase's first milestone is a clean check of the transcription **before** any test lands:

1. `Record` variants that mention neither generic (`RunStarted`, `CommandsDispatched`, `StopRequested`, `TurnCompleted`) need the `Record::<A::Event, A::Command>::…` turbofish at a generic `commit_record` call (§2.2) — required and sufficient; likewise `process_turn_result::<A, E, W>`, since `A` appears only in non-injective associated-type positions.
2. `Record`'s derive-generated `Serialize` bounds are discharged through `A: Application` inside `process_turn_result` (§2.2).
3. The NLL region endings in B3/B5/B6 behave as claimed — standard for non-`Drop` types, but they're exactly where a refactor (a `Drop` impl on `Context`, a borrowing return on an `Environment` method) would surface as new compile errors.
4. Generic inference at `fatal_exit`/`commit_record` call sites resolves without further annotation.

## 4. Test harness (`src/test_support.rs`, `#[cfg(test)] mod` in lib.rs)

The doubles are crate-shared test-only code because the future sim/live conformance suite (§10) reuses them. Engine tests themselves stay in `engine.rs`'s `#[cfg(test)] mod tests` per convention. journal.rs's byte-granular `ScriptedWriter` stays where it is — the engine-level sink below is commit-granular; partial-write behavior (including single-byte dribble, Interrupted mid-loop, and oversized write claims) is the Journal suite's settled job, and its `single_byte_writes_complete_the_record_before_flush` test already proves the stack is insensitive to sink write granularity. The commit-granular assumption below is sound *because* `TraceSink` always accepts full writes.

Note: `panic = "abort"` in the build profiles is ignored by Cargo for test targets, so `catch_unwind`-based tests work — and matrix group J depends on that.

**Shared chronological trace — the load-bearing mechanism.** One `Trace(Rc<RefCell<Vec<TraceEvent>>>)` cloned into every double, including the sink *and the payload `Serialize` impls*. Cross-component ordering (A5) collapses into subsequence/equality checks over one linear log. Trace entries carry payload **ids, never payload values** — that is what lets the payload types stay `!Clone` (see below):

```rust
enum TraceEvent {
    InitialState,
    EnvStart { ok: bool },
    OnStart { index: EventIndex, time: Timestamp },
    OnEvent { index: EventIndex, time: Timestamp, event_id: u32 },
    Emitted { command_id: u32 },
    RemainingObserved(usize),
    Serialized { kind: PayloadKind, id: u32 },   // logged by the payload Serialize impls
    EnvNextEvent { ok: bool },
    EnvDispatch { command_id: u32, ok: bool },
    EnvShutdown { mode: ShutdownMode, ok: bool },
    EnvDropped,                                  // ScriptedEnv::drop — distinguishes drop from shutdown
    SinkWrite { record: String },                // record = externally-tagged name parsed from the line
    SinkWriteFailed { record: String },
    SinkFlush { record: String, ok: bool },
}
```

Small scripts assert the *entire* trace with `assert_eq!` (the strongest ordering claim, matching journal.rs's exact `write_inputs` style). Larger runs use a forward-scan checkpoint matcher (`Trace::assert_ordered(&[Checkpoint])`) that pretty-prints the trace on mismatch.

**Payload types — deliberately hostile to engine misbehavior.** Three properties are structural, so the type system proves them before any assertion runs:

- **`!Clone`, `!Copy`.** `TestEvent` and `TestCommand` implement neither. The engine needs no clone anywhere (CP serializes `&[C]`; dispatch drains by value), so the harness *compiling* proves the engine never duplicates an Event or Command — half of "dispatched exactly once" discharged at the type level.
- **Drop-probed.** `TestCommand { id: u32, probe: DropProbe, trace: Trace }` where `DropProbe` increments a shared drop counter. Every run — not just dedicated sweeps — gets created == dropped-exactly-once accounting (bundle check U4).
- **Serialize-traced and fault-injectable.** Hand-written `Serialize` emits exactly what derive would (`{"id":N}`, plus `,"note":"…"` when the optional hostile-content field is `Some` — `None` omits the field so goldens are unaffected), logs `Serialized { kind, id }` to the trace, and returns `Err(custom)` when `id == u32::MAX` — folding encode-failure injection (`JournalFatal { EventAccepted, Encode }`, `{ CommandsPrepared, Encode }`) into the same scripted world. The trace log is what proves each payload is encoded **exactly once** per commit and that encoding strictly precedes the batch's first dispatch (A5 at the serialization layer, not just the sink layer).

`A::Fatal = TestFatal { id: u32, sentinel: &'static str, probe: DropProbe }` — probed so a *swallowed* Fatal payload (C5) is provably dropped exactly once, and sentinel-carrying (next paragraph).

**Sentinels — the leak dragnet.** Every scripted failure value carries a globally unique `"SENTINEL-…"` string: `EnvError` variants embed `&'static str` sentinels, sink faults construct `io::Error::new(kind, "SENTINEL-SINK-n")`, `TestFatal` carries one. Bundle check U3 scans every run's sink bytes for the substring `SENTINEL` — turning "no failure value is ever serialized" (§1.3/§1.4) from a per-case assertion into an automatic property of the entire suite, including tests not yet written.

**ScriptedApp.** Per-turn script popped from `RefCell<VecDeque<ScriptTurn>>` (handlers take `&self`): `ScriptTurn { actions: Vec<HandlerAction>, outcome: OutcomeSpec }` where actions are `Emit(id)` / `ObserveRemaining` and `OutcomeSpec` is `Continue | Stop | Fatal(TestFatal) | Panic` (`Panic` serves matrix group J). A dry script **panics** — faults can only truncate a run, never extend it, so an extra handler call is an engine bug caught immediately. `initial_state_calls: Rc<Cell<usize>>` survives `run()` consuming the app; `State = ScriptedState { handled: Vec<EventIndex> }` is the APP-STATE/A3 oracle, read from `EngineExit` — state mutation surviving Fatal needs no side channel.

**ScriptedEnv.** `start: Option<StartAction>` (`take()`n, so a second start panics — ENV-CALLS), `next_events: VecDeque<NextEventAction>`, `dispatch_faults: VecDeque<DispatchAction>` (dry = Ok), `shutdown_fault: Option<EnvError>`; each `*Action` is `Ok(..) | Err(EnvError) | Panic` so group J can inject panics at any env call site. `EnvError` is a tagged Copy+PartialEq enum (sentinel `&'static str` inside) so each scripted failure is distinguishable in exit assertions. `shutdown` consumes self, so mode/result observation flows only through the trace — which also makes "shutdown at most once" structurally checked. `Drop` logs `EnvDropped`, which is what lets group J assert "dropped, not shut down" during unwind.

**TraceSink.** Commit-granular scripted sink: because the Journal writes one complete buffered line per commit, each `write` call is exactly one record — fault indexing by commit ordinal is exact, and `SinkWrite { record }` is well-defined. Faults per commit: `WriteError(kind)` | `WriteZero` (`Ok(0)`) | `FlushError(kind)` (accept bytes, fail the flush) — `kind` is parametric, so `Interrupted` is covered without a dedicated variant. Bytes accumulate in an `Rc<RefCell<Vec<u8>>>` handle for golden assertions.

**One driver.** `run_script(script, fault, aftermath) -> RunObservation` builds fresh doubles (splicing the single fault into the right queue), builds the Engine, runs it, harvests every handle, and **always applies the universal bundle** before returning, so every targeted test inherits it for free:

```rust
struct RunScript { config, start_time, turns: Vec<ScriptTurn> /* [0] = start */, events: Vec<(EventSpec, Timestamp)> }
enum Fault { None, Start, NextEvent(usize), Dispatch(usize), Commit(usize, CommitFault), ShutdownGraceful }
enum Aftermath { Clean, AllFail }   // AllFail: after the primary fault, every queue is poisoned — sweep H5
struct RunObservation { exit, trace: Vec<TraceEvent>, bytes: Vec<u8>, initial_state_calls: usize, drops: DropCounts }
```

**Universal postcondition bundle** — run against every observation; each check cites the tier-1/2 text it enforces:

| # | Check | Enforces |
|---|---|---|
| U1 | Record grammar: committed sequence matches `RS · (EA(i) [CP(i) [CD(i)]] (SR(i) [TC(i,Stop)] | TC(i,Continue)))*` with a possibly-truncated tail, indices consecutive from 1, CD only after same-index CP, TC(Stop) only after same-index SR, nothing after TC(Stop), RS first and only first | §8.2 |
| U2 | Journal-prefix property: for a *result-channel* fault (`Start`, `NextEvent`, `Dispatch`, sink-level `Commit`, `ShutdownGraceful`), committed bytes are a byte-exact prefix of the same script's fault-free run, ending at the record boundary the model predicts. Content-level faults (encode ids, oversized payloads, regressing timestamps) change the script itself and are excluded — the model and targeted rows cover them | §1.3 determinism + §8.2 |
| U3 | Sentinel scan: sink bytes never contain `SENTINEL` | §1.3/§1.4 — no failure value serialized |
| U4 | Drop accounting: every created `TestCommand`/`TestFatal` dropped exactly once by exit | A3 + no leak/double-drop |
| U5 | `initial_state_calls == 1` for every run that constructed | APP-STATE |
| U6 | Every `SinkWrite` immediately followed by its `SinkFlush`, nothing between | flush-per-record, §1.5's evidence claim |
| U7 | Auditor (below) agrees with the trace | §8.2's forensic claim |

**Auditor — the forensic oracle.** §8.2 claims the evidence alone reconstructs the run: "`CommandsPrepared` plus the typed dispatch position in `EngineExit` identifies the exact successful prefix." Make that sentence executable: `reconstruct(bytes, exit) -> Evidence` parses the committed journal plus the typed exit and derives, per turn: handled indices, and the dispatched-command prefix (CP + CD present → all; CP + exit `Dispatch{position:k}` at that index → first k; CP + exit `Journal(CommandsDispatched)` → all; CP absent → none). U7 compares the reconstruction against the env's actual receive log in every non-panic run. If reconstruction and reality can diverge anywhere, the design's evidence story is broken — this is the test that would catch it.

**Model oracle.** `predict(script, fault) -> Prediction` is a straight-line transliteration of §8.4's four tables — each branch commented with its table row — predicting discriminants only, never JSON bytes: committed record names, `ExitShape` (exit with `io::Error` payloads reduced to discriminants), dispatched command list, handled indices, `remaining()` observation values, `next_event` call count, `Option<ShutdownMode>`. Three counters (commits, dispatches, next_events) are checked against the single `Fault` before each modeled operation. `NotAnObject` is modeled as unreachable (§2.2). Byte-level concerns (`max_record_bytes` overflow) are deliberately outside the model and covered by dedicated tiny-bound tests — that's what keeps the model small enough to be obviously correct.

**Oracle audits — the suite's own trust boundary.** A model transliterated from the same tables the engine was implemented from is a common-mode failure risk: both can share one misreading, and the sweeps then pass vacuously. Three defenses, all mandatory:

1. **Matrix calibration.** Every targeted case in groups A–E also calls `predict()` and asserts the model agrees with the hand-written expectation *before* asserting against the actual run. The matrix was derived by a human reading the spec; the model mechanically; disagreement between them is exactly where a spec misreading lives.
2. **Known-bad fixtures.** The grammar checker is unit-tested against corrupted sequences (CD without CP, nonconsecutive indices, a record after TC(Stop), RS not first, duplicate RS); the auditor against fabricated journal/exit pairs; the prefix and sentinel checks against synthetic violations. A checker that has never failed is not known to be able to.
3. **Structured-diff comparison helpers.** The determinism comparison (G5) and trace comparison return `Option<Divergence>` rather than asserting internally — normal tests assert `None`; group K asserts `Some` against deliberately violating components, proving the detectors have teeth.

**Panic-origin convention.** Harness dry-script and misuse panics use messages prefixed `test-double:`; engine invariant panics use the §6 convention (`kavod invariant violated: <ID>: …`). Any sweep that observes a panic can therefore classify it — a `test-double:` panic means the engine made a call the script didn't authorize (itself a bug signal), and a `kavod` panic in any test is an automatic, correctly-attributed failure.

## 5. Test matrix

Every commitment point in §8.2/§8.4, every injectable failure, expected observables. Cases marked ⚑ are the adversarial interactions the tables imply but don't spell out. Every case below is a **targeted layer-1 test**: it asserts its own expected observables inline (exit shape and payload values, journal sequence, trace order) *and* inherits the universal bundle via `run_script` *and* calibrates the model (§4). Redundancy between a named case and a sweep instance is deliberate — the named case localizes a failure to one spec row in seconds; the sweep proves the row wasn't the only place that could break.

### A. Construction

| # | Case | Expected |
|---|---|---|
| A1 | happy | `Ok`; zero app/env/sink calls (`initial_state` not yet called) |
| A2 | cmd buffer unreservable | `Err(CommandBuffer)` |
| A3 | `max_record_bytes = usize::MAX` / huge-but-valid | `Err(Journal(MaxBytesTooLarge))` / `Journal(AllocationFailed)` |
| A4 ⚑ | both fail | `Err(CommandBuffer)` — pins the §1 ordering fix |
| A5 | construct then drop without run | no app/env/sink calls ever |

### B. Startup

| # | Case | Expected |
|---|---|---|
| B1 | happy order | `InitialState` → `EnvStart` → RS bytes → `OnStart`, exactly once each, in the trace |
| B2 | `start` Err | `Fatal(Environment(Start))`; `J=∅`; `H=[]`; **no** `shutdown(Abort)`; exit state = initial state |
| B3 | RS commit: Write err / Flush err / BoundExceeded (tiny bound) | `Fatal(Journal({RunStarted, Sink{Write}}))` etc.; `on_start` never runs; `Env=[start, shutdown(Abort)]`; committed `J=∅` |
| B4 | RS content | first line = `{"RunStarted":{"schema_version":1,"logical_time":<start ts>}}`; every nonempty journal begins with RS |
| B5 | Context at index 0 | `ctx.index()==0`, `ctx.logical_time()==start ts` inside `on_start` |

### C. Turn-result protocol (test at index 0 *and* at an event turn — journal prefix and Abort context differ)

| # | Case | Expected |
|---|---|---|
| C1 | Continue, empty batch | `J=[.., TC(i,Continue)]`, no CP/CD, no dispatch |
| C2 | Continue, n commands | `J=[.., CP(i), CD(i), TC(i,Continue)]`; dispatches in emit order, strictly between CP bytes and CD bytes in trace; each command's `Serialized` entry appears exactly once, before the first dispatch |
| C3 | exactly-capacity emit | no overflow; all dispatched |
| C4 | `Outcome::Fatal(f)` with staged commands | `Fatal(Application(f))` with `f`'s exact id; batch discarded undispatched, each command dropped exactly once; Abort |
| C5 ⚑ | overflow + Continue / + Stop / + `Fatal(f)` | all three → `Fatal(Core(CommandBoundExceeded))` — order 1 beats the Outcome; the swallowed `f` is **drop-probed**: dropped exactly once, not leaked; no CP, no dispatch, no SR/graceful; Abort |
| C6 | CP commit: sink err | `Fatal(Journal(CommandsPrepared))`; **zero dispatches**; Abort |
| C7 ⚑ | Command `Serialize` Err at CP | `Fatal(Journal({CommandsPrepared, Encode}))`; zero dispatches, zero sink bytes for CP |
| C8 ⚑ | CP BoundExceeded (batch over `max_record_bytes`) | same shape — BOUND-SIZING violation surfaces as Journal Fatal, not construction error |
| C9 ⚑ | dispatch Err at position k (test k=0, middle, n−1) | `Fatal(Environment(Dispatch{position:k}))`; env received exactly `[0,k)` in order; `J=[.., CP(i)]`, no CD; all n commands dropped exactly once; Abort |
| C10 | CD commit err | all n dispatched, then `Fatal(Journal(CommandsDispatched))`; Abort |
| C11 | TC(Continue) commit err | `Fatal(Journal(TurnCompleted))`; Abort runs; no further `next_event` |
| C12 | batch isolation across turns | turn 1 emits [A], turn 2 emits [B] → CP(2) contains only [B]; includes reuse after a full-capacity turn |
| C13 ⚑ | `remaining()` semantics | via `ObserveRemaining`: counts down by exactly 1 per emit; equals 0 at exact capacity without overflow; equals 0 after the overflow marker is set (§2.1 doc claim, previously untested) |

### D. Acquisition

| # | Case | Expected |
|---|---|---|
| D1 | happy order per turn | bound check → `EnvNextEvent` → EA bytes → `OnEvent`, with the exact Event id passed through |
| D2 | `max_turns=1` exhaustion | `Fatal(Core(TurnBoundExceeded))`; env log shows exactly **one** `next_event` — bound checked first |
| D3 | `max_turns` counts external only | `max_turns=2`: start turn + 2 event turns complete; bound trips on 3rd acquisition |
| D4 | `next_event` Err | `Fatal(Environment(NextEvent))`; `J` ends at previous TC; no handler |
| D5 ⚑ | regression vs start time (first event) | `Fatal(Core(TimeRegression{previous: start_ts, offered}))`; candidate consumed, never re-requested; no EA; `H=[on_start]` only |
| D6 | regression vs prior event | `previous` = last *accepted* time |
| D7 | equal timestamps | accepted; normal turn |
| D8 | EA commit: sink err / Event `Serialize` Err ⚑ / BoundExceeded | `Fatal(Journal(EventAccepted))`; candidate consumed, `on_event` **never** invoked; Abort |
| D9 | EA content + index sequence | `{"EventAccepted":{"index":i,"logical_time":t,"event":…}}`; indices 1,2,3… consecutive; ctx agrees |
| D10 ⚑ | timestamp extremes | accept at `u64::MAX`; repeated equal turns at `u64::MAX`; regression `MAX → 0` (widest) and `prev → prev−1` (narrowest); regression after an equal-time run (`previous` = the equal accepted time). The engine only *compares* timestamps — these prove no arithmetic sneaks in |

### E. Stop path

| # | Case | Expected |
|---|---|---|
| E1 | Stop, empty batch (also at index 0) | `J=[.., SR(i), TC(i,Stop)]`; `Env=[.., shutdown(Graceful)]`; `Stopped{state}`; **zero further `next_event`** (at index 0: zero ever) |
| E2 | Stop with commands | dispatches and CD precede SR precede graceful precede TC(Stop) in the trace (A5) |
| E3 ⚑ | SR commit err | `Fatal(Journal(StopRequested))`; **graceful never attempted**; Abort runs instead |
| E4 ⚑ | graceful Err | SR committed; `Fatal(Environment(ShutdownGraceful))`; no TC; env consumed → no second shutdown call |
| E5 ⚑ | TC(Stop) commit err after successful graceful | `Fatal(Journal(TurnCompleted))` despite graceful success; **no Abort** (consumed); `J` ends at SR |
| E6 ⚑ | full protocol entirely at index 0 | start turn emits commands then Stops: `J=[RS, CP(0), CD(0), SR(0), TC(0,Stop)]`; graceful; `Stopped`; `next_event` never called in the entire run |

### F. Fatal finalization (cross-cutting, swept over every fatal case above)

| # | Property |
|---|---|
| F1 ⚑ | A4: re-run every Abort-running case with `shutdown(Abort)` scripted to Err → identical exit, Err invisible; the discarded Abort error's payload is drop-probed — dropped, not leaked |
| F2 | Abort ran **iff** env started ∧ unconsumed: start-Err → none; E4/E5 → none; all others → exactly one |
| F3 | no journal writes after primary failure (sink call count frozen; the poisoned-Journal invariant panic would abort the test on violation) |
| F4 | no handler/`next_event`/`dispatch` after the failing call (trace tail check) |
| F5 | A3: handler mutates state then triggers each failure class → exit state shows the mutation |
| F6 | at most one shutdown call per run (double panics on a second) |

### G. Record protocol & determinism

| # | Property |
|---|---|
| G1 | golden journal: fixed multi-turn script → byte-exact inline JSONL, freezing tag names, field order, `outcome` rendering, `schema_version` |
| G2 | structural companion: parse each committed line, assert tag + exact field-name set per §8.2 — disambiguates bytes-vs-schema drift |
| G3 | grammar checker runs on every test's journal (bundle U1) |
| G4 | fatal never journaled: no record kind outside the six, no failure payloads in bytes, in every fatal case (bundle U3 generalizes this to every run) |
| G5 | determinism: same script twice (fresh doubles) → identical bytes, identical trace, equal exit shape — also under each fault. The comparison helper returns `Option<Divergence>` (group K depends on that) |
| G6 ⚑ | irrelevant-config invariance: same script under two different-but-sufficient `max_record_bytes`, and under `Vec` sink vs `io::sink()` → identical trace and exit (bytes compared where observable). Config that shouldn't matter provably doesn't |

### H. Sweeps — exhaustive enumeration, not sampling

The single-fault space per script is finite: every commit ordinal × three sink fault kinds, every env-call ordinal × Err, plus Start and ShutdownGraceful. So the sweeps enumerate it **exhaustively and deterministically** over a fixed script *family* — randomized sampling is an overnight supplement, never the CI gate.

**Script family** (each exercises a different protocol shape): `representative` (start turn with commands, an empty-batch turn, a multi-command turn, equal-timestamp turns, terminal Stop — every record kind), `stop_at_zero` (E6's shape), `continue_heavy` (many empty and small turns, equal times throughout), `full_capacity` (every turn emits exactly `max_commands_per_turn`).

| # | Sweep |
|---|---|
| H1 | **fail-the-Nth-commit**: for each family script, count commits T from the happy trace; for each n < T × {WriteError, WriteZero, FlushError} → model comparison + bundle (U2 prefix property bites here). Count-then-sweep guarantees no commit point is silently skipped |
| H2 | **fail-the-Nth-env-call**: every `next_event` and `dispatch` ordinal, plus Start and ShutdownGraceful, for each family script → model comparison + bundle |
| H3 | drop accounting is bundle check U4, on every run automatically; the only dedicated cases left are C5's swallowed-Fatal probe and F1's discarded-Abort-error probe |
| H4 | seeded random scripts (outcomes, emit counts, timestamps incl. equal/regressing/extremes, fault points) vs model — optional overnight instrument once H1/H2/H5 pass; a cargo-fuzz target reusing the same bundle is the long-running form |
| H5 ⚑ | **scorched earth (A4 as a cascade, not a single fault)**: re-run every H1/H2 primary fault with `Aftermath::AllFail` — every subsequent operation scripted hostile (sink always fails, every env call errs, shutdown errs). Exit must be bit-identical to the single-fault run and the trace tail must show the engine attempted *only* `shutdown(Abort)` after the primary. Strictly stronger than F1: a buggy engine that makes even one extra journal or env call after the primary is caught by outcome, not just inspection |
| H6 ⚑ | **config grid**: H1/H2 family scripts × `max_commands_per_turn ∈ {1, exact-fit, large}` × `max_record_bytes ∈ {exact-fit, huge}` × `max_turns ∈ {1, 2, large}`. Capacity 1 with a 2-emit turn is the minimal overflow; exact-fit bounds pin BOUND-SIZING's failure shape at the engine level |

### I. Adversarial payloads — hostile content *through* the engine

The journal suite tortures encoding mechanics; nothing yet pushes hostile content through the full engine path. Uses the payload `note` field (§4).

| # | Case | Expected |
|---|---|---|
| I1 ⚑ | **JSONL injection**: event note contains `"}\n{"RunStarted":{"schema_version":1,…` | line count == commit count; every line parses; the injected text appears escaped *inside* the `EventAccepted` record; grammar checker still sees exactly one RS. This is an attempted journal forgery via payload — the forensic property §8.2 leans on |
| I2 | escaping and unicode: raw newlines, quotes, backslashes, control characters, astral-plane unicode, empty string | valid JSONL; golden pins the escaped bytes; bound counts encoded bytes (journal suite already proves the counting; this proves the engine path) |
| I3 | numeric extremes in records | golden with `logical_time` = `u64::MAX` and a large `index` — pins that no i64 truncation sneaks into serialization |

### J. Panic characterization (A8/§1.5 as executable claims)

Tests run unwinding (Cargo ignores `panic = "abort"` for test targets), which is exactly what lets these pin the *no-catch* claims. Each case wraps `run()` in `catch_unwind` at the test level and asserts: the panic propagates out of `run()` uncaught — no `EngineExit` is produced for it; the sink contains exactly the committed prefix at the moment of panic (flush-per-record makes it current — §1.5's evidence claim); **no** shutdown occurs during unwind (trace shows `EnvDropped` without `EnvShutdown`); no sink operation follows the panic. These tests also guard against someone later "helpfully" adding a `catch_unwind` or a `Drop`-based shutdown guard — both forbidden by design, both caught instantly here.

| # | Panic site |
|---|---|
| J1 | handler (`on_start` and `on_event`) |
| J2 | `initial_state` — additionally: env never started, no journal bytes, no env call at all |
| J3 | a payload `Serialize` impl, mid-commit |
| J4 | an env method (`next_event`, `dispatch`) |

### K. Obligation-violation detectors — §10 needs teeth

§10's "Verified by" column claims golden tests catch unstable `Serialize` and repeatability tests catch hidden authority — but nothing verifies the verification *means* can actually detect a violation. These tests run the harness's comparison helpers against deliberately violating components and assert `Some(divergence)`:

| # | Violation | Detector that must fire |
|---|---|---|
| K1 | `Cell`-stateful `Serialize` emitting different bytes per call | G5's byte comparison |
| K2 | handler reading mutable state outside `State` (a shared counter) | G5's repeatability comparison |

This matters beyond the engine: the sim/live conformance suite (§10) will rely on exactly these detectors, so their sensitivity is proven before anything depends on it.

### Anti-goals — what the suite must NOT assert

Torture must not calcify tier-3 freedom into accidental spec. Every blanket check cites tier-1/2 text (the U-table does); anything that can't is forbidden. Specifically out of bounds:

- **Drop timing** (assert dropped-exactly-once-by-exit, never when) — already the rule; stated here as policy.
- **Sink write-call granularity** at the engine level — the journal suite owns partial-write behavior.
- **Allocation identity or buffer pointers** beyond the specific claims §2.4 of the design makes (the §7 tripwire checks *counts*, not addresses).
- **Invented Environment obligations.** The engine owes nothing about duplicate or stale candidates — a scripted env offering the same event twice gets two normal turns, and a test asserting otherwise would test a rule that doesn't exist.
- **Batch-drop eagerness, `clear()` placement, closure-vs-inline `turn()`** — tier-3 per §3.2.

## 6. Engine assertion discipline (NASA PoT / TigerBeetle style, bounded by A8)

The design already draws the line the assertions must respect: A8 says a panic is a *bug*, aborting the process with no `EngineExit` — so an `assert!` may guard only states the design proves unreachable, never conditions the spec maps to typed values. Rules, in priority order:

- **R0 — unrepresentable beats asserted.** Where structure already makes violation impossible (B2's no-Abort-after-start-Err, B8's second-shutdown, `ENV-CALLS` serialization), add nothing: an assert there would imply the state is reachable. Prefer unrepresentable > asserted > tested.
- **R1 — typed paths are never asserts.** Any condition §8.1/§8.4 maps to `BuildError`, `JournalError`, or `FatalCause` must remain a typed value: time regression, command overflow, turn-bound exhaustion, every env/journal/application failure. An assert here would convert a specified Fatal into a process abort — a conformance bug, not defensiveness.
- **R2 — proven-unreachable states get always-on panics citing their invariant.** §3.3's list is the seed, realized as `expect`/`unreachable!` with messages naming the argument: `checked_next` (`BOUND-INDEX`), the checked `accepted` increment (guarded by acquisition step 1), `env.as_mut()` in dispatch/acquisition (§3.3), `env.take()` at 7b (§3.3), and the Journal's existing poisoned-commit assert (`FAIL-FINALIZE`).
- **R3 — cheap positive-space pre/postconditions on Kavod-owned state, always-on when O(1) and allocation-free.** Derived sites: at every handler invocation, `assert!(cmd_buf.is_empty())` and the overflow marker is clear (§2.4's "reused every turn, marker cleared at each invocation"); after each discard path, the batch is empty; inside `Context`, marker-set ⇒ `remaining() == 0` (§2.1); optionally, an O(1) last-record-kind successor check at `commit_record` mirroring the U1 grammar (§8.2) — the one place deliberate redundancy with the test oracle is worth having inside the shipping engine, TigerBeetle-style. Anything costlier than O(1) is `debug_assert!`.
- **R4 — asserts are never control flow.** Deleting every assert must leave a correct engine's observable behavior unchanged. Corollary for §7: `cargo-mutants` mutants that merely remove an assert are *expected* survivors — record them as reviewed, don't chase them.
- **R5 — message convention.** `kavod invariant violated: <INVARIANT-ID>: <state>` — machine-classifiable against the harness's `test-double:` prefix (§4), so any panic in any run is attributed instantly.

PoT's "minimum two assertions per function" is the aspiration, filtered through R1: never manufacture an assert that shadows a typed path just to hit a count.

**Testing obligations the asserts create.** Every assert is a claim, and claims get tested from both sides: (a) *unreachability* — no public-API path may fire one, which the full suite plus H-sweeps checks automatically since any panic fails its test with an attributable message; (b) *sensitivity* — where an assert is reachable at unit level, a direct test proves it fires (`EventIndex::checked_next` at `u64::MAX` via `catch_unwind`; the Journal's poisoned-recommit panic — already tested; `Context`'s marker invariant via a `test_support`-level poke if one exists). An assert nothing can ever fire and nothing ever tests is dead weight; each one lands with its side-(b) test or a one-line note that only (a) applies.

## 7. External instruments — auditing the suite itself

| Instrument | What it proves that tests can't | Notes |
|---|---|---|
| **`cargo-mutants`** on `engine.rs` | Which engine lines could be wrong without any test noticing. With the model sweeps in place the surviving-mutant list should be near-empty; every survivor is dead code, an R4-expected assert removal, or a hole in the matrix — triage all three. The single best answer to "am I certain" available at this scale | CI step; run after §8 step 7 lands, then on every engine change |
| **Miri** over the test suite | Leak-freedom on paths without a drop probe, complementing U4 (probes prove not-double-dropped; Miri proves not-leaked). UB is unlikely under `forbid(unsafe_code)` but the leak check is free | CI, can be nightly-only |
| **Allocation tripwire** | §2.4's "nothing in the turn loop allocates" is currently unverifiable prose. A counting `#[global_allocator]` test runs a multi-turn steady-state script and asserts the per-turn allocation count is **constant** (target zero) after the first turn — catching clone-creep, `format!`, or buffer growth forever after. Assert constancy, not zero, so serde_json internals can't flake it | One dedicated test binary/integration test |

Deliberately omitted: **loom** (the Core has no concurrency — it becomes relevant with the live Environment) and **trybuild** (the §3.3 borrow facts are engine-internal; a breaking refactor already surfaces as a compile error per §3.4 — the compiler *is* that test).

## 8. Implementation order (each step lands compiling + tested)

1. `BoundedBuffer::drain` + tests (§2.1).
2. `Record` enum + `SCHEMA_VERSION` + golden serialization unit tests (§2.2).
3. Conformance fixes: `Engine::new` order + A4 precedence test; `lib.rs` forbid + re-exports; doc nits (§1).
4. `test_support.rs`: Trace, `!Clone` drop-probed payloads with traced `Serialize`, sentinel-carrying failure values, doubles (incl. panic actions and `EnvDropped`), TraceSink, `run_script` with the universal bundle U1–U6; grammar checker **with its known-bad fixtures** (oracle audit 2). No model, no auditor yet.
5. The full `run()` from §3's pseudocode with §6's assertion sites — first milestone is a clean `cargo check` discharging §3.4's risks; then matrix groups B, C, D, E land as targeted tests against the doubles.
6. Cross-cutting group F, record-protocol group G (goldens, G6 invariance), panic group J, adversarial group I.
7. Model oracle + matrix calibration (oracle audit 1) + auditor with fixtures (U7) + sweeps H1, H2, H5, H6 + detector group K.
8. Instruments: `cargo-mutants` triage, Miri, allocation tripwire (§7). H4 seeded/fuzz optional thereafter.

Steps 1–3 are independent of each other; 4 needs nothing from 1–3; 5 needs all of 1–4; 6–7 need 5; 8 needs 7 (mutants are only meaningful against the finished suite).

## 9. Design-doc defects surfaced (fold back into design-final.md)

- `schema_version`'s value and type are unspecified; §8.2 names the field only. This plan pins `1` (u32) — the golden test becomes the de-facto spec, which the doc's own Template rule says is a doc defect. Record the value in §8.2.
- The record JSON is never shown despite "serialized form … is normative". The golden test freezes field order as the table lists fields; consider adding one example line to §8.2.
- `max_turns: NonZeroUsize` vs BOUND-INDEX's "`max_turns` may equal `u64::MAX`" — only coherent on 64-bit targets. Harmless — the Engine counts in `usize` against `max_turns` and `EventIndex` carries the u64 separately (§3.2) — but the doc claim silently weakens on 32-bit.
- Order 1 swallowing `Outcome::Fatal`'s payload on overflow is explicit but surprising; test C5 documents it deliberately (and drop-probes the swallowed payload).
- `Dispatch{position}`'s "may be an unrelated latched failure" comment is an Environment-side fact the Engine can't test; the sim/live conformance suite must revisit it.
- `JournalError::NotAnObject` is unreachable through the Engine (§2.2: externally tagged `Record` always serializes as an object). Not a defect in the Journal — direct consumers still need the check — but §8 could note that the Engine path cannot produce it, so a `JournalFatal { .., NotAnObject }` in the wild would itself indicate a bug.
