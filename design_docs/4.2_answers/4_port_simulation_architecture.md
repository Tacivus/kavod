# Port And Simulation Architecture

> **Status:** Settled for the Kavod MVP Port and deterministic-simulation boundary
> **Scope:** Logical Ports, live and simulated implementation contracts, shared simulated-world ownership, grouped bindings, virtual scheduling, zero latency, source exhaustion, and simulation completion

## Conclusion

Kavod keeps `PortSpec` as the shared application-facing abstraction. A Port is a logical typed protocol endpoint, not a thread, worker, process, object, model, or independent state owner.

Live and simulation bind the same logical Ports differently:

```text
Application graph:

MarketDataProvider -- MarketEvent --> Components
ExecutionVenue    <-- Command ----- Components
ExecutionVenue    -- ExecutionEvent -> Components

Live implementation units:

MarketDataProvider -> Databento worker
ExecutionVenue     -> IBKR worker

Simulation implementation units:

MarketDataProvider --\
                      > SimulatedMarketVenue model
ExecutionVenue     --/
```

The two simulated bindings remain distinct Port endpoints with distinct identities, Commands, Events, routing, and source attribution. They share one model because market-data publication and order execution depend on one coherent simulated venue state.

The binding invariant is therefore:

> Every logical Port has exactly one endpoint binding. Every simulated endpoint belongs to exactly one simulated model. One simulated model may provide one or several endpoint bindings.

The application graph remains unchanged. Grouping exists only in Environment composition and diagnostics.

## Application And Environment Boundary

Kavod has two different graphs of concern.

The application graph contains:

- Components and Reducers.
- Logical Port endpoints.
- Event edges from Ports.
- Message edges inside the application.
- Command edges to Ports.

The Environment implementation topology contains:

- Live workers and their placement, or simulated models and their endpoints.
- Live queues, supervision, and admission, or virtual time and scheduled actions.
- Implementation state external to deterministic application state.

The application graph is shared across environments. The implementation topology is not.

This separation does not promise physical isolation among Port endpoints. A Port boundary establishes application-facing protocol and addressing. It does not establish one process, one thread, one object, or one state owner per Port.

## Shared Port Contract

Live and simulation share:

- Logical Port identity and destination.
- Associated Command and Event types.
- Application-defined Command and Event meaning.
- Command addressing to one logical Port endpoint.
- Per-Port Command production and submission order.
- Event source attribution to the emitting logical Port endpoint.
- The distinction between a requested effect and an observed outcome.
- Application-defined operational and lifecycle facts when application logic must react.
- No silent drop, retry, duplication, coalescing, or suppression by Kavod.
- One accepted Event creating one isolated kernel turn.
- Commands becoming eligible for Environment publication only after turn quiescence.
- Application graph construction and validation.

These shared semantics are intentionally narrower than one shared implementation API.

## Intentional Differences

Live and simulation may differ in:

- Implementation traits and callback shapes.
- Thread, task, process, polling, or blocking mechanics.
- Real versus virtual time.
- Concurrent ingress versus deterministic scheduled actions.
- Queue and mailbox machinery.
- Backpressure and capacity mechanisms.
- External transport and connection state.
- Historical readers and source cursors.
- Simulated books, fills, and latency policies.
- Startup and shutdown implementation.
- Fault injection and deterministic-choice facilities.

Live Port implementations need not be deterministic. Their accepted Events freeze the external behavior the kernel observed.

Simulated models must be deterministic for the simulation run to be reproducible. This is an additional Environment contract, not an expansion of the narrow deterministic-kernel guarantee.

## Logical Ports And Implementation Units

`PortSpec` remains conceptually:

```rust
pub trait PortSpec: 'static {
    type Command;
    type Event;
}
```

It says nothing about implementation cardinality or placement.

An implementation unit is the Environment-owned object that executes external behavior:

- One dedicated live worker in the MVP live Environment.
- One independent simulated model.
- One shared simulated model exposing several endpoints.

For MVP live bindings, one logical Port maps to one dedicated worker. This is a live placement restriction, not a permanent property of `PortSpec`. A future live Environment may allow one adapter worker or process to expose several logical Ports without changing the application graph.

## Live Implementation Contract

