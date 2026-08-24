# Adversarial Review — `design_docs/design-v12.md`

**Date:** 2026-08-18
**Method:** eight independent adversarial passes, each restricted to this file alone (latch/error semantics; run graph & certificate; determinism; Live env; Sim env; doc meta-rules; Journal/records/bounds; app+port contracts & trust boundary). Findings below are the ones that survived cross-checking against the doc text. Failed attacks are listed at the end.

**Verdict: not solid yet.** The architecture is sound and unusually well-disciplined, but there are real defects — including six places where the specified API cannot be written in Rust as printed, and one contract row that no shipped implementation can satisfy.

**Overall:** the skeleton holds. Graph totality, `RUN-FINALIZE`'s case analysis, `ENV-SERIAL` delivery, first-Error absorption, index-domain arithmetic, and the record/`RecordKind` correspondence all survived direct attack. What does not hold: six API blocks that cannot be implemented as printed, one contract row (`ENV-LATCH`) that neither shipped Environment can satisfy, an axiom (A4) that says two different things, and a set of enforcement claims stronger than the mechanisms behind them.

---

## Tier 0 — Cannot be implemented as written

These are not judgment calls. Each is a printed API that does not compile or has no constructor.

**K1. `dispatch_batch(env, &[C])` cannot hand off Commands. — BLOCKER**
`Environment::dispatch(&mut self, command: Self::Command)` takes the Command **by value**. You cannot move a `C` out of `&[C]`. There is no `C: Clone` or `C: Copy` bound anywhere — `Application::Command: Serialize` is the only bound — and `NO-UNSAFE` closes the escape. This is the doc's single worked example of its central enforcement claim.
*Fix:* take a drainable handle: commit `CommandsPrepared` over `&*batch`, then drain into `dispatch`. This also fixes K3 and resolves whether the buffer is emptied by the drain or by `APP-OVERFLOW`'s clear-at-entry.

**K2. `checkpoint(env, answer) -> Checkpointed<answer>` is not a Rust signature. — BLOCKER**
`answer` is a runtime value; `Checkpointed<Continue>` and `Checkpointed<Stop>` are distinct types. One function cannot return one-of-two types without a sum the doc never names, never puts in an API block, and never adds to the phase list. Consequence: the States table lists `Checkpointed` as **one** state; the type encoding requires **two**.
*Fix:* name the sum in the Enforcement block, or make `Checkpointed` one phase carrying the answer privately (weaker, but writable).

**K3. `no_commands()` "asserts the batch empty" with no batch parameter. — MINOR**
The batch buffer lives in the Engine (§3 Mechanism). The transition as printed has no access to it and cannot perform the stated assert.

**K4. Nothing in the document can construct or increment an `EventIndex`. — MAJOR**
The §3 block gives one accessor (`as_u64`) and a private field. Yet Enforcement requires index creation inside `engine/record.rs` ("`accept_event`'s payload carries the derived next index"), and `RUN-INDEX` requires a checked increment. §11 puts `EventIndex` in `time.rs`, so `record.rs` cannot reach the field. The doc handled the identical need for `Timestamp` explicitly and justified it (`from_nanos`); the silence here is an omission, not a deliberate asymmetry — and it is the one site A6 calls "identities."
*Fix:* add `pub(crate) const START` and `pub(crate) fn checked_next(self) -> Option<Self>`, citing `RUN-INDEX`.

**K5. `Never` has no `Serialize` impl in any API block. — MAJOR**
The §4 block is `pub enum Never {}` — no derives, no impls. But `PortContract::Event: Serialize` plus "*Define:* an absent direction uses `Never`" jointly require `Never: Serialize`. The impl exists only in Mechanism **prose** — which §0 says cannot create obligations ("A rule in none of the four forms does not exist"). As printed, every simplex Contract fails to compile.

