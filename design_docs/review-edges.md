# Edge-case review

Each group's attack list was written from its production code and design rows before
any test was read, then every attack was resolved against test bodies. *Pinned* names
the test. *By construction* means a type or standard-library guarantee carries it.
*Derived* names the facts that compose to it. *Asserted* names the always-on site.
Only open attacks are findings.

## foundations

Reviewed 2026-09-08: `src/time.rs`, `src/bounded_buffer.rs`. Design rows A6,
`BOUND-NONZERO`, `BOUND-LOOPS`, `ENV-TIME`, `RUN-INDEX`, and the wire form of both
types. Green before the round opened.

**Result: no defects, no open attacks.** Every edge resolves to a test, a type, a
standard-library guarantee, or an assertion. Three items outside the open-attack rule
were raised and decided below.

### Time types

| # | Attack | Resolution |
|---|---|---|
| T1 | `checked_add(Duration::ZERO)` at 0 and at `u64::MAX` returns self | by construction — `u64::checked_add`; the mid-domain case is `timestamp_arithmetic::zero_elapsed_preserves_the_timestamp` |
| T2 | sum exactly `u64::MAX`; sum one past | pinned — `timestamp_arithmetic::exact_domain_maximum_sum_succeeds`, `::overflowing_sum_returns_none` |
| T3 | a Duration of exactly `u64::MAX` ns passes the conversion | pinned — `exact_domain_maximum_sum_succeeds` (0 + `from_nanos(u64::MAX)`) |
| T4 | a Duration of 2^64 ns from `Timestamp(0)`: a truncating cast would return `Some(0)`, a saturating one `Some(MAX)` | pinned — `timestamp_arithmetic::oversized_duration_returns_none` starts at 0, so both mutations fail it |
| T5 | `Duration::MAX` | by construction — the same `try_from` arm as T4 |
| T6 | a failed add leaves the original usable | pinned — `timestamp_arithmetic::failed_addition_leaves_original_timestamp_unchanged`; `Copy` |
| T7 | `Ord`/`Eq` compare the raw count; equal stamps compare equal | by construction — derive. Equal-stamp acceptance is pinned by `event_acceptance::an_equal_stamp_is_accepted` and `golden_sequences::repeated_events_advance_indices_and_preserve_time_boundaries` |
| T8 | both types serialize as bare integers; 0 and `u64::MAX` exact | pinned — `index_and_time_wire::both_serialize_as_transparent_u64`, `::maximum_values_serialize_without_loss`; 0 by `golden_journal.rs` |
| T9 | wire form through a record, at the ceiling | pinned — `golden_sequences::a_stop_run_writes_exactly_its_records`, `::an_event_run_writes_exactly_its_records`; `record_payload_wire::event_accepted_serializes_maximum_index_and_time_without_loss` |
| T10 | `EventIndex` round trip at 0 and `u64::MAX` | pinned — `index_and_time_accessors::event_index_round_trips_domain_boundaries` |
| T11 | no public `EventIndex` constructor; `from_nanos` is public | by construction — `pub(crate)`; every `tests/` suite calls `from_nanos` from outside the crate |
| T12 | a bare `Timestamp` committed alone is `NotAnObject` | derived — T8 + `journal_object_validation::every_non_object_json_kind_is_rejected` |

### Bounded buffer

