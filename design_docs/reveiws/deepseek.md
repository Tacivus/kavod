# Adversarial Review: design-v12.md

**Date:** 2026-08-18  
**Reviewers:** 6 parallel adversarial agents, each attacking from a different angle  
**Scope:** Semantic correctness, internal consistency, edge-case robustness  
**Excluded:** Implementation details, code quality, performance

---

## Summary

| Severity | Count |
|----------|-------|
| Critical | 3 |
| Major | 11 |
| Minor | 14 |
| Cosmetic | 8 |
| **Total** | **36** |

The design is very strong — most findings are documentation gaps, imprecise scoping, or under-justified choices. Three critical issues deserve immediate attention: the trace's dual role (output defined as input), the trace's ownership under A1, and a forensic gap in live concurrent Port failures.

---

## CRITICAL (3)

### C1. Trace defined as output, used as input — foundational ambiguity

**Agent:** Time & Determinism (Issue 1)  
**Sections:** Glossary §1 (line 125–130), A9 (line 148), `DET-RUN` (line 745), `DET-ENV` (line 746)

The Glossary defines **Trace** as "the run's full operation-result history" — observer-created output. But A9 says "every run output is a function of... **the trace**," treating it as input. `DET-RUN` and `DET-ENV` both use "the same trace" as a precondition for reproducing outputs. A thing cannot be both input and output simultaneously without clarification.

The sim Notes (lines 1055–1058) partially resolve this by describing replay with a "fixed or recorded trace presented by a user-written SimPort," confirming the trace-from-a-prior-run-as-input interpretation. But the Glossary never acknowledges this dual role, and `DET-RUN`/`DET-ENV` don't distinguish "recorded trace used as script" from "observed trace."

**Resolution:** Update the Glossary Trace definition to describe both roles: as the run's observed history, and as the input script for replay (the determinism function's `trace` argument). Reword `DET-RUN` to clarify that "the same trace" means "the same trace used as the Environment script," not "the same output trace reproduced."

---

### C2. A1 (Single authority) violated by the Trace — unowned normative fact

**Agent:** Cross-section (Issue 4)  
**Sections:** A1 (line 140), Glossary Trace (lines 125–130), Ownership map (lines 186–192)

A1 states "Every fact has exactly one owner; every appearance outside its owner is a read-only view of it." The Trace is a fact — referenced normatively by A9, `DET-RUN`, and `DET-ENV` — but it appears in no component's ownership row. The Run owns "The graph, the records, the certificate (index and time), Fatal classification." The Trace is not listed. It is described as a conceptual construct, never stored, but A1 makes no exception for derived or observational facts.

**Resolution:** Assign ownership of the Trace to the Run: extend the ownership map row to include "the Trace — a derived view the Run alone assembles from its own Journal and Environment-call results; not stored as a discrete artifact."

---

### C3. Live concurrent Port failures after `CommandsDispatched` leave no Journal evidence

**Agent:** Journal & Persistence (Issue 4)  
**Sections:** Records table (lines 722–723), `LIVE-DISPATCH` (line 916), `LIVE-SUPERVISION` (line 917)

In the live Environment, a Port processes Commands asynchronously on its own thread. `dispatch` hands off to inboxes without waiting for processing. `CommandsDispatched` commits after all handoffs. Then the checkpoint reads the latch. A Port could process a Command, commit side effects (network writes, database mutations), and then fail — all between dispatch and checkpoint. The Journal shows `CommandsDispatched` followed by silence. A forensic reader sees "all dispatched successfully" when in reality a Port failed *because of* the Command it processed. The `FatalCause` identifies only the *observation point* (`Checkpoint`), not which Slot or Port errored.

The derived note at lines 832–835 acknowledges temporal ambiguity but not the forensic gap: the Journal alone cannot identify the failing Port.

