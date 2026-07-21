# Kavod Design Workshop Prompts

These prompts divide the remaining Kavod design work into focused discussions. They are intentionally exploratory: the goal is to identify the smallest coherent semantics, expose tradeoffs, and reject bad assumptions before committing to public APIs or implementation.

For every discussion, prefer precise definitions, execution examples, failure cases, and explicit non-goals over code. Do not propose Rust traits, builder APIs, storage layouts, or implementation plans until the semantic design is settled and there is a clear reason to select one option.

The recommended order is:

1. Determinism and time.
2. Canonical state and reducers.
3. Turn scheduling and derived-state consistency.
4. Port and simulation architecture.
5. Live runtime, backpressure, and safety.
6. Causal trace, logs, and observability.

At the end of each discussion, capture only settled decisions, rejected alternatives, open questions, and dependencies on the other discussions. Do not manufacture certainty where a decision needs more evidence.

---

## 1. Determinism And Time

```text
I am designing Kavod, a deterministic single-writer trading application kernel.

Read these documents first:

- design_docs/design-v4.md
- design_docs/design-4.1.md
- design_docs/thoughts.md

design-4.1.md supersedes design-v4.md where they conflict.

I want a design workshop, not implementation code. Be brutally honest. Challenge the premise if it is weak, identify hidden assumptions, and help converge on precise semantics before suggesting public APIs.

Briefly examine how comparable deterministic state machines, realtime trading systems, event-sourced systems, and simulation frameworks define this boundary. Use established designs as evidence, not as cargo-cult templates: identify proven patterns, explain their original constraints, and say where Kavod should deliberately differ.

Kavod's intended deterministic boundary is the application kernel, not live Port implementations. Live Ports, networks, brokers, OS scheduling, and wall-clock delivery timing are nondeterministic. Kavod freezes observed external behavior when an Event is accepted.

My intended guarantee is approximately:

"Given the same application, graph, initial deterministic state, Engine configuration, and accepted Events with the same payloads, source identities, order, and timestamps, Kavod produces the same ordered Messages, Commands, state transitions, and causal relationships."

Explore and pressure-test this claim.

Topics to work through:

1. What does "same application" need to mean? Same source, binary artifact, compiler, dependencies, target, feature flags, numeric behavior, protocol schema, graph, or something else?
2. What can Kavod honestly guarantee for the same exact artifact versus a supposedly compatible later build?
3. Which outputs must be equal: Messages, Commands, causal trace, terminal state, state hashes, errors, and panic behavior?
4. What is the exact live Event acceptance linearization point?
5. Should acceptance time be captured when a Port emits, when ingress admits, or when the kernel begins processing? What are the consequences of each?
6. How should domain time, Port-observed time, acceptance time, logical time, and wall time differ?
7. Should every Message and Command in one turn inherit the root Event's logical time? If not, what alternative preserves replay?
8. How should wall-clock regression, NTP adjustment, and monotonicity work in live mode?
9. Which configuration and provenance values actually affect deterministic results?
10. What determinism claims should Kavod explicitly reject for now?
11. What practical ways can ordinary Rust Component code violate the determinism contract despite capability restrictions?
12. What testing, linting, review rules, and fresh-process replay checks would make the contract credible without pretending Components are sandboxed?

Please use concrete examples and counterexamples. Separate facts that must become core semantic commitments from operational details that can remain configurable.

End with:

- A proposed precise determinism statement.
- A glossary of time concepts.
- A list of deterministic inputs and outputs.
- Explicit non-guarantees.
- Decisions that depend on later discussions.
- Open questions that block progress.

Keep in mind that I dont want a dissertation. I want something actionable/simplified for an MVP. I want the core sematics worked out for my design so that I'm not shooting myself in the foot later 
```

---

## 2. Canonical State And Reducers

