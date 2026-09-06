# Round 2 — Adversarial

The attack list was written from the design and the source before any test was read,
then every attack was checked against the suite by reading the test bodies, not their
names. Where a candidate test existed, the question was whether it actually asserts the
attacked property; where none existed, the source was reread for the attacked path.
Green before the round opened, on the Round 1 batch.

**Result: three attacks survive, all gaps, no defects.** On every attack the Core does
what the design says; the three survivors are properties the suite does not pin. Two are
boundary cells of a pattern the suite already applies elsewhere and skipped. One is a
latch clause the two shipped Environments will build on.

## Per subsystem

Each row is one attack. *Pinned* names the test that asserts the property. *By
construction* means a type or a standard-library guarantee carries it and a test would
restate the compiler. *Derived* means two pinned facts compose to it with no code path
between them.

### Time types

| Attack | Resolution |
|---|---|
| a Duration beyond u64 nanoseconds, and a sum one past the domain | pinned — `oversized_duration_returns_none`, `overflowing_sum_returns_none`; the exact maximum by `exact_domain_maximum_sum_succeeds` |
| both types serialize as bare integers, and `u64::MAX` survives without float loss | pinned — `both_serialize_as_transparent_u64`, `maximum_values_serialize_without_loss` |
| `EventIndex` round trip at both ends of the domain | pinned — `event_index_round_trips_domain_boundaries` |
| derived ordering compares the raw count | by construction — a one-field newtype's derive; exercised by every regression and equal-stamp test |

### Bounded buffer

| Attack | Resolution |
|---|---|
| capacity 0: the first push is refused, the first write is `WriteZero` | pinned — `zero_capacity_refuses_first_push_and_returns_value`, `zero_capacity_rejects_all_writes` |
| exact capacity: N pushes hold, N+1 returns its value, contents and allocation unchanged | pinned — `pushes_up_to_capacity_preserve_order`, `push_beyond_capacity_is_refused_without_growth`, `refused_push_preserves_full_buffer_state` |
| a write longer than the remaining room takes exactly the room; the next write is `WriteZero` | pinned — `partial_writes_accumulate_without_loss`, `write_all_one_past_capacity_retains_accepted_prefix`, `full_buffer_returns_write_zero` |
| an empty write on a full buffer versus a nonfull one | pinned — `empty_write_returns_zero_only_while_capacity_remains` |
| clear and drain keep the allocation | pinned — `clear_and_drain_retain_capacity`, `clear_after_write_zero_restores_writes` |
| a drain dropped part way, the shape the dispatch loop's early return takes | pinned — `dropped_partial_drain_removes_remaining_values` |
| an unreservable capacity | pinned — `construction_failure_reports_the_reservation_error` |

### Journal

| Attack | Resolution |
|---|---|
| an object of `max`, `max + 1`, and more bytes | pinned — `encode_at_exactly_max_bytes_completes`, `object_of_region_size_has_no_newline_room`, `oversized_record_is_bound_exceeded_without_sink_calls` |
| a non-object that exactly fills the region | pinned — `non_object_top_level_is_rejected` |
| a non-object that overruns the region: the bound check precedes classification, so it is `BoundExceeded`, not `NotAnObject` | **survives — S2** |
| every non-object JSON kind, and `{}` | pinned — `every_non_object_json_kind_is_rejected`, `object_plus_newline_is_the_encoded_line` |
| a raw interior newline | pinned — `interior_newline_is_not_an_object`; through the Engine by `an_interior_newline_payload_is_rejected_with_nothing_written` |
| after each encode failure the next commit succeeds with exactly its own bytes | pinned — `every_encode_error_skips_sink_and_allows_later_commit`, `successful_encode_after_bound_exceeded_reuses_region`, `serializer_failure_clears_previous_bytes_and_region_remains_reusable`, `valid_object_encodes_after_missing_newline_room` |
| short writes: every byte once, in order, each call given the unwritten suffix | pinned — `short_successful_writes_are_retried_to_completion`, `the_sink_receives_exact_bytes_in_call_order` |
| `Ok(0)`, an over-reported count, `Interrupted`, an Error after progress: each poisons, no flush follows | pinned — `every_sink_failure_poisons_exactly_once` asserts the exact call list for all seven shapes |
| an over-report measured against the suffix, not the line | pinned — `over_reported_count_is_measured_against_remaining_suffix` |
| a flush failure leaves the whole line uncertain | pinned — `flush_failure_is_sink_flush_and_uncommitted`, `flush_failure_after_short_writes_leaves_a_complete_uncertain_line` |
| commit on a poisoned Journal | pinned — `commit_on_poisoned_journal_panics` |
| both constructor failures | pinned — `region_size_overflow_is_max_bytes_too_large`, `failed_reservation_is_allocation_failed` |
| two commits: write, flush, write, flush | pinned — `the_sink_receives_exact_bytes_in_call_order` |
| a map with non-string keys | derived — serde_json's rejection is not an IO error, and every non-IO failure is `Encode` by `serializer_failure_is_encode_without_sink_calls` |

