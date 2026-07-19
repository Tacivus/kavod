# Kavod Core Design v4

> **Status:** Draft for review
> **Scope:** Application protocol, Components, Ports, graph construction and validation, deterministic kernel semantics, live execution, historical backtesting, replay, and deterministic simulation testing (DST)
> **Supersedes:** `design-v1.md`, `design-v2.md`, `design-v3.md`, and `plan-v2-adendum.md` where they conflict with this document
> **Preserves:** The earlier designs' strongest invariants: a single-writer deterministic core, narrow callback capabilities, explicit production declarations, startup graph validation, no silent drops, immutable payloads, frozen callback time, replayable live ingress, isolated external IO, and no process-global mutable state

---

## 1. Document Conventions

This document separates settled semantics from recommendations and unresolved design work.

| Label | Meaning |
|---|---|
| **Decided** | Part of the v4 semantic model. Implementation must preserve it. |
| **Recommended** | Preferred implementation direction, but not yet a semantic commitment. |
| **Open** | Intentionally unresolved. The document describes constraints without selecting an answer. |

Code is conceptual Rust. It demonstrates type relationships and user experience; exact method names, trait syntax, error types, and macro syntax are not finalized unless explicitly marked **Decided**.

---

## 2. Purpose

Kavod is a deterministic application kernel for trading systems. It must support four environments without changing strategy or application logic:

1. Live trading against real feeds, venues, services, timers, and storage.
2. Historical backtesting against recorded market data and simulated external systems.
3. Replay of a recorded live or simulated run.
4. Deterministic simulation testing with virtual time, generated workloads, modeled failures, and reproducible scheduling choices.

The v1-v3 designs modeled nearly everything as a generic `Message` moving through reducers, handlers, and actors. That model preserved useful invariants but blurred three semantically different flows:

- Facts entering from outside the engine.
- Deterministic intermediate facts produced inside the engine.
- Requests for effects outside the engine.

It also placed live external adapters and simulated venues inside the same actor graph even though their internal data dependencies and execution mechanisms differ fundamentally.

v4 replaces the generic actor/message model with five core concepts:

- **Events:** external facts entering the engine.
- **Messages:** deterministic intermediate facts produced inside the engine.
- **Commands:** external effects requested by the engine.
- **Ports:** typed boundaries to systems outside the engine.
- **Components:** deterministic internal application logic.

The design thesis is:

> A single-writer deterministic application graph consumes ordered Events, propagates internal Messages, and emits Commands through typed Ports. Live and simulation use the same application graph, kernel, protocols, and callback code; only Port implementations and environment mechanics differ.

---

## 3. Design Goals

### 3.1 Identical Application Semantics Across Environments

**Decided.** The following are identical in live, historical backtest, replay, and DST:

- Event, Message, and Command protocols.
- Component instances and callback functions.
- Component-local business logic.
- Logical Port Specs.
- Application graph topology.
- Production declarations.
- Graph validation rules.
- Kernel turn semantics.
- Message propagation order.
- Command production order.
- State-transition semantics.
- Causal tracing semantics.

The following may differ:

- Concrete Port implementations.
- Port execution placement.
- Live threads, tasks, processes, and transport mechanisms.
- Simulated external models.
- Real versus virtual time.
- Real versus simulated network and storage behavior.
- Environment lifecycle and operational infrastructure.

Parity means shared application behavior and contracts, not physically identical IO runtimes.

### 3.2 Strict Determinism

**Decided.** Kavod's determinism claim is:

> Given the same application version, protocol version, graph, initial state, configuration, ordered Event tape, acceptance timestamps, and deterministic choice tape, the kernel produces the same ordered Message flow, Command tape, terminal state, and state hashes.

The phrase "same set of inputs" is insufficient because a set has no order. Ordering, timing, configuration, model versions, and deterministic choices are inputs.

Historical backtests and DST runs are deterministic during execution. Live runs contain unavoidable environmental nondeterminism, but the kernel freezes the observed Event order at acceptance so the run can be replayed deterministically.

### 3.3 Full-System DST From The Beginning

**Decided.** Simulation is a first-class runtime environment, not a test helper layered over a live engine.

The simulation environment must be capable of controlling:

- Virtual time.
- Event ordering.
- Port latency.
- Port availability.
- Network delivery models.
- Duplicate, delayed, reordered, and missing external responses.
- Storage completion and failure models where applicable.
- Crash and restart points.
- Workload generation.
- Randomized scheduling and fault decisions.

Every failing simulation must be reproducible from captured provenance.

### 3.4 Static, Executable, Inspectable Graph

**Decided.** The application graph is derived from actual registrations:

- Registering an actual callback creates an input edge.
- Declaring an actual callback production creates an output edge.
- Declaring a Port Spec creates a boundary node.
- Binding a Port implementation attaches an implementation to that boundary node.

There is no independent graph description that can drift from executable code.

The graph must support startup validation, human-readable diagnostics, and machine-readable export.

### 3.5 No Silent Drops

**Decided.** Kavod must not silently lose an Event, Message, Command, Port failure, queue overflow, or structural routing error.

At minimum:

- Every Event variant a bound Port may emit must have at least one Component consumer unless explicitly marked observational-only by a future approved mechanism.
- Every Message production must have at least one Component consumer.
- Every Command production must resolve to exactly one declared Port instance unless explicit multicast semantics are later added.
- Every declared Port must have exactly one environment binding.
- Runtime emission of an undeclared Message or Command is an invariant violation.
- Queue overflow and Port disconnection are visible failures or explicit domain Events, never implicit loss.

### 3.6 Capability Isolation

**Decided.** Component callbacks never receive `&mut Engine`, `&mut Kernel`, a scheduler, a wall clock, a channel, a Port implementation, or an executor.

Components can only:

- Inspect the current frozen logical time.
- Read permitted application state.
- Mutate their private state.
- Mutate canonical state only through the state capability chosen by the final state model.
- Emit declared Messages.
- Emit declared Commands to declared Port Specs.
- Request deterministic IDs or randomness only through separately designed capabilities if those capabilities are approved.

### 3.7 Domain-Agnostic Kernel

**Decided.** The kernel does not know about:

- Market data.
- Orders.
- Fills.
- Instruments.
- Venues.
- Strategies.
- Risk checks.
- IBKR, Tasty, FIX, or any other protocol.

The application defines all Event, Message, Command, Port, routing, and domain-state types.

### 3.8 One Source Of Truth For Wiring

**Decided.** Registration both installs behavior and declares topology. Kavod will not require users to keep callback code and a separate graph specification synchronized.

### 3.9 Configuration Is Not Mechanism

**Decided.** Users configure Port capacity, overflow policy, latency models, fault models, lifecycle behavior, and deployment choices. Runtime mechanisms such as channels, threads, schedulers, and task queues remain private.

### 3.10 Independent Engine Instances

**Decided.** There is no process-global mutable kernel state. Multiple backtests, simulations, or engines can run independently in one process, including on separate OS threads.

---

## 4. Non-Goals

v4 does not attempt to:

- Make live environmental behavior deterministic while it is happening.
- Make a simulated exchange internally identical to a real exchange.
- Force live and simulated Port implementations to share source code.
- Use CPU execution duration as simulated business latency.
- Expose channels, executors, or scheduler handles as application APIs.
- Support runtime mutation of the application graph in v1.
- Infer all possible callback outputs by inspecting arbitrary Rust function bodies.
- Eliminate every internal trait object; the prohibition is against `dyn Message` payloads and user-facing downcasting, not carefully contained registry erasure.
- Claim that DST replaces real adapter conformance, paper trading, or integration tests.

---

## 5. Architectural Overview

```text
                         EXTERNAL WORLD

        live systems, simulated systems, timers, services
                              │
                              │ Event
                              ▼
                     single acceptance site
                     order + time + journal
                              │
                              ▼
┌──────────────────────────────────────────────────────────┐
│                    KAVOD KERNEL                          │
│                                                          │
│  Event                                                   │
│    │                                                     │
│    ▼                                                     │
│  deterministic Components                               │
│    │       ▲                                             │
│    └─ Message ───────────────────────────────┐            │
│                                             │            │
│  private/canonical State                    │            │
│                                             ▼            │
│                                         Command          │
└─────────────────────────────────────────────┬────────────┘
                                              │
                                              ▼
                                      typed Port boundary
                                              │
                                              ▼
                                       EXTERNAL WORLD
```

The legal directions are:

```text
Port      ── Event   ──► Component
Component ── Message ──► Component
Component ── Command ──► Port
```

The following are illegal:

```text
Component produces Event
Port produces Message
Component consumes Command
Port consumes Message
```

Commands may cause a Port to emit a later Event, creating an explicit external feedback loop:

```text
Component ── SubmitOrder Command ──► Execution Port
Component ◄── OrderAccepted Event ── Execution Port
```

---

## 6. Core Terminology