**Resolution:** Add a record committed between `CommandsDispatched` and the Fatal drop that carries the Slot identity (as a string or variant tag, not the Error value per A7). Alternatively, accept the gap and document it explicitly in the Records section with a cross-reference to `FatalCause` in the exit as the sole forensic carrier.

---

## MAJOR (11)

### M1. "Three points stay runtime" list in Enforcement is incomplete

**Agent:** Type System (Issue 1)  
**Sections:** Enforcement (lines 805–808)

The Enforcement subsection lists three runtime-remaining points:
1. Index arithmetic behind `accept_event`
2. The answer passed to `checkpoint`
3. The batch slice passed to `dispatch_batch`

Actually, at least **five** points escape the type system:
- **The start time argument** passed to `run_started()` — a wrong value corrupts the certificate's `last_time`.
- **The (Event, Timestamp) tuple** passed to `accept_event()` — a wrong timestamp bypasses the `TimeRegression` check and pollutes the certificate's `last_time`.
- The three already listed.

The first two are identical in kind to the listed ones (bare runtime values entering certificate transitions). The list should be 5 items, not 3.

**Resolution:** Expand the list to include the start time and event/time tuple arguments.

---

### M2. `CommandBoundExceeded` as `CoreError` — classification erases Application intent

**Agent:** Error Handling (Issue 3)  
**Sections:** `TurnOpen` state (line 688), `CoreError::CommandBoundExceeded` (line 639), `TRUST-PURE` (lines 1142–1143)

When a handler overflows the Command batch, the exit reports `Core(CommandBoundExceeded)`. The handler's own `Outcome` — whether it returned `Continue`, `Stop`, or `Fatal("db failure")` — is discarded. Three problems:

1. **Causation mismatches ownership.** The Application authored the `emit` calls; the exit says "the Core failed."
2. **The Application has no defense.** `emit` is infallible (`APP-EMIT`). After `remaining() == 0`, the first over-bound `emit` has already set the marker. The handler can't avoid it.
3. **`TRUST-PURE` verification is impaired.** Two different Application bugs (one overflows and returns `Continue`, another overflows and returns `Fatal("db failure")`) produce identical exits — the Application Error is erased, hiding diagnostic information from its own author.

**Resolution:** Add the handler's `Outcome` to the `CoreError::CommandBoundExceeded` variant (e.g., `CommandBoundExceeded { outcome: Outcome<AE> }`). The cause remains Core, but the Application's intent is preserved for diagnostics and purity verification.

---

### M3. Port errors invisible in Journal — `CommandsDispatched` is misleading evidence

**Agent:** Journal & Persistence (Issue 1) — also relates to C3  
**Sections:** Records table (lines 722–723), `SIM-DISPATCH` (line 1026)

In the sim, a Port's `on_command` can return `Err` after having already committed side effects. The Error latches. The checkpoint observes it. The run goes Fatal. The Journal contains `CommandsDispatched` (evidencing "every prepared Command was handed off") but nothing about which Port failed or whether side effects were committed. A forensic reader sees a clean dispatch and then silence. The `FatalCause` carries the observation point (`Checkpoint`) but not which Slot.

Similar to C3 but in the sim — the gap is that `CommandsDispatched` reads as "everything was fine" when Ports may have failed mid-processing.

**Resolution:** Same as C3 — add a `PortError` record or document the gap explicitly.

---

### M4. Uncertain suffix language only covers sink failures, not crashes

**Agent:** Journal & Persistence (Issue 2)  
**Sections:** `JRN-COMMIT` (line 513), step 5 write loop (line 528)

`JRN-COMMIT` says "After a **sink failure**, bytes past the last committed record are an uncertain suffix." But a process crash (SIGKILL, power loss) during step 5's write loop produces the same physical artifact — partial bytes on the sink, no flush completed, no sink failure observed. The Journal is not poisoned. On restart, the sink holds garbage beyond the last committed boundary. The doc's panic-abort paragraph (lines 156–158) implicitly trusts that only flush-complete records survive a crash, which is false for anything written during step 5 but not yet flushed.

