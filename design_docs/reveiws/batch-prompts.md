# Batch execution prompts

How to use: run the batches **in order, one fresh session each**. For each batch, paste the
**Shared preamble** followed by that batch's **addendum** as one prompt. Each batch must be
applied (and ideally committed) before the next session starts, so later batches read the
merged text. The **Close-out** prompt at the end is standalone — do not attach the preamble.

---

## Shared preamble (include in every batch prompt)

```
You are executing one approved batch of edits to design_docs/design-v12.md, the
authoritative Kavod Core design document. The batch is defined in
design_docs/reveiws/synthesis-v12.md. This is a document-only session: src/ is read-only.

INPUTS, in authority order:
1. design_docs/design-v12.md — the artifact you edit. Its §0 reading rules govern every
   sentence you write. Read §0 first, in full.
2. design_docs/reveiws/synthesis-v12.md — the adjudicated work order. Read the "How to run
   a batch (all batches)" section, then YOUR batch's section only: its intro, its
   execution notes, and every contained SYN finding (Text / Adjudication / Witness / Fix
   direction). Finding quotes are excerpts — always re-read the full row or section in
   design-v12.md before editing it.
3. design_docs/reveiws/batch_0.md — the decision record (D1–D7). Where an execution note
   marks a decision "decided", that decision OVERRIDES the finding's original Fix
   direction; the batch notes say so wherever it happens.
Do NOT read the individual review files (deepseek.md, fable.md, gemini.md, grok.md,
kimi.md, muse.md, opus.md, ox.md, sol.md, sonnet.md, terra.md). They are testimony about
superseded text and will mislead you.

PROCESS — understand first, then plan, then edit:
1. Read the inputs above.
2. Pass the UNDERSTANDING GATE (below) — a written deliverable, produced before any
   patch text is drafted.
3. Write a patch plan: for every SYN in the batch (and any added scope the execution
   notes declare), the exact current text, the exact replacement text, and which of §0's
   binding forms the change lands in. Note any point where you departed from a Fix
   direction and why (a decision override, or merged-text drift).
4. Present the Understanding section and the patch plan together and STOP for approval.
   Do not edit before approval.
5. On approval, apply exactly the plan — nothing more.
6. Verify (below), then commit.

UNDERSTANDING GATE — most of this work is understanding; the edit is the last step.
Before drafting any patch, write an Understanding section with one entry per SYN (and
one per decision your batch applies). Each entry has five parts:
- THE DEFECT, restated in your own words without quoting the finding: what is broken,
  who it bites (an implementer, a bespoke Environment author, a test author, a reader),
  and why §0's rules make it a defect rather than a style preference.
- THE WITNESS, re-run by you against the CURRENT text: walk the finding's witness
  scenario through the rows as they stand after earlier batches and confirm the defect
  still reproduces as described. If it does not — earlier batches changed the text, or
  your reading of the rows differs — say so and stop for discussion. Never patch a
  defect you cannot reproduce.
- WHY THE RULE EXISTS: one sentence on what the row or sentence you are about to edit
  protects, so your edit preserves it. If you cannot say what a rule is for, you are
  not ready to change it.
- DONE-WHEN: the observable condition under which this SYN is closed — what a reader,
  implementer, or suite can now do (or can no longer misread) that they could not before.
- BLAST RADIUS: every other row, record, suite, or Glossary term that reads the text you
  are changing, and for each, why it is unaffected or changes as intended.
An entry you cannot complete means you do not yet understand the problem: go back to the
text and the decision record — never fill the gap with a guess.

RULES WHILE EDITING:
- Every new normative sentence lands in one of §0's four binding forms — a guarantee row
  with an ID, an API block, a binding-table row, or an Obligations row. Prose may only
  define, derive, or justify (and, once D6 lands, Mechanism illustrates nonbindingly).
  Apply §0's deletion test to any prose you add.
- Cite IDs, never section numbers. Citations point backward across sections; trust marks
  may point forward into the Obligations table. Glossary entries stay one line per term.
- Write in the document's voice: plain language, derive rather than enumerate, no
  hedging, no meta-commentary about the edit itself.
- The batch must leave the document self-consistent. If a genuinely required change falls
  outside the batch's scope, write it down as a flagged follow-up and stop — do not make it.
- Never improvise a design decision. If two decided rules seem to conflict, or a Fix
  direction cannot be applied as written, stop and report instead of choosing.

VERIFY BEFORE COMMITTING:
- Re-read every edited row in its full surrounding context.
- Check each SYN against its Fix direction as amended by the decisions: is the defect it
  names actually closed?
- Run the batch's own execution-note checks (each batch section lists its specific ones).
- Appendix A reconciles: every added or renamed ID appears exactly once; no citation in
  the body dangles.

COMMIT: one commit, message "design-v12: Batch N — <batch title> (SYN-.., SYN-.., ...)".
Do not edit synthesis-v12.md or batch_0.md.

FINAL REPORT: list each SYN with one line on how it was closed, every flagged follow-up,
and anything you could not close and why.
```