| Term | Definition |
|---|---|
| Event | An immutable fact observed outside the deterministic application and accepted into the kernel through a Port. |
| Message | An immutable deterministic intermediate fact produced and consumed entirely within the application graph. |
| Command | An immutable request for an effect outside the deterministic application, directed to a Port. |
| Port Spec | An application-defined typed external contract associating accepted Command types with emitted Event types. |
| Port implementation | A live or simulated implementation satisfying one Port Spec. |
| Component | Deterministic internal logic with private state and typed Event/Message callbacks. |
| Callback | An actual registered Component function consuming one Event or Message payload. |
| Application graph | The immutable topology formed from Component callbacks, productions, and logical Port Specs. |
| Binding | The association of one logical Port Spec instance with one concrete environment implementation. |
| Environment | Runtime machinery that obtains Events from bound Ports and delivers Commands to them. |
| Kernel | The single-writer deterministic executor of accepted Events and internal Messages. |
| Turn | Processing one accepted Event and all causally resulting internal Messages until quiescence. |
| Acceptance | The single operation that freezes an external Event's order, logical time, source, and journal identity before processing. |
| Domain time | Time meaningful to a payload, such as exchange event time. Stored in the payload. |
| Acceptance time | Frozen logical time assigned when the kernel accepts an Event. Exposed as `ctx.now()`. |
| Wall time | Real operating-system time. Never exposed directly to Components. |
| Virtual time | Simulation-controlled time. Used by the simulation environment and accepted as Event time. |
| Event index | Kernel-internal monotonic identity establishing total order among accepted Events. |
| Causation | Relationship connecting a root Event to resulting Messages and Commands. |
| Business ID | Domain identity such as client order ID. Separate from Event index and trace identity. |

---

## 7. Events

### 7.1 Meaning

**Decided.** An Event is an external fact from the application's perspective. "External" means outside the deterministic application graph, not necessarily outside the OS process.

Therefore all of the following are Events:

- A quote from a live market-data adapter.
- A historical bar emitted by a backtest market Port.
- A simulated fill emitted by a simulated execution Port.
- A real fill emitted by a broker adapter.
- A timer firing.
- A response from an ML inference service.
- An operator request entering through a control Port.
- A Port lifecycle outcome intentionally modeled as domain information.

### 7.2 Event Ownership

**Decided.** Only Ports emit Events. Components cannot synthesize an Event. A Component that derives new information emits a Message instead.

This distinction makes replay unambiguous:

- Replay injects recorded Events.
- Replay recomputes Messages.
- Replay compares Commands.

### 7.3 Event Immutability

**Decided.** Event payloads are immutable after acceptance. Components receive typed shared references and cannot mutate the accepted payload.

### 7.4 Event Time

**Decided.** Domain timestamps remain ordinary payload fields:

```rust
pub struct Trade {
    pub event_time: Timestamp,
    pub instrument: InstrumentId,
    pub price: Price,
    pub quantity: Quantity,
}
```

The kernel does not reorder live Events by `event_time`. Live ordering is the order accepted at ingress. If an application requires domain-time reordering, that behavior must be an explicit deterministic Component with explicit buffering and lateness policy.

### 7.5 Event Delivery

**Decided.** An Event may fan out to multiple Component callbacks. Matching callbacks run in deterministic registration order. All callbacks for the current Event and all resulting internal Messages complete before the next accepted Event begins processing.

### 7.6 Event Source Attribution

**Decided.** Each Event acceptance records a stable logical source identifying the bound Port instance or other approved ingress source. Source identity is diagnostic and replay metadata, not a business identifier or routing key.

---

## 8. Messages

### 8.1 Meaning

**Decided.** A Message is a deterministic fact derived inside the application graph.

Examples:

- `BarCompleted`
- `MovingAverageCrossed`
- `TargetPositionChanged`
- `OrderIntentCreated`
- `RiskApproved`
- `RiskRejected`
- `PositionChanged`

### 8.2 Message Boundary

**Decided.** Messages never cross a Port boundary. Ports cannot consume or emit Messages.

### 8.3 Message Replay

**Decided.** Internal Messages are not replay inputs. They are regenerated by executing the same Component callbacks against replayed Events.

Messages may be recorded in a diagnostic causal trace. Such records are observations of deterministic execution, not inputs to replay.

### 8.4 Message Scheduling

**Decided.** Messages are immediate within the current Event turn. They do not independently advance logical time.

A need to wait, schedule future work, or model latency crosses an external-time boundary and must be represented as a Command to an appropriate Port, followed by a later Event.

Examples:

```text
SetTimer Command → Timer Port → TimerFired Event
RunInference Command → Inference Port → InferenceCompleted Event
SubmitOrder Command → Execution Port → OrderAccepted/Fill Event
```

This replaces v2/v3 `send_at` for application-produced work. Future scheduling belongs to environment Ports, not to arbitrary internal Messages.

### 8.5 Message Ordering

**Decided.** The kernel maintains an internal FIFO for Messages produced during one turn:

1. Matching callbacks for the current input run in registration order.
2. Messages produced by those callbacks append to the turn queue in production order.
3. The next Message is popped from the front.
4. Its matching callbacks run in registration order.
5. Newly produced Messages append to the back.

This yields deterministic breadth-first propagation and prohibits recursive callback dispatch.

### 8.6 Message Cycles

**Decided.** The runtime must prevent an unbounded Message cycle from hanging a turn.

**Open.** The exact combination of static cycle analysis, causal-depth limits, total-message limits, and per-root descendant limits is unresolved.

---

## 9. Commands

### 9.1 Meaning

**Decided.** A Command requests an external effect. A Command does not prove that the effect occurred.

```text
SubmitOrder Command != OrderAccepted Event
CancelOrder Command != OrderCancelled Event
SetTimer Command != TimerFired Event
```

### 9.2 Command Producers And Consumers

**Decided.** Only Components produce Commands. Only Ports consume Commands.

### 9.3 Command Addressing

**Decided.** A Command production is directed to one logical Port Spec instance. Commands are not type-broadcast to all Ports.

The application may express destination using:

- A distinct Port marker type, such as `EquitiesExecution` or `OptionsExecution`.
- A future typed route selector over stable, low-cardinality dimensions.

For v1, separate logical Port marker types are the preferred simple mechanism.

### 9.4 Command Order

**Decided.** Commands produced in a turn have deterministic production order. The environment must preserve per-Port submission order.

After submission to different live Ports, no cross-Port completion ordering is promised. Resulting Events are ordered when accepted back into the kernel.

### 9.5 Command Outcomes

**Decided.** Normal external outcomes return as Events, including rejection, timeout, disconnect, and ambiguous status when those are part of the domain protocol.

Infrastructure failure to deliver a Command to a Port is not silently converted into success. The exact division between a domain Event and runtime fault is Port-protocol-specific and must be explicit.

### 9.6 Command Durability

**Recommended.** In live mode, externally consequential Commands should be durably recorded before submission, using an outbox-like discipline. This supports crash recovery, resend, and audit.

**Open.** Exact transactional boundaries among Event record, state mutation, Command record, and external submission are not finalized.

### 9.7 Retry And Idempotency

**Decided.** The kernel does not silently retry Commands. Retry policy must be explicit application logic, explicit Port protocol behavior, or explicit environment policy.

Externally consequential Commands require deterministic business IDs or idempotency keys. The facility for generating those IDs remains open.

---

## 10. Application Protocol Types

### 10.1 Closed Protocol For v1

**Decided.** v1 uses closed concrete enums rather than `Arc<dyn Message>`, `Any`, or user-facing downcasts.

Conceptually:

```rust
pub enum TradingEvent {
    Market(MarketEvent),
    EquitiesExecution(ExecutionEvent),
    OptionsExecution(ExecutionEvent),
    Timer(TimerEvent),
    Inference(InferenceEvent),
    Control(ControlEvent),
}

pub enum TradingMessage {
    BarCompleted(Bar),
    TargetPosition(TargetPosition),
    OrderIntent(OrderIntent),
    RiskApproved(ApprovedOrder),
    RiskRejected(RiskRejection),
}

pub enum TradingCommand {
    Market(MarketCommand),
    EquitiesExecution(ExecutionCommand),
    OptionsExecution(ExecutionCommand),
    Timer(TimerCommand),
    Inference(InferenceCommand),
    Control(ControlCommand),
}
```

The kernel is generic over the application protocol. It does not match domain variants itself.

### 10.2 Why Closed Enums

Closed enums provide:

- Exhaustive protocol visibility.
- Stable serialization planning.
- No payload vtable.
- No `Any` or runtime downcast in user code.
- Compile-time rejection of payloads outside the protocol.
- Clear replay schemas.
- A natural place for stable variant identifiers.

### 10.3 Typed Registration Over Enum Variants

**Recommended.** Derive-generated traits map inner payload types to their containing enum variants so user callbacks remain typed:

```rust
c.on_message(MaCross::on_bar)
    .produces_message::<TargetPosition>();
```

The callback receives `&Bar`, not `&TradingMessage` and not `&dyn Message`.

### 10.4 Future Protocol Macro

