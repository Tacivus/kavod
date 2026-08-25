# Batch 0 Decisions

## D1: Signal before final latch close

**Decision:** `shutdown` raises the shutdown signal, performs its bounded graceful-shutdown work while the latch remains open, and closes the latch as its final Error observation.

A typed Error published during the graceful-shutdown period and before that close makes a previously non-Fatal Stop path Fatal. Errors published after the close are discarded. If the Run already has a Fatal cause, A4 remains decisive: the original cause wins and shutdown Errors are discarded.

The resulting Stop-path outcomes are:

- Graceful shutdown completes without an Error: the report is clean and the Run may commit `TurnCompleted(Stop)` and return `Stopped`.
- A typed Error is published before the final close: the report carries it and the Run returns `Environment(Shutdown)` Fatal.
- The graceful-shutdown deadline expires without an Error: the Run returns `Core(ShutdownIncomplete)` Fatal.
- An Error is published and the deadline expires: the Error remains the Fatal cause, as specified by `StopPending`.
- A panic still aborts under A8; it is not represented as a typed Fatal.
- Errors arising after the final close, including after timeout and detachment, are discarded.

Merely raising the signal and immediately closing the latch is insufficient: in the Live Environment it would create a race in which only sufficiently fast Port Errors are captured. The latch therefore remains open through the bounded graceful-shutdown period and closes at its final completion or deadline observation.

**Required follow-up:** This decision changes the shipped Environment behavior assumed by SYN-01 and requires edits beyond Batch 1's stated §1/§5 scope:

- `LIVE-SHUTDOWN` must signal first, wait for completion or deadline with the latch open, and close the latch at the final observation.
- `LIVE-SUPERVISION` must publish a typed `run(Err)` reported during the graceful-shutdown period rather than discard it merely because shutdown has begun.
- `SIM-SHUTDOWN` must invoke `stop` while the latch is open, publish the first returned Error, and close the latch after the shutdown calls.
- Verification must cover successful shutdown, a shutdown Error, timeout without an Error, Error plus timeout, and publication after the final close.

## D2: Pending Error takes precedence

**Decision:** When a pending latched Error is ordered before an operation's commitment point, the operation returns and reports that Error in preference to any pre-commitment Error of its own. A `next_event` call woken because the latch became pending returns that Error.

The precedence follows the existing logical ordering rules:

- A publication completed before the call began is ordered first, so the pending Error wins.
- For a publication overlapping the call, the Environment may order it on either side of the operation's commitment point. The pending Error wins when ordered before it; the operation's own Error wins when fixed first.
- A publication begun after the operation returned remains pending for a later observation.

Returning the pending Error marks the latch reported forever. The operation's own contractual effect still has not occurred: `dispatch` hands off no Command, and `next_event` consumes no candidate. The operation's own Error is secondary and discarded.

This preserves the latch's purpose and A4's first-failure policy. If an operation's own Error won while an earlier Error remained pending, that earlier Error would reach finalizing `shutdown` only after the Fatal cause was fixed and `RUN-FINALIZE` would discard it.

**Required follow-up:** Batch 1 must place this precedence clause in `ENV-LATCH`. Verification must cover an already-pending Error against an operation's own pre-commitment failure, both permitted orderings for an overlapping publication, and a blocked `next_event` woken by publication.

## D3: Strongest available enforcement boundary

**Decision:** Preserve §0's universal claim that every ID outside the Obligations table is enforced. Do not scope or weaken its quantifier. Apply the enforcement order strictly: make a violation unrepresentable where ownership or types can carry the rule, otherwise detect it with an always-on assertion, otherwise pin every observable behavior with a named required test suite. Use a trusted obligation only where no execution trace can witness the rule.

The approved per-row disposition is:

