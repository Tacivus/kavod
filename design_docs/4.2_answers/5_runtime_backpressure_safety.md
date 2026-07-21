# Live Runtime, Backpressure, And Safety

> **Status:** Settled for the Kavod MVP live-ingress and Command-backpressure boundary
> **Scope:** Live Event admission, Acceptor sequencing, source fairness, per-Port capacity, Command-mailbox backpressure, startup gating, fatal overload, and turn limits

## Conclusion

Kavod separates operational admission from deterministic execution.

```text
Live Port thread
    |
    | offers typed Event
    v
bounded FIFO Event queue owned by that Port binding
    |
    | selected by one global Acceptor
    v
Event identity + time + configured audit gate + acceptance commit
    |
    | accepted Event
    v
single-threaded deterministic kernel turn
    |
    | bounded Command batch
    v
bounded FIFO Command mailbox for each destination Port
```

Every live Port binding has independent Event capacity and independent Command-mailbox capacity. A busy Port therefore cannot consume another Port's admission capacity.

One single-threaded logical Acceptor establishes the global Event order. It visits Port bindings in stable binding-order rounds and processes at most the globally configured quantum of Events from each Port per round. This provides bounded source fairness without Event priority.

Accepted Events always execute in Event-index order. Internal Messages remain breadth-first FIFO. Kavod exposes no Event or Message scheduling priority and has no `Critical` or `Normal` admission classes in the MVP.

## Separate Runtime Concerns

The following are different decisions and must not be represented by one priority value.

| Concern | Meaning | Authority |
|---|---|---|
| Event admission | Whether an offered Event has capacity to wait for the Acceptor | Live Environment and Port binding |
| Source fairness | How the Acceptor shares service among nonempty Port queues | Live Environment Acceptor |
| Event sequencing | Which offered Event is accepted next and receives the next Event index | Single Acceptor |
| Event processing | Execution of accepted Events in Event-index order | Deterministic kernel |
| Queue capacity | Maximum bounded work waiting at a live boundary | Port binding configuration |
| Command backpressure | Whether a completed turn's Commands can enter destination mailboxes | Live Environment |

An offered Event is not accepted merely because it entered a Port's Event queue. It has no committed Event index, acceptance time, or causation root until the Acceptor performs the acceptance operation defined by the determinism-and-time design.

## Per-Port Event Admission

Each live Port binding has one bounded FIFO Event queue between its dedicated worker thread and the Acceptor.

```text
MarketData Event queue ----\
Execution Event queue ------+--> one Acceptor
Control Event queue --------/
```

The queues provide capacity isolation:

- A MarketData Port cannot occupy the Execution Port's Event capacity.
- A high-rate Port cannot prevent another Port from offering an Event merely by filling its own queue.
- Total live ingress memory is bounded by the sum of configured per-binding capacities.
- FIFO order is preserved within each Port's emitted Event stream.

The central Event ingress boundary is therefore one logical Acceptor over per-binding queues, not one undifferentiated shared queue whose capacity can be monopolized by one Port.

This refines the earlier description of live Ports racing through one central ingress queue. Live queue visibility remains nondeterministic, but Acceptor selection is deterministic for the queues visible during a round: Port binding order breaks ties, and the global quantum bounds each visit.

Every binding must explicitly configure its Event capacity and full policy. Queue exhaustion is never silently ignored. The exact configuration syntax and policy vocabulary are deferred, but every permitted policy must remain bounded, explicit, observable, and consistent with Kavod's no-silent-drop rule.

## Acceptor Rounds And Global Quantum

The Acceptor uses stable binding-order rounds with one global Event quantum `Q`.

For each round:

1. Visit Port bindings in stable binding order.
2. At each binding, observe the Events already waiting when that visit begins.
3. Select, accept, and process at most `Q` of those Events in FIFO order.
4. Events offered during that visit wait for a later round.
5. Move to the next binding without waiting when the current binding is empty.
6. Begin the next round immediately after all bindings have been visited.