### Context

| Attack | Resolution |
|---|---|
| exact capacity holds N with the marker clear; N+1 sets it and stores nothing; N+2 stores nothing | pinned — `commands_append_in_call_order_through_exact_capacity`, `exact_capacity_keeps_overflow_marker_clear`, `first_over_bound_emit_stores_nothing_and_sets_the_marker`, `every_later_emit_stores_nothing` |
| capacity 1 with zero, one, and two emits | pinned — `one_slot_capacity_accepts_one_command_without_overflow`; two emits at capacity 1 by `overflow_outranks_the_returned_outcome` |
| index and time at the domain ceiling, before and after overflow | pinned — `index_and_logical_time_report_exact_boundary_values`, `index_and_logical_time_remain_stable_after_overflow` |
| a fresh `Context` over a buffer holding leftovers | pinned — `fresh_invocation_starts_empty_with_a_clear_marker`, `fresh_invocation_clears_a_non_overflowed_batch` |
| exactly full and a `Fatal` answer: `Application`, not `CommandBoundExceeded` | pinned — `a_start_handler_fatal_performs_no_effect_phase_or_event_request` and `a_handler_fatal_discards_the_batch_and_carries_the_error` both run at capacity 1 with one Command staged |
| overflow with each of `Continue`, `Stop`, `Fatal`, and the `Fatal` payload's fate | pinned — `overflow_outranks_the_returned_outcome` loops all three and tracks the drop; through the Engine by `overflow_beats_the_returned_outcome_and_discards_the_batch` |

### Port macro

| Attack | Resolution |
|---|---|
| with and without a trailing comma; one Slot | pinned — `single_slot_without_trailing_comma_expands`; the two-Slot fixture carries the comma |
| a restricted visibility propagates | pinned — the `pub(super)` receive-only fixture |
| `Never` in one direction | pinned — `never_command_arm_is_discharged_by_match`, `receive_only_never_arm_is_discharged_downstream` |
| a generic or path-qualified Contract type; two invocations in one module | by construction — the `ty` fragment admits any type, and the invocation name binds no item (`declaration_name_is_available_for_an_independent_item`) |

### Latch

| Attack | Resolution |
|---|---|
| an empty take leaves the latch open | pinned — `empty_take_keeps_the_latch_open` |
| take reports forever; a later publication and the close both find nothing | pinned — `take_marks_reported_forever` |
| close returns a pending Error once; a later publication is discarded | pinned — `close_returns_the_pending_error_exactly_once`, `publication_after_close_is_discarded` |
| a pending Error beats a local one, drops the local, reports; a reported latch returns the local | pinned — `a_pending_error_wins_and_discards_the_local_error`, drop-tracked |
| a local Error returned from an empty latch leaves it open: a later publication lands and leaves through the close | **survives — S3** |
| the report carries either quiescence, and the Error once | pinned — `close_into_report_preserves_quiescence_and_emits_once` |