**Resolution:** Broaden `JRN-COMMIT`'s statement to cover any non-flush termination. Add to `TRUST-SINK`: "On restart, the sink owner must truncate or ignore bytes after the last complete JSON line."

---

### M5. Committed-byte boundary unknowable from sink after Fatal

**Agent:** Journal & Persistence (Issue 7)  
**Sections:** `JRN-COMMIT` (line 513), `JRN-POISON` (line 514), `RUN-FINALIZE` (line 744)

When a sink failure poisons the Journal, the certificate is dropped, the Journal destroyed. The `EngineExit::Fatal` carries no offset marking the committed boundary. The caller retains the sink but has no way to separate committed bytes from the uncertain suffix. The doc itself says "replay needs a cleanly completed Journal or an externally trusted boundary" (lines 549–550). On a Fatal run, neither is available — the caller must guess.

**Resolution:** Add `committed_bytes: u64` to `EngineExit::Fatal` (or to `JournalFatal`) recording the number of bytes successfully flushed. The caller can truncate at that offset.

---

### M6. Trace includes sink results — circular with Journal bytes guarantee

**Agent:** Time & Determinism (Issue 2)  
**Sections:** Glossary Trace (lines 126–127), `DET-RUN` (line 745)

The Glossary includes "every sink call's result" in the trace. `DET-RUN` says "the same trace reproduce the same Journal bytes." If the trace already specifies sink results (success/failure), it partially predetermines Journal content. The guarantee should separate: the trace determines Environment operations and sink success/failure, but Journal bytes (payload content, field values) are Core output determined from those inputs — not already encoded in them.

**Resolution:** `DET-RUN` should clarify that the trace fixes Environment operations and sink success/failure indicators, but Journal bytes are the Core's output. Alternatively, define a separate "trace script" concept that omits sink write counts.

---

### M7. `DET-ENV` requires equal timestamps across Environments — never explained how to achieve this

**Agent:** Time & Determinism (Issue 3)  
**Sections:** `DET-ENV` (line 746), `LIVE-TIME` (line 915), `SIM-TIME` (line 1025)

`DET-ENV` states "equal traces produce equal... Journal bytes." But traces contain `(Event, Timestamp)` pairs. The live Environment stamps from a monotonic system clock (e.g., nanosecond epoch); the sim stamps from a configured origin. These are definitionally unequal. The guarantee acknowledges "binds where equal traces exist" but never explains when that holds. The answer (buried in sim Notes, lines 1060–1063) is: configure the sim's origin to the recorded `RunStarted` time and script replay Ports to arm at each recorded stamp.

**Resolution:** Cross-reference the replay preconditions directly from `DET-ENV`. Consider promoting the replay preconditions from sim Notes into a named guarantee row (`SIM-REPLAY`).

---

### M8. Round-robin cursor is hidden state not captured in the trace

**Agent:** Time & Determinism (Issue 5)  
**Sections:** `SIM-SELECT` (line 1028), Glossary Trace (lines 125–130)

The sim's round-robin cursor "starts at Slot 0, persists across `next_event` calls, and moves to the selected Slot's successor after every selected step." The trace captures the accepted `(Event, Timestamp)` sequence but not the cursor's value at each `next_event` entry. If two Ports arm at the same time, which fires first depends on cursor history. To replay a recorded trace, you need the cursor position — but the trace doesn't carry it. Same-timestamp events from different Slots have an ordering that depends on invisible state.

**Resolution:** Either (a) include the cursor in the trace definition, (b) replace round-robin with a stateless tiebreaker (always lowest Slot index first for equal times), or (c) document that replay requires cursor capture and that `DET-RUN`'s "same trace" precondition includes cursor history where ties exist. Option (b) is cleanest.

---

### M9. A3 (One commitment point) — unqualified universal quantifier contradicts Port design