| # | Attack | Resolution |
|---|---|---|
| B1 | capacity 0: first push refused with its value; first write `WriteZero` | pinned — `bounded_buffer_capacity::zero_capacity_refuses_first_push_and_returns_value`, `encode_buffer_write::zero_capacity_rejects_all_writes` |
| B2 | capacity 1: one push holds, second refused; one byte accepted, next write `WriteZero` | pinned — `bounded_buffer_capacity::refused_push_preserves_full_buffer_state`, `encode_buffer_write::one_byte_capacity_accepts_exactly_one_byte`, `::empty_write_returns_zero_only_while_capacity_remains` |
| B3 | `new(usize::MAX)` for `u8` fails, nothing constructed, error passed through | pinned — `bounded_buffer_capacity::construction_failure_reports_the_reservation_error`; through `Journal::new` by `journal_construction::failed_reservation_is_allocation_failed` |
| B4 | `new(usize::MAX)` for a zero-sized `T` succeeds | by construction — `Vec<ZST>` capacity is `usize::MAX` |
| B5 | N pushes never grow the allocation | derived — the reservation assertion in `new` + `Vec::push` grows only at `len == capacity()`; now also asserted on the push path (N2) |
| B6 | push at capacity: same value back, contents and allocation unchanged | pinned — `bounded_buffer_capacity::push_beyond_capacity_is_refused_without_growth`, `::refused_push_preserves_full_buffer_state` |
| B7 | order preserved through capacity | pinned — `bounded_buffer_capacity::pushes_up_to_capacity_preserve_order` |
| B8 | write on full: `WriteZero`, contents intact, next write also `WriteZero` | pinned — `encode_buffer_write::full_buffer_returns_write_zero`; two consecutive rejections by `::zero_capacity_rejects_all_writes` |
| B9 | empty slice: `Ok(0)` with room, `WriteZero` when full | pinned — `encode_buffer_write::empty_write_returns_zero_only_while_capacity_remains` |
| B10 | a write longer than the room takes exactly the room; suffix untouched; next write `WriteZero` | pinned — `encode_buffer_write::partial_writes_accumulate_without_loss`, `::write_all_one_past_capacity_retains_accepted_prefix`; suffix by construction |
| B11 | a write that exactly fits: `Ok(len)`, full afterwards | pinned — `encode_buffer_write::serde_json_encode_completes_at_exact_region_size`, `::one_byte_capacity_accepts_exactly_one_byte` |
| B12 | as a sink: never over-reports, `Ok(0)` only for empty input, never `Interrupted`, never `Err` after consuming bytes in one call | by construction — `remaining.min(bytes.len())`; the only `Err` precedes `extend_from_slice` |
| B13 | write then `try_push` (the Journal's newline): room 1 fills, room 0 refuses | pinned — `journal_newline_reservation::encode_at_exactly_max_bytes_completes`, `::object_of_region_size_has_no_newline_room` |
| B14 | `try_push` then write share one accounting | by construction — one `len`, one `capacity`; no caller interleaves in this order |
| B15 | `write_all` one past capacity keeps the accepted prefix | pinned — `encode_buffer_write::write_all_one_past_capacity_retains_accepted_prefix` |
| B16 | clear on full and after `WriteZero`: empty, allocation kept, pushes and writes resume | pinned — `bounded_buffer_reuse::clear_empties_buffer_for_reuse`, `::clear_and_drain_retain_capacity`, `encode_buffer_write::clear_after_write_zero_restores_writes` |
| B17 | drain consumed fully: empty, allocation kept, refill works | pinned — `bounded_buffer_reuse::drain_yields_owned_values_in_order_and_allows_refill`, `::clear_and_drain_retain_capacity` |
| B18 | drain dropped part way removes the rest before the caller's `?` return completes | pinned — `bounded_buffer_reuse::dropped_partial_drain_removes_remaining_values`; through dispatch by `batch_dispatch::error_at_position_k_keeps_the_prefix_and_discards_the_suffix` |
| B19 | `flush` always `Ok`, even when full | pinned — `encode_buffer_flush::flush_succeeds_without_mutating_any_fill_state` |
| B20 | `len <= capacity` before every mutation; no growth after a write or a push | asserted — `new`, `try_push`, `write` |
| B21 | a serde payload failing mid-encode leaves the prefix; the next encode clears it | by construction at the buffer; through the Journal by `journal_line_errors::serializer_failure_leaves_journal_reusable`, `::raw_bound_failure_leaves_journal_reusable` |
| B22 | `BOUND-NONZERO`: zero reaches the buffer only from tests | by construction — both `EngineConfig` fields are `NonZeroUsize`; the Journal region is `max + 1` |
| B23 | `BOUND-LOOPS`: the group owns no loop | by construction — `extend_from_slice` and `Drain` are bounded by `accepted` and `len` |

### Items decided

**N1 — landed.** `timestamp_arithmetic::equal_timestamp_is_valid` carried
`Design Doc: ENV-TIME` but pinned only `checked_add(Duration::ZERO)`; `ENV-TIME` is the
acceptor's nondecrease, pinned in `event_acceptance` and the golden repeated-events run.
Renamed to `zero_elapsed_preserves_the_timestamp`, citation dropped, `review-ledger.md`'s
`ENV-TIME` row trimmed to the test that pins it. `impl-steps.md` and `impl-plan-v12.md`
keep the old name as the record of their step.

**N2 — landed.** `try_push` now asserts, like `write`, that the push left the backing
allocation unchanged, naming A6. Mutation: `shrink_to_fit` inserted before the push, so
every push had to grow the allocation; the assertion failed all seven push-path tests in
`bounded_buffer::tests` at the new site. Reverted.

**N3 — declined.** The fixture's hand copy of `BoundedBuffer` returns `Self` from `new`
and omits `len`, `capacity`, `clear`, and the assertions. No signature changed in this
round, so nothing needs mirroring; realigning it would touch every case file under
`tests/grammar_fixture/cases/` for no behavioral gain. Devon delegated the call.

### Considered, not tested

- **`Timestamp(u64::MAX) + Duration::ZERO`.** `u64::checked_add` at the ceiling; a test
  would restate the standard library.
- **A zero-sized Command type at `NonZeroUsize::MAX`.** `Vec<ZST>` reserves nothing and
  reports `usize::MAX` capacity; the logical bound alone governs.

## journal

Reviewed 2026-09-08: `src/journal.rs`. Design rows `JRN-FORMAT`, `JRN-ENCODE`,
`JRN-COMMIT`, `JRN-POISON`, `JRN-SINK`, `TRUST-SINK`, `TRUST-SERIALIZE`, A3, A8. Green
before the round opened. Suites read: the file's nine test modules,
`faults::journal_fault_matrix`, `golden_journal::encoding_rejection`,
`tests/support/scripted_sink.rs`.

**Result: no defects, one open attack, landed.** The fixture's `Journal::new` and
`commit` are unchanged; no signature moved.

### Journal

| # | Attack | Resolution |
|---|---|---|
| N1 | `max = 1`: `{}` fits, newline has no room; `max = 2` commits `{}` | pinned — `journal_newline_reservation::object_of_region_size_has_no_newline_room`, `::encode_at_exactly_max_bytes_completes` |
| N2 | `max = usize::MAX` overflows the region size | pinned — `journal_construction::region_size_overflow_is_max_bytes_too_large` |
| N3 | `max = usize::MAX - 1`: reservation fails, not overflow | pinned — `journal_construction::failed_reservation_is_allocation_failed` |
| N4 | `new` touches the sink | pinned — `journal_sink_writes::empty_line_completes_without_sink_calls` builds on an unscripted sink that panics on any call |
| N5 | fresh Journal poisoned | pinned — `journal_construction::fresh_journal_is_not_poisoned` |
| N6 | region is not exactly `max + 1` | pinned — `journal_construction::minimum_record_bound_reserves_object_plus_newline_region`; `journal_newline_reservation::consecutive_exact_maximum_lines_reuse_full_region` at 7 |
| C1 | poisoned commit: panic, no sink call | asserted — `commit`'s first statement; pinned `journal_poisoning::commit_on_poisoned_journal_panics`, `journal_sink_matrix::every_sink_failure_poisons_exactly_once` |
| C2 | poisoned commit with a record that cannot encode | **open — E1, landed** |
| C3 | `Encode`: nothing written, not poisoned, region cleared, next commit exact | pinned — `journal_commit::every_encode_error_skips_sink_and_allows_later_commit`, `journal_encoding::serializer_failure_clears_previous_bytes_and_region_remains_reusable` |
| C4 | null, bool, number, string, array at top level | pinned — `journal_object_validation::every_non_object_json_kind_is_rejected` |
| C5 | unit, tuple, newtype, unit-variant payloads | by construction — serde_json's rules; on `review-adversarial.md`'s "Considered, not tested" list |
| C6 | object of `max + 2` → `BoundExceeded` via `WriteZero` | pinned — `journal_line_errors::raw_bound_failure_leaves_journal_reusable`, `journal_encoding::oversized_record_is_bound_exceeded_without_sink_calls` |
| C7 | object of exactly `max + 1` → `BoundExceeded` via `try_push` | pinned — `journal_newline_reservation::object_of_region_size_has_no_newline_room`, `::valid_object_encodes_after_missing_newline_room` |
| C8 | object of exactly `max` commits | pinned — `journal_newline_reservation::encode_at_exactly_max_bytes_completes` |
| C9 | non-object of exactly `max + 1` → `NotAnObject` | pinned — `journal_object_validation::non_object_top_level_is_rejected` |
| C10 | non-object of `max + 2` → `BoundExceeded` before classification | pinned — `journal_object_validation::an_overrunning_non_object_is_bound_exceeded_before_classification` (S2) |
| C11 | raw interior newline; escaped `\n` in a string | pinned — `journal_object_validation::interior_newline_is_not_an_object`, `::ordinary_string_newline_is_escaped_and_allowed`; through the Engine in `golden_journal::encoding_rejection` |
| C12 | two objects on one line pass the three byte checks | by construction — `RawValue::from_string("{} {}")` is rejected as trailing characters (verified against the locked serde_json); one `Serialize` call yields one value. A `SerializeMap` that skips `serialize_value` yields `{"a"}` and passes; that breaks serde's own trait contract, the payload author's obligation |
| C13 | `Serialize` that swallows the buffer's error and returns `Ok` | derived — truncation happens only at a full region, and a full region is C7 or C9 |
| C14 | `Serialize` that emits nothing | by construction — `S::Ok` is reachable only through a serializer method, each of which writes at least one byte |
| C15 | serde `Encode` mid-value leaves partial bytes in the region | pinned — `journal_encoding::serializer_failure_clears_previous_bytes_and_region_remains_reusable` asserts the region is empty afterward |
| C16 | long record then short record: stale tail | pinned — `journal_encoding::successive_encodes_replace_previous_bytes`, `journal_newline_reservation::consecutive_exact_maximum_lines_reuse_full_region` |
| W1 | `Ok(0)` first; after progress | pinned — `journal_sink_writes::zero_progress_maps_to_write_zero`; `every_sink_failure_poisons_exactly_once` rows 3 and 6 |
| W2 | over-report first; against the suffix; count exactly equal to the suffix | pinned — `journal_sink_writes::over_reported_count_maps_to_invalid_data`, `::over_reported_count_is_measured_against_remaining_suffix`; the equal case is the last call of `::short_successful_writes_are_retried_to_completion` |
| W3 | `count = usize::MAX` overflows `offset += count` | derived — the `count > remaining_len` arm precedes the add; W2 pins the arm |
| W4 | `Interrupted` first; after progress | pinned — `journal_sink_writes::interrupted_write_poisons_without_retry`; after progress is the same `Err` arm, matrix row 5 |
| W5 | one-byte writes: n calls, exact suffixes, no flush until done | pinned — `journal_sink_writes::short_successful_writes_are_retried_to_completion`, `journal_commit_boundaries::flush_failure_after_short_writes_leaves_a_complete_uncertain_line` |
| W6 | `Err` after progress: poisoned, no flush, sink holds the prefix | pinned — matrix row 5; `journal_commit_boundaries::partial_second_write_failure_preserves_the_prior_boundary`; `faults::a_partial_write_failure_preserves_the_prior_commit_boundary` |
| W7 | error kind and message survive `Sink { .. }` | pinned — `journal_sink_writes::interrupted_write_poisons_without_retry`, `journal_commit::flush_failure_is_sink_flush_and_uncommitted` |
| W8 | empty region reaches `write_line` | pinned — `journal_sink_writes::empty_line_completes_without_sink_calls`; unreachable through `commit` |
| F1 | flush `Err`: poisoned, full line is an uncertain suffix | pinned — `journal_commit_boundaries::only_successful_flush_advances_the_committed_boundary`; `faults::flush_failures_leave_the_failed_record_uncommitted` |
| F2 | flush `Interrupted` not retried | derived — flush is one `map_err` with no loop; matrix row 8 pins the arm |
| F3 | flush exactly once, after the last write, never after a write failure | pinned — `journal_commit::successful_flush_commits_exactly_the_line`, `::sink_error_poisons_permanently` |
| L1 | second record fails at first byte, last byte, flush: first line intact | pinned — all four of `journal_commit_boundaries`; `faults::each_record_kind_maps_to_its_journal_fatal` |
| L2 | poison outlives the sink's recovery | pinned — `every_sink_failure_poisons_exactly_once` retries every row |
| L3 | drop performs a sink operation | derived — no `Drop` impl; every scripted test drops a Journal whose sink panics on an unscripted call |
| L4 | writer dropped on a `new` failure | by construction — the design's `JournalBuildError` carries no writer |

### Items decided

**E1 — landed.** Every poisoned retry in the suite committed `json!({})`, which encodes
cleanly, so nothing pinned that the poison check precedes encoding. New test
`journal_poisoning::poisoned_commit_panics_before_encoding_the_record` commits a
payload whose `Serialize` always fails on a poisoned Journal and expects the
`JRN-POISON` panic. Mutation: the assertion moved below `encode_line`; the new test
failed alone, the other 50 journal tests stayed green. Reverted.

## contracts

Reviewed 2026-09-08: `src/application.rs`, `src/port.rs`, `tests/ports_macro.rs`.
Design rows `APP-CONTEXT`, `APP-EMIT`, `APP-OVERFLOW`, `APP-FUTURE`, `APP-STATE`,
`PORT-SUMS`, `PORT-ROUTING` (Core half), `TRUST-PURE`, `VERIFY-CONTEXT`. Green before
the round opened. Suites read: both files' test modules, `tests/ports_macro.rs`,
`faults::application_fault_matrix`, `tests/conformance.rs`,
`tests/support/recording_app.rs`, `tests/compile_fail.rs`; the Context and Port macro
tables in `review-adversarial.md`. G9 (`APP-FUTURE` has no named enforcement site) is
open and Devon's decision; not re-raised.

The `io::Write` contract and serde mid-serialization corners are unreachable here:
nothing in the group writes or serializes, and `Never::serialize` is uninhabited.

**Result: one defect, landed.** No signature moved; the fixture is untouched.

### Context

| # | Attack | Resolution |
|---|---|---|
| C1 | fresh `Context` over a buffer holding leftovers, after an overflowed and after a clean turn | pinned — `context_reuse::fresh_invocation_starts_empty_with_a_clear_marker`, `::fresh_invocation_clears_a_non_overflowed_batch` |
| C2 | N emits at capacity N: call order kept, marker clear, `remaining` counts N..0 | pinned — `context_emit::commands_append_in_call_order_through_exact_capacity`, `::remaining_reports_exact_free_capacity`; `context_overflow::exact_capacity_keeps_overflow_marker_clear` |
| C3 | emit N+1 stores nothing and sets the marker; N+2 stores nothing; `remaining` is 0 | pinned — `context_overflow::first_over_bound_emit_stores_nothing_and_sets_the_marker`, `::every_later_emit_stores_nothing`, `::remaining_is_zero_once_the_marker_is_set` |
| C4 | smallest capacity (1) with one and two emits | pinned — `context_emit::one_slot_capacity_accepts_one_command_without_overflow`; two emits inside `every_later_emit_stores_nothing` |
| C5 | capacity 0: `remaining` 0 with the marker clear, first emit sets it | pinned in-crate — `context_emit::zero_capacity_rejects_first_command_and_sets_marker`; unreachable through the Engine (`NonZeroUsize`) |
| C6 | ownership of a rejected Command, first and later: dropped before `emit` returns | pinned — `context_overflow::rejected_commands_are_dropped_immediately` |
| C7 | index and time at 0 and `u64::MAX`; unchanged after overflow | pinned — `context_observers::index_and_logical_time_report_exact_boundary_values`, `::index_and_logical_time_remain_stable_after_overflow` |
| C8 | a handler that reads `remaining() == 0` at exact capacity and stops emitting sees no Fatal | derived — C2's zero `remaining` composed with the clear marker in `exact_capacity_keeps_overflow_marker_clear` |
| C9 | a zero-sized Command type at capacity: the bound still counts | by construction — `BoundedBuffer::try_push` checks the logical count, never `Vec::capacity` |
| C10 | buffer length above capacity inside `Context` | by construction — `BoundedBuffer` is the only constructor; asserted besides at `application.rs:74` |
| C11 | overflow with `Continue`, `Stop`, `Fatal`; the Fatal payload's drop; State stands; batch cleared; start turn and later index | pinned — `turn_overflow_precedence::overflow_outranks_the_returned_outcome`, `::later_index_overflow_outranks_a_fatal_outcome` |
| C12 | overflow through the Engine on the start turn and an Event turn: nothing handed off, the prior turn's handoffs and records stand | pinned — `application_fault_matrix::an_over_emitting_application_is_command_bound_exceeded`, `::event_turn_overflow_preserves_prior_effects_and_dispatches_nothing_new` |
| C13 | State mutation stands on every Fatal exit, the failing handler's own included | pinned — `application_fault_matrix::state_mutations_survive_each_post_handler_fatal_exit` |
| C14 | State mutation stands on `Stop` and `Continue` exits | pinned — `graph_sequences::every_empty_and_command_turn_shape_has_its_required_sequence` |
| C15 | Application `Fatal` at exact capacity, start and Event turn: exact Error kept, batch discarded, one shutdown | pinned — `application_fault_matrix::an_application_fatal_preserves_its_error_and_discards_its_batch` |
| C16 | `Context` reports the certificate's index and time through the Engine: 0 with the start stamp, k with the Event's stamp | pinned — `app_calls` in `event_turn_overflow_preserves_prior_effects_and_dispatches_nothing_new`; every `golden_journal` trace |
| C17 | `initial_state` called exactly once, before `on_start` | pinned — every `AppCall` trace opens with one `InitialState` |
| C18 | `TRUST-PURE`'s stated verification: two runs, same scripted Environment and sink, identical bytes and `DET-RUN`-equal exits | pinned — `conformance_within_type::the_same_trace_reproduces_identical_journal_bytes`, `::the_same_trace_reproduces_det_run_equal_exits` |
| C19 | a handler reaches capabilities through `&self` (the recording fake itself does) | trusted — `TRUST-PURE`; the Core has no guard and claims none |
| C20 | a handler panics mid-emit | by construction — no Core site catches unwinding; no row claims otherwise |
| C21 | `APP-FUTURE`: a channel other than an External Event | G9, open, Devon's decision |

### Port macro

| # | Attack | Resolution |
|---|---|---|
| P1 | one Slot without a trailing comma; two Slots with one | pinned — `ports_macro_expansion::single_slot_without_trailing_comma_expands`; the `Reused` fixture |
| P2 | private, `pub(super)`, and downstream visibility | pinned — `Reused` (no vis), `receive_only_fixture` (`pub(super)`), `ports_macro_downstream::consumer_invocation_compiles_and_serializes` |
| P3 | two Slots of one Contract are distinct variants | pinned — `ports_macro_expansion::contract_bound_at_two_slots_yields_two_variants` |
| P4 | `Command = Never` arm discharged, in-crate and downstream | pinned — `never_direction::never_command_arm_is_discharged_by_match`, `ports_macro_downstream::receive_only_never_arm_is_discharged_downstream` |
| P5 | `Event = Never` | by construction — the same substitution into the other generated enum |
| P6 | Slot name as the sole outer tag; hand-written sum byte-identical | pinned — `ports_macro_expansion::generated_sums_are_externally_tagged`, `::hand_written_equivalent_is_byte_identical`, `ports_macro_downstream::every_generated_variant_serializes_with_its_own_slot_tag` |
| P7 | frozen fan-in constructors and an exhaustive fan-out downstream | pinned — `ports_macro_downstream::the_fanout_match_is_exhaustive` |
| P8 | `$crate::PortContract` resolves with nothing imported downstream | pinned — `tests/ports_macro.rs` compiles with no `use` of the trait |
| P9 | zero Slots; a duplicate Slot name; a Contract type outside `PortContract`; a payload without `Serialize` | by construction — the `+` repetition, E0428, the trait bounds on `PortContract::Event` and `::Command` |
| P10 | the declaration ident binds no item | pinned — `ports_macro_expansion::declaration_name_is_available_for_an_independent_item` |
| P11 | `Never::serialize` reached | by construction — uninhabited |
| P12 | a hand-written sum over `Never` "may add derives freely" (Port Mechanism prose) | **open — E1, landed** |
| P13 | a consumer without a direct `serde` dependency | unreachable in this package; doc-noted at the Port Mechanism |

### Items decided

**E1 — landed.** `Never` derived nothing, so a hand-written sum with a `Never` payload
failed `#[derive(Debug)]`, `Clone`, `PartialEq`, and the rest — E0277 and E0369 for
every receive-only or send-only Contract, against the Port Mechanism's "may add derives
freely". `Never` now derives `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash`,
the set `core::convert::Infallible` carries. New test
`never_derives::a_hand_written_sum_over_never_accepts_the_standard_derives` derives that
set on a hand-written receive-only Command sum and requires it through a bound. Mutation:
the test was landed before the derive line; the lib test binary failed to compile with
eight errors, one per missing trait plus E0204 for `Copy`. The derive line was then
added and every test passed.

`src/port.rs` and `tests/ports_macro.rs` carried pre-existing `rustfmt` drift and were
formatted; `src/environment.rs` carries the same drift and was left alone.

## environment

Reviewed 2026-09-08: `src/environment.rs`, `src/latch.rs`. Design rows `ENV-SERIAL`,
`ENV-START`, `ENV-ERRORS`, `ENV-LATCH`, `ENV-TIME`, `ENV-SHUTDOWN`, A4. Green before the
round opened. Suites read: `latch::tests` (five modules), `environment::tests`,
`tests/support/scripted_env.rs`, `harness_contract::scripted_environment_{trace,graph}`,
`faults::{startup_faults,environment_fault_matrix}`, the `stop_pending` and
`accept_event` tests in `record.rs`, the `conformance.rs` scripts, the `golden_journal`
call lists; the Latch and Environment tables and S3 in `review-adversarial.md`. Greps:
`take_error`, `TimeRegression`, `ShutdownIncomplete`, `Incomplete`,
`EnvCall::TakeError`, `equal.*stamp` across `src/` and `tests/`. `Latch` has no caller
yet; its `dead_code` allowance and `lib.rs` re-export were left alone. The `io::Write`
and serde corners are unreachable: nothing in the group writes or serializes.
`ENV-SEPARATION`, `ENV-BOUNDS`, `VERIFY-LATCH`, the `ENV-LATCH` ordering anchors, and
`ENV-SHUTDOWN`'s window belong to the Environment steps and were not attacked.

**Result: no defects, no open attacks.** Nothing landed; no file under `src/` or `tests/`
changed. Two observations, no action, are recorded below.

### Latch

| # | Attack | Resolution |
|---|---|---|
| L1 | `new()`: not pending, `take` None, `close` None | pinned — `latch_pending_state::pending_is_true_only_while_an_error_waits`, `latch_observation::empty_take_keeps_the_latch_open`, `latch_close::publication_after_close_is_discarded` |
| L2 | first `publish` on Empty makes it pending and returnable | pinned — `empty_take_keeps_the_latch_open`, `pending_is_true_only_while_an_error_waits` |
| L3 | second `publish` while pending: first kept; second dropped at once | pinned for retention — `latch_first_wins::first_publication_is_kept_and_later_discarded`; drop timing is on `review-adversarial.md`'s Considered list, no new argument |
| L4 | `publish` on Reported: dropped before return; later `take` and `close` None | pinned — `latch_first_wins::take_marks_reported_forever`, `latch_precedence::a_pending_error_wins_and_discards_the_local_error` (drop-tracked) |
| L5 | `publish` on Closed, after a close on Empty and on Pending: dropped at once, never pending, `take` None | pinned — `publication_after_close_is_discarded`, `latch_precedence::a_local_error_leaves_the_latch_open_for_a_later_publication` (drop-tracked) |
| L6 | `take` on Empty leaves the latch open | pinned — `empty_take_keeps_the_latch_open` |
| L7 | `take` on Pending: Some once; second `take` None; `close` None | pinned — `take_marks_reported_forever` |
| L8 | `take` on Closed returns None and the state stays Closed rather than Reported | pinned for the return — `close_returns_the_pending_error_exactly_once`; the state difference is unobservable, see below |
| L9 | `close` on Empty, Pending, Reported; a second `close` after each | pinned — `publication_after_close_is_discarded`, `close_returns_the_pending_error_exactly_once`, `take_marks_reported_forever` |
| L10 | order: publish→take→close and publish→close→take each yield the Error exactly once | pinned — `take_marks_reported_forever`, `close_returns_the_pending_error_exactly_once` |
| L11 | two publications then `close` returns the first | derived — L3's retained Pending(first) plus L9's close-on-Pending |
| L12 | `resolve_local_error` on Pending: pending wins, local dropped before return, latch reported, a later local returned unchanged | pinned — `a_pending_error_wins_and_discards_the_local_error` |
| L13 | `resolve_local_error` on Empty: local returned, owned by the caller, latch still open, the next publication leaves through the report | pinned — `a_local_error_wins_when_the_latch_is_empty`, `a_local_error_leaves_the_latch_open_for_a_later_publication` |
| L14 | `resolve_local_error` on Closed | unreachable under `ENV-SERIAL`: the close runs inside the consuming `shutdown`; by construction otherwise, L8's None plus `unwrap_or` |
| L15 | `close_into_report` across {Quiesced, Incomplete} × {Empty, Pending}; a second report | pinned — `latch_close::close_into_report_preserves_quiescence_and_emits_once` ({Incomplete, Pending}, then a second {Quiesced, None}), `a_local_error_leaves_the_latch_open_for_a_later_publication` ({Quiesced, Pending}); the two Empty cells derive from L9's close-on-Empty and the pinned quiescence pass-through |
| L16 | `is_pending` in all four states | pinned — `pending_is_true_only_while_an_error_waits` |
| L17 | an untaken pending Error is dropped with the latch; `E` zero-sized, `!Send`, `!Debug`; nothing runs between `take`'s replace and its restore | by construction — Rust drop order, no bounds on `E`, straight-line code |

### Environment contract

| # | Attack | Resolution |
|---|---|---|
| T1 | `Quiescence` derives match the design API block; `ShutdownReport` derives nothing, as the design shows | pinned for `PartialEq`/`Eq` — `quiescence_variants::both_states_are_distinct_and_comparable`; the rest by construction |
| T2 | Stop path, all four report shapes: {Quiesced, None} Stopped; {Quiesced, Some} and {Incomplete, Some} `Environment(Shutdown)` with the quiescence retained; {Incomplete, None} `Core(ShutdownIncomplete)` | pinned — `engine::tests::stopped_carries_the_final_state`; `faults::environment_fault_matrix::each_operation_error_maps_to_its_cause_and_quiescence`; `record::tests::a_report_error_outranks_incomplete`, `::incomplete_without_error_is_shutdown_incomplete`; conformance `ShutdownFailure`, `IncompleteShutdown` |
| T3 | Fatal path: every operation `Err` outranks an {Incomplete, Some} report; the report's Error is dropped | pinned — `the_operation_error_outranks_the_report_error` (four points), `engine::tests::the_shutdown_error_never_replaces_the_fixed_cause` (drop-tracked) |
| T4 | `ENV-TIME`: a stamp below the last accepted one, against the start stamp and against a later Event's; equal stamps accepted; `last_time` follows each accepted Event | pinned — `faults::a_decreasing_stamp_is_time_regression` (start 100, offered 99), `record::tests::a_decreasing_stamp_is_time_regression_with_the_candidate_consumed` (index 4, last 10), `::an_equal_stamp_is_accepted`, `golden_journal::repeated_events_advance_indices_and_preserve_time_boundaries`; the check is one site, `record.rs:327` |
| T5 | `ENV-SERIAL` as the Engine drives it: `start` first and once; `take_error` once per turn after every `dispatch`, on empty turns too; no `dispatch` on an empty batch; after `Err` or `take_error` Some only `shutdown`, once | pinned — the fake panics on any out-of-graph call and every run in `faults.rs`, `conformance.rs`, `golden_journal.rs` completes; call lists in `a_start_error_performs_no_shutdown`, `a_failed_dispatch_retains_only_the_successful_handoff_prefix`, `repeated_events_advance_indices_and_preserve_time_boundaries`; `shutdown_count == 1` in every post-start matrix test; the fake's graph in `scripted_environment_graph::rejects_operations_outside_the_environment_graph` |
| T6 | `take_error` Some on a later turn, not the start turn | derived — one production call site, `record.rs:280`, reached by the same `effects` for every turn; the start-turn pin is conformance `CheckpointFailure` and `environment_fault_matrix` |
| T7 | `next_event` `Err` at the first position and after an accepted Event | pinned first — `environment_fault_matrix` `NextEvent` point; the later position derived through T6's single-site argument at `record.rs:319` |
| T8 | ownership of a Command whose `dispatch` fails; the remaining batch never offered | pinned — `a_failed_dispatch_retains_only_the_successful_handoff_prefix`; the Command moves by value, by construction |
| T9 | a bespoke implementor: `start` `Err` after committing activation, `dispatch` `Err` after a handoff, `take_error` Some twice, a `Drop` that works after a failed start | trusted — `TRUST-ENV`; the Core has no guard and no row claims one; a second Some is unreachable after the first under T5 |
| T10 | trait rustdoc versus the `ENV-*` rows and the design API block | by inspection — `take_error`, `shutdown`, and `ShutdownReport::error` docs are verbatim |

### Observations, no action

- **Reported and Closed are indistinguishable through the API.** Only `is_pending`,
  `take`, `close`, and `publish` observe the state, and all four answer identically on
  both. The restore in `take`'s Closed arm is therefore untestable from outside; kept
  per `review-simplification.md`.
- **On a `start` failure the Environment is dropped when `run` returns**, after
  `finalize` builds the exit at `engine.rs:183`. `ENV-START` says only that the drop is
  safe, so no row fixes the instant.

## record

Reviewed 2026-09-08: `src/engine/record.rs`, `tests/compile_fail.rs`, the fourteen
cases and one legal case under `tests/grammar_fixture/cases/`, and
`tests/grammar_fixture/src/lib.rs`. Design rows `RUN-GRAMMAR`, `RUN-ENFORCEMENT`,
`RUN-RECORDS`, `RUN-INDEX`, `RUN-CHECKPOINT`, `ASSERT-INVARIANTS`, `VERIFY-GRAMMAR`,
A1, A3, A5, and the `JRN-*` rows as consumed. Green before the round opened. Suites
read: the file's eleven test modules, `golden_journal.rs`, `faults.rs`, the harness
tests for the scripted Environment and sink, and every `.stderr` in the fixture.

**Result: no defects, two open attacks, both landed.** Both are compile-fail cases;
`record.rs` and the fixture's hand copies are unchanged.

### Record grammar

| # | Attack | Resolution |
|---|---|---|
| T1 | six kind tags; field order per record; `schema_version` 1; `outcome` a bare tag | pinned — `record_kind_wire::kind_tags_match_their_variant_names`, `record_payload_wire::every_payload_leads_with_its_kind_in_table_order`, `::turn_completed_outcome_is_a_bare_tag_for_both_answers` |
| T2 | serialized tag and `JournalFatal.record_kind` disagree | by construction — both read `P::KIND` (S3); `record_payload_wire::payload_tag_and_kind_share_one_source` |
| T3 | `JournalFatal.outcome` `Some` exactly for `TurnCompleted`, with the attempted outcome, at all seven commit sites | pinned — `faults::journal_fault_matrix::only_turn_completed_carries_an_outcome` |
| T4 | index and time at 0, 1, `u64::MAX` in every record | pinned — `certificate_minting::run_started_preserves_frozen_time_boundaries`, `record_payload_wire::remaining_payloads_preserve_maximum_index`, `::event_accepted_serializes_maximum_index_and_time_without_loss`, `stop_closing::closing_preserves_index_and_time_boundaries` |
| T5 | `Send`/`Sync` depend on the phase or payload marker | pinned — `certificate_auto_traits::phase_marker_does_not_control_send_or_sync`, `record_payload_wire::kind_marker_auto_traits_do_not_depend_on_payload_type` |
| T6 | certificate, phases, or transitions nameable outside `engine` | by construction — `engine/mod.rs` and `lib.rs` re-export only `JournalFatal`, `RecordKind`, `TurnOutcome` |
| M1 | minted index is 0 for any start time | asserted — `mint`; pinned `certificate_minting::minting_asserts_the_prospective_index_base` |
| M2 | a sink call before the `RunStarted` write | pinned — `faults::journal_fault_matrix::each_record_kind_maps_to_its_journal_fatal`, `RunStarted` row: the sink script holds one step, so an earlier call panics the sink |
| M3 | `RunStarted` at exact capacity; one past with nothing written | pinned — both tests in `certificate_bounds` |
| M4 | `RunStarted` commit failure: kind, no outcome, Journal destroyed, no handler, one shutdown | pinned — `certificate_fatal_path::commit_failure_names_run_started_and_destroys_the_journal`; matrix `RunStarted` row |
| M5 | after `run_started`, `index()` is 0 and `logical_time()` the start time | pinned — `certificate_minting::run_started_commits_the_versioned_first_record` |
| C1 | either answer: matching variant, nothing committed, no Environment call, index and time kept | pinned — `turn_classification::classify_fixes_the_answer_in_the_phase_type`, `::classify_commits_nothing_for_either_answer`, `::classify_preserves_certificate_state_for_both_answers` |
| C2 | a classified certificate classified again with the other answer | **open — E2, landed** |
| C3 | `no_commands` on a nonempty buffer: panic, buffer and Journal untouched | asserted — `no_commands`; pinned `turn_classification::no_commands_panics_on_a_nonempty_buffer`, `::full_batch_rejection_preserves_buffer_and_journal` |
| C4 | empty buffer at capacity 0 and 2: no record, state kept | pinned — `turn_classification::the_empty_batch_edge_commits_nothing`, `::zero_capacity_empty_batch_advances_without_a_record`, `::no_commands_preserves_certificate_state` |
| D1 | empty buffer: panic, no record, no Environment call, buffer reusable | asserted — `dispatch_batch`; pinned `batch_dispatch::an_empty_buffer_is_an_invariant_panic`, `::empty_batch_panic_has_no_record_or_environment_side_effect` |
| D2 | one and capacity commands: ordered Prepared bytes, ordered handoffs, Dispatched after the last, buffer drained, slot reusable | pinned — `batch_dispatch::prepared_then_each_handoff_in_order_then_dispatched`, `::one_command_batch_preserves_phase_state_and_reusable_capacity`; `golden_journal::golden_sequences::a_command_run_writes_exactly_its_records` |
| D3 | Prepared commit fails by flush, write, partial write, `Encode`, `NotAnObject`, `BoundExceeded`: no handoff, buffer intact | pinned — `batch_dispatch::prepared_commit_failure_precedes_any_handoff`, `::command_serialization_failure_leaves_the_batch_undrained`, `::prepared_record_one_byte_past_capacity_hands_off_nothing`; `faults::journal_fault_matrix::a_partial_write_failure_preserves_the_prior_commit_boundary`; `golden_journal::encoding_rejection::an_interior_newline_payload_is_rejected_with_nothing_written` |
| D4 | dispatch `Err` at 0, at a middle k, at N−1: prefix handed off, `position == k`, buffer empty, no Dispatched, no `take_error` | pinned — `batch_dispatch::first_position_failure_hands_off_nothing_and_discards_all_commands`, `::error_at_position_k_keeps_the_prefix_and_discards_the_suffix`, `::last_position_failure_keeps_the_full_prefix_and_discards_the_failed_command`; `faults::environment_fault_matrix::a_failed_dispatch_retains_only_the_successful_handoff_prefix` |
| D5 | Dispatched commit fails: every handoff done, buffer empty, kind certifies the batch | pinned — `batch_dispatch::dispatched_commit_failure_follows_every_handoff`; matrix `CommandsDispatched` row |
| D6 | Prepared at exact capacity; one past | pinned — `batch_dispatch::prepared_record_succeeds_at_exact_record_capacity`, `::prepared_record_one_byte_past_capacity_hands_off_nothing` (S1) |
| D7 | Dispatched at the bound | derived — S1's argument: always shorter than the Prepared record the bound just admitted |
| D8 | a Command failing to serialize at k > 0 through the transition | derived — the `?` precedes the drain for every k (D3); `record_payload_wire::commands_prepared_serialization_failure_leaves_batch_and_record_reusable` pins k = 1 at the payload; region clearing is the journal group's C15 |
| D9 | `position` overflow | by construction — `enumerate` over at most `capacity` items; a `Vec` holds fewer than `isize::MAX` |
| D10 | Stop answer through `dispatch_batch`; dispatch `Err` under Stop | by construction — one generic body over `A`; the Stop success path is `one_command_batch_preserves_phase_state_and_reusable_capacity` |
| K1 | checkpoint `Some` under Continue and under Stop: `Environment(Checkpoint)`, no record; `None`: no record, state kept | pinned — `turn_checkpoint::a_pending_error_is_checkpoint_fatal_and_consumes_the_certificate`, `::a_stop_path_pending_error_commits_nothing`, `::a_clean_snapshot_preserves_state_for_both_answers` |
| K2 | `take_error` exactly once, after the last handoff and its record, before the completion record | pinned — `turn_checkpoint::the_snapshot_is_taken_exactly_once`, `::a_dispatched_batch_checkpoints_after_the_last_handoff` |
| K3 | no snapshot on an earlier Fatal: dispatch `Err`, overflow, handler `Fatal`, Prepared failure | pinned — D4's matrix test; `faults::application_fault_matrix::an_over_emitting_application_is_command_bound_exceeded`; `engine::run_turn_loop::a_start_handler_fatal_performs_no_effect_phase_or_event_request`; `prepared_commit_failure_precedes_any_handoff` |
| K4 | `CommandsDispatched` as the final record, both answers | pinned — `golden_journal::fatal_tails::commands_dispatched_can_be_the_final_record` (Stop); matrix `Checkpoint` row commits three records (Continue) |
| P1 | `TurnCompleted(Continue)` and `StopRequested` bytes; outcome from the marker, not a caller | pinned — `turn_completion::continue_commits_turn_completed_continue`, `::stop_commits_stop_requested`, `::the_committed_outcome_is_the_phase_marker_not_a_caller_value` |
| P2 | completion records at exact capacity; one past with metadata kept | pinned — `turn_completion::completion_records_succeed_at_exact_record_capacity`, `::completion_records_one_byte_past_capacity_fail_without_output` |
| P3 | `StopRequested` commits before `shutdown`; its failure still shuts down once | pinned — `stop_commits_stop_requested`; matrix `StopRequested` row |
| S1 | the four reports; `shutdown` before the commit; exactly once | pinned — `stop_closing::a_clean_report_commits_turn_completed_stop`, `::a_report_error_outranks_incomplete`, `::incomplete_without_error_is_shutdown_incomplete`, `::a_quiesced_report_error_is_shutdown_fatal`; `faults::environment_fault_matrix::each_operation_error_maps_to_its_cause_and_quiescence`, `::environment_failures_commit_no_records_after_their_boundary` |
| S2 | commit failure after a clean report by write, flush, bound: `Some(Stop)`, `Quiesced` retained, no second shutdown, `StopRequested` last committed | pinned — `stop_closing::commit_failure_after_a_clean_report_retains_quiesced`, `::completion_record_one_byte_past_capacity_retains_quiesced`; `faults::journal_fault_matrix::a_stop_commit_failure_retains_quiesced` |
| S3 | `TurnCompleted(Stop)` at exact capacity | pinned — `stop_closing::completion_record_succeeds_at_exact_capacity` |
| E1 | index `MAX`: `IndexExhausted`, `next_event` uncalled, no sink call; `MAX − 1` accepts to `MAX` | pinned — `event_acceptance::the_domain_check_precedes_next_event` |
| E2 | overflow past the domain check | asserted — the `expect` in `accept_event`; unreachable after E1 |
| E3 | `next_event` `Err`: `Environment(NextEvent)`, nothing committed, no sink call | pinned — `NextEvent` row of `environment_failures_commit_no_records_after_their_boundary` |
| E4 | regression against the start time and against a later accepted time; equal accepted; candidate consumed; nothing committed; no handler | pinned — `event_acceptance::a_decreasing_stamp_is_time_regression_with_the_candidate_consumed`, `::an_equal_stamp_is_accepted`; `faults::environment_fault_matrix::a_decreasing_stamp_is_time_regression`; `golden_journal::golden_sequences::repeated_events_advance_indices_and_preserve_time_boundaries` |
| E5 | commit failure by flush, write, bound, `Encode`, `NotAnObject`: candidate consumed once, nothing committed, kind `EventAccepted`, no `on_event` | pinned — `event_acceptance::acceptance_advances_index_and_time_only_on_commit`, `::event_record_one_byte_past_capacity_fails_after_consuming_candidate`, `::event_serialization_failure_is_journal_fatal_after_consumption`; matrix `EventAccepted` row; `golden_journal::encoding_rejection::an_interior_newline_event_preserves_the_committed_prefix` |
| E6 | success: record carries n+1 and the offered time; certificate updated; the returned Event is the recorded one, without `Clone` | pinned — `event_acceptance::event_accepted_bytes_carry_the_new_index_and_time`, `::a_non_clone_event_is_returned_after_commit` |
| E7 | `EventAccepted` at exact capacity | pinned — `event_acceptance::event_record_succeeds_at_exact_record_capacity` |
| H1 | `Ok(0)`, over-report, `Interrupted`, short write, error after progress at a record commit | derived — the journal group's W1–W6 fix the `JournalError`; `Certificate::commit` maps every `JournalError` one way, pinned per kind by the fault matrix; `a_partial_write_failure_preserves_the_prior_commit_boundary` walks one through `CommandsPrepared` |
| H2 | a `{Quiesced, Some}` report; `take_error` `Some` at the start turn | pinned — S1, K1 |
| H3 | a fabricated certificate from the fixture's position, by `advance` or by a struct literal | **open — E1, landed** |
| H4 | wrong-phase methods on `Initial`, unclassified `TurnOpen`, `EffectsComplete`, `Checkpointed<A>`, `BetweenTurns`, `Closed` | pinned — the fourteen cases in `compile_fail::grammar_compile_fail`; every `.stderr` read: E0599 naming the phase, E0382, E0277, E0624 on `commit`; no E0603 |
| H5 | fixture drift from `record.rs`'s use of `BoundedBuffer` and `EventIndex` | by construction — `legal.rs` compiles the real file against the hand copies, so any new call fails `the_fixture_reconstruction_itself_compiles` |
| H6 | a dropped certificate performs a sink operation | derived — no `Drop` impl (journal L3); every fault test drops one on a sink whose next unscripted call panics |
| G1 | "no handler runs before its acceptance record" | pinned — matrix `RunStarted` row (state `[0]`), `EventAccepted` row (state `[0, 1]`), `a_decreasing_stamp_is_time_regression` app calls |
| G2 | "`RunStarted` is the only possible first record" | derived — `mint` yields `Initial`, whose only transition is `run_started` (`illegal_transition_order`, `initial_context_access`), plus M2 |
| G3 | "`Stopped` implies a clean report" | by construction — `Closed` is produced only by `close`, after both report checks (`premature_stop_completion`) |
| G4 | "after `classify` no transition accepts an answer" | **open — E2, landed**; the same gap as C2 |

### Items decided

**E1 — landed.** `advance` is private and the certificate's four fields are private,
so a forgery from the Engine's position fails either way, but no case attempted one.
Two cases, not one: rustc reports E0451 field privacy in a later pass than E0624, so a
file carrying both attacks records only the `advance` error and leaves the literal
unpinned. `certificate_forgery_by_advance.rs` expects E0624 on `advance`;
`certificate_forgery_by_literal.rs` expects E0451 on all four fields. Driven by
`grammar_compile_fail::certificate_forgery_does_not_compile`. Mutations: `advance` made
`pub(super)` failed only the advance case; the four fields made `pub(super)` failed only
the literal case. Both reverted.

**E2 — landed.** `classify` lives on the unclassified `TurnOpen` alone, so an
answer-typed phase cannot be classified again, but no case attempted it; moving
`classify` into the `TurnOpen<A>` block beside the batch edges would compile a Stop-typed
successor from a Continue turn. `reclassified_turn.rs` expects E0599 on both
`TurnOpen<answer::Continue>` and `TurnOpen<answer::Stop>`. Driven by
`grammar_compile_fail::a_classified_turn_cannot_be_reclassified`. Mutation: the
`TurnOpen` impl block made generic over `A`; the new case compiled and failed the test,
and three sibling cases failed on the changed `found for` note. Reverted.

After the batch: `cargo test` green at 211 in-file tests plus every cross-file suite,
`cargo clippy --all-targets -- -D warnings` clean, `cargo fmt --check` clean.

## engine

Reviewed 2026-09-08: `src/engine/engine.rs`, `src/engine/mod.rs`, `src/lib.rs`, with
`record.rs`, `application.rs`, `bounded_buffer.rs`, `environment.rs`, `journal.rs`, and
`time.rs` read as callees. Design rows `RUN-SERIAL`, `RUN-FINALIZE`, `RUN-CHECKPOINT`,
`DET-RUN`, `BOUND-NONZERO`, `VERIFY-FAULTS`, `VERIFY-JOURNAL`, `VERIFY-CONFORMANCE`,
`ENV-START`, `ENV-SHUTDOWN`, `APP-OVERFLOW`, A2, A4, A9, and the Run section's startup,
phase, edge, and Notes tables. Green before the round opened. Suites read: the twelve
modules of `engine::tests`, `faults.rs`, `golden_journal.rs`, `conformance.rs`,
`harness_contract.rs`, `tests/support/*`, and `record::batch_dispatch`. Greps:
`Dispatch { position`, `position: [1-9]`, `EventIndex::new(` across `src/` and `tests/`.
Prior: the Engine table, Considered list, and Observations in `review-adversarial.md`;
S1, S2, and the kept items in `review-simplification.md`; ledger G1–G14 not re-raised.
`CRATE-EXPORTS` and the missing derives on `BuildError` and `EngineExit` wait on Wiring
and were not attacked.

**Result: no defects, one open attack, landed.** One test in `tests/faults.rs`; no file
under `src/` changed.

### Engine

| # | Attack | Resolution |
|---|---|---|
| N1 | `new` at capacity 1; `usize::MAX` commands → `CommandBuffer` | pinned — `engine_construction::one_slot_bounds_construct_an_empty_unpoisoned_engine`, `::batch_reservation_failure_is_command_buffer` |
| N2 | `usize::MAX` record bytes → `Journal(MaxBytesTooLarge)`; record bytes 1 builds, then `BoundExceeded` at `RunStarted` | pinned — `::journal_build_failure_is_journal`, `::one_slot_bounds_construct_an_empty_unpoisoned_engine`; the run derived from `record::certificate_bounds` plus the matrix `RunStarted` row (one `?` in `run_started`) |
| N3 | both unreservable → step 1 wins | pinned — `::command_buffer_failure_precedes_journal_failure` |
| N4 | `new` and a failed `new` call no Application, Environment, or sink method | pinned — `::construction_invokes_no_application_or_environment_method`, `::failures_invoke_no_application_or_environment_method`; sink by `startup_faults::a_start_error_invokes_no_handler_or_sink` (zero-step sink) |
| R1 | `initial_state` once, before `start`, on every path | pinned — `run_startup::state_is_created_before_any_fallible_step`; every full call list holds one `InitialState` first |
| R2 | `start` `Err`: `Environment(Start)`, `Quiesced`, no shutdown, no handler, no sink call, State carried | pinned — `run_startup::a_start_error_exits_fatal_quiesced_without_shutdown`, `::a_start_error_invokes_no_handler_and_writes_no_record`, `startup_faults::*` |
| R3 | start time 0 and `u64::MAX` through handler and `RunStarted` | pinned — `run_startup::boundary_start_times_reach_the_handler_and_journal_unchanged` |
| R4 | `RunStarted` commit fails by write, flush: no handler, shutdown once | pinned — matrix `RunStarted` row in `each_record_kind_maps_to_its_journal_fatal`, `flush_failures_leave_the_failed_record_uncommitted` |
| T1 | index 0 with an Event; index ≥ 1 without: panic before any handler | asserted — `engine.rs:75`; pinned `turn_event_invariant::*` |
| T2 | handler sees index and time equal to its acceptance record | pinned — `run_turn_loop::continue_turns_accept_events_in_sequence`; `golden::graph_sequences::an_event_turn_with_commands_uses_the_accepted_index` (trace and bytes) |
| T3 | overflow × {Continue, Stop, Fatal}, start and Event turn: `CommandBoundExceeded`, batch cleared, payload dropped, no dispatch or checkpoint | pinned — `turn_overflow_precedence::*`, `run_turn_loop::overflow_beats_the_returned_outcome_and_discards_the_batch`, `application_fault_matrix::event_turn_overflow_preserves_prior_effects_and_dispatches_nothing_new` |
| T4 | `Fatal` with an exactly full batch, start and Event turn: `Application`, batch cleared, State stands, payload owned | pinned — `turn_application_fatal::*`, `run_turn_loop::a_start_handler_fatal_performs_no_effect_phase_or_event_request`, `::a_handler_fatal_discards_the_batch_and_carries_the_error` (capacity 1, one Command) |
| T5 | {empty, one, capacity} × {Continue, Stop} record sequences, start and Event turn | pinned — `golden::graph_sequences::every_empty_and_command_turn_shape_has_its_required_sequence`, `::an_event_turn_with_commands_uses_the_accepted_index`, `golden_sequences::*`; Event-turn nonempty Continue derived from the index-1 `CommandsPrepared`/`CommandsDispatched` bytes and the index-1 `TurnCompleted(Continue)` bytes in `repeated_events_advance_indices_and_preserve_time_boundaries` |
| T6 | Stop at index 0 with and without Commands | pinned — `run_stop_path::stop_at_start_produces_the_three_record_journal`, `golden_sequences::a_command_run_writes_exactly_its_records` |
| T7 | the Event handed to `on_event` is the one returned and recorded | pinned — the three-way match of `AppCall::OnEvent`, `EnvCall::NextEvent`, and `"event":7` in every golden test |
| T8 | `on_start` once per run, `on_event` once per accepted Event, none after Fatal | pinned — the exact call lists in `run_turn_loop` and `golden_sequences` |
| F1 | empty → recordless edge; nonempty → dispatch; checkpoint once after, both answers | pinned — every call list shows `TakeError` after the last `Dispatch`, on empty turns too |
| F2 | dispatch `Err` on an accepted Event turn: `position` is batch-relative, the prior turn's handoffs stand | **open — E1, landed** |
| F3 | checkpoint `Some` after Commands and after none, under Continue and Stop | pinned — matrix `Checkpoint` point; `golden::fatal_tails::commands_dispatched_can_be_the_final_record`; `record::turn_checkpoint::a_stop_path_pending_error_commits_nothing` |
| D1 | `TurnCompleted(Continue)` commit fails: `Some(Continue)`, no `next_event`, shutdown once | pinned — matrix `TurnCompleted` row; no `next_event` derived — a commit on the poisoned Journal panics |
| D2 | `StopRequested` commit fails: `Unconsumed` arm, shutdown once | pinned — matrix `StopRequested` row |
| D3 | `TimeRegression` through `run`: `previous` the last accepted time, candidate consumed, no handler, nothing committed | pinned — `environment_fault_matrix::a_decreasing_stamp_is_time_regression`; a later `previous` by `record::event_acceptance::a_decreasing_stamp_is_time_regression_with_the_candidate_consumed` |
| D4 | `IndexExhausted` reaches the `Unconsumed` arm | derived — `record::event_acceptance::the_domain_check_precedes_next_event` plus `fatal_finalization::a_started_environment_is_shutdown_exactly_once`, which finalizes that cause |
| D5 | A2: the completion record commits before `next_event` | by construction — `accept_event` exists only on `BetweenTurns`, produced only by `complete_continue`; a stray call would fail the scripted Environment's phase assertion |
| Z1 | `StartFailed` → `Quiesced`, no shutdown | pinned — `fatal_finalization::a_start_error_skips_shutdown_and_is_quiesced` |
| Z2 | `Unconsumed` × all four reports: quiescence taken, Error dropped, cause kept | pinned — `::a_started_environment_is_shutdown_exactly_once` ({Incomplete, None}), `::the_shutdown_error_never_replaces_the_fixed_cause` ({Quiesced, Some}, drop-tracked), `the_operation_error_outranks_the_report_error` ({Incomplete, Some}, four points) |
| Z3 | `Retained` with either quiescence; through `run` after a `TurnCompleted(Stop)` write or flush failure | pinned — `::a_consumed_environment_uses_the_retained_quiescence`, `::a_consumed_environment_preserves_retained_quiesced`, `journal_fault_matrix::a_stop_commit_failure_retains_quiesced` |
| Z4 | one finalize, one shutdown, per run | by construction — three return sites; `shutdown_count == 1` in every post-start test |
| Z5 | every exit carries the post-handler State | pinned — `run_stop_path::stopped_carries_the_final_state`, `application_fault_matrix::state_mutations_survive_each_post_handler_fatal_exit` |
| C1 | `Stopped` only after a clean report and a committed `TurnCompleted(Stop)` | by construction — `Closed` comes only from `close`; pinned `environment_fault_matrix::each_operation_error_maps_to_its_cause_and_quiescence` |
| B1 | `Encode`, `NotAnObject`, `BoundExceeded` at `CommandsPrepared` and `EventAccepted` through `run` | pinned for `NotAnObject` — `golden::encoding_rejection::*`; the rest derived, one `?` per transition and the kind pinned in `record::batch_dispatch`, `::event_acceptance` |
| B2 | `Ok(0)`, over-count, `Interrupted`, short write, error after progress through `run` | derived — the journal group's sink rows fix the `JournalError`; the matrix pins the mapping per record; `a_partial_write_failure_preserves_the_prior_commit_boundary` walks one through |
| B3 | a bespoke `dispatch` that takes the Command and returns `Err`; `Drop` with effects | trusted — `TRUST-ENV`, `TRUST-PURE`; the environment group's T9 |
| G1 | `DET-RUN` via two runs: handler calls, State, Command intent, Journal bytes, exit discriminants and payloads (`position`, `previous`/`offered`, kind, outcome, sink operation, quiescence) | pinned — `conformance_within_type::*`; `ExitShape` carries every Core-owned payload |
| G2 | every scripted call checked against the graph, handoffs included | pinned — `::every_environment_call_is_graph_conformant` plus the fake's phase machine in `scripted_environment_graph` |
| G3 | `VERIFY-FAULTS`: each post-`start` `Err` × `{Incomplete, Some}`; `start Err` performs no shutdown; over-emitter; both bad reports | pinned — `the_operation_error_outranks_the_report_error`, `a_start_error_performs_no_shutdown`, `an_over_emitting_application_is_command_bound_exceeded`, `each_operation_error_maps_to_its_cause_and_quiescence` |
| G4 | `VERIFY-JOURNAL`: each answer at the `classify` call site | pinned — `golden::classify_call_site::each_non_fatal_answer_yields_its_required_outcome_records` |
| G5 | `RUN-SERIAL`, `BOUND-NONZERO` | by construction — owned by value, consuming `shutdown`, `NonZeroUsize` fields |

### Items decided

**E1 — landed.** `record.rs:285` restarts `enumerate()` for each turn's batch and the
Engine keeps no handoff count, so `position` names the failing Command within that
turn's `CommandsPrepared` record. Every pinned dispatch failure — the five
`record::batch_dispatch` tests, both `environment_fault_matrix` points, and the two
conformance traces — runs on the start turn, where "position in this batch" and
"handoffs so far in the run" are the same number.
`environment_fault_matrix::a_later_turn_dispatch_failure_reports_its_batch_relative_position`
runs a full Continue turn with two Commands, then an Event turn whose second handoff
fails: `Dispatch { position: 1 }`, handoffs `[10, 11, 20]`, the exact Environment call
list, and the Journal ending at the index-1 `CommandsPrepared` record. Mutation:
`position` offset by the certificate index in `dispatch_batch`; the new test was the only
failure in the workspace. Reverted.

### Observations, no action

- **The adversarial Considered entry "Discarded Commands are dropped before shutdown"
  has one exception.** When `CommandsPrepared` fails to commit, `dispatch_batch` returns
  before the drain, so the batch stays populated
  (`record::batch_dispatch::prepared_commit_failure_precedes_any_handoff`) and is dropped
  when `run` returns, after the finalizing `shutdown`. No row fixes the instant; under
  `TRUST-PURE` a Command's `Drop` observes nothing.
- **The adversarial observation on the accepted Event's lifetime is stale since S1.**
  `accepted_event` is a local of `drive`, so on a Fatal exit it drops when `drive`
  returns, before `finalize` and `shutdown`, not after.
- **The sink drops on different sides of `shutdown`.** A failure inside `drive` drops
  the certificate, Journal, and writer before `finalize` calls `shutdown`; on the Stop
  path `close` calls `shutdown` first and the writer drops after. No row names the
  instant.
- **`BuildError` and `EngineExit` derive nothing**, so a caller cannot `unwrap` or
  `assert_eq!` them. Export policy waits on Wiring.

After the batch: `cargo test` green at 209 in-file tests plus every cross-file suite
(`faults` now 17), `cargo clippy --all-targets -- -D warnings` clean, `cargo fmt --check`
clean.
