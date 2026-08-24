# Adversarial Review: OpenCode gpt-5.6-sol, 2026-08-24
**Target:** design_docs/design-v12.md — `> **Status:** Authoritative (v12). One section is open: Wiring & construction.`
**Verdict:** Unsound as a standalone implementable specification. The exact Run API is incomplete, exhaustive construction omits unavoidable behavior, and several guarantees are assigned enforcement mechanisms that cannot enforce them; the forensic and commitment-point claims also overstate what their mechanisms prove.

## Findings
### SOL-01 The exact API never declares `Engine` — MAJOR, confidence high, omission
- **Text attacked:** Section 0: “API blocks — item names, type shapes, trait bounds, and variant sets are exact”; Run API: `impl<A, E, W> Engine<A, E, W>`.
- **Claim:** No binding form declares `Engine`, so its item kind and type shape are underivable. Read literally, the exact API cannot compile.
- **Witness:** Reasoned, not executed: resolving the inherent `impl<A, E, W> Engine<A, E, W>` finds no struct, enum, union, or type alias named `Engine` anywhere in the document and therefore fails name resolution.
- **Fix sketch:** Add the exact public `Engine<A, E, W>` declaration with private storage.

### SOL-02 The exhaustive construction table omits unavoidable destruction — MAJOR, confidence medium, omission
- **Text attacked:** Section 0: “each table is exhaustive over its scope — work it does not list does not happen.” Run API: `pub fn new(config: EngineConfig, app: A, env: E, writer: W) -> Result<Self, BuildError>;`. The `Engine::new` table lists only “Reserve the complete Command batch” and “Build the Journal.” `TRUST-BLOCKING` expressly includes “destructors.”
- **Claim:** On either listed failure, Rust must dispose of or deliberately leak the consumed inputs and partial allocations, but neither action appears in the exhaustive table.
- **Witness:** Reasoned, not executed: choose `Command = u8`, `max_commands_per_turn = usize::MAX`, and a `W: Write` whose bounded `Drop` changes a shared counter from `0` to `1`. Step 1 returns a capacity `TryReserveError`; returning `BuildError` drops `W` and changes the counter, while `mem::forget(W)` instead leaks it. Both are unlisted observable work.
- **Fix sketch:** State the ownership and destruction behavior on each construction exit, or explicitly scope the table to fallible protocol operations rather than all work.

### SOL-03 Handoff completeness is not unrepresentable — MAJOR, confidence high, unenforceable claim
- **Text attacked:** `RUN-GRAMMAR`: “a `CommandsDispatched` without every handoff ... is unrepresentable.” `RUN-ENFORCEMENT`: “Certificate, phase, and transition types are module-private; every other illegal state listed by `RUN-GRAMMAR` is unrepresentable within that boundary.”
- **Claim:** Module privacy prevents callers from invoking record transitions directly, but Rust's affine ownership cannot prove that the private transition actually called `dispatch` for every Command.
- **Witness:** Reasoned, not executed: with batch `[7u8]`, a type-correct private `dispatch_batch` can commit `CommandsPrepared`, drain and drop `7` without calling `env.dispatch`, commit `CommandsDispatched`, and return `EffectsComplete`. The Environment observes zero handoffs, while all typestate and record types compile and external compile-fail tests still reject direct transition calls.
- **Fix sketch:** Classify handoff completeness as suite-tested and require an instrumented per-position dispatch trace rather than claiming compiler enforcement.

### SOL-04 Lossy serialization defeats “complete intent” evidence — MAJOR, confidence high, false claim
- **Text attacked:** Records table, `CommandsPrepared`: “The turn's complete Command intent.” Run prose: “`CommandsPrepared` plus the typed `Dispatch { position }` identify the exact handed-off prefix.” Journal: “Lossy serialization is evidence only of the fields it emits.” `TRUST-SERIALIZE` requires deterministic, side-effect-free, bounded, nonpanicking serialization, but not faithful serialization.
- **Claim:** A permitted lossy serializer can erase the values that distinguish Commands, so the record proves only cardinality and serialized representations, not complete intent or the exact semantic prefix.
- **Witness:** Reasoned, not executed: batch A is `[{account: 1, qty: 10}, {account: 2, qty: 20}]`; batch B is `[{account: 9, qty: 900}, {account: 8, qty: 800}]`. A deterministic `Serialize` implementation emits `{}` for every Command, producing `"commands":[{},{}]` in both runs; `Dispatch { position: 1 }` then identifies position `0` but cannot identify which actual Command was handed off.
- **Fix sketch:** Require evidence-faithful serialization for Commands or weaken the claims to the ordered serialized representations and their positional prefix.

