# Adversarial Review: Claude Fable 5, 2026-08-24

**Target:** design_docs/design-v12.md — `> **Status:** Authoritative (v12). One section is open: Wiring & construction.`
**Verdict:** Sound, with minor fixes. The core machinery — the phase/edge/record graph, the latch state machine and its ordering rules, the commitment-point discipline, the Journal byte arithmetic, and both Environment realizations — survived systematic walking of every state, failure path, and interleaving I could construct, and every executable realizability claim I tested ran as written (the doc's own `RunStarted` example reproduces byte-for-byte from the described mechanism). The surviving defects are gaps in the document's self-imposed enforcement discipline and one genuine API ambiguity, none of which makes any guaranteed outcome unreachable.

## Findings

### FBL-01 `DET-RUN` has no stated enforcement tier — MINOR, confidence medium, self-conformance violation

- **Text attacked:** Section 0: "Every ID outside the Obligations table is **enforced**: violation is unrepresentable, panics an always-on assertion, or is pinned by a named test suite." `DET-RUN`: "Within one Environment type: the same build … and trace reproduce the same handler calls, State transitions, Command intent, and Journal bytes …"
- **Claim:** No tier can be assigned to `DET-RUN` from the text: cross-run equality is not unrepresentable, no assertion can check it within one run, and no named suite pins it. The document's enforced/trusted dichotomy is therefore undischarged for this ID.
- **Witness:** Audit the three tiers: `VERIFY-CONFORMANCE` explicitly compares "every Core-owned discriminant and payload in **`DET-ENV`'s** list" (cross-type, not within-type); the only two-run repeatability check in the document is `TRUST-PURE`'s Verified-by column ("Two runs against the same scripted Environment and sink → identical Journal bytes and `DET-RUN`-equal exits"), which Section 0 assigns to the *Application author* as the check of a trusted obligation, not to Kavod as enforcement of `DET-RUN`. Counter-reading noted: A1–A9 also carry no suites, so the trichotomy may be intended loosely for derived-property rows — but `DET-RUN` is a guarantee row, exactly the form the sentence governs.
- **Fix sketch:** Add within-type repeatability cases to `VERIFY-CONFORMANCE` (run each scripted trace twice, compare `DET-RUN`'s list) and cite it from `DET-RUN`.

### FBL-02 `try_recv` post-drain behavior is unspecified — MINOR, confidence high, ambiguity

- **Text attacked:** `LiveCtx` API block: "`recv`: … Once raised, every call reports the signal; `try_recv` is the draining path." "`try_recv`: Nonblocking: pending Commands first, then the signal." `LIVE-LIFECYCLE`: "`try_recv` yields queued Commands first and the signal after them, which is the draining path."
- **Claim:** After `try_recv` has drained the inbox and returned the signal once, nothing fixes what subsequent calls return; the `None` case of `Option<PortInput>` is never described at all. "Every call reports the signal" sits in `recv`'s doc comment and does not bind `try_recv`.
- **Witness:** Signal raised, inbox holds C1, C2. Port calls `try_recv` four times. Implementation A returns `Some(Command(C1))`, `Some(Command(C2))`, `Some(Shutdown)`, `Some(Shutdown)`; implementation B returns `…`, `Some(Shutdown)`, `None`. Both satisfy "pending Commands first, then the signal," and neither "hides" the signal (`lifecycle()` still reports it) — yet a Port loop written `while let Some(input) = ctx.try_recv()` behaves differently under each. The intended reading (signal persists, mirroring `recv`) is suggested but not forced.
- **Fix sketch:** State that once raised and drained, every `try_recv` returns `Some(Shutdown)`, and define `None` as "no Command pending and no signal raised."

### FBL-03 `VERIFY-GRAMMAR`'s compile-fail suite cannot live where the row places it — MINOR, confidence medium, unenforceable claim (reasoned)

- **Text attacked:** `VERIFY-GRAMMAR`: "A compile-fail suite proves illegal transition sequences … do not compile …; it lives where the module-private grammar types are visible." `RUN-ENFORCEMENT`: "Certificate, phase, and transition types are module-private." Derive: "**Unforgeable means module-private.** The certificate, phases, and transitions hold their guarantees exactly as long as they stay behind their modules."
- **Claim:** Standard compile-fail tooling (trybuild/UI tests) compiles separate crates, from which `pub(super)` items are unnameable — every fixture then fails with a *privacy* error regardless of the grammar, proving nothing the row claims; and an in-tree fixture that fails to compile breaks the build, so no stable mechanism hosts a compile-fail test "where the module-private grammar types are visible."
- **Witness:** A trybuild fixture attempting `certificate.clone()` fails E0603 (private module) before the absence of `Clone` is ever tested; making the types visible to fix that contradicts `RUN-ENFORCEMENT`'s module-privacy, on which `RUN-GRAMMAR`'s unforgeability explicitly rests.
- **Fix sketch:** Name the mechanism — e.g., the fixture crate `include!`s `engine/record.rs` to reconstruct the module boundary and attempts the illegal sequences from the Engine's visibility position.

### FBL-04 "Publication" used against its Glossary binding — NIT, confidence high, vocabulary

- **Text attacked:** Glossary: "**Publication** — entry of an Error into the latch." `ENV-LATCH`: "Every publication after the first, and every publication after the close, is discarded."
- **Claim:** Under the Glossary binding, a discarded publish never *enters* the latch, so "a publication … discarded" is self-contradictory; the row plainly means the *act* of publishing (the attempt). One intended reading exists.
- **Witness:** Latch closed; a Port's Error is offered. By `ENV-LATCH` it is "discarded" — yet by the Glossary a discarded offer is not a publication, making the discard clause denote nothing.
- **Fix sketch:** Define Publication as "the act of offering an Error to the latch; entry succeeds only per `ENV-LATCH`."

## Attacked and held

- **`ENV-LATCH` × every operation and overlap placement** — walked all four latch states against `next_event`, `dispatch`, `take_error`, and the close, including the waiting-`next_event` forced return and failure-before-commitment non-observation; every interleaving lands on a defined, witnessed outcome.
- **Lost-Error hunt on non-Fatal runs** — per-turn checkpoint plus close-as-final-observation leaves no publication unobserved on any `Stopped` exit; could not construct an escaping Error.
- **Sim `Ended`-Port reachability** — tried to route `dispatch`/selection into an `Ended` lifecycle; the pending-latch-first check order provably intercepts every path, as the derive claims.
- **`SIM-SELECT` cursor determinism** — cursor fully specified including `step(None)`; the one unstated case (cursor after `step(Err)`) is unobservable because `ENV-SERIAL` permits only `shutdown` afterward.
- **`DET-RUN`/`DET-ENV` divergence construction** — failed: the trace's erasure rule plus the exact Core-owned comparanda list closes every divergence channel I tried, including Environment-only failure shapes (explicitly carved out).
- **Journal byte edges (executed)** — object = max, max+1, max+2 each land on the specified variant; `NonZeroUsize::MAX.checked_add(1) == None` confirmed.
- **Record wire format (executed)** — the ZST kind-field mechanism reproduces the doc's `RunStarted` example byte-for-byte: `{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}`; `outcome` serializes as bare `"Continue"`/`"Stop"`.
- **`ports!` expansion (executed)** — `#[derive(::serde::Serialize)]` with associated-type payloads, duplicate payload types across Slots, and `Never` discharge by `match never {}` all compile; externally tagged JSON as claimed.
- **`NotAnObject` unreachability for named-field structs (executed)** — serde_json escapes newlines in both values and keys (`"line1\nline2"`, `{"a\nb":1}`); newtype is transparent, tuple → array, unit → `null` — exactly the derive note's classification; non-stringable map key errors ("key must be a string").
- **`Timestamp::checked_add` premise (executed)** — a `Duration` really can exceed u64 nanoseconds, so both overflow cases are live.
- **First-commit-failure path** — `RunStarted` commit fails: real activation effects, empty Journal, cause in the exit; consistent with A5 (no record announces activation).
- **`StopPending` quiescence retention** — survives `TurnCompleted(Stop)` commit failure; `RUN-FINALIZE`'s three quiescence fixings exhaustively cover every Fatal (start-Err / unconsumed / consumed).
- **`Prepared` fusion vs. the two-edge binding** — the fused `dispatch_batch` preserves the record sequence and all three failure outcomes the binding tables fix.
- **Recordless edges "cannot fail"** — the checkpoint is phase work, not edge work; the claim survives.
- **Index domain edges** — turn at `u64::MAX` runs and may Stop; `IndexExhausted` fires before `next_event`, overflow past the check unreachable.
- **Live shutdown deadline race** — missed wakeups cannot produce a false `Incomplete` (the set is authority; the final synchronized observation decides); premature-vs-expected classification is linearized with the close.
- **Deadlock hunt** — `offer` and `dispatch` never wait, shutdown wakes every Kavod-owned blocking point, premature closure wakes a blocked `next_event`; every Kavod-owned wait has a waker or a deadline.

## Coverage

- 0 Reading rules — walked. 1 Glossary — walked (every term against every use). 2 Laws — walked. 3 Application — walked. 4 Port — walked (expansion executed). 5 Environment — walked. 6 Journal — walked (arithmetic executed). 7 The Run — walked (every phase × every input; records executed). 8 Live — walked. 9 Simulated — walked. 10 Wiring — skimmed (open by declaration; checked only that closed sections don't break without it). 11 Crate layout — walked. 12 Obligations & verification — walked. Appendix A — walked (all citations resolved mechanically; no dangling IDs).

## Questions the document cannot answer

- What `try_recv` returns on repeated calls after the signal has been drained, and what its `None` means (FBL-02).
- Which of the three enforcement tiers owns `DET-RUN` (FBL-01).
- What concrete tooling hosts a compile-fail suite inside the module-privacy boundary (FBL-03).
