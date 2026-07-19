# Kavod Core Design v3

> **Status:** Draft for review  
> **Scope:** Core engine, state, contexts, scheduling, configuration, execution modes, and initial actor semantics  
> **Replaces:** `design-v1.md` and `design-v2.md` where they conflict with this document

---

## 1. Purpose

Kavod is an event-driven kernel for deterministic backtesting, replay, and live trading.

The kernel processes typed messages through three kinds of consumers:

1. Reducers update engine-wide cached state.
2. Handlers make synchronous decisions and produce new messages.
3. Actors represent isolated components such as venues, feeds, and heavy computation.

The same strategy and kernel semantics should work across backtest, live, and replay. Mode-specific infrastructure may differ, and selected implementations such as simulated and live venues may have different internal subscriptions.

This document distinguishes:

- Decisions that are finalized.
- Recommended implementation structures.
- Decisions intentionally deferred.
- Approaches explicitly rejected.

---

## 2. Core Principles

### 2.1 Determinism

Given the same initial state and the same ordered ingress messages, a backtest or replay must produce the same observable outputs.

The kernel is single-threaded. It alone owns:

- The scheduler.
- The global cache.
- Handler-group state.
- Sequence allocation.
- Graph metadata.
- Message dispatch.
- Actor output ingress.

Live actors may run on separate threads, but all actor outputs cross a single kernel ingress boundary where ordering is frozen.

### 2.2 Explicit Capabilities

Callbacks do not receive `&mut Engine` or `&mut Runtime`.

Each callback receives a narrow context exposing only its permitted operations:

| Consumer | Cache | Output | Time |
|---|---|---|---|
| Reducer | Read/write | None | Dispatch time |
| Handler | Read-only | `send`, `send_at` | Dispatch time |
| Actor | None | `send`, `send_at` | Dispatch time |

Private handler and actor state is passed separately as `&mut S`.

### 2.3 No Silent Drops

Every dispatched message must have at least one consumer.

Every handler and actor callback must declare its possible output types.

Sending an undeclared output is an error.

Mailbox overflow, actor disconnection, and missing consumers must never silently discard messages.

### 2.4 Configuration Is Not Mechanism

Users configure behavior and limits. They do not construct runtime mechanisms.

For actors, users may configure:

- Inbox capacity.
- Overflow policy.
- Operational name.
- Actor-specific settings.

Users do not construct or provide:

- Mailboxes.
- Channels.
- `Sender` or `Receiver` values.
- Locks.
- Threads.
- Executors.
- Scheduler references.

The runtime selects and constructs those mechanisms from declarative configuration.

### 2.5 Isolation

The global cache remains kernel-owned.

Handlers can read it. Reducers can read and write it. Actors cannot access it directly.

Actors receive everything they need through owned messages and maintain private actor state.

There is no shared `RwLock<Cache>`.

### 2.6 No Process-Global Mutable State

Each engine owns all of its state.

There are no process-global actor registries, caches, clocks, runtimes, or sequence counters.

Multiple independent backtests can run on separate operating-system threads.

---

## 3. Terminology

| Term | Meaning |
|---|---|
| Message | Immutable typed domain payload |
| Consumer | Reducer, handler, or actor callback subscribed to a message type |
| Production | Message emitted by a handler or actor |
| Dispatch time | Scheduler timestamp of the message currently being handled |
| Domain time | Timestamp meaningful to a payload, such as exchange time |
| Wall time | Actual operating-system time when code executes |
| Sequence | Kernel-internal ordering value used for deterministic scheduling |
| Global cache | Engine-wide keyed state, mutable only through reducers |
| Handler-group state | Private mutable state shared by handlers in one group |
| Actor state | Private mutable state owned by one actor |
| Ingress | Boundary where an external or actor-produced message enters the kernel |
| Actor inbox | Runtime-owned queue delivering subscribed messages to an actor |
| Mode | Backtest, live, or replay infrastructure selection |

---

## 4. Engine Construction

Registration and execution use separate types.

```rust
let mut app = Engine::builder(config);

// Initial global cache state.
app.seed(SystemConfig::new())?;
app.seed(Portfolio::new())?;
app.seed(Position::new(instrument))?;

// Reducer.
app.reduce::<Fill>(|ctx, fill| {
    let portfolio = ctx
        .get_singleton_mut::<Portfolio>()
        .expect("portfolio must be seeded");

    portfolio.apply(fill);
});

// Stateless handler.
app.on::<Bar>(|ctx, bar| {
    let config = ctx
        .get_singleton::<SystemConfig>()
        .expect("system config must be seeded");

    if bar.close > config.signal_threshold {
        ctx.send(Signal::Buy);
    }
})
.produces::<Signal>();

// Handler group with private persistent state.
app.handler_group(SmaState::new(), |group| {
    group
        .on::<Bar>(|state, ctx, bar| {
            state.update(bar);

            if let Some(signal) = state.signal() {
                ctx.send(signal);
            }
        })
        .produces::<Signal>();

    group.on::<Reset>(|state, _ctx, _reset| {
        state.reset();
    });
});

// Actor with private state and declarative runtime configuration.
app.actor("sim-venue", SimVenue::new(), |actor| {
    actor.inbox_capacity(4_096);

    actor.on::<MarketData>(|venue, _ctx, market| {
        venue.apply_market_data(market);
    });

    actor
        .on::<SubmitOrder>(|venue, ctx, order| {
            for fill in venue.execute(order) {
                ctx.send(fill);
            }
        })
        .produces::<Fill>();
});

let mut engine = app.build()?;

engine.push_event(timestamp, initial_message)?;
engine.run()?;
```

The precise builder syntax is not finalized. The semantic requirements are finalized.

### 4.1 Builder Responsibilities

`EngineBuilder` owns configuration-time state:

- Seeded cache values.
- Reducer registrations.
- Handler registrations.
- Handler-group state.
- Actor registrations.
- Actor configuration.
- Production declarations.
- Consumer metadata.
- Mode configuration.

### 4.2 Build Responsibilities

`build()` must:

1. Resolve configuration.
2. Reject duplicate actor names.
3. Reject duplicate seeded cache entries.
4. Validate every declared production.
5. Verify that declared outputs have consumers.
6. Validate actor configuration required by the selected mode.
7. Compile dispatch indexes.
8. Freeze registrations.
9. Construct runtime-owned mechanisms.
10. Return `Engine` or `BuildError`.

After `build()`, the topology is immutable.

The runtime engine cannot register new reducers, handlers, actors, or message types.

---

## 5. Configuration Model

### 5.1 Engine Configuration

Conceptually:

```rust
pub struct EngineConfig {
    pub mode: Mode,
    pub actor_defaults: ActorDefaults,
    pub max_events_per_instant: usize,
    pub logging: LoggingConfig,
}
```