**Agent:** Cross-section (Issue 5)  
**Sections:** A3 (line 141), Ownership map Port row (line 189)

A3 states "Every effectful operation has exactly one commitment point" without qualification. But Ports own their native state and protocols — a live Port's `on_command` might write to a TCP socket and a database, each with its own commitment point outside Kavod's control. The axiom's literal reading forbids multi-step Port operations, which the rest of the design clearly permits. The scope resolution is implicit through the ownership map.

**Resolution:** Scope A3: "Every **Kavod-owned** effectful operation has exactly one commitment point. Port-internal commitment points are Port-authored and not governed by A3."

---

### M10. `SIM-STEPS` shared budget enables DoS from a single non-progressing Port

**Agent:** Time & Determinism (Issue 6)  
**Sections:** `SIM-SELECT` (line 1028), `SIM-STEPS` (line 1029)

The step budget is shared across all Ports in one `next_event` acquisition. A Port that re-arms at `now` and always returns `None` from `step` burns the entire budget on every call, starving other Ports. `TRUST-SIM-PORT` requires bounded `step` work, but this is trusted, not enforced. A single misbehaving Port can cause `next_event` to exhaust the budget and the run to go Fatal, even if other Ports have real work.

**Resolution:** Add a per-Port sub-budget, or make the scheduler skip a Port that returned `None` in the current acquisition without clearing its arm (only re-select it if no other armed Ports remain). Alternatively, explicitly state that repeated `step(None)` is a `TRUST-SIM-PORT` violation and the resulting Fatal is its enforcement.

---

### M11. `mem::forget` on the certificate bypasses `RUN-FINALIZE` drop

**Agent:** Type System (Issue 2)  
**Sections:** Enforcement (lines 802–803), `RUN-FINALIZE` (line 744)

The doc admits "Dropping a certificate and committing nothing type-checks — that is the Fatal path by design." The Fatal path works via `Drop`. But `mem::forget` leaks the certificate without running `Drop`. The Journal is leaked (never destroyed), the Environment is leaked (no `shutdown`), and no `RUN-FINALIZE` runs. This is safe Rust and unpreventable without `unsafe`. The doc never mentions it.

**Resolution:** Add a note acknowledging `mem::forget` as an unenforceable path (safe Rust provides no defense). State that it is a user bug equivalent to `Box::leak` — the process still terminates, and the evidence is whatever committed records exist plus OS cleanup. No design change possible within `#![forbid(unsafe_code)]`.

---

## MINOR (14)

### m1. Latch priority within `next_event` — correct but undocumented as a rule

**Agent:** Error Handling (Issue 1)  
`ENV-LATCH` says a waiting `next_event` returns once the latch is pending, and the mechanism checks the latch first. But no guarantee row states this ordering as a rule. The mechanism is correct; the gap is documentation only.

**Resolution:** Add to `ENV-LATCH` or `LIVE-SELECT`/`SIM-SELECT`: "Within `next_event`, the latch check precedes all fallible mechanism steps."

---

### m2. `start` returns `Err` — documentation gap on latch emptiness

**Agent:** Error Handling (Issue 4)  
`SIM-START` says failures before activation are `start`'s own `Err`, not latched — this is correct per `ENV-ERRORS`. But the prose doesn't state it explicitly; an implementer could double-publish.

**Resolution:** Add to `SIM-START` and `LIVE-START`: "Pre-commitment failures are returned as `start`'s own `Err` and are not published to the latch (`ENV-ERRORS`)."

---

### m3. Pre-cancellation shell observable work — undocumented invariant

**Agent:** Error Handling (Issue 7)  
`LIVE-START`'s gate prevents Port code execution, but the shell body before the gate-wait could execute Environment code. No rule prohibits observable effects there.

**Resolution:** Add to `LIVE-START`: "The shell body performs no observable work before waiting at the gate."

---

### m4. `Interrupted` poisons Journal — no justification given

