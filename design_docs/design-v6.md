# Kavod Core Design v6

> **Status:** Draft semantic design for validation
> **Scope:** MVP application model, deterministic Kernel, state, static Ports, live and simulated Environments, mandatory audit, fail-stop behavior, and terminal outcomes
> **Primary priorities:** Robustness, explicit authority, bounded resources, and the smallest semantics that can be implemented and tested with confidence

---

## 1. Purpose, Scope, And Thesis

Kavod is a domain-agnostic deterministic application Kernel intended to support historical simulation and consequential live trading without changing deterministic application logic.

The one-line thesis is:

> A single-writer deterministic Kernel durably accepts an ordered sequence of typed Port Events and Engine Events, applies explicit canonical-state transitions, propagates internal Messages, durably records its deterministic decisions, and hands deferred typed Port Commands to static Ports through bounded fail-stop boundaries.

The MVP deliberately chooses fail-stop behavior over in-run technical recovery. A Port implementation that panics, exits unexpectedly, loses its boundary, or otherwise becomes technically untrustworthy terminates the complete Engine run. Kavod does not quarantine, replace, or restart a Port within that run.

The core run protocol is:

```text
stage candidate input
-> select candidate
-> InputAccepted + data synchronization
-> deterministic Reducer/Component execution
-> TurnComputed + data synchronization
-> one-pass Port Command handoff
-> TurnCommitted + data synchronization
-> select next candidate
```

This document defines the semantic contract, not final Rust syntax. Trait shapes, queue implementations, audit encoding, executor mechanics, and concrete error types remain implementation decisions where explicitly deferred.

## 2. Document Conventions

Normative language is used narrowly:

- **Must** states required behavior.
- **May** states permitted variation.
- **Does not guarantee** states an explicit boundary of responsibility.
- **Deferred** means the capability or exact interface is intentionally outside this semantic design.

Port Event, Engine Event, Message, and Port Command refer to distinct protocol classes. An **accepted input** is either an accepted Port Event or an accepted Engine Event.

An audit record is **synchronized** only when the Engine observes successful completion of the configured storage synchronization operation. Enqueuing to a writer, copying to userspace memory, writing to an OS page cache, flushing a language buffer, and completing a file write are not called durable unless the configured storage contract explicitly makes them so.

Storage may persist a complete record even when synchronization reports failure or the process dies before observing success. Journal inspection therefore distinguishes a valid persisted record from an Engine-observed synchronization commit. A trailing record cannot by itself prove that the Engine observed its own synchronization succeed.

## 3. Normative Terminology

| Term | Meaning |
|---|---|
| Application | The immutable graph, protocols, Components, Reducers, initial `AppState`, initial Component-private state, and application configuration supplied to one Engine run |
| Engine | The owner and coordinator of one run, including the Kernel, Environment, audit journal, static Port bindings, global run gate, and terminal outcome |
| Kernel | The single-threaded executor of accepted inputs, Reducers, Components, Messages, and deterministic outputs |
| Environment | The runtime that realizes static Port bindings, supplies input candidates and acceptance time, accepts Port Command handoffs, and owns live or simulated runtime mechanics |
| Port | A static logical typed boundary between the deterministic application and an external or simulated system |
| Port Spec | The application-defined contract associating one logical Port with its Port Command and Port Event types |
| Port Event | An immutable external fact staged by one Port for possible Kernel acceptance |
| Engine Event | An immutable built-in application-visible fact originating from the Engine; the MVP variants are `Ready` and `ShutdownRequested` |
| Accepted input | A Port Event or Engine Event whose complete `InputAccepted` record has synchronized successfully |
| Message | An immutable deterministic fact produced and consumed entirely within one application turn |
| Port Command | An immutable request for an effect through one static Port; it is not proof of external receipt, execution, or effect |
| Component | Deterministic application logic with private state and typed callbacks; Components may produce declared outputs |
| Reducer | A restricted stateless callback that alone may mutate canonical `AppState` and produces no output |
| `AppState` | The application's one concrete canonical shared-state value |
| Component-private state | Deterministic state owned by one Component instance and inaccessible to other Components |
| Staged input | An immutable candidate held by an Environment before acceptance; staging is not acceptance and creates no processing guarantee |
| Turn | Processing of one accepted input and its causally produced Messages, followed by durable computation, Command handoff, and durable completion, unless shutdown, host stop, or fatal establishment terminates it by an explicit rule |
| Quiescence | The ordinary-turn point at which the Message FIFO is empty and no callback is active |
| Acceptance commit | Successful synchronization of the complete `InputAccepted` record before dispatch |
| Computation commit | Successful synchronization of the complete `TurnComputed` record and all preceding semantic action records before Command handoff |
| Handoff | Successful transfer of one Port Command across Kavod's local boundary to its statically bound Port implementation or simulated model |
| Turn commit | Successful synchronization of `TurnCommitted` after every Port Command handoff for the turn succeeds |
| Audit journal | The mandatory append-only ordered record of Kernel-visible semantic actions and run boundaries |
| Audit storage contract | The declared set of process, kernel, host, power, filesystem, metadata, namespace, and device failures covered by audit synchronization |
| Global run gate | The monotonic Engine-owned authority that admits acceptance, callback execution, Message dispatch, ordinary commit synchronization, simulation actions, Command handoff, and startup release, and prevents later admissions after closure |
| Run phase | The small private Engine state machine `Constructing -> Starting -> Running -> Closing(reason) -> Terminated`; it is not a Port lifecycle |
| Event index | The authoritative monotonic total order of accepted Port Events and Engine Events |
| Logical time | The accepted input's frozen acceptance time, inherited by its complete turn |
| Business ID | Application-domain identity such as a client order ID; it is never replaced by a Kavod run, Event, Command, or audit identity |
| Fatal establishment | The monotonic point after which the Engine is poisoned and no later application execution or Port Command handoff may begin |
| `RunOutcome` | The terminal host report for a run that started, distinguishing completion, startup failure, authoritative host stop, simulation completion, and fatal failure |

## 4. Settled Invariants

1. One Kernel thread executes every Reducer and Component callback for one Engine run.
2. One Engine owns all deterministic application state; process-global mutable state must not affect deterministic behavior.
3. Accepted inputs are ordered exclusively by Event index and execute one at a time.
4. `Ready` is structurally the first accepted input and uses the ordinary Reducer-before-Component pipeline.
5. Reducers run before Components for every delivered Port Event, Engine Event, and Message.
6. Only Reducers mutate canonical `AppState`.
7. Components mutate only their own private state and receive immutable canonical-state access.
8. Reducers have no private mutable state, behavior-affecting mutable captures, or output capability.
9. Messages propagate breadth-first through one turn-local FIFO and never dispatch recursively.
10. Port Commands remain staged until ordinary computation reaches quiescence and `TurnComputed` synchronizes.
11. Actual callback registrations and callback-local output declarations are the graph's executable source of truth.
12. Producing an undeclared Message or Port Command, or invoking undeclared shutdown authority, is an invariant violation and fatal.
13. The application graph, static Port bindings, callback order, source order, capacities, and deterministic configuration are frozen before `Ready`.
14. Components and Reducers receive no Engine, Environment, Port implementation, scheduler, executor, channel, thread, task, process, wall-clock, audit-writer, or external-IO handle.
15. Protocol payloads are immutable after staging or deterministic production.
16. Port queue insertion is staging only; successful `InputAccepted` synchronization is the sole acceptance commit.
17. No callback receives an execution admission before its root input's acceptance commit.
18. No Port Command crosses a Port boundary before `TurnComputed` synchronizes.
19. Port Commands are considered in global production order and receive at most one handoff attempt.
20. The first denied, failed, or indeterminate Command handoff establishes terminal closure and suppresses every later handoff attempt.
21. `TurnCommitted` exists only after every Command handoff for that turn succeeds.
22. Kavod never automatically retries or resends a Port Command.
23. Every technical Port failure terminates the complete Engine; failure before static preparation completes yields startup failure, while failure after the run enters `Running` is Engine-global fatal.
24. Once the run phase leaves `Running`, it never returns; application or simulation completion may be promoted to fatal if required terminal audit or cleanup fails.
25. Fatal state and state from a fatally incomplete turn are never reused.
26. Live and simulation share application semantics and typed Port protocols, not physical runtime behavior.
27. Application callbacks cannot observe the selected Environment mode.
28. Every Kavod-controlled queue, payload, turn, output collection, audit buffer, scheduler chain, and identifier domain has a finite bound.
29. Kavod identity never substitutes for application-owned business identity or idempotency policy.

## 5. Explicit Non-Goals And Deferred Capabilities

The MVP does not provide or promise:

