# Live Runtime, Backpressure, And Safety

> **Status:** Settled for the Kavod MVP live-ingress and Command-backpressure boundary
> **Scope:** Live Event admission, Acceptor sequencing, source fairness, per-Port capacity, Command-mailbox backpressure, startup gating, fatal overload, and turn limits
> **ControlPlane reconciliation:** `7_control_plane_lifecycle_supervision.md` owns Ready-first bootstrap, immediate lifecycle authority transitions, normally ordered lifecycle ControlEvents, unavailable-destination Command rejection, Port quarantine, and Engine-global fatal classification. This report continues to own ordinary Port admission and capacity mechanics.

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

ControlPlane lifecycle reports
    -> immediate authority update where required
    -> ordinary ControlPlane FIFO
    -> accepted ControlEvent
```

Every live Port binding has independent Event capacity and independent Command-mailbox capacity. A busy Port therefore cannot consume another Port's admission capacity.

One single-threaded logical Acceptor establishes the global Event order. After structurally first `Ready`, it visits the ControlPlane FIFO and Port binding FIFOs in one frozen source-order round and processes at most the globally configured quantum of Events from each source per round. This provides bounded source fairness without Event priority.

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
| Lifecycle authority | Immediate closure of new admission or handoff without reprioritizing the later ControlEvent | Engine ControlPlane |

An offered Event is not accepted merely because it entered a Port's Event queue. It has no committed Event index, acceptance time, or causation root until the Acceptor performs the acceptance operation defined by the determinism-and-time design.

Queue admission is nevertheless an irrevocable Environment commitment for the MVP. Admission freezes the payload, logical source, and source incarnation as eligible future input. Later stop, quarantine, or worker exit closes new admission but does not remove, rewrite, reprioritize, or invalidate Events already admitted. Acceptance later assigns Event index, acceptance time, and root causation in the ordinary Acceptor order.

## Per-Port Event Admission

Each active live Port binding has one bounded FIFO Event queue between its Environment-owned implementation unit and the Acceptor.

```text
MarketData Event queue ----\
Execution Event queue ------+--> one Acceptor
Other Port Event queue -----/

ControlPlane ingress ---------> same acceptance authority
```

The queues provide capacity isolation:

- A MarketData Port cannot occupy the Execution Port's Event capacity.
- A high-rate Port cannot prevent another Port from offering an Event merely by filling its own queue.
- Total live ingress memory is bounded by the sum of configured per-binding capacities.
- FIFO order is preserved within each Port's emitted Event stream.
- Once admitted, an Event remains eligible even if its source incarnation later stops or fails.

The central Event ingress boundary is therefore one logical Acceptor over per-binding queues, not one undifferentiated shared queue whose capacity can be monopolized by one Port.

ControlPlane input is not a Port binding and does not consume a Port's capacity. Ordinary ControlEvents participate in the same acceptance authority and ordinary Acceptor ordering. `Ready` is structurally first, but later lifecycle ControlEvents receive no bypass around the frozen source-order rounds. Authoritative quarantine may precede its application-visible ControlEvent without reprioritizing accepted input.

ControlPlane ingress is separately bounded and Engine-owned. Host requests may use explicitly defined coalescing, but supervisor reports and application-visible lifecycle outcomes are never silently dropped. Exhaustion that prevents the ControlPlane from preserving an authoritative lifecycle transition is an Engine-global fatal failure.

This refines the earlier description of live Ports racing through one central ingress queue. Live queue visibility remains nondeterministic, but Acceptor selection is deterministic for the queues visible during a round: Port binding order breaks ties, and the global quantum bounds each visit.

Every binding must explicitly configure its Event capacity and full policy. Queue exhaustion is never silently ignored. The exact configuration syntax and policy vocabulary are deferred, but every permitted policy must remain bounded, explicit, observable, and consistent with Kavod's no-silent-drop rule.

## Acceptor Rounds And Global Quantum

For ordinary ingress after `Ready`, the Acceptor uses one frozen source order containing the ControlPlane FIFO and every Port binding FIFO, with one global Event quantum `Q`. The ControlPlane's exact slot is frozen during Environment construction and is run provenance; it has no privileged bypass position.

For each round:

1. Visit sources in frozen source order.
2. At each source, observe the Events already waiting when that visit begins.
3. Select, accept, and process at most `Q` of those Events in FIFO order.
4. Events offered during that visit wait for a later round.
5. Move to the next source without waiting when the current source is empty.
6. Begin the next round immediately after all sources have been visited.

For each selected Event, the Acceptor prepares its identity and time, applies the configured automatic-audit policy, commits acceptance, and, absent fatal establishment, processes the complete turn before selecting the next Event. The Acceptor does not accept an entire quantum ahead of kernel execution.

Example with an empty ControlPlane FIFO, Port order `MarketData`, `Execution`, `ReferenceData`, and `Q = 4`:

```text
MarketData queue: M1 M2 M3 M4 M5 M6
Execution queue:  E1 E2
Reference queue:  R1

