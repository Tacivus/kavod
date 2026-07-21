# Turn Scheduling And Derived-State Consistency

> **Status:** Settled for the Kavod MVP turn-scheduling boundary
> **Scope:** Event turns, Message propagation, Reducer visibility, correlated derived facts, multi-timeframe bar closure, and Command timing

## Conclusion

Kavod retains its existing turn semantics:

```text
one accepted Event
    -> one isolated turn
    -> Reducers before ordinary Components for each delivered payload
    -> breadth-first Message FIFO until empty
    -> collected Commands become eligible for publication after quiescence
```

These semantics are deterministic, but they do not create a general turn-wide derived-state barrier. An ordinary Component sees canonical state fully reduced for its current delivered Event or Message. It is not guaranteed to see every state change that later Messages in the same turn may cause.

When one logical transition produces several related state changes that must be coherent for a decision, the application represents them as one complete domain fact. For multi-timeframe bars, one ordinary `BarAggregator` Component owns the in-progress bars for its configured aggregation scope and emits one `BarsClosed` Message containing every bar closure caused by the current input. Reducers project the complete aggregate into `AppState`; only then do Strategies consuming `BarsClosed` run.

The MVP adds no generic phases, additional callback classes, joins, state-settled callbacks, scheduling priorities, or application-visible transition identifiers for this purpose.

## Exact Current Guarantee

Absent terminal interruption, for each delivered Event or Message:

1. All matching Reducers run in stable registration order.
2. Each Reducer completes before the next callback begins.
3. All matching ordinary Components run only after every matching Reducer completes successfully.
4. Messages emitted by Components append to the turn FIFO in production order.
5. Commands are collected in production order and do not leave during a callback.

If runtime or required-diagnostics failure terminates dispatch between callbacks, no later ordinary Component bypasses unfinished matching Reducers; the Engine stops instead.

Therefore a Component handling payload `X` observes all canonical-state transitions registered directly on `X`.

It does not necessarily observe:

- State transitions caused by Messages still waiting in the FIFO.
- State transitions caused by Messages that a later Component may conditionally produce.
- A final snapshot of every field in `AppState` for the complete turn.
- State rolled back to the turn start if a Reducer panics.

Reducer-before-Component provides atomic visibility to ordinary Components for one delivered payload. It is not a database transaction and provides no rollback. If a Reducer panics after partial mutation, the Engine stops under the settled failure policy.

## The Actual Problem

The multi-timeframe example is not fundamentally a BFS-versus-DFS problem. The problem is representing one logical market transition as several independently actionable facts.

If these are separate Messages:

```text
Bar1mClosed
Bar5mClosed
Bar15mClosed
BarDailyClosed
```

then a consumer of `Bar1mClosed` is allowed to run after only that Message's Reducers. The protocol has told the kernel that `Bar1mClosed` is independently actionable.

No queue traversal can make four independent facts atomic without an additional completeness contract.

## Worked Multi-Timeframe Example

Assume one accepted `Tick(T)` closes the 1-minute, 5-minute, 15-minute, and daily bars. Initially:

```text
AppState bars:
1m    = old
5m    = old
15m   = old
daily = old
```

### Separate Messages Under BFS

All matching Tick Reducers run, then all matching Tick Components run. Suppose the bar logic emits four separate Messages:

```text
FIFO = [Bar1mClosed, Bar5mClosed, Bar15mClosed, BarDailyClosed]
```

The resulting order is:

```text
1. Reducer(Bar1mClosed)
   state = [new, old, old, old]

2. Strategy(Bar1mClosed)
   reads partially updated multi-timeframe state
   may produce TargetPosition or Command C1

3. Reducer(Bar5mClosed)
   state = [new, new, old, old]

4. Reducer(Bar15mClosed)
   state = [new, new, new, old]

5. Reducer(BarDailyClosed)
   state = [new, new, new, new]

6. Turn reaches quiescence
   C1 still contains the payload computed at step 2
```

BFS processes sibling Messages before their descendants, but the Strategy callback in step 2 already ran. BFS therefore reduces the opportunity for deep descendants to overtake siblings; it does not settle sibling state before direct sibling consumers.

An extra `Evaluate` Message can appear to repair this particular graph:

```text
Strategy(Bar1mClosed) emits Evaluate
FIFO = [Bar5mClosed, Bar15mClosed, BarDailyClosed, Evaluate]
```