### Environment contract, as the Engine drives it

| Attack | Resolution |
|---|---|
| after `start` returns `Err`, no further call | pinned — `a_start_error_performs_no_shutdown` asserts the whole call list |
| after `next_event` or `dispatch` returns `Err`, or `take_error` returns `Some`: shutdown only, once | pinned — `a_failed_dispatch_retains_only_the_successful_handoff_prefix` asserts the call list; the scripted Environment panics on any out-of-graph call, so every run in `faults.rs`, `golden_journal.rs`, and `conformance.rs` proves the order it took |
| no checkpoint on a turn that dies at the handler | pinned — the call lists in `a_start_handler_fatal_performs_no_effect_phase_or_event_request`, `overflow_beats_the_returned_outcome_and_discards_the_batch`, `event_turn_overflow_preserves_prior_effects_and_dispatches_nothing_new` |
| shutdown exactly once on every Fatal path after a successful start | pinned — `shutdown_count == 1` in every matrix test, and the consuming receiver makes a second call unrepresentable |

### Record grammar and Engine

| Attack | Resolution |
|---|---|
| index `MAX − 1` accepts one more; at `MAX` the domain check fires before `next_event` | pinned — `the_domain_check_precedes_next_event` does both halves |
| records at index `MAX` and time `MAX`, byte-exact | pinned — `remaining_payloads_preserve_maximum_index`, `event_accepted_serializes_maximum_index_and_time_without_loss`, `completion_transitions_preserve_index_and_time_boundaries` |
| regression against the start time; against a later accepted time, with `previous` naming that time; equal accepted at both | pinned — `a_decreasing_stamp_is_time_regression` (start), `a_decreasing_stamp_is_time_regression_with_the_candidate_consumed` (index 4, last time 10), `an_equal_stamp_is_accepted`, `repeated_events_advance_indices_and_preserve_time_boundaries` |
| the four Stop-path reports | pinned — `a_clean_report_commits_turn_completed_stop`, `a_quiesced_report_error_is_shutdown_fatal`, `incomplete_without_error_is_shutdown_incomplete`, `a_report_error_outranks_incomplete`; the first three also through the Engine in `each_operation_error_maps_to_its_cause_and_quiescence` |
| dispatch `Err` at position 0 and at N−1 | pinned — `first_position_failure_hands_off_nothing_and_discards_all_commands`, `last_position_failure_keeps_the_full_prefix_and_discards_the_failed_command`, `a_failed_dispatch_retains_only_the_successful_handoff_prefix` |
| `CommandsPrepared` failing before any handoff; `CommandsDispatched` after every one | pinned — `prepared_commit_failure_precedes_any_handoff`, `dispatched_commit_failure_follows_every_handoff` |
| `CommandsPrepared` and `CommandsDispatched` at exact record capacity and one byte past | **survives — S1** |
| `RunStarted` commit failure: no handler ran, the sink is empty, shutdown once | pinned — the `RunStarted` row of `each_record_kind_maps_to_its_journal_fatal`, whose State proves the handler never ran |
| every record kind under a write failure and under a flush failure | pinned — `each_record_kind_maps_to_its_journal_fatal`, `flush_failures_leave_the_failed_record_uncommitted`, `a_stop_commit_failure_retains_quiesced` |
| after a Journal Fatal, no sink call and no commit on the poisoned Journal | by construction — the certificate is gone, and a commit on a poisoned Journal panics, so every Journal-fault test in the suite would fail |
| a checkpoint `Some` under a `Stop` answer | pinned — `a_stop_path_pending_error_commits_nothing` |
| the Stop-path commit failure keeps the retained `Quiesced` and performs no second shutdown | pinned — `a_stop_commit_failure_retains_quiesced` |
| a faulting trace run twice | pinned — `every_failure_trace_reproduces_its_complete_observation`, `sink_write_and_flush_failures_reproduce_their_operations` |

## The compositions the plan names