The exact fields remain open.

Configuration contains values and policies, not runtime objects.

### 5.2 Actor Configuration

Conceptually:

```rust
pub struct ActorConfig {
    pub inbox_capacity: Option<NonZeroUsize>,
    pub overflow_policy: Option<ActorOverflowPolicy>,
}
```

The user may configure these values through the actor registration API:

```rust
app.actor("venue", SimVenue::new(), |actor| {
    actor.inbox_capacity(4_096);
    actor.overflow_policy(ActorOverflowPolicy::Fault);

    // Subscriptions...
});
```

This syntax does not expose a mailbox. It merely records configuration.

The runtime later constructs its private queue:

```text
ActorConfig
    ↓
selected executor
    ↓
private runtime queue/channel
```

### 5.3 Configuration Resolution

Actor configuration may come from:

- Explicit per-actor configuration.
- Explicit mode-level actor defaults.
- Engine-wide explicit defaults.

Recommended precedence:

```text
per-actor override
    > mode-specific actor default
    > engine actor default
```

A live build should fail if required actor configuration remains unresolved.

The effective configuration should be available in startup diagnostics.

There must be no undocumented queue capacity or overflow policy.

### 5.4 Backtest Configuration

Backtest actors execute inline initially and do not allocate runtime inbox channels.

Inbox configuration may still be accepted so the same engine definition can be used across modes. It has no queueing effect for the initial inline backtest executor.

---

## 6. Messages

Messages are immutable typed payloads.

```rust
pub trait Message:
    Send + Sync + Debug + Any + 'static
{
}
```

`Sync` is required because one message may be shared with multiple live actor threads.

### 6.1 Message Ownership

Runtime message ownership uses shared immutable payloads:

```rust
Arc<dyn Message>
```

This permits one message to be:

- Borrowed by reducers.
- Borrowed by handlers.
- Shared with multiple actor subscribers.
- Retained by an in-memory diagnostic log.

Cloning for fan-out clones the `Arc`, not the payload.

### 6.2 Message Contents

Messages contain domain data only.

They do not contain mandatory kernel metadata such as:

- Scheduler sequence.
- Dispatch sequence.
- Kernel receipt time.
- Scheduler priority.
- Actor identity.

A message may contain domain-specific timestamps when meaningful:

```rust
pub struct Trade {
    pub exchange_time: Timestamp,
    pub instrument: InstrumentId,
    pub price: Price,
    pub quantity: Quantity,
}
```

`exchange_time` is part of the trading event itself. It is distinct from the kernel's dispatch time.

### 6.3 No Public Event Envelope

There is no public `Envelope<M>` or `Event<M>` wrapper.

Callbacks receive ordinary typed message references:

```rust
Fn(&mut HandlerCtx, &M)
```

The scheduler necessarily stores an internal queue item containing a timestamp, sequence, and payload. That internal queue representation is not part of the `Message` trait or callback API.

### 6.4 Message Immutability

Reducers, handlers, and actors receive `&M`.

No consumer mutates a dispatched message.

Any transformed representation is produced as a new message.

---

## 7. Time Model

### 7.1 Time Categories

Kavod distinguishes three forms of time.

| Time | Meaning | Exposed to callbacks |
|---|---|---|
| Domain time | Timestamp carried by the business payload | Through message fields |
| Dispatch time | Scheduler timestamp of the current message | Through context |
| Wall time | Actual OS time while code executes | Not exposed to reducers or handlers |

### 7.2 Dispatch Time

Every callback context exposes:

```rust
ctx.dispatch_time() -> Timestamp
```

The definition is:

> The scheduler timestamp of the message currently being handled.

This definition is identical for reducers, handlers, and actors.

There is no separate actor input timestamp.

### 7.3 Stable Dispatch Time

Dispatch time is copied into the context.

Contexts do not contain `&dyn Clock`.

The value remains constant throughout processing of the current message.

```text
message dispatch time: 09:30:00.107
reducer executes:      09:30:00.108 wall time
handler executes:      09:30:00.109 wall time
ctx.dispatch_time():   09:30:00.107 in both callbacks
```

### 7.4 Backtest Time

A historical source supplies the timestamp at which its message should be processed:

```rust
engine.push_event(time("09:31:00"), bar)?;
```

Callbacks processing that bar observe:

```rust
ctx.dispatch_time() == time("09:31:00")
```

The simulated clock advances only when the scheduler pops a later timestamp.

### 7.5 Live Time

Live ingress uses kernel receipt time as dispatch time.

Example:

```text
exchange reports trade: 09:30:00.100
kernel receives trade:  09:30:00.107
callback physically runs: 09:30:00.109
```

The payload contains:

```rust
trade.exchange_time == time("09:30:00.100")
```

The callback context contains:

```rust
ctx.dispatch_time() == time("09:30:00.107")
```

The callback does not observe `09:30:00.109`.

The kernel reads `LiveClock` exactly once when accepting live ingress.

### 7.6 Same-Time Production

A handler calling:

```rust
ctx.send(signal);
```

schedules the message at the current dispatch time.

```text
Bar dispatched at T
Handler sends Signal
Signal scheduled at T with a later sequence
```

### 7.7 Future Production

A handler calling:

```rust
ctx.send_at(ctx.dispatch_time() + delay, message);
```

schedules the message at the requested future timestamp.

A timestamp before the engine's current logical time is a causality violation.

### 7.8 Actor Time

An actor callback processing a message dispatched at `T` observes:

```rust
ctx.dispatch_time() == T
```

`ActorCtx::send` returns a message to the kernel. The kernel timestamps the output when it receives it.

In an inline backtest actor, receipt occurs immediately at the current dispatch time.

In a live actor, receipt may happen later in wall time.

Example:

```text
SubmitOrder dispatch time:       09:30:00.107
Actor physically handles input:  09:30:00.110
Kernel receives actor output:    09:30:00.115
```

Inside the actor callback:

```rust
ctx.dispatch_time() == time("09:30:00.107")
```

When the output is later dispatched:

```rust
output_ctx.dispatch_time() == time("09:30:00.115")
```

No second timestamp method is added to `ActorCtx`.

### 7.9 Actor `send_at`

Actors may explicitly request future scheduling:

```rust
ctx.send_at(ctx.dispatch_time() + modeled_latency, fill);
```

This is especially useful for simulated venues.

The kernel validates the requested timestamp when the output arrives. If the engine has already advanced beyond it, the request is a causality violation.

The exact runtime error policy for this violation remains open.

---

## 8. State Model

Kavod has three distinct state classes.

### 8.1 Global Cache State

Global cache state is:

- Engine-wide.
- Keyed.
- Seeded during construction or created by reducers.
- Readable by handlers.
- Readable and writable by reducers.
- Inaccessible to actors.

