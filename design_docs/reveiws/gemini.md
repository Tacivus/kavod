# Adversarial Review: opencode (gemini-3.7-flash), August 24, 2026

**Target:** `design_docs/design-v12.md` — `> **Status:** Authoritative (v12). One section is open: Wiring & construction.`

**Verdict:** Sound with minor fixes. The Core architecture, typestate graph transitions, commitment mechanics, dual-environment contracts (Live/Simulated), and failure-propagation laws are robust, rigorously bounded, and mathematically coherent under adversarial analysis. The only identified findings are minor self-conformance and API qualification nits that do not compromise run-level soundness or determinism.

## Findings

### REV-01 Unqualified `TryReserveError` in `BuildError` API block — NIT, confidence high, self-conformance violation
- **Text attacked:** Section 7 API, line 588:
  ```rust
  pub enum BuildError {
      CommandBuffer(TryReserveError),
      Journal(JournalBuildError),
  }
  ```
  contrasted with Section 6 API, line 501:
  ```rust
  pub enum JournalBuildError {
      MaxBytesTooLarge,
      AllocationFailed(std::collections::TryReserveError),
  }
  ```
- **Claim:** `BuildError::CommandBuffer` references unqualified `TryReserveError`, which is not part of Rust's standard prelude (`std::prelude::v1`). Under Section 0 reading rules (lines 16–17), API blocks bind item names and type shapes exactly.
- **Witness:** Compiling the Section 7 API block in isolation without wildcard imports yields `error[E0412]: cannot find type 'TryReserveError' in this scope`, whereas Section 6 correctly qualifies `std::collections::TryReserveError`.
- **Fix sketch:** Qualify the variant payload as `CommandBuffer(std::collections::TryReserveError)`.

### REV-02 Section 7 Notes refer to shipped implementations in violation of Core placement rule — NIT, confidence high, self-conformance violation
- **Text attacked:** Section 0 Placement rules, lines 52–55:
  > "Core sections build only on the contracts and never name an implementation — earlier mentions of the two shipped Environments are navigation only (the Scope line, the contract's pointer to its implementations, the bounds registry)."
  and Section 7 Notes, line 875:
  > "Both shipped Environments check the latch before `next_event` selection and `dispatch` handoff, so an Error pending when either call begins returns first."
- **Claim:** Section 7 is a Core section whose derivation prose refers to "Both shipped Environments" instead of bounding its derivation strictly to the Environment contract. This breaches Section 0's placement mandate for Core sections.
- **Witness:** Compare Section 0 line 52 ("Core sections build only on the contracts and never name an implementation") with Section 7 line 875 ("Both shipped Environments check the latch..."), which is located in the non-exempt Core Run section.
- **Fix sketch:** Reframe line 875 around conforming Environment contract behavior under `ENV-LATCH` rather than referencing shipped implementations.

## Attacked and held
- State machine and graph completeness: typestate affine transitions and `RUN-ENFORCEMENT` guarantees across all phases and turn-0 `Stop`.
- First-failure wins (A4) and finalization cascade: `RUN-FINALIZE` strictly fixing the initial cause and discarding subsequent cleanup errors.
- `ENV-LATCH` publication vs. observation ordering: logical ordering rules and single-error latch transitions across concurrent port errors overlapping `next_event`, `dispatch`, `take_error`, and `shutdown`.
- Live Environment shutdown deadline and quiescence accounting: capability ownership in `LIVE-COMPLETION` and atomic state flipping in `LIVE-SHUTDOWN`.
- Simulated Environment step-budget and round-robin cursor: `SIM-SELECT`, `SIM-STEPS`, and `SIM-LIFECYCLE` enforcing step bounds, cursor advancement, and unreachable stale arms.
- Index and timestamp arithmetic overflow: checked arithmetic in `Timestamp::checked_add`, `Journal::new`, `RUN-INDEX`, and `LIVE-TIME`.
- Journal encoding validation and poison isolation: `JRN-ENCODE` and `JRN-POISON` guaranteeing object validation before sink dispatch and permanent poison without spin loops.
- Cross-environment and intra-environment determinism (`DET-RUN`, `DET-ENV`): exact state transition, byte, and exit reproducibility from equal traces across environments.

## Coverage
- Section 0 (Reading this document): walked
- Section 1 (Glossary): walked
- Section 2 (Laws): walked
- Section 3 (Application contract): walked
- Section 4 (Port contract): walked
- Section 5 (Environment contract): walked
- Section 6 (Journal): walked
- Section 7 (The Run): walked
- Section 8 (Live Environment): walked
- Section 9 (Simulated Environment): walked
- Section 10 (Wiring & construction — OPEN): walked
- Section 11 (Crate layout): walked
- Section 12 (Obligations & verification): walked
- Appendix A (Invariant index): walked

## Questions the document cannot answer
None. All closed sections (Sections 0–9, 11–12) are fully derivable from the text alone. All open questions reside exclusively in Section 10 (Wiring & construction), which is explicitly declared open.
