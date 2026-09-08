# Review prompts

Thirteen prompts for the Core's second review, in the order they run: for each of six
groups, Pass E then Pass I; then Synthesis. Each is one block to paste into a fresh
session with a high-grade model. Nothing is substituted; the group's files, design
rows, suites, and prior findings are inside the block.

Pass E finds edge cases nothing pins. Pass I judges shape: idiomatic, simple, robust
Rust, with a higher bar than the simplification round's "not worth the churn". E runs
before I in each group so the shape pass restructures under the fuller net; the groups
run in dependency order so a shape change below is in place before the group above it
is judged.

Every session reports first and edits nothing until told which numbers to land.
Findings accumulate in `review-edges.md` and `review-idiom.md`, one section per group,
created by the first session that needs the file; the synthesis session writes
`review-synthesis.md`. Nothing commits. `cargo fmt --check` fails today on four files;
each session formats its own group's files, so the check is clean by the last group.

## 1. foundations — edges

````
Review the foundations group of the Kavod core for edge cases that no test and no
assertion covers. Real cases, not coverage for its own sake. You did not write
this code. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/time.rs` (44 production lines, 10 tests), `src/bounded_buffer.rs` (84
production lines, 18 tests).
Design rows: A6, `BOUND-NONZERO`, `BOUND-LOOPS`, `ENV-TIME`, `RUN-INDEX`; the wire form
of both types, by name.
Suites: none target the group directly; `tests/golden_journal.rs` pins the wire form
through records.
Prior: `review-adversarial.md` — the Time types and Bounded buffer tables.
Heads up: `BoundedBuffer` is crate-private and backs both the Command batch and the
Journal's encode region. Its callers are `journal.rs`, `application.rs`, and
`engine/record.rs`, and `tests/grammar_fixture/src/lib.rs` carries a hand copy of it
and of `EventIndex`; a signature change here is mirrored there.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER — AND STOP BEFORE THE TESTS

1. The group's production code, top to bottom, stopping at `#[cfg(test)]`.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, the
   row, and its section's Notes. A law is a table row: `grep -n '^| A6 |'`. Never
   read the whole doc.

Now write the attack list. Do it before reading a single test: a list written after
the tests comes out shaped like the tests that exist.

THE ATTACK LIST

For every function, every input that sits on an edge and every state a failure
leaves behind. The edges that matter here:

- Boundaries: zero, one, capacity, capacity plus one, `u64::MAX`, `usize::MAX`,
  the smallest `NonZeroUsize`, the empty slice, a record that exactly fits.
- Every enum arm and every error path, and what stands after each failure: the
  next call's view, ownership of what was passed in, what was dropped and when.
- Order: two operations in both orders; a failure at the first position and at
  the last.
- The `io::Write` contract as a hostile sink honors it: `Ok(0)`, a count larger
  than the slice, `Interrupted`, a short write, an error after progress.
- serde corners: a non-object at the top level, an interior newline, a payload
  that fails mid-serialization.
- What a bespoke implementor of a trait in this group can do that the Core does
  not guard against.
- A claim in a design row or Notes paragraph naming this group that nothing
  asserts.

Skip an edge the group cannot reach; don't pad the table.

RESOLVE EACH ATTACK

Now read the tests: the group's own `mod tests` first, then every suite under
`tests/` named above, then `grep -rn` the whole test surface for the behavior.
Read bodies, not names; a test passes without pinning the attack more often than
its name admits. Resolve every attack to one of:

- pinned — the test, with its module.
- by construction — the type or standard-library guarantee.
- derived — the two pinned facts that compose to it.
- asserted — the site.
- open — none of the above.

Only open attacks are findings. A finding that duplicates a test anywhere in
`src/` or `tests/` is a defect in your report, so cite what you searched. Attacks
on the "Considered, not tested" list in `review-adversarial.md` are re-raised only
with a new argument.

For each open attack, read the code path once more and state what the code does.
If the code is wrong, that is a defect; it outranks every gap and goes first.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the test instead of describing it; the prose says what it pins and why
nothing else does. If the report is longer than the code it reviews, cut it.

THE REPORT

First the attack table: one row per attack, its resolution, the name it resolves
to. Then one entry per open attack, defects first, numbered E1, E2, …

    ### E2 — {the attack, one line}
    `path:line` — {what the code does there, one sentence}
    Searched: {modules and files read; the grep}
    Closes with: {test | assertion}
    ```rust
    {the complete test, or the assertion line}
    ```

A test follows `design_docs/test.md`: in the group's own `mod tests` unless it
needs the Engine loop, in which case name the `tests/` file and module it joins; a
nested `mod <subject>_<behavior>`; a doc comment opening with `Invariant:` and a
plain sentence with no IDs; a `Design Doc:` line naming one ID only when a row is
pinned. An assertion is always-on, constant-time, and names the invariant ID in
its message.

Then end your turn. Do not touch a file until I reply with the numbers to land.

WHEN I REPLY

Land the numbered entries. Before you trust a new test, break the code path it
pins and watch it fail; say which mutation you used, then revert it. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append the attack
table and your entries under `## foundations` in `design_docs/review-edges.md`, each
marked landed, or declined with my reason. Report the results and any test that
would not settle. Don't commit.
````

## 2. foundations — idiom

````
Review the foundations group of the Kavod core for shape: idiomatic, simple, robust
Rust. You did not write it. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/time.rs` (44 production lines, 10 tests), `src/bounded_buffer.rs` (84
production lines, 18 tests).
Design rows: A6, `BOUND-NONZERO`, `BOUND-LOOPS`, `ENV-TIME`, `RUN-INDEX`; the wire form
of both types, by name.
Suites: none target the group directly; `tests/golden_journal.rs` pins the wire form
through records.
Prior: `review-adversarial.md` — the Time types and Bounded buffer tables.
Heads up: `BoundedBuffer` is crate-private and backs both the Command batch and the
Journal's encode region. Its callers are `journal.rs`, `application.rs`, and
`engine/record.rs`, and `tests/grammar_fixture/src/lib.rs` carries a hand copy of it
and of `EventIndex`; a signature change here is mirrored there.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER

1. The group's files end to end: production code, then the `mod tests` beneath it.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, then
   the row and the API block of its section. A law is a table row:
   `grep -n '^| A6 |'`. Never read the whole doc.
3. The prior-review sections named under Prior above.

WHAT THE DESIGN FIXES, AND WHAT IT DOES NOT

The design fixes behavior: every record byte, every Environment call and its order,
every exit value, every error precedence, and the names in its API blocks. Nothing
you propose may move those. If you think a behavior is wrong, put it under
`Design notes` in your report and propose nothing.

The design does not fix shape. Derives, visibility, signatures, helper types,
control flow, module boundaries, assertions, and lint attributes are yours to
judge. A change that touches the public API is allowed; mark it, because the
export audit and the wiring decisions are still open.

WHAT TO JUDGE

Go through the group once for each of these. The bar is code a strong Rust
reviewer accepts without comment. "Works" is not the bar.

- Derives. Every public type carries what its users will need: `Debug` always;
  `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Default` wherever the semantics
  allow. Error types implement `Display` and `std::error::Error` with `source()`.
  A missing derive is a finding.
- Visibility. `pub` items no path reaches, `pub(crate)` where `pub(super)` is the
  truth, bounds on a struct that only its impl needs, a module boundary in the
  wrong place.
- Signatures. Tuples that want a named struct, generic soups that want an alias or
  a type, by-value arguments never consumed, lifetimes that elide, `const fn` where
  it is free, `#[must_use]` where dropping the value is a bug.
- Control flow. Nesting that `?`, `let … else`, `matches!`, or an early return
  flattens; a `match` that restores what it replaced; an `expect` or
  `unreachable!` that a type could delete.
