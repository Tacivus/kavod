# Kavod v6 Static-Topology Implementation Research

> **Status:** Research only; neither syntax nor implementation strategy is selected
> **Authority:** `design_docs/design-v6.md` remains normative
> **Related direction:** `design_docs/impl/impl-v6.md` remains the current implementation direction
> **Scope:** Rust macro-generated and Zig 0.16 comptime-generated application topology

## 1. Purpose

This document records two alternative implementations of the same Kavod application:

1. Rust with a compile-time application-topology macro.
2. Zig 0.16 with a comptime application-topology type factory.

Both alternatives separate:

```text
compile-time topology
    protocols
    logical Ports
    Component and Reducer registrations
    callback order
    callback-local output declarations
    source-qualified routes
    manifests and stable identities

runtime values
    AppState
    Component-private state
    live Port implementations
    simulation models
    credentials and endpoints
    capacities and finite bounds
    audit storage
```

The intended benefit is a graph that is structurally complete before runtime values are supplied. The generated application has concrete state storage and exhaustive dispatch rather than a mutable heterogeneous registration builder.

The examples are intentionally substantial enough to expose syntax and implementation costs. They are illustrative APIs, not compilable commitments.

## 2. Shared Example

The example application has two logical Ports:

```text
MarketData
    Command: Subscribe
    Event:   Bar

Execution
    Command: Submit
    Events:  OrderAccepted, Filled, OrderRejected
```

The application has two internal Messages:

```text
Signal
OrderApproved
```

Its deterministic flow is:

```text
Ready
-> Bootstrap produces MarketData.Subscribe

MarketData.Bar
-> Reducer updates canonical last-close state
-> Strategy updates private state and may produce Signal

Signal
-> RiskManager reads canonical position and may produce OrderApproved

OrderApproved
-> OrderManager allocates an application-owned business ID
-> OrderManager produces Execution.Submit

Execution.OrderAccepted
-> Reducer records the active order

Execution.Filled
-> Reducer updates canonical position and clears the active order

Execution.OrderRejected
-> Reducer clears the active order and records the reason

ShutdownRequested
-> ShutdownPolicy requests output-free application shutdown
```

Live mode binds network-backed implementations to both Ports. Simulation binds a historical-feed model and a synchronous exchange model. The application topology and deterministic callback code are identical in both modes.

## 3. Rust Static-Topology Alternative

### 3.1 Protocols

Protocol derives generate opaque typed descriptors, exhaustive injection and projection, and canonical manifests.

```rust
use kavod::prelude::*;

#[derive(Clone, KavodSchema, KavodEncode)]
#[kavod(schema_id = "example.instrument.v1")]
pub struct InstrumentId(pub u32);

#[derive(Clone, Copy, KavodSchema, KavodEncode)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, KavodSchema, KavodEncode)]
#[kavod(schema_id = "example.bar.v1")]
pub struct Bar {
    pub instrument: InstrumentId,
    pub close: i64,
}

#[derive(Clone, KavodSchema, KavodEncode)]
pub struct Subscription {
    pub instrument: InstrumentId,
}

#[derive(Clone, KavodSchema, KavodEncode)]
pub struct Signal {
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: u64,
}

#[derive(Clone, KavodSchema, KavodEncode)]
pub struct ApprovedOrder {
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: u64,
}

#[derive(Clone, KavodSchema, KavodEncode)]
pub struct Submission {
    pub client_order_id: u64,
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: u64,
}

#[derive(Clone, KavodSchema, KavodEncode)]
pub struct OrderAccepted {
    pub client_order_id: u64,
}

#[derive(Clone, KavodSchema, KavodEncode)]
pub struct Fill {
    pub client_order_id: u64,
    pub instrument: InstrumentId,
    pub side: Side,
    pub quantity: u64,
    pub price: i64,
}

#[derive(Clone, KavodSchema, KavodEncode)]
pub struct OrderRejected {
    pub client_order_id: u64,
    pub reason: String,
}

#[derive(KavodEventProtocol)]
#[kavod(
    protocol_id = "example.market-data.events",
    version = 1,
)]
pub enum MarketDataEvent {
    #[kavod(id = 1)]
    Bar(Bar),
}

#[derive(KavodCommandProtocol)]
#[kavod(
    protocol_id = "example.market-data.commands",
    version = 1,
)]
pub enum MarketDataCommand {
    #[kavod(id = 1)]
    Subscribe(Subscription),
}

#[derive(KavodPort)]
#[kavod(
    port_id = "market-data",
    events = MarketDataEvent,
    commands = MarketDataCommand,
)]
pub struct MarketData;

#[derive(KavodEventProtocol)]
#[kavod(
    protocol_id = "example.execution.events",
    version = 1,
)]
pub enum ExecutionEvent {
    #[kavod(id = 1)]
    OrderAccepted(OrderAccepted),

    #[kavod(id = 2)]
    Filled(Fill),

    #[kavod(id = 3)]
    OrderRejected(OrderRejected),
}

#[derive(KavodCommandProtocol)]
#[kavod(
    protocol_id = "example.execution.commands",
    version = 1,
)]
pub enum ExecutionCommand {
    #[kavod(id = 1)]
    Submit(Submission),
}

#[derive(KavodPort)]
#[kavod(
    port_id = "execution",
    events = ExecutionEvent,
    commands = ExecutionCommand,
)]
pub struct Execution;

#[derive(KavodMessageProtocol)]
#[kavod(
    protocol_id = "example.trading.messages",
    version = 1,
)]
pub enum TradingMessage {
    #[kavod(id = 1)]
    Signal(Signal),

    #[kavod(id = 2)]
    OrderApproved(ApprovedOrder),
}
```

The generated public descriptor constants include:

```rust
MarketDataEvent::BAR
MarketDataCommand::SUBSCRIBE
ExecutionEvent::ORDER_ACCEPTED
ExecutionEvent::FILLED
ExecutionEvent::ORDER_REJECTED
ExecutionCommand::SUBMIT
TradingMessage::SIGNAL
TradingMessage::ORDER_APPROVED
```

### 3.2 State And Callbacks