A live Port implementer must understand these public semantics even if the exact Rust trait remains deferred:

- The Environment supplies one bounded FIFO Command ingress for the binding.
- The Port observes Commands in per-Port submission order.
- The Port may emit only Events associated with its bound logical Port.
- Event emission offers work to the live Environment; it never invokes application callbacks directly.
- The Port cannot borrow kernel state, `AppState`, Component state, the Acceptor, or kernel channels.
- The Environment owns worker placement, queues, startup supervision, cooperative stop, and joining.
- Technical worker startup does not imply connectivity, authentication, reconciliation, or permission to trade.
- Expected operational outcomes are application-defined Events when application reaction is required.
- Unexpected worker exit, panic, mailbox failure, and prohibited overflow are visible technical failures.
- Kavod performs no hidden retry or restart.
- Command receipt by the worker does not prove remote receipt, execution, or completion.

The settled live admission, fairness, capacity, publication, and fatal-latch rules remain owned by `5_runtime_backpressure_safety.md`.

## Simulated Model Contract

A simulated model is a domain-defined, Environment-held synchronous deterministic state machine.

It must:

- Produce the same state transitions and staged outputs for the same state, callback input, time, configuration, and approved deterministic choices.
- Run one callback at a time on the simulation thread.
- Use only virtual time supplied by the Environment.
- Consume Commands through registered endpoint callbacks.
- Emit Events only through registered logical Port endpoints.
- Schedule and cancel only through a restricted model context.
- Avoid wall time, OS IO, OS entropy, task scheduling, process-global mutable state, and behaviorally observed unstable iteration.
- Never borrow or mutate deterministic application state.
- Never invoke the kernel, another model, or another Port directly.
- Never retain a simulation context after callback return.

Model state is external-world state, not `AppState`. Components cannot inspect it. Relevant observations enter the application only as Events.

## Independent Simulated Models

An independent model owns all state needed to implement its endpoint and has no causal state dependency on another Port implementation.

A Timer is the canonical example:

```text
Timer Port endpoint
    -> SimulatedTimer model
        -> timer registrations
        -> scheduled wake tokens
```

Ordinary simulation binding may use conceptual sugar such as:

```rust
simulation.bind::<Timer>(SimulatedTimer::new());
```

This is one model providing one endpoint.

## Shared Simulated Models

A shared simulated model is one ownership and serialization domain providing several logical Port endpoints.

A simulated venue commonly owns:

- Historical source readers and cursors.
- The venue market book.
- Resting simulated orders.
- Queue-position or liquidity-consumption state.
- Fill and acknowledgement state.
- Feed and execution latency policy.
- Deterministic model configuration.

It may expose:

- A `MarketDataProvider` endpoint.
- An `ExecutionVenue` endpoint.
- Additional application-defined venue endpoints where justified.

The endpoints do not communicate with each other. Commands and model actions are dispatched to their common owning model. The model mutates its own state directly under one run-to-completion authority.

Grouped ownership must not be implemented as separately registered Port objects sharing `Arc<Mutex<World>>`, `Rc<RefCell<World>>`, process-global state, or an undocumented side channel. Such an implementation hides ownership and prevents Kavod from enforcing one lifecycle and one callback authority.

## Grouped Binding

Conceptually, simulation composition may look like:

```rust
let environment = SimulationEnvironment::builder()
    .bind_model(SimulatedMarketVenue::new(history, config), |model| {
        model.endpoint::<MarketDataProvider>();
        model.endpoint::<ExecutionVenue>();
    })
    .bind::<Timer>(SimulatedTimer::new())
    .build(&application)?;
```

The exact API is deferred. Its semantic requirements are settled:

- The Environment stores the model exactly once.
- The model receives one model identity and one lifecycle.
- Each endpoint receives its own logical Port instance identity.
- Every declared Port still resolves to exactly one endpoint binding.
- Commands route by logical Port identity and then dispatch to the owning model endpoint.
- Emitted Events are attributed to the endpoint used to emit them.
- Model grouping is visible in Environment diagnostics but adds no application graph node or edge.
- Build validation rejects duplicate endpoints, missing endpoints, and endpoints for undeclared Ports.

## Ownership

