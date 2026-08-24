# Adversarial Review: ox-alpha (opencode), 2026-08-23
**Target:** design_docs/design-v12.md — `> **Status:** Authoritative (v12). One section is open: Wiring & construction. v11 is frozen as the artifact the adversarial reviews cite (`design_docs/reveiws/`).`
**Verdict:** Sound with fixes. No behavioral contradiction, unreachable guarantee, or conforming-implementation divergence survived attack; the Run grammar, ENV-LATCH ordering machinery, both shipped Environments, and the Journal contract held under sustained assault. The defects found are bookkeeping and edge-case gaps: §0's universal enforcement-tier claim has no locatable tier for a substantial set of behavioral rows, two Glossary definitions disagree about "External Event," and a few configurations (deadline overflow) have no representable outcome.

## Findings

### OA-01 §0 promises every non-Obligation ID an enforcement tier; many rows have none locatable — MINOR, confidence high, omission
- **Text attacked:** §0: "Every ID outside the Obligations table is **enforced**: violation is unrepresentable, panics an always-on assertion, or is pinned by a named test suite." Rows with no located tier: `APP-EMIT` ("while capacity remains it appends in call order"), `APP-OVERFLOW`, `APP-STATE`, `BOUND-LOOPS`, `NO-UNWIND` ("Shipped code relies on unwinding nowhere"), the stamp/dequeue and "nothing fallible follows it" clauses of `LIVE-SELECT`, `LIVE-DISPATCH`'s admission-identity clause beyond `VERIFY-LATCH`'s latch scope, `LIVE-EVENTS` ("`Full` or `Closed` returns the Event"), `SIM-WAKEUP` ("last-call-wins"), `SIM-STEPS`, `SIM-COMPLETION`, the round-robin cursor clauses of `SIM-SELECT`, and the Edges-table runtime checks (`Core(TimeRegression)`, `Core(IndexExhausted)` detection).
- **Claim:** None of these is unrepresentable; the only always-on asserts the document names are `RUN-ENFORCEMENT`'s three points, the batch-emptiness asserts, and `SIM-SELECT`'s Open-check; and no `VERIFY-*` row pins any of them (`VERIFY-SIM` covers `SIM-LIFECYCLE/START/SHUTDOWN` only; `VERIFY-LIVE` enumerates lifecycle/shutdown cases only; `VERIFY-FAULTS` covers operation `Err`s, not success-path Core checks; `VERIFY-CONFORMANCE` is differential and excludes single-type failure shapes). The tier mapping is missing work in an exhaustive-scope promise.
- **Witness:** Ask of the text: "which named suite or assertion fails if an implementation stamps *after* dequeue?" No row answers; every `VERIFY-*` scope excludes it; nothing in the type system forbids it. Same for `set_next` last-call-wins and `emit` call-order append.
- **Fix sketch:** Either assign tiers per row (add suites to §12, e.g., a contract-behavior suite) or scope §0's quantifier to rows that declare a tier.

