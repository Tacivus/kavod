# Adversarial Review — design-v12.md

**Date:** 2026-08-18
**Target:** `design_docs/design-v12.md` (1200 lines), reviewed in a vacuum — no other file consulted.
**Method:** Eight independent attack agents, each with a distinct lens (run-graph totality, latch/Error pipeline, shutdown/quiescence, determinism, bounds/time arithmetic, citation & self-conformance, API-block consistency, free-form scenario red team). Every contested finding was then handed to an independent skeptic agent instructed to *refute* it from the document's own text under §0's binding-form rules. Only survivors are reported. Raw output: 30 findings, four rated major by their finders. After verification: **0 critical, 0 major, 12 minor, 10 nit** — three findings killed outright, every "major" downgraded or narrowed.

**Bottom line:** the semantic core is sound. The run graph, the latch state machine, the ShutdownReport outcome matrix, the Fatal-finalization trichotomy, first-failure-wins, the determinism-via-trace story, and the index/byte arithmetic all survived deliberate attack, including twelve mandatory nasty end-to-end scenarios executed strictly by the tables. Every defect that survived lives at the edges: axiom-gloss wording that invites misreading, the Enforcement layer's ambiguous normative status, small definitional gaps in the sim/live rows, and terminology discipline. Nothing found requires a semantic redesign; everything found is a wording, definition, or placement fix.

---

## 1. What was attacked and held

- **The four graph descriptions agree.** Sketch, States table, Edges table, and Enforcement transitions name the same states, edges, records, and requirements; the `dispatch_batch` fusion of `Prepared` is explicitly reconciled and preserves the record sequence and failure outcomes.
- **RUN-FINALIZE's trichotomy is total and mutually exclusive** over every failure site: each startup step, all six record-commit failures, every state-row failure, `TimeRegression`, `IndexExhausted`, the checkpoint, and all three `StopPending` report cases. `shutdown` is called at most once on every path and skipped only on start-`Err`, where `ENV-START` licenses the claimed `Quiesced`.
- **The latch state machine is total and single-valued** over empty/pending/reported/closed for every publish/take/op-Err/close event; unlisted cells are unreachable under `ENV-SERIAL`'s call pattern. A pending Error cannot outlive its turn (per-turn checkpoint), cannot surface as two causes (move-out + single finalization), and cannot be silently lost.
- **The ShutdownReport matrix** — {Some, None} × {Quiesced, Incomplete}, on both the Stop path and the finalize path, live and sim — has exactly one defined outcome per cell. The sim derive that the Stop-path report Error is *structurally* `None` checks out (no publication site exists between checkpoint and close).
- **Determinism closes over the trace.** Every Engine decision (edge fired, FatalCause + payloads, record bytes, stopping point) is a function of the five stated premises; per-call sink results fully determine the write-retry loop; no wall-clock, cursor-order, latch-timing, or cross-turn buffer state reaches a decision except through a trace-recorded operation result; Error values never steer control flow before erasure.
- **Arithmetic survived off-by-one hunting.** The index chain (mint at 0, `u64::MAX` check before `next_event`, overflow unreachable past the check); the Journal byte chain (buffer = `max_record_bytes + 1`, newline rejection at step 4, committed objects ≤ max exactly as `JRN-FORMAT` states); the sim "every armed time ≥ now" invariant under every arming/advancing order; `set_next(now)` re-arming contained by the step budget.
- **The ID census is clean.** Every cited ID (all-caps and A-numbers) resolves to exactly one definition; zero dangling, zero duplicates; Appendix A matches the body for all 54 guarantee/obligation IDs (one omission — the axioms — reported below).
- **All 12 mandatory red-team scenarios resolved cleanly** by named rows, including: overflow + `Outcome::Fatal` + pending latch (explicit ordering wins); Journal poison at each of the six record kinds; the four live publication timings around Stop (pending→NextEvent, close-into-report, at-close linearization, post-close discard); sim mid-batch vs last-in-batch `on_command` errors; `u64::MAX±1`; first-commit failure (empty Journal reconciles with `RUN-RECORDS` via the committed/physical-suffix distinction); Incomplete + Some ("a reported Error outranks `Incomplete`").

---

## 2. Confirmed findings

### Minor