**Agent:** Journal & Persistence (Issue 3)  
Step 5 treats `Interrupted` as poison with zero retries. The design choice is defensible (retry loops can be unbounded per `BOUND-LOOPS`), but no justification exists.

**Resolution:** Add a one-line justification: "Retrying `Interrupted` would make the loop's termination depend on an unbounded external signal stream; treating it as poison keeps the bound structural."

---

### m5. Dual `BoundExceeded` paths in Journal — correct but confusing

**Agent:** Journal & Persistence (Issue 5)  
Step 2 catches `WriteZero` (object larger than buffer capacity). Step 4 catches the object filling the buffer exactly, leaving no room for the newline. Both produce `BoundExceeded` but through different code paths. The distinction is correct but undocumented.

**Resolution:** Add a clarifying note or simplify by checking `remaining() < 1` after encoding, eliminating step 4's path.

---

### m6. `TimeRegression`-rejected Event leaves no Journal record

**Agent:** Journal & Persistence (Issue 6)  
A candidate consumed by `next_event` but rejected by `TimeRegression` stays consumed but unaccepted. The Journal has no record of it. The trace preserves it, but the trace is not persistent.

**Resolution:** Document this as a known forensic gap in the Records section. Optionally commit a `EventRejected` record before the Fatal.

---

### m7. Hand-written Port sum "observationally identical" — undefined term

**Agent:** Type System (Issue 4)  
The `ports!` mechanism says hand-written equivalents are "observationally identical" and shifts `PORT-SUMS` onto `TRUST-ROUTING`. "Observationally identical" is never defined — what properties must a hand-written sum preserve? Variant names, ordering, serde representation, trait derivations?

**Resolution:** Define "observationally identical" explicitly: same variant names, same variant order, same serde representation, same trait implementations that Kavod depends on.

---

### m8. Wiring-section capacities lack committed `NonZero` types

**Agent:** Type System (Issue 3)  
`EngineConfig` uses `NonZeroUsize` (`BOUND-NONZERO`). But Wiring-defined capacities (shutdown deadline, step budget, inbox sizes, fan-in queue size) have no types committed. They could be plain `u64`/`usize`, violating `BOUND-NONZERO`.

**Resolution:** Add to the Wiring constraints: "All capacities declared in Wiring use nonzero types (`NonZeroU64`, `NonZeroUsize`, etc.) per `BOUND-NONZERO`."

---

### m9. "Unconstructible even in-module" overstates the ZST guarantee

**Agent:** Type System (Issue 5)  
The doc claims a kind/payload mismatch is "unconstructible even in-module" via the `RecordPayload` trait with a ZST field. This is true for the default `Serialize` derive. But a `Serialize` impl can return any bytes for any value — the ZST prevents Rust-level mismatches, not serialization-level ones. The serialization integrity relies on `TRUST-SERIALIZE` and golden tests.

**Resolution:** Replace "unconstructible even in-module" with "unconstructible in safe Rust; serialized correctness is enforced by the `TRUST-SERIALIZE` obligation and golden tests."

---

### m10. `BOUND-STATIC` "nonempty" enforcement unspecified

**Agent:** Cross-section (Issue 7)  
`BOUND-STATIC` requires a nonempty Port set, but enforcement is unspecified (e.g., type-level or builder assertion). The Wiring section is open, but the gap is not signposted.

**Resolution:** Add to Wiring constraints: "Enforcement of nonempty Port set: unrepresentable (type-level nonempty builder) or asserted (builder panic), pending Wiring design."

---

### m11. SimPort cleanup-on-Err obligation is implicit

**Agent:** Cross-section (Issue 8)  
`SIM-LIFECYCLE` says a Port returning `Err` gets no further method calls, including `stop`. This means Ports must self-cleanup on any `Err` return. Neither the Port contract nor the SimPort API block states this positive obligation.

**Resolution:** Add to the SimPort API block: "A method returning `Err` must leave the Port in a terminal state comparable to `stop` — all resources released, no further externally consequential work." Same for the live equivalent if applicable.