```text
I am designing Kavod, a deterministic single-writer trading application kernel.

Read:

- design_docs/design-v4.md
- design_docs/design-4.1.md
- design_docs/thoughts.md
- relevant state, cache, reducer, and projector documents under design_docs/old/

design-4.1.md currently proposes a canonical cache based on BTreeMap<TypeId, Box<dyn Any>>. I dislike this design. It feels like a weak service locator: its identity is not durable, dependencies are hidden, ownership is unclear, and it does not give a convincing path to snapshots, state hashing, or migration.

I want a design workshop, not code. Be brutally honest. Compare alternatives by their semantics, ergonomics, auditability, evolution path, and failure modes. Do not assume that the current cache deserves preservation.

Briefly compare how robust realtime state machines, event-sourced systems, trading engines, and simulation frameworks model canonical state, projections, ownership, and checkpoints. Use those systems to find mature patterns and traps, but do not import an abstraction without explaining why it fits Kavod's constraints.

Current requirements:

- One kernel thread physically owns and mutates deterministic state.
- Only Reducer callbacks mutate canonical shared state.
- Ordinary Components read canonical state and mutate only their own private state.
- Users must have typed access with no visible Any, TypeId, or downcasts.
- Missing dependencies should be found during application construction where possible.
- Reads and writes should be visible in graph metadata.
- Orders, positions, bars, and similar facts are dynamic keyed data.
- Multiple configured instances of the same state family may eventually be needed.
- Future snapshots, state hashes, schemas, and migration must remain possible.
- The design should be reasonably efficient without prematurely optimizing.
- Internal narrow erasure is acceptable if the public model remains typed.

Explore at least these conceptual models:

1. Engine-owned typed state slots or projections with stable identities.
2. Canonical state owned directly by dedicated projector Components.
3. One application-defined concrete state root.
4. Any alternative that better fits Kavod's intended composition model.

Questions to settle:

1. What is canonical state in Kavod? Is it an Engine-owned store, a set of published projections, or something else?
2. What should distinguish canonical shared state from Component-private state?
3. How should canonical state receive stable identity independent of Rust implementation details?
4. How should state dependencies be declared and validated?
5. Which dependency errors are build-time structural errors, and which remain runtime domain errors?
6. Should a state container have one logical writer owner by default?
7. Clarify the difference between one writer owner, many reducer callbacks, and dynamic entities inside a container.
8. When is ordered multiwriter state legitimate, and when is it a design smell?
9. Can one reducer transition several state containers atomically? When should related state instead live in one projection?
10. Should orders, positions, and bars be graph nodes or entities inside stable containers?
11. How should low-cardinality partitions such as account, venue, strategy, or timeframe differ from unbounded runtime IDs such as order IDs?
12. What ownership, schema, snapshot, hashing, and migration responsibilities belong to core versus application state types?
13. Does the chosen model help or hinder possible future parallel read-only Components?

Use trading examples such as order state, positions, portfolio cash, multi-timeframe bars, and reconciliation. Explain the consequences of each choice for graph inspection and auditability.

End with:

- A comparison of viable models.
- One preferred semantic model and why.
- Rejected alternatives and their failure modes.
- State ownership and dependency rules.
- What must be decided before public state APIs are stabilized.
- Open questions and dependencies on persistence or scheduling decisions.

Keep in mind that I dont want a dissertation. I want something actionable/simplified for an MVP. I want the core sematics worked out for my design so that I'm not shooting myself in the foot later 
```

---

## 3. Turn Scheduling And Derived-State Consistency