Examples:

- Portfolios.
- Positions.
- Instrument definitions.
- Global risk state.
- Order status.
- Strategy-independent market projections.

### 8.2 Handler-Group State

Handler-group state is:

- Private to one handler group.
- Passed as `&mut S`.
- Persistent across callback invocations.
- Shared only by handlers in the same group.
- Not visible through the global cache.
- Not mutable by reducers.
- Not visible to actors.

Examples:

- Moving-average state.
- Signal accumulators.
- Strategy-local configuration.
- Strategy-local finite-state machines.

### 8.3 Actor State

Actor state is:

- Private to one actor.
- Passed as `&mut A`.
- Persistent across actor callback invocations.
- Owned by the actor executor.
- Not stored in the global cache.
- Not visible to reducers or handlers.

Examples:

- Simulated venue order book.
- Connection state.
- Exchange session tokens.
- Model state.
- Actor-specific retry state.

### 8.4 Actor Snapshots

Actors receive required engine data through owned messages.

A snapshot may be created by a handler:

```rust
pub struct SimulateOrder {
    pub order: SubmitOrder,
    pub market: MarketSnapshot,
}
```

The handler reads the cache and emits an owned snapshot:

```rust
app.on::<SubmitOrder>(|ctx, order| {
    let market = ctx
        .get::<MarketSnapshot>(&order.instrument)
        .expect("market snapshot must exist")
        .clone();

    ctx.send(SimulateOrder {
        order: order.clone(),
        market,
    });
})
.produces::<SimulateOrder>();
```

The actor subscribes normally:

```rust
actor.on::<SimulateOrder>(|venue, ctx, command| {
    for fill in venue.execute(command) {
        ctx.send(fill);
    }
});
```

No actor borrows cache state across a thread boundary.

---

## 9. Cache Design

### 9.1 State Trait

Conceptually:

```rust
pub trait State: Send + 'static {
    type Key: Eq + Hash + Send + 'static;

    fn key(&self) -> Self::Key;
}
```

`Clone` is not required by the cache itself.

Individual state types may implement `Clone` when snapshots are needed.

### 9.2 Keyed Storage

Every state type is stored in its own typed map.

Conceptually:

```rust
pub struct Cache {
    stores: HashMap<TypeId, Box<dyn Any + Send>>,
}
```

The value behind `TypeId::of::<T>()` is:

```rust
HashMap<T::Key, T>
```

This is required for collision correctness.

The cache must never use only:

```rust
(TypeId, hashed_key)
```

as the actual identity. Hash collisions must be resolved using `Eq`, as with an ordinary `HashMap`.

### 9.3 Singleton State

Singletons use:

```rust
type Key = ();
```

Example:

```rust
impl State for SystemConfig {
    type Key = ();

    fn key(&self) {}
}
```

### 9.4 Cache API

Recommended core API:

```rust
cache.insert(value)
cache.try_insert(value)

cache.get::<T>(&key)
cache.get_mut::<T>(&key)

cache.remove::<T>(&key)
cache.contains::<T>(&key)

cache.get_singleton::<T>()
cache.get_singleton_mut::<T>()
cache.remove_singleton::<T>()
```

### 9.5 Insertion Semantics

`insert` is an explicit upsert and returns the replaced value, if any.

```rust
fn insert<T: State>(&mut self, value: T) -> Option<T>;
```

`try_insert` rejects duplicate `(state type, key)` entries.

```rust
fn try_insert<T: State>(
    &mut self,
    value: T,
) -> Result<(), DuplicateState>;
```

### 9.6 Seeding

`EngineBuilder::seed` uses duplicate-rejecting insertion.

```rust
app.seed(SystemConfig::new())?;
```

Duplicate seeded state is considered a configuration error.

Reducers may use upsert behavior intentionally at runtime.

### 9.7 Key Stability

A state's key is its identity while stored.

Reducers must not mutate fields in a way that changes the result of `State::key()`.

The exact mechanism for enforcing or validating key stability is not finalized.

Possible future approaches include:

- Documented invariant only.
- Debug validation after mutable access.
- Separating stored keys from state values.
- Making key-bearing fields immutable by domain design.

---

## 10. Reducers

Reducers perform global cache transitions.

```rust
app.reduce::<Fill>(|ctx, fill| {
    let position = ctx
        .get_mut::<Position>(&fill.instrument)
        .expect("position must exist");

    position.apply(fill);
});
```

### 10.1 Reducer Signature

Conceptually:

```rust
Fn(&mut ReducerCtx, &M)
```

### 10.2 Reducer Capabilities

`ReducerCtx` exposes:

```rust
ctx.dispatch_time()

ctx.get::<T>(&key)
ctx.get_mut::<T>(&key)

ctx.get_singleton::<T>()
ctx.get_singleton_mut::<T>()

ctx.insert(value)
ctx.try_insert(value)

ctx.remove::<T>(&key)
ctx.remove_singleton::<T>()
```

### 10.3 Reducer Restrictions

Reducers cannot:

- Send messages.
- Schedule messages.
- Access handler-group state.
- Access actor state.
- Access sequence numbers.
- Access the scheduler.
- Access the OS clock.
- Perform external IO.

The absence of output methods is compile-time enforced by `ReducerCtx`.

### 10.4 Reducer Ordering

For one dispatched message:

- All matching reducers execute before any handler.
- Matching reducers execute in reducer registration order.
- Every reducer completes before the next reducer begins.
- Every reducer completes before actors receive the message.

### 10.5 Why Reducers Do Not Emit

If reducers could mutate the cache and emit messages, they would become privileged handlers.

Keeping reducers state-only provides:

- A clear mutation phase.
- A complete cache view for handlers.
- Easier audit and testing.
- No partially reduced state visible to message-producing code.
- A meaningful distinction between reducers and handlers.

### 10.6 Canonical Reducer Model

The canonical reducer receives `ReducerCtx`, not a specific `&mut S`.

This supports reducers that update multiple related cache entries.

Typed reducer helpers may be added later as ergonomic wrappers, but they are not required by the core model.

There is no separately named "raw cache escape hatch." `ReducerCtx` is the intended mutable-cache capability.

---

## 11. Handlers

Handlers are synchronous deterministic message consumers.

### 11.1 Stateless Handlers

```rust
app.on::<Bar>(|ctx, bar| {
    if should_trade(ctx, bar) {
        ctx.send(Signal::Buy);
    }
})
.produces::<Signal>();
```

Conceptual signature:

```rust
Fn(&mut HandlerCtx, &M)
```

### 11.2 Stateful Handler Groups