For each selected Event, the Acceptor prepares its identity and time, applies the configured automatic-audit policy, commits acceptance, and processes the complete turn before selecting the next Event. The Acceptor does not accept an entire quantum ahead of kernel execution.

Example with binding order `MarketData`, `Execution`, `Control` and `Q = 4`:

```text
MarketData queue: M1 M2 M3 M4 M5 M6
Execution queue:  E1 E2
Control queue:    C1

accepted order:
M1 M2 M3 M4 E1 E2 C1 M5 M6
```

The global quantum is an operational fairness bound, not payload priority. It is the same for every Port in the MVP. Per-Port weights or per-Port quanta are deferred because they would introduce a more complex scheduling policy.

`Q = 1` maximizes source fairness. A larger `Q` improves burst throughput and locality but permits one Port to occupy more consecutive turns. Capacity and `Q` must be selected from measured workload and acceptable cross-Port latency rather than hidden defaults.

## Fairness And Time

Rounds do not intentionally wait or advance time. If only one Port is nonempty, repeated rounds behave like a direct loop over that Port. The material delay comes from processing one complete kernel turn per accepted Event, not from the round bookkeeping.

A single kernel thread cannot both drain an arbitrary burst from one Port first and guarantee bounded service to every other Port. The global quantum makes this tradeoff explicit and bounded in Event turns.

The approximate service delay before the Acceptor revisits another nonempty Port is determined by:

```text
global quantum
x number of other active Port bindings
x duration of their Event turns
```

The quantum bounds consecutive Event count, not wall-clock duration. Components remain responsible for synchronous, nonblocking callbacks, and turn metrics must reveal work that is too slow for the configured live latency budget.

Events that carry the same application domain timestamp do not thereby receive the same acceptance timestamp. The Acceptor freezes acceptance time as part of the acceptance operation before any configured required-audit acknowledgement and the final commit. Required acknowledgement may delay the commit after that value is frozen. Time spent waiting behind other Events is real queueing delay and must not be hidden by preparing a backlog ahead of execution.

If an application requires several external observations to form one atomic input with one logical time, its Port protocol may define one application-specific batch Event. Kavod does not infer or construct such batches.

## Accepted Event Order

Once an Event is accepted:

1. Its Event index is final.
2. It is processed after every lower Event index.
3. It is processed before every higher Event index.
4. Its turn reaches quiescence before the next accepted Event turn begins.
5. It is never reprioritized by Event type, source, payload, or later arrival.

Live queue visibility and Port timing may affect which accepted order comes into existence. Event indices freeze that observed order for deterministic execution. Configured diagnostics may record the order for later inspection.

## Internal Messages

Internal Messages remain breadth-first FIFO within the current Event turn.

- Matching callbacks run in stable registration order.
- Produced Messages append in production order.
- The next Message is removed from the front.
- Newly produced Messages append to the back.
- No Message crosses into a later Event turn.

Message priority is rejected because it would combine application data with hidden scheduling policy, permit starvation, and make causal execution harder to reason about.

## No Priority Or Admission Classes

Kavod does not expose:

- `Event::priority()`.
- `Message::priority()`.
- A generic scheduling-priority trait.
- `Critical` and `Normal` Event classes.
- Priority dequeueing after acceptance.

Payload priority cannot reserve queue capacity: a supposedly high-priority Event cannot enter capacity already occupied by other Events. It also cannot define source fairness, Command backpressure, or safe overload behavior.

Per-Port Event queues provide the required MVP isolation without reordering Events by semantic importance. If one future Port genuinely contains independent traffic classes with incompatible admission needs, that use case must justify a separate design. The MVP either treats the whole Port under one admission policy or models independent ordering domains as separate logical Ports.

## Configuration Ownership

Configuration is separated by semantic owner.

| Configuration | Owner |
|---|---|
| Message, callback, and Command limits per turn | Engine configuration |
| Global Acceptor quantum | Live Environment configuration |
| Event queue capacity and full policy | Live Port binding configuration |
| Command mailbox capacity and full policy | Live Port binding configuration |
| External subscriptions, decoding, reconnect behavior, batching, or domain-aware coalescing | Concrete live Port configuration |

