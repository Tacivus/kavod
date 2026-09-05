# Step prompt

One template, every step. Replace `{STEP}` with the step ID (`C4`) and `{NOTES}`
with anything specific to today, or delete that line. Everything conditional is
keyed to notation the step block already carries, so the session picks its own rules
out of the doc.

Continuing in a session that has already run a step, this shrinks to: *Next step:
{STEP}. Same rules.*

````
Implement step {STEP} of the Kavod core build. Nothing else.

READ FIRST, IN THIS ORDER

1. `design_docs/impl-steps.md`: the rules block at the top, then the `## Part` header
   above {STEP} and any prose under it before the first `###` — some parts state a
   standing rule once and every step under them inherits it — then the `### {STEP} ·`
   block itself.
2. Every ID the step cites. Find each with `grep -n '`THE-ID`' design_docs/design-v12.md`
   and work from the table row you land on. That row is the authority for the
   behavior; the step text is a summary of it, not a replacement.
3. `design_docs/impl-plan-v12.md` only where the step block points you there.

Never read design-v12.md or impl-plan-v12.md end to end — they are over 100KB each.
Grep for what the step names.

HOW TO READ THE STEP BLOCK

Its notation is load-bearing. Take each mark literally:

- `⛔` in the heading — the step is gated on the nine wiring decisions. Read section
  1a of `impl-plan-v12.md` and confirm with me that they're approved before writing
  any code. If the approval isn't recorded anywhere, stop and ask.
- `(~N lines)` — a sanity check on the production code, not a target, and not a cap
  on tests. If the non-test code lands far over it, you've taken on something the
  step didn't ask for; re-read the block.
- **Create** `path` — that file does not exist yet. If it does, stop and tell me,
  because something earlier went wrong.
- **Edit** `path` — add to what's there. Existing items in that file are finished
  work and stay as they are.
- **Copy … from probe Pn** — open that probe in section 2 of `impl-plan-v12.md` and
  follow its shape. It is compiling code that already settled the hard part; the step
  says "copy" rather than "write" for a reason.
- `Write:` — the complete list of items this step adds. Nothing else gets added.
- `Tests:` — required, and a floor rather than the whole job. See HARDENING.
- `Tests (representative):` — the same, and the prose above the list names further
  coverage you owe by description; write it, following the same naming pattern.
- "tests only" in the heading or body — no production code changes at all. If a test
  seems to need one, that's a finding to report, not a change to make.
- A path under `tests/` — a separate crate that sees only the public API. If a test
  needs something the crate doesn't export, that's a finding to report, not a reason
  to widen visibility.
- `Heads up:` — a trap someone already fell into. Not optional advice.

RULES THAT OVERRIDE YOUR DEFAULTS

- Scope is the step's production code. Don't start the next step, tidy neighboring
  code, or add docs the step didn't ask for. Tests are the deliberate exception —
  see HARDENING.
- Forward-only. Later steps extend earlier files; they never rewrite them. If
  finishing {STEP} looks like it needs a change to code an earlier step wrote, stop
  and tell me — that's a bug in the plan, not an invitation to refactor.
- Tests follow `design_docs/test.md` exactly: a `#[cfg(test)] mod tests` in the file
  under test, one nested `mod <subject>_<behavior>` per group, every test
  doc-commented with an `Invariant:` sentence in plain English that someone who has
  never opened the design doc understands, containing no IDs; when the test pins a
  design rule, a second `Design Doc:` line names the ID and nothing else. The step
  lists tests as `module::test_name (ID)` — that ID is the `Design Doc:` line
  verbatim, and the `Invariant:` sentence is yours to write.
- Assertions are always-on: `assert!` / `assert_eq!` / `expect`, never
  `debug_assert!`. The panic message names the invariant it protects. Add them
  liberally anywhere they stay constant-time.
- A new `src/` file means exactly two lines in `lib.rs`: `mod x;` and a `pub use` of
  its public items. Module files stay private; every public item is re-exported flat
  at the crate root.
- Prefer associated functions over free functions.
- A directory module is `foo/mod.rs` holding only `mod` and `pub use` wiring, with
  the real code in `foo/thing.rs`.

HARDENING — THE LISTED TESTS ARE A FLOOR

Rule 4 of `impl-steps.md`. The listed tests pin what the design says; they are not
the measure of what this code has to survive. After they pass, write every other test
that pins real behavior in what you just built: boundaries at zero, one, capacity and
one past it; every arm of every enum; every error path; every order two operations
can happen in; and what still holds after a failure. The goal is a system that
survives contact with reality, not one that matches a document — so treat an
uncovered edge case as work, not as someone else's step.

Three things keep it from sprawling. Search the later steps in `impl-steps.md` before
adding, and skip anything already listed there. Stay inside the subject this step
builds — a gap somewhere else is a finding to report, not a test to write here. And
give an unlisted test the `Invariant:` line alone; most pin behavior no design row
names, and inventing a citation is worse than having none.

When one of these tests fails, that is why you wrote it. Fix the step's own code. If
the failure instead contradicts the design or something an earlier step wrote, stop
and report it — that's worth more than the step.

DONE MEANS

    cargo test
    cargo clippy --all-targets -- -D warnings

both green, every listed test present and passing under the name it gives, and the
hardening tests written and passing too. Then tick {STEP}'s box in the Progress list
in `impl-steps.md`, commit with the subject line `{STEP}: <what the step built>`, and
stop.

IF THE STEP FIGHTS YOU

Borrow-checker wall, a macro that won't parse, a test that won't settle — read the
risk list (section 5) of `impl-plan-v12.md` before improvising. The symptom, the
fallback, and the probe that already solved it are likely there. If they aren't, say
so and stop; don't invent a way around it.

REPORT BACK

Files touched; the listed tests by name; the hardening tests by name, each with the
edge case it covers, called out separately; the result of both commands; and anything
in the step's text that turned out to be wrong, ambiguous, or impossible as written.

{NOTES}
````