```text
I am designing Kavod, a deterministic single-writer trading application kernel.

Read:

- design_docs/design-v4.md
- design_docs/design-4.1.md
- design_docs/thoughts.md

Current turn semantics are:

- One accepted Event creates one turn.
- Reducers run before ordinary Components for each delivered Event or Message.
- Components may emit Messages.
- Messages process breadth-first through a FIFO.
- Reducers cannot emit Messages or Commands.
- Commands are collected and leave only after the complete turn reaches quiescence.

I want to examine whether these semantics produce stale or partially derived decisions.

Example: one market tick can close a 1-minute, 5-minute, 15-minute, and daily bar. If those closures are separate Messages, a strategy handling the new 1-minute bar might run after the 1-minute state projection updates but before the other timeframe projections update.

I want a design workshop, not code. Be brutally honest. Do not accept BFS, DFS, reducers, phases, or barriers merely because they sound elegant. Show concrete execution orders and identify what each model actually guarantees.

Briefly research how comparable realtime engines, stream processors, simulation frameworks, and trading systems express derived-state barriers, multi-stage propagation, atomic market updates, and same-timestamp ordering. Extract useful principles, but distinguish their workload and consistency assumptions from Kavod's.

Explore:

1. Is this fundamentally a BFS versus DFS issue, or an issue of modeling one logical market transition as several independently actionable facts?
2. Show exact BFS and DFS traces for the multi-timeframe example.
3. Would DFS improve or worsen sibling consistency?
4. Would allowing Reducers to emit Messages solve the problem? What guarantees would it destroy?
5. Can "run every possible reducer before components" be defined when Components conditionally produce Messages?
6. Should the bar aggregator emit one atomic aggregate fact such as BarsClosed containing all closures caused by the tick?
7. How should a strategy express that it requires coherent multi-timeframe state rather than reacting to individual bars?
8. If separate aggregators are necessary, can an explicit join express the required completeness? How does it know which outputs are expected?
9. When is explicit domain-level aggregation enough?
10. When would Kavod need a generic derive/project-to-quiescence phase followed by a reaction/effect phase?
11. What callback classes and graph restrictions would a phased model require?
12. How would phases interact with cycles, turn limits, state mutation, and command production?
13. Are end-of-turn or state-settled callbacks useful, or do they hide important domain intent?
14. Does buffering Commands until turn end make a decision safe if its Command payload was computed against stale canonical state?
15. What is the smallest MVP rule that prevents accidental partial-state trading decisions?

Prefer explicit domain facts and visible causality over kernel magic. Distinguish a design that is generally correct from one that only happens to work for a particular queue order.

End with:

- A worked multi-timeframe example.
- A recommendation on BFS, DFS, and Reducer outputs.
- A minimal MVP semantic rule.
- A possible later phase model, only if justified.
- Tests and invariants that would prove the decision.
- Open questions that affect the state model or graph model.

Keep in mind that I dont want a dissertation. I want something actionable/simplified for an MVP. I want the core simple sematics worked out for my design so that I'm not shooting myself in the foot later 
```

---

## 4. Port And Simulation Architecture

