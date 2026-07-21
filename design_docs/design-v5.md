# Kavod Core Design v5

> **Status:** Semantic design ready for validation
> **Scope:** MVP application model, deterministic kernel, state, Ports, live and simulated Environments, runtime control, supervision, lifecycle, and diagnostics
> **Authority basis:** Synthesized from the seven 4.2 reports, then `design-4.1.md`, then `design-v4.md` and `thoughts.md`; explicit reconciliation in the 4.2 reports governs conflicts

---

## 1. Purpose, Scope, And Thesis

Kavod is a domain-agnostic deterministic application kernel intended to support historical simulation and consequential live trading without changing deterministic application logic.

The one-line thesis is:

> A single-writer deterministic kernel consumes an accepted sequence of typed Port Events and ControlEvents, applies explicit canonical-state transitions, propagates internal Messages, and emits deferred Port Commands and ControlCommands through boundaries whose runtime authority, failure, and loss semantics are explicit.

This document defines the MVP's semantic contract. It does not finalize Rust trait syntax, registry storage, queue implementations, executor choices, serialization formats, or public error types. Short Rust examples are illustrative only. They show intended relationships and capabilities, not a compatibility commitment.

The design deliberately focuses on:

- One deterministic application graph and kernel.
- One concrete canonical application state root.
- Typed logical Ports shared across live and simulation.
- Explicit control and supervision without exposing runtime machinery to application callbacks.
- Bounded live boundaries, no silent loss, and safe fail-stop behavior.
- Deterministic historical simulation without claiming full deterministic simulation testing.
- Diagnostics that explain a run without becoming recovery authority.

## 2. Document Conventions

Normative language is used narrowly:

- **Must** states required behavior.
- **May** states permitted variation.
- **Does not guarantee** states an explicit boundary of responsibility.
- **Deferred** means the capability or exact interface is intentionally outside this semantic design.

The terms Port Event, ControlEvent, Message, Port Command, and ControlCommand are capitalized when referring to Kavod protocol classes. Unqualified **accepted input** means either an accepted Port Event or an accepted ControlEvent. Unqualified **Command** is avoided where the distinction between Port Command and ControlCommand matters.

## 3. Normative Terminology

| Term | Meaning |
|---|---|
| Application | The immutable graph, protocol types, Components, Reducers, initial `AppState`, and application configuration supplied to one Engine run |
| Engine | The owner and coordinator of one run, including the Kernel, ControlPlane, Environment, application state, and diagnostics |
| Kernel | The single-threaded executor of accepted inputs, Reducers, Components, Messages, and deterministic outputs |
| Environment | Runtime machinery that binds logical Ports, admits Port Events, hands off Port Commands, supervises implementations, and, in simulation, owns virtual scheduling |
| ControlPlane | The unique Engine-owned authority for Engine and logical-Port technical lifecycle; it is not a Port |
| Port | A logical typed boundary between the deterministic application and an external or simulated system |
| Port Spec | The application-defined contract associating one logical Port with its Port Command and Port Event types |
| Port Event | An immutable external fact emitted through a logical Port and offered for kernel acceptance |
| ControlEvent | An immutable Engine-control fact produced by the ControlPlane and offered for kernel acceptance |
| Accepted input | A Port Event or ControlEvent whose acceptance commit has occurred |
| Message | An immutable deterministic fact produced and consumed entirely within one application turn |
| Port Command | An immutable request for an effect through one logical Port; it is not proof that the effect occurred |
| ControlCommand | An immutable deterministic request to the ControlPlane; it is not proof that the lifecycle operation completed |
| Component | Deterministic application logic with private state and typed callbacks; Components may produce declared outputs |
| Reducer | A restricted stateless callback that alone may mutate canonical `AppState` and produces no output |
| `AppState` | The application's one concrete canonical shared-state value |
| Component-private state | Deterministic state owned by one Component instance and inaccessible to other Components |
| Turn | One accepted input plus all causally produced Messages processed to quiescence |
| Quiescence | The point at which the current turn's Message FIFO is empty and no callback is active |
| Queue admission | The pre-acceptance commitment that a Port Event has entered its bounded Environment FIFO |
| Acceptance commit | The linearization point that fixes an accepted input's Event index, acceptance time, source, and root causation before dispatch |
| Logical Port identity | Stable application-graph identity retained across explicit restart |
| Implementation-unit identity | Identity of an Environment-owned worker, task host, process proxy, or simulated model |
| Port incarnation identity | Identity of one lifecycle episode of one logical Port |
| Endpoint | The Environment binding through which one logical Port reaches an implementation unit or model |
| Model | A domain-defined deterministic external-world state machine held by the Simulation Environment |
| Domain time | Application time carried in a payload, such as exchange event time |
| Port-observed time | Optional payload or operational metadata describing when a Port observed something; not Kernel acceptance time |
| Acceptance time | Time frozen by input acceptance and exposed as the turn's `ctx.now()` |
| Logical time | The root accepted input's acceptance time, shared by the complete turn |
| Wall time | OS civil time used by live infrastructure and never directly exposed to Components or Reducers |
| Virtual time | Simulation-controlled time used as acceptance time for simulated inputs |
| Event index | The authoritative monotonic total order of accepted Port Events and ControlEvents |
| Turn action sequence | Deterministic order of callbacks, Messages, and Commands within one turn |
| Diagnostic sequence | Recorder observation order; not application execution order |
| Business ID | Application-domain identity such as a client order ID; never replaced by Event, causal, control, or diagnostic identities |
| Fatal establishment | The monotonic point after which the Engine is poisoned and Kavod provides no further application-execution guarantee |

## 4. Settled Invariants

1. One kernel thread executes all Reducer and Component callbacks for one Engine run.
2. One Engine owns all deterministic application state; no process-global mutable state may affect deterministic behavior.
3. Every accepted input creates exactly one turn.
4. Accepted inputs execute in Event-index order, one complete turn at a time absent fatal establishment.
5. Reducers run before ordinary Components for each delivered Port Event, ControlEvent, or Message.
6. Only Reducers mutate canonical `AppState`.
7. Components mutate only their own private state and receive immutable canonical-state access.
8. Reducers have no private mutable state, behavior-affecting mutable captures, or output capability.
9. Messages propagate breadth-first through one FIFO and never dispatch recursively.
10. Port Commands and ControlCommands remain deferred until turn quiescence.
11. Actual callback registrations and callback-local production declarations are the graph's executable source of truth.
12. The application graph and Environment bindings are validated and frozen before `Ready`.
13. Components and Reducers receive no Engine, ControlPlane, Environment, Port, scheduler, executor, channel, worker, task, process, wall-clock, or external-IO handle.
14. Port Event, ControlEvent, Message, Port Command, and ControlCommand payloads are immutable after their semantic production or acceptance boundary.
15. Live queue admission is not input acceptance.
16. Event index, not timestamp, establishes accepted-input order.
17. No accepted Port Event is silently removed, rewritten, reprioritized, or invalidated by later stop, quarantine, worker failure, or restart.
18. Kavod never silently retries a rejected or ambiguous Port Command.
19. A contained Port-local failure quarantines affected logical Ports; it does not automatically poison the Engine.
20. An uncontained Kernel, ControlPlane, Acceptor, global scheduler, required-diagnostics, or global Environment failure establishes fatal state.
21. No Port restarts automatically. Restart requires an accepted application ControlCommand and a new incarnation.
22. Diagnostics have no application, lifecycle, delivery, restoration, or recovery authority.
23. Live and simulation share application semantics and logical protocols, not physical runtime behavior.
24. Fatal state is permanent. The Engine never resumes and its partial state is not reusable.

## 5. Explicit Non-Goals And Deferred Capabilities

The MVP does not provide or promise:

- Deterministic behavior before input acceptance in live mode.
- Deterministic live Ports, networks, brokers, OS scheduling, or external effects.
- Full deterministic simulation testing, generalized schedule exploration, fault tapes, shrinking, or production-adapter DST.
- Cross-build replay compatibility.
- State snapshots, restoration, migration, generic serialization, or state hashes.
- Engine recovery or continuation after fatal failure.
- A durable Command outbox, resend, exactly-once delivery, or external-effect rollback.
- Runtime graph mutation or dynamically introduced logical Ports.
- Fine-grained dynamic routing, multicast Port Commands, or arbitrary predicate subscriptions.
- Generic application phases, state-settled callbacks, joins, watermarks, or Reducer-produced Messages.
- Generic domain concepts such as order, fill, disconnect, gap, venue, reconciliation, or safe-to-trade state.
- Kavod-defined automatic Port restart, reconnect, reconciliation, arming, cancel, flatten, or business-safe shutdown.
- Safe forced termination of arbitrary in-process threads.
- Finalized async-task, pool, process-proxy, or grouped live implementation mechanics.
- Finalized public Rust traits, contexts, builders, error enums, identity representations, or queue types.
- A standardized disk diagnostic encoding, retention policy, or durability boundary.

Historical deterministic simulation is in scope. Full DST and replay execution are not prerequisites for the MVP determinism claim.

## 6. Four Communication Planes

Kavod separates four planes because they carry different authority.

### 6.1 Application Data Plane

```text
Port      -- Port Event   --> Reducer or Component
Component -- Message     --> Reducer or Component
Component -- Port Command --> Port
```

This plane contains application protocols, `AppState`, Component-private state, and deterministic application decisions.

### 6.2 Engine Control Plane

```text
ControlPlane -- ControlEvent   --> Reducer or Component
Component    -- ControlCommand --> ControlPlane
```

This plane communicates deterministic lifecycle facts and requests without exposing ControlPlane authority to callbacks.

### 6.3 Supervision Plane

The Environment and implementation units privately report startup, stop, exit, panic, cancellation, cleanup, and join results to the ControlPlane. Supervisor reports are not accepted inputs and are never delivered directly to application callbacks.

The ControlPlane classifies a supervisor report as either:

- A contained transition that changes runtime authority and later produces one or more normally ordered ControlEvents.
- An Engine-global failure that establishes fatal state without starting another application turn.

