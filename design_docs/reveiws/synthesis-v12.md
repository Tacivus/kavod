# Review Synthesis: design-v12, 2026-08-24

**Inputs:** 11 reviews (the prompt says twelve; `design_docs/reveiws/` holds eleven review files — deepseek, fable, gemini, grok, kimi, muse, opus, ox, sol, sonnet, terra), 93 raw findings → 69 clusters → 54 confirmed (3 MAJOR, 24 MINOR, 27 NIT; 0 CRITICAL).

**Verdict:** The document is sound: no review broke the Run graph, the certificate grammar, `RUN-FINALIZE`'s three arms, the latch's core state machine, the Journal's byte arithmetic, the determinism conditioning, or Rust realizability — the heavily-executed core held everywhere it was attacked, and the one CRITICAL claimed (terra's test-profile-panic-to-`Stopped`) dies against the document's own disclosed test-profile carve-out. Two themes dominate what survived: (1) §5 declares itself "the complete contract" while leaving two orderings free that change `EngineExit` for identical Port behavior — the shutdown-signal/latch-close order and the pending-Error-vs-own-failure corner of `ENV-LATCH` — both found by one review (opus) and verified here; (2) §0's promise that "every ID outside the Obligations table is enforced" by a named tier is provably undischarged for a set of behavioral rows (`ENV-BOUNDS`, `SIM-SELECT`'s cursor, `DET-RUN`, several `LIVE-*` clauses). Everything else confirmed is wording, citation, and self-conformance debt — real under the document's own reading rules, none behavioral.


## Confirmed findings

Grouped into **resolution batches**: each batch is one work order — one editing pass over one
region of the document, one commit naming the SYNs it closes — listed in the order to run
them. Finding bodies below are unchanged from adjudication. The adjudication themes map to
SYN ranges as: Environment contract SYN-01..10, enforcement tiers SYN-11..20, Run grammar
SYN-21..32, Live/Sim API SYN-33..36, panics SYN-37, vocabulary/self-conformance SYN-38..54.

### Batch 0 — Decisions (no edits)

Settle these before any batch runs; the later batches apply the answers mechanically.

- **D1 (gates Batch 1; SYN-01):** which order `ENV-SHUTDOWN` fixes. Recommended: close-before-signal — both shipped implementations already behave this way.
- **D2 (gates Batch 1; SYN-02, enables SYN-39):** adopt the `ENV-LATCH` precedence clause — a pending Error ordered before the commitment point wins over an operation's own Error, and a latch-woken `next_event` returns it.
- **D3 (gates Batch 3; SYN-11, -12, -16, -18):** the enforcement-tier strategy. Three ways to discharge §0's promise, not mutually exclusive: **(a) name suites** — strongest, keeps §0's claim intact, but creates real test-suite obligations; **(b) add `TRUST-*` rows** — honest for rules only review can check, but grows the trusted boundary §12 wants small; **(c) scope §0's quantifier** — cheapest, but weakens the document's strongest meta-claim and is a retreat the next review round will notice. Recommended: (a) for every behaviorally-testable row, (b) only where no trace can witness the rule, (c) never. The per-row assignment is the actual decision — approve or amend the **Proposed D3 disposition** table below; Batch 3 executes that table, not this bullet.
- **D4 (gates Batch 2; SYN-24, SYN-25):** approve the two API-block changes. For SYN-24 the two options are *not* equivalent: splitting `RecordKind`'s `TurnCompleted` variant changes the serialized `record_kind` tag strings — a wire-format change rippling into `RUN-RECORDS`, the Records table, and every byte-exact golden test — while adding an outcome field to `JournalFatal` changes only the exit type and leaves Journal bytes untouched. Recommended: `JournalFatal` carries the outcome; leave `RecordKind` alone. SYN-25 (the `pub struct Engine` declaration) has no real alternative — just approve it.
- **D5 (gates Batch 4; SYN-36):** reword `BOUND-STATIC` neutrally now (recommended), or defer it to Wiring close.
- **D6 (gates Batch 3; SYN-40):** whether §0 names Mechanism a fourth, explicitly nonbinding prose job (recommended); the three load-bearing sentences get promoted either way.
- **D7 (gates Batch 3; the Unresolved item):** declare in §0 whether the placement rules bind the current text. Recommended: yes — every violation is being fixed anyway, and the declaration ends the question permanently.

**Proposed D3 disposition** (approve or amend per row; Batch 3 executes this table):

| Untiered rule / clause | Proposed disposition |
|---|---|
| `SIM-SELECT`'s cursor and round-robin clauses; `SIM-STEPS` budget fenceposts; `SIM-WAKEUP` last-call-wins; `SIM-COMPLETION` | Extend `VERIFY-SIM`'s scope from lifecycle to scheduling: check order, persistent cursor, equal-time ties, wakeup mutation, budget boundaries (SOL-06's list, quoted in SYN-11) |
| `LIVE-SELECT`'s stamp/dequeue and nothing-fallible clauses; `LIVE-EVENTS`'s `Full`/`Closed` returns; `LIVE-DISPATCH`'s admission-identity clause | Extend `VERIFY-LIVE` with select/offer/dispatch behavior cases |
| `APP-EMIT`, `APP-OVERFLOW`, `APP-STATE` | One small named Context suite (new row, e.g. `VERIFY-CONTEXT`): call-order append, first-overflow marker semantics, fresh-invocation reset, State-stands-on-Fatal |
| `DET-RUN` two-run repeatability (SYN-12) | Extend `VERIFY-CONFORMANCE`: run each scripted trace twice, compare `DET-RUN`'s list; cite it from `DET-RUN`'s row |
| Edge runtime checks (`Core(TimeRegression)`, `Core(IndexExhausted)` classification) | No separate action — falls out of SYN-14's `VERIFY-FAULTS` extension |
| `ENV-BOUNDS` | Shipped implementations: witnessed by the extended `VERIFY-LIVE`/`VERIFY-SIM` rejection and exhaustion cases; bespoke: already rides `TRUST-ENV` — say both in the row |
| `PORT-STATE`'s "never reading the payload" (SYN-18) | Extend `TRUST-ROUTING`'s obligation text (upholder: wiring author; verified by: review) — no new row |
| `ASSERT-INVARIANTS`, `BOUND-LOOPS`, `NO-UNWIND` first clause | Reclassify as tier definitions per KV-03's steelman: move into §0 (or mark definitional in place); `NO-UNWIND`'s abort half already rides `TRUST-ABORT` |
| The answer passed to `classify` (KV-08); blocked-`next_event` wake (SOL-07) | Enforcement already exists in effect — add the missing citations: `RUN-ENFORCEMENT` cites `VERIFY-JOURNAL`'s sequence pinning; `VERIFY-LATCH` gains one explicit publish-while-blocked case |

### How to run a batch (all batches)

Ground rules for whoever executes a batch — read these before the batch's own notes:

- **`design-v12.md` is the sole authority.** The quotes in each finding are excerpts; re-read the full row or section in place before editing it. The eleven reviews are testimony — do not consult them for wording.
- **Scope is exactly the batch's SYNs.** If a fix seems to demand a further change, write it down as a follow-up and stop; do not make it.
- **Every new normative sentence must land in one of §0's four binding forms** — a guarantee row with an ID, an API block, a binding-table row, or an Obligations row. No rules in prose; run §0's own deletion test on any prose you add.
- **Follow the document's citation discipline:** cite IDs, never section numbers; citations point backward across sections; trust marks may point forward into the Obligations table.
- **Fix directions are directions, not drafted text.** Write the change in the document's voice: derive rather than enumerate, plain language, one line per Glossary term.
- **Appendix A must reconcile after the batch:** every added or renamed ID appears there exactly once; no dangling citations anywhere in the body.
- **One commit per batch**, message listing the SYNs closed. Do not edit this synthesis's finding bodies; track completion against the disposition index.

### Batch 1 — Environment contract & Glossary pass (§1 + §5)

Highest value first: both content MAJORs and all of the bespoke-implementor exposure. §5 declares itself "the complete contract"; this pass makes that claim true. One editing pass over the Glossary (§1) and the Environment contract (§5), including its API doc comments and commitment table. Requires D1 and D2.

**Contains:** `SYN-01`, `SYN-02`, `SYN-03`, `SYN-06`, `SYN-07`, `SYN-08`, `SYN-09`, `SYN-10`, `SYN-26`, `SYN-38`, `SYN-43`, `SYN-44`, `SYN-47`, `SYN-49`.

**Execution notes:**
- SYN-01 and SYN-02 both edit `ENV-SHUTDOWN`/`ENV-LATCH`: draft the two rows jointly, once, then check SYN-03 and SYN-08's wording against the result rather than patching sequentially.
- Write SYN-02's precedence clause inside `ENV-LATCH`'s existing row — no new ID — so Batch 2's SYN-39 has a citable home.
- The Glossary edits (SYN-38, -43, -49, and SYN-03/-06/-07 if the Glossary route is chosen) must keep §1's "one line per term."
- After the pass, re-read `VERIFY-LATCH` (§12): its wording tracks `ENV-LATCH`'s ordering language. If it no longer matches, flag it for Batch 3 — do not edit §12 here.

#### SYN-01 Shutdown signal vs latch close: order unfixed — MAJOR, omission
- **Sources:** sole: opus KV-01
- **Text:** `Environment::shutdown` doc comment: "Publishes the shutdown signal, closes admission and the latch" vs `ENV-SHUTDOWN`: "stops Event delivery, closes Event admission, closes the latch into the report (`ENV-LATCH`), and raises the shutdown signal." No row orders the two.
- **Adjudication:** Verified against every §5 row: nothing fixes the order, and the two binding statements list the actions in opposite orders. A signal-induced Port Error is captured under signal-then-close (latch pending at close → report `Some` → `Environment(Shutdown)` Fatal per the `StopPending` row) and discarded under close-then-signal (publication after close → discarded → clean report → `Stopped`). Both shipped implementations close-with-or-before-signal (`LIVE-SHUTDOWN`'s linearized instant; `SIM-SHUTDOWN` closes before `stop`), so only the `TRUST-ENV` audience — exactly whom §5 claims to serve completely — is exposed. The steelman (deliberate latitude, like the §7 race-resolution derive) fails: that latitude is stated for source races and nowhere for this, and both shipped implementations agreeing on close-first signals intent. `DET-ENV` cannot catch it: the traces differ, so the freedom sits upstream of its premise.
- **Witness:** KV-01's: a logging Port whose `run` returns `Err(FlushFailed)` after observing the signal; Application answers `Stop`. Close-then-signal → `Stopped { state }`, Journal ends `…StopRequested, TurnCompleted`. Signal-then-close → `Fatal { Environment(Shutdown), Quiesced }`, Journal ends at `StopRequested`. Divergence in `EngineExit` variant, `FatalCause`, and committed record sequence from identical Port behavior.
- **Fix direction:** One clause in `ENV-SHUTDOWN` fixing close-before-signal (matching both shipped implementations), and make the doc comment's list agree. Also closes the derivability half of SYN-39.

#### SYN-02 Pending latched Error vs an operation's own pre-commitment failure — MAJOR, ambiguity
- **Sources:** sole: opus KV-02 (KV-19 is the adjacent, separately-fixed cluster SYN-08)
- **Text:** `ENV-LATCH`: "For a call that reaches one of those observation points, a publication completed before the call began orders before the point…"; "An operation that fails before its commitment is not an observation point: it returns its own Error and a concurrent publication stays pending."
- **Adjudication:** Verified: sentence 1 is conditioned on reaching the observation point; sentence 3's qualifier is "a **concurrent** publication," which does not cover a publication *completed before the call began*. A call that fails pre-commitment while an Error is already pending is governed by neither sentence, so two conforming behaviors exist, and under one the first published Error reaches no record and no exit (`RUN-FINALIZE` discards the finalizing report's Error). Core-owned discriminants agree under both readings (which is why `DET-ENV` is silent), but the exit's Error value — observable to the caller — differs, `VERIFY-LATCH`'s "permanent first-Error reporting" cannot decide between the readings, and the latch's stated purpose (deliver the first Error) is silently defeated under reading B. MAJOR stands on those grounds.
- **Witness:** KV-02's: Port A publishes `E`, publication completes; then `dispatch(c_0)` finds B's inbox full. Reading A: `Err(E)`, latch reported, exit carries `E`. Reading B: `Err(InboxFull)`, latch pending, finalizing report's `Some(E)` discarded — `E` appears nowhere.
- **Fix direction:** One clause in `ENV-LATCH`: an operation returns a pending Error that orders before its commitment point in preference to any Error of its own, and a `next_event` woken by the latch returns that Error.

#### SYN-03 "Bounded quiescence policy" is load-bearing and undefined — MINOR, ambiguity
- **Sources:** sole: opus KV-07
- **Text:** `shutdown` doc comment and `ENV-SHUTDOWN`: "applies its own bounded quiescence policy"; no Glossary line. `LIVE-SHUTDOWN` and the §8 Notes show the bound covers waiting for completion state, not the join tail ("`shutdown` remains blocked in a join and produces neither a `ShutdownReport` nor an `EngineExit`").
- **Adjudication:** Confirmed: a bespoke author reading §5 alone can hold P1 (bound only the wait for outstanding activity — Live's shape, may hang in teardown) or P2 (bound total elapsed time in `shutdown`); both satisfy the words, and the reconciliation lives only in §8's *Justify*.
- **Witness:** KV-07's two conforming policies P1/P2; P1 can hang the process, P2 cannot.
- **Fix direction:** One Glossary line or one `ENV-SHUTDOWN` clause: the bound applies to waiting for outstanding activity, not to reclaiming activity already witnessed complete.

#### SYN-06 `ENV-START`'s "lifecycle" has no general binding meaning — MINOR, ambiguity
- **Sources:** sole: deepseek OCN-02
- **Text:** `ENV-START`: "no Port is left mid-lifecycle: every Port either never began or will receive no further call, its lifecycle ended before the return." Glossary defines only "Sim Port lifecycle."
- **Adjudication:** Confirmed, narrowly: the colon-apposition steelman (the sentence defines its own term in place) is strong but not forced — "its lifecycle ended before the return" can be read as a third conjunct adding a teardown-completion requirement beyond "will receive no further call." No divergence observable to the Run; a bespoke-implementor obligation is what wobbles.
- **Witness:** OCN-02's two implementors: A treats the colon explanation as exhaustive; B additionally requires Port-internal teardown complete before the `Err` return.
- **Fix direction:** State the exact observable condition in `ENV-START`, or add a general "Port lifecycle" Glossary line.

#### SYN-07 "Externally consequential work" used outside the term's Glossary scope — MINOR, ambiguity
- **Sources:** sole: opus KV-18
- **Text:** Glossary: "**Externally consequential** — of a Command: its delivery causes an effect outside the process." `ENV-SHUTDOWN`: "The Environment itself initiates no further externally consequential work."
- **Adjudication:** Confirmed: the definition is explicitly scoped to Commands; applied to Environment *work* the row is unevaluable under the document's own vocabulary discipline, and `TRUST-ENV` makes it an author-facing obligation with no fixed boundary. The obvious extension is available but nothing in the text commits to it.
- **Witness:** KV-18's: a bespoke `shutdown` flushes a metrics socket during teardown — forbidden under the extension, outside the term's scope under the Glossary.
- **Fix direction:** Broaden the Glossary line: "of a Command's delivery, or of any work: it causes an effect outside the process."

#### SYN-08 Overlap-placement witness is uncheckable for `Err` returns — MINOR, unenforceable claim
- **Sources:** sole: opus KV-19
- **Text:** `ENV-LATCH`: "one overlapping the call may order on either side; the returned value witnesses that placement." `VERIFY-LATCH`: "verifies that the call's result and resulting latch state agree with it."
- **Adjudication:** Confirmed with opus's own scope note: for `Ok` returns and `take_error` the witness claim is forced and checkable; for an `Err` return the placement is legible only from the Error value's provenance, and `Environment::Error` is opaque with no required discriminability between a republished latch Error and an operation-minted one — against a bespoke Environment with an indistinct Error sum, `VERIFY-LATCH`'s agreement check has nothing to compare.
- **Witness:** KV-19's: during a `next_event`, Event `E` arrives *and* Port `A` publishes `P`; the Environment's duration conversion also overflows. `Err(TimeExhausted)` with `P` pending and `Err(P)` with the latch reported are one `Err` of one opaque type each.
- **Fix direction:** Scope the witness sentence to `Ok` returns and `take_error`, or require the Error sum to distinguish republished latch Errors. SYN-02's precedence fix shrinks but does not close this.

#### SYN-09 `Quiescence`'s "witnessed complete" is not witnessed for Sim or bespoke Environments — MINOR, unenforceable claim
- **Sources:** sole: sonnet CR-02
- **Text:** Glossary: "`Quiesced` (witnessed complete)"; Commitment points, `shutdown`: "`Quiesced` witnesses that every unit of run-scoped activity completed"; `TRUST-SPAWN`: "run-scoped activity is otherwise unwitnessable"; Sim Notes: "the report always carries `Quiesced`."
- **Adjudication:** Confirmed: the document's own `TRUST-SPAWN` row concedes that beyond Live's joins the activity is *unwitnessable*, while the binding commitment row uses "witnesses" unconditionally — for Sim the value is structural and trust-derived, not witnessed. A wording overclaim on a disclosed limitation, so MINOR, not a hidden contradiction.
- **Witness:** CR-02's: a `SimPort::start` spawns `thread::spawn(|| loop {})` and never joins it; `SIM-SHUTDOWN` still reports `Quiesced`, indistinguishable from a clean run.
- **Fix direction:** Scope the Glossary parenthetical and the commitment-row "witnesses" to what the implementation accounts for, marking the Sim/bespoke residue `TRUST-SPAWN`/`TRUST-ENV`-scoped — matching the hedge §8's Notes already carry for Live.

#### SYN-10 `shutdown`'s commitment point "the call itself" is an interval, not an instant — NIT, wording
- **Sources:** sole: sol SOL-05 (severity moved from MAJOR)
- **Text:** Glossary: "Commitment point — the instant an operation's outcome becomes fixed"; Commitment points, `shutdown`: "The call itself."
- **Adjudication:** Downgraded from MAJOR: the table's preamble ("The table binds outcomes, not instants: where a commitment sits inside an implementation is that implementation's business") licenses the looseness; `shutdown` has no `Err` path, so the before/after-commitment split — the only machinery the commitment point drives — is vacuous for it, and no two conforming implementations diverge. SOL's observation that the Live report's contents are fixed at the final observation, not at invocation, is correct and worth one clause.
- **Witness:** SOL-05's: at call entry one entry is `Outstanding`, deadline 10 ms; completion at 9 ms → `Quiesced`, at 11 ms → `Incomplete`.
- **Fix direction:** One clause: the consuming call is the commitment (irrevocable at invocation); the report's contents fix within it, at the final latch/completion observation.

#### SYN-26 `ENV-SERIAL`'s "`start` exactly once" vs Environments dropped without a run — MINOR, ambiguity
- **Sources:** sole: opus KV-20
- **Text:** `ENV-SERIAL`: "one serial caller: `start` exactly once, first; … `shutdown` at most once"; construction preamble: "`Engine::new` … invokes no Application or Environment method; failure is `BuildError`."
- **Adjudication:** Confirmed: the deliberate quantifier contrast within one sentence invites the literal reading, and three real paths drop an Environment with zero `start` calls (both executed `BuildError` paths; an Engine simply never `run`) — a state no row covers (`ENV-START` covers only drop-after-`start`-`Err`). The permissive steelman ("assumes" describes the discipline during use; zero calls is trivially serial) is available but unforced against the explicit "exactly once."
- **Witness:** KV-20's executed: `max_commands_per_turn = usize::MAX` → `try_reserve` `Err` → `BuildError::CommandBuffer`, Environment dropped, `start` never called.
- **Fix direction:** "`start` at most once, and first if at all."

#### SYN-38 "External Event" is defined by delivery and by acceptance — MINOR, contradiction
- **Sources:** ox OA-02, terra TERRA-03
- **Text:** Glossary: "**External Event** — an Event delivered by `next_event` …; External Events carry indices from 1" vs "**Accepted** — … `EventAccepted` for a candidate becoming one External Event. Only acceptance gives a turn its index."
- **Adjudication:** Confirmed as one cluster: two normative Glossary lines make delivery sufficient and acceptance necessary respectively; a `TimeRegression`-lost candidate is an index-carrying External Event under one and never became one under the other (the §7 derive sides with acceptance). No rule's outcome changes — vocabulary-level contradiction only.
- **Witness:** TERRA-03's: `next_event` returns event 42 at time 9 below last accepted 10 → `Core(TimeRegression)`, consumed, no index.
- **Fix direction:** "an *accepted* Event delivered by `next_event`"; describe `next_event` as returning a Candidate.

#### SYN-43 A discarded "publication" never enters the latch, so the discard clause denotes nothing — NIT, vocabulary
- **Sources:** sole: fable FBL-04
- **Text:** Glossary: "**Publication** — entry of an Error into the latch"; `ENV-LATCH`: "Every publication after the first, and every publication after the close, is discarded."
- **Adjudication:** Confirmed: under the Glossary binding a discarded offer is not a publication; the row plainly means the act of publishing. One intended reading; vocabulary fix.
- **Witness:** FBL-04's closed-latch offer.
- **Fix direction:** Define Publication as the act of offering an Error to the latch; entry succeeds only per `ENV-LATCH`.

#### SYN-44 "Publishes"/"publish" used for the shutdown signal and the start/cancel gate against the Glossary's latch-only binding — NIT, vocabulary
- **Sources:** sole: opus KV-27
- **Text:** `Environment::shutdown` doc comment: "Publishes the shutdown signal"; `LIVE-START` steps: "publish cancel," "Publish start" — vs `ENV-SHUTDOWN`'s disciplined "raises the shutdown signal."
- **Adjudication:** Confirmed: the binding doc comment uses the reserved verb for a non-latch act while the guarantee row shows the document knows the distinction. Distinct from SYN-43 (different fix: verb choice, not redefinition).
- **Witness:** The quoted pair.
- **Fix direction:** "Raises the shutdown signal" in the doc comment; "signal cancel/start" at the gate.

#### SYN-47 "Subordinate effects … stand" is cited to A4's cleanup rule; the content lives in the Commitment-point definition — NIT, citation content
- **Sources:** sole: opus KV-25
- **Text:** Commitment points, `next_event` `Err`: "subordinate effects the implementation names stand (A4's cleanup rule)"; A4: "remaining work is best-effort cleanup whose Errors are discarded"; Glossary Commitment point: "subordinate effects its owner names may have, and they stand."
- **Adjudication:** Confirmed narrowly: §2's Failure prose does establish "A4's cleanup rule" as the document's no-rollback shorthand, which licenses the citation for *committed* effects — but a pre-commitment `Err`'s named subordinate effects standing is exactly the Glossary line's content, and `SIM-SELECT` cites the Commitment points table for the same fact, showing the right form.
- **Witness:** KV-25's side-by-side of the three quotes.
- **Fix direction:** Cite the Commitment-point definition (as `SIM-SELECT` does) where pre-commitment subordinate effects are meant.

#### SYN-49 The Trace ascribes an "Ok count" to `flush`, which returns none — NIT, wording
- **Sources:** sole: opus KV-29
- **Text:** Glossary Trace: "(one write or flush call: its Ok count, or its failure's presence)."
- **Adjudication:** Confirmed: `flush` returns `io::Result<()>`; the disjunction fails to distribute. Intended reading obvious; flush results are load-bearing trace members (`JRN-COMMIT` makes flush the commit), so the wording deserves the fix.
- **Witness:** `std::io::Write::flush`'s signature.
- **Fix direction:** "a write's Ok count or a flush's success, or the failure's presence."

### Batch 2 — The Run pass (§7, plus one A4 clause in §2)

One pass over §7 — graph preambles, grammar rows, records, enforcement prose — plus A4's cleanup-scope clause. The graph itself held everywhere; these are seam and scope fixes between the binding tables and the fused realization. Requires D4. SYN-39 lands here but depends on Batch 1's SYN-02 clause (cite the new `ENV-LATCH` rule, or delete the note).

**Contains:** `SYN-21`, `SYN-22`, `SYN-23`, `SYN-24`, `SYN-25`, `SYN-27`, `SYN-28`, `SYN-29`, `SYN-30`, `SYN-31`, `SYN-32`, `SYN-39`, `SYN-41`.

**Execution notes:**
- SYN-22, -23, -28, and -30 all land in §7's opening paragraph and Edges preamble: draft them as one rewrite of those few sentences, not four sequential patches that fight each other.
- SYN-24 applies D4's choice; under the recommended `JournalFatal`-carries-outcome option, `RUN-RECORDS`, the Records table, and the golden tests are untouched — verify that stays true.
- SYN-39 cites the `ENV-LATCH` clause Batch 1 added (or, if D2 was declined, delete the note outright — do not leave it citing nothing).
- SYN-41's target is nonbinding Mechanism prose — keep the fix in that register: one disclosing sentence, not a new rule.
- SYN-27's edit is one clause in §2's A4 row; touch nothing else in the axiom table.

#### SYN-21 `RUN-GRAMMAR`'s unrepresentability overclaims relative to the boundary's actual reach — MINOR, ambiguity
- **Sources:** muse KAV-04, sol SOL-03 (SOL-03 severity moved from MAJOR)
- **Text:** `RUN-GRAMMAR`: "Within `RUN-ENFORCEMENT`'s boundary, a transition requirement is never a caller-supplied witness …"; "An out-of-order record, … a skipped checkpoint, … a `CommandsDispatched` without every handoff … is unrepresentable." `RUN-ENFORCEMENT`: "Three points remain runtime: … the answer and batch the Engine passes …"; Derive: "a record *omitted* where the graph requires one is caught by golden-Journal tests, never the compiler."
- **Adjudication:** Confirmed as one cluster — one scoping clause resolves both. KAV-04: the universal denial of caller-supplied witnesses coexists with two admitted caller-supplied values (answer, batch), and the unrepresentability list includes omissions the affinity note says are test-enforced, so the scope of "within" is ambiguous. SOL-03: "a `CommandsDispatched` without every handoff is unrepresentable" cannot be a compiler fact — the private `dispatch_batch` could type-correctly skip `env.dispatch` — but the claim reads as caller-facing under "Unforgeable means module-private … hold their guarantees exactly as long as they stay behind their modules," which is why this is MINOR ambiguity, not a MAJOR false claim. The correcting text exists (RUN-ENFORCEMENT, the affinity derive); the defect is that `RUN-GRAMMAR`'s blanket sentence does not carry it.
- **Witness:** SOL-03's: with batch `[7u8]`, a type-correct private `dispatch_batch` commits `CommandsPrepared`, drains and drops `7` without calling `env.dispatch`, commits `CommandsDispatched` — all typestate compiles; only a suite can catch it.
- **Fix direction:** Scope the sentence: unrepresentable *for any caller outside the boundary*, excluding the three runtime points and record omission by drop (test-enforced); in-module transition conduct is pinned by `VERIFY-JOURNAL`/`VERIFY-CONFORMANCE`.

#### SYN-22 The Edges table's Requires/work taxonomy is inconsistently drawn; the checkpoint edge's "cannot fail" — MINOR, ambiguity
- **Sources:** deepseek OCN-01, opus KV-09
- **Text:** Edges preamble: "The two recordless edges commit nothing and cannot fail"; `EffectsComplete` → `Checkpointed` Requires: "latch snapshot `None`"; `BetweenTurns` → `EventAccepted` Requires: "the transition's successful `next_event` return; `ENV-TIME`'s nondecrease, checked before the commit."
- **Adjudication:** Confirmed as one cluster (one preamble rewrite fixes both): the Requires column mixes phase-established preconditions (empty batch, fixed answer) with transition-performed work (`next_event`, arguably the snapshot), and the preamble acknowledges work-failure as a category yet declares the checkpoint edge unable to fail. The forced reading of `RUN-GRAMMAR` ("never a caller-supplied witness … it is the phase itself or work the transition performs") plus `RUN-ENFORCEMENT`'s exactly-three runtime points puts `take_error` in the transition (the private table agrees: `checkpoint(env)` performs the snapshot and `Some` consumes the certificate) — so one of the two "cannot fail" edges has a Fatal outcome, and only the weaker reading "cannot fail *to commit*" survives. This resolves the fable-vs-opus dispute for opus (see Disputes).
- **Witness:** KV-09's: latch pending at `EffectsComplete`; `checkpoint(env)` consumes the certificate into `Environment(Checkpoint)` and returns no successor — the edge failed under the work-failure category the preamble itself established.
- **Fix direction:** Rewrite the Edges preamble to say per edge which Requires entries are transition-performed and each edge's failure mode: "The two recordless edges commit nothing; the empty-batch edge cannot fail, the checkpoint edge fails only as `Environment(Checkpoint)`."

#### SYN-23 `Prepared` is a binding phase no certificate value ever occupies — MINOR, ambiguity
- **Sources:** sole: opus KV-17
- **Text:** Glossary: "certificate — the value whose possession proves the position"; `RUN-GRAMMAR`: "Every transition consumes its source certificate and returns its successor"; private table: "`EffectsComplete<A>`, realizing the graph's `Prepared` state internally."
- **Adjudication:** Confirmed: the binding Edges table has an edge whose From is `Prepared`, `RUN-GRAMMAR` quantifies over transitions (Glossary: edges *are* the transitions), yet the realization — itself named by binding rows (`RUN-ENFORCEMENT`'s "the batch transition," `VERIFY-GRAMMAR`'s "fused batch transition") — never materializes a `Prepared` certificate. The reconciliation ("The graph's `Prepared` state and both its edges bind the record sequence, which is unchanged") exists only in nonbinding Enforcement prose. Three reviews (grok, kimi, sonnet) held this attack *by relying on that nonbinding prose*, which is precisely the gap. No divergence; one intended reading; MINOR.
- **Witness:** KV-17's: a nonempty batch passes "through" `Prepared` per the Edges table while no `Prepared` certificate ever exists for its outgoing edge to consume.
- **Fix direction:** One clause in the Edges preamble: rows bind record sequence and failure outcomes; a realization may fuse adjacent edges sharing a source certificate.

#### SYN-24 `JournalFatal` cannot separate a failed `TurnCompleted(Continue)` from a failed `TurnCompleted(Stop)` — MINOR, omission
- **Sources:** opus KV-11, sonnet CR-11
- **Text:** `RecordKind` has one `TurnCompleted` variant; `JournalFatal { record_kind, error }` carries no outcome; §7: "`EngineExit` is the run's only outcome channel."
- **Adjudication:** Confirmed as one cluster (same defect, same fix, independently found twice): both witnesses check out — a Continue-path commit failure whose fresh finalizing `shutdown` reports `Quiesced`, and a Stop-path commit failure after a clean report with retained `Quiesced`, produce bit-identical `Fatal { Journal(JournalFatal { TurnCompleted, Sink{Flush} }), Quiesced }` exits for operationally opposite runs (one died mid-stream, one completed all business and shut down deliberately). The Journal disambiguates (`StopRequested` committed in one), but the exit is declared the only outcome channel and a write-only-sink caller holds only the exit. No binding claim is false; a completeness gap.
- **Witness:** CR-11/KV-11's paired runs, above.
- **Fix direction:** Split the kind or carry the outcome in `JournalFatal`, mirroring the record's own `outcome` field.

#### SYN-25 The exact API never declares `Engine` — MINOR, omission
- **Sources:** sole: sol SOL-01 (severity moved from MAJOR)
- **Text:** §0: "API blocks — item names, type shapes … are exact"; §7: `impl<A, E, W> Engine<A, E, W>` with no `Engine` declaration anywhere.
- **Adjudication:** Confirmed as an omission, downgraded: the item kind and shape of `Engine` are genuinely underivable (SOL is right that no declaration exists), but API blocks are demonstrably excerpts assuming ambient context (unqualified `Serialize`, `NonZeroUsize` throughout), the document never claims each block compiles in isolation, and no observable divergence follows (all storage private, construction only via `new` — a struct vs. a one-variant enum is invisible). MAJOR required a divergence or a false binding claim; neither exists.
- **Witness:** SOL-01's: name resolution for the inherent impl finds no `Engine` item in the document.
- **Fix direction:** Add `pub struct Engine<A, E, W> { /* private */ }` to the §7 API block.

#### SYN-27 A4's post-close cleanup clause and the `StopPending` row point opposite ways on a failed `TurnCompleted(Stop)` — MINOR, ambiguity
- **Sources:** sole: opus KV-06
- **Text:** A4: "on a run that ends without a Fatal cause, that run-wide cleanup instead begins when the latch closes"; `StopPending`: "failure to commit that record finalizes with the retained `Quiesced`."
- **Adjudication:** Confirmed at MINOR with opus's own steelman accepted: the graph is decisive (the `Closed` phase is unreachable without the commit; `RUN-GRAMMAR` and the Edges table force Fatal), so no implementer diverges — but the deciding rows are two *other* rows, A4's clause cannot be evaluated prospectively at the instant it names, and nothing marks its antecedent as retrospective.
- **Witness:** KV-06's: `Stop` at index 7, clean report, flush of `TurnCompleted(Stop)` returns `Err(BrokenPipe)` — A4-literal reads it as discardable post-close cleanup; the row reads it as Fatal.
- **Fix direction:** Scope A4's clause to work outside the Run's own graph (Environment- and Port-side cleanup), which is the job it actually does.

#### SYN-28 "a transition *is* a commit" is false for two of nine edges — NIT, wording
- **Sources:** opus KV-22, sonnet CR-18
- **Text:** §7 opening: "a transition *is* a commit — the next phase is unreachable until the edge's record commits"; Edges preamble: "The two recordless edges commit nothing."
- **Adjudication:** Confirmed as one cluster: scene-setting prose contradicted 130 words later by the binding table; deleting it changes no obligation, so NIT. The §7 Notes even carry the correction ("the empty batch and the checkpoint take recordless edges because they bracket no effect").
- **Witness:** The two quoted sentences.
- **Fix direction:** "…a transition is a commitment point — where the edge carries a record, the next phase is unreachable until it commits."

#### SYN-29 `VERIFY-GRAMMAR` names a private decomposition ("the fused batch transition") the graph does not require — NIT, self-conformance violation
- **Sources:** sole: opus KV-26
- **Text:** `VERIFY-GRAMMAR`: "any caller attempt to commit `CommandsDispatched` independently of the fused batch transition."
- **Adjudication:** Confirmed narrowly: a binding verification row pins mechanism vocabulary ("fused") that only nonbinding prose establishes; KV-26's non-fusing counter-realization is arguable (RUN-ENFORCEMENT's "the batch transition" cuts against it), but the one-word fix is strictly better regardless and survives either realization.
- **Witness:** KV-26's single-buffer `prepare`/`dispatch_all` decomposition, which defeats the stated two-buffer hazard yet has no "fused" transition for the suite to name.
- **Fix direction:** "…independently of the transition that performs every handoff."

#### SYN-30 `Checkpointed`'s "the only edge out" has two referents in the Edges table — NIT, wording
- **Sources:** sole: opus KV-23
- **Text:** Phases: "`Checkpointed` | None; the fixed answer picks the only edge out." Edges: two rows From `Checkpointed`.
- **Adjudication:** Confirmed: the definite description resolves only against the nonbinding private table's typed refinements; intended reading (the fixed answer makes exactly one edge available) is obvious.
- **Witness:** The two Edges rows.
- **Fix direction:** "the fixed answer picks which of its two edges is available."

#### SYN-31 `RUN-ENFORCEMENT`'s assertion citation is ambiguous between two different assertion sites — NIT, wording
- **Sources:** sole: sonnet CR-16
- **Text:** `RUN-INDEX`: "Overflow past that check is an invariant panic"; `RUN-ENFORCEMENT`: "the index arithmetic behind `accept_event`, backed by one always-on assertion that a freshly minted certificate has the start index."
- **Adjudication:** Confirmed as citation clarity only: sonnet traced the arithmetic safe under either reading (the domain check at `BetweenTurns` runs strictly before any increment from `u64::MAX`), so no soundness exposure — the named assertion just reads as the one-time startup check while claiming to back per-turn arithmetic.
- **Witness:** CR-16's trace: index `u64::MAX - 1` → check passes → commit → `u64::MAX` → next entry's check fires before any further increment.
- **Fix direction:** Name the per-turn assertion site distinctly from the startup one, or say "every freshly minted successor certificate."

#### SYN-32 Startup calls index 0 prospective; `RUN-INDEX` calls it the latest accepted ordinal — NIT, wording
- **Sources:** sole: muse KAV-05
- **Text:** Startup row 3: "both are prospective values …, not accepted run state"; `RUN-INDEX`: "The certificate's index is the latest accepted turn's ordinal: 0 for the start turn."
- **Adjudication:** Confirmed as wording: in `Initial` no turn is accepted, so `RUN-INDEX`'s sentence read alone is false for that phase; the certificate's own field comment ("prospective 0 in Initial; latest accepted ordinal thereafter") and the enforcement note (`Initial` exposes no getters) fix the intended reading.
- **Witness:** KAV-05's reader expecting `Context::index() == 0` before `RunStarted` commits, contradicted by the startup row.
- **Fix direction:** "0 for the start turn once `RunStarted` commits; `Initial`'s stored 0 is prospective."

#### SYN-39 A Core section asserts a mechanism fact about "both shipped Environments" — MINOR, self-conformance violation
- **Sources:** gemini REV-02, kimi KK3-01, opus KV-14, sol SOL-10 (part)
- **Text:** §7 Notes: "Both shipped Environments check the latch before `next_event` selection and `dispatch` handoff, so an Error pending when either call begins returns first." §0: "Core sections build only on the contracts and never name an implementation — earlier mentions … are navigation only (the Scope line, the contract's pointer to its implementations, the bounds registry)."
- **Adjudication:** Confirmed as one cluster, four independent detections: the sentence sits in a Core section, matches none of the three exemptions, and — kimi's sharpening — tracks the *mechanism* level: the latch-first check appears only in the nonbinding §8/§9 Mechanism prose, so a conforming replacement mechanism falsifies the note while violating nothing (`LIVE-SELECT` binds only "follows `ENV-LATCH`'s publication ordering"). Worse, per SYN-02 the contract does not force the pre-check for anyone, which is the wrong signal to the `TRUST-ENV` audience.
- **Witness:** KK3-01's: a single-threaded replacement Live Environment whose publications and calls are totally ordered by construction, no pre-selection latch check, every guarantee row satisfied.
- **Fix direction:** Delete or reword at contract level; SYN-02's `ENV-LATCH` clause would give the note a citable row.

#### SYN-41 The record kind-marker needs a hand-written `Serialize` the Mechanism prose does not disclose — NIT, omission (executed)
- **Sources:** sole: sonnet CR-12 (severity moved from MINOR)
- **Text:** §7 Enforcement: "One payload struct per record, each deriving `Serialize`, its first field a kind-typed zero-sized value supplied by the shared `RecordPayload` trait."
- **Adjudication:** Confirmed at NIT: sonnet's execution is correct (a derived ZST serializes as `null`; the wire needs a hand-written impl plus `#[serde(rename = "record_kind")]`), but the passage is nonbinding realization prose, "each deriving" grammatically attaches to the payload structs rather than the marker, and the binding wire format (`RUN-RECORDS` + byte-exact golden tests) is unambiguous — a naive-reader trap in a sketch, not a false binding claim.
- **Witness:** CR-12's executed pair: naive derive → `{"kind":null,…}`; hand-written impl + rename → the documented bytes exactly.
- **Fix direction:** One sentence: the shared kind-marker's `Serialize` is one hand-written impl driven by `RecordPayload`'s tag.

### Batch 3 — Enforcement & verification pass (§0 + §12)

The enforcement-accounting pass: discharge §0's "Every ID outside the Obligations table is enforced" per D3, apply D6's Mechanism ruling and D7's placement declaration, and repair the `VERIFY-*`/`TRUST-*` rows. Touches `DET-RUN`'s row in §7 (SYN-12, SYN-13) and adds trust-mark citations at `RUN-FINALIZE` and in §9 (SYN-20).

**Contains:** `SYN-11`, `SYN-12`, `SYN-13`, `SYN-14`, `SYN-15`, `SYN-16`, `SYN-17`, `SYN-18`, `SYN-19`, `SYN-20`, `SYN-40`.

**Execution notes:**
- The approved **Proposed D3 disposition** table (Batch 0) is the scope for SYN-11 — execute it row by row; this batch's prose does not override it.
- Any new `TRUST-*` row needs all four cells (ID, obligation, upholder, verified-by); extended `VERIFY-*` rows stay single rows.
- SYN-20 is citation-only: trust marks are exempt from the backward rule, so cite `TRUST-BLOCKING` at `RUN-FINALIZE` and in §9 directly.
- Apply D6 (Mechanism as a declared nonbinding prose job) and D7 (the placement-rules declaration) in §0 here; SYN-40's three promotions (`JRN-ENCODE` encode-region size, `Never: Serialize` into the §4 API block, the §11 re-export rule) go wherever D6's answer sends them.
- §12's framing sentences ("complete trusted boundary") must still be true when the batch ends — re-read them last.
- Update Appendix A for every ID added or moved.

#### SYN-11 Behavioral rows with no locatable enforcement tier — MAJOR, unenforceable claim
- **Sources:** ox OA-01, opus KV-03, sol SOL-06, sol SOL-07, opus KV-08
- **Text:** §0's sentence above; `ENV-BOUNDS` ("Every operation preserves the Environment's own declared bounds"); `SIM-SELECT`'s cursor clauses; `LIVE-SELECT`'s stamp/dequeue clauses; `APP-EMIT`/`APP-OVERFLOW`/`APP-STATE`; `SIM-WAKEUP` last-call-wins; `SIM-STEPS`; `SIM-COMPLETION`; `ASSERT-INVARIANTS`, `BOUND-LOOPS`, `NO-UNWIND`'s first clause.
- **Adjudication:** Confirmed at MAJOR: the §0/§12 completeness claims are binding meta-rules and are provably false as written — auditing all seven `VERIFY-*` scopes, the named assertions, and the type system locates no tier for the listed rows. Verified the sharpest instances: `VERIFY-SIM` names only `SIM-LIFECYCLE`/`SIM-START`/`SIM-SHUTDOWN`, so a cursor-resetting Sim passes every named suite (SOL-06); no suite names bounds preservation, so a Live fan-in growing past capacity passes everything (KV-03); `VERIFY-CONFORMANCE` conditions on equal traces, so it cannot catch either. Two instances are weaker and noted as such: the answer passed to `classify` (KV-08) is in effect pinned by `VERIFY-JOURNAL`'s record-sequence golden tests, and a never-waking `next_event` (SOL-07) would hang `VERIFY-LATCH`'s overlap cases — enforcement exists but no text says so. KV-03's steelman stands for `BOUND-LOOPS`/`ASSERT-INVARIANTS` (readable as tier definitions belonging in §0, not guarantees) and reaches neither `ENV-BOUNDS` nor the behavioral clauses.
- **Witness:** SOL-06's: Slots 0 and 1 both `Open`, armed at time 100, cursor at 0; Slot 0 selected, re-arms at 100 — required next selection is Slot 1; an implementation resetting the cursor each `next_event` selects Slot 0 again, passes the `Open` assertion and every enumerated `VERIFY-SIM` case, and produces a different Event trace.
- **Fix direction:** Per-row tier accounting: add a contract-behavior suite (Sim scheduling, Live select/stamp, Application emit semantics), move the meta-rules (`ASSERT-INVARIANTS`, `BOUND-LOOPS`, `NO-UNWIND`) into §0's tier definitions or give them review-backed `TRUST` status, and cite `VERIFY-JOURNAL`/`VERIFY-LATCH` where they already carry a row.

#### SYN-12 `DET-RUN` is owned by no named suite — MINOR, self-conformance violation
- **Sources:** fable FBL-01, sonnet CR-05
- **Text:** `DET-RUN`; `VERIFY-CONFORMANCE` "compares every Core-owned discriminant and payload in **`DET-ENV`'s** list"; `TRUST-PURE`'s Verified-by: "Two runs … → identical Journal bytes and `DET-RUN`-equal exits."
- **Adjudication:** Confirmed: both reviews' audits check out — the only two-run repeatability check in the document is the Verified-by cell of a *trusted obligation*, which §0 assigns to the Application author as `TRUST-PURE`'s check, not to Kavod as `DET-RUN`'s enforcement. Cross-run equality is not unrepresentable and no assertion can check it within one run, so the suite tier is the only candidate, and no suite names it. Kept separate from SYN-11 because the fix is specific.
- **Witness:** FBL-01's tier audit (quoted above); the reverse-engineering path CR-05 describes is the only route a reader has.
- **Fix direction:** Add within-type repeatability cases to `VERIFY-CONFORMANCE` (run each scripted trace twice, compare `DET-RUN`'s list) and cite it from `DET-RUN`.

#### SYN-13 `DET-RUN`/`DET-ENV` omit the trust preconditions their own axiom states — MINOR, omission
- **Sources:** sole: sonnet CR-01 (severity moved from MAJOR)
- **Text:** A9: "under `TRUST-PURE` and `TRUST-SERIALIZE`, every Core-owned run output … is a function of …"; `DET-RUN` states no such qualifier.
- **Adjudication:** Confirmed as a citation-consistency gap, not a false claim: the Obligations architecture globally conditions every guarantee on the trusted boundary (that is what "trusted" means in §12), so the literal-falseness reading is not forced — but the document's own convention is to cite trust dependencies inline where they matter (`JRN-SINK` → `TRUST-SINK`; Application Notes → trusted obligation), and the two rows where the omission is most consequential are the two that skip it. Sonnet's witness (unstable-map-order `Serialize` breaking byte-reproducibility while violating no stated trait bound) is correct and shows why the qualifier belongs in the row.
- **Witness:** CR-01's `HashMap` payload: same build/trace, two iteration orders, divergent `CommandsPrepared` bytes — a `TRUST-SERIALIZE` violation invisible to every enforced rule.
- **Fix direction:** Add "under `TRUST-PURE` and `TRUST-SERIALIZE`" to `DET-RUN` (`DET-ENV` inherits via "`DET-RUN`'s premises").

#### SYN-14 `VERIFY-FAULTS`' enumeration reaches only one of four `CoreError` outcomes — MINOR, omission
- **Sources:** sole: opus KV-04
- **Text:** `VERIFY-FAULTS`: "exercises every edge: scripted sinks … and scripted Environments for each operation's `Err` and for a shutdown report carrying `Some(error)`."
- **Adjudication:** Confirmed: `Core(TimeRegression)` needs an `Ok` with a decreasing stamp, `Core(CommandBoundExceeded)` an over-emitting Application, `Core(ShutdownIncomplete)` a `{Incomplete, None}` report — none is an operation `Err` or a `Some(error)` report, so the headline "every edge" outruns the enumeration. `Core(IndexExhausted)` is correctly out of reach (unrepresentable tier). No other suite covers the classification step (`VERIFY-LIVE` covers producing `Incomplete`, not classifying it).
- **Witness:** KV-04's enumeration audit, quoted above.
- **Fix direction:** Extend the enumeration: a nonmonotonic-stamp Environment, an over-emitting Application, a `{Incomplete, None}` report.

#### SYN-15 `VERIFY-FAULTS`' cross-product includes an impossible `start` cell — MINOR, ambiguity
- **Sources:** sol SOL-09, terra TERRA-02 (severity moved from terra's MAJOR)
- **Text:** `VERIFY-FAULTS`: "this includes their cross-product, where the operation's Error remains the Fatal cause and the report's Error is discarded"; `ENV-SERIAL`: "After `start` returns `Err` there is no later call"; startup step 2: "finalization skips `shutdown`."
- **Adjudication:** Confirmed at MINOR: the literal cross-product contains `start Err × report Some`, which no conforming run can exercise — but the qualifying clause itself (a report whose Error is discarded) only makes sense where finalization calls `shutdown`, so the intended restriction is derivable, the suite is buildable, and no guarantee is unreachable. Terra's MAJOR was inflation.
- **Witness:** TERRA-02's: `start() -> Err(1)`, scripted `shutdown` report `Some(2)`; the Engine returns `Environment(Start)` and never observes `2`.
- **Fix direction:** One clause restricting the cross-product to post-`start` failures, and test the start-`Err`-no-shutdown rule separately.

#### SYN-16 `TRUST-ENV`'s stated verification means cannot check the obligation — MINOR, unenforceable claim
- **Sources:** sole: opus KV-13
- **Text:** `TRUST-ENV`: "upholds every Environment-contract row | Environment author | The conformance trace suite run against it."
- **Adjudication:** Confirmed: `VERIFY-CONFORMANCE` compares `DET-ENV`'s discriminant list, which cannot witness `ENV-BOUNDS`, `ENV-SEPARATION`, or `ENV-SHUTDOWN`'s no-further-consequential-work clause; and the definite description "The conformance trace suite" does not resolve, since `VERIFY-LATCH` is *also* described as "An Environment conformance suite." Same root shape as SYN-11 seen from the Obligations side; kept separate because the fix is in the Obligations cell.
- **Witness:** KV-13's: a bespoke Environment with an unbounded fan-in queue produces identical discriminants and Journal bytes on every suite trace.
- **Fix direction:** Name both suites in the Verified-by cell and add "review" for the rows no trace can witness.

#### SYN-17 `VERIFY-GRAMMAR`'s compile-fail suite has no stated mechanism that can live where the row places it — MINOR, unenforceable claim
- **Sources:** fable FBL-03 (grok raises the same as an unanswerable question)
- **Text:** `VERIFY-GRAMMAR`: "it lives where the module-private grammar types are visible"; `RUN-ENFORCEMENT`: "Certificate, phase, and transition types are module-private."
- **Adjudication:** Confirmed: standard compile-fail tooling compiles separate crates, from which `pub(super)` items are unnameable — every fixture fails with a privacy error regardless of the grammar (an accidentally-`Clone` certificate still "fails to compile" from outside), so the suite as naively hosted proves the wrong thing; an in-tree non-compiling fixture breaks the build. Realizable (an `include!`-based fixture reconstructing the module boundary), but the mechanism is unstated and the row's placement clause cannot be discharged as written.
- **Witness:** FBL-03's: a trybuild fixture attempting `certificate.clone()` fails E0603 before the absence of `Clone` is ever tested.
- **Fix direction:** Name the hosting mechanism in the row (e.g., a fixture crate that `include!`s `engine/record.rs` and attacks from the Engine's visibility position).

#### SYN-18 `PORT-STATE`'s "never reading the payload" has no tier and can have none as written — MINOR, unenforceable claim
- **Sources:** sole: muse KAV-02 (severity moved from MAJOR)
- **Text:** `PORT-STATE`: "routing by the Slot sum's discriminant alone and never reading the payload."
- **Adjudication:** Confirmed as unenforceable, downgraded from MAJOR: not unrepresentable (payloads are `Serialize` and inspectable), no assertion, no suite pins `PORT-STATE`, and `TRUST-ROUTING` covers only one-to-one routing and Error mapping. But muse's divergence is not observable in any Core-owned output (the payload-reading implementation's only difference is a side log outside Kavod's observation), so the consequence tier is MINOR.
- **Witness:** KAV-02's two `dispatch` fan-out matches, one logging a payload field before forwarding — both pass every named suite.
- **Fix direction:** Either move the clause to a `TRUST-*` row (wiring/Environment author, review-verified) or reword it as a derivation of A1 ownership rather than an enforced prohibition.

#### SYN-19 `VERIFY-LIVE` has no bullet for a completion racing shutdown's initiating instant — MINOR, omission
- **Sources:** sole: sonnet CR-04
- **Text:** `LIVE-SUPERVISION`: "The transition out of `Running` and the latch close are one linearized instant"; `VERIFY-LIVE`'s enumeration covers completion-before-shutdown and completion-concurrent-with-expiry only.
- **Adjudication:** Confirmed: the atomicity is realized by a lock only in nonbinding Mechanism prose, and the enumerated suite bullets bracket the race at both far ends but not at the initiating instant itself, leaving the linearization requirement's tier unstated — an instance of the Theme-2 shape with a one-bullet fix.
- **Witness:** CR-04's: an implementation flipping `Running` and closing the latch as two unsynchronized writes, a shell's `run()` returning in the gap; no bullet, read literally, is written to catch it.
- **Fix direction:** Add the bullet: a completion racing shutdown's initiating instant is classified premature or expected, never both or neither.

#### SYN-20 The hang consequence of `TRUST-BLOCKING` is disclosed only in §8's Notes, uncited where it is relied on — MINOR, omission
- **Sources:** sonnet CR-03, sonnet CR-13
- **Text:** `RUN-FINALIZE`: "… → call `shutdown` …" (no termination citation); `SIM-START`/`SIM-SHUTDOWN` cite `TRUST-SIM-PORT`/`TRUST-SPAWN` but not `TRUST-BLOCKING`; §8 Notes alone derive "`shutdown` remains blocked in a join and produces neither a `ShutdownReport` nor an `EngineExit`."
- **Adjudication:** Confirmed as one cluster (one citation pass fixes both): the obligation genuinely exists (`TRUST-BLOCKING` names Ports and destructors generically), so this is documentation-completeness, not a spec gap — but a hanging `SimPort::stop` stalls `SIM-START`'s cleanup exactly as the Live join tail stalls `shutdown`, and neither §9 nor `RUN-FINALIZE`'s row gives the reader the pointer, though §0's exemption for trust marks would permit it with no ordering problem.
- **Witness:** CR-13's: a Fatal at `Prepared` triggers `RUN-FINALIZE`'s `shutdown`; one Port's post-`run` teardown hangs forever; `Engine::run` never returns and no text at the row hints why.
- **Fix direction:** Cite `TRUST-BLOCKING` at `RUN-FINALIZE`'s shutdown clause and at `SIM-START`/`SIM-SHUTDOWN`; add a §9 derive mirroring §8's.

#### SYN-40 Mechanism prose is an undeclared fourth category and three passages carry load — MINOR, self-conformance violation
- **Sources:** sole: opus KV-05 (executed)
- **Text:** §0: "prose has exactly three jobs: define … derive … justify"; §6 Mechanism: "`new` computes `max_record_bytes.checked_add(1)` to size the buffer" (the only statement of the encode-region size); §4 Mechanism: "`Never`'s `Serialize` implementation is `match *self {}`"; §11: the `mod.rs` re-export rule.
- **Adjudication:** Confirmed: the three passages fail §0's own deletion test — the encode-region size has an executed divergence (a 61-byte non-object at `max_record_bytes = 60`: encode region max+1 → `NotAnObject`, region max → `BoundExceeded`; `JournalError` variant is Core-owned), `Never: Serialize` is required for the documented pattern to compile and appears in no API block's derive list, and deleting the re-export sentence changes public paths. Load-bearing facts in nonbinding clothing.
- **Witness:** KV-05's executed encode-region divergence, above.
- **Fix direction:** Either declare Mechanism a fourth, explicitly nonbinding prose job in §0 *and* promote these three sentences (encode-region size into `JRN-ENCODE`; `Never: Serialize` into the API block; the re-export rule into a row), or just promote them.

### Batch 4 — Live/Sim pass (§8 + §9, plus two Laws rows in §2)

One pass over the Live and Sim Port-facing rows, plus the two Laws-row wordings: A8's profile scoping (SYN-37) and `BOUND-STATIC` (SYN-36, requires D5).

**Contains:** `SYN-04`, `SYN-05`, `SYN-33`, `SYN-34`, `SYN-35`, `SYN-36`, `SYN-37`.

**Execution notes:**
- SYN-04: prefer the in-row fix (a saturation clause in `LIVE-SHUTDOWN`) — it stays inside closed text; construction-time validation belongs to open §10 and can be added at Wiring close instead. Pick one, don't do both halfway.
- SYN-05 edits both `LIVE-DISPATCH` and the Glossary's `Handoff`/`Admission` lines; those Glossary lines are disjoint from Batch 1's edits, but re-read them as merged before writing.
- SYN-36 applies D5; if D5 chose deferral, record the open question in §10's list instead of editing `BOUND-STATIC`.
- SYN-37's A8 fix is one scoping clause in the axiom's own sentence, citing `TRUST-ABORT` — do not restructure the axiom table or touch the Panics prose, which is already correct.
- SYN-33's fix lands in the `LiveCtx` API doc comments (binding under §0 form 1) — semantics there are normative even while exact signatures stay provisional.

#### SYN-04 Shutdown-deadline arithmetic overflow has no defined outcome — MINOR, omission
- **Sources:** kimi KK3-02, ox OA-03
- **Text:** `LIVE-SHUTDOWN`: "fixes one absolute shutdown deadline, the configured duration after that close"; A6: "Arithmetic on … times … is checked"; §10: "the shutdown deadline (nonzero milliseconds)".
- **Adjudication:** Confirmed: A6 forces the addition to be checked, `shutdown` returns no `Result`, the report's `error` field is defined as the latch's pending Error, and `Quiescence` has two variants — a checked failure has nowhere to go (ox), and the two natural recoveries diverge observably in `Quiescence`, a Core-owned compared payload (kimi). Reachable only under an absurd-but-legal configuration (`u64::MAX` ms), hence MINOR. §10's openness does not excuse it: the gap is in closed text (`LIVE-SHUTDOWN` + A6).
- **Witness:** KK3-02's: deadline `u64::MAX` ms; implementation A treats `checked_add` `None` as "no effective deadline" → can return `Quiesced`; implementation B treats it as already-expired → `Incomplete` with detached threads.
- **Fix direction:** Validate the deadline at construction (typed config error) or name saturation behavior in `LIVE-SHUTDOWN`; one sentence either way.

#### SYN-05 The per-Port Command inbox has two stated owners — MINOR, ambiguity
- **Sources:** opus KV-12, sonnet CR-10, ox OA-04
- **Text:** `LIVE-DISPATCH`: "Each destination Port owns one bounded Command inbox"; Glossary: "**Admission** — entry of a value into a Kavod-owned queue or inbox"; bounds registry: "per-Port Command inboxes | Live Environment"; Glossary: "**Handoff** — … transfer of one Command into its destination Port's ownership."
- **Adjudication:** Confirmed as one cluster (one ownership sentence fixes all three): `LIVE-DISPATCH`'s handoff commits at an "admission," which the Glossary defines only for Kavod-owned containers, while the same row says the Port owns the inbox; OA-04 adds that at the commitment instant the Command sits in a container the Port has not observed, leaving "into its destination Port's ownership" meaning value-ownership at best. Not CRITICAL despite two binding forms touching: "owns" in `LIVE-DISPATCH` reads naturally as possessive association ("has one inbox dedicated to it"), one clearly intended resolution — the Environment owns the inbox, the Port holds the receiving capability (`recv`/`try_recv`), the Command's value-ownership transfers at admission.
- **Witness:** KV-12's: ask A1's question of one inbox — who defines every way it can change? The Environment sizes it, admits into it, and closes it; the Port's only access is an owner-supplied capability, which is A1's word for *not* being the owner.
- **Fix direction:** "Each destination Port **has** one bounded Command inbox, owned by the Environment"; define Handoff as transfer of the Command value into the Port's exclusive inlet.

#### SYN-33 `try_recv`'s post-drain behavior and its `None` are unspecified — MINOR, ambiguity
- **Sources:** fable FBL-02 (kimi raises the same in its Questions section)
- **Text:** `recv` doc: "Once raised, every call reports the signal; `try_recv` is the draining path"; `try_recv` doc: "Nonblocking: pending Commands first, then the signal"; `LIVE-LIFECYCLE` repeats the ordering only.
- **Adjudication:** Confirmed: "every call reports the signal" sits in `recv`'s comment and does not bind `try_recv`; nothing fixes what repeated calls return after the signal has been yielded once, and the `None` case of the return type is never described. Two conforming drain loops behave differently.
- **Witness:** FBL-02's: signal raised, inbox C1,C2; four `try_recv` calls — `…, Some(Shutdown), Some(Shutdown)` vs `…, Some(Shutdown), None`; a `while let Some(input)` loop diverges.
- **Fix direction:** Once raised and drained, every `try_recv` returns `Some(Shutdown)`; define `None` as "no Command pending and no signal raised."

#### SYN-34 The fan-in queue's dequeue discipline is implied, not stated — MINOR, ambiguity
- **Sources:** sole: muse KAV-01 (severity moved from MAJOR)
- **Text:** `LIVE-EVENTS`: "Event fan-in is one bounded queue."
- **Adjudication:** Confirmed at MINOR, resolving the muse-vs-grok dispute mostly for grok: "queue" unqualified conventionally means FIFO (a LIFO is a stack), admission order among racing offers is already free by the §7 race-resolution derive, and muse's own witness concedes the traces differ — so no determinism row is breached and no truly conforming LIFO implementation exists under ordinary vocabulary. What survives is that the document's own discipline defines terms it leans on, "queue" has no Glossary line, and a replaceable-mechanism author could defensibly read "bounded queue" as covering a priority queue.
- **Witness:** KAV-01's paired FIFO/LIFO dequeues of concurrently admitted `Deposit`/`Withdraw`, divergent `balance` — valid as a consequence display, not as two conforming implementations.
- **Fix direction:** One clause in `LIVE-EVENTS`: "dequeue order is admission order."

#### SYN-35 `LIVE-SELECT` bounds the stamp on one side only — MINOR, ambiguity
- **Sources:** sole: opus KV-16
- **Text:** `LIVE-SELECT`: "The stamp is taken before the dequeue, and the dequeue is the consumption commitment."
- **Adjudication:** Confirmed: nothing relates the stamp to the wait or the Event's availability, so a stamp read at call entry — before a five-second wait — satisfies every word (`ENV-TIME` nondecrease included) while producing a `logical_time` seconds off the dequeue it stamps, in a Journal advertised as forensic evidence. The sentence's purpose (nothing fallible after consumption) is served by both readings; the intended one (stamp adjacent to the dequeue) is clear but unforced.
- **Witness:** KV-16's L_early/L_late pair: entry at 1_000 ns, Event admitted at 5_000_001_000 ns; stamps differ by five seconds for the same Event.
- **Fix direction:** "The stamp is taken immediately before the dequeue, after the wait."

#### SYN-36 `BOUND-STATIC` names registration as the Slot-order authority; §10 prefers declaration order — MINOR, ambiguity
- **Sources:** sole: opus KV-15
- **Text:** `BOUND-STATIC`: "Slot registration at construction fixes the Port set … and the Slot order: static, not configured, and fixed nowhere else." §10: "declaration order is the candidate that keeps one authority."
- **Adjudication:** Confirmed: a closed binding row prejudges an open decision against the option the open section itself prefers — if declaration order wins, the order is fixed by the `ports!` invocation and both of `BOUND-STATIC`'s clauses are wrong. Opus's steelman ("fixes" = locks in; "nowhere else" = at no later time) is available, not forced. Frozen Slot order is load-bearing in five closed rows, so the answers are observably different.
- **Witness:** §10's own phrase "keeps one authority" conceding the registration option produces two.
- **Fix direction:** Reword `BOUND-STATIC` to "Construction fixes the Port set and the Slot order" and let §10 choose the source.

#### SYN-37 A8's unconditional abort clause vs the disclosed test-profile unwind reaching `Stopped` — MINOR, ambiguity
- **Sources:** sole: terra TERRA-01 (severity moved from CRITICAL)
- **Text:** A8: "A panic — in Kavod or user code — is a bug: the process aborts, and no exit represents it." §2 Panics: "test code may catch panics under the test profile, which unwinds." `LIVE-COMPLETION`: "… or unwind under the test profile." §8 Notes: "under the unwinding test profile a panicked Port joins cleanly, so lifecycle tests read `Quiesced` as 'joined', never 'succeeded'."
- **Adjudication:** Downgraded from CRITICAL: terra's trace is reachable exactly as constructed (post-close unwind → guard marks `Complete`, nothing published because the completion is expected → clean report → `Stopped`), but the behavior is not a latent contradiction — it is the disclosed, designed test-profile consequence the §8 Notes derive and bless, and A8's abort clause is explicitly profile-scoped by the adjacent Panics prose, `NO-UNWIND`, and `TRUST-ABORT`. "No exit represents it" is literally true in the trace (the exit does not represent the panic). What remains is that the axiom's own sentence carries none of that scoping — an unconditional "the process aborts" that is false under a profile the same section admits.
- **Witness:** TERRA-01's trace, verified reachable, reclassified as the documented behavior rather than a contradiction.
- **Fix direction:** Scope A8's abort clause to the shipped profile in the axiom's own sentence ("under the shipped profile the process aborts…"), citing `TRUST-ABORT`.

### Batch 5 — NIT sweep (§3, §4, §9, §11, status line)

The cosmetic sweep: citation forms, spellings, layout homes, print fixes. No decisions needed; safe any time after Batch 0, but running it last keeps the earlier diffs clean.

**Contains:** `SYN-42`, `SYN-45`, `SYN-46`, `SYN-48`, `SYN-50`, `SYN-51`, `SYN-52`, `SYN-53`, `SYN-54`.

**Execution notes:**
- SYN-45 needs a home for the finite-source pattern *before* the citation can change — add the Glossary line (or ID) first, then repoint `SIM-COMPLETION`, in that order within this batch.
- SYN-54: renaming means `git mv design_docs/reveiws design_docs/reviews` **plus** the status-line pointer **plus** a repo-wide grep for the old path (this synthesis file lives in that directory; memory notes reference it too) — all in one commit, or decline the whole thing. Never fix the pointer without the directory or vice versa.
- SYN-46: prefer the wording fix ("before engine construction") over moving the justification.
- SYN-50 depends on D7's answer about whether §0's rule 4 reaches the Glossary; if D7 declared the rules binding, move the entry to a §9 `*Define:*`; otherwise the §0 exemption sentence suffices.
- Everything else is a single-line edit — keep it mechanical, no rewording beyond each fix direction.

#### SYN-42 The printed "complete expansion" of `ports!` does not compile verbatim — NIT, false claim (executed)
- **Sources:** sole: opus KV-21
- **Text:** §4 Mechanism: "Its complete expansion for the example above:" with `$crate::PortContract` in the printed block; Notes: "replaceable by hand."
- **Adjudication:** Confirmed: `$crate` is a macro-body token; the block invites hand-copying and fails to parse as printed (executed by opus); substituting `kavod::` compiles clean.
- **Witness:** KV-21's executed `error: expected identifier, found $`.
- **Fix direction:** Print the expansion with `kavod::`, or label the block the macro body.

#### SYN-45 `SIM-COMPLETION` cites "(Ports Notes)", a prose location, not an ID — NIT, self-conformance violation
- **Sources:** kimi KK3-04, opus KV-24, terra TERRA-04 (part), sol SOL-10 (part)
- **Text:** §0: "Cite IDs. Never section numbers"; `SIM-COMPLETION`: "the finite-source pattern (Ports Notes)."
- **Adjudication:** Confirmed, four independent detections: the target is §4's un-IDed `*Define:*` note, so the row cannot cite an ID even in principle — the lone prose-location pointer in the document (kimi's survey).
- **Witness:** KK3-04's survey of every other citation's form.
- **Fix direction:** Give the finite-source pattern a Glossary line or an ID and cite that.

#### SYN-46 §3's *Justify* forward-references `Engine::new`, defined four sections later — NIT, self-conformance violation
- **Sources:** kimi KK3-05, ox OA-07
- **Text:** §3 Notes: "anything fallible needed to build it happens before `Engine::new`"; §0: "Citations point backward … Navigation pointers are exempt: [list]."
- **Adjudication:** Confirmed: the mention matches no exemption class and the note's argumentative force leans on §7's construction table (kimi's steelman analysis). Ox's mitigation — the placement rules are prefixed "for every future edit" — is noted but not adopted (see Unresolved); rule 4 asserts current-text compliance, suggesting the set is meant to hold now.
- **Witness:** KK3-05's standalone-§3 reader.
- **Fix direction:** "before engine construction," or let the construction table carry the justification.

#### SYN-48 `SIM-STATE` names no contract row and defines no Port-facing API — NIT, self-conformance violation
- **Sources:** sole: opus KV-28
- **Text:** §9 head: "every guarantee below realizes a named contract row or defines the sim Port-facing API"; `SIM-STATE` does neither, restating `PORT-STATE`/`ENV-SEPARATION` uncited.
- **Adjudication:** Confirmed by inspection of the row.
- **Witness:** The row's text.
- **Fix direction:** Add the citations ("realizes `ENV-SEPARATION`, `PORT-STATE`") or fold the row into them.

#### SYN-50 "Sim Port lifecycle" is a Glossary line naming an implementation type; its Live analog is a local Define — NIT, placement
- **Sources:** sole: sonnet CR-15 (severity moved from MINOR)
- **Text:** Glossary: "**Sim Port lifecycle** — … of one bound SimPort"; §8's local "*Define:* Live completion state"; §0 rule 4.
- **Adjudication:** Confirmed at NIT: whether the Glossary counts as a Core section under rule 4 is genuinely unsettled (sonnet's question), the term is used only inside §9, and the asymmetry with the Live analog has no stated principle — but no obligation changes wherever the definition lives.
- **Witness:** CR-15's usage scan (no Core section needs the term).
- **Fix direction:** Move it to a `*Define:*` at the top of §9, or state in §0 that the Glossary is implementation-name-exempt.

#### SYN-51 `BuildError` leaves `TryReserveError` unqualified where §6 qualifies it — NIT, wording
- **Sources:** sole: gemini REV-01
- **Text:** §7: `CommandBuffer(TryReserveError)` vs §6: `AllocationFailed(std::collections::TryReserveError)`.
- **Adjudication:** Confirmed as an inconsistency nit only: API blocks assume ambient imports throughout (unqualified `Serialize`, `NonZeroUsize`), so "cannot compile in isolation" proves nothing the blocks claim — the defect is the two blocks disagreeing on qualification style for the same type.
- **Witness:** The two quoted variants.
- **Fix direction:** Qualify it in §7 (or unqualify §6); pick one style.

#### SYN-52 Three public API items have no home in the crate layout — NIT, omission
- **Sources:** sole: ox OA-05
- **Text:** §11 `engine.rs` and `environment.rs` item lists vs §7's public `EnvironmentFatal`, `EnvironmentOperation` and §5's public `ShutdownReport`.
- **Adjudication:** Confirmed at NIT: §11 declares itself mechanism, so omissions are nonbinding (kimi's observation) — but the section's own reachability claim ("Every public item is reachable at a path without repeated segments") is uncheckable for unhomed items.
- **Witness:** OA-05's searches.
- **Fix direction:** List them (`engine.rs` / `environment.rs` respectively).

#### SYN-53 "an empty Journal" after a failed first commit means committed-empty, not byte-empty — NIT, wording
- **Sources:** sole: kimi KK3-03
- **Text:** §7 Notes: "exits Fatal with real effects and an empty Journal"; `JRN-COMMIT`: "Bytes past the last committed record are an uncertain suffix."
- **Adjudication:** Confirmed: a short write before the failed flush leaves physical bytes; true only under the committed-records reading, which the sentence does not state (and which `JRN-COMMIT` supplies one section away).
- **Witness:** KK3-03's short-write-then-flush-failure.
- **Fix direction:** "…and no committed record."

#### SYN-54 The status line's `design_docs/reveiws/` carries the misspelling in the artifact's own name — NIT, cosmetic
- **Sources:** kimi KK3-06, ox OA-06
- **Text:** Status: "(`design_docs/reveiws/`)."
- **Adjudication:** Confirmed as cosmetic: the directory on disk *is* spelled `reveiws`, so the pointer resolves (verified; ox's "likely dangles" is wrong) — the anomaly is the misspelled directory name itself, spelled correctly in the same sentence's prose.
- **Witness:** `ls design_docs/` (executed here).
- **Fix direction:** Rename the directory and the pointer together, or leave both; never fix one without the other.

## Disputes resolved

- **terra TERRA-01 (CRITICAL) vs the field.** Terra alone rated the document unsound on the test-profile-panic-to-`Stopped` trace. The trace is real, but the §2 Panics prose, `NO-UNWIND`, `TRUST-ABORT`, and §8's Notes all disclose and derive exactly this behavior; only A8's own sentence lacks the scoping. Downgraded to MINOR (SYN-37). Terra's verdict of "Unsound" is rejected.
- **opus KV-09 vs fable's held "recordless edges cannot fail."** Fable held the claim by placing the snapshot in phase work (the Phases table's reading); opus showed `RUN-ENFORCEMENT`'s exactly-three runtime points force the snapshot into the transition (the private table agrees). Opus is right that the ambiguity is real; fable's reading survives only by ignoring the three-points inventory. Confirmed as SYN-22.
- **opus KV-17 vs grok/kimi/sonnet's held "Prepared fusion."** All three held the fusion attack by citing the reconciling sentence — which lives in nonbinding prose, which is opus's finding. The mechanism is sound; the binding forms don't say so. Confirmed as SYN-23.
- **muse KAV-01 (MAJOR) vs grok's held "queue is FIFO under the only reading 'queue' forces."** Grok is right on substance (no conforming LIFO exists; traces differ so determinism is untouched); muse's residue is a one-clause vocabulary gap. Downgraded to MINOR (SYN-34).
- **muse KAV-03 vs grok's and ox's held classifier-exactness.** Grok and ox are right: `JRN-ENCODE`'s "classified … exactly by" *defines* the byte-level classifier rather than asserting RFC validity, muse's own execution shows `RawValue::from_string` validates, and no safe `serde_json` path emits invalid JSON passing the three checks. Rejected.
- **sol SOL-06 vs sonnet's held "SIM-SELECT cursor fully pinned."** Both right on different axes: sonnet held the *specification* (no divergent conforming reading exists — correct), sol attacked the *enforcement tier* (no suite pins it — also correct). Folded into SYN-11 as its best witness.
- **opus KV-02 vs the five reviews that held `ENV-LATCH`'s trichotomy.** The held walks (deepseek, gemini, ox, sol, terra) covered the listed cases; opus found the unlisted corner — pre-commitment failure with a *prior-completed* publication, which sentence 3's "concurrent" does not cover. Opus is right. Confirmed as SYN-02.
- **terra TERRA-04 / sol SOL-10 (same-section forward citations) vs grok's held.** Grok is right: "Citations point backward" governs *section* order ("a fact that needs a forward reference is in the wrong section"); a row citing a neighbor row in its own section is in the right section. `SIM-TIME`→`SIM-WAKEUP`, `RUN-GRAMMAR`→`RUN-ENFORCEMENT`, `PORT-SUMS`→`PORT-ROUTING` rejected; the cross-section instances (SYN-46) and the non-ID citation (SYN-45) stand.

## Rejected findings

| Finding(s) | Claim | Why the text refutes it |
|---|---|---|
| muse KAV-03 | `JRN-ENCODE`'s three-byte classifier is "not exact" for arbitrary `Serialize` | "Classified as one single-line JSON object **exactly by**" defines the classification criterion; it asserts no RFC validity. Muse's own execution shows `RawValue::from_string` rejects invalid JSON, and no safe `serde_json` path emits invalid JSON passing the three checks (grok, ox held the same attack). |
| sol SOL-02 | `Engine::new`'s exhaustive table omits unavoidable destruction on failure | The table's scope is the Engine's fallible protocol steps. Disposal is Rust drop semantics whose conduct the document already assigns to value owners: `JRN-SINK` ("writer destructor behavior belong[s] to the sink's owner") and `TRUST-BLOCKING` ("destructors"). A deliberate `mem::forget` *would* be unlisted work and is thereby forbidden — the exhaustiveness claim cuts against SOL's witness, not for it. |
| sol SOL-04 | Lossy serialization falsifies `CommandsPrepared`'s "complete Command intent" evidence | §6 Derive states the bound the witness needs: "Lossy serialization is evidence only of the fields it emits." The Records-table claim reads under it; positional prefix identification (`Dispatch { position }` indexing the `commands` array) survives regardless of payload fidelity, and semantic recovery of external effects is `TRUST-KEY`'s job. |
| sol SOL-08 | Sim `stop`-in-Slot-order fails `ENV-SHUTDOWN`'s "observe the signal immediately" | `SIM-SHUTDOWN` (binding) defines "the `stop` call is the sim shutdown signal," and the sim is single-threaded: between the close and a Port's `stop`, no code of that Port can run, so "a means to observe immediately" can only mean at its next execution opportunity — the reading the binding row forces. |
| ox OA-08 | A2's "runs outside the turn" condemns the sim's synchronous `on_command` | The permissive reading (processing *need not* complete within the turn) is forced: `SIM-DISPATCH` is a binding row of an implementation §5 declares conforming. Ox concedes the forcing; grok and kimi held the same attack. No obligation is unclear. |
| ox OA-09 | The graph sketch's uniform "drop the certificate" failure arrow misses pre-mint failures | The sketch is labeled "Non-normative sketch; the two tables below are the guarantee," and it depicts the graph — startup steps 1–2 precede the graph and are handled by the startup table and `RUN-FINALIZE`'s start-`Err` arm. Within the sketch's scope the arrow is correct. |
| opus KV-10 | `LIVE-COMPLETION`'s set mismatches the supervisor set after a partial spawn | "Matching the frozen supervisor set and order (`BOUND-STATIC`)" — the citation fixes the referent: the construction-frozen bound-Slot set, not the dynamically-spawned subset. Under that forced reading the entry set always matches; the unowned-entry residue is unobservable (start returned `Err`; `ENV-SERIAL` forbids every later call), as opus's own "consequence is nil" concedes. |
| opus KV-30 | "Frozen" is undefined in three senses | Ordinary English with one uniform core meaning (fixed at its owner's fixing point, immutable after); the witness lists senses but constructs no divergent readings, and no Glossary binding exists to violate. §1 promises one line per *defined* term, not a line for every word. |
| opus KV-31 | `SIM-SELECT`'s cursor advance past the last Slot is unstated | Opus's own witness proves cursor `N` and cursor `0` select identically because the scan wraps — there are no two observably different readings, so no ambiguity exists under the review vocabulary. |
| opus KV-32 | `LIVE-SHUTDOWN`'s four-way instant vs the Mechanism's two-way lock | `LIVE-EVENTS` (binding) defines the fan-in close as the signal's publication, and the lifecycle cell the blocking points check *is* the signal — the two-element lock instant covers all four by the binding rows themselves; the Mechanism's brevity breaks nothing and is nonbinding. |
| sonnet CR-06 | "Sink failure" is misreadable under "fail/failure — no further meaning" | "Sink failure" has its own normative Glossary line spelling out all three outcomes, and `JRN-POISON` restates them; no reading survives in which `Ok(0)` does not poison. Sonnet's own text concedes "not a real contradiction." |
| sonnet CR-07 | `TurnOpen` bare vs `TurnOpen<A>` arity overload | Lives entirely in the table the document flags "neither an API block nor a binding table"; both renderings satisfy every binding row, so nonbinding notation shorthand creates no defect. |
| sonnet CR-08 | `DET-ENV`'s `Quiescence` equality reads falsely without §1 | The Glossary's Trace definition is binding and folds the `ShutdownReport` (hence `Quiescence`) into the trace as premise; a reader skipping the normative Glossary is not a conforming reader of a document whose §0 sends vocabulary there. |
| sonnet CR-09 | §3's Mechanism lacks its siblings' nonbinding disclaimer | §0's form rules make all such prose nonbinding by default (sonnet's own steelman: deleting the sentence changes no obligation); the disclaimer is redundant, so its absence is not a defect. |
| sonnet CR-14 | `remaining()` conflates exactly-full with overflowed | Fully specified behavior; no rule promises the distinction, `APP-OVERFLOW` gives the Core consequence, and a handler can distinguish by checking `remaining()` before an `emit`. A design suggestion, not a defect. |
| sonnet CR-17 | "Fatal" names both the run classification and `Outcome::Fatal` | The Glossary defines the run-level term; `Outcome::Fatal` is an API item name; the one collision site (`TurnOpen` overflow) explicitly rules the precedence and the payload's discard. No rule has two readings. |
| sonnet CR-19 | The race-resolution freedom appears only in §7 despite §5's completeness | §5's completeness claim means satisfying every row suffices — race resolution is unconstrained by any row, so an implementor resolving races arbitrarily conforms with no extra license needed; the §7 sentence is a *derive* for Run readers stating a consequence of that silence, correctly placed. |
| terra TERRA-04 (part), sol SOL-10 (part) | Same-section forward row-citations violate "Citations point backward" | The rule governs section order: "a fact that needs a forward reference is **in the wrong section**." A row citing a same-section neighbor is in the right section; `SIM-TIME`→`SIM-WAKEUP`, `RUN-GRAMMAR`→`RUN-ENFORCEMENT`, `PORT-SUMS`→`PORT-ROUTING` are all intra-section (grok held the same). |

## Unresolved

- **Do §0's placement rules claim current-text compliance?** The block is prefixed "Placement rules, for every future edit:" — read as forward-only, the citation/placement NITs (SYN-45, SYN-46, and parts of SYN-39) are discipline debt rather than violations; read as binding now, they are violations. Rule 4's own text asserts current compliance ("earlier mentions … are navigation only"), which is why this synthesis treated the set as binding now — but the prefix supports the other reading, and only the author's intent settles it. Either answer leaves the fixes worth making.

## Held under fire

- **`RUN-FINALIZE`'s three quiescence arms** — opus enumerated all 17 Fatal-producing points against the guards: pairwise disjoint, none matches zero or two; deepseek, kimi, ox, sol, terra walked the same branches independently.
- **The Run graph and certificate grammar** — every phase × input walked by grok/opus/sonnet/kimi; no unlisted transition, `Closed` only via a clean report, `Stopped` ⇒ clean report structurally.
- **`ENV-LATCH`'s core state machine** — the four states × every operation, both discard clauses load-bearing and present, unreachable cells proven unreachable (opus, fable, deepseek, ox, sol) — the two confirmed MAJORs are corners *outside* the listed lattice, not breaks in it.
- **Journal byte arithmetic** — max/max+1/max+2, `usize::MAX` → `MaxBytesTooLarge`, zero-progress, over-report, `Interrupted`, poison one-way (executed by fable and opus; walked by all).
- **Record wire format** — the documented `RunStarted` bytes reproduced byte-for-byte from the described mechanism (fable, opus, executed).
- **`ports!` expansion and `Never`** — compiles as claimed including associated-type projections and uninhabited discharge (fable, opus, muse executed; modulo SYN-42's `$crate` printing nit).
- **`RUN-INDEX`'s domain edge** — check-before-`next_event` exact at `u64::MAX`, no off-by-one, overflow unreachable (everyone who walked it).
- **`TurnOpen` overflow-beats-`Outcome`, Fatal-payload discard** — held (kimi, muse, sonnet).
- **`StopPending`'s retained quiescence** — survives `TurnCompleted(Stop)` commit failure on every path (grok, fable, ox, sol, sonnet).
- **Live shutdown deadline machine** — one deadline, no restart, final synchronized observation, join-tail honesty (deepseek, gemini, kimi, opus, ox, sol).
- **`SIM-LIFECYCLE` totality and `Ended`-arm unreachability** — twelve (state, method) pairs decided; selection provably never meets an `Ended` Port (fable, grok, kimi, opus, sonnet).
- **`SIM-SELECT`/`SIM-STEPS`/`SIM-TIME`** — cursor, budget fencepost, inductive monotonicity all pinned as specified (opus, kimi, sonnet — SYN-11 is about *enforcement*, not the specification).
- **`DET-RUN`/`DET-ENV` divergence hunts** — every constructed divergence either changed the trace or was an Error value the rows explicitly erase (fable, grok, kimi, ox, sonnet).
- **Typestate realizability** — full skeleton compiled and run: affine certificates, consuming `shutdown`, `PhantomData<fn() -> P>`, disjoint borrows in `dispatch_batch` (opus executed; grok, ox reasoned).
- **A5 vs activation/consumption preceding their records** — the records announce the handler call, not those effects; both consequences derived explicitly (grok, opus).
- **Appendix A** — all 76 IDs reconciled, none missing, none doubled, no section-number citation anywhere (opus).

## Disposition index

Every tagged finding from every review, exactly once. (grok: no findings — nothing to index.)

| Review: tag | Disposition |
|---|---|
| deepseek: OCN-01 | SYN-22 |
| deepseek: OCN-02 | SYN-06 |
| fable: FBL-01 | SYN-12 |
| fable: FBL-02 | SYN-33 |
| fable: FBL-03 | SYN-17 |
| fable: FBL-04 | SYN-43 |
| gemini: REV-01 | SYN-51 |
| gemini: REV-02 | SYN-39 |
| kimi: KK3-01 | SYN-39 |
| kimi: KK3-02 | SYN-04 |
| kimi: KK3-03 | SYN-53 |
| kimi: KK3-04 | SYN-45 |
| kimi: KK3-05 | SYN-46 |
| kimi: KK3-06 | SYN-54 |
| muse: KAV-01 | SYN-34 (severity moved MAJOR→MINOR) |
| muse: KAV-02 | SYN-18 (severity moved MAJOR→MINOR) |
| muse: KAV-03 | rejected (row 1) |
| muse: KAV-04 | SYN-21 |
| muse: KAV-05 | SYN-32 |
| opus: KV-01 | SYN-01 |
| opus: KV-02 | SYN-02 |
| opus: KV-03 | SYN-11 |
| opus: KV-04 | SYN-14 |
| opus: KV-05 | SYN-40 |
| opus: KV-06 | SYN-27 |
| opus: KV-07 | SYN-03 |
| opus: KV-08 | SYN-11 (weak instance; arguably covered by `VERIFY-JOURNAL`) |
| opus: KV-09 | SYN-22 |
| opus: KV-10 | rejected (row 7) |
| opus: KV-11 | SYN-24 |
| opus: KV-12 | SYN-05 |
| opus: KV-13 | SYN-16 |
| opus: KV-14 | SYN-39 |
| opus: KV-15 | SYN-36 |
| opus: KV-16 | SYN-35 |
| opus: KV-17 | SYN-23 |
| opus: KV-18 | SYN-07 |
| opus: KV-19 | SYN-08 |
| opus: KV-20 | SYN-26 |
| opus: KV-21 | SYN-42 |
| opus: KV-22 | SYN-28 |
| opus: KV-23 | SYN-30 |
| opus: KV-24 | SYN-45 |
| opus: KV-25 | SYN-47 |
| opus: KV-26 | SYN-29 |
| opus: KV-27 | SYN-44 |
| opus: KV-28 | SYN-48 |
| opus: KV-29 | SYN-49 |
| opus: KV-30 | rejected (row 8) |
| opus: KV-31 | rejected (row 9) |
| opus: KV-32 | rejected (row 10) |
| ox: OA-01 | SYN-11 |
| ox: OA-02 | SYN-38 |
| ox: OA-03 | SYN-04 |
| ox: OA-04 | SYN-05 |
| ox: OA-05 | SYN-52 |
| ox: OA-06 | SYN-54 |
| ox: OA-07 | SYN-46 |
| ox: OA-08 | rejected (row 5) |
| ox: OA-09 | rejected (row 6) |
| sol: SOL-01 | SYN-25 (severity moved MAJOR→MINOR) |
| sol: SOL-02 | rejected (row 2) |
| sol: SOL-03 | SYN-21 (severity moved MAJOR→MINOR) |
| sol: SOL-04 | rejected (row 3) |
| sol: SOL-05 | SYN-10 (severity moved MAJOR→NIT) |
| sol: SOL-06 | SYN-11 |
| sol: SOL-07 | SYN-11 (weak instance; `VERIFY-LATCH`'s overlap cases hang on a non-waking implementation) |
| sol: SOL-08 | rejected (row 4) |
| sol: SOL-09 | SYN-15 |
| sol: SOL-10 | split: shipped-Environments witness → SYN-39; "(Ports Notes)" witness → SYN-45; same-section forward citations → rejected (row 18) |
| sonnet: CR-01 | SYN-13 (severity moved MAJOR→MINOR) |
| sonnet: CR-02 | SYN-09 |
| sonnet: CR-03 | SYN-20 |
| sonnet: CR-04 | SYN-19 |
| sonnet: CR-05 | SYN-12 |
| sonnet: CR-06 | rejected (row 11) |
| sonnet: CR-07 | rejected (row 12) |
| sonnet: CR-08 | rejected (row 13) |
| sonnet: CR-09 | rejected (row 14) |
| sonnet: CR-10 | SYN-05 |
| sonnet: CR-11 | SYN-24 |
| sonnet: CR-12 | SYN-41 (severity moved MINOR→NIT) |
| sonnet: CR-13 | SYN-20 |
| sonnet: CR-14 | rejected (row 15) |
| sonnet: CR-15 | SYN-50 (severity moved MINOR→NIT) |
| sonnet: CR-16 | SYN-31 |
| sonnet: CR-17 | rejected (row 16) |
| sonnet: CR-18 | SYN-28 |
| sonnet: CR-19 | rejected (row 17) |
| terra: TERRA-01 | SYN-37 (severity moved CRITICAL→MINOR) |
| terra: TERRA-02 | SYN-15 (severity moved MAJOR→MINOR) |
| terra: TERRA-03 | SYN-38 |
| terra: TERRA-04 | split: "(Ports Notes)" witness → SYN-45; `SIM-TIME`→`SIM-WAKEUP` forward citation → rejected (row 18) |

**Completeness check:** 93 tagged findings (deepseek 2, fable 4, gemini 2, grok 0, kimi 6, muse 5, opus 32, ox 9, sol 10, sonnet 19, terra 4); 93 rows above (two split rows dispositioned per witness). Every tag appears exactly once. ✓