| Concern | Logical owner | Physical holder | Mutation authority |
|---|---|---|---|
| Canonical `AppState` | Application | Engine | Reducers |
| Component-private state | Component | Engine | Owning Component callbacks |
| Live adapter state | Live implementation | Live worker | Live implementation |
| Historical reader and cursor | Simulated model | Simulation Environment | Owning model callbacks |
| Simulated book and orders | Simulated model | Simulation Environment | Owning model callbacks |
| Deterministic latency policy | Simulated model | Simulation Environment | Owning model callbacks |
| Virtual clock | Simulation Environment | Simulation Environment | Scheduler |
| Global future-action queue | Simulation Environment | Simulation Environment | Scheduler and staged model operations |
| Schedule ordinal | Simulation Environment | Simulation Environment | Scheduler |

The Environment may physically store domain-defined models without understanding their state. It knows model identities, endpoint registrations, opaque wake payloads, scheduled times, cancellations, and typed Port boundary envelopes. It does not know about books, orders, fills, instruments, or venues.

## Simulated Lifecycle

### Construction And Start

Simulation startup proceeds as follows:

1. Validate the application and all Environment bindings.
2. Assign stable model and endpoint identities.
3. Construct the virtual clock and empty future-action queue.
4. Invoke each model's `start` callback once in stable model-registration order.
5. Stage and commit startup outputs using ordinary model-output rules.
6. Process no scheduled action or Event turn until every model has started successfully.

One shared model is started once, not once per endpoint.

Technical start means the deterministic state machine was initialized. Domain readiness, market session state, subscription acknowledgement, reconciliation, and permission to trade remain application-defined facts where required.

### Command Delivery

Components produce Commands during a kernel turn. Commands remain buffered until the turn reaches quiescence.

After a successful turn, the Simulation Environment submits its Commands to simulated endpoints in deterministic production order. For each Command:

1. Resolve the logical Port endpoint.
2. Invoke the owning model's endpoint callback synchronously.
3. Allow that callback to mutate model-private state and stage outputs.
4. Return from the callback.
5. Commit staged outputs before delivering the next Command.

The scheduler does not select another Event or wake during this post-turn Command phase. Events emitted by Command handlers are queued and cannot recursively enter the kernel.

`on_command` means the Command reached the simulated Port boundary. A model that represents outbound transport latency schedules a private command-arrival action for the future rather than applying the domain effect immediately.

### Wakes

A wake means "invoke this model again with this model-private wake value at virtual time T."

A wake is not a thread, task, sleep, Component callback, or public scheduler handle. Models may schedule their own opaque wakes but cannot schedule direct invocation of another model.

### Event Emission

Model callbacks stage Events through registered endpoints. An Event emission:

- Does not invoke acceptance inline.
- Becomes a scheduler action only after the model callback returns.
- Receives the requested virtual time and a global schedule ordinal.
- Is immutable once committed to the scheduler.
- Is accepted only when selected by the scheduler.
- Creates one ordinary isolated kernel turn.

Multiple Events emitted by one callback remain separate turns in emission order. If several observations must be one atomic application input, the model emits one application-defined batch Event.

### Cancellation

Scheduler cancellation is model machinery, not application Command-cancellation semantics.

- A model may cancel only a pending action for which it holds an authorized token.
- Cancellation staged during a callback becomes effective after that callback returns and before the scheduler selects its next action.
- Cancellation succeeds only while the target remains pending.
- An action already selected or completed cannot be cancelled.
- Cancelling a wake does not synthesize an application Event.
- A protocol emits an acknowledgement Event when application behavior needs to observe cancellation outcome.

### Model Completion

Models are not considered complete merely because they are currently idle. Finite source drivers explicitly report source exhaustion. Shared models with several endpoints still report source completion once for each declared finite source, not once per endpoint.

Kavod introduces no generic application `PortCompleted` Event. A model emits an application-defined `EndOfData` or similar Event only when application behavior needs that fact.

## Scheduler And Reentrancy

The simulation Environment owns one global future-action queue ordered by:

```text
(virtual_time, global_schedule_ordinal)
```

The global schedule ordinal is monotonic and allocated when a staged operation is committed. It is diagnostic ordering state, not a business identifier.