```rust
pub struct TradingState {
    pub last_close: Option<i64>,
    pub position: i64,
    pub active_order: Option<u64>,
    pub last_rejection: Option<String>,
}

impl TradingState {
    pub fn new() -> Self {
        Self {
            last_close: None,
            position: 0,
            active_order: None,
            last_rejection: None,
        }
    }
}

pub struct Bootstrap {
    instrument: InstrumentId,
}

impl Bootstrap {
    fn on_ready(
        &mut self,
        _ready: &Ready,
        _state: &TradingState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        ctx.command(
            MarketData,
            MarketDataCommand::SUBSCRIBE,
            Subscription {
                instrument: self.instrument.clone(),
            },
        );
    }
}

pub struct Strategy {
    previous_close: Option<i64>,
    order_quantity: u64,
}

impl Strategy {
    fn on_bar(
        &mut self,
        bar: &Bar,
        state: &TradingState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        let Some(previous_close) = self.previous_close.replace(bar.close)
        else {
            return;
        };

        if state.active_order.is_some() {
            return;
        }

        let side = if bar.close > previous_close {
            Side::Buy
        } else if bar.close < previous_close {
            Side::Sell
        } else {
            return;
        };

        ctx.message(
            TradingMessage::SIGNAL,
            Signal {
                instrument: bar.instrument.clone(),
                side,
                quantity: self.order_quantity,
            },
        );
    }
}

pub struct RiskManager {
    max_absolute_position: i64,
}

impl RiskManager {
    fn on_signal(
        &mut self,
        signal: &Signal,
        state: &TradingState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        let Ok(quantity) = i64::try_from(signal.quantity) else {
            return;
        };

        let signed_quantity = match signal.side {
            Side::Buy => quantity,
            Side::Sell => -quantity,
        };

        let Some(projected_position) =
            state.position.checked_add(signed_quantity)
        else {
            return;
        };

        if projected_position.abs() > self.max_absolute_position {
            return;
        }

        ctx.message(
            TradingMessage::ORDER_APPROVED,
            ApprovedOrder {
                instrument: signal.instrument.clone(),
                side: signal.side,
                quantity: signal.quantity,
            },
        );
    }
}

pub struct OrderManager {
    next_client_order_id: u64,
}

impl OrderManager {
    fn on_approved(
        &mut self,
        approved: &ApprovedOrder,
        _state: &TradingState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        let Some(next_id) = self.next_client_order_id.checked_add(1)
        else {
            // Application policy: stop creating orders when its business-ID
            // domain is exhausted. Kavod does not supply business identity.
            return;
        };

        let client_order_id = self.next_client_order_id;
        self.next_client_order_id = next_id;

        ctx.command(
            Execution,
            ExecutionCommand::SUBMIT,
            Submission {
                client_order_id,
                instrument: approved.instrument.clone(),
                side: approved.side,
                quantity: approved.quantity,
            },
        );
    }
}

pub struct ShutdownPolicy;

impl ShutdownPolicy {
    fn on_request(
        &mut self,
        _request: &ShutdownRequested,
        _state: &TradingState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        ctx.shutdown();
    }
}

fn reduce_bar(
    state: &mut TradingState,
    bar: &Bar,
    _ctx: ReducerCtx,
) {
    state.last_close = Some(bar.close);
}

fn reduce_order_accepted(
    state: &mut TradingState,
    accepted: &OrderAccepted,
    _ctx: ReducerCtx,
) {
    state.active_order = Some(accepted.client_order_id);
    state.last_rejection = None;
}

fn reduce_fill(
    state: &mut TradingState,
    fill: &Fill,
    _ctx: ReducerCtx,
) {
    let quantity = i64::try_from(fill.quantity)
        .expect("validated position domain");

    let signed_quantity = match fill.side {
        Side::Buy => quantity,
        Side::Sell => -quantity,
    };

    state.position = state
        .position
        .checked_add(signed_quantity)
        .expect("validated position domain");

    if state.active_order == Some(fill.client_order_id) {
        state.active_order = None;
    }
}

fn reduce_order_rejected(
    state: &mut TradingState,
    rejection: &OrderRejected,
    _ctx: ReducerCtx,
) {
    state.active_order = None;
    state.last_rejection = Some(rejection.reason.clone());
}
```

Technical Kavod context methods still return `()`. Application-owned business-ID exhaustion above is a domain policy, not arbitrary Component authority to make the Engine fatal.

### 3.3 Centralized Application Topology

```rust
kavod::application! {
    pub application TradingApp {
        state: TradingState;
        messages: TradingMessage;

        ports {
            market_data: MarketData;
            execution: Execution;
        }

        reducers {
            callback "state.market-data.bar"
                on event(market_data, MarketDataEvent::BAR)
                => reduce_bar;

            callback "state.execution.accepted"
                on event(
                    execution,
                    ExecutionEvent::ORDER_ACCEPTED,
                )
                => reduce_order_accepted;

            callback "state.execution.filled"
                on event(execution, ExecutionEvent::FILLED)
                => reduce_fill;

            callback "state.execution.rejected"
                on event(
                    execution,
                    ExecutionEvent::ORDER_REJECTED,
                )
                => reduce_order_rejected;
        }

        components {
            component bootstrap: Bootstrap {
                callback "bootstrap.ready"
                    on engine(Ready)
                    => Bootstrap::on_ready
                    may {
                        command(
                            market_data,
                            MarketDataCommand::SUBSCRIBE,
                        );
                    }
            }

            component strategy: Strategy {
                callback "strategy.on-bar"
                    on event(
                        market_data,
                        MarketDataEvent::BAR,
                    )
                    => Strategy::on_bar
                    may {
                        message(TradingMessage::SIGNAL);
                    }
            }

            component risk: RiskManager {
                callback "risk.on-signal"
                    on message(TradingMessage::SIGNAL)
                    => RiskManager::on_signal
                    may {
                        message(
                            TradingMessage::ORDER_APPROVED,
                        );
                    }
            }

            component orders: OrderManager {
                callback "orders.on-approved"
                    on message(
                        TradingMessage::ORDER_APPROVED,
                    )
                    => OrderManager::on_approved
                    may {
                        command(
                            execution,
                            ExecutionCommand::SUBMIT,
                        );
                    }
            }

            component shutdown: ShutdownPolicy {
                callback "shutdown.on-request"
                    on engine(ShutdownRequested)
                    => ShutdownPolicy::on_request
                    may {
                        shutdown;
                    }
            }
        }
    }
}
```

The graph is centralized and statically complete. Callback bodies remain ordinary Rust functions. Callback-local output authority is generated as immutable metadata and checked by `ComponentCtx` at runtime. Payload type, direction, source protocol, callback signature, and runtime-state type are compile-time checked.

An optional future design could generate callback-specific context marker types, but that would couple callback signatures to generated topology and is not assumed here.