- Assertions. Always-on, constant-time, message naming the invariant ID. An
  assertion the type system could make unrepresentable is a finding: the design's
  order is unrepresentable, then asserted, then tested.
- Lints. Run `cargo clippy --all-targets -- -W clippy::pedantic -W clippy::nursery`
  and take every hit in the group's files. Fix it, or suppress it with
  `#[expect(lint, reason = "…")]`. Never `allow`: `expect` fails when the lint
  stops firing, and an earlier round deleted forty-three stale allowances.
  Existing `allow` attributes become `expect`. Hits of one lint with one fix are
  one entry. Format the group's files; hunks elsewhere are another group's.
- Names. The design's glossary is the vocabulary. A name that drifted is a finding.
- Tests, shape only. Setup duplicated across tests that a helper names; `Err(_)`
  where the variant should be named; a group whose name no longer says what it
  holds; a test that pins a derive rather than behavior. Never remove a test that
  pins behavior. Two tests merge only when the merged one asserts everything both
  did.

Not findings: taste without an argument; documentation, which the export audit
owns, unless a comment is wrong; anything marked kept under Prior above unless you
argue against the reason `review-simplification.md` gives. Churn is a cost line, not
a reason to keep a worse shape.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the code instead of describing it; the prose says why. Never restate what the
code already says. If the report is longer than the code it reviews, cut it.

THE REPORT

One entry per finding, ranked: robustness first, then simplification, then lint
and style. Number them I1, I2, …

    ### I3 — {title, one line}
    `path:line`
    {why: one to three sentences}
    ```rust
    {the new shape, complete enough to apply}
    ```
    Cost: {what else moves — tests, fixture, callers}. API: {yes | no}.

Then `Design notes`, if any. Then end your turn. Do not touch a file until I reply
with the numbers to apply.

WHEN I REPLY

Apply the numbered findings in order. Keep every record byte, call order, exit,
and precedence as it was; the matrices in `tests/faults.rs` assert exact call
lists and will tell you if you slipped. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append your entries
under `## foundations` in `design_docs/review-idiom.md`, each marked landed, or declined
with my reason. Report the three commands' results and any finding that fought you.
Don't commit.
````

## 3. journal — edges

````
Review the journal group of the Kavod core for edge cases that no test and no
assertion covers. Real cases, not coverage for its own sake. You did not write
this code. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/journal.rs` (143 production lines, 42 tests).
Design rows: `JRN-FORMAT`, `JRN-ENCODE`, `JRN-COMMIT`, `JRN-POISON`, `JRN-SINK`;
`TRUST-SINK`, `TRUST-SERIALIZE`; A3, A8.
Suites: `tests/faults.rs` (`journal_fault_matrix`), `tests/golden_journal.rs`; the
sink fake is `tests/support/scripted_sink.rs`.
Prior: `review-adversarial.md` — the Journal table and S2; `review-simplification.md`
— S5 landed; "`encode_raw`, `encode_line`, `write_line` each have one caller" kept.
Heads up: `Journal::new` and `commit` reach `tests/grammar_fixture/src/lib.rs` through
`kavod::Journal`; a signature change here is mirrored there.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER — AND STOP BEFORE THE TESTS

1. The group's production code, top to bottom, stopping at `#[cfg(test)]`.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, the
   row, and its section's Notes. A law is a table row: `grep -n '^| A6 |'`. Never
   read the whole doc.

Now write the attack list. Do it before reading a single test: a list written after
the tests comes out shaped like the tests that exist.

THE ATTACK LIST

For every function, every input that sits on an edge and every state a failure
leaves behind. The edges that matter here:

- Boundaries: zero, one, capacity, capacity plus one, `u64::MAX`, `usize::MAX`,
  the smallest `NonZeroUsize`, the empty slice, a record that exactly fits.
- Every enum arm and every error path, and what stands after each failure: the
  next call's view, ownership of what was passed in, what was dropped and when.
- Order: two operations in both orders; a failure at the first position and at
  the last.
- The `io::Write` contract as a hostile sink honors it: `Ok(0)`, a count larger
  than the slice, `Interrupted`, a short write, an error after progress.
- serde corners: a non-object at the top level, an interior newline, a payload
  that fails mid-serialization.
- What a bespoke implementor of a trait in this group can do that the Core does
  not guard against.
- A claim in a design row or Notes paragraph naming this group that nothing
  asserts.

Skip an edge the group cannot reach; don't pad the table.

RESOLVE EACH ATTACK

Now read the tests: the group's own `mod tests` first, then every suite under
`tests/` named above, then `grep -rn` the whole test surface for the behavior.
Read bodies, not names; a test passes without pinning the attack more often than
its name admits. Resolve every attack to one of:

- pinned — the test, with its module.
- by construction — the type or standard-library guarantee.
- derived — the two pinned facts that compose to it.
- asserted — the site.
- open — none of the above.

Only open attacks are findings. A finding that duplicates a test anywhere in
`src/` or `tests/` is a defect in your report, so cite what you searched. Attacks
on the "Considered, not tested" list in `review-adversarial.md` are re-raised only
with a new argument.

For each open attack, read the code path once more and state what the code does.
If the code is wrong, that is a defect; it outranks every gap and goes first.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the test instead of describing it; the prose says what it pins and why
nothing else does. If the report is longer than the code it reviews, cut it.

THE REPORT

First the attack table: one row per attack, its resolution, the name it resolves
to. Then one entry per open attack, defects first, numbered E1, E2, …

    ### E2 — {the attack, one line}
    `path:line` — {what the code does there, one sentence}
    Searched: {modules and files read; the grep}
    Closes with: {test | assertion}
    ```rust
    {the complete test, or the assertion line}
    ```

A test follows `design_docs/test.md`: in the group's own `mod tests` unless it
needs the Engine loop, in which case name the `tests/` file and module it joins; a
nested `mod <subject>_<behavior>`; a doc comment opening with `Invariant:` and a
plain sentence with no IDs; a `Design Doc:` line naming one ID only when a row is
pinned. An assertion is always-on, constant-time, and names the invariant ID in
its message.

Then end your turn. Do not touch a file until I reply with the numbers to land.

WHEN I REPLY

Land the numbered entries. Before you trust a new test, break the code path it
pins and watch it fail; say which mutation you used, then revert it. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append the attack
table and your entries under `## journal` in `design_docs/review-edges.md`, each
marked landed, or declined with my reason. Report the results and any test that
would not settle. Don't commit.
````

## 4. journal — idiom

