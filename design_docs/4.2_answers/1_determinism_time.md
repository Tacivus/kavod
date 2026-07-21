# Determinism And Time

> **Status:** Settled for the Kavod deterministic kernel boundary
> **Scope:** The mapping from accepted Events to deterministic application execution

## Conclusion

Kavod uses a narrow deterministic state-machine contract:

```text
same executable Kavod/application build
+ same frozen graph
+ same initial deterministic application state
+ same determinism-affecting configuration
+ same accepted Event sequence and metadata
= same kernel outputs and application state transitions
```

The deterministic boundary is the application kernel. Live Ports, brokers, networks, OS scheduling, wall-clock delivery timing, and external Command effects remain outside that boundary.

This is intentionally narrower than a full deterministic simulation framework. Kavod does not need full DST, deterministic live Ports, cross-build replay compatibility, snapshots, or state hashes to claim that its kernel is deterministic.

## Determinism Statement

> Given the same executable Kavod/application build, frozen application graph, initial deterministic application state, determinism-affecting application and Engine configuration, and the same sequence of accepted Events with identical payloads, source identities, order, and acceptance timestamps, Kavod executes callbacks in the same order and produces the same ordered Messages, ordered Commands with logical Port destinations, and deterministic application state after each completed turn.
>
> This guarantee assumes that Components and Reducers obey Kavod's determinism contract.

"Same build" means the same executable code, not merely the same source revision or version label. Kavod makes no automatic compatibility claim between different builds.

## Boundary

```text
nondeterministic external world
        |
        | Port offers Event
        v
Event acceptance commit
        |
        | accepted Event: payload + source + index + timestamp
        v
deterministic Kavod kernel
        |
        | ordered produced Commands
        v
nondeterministic external world
```

Kavod freezes observed external behavior at Event acceptance. It does not make the behavior that produced the Event deterministic.

Likewise, Kavod deterministically produces a Command for one logical Port destination. Environment publication and the requested external effect remain outside that production guarantee.

## Event Acceptance

The single acceptance operation:

1. Validates its logical source and protocol membership.
2. Assigns its monotonic Event index.
3. Freezes its acceptance timestamp.
4. Establishes its root causation identity.
5. Emits the complete `EventAccepted` audit record according to the configured diagnostics policy.
6. Commits the Event as accepted.

The acceptance commit is the semantic linearization point. No Reducer or Component receives the Event before that commit.

Diagnostics policy determines whether recording gates the commit:

- Under best-effort recording, Kavod attempts the audit record and commits acceptance even if recording fails.
- Under required recording, the configured writer must acknowledge the audit record before Kavod commits acceptance.
- If required recording fails, the Event is not accepted or dispatched and the Engine stops.

The audit record is diagnostic evidence of acceptance. It is not application state, external truth, or recovery authority.

Port emission, ingress queue insertion, and kernel selection of the next offered Event are not acceptance. They remain nondeterministic live Environment behavior.

Exact diagnostics encoding, storage backend, buffering, batching, and acknowledgement policy are not deterministic callback semantics. Required recording may intentionally change Engine liveness under recorder failure, so the selected diagnostics policy is included in Engine provenance.

## Deterministic Execution Rules

The kernel must preserve these rules:

1. One kernel thread executes all application callbacks.
2. Accepted Events are processed in Event-index order.
3. One Event turn reaches quiescence before the next Event turn begins.
4. Matching Reducers execute before matching ordinary Components.
5. Reducers and Components execute in stable graph registration order.
6. Messages use the specified breadth-first FIFO propagation order.
7. Commands are collected in deterministic production order and leave only after turn completion.
8. Every callback in a turn observes the root Event's frozen acceptance time.
9. Internal Messages do not advance logical time.
10. Event index, not timestamp, establishes accepted Event order.

OS scheduling may change physical latency but cannot change these logical execution rules.

Turn quiescence separates accepted Events and delays Command publication. It does not imply that an earlier Component observed the final canonical state produced by later Messages in the same turn. Related state changes that a decision requires together must use the explicit aggregate-fact semantics settled by the turn-scheduling design.

## Time Concepts

| Term | Meaning |
|---|---|
| Domain time | Application time carried in an Event payload, such as exchange event time |
| Port-observed time | Optional payload or operational metadata describing when a Port observed something; not kernel acceptance time |
| Acceptance time | Time frozen into the Event record by the acceptance operation and exposed as `ctx.now()` |
| Logical time | The root Event's acceptance time shared by the complete turn |
| Wall time | OS civil time used by live infrastructure; never directly visible to Components |
| Virtual time | Simulation-controlled time used as an Event's acceptance time |
| Event index | The authoritative total order of accepted Events |
| Causal ordinal | Deterministic order of Messages, callback work, and Commands within a turn |

Every Message and Command in a turn inherits the root Event's logical time. Ordering within the turn comes from causal ordinals, not invented timestamp increments.

Whether live acceptance time must be nondecreasing is not required to establish determinism. The same recorded timestamps reproduce the same kernel behavior even if they regress. A monotonicity requirement, clock source, and NTP policy are live-time policies and remain open for the live runtime discussion. Event index always remains authoritative for ordering.

## Deterministic Inputs

- Executable Kavod/application build.
- Frozen application graph and registration order.
- Initial canonical application state.
- Initial Component-private state.
- Determinism-affecting application configuration.
- Determinism-affecting Engine configuration, including turn bounds.
- Ordered accepted Events.
- Event payload, source identity, Event index, and acceptance timestamp for each accepted Event.
- Any future explicitly approved deterministic capability input, such as an RNG choice tape.