### 3.4 Conceptual Generated Code

The macro conceptually generates:

```rust
pub struct TradingApp;

pub struct TradingRuntime {
    state: TradingState,
    bootstrap: Bootstrap,
    strategy: Strategy,
    risk: RiskManager,
    orders: OrderManager,
    shutdown: ShutdownPolicy,
}

pub enum TradingInput {
    Engine(EngineEvent),
    MarketData(MarketDataEvent),
    Execution(ExecutionEvent),
}

pub enum TradingCommand {
    MarketData(MarketDataCommand),
    Execution(ExecutionCommand),
}

impl StaticApplication for TradingApp {
    type State = TradingState;
    type Runtime = TradingRuntime;
    type Input = TradingInput;
    type Message = TradingMessage;
    type Command = TradingCommand;

    const MANIFEST: &'static ApplicationManifest =
        &GENERATED_TRADING_MANIFEST;

    fn dispatch_input(
        runtime: &mut TradingRuntime,
        input: &TradingInput,
        host: &mut DispatchHost<'_, Self>,
    ) {
        match input {
            TradingInput::MarketData(MarketDataEvent::Bar(bar)) => {
                host.reducer(REDUCE_BAR, |ctx| {
                    reduce_bar(&mut runtime.state, bar, ctx);
                });

                host.component(STRATEGY_ON_BAR, |ctx| {
                    Strategy::on_bar(
                        &mut runtime.strategy,
                        bar,
                        &runtime.state,
                        ctx,
                    );
                });
            }

            TradingInput::Execution(
                ExecutionEvent::OrderAccepted(accepted),
            ) => {
                host.reducer(REDUCE_ORDER_ACCEPTED, |ctx| {
                    reduce_order_accepted(
                        &mut runtime.state,
                        accepted,
                        ctx,
                    );
                });
            }

            TradingInput::Execution(ExecutionEvent::Filled(fill)) => {
                host.reducer(REDUCE_FILL, |ctx| {
                    reduce_fill(&mut runtime.state, fill, ctx);
                });
            }

            TradingInput::Execution(
                ExecutionEvent::OrderRejected(rejection),
            ) => {
                host.reducer(REDUCE_ORDER_REJECTED, |ctx| {
                    reduce_order_rejected(
                        &mut runtime.state,
                        rejection,
                        ctx,
                    );
                });
            }

            TradingInput::Engine(EngineEvent::Ready(ready)) => {
                host.component(BOOTSTRAP_READY, |ctx| {
                    Bootstrap::on_ready(
                        &mut runtime.bootstrap,
                        ready,
                        &runtime.state,
                        ctx,
                    );
                });
            }

            TradingInput::Engine(
                EngineEvent::ShutdownRequested(request),
            ) => {
                host.component(SHUTDOWN_ON_REQUEST, |ctx| {
                    ShutdownPolicy::on_request(
                        &mut runtime.shutdown,
                        request,
                        &runtime.state,
                        ctx,
                    );
                });
            }
        }
    }
}
```

Message dispatch is generated through a corresponding exhaustive `match`. The generated code invokes ordinary Engine-owned `DispatchHost` methods so audit, gate admission, bounds, context inertness, and fatal handling remain ordinary library logic rather than macro logic.

### 3.5 Runtime Application Initialization

```rust
fn build_application(
    instrument: InstrumentId,
) -> Result<trading_app::Application, ApplicationBuildError> {
    TradingApp::instantiate(TradingAppInit {
        state: TradingState::new(),

        components: TradingComponents {
            bootstrap: Bootstrap {
                instrument,
            },

            strategy: Strategy {
                previous_close: None,
                order_quantity: 10,
            },

            risk: RiskManager {
                max_absolute_position: 100,
            },

            orders: OrderManager {
                next_client_order_id: 1,
            },

            shutdown: ShutdownPolicy,
        },
    })
}
```

### 3.6 Live Port Implementations

```rust
pub struct LiveMarketData {
    socket: MarketDataSocket,
}

impl LivePort<MarketData> for LiveMarketData {
    fn run(
        &mut self,
        mut io: LivePortIo<'_, MarketData>,
    ) -> Result<(), PortFailure> {
        loop {
            match io.next()? {
                LivePortAction::Command(command) => match command {
                    MarketDataCommand::Subscribe(subscription) => {
                        self.socket.subscribe(
                            &subscription.instrument,
                        )?;
                    }
                },

                LivePortAction::ExternalReady => {
                    let frame = self.socket.read_frame()?;
                    let bar = decode_bar(frame)?;
                    io.stage(MarketDataEvent::BAR, bar)?;
                }

                LivePortAction::Closing => return Ok(()),
            }
        }
    }
}

pub struct LiveExecution {
    broker: BrokerConnection,
}

impl LivePort<Execution> for LiveExecution {
    fn run(
        &mut self,
        mut io: LivePortIo<'_, Execution>,
    ) -> Result<(), PortFailure> {
        loop {
            match io.next()? {
                LivePortAction::Command(command) => match command {
                    ExecutionCommand::Submit(submission) => {
                        self.broker.submit(submission)?;
                    }
                },

                LivePortAction::ExternalReady => {
                    match self.broker.read_update()? {
                        BrokerUpdate::Accepted(value) => {
                            io.stage(
                                ExecutionEvent::ORDER_ACCEPTED,
                                value,
                            )?;
                        }
                        BrokerUpdate::Filled(value) => {
                            io.stage(
                                ExecutionEvent::FILLED,
                                value,
                            )?;
                        }
                        BrokerUpdate::Rejected(value) => {
                            io.stage(
                                ExecutionEvent::ORDER_REJECTED,
                                value,
                            )?;
                        }
                    }
                }

                LivePortAction::Closing => return Ok(()),
            }
        }
    }
}
```

### 3.7 Running Live

The application macro generates a binding struct containing exactly one typed field per logical Port.