**V12-01 · ambiguity · SIM-START (~1024)** — **"lifecycle is open" is undefined for never-started Ports.**
`SIM-LIFECYCLE` (~1023) defines only how a lifecycle *ends* (a method `Err`); nothing anywhere defines when one *opens*. On a mid-order start failure (A ok, B errs, C never called), "calls `stop` on every Port whose lifecycle is open" has two conforming, Port-observable answers: `stop(C)` runs (stop-before-start must be tolerated) or C gets nothing. `ENV-START`'s "never began or will receive no further call" is satisfied either way. A dedicated skeptic ran seven rescue attempts (citation-as-complement-definition, pre-existence entailment, SIM-SHUTDOWN parallelism, ENV-START arbitration, A4 dissolution, totality, exhaustive search for an opening rule); all failed. `SIM-SHUTDOWN`'s identical phrase is immune (start succeeded ⇒ every Port began). Three lenses found this independently.
*Fix:* one Define: line ("a lifecycle opens at its first method invocation"), or reword to "every already-started Port whose lifecycle is open."

**V12-02 · contradiction · RUN-GRAMMAR (~740) + Enforcement preamble (~771)** — **the requirement-provenance clause is false for two edges.**
"Each edge requirement is the phase itself, or work its own transition performs" (binding row) and "A requirement is never a loose value a caller could forget, reuse, or forge" are false for the Requires entries "the frozen start time" (`run_started(start_time)`, Does: "—") and the identity component of "a candidate" (`accept_event(time, &event)` checks nondecrease and derives the index, but nothing ties the pair to `next_event`'s actual return). Both are Engine-passed, test-pinned trust points with exactly the profile of the two arguments the residue bullet *does* list. Skeptic verdict: the residue enumeration itself is acquitted — "Everything else in RUN-GRAMMAR's list is unrepresentable" scopes precisely to the row's seven-item violation list, which never mentions payload forgery, and the possession-proof clause survives because "Accepted" is constitutively defined by the commit. What stands is the overreaching universal in a binding row.
*Fix:* qualify the clause (e.g. "…or work its own transition performs; record payloads are the Engine's, pinned by golden tests") or add the two arguments to the runtime-points list.

**V12-03 · contradiction · A9 (~148) vs Trace (~126) / DET-RUN (~745) / DET-ENV (~746)** — **"every run output" quantifies over outputs the trace erases.**
`EngineExit` is "the run's only outcome channel" and carries Error *values*; the Trace erases them; DET-RUN explicitly blesses exits that differ only in erased Error values under identical premises. Two runs, same five premises, same flush-failure position, different `io::Error` payloads: DET-RUN says conforming, A9 says impossible. The skeptic confirmed no rescue survives: the author demonstrably writes "Core-owned" when it is meant (both DET rows), and A9 carries no such qualifier.
*Fix:* one qualifier in A9 ("every Core-owned run output…") or an erased-Error exception clause mirroring DET-RUN.

**V12-04 · api-mismatch · Enforcement transitions table (~774–783)** — **the mechanism as rendered cannot type-check.**
Three instances: `dispatch_batch(env, &[C])` must hand *owned* Commands to `Environment::dispatch(command: Self::Command)` out of a shared slice with `Serialize` as the only bound and unsafe forbidden — unwritable; `no_commands()` takes nothing and the Certificate holds no batch, so its "asserts the batch empty" has no operand; `Checkpointed<answer>` is a value-dependent return type Rust cannot express (the realizable encoding — two typed phases — is what the *next* two rows already use). Skeptic verdict: the table is mechanism, not a binding form, so no binding contradiction — but the rendering sits in the load-bearing exposition that RUN-GRAMMAR and §10 both cite, and the "slice" wording is reinforced twice in prose (~785, ~808). The forced repair (drainable/owned batch; batch view to `no_commands`) changes no record, guarantee, or failure semantics.
*Fix:* re-render the three signatures (batch by value/drain; batch view parameter; two checkpoint successors or an enum-of-phases return).

**V12-05 · self-conformance · §0 forms list (~21–27) vs RUN-GRAMMAR (~740) and §10 (~1099)** — **the Enforcement layer's normative status is self-contradictory.**
The transitions table and the residual-assert bullets sit in none of §0's four binding forms ("A rule in none of the four forms does not exist"), yet binding text leans on them: RUN-GRAMMAR delegates "The residue that stays runtime is listed there," and §10 lists "the Certificate transition set (Enforcement)" under "Constraints already fixed." The tension is live in both directions: V12-04 survives *only because* the table does not bind — the same table §10 declares fixed. Three lenses flagged this independently.
*Fix:* either admit the transitions table to the binding-table list (and ID the residual asserts), or reword RUN-GRAMMAR/§10 so nothing binding delegates content into mechanism prose.

**V12-06 · ambiguity · LIVE-SHUTDOWN (~920) vs Glossary Publication (~111) vs LIVE-SUPERVISION (~917)** — **"completion publications" collides with the normative sense of Publication.**
The glossary binds Publication = "entry of an Error into the latch." LIVE-SUPERVISION says expected completions "stay unpublished"; ENV-LATCH discards post-close publications. Strictly read, LIVE-SHUTDOWN then waits the full deadline "for completion publications" that can never come, joins nobody, and every clean live shutdown reports Incomplete → `Core(ShutdownIncomplete)` — an absurdity proving the row means a second, completion-tracking sense of "publish" that exists nowhere in the binding vocabulary (start mechanism step 5 uses the same verb for it).
*Fix:* rename the completion-tracking signal ("completion notices," "joins") or define the second sense.

**V12-07 · undefined-outcome · LiveCtx semantics (~893, ~918)** — **`try_recv` after the signal has been yielded is unspecified.**
The section binds semantics now ("Semantics here are normative; the exact LiveCtx signatures are provisional"). After `Some(Command)…Some(Shutdown)`, is the next `try_recv` `None` (sequence exhausted) or `Some(Shutdown)` again (signal "never hidden")? Both readings satisfy every sentence; a generic exit-on-None poll loop spins forever under one and terminates under the other.
*Fix:* one sentence fixing post-signal behavior (repeat `Some(Shutdown)` is the natural reading of "never hidden").

**V12-08 · ambiguity · SIM-SELECT (~1028) vs commitment table (~438)** — **subordinate effects are named for only one of three mid-selection error returns.**
The contract makes naming load-bearing ("subordinate effects the implementation names stand"). The naming clause attaches syntactically to `step(Err)` only — but a budget-exhaustion return (`SIM-STEPS`) or a mid-selection nothing-armed return (`SIM-COMPLETION`) carries the identical standing effects (advanced `now`, cleared arms, spent budget, Port mutations from `step(None)` iterations), and their status is textually undefined; the mutated Port state is observable when finalize's `shutdown` calls `stop`.
*Fix:* move the naming clause to cover every `next_event` `Err` in sim ("for every Err this row returns, the advanced now, cleared arms, spent budget, and Port mutations are the named subordinate effects").

**V12-09 · self-conformance · §8/§9 intros (~870, ~987) vs SIM-STATE (~1022); ENV-SEPARATION/ENV-BOUNDS (~453–454)** — **the realization discipline leaks in both directions.**
Both implementation intros claim "every guarantee below realizes a named contract row or defines the … Port-facing API." `SIM-STATE` does neither (structural ownership claim; no row named, no API defined — it plainly *should* cite `ENV-SEPARATION`). Inverted: `ENV-SEPARATION` and `ENV-BOUNDS` are cited by no row in §8 or §9 (verified by scan — they appear only at definition and in Appendix A), so the mapping from shipped enforcement to those two contract rows is untraceable by the doc's own citation discipline.
*Fix:* have SIM-STATE (and LIVE-THREADS) cite ENV-SEPARATION; add an ENV-BOUNDS citation where the shipped bounds are realized (queue/inbox/budget rows).

**V12-10 · api-mismatch · DET-ENV (~746)** — **"the report's Error presence" is a dead comparand misframed as an exit component.**
The list is framed "exits equal in every Core-owned discriminant and payload — [list]," but EngineExit carries no report. Skeptic verdict: the strong charges fail (the quantity has a normative trace-level referent, and presence-in-exit is a total predicate), but the item does zero independent work anywhere — `Stopped` ⇒ None, `Environment(Shutdown)` ⇒ Some, `Core(ShutdownIncomplete)` ⇒ None are all determined by already-listed items, and on finalize paths the Error is discarded and exit-inert. A dead list member, category-slipped.
*Fix:* drop the item, or move it explicitly to the trace-equality premise.

**V12-11 · ambiguity · ports! (~329, ~368) vs §11 (~1107)** — **the "naming stem" account is unimplementable in general.**
"The invocation's `Trading` is a naming stem: the expansion creates `TradingEvent` and `TradingCommand`" — but `ports!` is `macro_rules!` (no proc-macro, §11), and macro_rules cannot concatenate identifiers, so the output names can only come from the `Event =`/`Command =` parameters; no text says so. The one example makes stem+suffix coincide with the parameters, hiding the divergence: invoke `ports!(pub enum Wiring<Event = TapeEvent, …>)` and the two readings produce differently-named items.
*Fix:* state that the `Event =`/`Command =` idents are the output names and the stem is inert decoration (or drop the stem from the syntax).

**V12-12 · self-conformance · PORT-ROUTING (~344) vs §0 placement rules (~53–55)** — **a Core contract section names the shipped implementations substantively.**
"— sim: the fan-out arm; live: supervision — placed finally when Wiring closes" fixes each implementation's Error-mapping site inside §4, which is none of the three listed navigation exemptions (Scope line, contract's pointer, bounds registry) and a forward reference on no exemption. Content is consistent with §8/§9/§10; only the doc's own placement law is broken.
*Fix:* move the mapping-site clause to §10's Error-sum bullet (it is already half-there) and leave a bare trust mark in PORT-ROUTING.

