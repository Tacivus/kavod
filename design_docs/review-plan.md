# Kavod Core — Review Plan

The Core is built: every step through the compile-fail fixture is done, `cargo test`
and `cargo clippy --all-targets -- -D warnings` are green. The Environment work that
comes next adds implementations of contracts the Core already consumes; it does not
change the Core. That makes now the last cheap moment to check the Core against the
design, and the only moment where a finding costs one fix instead of two.

This doc is the plan for that check. It stands alone: it says what each round looks
for, why the rounds are in this order, and what ends them.

## What is being reviewed

| | lines |
|---|---|
| `src/`, code | 1,407 |
| `src/`, in-file tests | 7,220 |
| `tests/`, cross-file suites | 3,391 |

The whole Core is 1,407 lines and its longest function is 92. Reading all of it is one
sitting. The 10,611 lines of tests are the expensive surface, and they are also the
only thing standing between a Core rule and a silent regression. The rounds are
weighted accordingly: one round reads the code, three rounds work on what the tests
prove.

## Rules

1. **A round finds; it does not fix.** Findings go to the round's file, one entry each,
   with the ID or the code site it attacks. Fixes land as one batch at the round's
   close.
2. **Green between rounds.** `cargo test` and `cargo clippy --all-targets -- -D warnings`
   pass before a round opens and after its batch lands.
3. **The design is frozen.** `design-v12.md` carries two adjudicated adversarial review
   rounds and a rejected-attack table. A finding that says the design is wrong is
   recorded and deferred to the reserved Wiring-close slot — never fixed inline. A
   finding that says the code disagrees with the design is a code finding.
4. **Build-step rule 2 is suspended for the last round only.** "Code is final" holds
   through the ledger, conformance, and adversarial rounds: they add tests and fix
   defects, they do not restructure. The simplification round suspends it, and nothing
   else does.
5. **A test added in any round follows the existing shape** — nested module per subject
   and behavior, an `Invariant:` sentence a stranger can read, a `Design Doc:` line only
   where a row is pinned.
6. **Nothing here pulls Environment work forward.** A gap whose only fix lives in a
   later step is recorded as deferred, with the step named.

## Round 0 — the ledger

**Produces** `review-ledger.md`. Mechanical, not a review.

Every invariant ID and every law gets one row: the tests that pin it, or the step it is
deferred to, or `unpinned`. Then every clause of the five Core-scope enforced
verification rows — `VERIFY-CONTEXT`, `VERIFY-JOURNAL`, `VERIFY-FAULTS`,
`VERIFY-GRAMMAR`, `VERIFY-CONFORMANCE` — is split out and pointed at the test that
discharges it; those rows are paragraphs, and a row is only as enforced as its least
covered clause.

This round runs first because it is the only one that can find a test that was never
written, and because the other three consume its output: conformance reads it as the
list of rules to check code against, and the adversarial round starts from its
`unpinned` entries.

A row resolves to one of three things, and every row must resolve:

- **pinned** — named tests, and the round is done with it.
- **deferred** — the enforcing suite belongs to a later step; the step is named.
- **gap** — Core-scope, enforceable now, nothing pins it. Gaps are the round's output.

## Round 1 — conformance

**Produces** `review-conformance.md`. Reads `src/` against the design.

Bottom-up in dependency order — the time types, the bounded buffer, the Journal, the
Application contract, the Port contract, the latch, the Environment contract, the
record grammar, the Engine — checking both directions. A design rule with no code that
realizes it is a finding. Code that does something no rule asks for is also a finding:
unasked-for behavior is unowned behavior, and the next reader will build on it.

Assertions get their own read. Every asserted invariant is supposed to have an owning
guarantee and a named site under `ASSERT-INVARIANTS`; an assertion that names no
invariant, or an invariant asserted nowhere, is a finding either way.

This round is second because its findings change the code the next two rounds work on,
and it is cheap enough — 1,407 lines — that running it early costs almost nothing.

## Round 2 — adversarial

**Produces** `review-adversarial.md`. The round that asks what was missed.

Per subsystem, the attack list is written **before** the existing tests are read, then
checked against them; reading first produces a list shaped like the tests that already
exist. Attacks that survive become tests.

Then the cases no single-subsystem test can reach, which is where the real risk is: a
rule each subsystem honors alone and the composition breaks. Boundaries at the index
domain's ceiling and at the record-size bound; a batch that is empty, exactly full, and
one past; equal and decreasing timestamps at every entry point; a poisoned Journal
meeting every later commit site the graph allows; an overflow marker and a Fatal in the
same turn; and the full cross-product of latch-pending, operation-local, and
shutdown-report Errors, which is where A4's precedence either holds or does not.

Third because the conformance round can move the code out from under a test written
here.

## Round 3 — simplification

**Produces** `review-simplification.md`. Restructures; changes no behavior.

Last, for two reasons. It is the only round whose safety net is the test suite, so the
suite should be audited before it is trusted. And simplifying code that has not yet
been checked for defects spends the effort twice.

Scope is the Core's own shape: nesting that a uniform error type would flatten, helpers
that exist once, names that drifted from the design's vocabulary, and anything clippy
cannot see. Public documentation is **not** in scope — it is the export-audit step's,
and writing it now means writing it against an API the wiring decisions can still move.

## Not in scope

**Redesign.** Covered by rule 3.

**Anything Environment.** The Simulated and Live steps are gated on the nine wiring
decisions, which are unapproved and which the design's Wiring section still marks open.
Those decisions are the real blocker on the next phase, and they are a separate piece
of work from this one.

**The export audit and public docs.** Its own step, after the wiring settles.

## Done

The review is done when every ledger row reads pinned or deferred, the three finding
files are empty of open entries, and the suite and clippy are green. What is left over
is one list: findings deferred to the Wiring close, each naming the row it attacks.