**K6. The mapped Event sum crossing the Port-thread boundary has no `Send + 'static` bound. — MAJOR**
`LIVE-EVENTS`: "Mapping into the Application Event sum precedes admission." `offer` runs on the Port's thread, so the value entering the fan-in queue and leaving on the Engine thread is `A::Event`, not `C::Event`. The only bounds in the doc are on `LivePort` (`C::Event: Send + 'static`). **`A::Event: Send + 'static` is never declared** — nor is it on the live Error sum, which crosses from the supervision shell into the latch. §10 explicitly lists "`Send + 'static` boundaries" under **Constraints already fixed**, so this is not deferred.

**K7. `DET-ENV` mandates a comparison the API cannot perform. — MAJOR**
`DET-ENV` binds the conformance suite to compare "`JournalError` variant and `SinkOperation`". `JournalError` derives nothing and *cannot* derive `PartialEq` — its payloads are `serde_json::Error` and `std::io::Error`. There is no `kind()` accessor and no discriminant type. Same gap for `EngineExit`, `FatalCause`, `EnvironmentFatal`, `JournalFatal`, `ShutdownReport`, `BuildError`: none derives even `Debug`, which every fault-injection assertion needs. `TRUST-PURE`'s "identical Journal bytes and **exit**" is likewise unimplementable.
*Fix:* add a `JournalErrorKind` (`Debug, PartialEq, Eq`) + `JournalError::kind()`; add `Debug` across the exit types; restate `TRUST-PURE` as "`DET-RUN`-equal exits."

---

## Tier 1 — Semantic contradictions

**K8. `ENV-LATCH`'s linearization is anchored on the commitment; every named mechanism observes the latch strictly earlier. — BLOCKER**
*(Found independently by three passes.)*
`ENV-LATCH`: "a publication linearized **before an operation's own commitment** is taken, marked reported, and returned as that operation's `Err`."
But `LIVE-SELECT` makes the **dequeue** the commitment, and the mechanism reads the latch, *then* stamps, *then* dequeues. `LIVE-DISPATCH` makes the **admission** the commitment, and the mechanism reads the latch, *then* routes, *then* admits. A publication landing in the gap is ordered before the commitment instant, so the rule demands an `Err` — and the commitment table says `next_event`'s `Err` means "No candidate was consumed" about a candidate that was.

Both escapes are bad: read "linearized" against the *instant* and no shipped implementation conforms; read it against the *operation interval* and the rule goes vacuous (any concurrent publication may simply be ordered after), which makes §12's "prove **both sides** of `ENV-LATCH`'s linearization" untestable. The sim has the mirror problem — its latch is provably empty at every `next_event` entry, so the "before" side is unreachable there too.
*Fix:* re-anchor on the **observation**: "a publication linearized before an operation's latch observation is taken…; an operation's latch observation precedes its commitment, and a publication in between stays pending for the next observation." Then `RUN-CHECKPOINT`'s existing "a later publication stays pending for the next observing operation" becomes the derivation it already reads like.

**K9. `ENV-LATCH`'s two publication rules contradict each other. — MAJOR**
R1 (above) vs R2: "An operation that fails before its commitment is not an observation point: it returns its own Error and a concurrent publication stays pending."
When R1 fires, the operation returns `Err` — and therefore, by R2's literal wording, "fails before its commitment," so R2 forbids exactly what R1 just mandated. R2 is written as a predicate over the **outcome** while it means a predicate over the **cause**. Reachable divergence: `dispatch` at position k with a pending unrelated Port Error *and* a full inbox — R1 gives the root-cause Port Error, R2 gives the incidental inbox Error and discards the useful one.
*Fix:* "An operation that fails for a reason **of its own** — an Error not taken from the latch — is not an observation point… An operation observes the latch before attempting its own work."

**K10. A4 switches from "observes" to "exists" mid-row. — MAJOR (axiom)**
"The first Error or fatal Core condition the run **observes** is the Fatal cause. Once a first Error or fatal Core condition **exists** … everything after is best-effort cleanup whose Errors are discarded."
The doc's own Derive says these differ by arbitrarily many turns ("an already-latched Error surfaces at the next observing operation, so … the cause may lie in any earlier turn"). Reachable split: a Port publishes at t1 (unobserved); `StopRequested`'s flush fails at t2. Sentence 1 gives `Journal(StopRequested)`; sentence 2 says everything after t1 is discarded cleanup, leaving a Fatal with no expressible cause. Every window between an unobserved publication and the next observation point is reachable.
*Fix:* two words — "Once a first Error or fatal Core condition **has been observed**…". This also repairs the `Core(IndexExhausted)` case for free.

**K11. `SIM-DISPATCH` (binding) contradicts `ENV-LATCH` and `SIM-LIFECYCLE`. — MAJOR**
`SIM-DISPATCH` is unconditional: "`dispatch` synchronously routes to exactly one Port's `on_command`." Batch of three to Slot A; Command 0's `on_command` returns `Err` → published, lifecycle **ended**. The `Prepared` row then hands off Command 1, which `SIM-DISPATCH` says routes to `on_command` — into a dead Port. `ENV-LATCH` says the opposite (return the latched Error, hand off nothing). The Mechanism paragraph gets it right, but Mechanism is prose.
*Fix:* "Absent a pending latch Error (`ENV-LATCH`, which returns first and hands off nothing), `dispatch` synchronously routes…"

**K12. "Lifecycle is open" is load-bearing and undefined. — MAJOR**
*(Found by two passes.)* `SIM-LIFECYCLE` defines only how a lifecycle **ends** ("Any `Err` … ends that Port's lifecycle"). `SIM-START` and `SIM-SHUTDOWN` both quantify over "every Port whose lifecycle is **open**." Two consequences: (a) on `SIM-START`'s failure path, Slots whose `start` was never called are "open" and receive `stop` without ever having been started — a protocol `SimPort` never describes, and a Port that allocated in `start` panics, which under A8 **aborts during startup with an empty Journal**; (b) nothing ends the lifecycle of a Port whose `stop` returned `Ok`, so `SIM-START`'s closing claim "the return satisfies `ENV-START`" is not derivable.
*Fix:* define both ends in `SIM-LIFECYCLE`: opens at `start`, ends at the first `Err` or at `stop`'s return; a Port whose `start` was never called never opened one and receives no `stop`.

**K13. `LIVE-SUPERVISION`'s "unambiguously premature or expected" is false. — MAJOR**
The atomicity the row buys is *classification-with-publication*. What the claim needs is *completion-with-classification* — and `run` returning cannot be fused with a lock acquisition. Scenario: Port P's `run` returns `Err(e)` at T (genuine mid-run failure); P's shell is descheduled; at T+ε the Application answers `Stop` and the close runs; at T+2ε P's shell classifies against `Shutdown`, so the publication lands post-close and `ENV-LATCH` discards it. Report is clean → **`EngineExit::Stopped` for a run in which a Port died.** For a system whose stated purpose is forensic evidence, that is the wrong answer.
*Fix:* make classification **definitional** ("a completion is premature exactly when its classification, taken under the latch lock, observes `Running`") and add the residual as a Derive — or move the latch close to the *end* of `shutdown`, which `ENV-SHUTDOWN` permits (it only requires the close be the *final* observation).

**K14. The shutdown deadline does not bound `shutdown`; `Engine::run` can hang forever. — BLOCKER**
`LIVE-SHUTDOWN` waits "at most the shutdown deadline … for completion **publications**, joining publishers (prompt by construction: publication follows the Port's last work, destructors included), detaching stragglers at the deadline."
Three problems: (1) the parenthetical is a rule in prose clothes, and it is **false** — publication follows the *Port's* last work, not the *thread's*. A discarded second Error's `Drop` impl, or a thread-local destructor, runs after publication with no bound (`BOUND-BLOCKING` names destructors as unbounded trusted code). (2) §11 pins the crate to std, and **std has no timed join** — so the join after the deadline is unbounded. (3) The deadline is written as bounding *one wait*, not total shutdown time; total = deadline + Σ(join times). Result: no `EngineExit`, no `Incomplete`, and `TRUST-EXIT`'s supervisor never engages because there is no exit.
*Fix:* make the deadline an absolute instant bounding all of `shutdown`, and weaken `Quiesced` to "every supervised thread published its completion" (dropping the joins and the Note's "destructors included" claim) — that's the honest option given std.

**K15. `accept_event` and `run_started` take forgeable caller values, contradicting a closed enumeration. — MAJOR**
Transition preamble: "A requirement is never a loose value a caller could forget, reuse, or forge." Residue bullet: "**Three points stay runtime** … Everything else in `RUN-GRAMMAR`'s list is unrepresentable."
But two edge requirements *are* loose caller values: `run_started(start_time)` and `accept_event(time, &event)` — and neither is in the enumeration, which explicitly closes itself. The Engine can call `env.next_event()`, discard the result, and call `accept_event(fabricated_time, &fabricated_event)`: this type-checks, commits a well-formed `EventAccepted`, and sets `last_time` to a value `next_event` never produced. So `RUN-GRAMMAR`'s "the certificate's index **and last accepted time** are the run's" is trusted, not compiler-proved. `accept_event` also does not take `env`, contradicting "Transitions take the Environment … the requirement they perform is the edge's" for the one edge whose requirement *is* an Environment interaction.
*Fix:* `accept_event(env)` performs the `RUN-INDEX` check, calls `next_event`, checks nondecrease, and commits. Have minting take the start time so `run_started()` takes nothing (this also removes the duplicate time authority — `Certificate.last_time` is non-`Option` and `Timestamp` has no `Default`, so minting must already supply it, violating A1).

**K16. `Prepared` is a binding graph state with no certificate phase. — MAJOR**
*(Found by three passes.)* The States and Edges tables are binding and exhaustive and both list `Prepared`; Enforcement collapses it ("realizing the graph's `Prepared` state internally"). There is no `Certificate<W, Prepared>`, so `RUN-GRAMMAR`'s "possession of the certificate in phase S" is **vacuous at exactly the phase where a partial-handoff record would be the failure**. Either the phase table isn't exhaustive, or `RUN-GRAMMAR` is false for `S = Prepared`.

**K17. The Edges preamble's failure enumeration is false under Enforcement. — MAJOR**
"the two recordless edges commit nothing and **cannot fail**." But the recordless `EffectsComplete → Checkpointed` edge *is* `checkpoint(env, answer)`, which fails as `Environment(Checkpoint)`. Likewise `close(env)` fails as `Environment(Shutdown)` / `Core(ShutdownIncomplete)`, and `dispatch_batch` as `Environment(Dispatch)` — none of which is `Journal(JournalFatal)`. The States/Edges split (work in states, records on edges) and the Enforcement fold (work inside transitions) are two different attributions of the same operations, never reconciled. `RUN-GRAMMAR` *needs* the checkpoint inside the transition; the preamble forbids the transition from failing.

**K18. A2's "the turn ends at handoff" is falsified twice. — MINOR**
The turn demonstrably continues past handoff through the checkpoint, `StopRequested`, `shutdown`, and `TurnCompleted` ("End of every non-Fatal turn"). And in sim, `SIM-DISPATCH` makes handoff *be* the `on_command` invocation, so Command 0's processing runs strictly **inside** the turn while Commands 1..n are still being handed off.

---

## Tier 2 — Robustness holes

**K19. No liveness rule anywhere; total deadlock is reachable and there is no cancellation obligation. — MAJOR**
Two Ports blocked in `recv`; Engine blocked in `next_event`. No Command can arrive (Commands come only from turns), no Error can publish (publication requires a `run` to return), nothing raises the signal (`shutdown` is downstream of a turn). Deadlock, unkillable except by signal. The sim has the analogue (`SIM-COMPLETION`); live has nothing. `BOUND-LOOPS` explicitly disclaims it. `Engine::run` takes no cancellation channel. The only mitigation is a `*Define:*` note — which §0 says "creates no obligation by itself," and `BOUND-STATIC` requires only a *nonempty* Port set.
*Fix:* add `TRUST-CANCEL` — "The bound Port set contains at least one source that delivers a terminal Event under external cancellation; the Application answers `Stop` to it. Wiring + Application author. Verified by: signal-delivery test."

**K20. `&self` is authority Kavod supplies to every handler. — MAJOR**
`APP-CONTEXT`: `Context` "is the only capability Kavod supplies a handler." Derive: "the signatures admit nothing else."
The signatures admit `&self`. `Engine::new` takes `app: A` by value and hands `&self` to every handler call. An `A` holding a `TcpStream`, an `AtomicU64`, or a `Mutex<Cache>` is live authority on every turn, supplied by Kavod. This is the actual hole in the purity story, and `TRUST-PURE` never names the `Self` value.

**K21. `TRUST-PURE`'s verification cannot catch the obligation it states. — MAJOR**
Verified by "Two runs against the same scripted Environment and sink → identical Journal bytes and exit." That falsifies *nondeterminism*, a strict subset of *hidden authority*. Undetected: anything staying inside State (`state.started_at = SystemTime::now()` — a clock read the row explicitly names — never reaches the Journal, and exit equality on `state` is unimplementable per K7); process-stable globals and env vars (identical across two runs in one process); non-finite floats (both emit `null`); address-dependent ordering (same-process allocator reuse makes run 2 see run 1's addresses). Also a **false positive**: `initial_state` is not a Handler (Glossary), so a clock-reading `initial_state` violates nothing yet fails the test.
*Fix:* add `initial_state` and `Self` to `TRUST-PURE`'s subject list; require the two runs in **separate processes with a perturbed ambient environment**; have the conformance fixture emit a State digest as a Command so State divergence reaches the golden bytes.

**K22. An oversized inbound Event is a remotely-triggerable run kill, unfilterable by design. — MAJOR**
A5 forces `EventAccepted` (carrying `event`) to commit **before** `on_event`, and `next_event`'s consumption is irrevocable ("never retried, revoked, or re-offered"). So one oversized message from any peer → `BoundExceeded` → `Journal(JournalFatal { record_kind: EventAccepted })` → run dead, Event permanently lost, Application never given a chance to see or reject it. `APP-CONTEXT` offers no filtering hook; `offer` has no size discipline. The only stated mitigation is `BOUND-SIZING`, an obligation on "payload authors" verified by "config review." The classification also misdirects operators: it names evidence-writing, not input admission.
*Fix:* add a Port-author obligation ("a Port offers no Event whose encoding can exceed `max_record_bytes`; otherwise it truncates or reports an Error"), and state `BOUND-SIZING`'s batch inequality explicitly so config review has something to compute.

**K23. `serde_json::value::RawValue` in a payload breaks `JRN-FORMAT`. — MAJOR**
`RawValue`'s `Serialize` emits a magic newtype token the serializer writes **verbatim** — and any safe user code can hand-write the same impl. A payload holding `RawValue::from_string("{\n \"a\": 1\n}")` produces record bytes that start `{`, end `}`, pass step 3's byte test, pass the size bound, and commit — **containing raw newlines**. The JSONL file now has three physical lines for one record; "line order is the sequence" is false and every downstream reader mis-parses. `TRUST-SERIALIZE` ("deterministic, side-effect-free, bounded, nonpanicking, stable map order") says nothing about raw bytes or newlines. A stronger variant emits outright invalid JSON while still passing the `{`/`}` test.
*Fix:* add to `TRUST-SERIALIZE`: "and emit no raw bytes outside the serializer's value paths — no unescaped newline may appear in an encoded record," plus a newline scan of the buffer before step 4.

**K24. Three configured bounds are run-Fatal and have no obligation row, contradicting "the complete boundary." — MAJOR**
§12 claims "an obligation absent from it is enforced, not assumed." Missing, all the same class as `BOUND-INBOX`: the **live fan-in queue capacity** (undersized → Port latches an Error → Fatal), the **live shutdown deadline** (undersized → `Incomplete` → on the Stop path, `Core(ShutdownIncomplete)` turns a clean run Fatal), and the **sim step budget** (undersized → `Environment(NextEvent)` Fatal; also a stated replay precondition). Concretely: eight Slots, seven armed below the eighth, each `step` legally returning `None`, budget of 4 → Fatal on a run where every Port behaved correctly.

**K25. `BOUND-INBOX` is unsatisfiable, and the coupling that makes it so is unstated. — MAJOR (arguable)**
Fan-in `Full` is backpressure to the Port; inbox `Full` is Fatal. Under saturation: fan-in fills, Port P's `offer` returns `Full`, P retries "under its own pacing" — and while retrying is **not draining its inbox**. The Engine's next dispatch to P finds the inbox full → run dies. The obligation asks the deployer to size for "expected cross-turn residue," which is a function of P's drain rate, which is zero for an unbounded interval. Also `TRUST-LIFECYCLE` covers "blocking points"; an `offer` retry loop is a spin, so a Port retrying forever is outside every obligation in the doc.
*Fix:* widen `TRUST-LIFECYCLE` to blocking points **and retry loops**; add a Derive: a Port with an inbound Command protocol must interleave `try_recv` with `offer` retries.

**K26. `TRUST-DRAIN` and the deadline pull against each other. — MAJOR (arguable)**
The Port that most faithfully obeys `TRUST-DRAIN` (cancel N open orders, each a round trip) is the one most likely to be detached at the deadline — turning a successful run into `EngineExit::Fatal(Core(ShutdownIncomplete))`. And `ENV-SHUTDOWN` promises every Port "has a **means** to observe the signal immediately" — carefully worded, and useless to a Port blocked in a native `read()`. `LIVE-SHUTDOWN` wakes only *Kavod-owned* blocking points. The practical rule (never block natively without a timeout) appears nowhere.

**K27. `SIM-SHUTDOWN` reports `Quiesced` unconditionally while discarding the only contrary evidence. — MAJOR**
`stop`'s `Result<(), Self::Error>` is decorative — nothing consumes it on any path. A `SimPort` whose `stop` fails to join a helper thread still yields `Quiesced` → `Stopped`. Worse, the backing obligation doesn't reach sim at all: `TRUST-SPAWN` is scoped to "before **`run`** returns," and `run` is a `LivePort` method. `SimPort` has no `run`. So the one obligation the Glossary cites to make `Quiesced` meaningful (`Quiescence` = "whatever its Ports started") is live-only.
*Fix:* rescope `TRUST-SPAWN` to "before its last method returns" and add to `TRUST-SIM-PORT` that a SimPort starts no activity outliving its methods — or have `SIM-SHUTDOWN` report `Incomplete` on a `stop` `Err`.

**K28. `JRN-SINK`'s appended-sink clause is undecidable in exactly the case it matters. — MAJOR**
`TRUST-SINK` permits a sink "positioned immediately after a newline," but `JRN-COMMIT` says the committed boundary **cannot be determined from the bytes** after a sink failure ("an uncertain suffix, even if they form complete lines"). Verification is "Review" — the sink owner has no procedure. Meanwhile no record carries a run identifier, `schema_version` rides only on `RunStarted`, and indices restart at 0 each run, so run 2's bytes concatenate onto run 1's partial record with nothing to segment the file.
*Fix:* drop the append clause (fresh sink per run), or add a run id + `schema_version` to every record.

**K29. `DET-RUN` is classified enforced, is unconditionally stated, and is verified circularly. — MAJOR**
*(Found by two passes.)* §0: "Every ID outside the Obligations table is **enforced**: unrepresentable, asserted, or **pinned by a named test suite**." No named suite pins `DET-RUN` — the six §12 bullets pin `DET-ENV`, `RUN-RECORDS`, the edges, and `RUN-GRAMMAR`. Its only appearance as a test is inside the Obligations table, as `TRUST-PURE`'s verification method. So `DET-RUN` is true only if `TRUST-PURE` holds, and `TRUST-PURE` is checked by running `DET-RUN`. A failing two-run test is uninterpretable. Note the doc marks trust dependencies everywhere else (`NO-UNWIND`→`TRUST-ABORT`, `BOUND-LOOPS`→`BOUND-BLOCKING`); `DET-RUN` and A9 carry none.
Related: "Core-owned payload" is never defined (the Ownership map actually puts State under the *Application*, so `state: S` is arguably outside `DET-RUN`'s guarantee), and "State transitions" appears nowhere else in the doc and has no observation channel.

**K30. Ten `SIM-*` IDs have no enforcement of any kind. — MAJOR**
The §12 bullets name a live-lifecycle suite and no sim suite. `SIM-SELECT`'s round-robin cursor rule alone is pure behavior with no assertion, no unrepresentability, and no named test. Same gap for `BOUND-LOOPS`, `ENV-SEPARATION`, `ENV-BOUNDS`, `APP-EMIT`, `APP-OVERFLOW`, `LIVE-EVENTS`, `LIVE-TIME`, `LIVE-DISPATCH`, `LIVE-LIFECYCLE`, `LIVE-SUPERVISION`.
*Fix:* add an **Enforced by** column to every guarantee table (`unrepresentable` / `assert` / suite ID) and give §12's bullets IDs.

**K31. The compile-fail suite cannot exist as sited, and two of its five cases are vacuous. — MAJOR**
*(Found by three passes.)* "it lives where the module-private grammar types are visible" — but every practical harness (trybuild, compiletest, `compile_fail` doctests) compiles each candidate as a **separate crate**, which cannot see `pub(in crate::engine)` items; and you cannot place a file that fails to compile inside a crate that must compile. Both standard escapes are closed by the doc itself: `#[cfg(feature)]` by §11's "no feature gates," and `#[doc(hidden)] pub` by "Unforgeable means module-private." Separately, because `dispatch_batch` collapses prepare and dispatch, the "partial-dispatch `CommandsDispatched`" case can only reference a method that does not exist — it fails to compile for the same reason a typo does and pins nothing.
*Fix:* name the mechanism (e.g. a `RUSTFLAGS`-set `--cfg` gating a test-only re-export — not a Cargo feature, so §11 holds), and move the vacuous cases to the golden/fault-injection lists.

