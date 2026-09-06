# Round 0 — The Ledger

Every invariant ID and every law, resolved against the code as it stands at the
compile-fail fixture. Built mechanically: 267 tests parsed for their `Design Doc:`
citations, every assertion site in the non-test regions of `src/` extracted, and every
Core-scope verification row split into its clauses.

Three resolutions, and every row carries one:

- **pinned** — named tests hold it, or it is unrepresentable and the site is named.
  The design's enforcement order is unrepresentable beats asserted beats tested, so a
  named construction site is a pin, not an excuse.
- **deferred** — the enforcing suite belongs to a later step, named in the row.
- **gap** — Core-scope, enforceable now, nothing records it.

**Result: 14 gaps.** Eleven of them are a test that already exists without the citation
that would make it enforcement. Three are real absences.

## Laws

| ID | Resolution | Where |
|---|---|---|
| A1 single authority | **gap** — G1 | `certificate_duplication_does_not_compile` and `acceptance_advances_index_and_time_only_on_commit` prove the certificate is the sole owner of index and time; neither cites A1 |
| A2 serial turns | pinned | `run_turn_loop::continue_turns_accept_events_in_sequence`; `run_stop_path::the_call_sequence_matches_env_serial` |
| A3 one commitment point | **gap** — G2 | `journal_commit_boundaries::only_successful_flush_advances_the_committed_boundary`, `batch_dispatch::prepared_commit_failure_precedes_any_handoff`, `turn_classification::classify_commits_nothing_for_either_answer` all prove it, uncited |
| A4 first failure wins | pinned | 5 tests across `latch`, `engine`, `faults` |
| A5 intent precedes effect | pinned | `batch_dispatch::prepared_then_each_handoff_in_order_then_dispatched` |
| A6 bounded everything | pinned | 5 tests across `bounded_buffer`, `time` |
| A7 typed inside, rendered at the edge | **gap** — G3 | no test, no named site; the Core's error types are generic over the user's Error and carry no rendering, which is an unrepresentable-tier pin if someone writes it down |
| A8 panics are bugs | pinned | `journal_poisoning::commit_on_poisoned_journal_panics`; `panic = "abort"` in both profiles — but see G12 |
| A9 determinism | **gap** — G4 | the five `conformance_within_type::the_same_trace_reproduces_*` tests are exactly A9's claim, all cited to `DET-RUN` alone |

## Enforcement definitions

| ID | Resolution | Where |
|---|---|---|
| `ASSERT-INVARIANTS` | **gap** — G6 | pinned as behavior by `no_commands_panics_on_a_nonempty_buffer` and `an_empty_buffer_is_an_invariant_panic`; but 7 of the 13 assertion sites name no invariant |
| `BOUND-LOOPS` | **gap** — G5 | its three Core loops each have a test — `event_acceptance::the_domain_check_precedes_next_event` (index domain), `batch_dispatch::*` (batch length), `journal_sink_writes::zero_progress_maps_to_write_zero` (record length, no spin) — and none cites it. Environment budgets deferred to C42, C52 |

## Application contract

| ID | Resolution | Where |
|---|---|---|
| `APP-CONTEXT` | pinned | `context_emit::remaining_reports_exact_free_capacity` |
| `APP-EMIT` | pinned | `context_emit::commands_append_in_call_order_through_exact_capacity` |
| `APP-OVERFLOW` | pinned | 5 tests in `context_overflow`, `context_reuse`, `turn_overflow_precedence` |
| `APP-FUTURE` | **gap** — G9 | enforced by `Context`'s API shape — `emit` is the only channel — with nothing naming the site and no compile-fail case |
| `APP-STATE` | pinned | 3 tests across `engine`, `faults` |

## Port contract

| ID | Resolution | Where |
|---|---|---|
| `PORT-STATE` | deferred — C37, C47 | the Core half is the `ports!` sums; exclusive Port ownership and discriminant-only routing are `TRUST-ROUTING`, verified per-Slot once wiring closes |
| `PORT-SUMS` | pinned | 3 tests in `port`, `ports_macro` |
| `PORT-ROUTING` | pinned, partial | `ports_macro_downstream::the_fanout_match_is_exhaustive`. The per-Slot Error sum is "placed finally when Wiring closes" by the row's own words — deferred there |

