# Second review — Idiom

The Core reread once per group for shape: derives, visibility, signatures, control
flow, assertions, lints, names, and test setup. Behavior is the design's and did not
move: every record byte, Environment call, exit, and precedence is as it was, and the
suites that pin them stayed green on every batch. Each group's entries are numbered as
its report numbered them and marked landed or declined with the reason given.

## foundations

`src/time.rs`, `src/bounded_buffer.rs`. Landed 2026-09-08, uncommitted. Before: 12
pedantic and nursery hits across the two files. After: 0. Gates: all suites green,
`clippy -D warnings` clean, both files `rustfmt --check` clean.

- **I1 — `Hash` on `EventIndex` and `Timestamp`.** Both are `Eq` over one `u64`; a
  Port keying by index or a simulator keying arms by time could not add it from outside.
  `Default` stays off: a default `Timestamp` would mint the origin `ENV-TIME` gives the
  Environment, and a default `EventIndex` would be the public constructor `RUN-GRAMMAR`
  forbids. API additive. Landed.
- **I2 — `#[must_use]` on `as_u64`, `from_nanos`, `checked_add`, `as_nanos`.** A dropped
  `checked_add` loses the advance and the overflow signal; std marks its own the same
  way. Took the three `must_use_candidate` hits. Landed.
- **I3 — `#[serde(transparent)]` on both newtypes.** The API block's "transparent u64"
  now lives in the type instead of resting on serde_json's newtype convention; no byte
  moved and `both_serialize_as_transparent_u64` still pins it. Landed.
- **I4 — one private `remaining` owns the length invariant.** `try_push` and `write`
  each spelled it their own way; both now branch on `remaining() == 0`. Refusal and
  panic outcomes are unchanged for every input. Made `const` so the pedantic run stays
  at zero. Landed.
- **I5 — every assertion in `bounded_buffer.rs` names A6.** One of five did; the
  ledger's G6 records the crate-wide split. This file's share is closed. Landed.
- **I6 — `#[derive(Debug)]` on `BoundedBuffer`.** `Context` and `Journal` are public
  and hold one; this field was the only obstacle to `Debug` on them. Landed.
- **I7 — `const fn` on eight accessors and constructors.** The eight
  `missing_const_for_fn` hits; `record.rs` already used the shape. API additive.
  Landed.
- **I8 — `#[expect(clippy::redundant_pub_crate)]` on the struct.** The module is
  private so `pub` reaches the same set, but would read as an export in a crate whose
  exports all pass through `lib.rs`. Verified fulfilled under plain
  `clippy -D warnings` and under `-W clippy::nursery`. Landed.
- **I9 — `use std::io` in the `Write` impl.** Four `std::io::` paths in twelve lines;
  `journal.rs` already spells it this way. Landed.
- **I10 — tests: one `reserved(n)` helper; `encode_buffer_*` became
  `encode_region_*`.** Eighteen constructor sites shared one sentence; the modules test
  the `Write` impl in its `JRN-ENCODE` role, which the design calls the encode region.
  `construction_failure_reports_the_reservation_error` keeps calling `new` because it
  asserts the `Err`. No doc or suite cited the module names. Landed.

Fixture: `tests/grammar_fixture/src/lib.rs` mirrors I1, I3, I6, I7 on its hand copies.
The trybuild snapshots point only at case files, so the fixture's line shift is free.

Handed on, not this group's files:

- `Context::remaining` (`src/application.rs`) derives the same quantity as I4's
  `remaining` with a third message; widening I4's helper to `pub(crate)` is the Context
  group's call.
- `redundant_pub_crate` fires once more, on `latch.rs`.
- `design_docs/impl-steps.md` cites `timestamp_arithmetic::equal_timestamp_is_valid`,
  renamed to `zero_elapsed_preserves_the_timestamp` in the uncommitted round-3 diff.
- The API block derives `Serialize` only; reading a Journal back with the crate's own
  types needs `Deserialize` on both newtypes. Export audit's call.

