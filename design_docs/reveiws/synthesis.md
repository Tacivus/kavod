# v11 adversarial-review synthesis

> **Resolution:** landed in `design_docs/design-v12.md` (2026-08-16), per the approved repair plan.
> v11 stays frozen as the artifact these reviews and this synthesis cite.

> **Date:** 2026-08-16
> **Inputs:** design-v11.md + the seven reviews in this directory (1.md–7.md, seven models, one shared prompt).
> **Method:** every claim cross-checked against the doc's binding text (§0's rule: API blocks, guarantee
> rows, named binding tables); contested calls and the biggest proposed fix independently re-verified by
> three adversarial verification agents. Impl ignored throughout, per instruction.
> **Verdict:** the skeleton held in all seven reviews — graph totality, index arithmetic, Journal bound
> arithmetic and poison, prefix accounting, token affinity direction, latch algebra as far as it's defined.
> The real findings concentrate at six seams: the run's endgame, the determinism story, the sim's
> underspecification, the Environment contract's completeness, live liveness, and the doc's own meta-rules.

Eleven groups, ordered by importance. Each is designed as one coherent sweep. ~140 raw claims
deduplicated to ~60 real items; the rejected pile is at the end so nothing gets re-chased.

---

## G1 — The unobserved-error endgame  ★ the one real design decision

**Found by 4/7** (swallow: r3.1, r5.G3, r6.8, r7.2) **+ 5/7** (RUN-CHECKPOINT quantifier: r1.2, r2.12, r3.t2.1, r5.G5, r6.3).

**Problem.** A Port Error published between the final checkpoint's `take_error` snapshot and shutdown's
latch-close is silently discarded (`ENV-LATCH` closed state), and the run exits `Stopped` — a success
report hiding a real failure, in the exact window (StopRequested's write+flush) where an execution Port
dying matters most. The A4 citation licensing the discard is unsound on this path: A4's cleanup clause
presupposes a first Error exists, and here none does (r2.8, r3, r5). Adding more snapshots only shrinks
the window; the only complete closure is making the latch-close itself the run's final observation.

**Fix (recommended — verified by a dedicated stress-test agent, RECOMMEND-WITH-CHANGES):**
- `shutdown(self)` returns a report — `ShutdownReport { quiescence: Quiescence, error: Option<Self::Error> }`
  (name it `ShutdownReport`; `Shutdown` collides with the glossary signal, `PortInput::Shutdown`,
  `Lifecycle::Shutdown`). Closing the latch **returns** a pending Error instead of discarding it;
  publications after the close are still discarded (genuine shutdown work).
- `StopPending`: report `Some(error)` → Fatal with a new `EnvironmentOperation::Shutdown`, quiescence
  from the same call. `Some` + `Incomplete`: the error is the cause, `Incomplete` fills the exit's
  quiescence field — needs an explicit precedence rule row (A4 can't arbitrate; both arrive in one value),
  same shape as overflow-beats-Outcome. Keep the Stop-path checkpoint (it keeps `StopRequested` from
  committing on an already-failed run; the report is the backstop, not a replacement).
- `RUN-FINALIZE`'s cleanup shutdown discards the report's error — now properly grounded (a cause exists).
  Consolidate this rewrite with the two pending RUN-FINALIZE wording fixes: "Environment started" means
  "start returned Ok" (r5.A1), and "consumed by the Stop path" explicitly covers the
  `TurnCompleted(Stop)`-commit-failure case, i.e. "exactly when StopPending ran" (r4.5).
- A4 gains a second clause so every remaining discard site is grounded: cleanup-discard applies once a
  first Error **or fatal Core condition** exists — worded "once one exists", not "after the run observes
  it" (SIM-START's cleanup stops discard before the run sees the returned Err) — or, on a run ending
  without one, after the latch closes.
- Three companion rewrites the stress test showed are required, or the sweep introduces contradictions:
  1. `RUN-CHECKPOINT`: "on the Stop path, shutdown's close is the run's final latch observation" — merge
     with the quantifier fix: "every turn **that reaches EffectsComplete** takes the latch **snapshot**
     exactly once" (as written, "every turn observes exactly once" is false in both directions: mid-turn
     Fatals observe zero times, and a 3-dispatch turn observes four times under ENV-LATCH's own notion;
     a literal reading even demands `take_error` after a dispatch Err, violating ENV-SERIAL — r5.G5).
  2. `LIVE-SUPERVISION`/`LIVE-SHUTDOWN`: the transition out of `Running` and the latch close become **one
     linearized instant** (otherwise a publication in the sliver between them is discarded ungrounded —
     the same genus of bug the sweep fixes). This also simplifies the supervision wording.
  3. The Trace absorbs the report as shutdown's operation result (quiescence + error presence, value
     erased) — otherwise DET-ENV pins something the trace doesn't carry. The "plus the run's Quiescence"
     tail then dissolves into "every Environment operation result".
