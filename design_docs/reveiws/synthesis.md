# v12 Review Synthesis

**Date:** 2026-08-19
**Inputs:** the eight reviews in this directory (`deepseek`, `fable`, `gemini`, `grok`, `k3`, `opus`, `sol`, `terra`), all targeting `design_docs/design-v12.md`.
**Method:** every finding was clustered across reviewers; consensus items were accepted on convergence; contested and singleton items were adjudicated against the doc's own text, two by dedicated adversarial verification agents and two by running actual `serde_json` code. Findings a reviewer rated critical did not keep that rating unless the claim survived. The 2026-08-18 fable pass's refutation record (compile-fail siting, TRUST-SPAWN, A4-as-contradiction) was honored — re-raised versions of refuted attacks were re-checked, not re-accepted.

**Reviewer note:** `sol` and `terra` overlap heavily and were saved at the same minute; their shared findings are counted as correlated, not independent.

---

## Bottom line

**No finding requires a semantic redesign.** The run graph, latch state machine, ShutdownReport matrix, finalize trichotomy, Journal byte arithmetic, and index arithmetic survived eight independent attacks — several reviewers attacked the same mechanisms and independently reported them sound. What did not survive is concentrated in four places:

1. **One genuinely new bookkeeping gap** — the certificate's `last_time` is written by no binding row (Tier 1, item 1).
2. **The known owed fixes** — A9 overclaim, A4 wording, SIM lifecycle-open, the transition-table rendering — every one independently rediscovered by multiple fresh models, which settles that they are real.
3. **A vocabulary defect with teeth** — "Publication" overloaded in LIVE-SHUTDOWN, found by six of eight reviewers, whose strict reading makes every clean live shutdown report `Incomplete`.
4. **The Enforcement layer's normative status** — binding text delegating into non-binding forms, found by five reviewers.

Everything else is a one-sentence edit, a Wiring-close checklist item, or a judgment call. The rejected-findings list at the bottom matters as much as the confirmed list: three of deepseek's three "criticals" and two of opus's "blockers" do not survive scrutiny.

---

## Tier 1 — fix before implementation

### 1. Certificate time bookkeeping is unwritten *(new; the most substantive finding of this round)*
**k3 (G1, its flagship), opus (K15), fable (V12-02, partial)** — three reviewers hit the same seam from different angles. The startup table mints the certificate "consuming the Journal" and nothing else; `run_started(start_time)`'s Does column is literally "—"; Enforcement prose gives the update rule for `index` ("becomes the certificate's on success") and never for `last_time`. Yet `last_time` must equal the frozen start time for `Context::logical_time` at index 0 and for the first `EventAccepted` nondecrease check. The tables are declared exhaustive ("work it does not list does not happen"), so under the doc's own discipline the certificate's clock is never set. Same family, same fix pass: **k3 G2** — no row states the clean report survives a failed `TurnCompleted(Stop)` commit so `RUN-FINALIZE` can "reuse that report"; and **sol M-01** — say what the `Initial`-phase field values mean pre-commit.
*Fix:* one clause each in the mint/`run_started` row (`last_time` := the frozen start time), `accept_event` (the checked time becomes `last_time` on success), and `close` (the clean report is retained into the Fatal path on commit failure).

### 2. ENV-LATCH's "linearized" is undefined — adjudicated: ambiguity, not the claimed blocker
**opus (K8/K9, "blocker"), grok (H4)** attacked: the live mechanism reads the latch, then stamps/routes, then commits — a publication landing in that gap is real-time-before the commitment, so either no shipped implementation conforms or the rule is vacuous. **fable and k3** independently verified the same paragraph as sound. A dedicated verification pass settled it: **both sides are half right.** The attackers' reading is textually available (the row anchors on "commitment", and the term "linearized" is defined nowhere — the Glossary has no entry, and LIVE-SHUTDOWN uses "linearized" in a different sense). But the commitment-table preamble ("binds outcomes, not instants... the returned value is the caller's only witness") selects the coherent reading, under which the mechanism conforms, the rule is non-vacuous (the liveness clause and "stays pending" forbid loss), and the conformance bullet is testable with scripted timings. The attackers' proposed re-anchoring on "observation" would be worse — it mandates internal structure and misfits `take_error` and the close.
*Fix:* one sentence appended to `ENV-LATCH`: the order is the implementation's, not wall-clock's — a publication complete before the call orders before its commitment, one begun after the return orders after, one concurrent with the call may be placed on either side; the returned value witnesses the choice. Satellite fixes in the same pass: `SIM-DISPATCH`'s row should carve out the latch-first return the sim Mechanism already performs (**opus K11, gemini m2**), and `ENV-LATCH`'s "returned as that operation's `Err`" should read "per its commitment row" since `take_error`/`shutdown` have no `Err` channel (**fable V12-16**).

