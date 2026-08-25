# Close-out pass: design-v12, 2026-08-25

The verification pass defined at the end of the batches in `synthesis-v12.md`. All six
batches were merged (b97692c..5968156) before this pass ran. Three jobs: mechanical
reconciliation, a targeted adversarial re-review of the Batch 1 sections, and a version
recommendation. No design edits were made beyond the two trivial mechanical fixes listed
in §1.

---

## 1. Mechanical reconciliation

### Clean

- **Appendix A ↔ body.** 89 IDs (A1–A9 plus 80 rules). Bijective: every ID defined in
  the body appears in Appendix A exactly once; no appendix entry lacks a body
  definition; every section attribution is correct.
- **Citations.** Every backticked ID citation in the body resolves to a defined row or
  axiom. No section-number citation (§N) anywhere in the document.
- **Batch landings spot-verified.** SYN-25 (`pub struct Engine`), SYN-24/D4
  (`JournalFatal.outcome`), D6 (§0's fourth prose job, Mechanism), the full D3
  disposition table (`ASSERT-INVARIANTS`/`BOUND-LOOPS` in §0, `VERIFY-CONTEXT` added,
  `TRUST-ABORT` extended, `TRUST-ROUTING` payload clause, the `include!` fixture in
  `VERIFY-GRAMMAR`, `TRUST-BLOCKING` cited at `RUN-FINALIZE`/`SIM-START`/`SIM-SHUTDOWN`
  plus the §9 derive), and SYN-26, -38, -42..-53 all landed. No residue of `NO-UNWIND`,
  "fused batch transition", "(Ports Notes)", or "witnessed complete".
- **`ENV-ERRORS` discharge.** Both shipped implementations name their `start`
  activation and `next_event` consumption instants in guarantee rows
  (`LIVE-START`/`LIVE-SELECT`; `SIM-START`/`SIM-SELECT`).
- **D1/D2 intent-to-text.** `batch_0.md`'s complete Stop-path outcome matrix (clean →
  `Stopped`; `Some` → `Environment(Shutdown)`; `{Incomplete, None}` →
  `Core(ShutdownIncomplete)`; Error-plus-expiry → the Error wins; post-close discard)
  and D2's precedence clause (both overlap orderings plus the blocked-wake case) are
  present in `ENV-SHUTDOWN`/`ENV-LATCH`/`StopPending` and the `VERIFY-*` rows. D1's
  five required verification cases are all present.
- **§12 completeness sentences hold.** Every trust-flavored statement in the body cites
  a `TRUST-*` ID or points into the Obligations table with an unambiguous referent; no
  obligation lives outside the table. (The enforcement half of §0's claim is re-audited
  for the new Batch 1 text in §2 below — the re-review found tier gaps there: CO-03,
  CO-04, CO-06, CO-09.)
- **Repo-wide grep.** SYN-54's rename was declined, so `reveiws/` stands; nothing in
  the repo points at a `design_docs/reviews/` path (the two textual mentions are review
  testimony, not pointers). `src/` contains no design references. No stale ID anywhere.

### Flagged (reported, not edited)

1. **The Status line is stale on both halves** (design-v12.md line 4): "v11 is frozen
   as the artifact the adversarial reviews cite (`design_docs/reveiws/`)".
   (a) `design-v11.md` was moved to `design_docs/old/` at commit 04b2314 and deleted
   from the tree at 2ac4fcd — v11 is "frozen" only in git history. (b) The reviews now
   in `design_docs/reveiws/` are this round's eleven, which cite v12, not v11.
   Pre-existing (arose when the v12 reviews replaced the v11 set), surfaced by this
   reconciliation. Not edited: the Status block belongs to the version decision (§3).
2. **§0 forward citation outside its own exemption list** (line 27): "Fatal
   finalization is `RUN-FINALIZE`'s alone" — §0's exemption list does not exempt §0's
   own citations. NIT. The natural fix (exempting the reading rules' citations, which
   are inherently forward from position zero) is a rule edit, not mechanical — left
   open.
3. **`design_docs/test.md` is stale against `VERIFY-GRAMMAR`** (test.md line 7):
   "Compile-fail proofs … use `trybuild` under `tests/`" — the amended `VERIFY-GRAMMAR`
   (SYN-17) requires an `include!`-based fixture crate attacking from the Engine's
   visibility position, precisely because bare compile-fail hosting from outside the
   module proves only privacy. Content fix, not mechanical — left open.

### Mechanical fixes made (2)