- Deterministic behavior before acceptance in live mode.
- Deterministic live networks, brokers, OS scheduling, or external effects.
- Port-local failure containment, quarantine, restart, hot replacement, or runtime placement changes.
- Dynamic Port introduction or runtime graph mutation.
- A durable Command outbox, automatic resend, exactly-once delivery, or external-effect rollback.
- State snapshots, restoration, migration, or Engine continuation after failure.
- Replay execution, cross-build replay compatibility, or recovery from the audit journal.
- Full deterministic simulation testing, generalized fault injection, schedule exploration, or shrinking.
- Generic domain concepts such as order, fill, disconnect, reconciliation, arming, safe-to-trade state, cancel, or flatten.
- Generic joins, watermarks, state-settled callbacks, Reducer-produced Messages, or runtime priorities.
- Safe forced termination of arbitrary in-process callbacks, threads, or third-party code.
- Detailed simulation horizon, source-exhaustion, and completion policy semantics; these remain `SimulationEnvironment` configuration work.
- Final public Rust trait, builder, context, registry, queue, error, identity, or audit-encoding syntax.

The audit journal is evidence, not recovery authority, application state, broker truth, or an outbox.

## 6. Architecture And Authority

Kavod has one deterministic application path and three private supporting boundaries:

```text
Port or Engine input candidate
        |
        v
      Engine ---- mandatory semantic records ----> Audit journal
        |
        v
      Kernel ---- Port Commands ----------------> Environment ----> Port
        |
        +---- optional logs/metrics ------------> Observability
```

Engine Events enter the same accepted-input pipeline as Port Events. They do not form a separate lifecycle plane.

| Participant | May send | May receive | Owns or authoritatively decides | Must not do |
|---|---|---|---|---|
| Embedding host | Cooperative shutdown request, authoritative stop, process termination | Build errors and `RunOutcome` | Whether the containing process continues | Treat cooperative shutdown as immediate preemption |
| Engine | Accepted-input dispatch, static runtime coordination, terminal outcome | Input candidates, host requests, fatal reports | Acceptance, global run gate, audit boundaries, one run | Expose private runtime machinery to callbacks |
| Kernel | Typed callback delivery and deterministic output batches | Accepted Port Events and Engine Events | Callback order, Message FIFO, deterministic application state | Perform Port IO or inspect Environment mode |
| Environment | Port Event candidates and private failure reports | Port Commands and run closure | Static bindings, runtime resources, clocks, queues or scheduler, local Port boundary | Read or mutate application state |
| Port implementation or model | Port Event candidates and private technical failure reports | Port Commands and run closure | External or simulated-world state | Invoke the Kernel or read application state |
| Reducer | No semantic outputs | Port Events, Engine Events, Messages, mutable `AppState` | One canonical-state transition while invoked | Perform IO, retain state references, or hide mutable state |
| Component | Declared Messages, Port Commands, shutdown intent, optional user logs | Port Events, Engine Events, Messages, immutable `AppState` | Its private state and declared deterministic decisions | Mutate `AppState` or access runtime machinery |
| Audit journal | Storage failure | Mandatory semantic records | Durability at declared commit points | Authorize application behavior, handoff, replay, resend, or recovery |
| Optional observability | Nothing authoritative | User logs, metrics, traces | Its own projection and retention | Affect callback control flow or consume reserved audit capacity |

The Kernel remains the sole sequencer of application semantics. The global run gate is the one explicit cross-thread exception to ordinary single-writer mutation: runtime threads may atomically request a monotonic transition from running to fatal closure. The winning closure operation stores its cause and wakes the Engine. Runtime threads cannot mutate application state, assign Event indices, sequence callbacks, publish Commands, or begin cleanup themselves.

Admission under the gate is the linearization point for beginning one acceptance operation, callback invocation, Message dispatch, ordinary commit synchronization, simulation action, Command handoff, or startup release. A closure request atomically prevents new admissions while allowing operations already admitted to reach their defined boundary. The Engine waits for admitted operations and the active callback boundary before snapshotting terminal frontiers or starting cleanup.

### 6.1 Identity Discipline

V6 introduces identity only where one invariant requires it. The semantic minimum is:

- One run identity for external correlation.
- One Event index for accepted-input order.
- One static logical Port identity for source attribution and Command routing.
- One stable callback registration reference for graph authority and audit.
- One turn-local action or Command ordinal for deterministic production order.
- One audit-record sequence for journal order and framing validation.
- Application-owned business identities inside domain payloads.

Simulation additionally needs one private schedule ordinal to order equal virtual time. Exact representations are deferred. V6 has no implementation-unit, incarnation, lifecycle-operation, failure-sequence, restart, or generalized causal-tree identity.

## 7. Protocol Semantics

### 7.1 Closed Typed Protocols

An application supplies enumerable closed concrete Port Event, Message, and Port Command protocol manifests. Kavod supplies the closed Engine Event protocol.

Callbacks receive concrete typed payloads, never a top-level `dyn Message`, `Any`, or user-visible downcast. Port Event routing is keyed by static logical Port plus declared event variant, so identical Rust payload types may appear in different Port protocols without losing source authority. Engine Event routing is keyed by built-in variant and Message routing by declared Message variant. Narrow internal erasure may be used only if it preserves these typed keys.

### 7.2 Port Event

A Port Event is an immutable external fact staged by one logical Port. Examples include a quote, historical market occurrence, broker response, timer firing, service result, connection loss, or authentication result.

Expected external negative outcomes are domain facts, not technical Port failure, when the Port implementation remains trustworthy enough to report them through its declared protocol.

Port Events may carry domain timestamps and business IDs. The Kernel never reorders them by domain time.

### 7.3 Engine Event

The MVP Engine Event protocol contains exactly:

- `Ready`, the structurally first accepted input.
- `ShutdownRequested`, the one-shot cooperative host request.

Both use ordinary acceptance, Event-index, logical-time, Reducer-before-Component, Message, output-declaration, and audit semantics.

Every built-in Engine Event must have at least one registered Reducer or Component. Application construction fails otherwise.

Only the first cooperative host shutdown request stages `ShutdownRequested`. Later cooperative requests are idempotent, report that shutdown was already requested through the host API, and create no additional Engine Event.

Fatal failure, authoritative host stop, cleanup failure discovered after application execution, and terminal completion are not Engine Events because application execution has no authority to react after those boundaries.

### 7.4 Message

A Message is an immutable deterministic fact produced by a Component and consumed inside the current turn. It inherits the root input's logical time, appends to one breadth-first FIFO, and never crosses a Port boundary.

Future work is represented through a Port protocol, not by retaining or scheduling a Message:

```text
SetTimer Port Command -> Timer Port -> TimerFired Port Event
RunInference Port Command -> Inference Port -> InferenceCompleted Port Event
```

### 7.5 Port Command

A Port Command is a directed immutable request to one static logical Port. It proves neither implementation receipt nor external transmission, execution, completion, or effect.

Externally consequential protocols must include application-owned deterministic business identity or idempotency information sufficient for reconciliation. Kavod's run-scoped `(Event index, action ordinal)` references do not satisfy that obligation.

### 7.6 Shutdown Intent

`ctx.shutdown()` is a graph-authorized, payload-free request for normal application completion. It is not a Message, Port Command, or Engine Event. Its terminal control-flow semantics are defined in Section 15.

## 8. Static Application Graph

### 8.1 Executable Source Of Truth

The graph is derived from executable registration metadata:

```text
Port     -- Port Event   --> callback
Engine   -- Engine Event --> callback
callback -- Message      --> callback
callback -- Port Command --> Port
callback -- shutdown     --> Engine terminal boundary
```

Registering a callback creates an input edge. A callback-local declaration creates an authorized output edge. A declaration means **may produce**; it does not promise that arbitrary callback code will produce the output.

Every runtime output must be authorized by the currently executing callback's declarations. A callback that may call `ctx.shutdown()` must declare that authority explicitly.

### 8.2 Construction

Construction is separated by owner:

```text
Application construction
    -> protocols, AppState, Components, Reducers, callbacks, declarations, stable order

Environment construction against the Application
    -> one static binding per Port, capacities, private live or simulated mechanisms

Engine construction
    -> Kernel, global run gate, audit journal, fixed source order, terminal coordination
```

### 8.3 Validation

Before execution, construction must validate at least:

- Every declared Port Event variant has at least one matching callback.
- `Ready` and `ShutdownRequested` each have at least one matching callback.
- Every declared Message production has a matching callback.
- Every Port Command production targets one declared Port.
- Every callback input and output belongs to the correct closed protocol.
- Every callback's production and shutdown declarations are internally valid.
- Reducer and Component registration and fan-out order are stable.
- One initial `AppState` of the required concrete type is supplied and passes configured validation.
- Every declared Port has exactly one compatible static binding.
- No binding exists for an undeclared Port.
- Queue, mailbox, turn, payload, scheduler, and audit bounds are finite and valid.
- The complete run provenance required by the audit journal is available.

Potential Message cycles may be reported but are not automatically invalid because declarations mean “may produce.” Mandatory runtime bounds prevent an actual cycle from running forever.

### 8.4 Immutability

The graph, callback order, Port declarations, Port bindings, source order, capacities, deterministic configuration, and audit limits are frozen before `Ready`. The MVP permits no runtime graph mutation, Port introduction, callback registration, or subscription mutation.

## 9. Application State And Callback Authority

### 9.1 State Classes

Kavod recognizes four relevant state classes:

| State | Logical owner | Physical holder | Mutation authority |
|---|---|---|---|
| Canonical `AppState` | Application | Engine | Reducers |
| Component-private state | Component instance | Engine | Owning Component callbacks |
| Live implementation state | Port implementation | Environment | Owning implementation |
| Simulated external-world state | Model | Simulation Environment | Owning model callbacks |