### 6.4 Observability Plane

Diagnostics receives automatic audit records and write-only user logs. Metrics and OpenTelemetry are projections from operational activity. None is an application or lifecycle control channel. A configured required-record failure may nevertheless poison the Engine at its defined boundary.

## 7. Communication And Authority Matrix

| Participant | May send | May receive | Owns or authoritatively decides | Must not do |
|---|---|---|---|---|
| Embedding host | Cooperative request, authoritative technical stop, process termination | Technical status and terminal outcome | Process-level authority and whether the Engine process continues | Pretend a cooperative request is immediate preemption |
| Engine | Internal coordination and terminal outcome | Construction inputs and subsystem reports | One run and subsystem composition | Expose internal machinery as callback capabilities |
| Kernel | Accepted-input dispatch and deterministic output batches | Accepted Port Events and ControlEvents | Turn execution, Message FIFO, callback order, deterministic application state | Perform Port IO or technical lifecycle operations |
| ControlPlane | ControlEvents and backend lifecycle operations | Host requests, ControlCommands, supervisor reports | Engine and logical-Port technical lifecycle, quarantine, incarnation, fatal classification | Masquerade as a Port or let application projections replace its authority |
| Environment | Port Event offers, supervisor reports, physical operations | Port Commands and ControlPlane backend operations | Bindings, placement realization, queues, scheduler, implementation resources, cancellation, and joining | Read or mutate application state |
| Live Port implementation | Port Event offers and private supervisor reports | Port Commands and backend lifecycle operations | Its external-system adapter state and transitive child work | Read application state or invoke the Kernel or another Port directly |
| Simulated model | Staged Port Events, wakes, cancellations, and private supervisor reports | Port Commands and backend lifecycle operations | Its domain-defined external-world state | Read application state or invoke the Kernel or another model directly |
| Reducer | No semantic outputs | Port Events, ControlEvents, Messages, mutable `AppState` | One canonical-state transition while invoked | Retain state references, perform IO, or hide mutable state |
| Component | Messages, Port Commands, ControlCommands, user logs | Port Events, ControlEvents, Messages, immutable `AppState` | Its private state and deterministic application decisions | Mutate `AppState` or access runtime mechanisms |
| Diagnostics | Operational failure reports | Automatic records and user logs | Recording policy and configured acknowledgement behavior | Supply business facts, state, lifecycle authority, or delivery permission |

The Environment is both the ControlPlane's execution backend and the holder of Port-boundary mechanisms. This is not circular: the ControlPlane decides semantic authority; the Environment realizes physical operations and reports results; the Kernel processes only accepted application-visible facts.

## 8. Protocol Semantics

### 8.1 Closed Typed Protocols

An application supplies closed concrete Port Event, Message, and Port Command protocols. Kavod supplies a closed core ControlEvent and ControlCommand protocol. Callbacks receive concrete typed inner payloads, never a top-level enum, `dyn Message`, `Any`, or a user-visible downcast.

Conceptually:

```rust
enum AppEvent {
    Market(MarketEvent),
    Execution(ExecutionEvent),
    Timer(TimerEvent),
}

enum AppMessage {
    BarsClosed(BarsClosed),
    TargetPosition(TargetPosition),
}

enum AppCommand {
    Execution(ExecutionCommand),
    Timer(TimerCommand),
}
```

Kavod may use narrow internal erasure to store heterogeneous callbacks. That choice must not weaken typed registration or expose downcasting to application code.

The core control protocol has a closed semantic operation set even though exact variant spelling is deferred. ControlCommands cover:

- Start one declared logical Port with requested placement.
- Stop one logical Port.
- Restart one safely `Failed` logical Port with a new incarnation.
- Request normal Engine completion after application-managed Port shutdown.

ControlEvents cover:

- Structurally first `Ready`.
- Start, stop, quarantine, quiescence, and restart outcomes for one logical Port and incarnation.
- Rejection of invalid, duplicate, or unsupported lifecycle requests.
- Cooperative host requests admitted for application policy.
- One not-delivered consequence for each rejected Port Command while acceptance remains open.

Every lifecycle outcome identifies the logical Port, operation, incarnation where allocated, and success, rejection, or failure reason needed to interpret it. Multi-endpoint failure consequences also identify their common failure sequence. Terminal Engine completion, Engine-global fatal failure, and authoritative host-stop disposition are host outcomes or audit facts, not ControlEvents.

### 8.2 Port Event

A Port Event is a fact observed outside the deterministic application graph. Only a Port endpoint emits a Port Event. Examples include a live quote, historical market occurrence, broker response, timer firing, or service result.

Port Events may carry domain timestamps and business IDs as ordinary payload fields. The Kernel does not reorder them by domain time.

### 8.3 ControlEvent

A ControlEvent is an application-visible technical control fact from the ControlPlane. Examples include `Ready`, technical Port-start outcome, quarantine notification, quiescence, stop outcome, a rejected lifecycle request, a cooperative shutdown request, or a Port Command not-delivered disposition.

ControlEvents use the same acceptance, Event-index, turn, Reducer-before-Component, frozen-time, and causal semantics as Port Events. Fatal failure and terminal Engine completion are not ControlEvents because application execution no longer has authority to react to them.

### 8.4 Message

A Message is a deterministic fact derived inside the application graph. Only ordinary Components produce Messages. Messages remain inside the current turn, inherit its logical time, and never cross a Port or ControlPlane boundary.

Future work is not represented by scheduling a Message. It uses a Port protocol, such as:

```text
SetTimer Port Command -> Timer Port -> TimerFired Port Event
RunInference Port Command -> Inference Port -> InferenceCompleted Port Event
```

### 8.5 Port Command

A Port Command is a directed request to one logical Port. It does not prove receipt, execution, completion, or external effect. Only ordinary Components produce Port Commands, and only the addressed Port endpoint receives them.

Externally consequential protocols must carry application-owned deterministic business identity or idempotency information sufficient for their external reconciliation and duplicate-effect policy. Kavod's run-scoped causal identities do not satisfy that obligation.

### 8.6 ControlCommand

A ControlCommand is a deterministic request to the ControlPlane. It may request start, stop, explicit restart, placement, or normal Engine completion. Production of a ControlCommand is not completion of the operation. Completion or rejection returns later as a ControlEvent, except successful Engine completion, which returns a terminal outcome to the host.

### 8.7 Illustrative Port Contract

The shared Port abstraction is logical rather than executable:

```rust
trait PortSpec {
    type Command;
    type Event;
}

struct Execution;

impl PortSpec for Execution {
    type Command = ExecutionCommand;
    type Event = ExecutionEvent;
}
```

This example does not settle trait bounds or enum composition. A Port Spec does not define a worker, model, thread, task, process, mailbox, lifecycle policy, or implementation state.

## 9. Application Graph

### 9.1 Executable Source Of Truth

The graph is derived from executable registration metadata:

```text
Port         -- Port Event     --> callback
ControlPlane -- ControlEvent   --> callback
callback     -- Message        --> callback
callback     -- Port Command   --> Port
callback     -- ControlCommand --> ControlPlane
```

Registering a callback creates an input edge. A callback-local production declaration creates an output edge. A declaration means **may produce**; Kavod does not infer a guarantee that arbitrary callback code will produce an output.

Every runtime output must be authorized by the executing callback's declarations. Producing an undeclared Message, Port Command, or ControlCommand is an invariant violation.

### 9.2 Construction Stages

Application and Environment construction are separate:

```text
Application construction
    -> protocols, AppState type, Ports, callbacks, declarations, stable order

Environment construction against the Application
    -> endpoint bindings, supported placement, capacities, policies, models

Engine construction
    -> Kernel, ControlPlane, diagnostics, queues, scheduler, inert bindings
```

### 9.3 Validation

Before execution, construction must validate at least:

- Every declared Port Event flow has a matching callback.
- Every declared Message production has a matching callback.
- Every Port Command production targets one declared logical Port.
- Every ControlCommand production names a supported core operation and valid logical target where applicable.
- Every callback input and output belongs to the appropriate closed protocol.
- Every callback's production declarations are internally valid.
- Reducer and Component registration and fan-out order are stable.
- One initial `AppState` value is supplied with the required concrete type and passes application validation when configured.
- Every declared logical Port has exactly one compatible endpoint binding.
- No binding exists for an undeclared Port.
- Binding capacities, Event-full policies, placement capabilities, and simulation normalization are explicit and valid.
- Grouped model endpoints are unique and refer only to declared logical Ports.

The graph may report potential Message cycles because declarations mean “may produce.” Cycles are not automatically invalid. Mandatory runtime turn bounds prevent an actual cycle from running forever.

### 9.4 Immutability

The application graph, callback order, Port declarations, and endpoint bindings are frozen before `Ready`. The MVP permits no runtime callback registration, subscription mutation, or new logical Port introduction.

## 10. Application State And Callback Authority

### 10.1 State Classes

Kavod has two deterministic application-state classes:

1. One application-defined concrete canonical `AppState`.
2. Component-private state owned by one Component instance.

Environment implementation state, simulated-model state, authoritative ControlPlane state, and Kernel ordering state are not application state.

| State | Logical owner | Physical holder | Mutation authority |
|---|---|---|---|
| Canonical `AppState` | Application | Engine | Reducers |
| Component-private state | Component instance | Engine | Owning Component callbacks |
| ControlPlane lifecycle state | ControlPlane | Engine | ControlPlane |
| Live implementation state | Port implementation | Environment implementation unit | Owning implementation |
| Simulated external-world state | Model | Simulation Environment | Owning model callbacks |
| Kernel order and causation state | Kernel | Engine | Kernel |

### 10.2 Canonical `AppState`

`AppState` is one concrete typed root, not a `TypeId` cache, service locator, state-slot registry, or set of projector-owned containers. Dynamic entities such as orders, positions, instruments, bars, accounts, and reconciled observations are ordinary application-defined fields, maps, and collections inside it.

