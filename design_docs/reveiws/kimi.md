# Adversarial Review: kimi-k3, 2026-08-23

**Target:** design_docs/design-v12.md — `> **Status:** Authoritative (v12). One section is open: Wiring & construction.`

**Verdict:** Sound with fixes. Every binding state machine, failure path, and arithmetic claim I attacked held up under its own reading rules; the graph/latch/lifecycle tables are exhaustive over the inputs I could construct, and the enforcement-tier claims are honestly scoped (RUN-ENFORCEMENT admits exactly what is runtime- and test-enforced). What survives is minor: one placement-rule self-violation in the Run notes, one unspecified arithmetic edge, and citation/wording nits.

## Findings

### KK3-01 Run notes assert a mechanism fact about "both shipped Environments" — MINOR, confidence high, self-conformance violation
- **Text attacked:** Section 0: "Core sections build only on the contracts and never name an implementation — earlier mentions of the two shipped Environments are navigation only (the Scope line, the contract's pointer to its implementations, the bounds registry)." Section 7, Notes: "Both shipped Environments check the latch before `next_event` selection and `dispatch` handoff, so an Error pending when either call begins returns first."
- **Claim:** The Run section names the shipped implementations in a non-navigation derive note, outside the three permitted mention locations. Worse, "check the latch before" describes the *mechanism* level — the Live mechanism is explicitly "One workable mechanism, replaceable wherever the guarantees hold" — so the note tracks replaceable mechanism, not contract.
- **Witness:** A conforming replacement Live Environment that achieves ENV-LATCH's completed-before ordering without a pre-selection latch check (e.g., a single-threaded event loop in which publications and calls are totally ordered by construction) makes the note's first clause false while violating no guarantee row. The note's *conclusion* survives only because it is re-derivable from ENV-LATCH alone ("a publication completed before the call began orders before the point"), which is exactly why the placement rule wants it phrased at contract level.
- **Fix sketch:** Reword to cite ENV-LATCH's ordering rule without naming implementations, or move the sentence to the Live/Sim sections.

### KK3-02 Shutdown-deadline addition overflow has no specified outcome — MINOR, confidence medium, omission
- **Text attacked:** LIVE-SHUTDOWN: "then wakes every Kavod-owned blocking point and fixes one absolute shutdown deadline, the configured duration after that close." A6: "Arithmetic on counts, capacities, times, and identities is checked." Wiring: "`LiveConfig`: the shutdown deadline (nonzero milliseconds)".
- **Claim:** A6 forces the close-time + duration addition to be checked, but no row says what deadline results when the sum is unrepresentable. The behavior on that overflow is unlisted work in an otherwise fully-specified shutdown discipline.
- **Witness:** Configure the deadline duration as `u64::MAX` milliseconds (legal: nonzero). Implementation A uses `Instant::checked_add` and treats `None` as "no effective deadline," waits for outstanding entries, and can return `Quiesced`; implementation B treats `None` as already-expired and returns `Incomplete` with detached threads. Both check their arithmetic per A6; the two exits diverge observably (`Quiescence` is a Core-owned payload compared by DET-ENV) on identical inputs. Reachable only under an absurd configuration, hence MINOR rather than MAJOR.
- **Fix sketch:** Add half a sentence to LIVE-SHUTDOWN naming the saturation behavior (e.g., overflow fixes the deadline at the latest representable instant).

### KK3-03 "an empty Journal" after a failed first commit — NIT, confidence high, ambiguity
- **Text attacked:** Section 7, Notes: "so a run whose first commit fails exits Fatal with real effects and an empty Journal; the exit carries the cause." JRN-COMMIT: "Bytes past the last committed record are an uncertain suffix, even if they form complete lines — after a sink failure…".
- **Claim:** If the `RunStarted` commit writes some bytes and then the flush fails, the sink holds physical bytes; "empty Journal" is true only under the committed-records reading, which this sentence does not state.
- **Witness:** Sink accepts a short write of the `RunStarted` line, then `flush` returns an I/O error → Journal poisons → `Journal(RunStarted)` Fatal. Under reading A (Journal = committed sequence) the Journal is empty; under reading B (Journal = bytes passed through) it is not. The intended reading is clearly A, one section away in JRN-COMMIT.
- **Fix sketch:** "…exits Fatal with real effects and no committed record".