````
Review the journal group of the Kavod core for shape: idiomatic, simple, robust
Rust. You did not write it. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/journal.rs` (143 production lines, 42 tests).
Design rows: `JRN-FORMAT`, `JRN-ENCODE`, `JRN-COMMIT`, `JRN-POISON`, `JRN-SINK`;
`TRUST-SINK`, `TRUST-SERIALIZE`; A3, A8.
Suites: `tests/faults.rs` (`journal_fault_matrix`), `tests/golden_journal.rs`; the
sink fake is `tests/support/scripted_sink.rs`.
Prior: `review-adversarial.md` — the Journal table and S2; `review-simplification.md`
— S5 landed; "`encode_raw`, `encode_line`, `write_line` each have one caller" kept.
Heads up: `Journal::new` and `commit` reach `tests/grammar_fixture/src/lib.rs` through
`kavod::Journal`; a signature change here is mirrored there.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER

1. The group's files end to end: production code, then the `mod tests` beneath it.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, then
   the row and the API block of its section. A law is a table row:
   `grep -n '^| A6 |'`. Never read the whole doc.
3. The prior-review sections named under Prior above.

WHAT THE DESIGN FIXES, AND WHAT IT DOES NOT

The design fixes behavior: every record byte, every Environment call and its order,
every exit value, every error precedence, and the names in its API blocks. Nothing
you propose may move those. If you think a behavior is wrong, put it under
`Design notes` in your report and propose nothing.

The design does not fix shape. Derives, visibility, signatures, helper types,
control flow, module boundaries, assertions, and lint attributes are yours to
judge. A change that touches the public API is allowed; mark it, because the
export audit and the wiring decisions are still open.

WHAT TO JUDGE

Go through the group once for each of these. The bar is code a strong Rust
reviewer accepts without comment. "Works" is not the bar.

- Derives. Every public type carries what its users will need: `Debug` always;
  `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Default` wherever the semantics
  allow. Error types implement `Display` and `std::error::Error` with `source()`.
  A missing derive is a finding.
- Visibility. `pub` items no path reaches, `pub(crate)` where `pub(super)` is the
  truth, bounds on a struct that only its impl needs, a module boundary in the
  wrong place.
- Signatures. Tuples that want a named struct, generic soups that want an alias or
  a type, by-value arguments never consumed, lifetimes that elide, `const fn` where
  it is free, `#[must_use]` where dropping the value is a bug.
- Control flow. Nesting that `?`, `let … else`, `matches!`, or an early return
  flattens; a `match` that restores what it replaced; an `expect` or
  `unreachable!` that a type could delete.
- Assertions. Always-on, constant-time, message naming the invariant ID. An
  assertion the type system could make unrepresentable is a finding: the design's
  order is unrepresentable, then asserted, then tested.
- Lints. Run `cargo clippy --all-targets -- -W clippy::pedantic -W clippy::nursery`
  and take every hit in the group's files. Fix it, or suppress it with
  `#[expect(lint, reason = "…")]`. Never `allow`: `expect` fails when the lint
  stops firing, and an earlier round deleted forty-three stale allowances.
  Existing `allow` attributes become `expect`. Hits of one lint with one fix are
  one entry. Format the group's files; hunks elsewhere are another group's.
- Names. The design's glossary is the vocabulary. A name that drifted is a finding.
- Tests, shape only. Setup duplicated across tests that a helper names; `Err(_)`
  where the variant should be named; a group whose name no longer says what it
  holds; a test that pins a derive rather than behavior. Never remove a test that
  pins behavior. Two tests merge only when the merged one asserts everything both
  did.

Not findings: taste without an argument; documentation, which the export audit
owns, unless a comment is wrong; anything marked kept under Prior above unless you
argue against the reason `review-simplification.md` gives. Churn is a cost line, not
a reason to keep a worse shape.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the code instead of describing it; the prose says why. Never restate what the
code already says. If the report is longer than the code it reviews, cut it.

THE REPORT

One entry per finding, ranked: robustness first, then simplification, then lint
and style. Number them I1, I2, …

    ### I3 — {title, one line}
    `path:line`
    {why: one to three sentences}
    ```rust
    {the new shape, complete enough to apply}
    ```
    Cost: {what else moves — tests, fixture, callers}. API: {yes | no}.

Then `Design notes`, if any. Then end your turn. Do not touch a file until I reply
with the numbers to apply.

WHEN I REPLY

Apply the numbered findings in order. Keep every record byte, call order, exit,
and precedence as it was; the matrices in `tests/faults.rs` assert exact call
lists and will tell you if you slipped. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append your entries
under `## journal` in `design_docs/review-idiom.md`, each marked landed, or declined
with my reason. Report the three commands' results and any finding that fought you.
Don't commit.
````

## 5. contracts — edges

````
Review the contracts group of the Kavod core for edge cases that no test and no
assertion covers. Real cases, not coverage for its own sake. You did not write
this code. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/application.rs` (91 production lines, 13 tests), `src/port.rs` (33
production lines, 7 tests), `tests/ports_macro.rs` (4 tests: the `ports!` macro used
from a downstream crate).
Design rows: `APP-CONTEXT`, `APP-EMIT`, `APP-OVERFLOW`, `APP-FUTURE`, `APP-STATE`;
`PORT-SUMS`, `PORT-ROUTING` for its Core half; `TRUST-PURE`; `VERIFY-CONTEXT`.
`PORT-STATE` and the per-Slot Error sum wait on the Environment steps.
Suites: `tests/faults.rs` (`application_fault_matrix`), `tests/conformance.rs`; the
Application fake is `tests/support/recording_app.rs`.
Prior: `review-adversarial.md` — the Context and Port macro tables.
Heads up: G9 in `review-ledger.md` — `APP-FUTURE` is enforced by `Context`'s shape and
no site is named — is open and Devon's decision. Note it; don't re-find it.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER — AND STOP BEFORE THE TESTS

1. The group's production code, top to bottom, stopping at `#[cfg(test)]`.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, the
   row, and its section's Notes. A law is a table row: `grep -n '^| A6 |'`. Never
   read the whole doc.

Now write the attack list. Do it before reading a single test: a list written after
the tests comes out shaped like the tests that exist.

THE ATTACK LIST

For every function, every input that sits on an edge and every state a failure
leaves behind. The edges that matter here:

- Boundaries: zero, one, capacity, capacity plus one, `u64::MAX`, `usize::MAX`,
  the smallest `NonZeroUsize`, the empty slice, a record that exactly fits.
- Every enum arm and every error path, and what stands after each failure: the
  next call's view, ownership of what was passed in, what was dropped and when.
- Order: two operations in both orders; a failure at the first position and at
  the last.
- The `io::Write` contract as a hostile sink honors it: `Ok(0)`, a count larger
  than the slice, `Interrupted`, a short write, an error after progress.
- serde corners: a non-object at the top level, an interior newline, a payload
  that fails mid-serialization.
- What a bespoke implementor of a trait in this group can do that the Core does
  not guard against.
- A claim in a design row or Notes paragraph naming this group that nothing
  asserts.

Skip an edge the group cannot reach; don't pad the table.

RESOLVE EACH ATTACK

Now read the tests: the group's own `mod tests` first, then every suite under
`tests/` named above, then `grep -rn` the whole test surface for the behavior.
Read bodies, not names; a test passes without pinning the attack more often than
its name admits. Resolve every attack to one of:

- pinned — the test, with its module.
- by construction — the type or standard-library guarantee.
- derived — the two pinned facts that compose to it.
- asserted — the site.
- open — none of the above.

Only open attacks are findings. A finding that duplicates a test anywhere in
`src/` or `tests/` is a defect in your report, so cite what you searched. Attacks
on the "Considered, not tested" list in `review-adversarial.md` are re-raised only
with a new argument.

For each open attack, read the code path once more and state what the code does.
If the code is wrong, that is a defect; it outranks every gap and goes first.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the test instead of describing it; the prose says what it pins and why
nothing else does. If the report is longer than the code it reviews, cut it.

THE REPORT

First the attack table: one row per attack, its resolution, the name it resolves
to. Then one entry per open attack, defects first, numbered E1, E2, …

    ### E2 — {the attack, one line}
    `path:line` — {what the code does there, one sentence}
    Searched: {modules and files read; the grep}
    Closes with: {test | assertion}
    ```rust
    {the complete test, or the assertion line}
    ```

A test follows `design_docs/test.md`: in the group's own `mod tests` unless it
needs the Engine loop, in which case name the `tests/` file and module it joins; a
nested `mod <subject>_<behavior>`; a doc comment opening with `Invariant:` and a
plain sentence with no IDs; a `Design Doc:` line naming one ID only when a row is
pinned. An assertion is always-on, constant-time, and names the invariant ID in
its message.

Then end your turn. Do not touch a file until I reply with the numbers to land.

WHEN I REPLY