---

## Batch 1 addendum — Shutdown & latch redesign

```
Your batch is "Batch 1 — Shutdown & latch redesign (D1 + D2; §5, §8, §9, §12)".

This is the round's ONLY design-change batch; everything after it is mechanical. Before
planning anything, read batch_0.md D1 and D2 in full — D1's Stop-path outcome matrix is
the acceptance criterion for the rows you rewrite, and D1 deliberately reverses SYN-01's
original Fix direction (signal-first, latch open through the bounded graceful-shutdown
window, close as the final Error observation — NOT close-before-signal).

Expect to rewrite whole rows, not patch sentences: ENV-SHUTDOWN and ENV-LATCH (§5, plus
the shutdown doc comment and commitment row), LIVE-SHUTDOWN and LIVE-SUPERVISION (§8),
SIM-SHUTDOWN and the two §9 "structurally None" notes, and the D1/D2 cases in
VERIFY-LIVE, VERIFY-SIM, and VERIFY-LATCH (§12). SYN-03, -04, -09, -10, and -19 are folded
into this batch because they are part of the same design — write them into the new rows,
not as separate patches against the old ones.

Known constraint from the batch notes: after drafting ENV-SHUTDOWN and ENV-LATCH, check
that §7's StopPending row still reads correctly against the new report semantics. D1's
outcome matrix says it should need no change — if you find it does, that is a stop-and-
flag, not an edit.

DEEPER UNDERSTANDING REQUIREMENT for this batch (in addition to the Understanding Gate):
before writing any SYN entry, re-derive the D1 design yourself. In your own words, write
the complete shutdown timeline — signal raised, the graceful window with the latch open
(who may publish during it, and what each publication does), the final observation, the
close, the report, and how StopPending classifies each report — then check your
derivation against batch_0.md D1's outcome matrix line by line. Every line must match.
A mismatch means you and the decision disagree about the design: that is a stop-and-
discuss, never an interpretation you resolve silently. Also state, in one paragraph, WHY
close-before-signal was rejected (the race it creates) — if you cannot argue the
rationale, you will not write the rows correctly.
```

---

## Batch 2 addendum — Environment contract & Glossary wording residue

```
Your batch is "Batch 2 — Environment contract & Glossary wording residue (§1 + §5)".

Prerequisite: Batch 1 is merged. Several of your targets are rows Batch 1 just rewrote
(ENV-LATCH for SYN-08, ENV-SHUTDOWN for SYN-07, the shutdown doc comment for SYN-44), so
the text your findings quote may already be gone — the DEFECT each finding names is what
you close, in the merged text. In particular, SYN-08's scoping must fit the D2 precedence
clause now present in ENV-LATCH.

This batch is wording-level: prefer the smallest diff that closes each SYN. No decisions
are open here.
```

---

## Batch 3 addendum — The Run pass

```
Your batch is "Batch 3 — The Run pass (§7, plus one A4 clause in §2)".

Prerequisite: Batch 1 is merged (SYN-39 cites the ENV-LATCH precedence clause it added).

Decision context: D4 is settled and recorded in your batch notes — SYN-24 adds an outcome
field to JournalFatal; RecordKind, the record_kind wire tags, RUN-RECORDS, the Records
table, and every golden-test expectation stay untouched. Verify that stays true after
your edit. SYN-24 and SYN-25 are exact-form API-block edits (§0 form 1).

Drafting constraint from the batch notes: SYN-22, -23, -28, and -30 all land in §7's
opening paragraph and Edges preamble — draft those few sentences as ONE rewrite, then
check each of the four SYNs against the result.
```

---

## Batch 4 addendum — Enforcement & verification pass