```rust
app.handler_group(SmaState::new(), |group| {
    group
        .on::<Bar>(|state, ctx, bar| {
            state.update(bar);

            if let Some(signal) = state.signal() {
                ctx.send(signal);
            }
        })
        .produces::<Signal>();

    group.on::<Reset>(|state, _ctx, _reset| {
        state.reset();
    });
});
```

Conceptual signature:

```rust
Fn(&mut S, &mut HandlerCtx, &M)
```

### 11.3 Group Semantics

All handlers configured within one handler group share the same `S`.

Separate calls to `handler_group` create isolated state, even when they use the same Rust type.

Handler state persists for the lifetime of the engine.

### 11.4 Handler Capabilities

`HandlerCtx` exposes:

```rust
ctx.dispatch_time()

ctx.get::<T>(&key)
ctx.get_singleton::<T>()

ctx.send(message)
ctx.send_at(timestamp, message)
```

### 11.5 Handler Restrictions

Handlers cannot:

- Mutate the global cache.
- Access another handler group's state.
- Access actor state.
- Access sequence numbers.
- Access the scheduler directly.
- Access the OS clock.
- Perform blocking IO.
- Perform unbounded heavy computation.

External IO and heavy computation belong in actors.

### 11.6 Production Declarations

Every handler callback declares every message type it may emit:

```rust
app.on::<Bar>(...)
    .produces::<Signal>()
    .produces::<SubmitOrder>();
```

At runtime:

```rust
ctx.send(Signal::Buy);
```

checks the current handler's declarations.

Sending an undeclared type is an invariant violation.

Declaring a produced type with no consumer is a build error.

---

## 12. Contexts

### 12.1 Handler Context

Conceptually:

```rust
pub struct HandlerCtx<'a> {
    dispatch_time: Timestamp,
    cache: &'a Cache,
    output: &'a mut HandlerOutput<'a>,
}
```

`HandlerOutput` is private runtime machinery.

### 12.2 Reducer Context

Conceptually:

```rust
pub struct ReducerCtx<'a> {
    dispatch_time: Timestamp,
    cache: &'a mut Cache,
}
```

### 12.3 Actor Context

Conceptually:

```rust
pub struct ActorCtx<'a> {
    dispatch_time: Timestamp,
    output: &'a mut ActorOutput,
}
```

`ActorOutput` is runtime-owned and executor-specific.

### 12.4 Context Comparison

| Capability | HandlerCtx | ReducerCtx | ActorCtx |
|---|---:|---:|---:|
| `dispatch_time()` | Yes | Yes | Yes |
| Read global cache | Yes | Yes | No |
| Mutate global cache | No | Yes | No |
| `send` | Yes | No | Yes |
| `send_at` | Yes | No | Yes |
| Handler private state | Function parameter | No | No |
| Actor private state | No | No | Function parameter |
| Clock access | No | No | No |
| Sequence access | No | No | No |
| Scheduler access | No | No | No |
| Mode access | No | No | No |

### 12.5 No Unified Public Context

The contexts intentionally remain distinct.

A single context would either expose excessive capabilities or require runtime checks for operations that should be statically unavailable.

Shared implementation details may be composed internally, but the public types remain separate.

---

## 13. Scheduler

The scheduler owns a min-heap ordered by:

```text
(dispatch timestamp, sequence)
```

Conceptually, its private queued item is:

```rust
struct ScheduledItem {
    dispatch_time: Timestamp,
    sequence: SeqNo,
    payload: Arc<dyn Message>,
}
```

This is not a public message wrapper.

### 13.1 Scheduler Ordering

Lower dispatch time is processed first.

For equal dispatch times, lower sequence is processed first.

### 13.2 Same-Time Breadth-First Behavior

A message produced at the current dispatch time receives a later sequence.

Therefore it is processed after messages already queued at that time.

Example:

```text
Bar@T, seq 10
Timer@T, seq 11

Bar handler produces Signal@T, seq 12

Processing order:
Bar → Timer → Signal
```

### 13.3 Future Scheduling

`send_at(T, message)` inserts the message at `T`.

Future messages remain in the heap until their dispatch time.

### 13.4 Past Scheduling

Scheduling before the engine's current logical time is a causality violation.

The exact choice between panic, `EngineError`, and mode-specific failure remains open.

### 13.5 Sequence Visibility

Sequence is internal to the kernel.

It is not exposed through:

- `Message`.
- `HandlerCtx`.
- `ReducerCtx`.
- `ActorCtx`.

The minimum required sequence invariant is:

> Every scheduled message receives a unique monotonically increasing sequence from the kernel.

Whether reducer and handler invocations also consume a separate trace sequence remains open.

---

## 14. Event Loop

The conceptual event loop is:

```text
validate and freeze topology

while work remains:
    accept available live/actor ingress
    pop earliest scheduled message
    assert dispatch time is monotonic
    assert message has at least one consumer

    run matching reducers in registration order
    run matching handlers in registration order
    deliver message to subscribed actors

    continue
```

### 14.1 Dispatch Atomicity

For one message:

- All reducers complete before handlers.
- All handlers complete before actor delivery.
- Actor delivery begins only after synchronous kernel callbacks finish.
- The scheduler does not pop another message during the dispatch.
- Produced messages do not dispatch recursively.

### 14.2 Consumer Order

Reducer order and handler order are deterministic.

Subscribed actor delivery uses actor registration order.

Within one actor, matching actor callbacks use registration order.

### 14.3 Same-Instant Guard

The kernel must prevent infinite same-time cascades.

A deterministic runtime bound remains required.

The exact guard semantics remain open because counting every event at one timestamp can incorrectly reject legitimate high-volume fan-in.

Possible future guards include:

- Maximum messages processed at one timestamp.
- Maximum causal depth.
- Maximum descendants per root message.
- Static cycle detection for known immediate productions.
- A combination of static and runtime checks.

---

## 15. Actors

### 15.1 Actor Definition

An actor is a stateful isolated component that communicates only through messages.

Actors are suitable for:

- Exchange and broker connections.
- Market-data feeds.
- Simulated venues.
- Machine-learning inference.
- Portfolio optimization.
- Blocking external APIs.
- Heavy computation.
- Components with independent lifecycle state.

### 15.2 Actor Registration

Actors subscribe with `.on::<M>()`.

```rust
app.actor("venue", Venue::new(), |actor| {
    actor.on::<SubmitOrder>(...);
    actor.on::<CancelOrder>(...);
});
```

There is no separate `route::<M>()`.

`.on::<M>()` is both:

- Callback registration.
- Message subscription.
- Graph consumer declaration.

### 15.3 No Point-To-Point API

The initial actor design does not include:

```rust
ctx.send_to(...)
```

It also does not require public actor handles.

Messages use ordinary type-based broadcast dispatch.

Point-to-point actor messaging may be added later if concrete use cases justify it.