`Evaluate` sees all four projections in this exact topology. This is not a valid general guarantee. It depends on all required updates already being queued at the same causal depth and in a favorable production order. Conditional or multi-stage derivations can invalidate the assumption.

### Separate Messages Under DFS

A conceptual depth-first scheduler completes one branch before its siblings:

```text
Bar1mClosed
    -> Reducer
    -> Strategy
    -> TargetPosition
    -> Planner
    -> Risk
    -> Command

Bar5mClosed
Bar15mClosed
BarDailyClosed
```

With recursive dispatch, this branch may run before later Tick Components even produce the other bar Messages. With a deferred LIFO stack, it still completes the first Message subtree before its siblings.

DFS therefore worsens sibling consistency. A recursive DFS implementation would also reintroduce the reentrancy hazards Kavod intentionally avoids.

### One Aggregate Message

The settled design is:

```text
1. Tick(T) is delivered.

2. BarAggregator updates all relevant private in-progress bars.

3. BarAggregator emits:
   BarsClosed([Bar1m, Bar5m, Bar15m, BarDaily])

4. Every Reducer(BarsClosed) completes successfully.
   AppState bars = [new, new, new, new]

5. Strategy(BarsClosed) runs once against the updated bar projection.

6. The turn continues to quiescence before Commands become eligible
   for publication.
```

The bar-state guarantee no longer depends on BFS, DFS, an extra queue level, or callback registration luck. All required bar updates are represented by one delivered fact.

## Bar Aggregator Responsibilities

`BarAggregator` is an ordinary Component, not a Reducer and not a new callback class.

It:

- Consumes application-defined market or timer Events.
- Owns its in-progress bar builders as Component-private state.
- Updates every configured timeframe in one callback.
- Determines the complete set of bars closed by that input within its aggregation scope.
- Emits one nonempty `BarsClosed` Message when one or more bars close.
- Emits no `BarsClosed` Message when no bar closes.

Completed bars that must be shared across Components belong in canonical `AppState`. Application Reducers consuming `BarsClosed` perform that projection.

The aggregation scope must be clear from application construction and payload meaning. It may be one instrument and configured timeframe family, or another application-defined bounded grouping. `BarsClosed` guarantees completeness only within the producing Component's declared domain scope. It does not combine unrelated aggregator instances.

If one input closes multiple consecutive bars for one timeframe under the application's gap or empty-bar policy, the aggregate contains all such closures. Bars within the aggregate use a stable application-defined order. Application behavior must not depend on nondeterministic collection iteration.

Session calendars, interval boundaries, empty bars, late data, corrections, and gap filling are application-domain policies. Kavod only guarantees deterministic execution of the chosen policy.

## Strategy Responsibilities

A Strategy that requires coherent multi-timeframe bar state consumes `BarsClosed`, not an individual bar closure.

When its callback begins:

- Every matching `BarsClosed` Reducer has completed successfully.
- `AppState` reflects every bar that the registered Reducers project from that aggregate.
- The Strategy can update its private indicators for every bar in stable order.
- The Strategy can decide once for the aggregate transition.

This is a limited guarantee. Consuming `BarsClosed` does not settle unrelated `AppState` fields whose updates are represented by other Messages still in the FIFO. A Strategy must not claim whole-state coherence merely because its bar projection is coherent.

A Strategy may consume the raw Tick for tick-specific logic, but it must not make a decision requiring newly closed bars from that callback. Tick Components execute before Messages emitted by other Tick Components are reduced.

Singular `BarCompleted` facts remain legitimate when no consumer requires coherence with sibling bar closures. They are not the decision trigger for a Strategy that requires the complete multi-timeframe transition.

## Minimal MVP Semantic Rule

> If a decision depends on several related state changes caused by one input, the producer must represent the complete required set as one delivered Event or Message, and the decision callback must consume that complete fact rather than an independently actionable member fact.

For external observations that must be atomic, a Port emits one application-defined batch Event. For deterministic internal derivations, an ordinary Component emits one aggregate Message such as `BarsClosed`.

Kavod can enforce Reducer-before-Component delivery once that fact exists. It cannot inspect arbitrary application payloads or field-level `AppState` reads to prove that the aggregate is semantically complete. Aggregate completeness and correct Strategy wiring are application obligations supported by domain tests and review.

## Why Reducers Remain Output-Free

Allowing a Reducer to emit a Message does not establish turn-wide settlement.