| Case | Resolution |
|---|---|
| the index domain's ceiling | pinned, both sides of the ceiling, at the record level; the Engine cannot reach it in a test and need not |
| the record-size bound | four of the six record kinds have exact-capacity and one-past tests; the two batch records do not — **S1** |
| a batch that is empty, exactly full, and one past | pinned for every answer — `every_empty_and_command_turn_shape_has_its_required_sequence`, `exact_capacity_batches_dispatch_once_in_order_across_reused_turns`, `zero_one_and_capacity_command_traces_are_fully_reproducible`, `an_over_emitting_application_is_command_bound_exceeded`, and the two capacity-1 `Fatal` tests above |
| equal and decreasing timestamps at every entry point | pinned at both entry points; equal at the domain ceiling is the same `<` on a u64 and adds nothing |
| a poisoned Journal meeting every later commit site the graph allows | the graph allows none: the failing transition consumed the certificate. Every Journal-fault test proves it, because a commit on the poisoned Journal would panic the test |
| an overflow marker and a Fatal in the same turn | pinned, with the outranked payload's drop tracked |
| the cross-product of latch-pending, operation-local, and shutdown-report Errors | pending-versus-local is the Environment's to resolve and is pinned for the helper in `latch_precedence`; the shipped Environments' ordering is `VERIFY-LATCH`, deferred to C55. Every operation `Err` crossed with an `{Incomplete, Some}` report is `the_operation_error_outranks_the_report_error`, four points including the checkpoint. Journal, Application, and Core causes reach the same report through the one `finalize` funnel, whose `Unconsumed` arm `a_started_environment_is_shutdown_exactly_once` and `the_shutdown_error_never_replaces_the_fixed_cause` pin under an `Incomplete` report and a drop-tracked Error; `shutdown_count == 1` in each of those causes' matrix tests proves they take that arm |

## Survivors

### S1 — the batch records have no capacity-boundary tests

`certificate_bounds`, `turn_completion`, `stop_closing`, and `event_acceptance` each
prove their record commits at exact `max_record_bytes` and fails one byte past with
nothing written. `batch_dispatch` proves neither for `CommandsPrepared` nor for
`CommandsDispatched`, and no Engine-level test produces `BoundExceeded` at any record but
`RunStarted`. This is the record-size-bound cell of the plan's grid, and the cell where
`TRUST-SIZING`'s failure surfaces: a batch whose intent record does not fit.

By the source, `dispatch_batch` commits `CommandsPrepared` before the drain, so a
`BoundExceeded` there hands off nothing and leaves the batch intact; it commits
`CommandsDispatched` after the loop, so a `BoundExceeded` there follows every handoff and
leaves the batch drained.

Writing the tests narrowed the gap by half. `CommandsDispatched` cannot reach the
bound: at the same index its record is `{"record_kind":"CommandsDispatched","index":N}`,
and the `CommandsPrepared` record that must already have committed is the same line with
a two-byte-longer kind and a `,"commands":[…]` member of at least fourteen bytes — so
the dispatched record is always at least twelve bytes shorter than a record the bound
just admitted. A capacity failure at `CommandsDispatched` is unrepresentable in a run,
and a test for it would have to bypass the transition. The design does not say this; it
follows from the record table.

Tests, in `batch_dispatch`, following the four sibling modules' shape:

- `prepared_record_succeeds_at_exact_record_capacity` — Invariant: a prepared-command
  record that exactly fills the configured record capacity commits before the first
  handoff, and the shorter dispatched-command record then commits after the last.
- `prepared_record_one_byte_past_capacity_hands_off_nothing` — Invariant: a
  prepared-command record one byte beyond capacity fails without output, hands off no
  command, and leaves the complete batch intact. Design Doc: `JRN-ENCODE`.

**Landed.**
### S2 — a non-object that overruns the region is `BoundExceeded`