## Environment contract

| ID | Resolution | Where |
|---|---|---|
| `ENV-SERIAL` | pinned | `run_stop_path::the_call_sequence_matches_env_serial` |
| `ENV-START` | pinned | `fatal_finalization::a_start_error_skips_shutdown_and_is_quiesced`; `run_startup::a_start_error_exits_fatal_quiesced_without_shutdown` |
| `ENV-ERRORS` | **gap** — G7 | `environment_fault_matrix::each_operation_error_maps_to_its_cause_and_quiescence` proves the pre-commitment half and cites `VERIFY-FAULTS` only; the post-commitment half is `ENV-LATCH`, pinned. Naming the activation and consumption instants is each implementation's, deferred to C37, C49 |
| `ENV-LATCH` | pinned | 6 tests in `latch` |
| `ENV-TIME` | pinned | `event_acceptance::an_equal_stamp_is_accepted`; `timestamp_arithmetic::equal_timestamp_is_valid` |
| `ENV-SHUTDOWN` | deferred — C41, C51 | the Core consumes the report and that half is pinned by `stop_closing::*` and `fatal_finalization::*` under `RUN-FINALIZE`; the window, the signal, and the final observation are the implementations' |
| `ENV-SEPARATION` | deferred — C37, C49 | |
| `ENV-BOUNDS` | deferred — C42, C52 | by the row's own words: `VERIFY-SIM` and `VERIFY-LIVE` pin it |

## Journal

Every row pinned: `JRN-FORMAT` (1 test), `JRN-ENCODE` (11), `JRN-COMMIT` (3),
`JRN-POISON` (8), `JRN-SINK` (1). No gaps.

## The Run

| ID | Resolution | Where |
|---|---|---|
| `RUN-SERIAL` | **gap** — G8 | every claim holds — the Engine owns `env` and `journal` by value, `Environment::shutdown(self)` consumes, and `the_call_sequence_matches_env_serial` proves the ordering — and nothing cites the row |
| `RUN-GRAMMAR` | pinned | 5 tests |
| `RUN-ENFORCEMENT` | pinned | 4 tests |
| `RUN-RECORDS` | pinned | 10 tests |
| `RUN-INDEX` | pinned | `event_acceptance::the_domain_check_precedes_next_event` for the check; `acceptance_advances_index_and_time_only_on_commit` for the advance. The overflow assertion at `src/engine/record.rs:416` is unreachable behind that check and has no test, correctly |
| `RUN-CHECKPOINT` | pinned | 3 tests |
| `RUN-FINALIZE` | pinned | 7 tests |
| `DET-RUN` | pinned | 2 cited plus 5 uncited reproduction tests in `conformance_within_type` |
| `DET-ENV` | deferred — C56 | |

## Laws guarantees and crate layout

| ID | Resolution | Where |
|---|---|---|
| `NO-UNSAFE` | pinned, by construction | `src/lib.rs:1` |
| `BOUND-NONZERO` | pinned, by construction | `EngineConfig`'s two `NonZeroUsize` fields and `Journal::new`. Environment capacities deferred to C37, C47 |
| `BOUND-STATIC` | deferred — C37, C49 | |
| `CRATE-EXPORTS` | deferred — C57 | |

## Core-scope verification rows, by clause

**`VERIFY-CONTEXT`** — 5 clauses, 5 pinned.

| Clause | Test |
|---|---|
| Commands append in call order through exact capacity | `context_emit::commands_append_in_call_order_through_exact_capacity` |
| First over-bound `emit` stores nothing and sets the marker | `context_overflow::first_over_bound_emit_stores_nothing_and_sets_the_marker` |
| Every later emission stores nothing | `context_overflow::every_later_emit_stores_nothing` |
| Each fresh handler starts empty with a clear marker | `context_reuse::fresh_invocation_starts_empty_with_a_clear_marker` |
| State mutations stand on every Fatal path | `application_fault_matrix::state_mutations_survive_each_post_handler_fatal_exit`; `run_turn_loop::state_mutations_stand_on_every_fatal_exit` |

**`VERIFY-JOURNAL`** — 4 clauses, 4 pinned.