---

### m12. `PORT-STATE` "without interpreting" vs. routing — wording invites misreading

**Agent:** Cross-section (Issue 9)  
`PORT-STATE` says the Environment "relays [Port] values without interpreting them." But routing by enum discriminant *is* a form of interpretation. The intended distinction is "without inspecting the payload content."

**Resolution:** Amend: "relay its values through routing without interpreting their payload content."

---

### m13. Provisional LiveCtx signatures vs. guarantee rows — dependency unflagged

**Agent:** Cross-section (Issue 6)  
Live guarantee rows reference `LiveCtx` methods (`recv`, `try_recv`, `offer`, `lifecycle`) whose signatures are provisional. Changes to signatures could invalidate guarantee text.

**Resolution:** Add to the Live API block's provisional notice: "Guarantee rows referencing these methods bind the described behavior, not the exact signature."

---

### m14. `SIM-COMPLETION` startup requirement implicit

**Agent:** Shutdown/Lifecycle (Issue 6)  
A Port set that never arms exits with `SIM-COMPLETION` on the first `next_event`. The requirement that Ports arm during `start` to make progress is implicit.

**Resolution:** Add to `SIM-COMPLETION` or its notes: "A Port set must arm in `start` to make progress; a set that never arms exits with `SIM-COMPLETION`."

---

## COSMETIC (8)

### c1. Bare verb "commits" disambiguates only by context

**Agent:** Cross-section (Issue 1)  
The Glossary defines both "Commit" (Journal: encode, write, flush) and "Commitment point" (general). Prose uses "commits" for both senses without signaling which is intended.

**Resolution:** Adopt a convention: "Journal commit" for the Journal sense, "commitment point" for the general sense.

---

### c2. "Poisoned" vs. "reported" — asymmetry without cross-reference

**Agent:** Cross-section (Issue 2)  
"Poisoned" is a Glossary entry for the Journal's permanent error state. "Reported" is the latch's analog, defined inline with no glossary entry. No cross-reference connects them.

**Resolution:** Add a "reported" glossary entry, or add a cross-reference to the Latch entry.

---

### c3. "Fatal" vs. Quiescence boundary visually blurred

**Agent:** Cross-section (Issue 3)  
`EngineExit::Fatal` carries both `cause` and `quiescence`. The Glossary defines Fatal as "the classification of the Error or Core condition," which is only the `cause` field. The `quiescence` field's presence inside the Fatal variant invites misreading.

**Resolution:** Amend the Glossary: "Fatal — the run-level classification... (The exit also carries Quiescence, which is not part of the classification)."

---

### c4. `MaxBytesTooLarge` variant name misleading

**Agent:** Journal & Persistence (Issue 9)  
The variant fires when `max_record_bytes.checked_add(1)` overflows, not when the value is too large for the buffer.

**Resolution:** Rename to `MaxBytesWouldOverflow` or `ReservationOverflow`.

---

### c5. Write-loop "bounded by record length" is a trivial bound

**Agent:** Journal & Persistence (Issue 8)  
Step 5 says the write loop is "bounded by record length." This is just the loop's termination condition, not an independent A6 bound. The real bound is that each non-failing iteration advances by at least one byte, making the iteration count ≤ record length.

**Resolution:** Rephrase or remove.

---

### c6. ShutdownReport dual information — prose phrasing invites misreading

**Agent:** Error Handling (Issue 6)  
The `StopPending` row says "a reported Error outranks `Incomplete` as cause." Both `error` and `quiescence` are preserved separately in the exit, but the phrase "outranks as cause" could be interpreted as "Incomplete is discarded."

**Resolution:** Rephrase: "Error `Some` → `Environment(Shutdown)` with the report's quiescence preserved in the exit's `quiescence` field."

---

### c7. TimeRegression check redundant in live Environment — undocumented

