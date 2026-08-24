# Adversarial Review: openai/gpt-5.6-terra, 2026-08-24
**Target:** design_docs/design-v12.md — > **Status:** Authoritative (v12). One section is open: Wiring & construction.
**Verdict:** Unsound. The closed Live state machine permits a clean `Stopped` exit after a test-profile user panic, contradicting A8. Its mandatory fault-suite cross-product is impossible for `start` errors; smaller vocabulary and citation defects remain.

## Findings
### TERRA-01 Test-profile Port panic can return `Stopped` — CRITICAL, confidence high, contradiction
- **Text attacked:** `A8`: “A panic — in Kavod or user code — is a bug: the process aborts, and no exit represents it.” `LIVE-COMPLETION`: “Each spawned shell exclusively owns one module-private, non-cloneable capability that changes only its Slot's entry from `Outstanding` to `Complete`, exactly once and infallibly, when that shell begins any non-aborting terminal exit: gate cancellation, return from `LivePort::run` with either result, or unwind under the test profile.” `LIVE-SUPERVISION`: “The transition out of `Running` and the latch close are one linearized instant (`LIVE-SHUTDOWN`), so every completion is unambiguously premature — publishing atomically with its classification — or expected, staying unpublished (A4).” `LIVE-SHUTDOWN`: “If every entry is `Complete`, shutdown joins every supervised thread and returns `Quiesced`.” `Phases / StopPending`: “Error `None` with `Quiesced` → the `TurnCompleted(Stop)` edge”; `Phases / Closed`: “Return `EngineExit::Stopped { state }`.”
- **Claim:** A test-profile unwind marks the Port complete but has no required Fatal path. After shutdown close it is expected and unpublished, so the graph requires a clean `Stopped` exit forbidden by A8.
- **Witness (reasoned):** One Port waits in `recv`; `on_start` returns `Stop` with an empty latch. Shutdown closes the latch and signals the Port; `recv` returns `Shutdown`, then the Port panics under the unwinding test profile. The terminal guard marks it `Complete`; shutdown joins it and returns `{ Quiesced, None }`; `TurnCompleted(Stop)` commits and the Engine returns `Stopped`.
- **Fix sketch:** Make a test-profile Port panic resume as a panic before any `ShutdownReport`, or explicitly and consistently exempt test-profile execution from A8.

### TERRA-02 `VERIFY-FAULTS` requires an impossible `start`-error cross-product — MAJOR, confidence high, unenforceable claim
- **Text attacked:** `VERIFY-FAULTS`: “A fault-injection suite exercises every edge: scripted sinks for Journal failures and scripted Environments for each operation's `Err` and for a shutdown report carrying `Some(error)`, checking the resulting `FatalCause`; this includes their cross-product, where the operation's Error remains the Fatal cause and the report's Error is discarded.” `ENV-SERIAL`: “After `start` returns `Err` there is no later call.” `run startup / step 2`: “`Environment::start`. | `Environment(Start)` Fatal with `Quiescence::Quiesced` — `ENV-START` already holds, so finalization skips `shutdown`.”
- **Claim:** The required cross-product cannot exist for `start` because a conforming run never calls `shutdown` after `start` returns `Err`.
- **Witness:** Use an Environment with `Error = u8`, scripted as `start() -> Err(1)` and `shutdown() -> ShutdownReport { quiescence: Quiesced, error: Some(2) }`. The Engine returns `Environment(Start)` and skips `shutdown`, so no run can exercise or discard `2`.
- **Fix sketch:** Limit the cross-product to failures after successful startup and test the `start`-error no-shutdown rule separately.

### TERRA-03 `External Event` has two incompatible meanings — MINOR, confidence high, ambiguity
- **Text attacked:** `Glossary / External Event`: “an Event delivered by `next_event`, as opposed to the start turn; External Events carry indices from 1.” `Glossary / Candidate`: “an Event returned by `next_event`: consumed, not yet accepted.” `Glossary / Accepted`: “`EventAccepted` for a candidate becoming one External Event. Only acceptance gives a turn its index and logical time.” `Edges / BetweenTurns -> EventAccepted`: “violation is `Core(TimeRegression)`, nothing committed, and the candidate stays consumed.”
- **Claim:** The text can mean either that every `next_event` return is an External Event, or that it becomes one only on acceptance. Those readings disagree on whether an unaccepted candidate must carry an index.
- **Witness:** With last accepted time `10`, let `next_event` return event `42` at time `9`. The Run raises `Core(TimeRegression)` without committing `EventAccepted`; event `42` is consumed and has no index, despite the first definition making it an External Event.
- **Fix sketch:** Define an External Event as an accepted candidate, and describe `next_event` as returning a Candidate.

### TERRA-04 Citation rules are violated by the document itself — NIT, confidence high, self-conformance violation
- **Text attacked:** `Reading rules / Placement rules`: “Citations point backward. Section order is dependency order; a fact that needs a forward reference is in the wrong section.” “Cite IDs. Never section numbers, here or in tests.” `SIM-TIME`: “Every armed time is `>= now` (`SIM-WAKEUP`) and selection takes the minimum.” `SIM-COMPLETION`: “A run ends normally through the finite-source pattern (Ports Notes).”
- **Claim:** `SIM-TIME` cites the later `SIM-WAKEUP` row, outside an allowed navigation exception. `Ports Notes` is neither an ID nor a binding-table name.
- **Witness:** `SIM-TIME` precedes `SIM-WAKEUP` in the Guarantees table, so its required dependency is forward; `SIM-COMPLETION` cites prose rather than an allowed citation target.
- **Fix sketch:** Reorder the dependent rows and replace `Ports Notes` with a backward ID-based definition.

## Attacked and held
- `ENV-LATCH` publication races: before, after, and overlapping observation points have a return-value witness.
- `RUN-INDEX` and failed `EventAccepted`: max-index precheck and time-regression candidate loss are specified.
- `RUN-FINALIZE` paths after pre- and post-commit failures preserve a single first-observed cause.
- `JRN-ENCODE`, flush commitment, and poison behavior close the partial-write paths.
- `SIM-LIFECYCLE` startup, command, step, and shutdown eligibility transitions are covered.
- `DET-RUN` and `DET-ENV` hold under their stated trusted determinism premises.

## Coverage
- 0. Reading this document — walked
- 1. Glossary — walked
- 2. Laws — walked
- 3. Application contract — walked
- 4. Port contract — walked
- 5. Environment contract — walked
- 6. Journal — walked
- 7. The Run — walked
- 8. Live Environment — walked
- 9. Simulated Environment — walked
- 10. Wiring & construction — walked, declared open and excluded as instructed
- 11. Crate layout — walked
- 12. Obligations & verification — walked
- Appendix A — skimmed

## Questions the document cannot answer
None.