### 15.4 Actor Callback Signature

Conceptually:

```rust
Fn(&mut A, &mut ActorCtx, &M)
```

`A` is private actor state.

### 15.5 Actor Production Declarations

Actor callbacks declare produced message types:

```rust
actor
    .on::<SubmitOrder>(...)
    .produces::<OrderAccepted>()
    .produces::<Fill>();
```

`ActorCtx::send` and `ActorCtx::send_at` enforce the declaration.

Actor productions participate in graph validation.

### 15.6 Actor Cache Isolation

Actors never directly access the global cache.

Rejected actor cache designs include:

- `Arc<RwLock<Cache>>`.
- Borrowing `&Cache` across threads.
- Giving actors raw cache pointers.
- Letting actors call cache query APIs asynchronously.

A read lock prevents simultaneous writes but does not guarantee which historical cache version the actor observes.

Example:

```text
seq 1: MarketData A updates cache
seq 2: SubmitOrder delivered to actor
seq 3: MarketData B updates cache
```

Depending on thread scheduling, a cache-reading actor might observe A or B while processing the order.

Both reads would be lock-safe but causally different.

### 15.7 Message-Fed Actor State

A simulated venue can subscribe to market data and maintain a private book:

```rust
app.actor("sim-venue", SimVenue::new(), |actor| {
    actor.on::<MarketData>(|venue, _ctx, market| {
        venue.book.apply(market);
    });

    actor
        .on::<SubmitOrder>(|venue, ctx, order| {
            for fill in venue.execute(order) {
                ctx.send(fill);
            }
        })
        .produces::<Fill>();
});
```

The actor receives messages in kernel delivery order:

```text
MarketData A
SubmitOrder
MarketData B
```

The order therefore executes against market state A.

### 15.8 Snapshot-Fed Actors

An actor may receive an explicit owned snapshot instead of subscribing to every source event.

This is useful when:

- Only a small projection is required.
- Orders are infrequent.
- Maintaining a duplicate actor projection is wasteful.
- The snapshot itself is meaningful and testable.

Snapshots are ordinary messages.

### 15.9 Backtest Actor Execution

Initial backtest actors execute inline.

For each dispatched message:

```text
reducers
handlers
actor 1 callback
actor 2 callback
next scheduler pop
```

Properties:

- No actor thread is created.
- Actor state remains isolated.
- Actor callback order is deterministic.
- Actor outputs return through the kernel ingress path.
- Immediate output is received at the current dispatch time.
- Actor `send_at` can model latency.
- The kernel does not advance before inline actor processing finishes.

### 15.10 Live Actor Execution

Initial live actors use one dedicated thread per actor.

Each live actor has:

- One runtime-owned FIFO inbox.
- One runtime-owned output connection.
- One private state value.
- Serial callback execution.
- A stable actor name.
- Runtime-observed health and queue metrics.

The user does not receive the underlying channels.

### 15.11 Actor Input Ordering

The single kernel thread is the producer for actor inboxes.

Messages are inserted into each actor's inbox in kernel delivery order.

Each actor processes its own inbox serially.

The runtime must not invoke two callbacks concurrently against the same actor state.

### 15.12 Actor Output Ingress

`ActorCtx::send` sends an output to the kernel.

The actor does not assign:

- Kernel sequence.
- Final live dispatch timestamp.
- Scheduler priority.

The kernel assigns those when receiving the output.

Live actor completion order may vary. The kernel logs and sequences the order it actually observes.

### 15.13 Actor `send_at`

`ActorCtx::send_at(T, message)` requests appearance at `T`.

This supports simulation latency and explicit timers.

The kernel remains authoritative:

- It validates `T`.
- It assigns sequence.
- It schedules the message.
- It records ingress.
- It rejects causality violations.

---

## 16. Actor Capacity

### 16.1 Public Configuration

Users configure inbox capacity as a value:

```rust
actor.inbox_capacity(4_096);
```

Users do not construct a mailbox.

The actor API must not expose channel-library types.

### 16.2 Why Capacity Matters

A live actor may process messages more slowly than the kernel produces them.

Without a capacity policy, actor lag can cause:

- Unbounded memory growth.
- Increasing decision latency.
- Stale order handling.
- Stale market-data processing.
- Process termination through OOM.
- Hidden divergence between expected and actual behavior.

### 16.3 Runtime Observability

The runtime should track:

- Current inbox depth.
- Configured capacity.
- High-water mark.
- Oldest queued message age.
- Total messages enqueued.
- Total messages processed.
- Callback processing latency.
- Actor output rate.
- Actor fault status.

The actor itself cannot reliably detect queue growth because the queue is owned by the runtime and accumulates before callback invocation.

### 16.4 Overflow Policy

The user should configure overflow behavior declaratively.

Candidate policies:

| Policy | Behavior | Risk |
|---|---|---|
| Fault | Reject overflow and fault the actor or engine | Stops operation but preserves no-drop semantics |
| Block | Block kernel delivery until capacity exists | Slow actor can stop the kernel |
| Unbounded | Permit indefinite queue growth | OOM and unbounded latency |
| Drop newest | Discard the arriving message | Silent data loss unless explicitly surfaced |
| Drop oldest | Discard queued work | Causal discontinuity |
| Coalesce latest | Replace stale state-like messages | Requires message-specific semantics |

### 16.5 Current Decision

The following are decided:

- Capacity is explicit configuration.
- Mailbox construction is runtime-private.
- Overflow behavior must not be hidden.
- Silent dropping is forbidden by default.
- Queue metrics belong to the runtime.
- Backtest inline execution does not use queue capacity.

The default overflow policy is not finalized.

The current recommendation is bounded capacity plus an explicit fault policy, but this remains subject to approval.

### 16.6 Unbounded Capacity

Explicit unbounded capacity may be supported:

```rust
actor.unbounded_inbox();
```

It must not be an undocumented default.

If supported, it should still expose queue-depth and lag metrics.

An actor cannot fully "catch" unbounded backlog itself because actor code only runs after messages leave the queue.

---

## 17. Actor Failures

### 17.1 Domain Outcomes

Expected operational outcomes should be messages:

```rust
OrderRejected
VenueDisconnected
RequestTimedOut
AuthenticationFailed
```

These are normal domain events and may be handled by reducers and handlers.

### 17.2 Infrastructure Faults

Unexpected actor failures include:

- Actor panic.
- Broken runtime channel.
- Inbox overflow under a fault policy.
- Corrupt actor state.
- Unrecoverable IO runtime failure.
- Callback returning a fatal error.

The exact callback error type remains open.

### 17.3 Initial Failure Recommendation

Recommended initial semantics:

- Backtest actor failure terminates the run with `EngineError`.
- Live actor failure moves the engine into a visible faulted or disarmed state.
- Panics are caught at the actor thread boundary when possible.
- There is no automatic restart.
- There is no silent retry.
- Actor failure diagnostics include actor name and message type.

### 17.4 Startup

Live actors should report readiness before the engine is considered fully operational.

The exact readiness and lifecycle API is not finalized.

### 17.5 Shutdown

Recommended controlled shutdown sequence:

```text
stop accepting new external work
stop delivering new actor inputs
signal actor shutdown
apply configured drain behavior
join actor threads
report timeout or failure
```

Shutdown control is runtime-private and does not require a user-defined domain message.

---

## 18. Modes

```rust
pub enum Mode {
    Backtest,
    Live,
    Replay,
}
```

Mode configures infrastructure. It is not exposed through callback contexts.

### 18.1 Backtest

Backtest mode uses:

- Scheduler-driven logical time.
- Pulled historical sources.
- Inline deterministic actors.
- Simulated venue implementations.
- One single-threaded engine per run.

### 18.2 Live

Live mode uses:

- Kernel receipt time at ingress.
- External source and venue actors.
- Runtime-owned actor threads.
- Runtime-owned queues.
- Mandatory ingress sequencing.
- Operational actor capacity and failure policies.

### 18.3 Replay

Replay mode uses:

- Recorded ingress order.
- Recorded ingress dispatch times.
- No external IO.
- Disabled or replay-stubbed live actors.
- Deterministic kernel execution.

The exact durable replay format is not finalized.

### 18.4 Parallel Backtests

Kavod parallelizes optimization by running independent engines:

```text
thread 1 → parameter set A
thread 2 → parameter set B
thread 3 → parameter set C
thread 4 → parameter set D
```

The engine should be movable between threads.

This motivates `Send` bounds on:

- Messages.
- Cache state.
- Handler-group state.
- Handler closures.
- Reducer closures.
- Actor state.
- Actor callbacks.
- Clock implementations.

The engine is not required to be `Sync`.

### 18.5 Venue Parity

Backtest/live parity means shared contracts, not identical internal subscriptions.

Simulated venue:

```text
MarketData + SubmitOrder → Fill
```

Live venue:

```text
SubmitOrder + exchange responses → Fill
```

A live venue does not register a no-op market-data callback.

Strategy, risk, portfolio, and order-handling code remain unchanged.

---

## 19. Graph Validation

### 19.1 Consumers

The following registrations consume `M`:

```rust
app.reduce::<M>(...)
app.on::<M>(...)
handler_group.on::<M>(...)
actor.on::<M>(...)
```

### 19.2 Producers

Handlers and actor callbacks declare productions:

```rust
.produces::<M>()
```

Reducers never produce messages.

### 19.3 Build Validation

`build()` validates:

1. Every declared production has at least one consumer.
2. Every actor name is unique.
3. Every actor has valid mode-specific configuration.
4. Every registration refers to valid internal metadata.
5. Every seeded cache `(type, key)` is unique.
6. Every compiled subscription index preserves deterministic order.

### 19.4 Runtime Validation

Runtime ingress validates that an incoming message has a consumer.

This includes:

- Initial pushed events.
- Live source messages.
- Live actor outputs.
- Replay messages.
- Handler productions.
- Backtest actor productions.

A per-dispatch assertion may remain as defense in depth.

### 19.5 Production Enforcement

Each callback receives an output capability tied to its declaration set.

A callback cannot legitimately send a type it did not declare.

### 19.6 Cycle Validation

The exact static cycle model remains open.

Actor edges complicate static interpretation because:

- Inline backtest actors may produce immediately at the same timestamp.
- Live actors produce asynchronously at kernel receipt time.
- `send_at` may move an otherwise cyclic edge into the future.

A deterministic runtime cycle guard remains mandatory.

---

## 20. Determinism

### 20.1 Kernel Authority

Only the kernel assigns scheduler sequence numbers.

Only the kernel inserts messages into the scheduler.

Only the kernel accepts actor output ingress.

Only the kernel mutates global cache state through reducer execution.

### 20.2 Sequence

Every scheduled message has a unique kernel-assigned sequence.

Sequence provides deterministic ordering for equal dispatch timestamps.

Sequence is not a user-facing ID generator.

If deterministic business IDs are needed, a dedicated facility should be designed separately.

### 20.3 Sequence Increment Policy

The exact sequence increment policy is not finalized.

The minimum required policy is:

```text
allocate one unique sequence whenever a message is accepted for scheduling
```

An optional separate operation or trace sequence may later record:

- Reducer invocation.
- Handler invocation.
- Actor delivery.
- Message production.
- Ingress acceptance.

Scheduler ordering and operation tracing should not be conflated unless a concrete replay requirement justifies it.

### 20.4 Ingress Log

Every nondeterministic live ingress message must be recorded before dispatch.

This includes:

- Live market-data source output.
- Exchange and broker actor output.
- External control input.
- Other actor-produced messages entering the kernel.

A log record conceptually contains:

```text
dispatch time
kernel sequence
stable message type identifier
serialized payload
source identity
```

The log record is not the runtime `Message` type.

### 20.5 Replay

During replay:

- Live external actors are not contacted.
- Logged ingress messages are injected in recorded order.
- Kernel handlers and reducers execute normally.
- Deterministic actor implementations may be rerun.
- External actor-bound messages may be compared against expected recorded behavior.

The exact outbound verification scheme remains open.

### 20.6 Type Identity

Rust `TypeId` is sufficient for in-process dispatch.

`TypeId` is not a durable cross-build serialization identifier.

Durable logging requires stable message type identifiers or a registered codec schema. That design remains open.

---

## 21. Failure Model

### 21.1 Build Errors

Structural configuration failures should return `BuildError`.

Examples:

- Duplicate seeded state.
- Duplicate actor name.
- Missing consumer.
- Invalid actor capacity.
- Missing overflow policy when required.
- Invalid production metadata.

### 21.2 Runtime Domain Events

Expected operational conditions use messages.

Examples:

- Rejection.
- Disconnection.
- Timeout.
- Risk limit reached.
- Venue unavailable.

### 21.3 Runtime Invariant Violations

Examples:

- Scheduling into the past.
- Undeclared production.
- Impossible type downcast.
- Scheduler sequence overflow.
- Cache corruption.
- Actor runtime disconnection.
- Same-instant runaway cascade.

The exact division between panic, `EngineError`, and mode-specific fault handling remains open.

### 21.4 No Hidden Recovery

The initial design does not include:

- Automatic actor restart.
- Automatic message retry.
- Silent overflow recovery.
- Silent message dropping.
- Silent callback suppression.

Any future recovery behavior must be explicit and testable.

---

## 22. Recommended Implementation Structure