The scheduler loop is conceptually:

```text
pop next scheduled action
advance virtual time to its time
run its model or Event action to completion

if an Event is selected:
    accept exactly that Event
    run its kernel turn to quiescence
    deliver the turn's Commands to simulated endpoints
    commit endpoint outputs

return to the scheduler
```

There is no recursive transition among model and application callbacks:

```text
Component callback
    -> buffer Command
Component callback returns
turn reaches quiescence

simulated endpoint callback
    -> stage Event
simulated endpoint callback returns
Event enters scheduler

scheduler later accepts Event
```

A model callback, Reducer callback, ordinary Component callback, and another model callback never overlap.

## Zero Latency

Zero latency is valid simulation input. It means no virtual-time advance, not immediate recursive execution and not priority over work already queued.

Assume the scheduler contains:

```text
(T, 10) E1
(T, 11) E2
```

E1's turn produces C1. After E1 reaches quiescence, C1 is delivered to its simulated endpoint. The endpoint emits zero-latency E3, which is committed as `(T, 12)`.

The resulting order is:

```text
(T, 10) process E1
        post-turn: deliver C1
        enqueue E3 as (T, 12)

(T, 11) process E2
(T, 12) process E3
```

Command production and delivery remain separate semantic operations even when adjacent:

```text
CommandProduced(C1)
TurnCompleted(E1)
CommandDelivered(C1)
```

No artificial epsilon such as `T + 1ns` is introduced. Same-time ordering comes from schedule and Command-production ordinals.

Multiple Commands from one turn are delivered in production order before the scheduler resumes. Their emitted same-time Events enter the existing queue in commit order and therefore follow actions already queued at that time.

## Same-Time Ordering

The following rules are semantic and must be documented and tested:

1. Virtual time orders different timestamps.
2. Global schedule ordinal orders actions at the same timestamp.
3. Existing same-time actions precede newly committed same-time actions.
4. Model callbacks run to completion before staged outputs are committed.
5. Staged operations commit in model production order.
6. One Event is accepted and fully processed before the next Event is accepted.
7. Commands remain buffered until turn quiescence.
8. Post-turn Commands are delivered in deterministic production order before the scheduler selects another action.
9. Events emitted by Command handlers are queued and never processed in the post-turn Command phase.
10. Multiple emitted Events remain separate ordered turns.
11. Equal timestamps never imply atomicity or completeness.
12. No generic market-before-timer, Event-before-wake, or Port-category priority exists.
13. Scheduling into the past is a terminal causality error.
14. Cancellation may remove pending same-time work before the next scheduler selection but cannot cancel the active action.

Applications and models requiring atomic same-time observations use one batch Event. Kavod does not infer a batch from timestamp equality.

## Market Data And Execution Example

A historical occurrence and a later order must interact with one book without hidden coordination:

```text
T, action 40: market-occurrence wake
    historical model consumes exactly the due occurrence
    model applies occurrence to venue book
    model stages MarketDataProvider::Quote E1
    model schedules only the next source occurrence
    callback returns

T, action 41: E1 selected
    accept E1 through MarketDataProvider endpoint
    process complete kernel turn
    strategy produces ExecutionVenue::SubmitOrder C1
    turn reaches quiescence

post-turn:
    deliver C1 to ExecutionVenue endpoint
    same SimulatedMarketVenue receives C1
    model inspects the book updated by action 40
    model applies zero latency or schedules modeled arrival
    model stages acknowledgement or fill Events
    callback returns

T or later:
    acknowledgement and fill Events are selected normally
    each creates a separate kernel turn
```

The model may assign distinct feed, outbound, acknowledgement, and fill latencies. Those policies are domain-defined. Kavod supplies only deterministic scheduling.

## Anti-Look-Ahead

Determinism does not by itself prevent look-ahead. A deterministic model can be deterministically wrong.

The source/model contract is:

- A source uses only its next due time to arrange its next wake.
- A market occurrence becomes world-visible only when its scheduled action runs.
- The model consumes one occurrence or one explicitly atomic occurrence batch per source action.
- It applies that occurrence to venue truth before emitting its corresponding public Event.
- It stages the public Event before arranging a subsequent same-time occurrence when command interposition must remain possible.
- Future records must not influence current model state, output, latency, or fill decisions.