Only Reducers receive mutable access. Ordinary Components may read the complete immutable root. `AppState` must not use interior mutability to bypass Reducer-only mutation.

State fields and dynamic entities are not application-graph nodes. The graph records payload-to-callback edges and broad canonical read or mutation capability, not individual field reads and writes. Kavod must not present unenforced field-dependency annotations as authoritative metadata.

Applications may project ControlEvents into an application runtime view such as logical-Port status. That projection is deterministic decision state and may lag the ControlPlane. It never replaces authoritative runtime state.

### 10.3 Component-Private State

One Component instance may share its private state across its own registered callbacks. Other Components cannot access it. If independently registered Components require shared access, that information belongs in `AppState` and changes through Reducers.

Examples include strategy indicators, finite-state machines, signal accumulators, and in-progress bar builders.

### 10.4 Reducers

A Reducer:

- Consumes one typed Port Event, ControlEvent, or Message.
- Receives mutable access to the complete `AppState`.
- May read and change multiple related fields in one callback.
- Has no private mutable state or behavior-affecting mutable capture.
- Emits no Message, Port Command, or ControlCommand.
- Performs no external IO or blocking work.
- Retains no `AppState` reference after return.
- Has no generic application result or error channel.

Several Reducers may consume the same payload and run in stable registration order. This guarantees reproducibility, not semantic independence. Fields that must remain coherent for one input must be transitioned in one cohesive Reducer callback, normally through one cohesive `AppState` operation.

Reducer completion gives callback-isolated visibility, not transactionality. No callback observes a Reducer halfway through, but a panic after partial mutation provides no rollback. Fatal establishment ends the run and the partial state is not reusable.

### 10.5 Components

An ordinary Component callback may:

- Read immutable `AppState`.
- Mutate its own private state.
- Read the root input's frozen logical time.
- Emit declared Messages, Port Commands, and ControlCommands.
- Emit write-only user logs.

It must remain synchronous, nonblocking, and deterministic. It has no generic application result or error channel; expected outcomes use typed state, Messages, Port Commands, or ControlCommands. Heavy or external work belongs behind a service Port.

Components and Reducers must not observe the selected Engine mode. Environment selection changes boundary mechanisms, not deterministic application branching.

Illustrative callback capabilities might look like:

```rust
fn on_signal(
    state: &mut StrategyState,
    ctx: &mut ComponentCtx<AppState>,
    signal: &Signal,
) {
    let portfolio = &ctx.state().portfolio;
    ctx.message(TargetPosition::from(signal, portfolio));
    ctx.command::<Execution>(ExecutionCommand::Submit(...));
}
```

The exact arguments and methods are deferred. The capability separation is normative.

## 11. Normative Turn Lifecycle

One accepted input creates one isolated turn:

1. Acceptance commits the input's Event index, acceptance time, logical source, and root causation.
2. The Kernel creates callback contexts containing the frozen logical time and permitted capabilities.
3. All matching Reducers run in stable registration order.
4. After all matching Reducers return normally, all matching Components run in stable registration order.
5. Messages produced by Components append to one turn-local FIFO in production order.
6. The Kernel removes the next Message from the FIFO and repeats Reducer-then-Component dispatch for that payload.
7. Port Commands and ControlCommands accumulate in deterministic production order and do not take effect during callbacks.
8. When the Message FIFO is empty and no callback is active, the turn reaches quiescence.
9. The runtime finalizes deterministic state and causal output metadata.
10. The runtime accounts for Port Command publication or rejection.
11. Only after every Port Command has an accounted disposition does the ControlPlane apply ControlCommands in deterministic production order.
12. The next accepted input may begin only after post-turn handling completes, absent fatal establishment.

Message propagation is breadth-first and non-reentrant. Internal Messages do not advance logical time or create new Event indices.

Configured finite limits are mandatory for at least:

- Messages per turn.
- Callback invocations per turn.
- Combined Port Commands and ControlCommands per turn.

Exceeding a configured turn limit is an expected terminal run error identifying the limit. It is not an invariant panic, and no later accepted input is processed.

## 12. Derived-State Consistency

Reducer-before-Component visibility is local to the currently delivered payload. A Component handling payload `X` sees every canonical transition registered directly on `X`. It does not necessarily see state changes caused by sibling or descendant Messages still waiting in the FIFO.

Turn quiescence is therefore not a retrospective state barrier. A Port Command created from stale state is not recomputed, refreshed, or withdrawn merely because later Messages change that state before publication.

The MVP rule is:

> If one decision depends on several related changes caused by one input, the producer must represent the complete required set as one delivered Port Event or Message, the canonical fields that must remain coherent must be transitioned within one cohesive Reducer callback, and the decision callback must consume that complete fact.

For multi-timeframe bars:

```text
Tick
  -> BarAggregator updates all private builders
  -> BarsClosed([1m, 5m, 15m, daily])
  -> one cohesive Reducer projects every required completed bar
  -> Strategy consumes BarsClosed once against the coherent projection
```

For each input, the aggregator emits exactly one nonempty `BarsClosed` when one or more bars close and emits none when no bar closes. The aggregate contains every closure caused by that input within its declared scope, including every consecutive closure required by the application's gap policy, exactly once and in stable application-defined order.

The aggregate is complete only within its declared application-domain scope. Kavod cannot inspect arbitrary payloads or field reads to prove truthfulness. Aggregate completeness, stable ordering within the payload, and correct consumer wiring are application obligations verified through domain tests and review.

Equal domain or acceptance timestamps do not combine separate inputs or Messages into an atomic group. External observations requiring atomic application treatment must be offered as one application-defined batch Port Event.

## 13. Determinism Boundary

Kavod's deterministic claim is:

> Given the same executable Kavod/application build, frozen application graph and registration order, initial `AppState`, initial Component-private state, determinism-affecting application and Engine configuration, and the same accepted Port Event and ControlEvent sequence with identical payloads, source identities, Event order, and acceptance times, the Kernel executes callbacks in the same order and produces the same ordered Messages, Port Commands, ControlCommands, private-state transitions, and canonical-state transitions after each completed turn.

“Same build” means the same executable behavior, not merely the same source revision or version label. Kavod does not claim compatibility between different builds.

### 13.1 Deterministic Inputs

- Executable Kavod/application build.
- Frozen graph and stable registration order.
- Initial `AppState`.
- Initial Component-private state.
- Determinism-affecting application configuration.
- Determinism-affecting Engine configuration, including turn bounds and required-record policy.
- Ordered accepted Port Events and ControlEvents.
- Payload, source identity, Event index, and acceptance time of each accepted input.
- Any future explicitly approved deterministic capability input.

### 13.2 Deterministic Outputs

- Callback execution and delivery order.
- Ordered Messages.
- Ordered Port Commands and logical destinations.
- Ordered ControlCommands and operations.
- Component-private state transitions.
- Canonical `AppState` transitions.
- Application state after each completed turn.
- Kernel-derived causal relationships when recorded.

### 13.3 Conditional Application Contract

Ordinary Rust callback code is not sandboxed. Components and Reducers must not let behavior depend on wall-clock reads, OS entropy, IO, environment or process state, threads, task completion order, process-global mutable state, Port implementation state, or unstable collection iteration.

Kavod's capability boundary prevents it from supplying those observations directly. Testing, linting, dependency review, and code review remain necessary because the type system cannot prove arbitrary Rust code deterministic.

### 13.4 Explicit Non-Guarantees

Kavod does not guarantee:

- Which live Event wins an ingress race.
- Identical accepted input sequences from nominally identical live conditions.
- Identical physical latency.
- External Port Command delivery, completion, exactly-once effect, or cross-Port atomicity.
- Cross-platform numeric equivalence unless separately constrained and tested.
- Determinism from application code that violates the contract.
- Completion of an active turn after fatal establishment.
- A reusable or internally consistent state value after fatal establishment.
- Stable panic text, backtraces, OOM behavior, signals, or hardware failure behavior.

## 14. Time, Acceptance, And Causal Order

### 14.1 Acceptance

Port emission, ControlPlane production, queue insertion, and selection are not acceptance. The single acceptance operation:

1. Validates logical source and protocol membership.
2. Prepares the candidate Event index.
3. Freezes acceptance time.
4. Establishes root causation.
5. Executes the configured acceptance recording protocol.
6. Commits before any application callback receives the input.

The acceptance commit is the semantic linearization point.

Under best-effort recording, acceptance commits first and recording is attempted before processing. Recording failure does not undo acceptance or prevent processing.

Under required recording, the writer crossing its declared acknowledgement boundary for the complete `EventAccepted` record constitutes acceptance commit. Failure before commit means no dispatch and poisons the Engine. If acknowledgement committed but the Engine failed before observing it, diagnostics may contain a truthful accepted-but-unprocessed input.

An accepted record proves acceptance only. It does not prove callback execution, turn completion, external truth, or recoverability.

### 14.2 Time Rules

- Domain time remains ordinary payload data.
- Port-observed time, when useful, remains payload or operational metadata and never substitutes for acceptance time.
- Every callback, Message, Port Command, and ControlCommand in one turn inherits the root input's logical time.
- Internal Messages do not advance time.
- Event index orders accepted inputs even when acceptance timestamps are equal.
- Turn action sequence orders deterministic work within one turn.
- Diagnostic sequence orders recorder observations and may include racing live records.

### 14.3 Live Acceptance Time

At run initialization, the live Environment samples one wall-clock anchor and one monotonic-clock anchor. Live acceptance time is the wall-clock anchor plus monotonic elapsed time.

This value is nondecreasing within the run. Equal values are legal. Later civil-clock or NTP corrections do not move `ctx.now()` backward or rewrite the timeline. The resulting time may drift from current civil UTC.

### 14.4 Simulation Time

The Simulation Environment owns virtual time. It advances only when selecting the next scheduled action and uses that action's virtual time as acceptance time for an emitted Port Event or ControlEvent.

## 15. Logical Ports And Implementation Ownership