---

## Tier 3 — The doc violating its own meta-rules

**K32. Mechanism step tables are the sole source of binding rules. — MAJOR**
§0's binding-table list is closed and names only the commitment table and the Run's five tables. The Journal's `commit` step table is not on it, yet it is the only home of: the `{`/`}` object test (step 3), the newline `BoundExceeded` (step 4), the short-write retry loop (step 5), and the ordering of 3 before 4 — which decides which `JournalError` variant an oversized non-object gets, an observable `DET-ENV` compares and golden tests pin. Apply the doc's own deletion test to step 5: delete it and an implementer may issue one `write` and flush, silently truncating records, with no rule broken.

**K33. `Certificate`'s "No `Clone`, `Copy`, or `Default`" is unstateable. — MAJOR**
§0 form 1: "Listed derives are required; **further derives are free**." An API block therefore cannot forbid a derive. If the `Certificate` block is an API block, that sentence is void and `Clone` may be added — which makes a forged second grammar representable and destroys `RUN-GRAMMAR`. If it is not an API block, the certificate's shape is unbound prose.
*Fix:* amend §0 form 1 to "further derives are free except where a block lists prohibited derives, which bind."

**K34. The Enforcement transition table binds the entire grammar surface but is in no binding form. — MAJOR**
It fixes eight method names, receivers, arguments, record emissions, and return phases; §12's compile-fail bullet is written entirely against it; §10 lists "the Certificate transition set (Enforcement)" under **Constraints already fixed**. Yet §0 does not name it, and the section calls itself "mechanism." The "proof's boundary" bullets underneath likewise carry undeniable obligations ("`dispatch_batch` rejects an empty slice"; "the recordless batch edge asserts the batch it bypasses is empty").

