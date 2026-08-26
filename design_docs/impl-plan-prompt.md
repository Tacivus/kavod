# Implementation-plan prompt

Paste everything below the line into a fresh session as one prompt.

---

You are producing the implementation plan for Kavod Core from its design document. The
plan will be executed **by the user personally, by hand** — they are newer to Rust and
this is their biggest Rust project — so the plan's job is not just correct sequencing but
**confidence**: small verifiable steps, no rework, and no step whose success can't be
seen immediately.

INPUTS — exactly two files:

1. `design_docs/design-v12.md` — the design document. It is authoritative and it stands
   alone by declaration; every question you have is answerable from its text or is a
   finding.
2. `design_docs/test.md` — the test design pattern. It is a directive, not a suggestion:
   **every test the plan calls for MUST follow it** — unit tests in `#[cfg(test)] mod
   tests` in the file they test; cross-file suites under `tests/`; the compile-fail
   fixture as it describes; one nested module per subject-and-behavior, named
   `<subject>_<behavior>`; every test doc-commented with the specific invariant it
   verifies, cited by ID (never a section number), names cited by name where no ID
   exists; tests named for the observable behavior verified. The plan's per-chunk test
   lists must be written in this shape from the start, so the user never retrofits
   structure onto existing tests.

Do NOT read anything else in `design_docs/` or its history — review artifacts, surveys,
and prior plans are deleted or obsolete. Treat any existing `src/` as obsolete: you are
planning a fresh implementation against the document, not a migration.

Read the design document in full — §0's reading rules first, then every section — before
writing a word of the plan. §0 tells you what binds: API blocks are exact, guarantee rows
and binding tables are rules, Obligations rows are trusted, Mechanism prose is one
replaceable nonbinding realization. The `VERIFY-*` rows are not documentation — they are
required test targets, and an ID "enforced by suite" is unimplemented until its suite
exists and passes.

THE DOCUMENT IS NOT UP FOR REVIEW. It has been through a full adversarial round; its
design decisions are settled, and re-litigating them is out of scope. Your deep pass has a
different target: **implementation blockers** — places where you cannot write conforming
Rust without information, a decision, or a resolution the document does not supply. The
distinction that keeps you honest: a design critique says "this rule should be different";
a blocker says "I cannot discharge this rule as written until X is answered." Report only
the second kind. If a sentence merely leaves you a free implementation choice, that is not
a blocker — choose, and record the choice in the plan.

ITERATIVE AND FORWARD-ONLY — the plan's two hard constraints:

- **No rework, ever.** Every chunk produces final code that later chunks *extend*, never
  replace. No placeholder implementations, no "temporary version now, real version
  later", no scaffolding in `src/` that a later chunk deletes. If your sequencing would
  require rewriting something already built, the sequencing is wrong — fix the order, not
  the code. A later chunk may *grow* an earlier one (new method, new variant the design
  already names); the plan must call out every such touch explicitly so nothing is
  modified by surprise. Two clarifications: throwaway *probes* in a scratch directory are
  exploration, not implementation — they're exempt; and the scripted Environments, memory
  sinks, injected clocks, and fixture crates are NOT throwaway — the `VERIFY-*` rows make
  them permanent test infrastructure, so building them early is real work, not scaffolding.
- **Bite-sized, always-green chunks.** The unit of work is a chunk the user can finish in
  one sitting: one type, one mechanism, or one behavior — with its tests — and small
  (target well under ~150 lines of new code including tests; split anything bigger).
  Every chunk ends with `cargo test` (and `cargo clippy`) green and something *newly
  demonstrated*: a named test that did not pass before. There is no chunk whose
  correctness must be taken on faith until a later chunk; if a piece can only be tested
  after its neighbor exists, the neighbor comes first or they merge into one still-small
  chunk. Guarantees and their suites land together — never "tests later".

DEEP BLOCKER PASS — walk all of these before planning, and settle what you can by
compiling probes (mark those answers "executed", the rest "reasoned"):

1. **Every API block, as Rust.** Signatures, bounds, variant sets, consuming receivers,
   derive lists. Can each block exist exactly as written? Where blocks interact (the
   certificate owning the Journal while transitions borrow the Environment; `run(self)`
   destructuring; `Context`'s lifetime over the reusable buffer; `ports!` as
   `macro_rules!`; the `Never` uninhabited `Serialize`; the record kind-marker's
   hand-written `Serialize` sharing one source with `JournalFatal`), write a 20–50-line
   probe and compile it. A probe that fails is a blocker with evidence.
