# Kavod v6 Port And Graph Implementation Design

> **Status:** Selected MVP semantic direction; exact public Rust syntax remains provisional
> **Authority:** `design_docs/design-v6.md` remains normative
> **Scope:** Logical Port declarations, closed protocols, typed variant descriptors, application graph construction, manifest-local identity, private routing, and Port queue boundaries

## 1. Purpose And Scope

This document selects the implementation direction for Kavod's static logical Ports and application graph.

The central rule is:

> Application code names a Port Event or Port Command with one Port-qualified typed variant descriptor. Port implementations receive and produce the corresponding complete closed sum enums. An Environment binds one implementation to each frozen logical Port but does not define the Port's variant handlers.

This document deliberately does not freeze:

- Exact `Application` builder ownership or borrowing syntax.
- A derive macro, attribute macro, or function-like declaration macro.
- Live worker, thread, channel, or polling traits.
- Simulation model, private action, scheduler, or context traits.
- Environment capacity and configuration syntax.
- Audit codec and schema APIs.
- Concrete queue or type-erasure implementations.

Examples outside the Port and graph boundary are illustrative only. They show required information flow, not selected Environment interfaces.

## 2. Selected MVP Decisions

The MVP selects all of the following:

1. One `PortSpec` marker type denotes exactly one static logical Port.
2. Each logical Port exclusively owns one closed Event enum and one closed Command enum.
3. Complete Event or Command protocol enums are not reused by another logical Port in the MVP. Domain payload types may be shared freely.
4. Event descriptors have the semantic type `EventVariant<P, T>`, where `P` is the source Port and `T` is the leaf payload.
5. Command descriptors have the semantic type `CommandVariant<P, T>`, where `P` is the destination Port and `T` is the leaf payload.
6. Message descriptors have the semantic type `MessageVariant<T>`.
7. Reducers and Components register with descriptors and receive leaf payload references. They do not match the public Port protocol enums.
8. Component output declarations and `ComponentCtx` output methods use the same descriptors.
9. Port implementations receive the complete `P::Commands` enum and own its exhaustive matching.
10. Port implementations stage the complete `P::Events` enum through their bound Port capability.
11. An Environment binding associates one compatible implementation with one logical Port. A binding does not register per-variant Event or Command handlers.
12. One Event candidate queue exists per logical Port. One live Command mailbox exists per logical Port. Variants do not receive separate queues.
13. The MVP requires no application-authored Port, protocol, protocol-version, variant, schema, route, binding, Component, Reducer, or callback IDs.
14. Graph freezing assigns bounded manifest-local references for Ports, routes, and callback registrations. Audit records interpret those references through the frozen `RunStarted` manifest.
15. Exact declaration macro syntax remains illustrative until the manual descriptor primitive is compile-proven.

These decisions preserve the source attribution, destination authority, closed-protocol, static-binding, queue, audit, and fail-stop semantics in `design-v6.md`.

## 3. Minimal Identity Model

### 3.1 No Application-Authored Infrastructure IDs

The MVP does not ask application authors to define values such as:

```rust
port_id = "market-data"
protocol_id = "market-data.events"
protocol_version = 1
variant_id = 3
schema_id = "example.bar.v1"
callback_id = "strategy.on-bar"
```

Those values would primarily support comparison or compatibility across independently built applications. Cross-build replay, restoration, and protocol compatibility are explicit MVP non-goals. They do not justify the correspondence errors and configuration surface of separate public IDs.

Rust type identity is useful while constructing the graph, but `TypeId`, Rust type names, enum discriminants, function addresses, and descriptor addresses are not audit identity.

### 3.2 Frozen Manifest References

Graph freezing assigns finite references such as:

```rust
struct PortRef(u32);
struct RouteRef(u32);
struct CallbackRef(u32);
```

Exact widths and names are deferred. Allocation is checked and never wraps or reuses a value within the frozen manifest.

`RunStarted` records the complete mapping needed to interpret them. Conceptually:

```text
PortRef(0) -> MarketData, first Port source
PortRef(1) -> Execution, second Port source

RouteRef(0) -> PortRef(0), Event, Bar, payload Bar
RouteRef(1) -> PortRef(0), Command, Subscribe, payload Subscription
RouteRef(2) -> PortRef(1), Event, OrderAccepted, payload OrderAccepted
RouteRef(3) -> PortRef(1), Event, Filled, payload Fill
RouteRef(4) -> PortRef(1), Event, OrderRejected, payload OrderRejected
RouteRef(5) -> PortRef(1), Command, Submit, payload Submission
RouteRef(6) -> Engine Event, Ready
RouteRef(7) -> Engine Event, ShutdownRequested
RouteRef(8) -> Message, Signal, payload Signal
RouteRef(9) -> Message, OrderApproved, payload ApprovedOrder

CallbackRef(0) -> Ready -> Bootstrap::on_ready
CallbackRef(1) -> MarketData.Bar -> reduce_bar
CallbackRef(2) -> MarketData.Bar -> Strategy::on_bar
```

Diagnostic names and encoding metadata are recorded in the manifest for inspection, but they are labels and metadata rather than independently authored identities. Reordering declarations or registrations may change manifest-local references in another build. The recorded run identity, strong content identities of the executable and application build inputs, complete frozen manifest, and remaining normative `RunStarted` provenance distinguish the run and its deterministic starting point.

Later records may use compact manifest references:

- `InputAccepted` carries the accepted Event's `RouteRef`.
- Callback records carry `CallbackRef`.
- `MessageProduced` and `PortCommandProduced` carry output `RouteRef` values.
- Port-specific technical evidence carries `PortRef`.
- Handoff evidence uses the produced Command reference required by `design-v6.md`, with its route recoverable from the recorded production and manifest.

Audit segments must remain bound to the run and frozen manifest as required by the normative framing rules. A manifest-local reference has no meaning as a detached cross-build identifier.

### 3.3 Identities That Remain

The Engine still allocates the operational identities required by the normative design:

- Run identity for external correlation.
- Event index for accepted-input order.
- Action and Command ordinals for deterministic production and handoff evidence.
- Audit-record sequence for journal order and framing.
- Simulation schedule ordinal for equal-time ordering.

Application-owned business identities remain ordinary payload data. For example, `client_order_id` is not replaced by any manifest or Engine reference.

The journal framing format still has a format version. That version belongs to the audit storage format, not to each application Port protocol.

## 4. Core Public Semantic Model

The intended semantic relationships can be expressed in ordinary Rust as follows. Exact trait bounds and method names remain provisional.

```rust
pub trait PortSpec: Sized + 'static {
    type Events: EventProtocol<Port = Self>;
    type Commands: CommandProtocol<Port = Self>;
}

pub trait EventProtocol: Sized + Send + 'static {
    type Port: PortSpec<Events = Self>;

    // Private/generated classification and manifest access.
}

pub trait CommandProtocol: Sized + Send + 'static {
    type Port: PortSpec<Commands = Self>;

    // Private/generated classification and manifest access.
}
```

One-sided Ports still own explicit generated uninhabited protocol enums rather than using `()` or sharing one global protocol type. For example, a Port with no Commands may own:

```rust
pub enum MetricsCommands {}

impl CommandProtocol for MetricsCommands {
    type Port = Metrics;
}
```

The public descriptor types are opaque:

```rust
pub struct EventVariant<P: PortSpec, T> {
    // Private representation.
}

pub struct CommandVariant<P: PortSpec, T> {
    // Private representation.
}

pub struct MessageVariant<T> {
    // Private representation.
}
```

An `EventVariant<P, T>` establishes all of the following at one public API boundary:

```text
source logical Port = P
direction = Event
closed protocol = P::Events
one protocol variant
leaf payload type = T
generated checked projection
generated manifest membership
```

A `CommandVariant<P, T>` correspondingly establishes:

```text
destination logical Port = P
direction = Command
closed protocol = P::Commands
one protocol variant
leaf payload type = T
generated injection
generated manifest membership
```

The Port dimension is part of the descriptor. Public graph and output APIs never accept a separate source or destination argument that could disagree with it.