The common safe loop is:

```text
wake for next occurrence
consume due occurrence
update venue state
emit corresponding public Event
schedule next occurrence
```

This is rejected:

```text
apply occurrence E1
apply occurrence E2
emit public E1
```

An order caused by public E1 could otherwise inspect venue state from undisclosed E2.

Kavod can prevent Components from accessing model state and can prevent models from bypassing the public scheduler context. It cannot prove that arbitrary trusted Rust model code never inspects a future record it owns. Prefix-causality tests, restricted historical-source utilities, review, and model-specific invariants provide assurance. A future stronger source capability may enforce access more narrowly without making Kavod trading-aware.

## Timer Example

A Timer is an independent one-endpoint model:

```text
SetTimer(id, deadline)
    -> Timer endpoint stores registration
    -> schedules Wake(id, generation) at deadline

CancelTimer(id)
    -> Timer endpoint removes registration
    -> cancels pending wake token

Wake(id, generation)
    -> model verifies current generation
    -> stages TimerFired(id)
    -> callback returns
    -> TimerFired enters scheduler
```

A zero-duration timer schedules `TimerFired` at the current virtual time with a later ordinal. It never invokes the kernel inline.

If cancel delivery and firing share a timestamp, execution order decides:

- Cancel delivered before the wake is selected: the wake is cancelled.
- Wake selected first: cancellation is too late.

Timer replacement and stale-wake suppression use model-owned IDs or generations. Exact token APIs remain implementation work.

## Source Exhaustion And Completion

Source exhaustion is not automatically simulation completion. A historical source may end while acknowledgements, fills, cancellations, or finite timers remain scheduled.

MVP supports two conceptual termination policies:

- `UntilIdle`: succeed when every declared finite source is exhausted and the scheduler queue is empty.
- `Until(T)`: process actions through the inclusive virtual-time horizon, then stop before any later action and report remaining scheduled work.

Additional rules are:

- Queue emptiness while a required finite source is not exhausted is `SimulationStalled`, not success.
- Source exhaustion does not cancel already scheduled work.
- Open orders, positions, or ambiguous requests do not automatically prevent technical completion.
- Applications and tests inspect domain state and final invariants separately.
- Recurring timers require cancellation or an explicit horizon.
- Completion reports the last accepted Event, final virtual time, exhausted sources, and pending-work summary where applicable.

No generic shutdown callback may emit new application work after completion has been declared. Domain finalization that must affect behavior occurs through ordinary scheduled Events and Commands before the termination condition.

## Lifecycle And Fault Classification

Kavod core owns technical supervision concepts:

- Binding and configuration validation.
- Stable model construction and startup.
- Scheduler action selection.
- Model callback boundaries.
- Scheduling into the past.
- Invalid endpoint emission or Command routing.
- Model panic or unexpected callback error.
- Simulation action and same-time runaway limits.
- Source-stalled and horizon completion outcomes.
- Monotonic first-failure terminal behavior.

Applications own protocol facts such as:

- Feed connected, disconnected, or exhausted when behavior depends on it.
- Venue session and market status.
- Subscription acknowledgement.
- Order acceptance, rejection, fill, cancellation, and ambiguity.
- Reconciliation, readiness, arming, and permission to trade.
- Timer cancellation acknowledgement when required.

Expected modeled outcomes use Events. A model invariant violation, panic, past schedule, invalid endpoint, or deterministic resource-limit exhaustion is a terminal run failure. Model state is not rolled back after partial mutation, and the failed simulation instance never resumes.

## Simulation Limits

Zero latency may create an infinite same-time chain without recursion:

```text
E1 -> C1 -> E2 -> C2 -> E3 -> ...
```

MVP simulation configuration therefore includes finite limits for:

- Total simulation actions and model callback invocations.
- Simulation actions and model callback invocations at one virtual timestamp.
- Existing Messages, callbacks, and Commands per kernel turn.

Limit exhaustion is an ordinary terminal `RunError` identifying the limit and virtual time. Kavod does not silently advance time, drop work, or invent latency to escape the chain.

## One Interface Versus Separate Interfaces