### SOL-05 `shutdown` has no commitment point matching its definition — MAJOR, confidence high, false claim
- **Text attacked:** Glossary: “Commitment point — the instant an operation's outcome becomes fixed.” A3: “Every effectful operation commits at exactly one point, where its outcome becomes fixed.” Commitment table, `shutdown`: “The call itself.” `LIVE-SHUTDOWN`: “At expiry one final synchronized observation decides the race.”
- **Claim:** “The call itself” is an interval, not an instant, and the Live report's outcome is not fixed when the call begins.
- **Witness:** At call entry, one completion entry is `Outstanding` and the deadline is 10 ms. If completion orders at 9 ms, the final observation returns `Quiesced`; if it orders at 11 ms, it returns `Incomplete`, proving the outcome remained unfixed after invocation.
- **Fix sketch:** Make the final completion observation the operation commitment and describe the earlier close and signal as standing subordinate effects.

### SOL-06 Sim selection rules have no stated enforcement tier — MAJOR, confidence high, unenforceable claim
- **Text attacked:** Section 0: “Every ID outside the Obligations table is enforced.” `SIM-SELECT`: “the cursor starts at Slot 0, persists across `next_event` calls, and moves to the selected Slot's successor after every selected `step`.” The mechanism names only one assertion: “the selected Port is `Open`.” `VERIFY-SIM` says it verifies `SIM-LIFECYCLE`, `SIM-START`, and `SIM-SHUTDOWN`.
- **Claim:** Persistent round-robin selection is neither unrepresentable, asserted, nor included in a named suite.
- **Witness:** Slots 0 and 1 are both `Open` and armed at time `100`; the cursor begins at 0. Slot 0 is selected, re-arms at `100`, and returns `Some(E0)`, so the required next selection is Slot 1; an implementation resetting the cursor at each `next_event` selects Slot 0 again, passes the `Open` assertion, and can pass every enumerated `VERIFY-SIM` lifecycle case while producing a different Event trace.
- **Fix sketch:** Add a named Sim scheduling suite covering check priority, persistent cursor behavior, equal-time ties, wakeup mutation, and budget boundaries.

### SOL-07 The blocked-latch wake guarantee is not pinned — MAJOR, confidence medium, unenforceable claim
- **Text attacked:** `ENV-LATCH`: “A `next_event` call waiting for input returns once the latch is pending.” `LIVE-SUPERVISION`: publication must “wake a blocked `next_event`.” `VERIFY-LATCH` enumerates ordering, first-Error reporting, final-Command observation, close reporting, and clean `Stopped`, but no blocked-wait wake case.
- **Claim:** Waking a blocked waiter is not a type property or asserted invariant, and no named suite expressly requires that liveness behavior.
- **Witness:** At `t=0`, the Event queue and latch are empty and `next_event` blocks only on the Event queue; at `t=1`, a Port publishes Error `E`, setting the latch to pending but omitting notification. Every latch-state rule remains representable, yet `next_event` never returns as guaranteed.
- **Fix sketch:** Add a conformance case that publishes while `next_event` is blocked with no Event available and requires prompt return of the latched Error.

### SOL-08 Sim shutdown does not unambiguously provide the immediate common signal — MINOR, confidence medium, ambiguity
- **Text attacked:** `ENV-SHUTDOWN`: “From that instant every Port has a means to observe the signal immediately.” `SIM-SHUTDOWN`: “the `stop` call is the sim shutdown signal” and calls Ports “in frozen Slot order.” `TRUST-SPAWN` expressly covers Sim Port-started “threads, callbacks, timers.”
- **Claim:** “Immediately” can mean either at the common close instant or only at each Port's next invocation; only the latter makes sequential `stop` conform.
- **Witness:** Ports A then B are `Open`; the latch closes at `t=0`, and bounded `A.stop` runs until `t=5`. B owns a deterministic worker that it signals and joins inside `B.stop`, satisfying `TRUST-SPAWN`, but B has no retained `SimCtx` or lifecycle capability through which that worker can observe shutdown before `t=5`.
- **Fix sketch:** Define “immediately” as “before the Port's next execution opportunity,” or give every Sim Port a shared signal capability raised at the close.