**Recommended, deferred.** After v1 semantics stabilize, a centralized protocol macro may generate top-level enums, Port marker types, variant tags, conversions, extraction traits, and graph metadata:

```rust
kavod::protocol! {
    pub TradingProtocol {
        ports {
            MarketData: MarketCommand => MarketEvent,
            EquitiesExecution: ExecutionCommand => ExecutionEvent,
            OptionsExecution: ExecutionCommand => ExecutionEvent,
            Timer: TimerCommand => TimerEvent,
        }

        messages {
            Bar,
            TargetPosition,
            OrderIntent,
            ApprovedOrder,
            RiskRejection,
        }
    }
}
```

Rust cannot reliably discover an application-wide closed enum by inspecting unrelated registrations across modules. A centralized aggregation point remains required. Linker inventory and runtime discovery are rejected because they weaken stable ordering, closed-world validation, and protocol visibility.

### 10.5 Stable Type Identity

**Decided.** Durable logs cannot use Rust `TypeId` as a cross-build schema identifier.

**Open.** Stable variant identifiers, encoding, schema evolution, compatibility, and unknown-variant policy remain unresolved.

---

## 11. Components

### 11.1 Definition

**Decided.** A Component is deterministic application logic. It may own private state and register callbacks for Events and Messages.

Components may emit declared Messages and Commands. Components never perform external IO.

Examples:

- Bar aggregator.
- Strategy.
- Portfolio projector.
- Order-state projector.
- Risk policy.
- Order planner.
- Execution coordinator.
- Reconciliation policy.

### 11.2 Determinism Contract

Given the same:

- Input payload.
- Frozen callback time.
- Component private state.
- Permitted canonical state view.
- Configuration.
- Approved deterministic capabilities.

a Component callback must produce the same:

- Private-state transition.
- Canonical-state transition, if permitted.
- Ordered Messages.
- Ordered Commands.
- Error or success result.

Components must not observe:

- Wall time.
- OS scheduling.
- Thread identity.
- Network or filesystem state.
- Environment variables after build.
- OS entropy.
- Process-global mutable state.
- Unspecified hash-map iteration when order can affect output.

### 11.3 Component Execution

**Decided.** Component callbacks are synchronous and non-blocking. They execute on the single kernel thread, one callback at a time.

Heavy computation that must not block the kernel is represented through a service Port:

```text
Component emits RunInference Command
Inference Port performs work
Inference Port emits InferenceCompleted Event
```

### 11.4 Component Registration

**Decided.** Registering an actual function creates the graph input edge. Output declarations are scoped to the exact callback.

Recommended reusable style:

```rust
impl Component for OrderRouter {
    fn register(reg: &mut ComponentRegistrar<Self>) {
        reg.on_message(Self::on_approved)
            .produces_command::<EquitiesExecution>()
            .produces_command::<OptionsExecution>();
    }
}
```

Recommended application-local style:

```rust
app.component(OrderRouter::new(config), |c| {
    c.on_message(OrderRouter::on_approved)
        .produces_command::<EquitiesExecution>()
        .produces_command::<OptionsExecution>();
});
```

Both compile to the same internal registration representation.

### 11.5 Production Declarations

**Decided.** Every callback declares every Message type and Command Port it may produce.

Production declarations provide:

- Graph edges.
- Startup connectivity validation.
- Runtime output authorization.
- Causal trace metadata.
- Diagnostics describing exact potential flow.

An application-level declaration such as "this application may produce `ExecutionCommand`" is too coarse because it loses the producing callback and causal edge.

### 11.6 Output Inference

**Decided.** v1 does not attempt to infer outputs from callback function bodies. Helper functions and conditional control flow make such inference unsound.

**Open.** A future component attribute macro may generate registration from explicit attributes. Encoding output sets in callback return types is possible but not currently recommended because it may harm ergonomics.

### 11.7 Component Identity

**Decided.** Strings do not determine routing or correctness.

**Recommended.** Component Rust type plus an optional typed instance key identifies a Component. Human-readable names remain diagnostic labels.

**Open.** Durable Component identity across source changes, duplicate instances, and replay provenance is not finalized.

### 11.8 Internal Erasure

**Recommended.** The user-facing API remains fully typed while heterogeneous Component registries use narrow internal trait-object erasure.

This is acceptable because:

- Event, Message, and Command payloads remain concrete enums.
- Users never downcast.
- Type relationships are proven by typed registration wrappers.
- Erasure is localized to kernel registry machinery.

Large generic tuples or HLists are rejected as the default public model because they substantially worsen ergonomics without improving semantic correctness.

---

## 12. Ports

### 12.1 Port Spec

**Decided.** A Port Spec is an application-defined logical external contract:

```rust
pub trait PortSpec: 'static {
    type Command;
    type Event;
}
```

Examples:

```rust
pub struct MarketData;

impl PortSpec for MarketData {
    type Command = MarketCommand;
    type Event = MarketEvent;
}

pub struct EquitiesExecution;

impl PortSpec for EquitiesExecution {
    type Command = ExecutionCommand;
    type Event = ExecutionEvent;
}

pub struct OptionsExecution;

impl PortSpec for OptionsExecution {
    type Command = ExecutionCommand;
    type Event = ExecutionEvent;
}
```

Kavod provides one generic Port mechanism. It does not hardcode market-data, execution, timer, or service Port kinds.

### 12.2 Port Direction

**Decided.** A Port accepts its associated Command type and emits its associated Event type.

The same logical Port Spec is present in every environment. The implementation changes.

### 12.3 Port Implementations

**Decided.** Live and simulated implementations share the Port Spec but do not need to share an execution interface or source code.

This is intentional:

- A live implementation interacts with a real external system.
- A simulated implementation is a deterministic model controlled by virtual time.
- Their commonality is the application-facing protocol.

### 12.4 Live Port

Conceptually:

```rust
pub trait LivePort<P: PortSpec>: Send + 'static {
    fn run(
        self,
        io: LivePortIo<P::Command, P::Event>,
    ) -> Result<(), PortError>;
}
```

The exact trait signature is **Open**. A live Port may internally use blocking IO, async IO, a third-party runtime, or a process proxy. The Port implementation must not choose or expose its kernel-side mailbox mechanism.

### 12.5 Simulated Port

Conceptually:

```rust
pub trait SimPort<P: PortSpec> {
    fn on_command(
        &mut self,
        now: Timestamp,
        command: P::Command,
        sim: &mut SimContext<P::Event>,
    );
}
```

A simulated Port is a synchronous deterministic state machine. It may schedule future Events in virtual time through `SimContext`.

The exact trait signature is **Open**, but the synchronous state-machine semantics are **Decided** for v1.

### 12.6 Port Placement

**Decided.** Port implementations do not decide their own kernel execution placement.

The live environment may place a Port in:

- A dedicated OS thread.
- A runtime task.
- A thread pool.
- A separate process behind a typed proxy.

The simulation environment places all simulated Ports under one deterministic single-threaded scheduler.

Thread/task/process placement is deployment configuration, not application semantics.

### 12.7 Port State

**Decided.** Port implementation state is external to deterministic application state.

Examples:

- Network connection state.
- FIX sequence state.
- Exchange session token.
- Simulated exchange book.
- Simulated venue latency and queue state.

Port state cannot be directly borrowed by Components. Relevant facts enter as Events.

### 12.8 Port Lifecycle

**Open.** Readiness, startup ordering, reconciliation, arming, health, reconnect, shutdown, draining, and supervision protocols remain unresolved.

The design constrains future lifecycle work:

- Lifecycle must not expose transport primitives to Components.
- Expected operational outcomes should be modeled as typed Events where application reaction is required.
- Infrastructure faults must be observable.
- No silent automatic retry or restart is permitted unless explicitly configured and tested.

### 12.9 Port Capacity And Backpressure

**Decided.** Live Port command mailboxes and central ingress queues have explicit capacity and overflow policies. Silent dropping is forbidden by default.

The simulation environment must be capable of modeling the same capacity and backpressure policies.

**Open.** Default capacities, default overflow policy, blocking behavior, coalescing support, and fairness are unresolved.

### 12.10 Remote Ports

**Recommended.** A Port in another process is represented inside the engine process by a typed proxy satisfying the same Port Spec:

```text
Kernel → typed Command → proxy → serialized IPC → remote adapter
Kernel ← typed Event   ← proxy ← serialized IPC ← remote adapter
```

**Open.** IPC transport, wire format, authentication, compatibility negotiation, and delivery guarantees are unresolved.

---

## 13. Application Graph

### 13.1 Graph Nodes

**Decided.** The logical graph contains:

- Component callback nodes or Component nodes with callback metadata.
- Port Spec boundary nodes.
- Event edges from Ports to callbacks.
- Message edges between callbacks.
- Command edges from callbacks to Ports.

Concrete live or simulated implementation internals are not graph nodes. Binding diagnostics may annotate Port nodes with their concrete implementation.

### 13.2 Graph Edges

```text
Port --Event--> Component callback
Component callback --Message--> Component callback
Component callback --Command--> Port
```

Each edge corresponds to actual executable registration metadata.

