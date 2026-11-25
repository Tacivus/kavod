# Second review — Synthesis

One pass over the whole crate after the twelve group reviews in `review-edges.md` and
`review-idiom.md`. It looked for what only a crate-wide read can see: a policy decided
in one group and missing in another, a helper spelled once per file, a name that crosses
a module boundary under two spellings, and a composition of group boundaries that no
group could attack alone. Behavior is the design's and did not move: every record byte,
Environment call and its order, exit value, error precedence, and API-block name is as
it was, and every suite stayed green on the batch.

Landed 2026-09-08, uncommitted. Gates after the batch: `cargo test` green (209 lib, 7
trybuild, 10 + 17 + 20 + 4 + 9 across the suites), `cargo clippy --all-targets -- -D
warnings` clean with pedantic and nursery now enforced by S1, `cargo fmt --check` clean.
Every entry was first applied to a scratch copy and run through the same gates before it
touched the tree. Net: 13 files, 3,432 lines in, 3,424 out, of which 3,401 each way are
S10's re-indent of four suites.

## Entries

- **S1 — `[lints.clippy]` pedantic and nursery in `Cargo.toml`.** Six group reports
  each reached zero pedantic and nursery hits by hand and nothing held it there. Both
  groups are `warn` at priority −1, so the `-D warnings` gate now fails on the first
  regression and a single lint can still be overridden. Verified clean on all targets;
  every `expect` attribute stayed fulfilled. The `cargo` group is not included. Landed.
- **S2 — `latch.rs`: `expect` over `allow`, two attributes deleted.** Policy from
  contracts I8, record I12, engine I10 and I22; the environment round kept three
  `allow(dead_code)`. Mutation: all three as `expect`. The struct's and the `Default`
  impl's were unfulfilled in both the lib and lib-test builds, because a struct with a
  `Default` impl is live to rustc and a trait impl is never dead code; both were
  suppressing nothing and are deleted. The inherent impl's was unfulfilled only in the
  test build, where the tests use every method, so it is
  `#[cfg_attr(not(test), expect(dead_code, ..))]`, fulfilled in the one build that can
  fire it. The fixture's three `allow`s stay for record I12's reason. Landed.
- **S3 — `#[must_use]` on crate-private accessors, as if public.** Environment I4 and
  record I5 chose this because `must_use_candidate` is silent on private items;
  foundations and contracts had not applied it to theirs. Eight sites:
  `BoundedBuffer::{len, capacity, is_empty, as_slice, remaining}`,
  `Context::overflowed`, `EventIndex::new`, `Kind::new`. Landed.
- **S4 — `Copy` on `EnvironmentFatal<EE>`.** `ShutdownReport<E>` and `Outcome<E>`
  derive `Copy` under a generic payload; this was the one generic struct that did not,
  with `EnvironmentOperation` already `Copy`. API additive. Landed.
- **S5 — `Display` on `RecordKind` and `TurnOutcome`.** `EnvironmentFatal`'s `Display`
  delegates to `EnvironmentOperation: Display`; `JournalFatal`'s reached into the
  private `tag()` and printed the outcome with `{:?}`. Same seam, one shape;
  `JournalFatal::fmt` now uses `{}` for both. API additive. Landed.
- **S6 — `From<JournalFatal> for FatalCause<AE, EE>`.** Seven sites spelled the same
  lift, `map_err(FatalCause::Journal)`, four in `record.rs` and three in `engine.rs`;
  `?` carries it now and `close` uses `error.into()` inside its `fatal` closure. The
  value and precedence are the same function applied by a different path. The fixture's
  hand-copied `FatalCause` gained the same five-line impl; all seven trybuild snapshots
  stayed byte-identical. API additive. Landed.
- **S7 — one import style.** `application.rs`, `latch.rs`, and `environment.rs`
  reached sibling modules through the crate root while the other six files use the
  module path; `record.rs` alone nested its `std` imports on one line. All four now
  match the majority. Landed.
- **S8 — `env` in `engine.rs`.** The struct field, the API block's `new` parameter,
  `drive`, and `run` said `env`; `effects` and `finalize`'s `Unconsumed` arm said
  `environment`. Four lines. `record.rs` uses `environment` throughout its transitions
  and their tests and keeps its one name. Landed.
- **S9 — `BoundedBuffer::reserved`, `#[cfg(test)]`.** `fn reserved` existed in the
  test modules of `bounded_buffer.rs`, `application.rs`, and `engine.rs`, and
  `record.rs` inlined `BoundedBuffer::new(n).expect(..)` at thirteen sites under five
  messages. One associated constructor; 51 call sites. The ten engine sites spell
  `BoundedBuffer::<u8>::reserved` because the old helper fixed the element type and
  `no_commands(&reserved(1))` infers nothing else. Landed.