```rust
fn run_live(
    config: LiveTradingConfig,
) -> Result<RunOutcome, RunError> {
    let application = build_application(config.instrument)?;

    let environment =
        LiveEnvironment::<TradingApp>::build(
            &application,
            TradingLiveBindings {
                market_data: LiveMarketData {
                    socket: MarketDataSocket::connect(
                        config.market_data_endpoint,
                        config.market_data_credentials,
                    )?,
                },

                execution: LiveExecution {
                    broker: BrokerConnection::connect(
                        config.execution_endpoint,
                        config.execution_credentials,
                    )?,
                },
            },
            LiveEnvironmentConfig {
                event_queue_capacity_per_port: 1_024,
                command_mailbox_capacity_per_port: 256,
            },
        )?;

    let audit = FileAuditJournal::create(
        config.audit_path,
        AuditConfig {
            max_record_bytes: 256 * 1024,
            max_turn_bytes: 4 * 1024 * 1024,
            terminal_reserve_bytes: 1024 * 1024,
            synchronization: Synchronization::DataAndMetadata,
        },
    )?;

    let engine = Engine::build(
        application,
        environment,
        audit,
        EngineConfig {
            max_callbacks_per_turn: 10_000,
            max_messages_per_turn: 1_000,
            max_commands_per_turn: 1_000,
            max_actions_per_turn: 20_000,
        },
    )?;

    engine.run()
}
```

### 3.8 Simulation Models

```rust
pub struct TimedBar {
    pub time: VirtualTime,
    pub bar: Bar,
}

pub struct HistoricalFeed {
    bars: Vec<TimedBar>,
}

#[kavod::simulation_port(MarketData)]
impl HistoricalFeed {
    #[on_command(MarketDataCommand::SUBSCRIBE)]
    fn subscribe(
        &mut self,
        subscription: &Subscription,
        ctx: &mut SimulationPortCtx<'_, MarketData>,
    ) {
        for timed in &self.bars {
            if timed.bar.instrument.0 != subscription.instrument.0 {
                continue;
            }

            ctx.stage_at(
                timed.time,
                MarketDataEvent::BAR,
                timed.bar.clone(),
            );
        }
    }
}

pub struct SimulatedExchange {
    acceptance_latency: VirtualDuration,
    fill_latency: VirtualDuration,
    fill_price: i64,
}

#[kavod::simulation_port(Execution)]
impl SimulatedExchange {
    #[on_command(ExecutionCommand::SUBMIT)]
    fn submit(
        &mut self,
        submission: &Submission,
        ctx: &mut SimulationPortCtx<'_, Execution>,
    ) {
        ctx.stage_after(
            self.acceptance_latency,
            ExecutionEvent::ORDER_ACCEPTED,
            OrderAccepted {
                client_order_id: submission.client_order_id,
            },
        );

        ctx.stage_after(
            self.acceptance_latency + self.fill_latency,
            ExecutionEvent::FILLED,
            Fill {
                client_order_id: submission.client_order_id,
                instrument: submission.instrument.clone(),
                side: submission.side,
                quantity: submission.quantity,
                price: self.fill_price,
            },
        );
    }
}
```

### 3.9 Running Simulation

```rust
fn run_simulation(
    config: SimulationTradingConfig,
) -> Result<RunOutcome, RunError> {
    let application = build_application(config.instrument)?;

    let environment =
        SimulationEnvironment::<TradingApp>::build(
            &application,
            TradingSimulationBindings {
                market_data: HistoricalFeed {
                    bars: config.bars,
                },

                execution: SimulatedExchange {
                    acceptance_latency:
                        VirtualDuration::milliseconds(1),
                    fill_latency:
                        VirtualDuration::milliseconds(2),
                    fill_price: config.fill_price,
                },
            },
            SimulationConfig {
                start_time: VirtualTime::ZERO,
                horizon: Some(config.end_time),
                max_scheduled_actions: 100_000,
                completion: SimulationCompletion::SourcesExhausted,
            },
        )?;

    let audit = FileAuditJournal::create(
        config.audit_path,
        AuditConfig {
            max_record_bytes: 256 * 1024,
            max_turn_bytes: 4 * 1024 * 1024,
            terminal_reserve_bytes: 1024 * 1024,
            synchronization: Synchronization::DataAndMetadata,
        },
    )?;

    let engine = Engine::build(
        application,
        environment,
        audit,
        EngineConfig {
            max_callbacks_per_turn: 10_000,
            max_messages_per_turn: 1_000,
            max_commands_per_turn: 1_000,
            max_actions_per_turn: 20_000,
        },
    )?;

    engine.run()
}
```

### 3.10 Rust Implementation Strategy

The Rust implementation is divided into ordinary library contracts and code generation.

The ordinary library defines:

```text
PortSpec and protocol descriptor contracts
StaticApplication
DispatchHost
ReducerCtx and ComponentCtx
typed live staging and Command mailboxes
typed simulation endpoints
Engine sequencing and gate admission
audit encoding and synchronization
finite-bound enforcement
terminal coordination
```

The protocol derives generate:

```text
typed variant descriptors
injection and projection
canonical protocol manifests
stable protocol and variant metadata
payload schema references
audit encoding adapters
```

The application macro performs syntax-level analysis and builds an internal topology IR containing:

```text
logical Ports
Component state slots
Reducer and Component callbacks
source-qualified input routes
callback-local output declarations
stable callback IDs
registration and fan-out order
```

The application macro generates:

```text
concrete runtime-state structs
source-qualified input and Command envelopes
canonical application manifests
exhaustive input and Message dispatch
typed live and simulation binding structs
ordinary trait bounds and callback signature assertions
```

The macro does not attempt to resolve arbitrary Rust types. It emits ordinary Rust expressions, assignments, and trait bounds that cause `rustc` to validate callback and protocol compatibility.

The macro must not implement Engine semantics. Generated dispatch calls ordinary `DispatchHost` methods so callback admission, audit records, output staging, bounds, closure races, and fatal behavior remain testable without expanding or executing macro logic.

The macro should be implemented as parse, validate, and generate phases over one explicit IR. Verification should include expansion snapshots, compile-pass tests, compile-fail tests, direct-versus-generated differential dispatch, and canonical manifest tests.

### 3.11 Rust Costs And Limitations

1. The centralized topology is a custom language embedded in a macro invocation.
2. Macro expansion and generated-name diagnostics can be difficult to understand.
3. A procedural macro cannot semantically inspect arbitrary external Rust types.
4. The topology declaration duplicates information already visible in callback signatures.
5. Callback-local output authority remains runtime checked unless generated context types are introduced.
6. The macro becomes a major public API and compatibility obligation.
7. Direct generated dispatch increases application-specific monomorphized code.
8. Graph changes require recompiling application-specific generated code.

The principal benefit is retaining Rust ownership and invocation-scoped borrow checking while removing application payload, callback, and Component-state erasure.

## 4. Zig 0.16 Comptime Alternative

### 4.1 Protocols

Zig protocol factories consume comptime schema values and return namespace types. Each namespace contains a generated tagged union as `Value`, a canonical manifest, and a generated descriptor table as `variants`.