A logical Port is application topology, not an object, worker, model, queue, or state owner. Every declared logical Port resolves to exactly one endpoint binding in an Environment.

Live and simulation share:

- Logical Port identity.
- Port Command and Port Event protocol meaning.
- Application graph routing and source attribution.
- Technical lifecycle request and outcome meanings.
- Kernel turn semantics.
- Per-Port Command production order.
- No-reentrancy and no-silent-loss obligations.

They may differ in implementation interfaces, topology, physical placement, admission races, lifecycle timing, backpressure manifestation, and failure sequence.

One implementation unit may provide several logical endpoints. A simulated model identity is an implementation-unit identity. There is no application-visible grouped Port identity.

Each logical endpoint retains its own:

- Logical Port identity.
- Current incarnation identity.
- Port Command routing and Port Event source attribution.
- Admission and handoff authority.
- Technical lifecycle and quarantine state.

Model or implementation grouping remains private Environment topology and may appear in diagnostics.

The ownership hierarchy is:

| Subject | Created by | Authoritative owner | Closed, fenced, or destroyed by |
|---|---|---|---|
| Logical Port identity | Application construction | Frozen application graph | Engine-run termination; retained across restart within the run |
| Endpoint binding | Environment construction | Environment | Environment cleanup at run termination |
| Implementation-unit identity and resources | Environment | Environment | Environment supervision and joining |
| Simulated model identity and state | Simulation Environment construction | Domain-defined model, physically held by Environment | Simulation cleanup |
| Port incarnation identity | ControlPlane on accepted start/restart operation | ControlPlane | ControlPlane authority closure plus Environment termination or fencing |
| Port Event admission authority | ControlPlane, realized by Environment | Current logical-Port incarnation | Stop, quarantine, host stop, or fatal closure |
| Port Command handoff authority | ControlPlane, realized by Environment | Current logical-Port incarnation | Stop, quarantine, host stop, or fatal closure |

The Environment may hold these resources without understanding domain model state. The ControlPlane may govern lifecycle without owning workers, model storage, queues, or external resources.

## 16. Live Event Admission, Acceptance, And Fairness

### 16.1 Per-Binding Queues

Each live logical Port binding owns one bounded FIFO Port Event queue for the run. Admission into it is gated by the current incarnation's authority.

Successful queue admission:

- Freezes payload, logical source, and source-incarnation authority.
- Makes the Port Event an irrevocable eligible future input.
- Does not assign Event index, acceptance time, or root causation.

Stop, quarantine, or worker exit closes new admission but does not remove or invalidate Port Events already admitted. FIFO preserves successful admission-commit order for one logical Port queue. Any stronger order among several internal Port producer tasks belongs to that Port's contract.

The ControlPlane has a separate bounded ingress. A supervisor or backend result queued there remains a provisional lifecycle outcome until converted into an accepted ControlEvent. A stale result may be invalidated and audited before acceptance. Once ControlEvent acceptance commits, it is immutable and processes normally.

Supervisor transitions and lifecycle outcomes in ControlPlane ingress may not be silently dropped or coalesced. Cooperative host requests may use an explicitly defined idempotent coalescing rule. If bounded ingress cannot preserve both an authoritative transition and its required application-visible consequence, the Engine must establish fatal state because lifecycle accounting is no longer trustworthy.

### 16.2 One Acceptor

One logical Acceptor owns live accepted-input sequencing. After structurally first `Ready`, it visits the ControlPlane ingress and every Port Event queue in one frozen source order using one configured global quantum `Q`.

For each round:

1. Visit sources in frozen order.
2. At each source, observe entries waiting when that visit begins.
3. Select, accept, and process at most `Q` eligible entries in FIFO order.
4. Entries offered during that visit wait for a later round.
5. Move on immediately when the source is empty.
6. Begin another round after all sources have been visited.

For each selected input, acceptance commits and the complete turn runs before the Acceptor selects another input. It does not accept an entire quantum ahead of execution.

The quantum bounds consecutive input count, not wall-clock latency. A long synchronous callback can still delay every source.

### 16.3 Capacity And Full Policy

Every live binding must configure Port Event capacity and a full policy explicitly. Every supported policy must be bounded, observable, and consistent with no silent loss. The exact MVP policy names and default are deferred to the public binding pass.

A policy acts before queue admission. It must not remove, replace, reprioritize, or coalesce an already admitted Port Event. Domain-aware batching, replacement, deduplication, or coalescing may occur inside a concrete Port before it offers an application-defined Port Event, when that Port protocol makes the behavior explicit.

Every offer receives an authoritative admitted or not-admitted outcome. A full or closed outcome leaves the payload outside Kavod and visible to the Port implementation and runtime metrics. Kavod core does not retry it. The selected binding policy must define whether the implementation waits cancellably before offering, treats refusal as contained Port failure, or uses another supported bounded pre-admission behavior. A concrete Port remains responsible for external-protocol consequences of an observation it read but could not admit.

### 16.4 No Priority

The MVP exposes no Event or Message priority, generic scheduling-priority trait, or Critical/Normal admission class. Per-Port capacity isolates one Port's memory use; Acceptor rounds provide source fairness. Payload importance cannot create capacity that is already occupied.

## 17. Port Command Publication And Backpressure

Each live Port binding owns one bounded FIFO Port Command mailbox. The Kernel never blocks waiting for mailbox capacity.

After turn quiescence:

1. Finalize deterministic state and causal output metadata.
2. Classify every Port Command against authoritative ControlPlane state.
3. Reject Commands whose destinations are not currently available.
4. Group remaining Commands by destination while retaining each Command's global production ordinal.
5. Visit destinations in stable logical-Port order and reserve each complete destination batch all-or-none.
6. Before each reserved Command becomes visible, revalidate and atomically commit handoff to the exact target incarnation.
7. Release reservations for failed revalidation and mark those Commands not delivered.
8. After all Port Command dispositions are known, commit rejection consequences in original global Command production order.
9. Apply ControlCommands in their deterministic production order.

### 17.1 Dispositions

While the Engine remains trustworthy, every Port Command reaches one accounted Kavod disposition:

- **Handed off:** crossed the logical Port boundary to one incarnation.
- **Not delivered:** did not cross because the destination was unavailable, mailbox capacity was insufficient, authority closed, or another nonfatal publication rule rejected it.

If publication bookkeeping cannot establish a disposition, that accounting failure establishes fatal state. Kavod then makes no false claim that the Command was handed off or not delivered; available diagnostics record the unresolved boundary.

A nonfatal not-delivered disposition creates exactly one later `CommandNotDelivered` ControlEvent while application-input acceptance remains open. The event carries an unambiguous run-scoped reference to the produced Command, its destination, and its reason. A root Event index plus turn-wide production order is conceptually sufficient; exact representation is deferred. This technical reference is not a business ID.

Fatal closure or authoritative host-stop closure uses a terminal audit disposition instead of admitting a new feedback ControlEvent.

### 17.2 Capacity

If one destination lacks capacity:

- None of that destination's batch crosses the boundary.
- Every Command in that batch receives a not-delivered disposition.
- Healthy destinations remain independently eligible.
- No rejected Command is retained or retried.

There is no cross-Port publication transaction.

### 17.3 Handoff Linearization

Classification and reservation are preparatory; they do not grant handoff authority. Final handoff, quarantine, `StopPort` authority closure, and authoritative host-stop closure must serialize against the same target-incarnation authority.

- If handoff commits first, the Command belongs to that incarnation and may become externally ambiguous after failure.
- If quarantine or Port stop commits first, the Command does not cross and receives a not-delivered consequence while acceptance remains open.
- If authoritative host stop commits first, the Command does not cross and receives terminal host-stop audit disposition.

Crossing the Port boundary does not guarantee external receipt, execution, completion, or exactly-once effect. Kavod never silently resends an ambiguous Command.

## 18. Simulation Architecture

### 18.1 Model Contract

A simulated model is a domain-defined, Environment-held synchronous deterministic state machine. It must:

- Produce the same state transitions and staged outputs for the same model state, input, virtual time, configuration, and approved deterministic choices.
- Run one callback at a time on the simulation thread.
- Use only virtual time supplied by the Environment.
- Consume Port Commands through registered endpoints.
- Emit Port Events only through registered endpoints.
- Schedule and cancel only through a restricted model context.
- Avoid wall time, OS IO, OS entropy, task scheduling, process-global mutable state, and behaviorally observed unstable iteration.
- Never borrow application state or invoke the Kernel, another model, or another Port directly.
- Never retain its callback context after return.

Model state represents the simulated external world and is not `AppState`.

### 18.2 Independent And Grouped Models

An independent model may provide one endpoint, such as a Timer. A grouped model may provide several endpoints when those endpoints must share one coherent external-world state.

For example, one simulated venue may own historical source cursors, book state, resting orders, queue position, fill state, and latency policy while exposing separate MarketData and Execution endpoints.

The Environment stores the model once and serializes its callbacks. Endpoints do not communicate with each other; Commands and scheduled actions route to their common owner. Hidden shared global state, `Arc<Mutex<World>>`, or `Rc<RefCell<World>>` between separately registered models is not a valid substitute for declared grouped ownership.

Illustrative composition:

```rust
SimulationEnvironment::builder()
    .bind_model(SimulatedVenue::new(history, config), |model| {
        model.endpoint::<MarketData>();
        model.endpoint::<Execution>();
    })
    .bind::<Timer>(SimulatedTimer::new())
    .build(&application)?;
```

### 18.3 Scheduling And Reentrancy

The Simulation Environment owns one global future-action queue ordered by:

```text
(virtual_time, global_schedule_ordinal)
```

The global ordinal is allocated when a staged operation commits. At equal virtual time, existing actions precede newly committed actions.

The scheduler:

1. Pops the next action.
2. Advances virtual time to that action's time.
3. Runs its model, lifecycle, wake, or accepted-input action to completion.
4. Commits staged outputs in production order after callback return.
5. If a Port Event or ControlEvent action is selected, accepts it and runs its Kernel turn to quiescence.
6. Classifies every Port Command against authoritative lifecycle state and records unavailable-destination consequences.
7. Delivers eligible Port Commands to simulated endpoints synchronously in global Command production order. Each endpoint callback returns and commits staged outputs before the next Command is delivered.
8. Commits all not-delivered ControlEvent actions in original global Command production order.
9. Applies ControlCommands in deterministic production order only after every Port Command has an accounted disposition.
10. Commits resulting lifecycle ControlEvents as later scheduled actions with global schedule ordinals.
11. Returns to scheduler selection.

No model callback, Reducer, Component, or later model callback overlaps. A zero-latency emission receives a later same-time schedule ordinal; it does not recursively enter the Kernel and no artificial time epsilon is added.

Simulation does not use live mailbox-capacity reservation for ordinary historical execution. A Port Command delivered to a simulated endpoint has crossed that simulated Port boundary. Fault-oriented simulation of live mailbox pressure is deferred.

### 18.4 Prefix Causality And Anti-Look-Ahead

Deterministic simulation must also be causally truthful. A historical source/model:

- Uses only its next due time to arrange its next wake.
- Makes a market occurrence world-visible only when that occurrence's scheduled action runs.
- Consumes one occurrence or one explicitly atomic occurrence batch per source action.
- Applies the occurrence to external-world model state before staging its corresponding public Port Event.
- Stages that public Port Event before arranging a subsequent same-time occurrence when Port Command interposition must remain possible.
- Must not let future source records influence current state, output, latency, or effect decisions.

The safe conceptual order is:

```text
wake for next occurrence
-> consume exactly the due occurrence or declared batch
-> update external-world state
-> stage corresponding public Port Event
-> arrange the next occurrence
```

Applying a later occurrence before publishing an earlier one is invalid because a Port Command caused by the earlier public Event could then observe undisclosed future state. Kavod cannot prove arbitrary model code obeys prefix causality; source utilities, model-specific invariants, review, and tests provide assurance.

### 18.5 Cancellation And Failure

Pending wakes and model callbacks identify the endpoint incarnation or endpoint-incarnation set whose authority they require. Failure or stop cancels affected pending non-Event work. Already committed Port Event-delivery actions are admitted observations and survive to ordinary selection.

Ordinary token cancellation succeeds only while its target remains pending. It becomes effective after the active callback returns and before the next scheduler selection. It cannot cancel an action already selected or completed and does not synthesize a Port Event unless the application protocol explicitly requires an acknowledgement.

Scheduling into the past is a terminal simulation causality error. Total model-callback/action limits and per-virtual-timestamp limits are finite; exhaustion returns a terminal error identifying the limit and virtual time rather than dropping work, advancing time, or inventing latency.

Model state is not rolled back after partial mutation. An endpoint-local failure may quarantine only that endpoint when the remaining shared model state is known to remain valid. A panic or invariant failure that may have compromised shared state affects every dependent endpoint. If the affected set cannot be identified or isolated, the failure is Engine-global fatal.

### 18.6 Source Exhaustion And Completion

Source exhaustion is not automatically simulation completion. Scheduled acknowledgements, fills, cancellations, or timers may remain.

The MVP supports:

- `UntilIdle`: success when every declared finite source is exhausted and no scheduled action remains.
- `Until(T)`: process actions through the inclusive horizon and stop before later work, reporting retained work.

Queue emptiness while a required finite source is not exhausted is `SimulationStalled`, not success. Eligible Port Event deliveries and lifecycle consequences block completion under the selected policy. Work after an `Until(T)` horizon is retained and reported but does not execute.

Technical simulation completion is a terminal host outcome, not a final application ControlEvent. An application that must react to end-of-data uses an ordinary application-defined Port Event before completion.

The terminal simulation outcome reports the last accepted Event index, final virtual time, exhausted finite sources, and a pending-work summary where applicable.

Finite total-action and same-virtual-time action bounds must prevent zero-latency runaway chains.

## 19. Ready-First Startup

Startup is:

1. Build and validate the application graph.
2. Build and validate all endpoint bindings, capacities, policies, placement capabilities, and simulation normalization.
3. Construct the Kernel, ControlPlane, diagnostics, queues, scheduler, and Environment backend.
4. Establish every logical Port as `Stopped`, with no active Port Event admission or Port Command handoff authority.
5. Accept `ControlEvent::Ready` as the first application input.
6. Run its complete turn.
7. Apply lifecycle ControlCommands after turn quiescence.
8. Start requested live implementations or activate requested simulated endpoints.
9. Convert backend outcomes into later ControlEvents.

`Ready` means only that the Engine can process application and control turns. It does not mean any Port is running, connected, authenticated, reconciled, or safe to use.

When a backend reports successful start:

1. The ControlPlane validates operation and incarnation identity.
2. It queues a provisional lifecycle outcome; it does not open ordinary traffic.
3. Failure or cancellation before acceptance may invalidate and audit that stale outcome.
4. Acceptance of `PortStarted` transitions that logical Port to `Running` for outputs of that turn.
5. `PortStarted` Reducers project status before matching Components react.
6. Port Commands from that turn may target the now-running incarnation.
7. Ordinary Port Event admission opens only after the complete `PortStarted` turn and only while the same incarnation remains `Running`.

A starting live implementation waits behind a cancellable ingress gate before reading or constructing ordinary application Port Events. Kavod provides no hidden pre-Running Port Event buffer.

Simulated activation likewise stages only its technical lifecycle result. It must not begin ordinary model activity, schedule ordinary endpoint wakes, or emit ordinary Port Events until the corresponding `PortStarted` turn completes and opens that endpoint's authority.

## 20. Technical Port Lifecycle

The logical lifecycle is:

```text
Stopped -- Start --> Starting -- PortStarted accepted --> Running
Starting -- backend failure --> Quarantined
Starting -- Stop/cancel startup --> Stopping
Running -- Stop --> Stopping -- PortStopped accepted --> Stopped
Running/Stopping -- local failure --> Quarantined
Quarantined -- termination or fencing
            + affected non-Event work cancelled
            + admitted Port Event work empty
            + PortQuiesced accepted --> Failed
Failed -- explicit Restart --> Starting with new incarnation
Failed -- explicit Stop --> Stopped
```

Meanings are normative:

| State | Meaning |
|---|---|
| `Stopped` | Bound but inactive; no new Port Event admission or Port Command handoff, and no admitted work remains |
| `Starting` | A start operation is in progress; ordinary traffic is disabled |
| `Running` | Technical implementation controls are installed; external readiness is not implied |
| `Stopping` | Technical stop is in progress; new admission and handoff are closed while admitted Port Events drain |
| `Quarantined` | New authority is revoked, but cleanup, child work, or admitted Port Events may remain |
| `Failed` | The old incarnation is terminated or fenced, affected non-Event work is cancelled, admitted Port Event work is empty, and replacement is safe |

Every accepted start or restart operation allocates an operation identity and a new incarnation before backend invocation. Every backend report carries both. Late, duplicate, or stale reports are audited and cannot mutate a newer operation or incarnation.

Repeated or illegal lifecycle requests are not silently ignored. They produce later typed rejection ControlEvents while acceptance remains open.

The minimum operation behavior is:

| State | Start | Stop | Restart |
|---|---|---|---|
| `Stopped` | Begin `Starting` | Reject already stopped | Reject; use Start |
| `Starting` | Reject duplicate | Cancel startup and begin `Stopping` | Reject |
| `Running` | Reject duplicate | Begin `Stopping` | Reject |
| `Stopping` | Reject | Reject duplicate | Reject |
| `Quarantined` | Reject | Continue cleanup without claiming `Stopped` | Reject until `Failed` |
| `Failed` | Reject; use Restart | Transition to `Stopped` without reviving old incarnation | Begin `Starting` with a new incarnation |

A successful backend stop result remains ControlPlane-private until owned child work is terminated or fenced and every Port Event admitted by that incarnation has drained. Only then is `PortStopped` queued. Acceptance of `PortStopped` establishes `Stopped` for outputs of that turn. A stop timeout or failure queues `PortStopFailed` and leaves the Port quarantined.

Requested placement is deterministic ControlCommand intent. Realized placement is a live Environment mechanism. Normalized placement is the Simulation Environment's deterministic mapping. Supported requests and normalization are frozen before `Ready`. Unsupported requests fail visibly; a live Environment does not silently fall back.

The protocol may describe conceptual requests such as threaded, asynchronous task, or process placement without requiring every MVP Environment to implement them. A basic simulation may normalize every supported request to its single deterministic model scheduler.

## 21. Port-Local Failure, Quarantine, And Restart

A failure is Port-local only when the Kernel, ControlPlane, Acceptor, diagnostics requirements, global Environment infrastructure, and every unaffected endpoint remain trustworthy.

Contained examples may include startup failure, unexpected worker return, captured worker panic, Port-local mailbox failure, prohibited Port-local overflow, owned child failure, or an isolatable simulated endpoint/model failure.

Port-local failure immediately:

1. Quarantines every affected logical Port.
2. Closes new Port Event admission, model scheduling, child creation, and Port Command handoff for each affected incarnation.
3. Does not preempt an active synchronous callback.
4. Allows the already accepted turn to reach quiescence.
5. Preserves every Port Event admitted before closure.
6. Queues the typed failure consequence appropriate to the operation, normally `PortFailed` and, for technical stop timeout or failure, `PortStopFailed`.
7. Begins or continues cleanup.

`PortFailed` reports that quarantine already revoked routing authority. It does not mean lifecycle state is yet `Failed`. `PortQuiesced` is queued only after termination or fencing, affected non-Event cancellation, and admitted Port Event drainage. Acceptance of `PortQuiesced` transitions the Port to `Failed` for outputs of that turn.

For a multi-endpoint implementation failure, the ControlPlane identifies affected logical endpoints privately. Live consequences enter the ControlPlane FIFO contiguously in stable logical-Port order; simulation consequences receive contiguous global schedule ordinals in the same order. Every consequence carries one common failure-sequence identity and the number still pending so application policy can determine when the complete affected set has been reported. Grouping never becomes an application Port.