### Nit

**V12-13 · A4 (~143)** — **sentence 2's elided anaphora reliably produces a contradictory misreading.**
Four of eight attack lenses independently read "Once a first Error or fatal Core condition exists" as *any-Error*-existence and derived real contradictions (an unobserved latched Error putting the run "in cleanup"; the latch-close arm swallowing the final Stop-commit failure). The verified authoritative reading: the phrase is anaphoric to sentence 1's subject — "the first Error … **the run observes**" — with `RUN-FINALIZE`'s binding gloss "(A4: a cause exists)" as proof, and the misreading additionally makes the latch-close arm dead code. Under the correct reading every operational row is consistent (checked against every rule citing A4). But an axiom that misleads half a careful review panel is a drafting hazard.
*Fix:* restore the elided clause: "Once a first *observed* Error or fatal Core condition exists…" (or "once the Fatal cause exists").

**V12-14 · JournalFatal (~630)** — **`record_kind`'s meaning is bound nowhere.**
That it names *the record whose commit failed* is compelled by derivation (A4 + RUN-FINALIZE + the record table + the exact FatalCause variant set — a skeptic downgraded the original "smuggled rule" charge on these grounds) but is stated only in the Edges caption prose; the analogous `EnvironmentOperation` context fields all carry binding doc comments.
*Fix:* one doc comment on `JournalFatal.record_kind`.

