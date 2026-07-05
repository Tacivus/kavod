# Kavod Design

## 1. Core Principles

**Determinism.** Same inputs produce identical outputs, every run. The core event loop is single-threaded. Messages, handlers, and state transitions are recorded, making the system fully replayable. Strategy code is pure and synchronous — no non-deterministic side effects.

**Robustness.** Fail fast, fail loud. Every dispatched message *must* have at least one registered consumer. Every handler *must* declare what messages it produces. Unconsumed or undeclared messages cause an immediate panic. There are no silent drops.

**Correctness.** Compile-time type safety at every boundary. Messages, state, and handlers are all generic — no manual parsing, no `serde` in the hot path. The kernel enforces invariants: reducers run before handlers, handler execution is deterministic, actors are isolated from the main thread.

---

## 2. Core Concepts

### Messages

Typed events that flow through the system. User-definable.

```rust
trait Message: Send + 'static {}
```

| Example | Kind |
|---|---|
| `Bar`, `Tick`, `Quote` | Market data events |
| `NewOrder`, `Cancel`, `Amend` | Order commands |
| `Fill`, `Reject`, `ExecutionReport` | Execution events |
| `Signal`, `ApprovedOrder` | Derived messages |

Messages are **ephemeral** — they fire, dispatch, and are gone. They are not cached.

### State

User-defined structs that persist across messages. Two tiers:

**Per-handler state** — owned by a group of handlers, passed as `&mut State`. For indicators, strategy config, signal accumulators. Isolated — handlers in one group never see another group's state. Multiple handlers on the same `.state(S)` group execute in registration order.

**Global cache** — system-wide shared state updated by reducers. For portfolio, positions, order book, instrument definitions. Handlers can read and write the cache. Handlers have mutable access to the cache. With deterministic handler ordering, last-write-wins is deterministic and replayable.

```rust
trait State: Clone {
    type Key: Hash + Eq;
    fn key(&self) -> Self::Key;
}
```

### Reducers

Functions that translate a message into a state mutation. Registered per state + message pair.

```rust
app.state(Portfolio::new())
    .reduce::<Fill>(|portfolio, fill| portfolio.cash -= fill.cost);
```

Reducers are **pure** — they mutate state, they do not produce messages. They run before handlers, guaranteeing that the cache is up-to-date when handlers execute.

### Handlers

Synchronous functions subscribed to specific message types. No IO. No blocking. Fully deterministic.

```rust
// With per-handler state:
app.state(SmaStrat::new())
    .on::<Bar>(|state, ctx, bar| state.sma.update(bar.close))
    .produces::<NewOrder>();

// Stateless:
app.on::<Bar>(|ctx, bar| { /* ... */ })
    .produces::<Signal>();
```

Every handler **must** declare what messages it produces via `.produces::<M>()`. Any `ctx.send::<M>()` call that was not declared panics at runtime. Every declared `.produces::<M>()` is validated at startup — it must have at least one registered consumer.

### Actors

Components that run on their own thread and communicate via messages. Used for:

- External IO (exchange connections, market data feeds)
- Heavy compute (ML models, portfolio optimization)
- Anything that would block the main event loop

```rust
let actor = app.spawn(|inbox, outbox| {
    for msg in inbox {
        outbox.send(heavy_compute(msg));
    }
});
```

Don't own strategy state. Don't run on the main thread. Communicate purely through channels.

### Kernel

The orchestrator. Owns the message bus, cache, per-handler states, and the outbox. Runs a single-threaded event loop. Validates the message graph at startup. Enforces all invariants.

### Context

A narrowed, controlled view of the kernel passed to handlers. Exposes:

- `ctx.send::<M>(msg)` — produce a message (must be declared via `.produces`)
- `ctx.get::<T>()` / `ctx.get_mut::<T>()` — read/write the global cache
- `ctx.send(&actor, msg)` — send to an actor

---

## 3. Architecture

```
                   ┌─────────────────┐
                   │   IO Actors      │  (exchange, market data)
                   │   (own threads)   │
                   └────────┬────────┘
                            │ push messages via channel
                   ┌───────┴────────┐
                   │   ML Actor      │  (heavy compute)
                   └────────┬────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────┐
│                     KERNEL (single-threaded)          │
│                                                      │
│  Channel ──► For each message M:                     │
│               1. Assert consumers(M) is non-empty     │
│               2. Apply all M-reducers → update cache  │
│               3. Dispatch to all M-handlers           │
│               4. Route M to actor subscribers         │
│             ──► Drain outbox → route to actors        │
│             ──► Loop                                  │
│                                                      │
│  ┌─────────┐  ┌──────────────────────┐               │
│  │  Cache   │  │  Per-handler states  │               │
│  │ (global) │  │  (S, T, U, ...)     │               │
│  └─────────┘  └──────────────────────┘               │
│                                                      │
│  ┌─────────┐  ┌──────────────────────┐               │
│  │  Outbox  │  │  Handler registry     │               │
│  │ (pending│  │  (per message type)   │               │
│  │  msgs)  │  │  + produce/consume    │               │
│  └─────────┘  │  declarations         │               │
│               └──────────────────────┘               │
└──────────────────────────────────────────────────────┘
```

**The event loop:**