- **Dangling phrase-citation** — `LIVE-COMPLETION` and the §8 shutdown *Justify:* both
  cite "`ENV-SHUTDOWN`'s bounded quiescence policy", but the Batch 1 rewrite of
  `ENV-SHUTDOWN` left only the antecedent-less "The bounded policy". Fixed in
  `ENV-SHUTDOWN`: "The bounded policy" → "This bounded quiescence policy" (names the
  policy the citing rows already use; semantics unchanged). The fuller wording CO-12
  sketches remains open.
- **Missing paragraph break** — §8 Mechanism: "*Justify:* one workable realization…"
  was fused onto the `take_error` paragraph with no blank line, so the marker did not
  start a paragraph as everywhere else. Blank line inserted.

---

## 2. Adversarial re-review: Batch 1 sections

**Target:** design_docs/design-v12.md — Status line verbatim: "**Status:**
Authoritative (v12). One section is open: Wiring & construction."
**Scope:** `ENV-SHUTDOWN`, `ENV-LATCH`, the `shutdown` doc comment and commitment row,
`LIVE-SHUTDOWN`, `LIVE-SUPERVISION`, `SIM-SHUTDOWN`, the amended `VERIFY-LIVE` /
`VERIFY-SIM` / `VERIFY-LATCH` rows, and their interactions with `StopPending` and
`RUN-FINALIZE`. The rest of the document was context, not a target; the synthesis's
rejected-findings table and held-under-fire list stand for unchanged text.
**Process:** four scoped sub-reviews (contract, Live, Sim, Run/verification), every
finding re-verified against the text by the synthesizing reviewer before inclusion;
duplicates merged; one sub-review witness rejected on verification (noted under
Attacked and held).

**Verdict:** Sound with fixes. The D1/D2 redesign is coherent end-to-end where it was
walked: the Stop-path outcome matrix, `RUN-FINALIZE`'s three arms re-derived under the
new report semantics, the latch's four-state machine, and the sim's structural
`Quiesced` all held. The one MAJOR is a contract-level gap: `ENV-LATCH` anchors its
publication-ordering rules to the observing *call*, and for the close that call is the
entire `shutdown` window — so §5 alone, the complete contract for the `TRUST-ENV`
audience, lawfully permits discarding every graceful-window publication, resurrecting
the behavior D1 exists to remove. Everything else is enforcement-tier accounting
(SYN-11-shaped residue concentrated in the amended `VERIFY-LIVE` row) and wording.

### Findings

#### CO-01 The close's ordering anchors are the whole `shutdown` call, licensing discard of every window publication — MAJOR, confidence high, ambiguity
- **Text attacked:** `ENV-LATCH`: "The Environment chooses a logical order between each
  publication and `next_event` or `dispatch`'s commitment, `take_error`'s snapshot, or
  the close. For a call that reaches one of those observation points, a publication
  completed before the call began orders before the point, one begun after the call
  returned orders after the point, and one overlapping the call may order on either
  side." `ENV-SHUTDOWN`: "A publication ordered before that close follows the latch's
  ordinary first-wins rules; one ordered after it is discarded."
- **Claim:** For the close, "the call" is `shutdown`, whose span is the whole
  graceful-shutdown window; every window publication therefore "overlaps the call" and
  may conformingly be ordered after the close and discarded. "The latch remains open"
  does not block this — openness is a state, and placement is exactly what the
  chosen-order machinery decides.
- **Witness:** Application answers `Stop`; `StopRequested` commits; `StopPending` calls
  `shutdown`. Port L observes the signal, its flush fails, it publishes typed Error `E`
  two seconds into a ten-second window, then completes; all units complete well before
  the deadline. Bespoke Environment X orders `E` before the close: report
  `{Quiesced, Some(E)}` → `Environment(Shutdown)` Fatal, Journal ends at
  `StopRequested`. Bespoke Environment Y notes `E` overlapped the `shutdown` call and
  orders it after the close: report `{Quiesced, None}` → `TurnCompleted(Stop)` commits
  → `Stopped`; `E` appears in no record and no exit. Both satisfy every §5 row; the
  traces differ, so `DET-ENV` never engages. Both shipped realizations anchor at the
  final observation (`LIVE-SHUTDOWN`'s "one final synchronized observation …
  publication ordered before that observation … captured"; `SIM-SHUTDOWN`'s
  "published before the final close"), so only the `TRUST-ENV` audience is exposed —
  the same exposure pattern as SYN-01. Note the certification interplay:
  `VERIFY-LATCH`'s bullets ("a typed shutdown Error before the final close", "either
  consistent placement for a publication racing that close") presuppose the
  observation-anchored model, so under Y's reading the certification suite rejects an
  implementation §5's rows admit as conforming — the contract and its certification
  pull apart.
- **Fix sketch:** One clause in `ENV-LATCH` (or `ENV-SHUTDOWN`): for the close, the
  ordering anchors are the final observation, not the `shutdown` call — a publication
  completed before the final observation begins orders before the close; only one
  overlapping that observation may order on either side.