Zig's `std.Io` demonstrates that an interface can state semantic concurrency and cancellation guarantees without requiring every implementation to use the same OS mechanism. This is a useful principle for Kavod.

It does not imply that one Port implementation trait is the right MVP abstraction.

A shared trait would need to express the weakest legal semantics of both environments:

- External source readiness.
- Command delivery.
- Deferred Event emission.
- Cancellation.
- Time and sleeping.
- Worker or task ownership.
- No reentrant Event delivery even if work completes immediately.

Making live adapter code run unchanged under deterministic simulation would additionally require all network, time, storage, randomness, and concurrency dependencies to use Kavod-controlled primitives. That is adapter-level DST, not basic historical simulation.

One trait also does not solve shared-world ownership. One trait object per logical Port would reproduce the false assumption that each endpoint independently owns its state.

MVP therefore shares `PortSpec` and semantic contracts while keeping separate live-worker and simulated-model execution interfaces. A common façade may be added later if real implementations demonstrate useful commonality without weakening no-reentrancy or ownership rules.

## Comparable Patterns

| Pattern | Useful lesson | Kavod decision |
|---|---|---|
| [QuantConnect LEAN timeslices](https://www.quantconnect.com/docs/v2/writing-algorithms/key-concepts/time-modeling/timeslices) | A time frontier exposes present and past data and uses explicit batches where observations form one unit | Borrow explicit causal exposure and batching; do not add trading timeslices to core |
| [NautilusTrader backtesting](https://nautilustrader.io/docs/latest/concepts/backtesting/) | The simulated exchange processes market data before strategy dispatch and later settles venue Commands against that state | Use one domain-owned venue model behind several logical endpoints; do not import trading engines into core |
| [ABIDES](https://github.com/abides-sim/abides) | A central kernel owns time and delivery while exchange state remains behind controlled message boundaries | Keep one scheduler and prohibit direct model references; do not import generic agents or network topology |
| [SimPy scheduling](https://simpy.readthedocs.io/en/latest/topical_guides/time_and_scheduling.html) | Equal-time work is sequential and uses insertion identity to break ties | Order actions by virtual time and global schedule ordinal |
| Actor mailboxes and event loops | Run-to-completion handlers plus queued sends avoid overlapping state mutation | Stage model outputs and prohibit recursive Event acceptance |
| [FoundationDB simulation](https://apple.github.io/foundationdb/testing.html) | Full-system deterministic simulation requires controlling network, storage, time, failures, and concurrency | Defer full DST and production-adapter execution |
| Replicated state machines | One serialized transition authority makes ordering and state ownership explicit | Borrow ownership and ordering discipline; do not import consensus, replicated logs, or recovery |

These systems are broader or more domain-specific than Kavod. Their common lesson is that protocol boundaries need not equal state-ownership boundaries, and deterministic order must be explicit rather than inferred from timestamp equality.

## v4.1 Reconciliation

This report supersedes the following v4.1 statements where they imply one state owner per Port:

- `design-4.1.md:425`: "Each simulated Port owns its own model and source state."
- `design-4.1.md:450`: A Port-scoped context that can emit only one `P::Event` is insufficient for a grouped model.
- `design-4.1.md:639`: "Simulated Ports own model/source state" incorrectly binds ownership cardinality to logical Port cardinality.
- Gate D's historical source Port and simulated effect Port cannot be required to own unrelated state when they represent one venue world.

The following v4.1 decisions remain valid:

- Simulated execution is synchronous and deterministic.
- Models do not run live threads or async runtimes.
- The Environment owns global virtual time and future-action ordering.
- Models cannot borrow application state.
- Models cannot invoke other Ports or models directly.
- Scheduling into the past is invalid.
- Application and Environment construction remain separate.
- Every logical Port requires exactly one binding.

This report restores and refines the coherent v4 direction in which a shared `HistoricalSimulation` coordinates market source and exchange state while exposing separately typed Port bindings.

## Settled Rules

1. A Port is a logical typed application protocol endpoint.
2. Port identity does not imply a worker, process, object, model, or state owner.
3. Live and simulation share Port Specs, protocol meaning, graph topology, routing, source attribution, and kernel turn semantics.
4. Live and simulated implementations may use different execution interfaces.
5. Every logical Port has exactly one Environment endpoint binding.
6. Every simulated endpoint belongs to exactly one model.
7. One simulated model may expose one or several logical Port endpoints.
8. A grouped model is stored and started exactly once.
9. Grouped bindings do not change the application graph.
10. Model state is external-world state and is inaccessible to Components and Reducers.
11. Historical readers, cursors, books, orders, and deterministic latency policies belong to domain-defined models.
12. The Simulation Environment owns virtual time, global schedule ordinals, and the future-action queue.
13. The Environment stores models opaquely and remains domain-agnostic.
14. Simulated model and endpoint callbacks are synchronous, deterministic, and run to completion.
15. Model outputs are staged until the callback returns.
16. Event emissions and wakes become scheduler actions and never dispatch recursively.
17. One Event is accepted and fully processed before the next Event is accepted.
18. Commands remain buffered until kernel turn quiescence.
19. After a turn, simulated Commands are delivered to endpoints in production order before the scheduler resumes.
20. Events emitted by Command handlers are queued and cannot re-enter the kernel inline.
21. Same-time scheduled actions use global FIFO schedule ordinals.
22. Existing same-time actions precede newly committed same-time actions.
23. Zero latency advances logical order but not virtual time.
24. Equal timestamps do not imply atomicity; atomic observations use one batch Event.
25. A market occurrence updates venue truth before its corresponding public Event is emitted.
26. Future source occurrences must not affect current venue state or outputs.
27. Scheduling into the past is terminal.
28. Pending-action cancellation is effective only before target selection.
29. Finite source exhaustion is explicit and is not automatically application-visible.
30. Normal completion follows an explicit idle or horizon policy.
31. Technical model and scheduler faults stop the simulation permanently.
32. Historical deterministic simulation is MVP; full DST and adapter-level deterministic execution are deferred.

## Minimum Verification

The design should be proved by tests covering at least:

1. Live and simulation use identical application graph construction.
2. Two logical Port endpoints dispatch into one shared simulated model instance.
3. A grouped model starts once rather than once per endpoint.
4. Endpoint identity remains distinct for routing and Event source attribution.
5. Duplicate, missing, and undeclared grouped endpoint bindings fail at build time.
6. A market occurrence updates the book before its public Event is accepted.
7. An order caused by that Event observes the same book state.
8. Changing future historical records cannot alter the earlier execution prefix.
9. The next same-time occurrence is not applied before the current public Event and its post-turn Commands.
10. Event emission never enters the kernel before the model callback returns.
11. Commands remain unavailable to simulated endpoints until turn quiescence.
12. Zero-latency Commands are delivered in production order after the turn.
13. Events emitted by zero-latency Command handlers follow already queued same-time actions.
14. Multiple Events from one callback retain emission order and create separate turns.
15. Batch Events provide atomic input only when explicitly defined by the application protocol.
16. Positive latency schedules domain arrival at the correct future time.
17. Same-time cancel-before-wake and wake-before-cancel traces produce the documented outcomes.
18. Scheduling into the past terminates at the attempted action.
19. Same-time action runaway terminates at the configured deterministic limit.
20. Source exhaustion allows pending finite fills and timers to drain under `UntilIdle`.
21. Queue emptiness with an unfinished required source reports `SimulationStalled`.
22. Horizon completion reports pending scheduled work without silently executing it.
23. Model panic or invariant failure stops before another Event acceptance or Command delivery.
24. Repeated runs with identical model input and configuration produce identical scheduler, Event, Command, and terminal-state traces.

## Rejected Alternatives

- **One independent model per logical Port:** cannot coordinate one venue book without duplication or hidden communication.
- **Shared mutable handles between separately registered Ports:** hides ownership, lifecycle, and ordering from Kavod.
- **Putting simulated venue state in `AppState`:** lets external models and Components share state across the Port boundary and breaks replay semantics.
- **Environment-owned trading structures:** makes Kavod core know about books, fills, venues, and market data.
- **A simulated execution Port subscribing to application market Events:** creates fake live topology and processes public observations rather than venue truth.
- **A venue pulling future source state through a side channel:** permits look-ahead and ambiguous occurrence ordering.
- **Accepting several Events before processing their turns:** creates an accepted-but-unprocessed backlog and contradicts the acceptance linearization point.
- **Inline Event acceptance from a model callback:** reintroduces recursive kernel entry and partially transitioned model observations.
- **Treating zero latency as `T + epsilon`:** invents domain time and makes behavior depend on clock resolution.
- **Letting zero-latency Events jump ahead of existing same-time work:** creates hidden action-category priority and starvation risk.
- **Treating equal timestamps as atomic:** confuses time equality with domain completeness.
- **One mandatory live/simulation implementation trait:** forces a least-common-denominator runtime and does not solve grouped ownership.
- **Full DST in MVP:** requires controlled IO, storage, network, randomness, failures, and scheduling beyond historical simulation needs.

## Explicit Non-Guarantees

Kavod does not guarantee:

- That a simulated venue is behaviorally identical to a real venue.
- That live and simulated implementations share source code or execution APIs.
- That logical Port endpoints are physically isolated.
- That arbitrary simulation model Rust code cannot deliberately inspect future data.
- That equal domain or virtual timestamps represent simultaneous or atomic facts.
- That zero-latency effects are realistic for a particular domain.
- Cross-Port live completion order or parity with simulation schedule order.
- External live Command delivery, execution, exactly-once effects, or rollback.
- Automatic domain-safe completion, flattening, cancellation, or reconciliation.
- Full deterministic execution of production live adapters.
- Random schedule exploration, shrinking, or generalized fault simulation in MVP.

## MVP And Deferred DST

MVP includes:

- Logical Port Specs shared across environments.
- Dedicated-worker live bindings under the settled live runtime.
- Independent and grouped deterministic simulated models.
- One virtual clock and global future-action queue.
- Stable same-time schedule ordinals.
- Post-turn simulated Command delivery.
- Deferred, non-reentrant model Event emission.
- Zero- and positive-latency behavior.
- Historical source exhaustion, idle completion, horizon completion, and stalled detection.
- Timer scheduling and cancellation.
- Past-scheduling and deterministic action-limit failures.
- Simulation provenance covering model, source, ordering, and determinism-relevant configuration.

Deferred full DST includes:

- Random schedule exploration.
- Choice and fault tapes.
- Generic simulated network and storage systems.
- Crash, restart, recovery, and liveness stabilization.
- Shrinking and minimized failing schedules.
- Finite-entropy and probabilistic model facilities.
- Production live adapters running against deterministic IO primitives.
- Replay compatibility, state hashes, snapshots, and cross-build model evolution.

## Dependencies And Open Questions

This report preserves the determinism report's narrow kernel boundary. Simulation Environment and model ordering determine which accepted Event sequence exists; once accepted, the existing kernel guarantee applies unchanged.

It preserves the application-state report's prohibition on Port access to `AppState`. Shared simulated-world state belongs to one external model, not to canonical application state.

It preserves the turn-scheduling report's one-Event turn, breadth-first Messages, and Command publication only after quiescence. Post-turn simulated delivery occurs only after that boundary.

It preserves the live-runtime report's dedicated workers, bounded per-binding queues, Acceptor sequencing, and live Command-mailbox rules. Simulation scheduling does not become a live cross-Port ordering guarantee.

It preserves the diagnostics report's distinction between Command production, Port handoff, external outcome, and accepted Event. The following tracing and live-runtime details remain open:

- Whether every simulation model callback receives an automatic model-action audit record.
- Stable diagnostic identity for model instances versus their endpoint Port identities.
- How a source occurrence action links causally to the accepted Event it emits.
- How a simulated Command delivery links a produced Command to later accepted outcome Events across turns.
- Whether staged schedule and cancellation operations are recorded at `Debug` or `Trace` detail.
- Exact live `LivePort` and simulated-model trait signatures.
- Exact grouped-binding builder syntax and endpoint registration API.
- Exact wake-token representation and stale-token diagnostics.
- Live Event FIFO semantics when one adapter uses several internal producer tasks.
- Detailed live stop-accepting, Event drain, Command drain, timeout, and join behavior.
- Whether and when a future live Environment supports one worker or process exposing several logical Ports.

None of these questions blocks the MVP ownership, scheduling, zero-latency, or completion model.