```text
I am designing Kavod, a deterministic trading application kernel.

Read:

- design_docs/design-v4.md
- design_docs/design-4.1.md
- design_docs/thoughts.md
- design_docs/4.2_answers/* to see what has already been resolved in a deep dive

Kavod has logical Port specifications shared across live and simulation. Live Ports are concurrent external workers. Simulated Ports are synchronous deterministic state machines under virtual time. The application graph must remain the same across environments, but live and simulated implementations do not need the same execution interface.

An unresolved issue is shared simulated-world state.

Example: a historical market occurrence must update a simulated exchange book before the corresponding public market Event reaches the strategy. A later execution Command must arrive at that same book. Independent MarketData and Execution simulated Ports cannot safely coordinate this if each owns unrelated private model state.

I want a design workshop, not code. Be brutally honest. Preserve the useful separation between the application graph and environment mechanics, but do not create fake isolation that requires hidden side channels or look-ahead.

Briefly compare the Port and simulation boundaries used by mature trading simulators, exchange simulators, event-driven runtimes, and deterministic distributed-state-machine systems. Identify patterns that prevent look-ahead, hidden shared state, and reentrancy, while explaining where Kavod's domain-agnostic core should remain smaller.

Explore:

1. What exactly should be shared between live and simulation: protocol shape, Port identity, Command/Event meaning, lifecycle concepts, ordering contracts, or more?
2. What should intentionally differ between live and simulation?
3. Is a logical Port specification the right shared abstraction?
4. What minimum public semantics must a live Port implementer understand, even if the exact API is deferred?
5. What minimum public semantics must a simulated Port implementer understand?
6. How should simulated start, command delivery, wakes, event emission, cancellation, and completion behave conceptually?
7. Should all simulated emissions become scheduler actions only after the current simulated callback returns?
8. How should multiple emissions from one callback and zero-latency Command delivery avoid reentrancy?
9. Which same-timestamp ordering rules are semantic and must be documented?
10. What is an independent simulated Port versus a shared simulated model or world?
11. Should Kavod support an application-defined simulated model that exposes several logical Port boundaries?
12. How can that model remain domain-defined while the Kavod Environment remains domain-agnostic?
13. Who should own historical readers, source cursors, order books, latency models, and the global future-action queue?
14. How should a market occurrence update venue state, emit public data, receive orders, and schedule fills without hidden side channels?
15. How should grouped model bindings appear conceptually without changing the application graph?
16. How should historical source exhaustion and normal simulation completion work?
17. Which lifecycle and fault concepts belong to core technical supervision versus application-defined protocol facts?
18. What deterministic simulation capability is essential for MVP, and what belongs to later full DST?

Use a market-data-plus-execution example and a timer example. Identify any v4.1 choices that make coherent simulation impossible.

End with:

- A conceptual ownership model.
- The intended semantic roles of Port, live implementation, simulated implementation, shared simulated model, and Environment.
- Scheduler and reentrancy rules.
- Same-time ordering rules that must be settled.
- MVP scope and deferred DST work.
- Open questions that affect tracing or live Port semantics.

Keep in mind that I dont want a dissertation. I want something actionable/simplified for an MVP. I want the core sematics worked out for my design so that I'm not shooting myself in the foot later 
```

---

## 5. Live Runtime, Backpressure, And Safety

```text
I am designing Kavod, a live deterministic trading kernel.

Read:

- design_docs/design-v4.md
- design_docs/design-4.1.md
- design_docs/thoughts.md

The MVP live runtime currently proposes one kernel thread, one dedicated OS thread per live Port, a central Event ingress boundary, bounded FIFO Command mailboxes, no silent drops, capture-and-stop failures, and no automatic Port restart.

I considered adding priority() -> u64 to Events or Messages so fills and control traffic could outrank market data. I suspect that is the wrong abstraction because processing order, ingress admission, fairness, and Command backpressure are distinct problems.

I want a design workshop, not code. Be brutally honest. Treat overloaded live trading as a safety problem, not merely a queue implementation detail.

Briefly compare relevant patterns from LMAX/Disruptor-style systems, Aeron-like sequenced runtimes, trading gateways, and robust actor or queue systems. Focus on proven approaches to admission control, sequencing, backpressure, fairness, overload signaling, and shutdown. Do not recommend a technology merely because it is well known.

Explore:

1. Distinguish Event processing order, ingress admission, source fairness, queue capacity, and Command-mailbox backpressure.
2. Should accepted Events always process in acceptance order?
3. Should internal Messages remain breadth-first FIFO?
4. Why does payload-level priority fail to reserve ingress capacity?
5. Should Event or Message payloads expose priority at all?
6. If scheduling priority is ever needed, what constraints prevent starvation and preserve replay?
7. Should live Port bindings have small admission classes such as Critical and Normal?
8. How should reserved ingress capacity work?
9. Should execution and control traffic be protected from market-data saturation?
10. How should a Port that emits mixed traffic be classified?
11. How should FIFO order remain meaningful after admission?
12. How should quote snapshots, trades, book deltas, fills, disconnects, and operator controls differ in permitted coalescing or loss behavior?
13. How should sequence gaps and market-data loss be surfaced?
14. Can a kill-switch Event preempt a currently executing turn? If not, which safety controls must be outside the kernel?
15. Should the kernel block while publishing a Command to a full mailbox?
16. Can a turn reserve capacity for its whole Command batch? What are the tradeoffs?
17. What happens if one of several Command publications fails?
18. What startup barrier is needed before Events begin processing?
19. Define normal stop, stop-accepting, command drain, event drain, timeouts, and worker join behavior.
20. What happens when a Port fails concurrently with a kernel turn?
21. What should a fatal latch guarantee?
22. Should configured turn limits be ordinary terminal errors rather than panics?
23. Which overload, queue, lag, recorder, and turn metrics are required?

Use concrete scenarios: a market-data burst, a fill arriving during a long market-data turn, an execution mailbox filling, a Port unexpectedly exiting, and an operator requesting a kill switch.

End with:

- A recommended separation of deterministic sequencing and operational admission policy.
- A recommendation on priority.
- A safety-oriented overload policy.
- Startup, shutdown, and fatal-failure principles.
- Metrics and observability requirements.
- Decisions that must precede consequential live trading.

Keep in mind that I dont want a dissertation. I want something actionable/simplified for an MVP. I want the core sematics worked out for my design so that I'm not shooting myself in the foot later 
```

