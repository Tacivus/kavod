# Round 1 — Conformance

`src/` read against `design-v12.md`, bottom-up in dependency order, in both
directions: every Core-scope rule looked for the code that realizes it, and every piece
of code looked for the rule that asks for it. Read at the compile-fail fixture, with
`cargo test` and `cargo clippy --all-targets -- -D warnings` green before the round
opened.

**Result: four code findings, none a behavioral defect.** On every path checked the
Core does what the design says. What it does beyond the design is small: one private
helper represents a state `RUN-FINALIZE` has no row for and rejects it at runtime; one
method is public that no API block lists; one phase parameter is wider than the
mechanism it realizes; and one mechanism sentence is realized by two literals where the
design says one value. The assertion read found the thirteen sites the ledger counted,
one of them missing from the ledger's table, and named an owner for every one.

## The read

| Subsystem | Against | Result |
|---|---|---|
| Time types | the Application API block, A6, `ENV-TIME` | conforms — derives, signatures, and both overflow arms of `checked_add` exact |
| Bounded buffer | A6; `JRN-ENCODE`'s zero-progress clause; the Application and Journal mechanisms | conforms — bound checked before every push and write, allocation never grows, zero progress is `WriteZero` |
| Journal | the Journal API block, `JRN-FORMAT` through `JRN-SINK`, the `commit` table | conforms — region exactly `max_record_bytes + 1`; the three-check object classification; the three encode failures write and poison nothing; the write loop is bounded by record length and retries only a short success; every sink failure poisons with its mapped Error; only flush commits; commit on poisoned panics |
| Application contract | the Application API block, `APP-CONTEXT` through `APP-STATE`, the `emit` table | conforms — a handler sees exactly State, its Event, and `Context`; `emit`'s three steps in order; a fresh `Context` clears buffer and marker |
| Port contract | the Port API block, the `ports!` expansion | conforms — two enums, `::serde` derives, `$vis` propagated, the invocation name creates no item |
| Latch | `ENV-LATCH`'s state table; the `take_error` and `shutdown` commitment rows | conforms — four states; first publication wins; `take` reports forever; `close` returns a pending Error once; a local Error yields to a pending one |
| Environment contract | the Environment API block | conforms — signatures and the consuming `shutdown` exact; doc comments carry the commitment table's `Err` meanings |
| Record grammar | `RUN-GRAMMAR`, `RUN-ENFORCEMENT`, `RUN-RECORDS`, `RUN-INDEX`; the phase, edge, record, and transition tables | conforms — record fields in table order, one tag source, every transition consumes its source and commits before returning, the domain check precedes `next_event`, the time check precedes the commit, index and time advance only on commit. Width beyond the tables: C2, C3, C4 |
| Engine | the Run API block; the construction and startup tables; `RUN-SERIAL`, `RUN-CHECKPOINT`, `RUN-FINALIZE`; A4's precedence in `close` | conforms — State first, `start` second, no shutdown after a start `Err`, one checkpoint per turn, the report's Error outranks `Incomplete`, retained quiescence survives the `TurnCompleted(Stop)` commit. Width beyond the rule: C1 |
| Crate | `NO-UNSAFE`, `BOUND-NONZERO`, `CRATE-EXPORTS`, `TRUST-ABORT`'s profile | conforms today — every API-block item exported once at the root and nothing else; `panic = "abort"` in both profiles |

## Findings

### C1 — `finalize` represents a state `RUN-FINALIZE` has no row for

`src/engine/engine.rs:104` — `finalize(state, cause, retained_quiescence: Option<Quiescence>, environment: Option<E>)`.

`RUN-FINALIZE` fixes quiescence from exactly three sources: `start` returned `Err`; the
Environment is unconsumed; `StopPending` ran and retained the report's quiescence. The
helper takes two `Option`s, so its signature admits four states, and the fourth — a
retained quiescence *and* an unconsumed Environment — is rejected by `unreachable!` at
`:114`.