**V12-15 · A2 (~141)** — **"The turn ends at handoff" conflicts with the record table and SIM-DISPATCH.**
`TurnCompleted` commits at "End of every non-Fatal turn" — after the checkpoint, and on the Stop path after the whole `shutdown`; an empty-batch turn has no handoff and thus no A2-end at all; and in sim, `on_command` (post-handoff processing) runs synchronously *inside* the dispatch loop, so "processing … runs outside it" is spatially false for one shipped Environment. Operationally benign — every table agrees on the concrete order.
*Fix:* reword to what is meant: the turn's *effects* end at handoff; processing is outside the turn's *authority*, not its wall-clock extent.

**V12-16 · ENV-LATCH (~450)** — **the before-commitment branch names an outcome two of its four operations cannot express.**
"a publication linearized before an operation's own commitment is taken, marked reported, and returned as that operation's `Err`" — `take_error` returns `Option` and `shutdown` returns `ShutdownReport`; neither has an `Err` channel. The commitment rows carry the correct per-operation outcomes; the linearization sentence's uniform branch is wrong for half its domain.
*Fix:* "…returned as that operation's Err, Some, or report Error, per its commitment row."

**V12-17 · LIVE-SUPERVISION (~917)** — **`run(Err)` after the signal: publish or stay unpublished is parse-dependent.**
Sentence 1 ("`run(Err)` and `run` completing while `Running` … each publish") vs sentence 2 ("every completion is unambiguously premature … or expected, staying unpublished"). Observationally identical (the latch is provably closed at that instant, so a publication would be discarded), so wording-only.
*Fix:* scope "while `Running`" over both conjuncts explicitly.

**V12-18 · StopPending row (~693)** — **"a reported Error" collides with the latch state "reported."**
The report's Error was pending→closed and never entered the latch state "reported" (`ENV-LATCH` uses the strict sense two paragraphs away; the ShutdownReport doc comment uses it strictly too). The intended plain-English sense contradicts the reserved term.
*Fix:* "the report's Error outranks Incomplete as cause."

**V12-19 · JRN-POISON (~514)** — **"any sink failure" includes cases where the sink returned Ok.**
The glossary's two-word rule: "fail / failure — plain English for 'returned an Error'; no further meaning." `Ok(0)` and an over-reported count are sink *failures* per JRN-POISON only because the row re-enumerates the term — exactly the "further meaning" the glossary forbids. The Trace definition handles the same situation precisely ("its Ok count, or its failure's presence"). Contained: the enumeration removes implementer ambiguity.
*Fix:* "any sink fault" / "any sink misbehavior," or enumerate without the reserved word.

**V12-20 · Appendix A (~1186)** — **the invariant index omits A1–A9.**
The nine most-cited IDs in the document are the only citable IDs that do not resolve through the navigation index.
*Fix:* one Laws row addition.