**K35. The always-on-assertion requirement exists only in prose. — MAJOR**
§2: "Kavod checks its own invariants with always-on, constant-time assertions that panic on violation." No guarantee row states this, yet it is the entire second tier of §0's enforcement order and is cited by `RUN-INDEX`, `JRN-POISON`, and Enforcement. Delete it and `debug_assert!` throughout is conformant.
*Fix:* one Laws row, e.g. `ASSERT-ALWAYS`.

**K36. §12's verification conventions are prose, while §0 makes "a named test suite" the third enforcement tier. — MAJOR**
Delete the bullets and the golden-Journal, compile-fail, fault-injection, conformance-trace, and live-lifecycle suites are no longer required to exist. Every ID whose enforcement is "tested" becomes unenforced.

**K37. "Publication" is defined as Error-only and used for four other things — with teeth. — MAJOR**
*(Found by two passes.)* Glossary: "**Publication** — entry of an Error into the latch." §8 uses publish/publication for the shutdown signal, the cancel gate, the start gate, and thread completion — including inside the `Environment::shutdown` doc comment, which binds. The collision is load-bearing in two places: (a) `LIVE-START`'s "A Port failure **after publication** is a runtime failure" reads as "after an Error entered the latch," turning the sentence into a tautology; (b) `LIVE-SHUTDOWN` "waits for **completion publications**, joining **publishers**" while `LIVE-SUPERVISION` guarantees an expected completion "stays **unpublished**" — so **the target of shutdown's quiescence wait is undefined at the binding level.** The mechanism names "completion tracking," but no guarantee row connects it. That vocabulary confusion is very likely why nobody noticed K14.

