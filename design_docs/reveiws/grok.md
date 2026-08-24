# Adversarial Review: grok-4.6, 2026-08-23
**Target:** design_docs/design-v12.md — `Status: Authoritative (v12). One section is open: Wiring & construction.`
**Verdict:** Sound. Binding forms are consistent with each other and with the Glossary; every closed state machine, commitment row, and evidence claim survived a steelmanned walk. Section 10’s openness does not break any closed rule. No defect remains after constructing the strongest reading of each candidate.

## Findings
No findings.

## Attacked and held
- **Run graph walk:** every phase × every listed input (overflow, `Outcome::{Continue,Stop,Fatal}`, empty/nonempty batch, dispatch `Err` at `k=0` and mid-batch, checkpoint `Some`/`None`, `IndexExhausted` at `u64::MAX`, `next_event` `Err`, `TimeRegression` after consume, `EventAccepted` journal fail, Stop-path `Some`/`Incomplete`/`clean`, `TurnCompleted(Stop)` commit fail). No unlisted transition; `Closed` only via a clean report; Fatal always drops the certificate into `RUN-FINALIZE` once.
- **`RUN-FINALIZE` branches:** `start` `Err` → `Quiesced` and no second lifecycle call; `start` `Ok` + unconsumed → `shutdown`, report Error discarded; `StopPending` already consumed → retained quiescence, including after `TurnCompleted(Stop)` commit fail. No double-`shutdown`. Simultaneous `Some(error)` + `Incomplete` ranked by the `StopPending` row, not left to A4.
- **A4 observation vs wall-clock:** Application/`CommandBoundExceeded` Fatal skips the latch; a concurrent Port error is seen only in discarded finalization. First *observed* cause wins, as written.
- **At most one unaccepted consume:** `TimeRegression` and `EventAccepted` journal failure; the next call is `shutdown`. Matches the Trace’s “possibly the last `next_event` success.”
- **Evidence claims:** `TurnCompleted(Stop)` / `Stopped` require a clean report by the edge; `CommandsPrepared` + `Dispatch { position: k }` is the handed-off prefix `[0,k)` even when `Err` is a latched error; `Journal(CommandsDispatched)` requires every handoff; `take_error`/`ShutdownReport` `None` prove latch state at the snapshot/close, not “no Error ever”; `EnvironmentOperation` names observation, not cause.
- **`ENV-LATCH` interleavings:** before-call / after-return forced; overlap free on either side with the return as witness; pre-commitment own-`Err` is not an observation point; waiting `next_event` cannot ignore a latch that is the only wakeup. Live “check latch then admit/dequeue” realizes the before-call side.
- **Live shutdown × supervision:** one lock-step instant for signal / `Running` / fan-in / latch close makes completion premature-or-expected; `Complete` before close remains visible; expiry’s final observation decides the race; `Quiesced` joins only after every entry is `Complete`; join-after-deadline and hang-on-trusted-join are explicit. `VERIFY-LIVE` matches the row.
- **Sim lifecycle × select:** `Ended` only via `Err` or `stop`; after `on_command` `Err` the latch is pending so later observing calls never select; after `step` `Err` only `shutdown` follows; stale arms are unreachable. `SIM-START` prefix-`stop` and `SIM-SHUTDOWN` `Open`-only `stop` match `SIM-LIFECYCLE` and `VERIFY-SIM`.
- **`SIM-DISPATCH` vs commitment table:** `on_command` `Err` publishes and returns `Ok` (after-handoff). Contrast with `step(Err)` returning `next_event`’s own pre-consumption `Err` is forced by `ENV-ERRORS`. Publish-and-return of that same error would mark the latch `reported` (`pending → reported when an operation returns it as its Err`), so the final report is still `None`.
- **Equal-time sim arms:** min time + cursor scan + wrap is deterministic; selecting the min preserves `arm >= now` when `now` advances; `set_next` cannot arm another Port.
- **A2 vs synchronous `on_command`:** invocation *is* the handoff commitment, not processing after it. Survives.
- **A5 vs consume-then-`EventAccepted`:** the record announces acceptance, not consumption. Activation-before-`RunStarted` is the same pattern and is derived in Notes.
- **Journal arithmetic:** `max_record_bytes.checked_add(1)` overflows only at `usize::MAX` → `MaxBytesTooLarge`; committed object ≤ `max_record_bytes` because the extra byte is the newline; `Interrupted`/`Ok(0)`/over-report poison and do not retry; encode failures write nothing and poison nothing. `NotAnObject` is the document’s byte test, not RFC 8259 (raw `Serialize` can emit `{}}`).
- **serde claims (reasoned):** newtype `EventIndex`/`Timestamp` serialize as inner `u64`; named-field structs are objects; `serde_json` escapes interior newlines in ordinary values; unit-enum outcome tags are bare strings; `Never`’s `match *self {}` is a valid uninhabited `Serialize`.
- **Determinism (A9 / `DET-RUN` / `DET-ENV`):** Core has no free choice once the trace (including erased Error presence, sink `Ok` counts, and `ShutdownReport`) is fixed. Live source races and latch overlap are Environment choices recorded in the trace. Inbox/fan-in “queue” is FIFO under the only reading “queue” forces; result is in the trace either way.
- **`RUN-GRAMMAR` / fused `dispatch_batch`:** mid-transition after `CommandsPrepared` is not “possession of a `TurnOpen` certificate” in the proof sense; `Prepared` is realized internally; record sequence and prefix/`Journal(CommandsDispatched)` outcomes match the binding tables. Drop-without-commit is the Fatal path and is test-enforced, as admitted.
- **`EngineExit` `PartialEq` vs `serde_json::Error`/`io::Error` (reasoned):** listed derive on `EngineExit` compiles with a `FatalCause: PartialEq` bound; inner Error types need not be `Eq`. `DET-ENV` compares variants/`SinkOperation`, not those payloads.
- **Typestate / ownership:** consuming `shutdown`, non-`Clone` certificate, mint consuming the Journal, `Context` as the only handler capability, `Send + 'static` only on the Live boundary — realizable in safe Rust.
- **Forward-citation / placement rules:** trust-marks, bounds registry, ownership map, invariant index, and the contract’s pointer to shipped impls are the listed exemptions. Same-section row references (e.g. `RUN-GRAMMAR` → `RUN-ENFORCEMENT`) do not invert section dependency order.
- **Enforcement tiers:** rules claimed unrepresentable are carried by phase types, consuming `shutdown`, or fused dispatch; the three runtime points and omitted-record affinity are named in `RUN-ENFORCEMENT` and pinned by `VERIFY-GRAMMAR` / `VERIFY-JOURNAL`. Obligation IDs are the complete trusted boundary.
- **Failure of the failure path:** `shutdown` is infallible (report, not `Result`); post-close Errors stay inside the Environment; poisoned Journal is destroyed with the certificate; Sim `stop` Errors discarded; Live Incomplete detaches and relies on `TRUST-EXIT`.

## Coverage
- **0 Reading this document:** walked
- **1 Glossary:** walked (every term against uses)
- **2 Laws:** walked (axioms, guarantee rows, maps)
- **3 Application contract:** walked
- **4 Port contract:** walked
- **5 Environment contract:** walked (commitment table × all ops)
- **6 Journal:** walked
- **7 The Run:** walked (construction, startup, phases, edges, records, enforcement)
- **8 Live Environment:** walked
- **9 Simulated Environment:** walked
- **10 Wiring & construction:** skipped (declared open)
- **11 Crate layout:** skimmed
- **12 Obligations & verification:** walked
- **Appendix A Invariant index:** skimmed (ID census only)

## Questions the document cannot answer
- Everything Section 10 lists (builders, Error sums, `LiveCtx` finals, `LiveConfig`/`SimConfig`, what fixes Slot order, re-exports, thread names).
- `LiveCtx` behavior on a thread detached after `Incomplete` once the Environment value has been dropped — underivable; `TRUST-EXIT` forbids depending on it.
- How `VERIFY-GRAMMAR`’s compile-fail target names module-private types while remaining a passing test of the production visibility boundary (realizable, mechanism unstated).