An early Reducer could emit a payload based on state that a later Reducer for the same input subsequently changes. Reducer-produced Messages would also combine canonical mutation, causal production, and cycle creation in one capability.

Keeping Reducers output-free preserves:

- One explicit capability for canonical-state mutation.
- A clear projection-only mutation boundary.
- Ordinary Components as the only source of deterministic derived facts and external requests.
- Simpler Message-cycle and production-graph reasoning.
- The guarantee that a Reducer cannot recursively create more projection work.

The safe part of `BarsClosed` is that one Component constructs a complete domain fact and Reducers project that complete fact. Reducer emission is neither required nor useful for this design.

## Why All Possible Reducers Cannot Run First

The kernel cannot run every Reducer that might become relevant before running Components:

1. Components contain arbitrary conditional logic.
2. Executing that logic is the only way to discover which Messages actually exist.
3. Those Messages determine which additional Reducers have payloads to process.
4. Declared production edges mean "may produce," not "will produce exactly once."
5. Message cycles may make the static transitive closure unbounded.

The kernel can order callbacks for actual delivered payloads. It cannot execute a Reducer for a hypothetical payload or infer runtime branch outcomes from the graph.

## Separate Aggregators

The MVP does not introduce a generic join.

One multi-timeframe `BarAggregator` owns every timeframe that must be coherent and emits one `BarsClosed`. This avoids expected-participant tracking, missing-result semantics, correlation identifiers, and partial joins.

Separate aggregators may emit independent facts only when consumers do not require cross-aggregator coherence. If a future application requires independently implemented aggregators plus one complete decision boundary, it will need an explicit application-domain coordination protocol with a known completeness rule. Silence from a conditional producer cannot distinguish "no result" from "not completed," so current `may produce` graph declarations are insufficient. No such protocol is selected for the MVP.

## Message Ordering Decision

Kavod retains breadth-first FIFO Message propagation.

BFS provides:

- Stable production order.
- No recursive callback dispatch.
- Siblings before descendants already waiting behind them.
- Straightforward causal tracing and turn bounds.

BFS does not provide:

- Sibling projection atomicity.
- A turn-final state snapshot for each Component.
- Correctness for protocols that split one required transition into independently actionable facts.

DFS is rejected because it prioritizes descendants over siblings, worsens partial-state reactions, and encourages reentrant execution. Correct state coherence must not depend on either traversal policy.

## Command Timing

Commands remain collected in deterministic production order and become eligible for Environment publication only after the turn reaches quiescence.

This prevents callback-to-Port reentrancy and supports whole-turn publication-capacity reservation. It does not make the decision that created a Command safe:

```text
Component reads partial state
    -> constructs immutable Command payload
    -> later Reducers change relevant state
    -> turn reaches quiescence
    -> original payload remains unchanged
```

Kavod does not recompute, validate, or cancel a Command merely because later state differs. Publication may also fail under the configured Command-reservation policy, required diagnostics policy, or fatal-failure rules. Turn-end collection is not a guarantee of publication, external delivery, or external effect.

## Time And Same-Timestamp Ordering

`BarsClosed` inherits the root Event's frozen logical acceptance time, as every Message in the turn does. Each Bar's interval start, interval end, exchange time, or other market time remains ordinary domain data in the payload.

Equal domain timestamps do not create an atomic group:

- Separate accepted Events remain separate turns ordered by Event index.
- Messages are ordered by FIFO production order, not by their domain timestamps.
- Same-timestamp simulation actions use the Simulation Environment's global schedule ordinal and post-turn Command-delivery rules; timestamp equality still provides neither atomicity nor priority.
- If several external observations must be one atomic application input, the Port protocol must batch them into one Event.

No application-visible transition identifier is introduced for bar aggregation, joins, or scheduling. Existing kernel Event indices, causal ordinals, turn action sequences, and diagnostic identities remain unchanged and retain their settled non-business meaning.

## Cycles And Turn Limits

The aggregate design introduces no new cycle semantics.

- Reducers cannot emit and therefore cannot form Message-production cycles.
- Ordinary Components may still form declared conditional Message cycles.
- Existing maximum Message, callback-invocation, and Command limits remain mandatory.
- Exceeding a configured turn limit remains an ordinary terminal run error.
- Queue emptiness means causal quiescence, not that every earlier decision used final turn state.

## End-Of-Turn And State-Settled Callbacks