| Clause | Test |
|---|---|
| Every graph-required record sequence | `graph_sequences::every_empty_and_command_turn_shape_has_its_required_sequence` |
| Every record byte-exactly | the three `golden_sequences::*_writes_exactly_its_records` |
| Each non-Fatal answer against its outcome records at `classify`'s single call site | `classify_call_site::each_non_fatal_answer_yields_its_required_outcome_records` |
| An interior newline is `NotAnObject` with nothing written | `encoding_rejection::an_interior_newline_payload_is_rejected_with_nothing_written` |

**`VERIFY-FAULTS`** — 9 clauses, 9 pinned.

| Clause | Test |
|---|---|
| Scripted sinks for Journal failures | `journal_fault_matrix::each_record_kind_maps_to_its_journal_fatal` |
| Each Environment operation's `Err` | `environment_fault_matrix::each_operation_error_maps_to_its_cause_and_quiescence` |
| An `Ok` `next_event` with a decreasing timestamp | `environment_fault_matrix::a_decreasing_stamp_is_time_regression` |
| A report carrying `Some(error)` | `stop_closing::a_quiesced_report_error_is_shutdown_fatal` |
| A report carrying `{ Incomplete, None }` | `stop_closing::incomplete_without_error_is_shutdown_incomplete` |
| An over-emitting Application | `application_fault_matrix::an_over_emitting_application_is_command_bound_exceeded` |
| `Quiesced` retained across a `TurnCompleted(Stop)` commit failure | `journal_fault_matrix::a_stop_commit_failure_retains_quiesced` |
| Every post-`start` operation `Err` crossed with a `Some(error)` report | `environment_fault_matrix::the_operation_error_outranks_the_report_error` — it loops the fault points, so the cross-product is genuine |
| A `start` `Err` performs no shutdown | `startup_faults::a_start_error_performs_no_shutdown` |

The two report clauses are discharged in `src/engine/record.rs`, not in the fault suite
the row describes. See G14.

**`VERIFY-GRAMMAR`** — 7 clauses, 7 pinned, and six fixture cases beyond them.

| Clause | Fixture case |
|---|---|
| Illegal transition sequences | `illegal_transition_order` |
| A skipped checkpoint | `skipped_checkpoint` |
| A premature `TurnCompleted(Stop)` | `premature_stop_completion` |
| Independent `CommandsDispatched` commit | `independent_commands_dispatched` |
| An outcome disagreeing with the fixed answer | `disagreeing_outcome` |
| `Clone`, `Copy`, `Default` on the certificate | `certificate_clone`, `certificate_copy`, `certificate_default` |
| The fixture reconstructs the module and compiles | `legal`, `the_fixture_reconstruction_itself_compiles` |

Beyond the row: `between_turns_only_accepts_event`, `closed_is_terminal`,
`event_before_continue_completion`, `initial_context_access`, `repeated_run_started`,
`unclassified_checkpoint_dead_end`.

**`VERIFY-CONFORMANCE`** — 5 clauses, 2 pinned, 2 deferred, 1 gap.

| Clause | Resolution |
|---|---|
| Every scripted Environment call checked against the graph, Command handoffs included | pinned — `every_environment_call_is_graph_conformant` |
| Within a type, each trace run twice, comparing every value in `DET-RUN`'s list | pinned — five `the_same_trace_reproduces_*` tests cover handler calls, State transitions, Command intent, Journal bytes, and exits |
| Across the two shipped Environments, every Core-owned discriminant and payload in `DET-ENV`'s list | deferred — C56 |
| The expressible cross-type overlap | deferred — C56 |
| The same suite certifies a bespoke Environment | **gap** — G13 |

## Deferred Environment rows

`LIVE-THREADS`, `LIVE-EVENTS`, `LIVE-SELECT`, `LIVE-TIME`, `LIVE-DISPATCH`,
`LIVE-SUPERVISION`, `LIVE-COMPLETION`, `LIVE-LIFECYCLE`, `LIVE-START`, `LIVE-SHUTDOWN`
→ C44–C51, pinned by `VERIFY-LIVE` at C52–C54.

`SIM-STATE`, `SIM-LIFECYCLE`, `SIM-START`, `SIM-TIME`, `SIM-DISPATCH`, `SIM-WAKEUP`,
`SIM-SELECT`, `SIM-STEPS`, `SIM-COMPLETION`, `SIM-SHUTDOWN` → C36–C41, pinned by
`VERIFY-SIM` at C42.