## 5. Port And Protocol Declaration

### 5.1 Illustrative Declaration Syntax

A single declaration should be capable of supplying the Port marker, Event enum, Command enum, and descriptor correspondence from one syntax tree. The following function-like macro is illustrative, not selected syntax:

```rust
kavod::port! {
    pub port MarketData {
        events MarketDataEvent {
            Bar(Bar),
            Correction(Bar),
        }

        commands MarketDataCommand {
            Subscribe(Subscription),
            RequestSnapshot(InstrumentId),
        }
    }
}
```

The important output is semantic rather than syntactic. The declaration produces:

```rust
pub struct MarketData;

pub enum MarketDataEvent {
    Bar(Bar),
    Correction(Bar),
}

pub enum MarketDataCommand {
    Subscribe(Subscription),
    RequestSnapshot(InstrumentId),
}

impl PortSpec for MarketData {
    type Events = MarketDataEvent;
    type Commands = MarketDataCommand;
}
```

It also produces opaque associated descriptor constants:

```rust
impl MarketDataEvent {
    pub const BAR: EventVariant<MarketData, Bar> = /* generated */;
    pub const CORRECTION: EventVariant<MarketData, Bar> = /* generated */;
}

impl MarketDataCommand {
    pub const SUBSCRIBE: CommandVariant<MarketData, Subscription> =
        /* generated */;

    pub const REQUEST_SNAPSHOT: CommandVariant<MarketData, InstrumentId> =
        /* generated */;
}
```

`BAR` and `CORRECTION` intentionally have the same Rust descriptor type. Their private generated variant slots, projection functions, exhaustive classifier arms, and manifest entries distinguish them. Routing never uses the payload `TypeId` alone.

### 5.2 One Port Owns Its Protocol Enums

The direct constant:

```rust
MarketDataEvent::BAR
```

has exactly one source type:

```rust
EventVariant<MarketData, Bar>
```

The MVP therefore does not reuse `MarketDataEvent` as the complete Event protocol of a second logical Port. An application needing `PrimaryFeed` and `BackupFeed` declares two Port protocols and freely reuses their domain payload types and shared domain implementation helpers.

This restriction keeps source identity intrinsic to every final descriptor and avoids generic binding namespaces or a second source argument. Protocol-enum reuse may be reconsidered only for a concrete requirement.

### 5.3 Manual Primitive Before Macro Selection

Before selecting derive or declaration macro syntax, one complete Port must be implemented manually. The manual implementation must demonstrate that stable Rust can express:

- `PortSpec` association with the two enums.
- Opaque Port-qualified descriptor constants.
- Exhaustive variant classification.
- Command leaf injection and Event checked projection.
- Canonical manifest generation.
- Typed registration and output production.
- Typed Port queues and sum-enum boundaries.

The macro is correspondence-error prevention and ergonomics. It is not semantic authority unavailable to an ordinary manual implementation.

## 6. Descriptor Injection And Projection

An illustrative private representation is:

```rust
struct EventVariant<P: PortSpec, T> {
    slot: EventVariantSlot,
    project: for<'a> fn(&'a P::Events) -> Option<&'a T>,
}

struct CommandVariant<P: PortSpec, T> {
    slot: CommandVariantSlot,
    inject: fn(T) -> P::Commands,
}
```

This representation is explanatory, not selected layout. Per-descriptor function pointers are not required if generated exhaustive dispatch provides a better implementation.

For one Event variant, generated logic is conceptually:

```rust
fn project_bar(event: &MarketDataEvent) -> Option<&Bar> {
    match event {
        MarketDataEvent::Bar(bar) => Some(bar),
        _ => None,
    }
}

impl MarketDataEvent {
    pub const BAR: EventVariant<MarketData, Bar> =
        EventVariant::__generated(
            EventVariantSlot(0),
            project_bar,
        );
}
```

Command descriptors perform the opposite conversion because deterministic application code produces a leaf payload while the Port receives the complete Command enum:

```rust
fn inject_subscribe(subscription: Subscription) -> MarketDataCommand {
    MarketDataCommand::Subscribe(subscription)
}

impl MarketDataCommand {
    pub const SUBSCRIBE: CommandVariant<MarketData, Subscription> =
        CommandVariant::__generated(
            CommandVariantSlot(0),
            inject_subscribe,
        );
}
```

Messages are both produced and consumed inside the Kernel, so a Message descriptor supplies both generated injection and checked projection.

The generated protocol also has an exhaustive classifier:

```rust
fn classify(event: &MarketDataEvent) -> EventVariantSlot {
    match event {
        MarketDataEvent::Bar(_) => EventVariantSlot(0),
        MarketDataEvent::Correction(_) => EventVariantSlot(1),
    }
}
```

Protocol-local slots are private generated correspondence keys. They are not application-authored IDs and make no cross-build stability claim. Graph compilation maps each used or declared slot to its frozen `RouteRef`.

A projection returning `None` after graph validation and correct classification is an impossible internal mismatch. It is fatal and never silently skips a callback.

## 7. Application Graph API

### 7.1 Component Event Registration

Preferred registration uses one descriptor argument:

```rust
application.component(Strategy::new(config), |component| {
    component
        .on(MarketDataEvent::BAR, Strategy::on_bar)
        .produces_message(TradingMessage::SIGNAL)
        .produces_command(ExecutionCommand::SUBMIT);
});
```

There is no separate `.events(MarketData)` source selector and no separate `Execution` argument on `produces_command`.

The callback receives the leaf payload:

```rust
impl Strategy {
    fn on_bar(
        &mut self,
        bar: &Bar,
        state: &AppState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        // No MarketDataEvent match is required here.
    }
}
```

From `MarketDataEvent::BAR`, registration statically learns both `P = MarketData` and `T = Bar`.

### 7.2 Reducer Registration

Reducers use the same input descriptors with their restricted signature:

```rust
application.reducers(|reducers| {
    reducers.on(MarketDataEvent::BAR, reduce_bar);
});

fn reduce_bar(
    state: &mut AppState,
    bar: &Bar,
    _ctx: ReducerCtx,
) {
    state.apply_bar(bar);
}
```

Reducers receive no output or shutdown capability.

### 7.3 Callback-Local Output Authority

Output declarations attach to the callback registration that may produce them:

```rust
component
    .on(MarketDataEvent::BAR, Strategy::on_bar)
    .produces_message(TradingMessage::SIGNAL)
    .produces_command(ExecutionCommand::SUBMIT);
```

The callback uses the same descriptors:

```rust
ctx.message(TradingMessage::SIGNAL, signal);
ctx.command(ExecutionCommand::SUBMIT, submission);
```

The wrong leaf payload is a compile error. The descriptor also fixes the Command destination, so the application cannot pair `ExecutionCommand::SUBMIT` with another Port.

Whether output authorization is represented by callback-local dense sets, generated capabilities, or another private structure remains deferred. Undeclared production retains the fatal semantics in `design-v6.md`.

### 7.4 Engine Events And Messages

Engine Events remain built-in typed descriptors:

```rust
component
    .on_engine(Ready, Bootstrap::on_ready)
    .produces_command(MarketDataCommand::SUBSCRIBE);

component
    .on_engine(ShutdownRequested, ShutdownPolicy::on_request)
    .may_shutdown();
```

Application Messages use Port-free leaf descriptors because they never cross a Port boundary:

```rust
component
    .on_message(TradingMessage::SIGNAL, RiskManager::on_signal)
    .produces_message(TradingMessage::ORDER_APPROVED);
```

Exact Message declaration syntax is deferred, but its generated descriptor and manifest rules parallel Port variants without a `P` parameter.

## 8. Executable Graph And Validation

Actual registrations and callback-local declarations remain the executable source of truth:

```text
EventVariant<P, T>   -> Reducer callback
EventVariant<P, T>   -> Component callback
Engine Event         -> Reducer or Component callback
MessageVariant<T>    -> Reducer or Component callback
callback             -> declared MessageVariant<U>
callback             -> declared CommandVariant<Q, U>
callback             -> optional shutdown authority
```

