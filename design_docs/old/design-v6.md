# Kavod Core Design v6

> **Status:** Reviewed semantic design for MVP implementation
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
| Run phase | The small private Engine state machine `Constructing -> Starting -> Running -> Closing(initial_reason, effective_status) -> Terminated`; it is not a Port lifecycle |
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
20. The first failed or indeterminate Command handoff establishes terminal closure and suppresses every later handoff attempt. A handoff denied because closure already won observes that existing closure and suppresses every later attempt without replacing its reason.
21. `TurnCommitted` exists only after every Command handoff for that turn succeeds.
22. Kavod never automatically retries or resends a Port Command.
23. Every technical Port failure terminates the complete Engine; failure before static preparation completes requests startup failure, while failure after the run enters `Running` requests Engine-global fatal closure. An earlier authoritative closing reason remains primary.
24. Once the run phase leaves `Running`, it never returns; application completion, simulation completion, or authoritative host stop may preserve its initial reason while the effective status is promoted to fatal if required terminal audit, cleanup, or runtime termination fails.
25. Fatal state and state from a fatally incomplete turn are never reused.
26. Live and simulation share application semantics and typed Port protocols, not physical runtime behavior.
27. Application callbacks cannot observe the selected Environment mode.
28. Every Kavod-controlled queue, turn, output collection, audit buffer, scheduler chain, and identifier domain has a finite bound. Application payload values need not have a separate resident-memory size limit, but every mandatory audit encoding has a finite configured record and journal bound.
29. Kavod identity never substitutes for application-owned business identity or idempotency policy.
30. Kavod identifiers never wrap or reuse within their scope. Exhaustion is detected before the operation requiring the next identifier and is fatal once a run has started.
31. A live Port Command mailbox is a bounded non-evicting FIFO: successful insertion preserves one complete immutable Command until worker dequeue or terminal abandonment.

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
- Concrete simulation horizon, source-exhaustion, and completion policy choices; their common authority and scheduling contract is defined in Section 19.3, while the selected policy remains `SimulationEnvironment` configuration.
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

Admission under the gate is the linearization point for beginning one acceptance operation, callback invocation, Message dispatch, ordinary commit synchronization, simulation action, Command handoff, or startup release. A closure request atomically prevents new admissions while allowing operations already admitted to reach their defined boundary. An admitted operation does not complete until its authoritative result, frontier changes, handoff certainty, staged records, and failure classification are visible to terminal coordination. The Engine waits for admitted operations and the active callback boundary before snapshotting terminal frontiers or starting cleanup.

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

All Kavod identifier allocation uses checked monotonic advancement. An identifier is never wrapped, saturated into duplicate use, or recycled. Configuration must ensure that callback, Message, Command, scheduler, and audit limits fit their corresponding identifier domains. If the next Event index, action ordinal, Command ordinal, audit-record sequence, or simulation schedule ordinal is unavailable after `RunStarted` has synchronized, the operation that needed it does not begin or commit and the run closes fatally. Exhaustion before the Engine observes `RunStarted` synchronization is a typed construction or run-start error. Audit reservation includes sequence values for protected terminal evidence as well as bytes.

## 7. Protocol Semantics

### 7.1 Closed Typed Protocols

An application supplies enumerable closed concrete Port Event, Message, and Port Command protocol manifests. Kavod supplies the closed Engine Event protocol.

Callbacks receive concrete typed payloads, never a top-level `dyn Message`, `Any`, or user-visible downcast. Port Event routing is keyed by static logical Port plus declared event variant, so identical Rust payload types may appear in different Port protocols without losing source authority. A capability-bound typed staging queue establishes both source and protocol membership; ordinary acceptance does not trust candidate-supplied source metadata or dynamically revalidate a closed protocol. Engine Event routing is keyed by built-in variant and Message routing by declared Message variant. Narrow internal erasure may be used only if it preserves these typed keys. A mismatch after internal erasure is an impossible internal invariant violation and fatal, not an ordinary ingress outcome.

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
- Queue, mailbox, turn, scheduler, encoded-audit, journal, and identifier bounds are finite, mutually compatible, and valid.
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

`Starting` owns one private startup operation scope covering `RunStarted` encoding and synchronization, static runtime preparation, and publication of every partially created runtime resource into its cleanup owner. This scope is not a Port lifecycle and does not admit application work. A concurrent host or technical closure may latch while the scope is active, but cleanup cannot begin until the scope returns and its complete partial-resource set is visible to the Engine.

Successful `RunStarted` synchronization establishes that a run exists. Encoding or synchronization failure before the Engine observes that success returns a typed run-start error rather than `RunOutcome`; a complete trailing `RunStarted` record may still exist and is interpreted using Section 16.3.

A host or technical closure may latch while `RunStarted` synchronization is active, but it does not decide whether a run exists. The startup scope first resolves that synchronization. If the Engine observes success, the run is established and the latched closure follows the ordinary startup closing path to `RunOutcome` when terminal coordination reaches a return boundary. If the Engine does not observe success, no run was established: preparation does not begin, any partial startup resources are cleaned up best effort, and the host receives a typed run-start error carrying the available closure context rather than `RunOutcome`.

Port Command sinks must be available before the Ready turn begins. Autonomous external or model activity is forbidden before the Ready turn commits. A Ready Command may cause a simulated endpoint to stage a response, but that candidate remains ineligible for selection until Ready commits. Live workers remain behind the ordinary-activity gate and therefore cannot process Ready Commands until release.

`Ready` means only that the graph, audit journal, Engine, and static runtime boundaries can begin the run protocol. It does not mean connected, authenticated, subscribed, reconciled, or safe to trade.

If static runtime preparation fails before the Engine enters `Running`, the Engine accepts no `Ready` input and requests `Closing(StartupFailed, StartupFailed)`. If that transition wins, it cleans up prepared resources, attempts terminal startup-failure records, and returns startup failure if cleanup reaches a return boundary. If another closing reason already won, preparation failure follows the precedence rules in Section 17.2: in particular, authoritative host stop remains the initial reason and the effective status is promoted to fatal. Failure of terminal audit is secondary when startup failure won and cannot turn failed startup into success.

Successful preparation may atomically transition `Starting -> Running` only after the startup scope has published all resource ownership and only if `Starting` still owns the phase. Technical Port or audit failure after that transition is fatal even if `Ready` has not committed. A concurrent startup failure, authoritative host stop, and preparation completion follow whichever phase transition wins; losing preparation still returns through the startup scope and supplies its partial resources to cleanup. Ready acceptance cannot begin from `Starting` or after closure.