```zig
const std = @import("std");
const kavod = @import("kavod");

const InstrumentId = u32;

const Side = enum(u8) {
    buy = 1,
    sell = 2,
};

const RejectReason = enum(u8) {
    risk = 1,
    venue = 2,
};

const Bar = struct {
    pub const kavod_schema_id = "example.bar.v1";

    instrument: InstrumentId,
    close: i64,
};

const Subscription = struct {
    pub const kavod_schema_id = "example.subscription.v1";

    instrument: InstrumentId,
};

const Signal = struct {
    pub const kavod_schema_id = "example.signal.v1";

    instrument: InstrumentId,
    side: Side,
    quantity: u64,
};

const ApprovedOrder = struct {
    pub const kavod_schema_id = "example.approved-order.v1";

    instrument: InstrumentId,
    side: Side,
    quantity: u64,
};

const Submission = struct {
    pub const kavod_schema_id = "example.submission.v1";

    client_order_id: u64,
    instrument: InstrumentId,
    side: Side,
    quantity: u64,
};

const OrderAccepted = struct {
    pub const kavod_schema_id = "example.order-accepted.v1";

    client_order_id: u64,
};

const Fill = struct {
    pub const kavod_schema_id = "example.fill.v1";

    client_order_id: u64,
    instrument: InstrumentId,
    side: Side,
    quantity: u64,
    price: i64,
};

const OrderRejected = struct {
    pub const kavod_schema_id = "example.order-rejected.v1";

    client_order_id: u64,
    reason: RejectReason,
};

const MarketDataEvents = kavod.EventProtocol(.{
    .id = "example.market-data.events",
    .version = 1,
    .variants = .{
        .bar = kavod.variant(1, Bar),
    },
});

const MarketDataCommands = kavod.CommandProtocol(.{
    .id = "example.market-data.commands",
    .version = 1,
    .variants = .{
        .subscribe = kavod.variant(1, Subscription),
    },
});

const MarketData = kavod.Port(.{
    .id = "market-data",
    .events = MarketDataEvents,
    .commands = MarketDataCommands,
});

const ExecutionEvents = kavod.EventProtocol(.{
    .id = "example.execution.events",
    .version = 1,
    .variants = .{
        .order_accepted = kavod.variant(1, OrderAccepted),
        .filled = kavod.variant(2, Fill),
        .order_rejected = kavod.variant(3, OrderRejected),
    },
});

const ExecutionCommands = kavod.CommandProtocol(.{
    .id = "example.execution.commands",
    .version = 1,
    .variants = .{
        .submit = kavod.variant(1, Submission),
    },
});

const Execution = kavod.Port(.{
    .id = "execution",
    .events = ExecutionEvents,
    .commands = ExecutionCommands,
});

const TradingMessages = kavod.MessageProtocol(.{
    .id = "example.trading.messages",
    .version = 1,
    .variants = .{
        .signal = kavod.variant(1, Signal),
        .order_approved = kavod.variant(2, ApprovedOrder),
    },
});
```

Conceptually, `MarketDataEvents.Value` is:

```zig
const MarketDataEvent = union(enum(u16)) {
    bar: Bar = 1,
};
```

Zig 0.16 can generate the fields and explicit enum values with `@Enum` and `@Union`. It cannot synthesize arbitrary attached declarations such as `MarketDataEvent.BAR`, so generated descriptors are accessed through `MarketDataEvents.variants.bar`.

### 4.2 State And Callbacks

```zig
const TradingState = struct {
    last_close: ?i64 = null,
    position: i64 = 0,
    active_order: ?u64 = null,
    last_rejection: ?RejectReason = null,
};

const Bootstrap = struct {
    instrument: InstrumentId,

    fn onReady(
        self: *Bootstrap,
        _: *const kavod.Ready,
        _: *const TradingState,
        ctx: anytype,
    ) void {
        ctx.command(
            .market_data,
            MarketDataCommands.variants.subscribe,
            Subscription{ .instrument = self.instrument },
        );
    }
};

const Strategy = struct {
    previous_close: ?i64 = null,
    order_quantity: u64,

    fn onBar(
        self: *Strategy,
        bar: *const Bar,
        state: *const TradingState,
        ctx: anytype,
    ) void {
        const previous_close = self.previous_close orelse {
            self.previous_close = bar.close;
            return;
        };

        self.previous_close = bar.close;

        if (state.active_order != null) return;

        const side: Side = if (bar.close > previous_close)
            .buy
        else if (bar.close < previous_close)
            .sell
        else
            return;

        ctx.message(
            TradingMessages.variants.signal,
            Signal{
                .instrument = bar.instrument,
                .side = side,
                .quantity = self.order_quantity,
            },
        );
    }
};

const RiskManager = struct {
    max_absolute_position: i64,

    fn onSignal(
        self: *RiskManager,
        signal: *const Signal,
        state: *const TradingState,
        ctx: anytype,
    ) void {
        const quantity: i64 = @intCast(signal.quantity);
        const signed_quantity = switch (signal.side) {
            .buy => quantity,
            .sell => -quantity,
        };

        const projected_position = std.math.add(
            i64,
            state.position,
            signed_quantity,
        ) catch return;

        if (projected_position > self.max_absolute_position) return;
        if (projected_position < -self.max_absolute_position) return;

        ctx.message(
            TradingMessages.variants.order_approved,
            ApprovedOrder{
                .instrument = signal.instrument,
                .side = signal.side,
                .quantity = signal.quantity,
            },
        );
    }
};

const OrderManager = struct {
    next_client_order_id: u64,

    fn onApproved(
        self: *OrderManager,
        approved: *const ApprovedOrder,
        _: *const TradingState,
        ctx: anytype,
    ) void {
        const client_order_id = self.next_client_order_id;

        self.next_client_order_id = std.math.add(
            u64,
            client_order_id,
            1,
        ) catch return;

        ctx.command(
            .execution,
            ExecutionCommands.variants.submit,
            Submission{
                .client_order_id = client_order_id,
                .instrument = approved.instrument,
                .side = approved.side,
                .quantity = approved.quantity,
            },
        );
    }
};

const ShutdownPolicy = struct {
    fn onRequest(
        _: *ShutdownPolicy,
        _: *const kavod.ShutdownRequested,
        _: *const TradingState,
        ctx: anytype,
    ) void {
        ctx.shutdown();
    }
};

fn reduceBar(
    state: *TradingState,
    bar: *const Bar,
    _: kavod.ReducerCtx,
) void {
    state.last_close = bar.close;
}

fn reduceOrderAccepted(
    state: *TradingState,
    accepted: *const OrderAccepted,
    _: kavod.ReducerCtx,
) void {
    state.active_order = accepted.client_order_id;
    state.last_rejection = null;
}

fn reduceFill(
    state: *TradingState,
    fill: *const Fill,
    _: kavod.ReducerCtx,
) void {
    const quantity: i64 = @intCast(fill.quantity);
    const signed_quantity = switch (fill.side) {
        .buy => quantity,
        .sell => -quantity,
    };

    state.position += signed_quantity;

    if (state.active_order == fill.client_order_id) {
        state.active_order = null;
    }
}

fn reduceOrderRejected(
    state: *TradingState,
    rejection: *const OrderRejected,
    _: kavod.ReducerCtx,
) void {
    state.active_order = null;
    state.last_rejection = rejection.reason;
}
```