These operational capacities do not belong to `PortSpec`. A `PortSpec` is shared application topology and protocol meaning across live and simulation; queue construction is live Environment machinery.

The exact builder syntax is intentionally unresolved. The semantic requirement is that consequential capacities and full policies are explicit during Environment construction rather than hidden implementation defaults.

## Domain-Specific Loss And Coalescing

Kavod core knows nothing about quotes, trades, books, fills, disconnects, sequence gaps, operator controls, or any other application domain concept.

Kavod therefore does not define a generic `GapDetected`, `Disconnected`, `Snapshot`, or similar Event. It does not decide which application data may be dropped, coalesced, replaced, deduplicated, or reconstructed.

Those decisions belong outside the kernel:

- A concrete live Port may preprocess, batch, or coalesce external observations before offering an application-defined Event when its protocol permits that behavior.
- A live Port may emit an application-defined Event describing an external condition.
- A Component may derive an application-defined Message from Events it receives.
- Kavod sees only the application's generic Event, Message, and Command protocol types.

The runtime's responsibility is narrower: bounded admission, explicit full outcomes, no silent loss inside Kavod, and deterministic execution after acceptance.

## Command Mailboxes

Each live Port binding has one bounded FIFO Command mailbox.

- Commands are published only after the complete Event turn reaches quiescence.
- Per-Port Command production order is preserved.
- The kernel never blocks waiting for mailbox capacity.
- Mailbox-full behavior follows an explicit binding policy and is never a silent drop.
- Kavod performs no hidden retry.

Blocking is rejected because a full mailbox could stop the kernel from accepting and processing later Events, including Events from other Ports.

## Command Batch Bounds And Reservation

Engine configuration includes a mandatory global `max_commands_per_turn` alongside the existing Message and callback-invocation limits.

At turn completion, Kavod determines the number of Commands destined for each Port and reserves capacity for the complete turn batch before making any Command from that batch visible. Reservations are attempted in stable Port order.

If capacity cannot be obtained according to the configured policies:

- No partial publication is caused merely by ordinary mailbox exhaustion.
- Acquired reservations are released.
- The configured nonblocking failure policy is applied.
- No callback is resumed and the kernel does not wait for capacity.

Whole-batch reservation bounds in-process publication behavior. It does not create a transaction across external systems.

Turn quiescence and whole-batch reservation govern Command publication timing only. They do not prove that a Command was computed against final turn state, and they do not recompute or validate an immutable Command payload after later Reducers run.

Once Commands cross their Port boundaries, Kavod does not guarantee:

- Cross-Port atomicity.
- External delivery.
- External execution.
- Completion order across Ports.
- Exactly-once effects.
- Rollback of a Command already observed by a Port.

A Port failure concurrent with publication may therefore leave an externally ambiguous outcome. Kavod does not silently retry or claim that prior effects were undone.

## Startup Barrier

The live Environment uses a small private technical startup barrier:

1. Validate the application and Environment.
2. Construct every Event queue and Command mailbox.
3. Start every live Port worker.
4. Confirm that every worker entered its run loop and installed its runtime controls.
5. Open the Acceptor and permit Event processing.

No Reducer or Component executes before all required live workers have successfully crossed this barrier. If any worker fails before the barrier opens, startup fails and no Event turn begins.

This barrier establishes only technical worker startup. External connectivity, authentication, reconciliation, readiness, and permission to trade remain application- and Port-specific concepts.

## Fatal Latch And Terminal Failure

The live runtime has one monotonic first-failure fatal latch.

Once fatal failure is established:

- The primary failure is retained.
- Later failures may be retained as secondary diagnostics but cannot replace the primary failure.
- No new Event is accepted.
- No new Event turn begins.
- No new Command batch is published.
- All runtime waits are awakened so shutdown can begin.
- The Engine is permanently terminal and cannot resume.
- No Port is automatically restarted.

The fatal transition must be linearized with Event acceptance and Command publication so that these guarantees do not depend on a racy check-then-act sequence.

