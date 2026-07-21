# ControlPlane, Lifecycle, And Supervision

> **Status:** Settled for the Kavod MVP ControlPlane, application-directed Port lifecycle, placement, quarantine, and runtime-supervision boundary
> **Scope:** Engine/application communication, typed control protocols, Environment execution backends, Ready-first bootstrap, Port lifecycle, recoverable Port-local failure, Engine-global fatal failure, explicit restart, normal shutdown, replay, and observability
> **Supersedes:** Conflicting lifecycle, eager-startup, placement-authority, Control-Port, and universal worker-fatality statements in the earlier 4.2 reports

## Conclusion

Every Engine owns one `ControlPlane`. It is not a Port. It is the Engine subsystem that owns runtime lifecycle authority and provides one typed, deterministic communication boundary between the Engine and the application:

```text
ControlPlane -- ControlEvent --> Reducer or Component
Component    -- ControlCommand --> ControlPlane
```

The existing application data plane remains unchanged:

```text
Port      -- Event   --> Reducer or Component
Component -- Message --> Reducer or Component
Component -- Command --> Port
```

`ControlEvent`s use the same acceptance, turn, Reducer-before-Component, frozen-time, tracing, and replay semantics as Port Events. Ordinary Components emit declared `ControlCommand`s as deferred deterministic outputs. Reducers remain output-free. Components never receive an Engine, ControlPlane, Environment, Port, worker, scheduler, executor, process, task, or channel handle.

The ControlPlane validates and orders lifecycle intent. The selected Environment is its execution backend and runtime supervisor:

```text
Component
    -> ControlCommand
    -> ControlPlane
    -> LiveEnvironment or SimulationEnvironment
    -> supervisor result
    -> ControlPlane
    -> accepted ControlEvent
    -> application
```

Bindings and implementation resources are validated and constructed inertly before the first turn. `ControlEvent::Ready` is the first accepted input. Application lifecycle logic then requests logical Port startup and optional placement. A live Environment realizes thread, task, or process placement; a basic simulation Environment may normalize every placement request to deterministic single-threaded model execution.

Port-local startup failure, worker exit, worker panic, mailbox failure, or child-work failure quarantines the affected logical Port and becomes an application-visible ControlEvent. It does not automatically restart the Port and does not set the Engine-global fatal latch. Kernel, ControlPlane, Acceptor, global scheduler, required-diagnostics, and other Engine-wide failures remain fatal.

This report deliberately replaces two earlier assumptions:

1. The startup barrier establishes Engine and ControlPlane readiness, not eager startup of every Port worker.
2. A contained Port worker failure quarantines its logical endpoint rather than always terminating the Engine.

## Planes

Kavod has four communication planes with different authority.

| Plane | Contents | Authority |
|---|---|---|
| Application data plane | Port Events, Messages, Port Commands, `AppState`, Component-private state | Deterministic application and logical Port protocols |
| Engine control plane | ControlEvents, ControlCommands, Engine and Port lifecycle state | Engine-owned ControlPlane |
| Supervision plane | Backend start/stop results, worker/model exit, panic, cancellation, join, implementation-unit health | Environment reporting to the ControlPlane |
| Observability plane | Automatic audit, user logs, metrics, status projections | Observation only |

The supervision plane is not directly application-visible. The ControlPlane classifies supervisor reports and either:

- Converts a contained, actionable Port-local transition into one or more accepted ControlEvents.
- Establishes the Engine-global fatal latch without starting another application turn.

Diagnostics observe these decisions but never cause them.

## Authority

| Participant | May send | May receive | Owns |
|---|---|---|---|
| Embedding host | Cooperative request, authoritative stop, external process termination | Technical status and terminal outcome | Process-level authority |
| ControlPlane | ControlEvents and backend lifecycle operations | Host requests, ControlCommands, supervisor reports | Engine lifecycle, logical Port lifecycle, quarantine, incarnation, fatal classification |
| Kernel | Accepted input dispatch and deterministic outputs | Accepted Port Events and ControlEvents | Turn execution and deterministic application state |
| Environment | Start/stop/cancel operations to implementation units | ControlPlane operations and worker/model reports | Physical placement, resources, queues, scheduler, workers, models, joins |
| Port implementation/model | Port Events and private supervisor reports | Port Commands and backend lifecycle operations | External-system or simulated-model state |
| Reducer | No outputs | Events, ControlEvents, Messages, mutable `AppState` | Canonical application-state projection |
| Component | Messages, Port Commands, ControlCommands | Events, ControlEvents, Messages, immutable `AppState` | Deterministic application decisions |
| Diagnostics | Best-effort or required-record failure report | Audit and user-log records | Recording policy only |

The host may stop an Engine regardless of application approval. A cooperative host request may instead become a ControlEvent so deterministic application logic can disarm, reconcile, stop Ports, and request normal Engine completion.

## Typed Control Protocol

### Closed Types

Kavod supplies a closed core control protocol alongside the application's closed data protocol:

```text
AcceptedInput<AppEvent>
    = PortEvent(AppEvent)
    | ControlEvent(ControlEvent)

TurnOutput<AppMessage, AppCommand>
    = Message(AppMessage)
    | PortCommand(AppCommand)
    | ControlCommand(ControlCommand)
```

This is conceptual composition, not a finalized Rust enum or trait design. Users receive concrete typed payloads and never downcast.

The core defines ControlEvent and ControlCommand meanings because the ControlPlane must interpret them. Application-specific operator requests, trading modes, arming policies, reconciliation facts, and safety actions remain application protocol types.

### Registration

Ordinary Reducers and Components register typed callbacks for ControlEvents through the same application registration model used for Events and Messages.

For each accepted ControlEvent:

1. Matching Reducers execute in stable registration order.
2. Matching ordinary Components execute in stable registration order.
3. Messages propagate through the ordinary breadth-first FIFO.
4. Port Commands and ControlCommands remain deferred until turn quiescence.

There is no privileged lifecycle callback class, control Component group, direct ControlPlane callback, or short-circuit execution path.

A recommended application composition is:

```text
ControlEvent
    -> LifecycleReducer
        -> AppState.runtime projection
    -> LifecycleController
        -> ControlCommand
        -> application-defined Operational/Degraded/Armed Message

Strategy
    -> reads AppState.runtime when required
    -> consumes application-level lifecycle Messages when appropriate
```

`LifecycleReducer` and `LifecycleController` are ordinary application callbacks, not Kavod callback kinds.

### Production Declarations

Every callback declares each ControlCommand operation it may produce. A declaration creates:

- A callback-to-ControlPlane graph edge.
- Runtime output authorization.
- Causal and audit metadata.
- A build-time opportunity to validate logical Port targets and requested placement support.

Undeclared ControlCommand production is an invariant violation. Reducers cannot emit ControlCommands.

### Graph

The frozen application graph adds one distinguished ControlPlane boundary:

```text
Port         -- Event          --> callback
ControlPlane -- ControlEvent   --> callback
callback     -- Message        --> callback
callback     -- Command        --> Port
callback     -- ControlCommand --> ControlPlane
```

The ControlPlane:

- Is present exactly once per Engine.
- Has no `PortSpec`.
- Has no Environment binding.
- Has no worker, model, mailbox, or application-supplied implementation.
- Cannot be replaced or restarted as a Port.

Kavod may reuse internal registry and dispatch machinery across Port and control edges. That implementation reuse does not make the ControlPlane a Port.

## Identities

The design distinguishes four identities.

| Identity | Meaning |
|---|---|
| Engine run identity | One execution of one Engine instance |
| Logical Port identity | Stable application graph destination and Event source |
| Implementation-unit identity | Environment-owned worker, process proxy, task host, or simulated model |
| Port incarnation identity | One lifecycle episode of one logical Port |

A logical Port retains its identity across explicit restart. Every accepted start or restart operation allocates an operation identity and a new incarnation identity before backend invocation. Success, failure, cancellation, and every backend report carry both identities. A late, duplicate, or stale report is audited but cannot mutate a newer operation or incarnation. Old-incarnation Event authority is revoked before a replacement becomes active.

One implementation unit may own several logical Port endpoints. This remains private Environment topology. Application-visible lifecycle reports always identify logical Ports, never a grouped implementation as though it were an application Port.

## Authoritative And Projected State

The ControlPlane owns authoritative runtime state. A minimal logical Port lifecycle is:

```text
Stopped -> Starting -> Running -> Stopping -> Stopped
               |          |          |
               v          v          v
           Quarantined <- failure ---+
               |
               | old implementation terminated or isolated
               v
             Failed
```

The exact internal state representation is private, but these meanings are normative:

| State | Meaning |
|---|---|
| `Stopped` | Bound but inactive; no Event ingress or Command handoff |
| `Starting` | A start operation is in progress; no ordinary Port traffic is yet enabled |
| `Running` | Technical implementation controls are installed; this does not imply external readiness |
| `Stopping` | Technical stop is in progress; new ordinary Port Commands are rejected |
| `Quarantined` | Routing authority is revoked, but old implementation or child work may not yet be terminated or isolated |
| `Failed` | The failed incarnation is quiesced or fenced from further effects and is safe to replace or leave inactive |

Applications may project ControlEvents into `AppState`, for example:

```text
AppState.runtime.ports: LogicalPortId -> ApplicationPortStatus
```

That projection is deterministic application state, not runtime authority. It may lag the ControlPlane until the corresponding ControlEvent is accepted. Collections whose iteration affects behavior must have deterministic iteration semantics.

The application must keep these states distinct:

| Layer | Example |
|---|---|
| Engine ready | Kernel, ControlPlane, bindings, queues, and backend initialized |
| Port technically running | Worker/model lifecycle controls installed |
| Port operationally ready | Connected, authenticated, subscribed, reconciled |
| Application armed | Application policy permits active domain behavior |

The ControlPlane reports the first two. Port protocols report external operational facts. The application owns arming and safety policy.

## Environment As Control Backend

The ControlPlane defines semantic lifecycle operations. The Environment realizes them.

```text
ControlCommand
    -> ControlPlane validation and lifecycle transition
    -> Environment operation
    -> worker/model result
    -> ControlPlane classification
    -> ControlEvent
```

The Environment is not a Port. It is the ControlPlane execution backend and runtime supervisor. It also owns Event admission mechanisms, Command mailboxes, virtual scheduling, implementation resources, and joining, so it cannot be modeled as an application endpoint without circular authority.

### Placement

A Port-start ControlCommand may include a requested placement. Conceptual placement requests include:

```text
Threaded
AsyncTask
Process
```

The request identifies how the selected Environment should realize the already-declared logical binding. It never carries a Port implementation, worker, process, task, executor, or channel handle.

Three terms must remain distinct:

| Term | Meaning |
|---|---|
| Requested placement | Deterministic ControlCommand intent |
| Realized placement | Physical live thread, task, process, proxy, or other mechanism |
| Normalized placement | Simulation mapping to deterministic model execution |

A live Environment either realizes a supported request or reports visible start failure. It does not silently fall back to another physical mechanism. A basic simulation Environment may normalize all supported placement requests to deterministic single-threaded model execution.

Before `Ready`, each binding freezes the set of accepted placement requests and, for simulation, a total deterministic normalization mapping for that set. A statically declared unsupported request is a build error where graph declarations make it knowable. A dynamically produced unsupported request is rejected before entering `Starting`, leaves the Port `Stopped`, and produces a ControlEvent. Failure after the backend has accepted a supported start operation quarantines the allocated incarnation and follows ordinary cleanup-to-`Failed` semantics.

For MVP backtesting, normalization need not model OS thread scheduling, process isolation, IPC failure, executor behavior, or physical parallelism. Requested and effective placement are recorded in run provenance and diagnostics. A future fault-oriented simulation may model additional placement-specific guarantees without changing the ControlPlane protocol.

Placement does not change the logical application graph, Port protocol, Component code, or source attribution.

### Binding

Every declared logical Port still has exactly one compatible Environment binding before `Ready`. Binding means the backend knows how to construct or activate the endpoint. It does not mean the Port is started, connected, healthy, or operationally ready.