`non_object_top_level_is_rejected` proves a non-object that exactly fills the region is
`NotAnObject`, the design's "completed non-object of `max_record_bytes + 1` bytes". The
other half of that sentence is that an *incomplete* value never reaches classification:
the bound check comes first. Both oversized-record tests use objects, so the order is
unpinned for the case that distinguishes it.

By the source, `encode_raw` maps the buffer's `WriteZero` to `BoundExceeded` before
`encode_line` looks at the first byte.

Test, in `journal_object_validation`:

- `an_overrunning_non_object_is_bound_exceeded_before_classification` — Invariant: a
  non-object value too long for the encode region is rejected as a bound failure, not as
  a non-object, because the bound is checked before the object shape.

Design Doc line: `JRN-ENCODE`. **Landed.**

### S3 — a local Error from an empty latch leaves the latch open

`a_local_error_wins_when_the_latch_is_empty` proves the local Error comes back and the
latch is not pending. `ENV-LATCH` says more: "if the operation's own Error fixes first,
it is returned and a publication ordered after it stays pending" — the latch must not
have moved to reported, so the next publication lands and leaves through the close. The
`Latch` helper is what both shipped Environments will realize this clause with, and the
clause is the one the Live Environment's overlapping-publication cases turn on.

By the source, `resolve_local_error` is `take().unwrap_or(local)`, and `take` on an
empty latch restores `Empty`.

Test, in `latch_precedence`:

- `a_local_error_leaves_the_latch_open_for_a_later_publication` — Invariant: returning an
  operation's local Error from an empty latch does not report it, so the next published
  Error becomes pending and leaves through the close. Drop-tracked, so the local Error
  is proven owned by the caller and the discarded publication after the close is
  proven dropped.

Design Doc line: `ENV-LATCH`. **Landed.**

## Considered, not tested

Attacks that were resolved without a test, listed so the next reader does not re-raise
them.

- **Every Fatal cause class under an `Incomplete` report through `run`.** Journal,
  Application, and Core causes reach shutdown only through `finalize`'s `Unconsumed`
  arm, the arm that takes the report's quiescence and drops its Error, and that arm is
  pinned under an `Incomplete` report with a drop-tracked Error. Their matrix tests
  assert one shutdown, which only that arm performs. A test that reran those matrices
  under `{Incomplete, Some}` would add a row to a proof that is already closed.
- **Equal stamps at the domain ceiling.** The comparison is `<` on a `u64`; the
  ceiling is not a different code path.
- **Discarded Commands are dropped before shutdown.** Every discard site asserts the
  buffer is empty, and an empty `Vec` holds nothing; the drop precedes `finalize`
  structurally.
- **A second publication while pending is dropped at once.** The reported case is
  drop-tracked; the pending case takes the same `else` branch.
- **A map with non-string keys, tuple and unit struct payloads.** serde's rules, not
  Kavod's; the mapping they land in is pinned.

## Observations, no action

- The accepted Event is held in the Engine's loop until the next acceptance replaces
  it, or until after the finalizing `shutdown` on a Fatal exit. No rule says when the
  Core releases an Event, and under `TRUST-PURE` a payload's `Drop` observes nothing, so
  a conforming Application cannot tell. Recorded in case the Wiring close wants to say
  it.

## The batch

Landed 2026-09-06, uncommitted: four tests, no code changes. Two in `batch_dispatch`
for S1 — the two proposed for `CommandsDispatched` were dropped as unreachable, see S1 —
one in `journal_object_validation` for S2, one in `latch_precedence` for S3. Each
follows the sibling tests in its module, with the `Invariant:` sentence written above
and a `Design Doc:` line where it pins a row. The S1 tests build their certificate
directly, as the sibling capacity tests do, because the record under test is shorter
than the `RunStarted` record that would otherwise have to fit the same bound.

After the batch: `cargo test` green at 210 in-file tests plus every cross-file suite,
and `cargo clippy --all-targets -- -D warnings` clean. Only the two baseline-clean files
were formatted; the journal file's one pre-existing `rustfmt` hunk is untouched.