Land the numbered entries. Before you trust a new test, break the code path it
pins and watch it fail; say which mutation you used, then revert it. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append the attack
table and your entries under `## contracts` in `design_docs/review-edges.md`, each
marked landed, or declined with my reason. Report the results and any test that
would not settle. Don't commit.
````

## 6. contracts — idiom

````
Review the contracts group of the Kavod core for shape: idiomatic, simple, robust
Rust. You did not write it. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/application.rs` (91 production lines, 13 tests), `src/port.rs` (33
production lines, 7 tests), `tests/ports_macro.rs` (4 tests: the `ports!` macro used
from a downstream crate).
Design rows: `APP-CONTEXT`, `APP-EMIT`, `APP-OVERFLOW`, `APP-FUTURE`, `APP-STATE`;
`PORT-SUMS`, `PORT-ROUTING` for its Core half; `TRUST-PURE`; `VERIFY-CONTEXT`.
`PORT-STATE` and the per-Slot Error sum wait on the Environment steps.
Suites: `tests/faults.rs` (`application_fault_matrix`), `tests/conformance.rs`; the
Application fake is `tests/support/recording_app.rs`.
Prior: `review-adversarial.md` — the Context and Port macro tables.
Heads up: G9 in `review-ledger.md` — `APP-FUTURE` is enforced by `Context`'s shape and
no site is named — is open and Devon's decision. Note it; don't re-find it.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER

1. The group's files end to end: production code, then the `mod tests` beneath it.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, then
   the row and the API block of its section. A law is a table row:
   `grep -n '^| A6 |'`. Never read the whole doc.
3. The prior-review sections named under Prior above.

WHAT THE DESIGN FIXES, AND WHAT IT DOES NOT

The design fixes behavior: every record byte, every Environment call and its order,
every exit value, every error precedence, and the names in its API blocks. Nothing
you propose may move those. If you think a behavior is wrong, put it under
`Design notes` in your report and propose nothing.

The design does not fix shape. Derives, visibility, signatures, helper types,
control flow, module boundaries, assertions, and lint attributes are yours to
judge. A change that touches the public API is allowed; mark it, because the
export audit and the wiring decisions are still open.

WHAT TO JUDGE

Go through the group once for each of these. The bar is code a strong Rust
reviewer accepts without comment. "Works" is not the bar.

- Derives. Every public type carries what its users will need: `Debug` always;
  `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Default` wherever the semantics
  allow. Error types implement `Display` and `std::error::Error` with `source()`.
  A missing derive is a finding.
- Visibility. `pub` items no path reaches, `pub(crate)` where `pub(super)` is the
  truth, bounds on a struct that only its impl needs, a module boundary in the
  wrong place.
- Signatures. Tuples that want a named struct, generic soups that want an alias or
  a type, by-value arguments never consumed, lifetimes that elide, `const fn` where
  it is free, `#[must_use]` where dropping the value is a bug.
- Control flow. Nesting that `?`, `let … else`, `matches!`, or an early return
  flattens; a `match` that restores what it replaced; an `expect` or
  `unreachable!` that a type could delete.
- Assertions. Always-on, constant-time, message naming the invariant ID. An
  assertion the type system could make unrepresentable is a finding: the design's
  order is unrepresentable, then asserted, then tested.
- Lints. Run `cargo clippy --all-targets -- -W clippy::pedantic -W clippy::nursery`
  and take every hit in the group's files. Fix it, or suppress it with
  `#[expect(lint, reason = "…")]`. Never `allow`: `expect` fails when the lint
  stops firing, and an earlier round deleted forty-three stale allowances.
  Existing `allow` attributes become `expect`. Hits of one lint with one fix are
  one entry. Format the group's files; hunks elsewhere are another group's.
- Names. The design's glossary is the vocabulary. A name that drifted is a finding.
- Tests, shape only. Setup duplicated across tests that a helper names; `Err(_)`
  where the variant should be named; a group whose name no longer says what it
  holds; a test that pins a derive rather than behavior. Never remove a test that
  pins behavior. Two tests merge only when the merged one asserts everything both
  did.

Not findings: taste without an argument; documentation, which the export audit
owns, unless a comment is wrong; anything marked kept under Prior above unless you
argue against the reason `review-simplification.md` gives. Churn is a cost line, not
a reason to keep a worse shape.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the code instead of describing it; the prose says why. Never restate what the
code already says. If the report is longer than the code it reviews, cut it.

THE REPORT

One entry per finding, ranked: robustness first, then simplification, then lint
and style. Number them I1, I2, …

    ### I3 — {title, one line}
    `path:line`
    {why: one to three sentences}
    ```rust
    {the new shape, complete enough to apply}
    ```
    Cost: {what else moves — tests, fixture, callers}. API: {yes | no}.

Then `Design notes`, if any. Then end your turn. Do not touch a file until I reply
with the numbers to apply.

WHEN I REPLY

Apply the numbered findings in order. Keep every record byte, call order, exit,
and precedence as it was; the matrices in `tests/faults.rs` assert exact call
lists and will tell you if you slipped. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append your entries
under `## contracts` in `design_docs/review-idiom.md`, each marked landed, or declined
with my reason. Report the three commands' results and any finding that fought you.
Don't commit.
````

## 7. environment — edges

````
Review the environment group of the Kavod core for edge cases that no test and no
assertion covers. Real cases, not coverage for its own sake. You did not write
this code. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/environment.rs` (61 production lines, 2 tests), `src/latch.rs` (67
production lines, 10 tests).
Design rows: `ENV-SERIAL`, `ENV-START`, `ENV-ERRORS`, `ENV-LATCH`, `ENV-TIME`,
`ENV-SHUTDOWN`; A4. `ENV-SEPARATION`, `ENV-BOUNDS`, and `VERIFY-LATCH` belong to the
Environment steps.
Suites: `tests/faults.rs` (`environment_fault_matrix`), `tests/harness_contract.rs`;
the Environment fake is `tests/support/scripted_env.rs`.
Prior: `review-adversarial.md` — the Latch table, S3, and the "Environment contract,
as the Engine drives it" table; `review-simplification.md` — "`Latch::take` replaces
the state and restores it" kept.
Heads up: `Latch` has no caller yet. It and its `lib.rs` re-export sit under
`dead_code` and `unused_imports` allowances until the gated Environment steps land,
and its visibility is a Wiring question. Note; don't change.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER — AND STOP BEFORE THE TESTS

1. The group's production code, top to bottom, stopping at `#[cfg(test)]`.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, the
   row, and its section's Notes. A law is a table row: `grep -n '^| A6 |'`. Never
   read the whole doc.

Now write the attack list. Do it before reading a single test: a list written after
the tests comes out shaped like the tests that exist.

THE ATTACK LIST

For every function, every input that sits on an edge and every state a failure
leaves behind. The edges that matter here:

- Boundaries: zero, one, capacity, capacity plus one, `u64::MAX`, `usize::MAX`,
  the smallest `NonZeroUsize`, the empty slice, a record that exactly fits.
- Every enum arm and every error path, and what stands after each failure: the
  next call's view, ownership of what was passed in, what was dropped and when.
- Order: two operations in both orders; a failure at the first position and at
  the last.
- The `io::Write` contract as a hostile sink honors it: `Ok(0)`, a count larger
  than the slice, `Interrupted`, a short write, an error after progress.
- serde corners: a non-object at the top level, an interior newline, a payload
  that fails mid-serialization.
- What a bespoke implementor of a trait in this group can do that the Core does
  not guard against.
- A claim in a design row or Notes paragraph naming this group that nothing
  asserts.

Skip an edge the group cannot reach; don't pad the table.

RESOLVE EACH ATTACK

