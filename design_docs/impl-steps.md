# Kavod Core — Build Steps

This is the doc you work from. 57 steps, in order, each one sitting (~100–160 lines
including the listed tests — rule 4's additions push past that, and should). The
reference material lives in `impl-plan-v12.md` — you only open it when a step says
so: **the probe code** (compiling templates to copy, section 2), **the nine wiring
decisions** (section 1a, need your approval), and the verification maps. The design rules themselves are in `design-v12.md`; when a test cites an ID,
that's the row it pins.

## The rules — all of them

1. **Work in order.** A step is done when `cargo test` and
   `cargo clippy --all-targets -- -D warnings` are green and the step's tests — the
   listed ones and the ones rule 4 asks you to add — pass. Then tick the box and
   stop or continue — the crate is always in a finished state between steps.
2. **Code is final.** Later steps *add* to files. If you feel the need to rewrite an
   earlier step's code, stop — something's off; check the plan doc's risk list.
3. **Every test** goes in the file it tests, inside `#[cfg(test)] mod tests`, in a
   nested module per subject, under a doc comment that opens with `Invariant:` and a
   plain-English sentence — what has to be true, stated so someone who has never
   opened the design doc understands it, with no IDs in it. When the test pins a
   design rule, a second line, `Design Doc:`, names the row it pins; that is the only
   place an ID appears. The shape, once, so every step can just list names:

   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;

       mod journal_poisoning {
           use super::*;

           /// Invariant: once a Journal has been poisoned, committing to it panics
           /// instead of writing anything more.
           /// Design Doc: JRN-POISON
           #[test]
           #[should_panic(expected = "JRN-POISON")]
           fn commit_on_poisoned_journal_panics() { /* … */ }
       }
   }
   ```

   Steps list tests as `module::test_name (ID to cite)` — the parenthesised citation
   is the `Design Doc:` line, verbatim; the `Invariant:` sentence is yours to write.
   A test listed without a citation gets the `Invariant:` line alone. Files under
   `tests/` are cross-file suites — same shape, own file.
4. **The listed tests are a floor, not a target.** They pin what the design says;
   they are not the measure of what this code has to survive. Write every other test
   that pins real behavior in what the step just built — boundaries at zero, one,
   capacity and one past it; every arm of every enum; every error path; every order
   two operations can happen in; and what still holds after a failure. The goal is a
   system that survives contact with reality, not one that matches a document. Three
   things keep this from sprawling: search the later steps before adding, and skip
   anything already listed there; stay inside the subject the step is building; and
   give an unlisted test the `Invariant:` line alone, since most of them pin behavior
   no design row names. If one of these tests fails, that is the point of writing it
   — fix the step's own code, or, when the failure contradicts the design or an
   earlier step, stop and report it.
5. **New src file → two lines in `lib.rs`:** `mod x;` plus `pub use` for its public
   items (everything public is re-exported flat at the crate root; module files stay
   private). Not repeated in each step.
6. **Assertions are always-on** — `assert!`/`expect`, never `debug_assert!`, panic
   message naming the invariant (like the example above).
7. **⛔ steps** need the nine wiring decisions approved first (`impl-plan-v12.md`
   §1a). Every unmarked step can start right now.

## Progress

**A — Foundations**
- [x] C1 · time types
- [x] C2 · bounded buffer
- [x] C3 · buffer as `io::Write`

**B — Journal**
- [x] C4 · types + construction
- [x] C5 · bounded encode
- [x] C6 · classify + newline
- [ ] C7 · sink write loop
- [ ] C8 · commit + poison
- [ ] C9 · fault matrix (tests only)

**C — Application**
- [ ] C10 · Context / Outcome / trait

**D — Port**
- [ ] C11 · PortContract, Never, `ports!`
- [ ] C12 · downstream macro test

**E — Environment + latch**
- [ ] C13 · Environment trait
- [ ] C14 · latch

**F — Record grammar**
- [ ] C15 · engine skeleton + exit types
- [ ] C16 · record marker + first payloads
- [ ] C17 · remaining payloads
- [ ] C18 · certificate + `run_started`
- [ ] C19 · `classify` + `no_commands`
- [ ] C20 · `dispatch_batch`
- [ ] C21 · checkpoint + completion records
- [ ] C22 · `accept_event`
- [ ] C23 · `close`

**G — Engine**
- [ ] C24 · `Engine::new`
- [ ] C25 · turn helper
- [ ] C26 · finalize helper
- [ ] C27 · `Engine::run`
- [ ] C28 · loop behavior (tests only)

**H — Core suites**
- [ ] C29 · tests/support harness
- [ ] C30 · golden suite 1
- [ ] C31 · golden suite 2
- [ ] C32 · fault suite 1
- [ ] C33 · fault suite 2
- [ ] C34 · conformance
- [ ] C35 · compile-fail fixture

**I — Simulated Environment**
- [ ] C36 · SimPort + SimCtx
- [ ] C37 ⛔ sim wiring + start
- [ ] C38 ⛔ sim dispatch
- [ ] C39 ⛔ selection helpers
- [ ] C40 ⛔ `next_event`
- [ ] C41 ⛔ sim shutdown
- [ ] C42 ⛔ sim lifecycle suite
- [ ] C43 ⛔ sim conformance

**J — Live Environment**
- [ ] C44 · clock
- [ ] C45 · lifecycle cell + gate
- [ ] C46 · central monitor
- [ ] C47 ⛔ inboxes + LiveCtx
- [ ] C48 · supervision shell
- [ ] C49 ⛔ live wiring + start
- [ ] C50 ⛔ live next_event/dispatch
- [ ] C51 ⛔ live shutdown
- [ ] C52 ⛔ live suite 1
- [ ] C53 ⛔ live suite 2
- [ ] C54 ⛔ live suite 3
- [ ] C55 ⛔ latch suite
- [ ] C56 ⛔ cross-type conformance

**K — Close**
- [ ] C57 · export audit + docs

---

## Part A — Foundations

### C1 · Crate skeleton and the time types (~120 lines)

**Create** `src/lib.rs`, `src/time.rs`. **Edit** `Cargo.toml`.

Write:
- `lib.rs`: `#![forbid(unsafe_code)]`, module declarations, re-exports.
- `Cargo.toml`: keep the existing deps and `panic = "abort"` profile lines; add
  `[dev-dependencies] serde_json = { version = "1", features = ["raw_value"] }`.