Kernel ordering state, Environment mechanisms, and audit-writer state are private runtime state, not application state.

### 9.2 Canonical `AppState`

`AppState` is one concrete typed root, not a service locator, `TypeId` cache, state-slot registry, or collection of independently owned projector stores.

Only Reducers receive mutable access. Components may read the complete immutable root. `AppState` must not use interior mutability to bypass Reducer-only mutation.

State fields and dynamic entities are not graph nodes. Kavod does not pretend to know which fields arbitrary callback code reads or writes.

### 9.3 Component-Private State

One Component may share its private state among its own callbacks. Other Components cannot access it. Information shared across Components belongs in canonical `AppState` and changes through Reducers.

### 9.4 Reducers

A Reducer:

- Consumes one typed Port Event, Engine Event, or Message.
- Receives temporary mutable access to complete `AppState`.
- May change several related fields in one cohesive transition.
- Has no private mutable state or behavior-affecting mutable capture.
- Emits no Message, Port Command, shutdown request, or user-controlled result.
- Performs no external IO or blocking work.
- Retains no state reference after return.

Several Reducers may consume the same payload and run in stable registration order. Stable order guarantees reproducibility, not semantic independence. Fields that must remain coherent for one fact should be transitioned by one cohesive Reducer callback or one cohesive `AppState` operation.

Reducer completion is callback isolation, not rollback. A panic after partial mutation makes the run fatal and state unusable.

### 9.5 Components

A Component callback may:

- Read immutable `AppState`.
- Mutate its own private state.
- Read the root input's frozen logical time.
- Emit declared Messages and Port Commands.
- Invoke `ctx.shutdown()` when declared and dynamically legal.
- Emit optional write-only user logs.

It must remain synchronous, nonblocking, and deterministic. Heavy or external work belongs behind a Port.

Components and Reducers must not observe Environment mode. Ordinary Rust code is not sandboxed, so testing, linting, dependency review, and code review remain necessary.

## 10. Startup

Startup is one Engine-run protocol, not one lifecycle per Port:

```text
validate Application and Environment
-> create and synchronize RunStarted audit provenance
-> prepare every static Port runtime and local boundary
-> atomically enter Running
-> keep ordinary Port activity gated
-> accept EngineEvent::Ready as Event index 0
-> run Ready through ordinary Reducer/Component semantics
-> synchronize TurnComputed
-> hand off Ready Commands
-> synchronize TurnCommitted
-> release ordinary Port activity
```

Successful `RunStarted` synchronization establishes that a run exists. Append or synchronization failure before the Engine observes that success returns a typed run-start error rather than `RunOutcome`; a complete trailing `RunStarted` record may still exist and is interpreted using Section 16.3.

Port Command sinks must be available before the Ready turn begins. Autonomous external or model activity is forbidden before the Ready turn commits. A Ready Command may cause a simulated endpoint to stage a response, but that candidate remains ineligible for selection until Ready commits. Live workers remain behind the ordinary-activity gate and therefore cannot process Ready Commands until release.

`Ready` means only that the graph, audit journal, Engine, and static runtime boundaries can begin the run protocol. It does not mean connected, authenticated, subscribed, reconciled, or safe to trade.

If static runtime preparation fails before the Engine enters `Running`, the Engine accepts no `Ready` input, transitions from `Starting` to `Closing(StartupFailed)`, cleans up prepared resources, attempts terminal startup-failure records, and returns startup failure if cleanup reaches a return boundary. Failure of terminal audit is reported as secondary and cannot turn failed startup into success.

Successful preparation atomically transitions `Starting -> Running` before the Engine requests Ready acceptance admission. Technical Port or audit failure after that transition is fatal even if `Ready` has not committed. A concurrent startup failure and preparation completion follow whichever phase transition wins; Ready acceptance cannot begin from `Starting` or after closure.

After the Ready turn commits, the Engine admits ordinary-activity release through the global run gate. If application shutdown, host stop, or fatal closure has already won, release does not occur. If the Ready turn legally invokes `ctx.shutdown()`, ordinary Port activity never opens.

The exact thread, barrier, and `PortContext` startup API is deferred. It must preserve the ordering above.

## 11. Input Staging, Selection, Time, And Acceptance

### 11.1 Live Staging

Each live Port binding has one bounded FIFO staging queue for immutable Port Event candidates. Queue insertion:

- Freezes payload and logical source.
- Makes the candidate eligible for later selection.
- Does not assign Event index, logical time, or causal identity.
- Does not promise acceptance, processing, durability, or survival of run termination.

Within one source, FIFO preserves successful staging order. Kavod never silently removes, overwrites, rewrites, reprioritizes, or coalesces a candidate after successful staging.

A concrete Port may perform domain-defined batching, snapshot replacement, deduplication, or coalescing before offering a Port Event when its protocol makes that behavior explicit. Once offered to Kavod, staging-queue exhaustion is a technical boundary failure and fatal. Kavod does not provide configurable full policies in the MVP.

Staged but unaccepted inputs are abandoned when application shutdown, authoritative host stop, or fatal closure ends acceptance. They are not application history and require no synthetic input or per-Event disposition.

### 11.2 Engine Event Ingress

`Ready` is injected structurally before ordinary selection begins. After Ready, the one-shot cooperative host request occupies one Engine Event source in the fixed live source order. Repeated cooperative requests create no additional candidate.

### 11.3 Fixed Live Selection

The live Environment uses one frozen cyclic source order:

```text
Engine Event source
-> first declared Port
-> second declared Port
-> ...
-> final declared Port
-> repeat
```

The selector keeps a cursor. At each source visit it selects at most one head candidate. If selected, the Engine completes that input's full run protocol before visiting the next source. Empty sources are skipped immediately. New candidates at an already visited source wait for the next cycle.

This algorithm is fixed for the MVP. There is no public selector trait, priority, weight, or configurable quantum.

The algorithm bounds consecutive selections from one source but not wall-clock latency. One long synchronous callback or storage synchronization may delay every source.

### 11.4 Acceptance

For one selected candidate, the Engine:

1. Validates source and protocol membership.
2. Obtains one acceptance admission from the global run gate.
3. Assigns the next Event index.
4. Freezes acceptance time and establishes the Event index as the turn's causal root.
5. Appends the complete `InputAccepted` record.
6. Synchronizes the audit journal.
7. Commits acceptance only if the Engine observes synchronization success.
8. Releases the acceptance admission.
9. Dispatches no callback unless synchronization succeeded and the gate grants callback admission.

Successful synchronization is the acceptance commit. Selection and queue removal are private preparation, not separate semantic commits.

If the Engine observes synchronization failure, the candidate does not commit as an accepted input in the live run, no callback receives it, and the run fails. The candidate is not restored or retried. After crash, a complete trailing `InputAccepted` may exist despite the failed or unobserved result; its offline interpretation is synchronization-indeterminate until a later record proves the Engine proceeded beyond that boundary.

If the Engine observes `InputAccepted` synchronization success but fails before dispatch, the live run truthfully contains an accepted-but-unprocessed input. Offline inspection applies the trailing-record rules in Section 16.3.

An acceptance operation admitted before concurrent closure may finish its synchronization. Closure prevents any later acceptance operation from beginning. If the Engine observes acceptance success and then observes closure before callback admission, the input remains accepted but unprocessed.

### 11.5 Time

- Domain time remains ordinary payload data.
- Event index, not time, establishes accepted-input order.
- Every callback, Message, and Port Command in one turn inherits root logical time.
- Messages do not advance time.
- Live acceptance time uses a wall-clock anchor plus monotonic elapsed time and is nondecreasing.
- Simulation supplies virtual acceptance time from its deterministic scheduler.
- Equal acceptance or domain timestamps are legal and do not combine inputs.

## 12. Ordinary Turn Protocol

After acceptance commits, an ordinary turn proceeds:

1. Create callback contexts containing frozen logical time and only declared capabilities.
2. Obtain callback admission from the global run gate before each Reducer invocation.
3. Append `ReducerStarted`, invoke the Reducer, and append `ReducerCompleted` only after normal return.
4. Release callback admission and continue through matching Reducers in stable registration order.
5. Obtain callback admission before each Component invocation.
6. Append `ComponentStarted` and invoke the Component.
7. At each `ctx.message()` call, stage the Message in the turn-local FIFO and append `MessageProduced` immediately in production order.
8. At each `ctx.command()` call, stage the Port Command turn-locally and append `PortCommandProduced` immediately in global production order.
9. Append `ComponentCompleted` only after normal return, release callback admission, and continue through matching Components in stable registration order.
10. Obtain Message-dispatch admission, remove the next Message, release that admission, and repeat Reducer-then-Component dispatch subject to fresh callback admissions.
11. Reach quiescence only when the Message FIFO is empty and no callback is active.
12. Finalize deterministic state, action counts, output ordinals, and the complete ordered Port Command set.
13. Obtain computation-commit admission, append `TurnComputed`, synchronize it with every preceding action record, and release admission after observing the result.
14. Attempt Port Command handoff as defined in Section 14.
15. Obtain turn-commit admission, append `TurnCommitted`, synchronize it with all handoff records, and release admission after observing the result.
16. Only then select another input.