```
loop {
    for msg in channel.drain() {
        assert!(!consumers(msg.type_id()).is_empty());
        for reducer in reducers_of(msg.type_id()) { reducer.apply(msg, cache); }
        for handler in handlers_of(msg.type_id()) { handler.call(msg, ctx); }
    }
    for msg in outbox.drain() { route(msg); }
}
```

---

## 4. Invariants

| # | Invariant | Enforced |
|---|---|---|
| 1 | Every dispatched message has ≥1 consumer | Kernel panics before dispatch |
| 2 | Reducers apply before handlers for the same message | Kernel ordering |
| 3 | Every handler must declare what it produces | Required `.produces::<M>()` |
| 4 | Every declared `.produces::<M>()` has ≥1 consumer | Validated at startup |
| 5 | `ctx.send::<M>()` must match a `.produces::<M>()` declaration | Kernel panics at runtime |
| 6 | Strategy code is sync, no IO, no blocking | Handlers accept `&Context`, not `&mut Kernel` |
| 7 | All external IO and heavy compute runs on separate threads | Actors spawned via `app.spawn()` |
| 8 | Same core codepath in backtest and live | Kernel is identical; only IO actors differ |
| 9 | Per-handler state is isolated | Each `.state(S)` group owns its state independently |

---

## 5. The Message Graph

Required `.produces` declarations turn registrations into a self-documenting message graph. At startup, the kernel validates connectivity: every declared produce must have a consumer.

```
Sources     → Handler     → Productions      → Consumers
────────────────────────────────────────────────────────────
Bar           SmaStrat      NewOrder           Risk handler, Exchange actor
                            Signal             Signal handler
Fill          — (reducers   —                  —
               only)
NewOrder      OrderBook*    —                  —
ApprovedOrder —             —                  Exchange actor (route)
```

*Reducers consume messages but do not produce them.

```rust
app = Kernel::new()

    // ── State (reducers only, no produce) ──
    .state(Portfolio::new())
        .reduce::<Fill>(update_portfolio)
    .state(OrderBook::new())
        .reduce::<NewOrder>(track_order)
        .reduce::<Fill>(remove_filled)

    // ── Strategy ──
    .state(SmaStrat::new())
        .on::<Bar>(generate_signals)     // consumes Bar
        .produces::<NewOrder>()          // produces NewOrder
        .produces::<Signal>()            // produces Signal

    // ── Risk ──
    .on::<NewOrder>(check_risk)          // consumes NewOrder
        .produces::<ApprovedOrder>()     // produces ApprovedOrder
    .on::<Reject>(log_reject)            // consumes Reject
        // produces nothing

    // ── Exchange ──
    .route::<ApprovedOrder>(&exchange);  // consumes ApprovedOrder
```

**Validation at `app.run()`:**

1. Every handler's `.produces::<M>()` is checked against the consumer registry
2. If any `M` has no consumer → panic with exact message type and handler name
3. If the graph checks out → enter the event loop

---

## 6. Examples

### 6.1 Simple SMA Crossover

```rust
struct SmaStrat {
    fast: SMA,
    slow: SMA,
}

let mut app = Kernel::new();

app.state(SmaStrat { fast: SMA::new(10), slow: SMA::new(30) })
    .on::<Bar>(|state, ctx, bar| {
        state.fast.update(bar.close);
        state.slow.update(bar.close);

        if state.fast.value() > state.slow.value() {
            ctx.send(NewOrder {
                instrument: bar.instrument,
                side: Side::Buy,
                quantity: 100,
            });
        }
    })
    .produces::<NewOrder>();
```

### 6.2 State Transitions (Fill → Portfolio + OrderBook)

```rust
app.state(Portfolio::new())
    .reduce::<Fill>(|portfolio, fill| {
        portfolio.cash -= fill.cost;
        *portfolio.positions.entry(fill.instrument).or_default() += fill.qty;
    });

app.state(OrderBook::new())
    .reduce::<NewOrder>(|book, order| book.add_pending(order))
    .reduce::<Fill>(|book, fill| book.remove_filled(fill.cl_order_id));
```

### 6.3 Background Actor with Message Routing

```rust
let model = app.spawn(|inbox, outbox| {
    for features in inbox { outbox.send(expensive_predict(features)); }
});

app.on::<Bar>(|ctx, bar| {
    ctx.send(&model, Features::from_bar(bar));
})
.produces::<Signal>(); // this handler may put a `Signal` on the msg bus 

app.on::<Signal>(|ctx, signal| {
    if signal.confidence > 0.8 {
        ctx.send(NewOrder { ... });
    }
})
.produces::<NewOrder>();
```

### 6.4 Backtest ≠ Live

```rust
// Strategy code is identical:

// Backtest
app.spawn(BacktestLoader::new("data/*.parquet"));
app.run();

// Live
app.spawn(BrokerConnection::connect("wss://broker.example.com"));
app.run();
```

---

## 7. What is NOT in scope (for now)

- **Persistence / replay format** — message log serialization, checkpointing
- **Actor lifecycle** — supervision, restart policies, health checks
- **Reducer application ordering** — when multiple states reduce the same message
- **Time management** — clock types, time acceleration in backtest
- **Networking** — channel/mailbox implementation details