**K38. `BOUND-STATIC` names "thread count" in a Laws row. — MAJOR**
*(Found by three passes.)* §0: "Core sections build only on the contracts and **never name an implementation**," and the Laws section is not on the exemption list. Threads are live-only; `SIM-STATE` says the sim "runs no concurrency," so the row is unsatisfiable by one of the two shipped Environments. Same violation in the Glossary's `Run-scoped activity` ("its own **threads**, timers, and callbacks"), which drives `Quiescence` and `ShutdownReport` — both Core API. Also `PORT-ROUTING` (a Core section) describes both implementations' internal mapping sites, which exceeds §0's "a contract's **pointer** to its shipped implementations."

**K39. `BOUND-STATIC` also pre-decides a question §10 leaves open, and its "static, not configured" is incoherent. — MAJOR**
§10 open: "What fixes the Slot order: registration order, or the Slot sum's declaration order." If registration order wins, Slot order is fixed by a runtime sequence of builder calls — which is exactly "configured," under the doc's own usage where `EngineConfig`'s construction-time fields are "configured capacities." Four closed guarantees bind on frozen Slot order (`SIM-SELECT`'s cursor "starts at Slot 0", `SIM-START`, `SIM-SHUTDOWN`, `LIVE-SHUTDOWN`'s join order) and none of their tests is writable until this is decided. **This is the clearest case of the doc claiming more closure than it has** — and it is a one-line decision the doc has already stated a preference for.