### KK3-04 "(Ports Notes)" is a non-ID citation — NIT, confidence high, self-conformance violation
- **Text attacked:** Section 0: "**Cite IDs.** Never section numbers, here or in tests." SIM-COMPLETION: "A run ends normally through the finite-source pattern (Ports Notes)."
- **Claim:** The finite-source pattern is a `*Define:*` note with no ID, so SIM-COMPLETION cites it by prose location — a form the citation rule does not admit (it is neither an ID nor a sanctioned binding-table-name citation).
- **Witness:** The same document cites `(A4's cleanup rule)`, `(RUN-CHECKPOINT)`, `(the Commitment points table)` — all ID or table-name form; `(Ports Notes)` is the lone subsection-name pointer. Deleting the pointer loses no obligation, confirming the target is a definition that simply lacks a citable home.
- **Fix sketch:** Give the finite-source pattern a Glossary entry or an ID and cite that.

### KK3-05 Application Notes lean on a forward reference to `Engine::new` — NIT, confidence medium, self-conformance violation
- **Text attacked:** Section 0: "**Citations point backward.** Section order is dependency order; a fact that needs a forward reference is in the wrong section. Navigation pointers are exempt: the Glossary's citations, the open-section notice, the bounds registry, the ownership map, the invariant index…". Section 3, Notes: "*Justify:* `initial_state` is infallible by design: … anything fallible needed to build it happens before `Engine::new`, while constructing the Application value itself."
- **Claim:** The justification depends on construction ordering ("`Engine::new` runs before State creation and invokes no Application or Environment method") that is defined four sections later, and the mention is none of the exempted navigation pointers.
- **Witness:** Read standalone under the document's own rule ("This document stands alone"), a section-3 reader cannot resolve when `Engine::new` runs without jumping forward to the section-7 construction table. The steelman — the note merely restates what the `initial_state(&self) -> Self::State` signature already forces — softens but does not remove the forward dependency, since the note's argumentative force comes from the later table.
- **Fix sketch:** Drop the `Engine::new` clause from the note, or let the construction table carry the justification.

### KK3-06 Status block spells the review directory two ways in one sentence — NIT, confidence high, contradiction
- **Text attacked:** "> v11 is frozen as the artifact the adversarial reviews cite (`design_docs/reveiws/`)."
- **Claim:** The same sentence spells the word "reviews" correctly in prose and "reveiws" in the path. (The directory on disk is in fact spelled `reveiws`, so the path resolves — the anomaly is that the directory name itself carries the misspelling while the prose spells the word correctly.)
- **Witness:** The quoted line itself contains both spellings; no other text in the document uses "reveiws".
- **Fix sketch:** Rename the directory to `design_docs/reviews/` (or accept the spelling and note it is deliberate).

## Attacked and held