`ctx: anytype` permits the application factory to specialize each callback with a context type generated from that callback's output declaration. It does not provide Rust-style lifetime or non-escape checking.

### 4.3 Comptime Application Topology

```zig
const TradingApp = kavod.Application(.{
    .State = TradingState,
    .Messages = TradingMessages,

    .ports = .{
        .market_data = MarketData,
        .execution = Execution,
    },

    .reducers = .{
        kavod.reducer(.{
            .id = "state.market-data.bar",
            .on = kavod.event(
                .market_data,
                MarketDataEvents.variants.bar,
            ),
            .call = reduceBar,
        }),

        kavod.reducer(.{
            .id = "state.execution.accepted",
            .on = kavod.event(
                .execution,
                ExecutionEvents.variants.order_accepted,
            ),
            .call = reduceOrderAccepted,
        }),

        kavod.reducer(.{
            .id = "state.execution.filled",
            .on = kavod.event(
                .execution,
                ExecutionEvents.variants.filled,
            ),
            .call = reduceFill,
        }),

        kavod.reducer(.{
            .id = "state.execution.rejected",
            .on = kavod.event(
                .execution,
                ExecutionEvents.variants.order_rejected,
            ),
            .call = reduceOrderRejected,
        }),
    },

    .components = .{
        .bootstrap = kavod.component(Bootstrap, .{
            kavod.callback(.{
                .id = "bootstrap.ready",
                .on = kavod.engine(
                    kavod.EngineEvents.variants.ready,
                ),
                .call = Bootstrap.onReady,
                .may = .{
                    kavod.command(
                        .market_data,
                        MarketDataCommands.variants.subscribe,
                    ),
                },
            }),
        }),

        .strategy = kavod.component(Strategy, .{
            kavod.callback(.{
                .id = "strategy.on-bar",
                .on = kavod.event(
                    .market_data,
                    MarketDataEvents.variants.bar,
                ),
                .call = Strategy.onBar,
                .may = .{
                    kavod.message(
                        TradingMessages.variants.signal,
                    ),
                },
            }),
        }),

        .risk = kavod.component(RiskManager, .{
            kavod.callback(.{
                .id = "risk.on-signal",
                .on = kavod.message(
                    TradingMessages.variants.signal,
                ),
                .call = RiskManager.onSignal,
                .may = .{
                    kavod.message(
                        TradingMessages.variants.order_approved,
                    ),
                },
            }),
        }),

        .orders = kavod.component(OrderManager, .{
            kavod.callback(.{
                .id = "orders.on-approved",
                .on = kavod.message(
                    TradingMessages.variants.order_approved,
                ),
                .call = OrderManager.onApproved,
                .may = .{
                    kavod.command(
                        .execution,
                        ExecutionCommands.variants.submit,
                    ),
                },
            }),
        }),

        .shutdown = kavod.component(ShutdownPolicy, .{
            kavod.callback(.{
                .id = "shutdown.on-request",
                .on = kavod.engine(
                    kavod.EngineEvents
                        .variants
                        .shutdown_requested,
                ),
                .call = ShutdownPolicy.onRequest,
                .may = .{kavod.shutdown},
            }),
        }),
    },
});
```

`kavod.Application` is a comptime function returning a type. It validates the complete topology while the application types and callback bodies are comptime known.

### 4.4 Conceptual Generated Dispatch

The application type generates concrete runtime storage and specialized dispatch:

```zig
fn dispatchMarketData(
    runtime: *TradingApp.Runtime,
    event: *const MarketDataEvents.Value,
    host: *TradingApp.DispatchHost,
) void {
    switch (event.*) {
        .bar => |*bar| {
            host.invokeReducer(
                "state.market-data.bar",
                reduceBar,
                &runtime.state,
                bar,
            );

            host.invokeComponent(
                "strategy.on-bar",
                Strategy.onBar,
                &runtime.components.strategy,
                bar,
                &runtime.state,
            );
        },
    }
}
```

For generic dispatch over several variant payload types, generated code uses an inline switch prong:

```zig
switch (event.*) {
    inline else => |*leaf, tag| {
        // tag is comptime known and leaf has the concrete variant payload type.
        dispatchVariant(tag, leaf, runtime, host);
    },
}
```

This removes application payload and callback downcasts. The active union tag remains a runtime choice, but each prong is separately type checked and specialized.

### 4.5 Runtime Application Initialization

```zig
fn initApplication(
    instrument: InstrumentId,
) !TradingApp.Runtime {
    return TradingApp.init(.{
        .state = TradingState{},

        .components = .{
            .bootstrap = Bootstrap{
                .instrument = instrument,
            },

            .strategy = Strategy{
                .order_quantity = 10,
            },

            .risk = RiskManager{
                .max_absolute_position = 100,
            },

            .orders = OrderManager{
                .next_client_order_id = 1,
            },

            .shutdown = ShutdownPolicy{},
        },
    });
}
```

### 4.6 Live Port Implementations