The Environment may retain factories or equivalent private construction resources needed for explicit restart. Components never receive them.

## Ready-First Startup

Startup proceeds in this order:

1. Build and validate the application graph.
2. Build and validate all Port bindings, placement capabilities, capacities, and runtime policies.
3. Construct the Kernel, ControlPlane, diagnostics, queues, and Environment backend.
4. Establish every logical Port in `Stopped` state with no active Event or Command authority.
5. Accept `ControlEvent::Ready` as the first application input.
6. Run its complete Reducer, Component, Message, Port Command, and ControlCommand turn.
7. Apply lifecycle ControlCommands after turn quiescence.
8. Start requested live workers or activate requested simulated endpoints.
9. Convert each backend result into a later accepted Port lifecycle ControlEvent.

`Ready` means only that the Engine can process deterministic application and control turns. It does not claim that any Port is running or any external system is available.

No Port Event may be accepted and no ordinary Port Command may be handed off before that logical Port reaches `Running`. Events offered by a starting implementation remain backend-private until the ControlPlane opens ingress after the corresponding `PortStarted` ControlEvent turn completes.

A Component may produce Port Commands during `Ready`, but Commands targeting stopped or starting Ports are rejected under the ordinary unavailable-destination rule. Applications start Ports, wait for `PortStarted`, then issue ordinary Port Commands.

There is no hidden autostart. Convenience configuration may generate deterministic bootstrap lifecycle logic, but it must compile to the same Ready-turn ControlCommands and remain visible in the graph and audit stream.

## Port Lifecycle Operations

The core control protocol supports logical lifecycle intent. Exact enum spelling is deferred, but the semantic operation set includes:

- Start one declared logical Port with a requested placement.
- Stop one logical Port.
- Explicitly restart one failed logical Port after its previous incarnation is quiesced or isolated.
- Request normal Engine stop after application-managed Port shutdown.

Lifecycle operations are requests, not proof of completion. A successful or failed backend result returns later as a ControlEvent.

Lifecycle ControlEvents distinguish routing and termination facts. `PortFailed` reports immediate quarantine and loss of routing authority. `PortQuiesced` reports that the failed incarnation and all owned child work have terminated or are fenced from further effects, allowing transition to `Failed`. `PortStarted` and `PortStopped` report successful operation completion.

Repeated or illegal operations are never silently ignored. Every duplicate lifecycle ControlCommand is deterministically rejected against the authoritative state reached by earlier production-order operations. Cooperative host shutdown requests are the only control input that may use separately specified idempotent coalescing. The ControlPlane never starts two incarnations because equivalent requests raced.

The minimum operation matrix is:

| Current state | Start | Stop | Restart |
|---|---|---|---|
| `Stopped` | Begin `Starting` | Reject as already stopped | Reject; use Start |
| `Starting` | Reject duplicate | Cancel startup and begin `Stopping` | Reject |
| `Running` | Reject duplicate | Begin `Stopping` | Reject |
| `Stopping` | Reject | Reject duplicate | Reject |
| `Quarantined` | Reject | Continue required cleanup but do not claim Stopped | Reject until quiesced or isolated |
| `Failed` | Reject; use Restart | Transition to `Stopped` without reviving the old incarnation | Begin `Starting` with a new incarnation |

Every rejection becomes a later ControlEvent unless Engine-global fatal closure intervenes, in which case terminal audit records retain the disposition.

## Lifecycle Barriers

ControlPlane transitions that affect Port availability are barriers rather than ordinary payload priorities.

### Start Barrier

When an implementation reports successful technical start:

1. The ControlPlane validates the report against the pending operation and incarnation.
2. It places the result in the lifecycle-barrier FIFO without yet opening Port traffic.
3. If failure or cancellation revokes the incarnation before acceptance, the stale start result is discarded and audited.
4. Acceptance of the `PortStarted` ControlEvent atomically transitions the logical Port to `Running` for outputs of that turn.
5. Reducers project the running state before Components react.
6. Commands produced by the `PortStarted` turn may be handed to the now-running incarnation after quiescence.
7. Ordinary Port Event acceptance opens only after that turn completes and only if the same incarnation remains `Running`.

This guarantees that no application callback observes ordinary traffic from a Port whose technical-start fact has not yet been reduced.

### Failure Barrier

When the Environment reports a Port-local failure:

1. The ControlPlane immediately marks the affected logical Port quarantined.
2. It revokes old-incarnation Event ingress and new Command handoff.
3. An active synchronous callback is not preempted.
4. The already accepted turn continues to quiescence; a contained Port-local failure does not interrupt its remaining deterministic callbacks or Messages.
5. At the next kernel safe boundary, the affected `PortFailed` ControlEvent is accepted before ordinary Event selection resumes.
6. Reducers project quarantine before Components react and before another ordinary Event turn begins.

Multiple live supervisor reports retain the nondeterministic order in which the ControlPlane observes them. Simulation orders them deterministically. Accepted ControlEvents freeze that order for replay.

This is not a generic Event-priority mechanism. Only lifecycle transitions that change routing authority create a control barrier.

Lifecycle consequences enter one Engine-owned barrier FIFO ordered by monotonic control sequence. The FIFO drains before ordinary ControlEvents and Port Events. A multi-endpoint implementation failure enqueues all affected endpoint consequences contiguously in stable logical Port order before ordinary acceptance resumes. Components may run between those endpoint Events, but each event identifies its common failure sequence and the number of endpoint consequences still pending, allowing application lifecycle policy to remain conservatively degraded until the sequence completes.

ControlPlane ingress is bounded independently of every Port queue. Supervisor transitions and lifecycle outcomes cannot be dropped or coalesced. A cooperative host request may use an explicitly defined idempotent coalescing rule. If the ControlPlane cannot preserve an authoritative transition and its application-visible consequence, the Engine can no longer account for lifecycle state and must establish the Engine-global fatal latch.