accepted order:
M1 M2 M3 M4 E1 E2 R1 M5 M6
```

The global quantum is an operational fairness bound, not payload priority. It is the same for every Port in the MVP. Per-Port weights or per-Port quanta are deferred because they would introduce a more complex scheduling policy.

`Q = 1` maximizes source fairness. A larger `Q` improves burst throughput and locality but permits one Port to occupy more consecutive turns. Capacity and `Q` must be selected from measured workload and acceptable cross-Port latency rather than hidden defaults.

## Fairness And Time

Rounds do not intentionally wait or advance time. If only one Port is nonempty, repeated rounds behave like a direct loop over that Port. The material delay comes from processing one complete kernel turn per accepted Event, not from the round bookkeeping.

A single kernel thread cannot both drain an arbitrary burst from one Port first and guarantee bounded service to every other Port. The global quantum makes this tradeoff explicit and bounded in Event turns.

The approximate service delay before the Acceptor revisits another nonempty Port is determined by:

```text
 global quantum
 x number of other active ingress sources
x duration of their Event turns
```

The quantum bounds consecutive Event count, not wall-clock duration. Components remain responsible for synchronous, nonblocking callbacks, and turn metrics must reveal work that is too slow for the configured live latency budget.

Events that carry the same application domain timestamp do not thereby receive the same acceptance timestamp. The Acceptor freezes acceptance time from the run's wall-anchored monotonic clock as part of the acceptance operation before any configured required-audit acknowledgement. Under required recording, acknowledgement of `EventAccepted` is the acceptance commit. Required acknowledgement may delay processing after that value is frozen. Time spent waiting behind other Events is real queueing delay and must not be hidden by preparing a backlog ahead of execution.

If an application requires several external observations to form one atomic input with one logical time, its Port protocol may define one application-specific batch Event. Kavod does not infer or construct such batches.

## Accepted Event Order

Once an Event is accepted:

1. Its Event index is final.
2. It is processed after every lower Event index.
3. It is processed before every higher Event index.
4. Absent fatal establishment, its turn reaches quiescence before the next accepted Event turn begins.
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
| Command mailbox capacity | Live Port binding configuration; full means settled per-destination rejection |
| External subscriptions, decoding, reconnect behavior, batching, or domain-aware coalescing | Concrete live Port configuration |

These operational capacities do not belong to `PortSpec`. A `PortSpec` is shared application topology and protocol meaning across live and simulation; queue construction is live Environment machinery.

The exact builder syntax is intentionally unresolved. The semantic requirement is that consequential capacities and Event-admission full policies are explicit during Environment construction rather than hidden implementation defaults. Command-mailbox exhaustion has one core MVP outcome rather than a configurable policy menu.

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
- Mailbox-full rejects the complete destination batch visibly and never silently drops, retains, or retries it.
- Kavod performs no hidden retry.

Blocking is rejected because a full mailbox could stop the kernel from accepting and processing later Events, including Events from other Ports.

## Command Batch Bounds And Reservation

Engine configuration includes a mandatory global `max_commands_per_turn` alongside the existing Message and callback-invocation limits. The bound covers the combined Port Command and ControlCommand production count.

At turn completion, the ControlPlane first classifies Commands by authoritative destination state. Commands for stopped, starting, stopping, or quarantined Ports are rejected per destination and produce later ControlEvents. Kavod then groups eligible Commands by destination and attempts one all-or-none capacity reservation for each destination batch in stable Port order.

If one destination lacks capacity:

- None of that destination's Commands cross the Port boundary.
- Every Command in that destination batch receives a causally linked later `CommandNotDelivered` ControlEvent with reason `MailboxFull` while the Engine remains nonfatal.
- Commands for other destinations remain independently eligible and are not suppressed.
- No rejected Command is retained or retried.
- No callback is resumed and the kernel does not wait for capacity.

Per-destination all-or-none reservation bounds in-process publication behavior without inventing cross-Port atomicity. If Kavod cannot preserve the required rejection dispositions, that accounting failure poisons the Engine.

Unavailable-destination rejection is not partial publication caused by capacity exhaustion. Healthy-target Commands remain eligible; rejected Commands never cross their Port boundary and are never retained for implicit delivery after restart.

Turn quiescence and per-destination reservation govern Command publication timing only. They do not prove that a Command was computed against final turn state, and they do not recompute or validate an immutable Command payload after later Reducers run.

Once Commands cross their Port boundaries, Kavod does not guarantee:

- Cross-Port atomicity.
- External delivery.
- External execution.
- Completion order across Ports.
- Exactly-once effects.
- Rollback of a Command already observed by a Port.

A Port failure concurrent with publication may therefore leave an externally ambiguous outcome. Kavod does not silently retry or claim that prior effects were undone.

## Ready-First Startup Barrier

The live runtime uses a small private Engine-infrastructure barrier:

1. Validate the application and Environment.
2. Construct every Event queue and Command mailbox.
3. Construct the Kernel, ControlPlane, diagnostics, and inert Port bindings.
4. Establish every logical Port as stopped with ingress and Command handoff closed.
5. Accept `Ready` as the first ControlEvent turn.
6. Start workers only in response to deferred lifecycle ControlCommands.

No Reducer or Component executes before Engine infrastructure crosses this barrier. Port worker startup and failure occur after Ready and become later lifecycle ControlEvents under the ControlPlane report.

This barrier establishes only that deterministic control execution can begin. Technical Port start, external connectivity, authentication, reconciliation, readiness, and permission to trade remain distinct later transitions.

A starting live Port has no Event-admission authority. Before reading, decoding, or constructing an ordinary application Event, the implementation waits on the Environment's cancellable ingress gate until its `PortStarted` turn completes and opens admission. Any buffering performed by an external transport before that gate is Port/external-system state, not a hidden Kavod Event buffer. Simulated endpoints begin ordinary model activity only after the same completed-turn transition.

## Fatal Latch And Terminal Failure

The live runtime has one monotonic first-failure fatal latch for Engine-global failure. Contained Port-local failures use ControlPlane quarantine and do not set this latch.

Once fatal failure is established, the Engine is poisoned:

- The primary failure is retained.
- Later failures may be retained as secondary diagnostics but cannot replace the primary failure.
- No later Event is guaranteed to be accepted.
- No later Event turn is guaranteed to begin.
- No later Command batch is guaranteed to be published.
- All runtime waits are awakened so shutdown can begin.
- The Engine is permanently terminal and cannot resume.
- No Port is restarted after Engine-global fatal failure.

Fatal establishment occurs at the operation boundary where it is observed, whether or not diagnostics can retain that boundary. Kavod guarantees only the successfully completed prefix before it. The current callback or turn may be incomplete, its buffered outputs need not take effect, and its state is not reusable.

A panic on the kernel thread unwinds the active callback immediately when the build permits unwinding. An asynchronous fatal report from another thread sets the fatal latch but cannot safely preempt arbitrary synchronous Rust code; the kernel observes it when control returns. A global emergency stop requiring stronger preemption belongs outside the deterministic kernel and may terminate the process.

Port lifecycle, quarantine, application-managed shutdown, and worker ownership semantics are defined by the ControlPlane report. Concrete queue, cancellation, timeout, and join implementations remain deferred.

## Turn Limits

Configured resource limits are expected safety guards, not invariant failures.

Exceeding any configured limit, including:

- Maximum Messages per turn.
- Maximum callback invocations per turn.
- Maximum combined Port Commands and ControlCommands per turn.
- Any future explicit causal-depth bound.

produces an ordinary terminal `RunError` identifying the exceeded limit. The Engine stops and processes no later Event.

Panics remain reserved for:

- Internal invariant violations.
- Impossible runtime states.
- Explicit panics from application callbacks or Engine-global runtime code. A caught, contained Port worker panic follows quarantine semantics.

Kernel, callback, ControlPlane, and Engine-global runtime panics follow capture-and-stop behavior when the build's panic strategy permits unwinding. A caught Port worker panic discards the failed worker, closes new authority, and quarantines every affected logical endpoint; the worker itself never resumes, but Events it admitted before panic remain in the Environment FIFO.

## Concrete Scenarios

### Market-Data Burst

The MarketData Port may fill only its own bounded Event queue. It cannot consume another Port's queue or the ControlPlane ingress capacity. During each Acceptor round it receives at most the global quantum before the Acceptor visits later bindings. If its queue reaches capacity, its configured explicit full policy applies.

### Fill During A Long Market Turn

The fill waits in its emitting Port's Event queue. It cannot interrupt the currently executing market Event turn. When the turn completes, the Acceptor continues its binding-order round and eventually visits the fill's Port under the global quantum rule. Priority cannot solve a long synchronous turn; turn duration must remain operationally acceptable.

### Execution Command Mailbox Fills

The kernel does not block. A full destination rejects its complete destination batch with causally linked `CommandNotDelivered(MailboxFull)` ControlEvents. Other destinations remain eligible. Commands that crossed a Port boundary before an unrelated later failure retain no external completion guarantee.

### Port Unexpectedly Exits

An unexpected live worker exit is a Port-local supervisor failure when its affected logical endpoints can be isolated. The ControlPlane quarantines those endpoints, closes new Event admission and Command handoff, and enqueues normally ordered lifecycle ControlEvents. Events already admitted by the failed incarnation remain in their FIFO and are accepted under ordinary Acceptor fairness. No restart occurs without an application ControlCommand and without draining that admitted FIFO. If the failure cannot be isolated from Engine-global infrastructure, the fatal latch is set instead.

### Operator Requests A Kill Switch

A cooperative kill-switch request may enter as a ControlEvent and cannot preempt the currently executing turn. It follows ordinary acceptance and processing rules. A kill action requiring immediate preemption must operate outside Kavod's deterministic kernel, including process termination when that is the selected operational response.

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
2. Event admission capacity is isolated per Port binding, with separately bounded ControlPlane ingress.
3. One global single-threaded Acceptor establishes accepted Event order.
4. After structurally first `Ready`, the Acceptor uses one frozen source order containing the ControlPlane and Port FIFOs.
5. Each source receives at most one global configured quantum per round.
6. Events arriving during a source's visit wait for a later round.
7. Absent fatal establishment, each Event is accepted and fully processed before the next Event is accepted.
8. The acceptance commit is the Event acceptance linearization point; required diagnostics acknowledgement may gate that commit.
9. Accepted Events process strictly in Event-index order.
10. Internal Messages remain breadth-first FIFO.
11. Events and Messages expose no scheduling priority.
12. The MVP has no generic Critical or Normal admission classes.
13. Queue capacities and Event-admission full policies are explicit live binding configuration; Command-mailbox full behavior is fixed by core semantics.
14. Domain-aware loss, batching, coalescing, and gap handling are not Kavod kernel semantics.
15. Each live Port binding has one bounded FIFO Command mailbox.
16. The kernel never blocks while publishing Commands.
17. A turn has a mandatory global maximum combined Port Command and ControlCommand count.
18. After unavailable destinations are rejected visibly, Kavod reserves each eligible destination batch all-or-none; mailbox-full destinations receive `CommandNotDelivered(MailboxFull)` while healthy destinations remain eligible.
19. Crossing a Port boundary creates no external effect guarantee or cross-Port atomicity.
20. Engine infrastructure crosses a private startup barrier before Ready, and Ready precedes every Port worker start.
21. Contained Port-local failure quarantines affected endpoints. After Engine-global fatal establishment, the runtime must not intentionally dispatch or publish further work, but application correctness cannot depend on any post-fatal behavior.
22. There is no automatic Port restart; restart requires an accepted application ControlCommand.
23. Configured turn-limit exhaustion is an ordinary terminal error, not a panic.
24. A kill-switch Event cannot preempt a synchronous callback or current turn.
25. Event queue admission is irrevocable for the MVP even though it is not Event acceptance.
26. Stop or quarantine closes new admission but does not remove already admitted Events.
27. Port stop, quiescence, restart eligibility, and normal Engine completion require the old incarnation's admitted FIFO to be empty.

## Rejected Alternatives

- **Payload-level Event priority:** does not reserve capacity and combines application data with runtime scheduling.
- **Message priority:** weakens breadth-first causal semantics and permits starvation within a turn.
- **Critical and Normal MVP classes:** unnecessary once each Port has isolated Event capacity and likely to invite scheduling priority later.
- **One shared undifferentiated ingress queue:** permits one Port to consume capacity needed by every other Port.
- **Drain one Port completely before visiting another:** allows a continuously busy Port to starve later bindings.
- **Exactly one Event per Port per round:** provides maximum fairness but is unnecessarily rigid for burst handling; a bounded global quantum exposes the tradeoff.
- **Accept an entire quantum before processing it:** creates an accepted-but-unprocessed backlog and delays later Port observations behind already accepted Events.
- **Blocking Command publication:** can stall all Event processing behind one slow Port.
- **Partial publication within one destination on ordinary capacity exhaustion:** avoided through per-destination all-or-none reservation; unrelated destinations are not coupled.
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
- Automatic Port restart or reconciliation; explicit application-requested restart is defined separately.
- Generic business-safe shutdown, cancel, flatten, or disarm behavior.

## Observability Requirements

The diagnostics design owns exact metric names, labels, storage, and failure policy. This runtime design requires that it be possible to observe at least:

- Per-Port Event queue occupancy, capacity, high-water mark, full outcomes, and offer-to-accept lag.
- Acceptor rounds, quantum use, accepted counts, and time since each Port was last serviced.
- Per-Port Command mailbox occupancy, capacity, reservation failures, and queueing lag.
- Turn duration and Message, callback, and Command counts.
- Ready/bootstrap state, Port start and quarantine failures, fatal-latch origin, terminal turn-limit errors, unavailable-destination rejections, and abandoned queued work.

High-cardinality application identities do not belong in metric labels. Exact recording and telemetry semantics are defined by the dedicated causal trace, logs, and observability report.

## Dependencies On ControlPlane And Implementation Work

- Diagnostics implementations must provide the settled recording, metric, queue-lag, and fatal-diagnostic semantics without exposing diagnostics state to callbacks.
- Logical shutdown, quarantine, restart, and failure-race semantics are settled by the ControlPlane report; concrete worker timeout and join mechanisms remain implementation work.
- Exact worker-exit detection and queue implementation must preserve immediate ControlPlane authority changes, ordinary ControlEvent ordering, admitted-Event drainage, and per-destination Command-disposition rules.
- Domain applications must define their own readiness, reconciliation, disarming, loss handling, and external safety controls.

## Open Questions

No unresolved question blocks the MVP separation of deterministic sequencing from operational admission and backpressure.

The following details remain deliberately deferred:

- The numeric default or required explicit value for the global Acceptor quantum.
- Default Event and Command capacities.
- The exact set and spelling of permitted Event-admission full policies. Command-mailbox full behavior is settled as per-destination rejection with typed disposition.
- Public builder and binding syntax.
- Concrete queue and capacity-reservation implementations.
- Detailed technical shutdown and worker-join semantics.
- Detailed metric and recording schemas.