| Rule or clause | Enforcement disposition |
|---|---|
| `SIM-SELECT` cursor and round-robin behavior; `SIM-STEPS` fenceposts; `SIM-WAKEUP` last-call-wins; `SIM-COMPLETION` | Extend `VERIFY-SIM` from lifecycle-only coverage to scheduling and bounds coverage: frozen order, persistent cursor, equal-time ties, wakeup replacement and clearing, exact budget boundaries, no mutation on exhaustion, and no-armed-Port completion. |
| `LIVE-SELECT` stamp/dequeue ordering and post-dequeue infallibility; `LIVE-EVENTS` `Full`/`Closed` returns; `LIVE-DISPATCH` admission identity | Extend `VERIFY-LIVE` with select, offer, dispatch, ownership-return, and capacity-boundary cases. |
| `APP-EMIT`, `APP-OVERFLOW`, `APP-STATE` | Add the required `VERIFY-CONTEXT` suite: append in call order, first-overflow marker behavior, later-emission rejection, fresh-handler reset, and State mutations standing on every Fatal path. |
| `DET-RUN` repeatability | Extend `VERIFY-CONFORMANCE`: run every scripted trace twice within each Environment type and compare all values in `DET-RUN`'s list. Cite that suite from `DET-RUN`; retain the explicit `TRUST-PURE` and `TRUST-SERIALIZE` preconditions. |
| `Core(TimeRegression)`, `Core(CommandBoundExceeded)`, `Core(ShutdownIncomplete)` | Extend `VERIFY-FAULTS` with a decreasing successful timestamp, an over-emitting Application, and an `{ Incomplete, None }` shutdown report. Restrict the operation-Error/report-Error cross-product to post-`start` failures and separately prove that `start Err` performs no shutdown. |
| `Core(IndexExhausted)` | Keep the structural `RUN-INDEX` enforcement: check the index domain before `next_event`, make overflow past the check an always-on invariant panic, and do not add a test-only certificate-forging path that weakens `RUN-GRAMMAR`. |
| `ENV-BOUNDS` | For shipped Environments, require the extended `VERIFY-LIVE` and `VERIFY-SIM` rejection, exhaustion, and no-growth cases. For bespoke Environments, cite `TRUST-ENV`; its verification must name `VERIFY-CONFORMANCE`, `VERIFY-LATCH`, and review for bounds and other properties no trace can witness. |
| `PORT-STATE` payload noninspection | Extend `TRUST-ROUTING` to prohibit reading routed payloads, upheld by the wiring author and verified by review. Bespoke Environment routing also remains covered by `TRUST-ENV`. This is trusted because a payload read with no externally visible effect cannot be detected by a behavioral suite. |
| `ASSERT-INVARIANTS` | Move to §0 as the definition of the asserted tier: only always-on, constant-time assertions count as enforcement. Every invariant still requires an owning guarantee and assertion site. |
| `BOUND-LOOPS` | Move to §0 as the definition and registry of bounded active-loop enforcement. Each concrete loop remains enforced by its owner and bound; blocking waits remain governed by `TRUST-BLOCKING`. |
| `NO-UNWIND` | Do not erase this implementation property by calling it definitional. Extend `TRUST-ABORT` to require both that shipped code relies on unwinding nowhere and that the final binary uses `panic = "abort"`; uphold it jointly by the Kavod implementer and build configuration, verified by code review and a CI build-profile check. |
| Answer passed to `classify` | Cite `VERIFY-JOURNAL`; its required record sequences and outcomes pin the answer passed at the single runtime call site. |
| Blocked `next_event` wake | Extend `VERIFY-LATCH` with an explicit publish-while-blocked case. Per D2, a call woken by the latch must return and report the pending Error. |
| `VERIFY-GRAMMAR` visibility | Require a fixture mechanism that attacks from the Engine's module visibility position, such as an `include!`-based fixture crate, so compile failures test grammar restrictions rather than merely failing on privacy. |
| `TRUST-BLOCKING` dependencies | Cite `TRUST-BLOCKING` at `RUN-FINALIZE`, `SIM-START`, and `SIM-SHUTDOWN`, and state the nontermination consequence for Sim as well as Live. |

No new `TRUST-*` row may be added merely to avoid writing an observable behavior test. Every trusted row must name the exact obligation, upholder, and verification means, and §12 must remain the complete trusted boundary.

**D1/D2 amendments:** D1 supersedes SYN-19's old atomic signal-and-close test. `VERIFY-LIVE`, `VERIFY-SIM`, and `VERIFY-LATCH` must instead cover the latch remaining open during graceful shutdown, typed shutdown Errors before the final close, a publication racing that final close, timeout, and post-close discard. D2's publish-while-blocked case is the single required `VERIFY-LATCH` case above, not a duplicate suite obligation.

## D5: Neutral construction-frozen Slot order

**Decision:** Reword `BOUND-STATIC` neutrally now. Preserve the closed invariant that construction fixes one nonempty Port set and one Slot order, and that neither changes during the Environment's lifetime. Do not make `BOUND-STATIC` choose whether registration order or Slot-sum declaration order is the authority; that source remains an open Wiring decision.

Batch 4 should use this guarantee:

> Construction fixes the nonempty Port set and one Slot order; both remain unchanged for the Environment's lifetime.

This keeps the frozen order available to `LIVE-COMPLETION`, Live spawn and join order, Sim lifecycle order, and `SIM-SELECT` tie-breaking without prejudging the Wiring API.

## D6: Mechanism is a nonbinding prose job

**Decision:** Add Mechanism to §0 as a fourth, explicitly nonbinding prose job. Mechanism illustrates one workable realization of the binding rules, creates no implementer obligation, and is never authority over an API block, guarantee row, binding table, or obligation row. A mechanism may be replaced wherever all binding rules continue to hold.

The deletion test remains decisive: if removing Mechanism prose changes conformance, an API shape, or observable behavior, the load-bearing fact must move into one of §0's four binding forms.

Batch 3 should add this policy to §0:

> Mechanism illustrates one replaceable realization of the binding rules. It creates no obligation and is never authority over an API block, guarantee row, binding table, or obligation row.

SYN-40's three load-bearing facts must be promoted regardless of that declaration:

- Promote the Journal encode-region size and its checked `max_record_bytes + 1` construction into `JRN-ENCODE`, so classification between `NotAnObject` and `BoundExceeded` is binding.
- Declare `Never: Serialize` in the §4 API block; the uninhabited `match *self {}` implementation body may remain Mechanism.
- Promote the public re-export and no-repeated-path rule from §11 Mechanism prose into a guarantee row, and add its ID to Appendix A.

## D7: Placement rules bind current and future text

**Decision:** §0's placement rules bind the current document and every future edit. Current text must satisfy the ownership, dependency-order, citation, and implementation-separation rules; they are not merely prospective editorial guidance.

Batch 3 should introduce the rules with:

> Placement rules, for this document and every future edit:

The known current-text violations remain assigned to their existing batches:

- Reword §7's shipped-Environment mechanism claim at contract level (`SYN-39`).
- Replace the prose-location citation `(Ports Notes)` with an ID citation (`SYN-45`).
- Replace §3's forward reference to `Engine::new` with neutral earlier-section wording (`SYN-46`).
- Add the missing contract citations to `SIM-STATE` (`SYN-48`).
- Move the implementation-specific Sim Port lifecycle definition from the Glossary into §9 (`SYN-50`).

Every batch must leave its edited text conforming to these rules; placement violations cannot be accepted as historical debt.