This section is implementation guidance rather than public API.

```rust
pub struct EngineBuilder {
    config: EngineConfig,
    seed_cache: Cache,
    reducers: ReducerRegistry,
    handlers: HandlerRegistry,
    actors: ActorRegistryBuilder,
    graph: GraphBuilder,
}

pub struct Engine {
    runtime: Runtime,
    reducers: ReducerRegistry,
    handlers: HandlerRegistry,
    actors: ActorRuntime,
    graph: ValidatedGraph,
}

struct Runtime {
    scheduler: Scheduler,
    cache: Cache,
    clock: Box<dyn Clock>,
    sequence: Sequence,
    ingress_log: IngressLog,
    dispatch_time: Timestamp,
}
```

### 22.1 Borrow Splitting

The engine should borrow disjoint fields directly.

No `RefCell<Scheduler>` or `RefCell<Cache>` is required.

A narrow internal output sink may borrow:

```rust
struct HandlerOutput<'a> {
    scheduler: &'a mut Scheduler,
    sequence: &'a mut Sequence,
    dispatch_time: Timestamp,
}
```

This allows direct scheduling without exposing `Runtime` or `Engine`.

### 22.2 Handler Registry

Recommended representation:

```rust
struct HandlerRegistry {
    states: Vec<Box<dyn Any + Send>>,
    entries: Vec<HandlerEntry>,
    by_type: HashMap<TypeId, Vec<HandlerId>>,
}
```

Properties:

- Flat handler registration order.
- Explicit state-slot indexes.
- No stateful/stateless group enum duplication.
- Stateless handlers have no state slot.
- Stateful handlers reference a state slot.
- Dispatch uses the `by_type` index.

Public registration should return opaque registration types, not internal entries.

### 22.3 Reducer Registry

Recommended representation:

```rust
struct ReducerRegistry {
    by_type: HashMap<TypeId, Vec<ErasedReducer>>,
}
```

Reducers are flat and ordered by registration.

### 22.4 Actor Registry

The build-time registry stores:

- Stable actor name.
- Actor state.
- Typed callback registrations.
- Consumed message types.
- Produced message declarations.
- Declarative actor configuration.
- Registration order.

The selected executor transforms this into either:

- Inline backtest actor storage.
- Live threaded actor runtimes.
- Replay actor stubs.

### 22.5 No Public Runtime Primitives

The following remain private:

- Channel implementation.
- Actor inbox implementation.
- Actor output transport.
- Scheduler heap.
- Sequence allocator.
- Thread handles.
- Runtime actor IDs.
- Downcast machinery.

---

## 23. Complete Example

```rust
let mode = Mode::Backtest;

let config = EngineConfig {
    mode,
    actor_defaults: ActorDefaults::new()
        .inbox_capacity(4_096),
    // Additional configuration omitted.
};

let mut app = Engine::builder(config);

app.seed(SystemConfig {
    signal_threshold: Price::new("100.00"),
})?;

app.seed(Portfolio::new())?;

app.reduce::<MarketData>(|ctx, market| {
    ctx.insert(MarketSnapshot::from(market));
});

app.reduce::<Fill>(|ctx, fill| {
    let portfolio = ctx
        .get_singleton_mut::<Portfolio>()
        .expect("portfolio must be seeded");

    portfolio.apply(fill);
});

app.handler_group(SmaStrategy::new(10, 30), |group| {
    group
        .on::<MarketData>(|strategy, ctx, market| {
            strategy.update(market);

            if let Some(order) = strategy.order() {
                ctx.send(order);
            }
        })
        .produces::<SubmitOrder>();
});

app.actor("sim-venue", SimVenue::new(), |actor| {
    actor.inbox_capacity(8_192);

    actor.on::<MarketData>(|venue, _ctx, market| {
        venue.book.apply(market);
    });

    actor
        .on::<SubmitOrder>(|venue, ctx, order| {
            for fill in venue.execute(order) {
                ctx.send_at(
                    ctx.dispatch_time() + venue.latency(),
                    fill,
                );
            }
        })
        .produces::<Fill>();
});

app.on::<Fill>(|ctx, fill| {
    let portfolio = ctx
        .get_singleton::<Portfolio>()
        .expect("portfolio must exist");

    record_fill(portfolio, fill);
});

let mut engine = app.build()?;

engine.push_event(
    Timestamp::new(1_000),
    MarketData {
        exchange_time: Timestamp::new(990),
        instrument: InstrumentId::new(1),
        bid: Price::new("99.95"),
        ask: Price::new("100.05"),
    },
)?;

engine.run()?;
```

Conceptual execution:

```text
MarketData scheduled at 1,000
    MarketData reducer updates MarketSnapshot
    SmaStrategy handler reads updated cache and may produce SubmitOrder
    SimVenue receives MarketData and updates private book

SubmitOrder scheduled at 1,000 with later sequence
    No reducer
    No synchronous handler required
    SimVenue receives SubmitOrder
    SimVenue produces Fill at 1,000 + modeled latency

Fill dispatched at modeled timestamp
    Portfolio reducer applies Fill
    Fill handler observes updated Portfolio
```

---

## 24. Invariants

| # | Invariant | Enforcement |
|---:|---|---|
| 1 | Every dispatched message has at least one consumer | Build validation, ingress validation, dispatch assertion |
| 2 | Reducers execute before handlers | Kernel dispatch phase |
| 3 | Handlers complete before actor delivery | Kernel dispatch phase |
| 4 | Reducers execute in registration order | Reducer registry |
| 5 | Handlers execute in registration order | Handler registry |
| 6 | Actor delivery uses actor registration order | Actor registry |
| 7 | Actor callbacks execute serially per actor | Actor executor |
| 8 | Reducers cannot produce messages | `ReducerCtx` capability |
| 9 | Handlers cannot mutate global cache | `HandlerCtx` capability |
| 10 | Actors cannot access global cache | `ActorCtx` capability and isolation |
| 11 | Every handler production is declared | Runtime output check |
| 12 | Every actor production is declared | Runtime output check |
| 13 | Every declared production has a consumer | Build validation |
| 14 | Dispatch time is fixed throughout one callback dispatch | Timestamp copied into contexts |
| 15 | Live dispatch time is kernel receipt time | Ingress boundary |
| 16 | Domain timestamps remain payload fields | Message design |
| 17 | Messages do not expose kernel sequence | Public API |
| 18 | Equal-time scheduling is ordered by kernel sequence | Scheduler |
| 19 | Past scheduling is rejected | Scheduler/ingress validation |
| 20 | Same-time cascades are deterministically bounded | Runtime guard |
| 21 | Cache hash collisions preserve key identity | Typed `HashMap<Key, State>` stores |
| 22 | Only reducers mutate global cache | Context capabilities |
| 23 | Handler groups own isolated private state | State-slot ownership |
| 24 | Actors own isolated private state | Actor executor ownership |
| 25 | Actor-required engine state is delivered through messages | Actor isolation |
| 26 | Actor queues are runtime-owned | Configuration/runtime boundary |
| 27 | Actor capacity and overflow are explicit configuration | Builder validation |
| 28 | No silent message drops | Runtime policy |
| 29 | Backtest actor execution is deterministic | Inline executor |
| 30 | Live actor output order is frozen at ingress | Sequence assignment and logging |
| 31 | Mode is not visible to strategy callbacks | Context API |
| 32 | Multiple engines can run independently | No global mutable state |