Before `Ready`, graph compilation must at least validate:

- Each declared logical Port appears once in the application source order.
- Each Port has exactly its generated Event and Command protocol pair.
- Every declared Port Event variant has at least one matching Reducer or Component.
- `Ready` and `ShutdownRequested` each have at least one consumer.
- Every declared Message production has a consumer.
- Every Command production names a declared destination Port through its descriptor.
- Every callback-local output and shutdown declaration is internally valid.
- Reducer, Component, callback, and fan-out order are stable in the frozen manifest.
- Every declared Port has exactly one compatible Environment binding.
- No binding exists for an undeclared Port.
- All graph, queue, turn, manifest-reference, and audit bounds are finite and compatible.

Because one Event enum belongs to one Port, every Event variant is intrinsically source-qualified. Validation does not need to combine a neutral protocol descriptor with a separately supplied source.

Binding compatibility proves the logical Port and complete `P::Events` and `P::Commands` types. Kavod does not maintain a second per-variant Port-handler registry.

## 9. Graph Compilation And Private Erasure

The typed builder may erase heterogeneous registrations only after their Port, protocol, variant, and payload relationships have been established.

Conceptually, graph freezing performs:

```text
Port marker and declaration order
    -> PortRef

Port Event or Command descriptor and generated protocol slot,
built-in Engine Event variant, or Message descriptor and generated slot
    -> RouteRef

callback registration and stable registration order
    -> CallbackRef

manifest references
    -> bounded dense tables for runtime dispatch
```

An illustrative erased Event route is:

```rust
struct CompiledEventRoute<S> {
    route_ref: RouteRef,
    port_ref: PortRef,
    expected_type: TypeId,
    reducers: Box<[ErasedReducer<S>]>,
    components: Box<[ErasedComponentCallback<S>]>,
    encoder: ErasedEncoder,
}
```

This is not selected storage syntax. `TypeId` may serve only as a redundant checked process-local witness at the private erasure boundary. Runtime routing uses the compiled route established from the descriptor and classifier; it never selects a route from payload `TypeId` alone.

All downcasts remain checked. An impossible mismatch after validation is Engine-fatal. Kavod does not use unchecked downcasts, `transmute`, or unchecked unreachable assertions for protocol dispatch.

Generated exhaustive dispatch may later replace some erased callback or projector storage if measurement justifies it. That optimization must preserve the same public descriptors, manifest, callback order, audit references, and failure behavior.

## 10. Port Implementation Boundary

### 10.1 Descriptors Stop At The Application Boundary

Descriptors are for deterministic application graph declaration and leaf-payload output production. They are not the interface by which a Port implementation receives its Commands.

The complete flow for a Command is:

```text
application calls
    ctx.command(ExecutionCommand::SUBMIT, submission)

descriptor injects leaf payload
    ExecutionCommand::Submit(submission)

Kernel stages complete Command sum enum in global production order

after TurnComputed synchronization
    Environment transfers complete sum enum to bound Execution Port

Port implementation receives ExecutionCommand
    and owns its exhaustive match
```

The complete Event flow is the inverse:

```text
Port implementation constructs
    ExecutionEvent::Filled(fill)

bound Port capability stages the complete Event sum enum

generated classifier resolves the variant route

after acceptance commit
    generated projection supplies &Fill to Reducers and Components
```

### 10.2 Port Owns Command Matching

Illustrative Port code is:

```rust
fn receive_execution_command(command: ExecutionCommand) {
    match command {
        ExecutionCommand::Submit(submission) => {
            submit_to_external_system(submission);
        }
    }
}
```

The exact containing live worker or simulation model trait is deferred. The selected requirement is only that the bound Port implementation receives `P::Commands`, not a leaf callback registered by the Environment binding.

Adding a Command variant naturally makes exhaustive matches incomplete unless the Port implementation deliberately uses a wildcard. Kavod does not duplicate Rust's enum matching with a binding-time variant handler table.

### 10.3 Port Stages Complete Events