**K40. "State" names both application data and the graph's phases. — MAJOR**
The Glossary binds "**State** — all run-varying application data" and separately supplies the right word ("**Phase** … the run's position in its graph"). Yet §7's most prominent binding table is headed `| State | Work, in order |`, and `EngineExit<S, …>`'s `S` is Application State while `Certificate<W, S>`'s `S` is the phase.
*Fix:* rename the table to "Phases" and `Certificate`'s parameter to `P`. Cheap, and it removes a real reading hazard from the doc's centerpiece.

**K41. The `TurnOpen` row omits clearing the batch buffer and overflow marker. — MINOR**
`APP-OVERFLOW` requires "A fresh handler invocation starts with the buffer empty and the marker clear"; only §3 **Mechanism** says when ("cleared at handler entry"). Under the phase table's exhaustiveness ("work it does not list does not happen"), the clearing does not happen. It is also a placement violation — §0 puts "when an operation is called" in the Run.

**K42. Other terminology collisions. — MINOR**
"**Commit**" is defined Journal-specifically ("encode, write, flush") and then used as the verb of "commitment point" throughout (`LIVE-DISPATCH`, `SIM-DISPATCH`, `LIVE-SELECT`, `SIM-START`) where nothing is encoded or flushed. "**Contract**" is defined as a protocol pair and used four other ways, including in §0's own exemption text. "**Accepted**" is defined over candidates, which excludes the start turn, while `RUN-INDEX`, `Context::index`, and the `RunStarted` row all call the start turn accepted. "**Lifecycle**" carries three unreconciled senses (Environment orchestration, Port lifecycle, and the public `enum Lifecycle { Running, Shutdown }`).