- ENV-LATCH's before/after/overlap publication lattice against both shipped latch-first mechanisms and the "fails-before-commitment is not an observation point" clause — reconciled, no contradiction.
- A2's "processing of Commands already handed off runs outside the turn" against SIM-DISPATCH's synchronous `on_command` — holds vacuously: the invocation *is* the handoff commitment, so no post-handoff processing exists in sim.
- `classify` as a recordless certificate-consuming transition absent from the exhaustive edge table — explicitly a typed refinement of `TurnOpen`, "not additional graph phases"; does no work the tables omit.
- Fused `dispatch_batch` versus the graph's `Prepared` phase and RUN-GRAMMAR's per-phase certificate claim — record sequence, prefix semantics, and failure outcomes provably unchanged; omission risk is owned by golden-Journal tests, as RUN-ENFORCEMENT admits.
- TurnOpen's overflow-beats-`Outcome::Fatal` ordering, including discarding the Fatal payload — explicit, and A4-consistent (the overflow condition is observed first).
- Stop-path publication between checkpoint and close → `Environment(Shutdown)` with retained quiescence; RUN-FINALIZE's three branches walked against every phase/edge failure — exhaustive, including Journal-failure-on-`TurnCompleted(Stop)`.
- Consumed-but-unaccepted candidates (`TimeRegression`, failed `EventAccepted` commit) — edge table, Run notes, and the Glossary Trace definition agree exactly.
- Journal arithmetic: `max_record_bytes + 1` sizing, exact-fit object, zero-progress encode → `BoundExceeded`, newline-append bound, `usize::MAX` → `MaxBytesTooLarge`, write-loop short-write/`Ok(0)`/over-report/`Interrupted` mapping and poisoning — all consistent, with the `Interrupted`-no-retry choice justified against BOUND-LOOPS.
- Latch state machine (empty/pending/reported/closed) and SIM-LIFECYCLE (`NotStarted`/`Open`/`Ended`) walked over every input in every state — no unlisted, unreachable-but-claimed, or reachable-but-forbidden transitions found.
- RUN-INDEX domain check ordering (`u64::MAX` gate before `next_event` is ever called) — overflow is unreachable; the invariant panic is defense in depth.
- Live shutdown race discipline: one linearized close, one non-restarted deadline, final synchronized observation at expiry, joins only after all-`Complete`, honest nontermination caveat under TRUST-BLOCKING violation — held.
- Sim selection: round-robin cursor (starts at Slot 0, persists, advances on every selected `step` including `None`), step-budget pre-checks, `SIM-COMPLETION` as designed Fatal, arm-vs-`now` invariant maintenance — held.
- serde claims (newtype transparency, externally-tagged enums, newline escaping, unit/tuple-struct non-object forms, `match *self {}` for `Never`) — verified by reasoning about stock serde/serde_json behavior ("reasoned", not executed).
- DET-RUN/DET-ENV conditioned on the trace, with Error erasure matching the exit-comparison list (`JournalError` variant + `SinkOperation`, never the `io::Error` inside) — no constructible pair of conforming implementations diverges within the written premises.

## Coverage

- Section 0 (Reading rules): walked — extracted the four binding forms, exhaustiveness claims, citation rules, enforcement tiers; all findings above are keyed to them.
- Section 1 (Glossary): walked — every term checked against its uses (Commit, Accepted, Trace, Latch, Commitment point got the most scrutiny).
- Section 2 (Laws): walked — A1–A9 against later rows; bounds registry and ownership map cross-checked.
- Section 3 (Application): walked — emit table, overflow marker, `remaining()` edges, Context capability claims.
- Section 4 (Ports): walked — macro expansion realizability, `Never`, hand-written-sum equivalence.
- Section 5 (Environment): walked — commitment table against every guarantee row; ENV-LATCH line-by-line.
- Section 6 (Journal): walked — byte arithmetic, poison lattice, classification rules, serde derivations.
- Section 7 (Run): walked — construction/startup/phase/edge/record tables exhaustively, certificate mechanism, enforcement boundary, all notes.
- Section 8 (Live): walked — supervision, completion-state ownership, shutdown deadline protocol, all races I could enumerate.
- Section 9 (Simulated): walked — lifecycle, selection order, budget, cursor, shutdown; replay preconditions.
- Section 10 (Wiring): walked — open per the document's own declaration; checked only that no closed section's text breaks on its open decisions (none does).
- Section 11 (Crate layout): walked — consistency with API-block item names (section declares itself mechanism; omissions there are nonbinding).
- Section 12 (Obligations & verification): walked — trusted boundary completeness, suite targets vs. the IDs they claim to pin.
- Appendix A: walked — ID index completeness against the body.

## Questions the document cannot answer

1. When `close-time + configured shutdown duration` is unrepresentable in the platform's time type, what deadline does shutdown use? (The subject of KK3-02.)
2. After the shutdown signal is raised and a Port's inbox is drained, does `try_recv` return `Some(Shutdown)` on every subsequent call, or eventually `None`? ("pending Commands first, then the signal" fixes that the signal follows drained Commands; its repetition — immaterial to a conforming drain loop — is unspecified.)
