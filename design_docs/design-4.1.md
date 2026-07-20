# Kavod Core Design v4.1

> **Status:** Implementation design
> **Supersedes:** `design-v4.md` where they conflict
> **MVP scope:** A deterministic single-writer kernel, closed typed protocols, a validated immutable application graph, Reducers and typed canonical cache state, dedicated-thread live Ports, deterministic simulated Ports, causal tracing, turn bounds, and an accepted-Event tape.

---

## 1. Purpose And Scope

Kavod is a domain-agnostic deterministic application kernel. An application defines its own domain types, Components, Reducers, state slots, Ports, and lifecycle protocols. Kavod defines the rules under which they execute.

The MVP must make this claim:

> Given the same application version, graph, initial state, Engine configuration, and ordered accepted Event tape with acceptance times, the kernel produces the same ordered internal Messages, Commands, and terminal deterministic state.

Live arrival is inherently nondeterministic. The kernel freezes that nondeterminism at one Event acceptance boundary. Historical simulation uses the same kernel and application graph with simulated Port implementations.

The MVP does not implement durable recovery, snapshots, replay command comparison, generalized deterministic simulation testing, dynamic routing, remote Ports, async/task/pool live placement, or runtime graph mutation. These are future capabilities, not implied guarantees.

---

## 2. Core Model

Kavod has five application-level concepts:

- **Event:** An immutable fact from outside the deterministic application graph.
- **Message:** An immutable fact derived and consumed inside the application graph.
- **Command:** An immutable request for an effect outside the application graph.
- **Port:** An application-defined typed boundary to an external system.
- **Component:** Deterministic internal application logic.

The only legal directions are:

```text
Port      -- Event   --> Reducer or Component
Component -- Message --> Reducer or Component
Component -- Command --> Port
```

The following are illegal:

```text
Component produces Event
Port produces Message
Component consumes Command
Port consumes Message
Reducer produces Message or Command
```

Commands can cause later Events through an explicit Port feedback loop:

```text
Component -- Command --> Port -- later Event --> Component
```

"External" means outside the deterministic application graph, not necessarily outside the process. A Timer is therefore an ordinary Port: a Component requests a timer through a Command, and the Timer Port later emits an Event.

---

## 3. Domain-Agnostic Kernel

Kavod core knows nothing about orders, venues, market data, sessions, brokers, risk, positions, timers, databases, or any other application domain.

The application defines:

- Closed Event, Message, and Command protocol enums.
- Port Specs and marker types.
- State slot types.
- Component and Reducer types.
- All expected operational/lifecycle Events and Commands.
- All business identifiers, retry, reconnect, arming, reconciliation, and safety policies.

Kavod core owns only execution, ordering, graph validation, typed capability boundaries, Event acceptance, worker supervision, and fail-stop behavior.

There is no generic `KernelPort`, generic `Disconnected` Event, generic order protocol, or trading lifecycle hidden in the core.

---

## 4. Protocols And Typed Dispatch

### 4.1 Closed Protocols

Each application supplies closed concrete protocol enums. Conceptually:

```rust
pub enum AppEvent {
    Market(MarketEvent),
    Execution(ExecutionEvent),
    Timer(TimerEvent),
}

pub enum AppMessage {
    Signal(Signal),
    Intent(Intent),
}

pub enum AppCommand {
    Execution(ExecutionCommand),
    Timer(TimerCommand),
}
```

Callbacks receive their inner concrete payload type, such as `&Signal`, never a top-level enum, `dyn Message`, `Any`, or a user-visible downcast.

The kernel may use narrow internal erasure to store heterogeneous callbacks, Components, and cache values. Erasure is not exposed in the application API.

### 4.2 Port Specs

```rust
pub trait PortSpec: 'static {
    type Command;
    type Event;
}
```

A Port Spec is logical application topology. It does not specify a thread, task, process, channel, runtime, or implementation.

For MVP, distinct marker types select distinct logical destinations. Fine-grained keyed routing and multicast Commands are deferred.

---

## 5. Application Graph

### 5.1 Executable Source Of Truth

Actual callback registrations create graph input edges. Callback-local production declarations create output edges. Declared Port Specs create boundary nodes.

There is no independent graph file that can drift from executable registrations.

Rust types prove local relationships: a callback consumes a valid protocol payload, a Message belongs to the protocol, and a Command targets a valid Port Spec. They cannot prove application-wide connectivity across independent registrations, configuration, and arbitrary callback control flow.

Whole-application graph validation therefore happens at build time, before the first Event. It is not runtime hot-path work.

### 5.2 Build Stages

Application and Environment construction are distinct:

```text
Application::build()
  validates protocol membership, registrations, topology, and order

Environment::build(&application)
  validates one compatible binding for every declared Port and runtime policy
```

Application build validates:

1. Every declared Port Event has a registered consumer.
2. Every declared Message production has a consumer.
3. Every declared Command production targets a declared Port.
4. Every callback input and declared production belongs to the closed protocol.
5. Callback and fan-out ordering are stable.
6. State slots supplied during construction are internally valid.

Environment build validates:

1. Every declared Port has exactly one binding.
2. No binding exists for an undeclared Port.
3. Each binding implements the selected environment interface for its Port Spec.
4. Mailbox capacity and overflow policy are explicit.

### 5.3 Registration API

MVP uses exactly one Component registration style:

```rust
Application::builder()
    .port::<MarketData>()
    .port::<Execution>()
    .state(Orders::default())
    .state(Positions::default())
    .component(OrderState::default(), |c| {
        c.reduce_on_event(OrderState::on_execution);
    })
    .component(Strategy::new(config), |c| {
        c.on_message(Strategy::on_signal)
            .produces_message::<Intent>();
    })
    .build()?;
```

`.add(...)`, a reusable `Component::register` trait, component macros, and protocol macros are deferred. They can be introduced later as convenience APIs delegating to this registration representation.

### 5.4 Production Declarations

Every ordinary callback declares each Message type and Command Port it may emit:

```rust
c.on_message(Strategy::on_signal)
    .produces_message::<Intent>()
    .produces_command::<Execution>();
```

`.produces_*()` means **may produce**. There is no `must_produce()` in MVP. Mandatory business outcomes should be expressed in the domain protocol and checked through domain tests or invariants.

Runtime verifies that each emitted output was declared by the executing callback. Undeclared output is an invariant violation.

### 5.5 Graph Cycles And Turn Bounds

The application builder may report strongly connected components in the declared Message graph as diagnostics. A declared edge means "may emit," so a cycle is not inherently invalid.

Every turn has mandatory Engine limits:

- `max_messages_per_turn`.
- `max_callback_invocations_per_turn`.

If either limit is exceeded, the Engine panics, is captured at the run boundary, stops, and returns `RunError::Panic`. It never spins indefinitely and never processes another Event.

---

## 6. Components And Reducers

### 6.1 Ordinary Components

Components are synchronous, deterministic, non-blocking application logic. They may own private state. They receive typed Event or Message payloads, read-only canonical state, frozen logical time, and declared output capabilities.

They may:

- Read canonical state.
- Mutate their own private state.
- Emit declared Messages.
- Emit declared Commands.

They may not mutate canonical state, access another Component's private state, perform IO, block, observe wall time, inspect Engine mode, access a scheduler, or obtain Port/channel/executor handles.

### 6.2 Reducers

A Reducer is the only callback kind allowed to mutate canonical shared state. Reducers may be stateful Components, but their callback capability is restricted:

- A Reducer receives `&mut Cache` through `ReducerCtx`.
- A Reducer may read frozen logical time.
- A Reducer emits no Messages or Commands.
- A Reducer performs no IO or blocking work.

For each delivered Event or Message payload:

1. All matching Reducers execute in stable registration order.
2. All matching ordinary Components execute in stable registration order.

Therefore ordinary Components observe canonical state updated for the current input. Reducers run for both Events and Messages.

### 6.3 Context Capabilities

The exact lifetimes and internal storage remain private, but the public capability boundary is fixed:

```rust
pub struct ComponentCtx<'a, P> { /* private */ }

impl<'a, P> ComponentCtx<'a, P> {
    pub fn now(&self) -> Timestamp;
    pub fn state<T: 'static>(&self) -> &T;
    pub fn message<M>(&mut self, message: M);
    pub fn command<Port: PortSpec>(&mut self, command: Port::Command);
}

pub struct ReducerCtx<'a> { /* private */ }

impl<'a> ReducerCtx<'a> {
    pub fn now(&self) -> Timestamp;
    pub fn state<T: 'static>(&self) -> &T;
    pub fn state_mut<T: 'static>(&mut self) -> &mut T;
}
```

The real signatures carry the bounds required to prove protocol membership and output declarations. Missing declared state slots and impossible internal downcasts are invariants and panic.

Every callback in one root Event turn observes the same `ctx.now()`. Internal Messages do not advance time.

---

## 7. Canonical State

### 7.1 MVP Cache

Canonical shared state is an Engine-owned typed cache:

```rust
pub struct Cache {
    values: BTreeMap<TypeId, Box<dyn Any>>,
}
```

The public API is typed generic access. Users do not see `Any`, `TypeId`, or downcasts:

```rust
cache.get::<Orders>();
cache.get_mut::<Positions>();
```

There is exactly one value for each concrete Rust type. Applications use generic wrappers or newtypes when they require distinct slots of the same underlying type.

`BTreeMap` prevents randomized iteration order within one build/run. Components cannot enumerate the cache; they only request known concrete types.

`TypeId` is not a durable key. Rust does not guarantee its ordering across Rust releases. The MVP cache has no snapshot, cross-version persistence, or generic state-hash format. Those require a later stable slot-identity and serialization design.

### 7.2 State Invariants

- The Cache belongs to one Engine instance.
- Only the kernel thread mutates it.
- Only Reducer callbacks receive mutable access.
- Live Port threads cannot borrow it.
- State crosses the Port boundary only through application-defined Events and Commands.

Component-private state remains private to its Component and is mutable only during that Component's callback.

---

## 8. Kernel Execution

### 8.1 Acceptance

Every external Event enters through one acceptance operation. Acceptance establishes:

- Monotonic Event index.
- Frozen acceptance time.
- Stable logical source Port identity.
- Root causation identity.
- Required Event-tape record.

Live Events are ordered by central ingress acceptance, not domain timestamps. Domain timestamps remain payload fields. Accepted logical time must not move backward.

### 8.2 Turns

One accepted Event creates one turn:

1. Append the accepted Event to the Event tape before callback dispatch.
2. Create contexts with the Event's frozen acceptance time and causation root.
3. Run matching Reducers, then matching Components, in registration order.
4. Append emitted Messages to the turn FIFO in production order.
5. Pop the next Message and repeat Reducer then Component dispatch.
6. When the FIFO is empty, finalize causal trace data.
7. Submit collected Commands to the Environment in deterministic production order.

No later Event begins until the current turn reaches quiescence. Message dispatch is breadth-first and never recursive. Commands never invoke a Port reentrantly from a callback.

### 8.3 Causal Trace

The kernel records enough in-memory diagnostic metadata to explain each Command from its root Event:

```text
Event 4201
  Message 4201.1
    Message 4201.2
      Command 4201.3 -> Port
```

Trace indices are diagnostics, not business identifiers. Persisting every Message or callback invocation is deferred.

---

## 9. Live Ports

### 9.1 Binding And Placement

MVP live bindings use dedicated OS threads only:

```rust
let environment = LiveEnvironment::builder()
    .thread_port::<MarketData>(market_port)
    .thread_port::<Execution>(execution_port)
    .build(&application)?;
```

`Engine::run()` starts the Environment. The Environment starts, owns, supervises, requests shutdown from, and joins all Port threads. A Port implementation never selects its own kernel-side placement or owns a kernel mailbox.

Async-task, worker-pool, remote-process, and proxy bindings are deferred. They may later add distinct binding APIs without changing `PortSpec` or application topology.

### 9.2 Port IO Semantics

The exact `LivePort` trait spelling is private implementation work. Its semantic contract is fixed:

- The Environment gives each Port one bounded FIFO Command ingress.
- The Port receives Commands in submission order.
- The Port may emit only its associated typed Event payloads through central ingress.
- The Port receives a cooperative stop request.
- The Port does not receive raw kernel channels, an executor, scheduler, or Engine state.

Port construction and worker startup may fail before the worker is Running. That is a startup failure returned from `run()` as `RunError::Start`.

After a worker is Running, expected operational outcomes must be represented by the application's Port Event protocol. A running Port does not return ordinary operational errors to the kernel.

For example, an application may define `Disconnected`, `ReconnectFailed`, or `ServiceUnavailable` Events and let a user-defined Component emit corresponding reconnect Commands. A Port may instead own reconnect behavior internally. Kavod mandates neither approach.

### 9.3 Technical Worker Lifecycle

Kavod defines only technical worker state:

```text
Constructed -> Starting -> Running -> Stop requested -> Stopped
                         \-> unexpectedly exited / panicked
```

It does not define generic readiness, degraded, session, reconnect, arming, trading, or reconciliation semantics. Those are user-defined protocol concepts when the application needs to react to them.

An unrequested worker exit, mailbox disconnection, or queue overflow is never silent. It fails the Engine. There is no automatic restart in MVP.

### 9.4 Panic Capture And Stop

Kavod uses one panic policy: **capture and stop**.

The kernel run boundary and each Port worker boundary catch Rust unwinding panics only to report failure and stop execution. They never resume a callback, process another Event, or continue using possibly partial state.

On a kernel panic or a Port worker panic, the Environment requests shutdown from all workers and `run()` returns:

```rust
RunError::Panic {
    origin: PanicOrigin::Kernel | PanicOrigin::Port(PortInstanceId),
    message: Option<String>,
}
```

On `panic = "abort"`, Rust cannot capture a panic and the process terminates. The embedding binary chooses that stronger deployment behavior.

Expected external failures must not use panic. A Port thread panic is an infrastructure/programming failure, not a normal application Event.

---

## 10. Simulated Ports And Historical Simulation

### 10.1 Simulation Boundary

Simulated Ports share `PortSpec` with live Ports but are synchronous deterministic state machines. They do not run threads or async runtimes.

Each simulated Port owns its own model and source state. For example, a historical source Port owns its reader, decoder, cursor, and source-local ordering. The simulation Environment does not own application-domain market or venue data.

The Environment owns only the global virtual clock and private future-action ordering. This is necessary to select the next action across all Ports without hidden cross-Port timing or look-ahead.

### 10.2 Wake Semantics

A wake means: "call this simulated Port again at virtual time T." It is not a thread, sleep, callback into a Component, or public scheduler handle.

Conceptually, a simulated Port has three callbacks:

```rust
pub trait SimPort<P: PortSpec> {
    fn start(&mut self, now: Timestamp, sim: &mut SimPortCtx<P>);

    fn on_command(
        &mut self,
        now: Timestamp,
        command: P::Command,
        sim: &mut SimPortCtx<P>,
    );

    fn on_wake(&mut self, now: Timestamp, sim: &mut SimPortCtx<P>);
}
```

`SimPortCtx` allows a Port to schedule a future wake for itself, cancel a scheduled wake, and emit an associated typed Event at the current virtual time. It cannot invoke another Port, mutate application state, or access the scheduler itself.

The exact token/identifier API for multiple outstanding wakes remains implementation-private. Timer replacement and cancellation must be designed before timer APIs are stabilized.

### 10.3 Simulation Scheduler

The simulation Environment maintains one private future-action queue ordered by:

```text
(virtual_time, deterministic_schedule_ordinal)
```

At each step it:

1. Pops the next action.
2. Advances virtual time.
3. Invokes the target simulated Port synchronously.
4. If the Port emits an Event, accepts it and runs the kernel turn to quiescence.
5. Delivers resulting Commands to their simulated Ports according to the selected model.

The scheduler's deterministic tie-breaking rules are part of simulation provenance. A Port scheduling work in the past is a visible causality violation.

Historical Port implementations may internally use `poll(now)` when woken. `poll_all_ports(until_time)` is not a kernel API because the kernel cannot independently choose a globally safe next time.

### 10.4 MVP Simulation Scope

Deterministic historical simulation is a supported architecture target. Full deterministic simulation testing is deferred.

Deferred DST work includes randomized schedule exploration, fault tapes, generic network/storage models, crash/restart simulation, shrinking, liveness phases, and adapter-level deterministic execution. Real live Port implementations need not be deterministic; only their accepted Events are replayable inputs.

---

## 11. Event Tape, Replay, And Durability

### 11.1 MVP Event Tape

The MVP journal records accepted external Events before dispatch. Its logical record includes at least:

- Event index.
- Acceptance timestamp.
- Source Port instance identity.
- Protocol/schema identity.
- Complete top-level Event payload.

Run provenance records application identity, graph identity, and determinism-relevant Engine/simulation configuration.

The Journal receives the typed accepted Event record. A concrete persistent Journal selects the payload encoding; its encoding must be self-consistent for one application and protocol version. Kavod v4.1 does not standardize a durable cross-version wire schema, compatibility policy, or decoder.

If an Event record cannot be written under the selected journal policy, the Event is not dispatched and the Engine stops with `RunError`.

### 11.2 Deferred Replay And Recovery

The MVP does not promise:

- Replay execution.
- Passive command-verifying bindings.
- Command tapes.
- State hashes.
- Snapshots or state migration.
- Durable outbox submission.
- Crash recovery, resend, or reconciliation.

Later replay will inject recorded Events in acceptance order and bind passive command verifiers to the application's existing Port Specs. It will not add mode-specific logical `ReplayPort` nodes to the application graph.

---

## 12. Configuration

Configuration is immutable after build and scoped by ownership:

- **Application configuration:** application-owned Component construction and domain configuration.
- **Engine configuration:** turn bounds, tracing, deterministic kernel behavior.
- **Environment configuration:** Port thread binding, mailbox capacity, overflow policy, and shutdown policy.
- **Journal configuration:** Event-tape destination and durability policy.

Builder closures are the MVP configuration style:

```rust
Engine::builder(application, environment)
    .config(|c| {
        c.max_messages_per_turn(100_000);
        c.max_callback_invocations_per_turn(100_000);
        c.enable_causal_trace(true);
    })
    .journal(journal)
    .build()?;
```

Determinism-relevant Engine and simulation configuration is included in run provenance.

---

## 13. Failure Model

| Class | Behavior |
|---|---|
| Invalid protocol, graph, state slot, binding, or configuration | `BuildError` before execution |
| Port startup failure | `RunError::Start` before normal processing |
| Expected application/domain outcome | Application-defined typed Event or Message |
| Journal append failure | Stop; return `RunError` |
| Queue overflow, mailbox failure, unexpected worker exit | Stop; return `RunError` |
| Kernel or Port worker panic | Capture, stop, return `RunError::Panic` |
| Internal invariant violation | Panic, then capture and stop |

Assertions and `unwrap()` are appropriate for proven internal invariants and impossible states. They are not appropriate for invalid configuration, corrupt external input, network failure, rejection, timeout, or ordinary Port disconnection.

Kavod never catches a panic in order to continue operation. A returned `RunError::Panic` is a terminal outcome for that Engine instance.

---

## 14. Deferred Work

The following remain intentionally deferred:

- Protocol macros, component registration macros, and reusable `.add()` registration.
- Fine-grained routing, selectors, and multicast Commands.
- Graph export formats.
- Stable protocol and state schema identities, migration, snapshots, and persistence.
- Replay execution and command/state comparison.
- Durable command outbox, recovery, resend, and reconciliation guarantees.
- Async-task, worker-pool, process, and remote live Port placement.
- Generalized Port readiness, health, restart, and supervision policy.
- Full DST fault models, random-choice tape, shrinking, liveness, and adapter-level DST.
- Component deterministic IDs and randomness.

---

## 15. Implementation Gates

### Gate A: Protocol, Cache, And Graph

- Closed Event, Message, and Command protocols.
- Typed callback registration through `.component(...)`.
- Typed `BTreeMap<TypeId, Box<dyn Any>>` Cache.
- Reducer registration and phase ordering.
- Production declarations and Application graph validation.

### Gate B: Deterministic Kernel

- Single acceptance authority.
- Event tape append before dispatch.
- Frozen turn time.
- Reducer-before-Component delivery.
- Breadth-first Message FIFO.
- Deterministic Command collection.
- Turn limits and causal trace.
- Capture-and-stop panic boundary.

### Gate C: Threaded Live Environment

- Dedicated-thread bindings only.
- Per-Port FIFO command mailboxes.
- Central Event ingress.
- Worker startup, cooperative stop, join, and unexpected-exit detection.
- Explicit capacity and overflow policy.

### Gate D: Deterministic Simulation

- One virtual clock and future-action queue.
- `SimPort` start, command, and wake behavior.
- One historical source Port and one simulated effect Port.
- Deterministic end-to-end historical run.

### Gate E: Deferred Capabilities

Implement replay, durable recovery, alternate Port placement, and full DST only after the MVP kernel semantics are proven by tests and real application use.

---

## 16. Core Invariants

1. Only Ports emit Events.
2. Only ordinary Components emit Messages and Commands.
3. Only Reducers mutate canonical Cache state.
4. Events and Messages are immutable in callbacks.
5. Components receive typed payloads and never user-facing downcasts.
6. Actual registrations and production declarations are the graph source of truth.
7. The graph is validated and frozen before execution.
8. Every declared Event and Message flow has a consumer; every Command target has one declared bound Port.
9. One kernel thread owns deterministic application state.
10. Reducers run before ordinary Components for every matching input.
11. One accepted Event turn reaches quiescence before the next begins.
12. Message propagation is breadth-first and deterministic.
13. No Component sees wall time, Engine mode, scheduler, Port, channel, executor, or external IO.
14. Live concurrency becomes deterministic only at central Event acceptance.
15. Commands preserve per-Port submission order.
16. Queue overflow, worker exit, and routing failure are never silent.
17. Expected operational outcomes are application-defined typed Events; core worker failures stop the Engine.
18. Every panic is capture-and-stop; no Engine resumes after panic.
19. Simulated Ports own model/source state; the simulation Environment owns only global virtual action ordering.
20. No process-global mutable kernel state exists.

---

## 17. One-Line Thesis

Kavod v4.1 is a domain-agnostic, single-writer deterministic kernel where typed Ports emit accepted external Events, Reducers update typed canonical state, Components transform Events and Messages into directed Commands, dedicated-thread live concurrency is frozen at one ingress boundary, and simulated Ports reuse the same application graph under one deterministic virtual timeline.
