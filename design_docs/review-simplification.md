# Round 3 — Simplification

The Core reread for shape, with the suite the two earlier rounds audited as the net:
nesting a uniform error type would flatten, helpers that exist once, names that drifted
from the design's vocabulary, and what clippy cannot see. Build-step rule 2 is
suspended for this round and no other. Public documentation stays out of scope.

**Result: five changes, all behavior-preserving, and six shapes considered and kept.**
The largest is the run loop: a hundred-line ladder of nested matches whose every
failure arm does the same thing. Two are the items Round 1 carried here. Two are
duplicated three-line blocks that a helper names. Nothing here moves a rule, changes a
record byte, or reorders an Environment call.

## Changes

### S1 — the run loop is one ladder with eight identical rungs

`src/engine/engine.rs:144`–`248`. `run` is a hundred lines, and after the start check
every failure arm — eight of them — is `Self::finalize(state, cause,
Finalization::Unconsumed(env))`, differing only in whether it first wraps a
`JournalFatal`. The shape hides the one fact that matters: there are exactly three ways
to finalize, and they are the three arms of `Finalization`.

Change: a private `drive` helper owns the loop and returns
`Result<Certificate<W, StopPending>, FatalCause<..>>`. Inside it every transition is a
`?`, with `.map_err(FatalCause::Journal)` at the three transitions that return a bare
`JournalFatal`. `run` then has one `finalize` site per arm: `StartFailed` after
`start`, `Unconsumed(env)` after `drive`, `Retained(quiescence)` after `close`.

```rust
fn drive(app, state, env: &mut E, batch, certificate: Certificate<W, Initial>)
    -> Result<Certificate<W, StopPending>, FatalCause<A::Error, E::Error>>
{
    let mut certificate = certificate.run_started().map_err(FatalCause::Journal)?;
    let mut accepted_event = None;
    loop {
        match Self::turn(app, state, accepted_event.as_ref(), batch, certificate)? {
            ClassifiedTurn::Continue(classified) => {
                let (next, event) = Self::effects(classified, env, batch)?
                    .complete_continue()
                    .map_err(FatalCause::Journal)?
                    .accept_event(env)?;
                accepted_event = Some(event);
                certificate = next;
            }
            ClassifiedTurn::Stop(classified) => {
                return Self::effects(classified, env, batch)?
                    .request_stop()
                    .map_err(FatalCause::Journal);
            }
        }
    }
}
```

The call order is unchanged and so is every arm: `drive` borrows the Environment, so
its failure finalizes `Unconsumed`; `close` consumes it, so its failure finalizes
`Retained`. `pending_event` becomes `accepted_event` — "pending" is the latch's word
in the design, and this Event is accepted. A public `From<JournalFatal>` impl would let
the three `map_err` calls vanish, but it is public API and belongs to the export audit;
the `map_err` form costs three short lines and adds nothing to the surface.

Roughly a hundred lines become forty. No test calls `run` except through `Engine::new`,
and `turn` and `finalize` keep their signatures. **Landed.**

### S2 — `turn` asserts one invariant at two sites

`src/engine/engine.rs:74` and `:87`, carried from Round 1. The `assert_eq!` on
index-versus-Event is followed by an `expect` on the same `Option`. Selecting the
handler by matching the `Option` keeps the index check as the single assertion and
removes the `expect`:

```rust
let answer = match event {
    None => app.on_start(state, &mut context),
    Some(event) => app.on_event(state, event, &mut context),
};
```

The message stays as it is; no test compares it. Twelve assertion sites become eleven.
**Landed.**

### S3 — the `TurnCompleted` outcome comes from the payload

`src/engine/record.rs:189`, C4 from Round 1. `commit` takes `outcome:
Option<TurnOutcome>` beside the payload, so the two `TurnCompleted` callers write the
outcome twice and the five others pass `None` by hand. The design's mechanism says one
value supplies both.