A synchronous callback cannot be safely preempted. Fatal failure or an external kill request can take effect only when control returns to a kernel safe boundary. A global emergency stop requiring stronger preemption belongs outside the deterministic kernel and may terminate the process.

Exact worker supervision, technical shutdown, draining, joining, and timeout semantics are deferred from this discussion.

## Turn Limits

Configured resource limits are expected safety guards, not invariant failures.

Exceeding any configured limit, including:

- Maximum Messages per turn.
- Maximum callback invocations per turn.
- Maximum Commands per turn.
- Any future explicit causal-depth bound.

produces an ordinary terminal `RunError` identifying the exceeded limit. The Engine stops and processes no later Event.

Panics remain reserved for:

- Internal invariant violations.
- Impossible runtime states.
- Explicit panics from application callbacks or Port workers.

All panics follow capture-and-stop behavior when the build's panic strategy permits unwinding. Kavod never catches a panic in order to continue the Engine.

## Concrete Scenarios

### Market-Data Burst

The MarketData Port may fill only its own bounded Event queue. It cannot consume the Execution or Control Port queues. During each Acceptor round it receives at most the global quantum before the Acceptor visits later bindings. If its queue reaches capacity, its configured explicit full policy applies.

### Fill During A Long Market Turn

The fill waits in its emitting Port's Event queue. It cannot interrupt the currently executing market Event turn. When the turn completes, the Acceptor continues its binding-order round and eventually visits the fill's Port under the global quantum rule. Priority cannot solve a long synchronous turn; turn duration must remain operationally acceptable.

### Execution Command Mailbox Fills

The kernel does not block. Whole-turn Command reservation fails according to the binding's configured policy, and ordinary capacity exhaustion does not expose only part of the turn's Command batch. Commands that crossed a Port boundary before an unrelated later failure retain no external completion guarantee.

### Port Unexpectedly Exits

An unexpected live worker exit is a terminal infrastructure failure under the existing no-automatic-restart rule. It sets the fatal latch, prevents further acceptance and publication, and leaves the Engine permanently unusable. Exact worker detection and shutdown mechanics are deferred.

### Operator Requests A Kill Switch

An application-defined kill-switch Event, if the application has one, cannot preempt the currently executing turn. It follows ordinary Event acceptance and processing rules. A kill action requiring immediate preemption must operate outside Kavod's deterministic kernel, including process termination when that is the selected operational response.

## Comparable Patterns

| Pattern | Applicable lesson | Kavod decision |
|---|---|---|
| LMAX/Disruptor-style sequencers | Bounded storage and one ordering authority make capacity and order explicit | Preserve one Acceptor and bounded queues; do not import ring-buffer priority semantics |
| Aeron-like runtimes | Offer failure, positions, and backpressure must be visible rather than silently hidden | Event offers and Command publication have explicit bounded outcomes |
| Trading gateways | Independent traffic paths prevent one external flow from consuming every resource | Use per-Port capacity isolation without teaching Kavod trading-domain traffic classes |
| Robust actor and queue systems | Bounded mailboxes and supervision are useful; priority mailboxes risk starvation and reorder senders | Keep per-Port FIFO queues, reject payload priority, and fail visibly |

These comparisons support mechanisms, not technology selection. Kavod does not require Disruptor, Aeron, an actor runtime, or a particular channel implementation.

## Settled Rules

1. Every live Port binding has one bounded FIFO Event queue.
2. Event admission capacity is isolated per Port binding.
3. One global single-threaded Acceptor establishes accepted Event order.
4. The Acceptor uses stable binding-order rounds.
5. Each binding receives at most one global configured quantum per round.
6. Events arriving during a binding's visit wait for a later round.
7. Each Event is accepted and fully processed before the next Event is accepted.
8. The acceptance commit is the Event acceptance linearization point; required diagnostics acknowledgement may gate that commit.
9. Accepted Events process strictly in Event-index order.
10. Internal Messages remain breadth-first FIFO.
11. Events and Messages expose no scheduling priority.
12. The MVP has no generic Critical or Normal admission classes.
13. Queue capacities and full policies are explicit live binding configuration.
14. Domain-aware loss, batching, coalescing, and gap handling are not Kavod kernel semantics.
15. Each live Port binding has one bounded FIFO Command mailbox.
16. The kernel never blocks while publishing Commands.
17. A turn has a mandatory global maximum Command count.
18. Kavod reserves capacity for the whole turn Command batch before ordinary publication.
19. Crossing a Port boundary creates no external effect guarantee or cross-Port atomicity.
20. All live workers cross a private startup barrier before the first Event turn.
21. Fatal failure prevents further Event acceptance, turns, and Command publication.
22. There is no automatic Port restart in the MVP.
23. Configured turn-limit exhaustion is an ordinary terminal error, not a panic.
24. A kill-switch Event cannot preempt a synchronous callback or current turn.