`Ready` is structurally first. Cooperative host requests arriving during initialization remain in the bounded ControlPlane ingress and become eligible only after the Ready turn unless an authoritative host stop cancels initialization. After bootstrap, ordinary non-barrier ControlEvents receive one bounded FIFO visit in each Acceptor round under the global quantum. Lifecycle-barrier consequences drain first because routing authority has already changed; they are not mixed into ordinary fairness arbitration.

## Command And Control Output Semantics

One turn may produce Messages, Port Commands, and ControlCommands. All retain one deterministic causal production order for tracing. Their effects occur in phases after quiescence:

1. Finalize deterministic application state and causal output metadata.
2. Classify Port Commands against authoritative ControlPlane Port state.
3. Reserve and publish Commands for running destinations under existing capacity policy.
4. Record unavailable-destination Commands and schedule rejection ControlEvents.
5. Apply ControlCommands in deterministic production order.

A start or restart request does not make a Port available for sibling Port Commands in the same turn. The application must wait for the later `PortStarted` ControlEvent.

### Commands To Unavailable Ports

Port Commands are classified independently by logical destination:

- Commands targeting a running Port remain eligible for ordinary batch reservation and publication.
- Commands targeting a stopped, starting, stopping, or quarantined Port do not cross that Port boundary.
- Healthy-target Commands are not suppressed merely because another destination is unavailable.
- Every rejected Command receives a stable causal identity and, while acceptance remains nonfatal, a later ControlEvent reporting that it was not delivered and why. Fatal closure preserves terminal audit disposition instead.
- Rejected Commands are never retained for implicit delivery after restart.
- Commands already handed to an earlier incarnation are never retracted or silently resent and may remain externally ambiguous.

This per-Port unavailability rule is distinct from ordinary mailbox-capacity reservation. Existing whole-turn capacity reservation continues to prevent avoidable partial publication among Commands that are otherwise eligible. A concurrent Port failure may still divide Commands into handed-off, not-delivered, and externally ambiguous sets; diagnostics must report the exact known disposition.

### Quarantine And Handoff Linearization

Quarantine commitment and final Command handoff commitment for one logical Port are serialized by the ControlPlane against the same target incarnation:

- If handoff commits first, the Command belongs to the old incarnation and may be externally ambiguous after its failure.
- If quarantine commits first, the Command does not cross the boundary and receives not-delivered disposition.
- Classification and capacity reservation are preparatory and do not grant handoff authority.
- Immediately before making a reserved Command visible, the runtime revalidates that the exact target incarnation remains `Running` and commits handoff atomically with that check.
- A failed revalidation releases that Command's reservation and records not-delivered disposition without suppressing healthy-target Commands.

The monotonic control sequence and per-Command disposition are automatic audit data. They freeze a live failure-versus-handoff race without exposing control sequence as an application business identifier.

ControlCommands are applied only after every Port Command from the turn has reached an accounted post-turn disposition: published, rejected, or failed under a policy that terminates the turn. Any Engine-global failure, required-record failure, unaccountable publication failure, or turn-publication failure that prevents this completion skips all later ControlCommands from that turn.

## Port-Local Failure And Quarantine

The following are Port-local when the failure is contained to one or more known logical endpoints and the Kernel, ControlPlane, Acceptor, diagnostics requirements, and global Environment infrastructure remain trustworthy:

- Worker or process-proxy startup failure.
- Unexpected worker return.
- Captured worker panic under unwind builds.
- Port-local mailbox failure.
- Port-local prohibited overflow.
- Owned child-work panic, escape, or join failure.
- Simulated endpoint/model failure whose affected endpoints can be identified.
- Backend failure to realize a requested placement for one Port.

Port-local failure:

- Quarantines each affected logical Port.
- Revokes its current incarnation.
- Produces one ordered ControlEvent per affected logical Port.
- Starts or continues backend cleanup of the failed implementation and its children.
- Does not automatically restart any Port.
- Does not clear or repair application state.
- Does not claim that handed-off external effects are known.

Quarantine proves routing revocation, not implementation termination. A separate supervisor result establishes that the old incarnation has terminated or is fenced by a hard isolation boundary. The ControlPlane then transitions the logical Port to `Failed` and emits a later quiescence ControlEvent. Until that transition:

- Restart is rejected.
- Normal StopEngine is rejected.
- The Engine remains degraded and continues attempting or awaiting cleanup.
- A host may escalate to authoritative process termination.

### Grouped Implementation Units

One live or simulated implementation unit may provide several logical endpoints. Application topology and failure reporting remain endpoint-based.

If one endpoint fails while shared implementation state remains valid, only that endpoint is quarantined. If the implementation unit loses state required by several endpoints, the Environment privately identifies every affected endpoint, quarantines all of them before ordinary acceptance resumes, and emits one contiguous stable ordered `PortFailed` ControlEvent per logical Port under one failure-sequence identity.

There is no application-visible grouped Port identity. A shared model or worker is implementation topology, not application topology.

### Panic Boundary

A caught Port worker panic quarantines affected Ports. A Kernel, ControlPlane, or Engine-global Environment panic remains capture-and-stop fatal. With `panic = "abort"`, no in-process recovery is possible because the process terminates.

Port implementations that use unsafe process-global state or otherwise cannot contain failure to their declared endpoints cannot honestly promise recoverable in-process lifecycle. Such implementations should eventually use a process boundary when hard isolation is required.

## Engine-Global Fatal Failure

The fatal latch remains monotonic and first-failure-wins. It is reserved for failures that compromise Engine-wide execution authority, including:

- Kernel panic or invariant violation.
- ControlPlane panic or lifecycle-state corruption.
- Acceptor or global routing corruption.
- Global scheduler corruption or simulation causality violation.
- Required-diagnostics failure.
- Failure of publication bookkeeping such that Command disposition cannot be established.
- Global resource-limit failure defined as terminal.
- Environment failure whose affected logical endpoints cannot be safely identified or isolated.

Once fatal is established:

- No new Port Event or ControlEvent is accepted.
- No new turn begins.
- No new Port Command or ControlCommand takes effect.
- An active synchronous callback may return but is not followed by another callback.
- Unpublished outputs from the incomplete turn are abandoned and audited.
- Runtime cleanup begins and the Engine never resumes.