#### CO-02 The precedence clause's ordering anchors are not restated for the failure instant — MINOR, confidence medium, ambiguity
- **Text attacked:** `ENV-LATCH`: "For a call that reaches one of those observation
  points, a publication completed before the call began orders before the point …";
  "A call that would otherwise fail before commitment resolves any pending publication
  against the instant its own failure would fix. … If the operation's own Error fixes
  first, it is returned and a publication ordered after it stays pending."
- **Claim:** The completed-before-the-call-began constraint is conditioned on "a call
  that reaches one of those observation points", which a pre-commitment-failing call by
  definition does not; nothing restates the anchors for the failure instant. A
  strained-but-literal reading lets an implementation declare its own Error to "fix
  first" even against a publication completed before the call began — reopening the
  corner D2 was written to close. Same root as CO-01: the anchor trichotomy is
  quantified over too narrow a set of resolution points; one fix restating the anchors
  per resolution point closes both.
- **Witness:** Port A publishes `E`; the publication completes. Then `dispatch(c0)`
  finds B's inbox full. Intended reading: `Err(E)`, latch reported, exit carries `E`.
  Strained reading: the Environment orders `E` after the failure instant, returns
  `Err(InboxFull)`, `E` stays pending, and `RUN-FINALIZE` discards the finalizing
  report's `Some(E)` — `E` vanishes.
- **Fix sketch:** "A publication completed before the call began orders before that
  instant; one overlapping the call may order on either side of it."

#### CO-03 The amended `VERIFY-LIVE` pins neither `LIVE-SHUTDOWN`'s wake clause nor the `recv`/`try_recv` signal semantics — MINOR, confidence high, unenforceable claim
- **Text attacked:** `LIVE-SHUTDOWN`: "wakes every Kavod-owned blocking point."
  `LIVE-LIFECYCLE`: "Once raised, every `recv` reports it ahead of that Port's queued
  Commands…" `try_recv` doc comment (binding under §0 form 1): "Once the signal is
  raised and the inbox is drained, every call returns `Some(PortInput::Shutdown)`;
  `None` means no Command is pending and the signal has not been raised."
  `VERIFY-LIVE`: "It also verifies `LIVE-EVENTS`, `LIVE-SELECT`, `LIVE-DISPATCH`, and
  shipped `ENV-BOUNDS`."
- **Claim:** No `VERIFY-LIVE` bullet exercises `recv`/`try_recv` at all — not the
  signal-ahead-of-Commands ordering, the drain-then-`Some(Shutdown)` repetition, the
  `None` meaning, or the recv-side wake — and no assertion or type carries them;
  `TRUST-LIFECYCLE`/`TRUST-DRAIN` cover the Port author's side, not Kavod's delivery.
  §0's universal enforcement sentence is undischarged for these clauses (the SYN-11
  shape); found independently by three of the four sub-reviews.
- **Witness:** An implementation that sets the lifecycle cell but never wakes a Port
  blocked in `recv` on an empty inbox: every enumerated `VERIFY-LIVE` bullet can pass
  (no case blocks a Port in `recv` across the signal), yet a
  `TRUST-LIFECYCLE`-conforming Port sleeps through the window, its entry stays
  `Outstanding`, and shutdown returns `{Incomplete, None}` → `Core(ShutdownIncomplete)`
  where the waking implementation returns `Stopped`. Divergent `EngineExit`, all named
  checks green.
- **Fix sketch:** Add `LIVE-LIFECYCLE` to `VERIFY-LIVE`'s "also verifies" list with
  bullets for signal-ahead-of-Commands in `recv`, drain-then-`Some(Shutdown)`, `None`'s
  meaning, and a Port blocked in `recv` observing `Shutdown` within the window.

#### CO-04 `LIVE-SUPERVISION`'s pre-signal premature-completion publication has no suite case — MINOR, confidence high, omission
- **Text attacked:** `LIVE-SUPERVISION`: "Before the shutdown signal, `run(Err)` and
  `run(Ok)` completing prematurely each publish a typed Error to the latch and wake a
  blocked `next_event`." `VERIFY-LIVE`'s enumeration.
- **Claim:** No `VERIFY-LIVE` bullet exercises a premature completion *before* the
  signal publishing and waking: "a completion before shutdown remains visible at the
  final observation" pins completion state, not publication; "run(Ok) after the signal
  is expected and unpublished, while a typed `run(Err)` before the final close enters
  the report" pins the after side and the Err side. An implementation that silently
  swallows a premature `run(Ok)` (no publication, no wake) passes every enumerated
  case while a run with one silent dead Port blocks in `next_event` forever.