## Rejected Alternatives

- **Payload-level Event priority:** does not reserve capacity and combines application data with runtime scheduling.
- **Message priority:** weakens breadth-first causal semantics and permits starvation within a turn.
- **Critical and Normal MVP classes:** unnecessary once each Port has isolated Event capacity and likely to invite scheduling priority later.
- **One shared undifferentiated ingress queue:** permits one Port to consume capacity needed by every other Port.
- **Drain one Port completely before visiting another:** allows a continuously busy Port to starve later bindings.
- **Exactly one Event per Port per round:** provides maximum fairness but is unnecessarily rigid for burst handling; a bounded global quantum exposes the tradeoff.
- **Accept an entire quantum before processing it:** creates an accepted-but-unprocessed backlog and delays later Port observations behind already accepted Events.
- **Blocking Command publication:** can stall all Event processing behind one slow Port.
- **Partial publication on ordinary capacity exhaustion:** avoidable through whole-batch reservation.
- **Turn-limit panic:** misclassifies an anticipated safety bound as an impossible invariant failure.
- **Kernel-defined trading or feed-loss Events:** violates Kavod's domain-agnostic protocol boundary.

## Explicit Non-Goals

This design does not make Kavod responsible for:

- Domain-specific market-data validity or recovery.
- Quote, trade, fill, disconnect, or sequence-gap semantics.
- External Command delivery or exactly-once effects.
- Cross-Port transactions.
- Preempting synchronous Rust callbacks.
- Making a single kernel thread keep up with every offered workload.
- Automatic Port restart or reconciliation.
- Generic business-safe shutdown, cancel, flatten, or disarm behavior.

## Observability Requirements

The diagnostics design owns exact metric names, labels, storage, and failure policy. This runtime design requires that it be possible to observe at least:

- Per-Port Event queue occupancy, capacity, high-water mark, full outcomes, and offer-to-accept lag.
- Acceptor rounds, quantum use, accepted counts, and time since each Port was last serviced.
- Per-Port Command mailbox occupancy, capacity, reservation failures, and queueing lag.
- Turn duration and Message, callback, and Command counts.
- Startup failures, fatal-latch origin, terminal turn-limit errors, and abandoned queued work.

High-cardinality application identities do not belong in metric labels. Exact recording and telemetry semantics are defined by the dedicated causal trace, logs, and observability report.

## Dependencies On Later Discussions

- Diagnostics implementations must provide the settled recording, metric, queue-lag, and fatal-diagnostic semantics without exposing diagnostics state to callbacks.
- Technical shutdown, stop-accepting, Event drain, Command drain, worker timeout, and join behavior require a separate runtime lifecycle decision.
- Exact worker-exit detection and races between Port failure, Event acceptance, and Command publication require implementation-level supervision semantics consistent with the fatal-latch guarantees above.
- Domain applications must define their own readiness, reconciliation, disarming, loss handling, and external safety controls.

## Open Questions

No unresolved question blocks the MVP separation of deterministic sequencing from operational admission and backpressure.

The following details remain deliberately deferred:

- The numeric default or required explicit value for the global Acceptor quantum.
- Default Event and Command capacities.
- The exact set and spelling of permitted binding full policies.
- Public builder and binding syntax.
- Concrete queue and capacity-reservation implementations.
- Detailed technical shutdown and worker-join semantics.
- Detailed metric and recording schemas.