Port quarantine cannot clear the fatal latch. A failure is classified as contained before fatal establishment or it is terminal.

## Explicit Restart

Restart is always caused by an accepted application decision expressed as a ControlCommand. Configuration, elapsed time, supervisor policy, and Environment implementation may not restart a Port on their own.

An explicit restart:

1. Requires a `Failed` logical Port whose prior incarnation is confirmed terminated or isolated and no conflicting lifecycle operation.
2. Leaves the logical Port identity unchanged.
3. Allocates a new incarnation identity.
4. Constructs or resets backend implementation state according to the binding contract.
5. Never reuses old-incarnation Event authority.
6. Never silently transfers, retries, or resends old Commands.
7. Produces a later success or failure ControlEvent.
8. Does not imply connectivity, reconciliation, external-state recovery, or application arming.

After restart, ordinary Port Events and Commands perform application-defined reconnection and reconciliation. Strategy assumptions change only through accepted ControlEvents, Port Events, Messages, and Reducer projections.

## Child Work

Every child thread, task, process, job, or third-party runtime operation belongs transitively to one implementation unit and one lifecycle scope.

- No detached child work is permitted.
- New child creation stops when the owning endpoint or implementation begins stopping or is quarantined.
- The Environment initiates lifecycle cancellation; the implementation propagates it to children.
- Cancellation is cooperative and does not prove completion or rollback.
- Child failure is reported through the owning implementation and classified by affected logical endpoints.
- A worker is not stopped or joined until all owned children have terminated and been joined.
- Child-produced Events use the owning logical Port and incarnation authority.

Application-requested heavy work such as inference remains a service Port protocol. Environment worker pools are future placement mechanisms for Port-owned work, not capabilities exposed to Components.

A child-work timeout, escaped child, or failed join prevents the owning endpoint from leaving `Quarantined`. It cannot be treated as safe restart or successful normal Engine completion merely because routing authority was revoked.

## Normal Shutdown

Normal application-managed shutdown is explicit and has no hidden Port lifecycle behavior:

```text
cooperative ShutdownRequested ControlEvent
    -> application disarms and performs domain shutdown
    -> application requests StopPort for each active Port
    -> PortStopped or PortStopFailed ControlEvents
    -> application requests StopEngine
    -> terminal outcome to embedding host
```

Domain actions such as cancel orders, flatten positions, await acknowledgements, stop subscriptions, and reconcile external state occur through ordinary Port Commands and Events while the relevant Ports are running. Technical `StopPort` does not perform those actions implicitly.

The normal `StopEngine` request is accepted only when:

- Every logical Port is stopped or failed with its old incarnation confirmed terminated or isolated.
- No Port lifecycle operation remains pending.
- No ordinary application turn is incomplete.
- No earlier ControlEvent consequence remains pending.
- The producing turn emits no Port Command that requires publication or rejection feedback.
- `StopEngine` is the only lifecycle ControlCommand produced by that turn.

Otherwise the ControlPlane keeps the Engine running and emits a typed rejection ControlEvent. It does not silently stop remaining Ports.

After a successful `StopEngine` request, the producing turn reaches quiescence, ControlCommands are applied, acceptance closes, final technical cleanup runs, diagnostics are finalized, and the terminal outcome is delivered to the host. The application cannot receive a final `EngineStopped` ControlEvent because it no longer executes.

### Host Authority

The embedding host retains two distinct controls:

- A cooperative request that becomes a ControlEvent and permits application-managed shutdown.
- An authoritative technical stop that bypasses application approval and requests cleanup of all remaining runtime work.

An authoritative technical stop is serialized against Event acceptance. If acceptance wins first, that already accepted turn runs to quiescence and reaches a fully accounted Port Command disposition; no later input is accepted. If authoritative stop wins first, the offered input remains unaccepted. The ControlPlane then revokes all Port ingress and new handoff authority, requests stop or cancellation from every implementation, waits for owned child and worker termination where possible, records unresolved external ambiguity and cleanup failure, and returns a host-requested terminal outcome or shutdown error. It performs no implicit domain cancel, flatten, or reconciliation action.

External process termination remains the only hard preemption mechanism. It provides no callback completion, Port cleanup, join, diagnostic-flush, or consistent-state guarantee.

## Port Stop Semantics

A Port stop request is technical:

- New ordinary Commands to the Port become unavailable and are rejected visibly.
- The backend requests cooperative stop from the current incarnation.
- The implementation stops creating child work, cancels or completes owned work according to its contract, closes external resources, and joins children.
- Old-incarnation late Events are rejected after authority is revoked.
- A successful backend stop becomes a later `PortStopped` ControlEvent.
- Stop timeout or failure becomes `PortStopFailed` and leaves the Port quarantined.

Kavod cannot safely force-kill an arbitrary in-process thread. A deployment requiring bounded hard termination must use an external process boundary or terminate the entire Engine process.

## Simulation

The SimulationEnvironment implements the same logical ControlCommands and ControlEvents under one deterministic scheduler.

- `Ready` is the first accepted input.
- Simulated models and endpoints are constructed inertly.
- Start ControlCommands activate endpoints only after their producing turn completes.
- Placement requests may normalize to the same deterministic model mechanism.
- Model lifecycle output is staged and never re-enters the Kernel recursively.
- `PortStarted`, `PortFailed`, `PortQuiesced`, `PortStopped`, Command-rejection, and restart outcomes are scheduled ControlEvents.
- Explicit restart creates a new logical incarnation without hidden Command replay.
- Fault injection may fail an endpoint or implementation unit but may not automatically restart it.

Lifecycle outcomes, rejection consequences, and ordinary model Events staged at the same virtual time receive global schedule ordinals in commit order. A selected lifecycle outcome that changes routing enters the lifecycle barrier before the scheduler selects another ordinary action; it never re-enters application execution inline.

A grouped model is stored once. Logical endpoint activation and quarantine remain independently tracked where the model can preserve valid shared state. A model-wide failure privately affects every endpoint whose state is no longer trustworthy and reports each logical failure separately.