A capability bound to `P` accepts only `P::Events`. Illustrative staging is:

```rust
port_ctx.stage(ExecutionEvent::Filled(fill))?;
```

It is not:

```rust
port_ctx.stage(ExecutionEvent::FILLED, fill)?;
```

The latter descriptor-plus-leaf form belongs on the deterministic application side for declared outputs. At the Port boundary, construction of the complete enum is simpler and makes the closed protocol explicit.

The capability type and queue ownership establish source authority. Candidate payload metadata is not trusted to supply another Port identity.

### 10.4 Bindings Only Bind

An Environment binding conceptually supplies:

```text
logical Port P
one implementation compatible with P::Events and P::Commands
Environment-owned bounded runtime resources and configuration
```

It does not supply:

```text
per-variant Command callbacks
per-variant Event callbacks
application Reducers or Components
callback output declarations
graph routes
Port lifecycle semantics
```

The following is therefore the intended shape:

```rust
environment.bind::<MarketData>(market_data_implementation);
environment.bind::<Execution>(execution_implementation);
```

The method name, ownership, configuration, and implementation trait bounds are illustrative. The semantic point is that no closure such as `endpoint.on_command(...)` appears at binding time.

Live and simulation may expose different implementation mechanics, but both consume the same frozen application graph and bind implementations to the same logical Port types. Neither Environment changes source, destination, protocol, or callback topology.

## 11. Queue Consequences

### 11.1 One Live Event FIFO Per Logical Port

Every Event variant of one Port enters the same bounded Event candidate FIFO:

```text
MarketData Event FIFO
    MarketDataEvent::Bar(...)
    MarketDataEvent::Correction(...)
    MarketDataEvent::Bar(...)
```

There is no Bar queue and Correction queue. The fixed live selector still visits one source per logical Port, not one source per variant.

A typed queue is conceptually:

```rust
struct StagedEvent<P: PortSpec> {
    event: P::Events,
}

struct EventQueue<P: PortSpec> {
    fifo: BoundedFifo<StagedEvent<P>>,
}
```

The generated classifier may run at staging, selection, or another private point that preserves immutable staging, FIFO order, source authority, bounds, and failure semantics. A staged entry may additionally cache its compiled `RouteRef`; exact layout is deferred.

The Port identity need not be copied into each payload. Owning queue and capability already establish `P`.

### 11.2 One Live Command Mailbox Per Logical Port

All Command variants for one destination share that Port's bounded non-evicting mailbox:

```text
Execution Command mailbox
    ExecutionCommand::Submit(...)
    ExecutionCommand::Submit(...)
```

An illustrative envelope is:

```rust
struct CommandEnvelope<P: PortSpec> {
    event_index: EventIndex,
    command_ordinal: CommandOrdinal,
    logical_time: LogicalTime,
    route_ref: RouteRef,
    command: P::Commands,
}
```

Exact metadata visible to the Port implementation is deferred. The mailbox stores the complete immutable Command enum and preserves successful insertion order until worker dequeue or terminal abandonment.

### 11.3 Global Production And Per-Port FIFO

Commands remain staged first in one turn-global production sequence:

```text
0: ExecutionCommand::Submit(A)
1: TimerCommand::Set(B)
2: ExecutionCommand::Submit(C)
```

After `TurnComputed` synchronizes, handoff considers that sequence exactly once in global order:

```text
1. insert Submit(A) into Execution mailbox
2. insert Set(B) into Timer mailbox
3. insert Submit(C) into Execution mailbox
```

The Execution mailbox consequently observes `Submit(A)` before `Submit(C)`. Physical processing across different Port implementations is not globally ordered and remains outside the handoff guarantee.

Simulation may realize the same logical Port boundary synchronously rather than with a physical Command mailbox. Its exact model and scheduler traits are outside this document. It must still receive the complete `P::Commands` enum, stage complete `P::Events` enums, preserve the normative global Command order, and use one addressed scheduled Event queue per logical Port.

## 12. Focused End-To-End Example

The declaration syntax in this section is illustrative. The graph and boundary relationships are selected.