## journal

`src/journal.rs`. Landed 2026-09-08, uncommitted. Before: 7 pedantic and nursery hits
and one `rustfmt` hunk. After: 0 and 0. Gates: all suites green (211 lib, 43 of them
this file's), `clippy -D warnings` clean, the file `rustfmt --check` clean; the
`cargo fmt --check` hunks that remain are in `environment.rs`, `port.rs`, and
`tests/ports_macro.rs`. `Journal::new` and `commit` kept their signatures, so the
fixture is untouched.

- **I1 — `Display` and `Error` with `source()` on `JournalBuildError` and
  `JournalError`.** Both wrap a std error a caller reaches through `source()`. First
  error type in the crate to carry either trait; the export audit decides whether
  `BuildError` and the Fatal types follow. API additive. Landed.
- **I2 — `Clone, PartialEq, Eq` on `JournalBuildError`; `Clone, Copy, Hash` on
  `SinkOperation`; `Debug` on `Journal`.** `TryReserveError` already has the first
  three; the second is a fieldless enum; the third is what foundations I6 prepared, and
  the derive bounds only the impl, so a non-`Debug` sink still builds. `JournalError`
  stays `Debug` only: `serde_json::Error` and `io::Error` are neither `Clone` nor
  `PartialEq`. API additive. Landed.
- **I3 — `write_line` classifies first and poisons once.** The three arms produce an
  `io::Error` and one `?` through `poison` follows, the shape `commit` already uses for
  the flush; `remaining_len` went with it. Every arm is pinned by
  `every_sink_failure_poisons_exactly_once` and the `faults.rs` matrices, all
  unchanged. Landed.
- **I4 — `#[must_use] pub const fn is_poisoned`; `const fn poison`.** The two
  `missing_const_for_fn` hits; `must_use` matches the foundations accessors. Landed.
- **I5 — one `AlwaysFails` at the `tests` root.** Four identical failing serializers
  differed only in a message no test asserted. Landed.
- **I6 — `CountingWriter` deleted; seven `journal_encoding` setups call
  `line_journal(n)`.** It duplicated `LineCountingWriter` field for field. Landed.
- **I7 — `NonZeroUsize::MIN` for the five `NonZeroUsize::new(1).expect(..)`.** Two of
  the five went through I6. Landed.
- **I8 — `expect_sink_failure(error, operation) -> io::Error`.** Replaces six
  `match … _ => panic!` blocks; each site keeps only its kind and message asserts. The
  helper's non-`Sink` arm names every variant, so a new `JournalError` variant fails to
  compile here instead of falling into a wildcard. Landed.
- **I9 — `ScriptedSink::write` and `flush` return the `Result` they matched.**
  `if let … && …` and `if result.is_ok()` replace two matches that rebuilt their
  scrutinee. Landed.
- **I10 — the eight-row table moved to `fn failure_cases() -> [FailureCase; 8]`.**
  The `too_many_lines` hit (166/100); `faults.rs` keeps its table the same way. The
  `redundant_clone` on the last `remaining_after_one` went with it. Landed.
- **I11 — `# Errors` sections on `new` and `commit`.** The two `missing_errors_doc`
  hits; one sentence each pointing at the enum. Export audit may reword. Landed.
- **I12 — backticks on the one `/// Design Doc: JournalBuildError` citation.** The
  `doc_markdown` hit. Landed.
- **I13 — the file's pre-existing `rustfmt` hunk at the old line 1837.** Landed.

Handed on, not this group's files:

- No other error type in the crate implements `Display` or `Error`; I1 is the
  precedent for `BuildError`, `CoreError`, `EnvironmentFatal`, and `JournalFatal`.
- `JournalFatal` (`src/engine/record.rs`) derives nothing; `Debug` is now free since
  every field has it.
- `tests/faults.rs:606` cites `/// Design Doc: JournalFatal` without backticks, the
  same `doc_markdown` shape as I12.