Basic historical simulation does not model real thread scheduling, async-runtime behavior, process isolation, IPC, or hard termination. Full placement-specific failure simulation remains future DST work.

### Simulation Completion

Existing `UntilIdle`, horizon, source-exhaustion, stalled, and action-limit semantics remain. Once technical completion commits, the SimulationEnvironment revokes remaining endpoint authority, accounts for pending lifecycle consequences and scheduled work under the selected completion policy, and performs model cleanup without starting another application turn. Technical simulation completion is a host outcome, not a ControlEvent emitted after completion. If application behavior must react to end-of-data, a simulated source emits an ordinary application-defined Event before completion.

## Replay

Replay treats application-visible control communication as part of deterministic execution:

- Recorded ControlEvents are injected in accepted Event-index order with recorded acceptance times.
- `Ready`, lifecycle outcomes, quarantine, and Command-rejection ControlEvents are replay inputs.
- Messages, Port Commands, and ControlCommands are recomputed.
- Port Commands and ControlCommands are compared with recorded expectations.
- A passive replay ControlPlane updates logical lifecycle state from the recorded ControlEvents and compares ControlCommands without starting workers or generating new lifecycle consequences.
- Placement requests are compared; recorded normalization or realization remains provenance rather than a requirement to reproduce physical mechanics.
- An application `StopEngine` request terminates replay at the same completed-turn boundary.

Recorded lifecycle, quarantine, and Command-rejection ControlEvents are the sole application-visible replay inputs for those consequences. Replay output handling must not synthesize duplicates. Produced Port Commands are compared as deterministic outputs; recorded handoff, rejection, and ambiguity dispositions are audit evidence rather than recomputed application inputs.

Raw supervisor reports, worker panics, host process termination, and other technical causes are not separately injected when their application-visible consequences already appear as recorded ControlEvents. Reproducing the exact pre-acceptance authority transition, publication disposition, or Engine-global fatal timing requires a future runtime-control/fault tape containing the relevant control sequence boundaries. It is not implied by ordinary accepted-Event replay.

These are semantic requirements for a future replay implementation and for in-memory conformance harnesses. They do not promote full or diagnostic replay into the MVP implementation scope established by the earlier reports.

## Observability

Automatic audit must distinguish control intent, backend action, application-visible fact, and terminal outcome. Required records include at least:

- `ControlEventAccepted` with complete payload, source, Event index, time, and causation.
- `ControlCommandProduced` with operation, target, requested placement, callback, root Event, and production order.
- Lifecycle operation accepted or rejected.
- Requested, realized, or normalized placement.
- Port incarnation allocated and revoked.
- Port start, stop, quarantine, and explicit restart transitions.
- Port-local failure and affected logical endpoints.
- Command handed off, rejected as unavailable, or externally ambiguous where known.
- Engine-global fatal latch with primary and secondary causes.
- Cooperative and authoritative host requests.
- Terminal run outcome.

Required-record policy applies at different authority boundaries:

- A Component-produced lifecycle operation, placement decision, or incarnation allocation must receive its required audit acknowledgement before backend invocation.
- ControlEvent acceptance follows the ordinary required-record gate before application dispatch.
- Immediate quarantine and ingress revocation cannot wait for diagnostics because delaying them could permit invalid handoff. The ControlPlane commits the safety transition first, then attempts its required record. Failure to acknowledge that record escalates the already-quarantined run to Engine-global fatal failure; it never rolls quarantine back.
- Required terminal-record failure may change the host outcome to diagnostics failure but cannot revive stopped work.

Application callbacks cannot observe diagnostics configuration or use diagnostics as control. Physical placement may appear in diagnostics without becoming a Component capability.

Metrics should make it possible to observe:

- ControlEvent and ControlCommand counts and latency.
- Port counts by technical lifecycle state.
- Start, stop, quarantine, and explicit-restart outcomes.
- Requested versus realized or normalized placement.
- Commands rejected by destination state.
- Old-incarnation late Events.
- Port-local failure versus Engine-global fatal counts.
- Child cancellation and join duration.

High-cardinality run, Event, Command, Port-incarnation, and business identities remain audit fields rather than metric labels.

## Concrete Traces

### Ready-First Live Startup

```text
build validates MarketData and Execution bindings
-> ControlPlane accepts Ready as Event 1
-> LifecycleController emits:
       StartPort(MarketData, Threaded)
       StartPort(Execution, Process)
-> turn completes
-> LiveEnvironment starts requested implementations
-> PortStarted(MarketData, incarnation 1) accepted
-> MarketData ingress opens after that turn
-> PortStarted(Execution, incarnation 1) accepted
-> Execution ingress opens after that turn
-> Port-defined Connected/Authenticated/Reconciled Events follow
-> application policy enters Armed
```

### Equivalent Simulation Startup

```text
same Ready turn
-> same StartPort ControlCommands
-> SimulationEnvironment normalizes both placements to model activation
-> same logical PortStarted ControlEvents
-> deterministic simulated operational Events follow
```

The simulation makes no claim that a process or OS thread physically exists.

### Cooperative Operator Shutdown

```text
host cooperative request
-> ShutdownRequested ControlEvent accepted
-> application enters Disarmed/Draining
-> ordinary Port protocols cancel, flatten, and reconcile
-> application emits StopPort for each active Port
-> PortStopped ControlEvents update AppState
-> application emits StopEngine
-> ControlPlane verifies no active Port remains
-> normal terminal outcome goes to host
```

### Worker Exit During A Turn

```text
Market Event turn is active
-> Execution worker exits
-> ControlPlane immediately quarantines Execution incarnation 7
-> active synchronous callback returns and turn reaches quiescence
-> Commands for healthy Ports remain publishable
-> Commands for Execution are marked not delivered
-> PortFailed(Execution, incarnation 7) is accepted before ordinary ingress resumes
-> CommandNotDelivered ControlEvents follow in stable causal order
-> LifecycleReducer updates AppState
-> backend cleanup later reports old incarnation quiesced
-> LifecycleController chooses restart, degraded operation, or shutdown
```

### Explicit Restart