### 12.1 Protocols

```rust
kavod::port! {
    pub port MarketData {
        events MarketDataEvent {
            Bar(Bar),
        }

        commands MarketDataCommand {
            Subscribe(Subscription),
        }
    }
}

kavod::port! {
    pub port Execution {
        events ExecutionEvent {
            OrderAccepted(OrderAccepted),
            Filled(Fill),
            OrderRejected(OrderRejected),
        }

        commands ExecutionCommand {
            Submit(Submission),
        }
    }
}

kavod::messages! {
    pub enum TradingMessage {
        Signal(Signal),
        OrderApproved(ApprovedOrder),
    }
}
```

No declaration contains an application-authored infrastructure ID.

### 12.2 Deterministic Callbacks

```rust
impl Bootstrap {
    fn on_ready(
        &mut self,
        _ready: &Ready,
        _state: &TradingState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        ctx.command(
            MarketDataCommand::SUBSCRIBE,
            Subscription {
                instrument: self.instrument,
            },
        );
    }
}

fn reduce_bar(
    state: &mut TradingState,
    bar: &Bar,
    _ctx: ReducerCtx,
) {
    state.last_close = Some(bar.close);
}

impl Strategy {
    fn on_bar(
        &mut self,
        bar: &Bar,
        state: &TradingState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        if let Some(signal) = self.evaluate(bar, state) {
            ctx.message(TradingMessage::SIGNAL, signal);
        }
    }
}

impl RiskManager {
    fn on_signal(
        &mut self,
        signal: &Signal,
        state: &TradingState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        if let Some(order) = self.approve(signal, state) {
            ctx.message(TradingMessage::ORDER_APPROVED, order);
        }
    }
}

impl OrderManager {
    fn on_order_approved(
        &mut self,
        order: &ApprovedOrder,
        _state: &TradingState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        let submission = self.create_submission(order);
        ctx.command(ExecutionCommand::SUBMIT, submission);
    }
}
```

### 12.3 Graph Construction

```rust
let mut application = Application::builder(TradingState::default());

application.port::<MarketData>();
application.port::<Execution>();

application.reducers(|reducers| {
    reducers.on(MarketDataEvent::BAR, reduce_bar);
    reducers.on(ExecutionEvent::ORDER_ACCEPTED, reduce_order_accepted);
    reducers.on(ExecutionEvent::FILLED, reduce_fill);
    reducers.on(ExecutionEvent::ORDER_REJECTED, reduce_order_rejected);
});

application.component(bootstrap, |component| {
    component
        .on_engine(Ready, Bootstrap::on_ready)
        .produces_command(MarketDataCommand::SUBSCRIBE);
});

application.component(strategy, |component| {
    component
        .on(MarketDataEvent::BAR, Strategy::on_bar)
        .produces_message(TradingMessage::SIGNAL);
});

application.component(risk_manager, |component| {
    component
        .on_message(TradingMessage::SIGNAL, RiskManager::on_signal)
        .produces_message(TradingMessage::ORDER_APPROVED);
});

application.component(order_manager, |component| {
    component
        .on_message(
            TradingMessage::ORDER_APPROVED,
            OrderManager::on_order_approved,
        )
        .produces_command(ExecutionCommand::SUBMIT);
});

application.component(shutdown_policy, |component| {
    component
        .on_engine(
            ShutdownRequested,
            ShutdownPolicy::on_request,
        )
        .may_shutdown();
});

let application = application.build()?;
```

This creates the selected topology:

```text
Ready
    -> Bootstrap
    -> MarketDataCommand::Subscribe

MarketDataEvent::Bar
    -> reduce_bar
    -> Strategy
    -> TradingMessage::Signal

TradingMessage::Signal
    -> RiskManager
    -> TradingMessage::OrderApproved

TradingMessage::OrderApproved
    -> OrderManager
    -> ExecutionCommand::Submit

ExecutionEvent variants
    -> canonical-state Reducers

ShutdownRequested
    -> ShutdownPolicy
    -> declared shutdown authority
```

