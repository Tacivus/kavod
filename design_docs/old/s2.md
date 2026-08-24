# Kavod Core Design v12: Review Synthesis

## Bottom line

The eight reviews converge on a clear conclusion: **v12 has a strong behavioral
architecture, but it is not yet fully implementable or semantically closed.** The
remaining problems are concentrated at component boundaries and in claims that are
stronger than the specified mechanism.

Model agreement was used only as triage. Every issue retained below was checked
directly against v12.

One provenance correction: all eight files under the misspelled
`design_docs/reveiws/` directory actually review v12, not v11. They discuss v12-only
material such as A9 and `ShutdownReport`. Therefore `design-v12.md:4` is stale, and
none of these findings can be dismissed as already fixed by v12.

## What holds up

- **The Run graph is fundamentally sound.** Record ordering, partial dispatch,
  candidate consumption, index exhaustion, checkpoints, and Fatal finalization
  compose coherently.
- **The latch design is strong.** First-observed Error behavior, single reporting,
  shutdown close, and `Stopped` requiring a clean report are well structured.
- **Journal failure handling is unusually precise.** Bounded pre-encoding,
  short-write handling, flush-based commitment, poison behavior, and uncertain
  suffixes after sink failure are solid.
- **Simulated scheduling is deterministic and bounded.** Minimum-time selection,
  equal-time round-robin, persistent cursor, and per-call step budgets are coherent.
- **Live startup is carefully designed.** The start/cancel gate gives a credible
  implementation of `ENV-START`.
- **The design exposes trust assumptions instead of hiding them.** The Obligations
  table is a major strength, even though several obligations need correction.
- **v12 substantially improves v11.** The final shutdown latch observation,
  `ShutdownReport`, `ENV-ERRORS`, fixed checkpoint answer, richer trace, live Event
  ownership, and expanded obligations are meaningful improvements.

## Confirmed problems

### 1. The axioms lag behind the operational design

This was the strongest cross-review concern.

- **A9 is false as written.** Trace erases Error values, while `EngineExit` retains
  them. `DET-RUN` correctly qualifies equality, but A9 says every output is
  determined without that qualification (`design-v12.md:125-130`, `148`,
  `594-633`, `745-746`).
- **A4 mixes observation time with existence time.** A background Error can exist
  before the Run observes an unrelated Journal failure. The detailed protocol
  consistently uses first observation, but A4's second sentence uses "exists"
  (`design-v12.md:143`, `450`, `744`).
- **A2 says a turn ends at handoff**, but checkpointing, completion recording, and
  Stop shutdown happen afterward. Empty batches have no handoff at all
  (`design-v12.md:141`, `685-725`).
- **A3 conflates operation-result linearization with successful-effect commitment.**
  Failed operations never reach activation, consumption, or handoff; `shutdown`
  fixes its report later than call entry; and `APP-STATE` explicitly gives State
  mutation no commitment point (`design-v12.md:142`, `277`, `431-441`).
- **A1 needs capability language.** `Context`, `LiveCtx`, and `SimCtx` legitimately
  mutate owner-controlled resources through restricted capabilities; describing
  every external appearance as read-only is too absolute (`design-v12.md:140`,
  `252-266`, `888-900`, `1006-1010`).

These are not merely stylistic because the document declares the axioms foundational.

### 2. `RUN-GRAMMAR` overclaims what the mechanism proves

Multiple models found this independently, and direct inspection confirms it.

- `dispatch_batch(env, &[C])` cannot transfer arbitrary non-`Clone` Commands into
  consuming `Environment::dispatch` (`design-v12.md:405`, `575-577`, `778`). It
  needs an owned or drainable batch.
- `run_started(start_time)` and `accept_event(time, &event)` take loose
  caller-supplied values. Typestate proves ordering, but not that these are the exact
  values just returned by the Environment (`design-v12.md:740`, `769-783`).
- The `Initial` certificate contains `last_time`, but the specification does not say
  how it is initialized before `RunStarted` or explicitly updated after
  `EventAccepted` (`design-v12.md:654-659`, `756-760`, `776`, `783`).
- If shutdown returns a clean report and the final `TurnCompleted(Stop)` commit
  fails, Fatal finalization needs the consumed report's quiescence. Retention of
  that report is not specified (`design-v12.md:693`, `711`, `744`, `782`).