Now read the tests: the group's own `mod tests` first, then every suite under
`tests/` named above, then `grep -rn` the whole test surface for the behavior.
Read bodies, not names; a test passes without pinning the attack more often than
its name admits. Resolve every attack to one of:

- pinned — the test, with its module.
- by construction — the type or standard-library guarantee.
- derived — the two pinned facts that compose to it.
- asserted — the site.
- open — none of the above.

Only open attacks are findings. A finding that duplicates a test anywhere in
`src/` or `tests/` is a defect in your report, so cite what you searched. Attacks
on the "Considered, not tested" list in `review-adversarial.md` are re-raised only
with a new argument.

For each open attack, read the code path once more and state what the code does.
If the code is wrong, that is a defect; it outranks every gap and goes first.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the test instead of describing it; the prose says what it pins and why
nothing else does. If the report is longer than the code it reviews, cut it.

THE REPORT

First the attack table: one row per attack, its resolution, the name it resolves
to. Then one entry per open attack, defects first, numbered E1, E2, …

    ### E2 — {the attack, one line}
    `path:line` — {what the code does there, one sentence}
    Searched: {modules and files read; the grep}
    Closes with: {test | assertion}
    ```rust
    {the complete test, or the assertion line}
    ```

A test follows `design_docs/test.md`: in the group's own `mod tests` unless it
needs the Engine loop, in which case name the `tests/` file and module it joins; a
nested `mod <subject>_<behavior>`; a doc comment opening with `Invariant:` and a
plain sentence with no IDs; a `Design Doc:` line naming one ID only when a row is
pinned. An assertion is always-on, constant-time, and names the invariant ID in
its message.

Then end your turn. Do not touch a file until I reply with the numbers to land.

WHEN I REPLY

Land the numbered entries. Before you trust a new test, break the code path it
pins and watch it fail; say which mutation you used, then revert it. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append the attack
table and your entries under `## environment` in `design_docs/review-edges.md`, each
marked landed, or declined with my reason. Report the results and any test that
would not settle. Don't commit.
````

## 8. environment — idiom

````
Review the environment group of the Kavod core for shape: idiomatic, simple, robust
Rust. You did not write it. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/environment.rs` (61 production lines, 2 tests), `src/latch.rs` (67
production lines, 10 tests).
Design rows: `ENV-SERIAL`, `ENV-START`, `ENV-ERRORS`, `ENV-LATCH`, `ENV-TIME`,
`ENV-SHUTDOWN`; A4. `ENV-SEPARATION`, `ENV-BOUNDS`, and `VERIFY-LATCH` belong to the
Environment steps.
Suites: `tests/faults.rs` (`environment_fault_matrix`), `tests/harness_contract.rs`;
the Environment fake is `tests/support/scripted_env.rs`.
Prior: `review-adversarial.md` — the Latch table, S3, and the "Environment contract,
as the Engine drives it" table; `review-simplification.md` — "`Latch::take` replaces
the state and restores it" kept.
Heads up: `Latch` has no caller yet. It and its `lib.rs` re-export sit under
`dead_code` and `unused_imports` allowances until the gated Environment steps land,
and its visibility is a Wiring question. Note; don't change.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER

1. The group's files end to end: production code, then the `mod tests` beneath it.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, then
   the row and the API block of its section. A law is a table row:
   `grep -n '^| A6 |'`. Never read the whole doc.
3. The prior-review sections named under Prior above.

WHAT THE DESIGN FIXES, AND WHAT IT DOES NOT

The design fixes behavior: every record byte, every Environment call and its order,
every exit value, every error precedence, and the names in its API blocks. Nothing
you propose may move those. If you think a behavior is wrong, put it under
`Design notes` in your report and propose nothing.

The design does not fix shape. Derives, visibility, signatures, helper types,
control flow, module boundaries, assertions, and lint attributes are yours to
judge. A change that touches the public API is allowed; mark it, because the
export audit and the wiring decisions are still open.

WHAT TO JUDGE

Go through the group once for each of these. The bar is code a strong Rust
reviewer accepts without comment. "Works" is not the bar.

- Derives. Every public type carries what its users will need: `Debug` always;
  `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Default` wherever the semantics
  allow. Error types implement `Display` and `std::error::Error` with `source()`.
  A missing derive is a finding.
- Visibility. `pub` items no path reaches, `pub(crate)` where `pub(super)` is the
  truth, bounds on a struct that only its impl needs, a module boundary in the
  wrong place.
- Signatures. Tuples that want a named struct, generic soups that want an alias or
  a type, by-value arguments never consumed, lifetimes that elide, `const fn` where
  it is free, `#[must_use]` where dropping the value is a bug.
- Control flow. Nesting that `?`, `let … else`, `matches!`, or an early return
  flattens; a `match` that restores what it replaced; an `expect` or
  `unreachable!` that a type could delete.
- Assertions. Always-on, constant-time, message naming the invariant ID. An
  assertion the type system could make unrepresentable is a finding: the design's
  order is unrepresentable, then asserted, then tested.
- Lints. Run `cargo clippy --all-targets -- -W clippy::pedantic -W clippy::nursery`
  and take every hit in the group's files. Fix it, or suppress it with
  `#[expect(lint, reason = "…")]`. Never `allow`: `expect` fails when the lint
  stops firing, and an earlier round deleted forty-three stale allowances.
  Existing `allow` attributes become `expect`. Hits of one lint with one fix are
  one entry. Format the group's files; hunks elsewhere are another group's.
- Names. The design's glossary is the vocabulary. A name that drifted is a finding.
- Tests, shape only. Setup duplicated across tests that a helper names; `Err(_)`
  where the variant should be named; a group whose name no longer says what it
  holds; a test that pins a derive rather than behavior. Never remove a test that
  pins behavior. Two tests merge only when the merged one asserts everything both
  did.

Not findings: taste without an argument; documentation, which the export audit
owns, unless a comment is wrong; anything marked kept under Prior above unless you
argue against the reason `review-simplification.md` gives. Churn is a cost line, not
a reason to keep a worse shape.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the code instead of describing it; the prose says why. Never restate what the
code already says. If the report is longer than the code it reviews, cut it.

THE REPORT

One entry per finding, ranked: robustness first, then simplification, then lint
and style. Number them I1, I2, …

    ### I3 — {title, one line}
    `path:line`
    {why: one to three sentences}
    ```rust
    {the new shape, complete enough to apply}
    ```
    Cost: {what else moves — tests, fixture, callers}. API: {yes | no}.

Then `Design notes`, if any. Then end your turn. Do not touch a file until I reply
with the numbers to apply.

WHEN I REPLY

Apply the numbered findings in order. Keep every record byte, call order, exit,
and precedence as it was; the matrices in `tests/faults.rs` assert exact call
lists and will tell you if you slipped. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append your entries
under `## environment` in `design_docs/review-idiom.md`, each marked landed, or declined
with my reason. Report the three commands' results and any finding that fought you.
Don't commit.
````

## 9. record — edges