The reading rules order enforcement: unrepresentable beats asserted, and where a type
can carry a rule it must. A three-variant private enum carries this one exactly, and the
assertion site goes with it.

Fix: replace the two arguments with one enum — start failed; unconsumed, carrying the
Environment; retained, carrying the quiescence — and match its three arms. One private
function, nine call sites, no behavior change. **Round 1 batch — fixed.**

### C2 — `RecordKind::tag` is public and no API block lists it

`src/engine/record.rs:20` — `pub const fn tag(self) -> &'static str`.

`RecordKind` is re-exported at the crate root with exactly the derives its API block
lists; the block lists no methods. `tag` exists to give `Kind<P>`'s `Serialize` its one
source for the wire tag, and its only callers are that impl and one in-file test. Public,
it is a second way to obtain a record's tag that nothing owns.

Fix: `pub(crate)`. **Round 1 batch — fixed.**

### C3 — the batch and checkpoint edges admit an unclassified certificate

`src/engine/record.rs:298` and `:357` — `impl<W, A> Certificate<W, TurnOpen<A>>` and
`impl<W, A> Certificate<W, EffectsComplete<A>>`.

The transition table renders `no_commands`, `dispatch_batch`, and `checkpoint` on
`TurnOpen<A>` and `EffectsComplete<A>` with `A` the marker `Continue` or `Stop`, and the
mechanism says `classify` fixes the marker before either batch transition. Both impls are
unbounded, so `A = Unclassified` is accepted: `CommandsPrepared` and
`CommandsDispatched` can be committed, and the checkpoint taken, on a certificate that
was never classified. What follows is a dead end — `Checkpointed<Unclassified>` has no
completion method, which the fixture case `unclassified_checkpoint_dead_end` proves — so
no illegal record sequence is expressible and `RUN-GRAMMAR` holds. The width is unowned.

Fix: a sealed marker trait implemented by `answer::Continue` and `answer::Stop` alone,
bounding `EffectsComplete<A>`, `Checkpointed<A>`, and the two impls above. The fixture
case then fails at the batch edge instead of the completion edge and its expectation
moves with it. **Round 1 batch — fixed.**

### C4 — the `TurnCompleted` outcome is two literals, not one value

`src/engine/record.rs:260` — `commit(&mut self, payload, outcome: Option<TurnOutcome>)`;
the two `Some` callers at `:383` and `:489`.

The mechanism says one value supplies a `TurnCompleted` payload and, on commit failure,
`JournalFatal.outcome`. Each of the two callers writes the literal twice — once into the
payload, once into the argument — and the five other callers pass `None` by hand.
In-module, a mismatch is expressible; the doc comment on `JournalFatal.outcome` holds by
inspection of seven call sites, not by shape.

Fix: let the payload supply it — a method on `RecordPayload` returning `None` by default
and `Some(self.outcome)` for `TurnCompletedRecord` — and drop the argument. Behavior
unchanged; this is a shape change to a private helper. **Open — Round 3.**

## Assertions

Thirteen always-on sites in the non-test regions of `src/`; every one is constant-time.
The ledger's table lists twelve and splits them six named, seven not; the missing site is
the `unreachable!` at `engine.rs:114`, and the split is five and eight.

| Site | Checks | Owning guarantee | Names an ID |
|---|---|---|---|
| `bounded_buffer.rs:21` | a successful reservation covers the logical capacity | A6 — the bound is real storage; `Engine::new` step 1 | no |
| `bounded_buffer.rs:33`, `:68`; `application.rs:78` | length never exceeds logical capacity | A6 | no |
| `bounded_buffer.rs:80` | a write never grows the allocation | A6; the Application mechanism's "never grows" | no |
| `journal.rs:76` | commit on a poisoned Journal | `JRN-POISON`, which names the site | `JRN-POISON` |
| `record.rs:303`, `:319` | the batch edge taken matches the buffer | `RUN-ENFORCEMENT`, which names both sites | `ASSERT-INVARIANTS` — the tier, not the owner |
| `record.rs:416` | index overflow past the domain check | `RUN-INDEX`, which names the site | `RUN-INDEX` |
| `record.rs:505` | the `Initial` certificate stores 0 | `RUN-ENFORCEMENT`, which names the site | `RUN-ENFORCEMENT` |
| `engine.rs:68`, `:81` | a start-turn certificate has no Event and a later one has one | the Phases table's `TurnOpen` row; no row names a site | no |
| `engine.rs:114` | retained quiescence and an unconsumed Environment never coincide | `RUN-FINALIZE` — see C1: the rule's shape makes the state unrepresentable, so the site should not exist | no — gone with C1 |