### 3. "Publication" collides with completion tracking — six of eight reviewers
**fable (V12-06), gemini (M3), grok (C1, rated critical), k3 (A8), opus (K37/K14), sol (M-04).** The Glossary binds Publication = "entry of an Error into the latch"; LIVE-SUPERVISION guarantees expected completions "stay unpublished"; LIVE-SHUTDOWN then "waits... for completion publications, joining publishers." Strictly read, a clean live shutdown waits for publications that can never come and every clean run reports `Incomplete` → `Core(ShutdownIncomplete)`. Grok and opus escalate this to "live `Stopped` unreachable" because the mechanism's "completion tracking" is not a binding form; whether or not one accepts the strict reading, **the fix is identical and one word deep**: rename the completion-tracking signal ("completion notices"), and connect it to LIVE-SHUTDOWN's row so the quiescence wait has a binding target. Same pass: scope "while `Running`" over both conjuncts in LIVE-SUPERVISION and state that classification-at-lock-time is definitional (**fable V12-17; opus K13 and sol M-03 are this same race, correctly resolved by the close's linearization — wording, not semantics**).

### 4. Live shutdown deadline semantics under-written
**opus (K14), sol (M-04), k3 (A9), gemini (m3), deepseek (c8).** Real kernel after stripping overstatement: the deadline as written bounds the *wait for notices*, not the joins after them; a joined thread's post-notice teardown is trusted-bounded (`BOUND-BLOCKING` names destructors), so opus's "hangs forever" requires a trusted-code violation — but the doc never says whether the deadline is a total budget or per-wait, "joining publishers in Slot order" reads as serialized joins that could starve later ones, and "prompt by construction" claims more than the mechanism delivers.
*Fix:* define the deadline as one absolute bound on all of `shutdown`'s waiting; state that joins follow completion notices (the notice is the wait's target, the join is bounded-after by trusted teardown); replace "prompt by construction" with the structural claim (deepseek's phrasing works).

### 5. The Enforcement layer's normative status — five reviewers
**fable (V12-05), k3 (G4/G5), opus (K32–K36), sol (M-05), grok (M8).** Binding text delegates into non-binding forms: `RUN-GRAMMAR` says "the residue that stays runtime is listed there" (Enforcement is prose); §10 declares the transition set "fixed"; the Journal's `commit` step table is the *only* home of the `{`/`}` test, the newline bound, and the write-retry loop; the always-on-assertions requirement lives in a §2 paragraph; §12's test suites are prose bullets §0's third enforcement tier depends on; the Certificate's "No `Clone`, `Copy`, or `Default`" cannot be stated under §0's "further derives are free" (**opus K33**). One decision resolves the family: admit the transition table and the Journal step table to the binding-table list, add a Laws row for always-on assertions, give §12's suites IDs, and amend §0 form 1 to let a block list prohibited derives.

### 6. The transition mechanism as rendered cannot type-check *(known owed; reconfirmed)*
**fable (V12-04), gemini (M1), opus (K1/K2/K3), sol (M-05).** `dispatch_batch(env, &[C])` cannot move `C` by value into `dispatch` without `Clone`; `no_commands()` has no operand for its assert; `Checkpointed<answer>` is a value-dependent return type. Known fix shape: drainable/owned batch, a batch view to `no_commands`, two typed checkpoint successors. No record, guarantee, or failure semantics change.

### 7. A9 overclaims *(known owed; now unanimous)*
**fable (V12-03), grok (M6), k3 (C1), sol (H-03), terra (#1), deepseek (M6-adjacent).** Error values are erased from the trace; `EngineExit` carries them; `DET-RUN` carries the hedge, A9 doesn't. *Fix:* "every Core-owned run output…" plus the `(TRUST-PURE)` trust mark (fable V12-21).

### 8. A4's second sentence — drafting hazard, not contradiction
**grok (H2), opus (K10), sol (H-01)** claim a live contradiction; **fable (V12-13)** refuted it (the phrase is anaphoric to "the first Error *the run observes*"; `RUN-FINALIZE`'s "(A4: a cause exists)" is the binding gloss; the rival parse makes the latch-close arm dead code) — and **k3 (A2/A3)** independently reached the same adjudication: consistent, but only under a reading the text doesn't force. Scoreboard: roughly half the fresh reviewers misparsed an axiom. That *is* the finding.
*Fix:* restore the elided clause ("once a first **observed** Error or fatal Core condition exists…"). Add k3-A2's one-sentence forensic note: a pre-commitment failure with a concurrent unobserved publication discards the earlier root cause at finalization — intended, currently unnamed.

### 9. SIM Port lifecycle "open" undefined *(known owed; reconfirmed with new teeth)*
**fable (V12-01), opus (K12), sol (H-04).** Nothing defines when a lifecycle opens or whether a clean `stop` ends one; on a mid-order start failure, whether a never-started Port receives `stop` has two conforming answers — and opus adds that stop-before-start can panic a Port that allocates in `start`, aborting startup with an empty Journal. *Fix:* sol's shape: `NotStarted → Open → Ended`; opens at `start`'s invocation, ends at first `Err` or `stop`; never-started Ports get nothing. Same pass: rescope `TRUST-SPAWN` from "before `run` returns" (live-only vocabulary) to "before its last method returns" so it reaches SimPorts (**opus K27's kernel**).

---



## Tier 2 — confirmed wording, one batchable editing pass

- **Axiom glosses overreach.** A1's "read-only view" doesn't describe `emit`/`set_next`/`offer` — the design is "one owner, others hold capabilities" (grok H3); A2's "the turn ends at handoff" is falsified by the record table and by sim's synchronous `on_command` (fable V12-15, grok H1, opus K18, sol M-06); A3's universal quantifier collides with `APP-STATE`'s "no commitment point" and Port-internal effects (deepseek M9, grok H7, sol H-02). Scope each the way the detailed rows already do.
- **Replay derive is misleading — verified with a counterexample.** grok (H6) called it false, k3 (G8) under-covering, fable defended "needs" as necessity. Adjudication: necessity reading is correct, but the note is recipe-shaped and the recipe provably diverges: two Slots, one armed at `start` in replay where the original armed via `on_command`, produces an equal-time tie the original never had, and the cursor picks a different Slot — byte divergence at the first `EventAccepted` with all three preconditions satisfied. Arm *placement* is behavior no recorded artifact captures. *Fix:* one scoping clause ("necessary, not sufficient: multi-Slot replay must also reproduce when each arm was placed; failure runs must reproduce each Error's presence at its trace position; the single-Port case needs the three alone").
- **RawValue breaks JSONL framing — verified by execution.** opus (K23), sol (M-10). `RawValue::from_string("{\n \"a\": 1\n}")` is accepted, serializes verbatim with embedded newlines, and passes the `{`/`}` check — one record, three physical lines. *Fix:* extend `TRUST-SERIALIZE` (no unescaped newline in an encoded record) and optionally scan the buffer before the newline append.
- **The newtype serde Derive is factually wrong — verified by execution.** opus (K47), k3 (N8), sol (L-04). A newtype around a map serializes as `{"k":1}` — an object. The doc's own `EventIndex` relies on newtype transparency two sections earlier. Fix the Derive's claim.
- **SIM-SELECT's tie-break is not one algorithm.** grok (M3), opus (K52): scan-from-cursor and wrap are implied, never stated; two conforming sims could pick different equal-time winners. State the selection function. (Deepseek's version of this — cursor must be recorded in the trace — is wrong; see rejected list.)
- **Sim subordinate effects named for only one of three `Err` paths.** fable (V12-08), k3 (G6), opus (K53). One shared clause covering `SIM-STEPS` exhaustion and mid-selection `SIM-COMPLETION`.
- **"Accepted" / count-vs-ordinal vocabulary.** k3 (C2), opus (K42), sol (L-01): the Glossary's "Accepted" excludes the start turn three binding texts call accepted. Redefine over both acceptance records; pick count or ordinal.
- **`ports!` "naming stem" unimplementable in `macro_rules!`.** fable (V12-11), gemini (m1), opus (K51): the `Event =`/`Command =` idents must be the output names; say so.
- **Evidence holes to name (the doc's own honesty standard):** the `CommandBoundExceeded` intent vacuum — the run dies with no record of what the handler staged (k3 G9; deepseek M2's variant-payload proposal is the design option); observation-vs-cause at the API surface — one clause in the `Prepared` row ("not handed off" reads as attempted-and-rejected) plus a struct-level doc on `EnvironmentFatal` (k3 G11); generalize `JRN-COMMIT`'s uncertain suffix from "sink failure" to any non-flush termination (deepseek M4); the `Journal(CommandsDispatched)` full-handoff case in the prefix-identification sentence (grok M9).
- **Meta-rule self-conformance batch:** `PORT-ROUTING` naming sim/live inside §4 plus the mapping-site being wrong for `start`/`step` errors anyway (fable V12-12, k3 C3+G7, terra #5); Appendix A omits A1–A9 (fable V12-20, k3 N6, opus K46); citation-form violations — "(section 10)", "(Enforcement)", generic "(Obligations table)" (k3 A12/N1/N2, opus K44); `BOUND-*` prefix spanning enforced and trusted rows (grok, k3 A13); the Glossary missing from the forward-reference exemptions (k3 G12); `ENV-ERRORS`'s "binding row" is informal vocabulary (sol/terra); `ENV-SHUTDOWN`'s queue-flavored phrasing vs the sim (opus K43 — terra's #4 severity rejected); `BOUND-STATIC`'s "thread count" in a Laws row and the three freeze points (opus K38/K56, sol M-12); the §7 table header "States" vs the Glossary's own word "Phase" and `Certificate<W, S>` vs `EngineExit<S,…>` (opus K40); `JournalFatal.record_kind` doc comment (fable V12-14); "a reported Error" vs the latch state (fable V12-18); `JRN-POISON`'s "failure" vs the two-word rule (fable V12-19); the garbled RUN-INDEX assert sentence (fable V12-22); SIM-WAKEUP never says arms start disarmed (sol M-07); dead-Port arms' unreachability unstated (grok M2 — grok itself showed it unreachable); sim `next_event`'s latch path stated only by citation (grok M4); the step-3 empty-buffer guard (gemini m4); an `Interrupted`-poison justification line (deepseek m4); "observationally identical" undefined (deepseek m7); `PORT-STATE` "without interpreting" (deepseek m12); `DET-ENV`'s dead "report's Error presence" comparand (fable V12-10, k3 A4, grok M6); `TRUST-PURE`'s "identical exit" uncomparable for `state: S` — restate as DET-RUN-equal Core-owned content (opus K7, downgraded: variant comparison works by matching).

---

## Tier 3 — Wiring-close checklist (real, already queued by §10 being open)

- **Error-sum composition:** sim mapping sites are `start`/`step`/fan-out-arm, not "the fan-out arm" (k3 G7, terra #5); the premature-closure variant needs a defined value (k3 G3); inbox-`Closed` needs a variant distinct from "exhaustion" — a Port whose `run` already returned surfaces as a full inbox and misdirects the operator (grok M5, opus K54).
- **Bounds:** `Send + 'static` on `A::Event` and the live Error sum (opus K6); `NonZero*` types for every Wiring capacity (deepseek m8); nonempty-Port-set enforcement siting (deepseek m10) and its scope vs `TRUST-ENV` (sol H-06 — bespoke environments currently escape `BOUND-STATIC`; a scope note suffices).
- **Slot-order authority** — decide registration vs declaration order; four closed guarantees' tests wait on it (opus K39).
- **Obligations table completeness:** fan-in queue capacity, shutdown deadline, and step budget are run-Fatal when undersized and have no row, unlike `BOUND-INBOX` (opus K24).
- **API-block completeness:** declare `Engine`, `SimCtx`, final `LiveCtx`; put `Never: Serialize` in a binding form (three reviewers: opus K5, grok M8, sol M-08); `EventIndex`'s crate-internal construction (opus K4 — minor, `pub(crate)` suffices); `TurnOutcome`/`RecordPayload` referenced but never declared (opus K59).
- **TRUST-PURE's subject list:** the Application value itself (`&self` is live authority on every turn — opus K20), `initial_state` (terra #2, opus K21 — note the design already treats initial State as a premise, so this is a scoping clarification, not a hole), and Error types' `Drop` (sol M-09); reassign "Ports share no state" to Port/wiring authors (sol H-05, terra M-06); strengthen the two-run verification note (separate processes / perturbed environment — opus K21, k3 N11).
- **Lossy serialization vs evidentiary claims:** either scope `CommandsPrepared`'s "complete Command intent" to serialized intent, or require `TRUST-KEY`'s business key to appear in the encoding — post-abort reconciliation reads bytes (terra #3, sol M-11).

---

## Tier 4 — design judgment calls (not defects; Devon decides)

- **Liveness has no owner.** All Ports blocked in `recv` + Engine in `next_event` is a live deadlock with no cancellation channel; the mitigations are Define-notes that create no obligation. Opus (K19) proposes a `TRUST-CANCEL` row. The sim has `SIM-COMPLETION`; live has nothing. Cheap to add, philosophically consistent with the existing trust rows.
- **An oversized inbound Event is a remote run-kill** — `EventAccepted` must commit before `on_event` (A5), so the Application can never see or reject it; the only guard is `BOUND-SIZING` config review (opus K22, k3 N9). Options: a Port-author size obligation, or accept and document.
- **Forensic enhancements:** `committed_bytes` in the exit for suffix truncation (deepseek M5); a run id / per-record `schema_version` if appended sinks stay allowed (opus K28); a Slot-identity record for latched-cause runs (deepseek C3's kernel — note the *exit* already names the Slot via the mapped Error variant; only the Journal-alone view lacks it, which the doc currently documents as intended).
- **offer-retry vs inbox-drain:** a Port retrying `offer` under backpressure isn't draining its inbox; widen `TRUST-LIFECYCLE` to retry loops or add the interleaving derive (opus K25/K26).
- **SimPort `stop` errors are always discarded** — a `stop` `Err` on the Stop path still yields `Quiesced`/`Stopped` (opus K27). Consistent with A4's close arm; decide whether that's the wanted evidence story.

---

## Rejected findings — do not fix, kept so they aren't re-raised

| Claim | Source (rating) | Why rejected |
|---|---|---|
| Trace input/output dual role is a "foundational ambiguity" | deepseek C1 (critical) | Standard determinism phrasing: same history ⇒ same outputs. Two reviewers verified trace sufficiency outright. Glossary touch-up at most. |
| The Trace violates A1 by having no owner | deepseek C2 (critical) | The trace is a derived view, not a stored fact; nothing breaks. Pedantic application of A1. |
| `FatalCause` cannot identify the failing Port/Slot | deepseek C3 (critical) | Factually wrong: `PORT-ROUTING` mandates one mapped Error variant per Slot, so the exit names the Slot. The Journal-alone gap is real but documented as the design's observation/cause split. |
| Round-robin cursor must be captured in the trace for replay | deepseek M8 | Wrong: the cursor is a deterministic function of config + trace; two reviewers verified determinism closes over the trace. (The neighboring *under-specification* of the tie-break is real — Tier 2.) |
| Step-budget DoS by a re-arming Port | deepseek M10 | By design: the Justify note says the budget exists precisely because `step` may re-arm; exhaustion-as-typed-Error *is* the enforcement. k3 verified the Zeno case. |
| `mem::forget` on the certificate bypasses finalization | deepseek M11 | The certificate is module-private; only the Engine itself could forget it, and the Engine is Kavod's own tested code. |
| `Quiesced` claims a witness live cannot produce (TRUST-SPAWN) | grok H5 | Refuted in the 08-18 pass and re-checked: the glossary trust mark plus "otherwise unwitnessable" *is* the stated architecture; lifecycle tests read `Quiesced` as "joined", never "succeeded". (The live-only *wording* of TRUST-SPAWN survives — Tier 1 item 9.) |
| ENV-LATCH linearization is a blocker / vacuous | opus K8-K9, grok H4 (blocker) | Adjudicated: ambiguity only. The commitment-table preamble selects the coherent reading; the conformance bullet is testable under it. One defining sentence — Tier 1 item 2. |
| The compile-fail suite cannot be sited | opus K31 | Refuted in the 08-18 pass: a `#[path]`-mounting harness compiles `record.rs` as the case crate's own child module — exact `pub(super)` vantage, no feature gate. (Sub-point kept: the partial-dispatch case is vacuous post-fusion; reword that bullet.) |
| Journal step-4 newline `BoundExceeded` is dead code | opus K48 | Refuted: the three-regime byte arithmetic (= max commits; max+1 dies at the newline; > max+1 dies at encode) was verified independently by two reviews. |
| `dispatch` masks a prior latched root cause; fix by checking the latch first | gemini M4 (major) | The live mechanism already checks the latch first; the residual race window is exactly the ENV-LATCH ambiguity (Tier 1 item 2). The proposed fix is the already-written text. |
| Sim batch asymmetry violates DET-ENV | gemini M2 (major) | No violation: the two environments produce *different traces* in that scenario, and DET-ENV binds only equal traces. Kernels kept: SIM-DISPATCH's missing latch-first carve-out and the `Prepared`-row gloss (Tier 1 item 2 satellite, Tier 2). |
| The `TurnOpen` row omits batch clearing (exhaustiveness breach) | opus K41 | `APP-OVERFLOW` — a guarantee row — owns the rule; §0's placement law puts single-component facts in that component. |
| Sim sequential `stop` violates "observe the signal immediately" | terra #4 (high) | Single-threaded: no Port is ever running when shutdown fires; the sim binds "the `stop` call is the sim shutdown signal" as its realization. Phrasing note only (folded into opus K43, Tier 2). |
| A8/NO-UNWIND is "an axiom that is actually a trusted obligation" | grok M1 | The doc marks the dependency inline (`TRUST-ABORT`), the same device `BOUND-LOOPS` uses. Deliberate and consistent. |
| Live shutdown "can hang forever" | opus K14 (blocker) | The hang requires an unbounded post-notice destructor — a `BOUND-BLOCKING` violation, whose blast radius the doc assigns. The deadline-scope *wording* kernel survives (Tier 1 item 4). |
| LIVE-SUPERVISION race yields `Stopped` for a run where a Port died | opus K13 (major), sol M-03 | The close's linearization resolves the race by design; the 08-18 pass walked all four publication timings. Survives only as the "while Running" scoping fix (Tier 1 item 3). |

---

## Reviewer report card

- **k3** — best precision of the eight. Found the one substantive new gap (G1), adjudicated A4 correctly where three others called it a contradiction, and its failed-attacks list matches the verified record.
- **fable** (the 08-18 pass) — all 22 findings either reconfirmed by an independent model this round or unchallenged; all three of its refutations held against re-raises.
- **opus** — highest recall by far and the most unique real finds (RawValue, TRUST-SPAWN sim scoping, obligations-table gaps, K15's forgeability angle on item 1), but its severity dial runs hot: of five "blockers", two are rejected outright and two downgrade to one-sentence wording fixes. Trust its lists, not its ratings.
- **grok** — aggressive and half right: H4/H6 kernels confirmed at reduced severity, H2/H5 re-raise refuted attacks, H1/H3 are the real axiom-gloss findings. Its "attacked and solid" list is good.
- **gemini** — thin; two of four majors are by-design behavior misread as defects, the other two are consensus items it shares credit for. The minor list is fine.
- **sol / terra** — one-and-a-half reviewers, not two. sol is the substantive half (H-04's lifecycle shape, M-09's Error-destructor gap, M-11's key-serialization point); terra adds essentially nothing beyond sol except the CommandsPrepared-intent framing.
- **deepseek** — worst signal at the top: all three "criticals" rejected, and several majors (M8, M10, M11) are misunderstandings of the design. Its minor/cosmetic list is decent and several entries made Tier 2.

## Suggested fix order

1. Tier 1 items 1–2 (the bookkeeping gap and the ENV-LATCH sentence) — they unblock the engine's core story.
2. Tier 1 items 3–5 (Publication rename, deadline scope, Enforcement status) — one decision plus renames.
3. Tier 1 items 6–9 (the previously-owed set) — already planned.
4. The Tier 2 batch in one editing pass.
5. Tier 3 rides the Wiring close. Tier 4 needs decisions first.