````
Review the record group of the Kavod core for edge cases that no test and no
assertion covers. Real cases, not coverage for its own sake. You did not write
this code. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/engine/record.rs` (419 production lines, 72 tests);
`tests/compile_fail.rs` driving fourteen compile-fail cases and one legal case under
`tests/grammar_fixture/cases/`; `tests/grammar_fixture/src/lib.rs`, the
reconstruction they compile inside.
Design rows: `RUN-GRAMMAR`, `RUN-ENFORCEMENT`, `RUN-RECORDS`, `RUN-INDEX`,
`RUN-CHECKPOINT`; `ASSERT-INVARIANTS`; `VERIFY-GRAMMAR`; A1, A3, A5. The `JRN-*` rows
as consumed.
Suites: `tests/golden_journal.rs` for record bytes; `tests/faults.rs`
(`journal_fault_matrix`) per record kind.
Prior: `review-adversarial.md` — the "Record grammar and Engine" table and S1, with
the derived fact that `CommandsDispatched` cannot reach the record bound;
`review-simplification.md` — S3 and S4 landed; "`close` returns a tuple error" and
"`accept_event` and `effects` carry the same allowance" kept.
Heads up: the fixture `include!`s this file. A change to its `use` lines, or to any
item it names from `super::` — `CoreError`, `EnvironmentOperation`, `FatalCause` and
its `environment` constructor — is mirrored in `tests/grammar_fixture/src/lib.rs`,
which also carries hand copies of `BoundedBuffer` and `EventIndex`. Expectations
regenerate with `TRYBUILD=overwrite cargo test --test compile_fail`; read every
regenerated `.stderr` and confirm each failure still reaches the grammar, not privacy.
An `#[expect]` in this file is compiled inside the fixture too; where the lint does
not fire there, the unmet expectation is a warning in trybuild's stderr.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER — AND STOP BEFORE THE TESTS

1. The group's production code, top to bottom, stopping at `#[cfg(test)]`.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, the
   row, and its section's Notes. A law is a table row: `grep -n '^| A6 |'`. Never
   read the whole doc.

Now write the attack list. Do it before reading a single test: a list written after
the tests comes out shaped like the tests that exist.

THE ATTACK LIST

For every function, every input that sits on an edge and every state a failure
leaves behind. The edges that matter here:

- Boundaries: zero, one, capacity, capacity plus one, `u64::MAX`, `usize::MAX`,
  the smallest `NonZeroUsize`, the empty slice, a record that exactly fits.
- Every enum arm and every error path, and what stands after each failure: the
  next call's view, ownership of what was passed in, what was dropped and when.
- Order: two operations in both orders; a failure at the first position and at
  the last.
- The `io::Write` contract as a hostile sink honors it: `Ok(0)`, a count larger
  than the slice, `Interrupted`, a short write, an error after progress.
- serde corners: a non-object at the top level, an interior newline, a payload
  that fails mid-serialization.
- What a bespoke implementor of a trait in this group can do that the Core does
  not guard against.
- A claim in a design row or Notes paragraph naming this group that nothing
  asserts.

Skip an edge the group cannot reach; don't pad the table.

RESOLVE EACH ATTACK

Now read the tests: the group's own `mod tests` first, then every suite under
`tests/` named above, then `grep -rn` the whole test surface for the behavior.
Read bodies, not names; a test passes without pinning the attack more often than
its name admits. Resolve every attack to one of:

- pinned — the test, with its module.
- by construction — the type or standard-library guarantee.
- derived — the two pinned facts that compose to it.
- asserted — the site.
- open — none of the above.

Only open attacks are findings. A finding that duplicates a test anywhere in
`src/` or `tests/` is a defect in your report, so cite what you searched. Attacks
on the "Considered, not tested" list in `review-adversarial.md` are re-raised only
with a new argument.

For each open attack, read the code path once more and state what the code does.
If the code is wrong, that is a defect; it outranks every gap and goes first.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the test instead of describing it; the prose says what it pins and why
nothing else does. If the report is longer than the code it reviews, cut it.

THE REPORT

First the attack table: one row per attack, its resolution, the name it resolves
to. Then one entry per open attack, defects first, numbered E1, E2, …

    ### E2 — {the attack, one line}
    `path:line` — {what the code does there, one sentence}
    Searched: {modules and files read; the grep}
    Closes with: {test | assertion}
    ```rust
    {the complete test, or the assertion line}
    ```

A test follows `design_docs/test.md`: in the group's own `mod tests` unless it
needs the Engine loop, in which case name the `tests/` file and module it joins; a
nested `mod <subject>_<behavior>`; a doc comment opening with `Invariant:` and a
plain sentence with no IDs; a `Design Doc:` line naming one ID only when a row is
pinned. An assertion is always-on, constant-time, and names the invariant ID in
its message.

Then end your turn. Do not touch a file until I reply with the numbers to land.

WHEN I REPLY

Land the numbered entries. Before you trust a new test, break the code path it
pins and watch it fail; say which mutation you used, then revert it. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append the attack
table and your entries under `## record` in `design_docs/review-edges.md`, each
marked landed, or declined with my reason. Report the results and any test that
would not settle. Don't commit.
````

## 10. record — idiom

````
Review the record group of the Kavod core for shape: idiomatic, simple, robust
Rust. You did not write it. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/engine/record.rs` (419 production lines, 72 tests);
`tests/compile_fail.rs` driving fourteen compile-fail cases and one legal case under
`tests/grammar_fixture/cases/`; `tests/grammar_fixture/src/lib.rs`, the
reconstruction they compile inside.
Design rows: `RUN-GRAMMAR`, `RUN-ENFORCEMENT`, `RUN-RECORDS`, `RUN-INDEX`,
`RUN-CHECKPOINT`; `ASSERT-INVARIANTS`; `VERIFY-GRAMMAR`; A1, A3, A5. The `JRN-*` rows
as consumed.
Suites: `tests/golden_journal.rs` for record bytes; `tests/faults.rs`
(`journal_fault_matrix`) per record kind.
Prior: `review-adversarial.md` — the "Record grammar and Engine" table and S1, with
the derived fact that `CommandsDispatched` cannot reach the record bound;
`review-simplification.md` — S3 and S4 landed; "`close` returns a tuple error" and
"`accept_event` and `effects` carry the same allowance" kept.
Heads up: the fixture `include!`s this file. A change to its `use` lines, or to any
item it names from `super::` — `CoreError`, `EnvironmentOperation`, `FatalCause` and
its `environment` constructor — is mirrored in `tests/grammar_fixture/src/lib.rs`,
which also carries hand copies of `BoundedBuffer` and `EventIndex`. Expectations
regenerate with `TRYBUILD=overwrite cargo test --test compile_fail`; read every
regenerated `.stderr` and confirm each failure still reaches the grammar, not privacy.
An `#[expect]` in this file is compiled inside the fixture too; where the lint does
not fire there, the unmet expectation is a warning in trybuild's stderr.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER

1. The group's files end to end: production code, then the `mod tests` beneath it.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, then
   the row and the API block of its section. A law is a table row:
   `grep -n '^| A6 |'`. Never read the whole doc.
3. The prior-review sections named under Prior above.

WHAT THE DESIGN FIXES, AND WHAT IT DOES NOT

The design fixes behavior: every record byte, every Environment call and its order,
every exit value, every error precedence, and the names in its API blocks. Nothing
you propose may move those. If you think a behavior is wrong, put it under
`Design notes` in your report and propose nothing.

The design does not fix shape. Derives, visibility, signatures, helper types,
control flow, module boundaries, assertions, and lint attributes are yours to
judge. A change that touches the public API is allowed; mark it, because the
export audit and the wiring decisions are still open.

WHAT TO JUDGE

Go through the group once for each of these. The bar is code a strong Rust
reviewer accepts without comment. "Works" is not the bar.

- Derives. Every public type carries what its users will need: `Debug` always;
  `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Default` wherever the semantics
  allow. Error types implement `Display` and `std::error::Error` with `source()`.
  A missing derive is a finding.
- Visibility. `pub` items no path reaches, `pub(crate)` where `pub(super)` is the
  truth, bounds on a struct that only its impl needs, a module boundary in the
  wrong place.
- Signatures. Tuples that want a named struct, generic soups that want an alias or
  a type, by-value arguments never consumed, lifetimes that elide, `const fn` where
  it is free, `#[must_use]` where dropping the value is a bug.
