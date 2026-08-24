# v10 → v11 migration ledger

Working artifact for the v11 rewrite. Not authority; archived into `v10/` once the
rewrite is verified. Dispositions: **kept** (same ID, same owner), **renamed**,
**moved** (new owner section), **subsumed** (becomes a graph edge/state annotation,
API fact, or corollary), **split**, **retired** (with reason).

Section names used below: Laws (§2), App (§3), Ports (§4), Env (§5), Journal (§6),
Run (§7), Live (§8), Sim (§9), Wiring (§10, open), Obligations (§12).

## A. v10 invariant IDs (45)

| v10 ID | Disposition |
|---|---|
| BOUND-LOOPS | kept in Laws; reworded per D3 (run bounded by the index domain, not max_turns) |
| BOUND-BLOCKING | split: "blocking waits are not active loops" defined in Laws prose; the trust half (user code bounded) → Obligations row BOUND-BLOCKING |
| APP-FROZEN | subsumed: `&self` receivers + by-value `Engine::new` (API facts) + Run construction note; Wiring keeps "builders freeze before run" as an open constraint |
| APP-STATE | split: "initial_state exactly once, before any fallible step" → Run startup table; "run-varying data lives in State" → Obligations (determinism trust) |
| APP-AUTHORITY | split: signature is the API; "no hidden authority" → Obligations |
| APP-EMIT | kept in App |
| APP-OVERFLOW | kept in App, minus the consequence tail (→ Run graph, TurnOpen state work) |
| APP-OUTCOME | subsumed: exactly-one-Outcome is the signature; variant effects → Run graph edges |
| APP-FUTURE | kept in App (restated structurally: Context is the handler's only capability; future work re-enters as an Event) |
| PORT-STATE | kept in Ports |
| PORT-SUMS | kept in Ports |
| PORT-ROUTING | split: exhaustiveness/payload agreement enforced (kept in Ports); arm-correctness + Error mapping → Obligations |
| PORT-HANDOFF | subsumed: handoff commitment is the Env commitment table's dispatch row; "processing after handoff belongs to the destination Port" folded into PORT-STATE |
| ENV-CALLS | moved → RUN-SERIAL |
| ENV-LATCH | kept in Env; gains the 4th latch state (shutdown-closed, sweep A9) and sole ownership of latch closure (sweep A3) |
| ENV-TIME | kept in Env; origin statement lands here (sweep D2) |
| ENV-SHUTDOWN | kept in Env; "signal ahead of queued Commands" realization specified in Live recv priority (sweep A12) |
| ENV-SEPARATION | kept in Env |
| ENV-BOUNDS | kept in Env; "configured" dropped (sweep A2) — bounds are "its own", per the Laws registry |
| (new) ENV-START | minted per D8: start Err ⇒ quiesced, safe to drop, no Port left mid-lifecycle (fixes sweep A5) |
| LIVE-THREADS | kept in Live |
| LIVE-EVENTS | kept in Live; "disconnected" → "Closed" to match `OfferRejected` (sweep E2) |
| LIVE-SELECT | kept in Live |
| LIVE-TIME | kept in Live |
| LIVE-DISPATCH | kept in Live |
| LIVE-SUPERVISION | kept in Live |
| LIVE-LIFECYCLE | kept in Live; gains recv priority (signal before queued Commands; sweep A12) |
| LIVE-START | kept in Live; realizes ENV-START |
| LIVE-SHUTDOWN | kept in Live; latch closure wording defers to ENV-LATCH (sweep A3); deadline named "shutdown deadline" everywhere (sweep E4) |
| SIM-STATE | kept in Sim |
| SIM-START | kept in Sim; gains stop-on-Err cleanup per D8 (fixes sweep A5); realizes ENV-START |
| SIM-DISPATCH | kept in Sim |
| SIM-WAKEUP | kept in Sim |
| SIM-SELECT | kept in Sim |
| SIM-STEPS | kept in Sim |
| SIM-COMPLETION | kept in Sim; first sentence rewritten (sweep A18) |
| SIM-SHUTDOWN | kept in Sim; latch closure wording defers to ENV-LATCH |
| JRN-FORMAT | kept in Journal |
| JRN-ENCODE | kept in Journal |
| JRN-COMMIT | kept in Journal |
| JRN-POISON | kept in Journal; gains the lying-sink trigger (over-reported write count → InvalidData → poison; sweep C7, code-settled) |
| JRN-SINK | kept in Journal; negatives restated positively (Rule 5) |
| FAIL-FINALIZE | renamed → RUN-FINALIZE; "never writes again" demoted to a D13 certificate corollary (sweep E1) |
| BOUND-SIZING | moved → Obligations (trusted; sweep A15), name kept |
| BOUND-INDEX | renamed → RUN-INDEX; reworked per D3 (index-domain bound, IndexExhausted pre-acquisition, panic backstop inside `accept_event`) |
| RECORD-GRAMMAR | renamed → RUN-GRAMMAR; scope stated honestly once (sweep B1/B7): sequencing, checkpoint occurrence, and stop ordering are compile-time; omissions and payload content are assert/test territory |

New v11 IDs with no v10 ancestor: ENV-START (D8), RUN-SERIAL (ex ENV-CALLS),
RUN-CHECKPOINT (ex §8.4 order 6 + D15), DET-RUN / DET-MODE (v10 §1.3 was un-ID'd
prose legislation — now ID'd per Rule 3).

## B. v10 §4.2 commitment table (5 rows)

| Row | Disposition |
|---|---|
| start | kept in Env commitment table; Err-side unified as ENV-START (D8); "activation after start time frozen" kept |
| next_event | kept; the "becomes accepted only when EventAccepted commits" tail moves to the Run graph (acceptance is a Run fact) |
| dispatch | kept; PORT-HANDOFF folded in |
| take_failure | renamed take_error (D2); "one final take_failure" mis-description dropped (sweep A7) — per-turn checkpoint lives in the Run graph (RUN-CHECKPOINT); snapshot semantics stay here |
| shutdown | kept; consumes the Environment; Quiescence definition moves to glossary + Env (sweep A8) |

## C. v10 §8.4 tables

Construction (2 rows): kept as the Run construction table (batch reserve → Journal build).

Startup (4 rows): rows 1–2 kept as the Run startup table (State once; env.start with
ENV-START on Err — skip shutdown, Quiesced); rows 3–4 subsumed by the graph
(Initial → RunStarted → TurnOpen at index 0).

Acquisition (6 rows): row 1 → RUN-INDEX pre-acquisition check (IndexExhausted, D3);
row 2 → BetweenTurns state work; row 3 (time validation) → inside `accept_event` (D14);
row 4 (index assignment) → inside `accept_event` (D5); row 5 → the EventAccepted edge;
row 6 → the TurnOpen state. "max_turns counts accepted events" paragraph retired (D3).
Counter-advance ambiguity (sweep A11) resolved: the token's index is the accepted
count and advances only at a successful EventAccepted commit.

Turn result (11 rows): orders 1–2 → TurnOpen state work (overflow beats every Outcome,
Fatal payload discarded — stated explicitly, sweep A16); order 3 → CommandsPrepared
edge; order 4 → Prepared state work (Dispatch{position}); order 5 → CommandsDispatched
edge; order 6 → EffectsComplete state work = RUN-CHECKPOINT (witness minted, D15);
order 7a → TurnCompleted(Continue) edge (requires witness); order 7b → StopRequested
edge (requires witness); the false "checkpoint closed observation" note replaced by
"no latch-observing operation follows on the Stop path" (sweep A3); orders 8b–9b →
StopPending state work (shutdown consumes Env; Incomplete → ShutdownIncomplete);
order 10b → TurnCompleted(Stop) edge (requires quiescence witness, D15); order 11b →
Closed state (Stopped implies Quiesced). The a/b numbering scheme retires with the
tables (sweep A10).

Fatal finalization (3 rows): kept as RUN-FINALIZE, one statement (sweep E1).

## D. v10 §8.2 record rows (6)

All six kept in the Run wire-format table. Changes: every record carries `index`
(RunStarted gains `index: 0`, D7); field order is `record_kind`, `index`, then
record-specific fields; RunStarted evidences index-0 acceptance (v10 §2.2 wrongly
credited EventAccepted — sweep D1). "Exit never journaled" and "no failure or
Quiescence journaled" merged into one statement.

## E. Sweep findings (59)

| # | Disposition |
|---|---|
| A1 | fixed: Laws bounds registry — sim wakeup arms are sim-owned storage; the "only per-Port storage" sentence dies |
| A2 | fixed: ENV-BOUNDS drops "configured"; port/thread count stated once in the registry as static |
| A3 | fixed: ENV-LATCH owns closure (reported, or shutdown begun); Run annotation: no latch-observing op follows the checkpoint on the Stop path |
| A4 | fixed: glossary defines trace with timestamps included, Error values erased; DET-MODE restated against it |
| A5 | fixed: ENV-START (D8) + SIM-START stop-on-Err |
| A6 | fixed: Obligations gains the sim-Port hidden-authority row; completeness claim re-audited |
| A7 | fixed: checkpoint is per-turn in the graph; Env row describes only the snapshot |
| A8 | fixed: Quiesced defined once (glossary/Env); Live/Sim keep realization detail; start-Err Quiesced justified by ENV-START |
| A9 | fixed: ENV-LATCH lists four states |
| A10 | dissolved: numbering scheme retired with the graph |
| A11 | fixed: accepted count = token index, advances at successful EventAccepted commit |
| A12 | fixed: LIVE-LIFECYCLE recv priority (signal before queued Commands; try_recv drains) |
| A13 | fixed: fresh free-standing header (Rule 5) |
| A14 | fixed: "Error sum" terminology everywhere (D2); "Fatal sum" dies |
| A15 | fixed: trusted IDs live only in Obligations |
| A16 | fixed: TurnOpen annotation states the Fatal payload is discarded under overflow |
| A17 | fixed: D7 (RunStarted carries index) |
| A18 | fixed: SIM-COMPLETION rewritten |
| A19 | fixed: registry separates capacity bounds (nonzero-typed) from owned state |
| B1 | fixed: RUN-GRAMMAR proof boundary enumerates the residual asserts; "no runtime assertion" claim dies |
| B2 | decided: Recorder derives the index (D5) |
| B3 | decided: Prepared / EffectsComplete (D10) |
| B4 | doc states the re-export/wiring requirement (crate layout); code follow-up |
| B5 | decided: merge kept (D4); the contrary sentence dies with the absorbed doc (D6) |
| B6 | moot: satellite absorbed (D6) |
| B7 | fixed: RUN-GRAMMAR's scope stated once, honestly |
| B8 | fixed: token owns index+time (D5/D14); Context relays them to the handler; ownership map says so |
| B9 | moot: absorbed doc; layout wording rewritten |
| B10 | fixed: Journal Notes derive that a struct payload cannot produce NotAnObject (variant stays for direct consumers) |
| C1 | doc keeps public `from_nanos`; code follow-up |
| C2 | doc states public reachability of RecordKind/JournalFatal via the engine module; code follow-up |
| C3 | moot: max_turns removed (D3) |
| C4 | doc keeps Environment-owned origin; time.rs comment is a code follow-up |
| C5 | fixed: derives policy in §0 (normative derives only); Quiescence Ord not normative, code follow-up flag |
| C6 | fixed: `as_u64` documented |
| C7 | fixed: JRN-POISON gains the lying-sink trigger |
| C8 | fixed: crate layout lists bounded_buffer.rs; live/ sim/ marked planned |
| C9 | wiring keeps re-export policy as an open item; layout states intended public paths |
| C10 | fixed: panic stance scoped to shipped code (tests may unwind under the test profile) |
| C11 | covered by the Laws nonzero-config sentence; the buffer's wider range is mechanism |
| C12 | noted as deliberate in Journal (journal.rs committed) |
| D0 | no action (all IDs/refs resolved in v10) |
| D1 | fixed: RunStarted evidences index-0 acceptance |
| D2 | fixed: origin stated in ENV-TIME; live anchoring listed in Wiring (open) |
| D3 | moot: ID citations replace §-refs (Rule 4) |
| E1 | fixed: one statement (D13 corollary of RUN-FINALIZE) |
| E2 | fixed: "Closed" |
| E3 | fixed: poison defined once |
| E4 | fixed: "shutdown deadline" |
| E5 | fixed: two-word rule (D2) |
| E6 | fixed: terminal-Event completion stated once (Ports Notes); Sim cites it |
| E7 | dissolved: index exists only inside the token post-commit (D5) |
| E8 | fixed: EffectsComplete (D10) |
| E9 | fixed: two-word rule + glossary |
| F1–F2 | optional code follow-ups (journal test grouping/helpers) |
| F3 | fixed: test.md requires ID citations |
| F4 | fixed: test.md defines tests/ + trybuild homes |
| F5–F6 | optional code follow-ups (banners, test-name grammar) |