### 13.3 Graph Construction

**Decided.** Application graph construction is separate from environment binding:

```rust
let app = TradingApplication::builder()
    .port::<MarketData>()
    .port::<EquitiesExecution>()
    .port::<OptionsExecution>()
    .add(BarAggregator::new())
    .add(MaCross::new(config.ma))
    .add(OrderPlanner::new(config.orders))
    .add(RiskPolicy::new(config.risk))
    .add(OrderRouter::new(config.routing))
    .build()?;
```

Exact builder syntax is **Open**. The separation is **Decided**.

### 13.4 Graph Immutability

**Decided.** The graph is validated and frozen before processing the first Event. Runtime registration and dynamic subscription changes are not supported in v1.

### 13.5 Graph Validation

**Decided.** Build validation includes:

1. Every Event a declared Port may emit has at least one matching Component callback.
2. Every declared Message production has at least one matching Component callback.
3. Every declared Command production targets a declared Port.
4. Every callback emission type belongs to the closed application protocol.
5. Every callback's production declarations are internally valid.
6. Every Port has exactly one binding in the selected environment.
7. Every binding implements the appropriate live or simulated interface for the Port Spec.
8. No binding is supplied for an undeclared Port.
9. Callback and fan-out order is stable.
10. Immediate Message cycles are statically diagnosed where possible.

### 13.6 Runtime Validation

**Decided.** Runtime defense-in-depth checks include:

- Accepted Event variant is declared for its source Port.
- Accepted Event has a consumer.
- Emitted Message is declared by the current callback.
- Emitted Command target is declared by the current callback.
- Emitted payload belongs to the protocol.
- Command destination is bound.
- Turn limits are not exceeded.

### 13.7 Fine-Grained Routing

**Open.** v1 may begin with variant-level fan-out and distinct logical Port marker types.

Future fine-grained routing should preserve the prior `route_key_design.md` principles:

- Route on stable, low-cardinality dimensions such as venue, instrument, timeframe, asset class, or strategy.
- Correlate dynamic, high-cardinality IDs such as order IDs through state, not subscriptions.
- Avoid arbitrary predicate routing in the validated graph.
- Preserve deterministic ordering among matching consumers.
- Validate selector coverage over a declared finite universe where possible.

### 13.8 Graph Export

**Recommended.** The frozen graph should be exportable as:

- Human-readable startup diagnostics.
- Graphviz/DOT.
- Mermaid.
- JSON metadata for tooling.

Export format is not part of runtime semantics.

---

## 14. Port Binding

### 14.1 Meaning

**Decided.** Binding associates one declared logical Port Spec instance with one concrete environment implementation.

Compile time verifies that an implementation satisfies the required generic interface. Build time verifies that every required Port has exactly one binding.

### 14.2 Live Binding

Conceptually:

```rust
let environment = LiveEnvironment::builder()
    .bind::<MarketData>(IbkrMarketData::new(market_config))
    .bind::<EquitiesExecution>(IbkrExecution::new(ibkr_config))
    .bind::<OptionsExecution>(TastyExecution::new(tasty_config))
    .build(&application)?;
```

### 14.3 Simulation Binding

Conceptually:

```rust
let simulation = HistoricalSimulation::new(
    historical_data,
    simulation_config,
);

let environment = SimulationEnvironment::builder()
    .bind::<MarketData>(simulation.market_data())
    .bind::<EquitiesExecution>(simulation.equities_exchange())
    .bind::<OptionsExecution>(simulation.options_exchange())
    .build(&application)?;
```

The shared `HistoricalSimulation` may coordinate market source and exchange models internally while exposing separately typed logical Port bindings.

### 14.4 Runtime Selection

**Decided.** Environment selection is allowed at the composition root and forbidden inside Components.

Implementations may be selected from configuration using statically typed enums or factories. Runtime selection does not weaken the compile-time requirement that each selectable implementation satisfy its Port Spec.

### 14.5 Same Logical Graph

**Decided.** Live and simulation declare the same logical Ports and Components. Binding changes only the implementation attached to each Port node.

The composite diagnostic graph may show:

```text
EquitiesExecution → IbkrExecution        (live)
EquitiesExecution → SimulatedEquities    (simulation)
```

The application graph edge into `EquitiesExecution` remains unchanged.

---

## 15. Kernel Execution Model

### 15.1 Single Writer

**Decided.** One kernel thread owns and mutates deterministic application state. No Component callback overlaps another callback.

Live Ports may run concurrently, but they communicate with the kernel only through Event ingress and Command egress boundaries.

### 15.2 Conceptual Loop

```rust
while let Some(raw_event) = environment.next_event()? {
    let accepted = kernel.accept_event(raw_event)?;
    let commands = kernel.process_turn(accepted)?;
    environment.submit(commands)?;
}
```

The same kernel acceptance and turn-processing code runs in every environment. `Environment::next_event` and `Environment::submit` have different live and simulation mechanisms.

The exact public `Environment` trait is **Open**. The semantic loop is **Decided**.

### 15.3 Turn Semantics

**Decided.** One accepted Event is the root of one turn:

1. Accept and order the Event.
2. Record required Event metadata before callback dispatch.
3. Create callback context with frozen acceptance time and causation root.
4. Run matching Event callbacks in deterministic order.
5. Append emitted Messages to the internal FIFO.
6. Process Messages breadth-first until the FIFO is empty.
7. Collect Commands in deterministic production order.
8. Finalize turn state and causal metadata.
9. Record required output verification data.
10. Submit Commands to bound Ports according to durability policy.

No later external Event is processed until the current turn reaches quiescence.

### 15.4 No Reentrancy

**Decided.** Emitting a Message or Command never recursively invokes another callback or Port. Messages enqueue for later processing in the same turn. Commands leave only after turn finalization.

### 15.5 Concurrent Live Arrival

Events emitted by live Port threads or processes enter one central serialized acceptance boundary:

```text
Market Port Event ─┐
IBKR Event ────────┼──► ingress queue ─► accept_event ─► Event index
Tasty Event ───────┤
Timer Event ───────┘
```

Events arriving during a turn wait. They cannot observe partially transitioned state.

### 15.6 Internal Operation Order

**Decided.** Event indices and internal causal operation indices are separate concepts.

- Event index establishes accepted external Event order.
- Internal causal ordinals identify Messages, callback invocations, and Commands for tracing.
- Neither is exposed as a domain business ID.

**Open.** Exact internal trace-index structure and whether every callback invocation is persisted are unresolved.

---

## 16. Time Model

### 16.1 Time Categories

| Time | Meaning | Component access |
|---|---|---|
| Domain time | Time carried by a business payload, such as exchange event time | Through payload fields |
| Acceptance time | Frozen logical time assigned when Event is accepted | `ctx.now()` |
| Wall time | Real OS time | Never directly |
| Virtual time | Simulation-controlled environment time | Becomes acceptance time when Event is accepted |

### 16.2 Frozen Callback Time

**Decided.** Every callback in one Event turn observes the same `ctx.now()`: the root Event's acceptance time.

Internal Message propagation does not advance time.

### 16.3 Live Time

**Decided.** Live acceptance reads the ingress time authority once per Event. That value is frozen and journaled.

The callback does not observe later wall time even if processing takes time.

### 16.4 Simulation Time

**Decided.** The simulation scheduler advances virtual time only by popping the next scheduled simulation action. An Event emitted by that action is accepted at the corresponding virtual time.

### 16.5 Future Work

**Decided.** Components do not schedule future Messages directly. Future work uses Command/Event protocols such as Timer, Execution, or service Ports.

### 16.6 Time Monotonicity

**Decided.** Accepted Events must not move kernel logical time backward. Domain time may be older or newer than acceptance time and remains payload data.

**Open.** Exact policy for a simulation model attempting to schedule into the past is unresolved; it must be a visible causality violation.

---

## 17. State Model

### 17.1 Settled Constraints

**Decided.** Deterministic application state includes:

- Component-private state.
- Any canonical shared application state approved by the final state model.
- Kernel causal and ordering state required for replay.

State must be:

- Owned by one Engine instance.
- Mutated only on the kernel thread.
- Reconstructable from initial state plus accepted Events, or recoverable from a compatible snapshot plus later Events.
- Included in deterministic state hashing or otherwise covered by replay verification.
- Inaccessible to live Port threads except through Events and Commands.

No shared `RwLock<Cache>` or asynchronous borrowing of application state is permitted.

### 17.2 Component-Private State

**Decided.** A stateful Component owns private state and receives mutable access only while one of its callbacks runs. No other Component directly borrows that private state.

### 17.3 Canonical Shared State

**Recommended.** Preserve the v2/v3 projector/reducer principle:

- Dedicated projection callbacks mutate canonical shared state.
- Projection callbacks do not emit Messages or Commands.
- Matching projections run before decision callbacks for the same input.
- Decision Components receive read-only canonical state.

This provides updated portfolio, order, and market projections before strategies and policies react.

### 17.4 Open State Decisions

The following are intentionally **Open**:

- Whether projectors are a distinct public Component kind or a callback capability on ordinary Components.
- Whether canonical state uses the v3 typed keyed cache.
- Whether Components may depend on typed read-only projections directly.
- Exact projection order across Events and Messages.
- Whether Messages may trigger canonical projections before subsequent Message consumers.
- Snapshot trait and serialization format.
- State schema migration.
- Key-stability enforcement if the keyed cache is retained.

### 17.5 Deterministic Data Structures

**Decided.** Application behavior must not depend on nondeterministically seeded hash iteration.

**Recommended.** Use ordered collections where iteration affects outputs. Fixed-seed hash maps may be used for lookup-only hot paths provided iteration order is not observed semantically.

### 17.6 Numeric Determinism

**Recommended.** Trading values use checked deterministic decimal or integer representations. Uncontrolled floating-point behavior should not determine Commands or persisted state.

Exact numeric primitive policy remains **Open** outside existing `Price`, `Quantity`, `Timestamp`, and decimal modules.

---

## 18. Context Capabilities

### 18.1 Component Context

Conceptually:

```rust
pub struct ComponentCtx<'a, P: ApplicationProtocol> {
    now: Timestamp,
    state: StateView<'a>,
    outputs: &'a mut TurnOutputs<P>,
    declarations: &'a ProductionSet,
    causation: CausationId,
}
```

The exact type is **Open**. The capability boundary is **Decided**.

### 18.2 Permitted Capabilities

Depending on callback kind, context may expose:

- `now()`.
- Read-only canonical state access.
- Declared Message emission.
- Declared Command emission to a typed Port Spec.
- Diagnostic causation access if approved.
- Future deterministic ID or RNG capabilities if separately approved.

### 18.3 Forbidden Capabilities

Context never exposes:

- Wall clock.
- Scheduler.
- Event index as a business identifier.
- Port implementation.
- Channel sender or receiver.
- Executor.
- Engine mode.
- Mutable access to another Component's private state.
- External IO.

### 18.4 Emission APIs

Recommended conceptual APIs:

```rust
ctx.message(TargetPosition { ... });

ctx.command::<EquitiesExecution>(
    ExecutionCommand::Submit(order),
);
```

The exact API is **Open**. Emission must enforce the current callback's production declarations.

### 18.5 RNG

**Open.** Component randomness has not been approved or designed in v4.

If added, randomness must be deterministic, seeded or tape-driven, captured in replay provenance, and unavailable through ordinary OS APIs.

---

## 19. Live Environment

### 19.1 Topology

```text
                      Command mailbox
Kernel thread ─────────────────────────────► Market Port thread/task
Kernel thread ─────────────────────────────► IBKR Port thread/task
Kernel thread ─────────────────────────────► Tasty Port thread/task

                      central Event ingress
Kernel thread ◄───────────────────────────── all live Ports
```

### 19.2 Single Kernel Thread

**Decided.** Components and application state remain single-threaded in live mode. Only Port implementations and environment infrastructure run concurrently.

### 19.3 Live Port Ordering

**Decided.** Per-Port Command submission order is FIFO unless a future Port Spec explicitly defines otherwise.

Events from different Ports race at the central ingress. The kernel assigns total order in the order accepted. That observed order is part of the live Event tape.

### 19.4 Port Threading

**Decided.** A live Port implementation does not directly expose or control kernel channels. Environment machinery constructs mailboxes and chooses placement.

**Open.** Default placement policy, runtime choice, thread count, task model, and process model remain unresolved.

### 19.5 Async Runtime

**Decided.** The deterministic application does not depend on Tokio or any live async runtime.

A live Port may internally use an async runtime, but its nondeterministic completion order is external behavior captured as Event ingress.

### 19.6 Live Logging

**Decided.** Nondeterministic live Events are recorded before Component processing. Live Commands are recorded sufficiently for audit and replay comparison.

**Open.** Durable storage engine, fsync policy, batching, and transaction format remain unresolved.

---

## 20. Historical Backtest Environment

### 20.1 Definition

**Decided.** A historical backtest is a deterministic simulation using recorded market input and simulated external systems.

It is not a special application or alternate strategy node.

### 20.2 Topology

```text
one OS thread
└── deterministic simulation environment
    ├── historical market source
    ├── virtual clock
    ├── discrete-event scheduler
    ├── simulated equity execution Port
    ├── simulated options execution Port
    ├── kernel
    └── Components and state
```

### 20.3 Simulated Venue Market Data

**Decided.** The simulation environment owns both the historical market source and simulated exchange state.

For a market occurrence at virtual time `T`, the environment defines a deterministic causal order such as:

1. Apply the occurrence to the simulated exchange's private market model.
2. Emit the corresponding Market Event into the kernel.
3. Process the Event turn and resulting Commands.
4. Deliver execution Commands to the simulated exchange according to modeled outbound latency.
5. Schedule acknowledgements, fills, rejections, and other Events.

This avoids both fake live subscriptions and hidden racing side channels.

**Open.** Exact market occurrence micro-ordering, exchange arrival semantics, and handling of same-timestamp market and order actions are simulation-model decisions that require explicit specification.

### 20.4 Historical Data Loading

**Recommended.** Historical sources are lazy and bounded-memory. The simulation scheduler keeps only necessary next actions rather than loading an entire dataset.

Exact source API, multi-source merge semantics, and storage formats remain **Open**.

### 20.5 Parallel Backtests

**Decided.** Parallel optimization runs independent Engine and simulation instances on separate OS threads or processes. One backtest instance remains single-threaded internally.

---

## 21. Deterministic Simulation Environment

### 21.1 Discrete-Event Scheduler

**Decided.** v1 DST uses no ordinary multi-threaded async runtime. It runs one deterministic discrete-event scheduler on one OS thread.

Conceptually, scheduled actions are ordered by:

```text
(virtual_time, deterministic_tie_breaker)
```

Actions may include:

- Emit an Event into the kernel.
- Deliver a Command to a simulated Port.
- Deliver a simulated network packet.
- Wake a virtual timer.
- Apply a market occurrence to a world model.
- Crash or restart a simulated Port.
- Change simulated connectivity.
- Complete simulated storage IO.

### 21.2 Simulation Loop

```text
pop earliest scheduled action
    ↓
advance virtual time
    ↓
execute action synchronously
    ↓
if action emits Event:
    accept Event
    run kernel turn to quiescence
    collect Commands
    deliver/schedule Commands in simulation
    ↓
repeat
```

No OS scheduling decision affects the simulated result.

### 21.3 Deterministic Choices

**Decided.** Every randomized simulation decision comes from one controlled deterministic choice source or explicitly derived substreams. OS entropy is forbidden during a run.

**Recommended.** Persist both the seed and a structured decision/fault tape. A seed alone is fragile across refactors that change PRNG draw order.

**Open.** PRNG algorithm, finite-entropy model, substream derivation, shrinking strategy, and tape encoding remain unresolved.

### 21.4 Fault Model

Simulation should support at least:

- Port disconnect and reconnect.
- One-way or asymmetric communication loss where modeled transport supports it.
- Command delivery delay.
- Event delay, duplication, and reordering.
- Ack-before-fill and fill-before-ack orderings.
- Cancel/fill races.
- Partial fills.
- Command rejection.
- Ambiguous timeout outcomes.
- Sequence gaps and retransmission.
- Port stalls and mailbox backpressure.
- Process or Port crash and restart.
- Journal or snapshot failure when persistence simulation exists.
- Clock skew in domain timestamps.

Exact v1 fault catalog is **Open**.

### 21.5 Safety Phase

**Recommended.** During adversarial simulation, invariants are checked after every completed kernel turn and relevant external-world transition.

Examples:

- Filled quantity never exceeds valid remaining quantity.
- Duplicate execution reports are idempotent.
- Order-state transitions are legal.
- Cash, positions, fees, and fills reconcile.
- Risk invariants hold at every deterministic state boundary.
- Commands do not reference unknown domain identities.
- Replay state hashes remain stable.

### 21.6 Liveness Phase

**Recommended.** A simulation run transitions from adversarial fault injection to eventual stabilization:

1. Stop introducing new failures.
2. Restore required connectivity and healthy services.
3. Continue deterministic execution.
4. Require convergence within an explicit virtual-time or step bound.

Possible liveness properties:

- Pending Commands reach a terminal or explicitly ambiguous state.
- Port sessions reconcile.
- Local and simulated venue positions converge.
- The system reaches an operational, disarmed, or explicitly faulted state.
- No internal Message cascade or recovery protocol remains stuck.

### 21.7 Independent Oracles

**Recommended.** DST must not rely solely on production calculations to verify themselves.

Independent checkers should include simple models for:

- Order lifecycle.
- Position and cash accounting.
- Execution uniqueness.
- Simulated venue truth.
- Recovery state.
- Graph and trace consistency.

### 21.8 Simulation Provenance

**Decided.** A reproducible simulation artifact includes enough information to reconstruct the run:

- Application and kernel version.
- Protocol/schema version.
- Graph identity.
- Initial state or snapshot.
- Configuration.
- Seed.
- Decision/fault tape when enabled.
- Model versions.
- Historical input identity where used.

Exact provenance encoding is **Open**.

### 21.9 Adapter-Level DST

**Deferred.** Primary DST uses deterministic simulated Port implementations, not real live adapter runtimes.

Running production adapters under deterministic simulation requires abstraction of network, timers, DNS, storage, randomness, and task scheduling, or a deterministic hypervisor/runtime. This is valuable but is a separate layer of work.

---

## 22. Replay

### 22.1 Replay Input

**Decided.** Replay injects recorded external Events in recorded acceptance order with recorded acceptance times and source attribution.

Replay does not inject recorded internal Messages.

### 22.2 Replay Output

**Decided.** Replay recomputes Messages and Commands using the same application graph and kernel.

No live external effects are executed.

### 22.3 Command Verification

**Decided.** Replay detects missing, extra, or reordered Commands relative to recorded expectations.

**Open.** Exact versus semantic equality, normalization of transport-only fields, per-Port versus global comparison, and causal-chain matching are unresolved.

### 22.4 State Verification

**Recommended.** Replay compares deterministic state hashes at turn boundaries in addition to Command output. Command equality alone may miss internal divergence that happens to produce the same external output.

### 22.5 Replay Ports

**Decided.** Ports that originally emitted Events are not rerun while those Events are replayed. Otherwise replay would duplicate external outcomes.

Replay may use passive Port bindings that accept recomputed Commands only for comparison and never produce Events.

### 22.6 Replay Start

Replay may start from:

- Initial state plus the full Event tape.
- A compatible snapshot plus Events after the snapshot boundary.

Snapshot compatibility and partial replay are **Open**.

### 22.7 Corruption And Compatibility

**Open.** Behavior for corrupted logs, incomplete tails, schema changes, code changes, graph changes, and model changes remains unresolved. Replay must fail visibly rather than silently reinterpret incompatible data.

---

## 23. Acceptance, Journaling, And Causality

### 23.1 Single Acceptance Authority

**Decided.** Every external Event enters through exactly one acceptance method. Conceptually:

```rust
fn accept_event(
    &mut self,
    source: PortInstanceId,
    accepted_at: Timestamp,
    event: TradingEvent,
) -> Result<AcceptedEvent, EngineError>;
```

Acceptance atomically establishes:

- Monotonic Event index.
- Acceptance time.
- Source identity.
- Stable variant identity.
- Replay record requirement.
- Root causation identity.

### 23.2 Event Record

Conceptually:

```rust
pub struct EventRecord<E> {
    pub index: EventIndex,
    pub accepted_at: Timestamp,
    pub source: PortInstanceId,
    pub event_type: StableTypeId,
    pub payload: E,
}
```

The durable representation is **Open**.

### 23.3 Causal Trace

**Decided.** Runtime diagnostics can connect every internal operation to its root Event:

```text
Event 4201: Market.Bar(AAPL)
  Message 4201.1: TargetPosition(AAPL, 100)
    Message 4201.2: OrderIntent(Buy 100 AAPL)
      Message 4201.3: RiskApproved(...)
        Command 4201.4: SubmitOrder → EquitiesExecution
```

This metadata is kernel-managed. Ordinary callbacks receive typed payloads rather than public generic envelopes.

### 23.4 Static And Dynamic Trace

Kavod supports two complementary questions:

- Static graph: what can flow where?
- Dynamic causal trace: why did this particular Command occur?

### 23.5 Trace Persistence

**Open.** The minimum durable record is accepted nondeterministic Events plus enough Command output for replay verification. Persisting every Message and callback invocation is optional diagnostic functionality and not required for deterministic replay.

### 23.6 Business IDs

**Decided.** Event index, Message causal ordinal, and Command causal ordinal are not business identifiers.

**Open.** Deterministic client order IDs, request IDs, and correlation IDs require a separate facility that remains stable under replay and appropriate source changes.

---

## 24. Failure Model

### 24.1 Structural Build Errors

**Decided.** Structural invalidity returns `BuildError` before execution:

- Missing Event consumer.
- Missing Message consumer.
- Missing Command destination.
- Ambiguous Command destination.
- Missing Port binding.
- Duplicate binding.
- Invalid protocol registration.
- Invalid production declaration.
- Invalid initial state.
- Unresolved required capacity or policy.

### 24.2 Domain Outcomes

**Decided.** Expected operational outcomes use Events or Messages, not infrastructure exceptions:

- Order rejected.
- Venue disconnected, when application reaction is part of the protocol.
- Request timed out.
- Risk rejected.
- Trading disabled.
- Reconciliation mismatch.

### 24.3 Runtime Invariant Violations

Examples:

- Undeclared Message or Command emission.
- Protocol extraction mismatch.
- Event from an undeclared Port variant.
- Turn runaway.
- Logical time regression.
- State corruption.
- Arithmetic invariant failure.
- Journal inconsistency.

**Open.** Exact panic, abort, returned-error, faulted-engine, and disarmed-engine policy is unresolved.

### 24.4 Port Infrastructure Faults

Examples:

- Worker panic.
- Broken mailbox.
- Process termination.
- Queue overflow under fault policy.
- Unrecoverable codec or transport failure.

These faults must be observable to environment supervision. Whether they become application Events depends on the Port protocol and failure classification.

### 24.5 Recovery

**Open.** Crash-only restart, in-process fault/disarm, automatic Port restart, reconciliation, and arming policy remain unresolved.

Future recovery must be explicit and exercised regularly. Silent retries, hidden suppression, and implicit state repair are rejected.

---

## 25. Lifecycle And Control

### 25.1 Separation

Runtime-private process shutdown is distinct from domain control such as disable trading, cancel orders, or flatten positions.

### 25.2 Control Port

**Recommended.** Operator and control-plane input enters through a typed Control Port as Events. Application reactions produce ordinary Messages and Commands.

### 25.3 Trading Lifecycle

The v2 lifecycle `Booting → Reconciling → Armed` remains a useful candidate, but v4 does not finalize it.

**Open.** Readiness, reconciliation, arming, disarming, kill switch, operator stop, flattening, and shutdown sequencing require a dedicated design.

### 25.4 Port Shutdown

**Open.** Stop-accepting, command drain, Event drain, Port signal, timeout, join, and process cleanup semantics remain unresolved.

---

## 26. Construction And Ergonomics

### 26.1 Application Construction

Recommended shape:

```rust
fn application(config: StrategyConfig) -> Result<TradingApplication, BuildError> {
    TradingApplication::builder()
        .port::<MarketData>()
        .port::<EquitiesExecution>()
        .port::<OptionsExecution>()
        .component(BarAggregator::new(), |c| {
            c.on_event(BarAggregator::on_trade)
                .produces_message::<Bar>();
        })
        .component(MaCross::new(config.ma), |c| {
            c.on_message(MaCross::on_bar)
                .produces_message::<TargetPosition>();
            c.on_event(MaCross::on_fill);
        })
        .component(OrderPlanner::new(), |c| {
            c.on_message(OrderPlanner::on_target)
                .produces_message::<OrderIntent>();
        })
        .component(RiskPolicy::new(config.risk), |c| {
            c.on_message(RiskPolicy::on_order)
                .produces_message::<ApprovedOrder>()
                .produces_message::<RiskRejection>();
        })
        .component(OrderRouter::new(config.routing), |c| {
            c.on_message(OrderRouter::on_approved)
                .produces_command::<EquitiesExecution>()
                .produces_command::<OptionsExecution>();
        })
        .build()
}
```

Exact syntax is **Open**.

### 26.2 Reusable Components

**Recommended.** Reusable Components implement a registration method and can be added concisely:

```rust
TradingApplication::builder()
    .port::<MarketData>()
    .port::<EquitiesExecution>()
    .add(BarAggregator::new())
    .add(MaCross::new(config.ma))
    .add(OrderRouter::new(config.routing))
    .build()?;
```

### 26.3 Binding

Recommended live shape:

```rust
let environment = LiveEnvironment::builder()
    .bind::<MarketData>(IbkrMarketData::new(...))
    .bind::<EquitiesExecution>(IbkrExecution::new(...))
    .bind::<OptionsExecution>(TastyExecution::new(...))
    .build(&app)?;
```

Recommended simulation shape:

```rust
let environment = SimulationEnvironment::builder(seed)
    .bind::<MarketData>(historical.market_data())
    .bind::<EquitiesExecution>(historical.equities_exchange())
    .bind::<OptionsExecution>(historical.options_exchange())
    .build(&app)?;
```

### 26.4 One Run Pipeline

**Decided.** Application setup and kernel execution are shared:

```rust
fn run<A, E, J>(
    application: A,
    environment: E,
    journal: J,
) -> Result<RunOutcome, RunError> {
    Engine::new(application, environment, journal).run()
}
```

Mode-specific code constructs environment bindings; it does not redefine application wiring.

---

## 27. Complete Conceptual Example: Moving-Average Crossover