- Control flow. Nesting that `?`, `let … else`, `matches!`, or an early return
  flattens; a `match` that restores what it replaced; an `expect` or
  `unreachable!` that a type could delete.
- Assertions. Always-on, constant-time, message naming the invariant ID. An
  assertion the type system could make unrepresentable is a finding: the design's
  order is unrepresentable, then asserted, then tested.
- Lints. Run `cargo clippy --all-targets -- -W clippy::pedantic -W clippy::nursery`
  and take every hit in the group's files. Fix it, or suppress it with
  `#[expect(lint, reason = "…")]`. Never `allow`: `expect` fails when the lint
  stops firing, and an earlier round deleted forty-three stale allowances.
  Existing `allow` attributes become `expect`. Hits of one lint with one fix are
  one entry. Format the group's files; hunks elsewhere are another group's.
- Names. The design's glossary is the vocabulary. A name that drifted is a finding.
- Tests, shape only. Setup duplicated across tests that a helper names; `Err(_)`
  where the variant should be named; a group whose name no longer says what it
  holds; a test that pins a derive rather than behavior. Never remove a test that
  pins behavior. Two tests merge only when the merged one asserts everything both
  did.

Not findings: taste without an argument; documentation, which the export audit
owns, unless a comment is wrong; anything marked kept under Prior above unless you
argue against the reason `review-simplification.md` gives. Churn is a cost line, not
a reason to keep a worse shape.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the code instead of describing it; the prose says why. Never restate what the
code already says. If the report is longer than the code it reviews, cut it.

THE REPORT

One entry per finding, ranked: robustness first, then simplification, then lint
and style. Number them I1, I2, …

    ### I3 — {title, one line}
    `path:line`
    {why: one to three sentences}
    ```rust
    {the new shape, complete enough to apply}
    ```
    Cost: {what else moves — tests, fixture, callers}. API: {yes | no}.

Then `Design notes`, if any. Then end your turn. Do not touch a file until I reply
with the numbers to apply.

WHEN I REPLY

Apply the numbered findings in order. Keep every record byte, call order, exit,
and precedence as it was; the matrices in `tests/faults.rs` assert exact call
lists and will tell you if you slipped. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append your entries
under `## record` in `design_docs/review-idiom.md`, each marked landed, or declined
with my reason. Report the three commands' results and any finding that fought you.
Don't commit.
````

## 11. engine — edges

````
Review the engine group of the Kavod core for edge cases that no test and no
assertion covers. Real cases, not coverage for its own sake. You did not write
this code. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/engine/engine.rs` (261 production lines, 36 tests), `src/engine/mod.rs`
(12 lines), `src/lib.rs` (21 lines).
Design rows: `RUN-SERIAL`, `RUN-FINALIZE`, `RUN-CHECKPOINT`, `DET-RUN`;
`BOUND-NONZERO`; `VERIFY-FAULTS`, `VERIFY-JOURNAL`, `VERIFY-CONFORMANCE`; A2, A4, A9.
`ENV-START`, `ENV-SHUTDOWN`, and `APP-OVERFLOW` as consumed. `CRATE-EXPORTS` and the
export policy in `lib.rs` wait on Wiring.
Suites: `tests/faults.rs`, `tests/golden_journal.rs`, `tests/conformance.rs`,
`tests/harness_contract.rs`, and all of `tests/support/`.
Prior: `review-adversarial.md` — the "Record grammar and Engine" table, "The
compositions the plan names", "Considered, not tested", and "Observations, no
action"; `review-simplification.md` — S1 and S2 landed; "`turn` clears the batch
twice", "`engine.rs` defines its public exit types below the impl", and
"`accept_event` and `effects` carry the same allowance" kept.
Heads up: `review-ledger.md` lists gaps G1–G14. Ten are citation edits in test doc
comments, G3/G9/G12/G13 await Devon's decision, G13 goes to C57. They are known;
don't re-find them.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER — AND STOP BEFORE THE TESTS

1. The group's production code, top to bottom, stopping at `#[cfg(test)]`.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, the
   row, and its section's Notes. A law is a table row: `grep -n '^| A6 |'`. Never
   read the whole doc.

Now write the attack list. Do it before reading a single test: a list written after
the tests comes out shaped like the tests that exist.

THE ATTACK LIST

For every function, every input that sits on an edge and every state a failure
leaves behind. The edges that matter here:

- Boundaries: zero, one, capacity, capacity plus one, `u64::MAX`, `usize::MAX`,
  the smallest `NonZeroUsize`, the empty slice, a record that exactly fits.
- Every enum arm and every error path, and what stands after each failure: the
  next call's view, ownership of what was passed in, what was dropped and when.
- Order: two operations in both orders; a failure at the first position and at
  the last.
- The `io::Write` contract as a hostile sink honors it: `Ok(0)`, a count larger
  than the slice, `Interrupted`, a short write, an error after progress.
- serde corners: a non-object at the top level, an interior newline, a payload
  that fails mid-serialization.
- What a bespoke implementor of a trait in this group can do that the Core does
  not guard against.
- A claim in a design row or Notes paragraph naming this group that nothing
  asserts.

Skip an edge the group cannot reach; don't pad the table.

RESOLVE EACH ATTACK

Now read the tests: the group's own `mod tests` first, then every suite under
`tests/` named above, then `grep -rn` the whole test surface for the behavior.
Read bodies, not names; a test passes without pinning the attack more often than
its name admits. Resolve every attack to one of:

- pinned — the test, with its module.
- by construction — the type or standard-library guarantee.
- derived — the two pinned facts that compose to it.
- asserted — the site.
- open — none of the above.

Only open attacks are findings. A finding that duplicates a test anywhere in
`src/` or `tests/` is a defect in your report, so cite what you searched. Attacks
on the "Considered, not tested" list in `review-adversarial.md` are re-raised only
with a new argument.

For each open attack, read the code path once more and state what the code does.
If the code is wrong, that is a defect; it outranks every gap and goes first.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the test instead of describing it; the prose says what it pins and why
nothing else does. If the report is longer than the code it reviews, cut it.

THE REPORT

First the attack table: one row per attack, its resolution, the name it resolves
to. Then one entry per open attack, defects first, numbered E1, E2, …

    ### E2 — {the attack, one line}
    `path:line` — {what the code does there, one sentence}
    Searched: {modules and files read; the grep}
    Closes with: {test | assertion}
    ```rust
    {the complete test, or the assertion line}
    ```

A test follows `design_docs/test.md`: in the group's own `mod tests` unless it
needs the Engine loop, in which case name the `tests/` file and module it joins; a
nested `mod <subject>_<behavior>`; a doc comment opening with `Invariant:` and a
plain sentence with no IDs; a `Design Doc:` line naming one ID only when a row is
pinned. An assertion is always-on, constant-time, and names the invariant ID in
its message.

Then end your turn. Do not touch a file until I reply with the numbers to land.

WHEN I REPLY

Land the numbered entries. Before you trust a new test, break the code path it
pins and watch it fail; say which mutation you used, then revert it. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append the attack
table and your entries under `## engine` in `design_docs/review-edges.md`, each
marked landed, or declined with my reason. Report the results and any test that
would not settle. Don't commit.
````

## 12. engine — idiom

````
Review the engine group of the Kavod core for shape: idiomatic, simple, robust
Rust. You did not write it. Report first; edit nothing until I reply.

YOUR GROUP