The MVP exposes no public end-of-turn or generic state-settled application callback for decision making.

Such a callback would hide the domain fact that caused a decision. If it could emit a Message that triggered a Reducer, state was not actually settled. If restricted to Commands, it would become a privileged effect phase with additional graph and capability semantics.

Turn-boundary invariant checks, diagnostics, and metrics remain useful as kernel or test instrumentation. Application decisions use explicit Events and Messages such as `BarsClosed`.

## Generic Phases

No generic phase model is justified or selected for the MVP. The current requirement is satisfied by explicit domain aggregation without changing callback classes or graph semantics.

This decision should be revisited only if real applications require decisions against arbitrary turn-final canonical state assembled transitively across independent Components and those requirements cannot be expressed as truthful aggregate domain facts. Until that requirement exists, phases would add graph restrictions, cycle rules, and state-mutation boundaries without solving an observed MVP problem.

## Comparable Patterns

| Pattern | Useful lesson | Kavod decision |
|---|---|---|
| [QuantConnect LEAN timeslices](https://www.quantconnect.com/docs/v2/writing-algorithms/key-concepts/time-modeling/timeslices) | A strategy can receive one explicit batch of market data selected for an engine-time frontier | Borrow explicit batching where the domain needs it; do not infer batches from timestamps |
| [Flink watermarks](https://nightlies.apache.org/flink/flink-docs-stable/docs/concepts/time/) | Distributed event-time completeness requires an explicit progress assertion and late-data policy | Do not import watermarks into a synchronous single-writer turn |
| [Timely Dataflow frontiers](https://timelydataflow.github.io/timely-dataflow/chapter_5/chapter_5_2.html) | A frontier proves no more records can arrive at a logical time; timestamp equality alone does not | Use direct domain facts instead of distributed capability tracking |
| [SimPy scheduling](https://simpy.readthedocs.io/en/latest/topical_guides/time_and_scheduling.html) | Equal-time events are still processed sequentially using priority and insertion order | Keep explicit deterministic ordering and reject timestamp-based atomicity |
| [Bevy deferred application](https://docs.rs/bevy_ecs/latest/bevy_ecs/schedule/struct.ApplyDeferred.html) | Deferred mutations become visible at explicit schedule barriers, not merely because execution is single-threaded | Add no generic barrier when one aggregate Message expresses the actual requirement |
| [NautilusTrader market-data processing](https://nautilustrader.io/docs/latest/concepts/data/#bars-and-aggregation) | Per-item cache-before-publication gives per-item visibility; domain batches are used where a protocol defines a complete unit | Preserve per-payload Reducer visibility and add domain aggregation only where required |

These systems solve different problems, including distributed progress, runner-selected batches, staged parallel execution, or domain-specific engine scheduling. Their shared lesson is narrow: deterministic order does not imply completeness, and timestamp equality does not imply atomicity.

## Settled Rules

1. One accepted Event creates one turn.
2. Reducers run before ordinary Components for every delivered Event or Message.
3. Reducer-before-Component visibility applies to the current payload, not arbitrary later derivations.
4. Messages process breadth-first through one deterministic FIFO.
5. Reducers emit no Messages or Commands.
6. Ordinary Components are the only producers of internal Messages and external Commands.
7. Commands are collected until turn quiescence and are never dispatched reentrantly.
8. Command buffering does not validate or refresh a decision payload.
9. Related derived updates required together by a decision are represented by one complete domain fact.
10. One ordinary multi-timeframe `BarAggregator` owns the in-progress bars that require coherent closure.
11. It emits one nonempty `BarsClosed` containing every closure caused by one input within its aggregation scope.
12. `BarsClosed` Reducers project the complete aggregate before matching Strategies run.
13. A Strategy requiring multi-timeframe coherence consumes `BarsClosed`, not individual bar closures or the causative Tick.
14. `BarsClosed` guarantees completed-bar projection coherence, not a settled snapshot of unrelated `AppState`.
15. Aggregate completeness is an application protocol obligation; Kavod cannot infer it from payload contents or field reads.
16. The MVP has no generic join, additional derive/reaction callback classes, application decision phase, or public state-settled callback.
17. Equal timestamps never combine separate Events or Messages into one atomic transition.
18. Existing cycle diagnostics and mandatory turn limits remain sufficient.

## Minimum Verification

The decision should be proved by tests covering at least:

1. Exact Reducer-before-Component order for Events and Messages.
2. Exact breadth-first FIFO order for siblings and descendants.
3. A negative separate-bar trace demonstrating the partial-state decision.
4. A Tick that closes 1-minute, 5-minute, 15-minute, and daily bars produces exactly one `BarsClosed`.
5. Every closure caused by that Tick appears exactly once in stable order.
6. A Tick that closes no bars produces no `BarsClosed`.
7. Domain gap policy either includes every consecutive closure it defines or rejects the input visibly.
8. Every matching `BarsClosed` Reducer completes before any matching Strategy callback.
9. The Strategy runs once for the aggregate and observes every projected completed bar.
10. The Strategy updates all relevant private indicators before producing its one decision.
11. Repeated runs with the same build, graph, state, configuration, and Event tape produce identical aggregate payloads, state, Messages, and Commands.
12. Separate accepted Events with equal domain timestamps remain separate ordered turns.
13. Commands remain unpublished during callbacks and retain their original payload after production.
14. A Reducer panic stops the Engine without running later Components or publishing the turn's Commands.
15. Application graph tests confirm that callbacks making coherent bar decisions consume `BarsClosed` rather than singular closures or the triggering Tick.

## Rejected Alternatives

- **Switch to DFS:** completes one branch before sibling projections and worsens the motivating failure.
- **Rely on an extra `Evaluate` Message:** works only for favorable queue depth and production order.
- **Allow Reducers to emit:** combines state mutation and causal production without establishing global settlement.
- **Run every possible Reducer first:** impossible without executing the conditional Components that create actual payloads.
- **Treat turn quiescence as retrospective safety:** earlier callbacks and immutable Command payloads are not recomputed.
- **Use equal timestamps as a barrier:** timestamps order neither external completeness nor internal sibling visibility.
- **Add a generic join for the MVP:** unnecessary when one Component can own all timeframes that require coherence.
- **Add an application-visible transition ID for this batch:** unnecessary within one Component-produced aggregate and distinct from existing diagnostic causality.
- **Add generic phases or new callback classes:** no current requirement justifies their graph and capability complexity.
- **Use a public turn-end callback for decisions:** obscures domain causality and creates ambiguous settlement semantics.

## Explicit Non-Guarantees

Kavod does not guarantee:

- A whole-`AppState` settled snapshot before every Component.
- That every Message caused by an Event is produced before any Component runs.
- That a Command was computed from final turn state.
- Rollback of partial Reducer mutation after panic.
- Semantic completeness of arbitrary application aggregate payloads.
- Detection of undeclared field-level state dependencies.
- Atomic grouping of separate Events or Messages with equal timestamps.
- Cross-Component aggregation completeness without an explicit domain protocol.
- Convergence of arbitrary Message cycles below configured limits.

## Dependencies And Reconciliation

This report preserves the determinism report's fixed callback and FIFO ordering. It refines the meaning of turn quiescence: quiescence separates accepted Events and delays Command publication, but does not retroactively settle state for callbacks that already ran.

It preserves the canonical-state report's single concrete `AppState`, output-free Reducers, Component-private state, and Reducer-before-Component visibility. The report's illustrative singular `BarCompleted` composition remains valid where no consumer requires sibling-bar coherence. `BarsClosed` is the required aggregate when multi-timeframe coherence matters.

It agrees with the live-runtime report that external observations requiring one atomic input must be represented by an application-defined batch Event. The kernel does not infer batches from timestamps or queue timing.

It agrees with the Port and simulation report that simulated Commands reach model endpoints only after turn quiescence. Zero-latency model Events are queued at the current virtual time and never re-enter the kernel recursively.

It preserves existing causal diagnostics. `BarsClosed` retains the root Event index; its immediate parent action is retained when detailed causal recording is enabled. No new application business identifier is required.

## Open Questions

No unresolved question blocks the MVP turn-scheduling or derived-state semantics.

The following application-domain details remain deliberately outside Kavod core:

- Bar interval and session-calendar rules.
- Tick-driven versus timer-driven bar closure.
- Empty-bar, gap, late-data, and correction policy.
- The exact stable ordering of bars inside one aggregate.
- The aggregation scope owned by one configured `BarAggregator` instance.

If a future application requires coherent outputs from separate aggregator Components, that requirement must trigger a separate domain-coordination design. It must not silently weaken the settled aggregate-completeness rule or rely on current `may produce` declarations as proof of runtime completion.
