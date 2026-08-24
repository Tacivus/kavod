# Record typestate

> **Status:** Settled mechanism for §8.2's compile-time record grammar (`RECORD-GRAMMAR`),
> 2026-08-15. Authority on *what* the records mean stays with `design-final.md` §8; this doc
> owns *how* the grammar is made unrepresentable to violate. Concrete types here are tier 3;
> the serialized form (§8.2's flat `record_kind` objects) is normative.

## 1. Purpose & scope

Replace the runtime record-grammar assertion (`last_record_kind` plus a per-commit `match`)
with an affine typestate token, so that an illegal record sequence fails to type-check instead
of panicking at runtime. The typestate enforces **journal grammar only** — dispatch, failure
observation, and shutdown ordering remain Engine responsibilities per §8.4's tables.

Compared to the bigram assertion it replaces, this is strictly stronger in three ways:

1. **Kind/payload coupling** — the committed kind derives from the payload type; a record
   labeled with the wrong `RecordKind` is unconstructible (previously: two decoupled
   arguments to `commit_record`).
2. **Outcome/phase coupling** — `TurnCompleted(Continue)` after `StopRequested` is
   unrepresentable (the bigram check accepted any `TurnCompleted` there).
3. **Whole-word grammar** — the token encodes the run's full history, not one lookbehind:
   e.g. `EventAccepted` directly after `RunStarted` (bigram-legal, never emitted by the real
   protocol) is unrepresentable.

## 2. Module & structural guarantees

Everything below lives in `src/engine/record.rs`, declared `mod record;` in a wiring-only
`src/engine/mod.rs` alongside `mod engine;` (the run loop). As siblings under `engine`, the
run loop reaches record's `pub(super)` API via `use super::record::…` while record's private
internals stay invisible to it — `pub(super)` covers the whole `engine` subtree, so no
visibility loosening is needed, and `engine`'s test modules see the API the same way. The
module must stay a *child* of `engine`: a top-level sibling would force the API to
`pub(crate)`, exposing the transitions crate-wide. `RecordKind` and `JournalFatal` live here
(commit constructs them) and are re-exported through `mod.rs`. The module boundary *is* the
guarantee — two rules hold by visibility, not convention:

- **Tokens are unforgeable.** `RecordState`'s constructor is private to the module; the sole
  public entry produces `RecordState<Initial>`. Outside the module — including the Engine's
  own run loop — the only way to obtain a token is to be handed one by a transition.
- **The module owns the only commit path.** The Engine does not store a `Journal<W>`; it
  stores a newtype whose field is private to the record module:

```rust
pub(super) struct Recorder<W>(Journal<W>);   // field private to `mod record`
```

  No engine code can reach `Journal::commit`, so "records are committed only through
  typestate transitions" is a compile-time fact (`RECORD-GRAMMAR`). `Recorder` exposes
  construction (wrapping the Journal built per §8.4's Construction table) and one
  module-internal commit:

```rust
fn commit<R: RecordPayload>(&mut self, record: &R) -> Result<(), JournalFatal>
// journal.commit(record), Err(error) → JournalFatal { record_kind: R::KIND, error }
```

The token:

```rust
pub(super) struct RecordState<S> {
    index: EventIndex,                 // START in Initial; current turn thereafter
    _phase: PhantomData<fn() -> S>,
}
```

No `Clone`, no `Copy`, no `Default`. `index()` is the one getter — the Engine reads it for
`Context::new`, making the token the single index authority after acceptance. Phases carry no
data of their own; `PhantomData<fn() -> S>` keeps the token `Send`/`Sync`-independent of `S`.

## 3. Record types

`RecordKind` is unchanged in shape (public via `JournalFatal`) and now derives `Serialize` —
its unit variants render as the §8.2 tag strings.

One payload struct per record — `RunStarted`, `EventAccepted<'a, E>`,
`CommandsPrepared<'a, C>`, `CommandsDispatched`, `StopRequested`, `TurnCompleted` (plus
`TurnOutcome`) — each `#[derive(Serialize)]`, first field `record_kind: RecordKind`, remaining
fields in §8.2 table order. Borrowed `event`/`commands` live only for the synchronous commit;
no typestate is ever serialized.

```rust
trait RecordPayload: Serialize {
    const KIND: RecordKind;
}
```

Every construction site (the six committing transitions below) writes
`record_kind: Self::KIND`, so the serialized tag and the `JournalFatal` kind share one source
and cannot diverge. There is no serialization enum and no hand-written `Serialize` — the
derive emits §8.2's flat line directly.

Structural consequence carried forward: a struct always serializes as a JSON object, so
`JournalError::NotAnObject` is unreachable through the Engine. The variant stays live for
direct Journal consumers.

## 4. Phases & transitions

```rust
mod phase {
    pub(super) struct Initial;
    pub(super) struct TurnOpen;
    pub(super) struct Prepared;        // not `CommandsPrepared`: avoids colliding with the payload struct
    pub(super) struct EffectsComplete;
    pub(super) struct BetweenTurns;
    pub(super) struct StopPending;
    pub(super) struct Closed;
}
```

Transitions are associated fns on `RecordState<S>`, generic only over what they touch
(`W`, and `E`/`C` where a payload borrows one). `rec` is `&mut Recorder<W>`.

| Token consumed | Method | Record | Token returned |
|---|---|---|---|
| `Initial` | `run_started(rec, logical_time)` | `RunStarted` | `TurnOpen` (index = `START`) |
| `BetweenTurns` | `accept_event(rec, index, time, &event)` | `EventAccepted` | `TurnOpen` (index baked) |
| `TurnOpen` | `no_commands()` — infallible, no commit | — | `EffectsComplete` |
| `TurnOpen` | `prepare_commands(rec, &[C])` — asserts nonempty | `CommandsPrepared` | `Prepared` |
| `Prepared` | `commands_dispatched(rec)` | `CommandsDispatched` | `EffectsComplete` |
| `EffectsComplete` | `complete_continue(rec)` | `TurnCompleted(Continue)` | `BetweenTurns` |
| `EffectsComplete` | `request_stop(rec)` | `StopRequested` | `StopPending` |
| `StopPending` | `complete_stop(rec)` | `TurnCompleted(Stop)` | `Closed` |

Every committing transition: consumes the token, builds its payload (index from the token —
only `run_started` fixes it at `START` and `accept_event` takes it as a parameter), commits
through `Recorder`, and returns the next token **only on success**; `Err(JournalFatal)`
returns no token. `TurnOutcome` is chosen by the method, never by the caller.

The index is passed exactly once, at `accept_event`: the Engine keeps `checked_next` and the
accepted-turn bound where §8.4's acquisition table puts them (`BOUND-INDEX` reasoning
unchanged), and every later record derives its index from the token — a wrong-index record
after acceptance is unconstructible.

## 5. Semantic preconditions

Enforced by the Engine per §8.4's turn-result table; the typestate only sequences the
commits. By citation, not restatement:

- `no_commands` vs `prepare_commands` — order 3's empty/nonempty split.
- `commands_dispatched` — order 5 (after order 4's last handoff).
- `complete_continue` / `request_stop` — after order 6's `take_failure` checkpoint returns
  `None`; 7a / 7b respectively.
- `complete_stop` — orders 8b–10b: only after `shutdown` returns `Quiesced`.

## 6. Failure semantics

A journal failure returns `JournalFatal` and no next token. On any Fatal from any nonterminal
phase, the Engine drops the current token; combined with the module-private commit path, no
further commit is *expressible* (`RECORD-GRAMMAR`), which backs `FAIL-FINALIZE`'s "never
writes to the Journal again" structurally rather than by discipline. No rollback is performed.
The `Recorder` is Engine-owned and never attached to the token.

## 7. Proof boundary (flagged risks)

What the compiler proves, and where the proof stops:

- **Affinity ≠ linearity.** Dropping a token and committing nothing type-checks — intended on
  Fatal paths, but it means a record *omitted* where the protocol requires one is caught by
  golden-Journal tests, never the compiler.
- **Payload content beyond kind, outcome, and index is unproven.** Residual always-on asserts
  where the types run out, per the existing assertion discipline (R1-bounded):
  `prepare_commands` asserts its slice is nonempty.
- **Test/compiler-discharged claims, not inspection:** the flat wire format is pinned by
  byte-exact goldens (including that `RecordKind` unit variants serialize as bare tag strings
  and `record_kind` lands first); the generic transition signatures must survive a clean
  `cargo check` before anything builds on them.
- **"Private construction" means module-private.** The guarantee holds against the Engine
  itself only because the token, phases, transitions, and `Recorder` field all sit behind
  `mod record`. Moving any of them out re-opens forgery and direct commits.

## 8. Engine integration sketch

`TurnFlow::Continue` carries `RecordState<BetweenTurns>`; the shared turn helper takes
`RecordState<TurnOpen>` (start turn and event turns identically, per A2). The Engine keeps
`checked_next` and the `accepted < max_turns` bound; `Context::new` reads `token.index()`.
Removed outright: the field-struct `Record` enum, `commit_record`'s separately supplied
`RecordKind`, `last_record_kind`, and the runtime grammar `match`.

## 9. Verification

- Byte-exact golden JSON for every record (freezes the flat `record_kind`-first form).
- Empty and nonempty `Continue` turns; start-turn and external-event `Stop` turns.
- Journal-failure injection at every committing transition, checking the `JournalFatal`'s
  `RecordKind`.
- Dispatch failure leaves the run holding `RecordState<Prepared>` with no later record.
- Illegal transition sequences fail to type-check; there is no runtime grammar assertion to
  test.