---

## 25. Finalized Decisions

| Area | Decision |
|---|---|
| Construction | Separate `EngineBuilder` and runtime `Engine` |
| Validation | Topology is validated and frozen at `build()` |
| Message dispatch | Type-based broadcast |
| Actor subscription | `actor.on::<M>()` is the subscription |
| Explicit actor routes | Removed |
| `send_to` | Removed |
| Public actor handles | Not required initially |
| Message mutability | Immutable |
| Message sharing | `Arc`-based fan-out |
| Message time | No mandatory timestamp on `Message` |
| Public event wrapper | None |
| Context time | `dispatch_time()` |
| Handler cache | Read-only |
| Reducer cache | Read/write |
| Actor cache | No access |
| Reducer output | Forbidden |
| Handler output | `send` and `send_at` |
| Actor output | `send` and `send_at` |
| Sequence visibility | Kernel-internal |
| Cache model | Fully keyed, singleton via `Key = ()` |
| Cache collision handling | Real typed key maps |
| Handler private state | Group-owned, passed as `&mut S` |
| Actor private state | Actor-owned, passed as `&mut A` |
| Live ingress time | Kernel receipt time |
| Dispatch order | Reducers, handlers, actor delivery |
| Backtest actors | Inline and deterministic initially |
| Live actors | Runtime-managed threads initially |
| Live venue market data | No required no-op subscription |
| Sim venue market data | May subscribe and own private market projection |
| Shared cache lock | Rejected |
| Actor inbox | Runtime-owned and hidden |
| Actor inbox capacity | User-configurable declarative value |
| Mode access in callbacks | Forbidden |
| Parallel backtests | One independent engine per thread |

---

## 26. Open Decisions

### 26.1 Actor Overflow Policy

Not finalized.

The initial recommendation is bounded capacity with explicit fault behavior.

Unbounded capacity may be supported as explicit opt-in.

Silent drop policies are not acceptable defaults.

### 26.2 Source Actor API

A feed actor may produce messages without first receiving an engine message.

The callback-based `actor.on::<M>()` API does not yet define:

- Actor startup hooks.
- Long-running source loops.
- Unsolicited actor output.
- Reconnection loops.
- Source shutdown.
- Dispatch time availability when there is no triggering message.

A source-output capability may need to differ from callback `ActorCtx`.

### 26.3 Actor Readiness

The protocol for reporting actor readiness before the engine becomes operational is not finalized.

### 26.4 Actor Error Type

The exact callback return type and fatal-error representation are not finalized.

### 26.5 Actor Shutdown

Drain versus immediate shutdown, timeout behavior, and join failure policy remain open.

### 26.6 Actor Supervision

Automatic restart, retry, and supervision are deferred.

### 26.7 Durable Log Format

Serialization, schema evolution, stable message identifiers, compression, and storage are deferred.

### 26.8 Sequence Increment Policy

Message scheduling requires a unique sequence.

Additional operation sequencing for reducers, handlers, actor delivery, and tracing remains undecided.

### 26.9 Deterministic IDs

Business identifiers must not depend directly on public scheduler sequence because sequence is not exposed.

A dedicated deterministic ID facility may be added later.

### 26.10 Cycle Detection

The division between static cycle analysis and runtime same-time bounds remains open.

### 26.11 Same-Instant Bound

The exact definition of an iteration, causal depth, and legitimate high-volume fan-in remains open.

### 26.12 Runtime Fault Policy

The exact use of panic, `EngineError`, faulted state, and mode-specific behavior remains open.

### 26.13 Key Stability

Enforcement of stable `State::key()` values during mutation remains open.

### 26.14 Configuration Syntax

The exact Rust builder syntax for engine and actor configuration remains open.

The configuration/mechanism boundary is finalized even though method names are not.

### 26.15 Control Plane

Trading lifecycle, reconciliation, arming, kill-switches, and operator commands are not finalized by this document.

### 26.16 Deterministic RNG

RNG ownership, seeding, and callback capabilities are deferred.

### 26.17 Parallel Backtest Actors

Parallel execution inside one backtest is deferred.

A future deterministic executor may run independent actor work on worker threads while committing outputs in causal order.

### 26.18 Fine-Grained Routing

Point-to-point actor delivery, keyed routing, selectors, and dynamic subscriptions are deferred.

The initial system uses broadcast-by-message-type only.

---

## 27. Rejected Designs

### 27.1 Shared Actor Cache

Rejected because locks provide mutual exclusion but not causal snapshot consistency.

### 27.2 `RefCell` Scheduler Or Cache

Rejected because kernel field borrows can be split statically. Runtime borrow failures are unnecessary.

### 27.3 Internal Kernel Channels For Handler Output

Rejected because handlers run synchronously on the kernel thread and can schedule through narrow mutable field borrows.

Actor channels remain necessary at the actor boundary.

### 27.4 Public Mailboxes

Rejected because users should configure actor behavior, not construct transport mechanisms.

### 27.5 Message-Owned Scheduler Time

Rejected because domain time and engine dispatch time are separate concerns.

### 27.6 Public Message Sequence

Rejected because sequence is kernel scheduling and replay infrastructure.

### 27.7 Unified Context

Rejected because it would expose excessive capabilities and weaken compile-time separation.

### 27.8 Explicit Actor Routes

Rejected because `actor.on::<M>()` already declares the subscription.

### 27.9 Initial Point-To-Point Messaging

Deferred because broadcast subscriptions satisfy the current use cases and produce a smaller core.

### 27.10 Identical Venue Subgraphs

Rejected because parity concerns contracts and strategy semantics, not implementation-specific subscriptions.

A simulated venue may consume market data while a live venue does not.

---

## 28. Next Design Topic

The next actor design discussion should resolve:

1. Source actors that produce without an input message.
2. Actor runtime configuration and overflow behavior.
3. Actor startup and readiness.
4. Actor callback errors and runtime faults.
5. Controlled shutdown.
6. Replay stubs and outbound verification.
7. Durable actor ingress logging.
8. Whether every live actor requires a dedicated thread or whether executors may share worker infrastructure.