```zig
const LiveMarketData = struct {
    socket: MarketDataSocket,

    pub fn run(
        self: *LiveMarketData,
        io: *kavod.LivePortIo(MarketData),
    ) kavod.PortError!void {
        while (try io.next(self.socket.waitable())) |action| {
            switch (action) {
                .command => |command| switch (command) {
                    .subscribe => |subscription| {
                        try self.socket.subscribe(
                            subscription.instrument,
                        );
                    },
                },

                .external_ready => {
                    const frame = try self.socket.readFrame();
                    const bar = try decodeBar(frame);
                    try io.stage(
                        MarketDataEvents.variants.bar,
                        bar,
                    );
                },

                .closing => return,
            }
        }
    }
};

const LiveExecution = struct {
    broker: BrokerConnection,

    pub fn run(
        self: *LiveExecution,
        io: *kavod.LivePortIo(Execution),
    ) kavod.PortError!void {
        while (try io.next(self.broker.waitable())) |action| {
            switch (action) {
                .command => |command| switch (command) {
                    .submit => |submission| {
                        try self.broker.submit(submission);
                    },
                },

                .external_ready => {
                    const update = try self.broker.readUpdate();

                    switch (update) {
                        .accepted => |accepted| {
                            try io.stage(
                                ExecutionEvents
                                    .variants
                                    .order_accepted,
                                accepted,
                            );
                        },
                        .filled => |fill| {
                            try io.stage(
                                ExecutionEvents.variants.filled,
                                fill,
                            );
                        },
                        .rejected => |rejection| {
                            try io.stage(
                                ExecutionEvents
                                    .variants
                                    .order_rejected,
                                rejection,
                            );
                        },
                    }
                },

                .closing => return,
            }
        }
    }
};

const TradingLive = kavod.LiveEnvironment(
    TradingApp,
    .{
        .market_data = LiveMarketData,
        .execution = LiveExecution,
    },
);
```

### 4.7 Running Live

```zig
fn runLive(
    allocator: std.mem.Allocator,
    config: LiveTradingConfig,
) !kavod.RunOutcome {
    var application = try initApplication(config.instrument);
    defer application.deinit();

    var environment = try TradingLive.init(allocator, .{
        .bindings = .{
            .market_data = LiveMarketData{
                .socket = try MarketDataSocket.connect(
                    allocator,
                    config.market_data_endpoint,
                ),
            },

            .execution = LiveExecution{
                .broker = try BrokerConnection.connect(
                    allocator,
                    config.execution_endpoint,
                ),
            },
        },

        .capacities = .{
            .event_queue_per_port = 1024,
            .command_mailbox_per_port = 256,
        },
    });
    defer environment.deinit();

    var audit = try kavod.FileAuditJournal.init(
        allocator,
        config.audit_path,
        .{
            .max_record_bytes = 256 * 1024,
            .max_turn_bytes = 4 * 1024 * 1024,
            .terminal_reserve_bytes = 1024 * 1024,
        },
    );
    defer audit.deinit();

    const LiveEngine = kavod.Engine(TradingApp, TradingLive);

    var engine = try LiveEngine.init(
        allocator,
        &application,
        &environment,
        &audit,
        .{
            .max_callbacks_per_turn = 10_000,
            .max_messages_per_turn = 1_000,
            .max_commands_per_turn = 1_000,
            .max_actions_per_turn = 20_000,
        },
    );
    defer engine.deinit();

    return try engine.run();
}
```

### 4.8 Simulation Models

```zig
const TimedBar = struct {
    time: kavod.VirtualTime,
    bar: Bar,
};

const HistoricalFeed = struct {
    bars: []const TimedBar,

    fn onSubscribe(
        self: *HistoricalFeed,
        subscription: *const Subscription,
        ctx: anytype,
    ) void {
        for (self.bars) |timed| {
            if (timed.bar.instrument != subscription.instrument) {
                continue;
            }

            ctx.stageAt(
                timed.time,
                MarketDataEvents.variants.bar,
                timed.bar,
            );
        }
    }
};

const SimulatedExchange = struct {
    acceptance_latency: kavod.VirtualDuration,
    fill_latency: kavod.VirtualDuration,
    fill_price: i64,

    fn onSubmit(
        self: *SimulatedExchange,
        submission: *const Submission,
        ctx: anytype,
    ) void {
        ctx.stageAfter(
            self.acceptance_latency,
            ExecutionEvents.variants.order_accepted,
            OrderAccepted{
                .client_order_id = submission.client_order_id,
            },
        );

        ctx.stageAfter(
            self.acceptance_latency.add(self.fill_latency),
            ExecutionEvents.variants.filled,
            Fill{
                .client_order_id = submission.client_order_id,
                .instrument = submission.instrument,
                .side = submission.side,
                .quantity = submission.quantity,
                .price = self.fill_price,
            },
        );
    }
};

const TradingSimulation = kavod.SimulationEnvironment(
    TradingApp,
    .{
        .market_data = kavod.simulatedPort(
            HistoricalFeed,
            .{
                kavod.onCommand(
                    MarketDataCommands.variants.subscribe,
                    HistoricalFeed.onSubscribe,
                ),
            },
        ),

        .execution = kavod.simulatedPort(
            SimulatedExchange,
            .{
                kavod.onCommand(
                    ExecutionCommands.variants.submit,
                    SimulatedExchange.onSubmit,
                ),
            },
        ),
    },
);
```

### 4.9 Running Simulation

```zig
fn runSimulation(
    allocator: std.mem.Allocator,
    config: SimulationTradingConfig,
) !kavod.RunOutcome {
    var application = try initApplication(config.instrument);
    defer application.deinit();

    var environment = try TradingSimulation.init(
        allocator,
        .{
            .models = .{
                .market_data = HistoricalFeed{
                    .bars = config.bars,
                },

                .execution = SimulatedExchange{
                    .acceptance_latency =
                        kavod.VirtualDuration.milliseconds(1),
                    .fill_latency =
                        kavod.VirtualDuration.milliseconds(2),
                    .fill_price = config.fill_price,
                },
            },

            .scheduler = .{
                .start_time = kavod.VirtualTime.zero,
                .horizon = config.end_time,
                .max_scheduled_actions = 100_000,
                .completion = .sources_exhausted,
            },
        },
    );
    defer environment.deinit();

    var audit = try kavod.FileAuditJournal.init(
        allocator,
        config.audit_path,
        .{
            .max_record_bytes = 256 * 1024,
            .max_turn_bytes = 4 * 1024 * 1024,
            .terminal_reserve_bytes = 1024 * 1024,
        },
    );
    defer audit.deinit();

    const SimulationEngine = kavod.Engine(
        TradingApp,
        TradingSimulation,
    );

    var engine = try SimulationEngine.init(
        allocator,
        &application,
        &environment,
        &audit,
        .{
            .max_callbacks_per_turn = 10_000,
            .max_messages_per_turn = 1_000,
            .max_commands_per_turn = 1_000,
            .max_actions_per_turn = 20_000,
        },
    );
    defer engine.deinit();

    return try engine.run();
}
```