---

## 6. Causal Trace, Logs, And Observability

```text
I am designing Kavod, a deterministic trading kernel that must be highly observable during live trading and diagnosable after failures.

Read:

- design_docs/design-v4.md
- design_docs/design-4.1.md
- design_docs/thoughts.md

I want Kavod's only persistence feature to be diagnostic and audit recording. Kavod never restores state, resumes a previous Engine instance, reissues historical Commands, or uses recorded data as a source of truth after a crash. A replacement Engine always cold-starts, backfills required external data, reconciles against external truth, remains disarmed until ready, and then begins a new live session.

The recording system exists only to answer after the fact:

"What did this Engine instance observe, do, and decide?"

Recorded Events, Messages, Commands, and traces may support debugging, audit, and diagnostic replay. They are never recovery inputs, Command-delivery obligations, broker truth, or permission to resume an earlier session.

The system may need different recording modes, such as in-memory capture for tests, local disk persistence for live debugging, and possibly external export. Human logs may include wall time, thread information, transport errors, and formatting. OpenTelemetry tracing is sampled and exporter-dependent. Components must not change business behavior based on telemetry configuration, logger availability, sampling, or exporter backpressure.

I want a design workshop, not code. Be brutally honest. Seek the simplest storage and observability model that preserves strict distinctions of authority and does not allow telemetry to affect deterministic behavior.

Briefly compare how comparable realtime and trading systems separate deterministic records, causal diagnostics, audit trails, structured logs, metrics, and distributed tracing. Use patterns from systems such as LMAX, Aeron, event-sourced applications, trading frameworks, and OpenTelemetry where relevant, but do not import their recovery or persistence semantics by default.

Explore:

1. Should Kavod call this facility a recorder, trace recorder, audit recorder, or journal? Which name avoids implying recovery authority?
2. Can one serializable append-only run record replace separate physical Event tapes, Command tapes, and causal trace files while still exposing distinct logical views?
3. What minimum records make a live incident explainable: accepted Events, produced Commands, root causation, callback identity, graph/config identity, faults, and Port observations?
4. Which records should be mandatory in every recording mode, and which are optional deep-trace detail?
5. Should internal Message payloads be recorded, hashed, sampled, or only regenerated during diagnostic replay?
6. Should callback invocation and completion be recorded? Under which trace-detail policies?
7. What causal information must every produced Command retain to explain why it happened?
8. How should deterministic Kavod causal identity differ from OpenTelemetry trace and span identity?
9. Should diagnostic replay create new observability traces and link to original runs rather than reuse original trace IDs?
10. How should distributed trace context cross Port boundaries without becoming deterministic business data?
11. What do in-memory, disk-backed, and externally exported recording modes mean semantically?
12. Which configuration modes are useful, for example Disabled, Memory, DiskBestEffort, DiskRequired, and external export?
13. If DiskRequired recording fails, should the Engine stop to preserve a complete audit trail? If best-effort recording, logs, or OpenTelemetry export fails, should execution continue?
14. How should disk trace framing, serialization, schema identity, retention, segmentation, truncation, corruption detection, and backpressure work?
15. What does "full tracing" mean in storage volume and performance terms?
16. Should Component and Reducer contexts expose logging or tracing directly?
17. Would a narrow write-only structured annotation mechanism be safer? What fields may it contain?
18. Why must annotation calls reveal neither sampling state nor recorder success/failure?
19. Which business facts must remain typed Events, Messages, or Commands rather than becoming annotations or logs?
20. How should Ports record nondeterministic operational details such as reconnects, wire errors, Command submission attempts, and wall-clock latency?
21. How should logs correlate with run ID, Event index, causal operation, Port instance, and produced Command identity?
22. Which metrics are essential, and which high-cardinality identifiers must be excluded?
23. How should diagnostic replay identify and report the first divergence using recorded Events, Commands, state hashes, and optional causal records?
24. Research comparable approaches in LMAX, Aeron Archive and Cluster, event-sourced systems, NautilusTrader, and OpenTelemetry. Separate verified patterns from guesses and do not import their recovery semantics by default.

End with:

- A recommended recording architecture and name.
- Required and optional diagnostic record categories.
- Causal identity principles.
- In-memory and serializable disk-recording semantics.
- Trace and logging failure policies that do not create recovery semantics.
- Safe context-level diagnostic capabilities, if any.
- Replay divergence-reporting principles.
- Explicit confirmation that recording is not a recovery, state-restoration, or Command-delivery mechanism.

Keep in mind that I dont want a dissertation. I want something actionable/simplified for an MVP. I want the core sematics worked out for my design so that I'm not shooting myself in the foot later 
```