Restart is always explicit deterministic application intent. It:

- Requires state `Failed` and no conflicting lifecycle operation.
- Retains logical Port identity.
- Allocates a new incarnation.
- Constructs or resets backend state according to the binding contract.
- Reuses no old admission authority.
- Retries or resends no old Port Command.
- Produces a later success or failure ControlEvent.
- Does not imply reconnect, reconciliation, state recovery, or arming.

No elapsed-time, configuration, supervisor, or Environment policy may restart a Port automatically.

## 22. Normal Application-Managed Shutdown

Normal shutdown is deterministic application policy followed by technical lifecycle operations:

```text
cooperative ShutdownRequested ControlEvent
  -> application disarms
  -> ordinary Port protocols cancel, flatten, or reconcile as required
  -> application emits StopPort ControlCommands
  -> PortStopped or PortStopFailed ControlEvents
  -> application emits StopEngine
  -> terminal outcome to embedding host
```

Technical `StopPort` does not cancel orders, flatten positions, stop subscriptions safely, reconcile external state, or perform any other domain action implicitly.

A normal `StopEngine` request succeeds only when:

- Every logical Port is `Stopped` or safely `Failed`.
- Every old-incarnation admitted Port Event set is empty.
- No lifecycle operation remains pending.
- No application turn is incomplete.
- No earlier ControlEvent consequence remains pending.
- The producing turn emits no Port Command requiring publication or feedback.
- `StopEngine` is the only lifecycle ControlCommand from that turn.

Otherwise the ControlPlane keeps running and produces a typed rejection ControlEvent.

After successful `StopEngine`, acceptance closes, final technical cleanup and diagnostics run, and the terminal result goes to the host. There is no final `EngineStopped` ControlEvent because application execution has ended.

## 23. Host Stop, Fatal Failure, And Process Termination

### 23.1 Host Authority

The embedding host has three distinct controls:

- A cooperative request that becomes a normally ordered ControlEvent.
- An authoritative technical stop that bypasses application approval and requests cleanup.
- External Engine-process termination for hard preemption.

An authoritative stop serializes against new Port Event admission, accepted-input acceptance, and final Port Command handoff.

- If Port Event admission commits first, that Event remains eligible and drains.
- If admission closure commits first, the offer remains outside Kavod.
- If acceptance commits first, that turn runs to quiescence and reaches accounted post-turn disposition.
- If Port Command handoff commits first, it remains handed off and may be externally ambiguous.
- If host stop commits first, no new Port Command crosses.

During authoritative drainage:

- No new Port Event, ordinary ControlEvent, rejection consequence, or lifecycle consequence is admitted.
- Port Events admitted before closure and still-valid ControlPlane entries queued before closure continue through ordinary acceptance and turn execution. A ControlEvent whose acceptance already committed remains immutable.
- In simulation, still-valid ControlEvent actions committed to the global schedule before closure follow the same rule and retain their schedule order.
- A provisional lifecycle result invalidated by cancellation is discarded and audited before acceptance rather than reopening a closed Port.
- Messages and deterministic state transitions inside drained turns execute normally unless fatal state intervenes.
- Produced Port Commands receive terminal host-stop not-delivered audit dispositions.
- Produced ControlCommands receive terminal host-stop rejection audit dispositions.
- The ControlPlane requests cancellation or stop from implementations and waits for admitted work, children, and workers where possible.

Authoritative technical stop performs no implicit domain cancel, flatten, or reconciliation.

### 23.2 Engine-Global Fatal Failure

Fatal state is monotonic and first-failure-wins. Causes include:

- Kernel panic or invariant violation.
- ControlPlane panic or authority-state corruption.
- Acceptor, global routing, or global scheduler corruption.
- Required-diagnostics failure.
- Publication bookkeeping failure that prevents Command disposition accounting.
- A configured global resource-limit terminal failure.
- Environment failure whose affected endpoints cannot be identified or isolated.

After fatal establishment:

- The primary cause is retained; later causes are secondary diagnostics.
- No later input acceptance, turn, or publication is guaranteed.
- Runtime waits are awakened so cleanup may begin.
- Cleanup and final diagnostics are best effort.
- Only the successfully completed prefix before fatal establishment is guaranteed.
- The active callback or turn may be partial.
- Unpublished output need not take effect.
- In-memory application state is not reusable.
- The Engine never resumes or restarts a Port.

The runtime must not intentionally dispatch later Components, publish output from the incomplete turn, accept later inputs, or apply later ControlCommands. Application correctness cannot rely on how quickly asynchronously executing infrastructure observes that suppression boundary.

An asynchronous fatal report cannot preempt arbitrary synchronous Rust code. The Kernel observes it when execution returns to a runtime boundary. Hard preemption requires external process termination.

### 23.3 Process Termination

External Engine-process termination provides no guarantee of callback completion, Port cleanup, child joining, diagnostic flush, terminal outcome, or consistent state. It is operational authority outside the deterministic contract.

### 23.4 Terminal Outcomes

Terminal outcomes are delivered to the embedding host and remain distinct from ControlEvents, Port Events, supervisor reports, and diagnostics. The host must be able to distinguish at least:

- Normal application-requested completion.
- Authoritative host-requested technical stop, including cleanup failure or unresolved ambiguity.
- Technical simulation completion, horizon completion, source stall, or simulation limit failure.
- Engine-global fatal failure with primary cause and available secondary diagnostics.
- Required terminal-record failure, which may replace an otherwise successful host outcome with diagnostics failure.

External process termination commonly prevents any reliable in-process terminal outcome and must not be represented as though cleanup succeeded.

## 24. Port Child Work

Every child thread, task, process, job, or third-party runtime operation belongs transitively to one implementation unit and one lifecycle scope.

- Detached child work is forbidden.
- New child creation stops when its owning endpoint or implementation begins stopping or is quarantined.
- The Environment initiates lifecycle cancellation; the implementation propagates it.
- Cancellation is cooperative and proves neither completion nor rollback.
- Child failure reports through its owning implementation and is classified by affected logical endpoints.
- An implementation is not reported quiesced or safely finalized until all owned children terminate and are joined or are fenced by a hard isolation boundary.
- Child-produced Port Events use the owning logical Port and incarnation authority.
- Child Port Events admitted before closure drain normally; later offers are rejected before admission.

A timeout, escaped child, or failed join prevents the affected endpoint from becoming safely `Failed`, restarting, or participating in normal Engine completion. A deployment requiring bounded hard termination must use an appropriate process boundary or terminate the Engine process.

Application-requested heavy work such as inference remains a service Port protocol. Environment worker pools, if later added, are placement mechanisms for Port-owned work and are never Component capabilities.

## 25. Diagnostics And Observability

### 25.1 Authority And Structure

Each Engine run has one Engine-owned diagnostics facility and one ordered diagnostic stream containing two distinct record classes:

```text
automatic audit records --\
                           +--> diagnostic stream --> configured outputs
write-only user logs -----/
```

The automatic subset is the run's audit view. Metrics are aggregate operational measurements and are not ordered stream records. OpenTelemetry is an optional projection and not Kavod causal identity or storage.

This facility is not a journal. It is never application state, broker truth, a recovery log, an outbox, or authority to resume an Engine.

### 25.2 Causal Identity

Deterministic causality is described by the applicable combination of:

- Engine run identity.
- Root Event index.
- Turn action sequence.
- Parent action sequence.
- Callback identity.
- Produced Command identity or run-scoped reference.
- Logical Port, implementation-unit, and incarnation identities where applicable.

Diagnostic sequence describes recorder observation order and may reflect concurrent live records. No diagnostic or causal identity is a durable business identifier.

### 25.3 Automatic Detail Levels

Useful automatic detail levels are:

| Level | Meaning |
|---|---|
| `Off` | No automatic audit records |
| `Audit` | Run lifecycle, accepted inputs, produced Port Commands and ControlCommands, consequential boundaries, gaps, and faults |
| `Debug` | Audit plus turns, callback invocation, and produced Messages |
| `Trace` | Debug plus callback completion, mutation boundaries, and detailed runtime actions |

`Trace` records semantic actions visible to Kavod, not every statement or field change. The MVP does not record old/new state, field diffs, generic state serialization, or state hashes.

At `Audit`, automatic records include at least:

- Run start with executable/application, graph, determinism-affecting configuration, and diagnostics identities.
- `EventAccepted` with the complete Port Event or ControlEvent payload, source, Event index, and acceptance time.
- `EventProcessingStarted`, which proves processing began without proving completion.
- Every produced Port Command and ControlCommand with complete payload, destination or operation, root Event, producing callback, and production order.
- Consequential Port and ControlPlane boundaries, including lifecycle intent and result, requested and effective placement, incarnation, handoff or known disposition, quarantine, restart, and infrastructure fault.
- Recording gaps where best-effort loss is later reportable.
- Engine fault and terminal outcome, including the last completed Event index where available.

At `Debug`, invocation records precede callback entry. At `Trace`, completion records exist only after normal return. `ReducerMutationBoundaryCompleted` proves only successful completion with mutable access; it does not prove that any field changed.

### 25.4 User Logging

Components, Reducers, and Ports may emit user logs through a narrow write-only capability. Application callbacks receive no enablement, sampling, writer, flush, or exporter status and cannot branch on diagnostic configuration.

The logging API must not conditionally execute application-supplied lazy logic based on filtering. User logs are best effort and cannot consume capacity reserved for required automatic records. A log may explain a business fact but cannot be its only representation when application behavior depends on that fact.

Application-provided formatting, `Display`, `Debug`, cloning, or serialization executed for diagnostics remains subject to the deterministic purity contract. A panic while Kavod invokes required formatting or encoding poisons the Engine. Diagnostics do not sandbox impure application code.

### 25.5 Best-Effort And Required Recording

Diagnostics configuration independently selects detail, user-log filtering, outputs, buffering, and automatic-record failure policy.