After the Ready turn commits, the Engine admits ordinary-activity release through the global run gate. Release admission is the release linearization point. If closure wins first, release is denied and ordinary Port activity never opens. If release admission wins first, closure does not revoke it: workers may process already-handed-off Ready Commands until cleanup stop propagates, but closure still prevents every later Kavod admission. If the Ready turn legally invokes `ctx.shutdown()`, ordinary Port activity never opens.

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

1. Checks that the next Event index exists without wrapping or reuse.
2. Obtains one acceptance admission from the global run gate.
3. Takes the candidate from its capability-bound typed source, which already establishes logical source and protocol membership.
4. Freezes acceptance time and prepares the complete encoded `InputAccepted` record under the configured record bound.
5. Assigns the Event index and establishes it as the turn's causal root.
6. Appends the complete record to the reserved in-memory turn buffer.
7. Synchronizes the audit journal.
8. Commits acceptance only if the Engine observes synchronization success.
9. Releases the acceptance admission.
10. Dispatches no callback unless synchronization succeeded and the gate grants callback admission.

Successful synchronization is the acceptance commit. Selection identifies the candidate, while queue removal occurs only inside the admitted acceptance operation; neither is a separate semantic commit. If closure denies acceptance admission, the candidate remains staged until terminal abandonment.

If the next Event index is unavailable or the complete `InputAccepted` encoding exceeds its configured record bound, fatal closure occurs without assigning the index, appending `InputAccepted`, accepting the candidate, or restoring it to its source queue. Failure to encode a mandatory record for any other reason follows the same preacceptance result.

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
7. At each `ctx.message()` call, check the next action ordinal and all Message, FIFO, encoding, and reserved-audit bounds, then atomically stage the Message and append `MessageProduced` to the in-memory turn buffer in production order.
8. At each `ctx.command()` call, check the next action and Command ordinals and all Command, encoding, and reserved-audit bounds, then atomically stage the Port Command and append `PortCommandProduced` to the in-memory turn buffer in global production order.
9. Append `ComponentCompleted` only after normal return, release callback admission, and continue through matching Components in stable registration order.
10. Obtain Message-dispatch admission, remove the next Message, release that admission, and repeat Reducer-then-Component dispatch subject to fresh callback admissions.
11. Reach quiescence only when the Message FIFO is empty and no callback is active.
12. Finalize deterministic state, action counts, output ordinals, and the complete ordered Port Command set.
13. Obtain computation-commit admission, append `TurnComputed`, synchronize it with every preceding action record, and release admission after observing the result.
14. Attempt Port Command handoff as defined in Section 14.
15. Obtain turn-commit admission, append `TurnCommitted`, synchronize it with all handoff records, and release admission after observing the result.
16. Only then select another input.

Callback admission, not the machine instruction entering user code, is the callback-start linearization point. Closure may occur after admission while that callback executes. On return, the Engine observes closure, releases the admission, and suppresses all remaining callbacks, Messages, and turn-local Commands. Production records from the active callback remain truthful records of uncommitted work; no `TurnComputed` is created for that incomplete ordinary turn.

Callback-start, callback-completion, and other fixed-shape control records use capacity reserved before acceptance. Their append to the Engine-owned in-memory turn buffer has no expected resource or storage failure path after admission. An impossible buffer-accounting failure or panic is an internal fatal invariant violation. Storage write and synchronization failure remain possible only where the audit protocol explicitly performs storage operations.

Callback admission completes on both normal return and unwind. Completion audit records exist only for normal return. An unwind completes the admission as terminal failure, establishes or reports fatal closure according to the current phase, and permits terminal draining to continue without invoking later work.

An ordinary audit synchronization operation begins only after obtaining its named gate admission and may finish after a concurrent closure request. If `TurnComputed` synchronization succeeds, the computed frontier advances but closure prevents the first handoff. If `TurnCommitted` synchronization succeeds, the committed frontier advances but closure prevents the next input. Cleanup begins only after the active synchronization and all other admitted operations drain. Terminal audit performed after an authorized closing transition is the explicit exception: it is authorized by the closing phase rather than by a `Running` admission.

Engine-observed `TurnComputed` synchronization establishes that deterministic callbacks completed and intended outputs were recorded; it does not establish resulting state independently of those callbacks or prove Port handoff. Engine-observed `TurnCommitted` synchronization establishes successful local handoff of every Port Command in that turn, not external receipt or effect.

Finite bounds are mandatory for at least:

- Callback invocations per turn.
- Messages per turn.
- Port Commands per turn.
- Encoded audit bytes per input, Message, and Port Command record.
- Semantic action records and encoded audit bytes per turn.

Exceeding a bound establishes fatal state and produces a typed fatal cause where possible. The operation that exceeds its bound does not stage an output or consume an ordinal. Earlier turn-local Messages may already have executed and earlier deterministic outputs may remain recorded as uncommitted work, but no Port Command from the incomplete turn is handed off and the turn's state is unusable.

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
    classify the transfer result
    if the attempt is unsuccessful, establish its fatal cause
    append PortCommandHandoffResult
    complete the admission as success or terminal failure