- `no_commands()` has no batch argument to assert against, and
  `Checkpointed<answer>` is descriptive pseudocode rather than a realizable Rust
  type shape.
- The Enforcement transition table is load-bearing but appears under "Mechanism,"
  outside Section 0's exhaustive binding forms. `RUN-GRAMMAR` then depends on it
  through a non-ID citation (`design-v12.md:14-36`, `740`, `748-815`).

The graph should be retained, but the concrete transition API and the exact
compile-time claim need revision.

### 3. Simulated lifecycle is incomplete

The Sim design needs a complete lifecycle state machine.

- "Lifecycle is open" is never defined. Startup failure therefore cannot precisely
  determine which Ports receive `stop` (`design-v12.md:1023-1024`).
- Successful `stop` is not explicitly said to end the lifecycle.
- Wakeup arms have no binding initial state, although later derivation assumes they
  start disarmed (`design-v12.md:1027-1030`, `1065-1066`).
- `SIM-DISPATCH` says dispatch always invokes `on_command`, while the shared latch
  rules require a pending Error to be returned first without invocation
  (`design-v12.md:450`, `1026`, `1035-1040`).
- Generic shutdown requires every Port to have immediate access to one shutdown
  signal. Sequential `stop` calls do not provide that shared instant, and `SimCtx`
  exposes no lifecycle flag (`design-v12.md:452`, `993-1015`, `1031`).
- Per-Slot Error mapping is required for `start`, `on_command`, and `step`; locating
  simulated mapping only at the command fan-out arm is insufficient
  (`design-v12.md:344`, `1024-1028`, `1083-1087`).

A small `NotStarted -> Open -> Ended` model plus a shared shutdown flag would resolve
most of this.

### 4. Live completion and shutdown need sharper semantics

The intended implementation is understandable, but the guarantees are inconsistent.

- "Publication" is defined exclusively as Error insertion into the latch, yet
  shutdown waits for "completion publications" even though expected completion
  remains unpublished (`design-v12.md:111`, `917`, `920`).
- A Port can physically return while `Running`, then be descheduled before the
  supervisor acquires the classification lock. Shutdown can close the latch first
  and classify that return as expected. Either "completion" must mean supervisor
  classification, or the premature-completion guarantee is too strong.
- A completion notification is not proof that the thread has exited. TLS destructors
  or scheduling can delay a subsequent join beyond the shutdown deadline
  (`design-v12.md:920`, `947-954`, `958-964`).
- The implementation sketch should explicitly use one global absolute deadline and
  only join handles independently known to have finished.
- The latch-versus-queue linearization requirement is sound, but the mechanism should
  state that the decisive latch check and dequeue or admission share one arbitration
  mechanism.

### 5. The trust boundary is incomplete

Several claims described as enforced are only author obligations.

- `TRUST-PURE` does not explicitly cover the Application object or `initial_state`,
  both of which can use interior mutability, globals, clocks, or I/O
  (`design-v12.md:224-243`, `295-310`, `1142`).
- Port isolation cannot be enforced by safe Rust: Ports can share globals or
  `Arc<Mutex<_>>`. That belongs in a Port-author obligation, not an unconditional
  guarantee (`design-v12.md:342`, `912`, `1022`).
- Error values and their `Drop` implementations can contain hidden authority or
  nondeterministic effects, but they are outside `TRUST-PURE`.
- Running the same test twice does not detect stable hidden authority such as
  environment variables or machine identity.
- A trace conformance suite cannot prove a bespoke Environment cleaned up internal
  activity, uses one clock authority, or respects internal bounds
  (`design-v12.md:1144`, `1161-1165`).
- `BOUND-BLOCKING` says infallible operations and destructors report Errors, although
  they have no Error channel (`design-v12.md:1145`).
- `TRUST-MEMORY` is effectively unverifiable as written: "Owner-defined" names
  neither a bound nor a check (`design-v12.md:1157`).

### 6. Journal framing and evidence claims need tightening

The Journal core is strong, but three claims overreach.

- Checking only the first and last braces does not guarantee one physical JSON line.
  A raw or custom serialized nested value can contain literal CR/LF bytes
  (`design-v12.md:511-529`, `1149`).