- `EventIndex` and `Timestamp` exactly as the design's API block prints them:
  one-field tuple structs, private field, `#[derive(Serialize)]` (a one-field newtype
  serializes as the bare number — that's the "transparent u64" requirement),
  `pub(crate)` minting fns, checked arithmetic.

Tests:
- `timestamp_arithmetic::overflowing_sum_returns_none` (A6)
- `timestamp_arithmetic::oversized_duration_returns_none` (A6)
- `timestamp_arithmetic::equal_timestamp_is_valid` (ENV-TIME)
- `index_and_time_wire::both_serialize_as_transparent_u64` (the EventIndex/Timestamp API block)

Heads up: `Duration::as_nanos()` returns `u128`. Convert with `u64::try_from` first,
*then* `checked_add`. Never truncate.

### C2 · Bounded buffer (~130 lines)

**Create** `src/bounded_buffer.rs`.

Write `BoundedBuffer<T>` (crate-private):
- Construction: one `try_reserve_exact` for the full logical capacity, returning
  `Result<Self, TryReserveError>`.
- `try_push` (hands the value back when full), `len`, `capacity`, `is_empty`,
  `as_slice`, `clear`, `drain`. No growing operation exists, ever.

Tests:
- `bounded_buffer_capacity::push_beyond_capacity_is_refused_without_growth` (A6)
- `bounded_buffer_capacity::construction_failure_reports_the_reservation_error` (A6)
- `bounded_buffer_reuse::clear_and_drain_retain_capacity` (A6)

Heads up: logical capacity is a separate field from `Vec::capacity` — every push
checks the logical one. `drain(..)` yields owned items; the Engine's command handoff
will consume through exactly this.

### C3 · The buffer as `io::Write` (~90 lines)

**Edit** `src/bounded_buffer.rs`: add `impl std::io::Write for BoundedBuffer<u8>`.

**Copy the shape from probe P4** (plan doc §2): accept up to the remaining capacity
and return `Ok(n)`; at zero remaining return `Err(ErrorKind::WriteZero)`; `flush` is
`Ok(())`.

Tests:
- `encode_buffer_write::full_buffer_returns_write_zero` (JRN-ENCODE)
- `encode_buffer_write::partial_writes_accumulate_without_loss` (JRN-ENCODE)
- `encode_buffer_write::serde_json_encode_completes_at_exact_region_size` (JRN-ENCODE)

Heads up: `Write::write` returning short counts is legal — `serde_json` retries them
(P4 proved it). That's what makes the region boundary exact.

---

## Part B — Journal

The Journal is built as four small final helpers, then `commit` composes them. Each
helper gets tested through this file's own test module the day it's written.

### C4 · Journal types and construction (~110 lines)

**Create** `src/journal.rs`.

Write:
- `JournalBuildError`, `JournalError`, `SinkOperation` — variants per the API block.
- `Journal<W: io::Write>`: the writer, a `BoundedBuffer<u8>` encode region, a poison
  flag.
- `Journal::new`: region size is `max_record_bytes.checked_add(1)` (overflow →
  `MaxBytesTooLarge`), then the reservation (failure → `AllocationFailed`, keeping
  the `TryReserveError` by value). Plus `is_poisoned()`.

Tests:
- `journal_construction::region_size_overflow_is_max_bytes_too_large` (JRN-ENCODE)
- `journal_construction::failed_reservation_is_allocation_failed` (JournalBuildError, by name)
- `journal_construction::fresh_journal_is_not_poisoned` (JRN-POISON)

Heads up: the overflow test uses `NonZeroUsize::MAX` to trip the `checked_add` — it
must never actually try to allocate that.

### C5 · Bounded encode (~120 lines)

**Edit** `src/journal.rs`: add private `fn encode_raw<R: Serialize>` — clear the
region, `serde_json::to_writer` into it, then map the error: the buffer's `WriteZero`
(found via `serde_json::Error::io_error_kind()`) becomes `BoundExceeded`; every other
serde failure becomes `Encode`. No sink is touched anywhere in this function.

Also write two tiny fixtures in the test module: a `Serialize` impl that always fails
(~5 lines) and a counting writer (to prove "no sink calls").

Tests:
- `journal_encoding::oversized_record_is_bound_exceeded_without_sink_calls` (JRN-ENCODE)
- `journal_encoding::serializer_failure_is_encode_without_sink_calls` (JRN-ENCODE)
- `journal_encoding::encode_failures_do_not_poison` (JRN-ENCODE)

### C6 · Classification and the newline (~120 lines)

**Edit** `src/journal.rs`: add private `fn encode_line` — call `encode_raw`, then
classify the bytes: first byte `{`, last byte `}`, no raw newline anywhere, otherwise
`NotAnObject`; then append the `\n` — no room left means `BoundExceeded`.

Tests:
- `journal_object_validation::object_plus_newline_is_the_encoded_line` (JRN-FORMAT)
- `journal_object_validation::interior_newline_is_not_an_object` (JRN-ENCODE)
- `journal_object_validation::non_object_top_level_is_rejected` (JRN-ENCODE)
- `journal_newline_reservation::object_of_region_size_has_no_newline_room` (JRN-ENCODE)
- `journal_newline_reservation::encode_at_exactly_max_bytes_completes` (JRN-ENCODE)

Heads up: a `"\n"` inside a string gets *escaped* by serde — that's not the test
case. The real vector is `serde_json::value::RawValue` with a literal newline between
tokens (valid JSON, raw byte preserved — probe P4 confirmed). And write the two
boundary tests (`max` fits, `max + 1` object doesn't) straight from the JRN-ENCODE
row's sentences.

### C7 · The sink write loop (~130 lines)

**Edit** `src/journal.rs`: add private `fn write_line` — write the encoded line to
the sink in a loop that retries **only short successful writes**. Classify everything
else and poison before returning: any `Err` (including `Interrupted`) → that error;
`Ok(0)` → `WriteZero`; a count larger than what remains → `InvalidData`.

Also write the scripted sink fixture in the test module: per-call scripted
write/flush results plus a call log.

Tests:
- `journal_sink_writes::short_successful_writes_are_retried_to_completion` (JRN-POISON)
- `journal_sink_writes::interrupted_write_poisons_without_retry` (JRN-POISON)
- `journal_sink_writes::zero_progress_maps_to_write_zero` (JRN-POISON)
- `journal_sink_writes::over_reported_count_maps_to_invalid_data` (JRN-POISON)

Heads up: `Interrupted` is deliberately *not* retried — the design's Justify note
explains why. Don't "fix" it. Advance the offset only after checking
`count <= remaining`.

### C8 · `commit` and the poison precondition (~100 lines)

**Edit** `src/journal.rs`: write `commit` — `assert!(!poisoned)` (always-on, A8),
then `encode_line` → `write_line` → `flush`. A flush failure poisons and returns
`SinkFlush`; only a successful flush commits.

Tests:
- `journal_commit::successful_flush_commits_exactly_the_line` (JRN-COMMIT)
- `journal_commit::flush_failure_is_sink_flush_and_uncommitted` (JRN-COMMIT)
- `journal_commit::sink_error_poisons_permanently` (JRN-POISON)
- `journal_poisoning::commit_on_poisoned_journal_panics` (JRN-POISON, A8) — `#[should_panic(expected = …)]`

Heads up: bytes written before a failed flush are an uncertain suffix — the tests
inspect the call log, they never pretend a rollback happened.

### C9 · Journal fault matrix — tests only (~100 lines)

**Edit** `src/journal.rs` tests only. Using the C7 scripted sink, cover the full
call/result trace and committed-boundary behavior.

Tests:
- `journal_commit_boundaries::only_successful_flush_advances_the_committed_boundary` (JRN-COMMIT)
- `journal_sink_matrix::every_sink_failure_poisons_exactly_once` (JRN-POISON)
- `journal_sink_matrix::the_sink_receives_exact_bytes_in_call_order` (JRN-SINK, TRUST-SINK)

---

## Part C — Application

### C10 · `Context`, `Outcome`, the trait (~140 lines)

**Create** `src/application.rs`. **Copy the shape from probe P7** (plan doc §2) — it
is the whole chunk.

Write: `Outcome`, the `Application` trait, and `Context<'a, C>` borrowing
`&'a mut BoundedBuffer<C>` with the overflow marker stored in the Context.
Construction and marker readback are `pub(crate)`; only the observers and `emit` are
public. Construction clears the buffer (fresh handler = empty batch, clear marker).

Tests:
- `context_emit::commands_append_in_call_order_through_exact_capacity` (APP-EMIT)
- `context_emit::remaining_reports_exact_free_capacity` (APP-CONTEXT)
- `context_overflow::first_over_bound_emit_stores_nothing_and_sets_the_marker` (APP-OVERFLOW)
- `context_overflow::every_later_emit_stores_nothing` (APP-OVERFLOW)
- `context_overflow::remaining_is_zero_once_the_marker_is_set` (APP-OVERFLOW)
- `context_reuse::fresh_invocation_starts_empty_with_a_clear_marker` (APP-OVERFLOW)

Heads up: `emit` takes ownership of `C` and silently drops rejected Commands —
that's what makes it infallible by signature.

---

## Part D — Port

### C11 · `PortContract`, `Never`, `ports!` (~130 lines)

**Create** `src/port.rs`. **Copy probe P2** (plan doc §2), adjusting to the real
crate root.

Write: `PortContract`, `Never` with the hand-written `Serialize` (`match *self {}`),
and `ports!` as `#[macro_export] macro_rules!` — expansion is exactly two derived
enums, using `::serde::Serialize` and `$crate::PortContract` paths.

Tests:
- `ports_macro_expansion::generated_sums_are_externally_tagged` (the Port Mechanism's wire form, by name)
- `ports_macro_expansion::hand_written_equivalent_is_byte_identical` (PORT-SUMS)
- `ports_macro_expansion::contract_bound_at_two_slots_yields_two_variants` (PORT-SUMS)
- `never_direction::never_command_arm_is_discharged_by_match` (Never, by name)

Heads up: resist adding anything to the macro beyond the two enums — the wiring
decision W7 depends on that restraint.

### C12 · The downstream macro test (~70 lines)

**Create** `tests/ports_macro.rs`. Integration tests are a separate crate, so this is
the real exported-macro boundary: invoke `kavod::ports!` as a consumer and write an
exhaustive match over both generated sums.

Tests:
- `ports_macro_downstream::consumer_invocation_compiles_and_serializes` (PORT-SUMS)
- `ports_macro_downstream::the_fanout_match_is_exhaustive` (PORT-ROUTING)

---

## Part E — Environment contract and the latch

### C13 · The `Environment` trait (~80 lines)

**Create** `src/environment.rs`: the trait, `ShutdownReport`, `Quiescence`, with doc
comments quoting the behavior rows they bind.

Test:
- `environment_contract_shape::a_scripted_implementation_drives_all_five_operations` (the Environment API block, by name) — a minimal in-file impl walking the ENV-SERIAL call pattern, ending in consuming `shutdown`.

Heads up: `fn shutdown(self)` in a trait needs `Self: Sized` — it has it; trait
objects aren't a goal because the Engine is generic over `E`.

### C14 · The latch (~140 lines)

**Create** `src/latch.rs` (crate-internal): `Latch<E>` — empty / pending / reported /
closed. `publish` (first wins; discarded after close), `take` (pending → reported,
forever), `close` (returns the pending Error once), `is_pending`, plus the two
precedence helpers both Environments will share: *pending beats a local failure*, and
*close into a report exactly once*.

Tests:
- `latch_first_wins::first_publication_is_kept_and_later_discarded` (ENV-LATCH)
- `latch_first_wins::take_marks_reported_forever` (ENV-LATCH)
- `latch_close::close_returns_the_pending_error_exactly_once` (ENV-LATCH)
- `latch_close::publication_after_close_is_discarded` (ENV-LATCH)
- `latch_precedence::a_pending_error_wins_and_discards_the_local_error` (ENV-LATCH, A4)
- `latch_precedence::a_local_error_wins_when_the_latch_is_empty` (ENV-LATCH)

Heads up: enum-with-payload state machine; `std::mem::replace` moves the Error out.
`take` on `reported` returns `None` — reported is forever, not re-readable. The latch
has no lock of its own; whoever owns it supplies the critical section.

---

## Part F — Record grammar (`src/engine/`)

One standing rule for this whole part: **`engine/record.rs` imports only through
`crate::journal::…`, `crate::time::…`, `crate::environment::…`,
`crate::bounded_buffer::…`, `crate::application::…`** — from its first line. The
compile-fail fixture (C35) mirrors exactly those paths; any other import breaks it.
`engine/mod.rs` stays declarations + re-exports only.

### C15 · Engine skeleton and the exit types (~140 lines)

**Create** `src/engine/mod.rs`, `src/engine/record.rs`, `src/engine/engine.rs`.

Write declarations only (this is deliberately a breather before typestate):
- `record.rs`: `RecordKind` + `const fn tag()`, `TurnOutcome` (derive `Clone, Copy`
  too), `JournalFatal`.
- `engine.rs`: `EngineConfig`, `BuildError`, `EngineExit`, `FatalCause`,
  `EnvironmentFatal`, `EnvironmentOperation`, `CoreError` — exactly per the API
  blocks, doc comments included.

Tests:
- `record_kind_wire::turn_outcome_serializes_as_a_bare_tag` (RUN-RECORDS)
- `record_kind_wire::kind_tags_match_their_variant_names` (RUN-RECORDS)
- `journal_fatal_metadata::outcome_is_present_only_for_turn_completed` (JournalFatal, by name)

### C16 · The record marker and first payloads (~110 lines)

**Edit** `src/engine/record.rs`. **Copy probe P3** (plan doc §2).

Write: `RecordPayload { const KIND }`, `Kind<P>` (zero-sized, one shared hand-written
`Serialize` emitting `P::KIND.tag()`), then `RunStartedRecord` and
`EventAcceptedRecord<'a, Ev>` — fields in exactly the Records table's order,
`record_kind: Kind<Self>` first.

Tests:
- `record_payload_wire::run_started_matches_the_documented_example_line` (RUN-RECORDS)
- `record_payload_wire::event_accepted_serializes_index_time_and_borrowed_event` (RUN-RECORDS)
- `record_payload_wire::payload_tag_and_kind_share_one_source` (RUN-GRAMMAR)

Heads up: struct field order controls serde's output order. The Event is *borrowed*
(`&'a Ev`) so a successful commit can later return the owned Event without `Clone`.

### C17 · The remaining payloads (~110 lines)

**Edit** `src/engine/record.rs`: `CommandsPreparedRecord<'a, C>` (borrowing the batch
as a slice), `CommandsDispatchedRecord`, `StopRequestedRecord`, `TurnCompletedRecord`.

Tests:
- `record_payload_wire::commands_prepared_keeps_batch_order_in_bytes` (RUN-RECORDS)
- `record_payload_wire::every_payload_leads_with_its_kind_in_table_order` (RUN-RECORDS)
- `record_payload_wire::turn_completed_outcome_is_a_bare_tag_for_both_answers` (RUN-RECORDS)

### C18 · The certificate, minting, `run_started` (~150 lines)

**Edit** `src/engine/record.rs`. **Copy probe P1's certificate half** (plan doc §2)
with the real types (`EventIndex`, `Timestamp`, `Journal<W>`).

Write:
- The phase marker types `Initial` … `Closed`, with the answer markers
  (`Continue`/`Stop`) in a private module so they can't collide with `Outcome`.
- `Certificate<W, P>` with `PhantomData<fn() -> P>` and the private `advance` helper.
- `mint` (consumes the Journal + frozen start time; `assert_eq!(index, 0)` — the
  induction base, always-on).
- The private generic commit helper: takes a payload, commits it, and on failure
  builds `JournalFatal` from the same `P::KIND` (plus the outcome for
  `TurnCompleted`).
- `run_started()`.
- A minimal failing-writer fixture in the test module (~15 lines — a deliberate small
  copy of C7's idea; both are pinned by ID-cited tests so drift shows up loudly).

Tests:
- `certificate_minting::minting_asserts_the_prospective_index_base` (RUN-ENFORCEMENT)
- `certificate_minting::run_started_commits_the_versioned_first_record` (RUN-RECORDS)
- `certificate_fatal_path::commit_failure_names_run_started_and_destroys_the_journal` (RUN-GRAMMAR)

Heads up: `PhantomData<fn() -> P>` (not `PhantomData<P>`) keeps `Send`/`Sync`
independent of the marker. Not deriving `Clone`/`Copy`/`Default` *is* the
enforcement — don't add them for convenience in tests.

### C19 · `classify` and the recordless edge (~100 lines)

**Edit** `src/engine/record.rs`: `ClassifiedTurn`, `classify(self, answer)` (a match
that moves the certificate into the answer-typed variant), and
`no_commands(&BoundedBuffer<C>)` — always-on empty assert, phase advance, no commit.

Tests:
- `turn_classification::classify_fixes_the_answer_in_the_phase_type` (RUN-ENFORCEMENT)
- `turn_classification::the_empty_batch_edge_commits_nothing` (the Edges table's recordless row, by name)
- `turn_classification::no_commands_panics_on_a_nonempty_buffer` (ASSERT-INVARIANTS) — `#[should_panic]`

### C20 · `dispatch_batch` (~150 lines)

**Edit** `src/engine/record.rs`: the fused transition — always-on nonempty assert,
commit `CommandsPrepared` from a shared view of the batch, drain each Command by
value into `env.dispatch` in order (`Err` at position k → `{ position, error }`, the
suffix is discarded with the drain), commit `CommandsDispatched` after the last
handoff.

Also write the in-file scripted Environment double: a call log plus a scripted
failure position. It serves every remaining step in this part.

Tests:
- `batch_dispatch::prepared_then_each_handoff_in_order_then_dispatched` (A5)
- `batch_dispatch::error_at_position_k_keeps_the_prefix_and_discards_the_suffix` (the Prepared phase row, by name)
- `batch_dispatch::prepared_commit_failure_precedes_any_handoff` (RUN-GRAMMAR)
- `batch_dispatch::dispatched_commit_failure_follows_every_handoff` (the Edges table, by name)
- `batch_dispatch::an_empty_buffer_is_an_invariant_panic` (ASSERT-INVARIANTS) — `#[should_panic]`

Heads up: the commit borrows the buffer immutably, the drain needs it mutably —
finish the commit *before* starting the drain; the borrow checker will hold you to
it. The design explains why prepare/dispatch can't be two separate calls — read that
Derive before coding.

### C21 · Checkpoint and the completion records (~130 lines)

**Edit** `src/engine/record.rs`:
- `checkpoint(env)` on `EffectsComplete<A>` — one `take_error` snapshot, commits
  nothing; a pending Error consumes the certificate (the Error branch returns no
  certificate — that's what makes a completion record after a pending Error
  impossible).
- `complete_continue()` — exists only on `Checkpointed<Continue>`.
- `request_stop()` — exists only on `Checkpointed<Stop>`; note it doesn't borrow the
  Environment, which is what makes intent-commit precede shutdown structurally.

Tests:
- `turn_checkpoint::the_snapshot_is_taken_exactly_once` (RUN-CHECKPOINT)
- `turn_checkpoint::a_pending_error_is_checkpoint_fatal_and_consumes_the_certificate` (RUN-CHECKPOINT)
- `turn_completion::continue_commits_turn_completed_continue` (the Edges table, by name)
- `turn_completion::stop_commits_stop_requested` (the Edges table, by name)
- `turn_completion::the_committed_outcome_is_the_phase_marker_not_a_caller_value` (RUN-ENFORCEMENT)

### C22 · `accept_event` (~150 lines)

**Edit** `src/engine/record.rs`: on `BetweenTurns` — in this order: the `u64::MAX`
domain check (before touching the Environment), `env.next_event()`, the
time-regression check, commit `EventAccepted` with the derived index and returned
time, and only after `Ok` update the certificate's index/time and return the Event.
The post-guard increment is `checked_add(1).expect("RUN-INDEX: …")` (always-on).

Tests:
- `event_acceptance::the_domain_check_precedes_next_event` (RUN-INDEX)
- `event_acceptance::a_decreasing_stamp_is_time_regression_with_the_candidate_consumed` (the EventAccepted edge row, by name)
- `event_acceptance::an_equal_stamp_is_accepted` (ENV-TIME)
- `event_acceptance::acceptance_advances_index_and_time_only_on_commit` (RUN-GRAMMAR)
- `event_acceptance::event_accepted_bytes_carry_the_new_index_and_time` (RUN-RECORDS)

Heads up: the test module shares the file, so it may build a certificate at
`u64::MAX - 1` directly — that's the sanctioned way to test the domain check.

### C23 · `close` (~140 lines)

**Edit** `src/engine/record.rs`: on `StopPending` — `close(env)` takes the
Environment *by value*, calls consuming `shutdown`, **stores quiescence before
looking at the Error**, then: report Error → fatal (Error outranks Incomplete);
Incomplete without Error → `ShutdownIncomplete`; clean report → commit
`TurnCompleted(Stop)` → `Closed`. Every failure carries the retained quiescence.

Tests:
- `stop_closing::a_clean_report_commits_turn_completed_stop` (the Edges table, by name)
- `stop_closing::a_report_error_outranks_incomplete` (the StopPending phase row, by name)
- `stop_closing::incomplete_without_error_is_shutdown_incomplete` (the StopPending phase row, by name)
- `stop_closing::commit_failure_after_a_clean_report_retains_quiesced` (RUN-FINALIZE)

---

## Part G — Engine

The driver is two final private helpers plus the loop that composes them. Nothing
here is a placeholder — each helper is tested the day it lands, and C27 only wires
them together.

### C24 · `Engine::new` (~100 lines)

**Edit** `src/engine/engine.rs`: the `Engine` struct fields and `new` per the
construction table — reserve the Command batch (failure → `CommandBuffer`), build the
Journal (failure → `Journal`), and call **no** Application or Environment method.

Tests:
- `engine_construction::batch_reservation_failure_is_command_buffer` (the construction table, by name)
- `engine_construction::journal_build_failure_is_journal` (the construction table, by name)
- `engine_construction::construction_invokes_no_application_or_environment_method` (the construction table, by name)

Heads up: write the three-way generic bounds once, on the `impl` block, as the API
block does.

### C25 · The turn helper (~130 lines)

**Edit** `src/engine/engine.rs`: one private final associated fn that runs a single
turn's handler phase — clear the batch, build a `Context` (index/time come from the
certificate), call exactly one handler (`on_start` at index zero, `on_event` after),
end the Context borrow, then: **overflow beats the returned `Outcome`**; `Fatal(e)`
discards the batch; `Continue`/`Stop` goes to `classify`.

Tests:
- `turn_handler_selection::index_zero_calls_on_start_once` (the Phases table, by name)
- `turn_handler_selection::a_later_index_calls_on_event_once` (the Phases table, by name)
- `turn_overflow_precedence::overflow_outranks_the_returned_outcome` (APP-OVERFLOW, A4)
- `turn_application_fatal::state_mutation_and_the_fatal_payload_both_stand` (APP-STATE)

Heads up: put the Context in its own `{ }` block so its mutable borrow ends before
you inspect or drain the buffer. State stays outside every `Result` branch.

### C26 · The finalize helper (~110 lines)

**Edit** `src/engine/engine.rs`. **Copy `finalize` from probe P1** (plan doc §2).

One private associated fn keyed on `(retained_quiescence, Option<E>)`:
- `(Some(q), None)` — Stop's `close` already consumed the Environment: use the
  retained quiescence.
- `(None, Some(env))` — the Environment is still here: call `shutdown` once, keep its
  quiescence, discard its Error (the fixed cause never changes).
- `(None, None)` — the `start`-`Err` path: `Quiesced`, no shutdown call.
- `(Some(_), Some(_))` — `unreachable!`, and the message says why.

Tests:
- `fatal_finalization::a_started_environment_is_shutdown_exactly_once` (RUN-FINALIZE)
- `fatal_finalization::the_shutdown_error_never_replaces_the_fixed_cause` (A4, RUN-FINALIZE)
- `fatal_finalization::a_start_error_skips_shutdown_and_is_quiesced` (ENV-START, RUN-FINALIZE)
- `fatal_finalization::a_consumed_environment_uses_the_retained_quiescence` (RUN-FINALIZE)

Heads up: `Option<E>` exists only at this one boundary — `None` is the *proof* that
Stop consumed the Environment. The transitions themselves keep taking `&mut E` (and
`close` takes `E`); don't let `Option` leak into them.

### C27 · `Engine::run` (~130 lines)

**Edit** `src/engine/engine.rs`. **Copy the loop from probe P1** (plan doc §2) — it
is the template, verbatim, with real types.

`run(self)`: destructure, create State first (before anything fallible), `env.start()`
(`Err` → Fatal, quiesced, no shutdown), mint the certificate, `run_started`, then
loop: turn helper → the marker-generic `effects` helper (empty batch → `no_commands`,
else `dispatch_batch`, then `checkpoint`) → completion → `accept_event` at the back
edge. Every `Err(f)` routes to `finalize`. The Stop arm's `close(env)` moves the
Environment out — after it, that arm never touches `env` again.

Tests:
- `run_startup::state_is_created_before_any_fallible_step` (the startup table, by name)
- `run_startup::a_start_error_exits_fatal_quiesced_without_shutdown` (ENV-START)
- `run_stop_path::stop_at_start_produces_the_three_record_journal` (RUN-GRAMMAR, RUN-RECORDS)
- `run_stop_path::stopped_carries_the_final_state` (EngineExit, by name)
- `run_stop_path::the_call_sequence_matches_env_serial` (ENV-SERIAL)

Heads up: resist "simplifying" the nested matches until it compiles as P1 has it —
then extract helpers only if they still borrow cleanly. If the borrow checker fights,
go back to P1's exact structure; don't reach for `Rc` or `Option::take` tricks.

### C28 · Loop behavior — tests only (~140 lines)

**Edit** `src/engine/engine.rs` tests only: the Continue path with Events and
Commands, over-emit, handler `Fatal`, State-on-Fatal.

Tests:
- `run_turn_loop::continue_turns_accept_events_in_sequence` (A2)
- `run_turn_loop::overflow_beats_the_returned_outcome_and_discards_the_batch` (the TurnOpen phase row, by name)
- `run_turn_loop::a_handler_fatal_discards_the_batch_and_carries_the_error` (A4)
- `run_turn_loop::state_mutations_stand_on_every_fatal_exit` (APP-STATE)
- `run_turn_loop::an_over_emitting_turn_leaves_no_command_record` (the intent-vacuum derivation, by name)

---

## Part H — Core suites (`tests/`)

### C29 · The `tests/support` harness (~150 lines)

**Create** `tests/support/mod.rs` (declarations + re-exports only) and one file per
subject:
- `scripted_env.rs` — `ScriptedEnv`: per-call scripted results, asserts every call is
  graph-conformant as it happens, records calls/handoffs/shutdown count.
- `scripted_sink.rs` — scripted write/flush results, captured bytes, call log (the
  integration-side twin of C7's unit fixture).
- `recording_app.rs` — an `Application` that records handler calls and mutates
  scripted State.
- Golden-line helpers.

**Create** `tests/harness_contract.rs` as the first consumer.

Tests:
- `scripted_environment_trace::records_each_operation_and_result_in_order` (the Trace definition, by name)
- `scripted_environment_trace::a_failed_dispatch_records_no_handoff` (the dispatch commitment row, by name)
- `scripted_sink_trace::stores_exactly_the_reported_prefix` (TRUST-SINK)

Heads up: `tests/support` is a module of each test crate, not its own crate.
`Rc<RefCell<_>>` is fine here — Engine tests are serial. Scripts own their values and
consume each step exactly once.

### C30 · Golden suite, part 1 (~140 lines)

**Create** `tests/golden_journal.rs`: full-run record sequences as **byte literals**
— every comma, field order, bare tag, schema version, trailing newline.

Tests:
- `golden_sequences::a_stop_run_writes_exactly_its_records` (VERIFY-JOURNAL)
- `golden_sequences::a_command_run_writes_exactly_its_records` (VERIFY-JOURNAL)
- `golden_sequences::an_event_run_writes_exactly_its_records` (VERIFY-JOURNAL)

### C31 · Golden suite, part 2 (~140 lines)

**Edit** `tests/golden_journal.rs`: the per-answer outcome pinning, the
interior-newline rejection through a full Engine run, `CommandsDispatched` as a legal
final record, and a table of empty/nonempty × Continue/Stop sequences.

Tests:
- `classify_call_site::each_non_fatal_answer_yields_its_required_outcome_records` (RUN-ENFORCEMENT)
- `encoding_rejection::an_interior_newline_payload_is_rejected_with_nothing_written` (VERIFY-JOURNAL)
- `fatal_tails::commands_dispatched_can_be_the_final_record` (RUN-CHECKPOINT)
- `graph_sequences::every_empty_and_command_turn_shape_has_its_required_sequence` (VERIFY-JOURNAL)

### C32 · Fault suite, part 1 — Journal faults (~150 lines)

**Create** `tests/faults.rs`: a table-driven matrix — sink failure at each record
kind, checking `JournalFatal { record_kind, outcome }`, the exit, the last committed
bytes, and the handoff count; plus `start`-`Err` proving no shutdown call, and the
Stop-path commit failure retaining `Quiesced`.

Tests:
- `journal_fault_matrix::each_record_kind_maps_to_its_journal_fatal` (VERIFY-FAULTS)
- `journal_fault_matrix::only_turn_completed_carries_an_outcome` (JournalFatal, by name)
- `journal_fault_matrix::a_stop_commit_failure_retains_quiesced` (RUN-FINALIZE)
- `startup_faults::a_start_error_performs_no_shutdown` (VERIFY-FAULTS)

### C33 · Fault suite, part 2 — Environment and Application faults (~150 lines)

**Edit** `tests/faults.rs`: each operation `Err` (`NextEvent`,
`Dispatch { position }`, `Checkpoint`, `Shutdown`), the decreasing timestamp, both
bad shutdown reports, the over-emitting Application, and the cross-product of every
post-`start` operation `Err` with a `Some(error)` report.

Tests:
- `environment_fault_matrix::each_operation_error_maps_to_its_cause_and_quiescence` (VERIFY-FAULTS)
- `environment_fault_matrix::the_operation_error_outranks_the_report_error` (A4, RUN-FINALIZE)
- `environment_fault_matrix::a_decreasing_stamp_is_time_regression` (VERIFY-FAULTS)
- `application_fault_matrix::an_over_emitting_application_is_command_bound_exceeded` (VERIFY-FAULTS)
- `application_fault_matrix::state_mutations_survive_each_post_handler_fatal_exit` (VERIFY-CONTEXT, APP-STATE)

### C34 · Conformance, within-type (~150 lines)

**Create** `tests/conformance.rs`: a catalog of scripted traces (success runs, each
failure shape), each run **twice** against freshly built everything, comparing the
full DET-RUN list: handler calls, State transitions, Command intent, Journal bytes,
DET-RUN-equal exits.

Tests:
- `conformance_within_type::the_same_trace_reproduces_identical_journal_bytes` (DET-RUN)
- `conformance_within_type::the_same_trace_reproduces_det_run_equal_exits` (DET-RUN)
- `conformance_within_type::every_environment_call_is_graph_conformant` (VERIFY-CONFORMANCE)

Heads up: rebuild Application, State, Environment, writer, and config for each run —
a reused mutated fixture doesn't test determinism.

### C35 · The compile-fail fixture (~150 lines)

**Create** `tests/compile_fail.rs` (a trybuild runner — add `trybuild` to
`[dev-dependencies]`) and the fixture crate `tests/grammar_fixture/`. **Copy probe
P6's layout** (plan doc §2): stub modules mirroring exactly the import paths
`record.rs` uses (Part F's standing rule), then
`mod engine { mod record { include!(…/src/engine/record.rs) } }`, and one `.rs` case
per attack:

illegal transition orders · a skipped checkpoint · a premature
`TurnCompleted(Stop)` · committing `CommandsDispatched` outside the transition · an
outcome disagreeing with the fixed answer · `Clone` / `Copy` / `Default` on the
certificate — plus `legal.rs`, a case that **must compile** (the control that proves
the fixture itself works).

Tests:
- `grammar_compile_fail::illegal_transitions_do_not_compile` (VERIFY-GRAMMAR)
- `grammar_compile_fail::certificate_duplication_does_not_compile` (VERIFY-GRAMMAR)
- `grammar_compile_fail::the_fixture_reconstruction_itself_compiles` (VERIFY-GRAMMAR)

Heads up: every attack must sit *inside* the reconstructed module — outside it, the
attack dies on privacy (`E0603`) instead of the grammar (`E0599`), and the test
proves nothing. P6 hit exactly this trap and shows both outcomes. Test `Copy` by
using a certificate after a move. Use trybuild's wildcards where rustc's wording is
unstable.

---

## Part I — Simulated Environment

Single-threaded first: the same Environment contract as Live, without the threads.
**C37 onward needs the wiring decisions (W1–W3, W6, W7) approved.**

### C36 · `SimPort`, `SimCtx`, lifecycle (~120 lines)

**Create** `src/sim/mod.rs` (wiring-only) and `src/sim/port.rs`: the `SimPort` trait,
`SimCtx<'a, C>` (borrows `now` and *that Port's own* arm cell — a Port can't reach
another Slot's arm by construction; `PhantomData<fn() -> C>`), `SimCtxError`, and the
crate-internal `PortLifecycle { NotStarted, Open, Ended }`.

Tests:
- `sim_ctx_wakeup::set_next_before_now_is_rejected_unchanged` (SIM-WAKEUP)
- `sim_ctx_wakeup::later_set_next_replaces_the_arm` (SIM-WAKEUP)
- `sim_ctx_wakeup::clear_next_disarms` (SIM-WAKEUP)
- `sim_ctx_wakeup::now_is_readable_during_port_code` (SimCtx::now, by name)

### C37 ⛔ Sim wiring and `start` (~160 lines)

**Create** `src/sim/error.rs` (`SimError<PE>`), `src/sim/wiring.rs` (`SimConfig`,
the two-state `SimWiring` builder — no `build` until the first `slot` call —
`SlotHandle<C>`, and the erased per-Slot runtime: boxed Port + fan-in constructor +
`err_map`), and `src/sim/env.rs` with `Environment::start`: fix `now` to the
configured origin, start each `NotStarted` Port in frozen order; on the first `Err`,
mark the failer `Ended`, `stop` exactly the already-Open prefix in order (discarding
those Errors), leave the suffix untouched, return the original Error.

Also write the trace-recording `SimPort` double in this file's test module (it moves
to `tests/support` in C42).

Tests:
- `sim_startup::ports_start_in_frozen_slot_order` (SIM-START)
- `sim_startup::failure_at_slot_k_stops_exactly_the_open_prefix_in_order` (SIM-START)
- `sim_startup::the_failing_and_unstarted_ports_receive_no_stop` (SIM-START)
- `sim_startup::startup_returns_the_original_error` (SIM-START)

Heads up: `Box<dyn Trait>` erases the per-Slot runtime; the erased trait covers
`start`/`step`/`stop` but *not* `on_command` — the payload type differs per Slot,
which is why delivery goes through `Box<dyn Any>` (next step). Move a failing entry
to `Ended` *before* its prefix cleanup so even a `stop` Error can't cause a second
lifecycle call.

### C38 ⛔ Sim `dispatch` and `take_error` (~120 lines)

**Edit** `src/sim/env.rs`:
- `dispatch`: pending-latch check first (pending → return it, no invocation); then
  the router's exhaustive match hands the typed payload to `hand_off`; the
  `on_command` *invocation* is the handoff commitment — a Port `Err` afterward is
  published to the latch and `dispatch` still returns `Ok`.
- `take_error`: one latch snapshot.

The `hand_off` downcast (`Box<dyn Any>` back to the concrete Command, guarded by the
typed `SlotHandle`) carries an always-on `expect` — a mismatch is a Kavod bug (A8).

Tests:
- `sim_dispatch::handoff_commits_at_the_on_command_invocation` (SIM-DISPATCH)
- `sim_dispatch::an_on_command_error_latches_and_dispatch_returns_ok` (SIM-DISPATCH)
- `sim_dispatch::a_pending_error_returns_first_with_no_invocation` (ENV-LATCH)
- `sim_dispatch::a_final_command_error_reaches_the_run_via_take_error` (the sim take_error note, by name)

### C39 ⛔ Selection helpers (~130 lines)

**Edit** `src/sim/env.rs`, two private final helpers:
- A **read-only scan**: find the armed Slot with the lowest time; equal times resolve
  to the first one met scanning from the cursor, wrapping once, in frozen order. No
  mutation.
- A **one-selection step**: advance `now` to the arm's time, clear the arm, assert
  the Port is `Open` (always-on), call `step`, mark `Ended` on `Err`, advance the
  cursor after every selected call, return `Selected::{Event, Continue}` for
  `Some`/`None`.

Tests:
- `sim_selection_scan::the_lowest_time_wins_and_ties_follow_the_cursor` (SIM-SELECT)
- `sim_selection_scan::the_scan_wraps_once_in_frozen_order` (SIM-SELECT, BOUND-LOOPS)
- `sim_selection_step::now_advances_and_the_arm_clears_before_step` (SIM-TIME)
- `sim_selection_step::a_selected_step_none_moves_the_cursor_and_continues` (SIM-SELECT)
- `sim_selection_step::a_step_error_ends_the_port_with_subordinate_effects_standing` (SIM-LIFECYCLE)
- `sim_selection_step::a_selected_closed_port_is_an_invariant_panic` (ASSERT-INVARIANTS) — `#[should_panic]`

Heads up: the cursor moves regardless of `Some`/`None`/`Err` after a selected call.
The scan is `(cursor + i) % len` over the frozen order.

### C40 ⛔ Sim `next_event` (~120 lines)

**Edit** `src/sim/env.rs`: the loop over C39's helpers, with the checks **in this
order, before any effect**: pending latch → nothing armed (`NothingArmed`) → step
budget (`StepBudgetExhausted`). One fresh counter per call; each selected `step`
spends one unit. Every `Err` return leaves already-made selections standing (advanced
`now`, cleared arms, spent budget).

Tests:
- `sim_next_event::budget_exhaustion_precedes_any_selection_effect` (SIM-STEPS)
- `sim_next_event::the_exact_configured_budget_is_permitted` (SIM-STEPS)
- `sim_next_event::the_budget_is_fresh_for_each_call` (SIM-STEPS)
- `sim_next_event::nothing_armed_is_the_completion_error` (SIM-COMPLETION)
- `sim_next_event::stamps_never_decrease_and_equal_stamps_are_valid` (SIM-TIME)
- `sim_next_event::an_error_leaves_completed_selections_standing` (SIM-SELECT)

### C41 ⛔ Sim shutdown (~120 lines)

**Edit** `src/sim/env.rs`: `shutdown(self)` — close admission, then one ordered loop:
each `Open` Port is marked `Ended` and `stop`ped exactly once, in frozen order, every
`Err` published first-wins, the loop never stopping early; then the final
observation closes the latch into the report. Quiescence is always `Quiesced` —
every callback has returned, so it's structural.

Tests:
- `sim_shutdown::stop_runs_once_per_open_port_in_frozen_order` (SIM-SHUTDOWN)
- `sim_shutdown::a_stop_error_does_not_prevent_remaining_stops` (SIM-SHUTDOWN)
- `sim_shutdown::the_first_stop_error_reaches_the_report` (ENV-LATCH)
- `sim_shutdown::an_all_ok_shutdown_reports_quiesced_none` (SIM-SHUTDOWN)

### C42 ⛔ The sim lifecycle suite (~150 lines)

**Create** `tests/sim_lifecycle.rs`; move the recording `SimPort` double into
`tests/support` (explicit touch). Cover the VERIFY-SIM matrix with per-Port call
traces: startup failure at every Slot position; `on_command`/`step` `Err` then
shutdown; `stop` `Ok`/`Err` at every position; `Ended` never sees another method; the
wakeup/selection/budget boundary items; storage never grows; per-Slot routing for
this suite's own wiring.

Tests (representative — grow by matrix row, each sitting green):
- `sim_lifecycle_matrix::an_ended_port_receives_no_later_method` (SIM-LIFECYCLE)
- `sim_lifecycle_matrix::startup_failure_at_every_position_cleans_exactly_the_prefix` (VERIFY-SIM)
- `sim_bounds::one_arm_per_port_never_grows` (ENV-BOUNDS)
- `sim_bounds::exact_budget_boundaries_permit_the_configured_calls` (SIM-STEPS)
- `sim_routing::each_slot_receives_only_its_commands` (TRUST-ROUTING)

### C43 ⛔ Sim conformance and the finite-source example (~130 lines)

**Edit** `tests/conformance.rs`: Engine-over-Sim end-to-end runs, run twice and
compared (DET-RUN); the byte-equal single-Port replay from the design's replay Derive
(script its three preconditions); and a finite-source example Port (terminal Event →
`Stop`) as a permanent fixture — it becomes the doc example in C57.

Tests:
- `conformance_sim::a_sim_run_repeats_byte_identically` (DET-RUN)
- `conformance_sim::a_single_port_replay_reproduces_the_recorded_run` (the replay Derive, by name)
- `conformance_sim::a_finite_source_run_ends_stopped` (finite-source pattern, by name)

---

## Part J — Live Environment

Threads at last. C44–C46 and C48 need no wiring approval; the rest does. Two standing
rules for every Live test: **no sleeps** — barriers, cue channels, and the scripted
clock control every boundary; and for races, **never assert which side wins** — only
that the returned result and the resulting state agree.

### C44 · The clock (~110 lines)

**Create** `src/live/mod.rs` (wiring-only) and `src/live/clock.rs`:
- `MonotonicClock` — a **public, documented** one-method trait
  (`fn now_nanos(&mut self) -> u64`), its contract in the doc comment: readings never
  decrease. Public on purpose: the live tests in `tests/` need to inject time, and so
  does any user testing their own Live topology.
- `StdClock` — the shipped impl: one anchored `Instant`, checked elapsed→u64
  conversion.
- Crate-internal deadline helpers: `saturating_add` for the deadline,
  `saturating_sub` for remaining time (copy from probe P5, plan doc §2).

Tests:
- `clock_stamps::production_stamps_never_decrease` (LIVE-TIME)
- `clock_stamps::conversion_exhaustion_is_a_typed_error_value` (LIVE-TIME)
- `clock_deadline::deadline_addition_saturates_at_the_domain_maximum` (A6)
- `clock_deadline::remaining_time_never_underflows` (LIVE-SHUTDOWN)

Heads up: everything Kavod compares lives on the u64-nanosecond axis; `Instant`
exists only inside `StdClock`.

### C45 · Lifecycle cell and the start/cancel gate (~130 lines)

**Create** `src/live/sync.rs`. **Copy the gate from probe P5** (plan doc §2): the
lifecycle cell (`Running`/`Shutdown` behind a lock, readable via `Lifecycle`) and the
`Mutex`+`Condvar` gate (`Pending`/`Start`/`Cancel`).

Tests:
- `start_gate::no_shell_proceeds_while_the_gate_is_pending` (LIVE-START)
- `start_gate::cancel_wakes_every_waiting_shell` (LIVE-START)
- `start_gate::start_wakes_every_waiting_shell` (LIVE-START)

Heads up: first threaded step. The condvar wait is always a loop re-checking the
predicate — spurious wakes are real.

### C46 · The central monitor (~150 lines)

**Create** `src/live/central.rs`: `Central<Ev, E>` — **one** `Mutex` + `Condvar`
owning: the bounded fan-in `VecDeque` (preallocated), the C14 `Latch<E>`, the
lifecycle mirror, and the fixed completion-entry array with its one-wake-token-per-
Slot bound (always-on assert). Operations: `offer` admission (map through the frozen
constructor first, bounded, never waits; `Full`/`Closed` return the Event to the
caller), publication (publish + notify), and the select wait predicate ("latch
pending or event available").

Tests:
- `fan_in_admission::offer_succeeds_through_exact_capacity_then_full_returns_the_event` (LIVE-EVENTS)
- `fan_in_admission::dequeue_order_is_admission_order` (LIVE-EVENTS)
- `fan_in_admission::offer_after_close_returns_closed_with_the_event` (LIVE-EVENTS)
- `select_wait::an_event_or_a_publication_wakes_the_wait` (LIVE-SELECT)

Heads up: one lock owning several facts is the design's own Justify note — implement
it verbatim. `notify_all` and filter by predicate; don't try to be clever with
`notify_one`.

### C47 ⛔ Inboxes and `LiveCtx` (~150 lines)

**Create** `src/live/inbox.rs` and `src/live/ctx.rs`:
- The per-Port bounded inbox (`Mutex`+`Condvar`, shutdown flag): blocking `recv`
  reports a raised signal *ahead of* queued Commands; `try_recv` drains Commands
  first, then reports `Shutdown`; admission never waits.
- `PortInput`, `OfferRejected`, `Lifecycle`, and `LiveCtx<C>`: the boxed offer
  closure (`Box<dyn FnMut(C::Event) -> Result<(), OfferRejected<C::Event>> + Send>` —
  it captures the fan-in constructor, so `LiveCtx` never mentions the app Event sum),
  the `Arc` of its own inbox, the `Arc` of the lifecycle cell. Not cloneable.

Tests:
- `live_ctx_signal::recv_reports_a_raised_signal_ahead_of_queued_commands` (LIVE-LIFECYCLE)
- `live_ctx_signal::try_recv_drains_commands_before_reporting_shutdown` (LIVE-LIFECYCLE)
- `live_ctx_signal::try_recv_none_means_no_command_and_no_signal` (LIVE-LIFECYCLE)
- `inbox_admission::admission_never_waits_and_full_is_refusal` (LIVE-DISPATCH)

Heads up: `recv` must wake when the signal is raised while it sleeps — the raise path
notifies every inbox.

### C48 · Supervision shell and the terminal guard (~150 lines)

**Create** `src/live/supervise.rs`:
- The shell fn each spawned thread runs: wait at the gate → on `Cancel` return, on
  `Start` call `LivePort::run` → classify the result per LIVE-SUPERVISION and publish
  under the central lock.
- The terminal guard: non-cloneable, lives **only on the shell's stack frame** (the
  Port and `LiveCtx` can never touch it). Its `Drop`: publish a pre-signal unwind's
  premature-closure Error first, then flip the Slot's completion entry exactly once
  (always-on assert: it was `Outstanding`), then one nonblocking wake.

Publish-then-complete under the one lock is probe P8's discipline — read it first.

Tests:
- `supervision_completion::each_terminal_path_completes_the_entry_exactly_once` (LIVE-COMPLETION)
- `supervision_completion::every_required_publication_precedes_complete` (LIVE-SUPERVISION)
- `supervision_completion::a_premature_ok_publishes_and_wakes_the_select` (LIVE-SUPERVISION)
- `supervision_completion::a_pre_signal_unwind_publishes_premature_closure` (LIVE-SUPERVISION)
- `supervision_completion::a_post_signal_ok_stays_unpublished` (LIVE-SUPERVISION)

Heads up: no `catch_unwind` — the guard's `Drop` runs during test-profile unwind by
itself; that's the whole trick.

### C49 ⛔ Live wiring and `start` (~160 lines)

**Create** `src/live/error.rs` (`LiveError<PE>` — `InboxFull`, `SpawnFailed`,
`TimeExhausted`, `PrematureClosure`, `Port(PE)`; no closed-inbox variant, see the W3
note in the plan doc), `src/live/wiring.rs` (`LiveConfig`, the two-state `LiveWiring`
builder — `slot` makes the typed inbox + `SlotHandle<C>` + spawn closure;
`build(config, router)` and the documented `build_with_clock(config, router, clock)`),
and `src/live/env.rs` with `Environment::start` per the Mechanism's six steps: create
shared state → spawn named shells (`kavod-<slot-name>`) in frozen order, parked at
the gate → finish all fallible setup → stamp and freeze the start time → on any
failure cancel, join every shell, return the original `Err` → otherwise signal
`Start` (the commitment).

Tests:
- `live_startup::no_port_code_runs_before_gate_activation` (LIVE-START)
- `live_startup::failed_setup_cancels_joins_every_shell_and_errs` (LIVE-START)
- `live_startup::spawn_failure_maps_to_its_slot_name` (LiveError::SpawnFailed, by name)
- `live_startup::the_frozen_start_time_is_returned_after_activation` (LIVE-START)

Heads up: `thread::Builder::name(…).spawn` returns `io::Result<JoinHandle>` — keep
the handles in frozen order from the moment of spawn.

### C50 ⛔ Live `next_event`, `dispatch`, `take_error` (~150 lines)

**Edit** `src/live/env.rs`:
- `next_event`: wait on C46's predicate; a pending latch Error is taken first; the
  stamp is taken *after* the wait and *before* the dequeue (a stamp failure leaves
  the Event queued); the dequeue is the consumption — nothing fallible after it.
- `dispatch`: pending latch first; the router's match; one non-waiting admission to
  the destination inbox — full → typed `Err`, nothing handed off.
- `take_error`: one snapshot.

Write a small scripted-clock fixture in the test module (the `tests/`-side twin
arrives in C52).

Tests:
- `live_acceptance::the_stamp_is_taken_after_the_wait_before_the_dequeue` (LIVE-SELECT)
- `live_acceptance::time_exhaustion_leaves_the_event_queued` (LIVE-SELECT)
- `live_acceptance::a_waking_event_is_stamped_no_earlier_than_its_admission` (LIVE-SELECT)
- `live_dispatch::a_full_inbox_is_a_typed_error_with_no_handoff_or_growth` (LIVE-DISPATCH)
- `live_dispatch::a_pending_error_returns_before_any_routing` (ENV-LATCH)

Heads up: lock order — never take the central lock while holding an inbox lock. Don't
hold the central lock across the router call; re-take it for the admission commit.
Hold the queue lock across stamp-and-dequeue so the selected front stays put.

### C51 ⛔ Live shutdown (~160 lines)

**Edit** `src/live/env.rs`: `shutdown(self)` in three movements, all under P8's
one-lock discipline:
1. **Initiate** (one critical section): raise the signal, end `Running`, close the
   fan-in, notify every blocking point, fix the one saturated deadline. Nothing ever
   restarts it.
2. **Wait**: scan the fixed completion set; wait (`wait_timeout` in a loop against
   the remaining-time helper) while any entry is `Outstanding` and budget remains;
   consume at most one wake token per entry.
3. **Final observation** (one critical section): decide `Quiesced`/`Incomplete`,
   close the latch into the report. Then: `Quiesced` → join every handle in frozen
   order; `Incomplete` → drop the unjoined handles (detach).

Split signal: land 1+2 in one sitting, 3+joins in the next — each half tests alone.

Tests:
- `live_shutdown::one_deadline_fixed_at_initiation_governs_every_wait` (LIVE-SHUTDOWN)
- `live_shutdown::quiesced_joins_every_supervised_thread` (LIVE-SHUTDOWN)
- `live_shutdown::expiry_detaches_unjoined_threads_and_reports_incomplete` (LIVE-SHUTDOWN)
- `live_shutdown::the_latch_stays_open_through_the_window` (ENV-SHUTDOWN)
- `live_shutdown::a_completion_during_the_wait_ends_the_wait_promptly` (LIVE-SHUTDOWN)

Heads up: don't cache a completion count — scan the fixed set each time; a cached
count is a second invariant that can drift.

### C52 ⛔ Live suite, part 1 — lifecycle, supervision, completion (~150 lines)

**Create** `tests/live_lifecycle.rs`; grow `tests/support` (explicit touch) with the
Live doubles: `CuePort` (blocks on a cue channel, releases on command), `ErrPort`,
`UnwindPort`, `PrematureOkPort`, the scripted `MonotonicClock`, and barrier helpers.

Tests (representative):
- `live_gate::no_run_begins_before_gate_activation` (VERIFY-LIVE)
- `live_gate::failed_startup_cancels_and_joins_every_shell` (VERIFY-LIVE)
- `live_completion::normal_err_and_unwind_each_complete_exactly_once` (VERIFY-LIVE)
- `live_completion::a_completion_before_shutdown_remains_visible_at_the_final_observation` (VERIFY-LIVE)
- `live_completion::port_code_cannot_reach_the_terminal_guard` (VERIFY-LIVE)

### C53 ⛔ Live suite, part 2 — events, select, dispatch, bounds (~140 lines)

**Edit** `tests/live_lifecycle.rs`: capacity boundaries, admission order,
stamp-vs-admission ordering, exhaustion-leaves-queued, exactly-once admission, and
no-storage-growth — all under the scripted clock.

Tests (representative):
- `live_bounds::fan_in_and_inbox_occupancy_never_exceed_capacity` (ENV-BOUNDS)
- `live_bounds::completion_and_wakeup_storage_never_grows_past_one_per_slot` (ENV-BOUNDS)
- `live_select_suite::a_blocked_next_event_wakes_on_publication` (VERIFY-LIVE)

### C54 ⛔ Live suite, part 3 — shutdown, deadline, races (~150 lines)

**Edit** `tests/live_lifecycle.rs`: signal-ahead-of-commands, window observability,
`run(Ok)`-after-signal unpublished, `run(Err)`-before-close reported, saturation,
no-join-while-outstanding, both final-observation race classifications,
`{ Incomplete, None }`, `{ Incomplete, Some }`, post-close discard, detach, and
per-Slot routing for this suite's wiring.

Tests (representative):
- `live_shutdown_suite::a_port_blocked_in_recv_observes_shutdown_within_the_window` (VERIFY-LIVE)
- `live_shutdown_suite::error_plus_expiry_reports_incomplete_with_the_first_publication` (VERIFY-LIVE)
- `live_shutdown_suite::a_post_close_publication_is_discarded` (VERIFY-LIVE)
- `live_shutdown_suite::races_at_the_final_observation_are_classified_by_it` (VERIFY-LIVE)
- `live_shutdown_suite::a_joined_panicked_thread_is_quiesced_not_succeeded` (VERIFY-LIVE)

### C55 ⛔ The latch suite — both Environments (~150 lines)

**Create** `tests/latch.rs`: the ordering suite, run against Live *and* Sim through
public API only (that's what later lets it certify a bespoke Environment for
TRUST-ENV): publication before-call/after-return placement; overlaps accepted either
way with result/state agreement; pending-beats-own-Error; the blocked `next_event`
wake; permanence; the sim final-Command observation; open-through-shutdown;
racing-the-close; post-close discard; and the three Stop-report integrations
(`{Quiesced, None}` → `Stopped`; `Some(error)` → `Environment(Shutdown)` even with
`Incomplete`; `{Incomplete, None}` → `Core(ShutdownIncomplete)`).

Tests (representative):
- `latch_ordering::a_pending_error_wins_over_the_operations_own_failure` (ENV-LATCH)
- `latch_ordering::a_blocked_next_event_returns_the_error_that_wakes_it` (ENV-LATCH)
- `latch_stop_path::only_a_clean_report_reaches_stopped` (VERIFY-LATCH)
- `latch_stop_path::a_report_error_is_environment_shutdown_even_with_incomplete` (VERIFY-LATCH)

### C56 ⛔ Cross-type conformance (~130 lines)

**Edit** `tests/conformance.rs`: equal scripted traces driven through Live (cue
ports) and Sim, comparing every Core-owned discriminant and payload in DET-ENV's
list; Journal bytes equal through the last committed record; Environment-specific
failure shapes named and excluded.

Tests:
- `conformance_cross_type::equal_traces_produce_equal_core_owned_outputs` (DET-ENV)
- `conformance_cross_type::equal_traces_produce_equal_journal_bytes` (DET-ENV)
- `conformance_cross_type::environment_specific_failure_shapes_are_not_compared` (DET-ENV)

Heads up: what must match is the *trace*, not the clock — the cue ports deliver a
scripted Event sequence.

---

## Part K — Close

### C57 · Export audit, docs, CI notes (~80 lines)

**Edit** `src/lib.rs` (final touch): audit the flat re-exports — one path per public
item, no repeated segments, nothing `#[doc(hidden)]` anywhere. Crate-level rustdoc
with the finite-source example from C43. Note the TRUST-ABORT CI build-profile check
as a deployment TODO.

**Create** `tests/exports.rs`:
- `crate_exports::every_public_item_is_reachable_without_repeated_segments` (CRATE-EXPORTS) — one `use` list that fails to compile if a path regresses.

Done. 🎉

---

*If a step fights you — borrow-checker walls, a flaky Live test, a macro that won't
parse — the plan doc's risk list (§5) has the symptom, the fallback, and the probe
that already solved it. Don't improvise around a fight; look it up first.*
