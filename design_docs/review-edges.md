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