- `CommandsPrepared` cannot universally evidence "complete Command intent" because
  serialization is explicitly allowed to be lossy, and `TRUST-KEY` does not require
  the business key to be serialized (`design-v12.md:540-541`, `722`, `854-856`,
  `1148-1149`).
- An abort or process termination during write can leave an uncertain physical
  suffix even when no sink failure returned. The uncertain-suffix rule currently
  discusses only returned sink failures (`design-v12.md:513-515`).

The Journal should reject literal line breaks, call the record "serialized intent,"
and apply uncertain-suffix semantics to every termination before successful flush.

### 7. Wiring is not the only remaining open work

Wiring is candidly marked open and remains the largest known implementation gap
(`design-v12.md:1072-1102`). However, the issues above mean the status line's
implication that everything else is closed is inaccurate.

Wiring specifically still must settle:

- Builder and registration APIs.
- Slot-order authority.
- Fan-in and fan-out placement.
- Error sums and all per-Slot mapping sites.
- Final `LiveCtx`, `LiveConfig`, and `SimConfig`.
- Destination-full versus destination-closed dispatch errors.
- Public re-exports and construction of the missing API types.

There are also smaller API-authority gaps: `Engine`, `LiveCtx`, and `SimCtx` lack
explicit struct declarations; `Never: Serialize` appears only in nonbinding mechanism
prose; and the macro's "naming stem" wording suggests identifier concatenation that
stable `macro_rules!` does not perform.

## Valid minority findings

These did not have broad model consensus, but survived direct verification:

- **K3:** missing `last_time` initialization or update and clean-report retention
  after final Journal failure.
- **Opus:** unconditional `SIM-DISPATCH` contradicts latch-first behavior.
- **DeepSeek:** crash or abort can leave an uncertain suffix even without a returned
  sink Error.
- **Grok:** transparent `u64` JSON values are an interoperability risk for
  JavaScript/f64 consumers. This is not a Core determinism defect, but should be
  documented.
- **Terra and Sol:** "complete Command intent" is incompatible with permitted lossy
  serialization.

## Rejected or overstated claims

The following were excluded from the problem list:

- **Trace is circular because it is both execution output and replay input.**
  Conditional determinism over an observed external trace is coherent.
- **The simulated cursor must be recorded in the trace.** It is deterministic
  internal Environment state.
- **Sim post-handoff `on_command` Errors break dispatch records or determinism.**
  Handoff succeeded; observing the Error at a later operation is intentional.
- **The shared Sim step budget permits an unbounded denial of service.** Exhaustion
  is the intended bounded failure outcome.
- **Stale wakeup arms after a Sim Error violate lifecycle.** Every such path
  immediately exits the Run protocol, making the arm unreachable.
- **`mem::forget` defeats certificate guarantees.** The certificate is module-private;
  deliberate leaking by Kavod implementation code is simply an implementation bug.
- **Journal's write loop is unbounded.** Every successful iteration advances, while
  zero progress fails.
- **`remaining()` can underflow.** The buffer invariant prevents `len > capacity`.
- **Record sizing has a newline off-by-one.** The object bound intentionally excludes
  the separately reserved newline.
- **Port failure identity is entirely missing.** The typed Environment Error sum is
  intended to carry a per-Slot variant; the Journal intentionally records observation
  rather than causal Port identity.
- **Compile-fail testing private grammar types is impossible.** Same-module or
  `#[path]` UI test harnesses can access them.
- **A no-armed-Port Sim failure is an accidental bug.** It is the documented result
  when the finite-source protocol is not followed.
- **Open Wiring is a hidden defect.** It is explicitly disclosed; the problem is only
  that additional non-Wiring sections also remain unsettled.

## Recommended order

1. Rewrite A1-A4 and A9 to match the detailed operational rules.
2. Make the transition table binding and replace its pseudocode with implementable,
   ownership-correct Rust shapes.
3. Define complete Sim and Live lifecycle state machines, including shutdown
   signaling and completion tracking.
4. Close Wiring and all Error-mapping sites.
5. Correct the trust boundary and verification claims.
6. Harden JSONL framing and narrow the forensic evidence claims.
7. Clean up the remaining terminology, citation, and API-declaration defects.

The design's core should be preserved. The reviews do not justify a rewrite; they
justify tightening the formal boundary around an otherwise strong graph, latch, and
Journal architecture.