Callback admission, not the machine instruction entering user code, is the callback-start linearization point. Closure may occur after admission while that callback executes. On return, the Engine observes closure, releases the admission, and suppresses all remaining callbacks, Messages, and turn-local Commands. Production records from the active callback remain truthful records of uncommitted work; no `TurnComputed` is created for that incomplete ordinary turn.

Callback admission completes on both normal return and unwind. Completion audit records exist only for normal return. An unwind completes the admission as terminal failure, establishes or reports fatal closure according to the current phase, and permits terminal draining to continue without invoking later work.

An ordinary audit synchronization operation begins only after obtaining its named gate admission and may finish after a concurrent closure request. If `TurnComputed` synchronization succeeds, the computed frontier advances but closure prevents the first handoff. If `TurnCommitted` synchronization succeeds, the committed frontier advances but closure prevents the next input. Cleanup begins only after the active synchronization and all other admitted operations drain. Terminal audit performed after an authorized closing transition is the explicit exception: it is authorized by the closing phase rather than by a `Running` admission.

Engine-observed `TurnComputed` synchronization establishes that deterministic callbacks completed and intended outputs were recorded; it does not establish resulting state independently of those callbacks or prove Port handoff. Engine-observed `TurnCommitted` synchronization establishes successful local handoff of every Port Command in that turn, not external receipt or effect.

Finite bounds are mandatory for at least:

- Callback invocations per turn.
- Messages per turn.
- Port Commands per turn.
- Encoded bytes per input, Message, and Port Command.
- Semantic action records and encoded audit bytes per turn.

Exceeding a bound establishes fatal state, publishes no uncommitted output, and produces a typed fatal cause where possible.

## 13. Derived-State Consistency

Reducer-before-Component visibility is local to the currently delivered payload. A Component handling payload `X` sees every canonical transition registered directly on `X`. It does not necessarily see changes caused by sibling or descendant Messages still waiting in the FIFO.

Turn quiescence is not a retrospective state barrier. A Port Command created from stale state is not recomputed or withdrawn because later Messages changed state before publication.

The MVP rule is:

> If one decision depends on several related changes caused by one input, the producer must represent the complete required set as one Port Event or Message, the canonical fields that must remain coherent must transition within one cohesive Reducer callback, and the decision callback must consume that complete fact.

For multi-timeframe bars:

```text
Tick
-> BarAggregator updates all private builders
-> BarsClosed([1m, 5m, 15m, daily])
-> one cohesive Reducer projects every required completed bar
-> Strategy consumes BarsClosed once against the coherent projection
```

Aggregate completeness and domain ordering remain application obligations verified through tests and review. Equal timestamps do not create atomic groups. External observations requiring atomic application treatment must be represented by one application-defined batch Port Event.

## 14. Port Command Handoff

### 14.1 One-Pass Algorithm

After `TurnComputed` synchronizes, the Engine considers Port Commands once in global production order:

```text
for each Port Command in production order:
    acquire one handoff admission from the global run gate
    append PortCommandHandoffStarted
    perform one endpoint transfer attempt
    record the transfer result
    complete the admission as success or terminal failure
```

The static graph already proves that the destination exists, the Command type matches, the callback declared the output, and one binding exists. There is no destination lifecycle authority to classify or revalidate.

Successful handoff admission is the operation-start linearization point against fatal closure and authoritative host stop. Closure denies new admission but waits for an already admitted transfer attempt to finish. An admitted attempt can atomically close the gate as fatal without reopening a window for the next Command.

Every bound handoff operation must define one exact local transfer linearization point and return one of:

- **Handed off:** the Command definitely crossed Kavod's local boundary.
- **Not handed off:** the Command definitely did not cross.
- **Indeterminate:** Kavod cannot establish whether the Command crossed.

After admission, failure to append `PortCommandHandoffStarted` requests fatal closure before transfer. After the attempt, the Engine appends `PortCommandHandoffResult` with `HandedOff`, `NotHandedOff`, or `Indeterminate`. Failure to append that result record does not undo or resolve the transfer; it requests fatal closure while retaining the known in-memory certainty. Failure before transfer admission means no transfer began. Panic or boundary failure spanning the transfer point is indeterminate unless the concrete boundary can prove one side.

If the global gate denies admission because host stop or fatal closure already won, no transfer begins and publication ends under that existing closing reason. Gate denial does not replace an authoritative host stop with a new fatal cause.

There is no:

- Destination grouping.
- Turn-local destination transaction.
- Reservation phase.
- Revalidation phase.
- Healthy-destination continuation after failure.
- Nonfatal rejection.
- `CommandNotDelivered` feedback Event.

### 14.2 Failure

Mailbox full, disconnected receiver, unavailable static boundary, simulated model failure, handoff-record failure, indeterminate transfer, or any other unsuccessful admitted attempt requests Engine-global fatal closure. If host stop or another closing reason already won, the handoff failure is secondary and cannot replace that primary reason.

On the first failure:

- Earlier confirmed handoffs remain handed off locally.
- The current Command is recorded in memory as handed off, not handed off, or indeterminate according to the boundary result.
- Later Commands are not attempted.
- No `TurnCommitted` record is created.
- Kavod does not retry any Command.
- Fatal audit and cleanup are attempted best effort.

Terminal evidence therefore consists of a confirmed handed-off prefix, at most one current terminal ordinal with explicit certainty, and an unattempted suffix. A plain negative result such as live mailbox full is definitely not handed off. A failure after known mailbox insertion is definitely handed off locally even when its audit record is absent.

### 14.3 Meaning Of Handoff

Live handoff linearizes at successful insertion into the bound Port implementation's mailbox; live mailbox admission is nonblocking. Simulation handoff linearizes when the addressed synchronous model callback is entered. A model callback must return normally before the Engine may continue to the next Command or commit the turn; a panic after entry is fatal and leaves model state unusable even though the Command crossed the simulated local boundary.

Handoff does not guarantee:

- Port worker receipt after a process crash.
- Network transmission.
- Broker or venue receipt.
- External execution.
- Exactly-once effect.
- Cross-Port atomicity.

If several domain operations require atomic intent, the application must represent them as one Port Command whose Port protocol defines the available external guarantee.

## 15. Normal Application Shutdown

### 15.1 Cooperative Policy

A cooperative host request stages the one built-in `ShutdownRequested` Engine Event. The application may use ordinary state, Messages, Port Commands, and later Port Events to disarm, cancel, flatten, reconcile, or perform any other domain-specific shutdown policy.

Kavod performs none of those business actions implicitly.

After domain shutdown is complete, an authorized Component may call `ctx.shutdown()` in a later output-free turn.

### 15.2 Dynamic Legality

`ctx.shutdown()` is legal only when:

- The executing callback declared shutdown authority.
- No Message or Port Command has been produced anywhere in the current turn.
- No earlier shutdown request exists in the current turn.

The call validates those conditions immediately, appends `ApplicationShutdownRequested`, and latches a provisional callback-local shutdown request. It cannot forcibly interrupt arbitrary Rust code. The callback must return normally before the request may close the run.

After the call, producing a Message or Port Command, or calling `ctx.shutdown()` again, is an invariant violation. User logging remains permitted because it has no semantic output authority.

Calling shutdown after any earlier current-turn Message or Port Command is also an invariant violation. Kavod never silently discards a previously staged deterministic output to claim clean completion.

### 15.3 Terminal Shutdown Turn

When the requesting callback returns normally, one atomic gate transition chooses application shutdown or observes that fatal or authoritative host-stop closure already won. A losing provisional shutdown request is suppressed, not retroactively illegal. When application shutdown wins:

1. Atomically close the global run gate for application shutdown while completing the callback admission.
2. Append `ComponentCompleted`.
3. Invoke no remaining callback.
4. Dispatch no queued Message.
5. Accept no later input.
6. Append `RunClosing` for application shutdown.
7. Append `TurnComputed` identifying intentional application shutdown, the cutoff callback, the shutdown intent, and an empty Message and Port Command set.
8. Synchronize the complete terminal computation record.
9. Perform an empty Command handoff phase.
10. Append and synchronize `TurnCommitted` identifying application-requested completion.
11. Wait for any previously admitted operation to drain, then stop and join static Port runtimes.
12. Append and synchronize `RunTerminated` after successful cleanup.
13. Return `RunOutcome::Completed`.

This turn is intentionally complete by the shutdown rule even though it does not reach ordinary Message quiescence or full callback fan-out.

If the callback panics after requesting shutdown, fatal failure wins. If required audit or cleanup fails after application shutdown begins, the private run phase is promoted from application closing to fatal; it never returns to running.

## 16. Audit Journal And Optional Observability

### 16.1 Mandatory Journal

Every run has exactly one mandatory Engine-owned append-only audit journal. It records every Kernel-visible semantic action using one fixed record set. There are no `Off`, `Debug`, `Trace`, sampling, or best-effort modes for this journal.

The initial record set includes:

- `RunStarted`
- `InputAccepted`
- `ReducerStarted`
- `ReducerCompleted`
- `ComponentStarted`
- `MessageProduced`
- `PortCommandProduced`
- `ApplicationShutdownRequested`
- `ComponentCompleted`
- `TurnComputed`
- `PortCommandHandoffStarted`
- `PortCommandHandoffResult`
- `TurnCommitted`
- `RunClosing`
- `RunTerminated`

Exact names and binary representation are deferred. The semantic distinctions are normative.

### 16.2 Run Provenance

Before static runtime startup, `RunStarted` must synchronize sufficient information to identify the run's complete deterministic starting point:

- Exact executable and application build fingerprint.
- Frozen graph, callback order, source order, and Port bindings.
- Initial canonical `AppState`.
- Initial Component-private state.
- Determinism-affecting application, Engine, and Environment configuration.
- Queue, mailbox, turn, scheduler, payload, and audit bounds.
- Audit schema, location, and durability contract.

For simulation, provenance also includes initial model and source state, source-corpus identity, initial virtual time, initial scheduled work, and every explicit deterministic seed or choice input.

The audit storage contract must state which failures successful synchronization covers, including its assumptions about process and kernel crashes, host power loss, filesystem and device behavior, volatile caches, file data and length, file and directory metadata, namespace operations, segments, manifests, and content-addressed artifacts. Every claim of durability in this document is relative to that declared contract.

Large starting values may be embedded or referenced by immutable content-addressed artifacts. Referenced artifacts must be finitely bounded, content-verified, synchronized, durably published, and covered by retention and capacity policy before `RunStarted` may reference them. The journal and referenced artifacts together must uniquely identify the initial deterministic state. This requirement does not grant snapshot restoration or replay authority.

### 16.3 Three Turn Durability Boundaries

During a surviving run, Engine-observed synchronization has these exact meanings:

| Engine-observed boundary | Runtime consequence |
|---|---|
| `InputAccepted` success | Acceptance committed; callback dispatch may be admitted |
| `TurnComputed` success | Computation committed; Command handoff may be admitted |
| `TurnCommitted` success | Turn committed; the next input may be selected |

The three synchronized points are:

1. `InputAccepted`, before dispatch.
2. `TurnComputed`, before the first Command handoff.
3. `TurnCommitted`, after the final successful Command handoff.

The MVP file-backed implementation performs a hard `fsync`-equivalent operation satisfying its declared storage contract at each point and waits for the result. Future batching or group commit may optimize physical synchronization only if it preserves the same per-run admission barriers and truth claims.

A journal inspector cannot infer the Engine's observation of a trailing synchronization result from record validity alone:

| Last valid persisted record after failure | Truthful forensic interpretation |
|---|---|
| No `InputAccepted` for a candidate | No acceptance commit occurred within the declared storage fault model |
| Trailing `InputAccepted` | The Engine appended the candidate; synchronization success and acceptance are indeterminate unless a later execution record proves dispatch |
| Trailing `TurnComputed` | Computation reached the boundary and intended Commands were recorded; synchronization success and any handoff remain indeterminate unless later handoff evidence exists |
| Trailing `PortCommandHandoffResult` | The Engine observed the recorded local transfer certainty before appending it; later transfer and turn-commit status remain unknown |
| Trailing `TurnCommitted` | Every local handoff succeeded before append; whether the Engine observed this record's synchronization and advanced its committed frontier is indeterminate |

A later-phase record proves that the Engine observed the prerequisite earlier synchronization because the protocol forbids that later phase otherwise. A pre-handoff record cannot prove handoff. A post-handoff record cannot eliminate the crash window between transfer and append or synchronization. Absence of `TurnCommitted` therefore does not prove that no Command crossed a Port boundary. Application business identity and external reconciliation resolve that ambiguity.

### 16.4 Framing And Bounds

The journal must use versioned framing, run and segment binding, complete-record detection, strong checksums, and monotonic record ordering sufficient to identify one valid prefix and detect a torn or corrupt suffix within the declared storage fault model. Inspection stops at the first invalid frame. Integrity mechanisms have explicitly documented residual failure probability and must not be described as proof against arbitrary corruption. The format contains no intentional internal gaps and must protect previously synchronized frames under its declared layout and storage assumptions.

Finite configured limits are mandatory for:

- One encoded record.
- One encoded input, Message, and Port Command.
- Semantic records and bytes per turn.
- In-memory staging and writer buffering.
- Total journal storage or preallocated segments.
- Reserved terminal evidence.

Before `RunStarted`, the audit facility must reserve bounded capacity for run provenance, referenced artifacts, startup failure, and terminal evidence. Before accepting another input, it must provide a preallocated maximum-sized turn buffer and confirm capacity for the inclusive worst case: `InputAccepted`, framing overhead, every permitted action record, `TurnComputed`, every handoff-start and handoff-result record, `TurnCommitted`, segment metadata, and protected terminal headroom. Post-computation records must be size-validated or pre-encoded before the first handoff. Terminal records and secondary-failure representations must themselves have fixed maximum sizes.

Capacity exhaustion before acceptance establishes fatal state without accepting the candidate. Unexpected write or synchronization failure remains possible and fatal.

Journal rotation, segmentation, preallocation technique, checksums, and synchronization optimization are deferred. No implementation may overwrite unretained audit history or silently downgrade durability.

### 16.5 Failure

Audit encoding, write, capacity, corruption, or synchronization failure while the run is `Running`, or while application or simulation completion still requires terminal audit, establishes or promotes to fatal state. Audit failure after startup failure, authoritative host stop, or an existing fatal closure is secondary because it cannot replace an authority boundary that already occurred. A failed synchronization never rolls back application mutation or a Command handoff that already occurred.

Authority closure happens before attempting its `RunClosing` audit record. For fatal, startup-failure, and authoritative-host-stop paths, `RunClosing`, `RunTerminated`, and final synchronization are best effort because the journal, runtime, or process may itself be failing.

`RunTerminated` records the closure reason, last known frontiers, cleanup result, and secondary failures known before the record was appended. It does not claim that its own synchronization succeeded. Application and simulation completion require the Engine to observe successful terminal synchronization; failure promotes the effective outcome to fatal audit failure. Startup failure, authoritative host stop, and fatal closure retain their primary category and report terminal-audit failure as secondary because closure already occurred independently of the journal.

### 16.6 Optional Observability

User logs, metrics, OpenTelemetry, profiling, console output, and external exporters are separate best-effort projections. They:

- Cannot consume audit-reserved capacity.
- Cannot affect callback behavior.
- Cannot substitute for a semantic record.
- May be filtered, sampled, lost, or disabled.
- Must not expose enablement or writer status to deterministic callbacks.

## 17. Host Authority, Fatal Failure, And Cleanup

### 17.1 Host Controls

The embedding host has three distinct controls:

- A one-shot cooperative request that becomes `ShutdownRequested`.
- An authoritative stop that bypasses application policy.
- External process termination for hard preemption.

An authoritative stop atomically transitions the private run phase from `Starting` or `Running` to `Closing(HostStop)` and closes the global run gate. It admits no later operation, performs no implicit business cancel, flatten, or reconciliation, and suppresses unfinished turn output at the next callback boundary.

An acceptance, callback, Message dispatch, synchronization, handoff, or startup-release operation admitted before closure may reach its defined boundary. Closure prevents the next operation from starting. The Engine waits for admitted operations and the active callback boundary to drain, snapshots terminal frontiers, attempts `RunClosing`, directs Environment cleanup, and then attempts `RunTerminated` with cleanup and secondary-failure status. Host and runtime threads never clean up resources concurrently with admitted Engine operations.

### 17.2 Run Phase And Precedence

The Engine maintains one small private run phase:

```text
Constructing -> Starting -> Running -> Closing(reason) -> Terminated
                    |                       ^
                    +-----------------------+
```

This run-wide state machine is not application-visible and does not recreate per-Port lifecycle. `Starting` may close as startup failure, authoritative host stop, or fatal audit failure before runtime preparation completes. Successful preparation is the only transition to `Running`. While `Running`, the first successful gate transition chooses application shutdown, simulation completion, host stop, or fatal as the closing reason. Later reports are secondary unless the following explicit promotion rule applies:

| Closing reason | Later required audit or cleanup failure |
|---|---|
| Application or simulation completion | Promote effective outcome to `Fatal` on required audit failure, cleanup failure, or unexpected technical runtime failure before successful termination |
| Startup failure | Retain `StartupFailed`; attach secondary failures |
| Authoritative host stop | Retain `HostStopped`; attach secondary failures |
| Fatal | Retain the first fatal cause; attach secondary failures |

The run phase never returns to `Running`.

When the Engine observes the winning transition it appends one `RunClosing` with the primary reason and current frontiers. Application shutdown synchronizes this record with its terminal `TurnComputed`. Simulation completion synchronizes it before cleanup. Startup failure, host stop, and fatal paths attempt it best effort after authority is already closed.

### 17.3 Fatal Causes

Fatal causes include:

- Kernel, callback, Port, model, selector, scheduler, or audit panic.
- Internal invariant violation or undeclared application output.
- Technical Port failure after the run enters `Running`, including unexpected return or worker exit before, during, or after the Ready turn.
- Staging queue overflow or corruption.
- Port Command mailbox full, disconnect, or failed handoff.
- Audit capacity, encoding, write, corruption, or synchronization failure.
- Configured turn, callback, Message, Command, payload, audit, or simulation limit exhaustion.
- Simulation causality violation such as scheduling into the past.
- Cleanup failure following requested normal completion.
- Any Environment failure that makes ordering, authority, or boundary accounting untrustworthy.

Expected external conditions remain typed Port Events when the Port implementation is still trustworthy. A broker rejection, exchange halt, remote disconnect, authentication expiration, or reconciliation mismatch is not automatically a technical Port failure.

### 17.4 Fatal Establishment

Fatal state is monotonic and first-failure-wins while the run is active. The winning atomic closure stores a bounded cause token and wakes the Engine. Every runtime worker and transitive child is supervised. During incomplete `Starting`, a technical Port failure requests `Closing(StartupFailed)`. During `Running`, it requests fatal closure. During application or simulation completion, an unexpected exit before the Environment issued that owner a stop request promotes the outcome to fatal. During startup failure, host stop, or existing fatal closure, such an exit is secondary. An exit after its cleanup stop request is expected. Only the Engine sequences detailed audit, frontier snapshots, and terminal reporting.

After fatal establishment:

- No later acceptance operation, callback, Message dispatch, or Port Command handoff begins.
- An active synchronous callback cannot be forcibly preempted.
- When it returns or unwinds, remaining callbacks, Messages, and staged outputs are suppressed.
- `TurnComputed` and `TurnCommitted` are not fabricated for an incomplete ordinary turn.
- In-memory application and model state is not reusable.
- Cleanup, child cancellation, joining, and final audit are best effort.
- No Port is quarantined, replaced, or restarted.
- No replacement Engine resumes the run.

Assertions and panic remain appropriate for impossible internal states. Known resource and infrastructure failures should retain typed fatal causes. Catching an unwind at the outer Engine boundary permits audit and cleanup attempts but never continuation.

### 17.5 Process Termination

External process termination provides no guarantee of callback completion, audit synchronization, Port cleanup, child joining, terminal outcome, or consistent state. Hard preemption of arbitrary in-process code requires process authority outside Kavod's deterministic contract.

### 17.6 Cleanup

Every thread, task, process, job, or third-party operation belongs transitively to exactly one static Port scope or one Environment run-wide scope. Privately shared passive state does not require a scope; privately shared active work must have one unambiguous Environment owner. Detached child work is forbidden.

After application shutdown, host stop, startup failure, or fatal closure enters `Closing` and admitted Engine operations drain, the Environment issues stop or cancellation and joins owned work. Cancellation proves neither completion nor rollback.

An in-process binding is required to provide cooperative stop and join, but arbitrary bugs may violate that contract. Kavod cannot return a truthful terminal outcome while forbidden detached work remains. If cleanup hangs, no `RunOutcome` is guaranteed even if the process itself remains alive. A deployment requiring bounded termination must place such work behind a killable process boundary and external watchdog.

Normal application shutdown becomes `RunOutcome::Completed` only after required cleanup and terminal audit succeed. Cleanup failure after application-requested shutdown becomes fatal cleanup failure.

Authoritative host stop remains `RunOutcome::HostStopped` and reports any cleanup failure that completed with a report. It makes no return-time promise for cleanup that never returns.

## 18. Static Ports And Runtime Ownership

A logical Port is immutable application topology, not a lifecycle object, worker, queue, model, or state owner. Every declared Port has exactly one compatible Environment binding for the entire run.

Live and simulation share:

- Logical Port identity.
- Port Event and Port Command protocol meaning.
- Graph routing and source attribution.
- Kernel turn semantics.
- Global Command production order.
- No reentrancy and no automatic retry.

They may differ in implementation interfaces, physical topology, scheduling, queueing, latency, input races, and handoff mechanism.

There is no application-visible or Engine-semantic grouping. If one live implementation supports several Port Specs, separate bindings may privately share passive thread-safe state through an `Arc<T>` or another implementation mechanism. Shared active work has one Environment run-wide owner rather than several binding owners. If one simulated world object supports several Port Specs, the Simulation Environment may serialize access to its shared model state. Shared implementation state creates no group identity, lifecycle, failure sequence, or partial recovery domain because any technical failure terminates the Engine.

Ports cannot be started, stopped, replaced, restarted, or moved by application callbacks. Technical preparation and cleanup are private Environment mechanics around one static run.

## 19. Live And Simulation Environments

### 19.1 Shared Application Semantics

Both Environments use the same:

- Application graph and callback code.
- Port Specs and payload meanings.
- Engine Events.
- `AppState` and Component-private state rules.
- Reducer-before-Component and breadth-first Message behavior.
- Acceptance, audit, Command ordering, shutdown, and fail-stop semantics.
- `RunOutcome` categories.

Application callbacks cannot branch on Environment mode.

### 19.2 Live Environment

Live Port implementations may perform concurrent external IO but communicate with the Engine only by staging immutable Port Events and receiving handed-off Port Commands.

The MVP live Environment uses one dedicated runtime thread per static Port binding. A binding may privately share passive thread-safe implementation state with another binding, but Kavod gives that shared state no group identity or lifecycle semantics. Transitive child work has exactly one binding owner or one Environment run-wide owner for cleanup.

The live Environment uses bounded per-Port staging queues, bounded Port Command mailboxes, fixed cyclic input selection, anchored monotonic acceptance time, and the global run gate.

Which live candidate wins an availability race is not deterministic. During the run, the Engine-observed accepted sequence is authoritative. After failure, the journal is evidence subject to the trailing synchronization ambiguity in Section 16.3 and never becomes replay or delivery authority.

### 19.3 Simulation Environment

A simulated model is a deterministic external-world state machine held by the Simulation Environment. The MVP Simulation Environment runs the scheduler, models, Kernel, Reducers, and Components on one thread with no overlapping callback. A model must:

- Produce the same transitions and staged outputs for the same model state, input, virtual time, configuration, and explicit deterministic seed or choice inputs recorded in run provenance.
- Run synchronously with no overlapping model, Reducer, or Component callback.
- Use only Environment-supplied virtual time.
- Consume Port Commands only through static bound endpoints.
- Stage Port Events only through static bound endpoints.
- Avoid wall time, OS IO, OS entropy, task scheduling, process-global mutable state, and unstable iteration that affects behavior.
- Never access application state or invoke the Kernel directly.
- Never retain callback context after return.

Model state represents the simulated external world and is not `AppState`.

Simulation handoff invokes the addressed model endpoint synchronously in global Command production order. One model callback must return and commit its staged outputs before the next Command handoff begins. Staged model outputs become later candidate actions and never recursively enter the Kernel.

Future actions are ordered by:

```text
(virtual_time, schedule_ordinal)
```

At equal virtual time, earlier committed actions precede later committed actions. Zero-latency output receives a later same-time ordinal rather than recursive execution or invented time.

After the Ready turn, the Simulation Environment executes one fixed loop:

1. Obtain simulation-action admission from the global run gate and pop the minimum `(virtual_time, schedule_ordinal)` action.
2. Advance virtual time to that action's time.
3. If it is a model wake or source action, run that callback once, commit its staged actions in production order only after normal return, and complete simulation-action admission on normal return or unwind. Unwind completes the admission as terminal failure and commits no staged actions.
4. If it is a Port Event or Engine Event delivery, convert it to one candidate, release simulation-action admission, and execute its complete acceptance, turn, audit, and Command-handoff protocol before selecting another scheduled action.
5. Commands delivered during that turn may stage later actions, including same-time actions with larger ordinals, but never reenter the Kernel.
6. A closure request may wait for an admitted model callback, but it prevents admission of the next scheduled action. On panic, causality violation, bound exhaustion, or closure, select no later action.

A cooperative host request in simulation stages one `ShutdownRequested` action at current virtual time with the next schedule ordinal. Repeated requests remain idempotent as in live mode.

Historical source and model code must preserve prefix causality:

```text
wake for next occurrence
-> consume exactly the due occurrence or explicit atomic batch
-> update external-world model state
-> stage the corresponding public Port Event
-> arrange the next occurrence
```

Future records must not influence earlier state, output, latency, or effect decisions. Scheduling into the past is fatal. Finite total-action and same-time-action bounds are mandatory.

Detailed source exhaustion, `run_until`, horizon, pending-work, and simulation-completion eligibility policy is deferred to `SimulationEnvironment` configuration. The common terminal boundary is not deferred: completion may be selected only between committed turns and scheduled actions, closes the global run gate, runs required cleanup, and attempts `RunTerminated`. Required terminal audit or cleanup failure promotes simulation completion to fatal. Environment-selected technical completion returns a terminal `RunOutcome`; it is not an Engine Event.

## 20. Determinism Boundary

Kavod's deterministic claim is:

> Given the same executable build, frozen graph and registration order, initial canonical and Component-private state, determinism-affecting configuration, and accepted Port Event and Engine Event sequence with identical payloads, sources, Event indices, and logical times, and assuming no technical interruption before the compared frontier or the same linearized interruption trace, the Kernel executes callbacks in the same order and produces the same Messages, Port Commands, shutdown intent, private-state transitions, and canonical-state transitions through the last Engine-observed `TurnCommitted` synchronization.