```text
Execution is quarantined
-> old incarnation terminates or is isolated
-> PortQuiesced transitions Execution to Failed
-> application emits RestartPort(Execution, Process)
-> ControlPlane allocates incarnation 8
-> no old Command is resent
-> old incarnation Events remain rejected
-> Environment starts replacement
-> PortStarted(Execution, incarnation 8)
-> application performs ordinary reconnect and reconciliation
-> application decides whether to arm
```

### Shared Model Failure

```text
simulated venue model provides MarketData and Execution endpoints
-> endpoint-local Execution fault preserves market model validity
-> only Execution is quarantined and reported failed

or

-> model-wide corruption invalidates both endpoints
-> ControlPlane privately quarantines both before ordinary acceptance resumes
-> PortFailed(MarketData) accepted
-> PortFailed(Execution) accepted
```

No grouped application Port appears.

### ML Child Work During Port Stop

```text
Inference Port owns running child job
-> application completes domain policy and requests StopPort(Inference)
-> new inference Commands are rejected visibly
-> worker requests child cancellation and joins it
-> late old-incarnation completion Event is rejected
-> PortStopped or PortStopFailed ControlEvent is accepted
```

## Normative State Machines

### Engine

```text
Constructed
    -> Initializing
    -> Ready turn
    -> Running <-> Degraded
    -> Stopping
    -> Stopped

Any nonterminal state
    -> FatalLatched
    -> FatalStopping
    -> Failed
```

`Degraded` means at least one logical Port is stopped unexpectedly, quarantined, or failed while the Engine remains trustworthy. It does not itself determine application arming.

### Logical Port

```text
Stopped
    -- Start --> Starting
Starting
    -- success --> Running(new incarnation)
    -- failure --> Quarantined
Running
    -- Stop --> Stopping
    -- local failure --> Quarantined
Stopping
    -- success --> Stopped
    -- failure --> Quarantined
Quarantined
    -- old incarnation terminated/isolated --> Failed
Failed
    -- explicit Restart --> Starting(new incarnation)
    -- explicit Stop --> Stopped
```

No transition from `Quarantined` directly to `Starting` is permitted. Restart requires both backend-confirmed termination or isolation of the old incarnation and an accepted application ControlCommand from `Failed`.

## Settled Rules

1. Every Engine owns exactly one ControlPlane.
2. The ControlPlane is not a Port and has no PortSpec or binding.
3. ControlEvents and ControlCommands form the typed Engine/application control protocol.
4. ControlEvents use ordinary acceptance, Event-index, frozen-time, Reducer-before-Component, turn, and replay semantics.
5. Ordinary Reducers and Components handle ControlEvents.
6. Only ordinary Components produce ControlCommands.
7. Reducers remain output-free.
8. Every ControlCommand production is callback-declared and causally recorded.
9. ControlCommands are deferred until turn quiescence and never invoke the Environment inline.
10. Mandatory turn-output bounds include both Port Commands and ControlCommands.
11. The ControlPlane owns lifecycle authority; the Environment owns execution mechanisms.
12. Every logical Port has one validated inert binding before Ready.
13. Ready is the first accepted application input.
14. No Port worker or model is implicitly started before Ready.
15. No Port Event or Command handoff occurs before logical technical start.
16. Applications request logical Port lifecycle explicitly.
17. Placement requests never carry implementation or runtime handles.
18. LiveEnvironment realizes physical placement; SimulationEnvironment may normalize it.
19. Basic simulation need not model physical thread, task, process, or IPC behavior.
20. PortStarted means technical runtime availability, not connectivity, reconciliation, or application readiness.
21. Applications own operational projections and arming policy.
22. Port availability transitions create control barriers, not generic Event priority.
23. Port-local failures quarantine affected logical endpoints.
24. Port-local failure does not set the Engine-global fatal latch.
25. Application-visible failure identity is always a logical Port identity.
26. A failed shared implementation may quarantine several logical endpoints, reported separately.
27. Commands to unavailable Ports do not cross the Port boundary.
28. Healthy-target Commands remain eligible when another destination is unavailable.
29. Every unavailable-destination Command receives ControlEvent disposition while the Engine remains nonfatal, or terminal audit disposition after fatal closure.
30. Rejected Commands are never silently retained for restart.
31. Already handed-off Commands are never silently retried and may remain ambiguous.
32. No Port restarts automatically.
33. Explicit restart retains logical Port identity and creates a new incarnation.
34. Explicit restart requires confirmed termination or isolation of the old incarnation.
35. Old-incarnation Events are rejected before acceptance.
36. Restart does not imply external reconciliation or application arming.
37. All Port child work is transitively owned, cancelled, and joined.
38. Application-managed normal shutdown stops Ports before StopEngine.
39. StopEngine is rejected while active or unquiesced Port lifecycle remains.
40. The embedding host retains authoritative technical-stop and process-termination authority.
41. Only Engine-global failure establishes the fatal latch.
42. Fatal failure permits no later Event, ControlEvent, turn, Port Command, or ControlCommand.
43. Diagnostics are observational and never a control channel.
44. Terminal outcomes go to the embedding host, not to stopped application callbacks.

## Minimum Verification

The design must be proved by tests covering at least:

1. Ready is the first accepted input in live and simulation.
2. No Port worker/model starts before the Ready turn completes.
3. ControlEvent Reducers run before matching Components.
4. ControlCommands never execute inline or before turn quiescence.
5. Undeclared ControlCommand production fails visibly.
6. A Ready callback can request several Port starts in deterministic order.
7. Live placement requests resolve to the configured supported mechanisms.
8. Simulated placement normalization is deterministic and recorded.
9. Unsupported live placement yields visible start failure without fallback.
10. PortStarted is accepted before ordinary ingress from that incarnation.
11. Port Commands produced before PortStarted are rejected visibly.
12. Port startup failure quarantines only affected logical endpoints.
13. Worker exit and caught worker panic quarantine without setting fatal.
14. Kernel or ControlPlane panic sets fatal and permits no later turn.
15. Failure during an active callback does not preempt that callback.
16. A quarantined destination rejects Commands while healthy destinations publish.
17. Every rejected Command yields exactly one causally linked disposition event while acceptance remains open, or one terminal audit disposition after fatal closure.
18. No rejected or old Command is delivered after restart.
19. Restart is rejected until the old incarnation is confirmed terminated or isolated; a permitted restart allocates a new incarnation and rejects old-incarnation Events.
20. No restart occurs from elapsed time, configuration, or supervisor action alone.
21. A shared-model endpoint-local failure preserves unaffected endpoint operation.
22. A model-wide failure quarantines and separately reports every affected endpoint.
23. Port lifecycle ControlEvents update AppState before lifecycle Components react.
24. Strategy behavior can gate on projected Port status without runtime handles.
25. StopEngine is rejected while a Port is running, starting, stopping, or quarantined without confirmed termination or isolation.
26. Explicit Port stop followed by StopEngine produces normal completion.
27. Authoritative host stop bypasses application approval.
28. Replay injects ControlEvents and compares ControlCommands without starting live workers.
29. Repeated simulation runs produce identical control, lifecycle, and placement-normalization traces.
30. Audit records distinguish requested, realized, normalized, rejected, quarantined, fatal, and terminal states.
31. Combined Port Command and ControlCommand turn limits terminate deterministically without applying a partial control output set.
32. Quarantine and final Command handoff to one incarnation have exactly one serialized winner.
33. Late, duplicate, and stale backend reports cannot mutate a newer lifecycle operation or incarnation.
34. Multi-endpoint failure consequences are contiguous, stable, and expose completion of their common failure sequence.
35. Authoritative host stop racing Event acceptance either includes one complete final turn or leaves the offered Event unaccepted.

## Rejected Alternatives

- **ControlPlane as a Port:** creates false binding, worker, replay, restart, and authority semantics.
- **Internal or Engine PortSpec:** reuses syntax by introducing exceptions to nearly every Port rule.
- **Special lifecycle callback class:** duplicates ordinary Reducer/Component ordering and state visibility.
- **Direct Engine or Environment handles:** breaks deterministic capability isolation.
- **Inline ControlCommand execution:** permits reentrancy and mid-turn runtime mutation.
- **Eager startup of every Port:** hides application lifecycle decisions and prevents Ready-first bootstrap.
- **PortStarted as operational readiness:** conflates a run loop with connectivity and reconciliation.
- **Silent live placement fallback:** makes requested deployment behavior unknowable.
- **Requiring physical process/thread parity in basic simulation:** expands backtesting into adapter-level DST.
- **Treating a shared model as one application Port:** changes logical graph identity between live and simulation.
- **Universal fatal worker failure:** prevents the application from reacting to contained real-world infrastructure loss.
- **Automatic restart after quarantine:** hides recovery policy and risks stale Events, lost Commands, and duplicate effects.
- **Holding Commands until restart:** is implicit retry and changes effect timing without application authority.
- **Rejecting every healthy Command because one destination failed:** creates unnecessary cross-Port coupling.
- **Implicit Port shutdown inside normal StopEngine:** hides ending semantics from deterministic application logic.
- **Fatal as a ControlEvent:** promises application authority after the Engine has declared it cannot continue safely.

## Explicit Non-Guarantees

Kavod does not guarantee:

- That technical Port start means external connectivity or readiness.
- That restart restores external sessions, subscriptions, sequence state, or outstanding effects.
- That a Command handed to a failed incarnation completed or did not complete externally.
- That a basic SimEnv models OS scheduling, process isolation, IPC, async-runtime behavior, or physical contention.
- That live and simulation produce identical lifecycle timing or failure sequences without the same accepted Event tape.
- That arbitrary unsafe or process-global Port code can fail without compromising the process.
- That an in-process worker can be forcibly terminated safely.
- That application arming, disarming, flattening, cancellation, or reconciliation is automatic.
- That ControlPlane lifecycle audit is durable recovery authority.
- That terminal failure can be delivered to application callbacks after execution stops.

## Deferred Work

- Exact Rust representation of the composed control and application protocols.
- Exact callback registration and ControlCommand emission syntax.
- Stable durable identities for ControlEvent and ControlCommand variants.
- Async-task, pool, process, proxy, and grouped live implementation mechanics.
- Placement-specific simulation fault models.
- Process isolation and hard child-process termination.
- Durable Command outbox, restart recovery, resend, and reconciliation.
- Pause and resume semantics.
- Runtime graph mutation or dynamically introduced logical Ports.
- Adapter-level DST using controlled network, storage, time, randomness, and concurrency.
- Cross-build replay and schema migration.

## Reconciliation With Earlier 4.2 Reports

This report preserves:

- The narrow deterministic-kernel boundary from `1_determinism_time.md` while adding accepted ControlEvents and deterministic ControlCommands.
- One concrete application-owned `AppState` and output-free Reducers from `2_app_state.md`.
- One-input turns, Reducer-before-Component ordering, breadth-first Messages, and post-turn output deferral from `3_turn_scheduling_derived_state.md`.
- Separate live and simulated implementation mechanisms, grouped simulated models, virtual scheduling, and no reentrancy from `4_port_simulation_architecture.md`.
- Bounded queues, one acceptance authority, per-Port capacity, and visible failure from `5_runtime_backpressure_safety.md`.
- One Engine-owned diagnostics facility, causal records, required-record failure, and observational logging from `6_causal_trace_logs_observability.md`.

It supersedes only these earlier claims:

- Every application-visible Event originates from a Port.
- Every Component external output targets a Port.
- Every Port worker/model starts before the first application turn.
- Placement is exclusively static Environment configuration.
- Every worker/model startup failure, exit, or panic is Engine-fatal.
- A Control Port is the mechanism for Engine/application runtime control.
- Port restart is solely future supervisor policy rather than an explicit deterministic application request.

## Open Questions

No unresolved semantic question blocks the ControlPlane, Ready-first bootstrap, Environment backend, endpoint quarantine, explicit restart, or application-managed shutdown model.

The public Engine, Environment, Port, registrar, and context interfaces remain intentionally deferred until these semantics are reflected consistently across the other 4.2 reports.