```

The static graph already proves that the destination exists, the Command type matches, the callback declared the output, and one binding exists. There is no destination lifecycle authority to classify or revalidate.

Successful handoff admission is the operation-start linearization point against fatal closure and authoritative host stop. Closure denies new admission but waits for an already admitted transfer attempt to finish. An admitted attempt can atomically close the gate as fatal without reopening a window for the next Command.

Every bound handoff operation must define one exact local transfer linearization point and return one of:

- **Handed off:** the Command definitely crossed Kavod's local boundary.
- **Not handed off:** the Command definitely did not cross.
- **Indeterminate:** Kavod cannot establish whether the Command crossed.

After admission, an impossible failure to append the reserved `PortCommandHandoffStarted` control record establishes fatal closure before transfer. After the attempt, the Engine first classifies the boundary result as `HandedOff`, `NotHandedOff`, or `Indeterminate`. An unsuccessful endpoint result atomically establishes the primary fatal cause while retaining the current handoff admission, before the Engine appends `PortCommandHandoffResult`. A later result-record or audit failure is secondary to that already-established endpoint failure. If the endpoint transfer itself succeeded and only result recording fails, the audit failure is primary and does not undo the known in-memory `HandedOff` certainty. Failure before transfer admission means no transfer began. Panic or boundary failure spanning the transfer point is indeterminate unless the concrete boundary can prove one side.

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

Mailbox full, disconnected receiver, unavailable static boundary, simulated model failure, indeterminate transfer, or any other unsuccessful admitted endpoint attempt establishes Engine-global fatal closure before result recording. A handoff-record failure with no earlier endpoint failure establishes fatal audit closure. If authoritative host stop already won, the handoff or audit failure preserves `HostStop` as the initial reason and promotes the effective status to fatal. If startup failure, application or simulation completion, or fatal closure already won, the failure follows the precedence rules in Section 17.2 and never replaces that initial reason.

On the first failure:

- Earlier confirmed handoffs remain handed off locally.
- The current Command is recorded in memory as handed off, not handed off, or indeterminate according to the boundary result.
- Later Commands are not attempted.
- No `TurnCommitted` record is created.
- Kavod does not retry any Command.
- Fatal audit and cleanup are attempted best effort.

Terminal evidence therefore consists of a confirmed handed-off prefix, at most one current terminal ordinal with explicit certainty, and an unattempted suffix. A plain negative result such as live mailbox full is definitely not handed off. A failure after known mailbox insertion is definitely handed off locally even when its audit record is absent.

### 14.3 Meaning Of Handoff

Live handoff linearizes at successful insertion into the bound Port implementation's mailbox; live mailbox admission is nonblocking. Each live mailbox is a bounded non-evicting FIFO of complete immutable Command envelopes. After successful insertion, Kavod does not silently remove, overwrite, coalesce, reprioritize, reorder, or duplicate the Command before the Port worker dequeues it or terminal cleanup abandons it. A full mailbox rejects the current insertion as definitely `NotHandedOff` and preserves every earlier entry. Once the worker dequeues a Command, its handling and any external effect are Port-internal behavior outside Kavod's handoff guarantee. Simulation handoff linearizes when the addressed synchronous model callback is entered. A model callback must return normally before the Engine may continue to the next Command or commit the turn; a panic after entry is fatal and leaves model state unusable even though the Command crossed the simulated local boundary.

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

When the requesting callback returns normally, the Engine appends `ComponentCompleted` while retaining the callback admission. One atomic gate transition then chooses application shutdown or observes that fatal or authoritative host-stop closure already won, and completing that transition releases the callback admission. A losing provisional shutdown request is suppressed, not retroactively illegal. When application shutdown wins:

1. Atomically close the global run gate for application shutdown while completing the callback admission.
2. Invoke no remaining callback.
3. Dispatch no queued Message.
4. Accept no later input.
5. Append `RunClosing` for application shutdown.
6. Append `TurnComputed` identifying intentional application shutdown, the cutoff callback, the shutdown intent, and an empty Message and Port Command set.
7. Synchronize the complete terminal computation record.
8. Perform an empty Command handoff phase.
9. Append and synchronize `TurnCommitted` identifying application-requested completion.
10. Wait for any previously admitted operation to drain, then stop and join static Port runtimes.
11. Append and synchronize `RunTerminated` after successful cleanup.
12. Return `RunOutcome::Completed`.

This turn is intentionally complete by the shutdown rule even though it does not reach ordinary Message quiescence or full callback fan-out.

If the callback panics after requesting shutdown, fatal failure wins over the provisional completion request unless an earlier authoritative closure already won. If required audit or cleanup fails after application shutdown wins, the initial closure reason remains application shutdown and the effective terminal status is promoted to fatal; the phase never returns to running.

## 16. Audit Journal And Optional Observability

### 16.1 Mandatory Journal

Every run has exactly one mandatory Engine-owned append-only audit journal. It records every Kernel-visible semantic action using one fixed record set. There are no `Off`, `Debug`, `Trace`, sampling, or best-effort modes for this journal.

In this document, **append** means adding a complete encoded record to the Engine-owned bounded in-memory audit buffer in journal sequence order. It is distinct from a storage write and from synchronization. Variable payload records are encoded and checked against the configured maximum record size before the corresponding acceptance or deterministic production becomes established. Fixed-shape control records use capacity reserved before the turn or terminal path begins. Once those checks and reservations succeed, an append has no expected capacity or storage failure path; an impossible append failure is an internal fatal invariant violation. Storage may receive buffered records before a named synchronization point, but only Engine-observed synchronization has the durability meaning defined in Section 16.3.

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

- Strong content hashes of the actual executable and application build inputs; a source-control revision alone is insufficient.
- Frozen graph, callback order, source order, and Port bindings.
- Strong content identities of the initial canonical `AppState` and initial Component-private state.
- Determinism-affecting application, Engine, and Environment configuration.
- Queue, mailbox, turn, scheduler, encoded-record, journal, and identifier bounds.
- Audit schema, location, and durability contract.

For simulation, provenance also includes strong content identities of initial model state, source state, and source corpus; initial virtual time and scheduled work; the configured completion policy; and every explicit deterministic seed or choice input.

The audit storage contract must state which failures successful synchronization covers, including its assumptions about process and kernel crashes, host power loss, filesystem and device behavior, volatile caches, file data and length, file and directory metadata, namespace operations, segments, and manifests. Every claim of durability in this document is relative to that declared contract.

Starting values may be embedded when they fit the configured record bounds, but the MVP provenance requirement is content identity rather than retained artifact availability. Strong content hashes must cover the exact bytes and schema needed to distinguish the executable, state, configuration, model, and source corpus that began the run. Kavod does not retain those inputs, promise that they remain obtainable, reconstruct them from a source-control revision, or grant restoration or replay authority. Integrity mechanisms have a documented residual collision probability and are evidence, not proof against arbitrary corruption.

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
| Trailing `RunStarted` | The Engine encoded the run provenance; whether it observed run-start synchronization and established a run is indeterminate unless a later ordinary record proves progression |
| No matching `InputAccepted` | No acceptance of that candidate is evidenced; a candidate has no Kavod Event identity before acceptance, so the journal alone may not distinguish never staged, abandoned, or preacceptance failure |
| Trailing `InputAccepted` | The Engine appended the candidate; synchronization success and acceptance are indeterminate unless a later execution record proves dispatch |
| Trailing `ReducerStarted` or `ComponentStarted` | The Engine observed `InputAccepted` synchronization and admitted that callback; `Started` denotes callback admission, not proof that a user-code instruction executed |
| Trailing `ReducerCompleted` or `ComponentCompleted` | The named callback returned normally; later callbacks, quiescence, and computation commit remain unknown |
| Trailing `MessageProduced` | The Message was staged in the turn-local FIFO; its dispatch and consumption remain unknown |
| Trailing `PortCommandProduced` | The Command was staged as deterministic intent; computation commit, local handoff, and external effect remain unproved |
| Trailing `ApplicationShutdownRequested` | The request was dynamically legal and provisionally latched; callback return and application-shutdown authority remain unknown |
| Trailing `TurnComputed` | Computation reached the boundary and intended Commands were recorded; synchronization success and, for a nonempty Command set, any handoff remain indeterminate unless later handoff evidence exists. An empty set proves that this turn had no Command handoff |
| Trailing `PortCommandHandoffStarted` | The Engine observed `TurnComputed` synchronization and admitted that Command attempt; whether the Command crossed its local boundary is indeterminate |
| Trailing successful `PortCommandHandoffResult` | The Engine observed the recorded local transfer certainty before appending it; later Command handoffs and turn-commit status remain unknown |
| Trailing unsuccessful `PortCommandHandoffResult` | The Engine observed the recorded local transfer certainty and endpoint failure before append; every later Command is unattempted and no `TurnCommitted` exists |
| Trailing `TurnCommitted` | Every local handoff succeeded before append; whether the Engine observed this record's synchronization and advanced its committed frontier is indeterminate |
| Trailing `RunClosing` | Closure and the explicitly captured frontiers and initial reason were known before append; no later Engine-observed ordinary synchronization preceded that snapshot. The record does not exclude complete failed or unobserved boundary records and does not prove its own synchronization |
| Trailing `RunTerminated` | The recorded effective status, frontiers, cleanup result, and bounded failures were known before append. A later failure, including failure synchronizing this record, may have promoted the effective status and produced only unsynchronized in-memory evidence. The record does not prove its own synchronization, that no later promotion occurred, or that a `RunOutcome` returned |

Later ordinary-phase evidence may prove that the Engine observed a prerequisite synchronization because admission to that ordinary phase requires the observation. Callback execution evidence may prove prior `InputAccepted` synchronization; handoff-start evidence may prove prior `TurnComputed` synchronization; and ordinary execution of a later input may prove prior `TurnCommitted` synchronization. After fatal, authoritative-host-stop, or startup-failure closure, audit is best effort. Terminal records prove only their explicitly captured frontiers and facts, never ordinary prerequisite synchronization by presence alone. A pre-handoff record cannot prove handoff. A handoff-result record proves only the local certainty and endpoint status it explicitly contains, not external effect. Absence of a result or `TurnCommitted` does not prove that no Command crossed a Port boundary. Application business identity and external reconciliation resolve that ambiguity.

### 16.4 Framing And Bounds

The journal must use versioned framing, run and segment binding, complete-record detection, strong checksums, and monotonic record ordering sufficient to identify one valid prefix and detect a torn or corrupt suffix within the declared storage fault model. Inspection stops at the first invalid frame. Integrity mechanisms have explicitly documented residual failure probability and must not be described as proof against arbitrary corruption. The format contains no intentional internal gaps and must protect previously synchronized frames under its declared layout and storage assumptions.

Finite configured limits are mandatory for:

- One encoded record.
- Semantic records and bytes per turn.
- In-memory staging and writer buffering.
- Total journal storage or preallocated segments.
- Reserved terminal evidence.

The audit configuration has distinct maximum encoded-record and total-journal sizes. Before `RunStarted`, the audit facility must reserve bounded byte and audit-sequence capacity for run provenance, startup failure, and terminal evidence, including one later promotion snapshot and final fatal evidence. Before accepting another input, it must provide a preallocated maximum-sized turn buffer and confirm capacity for the inclusive worst case: `InputAccepted`, framing overhead, every permitted fixed-shape action record, the configured maximum encoded payload records, `TurnComputed`, every handoff-start and handoff-result record, `TurnCommitted`, segment metadata, and protected terminal headroom. Post-computation records must be size-validated or pre-encoded before the first handoff. Terminal records have fixed maximum sizes.

Terminal reporting retains at most a configured finite number of secondary failures, defaulting to eight. It preserves the first failures in Engine observation order and sets a bounded `additional_failures_omitted` indicator if more are observed. The initial closure reason and any outcome-promotion cause have separately reserved representation and do not consume this secondary-failure allowance.

Capacity exhaustion before acceptance establishes fatal state without accepting the candidate. Unexpected write or synchronization failure remains possible and fatal.

Journal rotation, segmentation, preallocation technique, checksums, and synchronization optimization are deferred. No implementation may overwrite unretained audit history or silently downgrade durability.

### 16.5 Failure

Audit encoding, write, capacity, corruption, or synchronization failure while the run is `Running`, or while application completion, simulation completion, or authoritative host stop still requires terminal audit, establishes or promotes the effective status to fatal. The immutable initial reason remains unchanged. Audit failure after startup failure or an existing fatal closure is secondary because it cannot replace an authority boundary that already occurred. A failed synchronization never rolls back application mutation or a Command handoff that already occurred. Fatal establishment prevents later application admission but does not guarantee later durable evidence; it does not make facts already observed by the Engine false.

Authority closure happens before attempting its `RunClosing` audit record. For fatal, startup-failure, and authoritative-host-stop paths, `RunClosing`, `RunTerminated`, and final synchronization are best effort because the journal, runtime, or process may itself be failing.

`RunTerminated` records the initial closure reason, effective terminal status and promotion causes known before append, last known frontiers, cleanup result, the bounded secondary-failure prefix, and whether additional failures were omitted. It is a terminal-status snapshot, not a claim that its own synchronization succeeded or that no later terminal failure occurred. If failure of an earlier terminal attempt changes the effective status, the Engine may append one later `RunTerminated` snapshot rather than rewriting the earlier record; journal order identifies the later snapshot when it persists. Application completion, simulation completion, and authoritative host stop require the Engine to observe successful terminal synchronization; failure promotes the effective outcome to fatal audit failure while preserving the initial closure reason. The Engine appends final fatal terminal evidence to the reserved in-memory suffix and makes one best-effort write and synchronization attempt for the remaining suffix. Startup failure and an existing fatal closure retain their effective category and report terminal-audit failure as secondary because closure already occurred independently of the journal.

If that final best-effort synchronization succeeds, the fatal outcome reports synchronized terminal evidence. If it returns failure but terminal coordination still reaches a `RunOutcome` return boundary, the outcome includes the complete remaining bounded in-memory audit suffix, including final fatal evidence, with explicit unsynchronized status and a completeness indicator. This suffix is evidence available to the caller, not journal durability. Missing or omitted handoff evidence never means `NotHandedOff`; certainty may be reported only when the Engine retained it. No `RunOutcome`, suffix, or complete terminal evidence is guaranteed if a callback, handoff, storage operation, cleanup operation, runtime, or process fails to reach a return boundary.

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

An authoritative stop atomically transitions the private run phase from `Starting` or `Running` to `Closing(HostStop, HostStopped)` and closes the global run gate. It admits no later operation, performs no implicit business cancel, flatten, or reconciliation, and suppresses unfinished turn output at the next callback boundary.

An acceptance, callback, Message dispatch, synchronization, handoff, or startup-release operation admitted before closure may reach its defined boundary. A startup operation scope already active in `Starting` may return and publish its partial-resource set. Closure prevents the next operation from starting. The Engine waits for the startup scope, admitted operations, and the active callback boundary to drain, snapshots terminal frontiers, attempts `RunClosing`, directs Environment cleanup, and then attempts `RunTerminated` with cleanup and bounded secondary-failure status. Host and runtime threads never clean up resources concurrently with startup preparation or admitted Engine operations.

### 17.2 Run Phase And Precedence

The Engine maintains one small private run phase:

```text
Constructing -> Starting -> Running -> Closing(initial_reason, effective_status) -> Terminated
                    |                       ^
                    +-----------------------+