Files: `src/engine/engine.rs` (261 production lines, 36 tests), `src/engine/mod.rs`
(12 lines), `src/lib.rs` (21 lines).
Design rows: `RUN-SERIAL`, `RUN-FINALIZE`, `RUN-CHECKPOINT`, `DET-RUN`;
`BOUND-NONZERO`; `VERIFY-FAULTS`, `VERIFY-JOURNAL`, `VERIFY-CONFORMANCE`; A2, A4, A9.
`ENV-START`, `ENV-SHUTDOWN`, and `APP-OVERFLOW` as consumed. `CRATE-EXPORTS` and the
export policy in `lib.rs` wait on Wiring.
Suites: `tests/faults.rs`, `tests/golden_journal.rs`, `tests/conformance.rs`,
`tests/harness_contract.rs`, and all of `tests/support/`.
Prior: `review-adversarial.md` — the "Record grammar and Engine" table, "The
compositions the plan names", "Considered, not tested", and "Observations, no
action"; `review-simplification.md` — S1 and S2 landed; "`turn` clears the batch
twice", "`engine.rs` defines its public exit types below the impl", and
"`accept_event` and `effects` carry the same allowance" kept.
Heads up: `review-ledger.md` lists gaps G1–G14. Ten are citation edits in test doc
comments, G3/G9/G12/G13 await Devon's decision, G13 goes to C57. They are known;
don't re-find them.
The `Heads up` line is not optional.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER

1. The group's files end to end: production code, then the `mod tests` beneath it.
2. Each design ID listed above: `grep -n '`THE-ID`' design_docs/design-v12.md`, then
   the row and the API block of its section. A law is a table row:
   `grep -n '^| A6 |'`. Never read the whole doc.
3. The prior-review sections named under Prior above.

WHAT THE DESIGN FIXES, AND WHAT IT DOES NOT

The design fixes behavior: every record byte, every Environment call and its order,
every exit value, every error precedence, and the names in its API blocks. Nothing
you propose may move those. If you think a behavior is wrong, put it under
`Design notes` in your report and propose nothing.

The design does not fix shape. Derives, visibility, signatures, helper types,
control flow, module boundaries, assertions, and lint attributes are yours to
judge. A change that touches the public API is allowed; mark it, because the
export audit and the wiring decisions are still open.

WHAT TO JUDGE

Go through the group once for each of these. The bar is code a strong Rust
reviewer accepts without comment. "Works" is not the bar.

- Derives. Every public type carries what its users will need: `Debug` always;
  `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Default` wherever the semantics
  allow. Error types implement `Display` and `std::error::Error` with `source()`.
  A missing derive is a finding.
- Visibility. `pub` items no path reaches, `pub(crate)` where `pub(super)` is the
  truth, bounds on a struct that only its impl needs, a module boundary in the
  wrong place.
- Signatures. Tuples that want a named struct, generic soups that want an alias or
  a type, by-value arguments never consumed, lifetimes that elide, `const fn` where
  it is free, `#[must_use]` where dropping the value is a bug.
- Control flow. Nesting that `?`, `let … else`, `matches!`, or an early return
  flattens; a `match` that restores what it replaced; an `expect` or
  `unreachable!` that a type could delete.
- Assertions. Always-on, constant-time, message naming the invariant ID. An
  assertion the type system could make unrepresentable is a finding: the design's
  order is unrepresentable, then asserted, then tested.
- Lints. Run `cargo clippy --all-targets -- -W clippy::pedantic -W clippy::nursery`
  and take every hit in the group's files. Fix it, or suppress it with
  `#[expect(lint, reason = "…")]`. Never `allow`: `expect` fails when the lint
  stops firing, and an earlier round deleted forty-three stale allowances.
  Existing `allow` attributes become `expect`. Hits of one lint with one fix are
  one entry. Format the group's files; hunks elsewhere are another group's.
- Names. The design's glossary is the vocabulary. A name that drifted is a finding.
- Tests, shape only. Setup duplicated across tests that a helper names; `Err(_)`
  where the variant should be named; a group whose name no longer says what it
  holds; a test that pins a derive rather than behavior. Never remove a test that
  pins behavior. Two tests merge only when the merged one asserts everything both
  did.

Not findings: taste without an argument; documentation, which the export audit
owns, unless a comment is wrong; anything marked kept under Prior above unless you
argue against the reason `review-simplification.md` gives. Churn is a cost line, not
a reason to keep a worse shape.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the code instead of describing it; the prose says why. Never restate what the
code already says. If the report is longer than the code it reviews, cut it.

THE REPORT

One entry per finding, ranked: robustness first, then simplification, then lint
and style. Number them I1, I2, …

    ### I3 — {title, one line}
    `path:line`
    {why: one to three sentences}
    ```rust
    {the new shape, complete enough to apply}
    ```
    Cost: {what else moves — tests, fixture, callers}. API: {yes | no}.

Then `Design notes`, if any. Then end your turn. Do not touch a file until I reply
with the numbers to apply.

WHEN I REPLY

Apply the numbered findings in order. Keep every record byte, call order, exit,
and precedence as it was; the matrices in `tests/faults.rs` assert exact call
lists and will tell you if you slipped. Then:

    cargo test
    cargo clippy --all-targets -- -D warnings
    cargo fmt --check

A `fmt` hunk outside the group's files is not yours; leave it. Append your entries
under `## engine` in `design_docs/review-idiom.md`, each marked landed, or declined
with my reason. Report the three commands' results and any finding that fought you.
Don't commit.
````

## 13. Synthesis

````
Synthesize the twelve Kavod core reviews into one pass over the whole crate. You
did not run them. Report first; edit nothing until I reply.

BEFORE YOU START

    cargo test
    cargo clippy --all-targets -- -D warnings

Both green, or stop and tell me.

READ FIRST, IN THIS ORDER

1. `design_docs/review-edges.md` and `design_docs/review-idiom.md`, every section,
   landed and declined entries both.
2. The production code of every file in `src/`, stopping at each `#[cfg(test)]`,
   `lib.rs` last. About 1,300 lines; one sitting.

The design fixes behavior — every record byte, Environment call and its order, exit
value, error precedence, and API-block name — and nothing you propose moves those.

WHAT ONLY THIS PASS CAN SEE

- A policy that surfaced in two or more groups: derives, error-trait impls,
  `expect` over `allow`, pedantic and nursery under `[lints.clippy]` in
  `Cargo.toml`, a visibility convention. One decision each, applied everywhere,
  with the sites still missing it.
- A finding landed in one group that a later group's pass never saw because it
  ran first; a finding declined in one group and landed in another.
- Helpers that exist once per file and want one home. Seven error-shaped types —
  `BuildError`, `JournalBuildError`, `JournalError`, `JournalFatal`, `FatalCause`,
  `CoreError`, `EnvironmentFatal` — say whether the hierarchy is right.
- Names across module boundaries: one idea under two names, two ideas under one.
- The export surface in `lib.rs` as a whole, given wiring is open.
- Edges between groups: a boundary each group honors alone and the composition
  breaks. The compositions listed in `review-adversarial.md` are pinned; find the
  ones it did not list.
- `cargo clippy --all-targets -- -W clippy::pedantic -W clippy::nursery` and
  `cargo fmt --check` on the whole crate.

HOW TO WRITE

Every sentence carries a fact or a decision. No preamble, no summary of what you
read, no praise, no hedging, no closing offer. One idea per sentence, plain words.
Show the code instead of describing it; the prose says why. If the report is
longer than the change it proposes, cut it.

THE REPORT

Numbered S1, S2, …, each in the shape of the group reports: a shape entry carries
`path:line`, why, the code, and a cost line; an edge entry carries the attack, what
the code does, what was searched, and the test or assertion. Then the deferred
list: design notes for the Wiring close, and API-touching changes for the export
audit, each naming the group entry it came from. Then end your turn.

WHEN I REPLY

Land the numbered entries, run the three commands, and write
`design_docs/review-synthesis.md`: the entries as landed or declined, and the
deferred list. Report the results. Don't commit.
````