2. **Every "unrepresentable" claim.** For each, name the exact Rust mechanism that will
   carry it (module privacy, affine consumption, typestate parameter, uninhabited type).
   One you cannot name a mechanism for is a blocker.
3. **Every always-on assertion.** The document names its assertion sites; enumerate them
   into a checklist the code must satisfy.
4. **Every `VERIFY-*` row, as a test suite you must build.** For each bullet, ask: what
   test harness does this need (scripted Environments, scripted sinks, injected clocks,
   golden files, the `include!`-based compile-fail fixture per `test.md`, threaded
   lifecycle tests)? A bullet you cannot see how to test is a blocker.
5. **§10 Wiring — the known-open section.** Its openness is not a finding; it is the
   decision backlog. Enumerate every decision it lists, state exactly which chunks each
   one blocks, and **propose a concrete answer for each** (with one line of rationale)
   for the user to approve. Note the constraints §10 already fixes — your proposals must
   satisfy them.
6. **The seams.** Concurrency machinery (the latch's lock discipline, the shutdown final
   observation as one critical section, publication-precedes-Complete, the start/cancel
   gate), the Journal's bounded buffer implementing `std::io::Write`, and the byte-exact
   record format — anywhere the document specifies observable behavior whose realization
   is subtle, name the realization the plan will use.

HOUSE PREFERENCES (binding on the plan):
- Associated functions over free functions; avoid re-declaring generics.
- Liberal always-on assertions, within the document's `ASSERT-INVARIANTS` discipline.
- Module convention: `mod_name/mod.rs` with `mod.rs` wiring-only (declarations and
  re-exports), per the document's crate layout.
- All test structure, naming, placement, and doc-comment citation per
  `design_docs/test.md`, without exception.
- The plan is approved before any `src/` code exists. Do not write implementation code in
  this session beyond throwaway probes in a scratch directory.

OUTPUT — write `design_docs/impl-plan.md`, then STOP for approval. Structure:

1. **Blockers & decisions register** — first, because it gates everything. Three
   categories, one table each: (a) §10 decisions, each with your proposed answer;
   (b) genuine blockers found by the deep pass, each with the quoted text, why code
   cannot proceed, and the smallest resolution (expect few — the document is mature; an
   empty table is a legitimate result you must be able to defend); (c) free choices you
   made, each with the choice recorded so it is deliberate, not accidental.
2. **Probe results** — each realizability probe: what it tested, executed/reasoned,
   verdict, and the probe code preserved in the plan for the user to reuse.
3. **Chunked build plan** — the core deliverable. Phases give the shape (the document's
   section order is dependency order: foundation types → Journal → Port/`ports!` →
   certificate grammar and Engine → Environment-contract test doubles → Sim → Live →
   Wiring last — which conveniently doubles as a Rust learning ramp, simple leaf types
   before typestate and threads), but the unit is the **chunk**, numbered C1, C2, …
   For every chunk:
   - **Builds:** the items and files touched (and any explicit, justified touch to an
     earlier chunk's code).
   - **Discharges:** the design-doc IDs or table rows this chunk makes true.
   - **Proves:** the named test(s) that go green in this chunk — the done-when — written
     in `test.md` form: group `<subject>_<behavior>`, test named for the observable
     behavior, and the invariant ID its doc comment will cite.
   - **Rust notes:** the language concepts this chunk exercises (ownership moves,
     `PhantomData`, `macro_rules!`, trait objects, lifetimes, `Mutex`/`Condvar`, …) with
     one line on the tricky part, so the user knows what to read up on before starting.
   - **Size:** rough line count, as a splitting signal.
   Also mark which chunks are §10-blocked so everything else can proceed while those
   decisions are pending.
4. **Suite build-out map** — every `VERIFY-*` row mapped to the chunks that build it and
   the shared harness pieces it needs (scripted Environment, memory sink, injected clock,
   compile-fail fixture crate per `test.md`), so the permanent test infrastructure is
   planned once and built exactly when first needed.
5. **Risk list** — the three to five places most likely to fight back during
   implementation (borrow-checker seams, the macro, the concurrency lattice), each with:
   the symptom the user will see, the fallback realization, and the probe from §2 that
   de-risks it.

Calibration: the reader of the plan knows the design document well but is newer to Rust —
spell out the Rust mechanism in each chunk rather than assuming idiom fluency, and never
gate a chunk on unstated knowledge. Precision beats coverage in the blocker register: a
false blocker costs a decision cycle, a missed one costs a mid-implementation stall. When
the text genuinely resolves a question you almost raised, say nothing — the register is
for what the text does not settle. The measure of a good plan: at every point between two
chunks, the user has a compiling, fully-tested crate and can say exactly what it does.