```

This run-wide state machine is not application-visible and does not recreate per-Port lifecycle. `Starting` may close as startup failure, authoritative host stop, or fatal audit failure before runtime preparation completes; its startup scope must still return before cleanup. Successful preparation is the only transition to `Running`. While `Running`, the first successful gate transition chooses application shutdown, simulation completion, host stop, or fatal as the immutable initial closure reason. The effective terminal status initially matches that reason. Later reports are secondary unless the following explicit promotion rule applies:

| Initial closure reason | Later required audit or cleanup failure |
|---|---|
| Application or simulation completion | Promote effective outcome to `Fatal` on required audit failure, cleanup failure, or unexpected technical runtime failure before successful termination |
| Startup failure | Retain `StartupFailed`; attach secondary failures |
| Authoritative host stop | Promote effective outcome to `Fatal` on terminal audit failure, cleanup failure, or unexpected technical runtime failure; retain `HostStop` as the initial reason |
| Fatal | Retain the first fatal cause; attach secondary failures |

The run phase never returns to `Running`.

When the Engine observes the winning transition it appends one `RunClosing` with the immutable initial reason, current effective status, and current frontiers. Application shutdown synchronizes this record with its terminal `TurnComputed`. Simulation completion synchronizes it before cleanup. Startup failure, host stop, and fatal paths attempt it best effort after authority is already closed. A later promotion changes only the effective status and appends its cause to later terminal evidence where possible; it always appears in the bounded in-memory suffix and `RunOutcome` if terminal coordination reaches a return boundary. It does not rewrite the initial reason or append a second `RunClosing`.

### 17.3 Fatal Causes

Fatal causes include:

- Kernel, callback, Port, model, selector, scheduler, or audit panic.
- Internal invariant violation or undeclared application output.
- Technical Port failure after the run enters `Running`, including unexpected return or worker exit before, during, or after the Ready turn.
- Staging queue overflow or corruption.
- Port Command mailbox full, disconnect, or failed handoff.
- Audit capacity, encoding, write, corruption, or synchronization failure.
- Configured turn, callback, Message, Command, encoded-audit, journal, identifier, or simulation limit exhaustion.
- Simulation causality violation such as scheduling into the past.
- Cleanup failure following requested normal completion or authoritative host stop.
- Any Environment failure that makes ordering, authority, or boundary accounting untrustworthy.

Expected external conditions remain typed Port Events when the Port implementation is still trustworthy. A broker rejection, exchange halt, remote disconnect, authentication expiration, or reconciliation mismatch is not automatically a technical Port failure.

### 17.4 Fatal Establishment

Fatal state is monotonic and first-failure-wins while the run is active. The winning atomic closure stores a bounded cause token and wakes the Engine. Every runtime worker and transitive child is supervised. Each worker scope has one private graceful-stop authorization and a top-level supervision boundary. Before the scope completes, that boundary classifies the actual termination and publishes any technical failure; later delivery of a supervision notification does not determine termination order. During incomplete `Starting`, a technical Port failure requests `Closing(StartupFailed, StartupFailed)`. During `Running`, it requests fatal closure. During application completion, simulation completion, or authoritative host stop, an unexpected exit preserves the initial reason and promotes the effective status to fatal. Graceful-stop authorization permits only a subsequent normal cooperative termination; it does not make an earlier exit, panic, abnormal process status, supervision loss, or unknown disposition expected. During startup failure or existing fatal closure, such a failure is secondary. Only the Engine sequences detailed audit, frontier snapshots, and terminal reporting.

After fatal establishment:

- No later acceptance operation, callback, Message dispatch, or Port Command handoff begins.
- An active synchronous callback cannot be forcibly preempted.
- When it returns or unwinds, remaining callbacks, Messages, and turn-local application outputs are suppressed. Closure does not revoke an acceptance or simulation action admitted before closure; all other staged Port candidates and simulation actions remain staged and are never accepted or selected.
- `TurnComputed` and `TurnCommitted` are not fabricated for an incomplete ordinary turn.
- In-memory application and model state is not reusable.
- Cleanup, child cancellation, joining, and final audit are best effort.
- No Port is quarantined, replaced, or restarted.
- No replacement Engine resumes the run.

Assertions and panic remain appropriate for impossible internal states. Known resource and infrastructure failures should retain typed fatal causes. Catching an unwind at the outer Engine boundary permits audit and cleanup attempts but never continuation.

### 17.5 Process And Non-Unwinding Termination

External process termination, allocator abort, OOM kill, stack-overflow abort, `panic=abort`, double-panic abort, and equivalent non-unwinding destruction provide no guarantee of callback completion, audit synchronization, Port cleanup, child joining, terminal outcome, or consistent state. They are not required to execute the fatal-return path. Hard preemption of arbitrary in-process code requires process authority outside Kavod's deterministic contract.

### 17.6 Cleanup

Every thread, task, process, job, or third-party operation belongs transitively to exactly one static Port scope or one Environment run-wide scope. Privately shared passive state does not require a scope; privately shared active work must have one unambiguous Environment owner. Detached child work is forbidden.

After application shutdown, simulation completion, host stop, startup failure, or fatal closure enters `Closing` and admitted Engine operations drain, the Environment issues stop or cancellation and joins owned work. Cancellation proves neither completion nor rollback.

An in-process binding is required to provide cooperative stop and join, but arbitrary bugs may violate that contract. Kavod cannot return a truthful terminal outcome while forbidden detached work remains. If cleanup hangs, no `RunOutcome` is guaranteed even if the process itself remains alive. A deployment requiring bounded termination must place such work behind a killable process boundary and external watchdog.

Normal application shutdown becomes `RunOutcome::Completed` only after required cleanup and terminal audit succeed. Cleanup failure after application-requested shutdown becomes fatal cleanup failure.

Authoritative host stop returns `RunOutcome::HostStopped` only when cleanup and required terminal audit succeed. A reportable cleanup, terminal-audit, or unexpected technical runtime failure preserves `HostStop` as the initial reason and promotes the effective outcome to `Fatal`. Cleanup that never returns provides no return-time guarantee.

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

The MVP live Environment uses one dedicated runtime thread per static Port binding. A binding may privately share passive thread-safe implementation state with another binding, but Kavod gives that shared state no group identity or lifecycle semantics. Transitive child work has exactly one binding owner or one Environment run-wide owner for cleanup. Each worker runs inside the supervision boundary defined in Section 17.4, so its actual normal or abnormal termination is classified before its scope completes rather than by later notification-delivery order.

The live Environment uses bounded per-Port staging queues, bounded Port Command mailboxes, fixed cyclic input selection, anchored monotonic acceptance time, and the global run gate.

Which live candidate wins an availability race is not deterministic. During the run, the Engine-observed accepted sequence is authoritative. After failure, the journal is evidence subject to the trailing synchronization ambiguity in Section 16.3 and never becomes replay or delivery authority.

### 19.3 Simulation Environment

A simulated model is a deterministic external-world state machine held by the Simulation Environment. The MVP Simulation Environment runs the scheduler, models, Kernel, Reducers, and Components on one thread with no overlapping callback. Each simulated Port has one bounded scheduled-event queue configured by the Simulation Environment, analogous to its live Port's bounded staging queue. Every model wake, source action, and Port Event delivery action belongs to exactly one addressed Port queue. The Simulation Environment additionally reserves one bounded Engine Event action slot for the one-shot cooperative request. A model must:

- Produce the same transitions and staging calls for the same model state, input, virtual time, configuration, and explicit deterministic seed or choice inputs recorded in run provenance.
- Run synchronously with no overlapping model, Reducer, or Component callback.
- Use only Environment-supplied virtual time.
- Consume Port Commands only through static bound endpoints.
- Stage Port Events only through static bound endpoints.
- Avoid wall time, OS IO, OS entropy, task scheduling, process-global mutable state, and unstable iteration that affects behavior.
- Never access application state or invoke the Kernel directly.
- Never retain callback context after return.

Model state represents the simulated external world and is not `AppState`.

Simulation handoff invokes the addressed model endpoint synchronously in global Command production order. One model callback must return before the next Command handoff begins. A model `wake_at` or equivalent staging call immediately inserts one immutable action into the addressed Port's bounded scheduled-event queue, just as a live Port stages one candidate into its bounded queue. Successful insertion receives the next checked schedule ordinal. Queue full, scheduling into the past, or schedule-ordinal exhaustion establishes fatal closure and the attempted action is not staged. No later staging call succeeds after closure, including one made before the active model callback returns. Successfully staged actions become later candidates and never recursively enter the Kernel. If a staging failure occurs during a simulated Command handoff, that Command remains `HandedOff` because model entry was its transfer point, while the staging failure is the primary technical endpoint cause.

Future actions are ordered by:

```text
(virtual_time, schedule_ordinal)
```

At equal virtual time, earlier successfully staged actions precede later successfully staged actions. Zero-latency output receives a later same-time ordinal rather than recursive execution or invented time. The total-action bound counts every initial or runtime action successfully inserted into the scheduler during the run, including actions later popped, executed, or abandoned. The same-time-action bound counts every successful insertion for one virtual timestamp, including actions already popped or executed, and does not reset while virtual time remains at that timestamp. Initial scheduled work is validated against both bounds, uses unique schedule ordinals, and cannot precede the initial virtual time. Each runtime insertion atomically checks causality, both action bounds, owning-queue capacity, and ordinal availability before changing any counter, allocating an ordinal, or inserting the action.

After the Ready turn, the Simulation Environment executes one fixed loop:

1. Obtain simulation-action admission from the global run gate and pop the minimum `(virtual_time, schedule_ordinal)` action.
2. Advance virtual time to that action's time.
3. If it is a model wake or source action, run that callback once and complete simulation-action admission on normal return or unwind. Each staging call during the callback has already inserted its action or established fatal closure. Unwind completes the admission as terminal failure; actions successfully staged before unwind remain staged but are abandoned because closure prevents their selection.
4. If it is a Port Event or Engine Event delivery, convert it to one candidate, release simulation-action admission, and execute its complete acceptance, turn, audit, and Command-handoff protocol before selecting another scheduled action.
5. Commands delivered during that turn may stage later actions immediately, including same-time actions with larger ordinals, but never reenter the Kernel.
6. A closure request may wait for an admitted model callback, but it prevents admission of the next scheduled action. On panic, causality violation, bound exhaustion, or closure, select no later action.

A cooperative host request in simulation atomically latches one pending request; the host thread does not mutate scheduler storage or read virtual time directly. At the next scheduler boundary between actions or committed turns, before completion-policy evaluation or next-action selection, the Simulation Environment converts that pending request into the reserved Engine Event action slot at current virtual time with the next checked schedule ordinal. The insertion remains subject to the cumulative action bounds and schedule-ordinal availability; failure is fatal and inserts nothing. Repeated requests remain idempotent as in live mode. If closure already won, no action is inserted.

Historical source and model code must preserve prefix causality:

```text
wake for next occurrence
-> consume exactly the due occurrence or explicit atomic batch
-> update external-world model state
-> stage the corresponding public Port Event
-> arrange the next occurrence
```

Future records must not influence earlier state, output, latency, or effect decisions. Virtual time never decreases. Scheduling into the past, encountering invalid initial work before initial virtual time, or popping an action before current virtual time is fatal. Finite per-Port queue, cumulative total-action, and cumulative same-time-action bounds are mandatory. Each failed insertion is fatal and stages no action, consumes no schedule ordinal, and changes no action counter; earlier successful insertions remain staged but cannot execute after closure.

The concrete source-exhaustion, `run_until`, horizon, and completion policy is immutable `SimulationEnvironment` configuration and part of run provenance. Every policy must define its complete deterministic inputs, whether pending scheduled actions inhibit completion, and whether otherwise-pending work is abandoned when completion wins. The policy is evaluated only between committed turns and scheduled actions, after converting any pending cooperative host request, and deterministically chooses either completion or the next scheduled action before requesting authority. For the next-action branch, the selected action requests simulation-action admission. For the completion branch, the Environment atomically requests `Running -> Closing(SimulationCompletion, SimulationCompleted)` rather than an operation admission. The chosen request may race concurrent host or fatal closure, but completion and the next action do not race each other. If the selected action obtains admission, it reaches its defined boundary before eligibility is reconsidered; if the completion transition wins, no later action begins. Completion runs required cleanup and attempts `RunTerminated`. Required terminal audit or cleanup failure preserves simulation completion as the initial reason but promotes the effective status to fatal. Environment-selected completion returns a terminal `RunOutcome`; it is not an Engine Event. Exact policy names and Rust configuration syntax are deferred.

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
| Host stopped | Authoritative host closure ended the run, required cleanup completed, and terminal audit synchronized |
| Simulation completed | The configured Simulation Environment completion policy ended the run, required cleanup completed, and terminal audit synchronized |
| Fatal | A technical failure, invariant violation, limit, audit failure, handoff failure, or cleanup failure poisoned the run |

Available outcomes must report at least:

- Run identity.
- Immutable initial closure reason.
- Effective terminal status and any promotion cause.
- Last Event index whose `InputAccepted` synchronization the Engine observed succeed.
- Last Event index whose `TurnComputed` synchronization the Engine observed succeed.
- Last Event index whose `TurnCommitted` synchronization the Engine observed succeed.
- In-flight phase when termination interrupted a turn.
- Available local handoff evidence for any computed but uncommitted turn, its completeness, current ordinal certainty when known, cutoff reason, and known unattempted suffix. Missing evidence is unknown, never implicitly `NotHandedOff`.
- Audit status, including whether a terminal record synchronized and any available bounded in-memory audit suffix explicitly marked unsynchronized and complete or incomplete.
- Cleanup status, the bounded secondary-failure prefix, and whether additional failures were omitted.

Build or validation failure, or failure before the Engine observes successful `RunStarted` synchronization, may remain a construction or run-start error rather than `RunOutcome`. A trailing complete `RunStarted` record may exist despite that error.

No `RunOutcome` is guaranteed after external process destruction, an unreturned callback, handoff, startup scope, storage operation, or cleanup operation.

## 22. Configuration And Illustrative Composition

Configuration is separated by owner:

| Configuration | Owner |
|---|---|
| Domain data, Components, initial `AppState`, initial private state | Application |
| Turn and deterministic Kernel bounds | Engine |
| Static Port bindings, queue and mailbox capacities, private runtime mechanics | Environment |
| Models, per-Port scheduled-event capacities, virtual scheduling, action bounds, and completion policy | Simulation Environment |
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
31. Returned terminal publication reports available handoff evidence and its completeness; known evidence distinguishes a confirmed prefix, explicit current-ordinal certainty, and an unattempted suffix, while missing evidence remains unknown.
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
43. Authoritative host stop performs no application business action; successful cleanup and terminal audit return `HostStopped`, while a reportable preparation, runtime, cleanup, or terminal-audit failure preserves `HostStop` as the initial reason and promotes the effective status to fatal.
44. External process termination is never represented as clean completion.

### 23.6 Simulation

45. Simulation uses the same Application and Kernel semantics as live for an identical accepted sequence.
46. Model callbacks never recursively enter the Kernel.
47. Equal virtual time is ordered by schedule ordinal.
48. Existing same-time actions precede newly staged same-time actions.
49. One simulated Command callback returns before the next Command handoff; each successful staging call inserts immediately without recursive Kernel entry.
50. Future source records cannot alter an earlier execution prefix.
51. Scheduling into the past and model panic are Engine-fatal.
52. A trailing valid commit-point record is not mistaken for Engine-observed synchronization success without later causal evidence.
53. Callback admission racing closure has one winner; an admitted callback may finish but no later callback is admitted.
54. Terminal frontier snapshots wait for the startup scope, every admitted operation, and the active callback boundary to drain.
55. Ready Command responses in simulation may stage behind the closed eligibility gate and cannot execute before Ready commits.
56. When terminal evidence is available, a boundary failure after known transfer identifies the current Command as handed off and an indeterminate transfer identifies ambiguity.
57. Application completion, simulation completion, and authoritative host stop preserve their initial closure reason but promote the effective status to fatal on required terminal failure, while startup failure retains its effective category with secondary status.
58. Simulation follows the fixed action loop, records strong content identities for its initial model, source, and corpus, and records exact initial time, schedule, completion policy, and deterministic seed inputs in provenance.
59. Port Event dispatch distinguishes identical payload types emitted by different logical Ports.
60. `RunStarted` records actual executable and deterministic-input content identities without claiming artifact retention, reconstruction, or replay.
61. Closure waits for the startup scope, admitted operations, and the active callback boundary before cleanup touches runtime resources.
62. A hung in-process cleanup produces no false `RunOutcome`; bounded termination requires a process boundary.

### 23.7 Exhaustion, Startup, Evidence, And Completion

63. Event, action, Command, audit-sequence, and simulation schedule identifiers never wrap or reuse; exhaustion fails before the requiring operation commits and is fatal after run start.
64. An oversized `RunStarted` is a run-start error, an oversized `InputAccepted` does not accept its candidate, and an oversized Message or Command record fatally terminates its already-accepted turn before handoff.
65. A host stop or startup failure racing active preparation waits for the startup scope to return and publish all partial resources before cleanup.
66. A failed endpoint result establishes the primary fatal cause before result recording; a later result-record failure is secondary.
67. Terminal records prove only their explicitly captured frontiers and never prove ordinary prerequisite synchronization by their presence alone.
68. A simulated per-Port scheduled-event queue rejects an overflowing insertion as fatal, stages no replacement action, and selects no staged action after closure.
69. Every simulation completion policy deterministically defines whether pending work inhibits completion, chooses completion or the next action before requesting authority, and defines what happens to pending work when completion wins; completion uses the closing transition rather than an operation admission.
70. A returned fatal outcome labels its available in-memory audit suffix unsynchronized, reports whether that evidence is complete, and never treats missing handoff evidence as `NotHandedOff`.
71. Terminal reporting preserves at most its configured secondary-failure maximum, defaults that maximum to eight, and truthfully indicates omitted additional failures.
72. Application completion, simulation completion, or authoritative host stop preserves its initial closure reason when terminal failure promotes the effective status to `Fatal`.
73. A live Command mailbox preserves successful insertions in non-evicting FIFO order until worker dequeue or terminal abandonment; full insertion is definitely `NotHandedOff` and cannot evict an earlier Command.
74. Worker termination classification follows the supervised scope's actual termination order, not notification delivery order; stop authorization does not make panic or abnormal exit expected.
75. A host stop racing `RunStarted` synchronization returns `RunOutcome` only if the Engine observes synchronization success; otherwise it returns a typed run-start error and preparation never begins.
76. Ready release racing closure follows release admission: a winning admission may release already-handed-off Ready Commands, while a winning closure denies release.
77. Every admitted operation publishes its authoritative result, frontier changes, handoff certainty, and failure classification before its admission drains.
78. A failed final fatal synchronization returns the complete bounded in-memory suffix as explicitly unsynchronized if terminal coordination reaches a return boundary; a hung operation or process destruction returns nothing.
79. Simulation total-action and same-time-action bounds count cumulative successful insertions, including actions already popped, and terminate a one-at-a-time zero-latency chain at the configured bound.
80. A concurrent simulation cooperative request is latched by the host and, unless closure already won or conversion fails fatally, is converted by the scheduler before completion evaluation or next-action selection.

## 24. Required Failure Traces

Before public Rust interfaces are frozen, tests and design review must walk through these traces end to end.

### 24.1 Acceptance Audit Failure

```text
candidate selected
-> InputAccepted encoding or synchronization fails
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
-> C2 is classified as HandedOff, NotHandedOff, or Indeterminate at its local boundary
-> C3 is never attempted
-> no TurnCommitted
-> C1 may have external effect
-> reconciliation covers C1 and also C2 whenever C2 is HandedOff or Indeterminate
-> business reconciliation uses each affected Command's application-owned business identity
```

### 24.5 Commit Audit Failure

```text
TurnComputed synchronized
-> every Command handoff succeeds
-> TurnCommitted synchronization fails
-> handoffs are not rolled back
-> durable journal may end at TurnComputed
-> final fatal terminal evidence is appended and receives one best-effort synchronization attempt
-> if that attempt succeeds, the fatal outcome reports synchronized terminal evidence
-> if it returns failure and terminal coordination reaches a return boundary, the fatal outcome attaches the complete remaining bounded suffix as unsynchronized
-> absent evidence is unknown and never means that a Command was not handed off
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
-> any startup scope, acceptance, callback, Message dispatch, synchronization, simulation action, handoff, or startup-release operation already active or admitted may reach its defined boundary
-> global gate prevents the next operation
-> active callback output is suppressed at return
-> staged but unaccepted candidates are abandoned
-> accepted, computed, and committed frontiers include only synchronization success the Engine observed
-> cleanup and terminal audit are attempted
-> RunOutcome::HostStopped only if admitted operations drain, cleanup succeeds, and required terminal audit synchronizes
-> a reportable cleanup, runtime, or terminal-audit failure preserves HostStop as the initial reason and promotes the effective outcome to Fatal
```

### 24.11 Simulation Anti-Look-Ahead

```text
source wakes for occurrence N
-> consumes and applies only N
-> stages public Event N
-> schedules N+1 afterward
-> Command caused by Event N cannot observe N+1 early
```

### 24.12 Endpoint Failure Plus Audit Failure

```text
Command handoff admission succeeds
-> endpoint returns a definite or indeterminate unsuccessful result
-> that endpoint result establishes the primary fatal cause while admission remains held
-> an impossible PortCommandHandoffResult append failure or later audit synchronization failure occurs
-> audit failure is secondary
-> known in-memory Command certainty is retained when terminal coordination reaches a return boundary
```

### 24.13 Host Stop During Startup Preparation

```text
RunStarted synchronization succeeds
-> static runtime preparation creates partial resources inside the startup scope
-> authoritative host stop wins Starting -> Closing(HostStop, HostStopped)
-> preparation cannot enter Running
-> startup scope returns and publishes every partial resource
-> cleanup begins only afterward
-> HostStopped returns only if preparation returns normally, cleanup succeeds, and required terminal audit synchronizes
-> a reportable preparation, cleanup, runtime, or terminal-audit failure preserves HostStop as the initial reason and promotes the effective outcome to Fatal
```

### 24.14 Simulation Queue Overflow

```text
model callback calls wake_at for Port P
-> P's bounded scheduled-event queue is full
-> no action or schedule ordinal is staged for that call
-> fatal closure wins
-> actions staged by earlier successful calls remain queued but are never selected
-> no later simulation action, callback, input, or Command handoff begins
```

### 24.15 Terminal Forensic Boundary

```text
ordinary synchronization fails or its result is unobserved
-> fatal or host-stop closure has authority
-> best-effort RunClosing or RunTerminated persists
-> inspector accepts only the frontiers explicitly captured by that terminal record
-> terminal-record presence does not prove the earlier ordinary synchronization
-> trailing RunTerminated does not prove its own synchronization or a returned RunOutcome
```

### 24.16 Host Stop During Run Establishment

```text
RunStarted synchronization is active
-> authoritative host stop latches
-> if the Engine observes RunStarted synchronization success, a run exists and follows the HostStop closing path
-> if the Engine does not observe success, no run exists, static preparation never begins, and the host receives a typed run-start error with available closure context
-> a complete trailing RunStarted remains synchronization-indeterminate offline
```

### 24.17 Worker Termination Ordering

```text
Port worker terminates before graceful-stop authorization
-> its top-level supervision boundary classifies the exit before the worker scope completes
-> while Starting or Running, unexpected termination requests startup failure or fatal closure according to the current phase
-> during application completion, simulation completion, or host stop, it preserves the initial reason and promotes the effective status to Fatal
-> during existing startup-failure or fatal closure, it is secondary
-> delayed notification delivery cannot reclassify that earlier termination
-> a later stop request cannot make it expected
```

### 24.18 Final Fatal Audit Failure

```text
terminal synchronization fails and promotes the effective status to Fatal
-> Engine appends final fatal evidence to the reserved bounded in-memory suffix
-> final best-effort write and synchronization returns failure
-> if terminal coordination reaches a return boundary, RunOutcome includes the complete remaining suffix explicitly marked unsynchronized
-> the persisted journal may omit that fatal evidence or end with an earlier terminal-status snapshot
-> if storage hangs or the process is destroyed, no RunOutcome or suffix is guaranteed
```

### 24.19 Zero-Time Simulation Bound

```text
action A0 at virtual time T stages A1 at T
-> each action is popped before staging the next, so queue occupancy remains one
-> cumulative same-time count includes every successful insertion at T
-> the first insertion beyond the configured bound stages nothing and consumes no ordinal
-> fatal closure wins and no later simulation action executes
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
- Exact numeric defaults for capacities and limits other than the settled secondary-failure default.
- Audit binary encoding, checksum, file segmentation, preallocation, rotation, and group-commit optimization.
- Concrete Port thread, task, process, or third-party runtime mechanics.
- Exact `RunOutcome`, build-error, and fatal-cause Rust enums.
- Exact identity representation and user-facing audit inspection tools.
- State validation and provenance-encoding APIs.
- Optional logging facade and metrics/exporter choices.
- Concrete simulation exhaustion, `run_until`, horizon, retained-work, and completion policy choices that implement the common contract in Section 19.3.
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
12. **MVP gate:** No required interface presupposes restart, replay, restoration, outbox behavior, dynamic placement, a concrete simulation completion policy, or speculative future policy.

## 28. Readiness Statement

The semantic gates and required failure traces above have been reviewed. V6 is ready for MVP implementation and Rust API design, subject to implementation conformance tests preserving these boundaries.

The implementation pass must derive the smallest public API that enforces these capabilities. It must not copy v5's ControlPlane, lifecycle, reservation, diagnostic-mode, or identity machinery under different names.