Change: `RecordPayload` gains `fn outcome(&self) -> Option<TurnOutcome> { None }`,
`TurnCompletedRecord` returns `Some(self.outcome)`, and `commit` builds `JournalFatal`
from `R::KIND` and `payload.outcome()`. Seven call sites lose an argument; a kind/outcome
mismatch is no longer expressible in-module. No test calls `commit` directly; one
compile-fail case does, `independent_commands_dispatched`, whose attack passed the old
second argument. The attack now matches the new signature so that privacy remains its
only error, and its expectation was regenerated and reviewed. **Landed.**

### S4 — `record.rs` spells `super::` twenty times and builds `EnvironmentFatal` four times

`src/engine/record.rs`, throughout the transitions. Every `FatalCause`,
`EnvironmentFatal`, `EnvironmentOperation`, and `CoreError` is written with a `super::`
prefix, and four sites build the same three-line
`FatalCause::Environment(EnvironmentFatal { error, operation })`.

Change: `use super::{CoreError, EnvironmentOperation, FatalCause};` at the top —
`EnvironmentFatal` itself is no longer named once the constructor exists — and one associated constructor, `FatalCause::environment(error, operation)`,
`pub(super)` in `engine.rs`, so a site reads
`FatalCause::environment(error, EnvironmentOperation::NextEvent)`.

The compile-fail fixture reconstructs `FatalCause` by hand, so its copy gains the same
six-line constructor; the import already resolves there, because the fixture defines
those items in the module `record` sits under. **Landed.**

### S5 — the Journal poisons at four sites with the same three lines

`src/journal.rs:79`, `:123`, `:130`, `:141`. Each sink failure sets `poisoned` and
builds `Sink { operation, error }`. One helper names the act the design names:

```rust
fn poison(&mut self, operation: SinkOperation, error: io::Error) -> JournalError {
    self.poisoned = true;
    JournalError::Sink { operation, error }
}
```

The flush site becomes `.map_err(|error| self.poison(SinkOperation::Flush, error))`
and the three write arms `return Err(self.poison(SinkOperation::Write, ..))`. The
borrow story is unchanged: each site already writes `self.poisoned` at that point.
**Landed.**

## Considered and kept

- **`close` returns a tuple error** under a `type_complexity` allowance. A struct
  naming the retained quiescence would read better and drop the allowance, but seven
  tests and the fixture destructure the tuple. Not worth the churn for a name.
- **`accept_event` and `effects` carry the same allowance.** A crate-private alias for
  `FatalCause<A::Error, E::Error>` would shorten five signatures and probably drop them,
  at the cost of hiding what the type is in the one file that spells it out. Kept.
- **`Latch::take` replaces the state and restores it** for the `Empty` and `Closed`
  arms. Every shape I tried either introduced an `unreachable!` or matched twice. The
  current one is total and reads in order. Kept.
- **`encode_raw`, `encode_line`, `write_line` each have one caller.** They are the
  mechanism table's steps two, three-plus-four, and five by name, and the Journal
  suite targets them directly. Kept.
- **`turn` clears the batch twice**, once per Fatal row of the Phases table. Folding
  the two into one discard site would read less like the table. Kept.
- **`engine.rs` defines its public exit types below the impl that uses them.**
  Reordering is a diff with no reader who asked for it. Kept.

## Not drift

`app` and `env` are the design's own parameter names in the `Engine::new` block;
`BoundedBuffer` is the layout table's name for the file, and the mechanism table's
`CommandBuffer<C>` is that type in its batch role. Neither is renamed.

## The batch

Landed 2026-09-06, uncommitted: S1 through S5, plus the fixture's `FatalCause` copy for
S4 and the one compile-fail attack S3 moved out from under. The non-test diff of the
three source files was read end to end: `run` and `drive` together are fifty-nine
lines where `run` alone was a hundred and five, every failure arm reaches the same
`Finalization` variant it reached before, the record transitions are the same calls in
the same order with shorter spelling, and the Journal's four poison sites are one
helper. Eleven assertion sites remain, none added.

After the batch: `cargo test` green — 210 in-file tests, every cross-file suite, and
the five compile-fail cases with one regenerated expectation — and
`cargo clippy --all-targets -- -D warnings` clean. The fault matrices assert the exact
Environment call list per fault point and all pass, which is the byte-for-byte
call-order check. Only the two baseline-clean engine files were formatted; the journal
file's single pre-existing `rustfmt` hunk is untouched.