### 4.10 Zig Implementation Strategy

The protocol factory uses comptime reflection and type construction:

```text
read the comptime variant specification
validate explicit stable IDs and payload schemas
construct an explicit tag enum with @Enum
construct a tagged payload union with @Union
construct a descriptor table with @Struct
generate injection, projection, manifest, and encoding functions
```

The application factory consumes one comptime topology value and returns an application namespace type. It uses `@typeInfo`, `@hasDecl`, `@FieldType`, inline loops, and `@compileError` to validate:

```text
Port shapes and protocol compatibility
callback parameter and return types
descriptor membership and direction
stable ID uniqueness within the application
consumer completeness
Message producer-consumer closure
Command destination validity
callback-local output authority
stable declaration and fan-out order
```

The application type contains generated declarations with fixed names whose values are generated types:

```text
Runtime
Init
Input
Command
Manifest
DispatchHost
dispatchInput
dispatchMessage
LiveEnvironment
SimulationEnvironment
```

Component-private state is stored in a comptime-generated concrete struct or tuple. Runtime values populate that generated type through `TradingApp.init`.

Dispatch uses generated exhaustive `switch` expressions. Generic protocol dispatch may use `inline else` so each runtime union tag selects a separately analyzed payload type. Per-Port queues retain their concrete protocol union types. Source selection uses an outer generated source enum or an inline switch over the runtime source index.

Callback-specific context types may be generated from each static output declaration. Generic callback bodies using `ctx: anytype` are specialized against those context types. Undeclared output can therefore fail compilation, although dynamic shutdown legality, bounds, and context inertness still require runtime checks.

The live and simulation environment factories similarly consume comptime binding topologies and runtime implementation values. Simulation endpoint dispatch uses direct specialized callbacks and generated closed scheduled-action unions.

### 4.11 Zig Costs And Limitations

1. Protocol descriptors containing types are comptime-only and cannot be selected by runtime control flow.
2. A complete comptime graph requires topology and runtime initialization to use separate APIs.
3. `anytype` callback signatures provide weak standalone documentation until specialized.
4. Zig has no trait declaration corresponding to `PortSpec`; structural validation is repeated through comptime reflection.
5. Zig cannot generate arbitrary attached declarations, requiring wrapper namespaces and descriptor tables.
6. Zig has no language-level runtime type identity or checked `Any` downcast.
7. A dynamic graph would require `anyopaque`, generated thunks, runtime tags, and pointer casts.
8. Zig has no borrow checker or lifetime parameters and cannot prove callback references do not escape.
9. A Component can retain payload, state, or context pointers unless prevented by convention and review.
10. Comptime failures and deeply specialized generic code may produce difficult diagnostics.

The principal benefit is that a static graph can use direct tagged-union dispatch and concrete heterogeneous storage without a separate source-generation language.

## 5. Comparison

| Concern | Rust Static Topology | Zig 0.16 Comptime Topology |
|---|---|---|
| Topology declaration | Custom procedural macro DSL | Ordinary comptime value |
| Protocol sum types | Native enums plus derive generation | Generated or handwritten tagged unions |
| Variant descriptors | Runtime-usable typed zero-sized values | Normally comptime-only type or value tokens |
| Application state storage | Macro-generated concrete struct | Comptime-generated concrete struct or tuple |
| Payload dispatch | Macro-generated exhaustive `match` | Generated exhaustive or inline `switch` |
| Application payload erasure | Not required | Not required |
| Component-state erasure | Not required | Not required |
| Runtime graph composition | Deliberately unavailable | Deliberately unavailable |
| Callback signature validation | Generated Rust assignments and trait bounds | Contextual specialization and reflection |
| Callback-local authority | Generated metadata plus runtime check in this sketch | May be encoded in a specialized context type |
| Temporary-reference non-escape | Enforced for safe borrowed references | Not enforced by the type system |
| Port interface declaration | Traits and associated types | Structural declarations checked at comptime |
| Generated attached declarations | Supported by procedural macros | Not supported by type-construction builtins |
| Diagnostics | Macro spans plus ordinary rustc errors | Comptime and specialization errors |
| Manual semantic primitive | `StaticApplication` implementation | Application namespace type implemented manually |

## 6. Shared Runtime Boundary

Neither compile-time approach removes runtime semantics. Both still require ordinary runtime implementation and tests for:

```text
initial-state validation
runtime capacity compatibility
audit encoding bounds
identifier exhaustion
gate admission and closure races
callback panic or unwind handling
Message FIFO bounds
Command staging and handoff
shutdown dynamic legality
context inertness after fatal failure
live queue exhaustion
simulation schedule bounds
audit synchronization outcomes
terminal cleanup and RunOutcome
```

Compile-time topology answers which routes and values may exist. It does not answer whether a particular runtime operation is currently legal or can complete within configured bounds.

## 7. Verification Requirements

Either static-topology implementation would require:

1. A manually implemented application equivalent to the generated application.
2. Canonical manifest equality between manual and generated implementations.
3. Differential comparison of generated dispatch with an explicit exhaustive reference dispatcher.
4. Compile-fail wrong payload, wrong Port, wrong direction, and wrong callback signature cases.
5. Same payload type in multiple variants of one protocol.
6. Same protocol and payload type through multiple logical Ports.
7. Mutation tests removing source or variant identity from routes.
8. Tests proving callback and fan-out order match the manifest.
9. Tests proving live and simulation use the same application dispatch.
10. Tests proving generated output declarations and runtime authorization cannot diverge.

Rust additionally requires macro expansion tests and Miri over generated borrows and callback invocation. Zig additionally requires adversarial tests for retained pointers and every manually maintained liveness defense because the type system cannot enforce non-escape.

## 8. Research Conclusion

Both alternatives can make the complete application topology compile-time known while supplying application and Environment state at runtime.

The Rust approach retains stronger invocation-scoped reference guarantees and familiar callback signatures, but introduces a substantial procedural macro language and generated-code diagnostic surface.

The Zig approach makes static topology and direct tagged-union dispatch ordinary language-level comptime programming, but requires factory-heavy syntax, comptime-only descriptors, structural interfaces, and non-type-checked pointer-lifetime discipline.

Neither syntax in this document is selected. The examples are retained to document the design space, implementation mechanics, and costs discovered during the Rust-versus-Zig investigation. Any future static-topology proposal should be judged against both the simpler public API in `impl-v6.md` and the verification obligations in the normative v6 design.