Deterministic inputs include:

- Exact executable and application build.
- Frozen graph, callback order, source order, and Port declarations.
- Initial `AppState` and Component-private state.
- Determinism-affecting application, Engine, Environment, turn, and audit bounds.
- Ordered accepted Port Events and Engine Events.
- Payload, source, Event index, and logical time of each accepted input.

Deterministic outputs include:

- Callback and delivery order.
- Ordered Messages.
- Ordered Port Commands and logical destinations.
- Shutdown intent and its invoking callback.
- Component-private state transitions.
- Canonical-state transitions.

Kavod does not guarantee:

- Which live candidate wins an ingress race.
- Identical accepted sequences from nominally identical live conditions.
- Identical physical latency.
- External Port Command delivery, execution, exactly-once effect, or cross-Port atomicity.
- Cross-build or cross-platform equivalence unless separately constrained.
- Determinism from application code that violates the conditional purity contract.
- The exact point of an asynchronous failure unless that interruption is supplied as an additional fault input.
- A reusable state value after fatal establishment.

Ordinary callback code must not let behavior depend on wall-clock reads, IO, OS entropy, environment variables, threads, task completion order, process-global mutable state, Port implementation state, or unstable collection iteration.

## 21. Terminal Outcomes

Successful Engine observation of `RunStarted` synchronization establishes that a run has started. Terminal status is then returned through `RunOutcome` if the Engine reaches a return boundary. Exact Rust representation is deferred, but semantic categories must distinguish:

| Category | Meaning |
|---|---|
| Application completed | Authorized `ctx.shutdown()` committed, cleanup completed, and normal terminal audit synchronized |
| Startup failed | A run journal existed but static runtime preparation failed before `Ready` acceptance |
| Host stopped | Authoritative host closure ended the run; cleanup and ambiguity are reported |
| Simulation completed | The configured Simulation Environment completion policy ended the run |
| Fatal | A technical failure, invariant violation, limit, audit failure, handoff failure, or cleanup failure poisoned the run |

Available outcomes must report at least:

- Run identity.
- Primary terminal cause.
- Last Event index whose `InputAccepted` synchronization the Engine observed succeed.
- Last Event index whose `TurnComputed` synchronization the Engine observed succeed.
- Last Event index whose `TurnCommitted` synchronization the Engine observed succeed.
- In-flight phase when termination interrupted a turn.
- Confirmed local handoff prefix, current ordinal certainty, cutoff reason, and unattempted suffix when handoff terminated.
- Audit status, including whether a terminal record synchronized.
- Cleanup status and available secondary failures.

Build or validation failure, or failure before the Engine observes successful `RunStarted` synchronization, may remain a construction or run-start error rather than `RunOutcome`. A trailing complete `RunStarted` record may exist despite that error.

No `RunOutcome` is guaranteed after external process destruction, an unreturned callback or handoff, or cleanup that never reaches a reportable boundary.

## 22. Configuration And Illustrative Composition

Configuration is separated by owner:

| Configuration | Owner |
|---|---|
| Domain data, Components, initial `AppState`, initial private state | Application |
| Turn, payload, and deterministic Kernel bounds | Engine |
| Static Port bindings, queue and mailbox capacities, private runtime mechanics | Environment |
| Models, virtual scheduling, action bounds, future completion policy | Simulation Environment |
| Journal location, capacity, record bounds, storage contract | Audit journal |
| Filters and exporters | Optional observability |

All configuration is immutable after build. Determinism-affecting configuration is part of run provenance.

Illustrative composition:

```rust
let application = Application::builder(AppState::new(config.state))
    .port::<MarketData>()
    .port::<Execution>()
    .port::<Timer>()
    .reducer::<Ready>(initialize_state)
    .reducer::<ExecutionEvent>(apply_execution)
    .component(Bootstrap::new(), |c| {
        c.on::<Ready>(Bootstrap::on_ready)
            .produces_command::<MarketData>()
            .produces_command::<Execution>();
    })
    .component(ShutdownPolicy::new(), |c| {
        c.on::<ShutdownRequested>(ShutdownPolicy::on_request)
            .produces_message::<DisarmRequested>();
        c.on::<Reconciled>(ShutdownPolicy::on_reconciled)
            .may_shutdown();
    })
    .component(Strategy::new(config.strategy), |c| {
        c.on::<BarsClosed>(Strategy::on_bars)
            .produces_command::<Execution>();
    })
    .build()?;

let environment = LiveEnvironment::builder()
    .bind::<MarketData>(market_data, |b| b.event_capacity(4096))
    .bind::<Execution>(execution, |b| {
        b.event_capacity(1024)
            .command_capacity(1024)
    })
    .bind::<Timer>(timer, |b| b.event_capacity(256))
    .build(&application)?;

let audit = AuditLog::builder(config.audit_path)
    .max_record_bytes(config.max_record_bytes)
    .max_turn_bytes(config.max_turn_bytes)
    .max_journal_bytes(config.max_journal_bytes)
    .build()?;

let outcome = Engine::builder(application, environment, audit)
    .turn_limits(config.turn_limits)
    .build()?
    .run();
```

This example is non-normative and does not settle ownership types, closure shapes, method names, codec bounds, or error representations.

## 23. Required Semantic Conformance Tests

### 23.1 Graph, State, And Turn Execution

1. Repeated runs with identical deterministic inputs and no interruption before the compared frontier, or the same linearized interruption trace, produce identical callback, Message, Port Command, shutdown, and committed-state traces.
2. Reducers run before Components for every Port Event, Engine Event, and Message.
3. Messages propagate in exact breadth-first production order without recursion.
4. Only declared outputs may be produced by one callback.
5. Unauthorized `ctx.shutdown()` is fatal.
6. Construction rejects missing Port or Engine Event consumers, undeclared targets, duplicate or missing bindings, and invalid finite bounds.
7. A cohesive aggregate fact produces coherent state before its decision Component runs.
8. Turn-bound exhaustion produces no `TurnComputed` or Command handoff.

### 23.2 Startup And Staging

9. `RunStarted` synchronizes before static runtime preparation.
10. Runtime preparation failure accepts no `Ready` input.
11. `Ready` is Event index 0 and uses ordinary Reducer-before-Component dispatch.
12. No ordinary Port activity begins before the Ready turn commits.
13. The Ready turn may hand off startup Commands before the ordinary-activity gate opens.
14. Fixed cyclic live selection visits sources in frozen order with at most one candidate per source per cycle.
15. Per-Port FIFO is preserved.
16. Staging queue overflow is fatal and does not create an accepted input.
17. Staged inputs abandoned at terminal closure never appear as accepted.
18. Only the first cooperative shutdown request creates `ShutdownRequested`.

### 23.3 Audit And Acceptance

19. Observed `InputAccepted` synchronization failure dispatches no callback and does not advance the Engine's accepted frontier; offline inspection treats a trailing record as indeterminate.
20. An Engine-observed successful `InputAccepted` synchronization may truthfully remain accepted but unprocessed.
21. Every Kernel-visible semantic action uses the fixed audit record set.
22. A callback panic leaves a started record without a completed record where fatal audit succeeds.
23. Insufficient maximum-turn journal capacity prevents acceptance.
24. `TurnComputed` synchronization failure performs no Command handoff.
25. No `TurnCommitted` record exists before every handoff succeeds.
26. A torn suffix leaves one verifiable contiguous journal prefix.
27. Optional logging failure never changes callback control flow.

### 23.4 Command Handoff

28. Commands are considered in global production order and attempted at most once until the first denied, failed, or indeterminate ordinal.
29. Commands are not grouped by destination.
30. The first mailbox-full, disconnected, unavailable, or failed handoff establishes fatal state.
31. Terminal publication reports a confirmed handoff prefix, explicit current-ordinal certainty, and an unattempted suffix.
32. No failed or ambiguous Command is retried or returned as an application Event.
33. Handoff racing fatal or authoritative closure has one winner under the global run gate.
34. Failure synchronizing `TurnCommitted` after handoff is fatal and does not roll back handoff.

### 23.5 Shutdown, Fatal, And Outcomes

35. `ShutdownRequested` uses ordinary accepted-input semantics.
36. Legal `ctx.shutdown()` after an output-free prefix stops after the requesting callback returns.
37. A successful shutdown invokes no later callback, Message, input, or Command handoff.
38. Shutdown after any current-turn Message or Port Command is fatal.
39. Output production or duplicate shutdown after `ctx.shutdown()` is fatal.
40. A panic after requesting shutdown wins over normal completion.
41. Fatal state racing an active callback suppresses its staged outputs at callback return.
42. Normal shutdown cleanup failure produces fatal outcome.
43. Authoritative host stop performs no application business action and reports cleanup status.
44. External process termination is never represented as clean completion.

### 23.6 Simulation