---

## Final Synthesis Prompt

Use this only after the seven focused discussions have produced settled decisions.

```text
I have completed focused Kavod design discussions covering:

1. Determinism and time.
2. Canonical state and reducers.
3. Turn scheduling and derived-state consistency.
4. Port and simulation architecture.
5. Live runtime, backpressure, and safety.
6. Causal tracing, logging, and observability.

I will provide the conclusions, rejected alternatives, and open questions from each discussion.

Synthesize them into one internally consistent design. This is still a design review, not an implementation request. Be brutally honest. Do not preserve a prior conclusion merely because it already exists. Prefer the smallest coherent semantics that are safe for consequential live trading.

Check for:

1. Contradictory terminology or definitions.
2. Conflicting ownership or unclear authority.
3. Duplicate abstractions that should be combined.
4. Concepts overloaded with incompatible responsibilities.
5. Circular dependencies between design decisions.
6. Public semantic commitments unsupported by the design.
7. Live startup, reconciliation, and arming gaps.
8. Hidden nondeterminism.
9. Simulation and live semantic drift.
10. Incomplete state boundaries.
11. Backpressure, shutdown, or failure behavior that violates no-silent-drop.
12. Observability behavior that can affect deterministic execution.
13. Claims that should be deferred rather than promised by the MVP.

Produce:

- A concise conceptual model and glossary.
- Settled invariants and explicit non-goals.
- The normative deterministic turn lifecycle.
- The normative cold-start, reconciliation, and arming lifecycle.
- State, Port, simulation, and observability ownership boundaries.
- Failure and safety principles.
- The narrowed MVP scope.
- Design decisions that still block implementation.
- A dependency-ordered list of the next design conversations or validation exercises.

Do not write code or settle syntax. If a public interface cannot be derived from settled semantics, say so directly and identify the missing decision.
```