```
Your batch is "Batch 4 — Enforcement & verification pass (§0 + §12)".

Prerequisite: Batches 1–3 are merged. Read batch_0.md D3, D6, and D7 in full before
planning.

The "Approved D3 disposition" table inside your batch section is SYN-11's scope — execute
it row by row; neither the batch prose nor your judgment overrides a table row. Batch 1
already added the D1/D2 shutdown and publish-while-blocked cases to VERIFY-LIVE,
VERIFY-SIM, and VERIFY-LATCH: verify they are present, extend those rows per the table,
and do not regress or duplicate them (the table's blocked-wake row is already satisfied).

This batch edits §0 itself (D6's Mechanism sentence, D7's placement declaration) — the
highest blast radius in the document, since §0 governs how every other sentence is read.
Plan those two edits word-for-word in the patch plan. End your verification by re-reading
§12's framing sentences ("complete trusted boundary") against the finished text, and
expect heavy Appendix A updates.

DEEPER UNDERSTANDING REQUIREMENT for this batch (in addition to the Understanding Gate):
for each row of the Approved D3 disposition table, state in your own words why its
assigned tier is the FIRST AVAILABLE tier under §0's enforcement order — that is, why the
rule cannot be made unrepresentable, and (where the row assigns a suite or trust) why an
always-on assertion cannot carry it either. A row whose tier you cannot justify is a row
you flag before executing, not one you execute on faith. Then state the D3 strategy's
own boundary rule back: a trusted obligation is legitimate only where no execution trace
can witness the rule — and confirm each TRUST-touching row in the table meets it.
```

---

## Batch 5 addendum — Live/Sim residue

```
Your batch is "Batch 5 — Live/Sim residue (§8 + §9, plus two Laws rows in §2)".

Prerequisite: Batches 1–2 are merged. LIVE-EVENTS and LIVE-SELECT (SYN-34, -35) were
mostly untouched by Batch 1, but LIVE-EVENTS' fan-in-close sentence interacts with the D1
signal — re-read the merged rows before writing. SYN-05 touches Glossary lines
(Handoff/Admission) adjacent to ones earlier batches edited; re-read them as merged.

D5 is settled: BOUND-STATIC gets the exact wording in your batch notes, and the order's
source (registration vs declaration) stays an open Wiring decision — do not choose it.
This batch is small and mechanical.
```

---

## Batch 6 addendum — NIT sweep

```
Your batch is "Batch 6 — NIT sweep (§3, §4, §9, §11, status line)".

Prerequisite: Batches 1–5 are merged. Everything here is mechanical; keep every edit to
the smallest form that closes its SYN, with two ordering constraints from the batch
notes: SYN-45's finite-source pattern needs a citable home BEFORE SIM-COMPLETION's
citation can change, and SYN-54's rename is all-or-nothing (git mv the directory + the
status-line pointer + a repo-wide grep for the old path, in one commit — or decline the
whole thing). ASK THE USER whether to take the SYN-54 rename before planning it; do not
decide it yourself. Note that the rename changes the paths this prompt and the synthesis
use.
```

---

## Close-out prompt (standalone — do NOT attach the shared preamble)

```
All six batches of the design-v12 review round are merged. You are running the close-out
pass defined at the end of the batches in design_docs/reveiws/synthesis-v12.md. This
pass is verification, not editing: produce a report; make no edits beyond trivial
mechanical fixes the reconciliation itself surfaces (a dangling citation, a missing
Appendix A row), each listed in the report.

Three jobs:

1. MECHANICAL RECONCILIATION of design_docs/design-v12.md: every ID in the body appears
   in Appendix A exactly once and vice versa; every citation resolves; §0's reading rules
   and §12's completeness sentences ("complete trusted boundary", the enforced/trusted
   split) are true of the finished text; one repo-wide grep for stale references
   (including the old reveiws/ path if the Batch 6 rename was taken).

2. TARGETED ADVERSARIAL RE-REVIEW of the Batch 1 sections only: ENV-SHUTDOWN, ENV-LATCH,
   the shutdown doc comment and commitment row, LIVE-SHUTDOWN, LIVE-SUPERVISION,
   SIM-SHUTDOWN, the amended VERIFY rows, and their interactions with StopPending and
   RUN-FINALIZE. This text is the round's only new design and has never been reviewed.
   Apply the rules of design_docs/reveiws/review-prompt.md — its severity ladder,
   classification vocabulary, steelman-first discipline, and witness requirement — scoped
   to those rows. The rest of the document is context, not a target.

3. VERSION RECOMMENDATION: the shutdown redesign changed shipped behavior. Recommend, with
   one paragraph of reasoning, whether the result should remain v12 or become v13 with
   v12 frozen the way v11 was. This is the user's decision — present the recommendation
   and stop; do not edit the Status block.

Report: reconciliation results (with any mechanical fixes made), the re-review in
review-prompt.md's output format, and the version recommendation.
```