The default MVP path uses bounded in-memory buffering and batched output writes. Long-running live operation must not assume unbounded retention.

- Best-effort loss does not stop execution. It increments loss accounting and emits a gap record if the stream later becomes writable.
- Required automatic-record failure is terminal and is never silently downgraded.
- User-log failure never changes callback control flow.
- Optional console, OpenTelemetry, or external-export pressure must not consume required capacity or block deterministic callbacks.

The required writer's acknowledgement boundary must be explicit. Memory admission, buffered disk admission, write completion, flush, and data synchronization are different guarantees. Kavod must not call buffered admission durable.

Required acknowledgement occurs at the authority boundary of the action being recorded:

- Required `EventAccepted` acknowledgement is acceptance commit.
- Required `ReducerInvoked` and `ComponentInvoked` records are acknowledged before invocation.
- Required `MessageProduced` and `CommandProduced` acknowledgement commits deterministic production; under best effort, production commits first and recording follows.
- Reducer and Component completion records exist only after normal return. Their absence may identify a partially mutated callback boundary but provides no rollback.
- A required Component-produced lifecycle operation is acknowledged before backend invocation.
- Required Port Command publication records are acknowledged before handoff.
- Immediate quarantine cannot wait for diagnostics; authority closes first, recording follows, and recording failure then escalates to fatal without rolling quarantine back.
- Required terminal-record failure may change the host outcome to diagnostics failure but cannot revive or roll back stopped work.

### 25.6 Metrics And OpenTelemetry

The MVP must make queue occupancy and lag, Acceptor service, turn duration and counts, mailbox reservation failure, lifecycle state and latency, quarantine, child cancellation and joining, diagnostics health, and terminal causes observable through metrics.

High-cardinality run, Event, Command, incarnation, callback, and business identifiers belong in audit records, not metric labels.

OpenTelemetry sampling, trace IDs, exporter queues, and failure are outside deterministic semantics. OTel IDs may correlate diagnostics but may not determine routing, ordering, business identity, or replay comparison.

Instrumentation consumes resources and may change live latency or which external offer becomes visible first. The narrower guarantee is that callbacks cannot observe diagnostic configuration and successful diagnostic work does not reorder an already accepted turn.

## 26. Replay And Persistence Boundaries

Replay, state hashes, and divergence tooling are not MVP guarantees. Diagnostics are designed so a future tool may use a sufficiently complete and compatible audit view.

Such a tool must:

- Cold-start a new isolated Engine.
- Use the same executable build, graph, initial state, and relevant configuration.
- Inject recorded Port Events and ControlEvents in Event-index order with recorded acceptance times.
- Recompute Messages, Port Commands, and ControlCommands.
- Use passive Port and ControlPlane backends that create no live effects or duplicate lifecycle consequences.
- Compare produced outputs and report observed divergence.
- Create new diagnostic and distributed-trace identities linked to, but distinct from, the original run.

Recorded rejection, lifecycle, and quarantine ControlEvents are replay inputs. Handoff, rejection, and external-ambiguity dispositions are audit evidence, not independently reinjected application inputs.

A produced `StopEngine` request terminates replay only when the recorded run shows that the request was successfully validated and established the terminal boundary. A rejected request is compared as output, its recorded rejection ControlEvent remains an input, and replay continues.

Ordinary accepted-input replay cannot reproduce the exact timing of pre-acceptance authority races, asynchronous fatal establishment, worker panics, or process termination. Those would require a future runtime-control or fault tape.

No diagnostic or replay result:

- Restores or resumes the original Engine.
- Authorizes Port Command delivery.
- Establishes broker or venue truth.
- Provides crash recovery or resend authority.
- Guarantees cross-build compatibility.

A replacement live Engine always cold-starts, obtains needed external data, reconciles against external truth through application protocols, and remains governed by application arming policy.

## 27. Configuration And Illustrative Composition

Configuration is separated by owner:

| Configuration | Owner |
|---|---|
| Domain data, Component construction, initial `AppState` | Application |
| Turn bounds and deterministic Kernel behavior | Engine |
| Port bindings, supported placement, Event capacity/full policy, Command capacity, Acceptor quantum, shutdown policy | Environment and binding |
| Models, horizons, action limits, deterministic choices | Simulation Environment |
| Detail, outputs, buffering, acknowledgement, user-log filters | Diagnostics |

All application, Engine, Environment, binding, simulation, and diagnostics configuration is immutable after build. Determinism-affecting configuration is included in run provenance. Runtime mechanisms such as channels, locks, executors, thread handles, schedulers, and writer handles remain private.

An illustrative composition might be:

```rust
let application = Application::builder(AppState::new(config.state))
    .port::<MarketData>()
    .port::<Execution>()
    .port::<Timer>()
    .reducer::<ExecutionEvent>(apply_execution)
    .component(BarAggregator::new(config.bars), |c| {
        c.on::<MarketEvent>(BarAggregator::on_market)
            .produces_message::<BarsClosed>();
    })
    .component(Strategy::new(config.strategy), |c| {
        c.on::<BarsClosed>(Strategy::on_bars)
            .produces_command::<Execution>()
            .produces_control();
    })
    .build()?;

let environment = LiveEnvironment::builder()
    .bind::<MarketData>(market_data, |b| {
        b.event_capacity(4096)
            .event_full_policy(config.market_data_full_policy)
    })
    .bind::<Execution>(execution, |b| {
        b.event_capacity(1024)
            .event_full_policy(config.execution_full_policy)
            .command_capacity(1024)
    })
    .bind::<Timer>(timer, |b| {
        b.event_capacity(256)
            .event_full_policy(config.timer_full_policy)
    })
    .acceptor_quantum(4)
    .build(&application)?;

let engine = Engine::builder(application, environment)
    .turn_limits(...)
    .diagnostics(...)
    .build()?;
```

This example intentionally does not settle ownership types, closure shapes, method names, error types, lifecycle declaration syntax, or whether protocols use derives or macros.

## 28. Narrowed MVP Scope

The Kavod MVP includes:

- Closed typed application protocols and a closed core control protocol.
- One validated immutable application graph.
- One concrete canonical `AppState` and Component-private state.
- Stateless output-free Reducers and deterministic Components.
- Single-threaded turn execution with breadth-first Messages and finite turn bounds.
- Logical Port Specs and exactly one endpoint binding per declared Port.
- A live Environment contract with bounded per-binding Port Event queues, bounded Port Command mailboxes, one Acceptor, explicit capacity policy, and visible dispositions.
- Ready-first startup and a ControlPlane-owned technical lifecycle.
- Contained Port quarantine, admitted-work drainage, explicit application-requested restart, and safe fatal classification.
- Application-managed normal shutdown and authoritative host-stop semantics.
- Deterministic historical simulation with grouped models, virtual time, global schedule ordinals, cancellation, and finite completion policies.
- One diagnostics facility with causal audit, write-only user logging, metrics boundaries, and best-effort or required automatic recording.

The MVP need not include every conceptual live placement. One implementation may initially support a small explicit placement set while rejecting unsupported requests visibly. The semantic protocol must not assume dedicated threads, async tasks, or process proxies are interchangeable.

The MVP does not include full DST, generalized fault injection, replay execution, snapshots, recovery, durable outbox behavior, schema migration, runtime graph mutation, or finalized alternate Environment mechanisms.

## 29. Required Semantic Conformance Tests

The implementation must eventually prove at least the following semantic properties.

### 29.1 Kernel And State

1. Repeated runs with identical deterministic inputs produce identical callback, Message, Port Command, ControlCommand, and completed-turn state traces.
2. Reducers run before Components for every Port Event, ControlEvent, and Message.
3. Messages propagate in exact breadth-first production order without recursion.
4. A Reducer panic exposes no later Component or publication guarantee and the Engine never resumes.
5. Turn-limit exhaustion returns the matching terminal limit error without applying a partial ControlCommand set.
6. Graph construction rejects missing consumers, undeclared targets, duplicate or missing bindings, unstable configuration, and undeclared runtime production.

### 29.2 Derived State

7. Separate bar-closure Messages demonstrate the stale partial-state failure.
8. One `BarsClosed` aggregate projects every required closure exactly once in stable order before the Strategy runs once; no aggregate is emitted when nothing closes, and configured gap behavior is covered.
9. Equal timestamps remain separate turns or Messages unless one explicit batch payload represents the atomic fact.
10. A deliberately order-dependent multi-Reducer design demonstrates deterministic but stale behavior and is rejected by application review/tests rather than misrepresented as kernel coherence.

### 29.3 Live Boundaries

11. Per-Port queues isolate capacity and Acceptor rounds preserve frozen source order and quantum behavior.
12. A Port Event admitted immediately before stop or quarantine remains eligible and blocks quiescence/restart until drained.
13. An offer losing to admission closure remains outside Kavod.
14. ControlPlane-ingress exhaustion that cannot preserve an authoritative transition and consequence establishes fatal state rather than dropping or coalescing it.
15. Mailbox exhaustion rejects the complete destination batch while healthy destinations remain eligible.
16. Multiple rejected Port Commands produce exactly one disposition each in original global production order with unambiguous Command references.
17. Final handoff racing quarantine, Port stop, or host stop has exactly one serialized winner.
18. Required publication-record failure prevents handoff and poisons the Engine.

### 29.4 Lifecycle And Supervision

19. `Ready` is the first accepted input and no implementation starts before its turn completes.
20. Ordinary ingress opens only after the matching `PortStarted` turn completes.
21. A stale provisional start result cannot reopen a cancelled or newer incarnation.
22. A successful stop result remains private until child and admitted work drain; stop failure produces `PortStopFailed` and quarantine.
23. Contained worker exit or panic quarantines only the identified affected endpoints and does not set fatal state.
24. A grouped-model failure quarantines every endpoint whose shared state may be compromised.
25. Multi-endpoint failure consequences are contiguous, stable, and expose completion of their common sequence.
26. `PortFailed` reports quarantine, while restart remains rejected until `PortQuiesced` acceptance establishes `Failed`.
27. Explicit restart allocates a new incarnation, rejects stale offers, and never resends old Commands.
28. Escaped or unjoined child work prevents safe quiescence, restart, and normal completion.
29. Normal `StopEngine` is rejected while active, pending, admitted, or feedback work remains.
30. Authoritative host stop drains admitted work without new handoff or feedback input.