**Agent:** Time & Determinism (Issue 8)  
The `TimeRegression` check in the Run can never fire in the shipped live Environment (`LIVE-TIME` structurally prevents it). This is by design (belt-and-suspenders for bespoke Environments), but not stated.

**Resolution:** Add a note: "This check is structurally redundant in the shipped live Environment but enforced for all Environments, including bespoke ones."

---

### c8. "Prompt by construction" overstated in `LIVE-SHUTDOWN`

**Agent:** Shutdown/Lifecycle (Issue 2)  
The guarantee says completion publications are "prompt by construction." The publication acquires the latch lock, which `shutdown` holds during its close. If the deadline expires during that block, the Port may be classified `Incomplete` despite having already finished.

**Resolution:** Replace "prompt by construction" with "structurally guaranteed: publication follows the Port's last work with no intervening operation."

---

## Issues Found to be Correct (No Defect)

The following concerns were investigated and found to be correctly handled by the design:

1. **State mutation + discarded batch vs. A9** — The handler cannot observe the batch's disposition through `Context`'s API surface. A9 holds.
2. **Multi-Port dispatch partial handoff** — The `Dispatch { position }` semantics and the `ENV-LATCH` interaction are correctly specified at three locations.
3. **`start Err` → latch content lost** — `ENV-ERRORS` correctly draws the line; pre-commitment failures are never published to the latch.
4. **`recv`/`try_recv` split for draining** — Correctly puts the draining obligation on Port authors via `TRUST-DRAIN`, where protocol knowledge lives.
5. **`LIVE-SUPERVISION` premature/expected classification** — The linearized lifecycle-flip-and-latch-close makes every completion unambiguously premature or expected.
6. **`SIM-SHUTDOWN` "structurally None" claim** — Correct for Environment Errors; clarified scoping resolves the cosmetic issue.
7. **Encoding-failure buffer state** — Confirmed benign: step 1 clears the buffer before every `commit` call.

---

## Cross-Cutting Themes

### 1. The trace is the document's most overworked concept
The trace appears as input, output, determinism guarantee, cross-Environment comparison basis, and replay script. Its definitional ambiguity (C1, M6, M7, M8) accounts for the most critical and several major issues. Clarifying trace-as-input vs. trace-as-output would cascade into corrected wording in A9, `DET-RUN`, and `DET-ENV`.

### 2. The Journal's forensic claims overstate its completeness
The design honestly acknowledges the uncertain suffix and the possibility of `CommandsDispatched` as a final record. But two gaps (C3, M5) mean a forensic reader cannot always determine which Port failed or where committed bytes end. These are fixable with a `PortError` record and a `committed_bytes` field in the exit.

### 3. Several enforcement claims are slightly overstated
- "Three points stay runtime" is really five (M1)
- "Unconstructible even in-module" is only true for Rust-level construction, not serialization (m9)
- "Prompt by construction" is structurally correct but not latency-guaranteed (c8)

### 4. The Wiring section's openness hides capacity type commitments
Several capacity values (inbox sizes, step budget, shutdown deadline) have no committed types. `BOUND-NONZERO` requires `NonZero*` types, but these are not yet specified (m8).

---

## Overall Assessment

The design is **robust and internally consistent at its core**. The axioms, guarantee rows, binding tables, and the certificate/phase/edge graph form a mechanically sound system. Most findings are:

- **Documentation gaps** (latch priority ordering, `start Err` path, SimPort cleanup contract) — fixable with additional prose, no design changes.
- **Imprecise scoping** (A3's universal quantifier, "without interpreting," "observationally identical") — fixable with tighter wording.
- **Forenisc gaps** (Port identity in Fatal Journals, committed-byte boundary) — fixable with additional exit fields or records.
- **The trace definition** — needs foundational clarification of its dual role.

The three critical issues (C1, C2, C3) should be resolved before the design is considered frozen. The eleven major issues deserve attention but none fundamentally invalidates the approach.
