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