### 27.1 Protocol

```rust
pub enum TradingEvent {
    Market(MarketEvent),
    EquitiesExecution(ExecutionEvent),
    OptionsExecution(ExecutionEvent),
}

pub enum TradingMessage {
    Bar(Bar),
    TargetPosition(TargetPosition),
    OrderIntent(OrderIntent),
    RiskApproved(ApprovedOrder),
    RiskRejected(RiskRejection),
}

pub enum TradingCommand {
    EquitiesExecution(ExecutionCommand),
    OptionsExecution(ExecutionCommand),
}
```

### 27.2 Port Specs

```rust
pub struct MarketData;

impl PortSpec for MarketData {
    type Command = MarketCommand;
    type Event = MarketEvent;
}

pub struct EquitiesExecution;

impl PortSpec for EquitiesExecution {
    type Command = ExecutionCommand;
    type Event = ExecutionEvent;
}

pub struct OptionsExecution;

impl PortSpec for OptionsExecution {
    type Command = ExecutionCommand;
    type Event = ExecutionEvent;
}
```

### 27.3 Strategy

```rust
pub struct MaCross {
    instrument: InstrumentId,
    fast: MovingAverage,
    slow: MovingAverage,
    previous: Option<Relation>,
}

impl MaCross {
    fn on_bar(&mut self, ctx: &mut ComponentCtx, bar: &Bar) {
        if bar.instrument != self.instrument {
            return;
        }

        self.fast.push(bar.close);
        self.slow.push(bar.close);

        let Some(relation) = relation(self.fast.value(), self.slow.value()) else {
            return;
        };

        if self.previous.is_some_and(|previous| previous != relation) {
            let target = match relation {
                Relation::FastAbove => Quantity::new(100),
                Relation::FastBelow => Quantity::ZERO,
            };

            ctx.message(TargetPosition {
                instrument: self.instrument,
                target,
            });
        }

        self.previous = Some(relation);
    }
}
```

The strategy emits intent, not a venue-specific external Command.

### 27.4 Deterministic Pipeline

```text
Market Port
  -- Market Event --> BarAggregator
  -- Bar Message --> MaCross
  -- TargetPosition Message --> OrderPlanner
  -- OrderIntent Message --> RiskPolicy
  -- ApprovedOrder Message --> OrderRouter
  -- Execution Command --> EquitiesExecution or OptionsExecution Port
```

### 27.5 Router

```rust
impl OrderRouter {
    fn on_approved(
        &mut self,
        ctx: &mut ComponentCtx,
        order: &ApprovedOrder,
    ) {
        match order.asset_class {
            AssetClass::Equity => {
                ctx.command::<EquitiesExecution>(
                    ExecutionCommand::Submit(order.to_venue_order()),
                );
            }
            AssetClass::Option => {
                ctx.command::<OptionsExecution>(
                    ExecutionCommand::Submit(order.to_venue_order()),
                );
            }
        }
    }
}
```

### 27.6 Application Graph

```text
[MarketData Port]
        │ Market Event
        ▼
[BarAggregator]
        │ Bar Message
        ▼
[MaCross]
        │ TargetPosition Message
        ▼
[OrderPlanner]
        │ OrderIntent Message
        ▼
[RiskPolicy]
        │ RiskApproved Message
        ▼
[OrderRouter]
        ├── Execution Command ──► [EquitiesExecution Port]
        └── Execution Command ──► [OptionsExecution Port]

[EquitiesExecution Port] ── Execution Event ──► [Order/Portfolio Components]
[OptionsExecution Port]  ── Execution Event ──► [Order/Portfolio Components]
```

### 27.7 Live Bindings

```text
MarketData          → IBKR market adapter
EquitiesExecution  → IBKR execution adapter
OptionsExecution   → Tasty execution adapter
```

### 27.8 Backtest Bindings

```text
MarketData          → historical market model
EquitiesExecution  → simulated equities exchange
OptionsExecution   → simulated options exchange
```

The strategy, risk logic, router, Event/Message/Command types, Port Specs, graph, kernel, and causal semantics are identical.

---

## 28. Invariants

| # | Invariant | Enforcement |
|---:|---|---|
| 1 | Components are identical across live, backtest, replay, and DST | Application construction and mode-inaccessible callbacks |
| 2 | The logical application graph is identical across environments | Graph built before binding; bindings only annotate Port nodes |
| 3 | Only Ports emit Events | Typed Port environment API |
| 4 | Only Components emit Messages and Commands | Component context capabilities |
| 5 | Messages never cross Port boundaries | Separate protocol categories and APIs |
| 6 | Commands never dispatch to Components | Typed Command-to-Port routing |
| 7 | Events and Messages are immutable | Shared typed references in callbacks |
| 8 | Every Event variant emitted by a Port has a consumer | Build validation and ingress assertion |
| 9 | Every Message production has a consumer | Build validation |
| 10 | Every Command production has exactly one destination | Build validation and typed Port target |
| 11 | Every runtime production was declared by its callback | Context output enforcement |
| 12 | Actual callback registration is the source of graph edges | Registrar design |
| 13 | The graph is immutable after build | Builder/runtime type separation |
| 14 | One kernel thread mutates deterministic application state | Runtime ownership |
| 15 | No callback overlaps another callback | Turn executor |
| 16 | One Event turn reaches quiescence before the next Event | Kernel loop |
| 17 | Internal Messages process breadth-first in deterministic order | Turn FIFO plus registration order |
| 18 | Message cycles are deterministically bounded | Runtime guard; exact policy open |
| 19 | Every external Event enters through one acceptance site | Kernel API isolation |
| 20 | Live Event order is frozen at acceptance | Central ingress and Event index |
| 21 | `ctx.now()` is frozen for the complete Event turn | Context construction |
| 22 | Domain time remains payload data | Protocol design |
| 23 | Components cannot observe wall clock or mode | Context capability boundary |
| 24 | Components perform no IO or blocking work | Component contract and review/tooling |
| 25 | Port implementation state cannot be borrowed by Components | Port boundary |
| 26 | Per-Port Command submission order is preserved | Environment contract |
| 27 | Queue overflow and Port failure are never silent | Explicit policy and runtime observability |
| 28 | Simulation execution is single-threaded and independent of OS scheduling | Discrete-event scheduler |
| 29 | Simulation randomness comes only from controlled deterministic choices | Simulation environment |
| 30 | Replay injects Events, recomputes Messages, and compares Commands | Replay environment |
| 31 | Replay does not contact live Ports | Passive bindings/effect suppression |
| 32 | Event and trace indices are not business IDs | API isolation |
| 33 | No process-global mutable kernel state exists | Instance ownership |
| 34 | Deterministically relevant collection iteration is stable | Data-structure policy |
| 35 | A simulated venue's market state is owned by the simulated world, not the application graph | Simulation environment structure |

---

## 29. Rejected Designs

### 29.1 One Generic `dyn Message` Category

Rejected because it erases the semantic distinction among external facts, internal derivations, and external effect requests; requires runtime downcasting; weakens replay semantics; and obscures Port direction.

### 29.2 External Actors Inside The Application Graph

Rejected because live and simulated external systems have different internal data needs and execution mechanisms. Ports represent stable contracts without pretending their implementations are identical Components.

### 29.3 Simulated Live No-Op Subscriptions

Rejected. A live venue does not subscribe to application market data merely to mimic a simulated venue. The simulated world owns its venue market model outside the application graph.

### 29.4 Simulated Venue Pulling Through An Uncontrolled Side Channel

Rejected as the primary model because hidden side-channel ordering can introduce lookahead or ambiguity. The simulation environment coordinates market occurrence, venue state, Event emission, and Command delivery explicitly.

### 29.5 Separate Live And Backtest Application Nodes

Rejected. There is one application graph and one kernel. Environment construction differs only in bindings and mechanics.

### 29.6 Mandatory Shared Async Runtime

Rejected for v1. Ordinary live async runtimes are nondeterministic, and simulated Ports do not need async syntax. DST uses synchronous state machines under one deterministic scheduler.

### 29.7 Port-Owned Threads And Channels

Rejected. Port placement and mailbox construction belong to environment runtime machinery. Port implementations satisfy a protocol and receive controlled IO handles or synchronous simulation contexts.

### 29.8 Strategy Produces Venue-Specific IO Directly By Default

Rejected as the default application pattern. Strategies should normally produce intent Messages; deterministic planning, risk, and routing Components produce external Commands.

Direct Commands remain possible for Components whose semantic responsibility is external coordination.

### 29.9 Application-Wide Coarse Production Declarations

Rejected because "the application may produce X" loses the specific callback edge, weakens tracing, and cannot enforce callback-local capabilities.

### 29.10 Runtime Discovery Of Protocol Types

Rejected. Linker registries and open runtime discovery weaken closed-world validation, stable ordering, and schema visibility. v1 uses explicit enums; a future centralized macro may generate them.

### 29.11 Public Scheduler Or Wall Clock

Rejected because Components could make mode-dependent and physically timing-dependent decisions.