### 29.5 Simulation And Diagnostics

31. Same virtual time is ordered by global schedule ordinal; zero latency never re-enters callbacks recursively.
32. Existing same-time actions precede outputs newly committed at that time.
33. Port Events and ControlEvents both execute as scheduled accepted inputs, and post-turn ControlCommands produce later scheduled consequences.
34. Eligible simulated Port Commands execute synchronously in global production order, committing each callback's staged output before the next Command.
35. Changing future historical records cannot alter an earlier execution prefix, and a due occurrence updates model truth before its public Port Event.
36. Endpoint failure preserves committed Port Event deliveries and cancels affected pending wakes/model callbacks.
37. Scheduling into the past and total or same-time action-limit exhaustion terminate visibly.
38. `UntilIdle`, `Until(T)`, source exhaustion, stalled, and action-limit outcomes satisfy their distinct completion rules.
39. Best-effort diagnostic failure creates accounted loss without changing callback control flow.
40. Required acceptance-record failure prevents dispatch; an acknowledged acceptance may remain accepted but unprocessed.
41. Immediate quarantine precedes required recording and remains committed if recording then fails fatally.
42. Required invocation and production records commit at their specified boundaries, and completion records exist only after normal return.
43. Logging configuration, filtering, and OTel sampling are unobservable to deterministic callbacks.

## 30. Required Failure Traces

Before implementation interfaces are frozen, design and tests must walk through these traces end to end:

### 30.1 Partial Derived State

```text
Tick closes four bars
-> four separate closure Messages
-> Strategy reacts after only the first projection
-> stale Command is produced

Required resolution:
one complete BarsClosed fact and one cohesive canonical transition
```

### 30.2 Failure During A Turn

```text
turn is executing
-> Execution worker fails
-> ControlPlane quarantines the incarnation immediately
-> active deterministic turn completes
-> Commands to Execution are not delivered
-> healthy destinations remain eligible
-> admitted old-incarnation Port Events retain order
-> PortFailed and CommandNotDelivered arrive later in ordinary order
```

### 30.3 Handoff Race

```text
Command is classified and capacity reserved
-> quarantine or host stop races final handoff
-> one atomic authority boundary chooses the winner
-> handoff-first is externally ambiguous
-> closure-first is explicitly not delivered
```

### 30.4 Start Cancellation

```text
StartPort accepted; incarnation 8 starts
-> backend reports success into provisional ControlPlane ingress
-> StopPort or authoritative host stop cancels incarnation 8
-> stale result is invalidated and audited before acceptance
-> no PortStarted turn reopens ingress
```

### 30.5 Grouped Model Panic

```text
Execution callback mutates shared venue state
-> callback panics before return
-> staged output is not committed and model state is not rolled back
-> every endpoint depending on uncertain shared state is quarantined
-> unidentified or unisolatable blast radius is Engine-fatal
```

### 30.6 Required Diagnostics Failure

```text
candidate input prepared
-> required EventAccepted acknowledgement fails
-> acceptance does not commit
-> no callback receives the input
-> fatal state is established

but:

Port failure is observed
-> quarantine closes authority immediately
-> required quarantine record fails
-> quarantine remains committed
-> Engine escalates to fatal
```

### 30.7 Authoritative Stop

```text
host stop commits
-> new Port Event admission, new ControlPlane entry creation, feedback, and handoff close
-> acceptance continues only for Port Events admitted and valid live ControlPlane entries or simulated ControlEvent actions committed before closure
-> Messages and state transitions inside those turns complete
-> outputs receive terminal audit dispositions
-> implementations and children are stopped/joined where possible
-> terminal host outcome reports cleanup or ambiguity
```

## 31. Migration From v4 And v4.1

| Earlier concept | v5 replacement |
|---|---|
| Determinism by application version, Event tape, and terminal state | Exact executable, frozen graph, complete initial application state, configuration, and accepted Port Event plus ControlEvent sequence; guarantee only the successfully completed execution prefix before fatal |
| `BTreeMap<TypeId, Box<dyn Any>>` Cache and state slots | One application-defined concrete canonical `AppState` |
| Stateful Reducer Component or projector owner | Stateless restricted Reducer callback with temporary mutable `AppState` access |
| Stable Reducer order as implicit coherence | Stable order gives reproducibility only; one cohesive transition preserves related invariants |
| Turn quiescence as a possible settled-state barrier | Per-payload visibility only; complete aggregate facts express required coherence |
| Every application-visible input originates at a Port | Accepted inputs are Port Events or ControlEvents |
| Every external application output targets a Port | Components emit Port Commands or ControlCommands |
| Control Port | Unique Engine-owned ControlPlane that is explicitly not a Port |
| Eager Port startup before application execution | Inert bindings, `Ready` first, explicit start ControlCommands |
| Dedicated OS thread as the live semantic model | Requested placement, Environment realization, and simulation normalization; concrete mechanisms deferred |
| One simulated model per Port | One model may expose several independently addressed logical endpoints |
| Simulated Port owns all source/model state by definition | Domain model owns coherent external-world state; Environment physically holds it and owns scheduling |
| One undifferentiated central live ingress queue | Per-binding bounded Port Event FIFOs plus separate ControlPlane ingress and one Acceptor |
| Event/Message priority or Critical/Normal classes | Per-Port capacity isolation and frozen-round Acceptor fairness |
| Full mailbox or Port failure always stops Engine | Per-destination rejection and contained Port quarantine; uncontained failures remain fatal |
| No Port restart | No automatic restart; explicit application-requested restart from safely `Failed` state |
| Every turn-limit exhaustion panics | Typed terminal run error for configured limits |
| Mandatory journal/Event tape | Configurable diagnostic stream; best-effort or required acknowledgement at explicit boundaries |
| Journal as replay/recovery basis | Audit evidence only; no restoration, resend, broker truth, or recovery authority |
| Full DST as MVP | Deterministic historical simulation only; generalized DST deferred |
| Technical start implies readiness | Technical `Running`, operational readiness, reconciliation, and application arming are distinct |
| Universal Booting/Reconciled/Armed core state | Application-defined operational state and policy; Kavod owns only technical lifecycle |

Concepts retained and refined include closed typed protocols, immutable payloads, actual registrations as graph truth, callback-local production declarations, build-time graph validation, one single-writer Kernel, narrow callback capabilities, logical Port Specs, breadth-first Messages, deferred Commands, frozen turn time, explicit capacities, no silent loss, and independent Engine instances.

## 32. Intentionally Deferred Implementation Decisions

The following do not block this semantic model and belong to the Rust/API or implementation pass:

- Exact protocol aggregation, derive, and registration syntax.
- Exact Component, Reducer, context, Port, model, ControlPlane, and Environment trait signatures.
- Registry erasure and storage layout.
- Queue and reservation implementations.
- Numeric defaults for capacities, Acceptor quantum, and turn/action bounds.
- Exact names and API spelling of supported pre-admission full policies.
- Concrete live placement implementations and shutdown timeout mechanisms.
- Identity token representation and diagnostic schema encoding.
- Exact terminal outcome and error enum shapes.
- Initial `AppState` validation hook syntax.
- Logging facade dependency and structured-field syntax.
- Disk framing, checksums, segmentation, retention, flush, and synchronization modes.
- Future replay implementation, state comparison, and cross-build schema evolution.

Any selected implementation must preserve the semantic boundaries in this document. An API convenience must not expose Engine machinery, weaken no-silent-loss accounting, imply recovery authority, or make live and simulation claim physical parity.

Before a live binding API is implemented, it must select a closed supported set of pre-admission full outcomes satisfying Section 16.3. The semantic model intentionally does not choose that policy vocabulary for every Port.

## 33. Semantic Gates Before Rust Implementation

Before public Rust interfaces are designed, the following gates must be reviewed and accepted:

1. **Protocol gate:** The five protocol classes and their legal directions are unambiguous.
2. **State gate:** `AppState`, Component-private state, ControlPlane state, and model state have non-overlapping ownership and mutation authority.
3. **Turn gate:** Per-payload Reducer visibility, BFS Messages, aggregate-fact coherence, output deferral, and finite bounds admit no conflicting execution trace.
4. **Acceptance gate:** Offer, queue admission, provisional control outcome, acceptance commit, and processing are distinct.
5. **Publication gate:** Every Port Command has one disposition; rejection ordering, capacity isolation, handoff linearization, and external ambiguity are explicit.
6. **Lifecycle gate:** `Ready`, start, running, stop, quarantine, quiescence, failed, restart, normal completion, and host stop account for all admitted and child work.
7. **Failure gate:** Port-local containment and Engine-global fatal classification cannot both claim authority over the same failure.
8. **Simulation gate:** Model ownership, staging, same-time order, cancellation, source exhaustion, and completion have one deterministic interpretation.
9. **Diagnostics gate:** Required recording affects only its declared authority boundaries and never grants state, control, delivery, or recovery authority.
10. **MVP gate:** No interface requires full DST, replay, snapshots, recovery, generalized placement, or runtime graph mutation.

## 34. Readiness Statement

The v5 semantic model is coherent and ready for a dedicated Rust API and implementation-design pass after the semantic gates above are validated.

No major semantic blocker remains. The intentionally deferred decisions concern representation, mechanism, defaults, and future capabilities rather than ownership, ordering, authority, lifecycle, or deterministic behavior.

Rust implementation must not begin by copying v3, v4, or v4.1 interfaces. Those documents contain superseded Cache, actor, eager-startup, universal-fatality, journal, and full-DST assumptions. The implementation pass must derive the smallest public API that can enforce this document's capabilities and make violations visible.