Environment details such as Port thread scheduling and ingress races determine which accepted Event sequence comes into existence. Once that sequence is fixed, they are not kernel inputs.

An external runtime or required-diagnostics failure may terminate execution at a kernel safe boundary. Such a failure truncates an otherwise deterministic execution prefix; it does not reorder or change callbacks and outputs that completed successfully before the failure.

## Deterministic Outputs

- Callback execution and delivery order.
- Ordered internal Messages.
- Ordered Commands and their logical Port destinations.
- Component-private state transitions.
- Canonical application state transitions.
- Application state after each completed turn.
- Deterministic causal relationships derived by the kernel when causal tracing is enabled.

State hashes are not required to define determinism. They may later be used as a verification mechanism after stable state identity and serialization are designed.

Deterministic turn-limit and invariant failures must occur at the same logical execution point. Exact panic text, backtraces, and process-level failure behavior are not deterministic outputs.

## Component And Reducer Contract

Kavod makes its own scheduler and kernel behavior deterministic, but ordinary Rust code is not sandboxed. The guarantee is conditional on Components and Reducers avoiding nondeterministic observations.

Components and Reducers must not let behavior depend on:

- Wall-clock reads.
- OS randomness.
- Network, filesystem, process, or environment state.
- Process-global mutable state.
- Threads, tasks, or asynchronous completion order.
- Unspecified collection iteration order.
- Port implementation state.

The capability API prevents Kavod from supplying these facilities through callback contexts. Testing, linting, dependency review, and code review provide additional confidence but cannot prove arbitrary Rust code deterministic.

## Minimum Verification

The determinism contract should be supported by:

1. Running the same application, initial state, configuration, and accepted Event sequence repeatedly and comparing Messages, Commands, causal order, and final state.
2. Explicit tests for registration order, Reducer-before-Component order, breadth-first Message order, Command production order, and frozen turn time.
3. Selected fresh-process tests to detect accidental global state, environment dependence, and randomized collection iteration.
4. Review or lint rules forbidding direct clocks, entropy, IO, threading, and inappropriate unordered iteration in deterministic application code.

These are deterministic-kernel tests, not a full DST framework.

## Explicit Non-Guarantees

Kavod does not guarantee:

- Determinism before Event acceptance.
- Which live Event wins an ingress race.
- Deterministic live Port, network, broker, or OS behavior.
- Identical Event sequences from nominally identical live conditions.
- Identical physical execution latency.
- External Command delivery or effect execution.
- Exactly-once external effects.
- Compatibility between different application or Kavod builds.
- Cross-platform numeric equivalence unless separately constrained and tested.
- Determinism from Components or Reducers that violate their contract.
- Completion of an accepted Event turn after an external runtime or required-diagnostics failure terminates the Engine.
- State hashes, snapshots, state restoration, or full replay in the MVP.
- Full deterministic simulation testing or deterministic execution of live adapters.
- Stable panic text, backtraces, OOM behavior, signals, or hardware failures.

## Rejected Alternatives

- **"Same application version" as identity:** a version label does not prove identical executable behavior.
- **Port emission as acceptance:** Port execution and timing are outside the deterministic boundary.
- **Ingress queue insertion as acceptance:** queue arbitration is live Environment behavior and occurs before the acceptance commit.
- **Callback start as acceptance:** the Event must already be committed as accepted before application execution.
- **Domain timestamp ordering:** late or reordered domain timestamps must not silently reorder live Events.
- **Advancing time for internal Messages:** this invents time inside an otherwise immediate turn.
- **Including live Port behavior in the guarantee:** the accepted Event sequence is the frozen observation of that nondeterminism.
- **Cross-build compatibility as part of basic determinism:** compatibility is a separate future replay and evolution concern.
- **Requiring full DST to justify kernel determinism:** controlled whole-system simulation is useful but outside the minimum contract.

## Dependencies On Other Discussions

- The canonical-state discussion defines deterministic application state as one application-owned `AppState` plus Component-private state, all physically owned by the Engine.
- The turn-scheduling discussion preserves per-payload Reducer-before-Component ordering and breadth-first FIFO propagation. It adds no generic phases; related derived updates that require coherent visibility use one explicit aggregate domain fact.
- The Port and simulation design defines how deterministic simulated models, post-turn Command delivery, virtual scheduling, and same-time tie-breaking generate accepted Event order and timestamps without expanding the narrower kernel determinism guarantee.
- The live-runtime discussion defines ingress admission, capacity, and overload policy. Live clock source and acceptance-timestamp monotonicity remain deferred.
- The observability design defines automatic audit detail, user logging, buffering, outputs, and configurable best-effort or required recording without granting recovery authority. Externally caused recording failure may terminate the Engine but cannot alter a successful callback's outputs.

## Open Questions

No unresolved question blocks the minimum determinism contract.

The following questions are deliberately deferred because they do not determine whether the kernel is deterministic:

- Must live acceptance timestamps be nondecreasing?
- Which live clock supplies acceptance timestamps, and how are NTP adjustments handled?
- Which acknowledgement boundary will a future disk-required writer provide: buffered admission, write completion, flush, or data synchronization?
- Which exact causal records are persisted versus generated only in memory?
- How will later builds demonstrate replay compatibility?
- Will stable state serialization and state hashes be added later?