- **Witness:** One bound Port whose `run` returns `Ok` at t=1s with no shutdown in
  sight; no Event is ever offered again. Required: a typed premature-closure Error is
  published and wakes `next_event` → `Fatal(Environment(NextEvent))`. A non-publishing
  implementation hangs instead; no enumerated bullet distinguishes them.
- **Fix sketch:** One bullet: a premature completion before the signal publishes a
  typed Error that wakes a blocked `next_event`.

#### CO-05 A premature test-profile unwind's publication status is unspecified — MINOR, confidence medium, omission
- **Text attacked:** `LIVE-SUPERVISION`: "Before the shutdown signal, `run(Err)` and
  `run(Ok)` completing prematurely each publish a typed Error…" `LIVE-COMPLETION`:
  "any non-aborting terminal exit: gate cancellation, return from `LivePort::run` with
  either result, or unwind under the test profile."
- **Claim:** `LIVE-COMPLETION` names three terminal-exit classes; `LIVE-SUPERVISION`'s
  publication rules cover only the two `run`-return classes, so whether a pre-signal
  unwind publishes a premature-closure Error is unconstrained — two conforming
  implementations diverge observably. Confined to the unwinding test profile, where the
  triggering panic is already a bug (A8), hence MINOR.
- **Witness:** Test profile, one bound Port, `run` panics before any signal; the guard
  marks `Complete`. Implementation A publishes → the blocked `next_event` wakes →
  `Fatal(Environment(NextEvent))`. Implementation B publishes nothing → `next_event`
  blocks forever. Both satisfy every quoted row.
- **Fix sketch:** One clause in `LIVE-SUPERVISION` classifying test-profile unwind,
  mirrored by one `VERIFY-LIVE` bullet.

#### CO-06 The exit's `quiescence` value is checked by no named suite — MINOR, confidence high, omission
- **Text attacked:** `RUN-FINALIZE`'s three quiescence arms; `StopPending`: "failure to
  commit that record finalizes with the retained `Quiesced`"; `VERIFY-FAULTS`:
  "checking the resulting `FatalCause`"; `VERIFY-LATCH`'s Stop-path integration
  sentences (causes only).
- **Claim:** Every suite bullet that reaches a Fatal exit names only the `FatalCause`;
  `VERIFY-CONFORMANCE` compares `Quiescence` for *equality* across runs and types, not
  correctness against the scripted report. The retention machinery Batch 1 added has no
  locatable enforcement tier (the SYN-11 shape).
- **Witness:** Stop at index 7, report `{Quiesced, None}`, flush of
  `TurnCompleted(Stop)` returns `Err(BrokenPipe)`. Required exit:
  `Fatal { Journal(TurnCompleted, Some(Stop), Sink{Flush}), Quiesced }`. An Engine
  hardcoding `Incomplete` on every Journal path passes `VERIFY-FAULTS` (cause correct),
  `VERIFY-LATCH` (causes only), and `VERIFY-CONFORMANCE` (the wrong value is
  deterministic and equal cross-type).