### 12.4 Port-Side Sum-Enum Handling

The bound Market Data implementation receives:

```rust
fn on_market_data_command(command: MarketDataCommand) {
    match command {
        MarketDataCommand::Subscribe(subscription) => {
            // Port-owned external or simulated behavior.
        }
    }
}
```

It stages:

```rust
port_ctx.stage(MarketDataEvent::Bar(bar))?;
```

The bound Execution implementation receives:

```rust
fn on_execution_command(command: ExecutionCommand) {
    match command {
        ExecutionCommand::Submit(submission) => {
            // Port-owned external or simulated behavior.
        }
    }
}
```

It may stage:

```rust
port_ctx.stage(ExecutionEvent::OrderAccepted(accepted))?;
port_ctx.stage(ExecutionEvent::Filled(fill))?;
port_ctx.stage(ExecutionEvent::OrderRejected(rejection))?;
```

No binding-time handler declaration participates in this code. Exact `port_ctx` types and return values are deferred.

## 13. Failure Rules At This Boundary

| Failure | Treatment |
|---|---|
| Wrong leaf payload passed with a descriptor | Compile error |
| Command descriptor names an undeclared Port | Application build error |
| Missing Event consumer | Application build error |
| Missing or incompatible Port binding | Environment build error |
| Duplicate declaration of one Port marker | Application build error |
| Manifest reference or configured graph bound exhausted before run start | Application or run-start error |
| Generated classifier, descriptor, or manifest disagree | Fatal internal invariant violation |
| Checked projection fails after classification | Fatal internal invariant violation |
| Checked downcast fails after graph validation | Fatal internal invariant violation |
| Port Event queue full | Technical Port failure under normative staging rules |
| Live Port Command mailbox full or disconnected | Failed handoff and Engine-global fatal closure |

Expected external negative results remain ordinary variants of `P::Events` when the Port remains technically trustworthy enough to report them.

## 14. Verification Gates

Before freezing public Rust syntax:

1. Implement one Port, its two enums, and every descriptor manually.
2. Compile-prove `EventVariant<P, T>` registration with a leaf callback.
3. Compile-prove `CommandVariant<P, T>` declaration and production without a separate destination argument.
4. Compile-prove staging accepts `P::Events` and rejects another Port's Event enum.
5. Implement the same Port through generated declaration syntax and compare canonical manifests.
6. Property-test applicable injection, classification, and projection correspondence for every Event, Command, and Message variant.
7. Test the same payload type in two variants of one Port protocol and require distinct routes.
8. Compile-fail wrong payload, wrong direction, wrong Port, and escaping callback-reference cases.
9. Test one Event FIFO and one Command mailbox per Port with several interleaved variants.
10. Test that graph registration order deterministically assigns the same manifest-local references for the same build and configuration.
11. Test that declaration or registration reordering produces a distinct manifest and the corresponding source or callback order, without changing the interpretation of an existing manifest or making a cross-build identity claim.
12. Differentially compare erased dispatch with a direct generated exhaustive-match dispatcher.
13. Inject descriptor, classifier, route, and payload mismatches through test-only seams and require fatal failure before incorrect callback invocation.
14. Keep impossible-state assertions and checked projections active in production.

## 15. Deferred API Decisions

The following remain deliberately open:

1. Derive, attribute, or function-like macro syntax for Port and Message declarations.
2. Exact manual descriptor constructors and protocol manifest traits.
3. Exact `Application` and nested registrar ownership syntax.
4. Exact callback storage, erasure, and invocation representation.
5. Exact payload encoding and manifest schema metadata APIs.
6. Whether typed per-Port queues or bounded arenas avoid per-value boxing initially.
7. Exact live Port implementation, worker, stop, join, and IO context traits.
8. Exact simulation model, private action, scheduling, and context traits.
9. Exact Environment binding and capacity configuration syntax.
10. Exact audit binary representation of manifest references.

These decisions must preserve the selected Port-qualified graph, sum-enum Port boundary, manifest-local identity model, one-queue-per-Port semantics, and every normative authority and failure rule in `design-v6.md`.