- **S10 — the four engine-group suites lost their `#[cfg(test)] mod tests` wrapper.**
  Contracts I10 and record I17 removed it from `ports_macro.rs` and `compile_fail.rs`;
  `conformance.rs`, `faults.rs`, `golden_journal.rs`, and `harness_contract.rs` kept it.
  `use super::support::` became `use support::`; nothing else in the bodies changed
  except indentation. The dedent exposed fifteen `missing_const_for_fn` hits the wrapper
  had hidden, all taken as `const fn`. Test names no longer carry `tests::`; no design
  doc cited one. Landed.

## Edges

Compositions the pinned table in `review-adversarial.md` does not list. No open attack.

| Composition | Resolution |
|---|---|
| `Dispatch { position }` (engine E1) indexes the failing turn's `CommandsPrepared.commands` array (record D2) | derived — `a_later_turn_dispatch_failure_reports_its_batch_relative_position` pins `position: 1` against the index-1 record `[20,21]` |
| the decimal width of `index` (time T8) grows a record past `max_record_bytes` (journal C6) on a later turn (record E6) | by construction — one `commit` path bounds each record's encoded bytes; the kind is pinned per record by the fault matrix; the golden suites run at 256 bytes and never approach it |
| the Journal's non-poisoning recovery after `Encode`, `NotAnObject`, `BoundExceeded` (journal C3, B21) met by the certificate's affinity (record) | derived — every `JournalError` through `Certificate::commit` consumes the certificate, so the path is reachable only by a standalone `Journal` caller; deferred below |
| `Hash` on `CoreError` (engine I5) needs `Hash` on `Timestamp` (foundations I1) | by construction — landed in that order |
| S4's `Copy` and S6's `From` against `FatalCause` | by construction — `JournalFatal` holds `io::Error`, so `FatalCause` stays `Debug` only and the conformance `ExitShape` projection stands |

## Hierarchy and exports

The seven error types are right. `BuildError` and `JournalBuildError` mirror the two
reservations `Engine::new` makes. `JournalError` names what failed at one commit and
`JournalFatal` adds which record. `FatalCause` has two bare arms and two "error plus
where" arms, `EnvironmentFatal` and `JournalFatal`, of one shape. Every one carries
`Display` and `Error` with `source()` since journal I1 propagated to engine I1. The
assertion policy is closed: all ten production sites name an owner.

`lib.rs` exports 28 items. All are reachable from the Engine's API except `Journal`,
which the grammar fixture reaches as `kavod::{Journal, JournalError}` and no Engine
caller needs. `Latch` stays `pub(crate)`. `engine/mod.rs` and `lib.rs` each list the
eleven engine items once, as `CRATE-EXPORTS` asks.

## Deferred

Design notes for the Wiring close:

- The §11 layout table has no `latch.rs`; `Latch` has no caller (environment).
- The sink drops before `shutdown` on a Fatal exit and after it on the Stop path; a
  failed `CommandsPrepared` commit leaves the batch to drop after `shutdown` (engine
  observations).
- `Reported` and `Closed` are indistinguishable through the `Latch` API; the
  start-failure drop instant is unfixed (environment observations).
- A `TurnOpen` carrying its Event would delete the `turn` assertion (engine design note).
- G9, `APP-FUTURE` has no named enforcement (contracts C21).
- The Journal's non-poisoning path is unreachable through the Engine (this pass).
- Ledger G6 is stale: all ten production assertions now name an owner (foundations I5,
  record I4, engine I6).

API-touching, for the export audit:

- `Deserialize` on `EventIndex` and `Timestamp` (foundations hand-on).
- API-block derive lists trail the code on `TurnOutcome` (record note),
  `EnvironmentOperation` and `CoreError` (engine note), `Quiescence`, `RecordKind`,
  `SinkOperation`, `JournalBuildError`, `ShutdownReport`, and after S4 and S5
  `EnvironmentFatal` and `RecordKind`.
- `# Errors` wording on `Journal::new`, `commit`, the three `Environment` operations,
  and `Engine::new` (journal I11, environment I10, engine I10).
- `Journal` is public only for the fixture (this pass).
- `missing_docs` is off; `Application`, `Outcome`, `EngineConfig`'s fields,
  `EngineExit`, `FatalCause`'s variants, `RecordKind`, `TurnOutcome`, `PortContract`,
  `Never`, and `ports!` carry no rustdoc (this pass).