### OA-02 Glossary contradicts itself on when an Event becomes an "External Event" — MINOR, confidence medium, contradiction
- **Text attacked:** Glossary: "**External Event** — an Event delivered by `next_event`, as opposed to the start turn; External Events carry indices from 1." vs. "**Accepted** — … `EventAccepted` for a candidate becoming one External Event." (and "**Candidate** — an Event returned by `next_event`: consumed, not yet accepted").
- **Claim:** Definition 1 makes delivery sufficient for External-Eventhood; the Accepted definition (and §7's derive "a candidate lost to `TimeRegression` … never had an index") makes acceptance necessary. Both are normative Glossary lines.
- **Witness:** `next_event` returns `Ok((e, t))` with `t` below the last accepted time → `Core(TimeRegression)`. Under def 1, `e` is an External Event (delivered by `next_event`) carrying an index from 1; under the Accepted row, `e` never became one and had no index. No rule's outcome changes, but the term is used both ways.
- **Fix sketch:** Reword def 1 to "an accepted Event delivered by `next_event`…" or drop the status claim.

### OA-03 Shutdown-deadline arithmetic has no representable failure outcome — MINOR, confidence medium, omission (reasoned; execution unavailable)
- **Text attacked:** `LIVE-SHUTDOWN`: "fixes one absolute shutdown deadline, the configured duration after that close"; `LIVE-TIME`: "duration conversion is checked (A6)"; A6: "Arithmetic on … times … is checked."; `fn shutdown(self) -> ShutdownReport<Self::Error>` with fields `quiescence: Quiescence` and `error: Option<E>` (doc: "The pending Error the latch held when it closed").
- **Claim:** A6 mandates checked time arithmetic, but a checked failure of `close_time + configured_duration` has nowhere to go: `shutdown` returns no `Result`; `report.error` is defined as the latch's pending Error alone (routing a config error there falsifies the `None` proof); `Quiescence` has only two variants; and A8 brands a plain panic a bug, which a legal configuration is not.
- **Witness:** `LiveConfig` shutdown deadline = `u64::MAX` milliseconds (legal: nonzero, `BOUND-NONZERO` satisfied); close at monotonic time `T` with `T + duration > Instant` range → the checked addition fails and the document defines no outcome.
- **Fix sketch:** Validate the deadline at construction (`start`/builder) and reject oversized durations as a typed build/config error.

### OA-04 "Handoff … into its destination Port's ownership" vs. Environment-owned inbox admission — MINOR, confidence medium, ambiguity
- **Text attacked:** Glossary: "**Handoff** — `dispatch`'s commitment: transfer of one Command into its destination Port's ownership." vs. `LIVE-DISPATCH`: "Each destination Port owns one bounded Command inbox; one non-waiting admission to it is where `dispatch`'s handoff commits", with the bounds registry assigning "per-Port Command inboxes" to the **Live Environment**.
- **Claim:** At the live commitment instant the Command sits in an Environment-owned container the Port has not observed; strict reading makes that instant not a Handoff as defined (which would break the live async model), while the intended reading — value-ownership transferred, residency in a Port-private inbox — leaves "ownership" informal.
- **Witness:** `dispatch(c)` returns `Ok` (committed); `c` lies in the Environment-owned inbox; Port dequeues only after `shutdown`. Owned-by-Port? Of the value, yes; of the location, no — both readings conform to every written sentence.
- **Fix sketch:** Define Handoff as transfer into the destination Port's exclusive inlet, owned by the Port's behalf-owner per the bounds registry.

### OA-05 Crate layout assigns no home to published API items — NIT, confidence high, omission
- **Text attacked:** §11 `engine.rs` — "Engine, EngineConfig, EngineExit, FatalCause, CoreError"; `record.rs` — "(private; RecordKind and JournalFatal re-exported)"; `environment.rs` — "Environment, Quiescence" — vs. §7's public `EnvironmentFatal`, `EnvironmentOperation` (lines 631–649) and §5's public `ShutdownReport`.
- **Claim:** Three public items owned by API blocks appear in no module of the layout; "every public item is reachable at a path without repeated segments" cannot be checked for them.
- **Witness:** Search §11 for `EnvironmentOperation`: absent; search §7: declared `pub`.
- **Fix sketch:** List them (engine.rs / environment.rs respectively).

### OA-06 Status line cites `design_docs/reveiws/` — NIT, confidence high, false claim (cosmetic)
- **Text attacked:** "**Status:** … v11 is frozen as the artifact the adversarial reviews cite (`design_docs/reveiws/`)."
- **Claim:** "reveiws" is a misspelling of "reviews"; the pointer likely dangles (existence unverifiable under the sole-input rule — itself noted below).
- **Witness:** The quoted string itself.
- **Fix sketch:** Correct the directory name.

### OA-07 Forward reference to `Engine::new` in §3 — NIT, confidence high, self-conformance violation (of the document's own stated discipline)
- **Text attacked:** §3 *Justify*: "anything fallible needed to build it happens before `Engine::new`" vs. placement rule "Citations point backward… Navigation pointers are exempt: [six listed classes]."
- **Claim:** `Engine::new` is defined in §7; this mention predates it and matches no exemption class. Mitigated: the placement rules are introduced "for every future edit," so current-text compliance is not claimed.
- **Witness:** §3 (line 315) precedes §7 (line 598).
- **Fix sketch:** Say "before engine construction" or add the pointer class.

### OA-08 A2's "runs outside the turn" reads universally; the sim runs handed-off processing synchronously inside the turn — NIT, confidence medium, ambiguity (vacuous-satisfaction reading clearly intended)
- **Text attacked:** A2: "A destination Port's processing of Commands already handed off runs outside the turn." vs. `SIM-DISPATCH`: "dispatch synchronously routes to exactly one Port's `on_command`; the invocation is where `dispatch`'s handoff commits", under §5: "an implementation satisfying every row here … is a conforming Environment."
- **Claim:** Read as a universal placement rule, A2 condemns the sim; read as the carve-out permitting live async residue (with sim having zero residue, since processing coincides with commitment), it holds. The second reading is forced by the sim's conformance claims, leaving only wording looseness in an axiom.
- **Witness:** Trace: turn in `Prepared`, `dispatch(c)` → `on_command` executes to completion before `dispatch` returns — Port processing of a handed-off Command occurs inside the turn under the universal reading.
- **Fix sketch:** Recast as "need not occur within the turn."

### OA-09 Graph sketch's uniform failure entry assumes a certificate that startup-step-2 failures lack — NIT, confidence high, omission (in a declared-non-normative artifact)
- **Text attacked:** Graph sketch: "any failure: drop the certificate ──▶ RUN-FINALIZE" vs. startup step 2: "`Environment(Start)` Fatal with `Quiescence::Quiesced` — `ENV-START` already holds, so finalization skips `shutdown`" (no certificate exists before step 3 mints one; likewise `BuildError` paths).
- **Claim:** The binding tables handle the case (`RUN-FINALIZE`'s "`start` returned `Err`" branch); only the sketch overgeneralizes.
- **Witness:** `start` returns `Err`: nothing to drop, yet the sketch's only failure arrow routes through dropping.
- **Fix sketch:** Annotate the sketch arrow "from first minting onward."

## Attacked and held
- Clean-Stop licensing under `ENV-SERIAL`: the permissive "at most once" reading is forced by the Stop path itself (attack rejected).
- `JRN-ENCODE` classifier exactness: multi-object payloads (`{"a":1}{"b":2}`) are unrealizable through `serde_json::to_writer`; achievable non-objects all trip the brace/newline test (reasoned).
- Compile-time grammar: certificate affinity, fused `dispatch_batch`, classify refinements, `Clone/Copy/Default` prohibition vs. the three admitted runtime points — reconciled explicitly and consistently.
- `ENV-LATCH` trichotomy at all four observation points, including pre-commitment-failure exclusion, single pending→reported transition, and `Ok`-return-forces-after-placement in `LIVE-SELECT` overlaps.
- `RUN-FINALIZE` exhaustiveness: {start-Err, unconsumed, StopPending-consumed} partitions reachable fatal states; retained-quiescence covers `Some`, `Incomplete`, and `TurnCompleted(Stop)` commit failure.
- Evidence column of the Records table; `CommandsPrepared`+`Dispatch{k}` prefix identification incl. unrelated-latched-Error case; `Stopped` ⇒ clean report; `report.error: None` proof; consumed-but-unaccepted candidate accounting in Trace.
- Arithmetic: `max_record_bytes+1` sizing, `usize::MAX` → `MaxBytesTooLarge`, object-exactly-max newline handling, zero-progress→`BoundExceeded`, `u64::MAX` index check before `next_event` with panic backstop, `checked_add` dual clause.
- Sim closure: selection never meets an `Ended` arm (latch-first ordering is airtight single-threaded); structural `Quiesced`/`None` on Stop path; startup prefix-stop; budget/arm/now subordinate-effect accounting.
- Live races: latch-lock linearization of close vs. supervision classification; final synchronized expiry observation; joins exceeding deadline blessed by `BOUND-LOOPS`; post-`Complete` join hang honestly disclosed.
- Determinism: repeated attempts to build two conforming implementations diverging on equal traces failed — publication-placement freedom changes the trace, not the trace→output map (A9 holds).
- Realizability (reasoned; plan mode barred execution): affine transitions, ZST tag fields, `PhantomData<fn() -> P>`, consuming `shutdown`, transparent-u64 doc bindings, `ports!` macro shape, wake-token scheme.

## Coverage
- §0 Reading rules: walked. §1 Glossary: walked. §2 Laws/registry/ownership: walked. §3 Application: walked. §4 Port: walked. §5 Environment contract + commitment table: walked. §6 Journal: walked. §7 Run (construction/startup/phases/edges/records/guarantees/enforcement): walked. §8 Live: walked. §9 Simulated: walked. §10 Wiring: skimmed (open by declaration). §11 Crate layout: walked. §12 Obligations & verification: walked. Appendix A: walked.

## Questions the document cannot answer
- Which concrete suite, assertion, or type-level mechanism enforces each behavioral row listed in OA-01?
- What is the defined outcome when the close time plus the configured shutdown deadline overflows the time domain?
- Does "into its destination Port's ownership" mean ownership of the Command value or of its storage location?
- Reviews are frozen to v11 while v12 is authoritative — which artifact governs a review finding that conflicts with v12 text?