**V12-21 · A9 (~148)** — **missing inline trust mark.**
`BOUND-LOOPS` carries "(`BOUND-BLOCKING`)" and `NO-UNWIND` carries "(`TRUST-ABORT`)" for exactly this situation; A9's dependence on `TRUST-PURE` is real (the §3 derive says so) but unmarked in the row. (The stronger "A9 is unenforced" charge was refuted: the glossary's normative *pure* Application puts impure artifacts outside A9's quantified domain, and TRUST-PURE is the sanctioned carve-out.)
*Fix:* append "(`TRUST-PURE`)" to A9.

**V12-22 · Enforcement residue prose (~805–809)** — **the assert backing RUN-INDEX's overflow panic is garbled.**
"The index arithmetic behind `accept_event`, backed by one always-on assert — a freshly minted certificate sits at the start index —": either the appositive is the assert's content (a mint-time check that cannot fire at accept #k, leaving RUN-INDEX's promised "invariant panic" without a named mechanism) or the checked increment is the assert and the count "one" is wrong. No reachable input misbehaves (the BetweenTurns check makes overflow unreachable); the enforcement story is merely miswritten.
*Fix:* name both checks: the mint-position assert and the checked increment.

---

## 3. Attacks that failed (kept for the record — each is a place careful readers stumbled)

- **"A4's second sentence contradicts first-observed-wins" (4 lenses, rated major)** — REFUTED. The trigger is anaphoric cause-existence, not any-Error-existence; `RUN-FINALIZE`'s "(A4: a cause exists)" is the binding gloss; the rival parse voids "first," yields no cause assignment at all in the test scenario, and makes the latch-close arm dead code. Checked against every rule citing A4, including SIM-START's stop-discards and ENV-LATCH's second-publication discards; the reading is total. Survives only as V12-13 (drafting hazard).
- **"Quiesced witnesses more than TRUST-SPAWN allows"** — REFUTED. The glossary's run-scoped-activity definition carries the `TRUST-SPAWN` mark, that row's "otherwise unwitnessable" clause *states* the witness architecture, LIVE-SHUTDOWN binds only the enforceable join criterion, and the named lifecycle tests pin exactly that ("read as 'joined', never 'succeeded'"). No text claims Kavod enforces spawn hygiene.
- **"The compile-fail suite has no possible siting"** — REFUTED. The three impossible sitings are real (external crates can't name `pub(super)` items; doctests compile externally; `cfg(test)` can't host non-compiling code), but a standard `#[path]`-mounting harness compiles `record.rs` as the case crate's own child module, reproducing the Engine's exact visibility vantage with no feature gate and no visibility widening. The doc's siting clause reads as anticipating precisely this.
- **"The sim replay preconditions are insufficient for failure-bearing runs"** — REFUTED. "Needs" is necessity language (used identically for Journal replay at ~550); the preceding sentence already frames replay as presenting the recorded *trace*, which by definition includes every failure's presence and position; §0 strips Derive notes of guarantee force regardless.
- **"The JournalFatal mapping is a smuggled rule" (rated major by its finder)** — DOWNGRADED. The mapping is compelled by binding forms (A4 + RUN-FINALIZE's cause delegation + the record table's contrapositive + the exact variant set); §0 itself assigns Fatal outcomes away from the edge table ("Fatal finalization is RUN-FINALIZE's alone"); the caption is licensed derivation. Survives only as V12-14.
- **"The runtime-residue list undercounts"** — REFUTED as an incompleteness claim ("Everything else in RUN-GRAMMAR's list" scopes exactly to the row's seven enumerated violations). The provenance overreach survives separately as V12-02.

---

## 4. Coverage inventory

Walked exhaustively: §7's five tables + sketch + Enforcement (8 states, 9 edges, 8 transitions, 6 record kinds, every failure site); the latch as a four-state machine under every publish/take/close linearization; the ShutdownReport matrix on both paths × both Environments; every Engine decision against the five determinism premises; the index, Journal-byte, and sim-time arithmetic chains; the full ID census (54 guarantee/obligation IDs + 9 axioms) and Appendix A; every "realizes" claim forward and inverted; every glossary term against its uses ("candidate," "commit," "accepted," "turn," "reported," "fail," Publication, Error/Fatal); every API block against every prose claim about it; 12 mandatory + ~20 volunteer end-to-end adversarial scenarios; ENV-SERIAL's after-Err/after-Some restrictions against every post-failure call the graph makes.

Not in scope, by design: §10 (declared open — checked only for conflicts between its fixed constraints and the rest, one found: V12-04/V12-05 tension), implementation code, and anything outside the file.