- **Fix sketch:** Extend `VERIFY-FAULTS` (or `VERIFY-LATCH`'s Stop-path sentence) to
  check the exit's `quiescence` alongside the cause, including
  retained-`Quiesced`-after-commit-failure.

#### CO-07 Three `VERIFY` report clauses omit the first-wins qualifier — MINOR, confidence high, false claim
- **Text attacked:** `VERIFY-SIM`: "any `stop` Error returns
  `{ Quiesced, Some(error) }`." `VERIFY-LIVE`: "a typed `run(Err)` before the final
  close enters the report"; "Error plus expiry returns `{ Incomplete, Some(error) }`."
- **Claim:** Read as universals these contradict `ENV-LATCH`'s first-wins rule inside
  scenario space the rows themselves enumerate: an earlier publication can already be
  the latch's first (pending or reported), and the later `stop`/`run` Error is then
  discarded. A suite implementing the literal bullets would reject a conforming
  Environment.
- **Witness (sim, fully in-suite):** Ports A, B `Open`. A turn's final dispatch →
  `A.on_command` returns `Err(E1)` → published; checkpoint `take_error` → `Some(E1)`
  (latch reported) → `Environment(Checkpoint)` Fatal → `RUN-FINALIZE` calls `shutdown`.
  `stop(B)` returns `Err(E2)` → publication after the first → discarded. Report:
  `{ Quiesced, None }` — a `stop` Error occurred and the report carries nothing, inside
  the row's own scenario list ("`Err` from `on_command` … followed by shutdown; and
  `stop` returning `Ok` or `Err` at every Slot position"). Live analog: `E1` pending
  pre-window; a shell's `run(Err)` publishes `E2` during the window → the report
  carries `E1`, not the Error the clause says "enters the report".
- **Fix sketch:** Qualify each: "…where it is the first publication" (or scope the
  bullets to single-Error scenarios explicitly).

#### CO-08 `SIM-SHUTDOWN`'s stated order inverts `ENV-SHUTDOWN`'s "first raises the shutdown signal" — MINOR, confidence high, ambiguity
- **Text attacked:** `ENV-SHUTDOWN`: "`shutdown` first raises the shutdown signal,
  stops Event delivery, and closes Event admission." Doc comment: "Raises the shutdown
  signal first, stops Event delivery, and closes Event admission." `SIM-SHUTDOWN`: "it
  stops Event delivery and closes Event admission, then delivers the sim shutdown
  signal by invoking `stop`".
- **Claim:** Under the serial reading — signal before the other two acts, which the doc
  comment's "first" invites — the shipped Sim realization contradicts the contract row
  it claims to realize. The saving reading (the initiating acts form one block ordered
  before the window and close, internal order free — what `LIVE-SHUTDOWN`'s "one
  linearized initiating instant" embodies) is clearly intended but stated nowhere.
  Unobservable in the sim (single-threaded; no code of any Port can run between the
  acts), so MINOR. Found independently by two sub-reviews.
- **Witness:** The three quotes side by side: contract and doc comment put the signal
  first in the list; the realization puts it last.
- **Fix sketch:** State the block reading in `ENV-SHUTDOWN` ("begins by raising the
  signal, stopping Event delivery, and closing Event admission; their internal order is
  the implementation's Port-facing API"), or reorder `SIM-SHUTDOWN`'s sentence.

#### CO-09 "initiates no further externally consequential work" has no tier for the shipped implementations — MINOR, confidence medium, unenforceable claim
- **Text attacked:** `ENV-SHUTDOWN`: "the Environment itself initiates no further
  externally consequential work after raising the signal." §0: "Every ID outside the
  Obligations table is **enforced** … Kavod enforces them in the implementations it
  ships."
- **Claim:** For a bespoke Environment the clause rides `TRUST-ENV`'s review cell; for
  the shipped Environments no tier is locatable — not unrepresentable, no assertion
  site, no `VERIFY-LIVE`/`VERIFY-SIM` bullet, and no execution trace can witness the
  absence of external work, which by the document's own D3 logic puts it outside the
  enforced tier. No Obligations row covers Kavod's own shipped conduct here (compare
  `TRUST-ABORT`, which does exactly that for the panic profile).
- **Witness:** Audit of every `VERIFY-LIVE`/`VERIFY-SIM` bullet: none observes or could
  observe whether shutdown-side code initiated an external effect; a shipped shutdown
  that flushed a metrics socket mid-window would pass every named suite.
- **Fix sketch:** Extend an Obligations row (Kavod implementer, review-verified) to
  cover shipped no-external-work conduct, or scope the clause explicitly as
  review-verified.

#### CO-10 "every Port has a means to observe it immediately" is unsatisfiable for already-ended Ports — MINOR, confidence medium, ambiguity
- **Text attacked:** `ENV-SHUTDOWN`: "From the signal's initiating instant every Port
  has a means to observe it immediately". `SIM-LIFECYCLE`: "no method may be invoked in
  `Ended`."
- **Claim:** A sim Port `Ended` mid-run (or a Live Port whose `run` already returned)
  will never execute again and has no means to observe the signal — the universal is
  structurally unsatisfiable as written. The intended scope (Ports the signal can still
  concern; the ended residue discharged via `TRUST-SPAWN`) is clear but stated nowhere.
- **Witness:** Port B `Ended` via `on_command(Err)` at turn 3; checkpoint Fatal;
  finalizing `shutdown` invokes `stop` on the `Open` set only. B never observes the
  signal; under the quoted universal the sim is nonconforming, under the intended scope
  it conforms.
- **Fix sketch:** Scope the clause to Ports not already ended/returned, citing
  `TRUST-SPAWN` for the discharged residue.

#### CO-11 `RUN-CHECKPOINT`'s Stop-path sentence points the wrong way on the `StopRequested`-commit-failure branch — MINOR, confidence medium, ambiguity
- **Text attacked:** `RUN-CHECKPOINT`: "on the Stop path the next and final latch
  observation is shutdown's close, and the `StopPending` row is decisive on its
  report." `RUN-FINALIZE`: "…call `shutdown` (`TRUST-BLOCKING`), take the report's
  quiescence, and discard the report's Error (A4: a cause exists)."
- **Claim:** When the `StopRequested` commit fails, the run is on the Stop path (the
  answer is fixed `Stop`) and the final latch observation is still a shutdown close —
  but the *finalizing* one, whose report `RUN-FINALIZE` rules by discarding its Error;
  `StopPending` never ran. One reading is clearly intended (the clause presupposes
  `StopPending` is reached; the graph forces the diversion), so no implementer
  diverges — SYN-27's shape.
- **Witness:** Stop answer at index 3; `StopRequested`'s flush returns
  `Err(BrokenPipe)` → `Fatal{Journal(StopRequested, None, …)}`; finalization's
  shutdown reports `{Quiesced, Some(E)}`. `RUN-FINALIZE` discards `E`; read literally,
  `RUN-CHECKPOINT` says the `StopPending` row is decisive on that report, which would
  surface `E` as `Environment(Shutdown)`.
- **Fix sketch:** "…and once `StopPending` runs, its row is decisive on the report."

#### CO-12 "The bounded policy" had no antecedent; §8 cited a phrase §5 no longer carried — NIT, confidence high, wording
- **Text attacked:** `ENV-SHUTDOWN` (pre-fix): "waits according to its
  run-scoped-activity accounting. The bounded policy applies only to waiting…";
  `LIVE-COMPLETION` and the §8 *Justify:*: "`ENV-SHUTDOWN`'s bounded quiescence
  policy."
- **Claim:** The definite description named a policy no prior sentence introduced, and
  the two §8 references cited a phrase the rewritten row no longer used.
- **Witness:** The three quotes; grep for "bounded quiescence policy" hit only §8.
- **Fix:** Applied mechanically in §1 ("This bounded quiescence policy"). The fuller
  in-row wording (naming the bound's owner via `ENV-BOUNDS`) remains open.

#### CO-13 Wake-promptness during the completion wait is pinned by nothing — NIT, confidence high, unenforceable claim
- **Text attacked:** `LIVE-COMPLETION`: "While shutdown is waiting, the transition
  wakes it." `LIVE-SHUTDOWN`: "it waits only for outstanding entries."
- **Claim:** No bullet distinguishes a wake-driven wait from an implementation sleeping
  to the full deadline before its final observation; the violation is observable only
  as elapsed time, which no Core-owned output captures. NIT because no exit, report, or
  Journal byte diverges.
- **Witness:** Deadline 30 s; the only entry turns `Complete` at 1 ms; a
  sleep-to-deadline implementation still returns `{ Quiesced, None }` and passes every
  enumerated bullet.
- **Fix sketch:** One bullet: a completion during the wait ends the wait promptly
  (bounded observation latency under an injected clock).

#### CO-14 "stops Event delivery" binds nothing observable — NIT, confidence medium, wording
- **Text attacked:** `Environment::shutdown` doc comment and `ENV-SHUTDOWN`: "stops
  Event delivery."
- **Claim:** `shutdown(self)` consumes the Environment and `ENV-SERIAL` permits no
  later call, so no `next_event` exists for delivery to be stopped from; the observable
  content is carried entirely by "closes Event admission." Fails §0's deletion test.
- **Witness:** After `shutdown` is invoked the Environment value is moved; an
  implementation that "stops delivery" and one that does nothing are indistinguishable
  at every boundary.
- **Fix sketch:** Drop the phrase or fold it into the admission-close clause.

#### CO-15 "Every Environment-accounted lifecycle is then `Ended`" is false for a never-started Environment — NIT, confidence low, false claim
- **Text attacked:** `SIM-SHUTDOWN`: "Every Environment-accounted lifecycle is then
  `Ended`, so the report carries `Quiesced`." `ENV-SERIAL`: "`start` at most once, and
  first if at all; … `shutdown` at most once."
- **Claim:** `ENV-SERIAL`'s quantifiers admit `shutdown` with no prior `start`, leaving
  every lifecycle `NotStarted`: the justifying premise is false though the `Quiesced`
  conclusion still holds. Unreachable from the Engine; whether the bare-`shutdown`
  pattern is inside `ENV-SERIAL`'s assumed discipline is itself unforced — hence NIT.
- **Witness:** `let env = build(); env.shutdown()` — zero `stop` calls, all lifecycles
  `NotStarted`, report still `{ Quiesced, None }`.
- **Fix sketch:** "Every started lifecycle is then `Ended`."

### Attacked and held

- **Signal-first end-to-end consistency** — doc comment, `ENV-SHUTDOWN`,
  `LIVE-SHUTDOWN`, `StopPending`, and `RUN-CHECKPOINT`'s Stop-path sentence agree on
  raise → window (latch open) → final observation closes; no residue of the old
  close-first order anywhere.
- **The Stop-path outcome matrix** — all four `{Quiescence} × {Error}` report shapes
  plus the `TurnCompleted(Stop)` commit failure land on exactly one `StopPending`
  clause each; `VERIFY-LATCH`'s three integration sentences match the row cell-for-cell
  (causes; but see CO-06 on the quiescence value).
- **`RUN-FINALIZE` arm exhaustiveness under the new report semantics** — all 17
  Fatal-producing points hit exactly one arm; consumed-without-`StopPending` is
  unconstructible (only `close(env)` and finalization itself call `shutdown`);
  independently re-walked by the synthesizing reviewer.
- **A completion racing shutdown's initiating instant** — benign under D1: `run(Err)`
  publishes on both sides of the instant; for `run(Ok)` each physical
  resolution coincides with one legal linearization and self-witnesses through the
  report, so no torn both-or-neither behavior is constructible (a sub-review witness
  claiming otherwise was rejected on verification; the residual coverage gap is CO-04).
- **The latch's four-state machine** — all four states × {publication, `take_error`,
  operation-`Err` resolution, close}: every cell has exactly one listed outcome;
  reported-then-close and pending-at-close both agree with the report's doc comment.
- **`run(Err)` between deadline expiry and the final observation** — captured,
  `{ Incomplete, Some(error) }`, Error outranks `Incomplete`; pinned by the
  Error-plus-expiry bullet. The mirrored completion-between-expiry-and-observation case
  is exactly the "completion concurrent with expiry" bullet.
- **Publication-precedes-`Complete`** — holds on return-`Err`, premature, and
  cancellation paths; the final scan cannot count a shell complete and miss its Error;
  the post-`Quiesced` join tail has no surviving publication source.
- **Sim Stop-path latch-empty-at-entry** — every sim publication is caught by its own
  turn's checkpoint (single-threaded; completed-before ordering forced), so the §9
  derive's "otherwise carries `None`" is airtight, and the Fatal-path
  stale-pending-Error variant resolves consistently end-to-end (report carries it,
  `stop` Errors discarded, finalization discards the report Error).
- **`SIM-START` cleanup "discarding those Errors"** — licensed by A4's cleanup rule and
  `ENV-ERRORS`' pre-commitment clause; no contradiction with publication rules.
- **`SIM-DISPATCH`/`SIM-SELECT` latch-precedence clauses** — defer to `ENV-LATCH`'s D2
  clause exactly; both shipped mechanisms realize it.
- **Deadline arithmetic** — saturation clause present and pinned; one shared deadline,
  no restart; zero excluded by §10 + `BOUND-NONZERO`.
- **Timeout cannot masquerade as an Error** — "`deadline expiry without an Error
  returns { Incomplete, None }`" rules out Environment-minted timeout Errors.
- **`Quiesced`-implies-joined derive; "Stopped implies a clean report"; "a `Stop`
  outcome also witnesses the clean report"** — each held against its producing rows
  inside the disclosed enforcement boundary.
- **`VERIFY-FAULTS` cross-product vs the new semantics** — post-`start` restriction,
  report-Error discard, and the separate start-`Err`-no-shutdown case all agree with
  `RUN-FINALIZE`'s arms.
- **SYN-08 residue under D1** — the close returning the pending Error through the
  report makes overlap placement post-hoc checkable even for opaque `Err` returns; the
  redesign strengthened `VERIFY-LATCH`'s agreement check.
- **`start`-`Err` leaves the latch open forever** — harmless; no rule claims every
  latch closes, and `ENV-SERIAL` forbids every later observation.
- **Finite-source pattern against signal-first** — terminal Event → `Stop` → signal →
  Port returns; timing consistent.

### Coverage

- §5 `ENV-SHUTDOWN`, `ENV-LATCH`, API block doc comments, commitment rows, Notes, and
  the Glossary lines they lean on: walked.
- §7 `StopPending`, `close(env)`, `RUN-FINALIZE`, `RUN-CHECKPOINT`, the
  `EnvironmentOperation` docs, shutdown-touching Notes: walked.
- §8 `LIVE-SHUTDOWN`, `LIVE-SUPERVISION` (plus `LIVE-COMPLETION`/`LIVE-LIFECYCLE`/
  `LIVE-EVENTS`/`LIVE-START` as interaction targets), Mechanism and Notes: walked.
- §9 `SIM-SHUTDOWN` (plus `SIM-LIFECYCLE`/`SIM-START`/`SIM-DISPATCH`/`SIM-SELECT` as
  interaction targets), Mechanism and Notes: walked.
- §12 `VERIFY-LIVE`, `VERIFY-SIM`, `VERIFY-LATCH`, `VERIFY-FAULTS` shutdown cases:
  walked.
- Remainder of the document: context only for this pass (mechanically reconciled in
  §1; not re-reviewed).

### Questions the document cannot answer

None within the scoped rows beyond what the findings capture; every other question
raised during the walks resolved from the text.

---

## 3. Version recommendation

**Recommendation: v13, with v12 frozen the way v11 was.** D1 changed shipped,
observable behavior — the same Port conduct that produced `Stopped` under
v12-as-reviewed now produces `Fatal { Environment(Shutdown) }`, and the latch lifetime,
`ShutdownReport` fixing, and all three shipped shutdown rows were rewritten — so the
eleven reviews and every finding body in `synthesis-v12.md` quote text the current
document no longer contains, which is precisely the situation that froze v11 ("the
artifact the adversarial reviews cite"); the synthesis already relies on the
distinction (its Held-under-fire note records verdicts "of v12-as-reviewed, not of the
post-D1 design"), the current Status line's citation pointer is stale either way and
must be rewritten (reconciliation item 1), and this close-out has now produced a fresh
adversarial record (§2, including one MAJOR) that needs an unambiguous target name for
the next round. Mechanics if adopted: v12-as-reviewed survives at commit 2ac4fcd
("reviews added" — the tree the eleven reviews read); the Status block would name v13
authoritative and point the v12 freeze at that commit (or a tag), with
`design_docs/reveiws/` as the v12 round's record. The decision is Devon's; no Status
edit was made.

---

## 4. Resolutions (2026-08-25, approved by Devon)

Every finding and flagged item above is now dispositioned; the design-doc edits landed in
the close-out resolution commit alongside this section.

| Item | Disposition |
|---|---|
| CO-01 | Fixed — `ENV-LATCH` anchors the close at the final observation, not the `shutdown` call. |
| CO-02 | Fixed — the pre-commitment failure instant carries the same anchors, stated in-row. |
| CO-03 | Fixed — `VERIFY-LIVE` now verifies `LIVE-LIFECYCLE` and the `LiveCtx` signal semantics (recv ordering, blocked-recv wake, `try_recv` drain/`None`). |
| CO-04 | Fixed — `VERIFY-LIVE` bullet: pre-signal premature completion publishes and wakes. |
| CO-05 | Fixed per **D8** (decided at this gate): a pre-signal test-profile unwind is a premature closure and publishes; post-signal it stays unpublished like an expected `run(Ok)`. `LIVE-SUPERVISION`, the shell-guard mechanism (publish-before-`Complete` preserved), and a `VERIFY-LIVE` bullet updated. |
| CO-06 | Fixed — `VERIFY-FAULTS` checks the exit's `quiescence`, including retained `Quiesced` across a `TurnCompleted(Stop)` commit failure. |
| CO-07 | Fixed — all three report bullets qualified with first-publication scope. |
| CO-08 | Fixed — `ENV-SHUTDOWN` and the doc comment state one initiating step with free internal order; `SIM-SHUTDOWN` conforms. |
| CO-09 | Fixed per **D9** (decided at this gate): new Obligations row `TRUST-SHUTDOWN` (Kavod implementer; code review) — the Obligations-table route, forced by §12's completeness sentence; `ENV-SHUTDOWN` cites it for shipped conduct and `TRUST-ENV` for bespoke. Appendix A updated. |
| CO-10 | Fixed — observability scoped to Ports not already ended; ended residue cited to `TRUST-SPAWN`. |
| CO-11 | Fixed — "once `StopPending` runs, its row is decisive on the report." |
| CO-12 | Closed — the mechanical antecedent fix in §1 suffices; the fuller in-row wording is declined as unnecessary. |
| CO-13 | Fixed — `VERIFY-LIVE` bullet: a completion during the wait ends the wait promptly under an injected clock. |
| CO-14 | Fixed — "stops Event delivery" removed from the doc comment, `ENV-SHUTDOWN`, and `SIM-SHUTDOWN`. |
| CO-15 | Fixed — "Every started lifecycle is then `Ended`." |
| Flagged 1 (Status line) | Fixed with the version decision (below). |
| Flagged 2 (§0 self-citation) | Fixed — "this section's own citations" added to the exemption list. |
| Flagged 3 (test.md) | Fixed — test.md's compile-fail sentence aligned with the amended `VERIFY-GRAMMAR`. |

**Version decision (Devon): stay v12.** Version numbers mark design revisions, not review
rounds; the document is not at a boundary while Wiring is open, and Wiring close is the
natural v13. The stale-citation problem §3 correctly identified is solved by the tag
`v12-as-reviewed` (→ 2ac4fcd, the tree the eleven reviews read) and the rewritten Status
block, which names the round's record and reserves v13 for the Wiring close. §3's
recommendation is thereby declined on the remedy while its premise is accepted and
addressed.