**K43. `ENV-SHUTDOWN` is phrased in queue terms the sim has no instance of. — MINOR**
"closes Event **admission**" / "how a **channel** orders the signal against already-**queued** Commands." The sim has no Event queue (Events come from `step`'s return) and `SIM-SHUTDOWN` never mentions admission — so the row it claims to realize is unsatisfiable as phrased.

**K44. Citation-form violations. — MINOR**
§0: "**Cite IDs.** Never section numbers, here or in tests." Violations: the Status line's "(section 10)"; `SIM-COMPLETION`'s "(Ports Notes)" pointing at an ID-less `*Define:*`; `RUN-GRAMMAR`'s "(Enforcement)"; and five "(Obligations table)" citations where the specific IDs exist (`TRUST-ENV`, `TRUST-ABORT`, `TRUST-PURE`, `TRUST-SERIALIZE`, `TRUST-EXIT`).

**K45. Rules hiding in `*Define:*` notes. — MINOR**
Two definitions carry obligations, which §0 forbids ("A definition binds vocabulary … and creates no obligation by itself"): the finite-source pattern's "and **awaits the shutdown signal**" (delete it and a finite source that returns `Ok` becomes premature closure under `LIVE-SUPERVISION`, turning a correct run Fatal), and "external cancellation is a Port" (K19).

**K46. The axioms' binding form and enforcement status are unstated. — MINOR**
The Laws table's columns are `# | Axiom | Statement`; §0's binding-table list does not include it; Appendix A indexes all 69 other IDs and omits A1–A9, which are cited 30+ times. Read as form 2 (rows with IDs), §0's "Every ID outside the Obligations table is enforced" then applies to A1, A3, A5, A7 — none of which has any enforcement mark.

---

## Tier 4 — Smaller, still real

- **K47.** The `*Derive:*` "newtype, tuple, and unit structs do not [serialize as objects]" is **factually wrong** for newtypes — serde forwards `serialize_newtype_struct`, and the doc relies on exactly that in §3 (`EventIndex(u64)` "serialize as transparent u64"). `struct W(Inner)` over a named-field struct *is* an object.
- **K48.** Journal step 4's newline-`BoundExceeded` is either **dead code** (if the encode phase caps at `max_record_bytes` and the newline uses the reserved byte, as `JRN-FORMAT` and `MaxBytesTooLarge` both say) or a second, unstated bound. The two readings differ observably for an oversized non-object payload — an outcome `DET-ENV` compares and golden tests pin.
- **K49.** `TRUST-SINK` says the sink is "**exclusively owned** by the Journal"; the §6 `*Derive:*` recommends "a **shared** `Vec<u8>` handle" for tests. Also, `JRN-SINK` assigns "writer destructor behavior" to the sink's owner, but no obligation row mentions destructors — a `BufWriter` flushing on drop after a Fatal writes uncommitted bytes with nobody responsible.
- **K50.** `PORT-SUMS`'s compile-time proof is **vacuous for two Slots sharing a Contract** — and the doc's showcase example is exactly that (`Primary(MarketData)`, `Secondary(MarketData)`): identical payload types, so swapping the constructors or the match arms type-checks. `TRUST-ROUTING` catches it, but the reader's first example is the degenerate case.
- **K51.** `ports!`'s invocation `pub enum Trading<Event = TradingEvent, Command = TradingCommand>` contradicts "The invocation's `Trading` is a **naming stem**." If the stem determines the names, the bindings are redundant; if the bindings do, the stem rule is wrong. The doc does not say which is authoritative.
- **K52.** `SIM-SELECT` states cursor *mechanics* but never the *selection function*. An implementation that takes "lowest Slot index wins" and maintains the cursor exactly as specified without ever consulting it satisfies every literal clause — and starves Slot 1 forever when Slots 0 and 1 both `set_next(now)`. "Successor" also has no wrap rule.
- **K53.** `next_event`'s subordinate effects are named for only one of three failure paths (`step(Err)`); the `SIM-COMPLETION` and `SIM-STEPS` paths leave an advanced `now`, cleared arms, spent budget, and Port mutations unnamed — which under the commitment table's phrasing reads as a rollback obligation the sim cannot discharge.
- **K54.** `dispatch` to a Port whose `run` already returned surfaces as "inbox exhaustion," sending the operator to `BOUND-INBOX` to enlarge an empty inbox. Needs a distinct "destination Port already completed" variant.
- **K55.** **Per-Slot Event ordering is never guaranteed.** `LIVE-EVENTS` says only "one bounded queue"; §7 disclaims global ordering. Every Port protocol will assume its own offers arrive in order. It's free with any real queue — say it.
- **K56.** §10's "everything frozen before `Engine::run`" contradicts live `start` step 1 ("Freeze Slot order and capacities"), which runs *inside* `Engine::run`, and `BOUND-STATIC`'s "at construction." Three freeze points.
- **K57.** §10 asks where `SimConfig` "lives relative to `EngineConfig`" — already foreclosed: `EngineConfig` is a closed API block with two fields and `Engine::new` takes an already-constructed `env`.
- **K58.** `TRUST-MEMORY` is verified "Owner-defined" — no means at all, and it is the only ID never cited. `TRUST-ENV` requires upholding "every Environment-contract row," including `ENV-SERIAL`, which is a *caller* obligation an Environment author cannot uphold. `BOUND-BLOCKING`'s "A8 defines the blast radius" is false for the boundedness half — an `on_event` that blocks forever has no detection and no blast radius.
- **K59.** `Engine`, `LiveCtx`, and `SimCtx` are never declared — only `impl` headers appear. `SimCtx` takes `<'_, C>` and `LiveCtx` takes `<C>` with nothing explaining the asymmetry, and §9 carries no provisional marker. `TurnOutcome` and `RecordPayload` are referenced in Enforcement but declared nowhere. `Context` has no constructor and its `'a` appears in no signature.
- **K60.** `Quiescence` is glossed as "**witnessed** complete," but `RUN-FINALIZE` mints `Quiesced` on the `start`-`Err` path with no `shutdown`, no report, and no witness — it's an inference from `ENV-START`.
- **K61.** `RUN-SERIAL` says the Engine "owns the Environment and **the Journal** by value," but minting consumes the Journal — after which the certificate owns it (A1: one owner).
- **K62.** `TRUST-KEY`'s stated purpose is post-abort reconciliation *across process lifetimes*, but "Per-Slot tests" check presence and within-run stability. A per-run counter passes and is worthless.

---

## Cross-cutting: three root causes

Most of the above collapses into three:

1. **The latch contract was written for a single-threaded world.** `ENV-LATCH` anchors on the commitment because for `take_error` and the close, observation *is* the commitment — and in sim it always is. It is false for exactly the two operations the row names in the one Environment that is concurrent. K8, K9, K11, K13, K37 are all this. Fixing the anchor (observation, not commitment) fixes most of them at once.

2. **Two incompatible attributions of the same work.** The States/Edges tables put work in *states* and records on *edges*; Enforcement puts work *inside transitions*. `RUN-GRAMMAR` needs the second; the Edges preamble asserts the first. K16, K17, K15, K2, K34 are all this seam. The typestate is genuinely strong — but the doc has never picked which of the two descriptions binds.

3. **Enforcement claims outrunning enforcement.** §0 sets a high bar ("unrepresentable, asserted, or pinned by a *named* suite") and then a large fraction of IDs meet none of it, several key rules live only in prose the doc's own deletion test rejects, and the one suite that would prove the headline claim (`RUN-GRAMMAR`) cannot be sited. K29–K36 are all this.

---

## What held up under direct attack

Listed so it isn't re-litigated, and because it's most of the doc:

- **Graph totality.** Every phase's condition space is covered with no overlap; all phases reachable; `Closed` the only non-Fatal terminus. The `TurnOpen` overlap I tried (overflow marker set *and* empty batch) is unconstructible via `BOUND-NONZERO`.
- **`RUN-FINALIZE`.** The three branches are genuinely exhaustive and mutually exclusive. "Consumed, exactly when `StopPending` ran" is *true*, not merely asserted. Double `shutdown` is both unrepresentable and unreachable.
- **First-Error absorption.** `reported` is an absorbing state, so an Error can never be surfaced twice, and `ShutdownReport.error`'s `None`-proof is sound. No path drops a published Error outside A4's deliberate discard. Every tie the doc can reach is explicitly broken.
- **`Stopped` implies a clean report.** Airtight — `Closed` is reachable only through the closing transition. `EngineExit::Stopped` correctly omits `quiescence`.
- **Error erasure from the Trace is sound.** No Core control-flow path inspects an Error's contents; classification depends only on *where* it was observed. The `ShutdownReport` being in the Trace is the right factoring — it keeps `Quiescence` trace-determined even though a wall-clock race decides it.
- **The short-write retry loop and `JRN-POISON`.** Sound and byte-reproducible from recorded counts; every terminal condition is determined; `io::ErrorKind` never changes loop behavior.
- **Index arithmetic.** The `u64::MAX` check placement makes the increment genuinely unable to overflow; "Overflow past that check is an invariant panic" is real dead code.
- **`ENV-SERIAL` delivery, `JRN-POISON`'s unreachability from the Engine, `RUN-CHECKPOINT`'s once-ness, `APP-OVERFLOW`'s no-partial-batch, `remaining()`'s honesty, `Context` not leaking via lifetime or `&mut`, the finite-source pattern in sim, `Never`'s discharge in both directions, `serde_json` feature unification pre-empted by "full dependency set", the sim's structural time monotonicity and bounded acquisition loop** — all attacked, all held.
- **Appendix A is mechanically clean:** 69 IDs in the doc, 69 indexed, sets identical, no duplicates, every section assignment correct. (The only omission is A1–A9, per K46.)

---

**Suggested order of attack:** K8 + K10 (one clause each, and they unblock the whole error story) → K1/K2/K4/K5/K6/K7 (the six compile-level fixes) → K39 (decide Slot order; it's one line and four guarantees depend on it) → K16/K17 (pick which of the two graph descriptions binds) → K14/K19 (live liveness, the two ways the process can hang with no exit) → the rest.