45. Simulation uses the same Application and Kernel semantics as live for an identical accepted sequence.
46. Model callbacks never recursively enter the Kernel.
47. Equal virtual time is ordered by schedule ordinal.
48. Existing same-time actions precede newly committed same-time actions.
49. One simulated Command callback commits staged outputs before the next Command handoff.
50. Future source records cannot alter an earlier execution prefix.
51. Scheduling into the past and model panic are Engine-fatal.
52. A trailing valid commit-point record is not mistaken for Engine-observed synchronization success without later causal evidence.
53. Callback admission racing closure has one winner; an admitted callback may finish but no later callback is admitted.
54. Terminal frontier snapshots wait for every admitted operation and active callback boundary to drain.
55. Ready Command responses in simulation may stage behind the closed eligibility gate and cannot execute before Ready commits.
56. A boundary failure after known transfer reports the current Command handed off; an indeterminate transfer reports ambiguity.
57. Application and simulation completion promote to fatal on required terminal failure, while startup failure and host stop retain their primary category with secondary status.
58. Simulation follows the fixed action loop and records every initial model, source, corpus, time, schedule, and deterministic seed input in provenance.
59. Port Event dispatch distinguishes identical payload types emitted by different logical Ports.
60. `RunStarted` references only bounded artifacts durably published under the declared storage contract.
61. Closure waits for admitted operations and the active callback boundary before cleanup touches runtime resources.
62. A hung in-process cleanup produces no false `RunOutcome`; bounded termination requires a process boundary.

## 24. Required Failure Traces

Before public Rust interfaces are frozen, tests and design review must walk through these traces end to end.

### 24.1 Acceptance Audit Failure

```text
candidate selected
-> InputAccepted append or synchronization fails
-> Engine does not commit acceptance or dispatch the candidate
-> a trailing complete record may nevertheless persist with indeterminate synchronization acknowledgement
-> no callback runs
-> global fatal closure
```

### 24.2 Callback Panic

```text
InputAccepted synchronized
-> ReducerStarted or ComponentStarted appended
-> callback partially mutates owned state
-> callback panics
-> no completion record for that callback
-> no TurnComputed
-> no Command handoff
-> state is unusable
```

### 24.3 Computation Audit Failure

```text
callbacks reach ordinary quiescence
-> complete output set exists in memory
-> TurnComputed synchronization fails
-> no Command crosses a Port boundary
-> fatal cleanup begins
```

### 24.4 Partial Handoff Prefix

```text
TurnComputed lists C1, C2, C3
-> C1 handoff succeeds
-> C2 handoff fails
-> C3 is never attempted
-> no TurnCommitted
-> C1 may have external effect
-> business reconciliation uses C1's business identity
```

### 24.5 Commit Audit Failure

```text
TurnComputed synchronized
-> every Command handoff succeeds
-> TurnCommitted synchronization fails
-> handoffs are not rolled back
-> durable journal may end at TurnComputed
-> fatal outcome reports known local handoff if terminal coordination reaches a return boundary
```

### 24.6 Legal Application Shutdown

```text
Reconciled accepted
-> Reducers establish safe-to-stop application state
-> authorized Component calls ctx.shutdown()
-> callback returns normally
-> no earlier current-turn Message or Command exists
-> later callbacks and Messages are skipped by rule
-> terminal TurnComputed and TurnCommitted synchronize
-> Ports clean up
-> RunTerminated synchronizes
-> RunOutcome::Completed
```

### 24.7 Illegal Application Shutdown

```text
Component A produces Port Command C
-> Component B calls ctx.shutdown()
-> current turn already has semantic output
-> invariant violation
-> no TurnComputed
-> C is not handed off
-> fatal cleanup begins
```

### 24.8 Fatal During Callback

```text
Component callback is active
-> Port worker exits unexpectedly
-> global fatal gate closes
-> callback cannot be preempted
-> callback returns and its staged output is suppressed
-> no later callback, Message, acceptance, or handoff begins
```

### 24.9 Ready Failure

```text
static runtimes are prepared behind ordinary-activity gate
-> Ready acceptance commits
-> Ready callback fails or requests legal shutdown
-> ordinary Port activity never opens
-> failure or completion follows its ordinary terminal path
```

### 24.10 Authoritative Stop Race

```text
host requests authoritative stop
-> any acceptance or handoff permit already acquired may finish
-> global gate prevents the next operation
-> active callback output is suppressed at return
-> staged but unaccepted candidates are abandoned
-> cleanup and terminal audit are attempted
-> RunOutcome::HostStopped if admitted operations and cleanup reach a return boundary
```

### 24.11 Simulation Anti-Look-Ahead

```text
source wakes for occurrence N
-> consumes and applies only N
-> stages public Event N
-> schedules N+1 afterward
-> Command caused by Event N cannot observe N+1 early
```

## 25. Migration From V5

| V5 concept | V6 treatment |
|---|---|
| ControlPlane | Removed; Engine Events use the ordinary Kernel path and private runtime coordination remains inside Engine/Environment |
| ControlEvents and ControlCommands | Removed; only `Ready`, `ShutdownRequested`, and graph-authorized `ctx.shutdown()` remain |
| Port lifecycle state machine | Removed entirely |
| Quarantine and Port-local continuation | Removed; every technical Port failure is Engine-fatal |
| Incarnations and restart | Removed; Ports are static for one run and never restart |
| Runtime placement requests | Removed; physical mechanics belong to immutable Environment construction |
| Grouped Port or endpoint semantics | Removed; shared implementation state is private topology |
| Queue admission commitment and drainage | Replaced by non-authoritative staging and one durable acceptance commit |
| Configurable Acceptor quantum | Replaced by fixed cyclic one-candidate selection |
| Destination grouping and reservation | Replaced by one global production-order handoff pass |
| `CommandNotDelivered` | Removed; failed handoff is fatal and no feedback turn runs |
| Configurable diagnostics levels and acknowledgement | Replaced by one mandatory bounded audit journal with three fixed turn synchronization points |
| StopPort/StopEngine choreography | Replaced by cooperative Engine Event policy followed by output-free `ctx.shutdown()` |
| Replay-oriented lifecycle audit | Deferred; journal remains evidence, not recovery authority |

Core concepts retained from v5 include closed typed protocols, immutable graph topology, callback-local output declarations, Reducer-only canonical mutation, Component-private state, breadth-first Messages, deferred Commands, one single-writer Kernel, frozen logical time, explicit finite limits, static Port abstraction, Environment-independent application logic, anti-look-ahead simulation, business-owned identity, and cold-start reconciliation.

## 26. Intentionally Deferred Implementation Decisions

The following do not block this semantic model:

- Exact Rust protocol aggregation, registration, derive, and builder syntax.
- Registry erasure and storage layout.
- Queue, mailbox, synchronization primitive, and global-gate implementation.
- Exact numeric defaults for capacities and limits.
- Audit binary encoding, checksum, file segmentation, preallocation, rotation, and group-commit optimization.
- Concrete Port thread, task, process, or third-party runtime mechanics.
- Exact `RunOutcome`, build-error, and fatal-cause Rust enums.
- Exact identity representation and user-facing audit inspection tools.
- State validation and provenance-encoding APIs.
- Optional logging facade and metrics/exporter choices.
- Detailed simulation exhaustion, `run_until`, horizon, retained-work, and completion policies.
- Future weighted input selection, replay, snapshots, recovery, and DST.

Any implementation must preserve the semantic boundaries in this document. API convenience must not expose runtime authority, weaken graph authorization, introduce silent loss, claim false durability, imply external effect, or add in-run recovery through a side channel.

## 27. Semantic Gates Before Rust API Design

Before public Rust interfaces are frozen, review must confirm:

1. **Protocol gate:** Port Events, Engine Events, Messages, Port Commands, and shutdown intent have one legal direction each.
2. **Graph gate:** Actual registrations and callback-local declarations authorize every runtime input and output.
3. **State gate:** Canonical, Component-private, Port, model, Kernel, and audit state have non-overlapping authority.
4. **Turn gate:** Ordinary quiescence, shutdown truncation, breadth-first Messages, and finite bounds admit no conflicting trace.
5. **Acceptance gate:** Staging, selection, durable acceptance, dispatch, and accepted-but-unprocessed evidence are distinct.
6. **Audit gate:** `InputAccepted`, `TurnComputed`, and `TurnCommitted` make only truthful durability claims.
7. **Handoff gate:** Commands receive one global-order attempt and partial-prefix failure is explicit.
8. **Shutdown gate:** Cooperative policy, legal `ctx.shutdown()`, authoritative stop, fatal closure, and cleanup cannot all claim authority over one boundary.
9. **Port gate:** Static startup, ordinary-activity gating, global failure, and cleanup require no hidden lifecycle protocol.
10. **Environment gate:** Live and simulation preserve application semantics without claiming physical parity.
11. **Outcome gate:** Every in-process terminal path maps to one truthful `RunOutcome`, while external process destruction makes no false claim.
12. **MVP gate:** No required interface presupposes restart, replay, restoration, outbox behavior, dynamic placement, detailed simulation completion, or speculative future policy.

## 28. Readiness Statement

V6 is ready for Rust API design only after the semantic gates and required failure traces above are reviewed without contradiction.

The implementation pass must derive the smallest public API that enforces these capabilities. It must not copy v5's ControlPlane, lifecycle, reservation, diagnostic-mode, or identity machinery under different names.