### SOL-09 The fault-suite cross-product includes an impossible cell — MINOR, confidence high, ambiguity
- **Text attacked:** `VERIFY-FAULTS`: scripted Environments cover “each operation's `Err`” and a shutdown report carrying `Some(error)`; “this includes their cross-product.” `ENV-SERIAL`: “After `start` returns `Err` there is no later call.” `RUN-FINALIZE` skips `shutdown` after a start Error.
- **Claim:** The literal cross-product includes `start Err × shutdown Some`, while the lifecycle rules forbid observing both; the likely exclusion of start failures is unstated.
- **Witness:** `start` returns `Err(E1)`. The Run must immediately produce `Environment(Start)` with `Quiesced` and may not call `shutdown`, so no conforming trace can also contain `ShutdownReport { error: Some(E2), ... }`.
- **Fix sketch:** Restrict the cross-product explicitly to post-start failures for which finalization calls `shutdown`.

### SOL-10 Citation and placement rules are violated — NIT, confidence high, self-conformance violation
- **Text attacked:** Section 0: “Citations point backward,” “Cite IDs,” and “Core sections build only on the contracts and never name an implementation.”
- **Claim:** Multiple non-exempt references violate the document's declared dependency and placement grammar.
- **Witness:** `PORT-SUMS` says it “rides `PORT-ROUTING`'s trusted obligation” before `PORT-ROUTING` is defined; `RUN-GRAMMAR` cites the following `RUN-ENFORCEMENT` row; `SIM-COMPLETION` cites “(Ports Notes)” rather than an ID; and the Run says, “Both shipped Environments check the latch before `next_event` selection and `dispatch` handoff.”
- **Fix sketch:** Reorder dependent rows, replace location references with IDs, and move implementation-specific behavior out of the Run.

## Attacked and held
- Environment latch ordering held for before-call, overlapping, after-return, operation failure before commitment, reporting, and close interleavings.
- Run failure precedence held across handler Fatal, overflow, partial dispatch, record failure, time regression, checkpoint Error, and finalizing shutdown.
- Stop-path report precedence held: report Error outranks `Incomplete`, and retained quiescence survives failure of `TurnCompleted(Stop)`.
- Index arithmetic held at `0`, `u64::MAX`, and the pre-`next_event` exhaustion boundary.
- Journal bounds and sink failures held by reasoning: `usize::MAX + 1`, short writes, `Ok(0)`, over-reporting, `Interrupted`, flush failure, poison, and uncertain suffixes.
- Live completion races held for pre-close completion, deadline expiry, final synchronized observation, join-after-completion, and detachment on `Incomplete`.
- Sim lifecycle transitions held for every method result; stale arms after an Error remain unreachable under `ENV-SERIAL`.
- Conditional determinism held once the complete initial State, exact trace values, serialization obligations, and sink obligation are fixed.
- Acceptance, dispatch-completion, Stop, and clean-report evidence held apart from the lossy Command-identity defect above.

## Coverage
- Section 0: walked.
- Section 1: walked.
- Section 2: walked.
- Section 3: walked.
- Section 4: walked.
- Section 5: walked.
- Section 6: walked.
- Section 7: walked.
- Section 8: walked.
- Section 9: walked.
- Section 10: walked; its declared openness was not treated as a finding.
- Section 11: walked.
- Section 12: walked.
- Appendix A: walked.

## Questions the document cannot answer
- Does `TRUST-PURE` constrain the behavior of `initial_state`, or only the State value it returns?
- What equality relation defines “equal traces” for Event, Command, and State types that have no `Eq` or `PartialEq` bound?
- Under `DET-ENV`, which configuration is held equal when Live and Sim have different configuration types and fields?