- **Attribution honesty** (subsumes r7.1's real residue, r6.6, r5.G1's forensic half, r1.3/r5.A2):
  - One derive: a Fatal's `EnvironmentOperation`/`position`/turn localize where the Error was
    **observed**, not where it was caused; in live, the cause may lie in any earlier turn since the last
    snapshot. `NextEvent` gets the "possibly an unrelated already-latched Error" caveat `Dispatch`
    already carries (r2.t4).
  - Pin the pre-commitment race (r1.3, r5.A2): a publication landing after an operation's entry
    latch-check, when that operation then fails before its own commitment, is **not** taken — the
    operation returns its own Err and the publication stays pending (surfacing in the finalization
    shutdown's report, discarded there under the now-sound A4).
  - One Laws sentence scoping A2: serial turns bind the Engine's calls; Port-side processing of
    handed-off Commands is concurrent by design (r4.t5).

**Fallback** if the API change is refused: keep the discard but state the forfeit as a rule + derive
("`Stopped` means no observation point saw an Error; the final window is forfeited; terminal Port state
is readable through user-owned handles"). Strictly less robust; the swallow stays possible.

---

## G2 — Determinism: DET-ENV's premises, the Trace, the comparator

**Found by 3/7 explicitly (r1.7, r3.8, r5.C1), 2 more adjacent (r4.8–11, r6.12).**

**Problem.** The flagship guarantee is false as written. DET-ENV's only premise is "equal traces", and
the trace omits handler Outcomes — so two different Applications (Stop vs Continue on start; Fatal vs
overflow) produce equal traces and different pinned discriminants; r5's counterexamples are concrete and
verified. Satellite defects: the trace never says Ok payloads are included (DET-ENV pins
`TimeRegression { previous, offered }`, and `offered` exists only in a `next_event` Ok payload — r2.5);
"sink operation result" granularity is undefined (per-call with Ok counts vs per-commit — DET-RUN's byte
claim only holds under per-call — r4.8); the trace is circular as a replay input (acceptance and
Quiescence are outputs — r4.10); the §12 comparator under-enumerates (omits the `EngineExit` variant
itself, `Fatal.quiescence`, and payload equality for `Dispatch { position }` — r3.8, r5.G6); the
Application-determinism verification is vacuous ("same trace twice" — the trace is an output; a clock-
reading handler changes the trace and the check reports nothing — r3.8); "build" is undefined while
DET-RUN leans on it (serde_json drift changes bytes — r5.G6); and no axiom grounds determinism at all
while §2 claims everything follows from the eight (r3.8).

**Fix (one definitional sweep):**
- DET-ENV inherits DET-RUN's fixed inputs (build, Application, initial State, configuration) — one sentence.
- Rewrite the Trace as guarantee content, not a glossary bullet: full operation results including Ok
  payloads; a sink operation is one write/flush call with its Ok count; Error values erased, presence and
  position kept; shutdown's result is the report (per G1). Scope DET-RUN's byte claim to the committed
  prefix (JRN-COMMIT already owns the uncertain suffix).
- Split **recorded trace** (output; what DET-RUN/DET-ENV quantify over) from **replay input** (candidate
  sequence + scripted operation results; no acceptance, no Quiescence), and add the sim-replay
  preconditions note (origin = recorded start time; arm each recorded stamp; budget ≥ event count — r4.11).
- Complete the comparator enumeration (EngineExit variant, quiescence, all Core-owned payloads uniformly)
  and state DET-ENV's honest domain: it compares runs with equal traces, and many live/sim failure shapes
  never produce equal traces (inbox Full, ShutdownIncomplete, premature closure vs step budget,
  nothing-armed) — the conformance suite compares the expressible overlap (r4.9, r6.9, r6.12).
- Fix the verification procedure: two runs against the same scripted Environment and sink, not
  "same trace twice". Define "build". Reword the derive "Journal bytes are Environment-independent" to
  "Environment-independent given the trace" (logical_time is Environment-stamped).
- Ground determinism: either one new axiom ("the Core adds no choice of its own: everything not in the
  trace is a function of the fixed inputs") or soften §2's "everything is a consequence of eight axioms".

---

## G3 — Sim time and scheduler pinning

**Found by 5/7** (the stamp: r1.12, r2.1, r3.6, r4.2, r6.10) — the most independently confirmed single
defect in the whole set.

**Problem.** SIM-SELECT advances `now`, clears the arm, calls `step` — and never says what `Timestamp`
the returned candidate carries, nor what `now` advances **to**. No SIM row realizes ENV-TIME (LIVE-TIME
exists; §9's own preamble promises every guarantee realizes a contract row). A sim stamping a constant,
or pre-advance `now`, violates no written row and produces different `EventAccepted.logical_time` bytes —
wire-visible divergence in the Environment that exists to be the deterministic reference.

**Fix (one SIM-TIME row + a handful of pinning clauses):**
- `SIM-TIME`: `next_event` advances `now` to the selected arm's time before the selected `step`; the
  returned candidate is stamped with the advanced `now`; realizes ENV-TIME — and state the invariant that
  makes nondecrease structural (every armed time ≥ `now`, from SIM-WAKEUP + minimum selection).
- Cursor: initial value (Slot 0), advance rule (cursor := selected + 1 mod n), persists across
  `next_event` calls (r3.t2.11, r5.A5).
- Selection-loop precedence, per iteration: latch first (already stated), then nothing-armed →
  `SIM-COMPLETION`'s Error (including when the arm set empties mid-loop), then budget →
  step-budget Error, then select (r3.t2.12–13).
- One Define: the sim shutdown signal **is** the `stop` call (r3.t2.14, r2.t4, r5).
- Drop §10's sim "time-domain exhaustion" Error variant — unreachable; the sim performs no time
  arithmetic (r2.t4, r3.t2.15, r6.5). (Registry row goes with it.)
- Two derive notes: a reactive-only Port set (nothing armed after start) Fatals at the first `next_event`
  by design (r5.G7); `set_next(now)` self-re-arm spin is the step budget's motivating case (r5).

---

## G4 — Environment-contract completeness (what a bespoke implementor needs)

**Found by:** r3.3 + r6.1 (the contradiction), r3.6, r4.4, r5.A2/A3, r2.t4/15.

**Problem.** §5 claims to be "the complete contract" and §12 sells the conformance suite as bespoke
certification, but: (1) a genuine contradiction — ENV-SERIAL permits `shutdown` after any Err, while
SIM-START says the failing Port's Err means "no further call, stop included, reaches it" and SIM-SHUTDOWN
says stop **every** Port: a conforming caller double-stops cleaned-up Ports, stops the failed Port, and
stops never-started Ports. (2) Nothing requires a blocked `next_event` to wake when the latch goes
pending (LIVE-SELECT says it; the contract doesn't — a bespoke env can park forever on a stable Error).
(3) Neither implementation names `next_event`'s consumption commitment in binding text (live's "the
dequeue is the consumption instant" is Mechanism prose, which §0 says doesn't bind). (4) The
pre/post-commitment error-classification rule — fail before commitment → the operation's own Err; fail
after → **must be published** to the latch — exists only in sim Notes, and nothing anywhere *requires*
the publication (r1.8: an implementation could discard `on_command`'s Err after handoff and violate no
row). (5) ENV-SHUTDOWN's "so each Port can observe it before processing any further queued Command" is
only true under a capability reading recoverable from live prose (r4.4, r5.A3). (6) ENV-LATCH's state
chain omits `empty→closed` and `reported→closed`; after `take_error` returns `Some`, ENV-SERIAL imposes
no call restriction (error-blind bespoke callers are legal).

**Fix (one contract-section sweep; drafts G1's latch semantics once):**
- ENV-SERIAL rescope: after `start` returns `Err`, **no later call at all** — the Environment is already
  quiesced (ENV-START) and safe to drop; after any other operation's `Err`, **or after `take_error`
  returns `Some`**, the only later call is `shutdown`. (Stress-tested: nothing relies on
  shutdown-after-start-Err; kills the contradiction and the error-blind gap in one sentence.)
- SimPort lifecycle rule, stated once: any `Err` a SimPort returns ends that Port's lifecycle; no further
  call, `stop` included, reaches it; SIM-SHUTDOWN stops only open lifecycles.
- New contract row (drafted together, per the verification pass): error-channel classification — a
  failure before the operation's commitment returns as that operation's `Err`; a failure after it **must
  be published** (ENV-LATCH); and each implementation must name its `next_event` consumption commitment
  in a binding row (live: the dequeue, promoted from Mechanism; sim: the selected `step`'s `Some` return).
- ENV-LATCH single consolidated rewrite: complete the state machine (`empty→closed`, `reported→closed`,
  publish-into-pending discarded); replace "observed" with linearized-publication wording (a
  never-looking implementation must not be vacuously conforming — r4.t5); pin the pre-commitment race
  (G1's clause); close semantics per G1 (pending-at-close returned via the report).
- ENV-SHUTDOWN: state the capability invariant in the contract ("from that instant each Port has a means
  to observe the signal immediately, regardless of queued Commands; a channel's ordering of signal vs
  queued Commands is implementation API"). Add the latch-wakeup liveness obligation ("an in-progress
  `next_event` wait ends once the latch is pending"). ENV-START's "lifecycle ended" reworded in contract
  vocabulary ("will receive no further call"). ENV-BOUNDS gets real content for bespoke implementors or
  an honest scope note (r3.t2.26).

---

## G5 — Live liveness, termination, and delivery

**Found by:** r3.5 (five sub-findings), r6.7, r6.9, r5.G2, r1.4, r3.t2.20, r3.#17–19 (reconstructed).

**Problem.** Live has hang states and delivery holes: `offer` takes the Event by value and
`OfferRejected` carries no payload, so a rejected Event is destroyed — "may recover" is false for retry,
and the finite-source pattern (the only documented normal live ending) can lose its terminal Event to a
full queue and hang the run forever (BOUND-LOOPS deliberately doesn't bound the wait). A zero-Port live
Environment is constructible and hangs unconditionally; there is no live analogue of SIM-COMPLETION for
the decidable case. `recv`'s post-signal behavior is unpinned (once-then-drain vs sticky), which breaks
the Run's own advice: "emit stop-specific Commands before answering Stop" is unreliable — live `recv`
reports the signal ahead of queued Commands and abandonment is permitted, so the recommended pattern can
silently never deliver (r6.7; sim processes those Commands synchronously — opposite effects from the
same Application intent). `OfferRejected::Closed` has no stated trigger and "closes Engine-facing
admission" is dead wording (shutdown consumes the Environment; the closure that matters is Port-facing
fan-in — r5.G2). External cancellation (SIGINT) is never stated to be "model it as a Port".

**Fix:**
- `offer` returns the rejected Event (`OfferRejected { event, reason }` or equivalent) — one API-shape
  change; the Port can then retry under its own pacing while observing the lifecycle.
- Pin `recv`: once raised, the signal is sticky — every `recv` reports it; `try_recv` is the drain path
  (already stated). LIVE-SHUTDOWN: replace "closes Engine-facing admission" with "closes Event fan-in;
  a later `offer` returns `Closed`".
- Rewrite the Run's stop-Commands derive honestly: handoff to the inbox is guaranteed; **processing** is
  the destination Port's draining policy — and add an Obligations row: a Port whose protocol includes
  final Commands drains its inbox on shutdown before returning.
- §10 constraints: reject an empty Port set at construction; state SIGINT-as-a-Port as the cancellation
  story (one sentence).
- Quiescence honesty: define "run-scoped activity" (glossary, G10) and add the Obligations row — a Port
  ends everything it started before `run` returns (Kavod cannot see foreign threads; this is the only
  enforceable shape — verified).
- LIVE-SHUTDOWN deadline semantics: the deadline bounds the wait for completion **publications**;
  joining after publication is prompt by construction (no timed join exists — r3.t2.20).
- Test-profile caveat (reconstructed r3 item): under the unwinding test profile a panicked Port thread is
  joinable, and `shutdown` would report `Quiesced` — falsifying "finished entirely, destructors included"
  in exactly the configuration the lifecycle tests certify. State the caveat, or classify an
  unwind-detected join as premature closure. Also state plainly: live Port panics are unsupervised under
  `panic = "abort"` (r4.t5).

---

## G6 — Commitment-point vocabulary (one definition fixing five "contradictions")

**Found by:** r1.1, r1.13, r1.14, r3.2, r3.9, r4.13.

**Problem.** The glossary's "Before it, nothing happened; after it, nothing is retried, revoked, or
rolled back" is falsified by the doc's own rows: sim `next_event` advances `now`, clears the arm, spends
budget, and mutates Port state before any candidate exists (budget exhaustion then errors with no
candidate consumed); JRN-COMMIT's uncertain suffix puts bytes in the sink before commitment; SIM-START
places the startup commitment before the first Port `start` and then unwinds activation on a later Err
(a revoked commitment — r3.2); the `start` Success cell "run-scoped activity is live" is false at return
(every Port may already have failed — r1.14). And the most consequential effect in the system — handler
State mutation — has no commitment point anywhere: a Fatal exit hands back State claiming "orders sent"
for a discarded batch (r3.9, r4.13).

**Fix (vocabulary sweep, no behavior changes):**
- Redefine: a commitment point is the instant the operation's **outcome is fixed**; before it the
  operation's contractual effect has not occurred — subordinate effects its owner names may have, and
  they stand (A4); after it, nothing is retried, revoked, or rolled back.
- Each commitment-table Err column states what a failed call leaves standing (sim `next_event`: advanced
  `now`, cleared arm, spent budget, Port mutations; Journal: the uncertain sink suffix; `start`: already
  there).
- Reword `start`'s Success cell: activation committed irrevocably; any later failure is a runtime
  failure surfacing per ENV-LATCH.
- Move the sim startup commitment to successful return (stress-tested: sanctioned by "binds outcomes,
  not instants"; nothing depends on the old placement; unifies with the no-Ports clause and LIVE-START's
  "no fallible work follows" shape).
- One sentence in §3 or A4's Failure paragraph: handler State mutations have no commitment point and are
  never rolled back; they stand on every exit.
- Scope the two Journal sentences: "the Journal simply ends at its last committed record" is the logical
  committed sequence, not sink bytes (r1.15); JRN-FORMAT's bound is of committed records (r5).
- Derive note for the startup evidence gap: Environment activation is effectful before `RunStarted` can
  commit, so a run with real effects can leave an empty Journal; the exit carries the cause (r4.t5).

---

## G7 — RUN-GRAMMAR: claim what the types actually prove

**Found by 4/7:** r1.11, r2.14, r3.7, r4.12.

**Problem.** "Enforced at compile time" overclaims, and the section's own honesty inventory admits only
omission while the real gaps are commission: `complete_continue` after a Stop answer compiles (both
transitions take the same witness; the edge table's "Requires: answer was Continue" is invisible to the
compiler); `commands_dispatched()` takes no argument, so committing it after a partial dispatch compiles
and writes a record whose bound meaning is false; a "wrong index" is runtime arithmetic behind one
assert; the witnesses carry no turn identity (a spare checkpoint witness minted in turn N spends in turn
N+1; the shutdown helper can run before `request_stop`, inverting the record table's "before shutdown");
and "possession of the token in phase S proves the run is non-Fatal" is false — every state-detected
Fatal holds a live token.

**Fix (recommended: strengthen, then rescope to the truth):**
- Fold the witnesses into phases: the checkpoint becomes a transition
  (`EffectsComplete` → `Checkpointed`), the quiescence witness becomes the shutdown call's typed result
  feeding `complete_stop` — free-standing affine values disappear, and cross-turn reuse becomes
  unrepresentable. (G1's report integrates cleanly: the witness is minted only from
  `{None, Quiesced}` — the compile-time story gets stronger.)
- Branch the answer once: the handler's remembered Outcome selects a typed continuation
  (`Checkpointed<Continue>` / `Checkpointed<Stop>` or equivalent), shrinking the trusted window to one
  match.
- Let the dispatch loop own its transition (`Prepared` → dispatch-all → `EffectsComplete` or the Fatal
  path), so a partial-dispatch `CommandsDispatched` is unrepresentable at the call site.
- Then rescope the row to exactly what is true (order of records, no completion without the typed
  results, kind/payload agreement by construction, no second lifecycle) and extend the honesty inventory
  with whatever remains runtime (index arithmetic, the one answer-match). Fix the token-possession
  sentence to what it proves (the Journal holds exactly the path's records; the token's index and time
  are the run's).

---

## G8 — Turn-loop and wire-format pinning

**Found by:** r2.3 (two internal agents), r2.2/r3.t2.9, r3.t2.8, r4.5–7, r2.13/r5.C2, r2/r3 ("flat"),
r1.10, r3.t2.27, assorted drift.

**Problem + fix, itemized (all one-clause edits):**
- **Batch buffer never emptied between turns** — APP-OVERFLOW resets only the marker; the mechanism says
  "reused every turn"; the glossary definition creates no obligation. An implementation that never clears
  re-records and re-dispatches turn 1's Commands and violates no ID. Fix: "a fresh handler invocation
  starts with the **buffer empty** and the marker clear." The sharpest single pinning gap.
- **EventAccepted's index is derivable two ways** — "builds its payload from the token's own index" vs
  `accept_event` "derives the next index": literal reading puts `index: 0` on the first External Event,
  colliding with `RunStarted` and the EventIndex doc. Fix: a record carries the index of the turn it
  opens; `accept_event`'s payload carries the derived next index, which becomes the token's on success.
- **`outcome`'s wire form is unbound** — `TurnOutcome` is in no API block; RUN-RECORDS pins bare-tag for
  `record_kind` only; `{"outcome":"Continue"}` and `{"outcome":{"Continue":null}}` both conform. Pin bare
  tag string.
- RUN-INDEX boundary pinned to arithmetic: the check is `index == u64::MAX` (and note the
  TimeRegression-consumed candidate exists only in the trace).
- `accept_event`: the nondecrease check **precedes** the commit; a violation commits nothing.
- "Records carry indices, times, Events, Commands, and outcomes and nothing else" — add `record_kind` and
  `schema_version` to the list.
- Define "flat": the top-level keys are exactly the row's fields (no envelope); values may nest.
- Scope the exhaustive-tables claim: the state/edge tables are the run's **non-Fatal** graph; Fatal
  finalization is RUN-FINALIZE's row (r1.10). Fix the edges-table preamble for the recordless row
  (r3.t2.27).
- Drift batch: `Regression` vs `TimeRegression`; "(Laws registry)" vs bounds registry; "per acquisition"
  vs "per next_event invocation"; `ports!` names `Trading`, an item the expansion never creates (clarify
  the macro grammar or emit a marker type); "mode-specific" purge residue at the Port-contract intro;
  redraw the sketch's only cycle (EventAccepted out of BetweenTurns); delete ENV-SHUTDOWN's dead
  "rejects new Commands" clause; the section-number citation in the Assertions paragraph.

---

## G9 — Sizing and evidence honesty

**Found by 4/7 on JRN-SINK** (r1.6, r2.6, r3.10, r4.15) **+ 3/7 on sizing** (r3.4, r4.15, r5.G4).

**Problem.** Two config relations can kill healthy runs and neither is owned: BOUND-SIZING sizes
`max_record_bytes` against "the largest batch" but `EventAccepted` carries the whole inbound Event —
one oversized Event is a turn-1 Fatal with the candidate consumed for good, and deployment can't size
against payloads it doesn't control; and per-Port inbox capacity vs `max_commands_per_turn` has no
stated relation — transient backpressure escalates to Fatal, then likely `Incomplete`, then the
process-kill obligation. Separately, JRN-SINK's "fresh, or positioned immediately after a newline" is
unenforceable by Kavod (`Write` has no position) and absent from the Obligations table whose own rule
says "absent ⇒ enforced" — a dirty or aliased sink silently breaks JRN-FORMAT and RUN-RECORDS. And the
forensic story overclaims: the dispatch prefix is identified by "CommandsPrepared plus
`Dispatch { position }`", but `position` lives only in the exit — after a panic/abort there is no exit,
and the Journal alone cannot bound the handed-off prefix (r4.3, r6.11).

**Fix:**
- Restate BOUND-SIZING over the **largest record** (Events included), with a payload-author obligation to
  bound encoded size; add the inbox-capacity/batch/drain-rate relation as its own obligation row.
- Obligations rows for the sink owner: freshness/positioning, exclusive access, content fidelity.
- Scope the forensic claims honestly rather than journaling the position (a Fatal-path record would break
  "after any Fatal, no commit is expressible" — the affine-token corollary — and the Journal may itself
  be the Fatal): after an abort, the Journal bounds the uncertainty to the prepared batch, and the
  business-key obligation covers external reconciliation. Assign the **recognition** half of the
  business key (some row must obligate its use — today only supplying it is owned). Note the input-side
  twin (a consumed, never-accepted candidate has no key) and the sim start-Err diagnostic (empty Journal;
  the Error payload is the sole evidence).

---

## G10 — The doc's meta-model: obligations, glossary, IDs, axiom citations

**Found by 6/7 in some form** — the largest group by count, all wording, zero behavior.

**Problem.** §0's epistemology is load-bearing and the doc violates it: the Obligations table is a
fourth binding form §0 never sanctions, and its rows have no IDs (uncitable under "Cite IDs"); "enforced"
is defined as "impossible or panics" while the enforcement ladder ends "Tests cover the rest" and DET
rows rest on trusted obligations; `#![forbid(unsafe_code)]`, the no-unwinding rule, Slot-registration-
static, and nonzero-capacities are prose-only rules (a `LiveConfig` taking plain `usize` would violate
nothing); load-bearing vocabulary is undefined — External Event (used in binding text), run-scoped
activity (Quiescence's truth condition), flat, witness, publication, handoff, admission, externally
consequential; the payload-authority obligation covers only handlers and State, leaving Event/Command
aliasing (`Arc<AtomicU64>` in a payload), Drop impls, and cross-Port shared state unassigned (r1.5,
r6.4); PORT-ROUTING is simultaneously an enforced row and a trusted obligation, and its error-mapping
clause has no live referent (live arms do inbox admission; the Port Error surfaces via supervision —
r2.4); mechanism step tables own normative content §0 never binds (the commit table is the sole owner of
the Encode-vs-BoundExceeded discrimination that DET-ENV pins — r3.t2.16); and five axiom citations are
wrong (A4 cited for the overflow priority — a Core condition, not an Error, and the temporal claim is
unverifiable; SIM-START citing A3 for A4's cleanup; LIVE-SUPERVISION's clean-path A4; A1's "one
representation" false of the design's own derived views).

**Fix (one meta sweep):** sanction the Obligations table as the fourth form and ID every row; widen
"enforced" (unrepresentable / asserted / test-pinned) and state the DET rows' conditionality in the rows;
add the missing obligation rows (payload/Port aliasing and Drop; plus G5's and G9's rows); give the four
prose rules IDs; add the glossary entries (and fix "appear only here"); split PORT-ROUTING into enforced
routing + trusted per-Slot mapping, with each Environment placing the mapping at its own site; tighten
PORT-SUMS to say what the wiring type-checks vs what the bijection trusts; decide whether step tables
bind or move their normative content into rows; sanction citing binding tables by name (or ID their
rows); scope "receiver style is free" (consuming receivers are load-bearing); fix the axiom citations and
either add the determinism axiom (G2) or soften the eight-axioms claim; the two false navigation
sentences (§0's "one earlier mention"; obligations row 1013's "identical bytes" overclaim; the
Environment-dependence wording vs `logical_time`).

---

## G11 — Type-level and API-block fixes

**Found by:** r4.1 + r7.3 (2/7 on Send), r7.5/r4.t5, r3.t2.7/21/28, r7.6–7, r3.6.

- `LivePort` payload bounds: add `C::Event: Send + 'static, C::Command: Send + 'static` (both cross
  thread channels; under `forbid(unsafe_code)` the stated bounds cannot compile the design; keep them
  off `PortContract` — sim needs none).
- `ports!` expansion must use `$crate::PortContract`, not `::kavod::` (the pinned expansion literally
  fails inside the crate and under dependency renaming; the doc gets `::serde` right, which makes it
  conspicuous). Attribute pass-through is optional — hand-written sums are the sanctioned escape.
- List the derives the conformance comparator needs on Core-owned discriminant types
  (Debug/PartialEq/Eq on RecordKind, EnvironmentOperation, SinkOperation, CoreError, Quiescence) — a
  floor for the doc's own test story, nothing more (verified: no row otherwise requires unlisted derives).
- `initial_state`: add it to BOUND-BLOCKING's enumeration (it is currently in no obligation — "Handler"
  is glossary-pinned to on_start/on_event) plus a Justify note for its infallibility.
- Fix the `AllocationFailed` Justify (contradicted by `BuildError::CommandBuffer(TryReserveError)`
  carrying full detail on the same path — a Justify that invites relitigation).
- §10 inputs: live Error sum gains a thread-spawn-failure variant; disambiguate "queue exhaustion" (only
  dispatch inboxes produce an Environment Error; fan-in Full goes to the Port); drop the sim
  time-exhaustion variant (G3); fix Slot order to declaration order (single-authority; registration
  order isn't nondeterministic, but two authorities that must agree is the thing v11 exists to avoid).
- Small, optional: `Context` struct declaration in the API block; a `Timestamp` subtraction helper;
  `SimCtx`'s inert `C`; the EINTR no-retry Justify; `TurnCompleted(Stop)`'s Evidences cell understating
  quiescence; phase-type visibility note; `shutdown`'s "commitment point" reworded ("no commitment
  point: infallible and consuming"); a public `Poisoned` decision note for direct Journal consumers
  (coherent as-is — precondition panic — flag only if the public-consumer story matters).

---

## Rejected (verified nonsense — don't chase)

- **r7.1's remediation** (sim `dispatch` returns `Err` when `on_command` fails): would put processing
  before handoff and falsify the commitment table for a Command that was handed off with standing
  mutations — breaks A3/PORT-STATE. The Ok-then-latch design is correct; only the attribution derive
  survives (G1). Its "DET-ENV violation" also fails — the traces differ.
- **r6.6's barrier** (live must not fetch the next Event while prior turns' Port work can publish): no
  binding row promises within-turn observation — RUN-CHECKPOINT states the opposite — and a per-turn
  quiescence barrier kills the async design. The residue is G1's attribution derive.
- **r6.2 as an enforcement hole** (hand-written sums escape PORT-SUMS): the wiring type-checks payload
  agreement (fan-in constructor + fan-out arms); the residual bijection is already PORT-ROUTING's named
  trusted obligation. Wording precision only (G10).
- **r7.4's bulk derives**: §0's listed derives are a floor ("further derives … are free"); only the
  comparator listing survives (G11).
- **r7.7 as a defect** (registration-order nondeterminism): program-text order is deterministic and
  DET-RUN already premises "same build". Survives only as a §10 preference (G11).
- **r5.A4** (sim latch-vs-budget order): provably unobservable — latch first per SIM-SELECT, budget
  fresh and nonzero, sim single-threaded, `step(Err)` returns directly.
- **r1.4 as a mechanism change** (Quiesced vs foreign threads): unenforceable in principle under
  `forbid(unsafe_code)`; the obligation + glossary shape (G5/G10) is the only real fix.
- Also noted: 3.md is corrupted mid-file (transcript fragments; tier-2 #17–19 lost). #17–19 were
  substantially reconstructed (the test-profile/Quiesced item now in G5), but one or two findings may be
  genuinely unrecovered — re-running that review's lost segment is cheap insurance.

---

## What held (all seven agree)

Phase-graph totality (every state × outcome × batch × latch configuration has exactly one edge; no stuck
state), index arithmetic including the u64::MAX boundary, candidate-consumption vs acceptance, Journal
bound arithmetic (the +1 sizing exact at every boundary), the poison state machine and its
unreachability from the Run, dispatch-prefix accounting (including sim's Ok-while-latching),
first-failure-wins under every race interleaving tried, Error-value erasure soundness, the
`PhantomData<fn() -> S>` reasoning, `Never`/`ports!` type threading, SIM-WAKEUP cross-arming and `now`
monotonicity, the live start gate ("no Port code ever runs" on the cancel path), and Appendix A's ID
inventory (exact). The machinery is sound; the findings above are almost entirely about what the words
claim versus what the machinery does, concentrated at the seams the graph doesn't reach.