Every site the design names exists. Beyond C1:

- `engine.rs:68` and `:81` assert one invariant twice — the `assert_eq!` on
  index-versus-Event, then an `expect` on the same `Option`. Matching the `Option` once,
  with the index check as the assertion, leaves one site. **Open — Round 3.**
- `record.rs:303` and `:319` cite `ASSERT-INVARIANTS`, the definition of the tier,
  where the owning row is `RUN-ENFORCEMENT`. Whether the message must carry the owner is
  G6's build-rule question; this table records the owner either way.

## Outside the round's question

**Stale lint allowances.** Forty-six `#[allow(dead_code)]` attributes in `src/` carry
reasons of the form "used in a later build step". Every such step has arrived. Stripping
all of them on a scratch copy and running clippy under `-D warnings` leaves three
genuine hits: `Latch` and its impl, unused until the Environment steps, and one test
fixture's uninhabited variant in `port.rs`. The other forty-three, and the
`private_interfaces` allowance on `ClassifiedTurn`, suppress nothing. Not a conformance
finding — but while they stand, clippy's dead-code lint is disarmed for the two rounds
that follow, and deleting them changes no code. **Round 1 batch — done.**

## Deferred

| | Finding | To |
|---|---|---|
| D1 | The crate-layout table does not list `latch.rs`. The table is mechanism, and the file realizes `ENV-LATCH`'s state table for the two shipped Environments to share; a one-row addition, design-side. | Wiring close (rule 3) |
| D2 | `BuildError`, `EngineExit`, `FatalCause`, `EnvironmentFatal`, `JournalFatal`, `Outcome`, `ShutdownReport`, and `Never` derive nothing. The design lists nothing for them and calls further derives free, so the Core conforms; whether a caller can `expect` an `Engine::new` result is the public surface's question. | C57 |

## The batch

Landed 2026-09-06, uncommitted: C1, C2, C3, and the lint allowances. C4 and the doubled
assertion wait for Round 3. Nothing moved a rule, and no byte the golden suite pins
changed.

- **C1.** `finalize` takes one private `Finalization` enum — `StartFailed`,
  `Unconsumed(env)`, `Retained(quiescence)` — and matches its three arms. The
  `unreachable!` is gone, and with it the test that provoked it,
  `contradictory_retained_quiescence_and_environment_is_an_invariant_panic`: the state it
  rejected is no longer representable. Twelve assertion sites remain.
- **C2.** `RecordKind::tag` is `pub(crate)`.
- **C3.** A sealed `answer::Answer` trait, implemented by `Continue` and `Stop` alone,
  bounds `EffectsComplete<A>`, `Checkpointed<A>`, and the two batch-edge impls. The
  fixture case `unclassified_checkpoint_dead_end` is replaced by
  `unclassified_batch_edge`, which proves an unclassified certificate can take neither
  batch edge and that `EffectsComplete<Unclassified>` is not a type; every error in its
  expectation names the bound.
- **Allowances.** Forty-three `dead_code` attributes deleted, one of them also carrying
  `private_interfaces`. The three live ones stay: `Latch` and its impl, and the `port.rs`
  test fixture's uninhabited variant.

After the batch: `cargo test` green — 206 in-file tests, one fewer than before, and every
cross-file suite — and `cargo clippy --all-targets -- -D warnings` clean. Only the two
engine files and the compile-fail harness were formatted; the four files that were
already `rustfmt`-dirty in their test regions before the round are left as they were.
