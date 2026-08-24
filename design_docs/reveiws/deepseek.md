# Adversarial Review: opencode/deepseek-v4-pro, 2026-08-23

**Target:** design_docs/design-v12.md — `Status: Authoritative (v12). One section is open: Wiring & construction.`
**Verdict:** Sound. The document is internally consistent, every state-machine transition is accounted for, exhaustion claims hold under scrutiny, failure paths are complete, and the enforcement architecture (unrepresentable / asserted / tested) is coherent and realizable in Rust. No contradictions, false claims, or unreachable guaranteed outcomes were found.

## Findings

Ordered by severity.

### OCN-01 `EventAccepted` edge's "Requires" column breaks table convention — MINOR, confidence low, ambiguity
- **Text attacked:** Edge table (line 733), `BetweenTurns` → `EventAccepted`: Requires "the transition's successful `next_event` return; `ENV-TIME`'s nondecrease, checked before the commit." Compare every other edge (lines 726–732, 734), whose Requires columns state preconditions established by the prior phase (e.g. "empty batch", "the phase's fixed answer is `Continue`"). The `EventAccepted` row's Requires instead *describes work the transition performs* (calling `next_event`), not a precondition.
- **Claim:** An implementer reading only the edge table and treating "Requires" as a precondition will place the `next_event` call in the `BetweenTurns` phase. An implementer reading only the phase table (line 714, "Its transition performs the index-domain check... Then it calls `next_event`") will place the call in the edge transition. Both produce identical observable behavior, but the table convention is inconsistent.
- **Witness:** Edge table rows 1–8 (all others): Requires column = precondition. Row 9 (`EventAccepted`): Requires column = work description + precondition. The edge preamble (line 722) acknowledges the blur — "Work its transition performs before the commit can fail as the Phases and Requires rows name" — but does not warn that EventAccepted is the only edge where the Requires column itself describes the work.
- **Fix sketch:** Add a sentence to the edge preamble noting that `EventAccepted`'s Requires column names work the transition performs rather than a precondition, or split it into a precondition ("the candidate and its timestamp are acquired") and a performed check ("ENV-TIME's nondecrease").

### OCN-02 General Port "lifecycle" undefined for bespoke Environments — MINOR, confidence medium, ambiguity
- **Text attacked:** `ENV-START` row (line 456): "...no Port is left mid-lifecycle: every Port either never began or will receive no further call, its lifecycle ended before the return."
- **Claim:** The Glossary (line 125) defines only "Sim Port lifecycle" (the `NotStarted`/`Open`/`Ended` state machine). The unqualified word "lifecycle" used in `ENV-START` has no general binding definition that a bespoke `Environment` implementor can verify against. The sentence's self-explanation ("every Port either never began or will receive no further call") resolves the intent, but two bespoke implementors could disagree about what additional liveness condition "its lifecycle ended" imposes beyond "will receive no further call."
- **Witness:** Implementor A interprets "lifecycle ended" as equivalent to "will receive no further call" (the colon explanation is exhaustive). Implementor B interprets "lifecycle ended" as an additional requirement that the Port's internal cleanup/destructors must have run before `start`'s `Err` return. Both conform to the text under the two readings, producing observably different guarantees.
- **Fix sketch:** Replace "its lifecycle ended before the return" with a sentence that states the exact observable condition (e.g., "and any Port-internal teardown the Environment initiated has completed"), or add a general "Port lifecycle" term to the Glossary committing to an observable condition.

## Attacked and held

- **State-machine exhaustiveness:** Walked every phase/transition pair; all Fatal exits are covered in phase-table prose (not edges), all 9 non-Fatal edges match 6 records, and the track is marked as the non-Fatal complement of `RUN-FINALIZE`. No missing edge or reachable-but-forbidden state found.
- **Index-domain arithmetic:** Traced `u64::MAX` check before `next_event`, overflow panic after the check, and time regression consuming the candidate but committing nothing. The trace definition (Glossary) accounts for the consumed-but-unaccepted `(Event, Timestamp)`. Consistent.
- **Failure-path completeness:** Walked every error site (startup, TurnOpen overflow, TurnOpen Fatal outcome, dispatch, checkpoint, EventAccepted, shutdown, Journal commit) through `RUN-FINALIZE`. All three quiescence branches (start Err / unconsumed / consumed) are covered; A4's first-failure-wins and discard-cleanup-errors are consistently applied.
- **Determinism (`DET-RUN` / `DET-ENV`):** `DET-RUN` binds within one Environment type; `DET-ENV` binds only where equal traces exist and explicitly excludes failure shapes unique to one Environment. No contradiction.
- **Rust realizability:** Type-state certificate pattern (`PhantomData<fn() -> P>`, no `Clone`/`Copy`/`Default`) is standard and realizable. `ports!` macro expansion into externally-tagged serde enums with `Never` uninhabited arms compiles. `serde_json` determinism pinned by "same build" clause. `Send + 'static` boundaries on Live ports, absent on Sim ports, are correct.
- **Commitment-point table:** Matched each operation's `Err`/success semantics to `ENV-LATCH` ordering rules and the Run's usage. No mismatch between what `Err` means and what the Run does with it.
- **Journal poison and bounded writes:** The write loop's bounded-by-record-length retry, `Interrupted` non-retry, zero-progress and over-reported-count mappings, and flush-as-commit are all consistent with `BOUND-LOOPS` and `JRN-COMMIT`.
- **`ENV-LATCH` publication ordering:** Traced concurrent publication against `next_event`, `dispatch`, `take_error`, and close through all before/overlapping/after scenarios. The spec correctly assigns ordering responsibility to the Environment and uses the returned value as the witness.
- **Live completion (`LIVE-COMPLETION`):** The non-cloneable terminal guard's Drop covering normal returns, `Err`, and test-profile unwind (but not shipped-profile panic, which aborts) is consistent with `NO-UNWIND` and `LIVE-SHUTDOWN`'s deadline behavior.
- **Sim selection (`SIM-SELECT`):** Round-robin cursor handling, step budget consumption, `set_next(now)` self-arming, and `step(None)` continuation are all well-defined even at zero armed Ports (`SIM-COMPLETION`).

## Coverage

- Section 0 (Reading): walked
- Section 1 (Glossary): walked
- Section 2 (Laws): walked
- Section 3 (Application): walked
- Section 4 (Port): walked
- Section 5 (Environment contract): walked
- Section 6 (Journal): walked
- Section 7 (The Run): walked — every phase, every edge, all records, graph, enforcement, guarantees
- Section 8 (Live Environment): walked — every guarantee, mechanism, lifecycle state machine
- Section 9 (Simulated Environment): walked — every guarantee, lifecycle state machine, selection algorithm
- Section 10 (Wiring): skimmed (declared open — excluded per scope)
- Section 11 (Crate layout): skimmed
- Section 12 (Obligations): walked — every obligation row verified against use sites; verification suite guarantees checked
- Appendix A (Invariant index): skimmed

## Questions the document cannot answer

None. Every question raised during analysis was resolved from the document's own text. The two findings above are minor ambiguities, not unresolvable gaps.