### 29.12 Shared Mutable Cache Across Port Threads

Rejected because locks provide mutual exclusion but not causal snapshot consistency. Ports communicate through Events and Commands.

### 29.13 Scheduler Sequence As Business ID

Rejected. Kernel ordering identity and business identity have different stability requirements.

### 29.14 Silent Drops And Hidden Recovery

Rejected. Dropping, retrying, coalescing, restarting, or suppressing work must be explicit, observable, and testable.

---

## 30. Evolution From Earlier Designs

### 30.1 Preserved From v1

- Single-threaded deterministic kernel.
- Synchronous strategy logic.
- Explicit production declarations.
- Startup graph validation.
- No silent drops.
- Isolated external IO.
- Self-documenting flow graph.

### 30.2 Preserved From v2

- Backtests deterministic by construction.
- Live arrival nondeterminism frozen at ingress.
- Replay through the same deterministic logic.
- Typed domain values and checked behavior as a strong recommendation.
- Independent Engine instances for parallel testing.
- Explicit failure classification.
- Simulated latency as logical time, not CPU duration.

### 30.3 Preserved From v3 And Its Addendum

- Narrow callback capabilities.
- Immutable payloads.
- No wall clock or sequence exposure to business code.
- Frozen `now()` during callback processing.
- Domain time separate from acceptance time.
- One acceptance authority.
- Stable source attribution.
- Typed keyed state as a candidate canonical state model.
- Runtime-owned queues and explicit capacity.
- Different live and simulated implementations may have different internals.

### 30.4 Superseded From v1-v3

| Earlier concept | v4 replacement |
|---|---|
| All payloads are generic Messages | Separate Event, Message, and Command categories |
| `Arc<dyn Message>` and `Any` | Closed application enums with typed registration |
| Actors for feeds, venues, and compute | Typed Ports with live and simulated implementations |
| Actor subscriptions inside application graph | Port boundary nodes and typed bindings |
| Actor output re-enters as generic Message | Port output is external Event acceptance |
| Handler `send_at` schedules arbitrary future Messages | Timer/service/execution Command followed by later Event |
| Scheduler heap inside application semantics | Internal Message FIFO plus environment time scheduling |
| Simulated venue subscribes to application market data | Simulation world owns and coordinates venue market model |
| Backtest inline actor versus live threaded actor | `SimPort<P>` synchronous model versus `LivePort<P>` implementation sharing `PortSpec` |
| Type broadcast for all payloads | Event/Message fan-out plus directed Command-to-Port routing |
| Replay ambiguity over actor rerun versus injected output | Replay injects Events once, recomputes Messages, suppresses effects, compares Commands |

### 30.5 Explicit Addendum Changes

The v3 addendum's single acceptance authority remains, narrowed to external Events. Internal Messages no longer pass through external Event acceptance, and Commands are recorded outputs rather than scheduler ingress.

The v3 scheduler sequence model is replaced by:

- Event index for accepted external ordering.
- Internal causal ordinals for deterministic trace order.
- Simulation scheduler ordering owned by the simulation environment.

None is exposed as a business ID.

---

## 31. Finalized v4 Decisions

| Area | Decision |
|---|---|
| Fundamental flow | Ports emit Events; Components emit Messages and Commands |
| Event semantics | External fact accepted and journaled |
| Message semantics | Deterministic internal fact, recomputed on replay |
| Command semantics | Directed request for external effect |
| Application logic | Deterministic synchronous Components |
| External systems | Typed Ports outside the application graph |
| Protocol representation | Closed Event/Message/Command enums for v1 |
| Payload dispatch | No public `dyn Message`, `Any`, or downcasting |
| Graph source | Actual callback registrations and production declarations |
| Graph lifecycle | Validate and freeze before execution |
| Port identity | Application-defined marker types/specs, not kernel hardcoding |
| Port binding | Compile-time interface conformance, build-time completeness |
| Live execution | Single kernel writer plus concurrent Port workers/processes |
| Simulation execution | One OS thread and deterministic discrete-event scheduler |
| Port parity | Same Port Spec, different live/sim implementation allowed |
| Kernel parity | Same acceptance and turn-processing code in every environment |
| Internal ordering | Breadth-first Message FIFO and stable callback order |
| Time | Frozen acceptance time per Event turn; domain time on payload |
| Future scheduling | Command to external-time Port, then later Event |
| Replay | Inject Events, recompute Messages, compare Commands and preferably state |
| Causality | Kernel-managed root Event and descendant operation identities |
| Mode visibility | Forbidden inside Components |
| IO | Forbidden inside Components |
| Global state | No process-global mutable kernel state |
| Drops | No silent drops or hidden recovery |

---

## 32. Open Decisions

### 32.1 State And Projection Model

- Final canonical state representation.
- Public projector/reducer API.
- Projection ordering for Event and Message inputs.
- Component access to shared projections.
- Snapshot and state migration interfaces.

### 32.2 Protocol And Serialization

- Stable variant IDs.
- Durable encoding.
- Schema evolution.
- Unknown variant handling.
- Cross-version replay compatibility.
- Process-to-process wire protocol.

### 32.3 Component Identity And Registration API

- Stable identity for multiple instances of one type.
- Exact builder syntax.
- Exact reusable `Component::register` syntax.
- Future `#[component]` macro.
- Future `kavod::protocol!` macro.

### 32.4 Routing

- v1 variant-level versus keyed routing scope.
- Port instance selection when several instances share a protocol.
- Selector and finite-universe validation.
- Multicast Commands, if ever needed.

### 32.5 Turn Bounds

- Static Message-cycle detection.
- Maximum causal depth.
- Maximum Messages per turn.
- Error behavior on bound violation.

### 32.6 Deterministic IDs And Randomness

- Client order ID facility.
- Request and correlation IDs.
- Component RNG capability, if permitted.
- Seed and substream ownership.

### 32.7 Journal And Outbox

- Durable backend.
- Record format.
- Atomicity boundary.
- Fsync and batching policy.
- Snapshot cadence.
- Command comparison records.
- Causal trace persistence level.

### 32.8 Live Runtime

- Thread/task/process placement defaults.
- Async runtime choice.
- Mailbox implementation.
- Capacity and overflow defaults.
- Fairness.
- Port readiness and health.
- Remote Port proxy protocol.

### 32.9 Simulation Runtime

- Exact scheduler data structure.
- Tie-breaking and schedule exploration policy.
- PRNG and decision tape.
- Fault catalog.
- Workload API.
- Shrinking and minimization.
- Safety/liveness phase API.
- Independent oracle framework.

### 32.10 Lifecycle And Failure

- Boot, reconciliation, and arming model.
- Kill switch and operator control.
- Crash-only versus in-process fault/disarm.
- Port restart and supervision.
- Controlled shutdown and drain.

### 32.11 Replay

- Exact versus semantic Command equality.
- State-hash algorithm.
- Snapshot compatibility.
- Partial replay and seeking.
- Divergence reports.
- Corrupt or incomplete log handling.

### 32.12 Adapter-Level DST

- Whether and when to build a deterministic async runtime.
- Injectable network, timer, storage, DNS, and randomness interfaces.
- Simulated counterparties for production protocol adapters.
- Potential external deterministic-hypervisor testing.

---

## 33. Design Gates

Implementation should proceed through explicit gates rather than filling open questions speculatively.

### Gate A: Protocol And Graph Core

- Closed Event/Message/Command enums.
- Port Spec marker types.
- Typed Component registration.
- Production declarations.
- Graph validation and export.
- No `dyn Message` or user downcasts.

### Gate B: Deterministic Turn Engine

- Single Event acceptance.
- Internal Message FIFO.
- Callback ordering.
- Command collection.
- Causal trace.
- Turn bounds.

### Gate C: Minimal State Model

- Component-private state.
- Decision on canonical projection model.
- Deterministic hashing.
- Initial snapshot shape.

### Gate D: Historical Simulation

- Virtual time.
- Discrete-event scheduler.
- Historical market Port.
- One simulated execution Port.
- Deterministic end-to-end backtest.

### Gate E: Replay

- Event journal.
- Passive Command comparison.
- State-hash comparison.
- Reproducible divergence diagnostics.

### Gate F: DST

- Deterministic choices.
- Fault injection.
- Independent invariants.
- Safety and liveness phases.
- Reproduction artifact.

### Gate G: Live Runtime

- Central ingress.
- Live Port workers.
- Explicit backpressure.
- Durable Event and Command records.
- Lifecycle and fault visibility.

### Gate H: Production Adapters

- Market-data adapter.
- Execution adapter.
- Protocol conformance tests.
- Paper/shadow integration.
- Reconciliation and operational controls.

---

## 34. One-Line Thesis

Kavod v4 is a single-writer deterministic application graph in which typed Ports emit external Events, deterministic Components transform Events and internal Messages into directed Commands, live concurrency is frozen at one Event acceptance boundary, and historical backtesting, replay, and DST reuse the same protocol, graph, Components, kernel, and causal semantics while replacing only Port implementations and environment mechanics.