`VERIFY-LATCH` → C55.

## Obligations

Trusted rows are not enforced, but each names a verification means, and the ledger
records whether that means exists.

| ID | Means exists |
|---|---|
| `TRUST-PURE` | yes — `conformance.rs`'s two-run comparison is exactly the stated means, uncited. **G10** |
| `TRUST-SERIALIZE` | yes — the golden-Journal suite, uncited. **G11** |
| `TRUST-SINK` | yes, cited — 2 tests |
| `TRUST-ABORT` | half. `panic = "abort"` is set in both profiles; the stated CI build-profile check does not exist, because the repository has no CI. **G12** |
| `TRUST-BLOCKING`, `TRUST-EXIT`, `TRUST-MEMORY`, `TRUST-SPAWN` | review-based; nothing to build |
| `TRUST-SIM-PORT`, `TRUST-ENV`, `TRUST-ROUTING`, `TRUST-KEY`, `TRUST-LIFECYCLE`, `TRUST-DRAIN`, `TRUST-SIZING`, `TRUST-INBOX`, `TRUST-SHUTDOWN` | deferred to the Environment steps |

## Assertion sites

Thirteen sites in the non-test regions of `src/`. Six name the invariant they enforce;
seven name only an English sentence.

| Site | Names an ID |
|---|---|
| `src/engine/record.rs:303`, `:319` | `ASSERT-INVARIANTS` |
| `src/engine/record.rs:416` | `RUN-INDEX` |
| `src/engine/record.rs:505` | `RUN-ENFORCEMENT` |
| `src/journal.rs:76` | `JRN-POISON` |
| `src/application.rs:78` | no — Context buffer length against logical capacity |
| `src/bounded_buffer.rs:21`, `:33`, `:68`, `:80` | no — reservation and length against logical capacity |
| `src/engine/engine.rs:68`, `:81` | no — the start-turn and later-turn Event invariant |

Whether the panic message must carry the ID or only the sentence is a build-rule
reading, not a design rule. Either answer is fine; the seven and the six disagreeing is
not. **G6.**

## Gaps

Eleven citations and three absences.

| | Gap | Kind |
|---|---|---|
| G1 | A1 has no citation on the tests that prove it | citation |
| G2 | A3 has no citation | citation |
| G3 | A7 has neither a test nor a named construction site | absence |
| G4 | A9 has no citation; its tests all cite `DET-RUN` instead | citation |
| G5 | `BOUND-LOOPS` has no citation on any of its three Core loops | citation |
| G6 | Seven of thirteen assertion sites name no invariant, six do | citation |
| G7 | `ENV-ERRORS` has no citation on the fault-matrix test that proves its Core half | citation |
| G8 | `RUN-SERIAL` has no citation | citation |
| G9 | `APP-FUTURE` has no named enforcement of any tier | absence |
| G10 | `TRUST-PURE`'s verification means exists uncited | citation |
| G11 | `TRUST-SERIALIZE`'s verification means exists uncited | citation |
| G12 | `TRUST-ABORT`'s CI build-profile check does not exist | absence |
| G13 | The conformance suite cannot certify a bespoke Environment — it is a private integration test, not something an external author can run | absence |
| G14 | `VERIFY-CONTEXT` and `VERIFY-FAULTS` each describe one suite; their clauses are split across `src/` unit modules and `tests/` files, so neither has a single named target | classification |

Three of these need a decision rather than a patch. **G3** and **G9** ask whether an
API shape that makes something unrepresentable counts as enforcement with the site
merely named — the design's own enforcement order says yes, but nothing in the crate
records the site. **G13** is the one with weight: `TRUST-ENV` says a bespoke
Environment's certification *is* this suite, and today the suite cannot leave the
crate. That is a packaging decision, and it belongs with the export audit at C57 rather
than here.

**G12** is a half-hour of CI and removes a trusted row's only stated check.

The remaining ten are citation edits inside existing test doc comments. They are worth
doing not for tidiness but because the ledger is only re-runnable if citations are
complete: the next person to ask "what pins A3?" gets the same silence this round did.
