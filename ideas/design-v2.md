# Kavod Core Design (MVP)

This document specifies the core kernel architecture: the single-threaded
deterministic event loop, message/handler/state model, clock & determinism
infrastructure, scheduler, lifecycle, failure model, and the backtest/live
substrate split. Routing beyond broadcast (per-instrument `RouteKey` dispatch)
is intentionally **out of scope for the MVP** — see `route_key_design.md` for
that extension. In the MVP, handlers that need fine-grained filtering (e.g.
"only 1-minute bars for AAPL") receive every message of the type and filter
in their closure.

---

## 1. Core Principles

**Determinism.** Same recorded inputs produce byte-identical outputs, every
run. The core event loop is single-threaded. All non-determinism (thread
scheduling, wall-clock, IO arrival) is frozen at the kernel's ingress
boundary: every inbound message is logged with a timestamp and a monotonic
sequence number before it is processed. Backtests are deterministic *by
construction*; live runs are deterministic *on replay* of the log. Strategy
code is pure and synchronous, and gets time and randomness only from the
context — never from the OS.

**Robustness.** Fail fast, fail loud — but distinguish *structural* faults
from *runtime* faults. Structural faults (a message with no consumer, an
undeclared production) are validated at startup and panic before any capital
is at risk. Unrecoverable runtime faults use **crash-only** semantics (abort
→ fast restart → reconcile from externalized state). Risk/control events
(kill-switch, loss-limit, operator stop) are handled as *normal, well-tested*
operations, not error paths. No silent drops.

**Correctness.** Message payloads are compile-time typed at every handler
boundary. The produce/consume wiring is a dynamic graph validated
exhaustively at startup. Reducers run before handlers; handler execution
order is deterministic; actors are isolated from the main thread. Trading
domain values (`Price`, `Quantity`, `Timestamp`) are typed newtypes with
fail-fast checked arithmetic — no silent overflow/NaN/negative propagation.

---

## 2. Core Concepts

### Messages

Typed events that flow through the system. User-definable. Every message
carries a timestamp (`ts: u64`, nanoseconds since Unix epoch) assigned at
ingress.

```rust
trait Message: Send + 'static {}
```

| Example | Kind |
|---|---|
| `Bar`, `Tick`, `Quote` | Market data events |
| `NewOrder`, `Cancel`, `Amend` | Order commands |
| `Fill`, `Reject`, `ExecutionReport` | Execution events |
| `Signal`, `ApprovedOrder` | Derived messages |
| `Command` (and variants) | Control-plane commands (see §6) |

Messages are **ephemeral** — they fire, dispatch, and are gone. They are not
cached.

### Domain primitives

The MVP ships typed wrappers for the load-bearing trading values. Operations
on these newtypes panic on overflow / NaN / negative-out-of-range, in line
with fail-fast (see §7):

```rust
pub struct Price(Decimal);     // non-negative
pub struct Quantity(Decimal);  // non-negative
pub struct Timestamp(u64);    // nanos since Unix epoch; monotonic per kernel
pub struct InstrumentId(u32); // interned index into the registered universe
```

`InstrumentId` is an interning handle: symbols are strings *only* at the
adapter boundary and are interned to a `u32` index into the universe table on
ingest. Everything internal uses the integer — `Copy`, no hashing of
strings in the loop, and routing tables (when added) are arrays.

### State

User-defined structs that persist across messages. Two tiers:

**Per-handler state** — owned by a group of handlers, passed as `&mut State`.
For indicators, strategy config, signal accumulators. Isolated — handlers
in one group never see another group's state. Multiple handlers on the same
`.state(S)` group execute in registration order.

**Global cache** — system-wide shared state, updated *only* by reducers.
For portfolio, positions, order book, instrument definitions. Handlers read
the cache (`&T`); all cache mutation flows through reducers (see Open Item 1;
the MVP enforces this — helpers can be added to make common mutations
ergonomic). With deterministic handler ordering, last-write-wins is
replayable, but writing from reducers only keeps the mutation surface
explicit and audit-traceable.

The cache is **keyed** so it can hold multiple instances of a type
(per-account portfolio, per-instrument book, per-order book entry).
Singletons use `Key = ()`.

```rust
trait State: Clone {
    type Key: Hash + Eq;
    fn key(&self) -> Self::Key;
}

// read only:
ctx.get::<Portfolio>(&key)        // -> Option<&Portfolio>
ctx.get::<OrderBook>(&clOrderId)  // -> Option<&OrderBookEntry>
```

### Reducers

Functions that translate a message into a state mutation. Registered per
(state + message) pair. Pure in the sense that they mutate state and produce
**no** messages. They run before handlers for the same message, guaranteeing
the cache is up-to-date when handlers execute. Reducers for a given message
run in registration order. Reducers can touch the cache mutably; handlers
cannot.

```rust
app.state(Portfolio::new())
    .reduce::<Fill>(|portfolio, fill| portfolio.cash -= fill.cost);
```

### Handlers

Synchronous functions subscribed to specific message types. No IO, no
blocking, fully deterministic. Read-only access to the global cache;
read/write to their own per-handler state; may produce messages. Handlers
that need fine-grained filtering (MVP) filter inside their closure.

```rust
// With per-handler state:
app.state(SmaStrat::new())
    .on::<Bar>(|state, ctx, bar| {
        if bar.instrument != state.instrument { return; }       // MVP filter
        state.sma.update(bar.close);
        // ...
    })
    .produces::<NewOrder>();

// Stateless:
app.on::<Bar>(|ctx, bar| { /* ... */ })
    .produces::<Signal>();
```

Every handler **must** declare what it produces via `.produces::<M>()`. Any
`ctx.send::<M>()` not declared panics at runtime. Every declared
`.produces::<M>()` is validated at startup — it must have ≥1 registered
consumer.

### Context

A narrowed, controlled view of the kernel passed to handlers:

- `ctx.send::<M>(msg)` — produce a message at the current instant (must be declared)
- `ctx.send_at(ts, msg)` — schedule a message at a future instant (panics if `ts < ctx.now()`)
- `ctx.get::<T>(&key)` — read the global cache (immutable)
- `ctx.now() -> Timestamp` — current time. `SimClock` in backtest, `LiveClock` in live, logged value on replay. **Never** read the OS clock directly.
- `ctx.seq() -> u64` — the current global sequence number (see §3)
- `ctx.rng()` — deterministic, seeded RNG (seed recorded in the log)
- `ctx.send_to(&actor, msg)` — send to an actor out-of-band (point-to-point)
- `ctx.trading_enabled() -> bool` — whether the kernel is `Armed` and algo trading is on (see §6)

### Clock & Time

Time is a first-class, swappable component behind one trait:

- `LiveClock` — reads wall-clock; the read value is stamped onto the message at ingress and written to the log.
- `SimClock` — advances only via the scheduler (§4); `now()` equals the timestamp of the event currently being processed.

On **replay**, `now()` returns the logged value regardless of clock type.
Any use of time or randomness that doesn't go through `ctx` is a determinism
bug.

### Actors

Components that perform external IO or heavy compute off the main loop:

- External IO (exchange connections, market data feeds, the simulated exchange)
- Heavy compute (ML models, portfolio optimization)

Actors don't own strategy state, don't run on the main thread, and
communicate purely through messages. They express *when* a message should
appear, not just *whether*:

```rust
let actor = app.spawn(|inbox, outbox, ctx| {
    for msg in inbox {
        let result = heavy_compute(msg);
        outbox.send_at(ctx.now() + latency_ns, result); // fires at time T
    }
});
```

`send_at(T, msg)` is the single time-aware primitive that unifies backtest
and live (§5). `T < now` is rejected (causality violation). In the MVP,
the only routing to actors is `ctx.send_to(&actor, msg)` (point-to-point)
and `app.route::<M>(&actor)` (every `M` to this actor). Per-key actor routing
is deferred (see `route_key_design.md`).

### Scheduler

Owns the time-ordered event queue: a min-heap keyed by `(ts, seq)`. It pops
the earliest event, advances `SimClock` to its `ts`, and hands it to the
kernel. Produced messages (same-instant or future) re-enter the heap. In
backtest the scheduler *is* the driver of time; in live it schedules real
timers and forwards channel arrivals (§5).

### Kernel

The orchestrator. Owns the message bus, cache, per-handler states, the
inbound log, and the sequence counter. Runs a single-threaded event loop.
Validates the message graph at startup. Enforces all invariants. The
kernel **owns all its state** — there is no global mutable state anywhere,
so multiple kernels can run in one process (critical for parameter
optimization / walk-forward).

---

## 3. Determinism Model

### Inbound log

Every message entering the kernel is appended to a log as `(seq, ts, payload)`
*before* processing. This is the determinism boundary: whatever
non-determinism produced the arrival order (thread scheduling, network) is
frozen here. Replay = feed the log back through the identical kernel.

### Global sequence counter (`seq`)

A monotonic `u64` incremented on every observable operation — message
received, reducer applied, handler invoked, message produced. It provides:

- A total order and deterministic tiebreaker for same-timestamp events (the heap key is `(ts, seq)`).
- A **replay-verification invariant**: replaying a log must reproduce the *identical seq stream*. Any divergence means non-determinism leaked in — a built-in regression canary.
- **Deterministic ID generation**: client order IDs derived from `seq` are reproducible across backtest, live, and replay, instead of being a non-deterministic side channel (a very common determinism leak in trading systems).

The log itself is mandatory for live (it is the crash-recovery artifact —
see §7) and is also written in backtest (verification + audit).

---

## 4. Execution Model

Single time-ordered loop over the scheduler's heap:

```
loop {
    let ev = heap.pop_min();            // earliest (ts, seq)
    assert!(ev.ts >= now);              // monotonic; past = causality bug
    now = ev.ts;
    assert!(!consumers(ev.type_id()).is_empty());
    for r in reducers_of(ev.type_id()) { r.apply(ev, cache); }  // reducers first
    for h in handlers_of(ev.type_id()) { h.call(ev, ctx); }     // then handlers
    // ctx.send / send_at during the above push new events into the heap
}
```

### Ordering guarantees

- All reducers, then all handlers, for a single message complete before the next message is popped.
- Produced messages re-enter the heap:
  - **Same instant** (`ts == now`): larger `seq`, so popped after everything already queued at `now` → BFS within an instant. A full causal cascade at time T (`Bar → Order → ApprovedOrder`) resolves before `now` advances.
  - **Future** (`send_at`, `ts > now`): popped at its timestamp.
  - **Past** (`ts < now`): panic — causality violation.
- **Same-instant cycle guard:** a max-iterations-per-instant bound (or a static acyclicity check on the same-instant subgraph) prevents `A@T → B@T → A@T …` from hanging time. Future-timestamped cycles are self-limiting because `now` keeps moving.

### Lazy sources / k-way merge

Data sources are not dumped into the heap up front. The heap holds only the
*next* pending event per source (plus scheduled events); popping from a
source pulls its next row. Memory stays bounded regardless of dataset
size (a 40-year parquet is not loaded into RAM). In live, that same source
is a thread pushing events; in backtest it is pulled by the scheduler. Same
source logic, different pump.

---

## 5. Backtest vs Live Substrate

The kernel, reducers, handlers, strategy code, and actor logic are
**identical** in both. Only two contained substrate pieces differ:

| Concern | Backtest | Live |
|---|---|---|
| Clock | `SimClock` — advances via scheduler to next event `ts` | `LiveClock` — wall clock, stamped at ingress |
| `send_at(T, msg)` | schedules into the min-heap; fires at sim-time T | arms a real timer; fires at wall-time T |
| Data source | pulled lazily by the scheduler (k-way merge) | a thread pushing events into the kernel |
| Execution venue | simulated-exchange actor (models latency, partials, slippage) | real exchange/broker actor |

Because the exchange sim schedules `Fill @ now + latency` via `send_at`, a
fill that lands *between* two bars is processed in correct temporal order by
the heap — no look-ahead. Same order-handling logic
(`send_at(now + latency, fill)`) runs unchanged live.

### Sandbox / paper trading

Falls out for free and should be advertised as a first-class mode:
`LiveClock` + real-data feed actor + `SimExchange` actor. Zero new code in
the kernel; just a different substrate combination. It is the critical
pre-live testing mode (real-time data, simulated fills, no capital at risk).

```rust
// Strategy + wiring identical; only the spawned source/venue differ:

// Backtest
app.spawn(ParquetSource::new("data/*.parquet"));
app.spawn(SimExchange::new(LatencyModel::fixed(300_000))); // 0.3ms
app.run();

// Sandbox (paper trading — real-time data, sim fills)
app.spawn(WebSocketFeed::connect("wss://feed.example.com"));
app.spawn(SimExchange::new(LatencyModel::uniform(100_000..500_000)));
app.run();

// Live
app.spawn(BrokerFeed::connect("wss://feed.example.com"));
app.spawn(BrokerExchange::connect("wss://broker.example.com"));
app.run();
```

---

## 6. Lifecycle & Failure

### Boot lifecycle

Reconciliation is the actors' responsibility (each venue actor reconciles
its own external truth and emits state-setting messages into the cache via
reducers). To prevent strategies from trading off half-reconciled state, the
kernel gates through explicit phases:

```
Booting → Reconciling → Armed
```

- **Booting** — graph validation, state construction, actor spawn.
- **Reconciling** — actors reconcile external state and push it in; order-producing handlers are inert.
- **Armed** — fully operational; orders may flow.

A `trading_enabled` flag (mutable via control-plane commands) further gates
algo order production in `Armed` — see below. Strategies arm only once all
registered venue actors report reconciled.

### Control-plane commands

There is a first-class **Command** message class for operator/runtime control
(toggle algo trading, operator stop, etc.), delivered over the same bus:

```rust
app.on::<Command>(|ctx, cmd| match cmd {
    Command::EnableAlgo   => ctx.set_trading_enabled(true),
    Command::DisableAlgo  => ctx.set_trading_enabled(false),
    Command::CloseAllPositions => { ctx.send(CancelAll); ctx.send(FlattenAll); }
    Command::Halt         => ctx.set_trading_enabled(false),
});

app.on::<NewOrder>(|ctx, order| {
    if !ctx.trading_enabled() { return; }           // kill-switch gate
    // ...
})
.produces::<ApprovedOrder>();
```

This makes the kill-switch a **normal, well-tested code path** rather than an
error path — exercised every time you stop a backtest, ever-present in tests.

### Failure regimes (three distinct, intentionally forked by mode)

1. **Structural faults (startup, pre-Armed).** Missing consumer, undeclared
   production, graph cycle, same-instant cycle unable to bound. → `panic`.
   No capital is at risk yet.

2. **Unrecoverable runtime faults (live): corrupt data, invariant violation,
   arithmetic overflow on a domain primitive.** → **crash-only**: the
   process aborts (`panic = abort`), restarts fast, and reconciles from
   externalized state on restart. The rationale (from crash-only design):
   a separate, rarely-exercised graceful-flatten-on-fault path is itself
   buggy; the restart+reconcile path is exercised on every start and is
   therefore trustworthy. The inbound log + actor reconciliation *is* the
   recovery path — which is why the log is mandatory for live.

3. **Risk / control events (live or backtest): loss-limit hit, operator
   stop, circuit break.** → handled as normal operations via the control
   plane (above), not as panics. Cancel-all / flatten run, alerts fire,
   trading is disabled — the process may *continue* running and observe.

4. **Runtime faults (backtest):** panic freely; the run is reproducible.

This is the one place the "identical codepath" intentionally forks: *failure
semantics* differ by mode.

### No global mutable state

The kernel owns all of its state in instance fields, not process-wide
`OnceLock`/`lazy_static`/global Tokio runtime. This means **multiple kernels
can run in one process**, which is exactly what parameter sweeps / walk-forward
/ walk-over-instruments need (parallel backtests, isolated result collection).
This is an explicit, non-negotiable property.

---

## 7. Architecture

```
        ┌─────────────────────────────────┐
        │  IO / Compute Actors            │  feed, exchange (real or sim), ML
        │  live: threads · bt: pulled      │
        └───────────────────┬─────────────┘
              send_at(T, msg) / ingress
                        ▼
        ┌─────────────────────────────────┐
        │  Scheduler (min-heap by (ts,seq))│
        │  + Inbound Log (seq, ts, payload)│
        └───────────────────┬─────────────┘
                        ▼
┌───────────────────────────────────────────────────┐
│              KERNEL (single-threaded)              │
│  pop → assert consumers → reducers → handlers     │
│  ┌──────────┐  ┌──────────────────────────┐        │
│  │  Cache    │  │  Per-handler states      │        │
│  │ (keyed,   │  │  (S, T, U, ...)          │        │
│  │  read-    │  │                          │        │
│  │  only to  │  │                          │        │
│  │  handlers)│  │                          │        │
│  └──────────┘  └──────────────────────────┘        │
│  seq counter · clock · handler/producer registry   │
│  command/control path · boot FSM · trading_enabled │
└───────────────────────────────────────────────────┘
```

---

## 8. Invariants

| # | Invariant | Enforced |
|---|---|---|
| 1 | Every dispatched message has ≥1 consumer | Startup validation + kernel assert |
| 2 | Reducers apply before handlers for the same message | Kernel ordering |
| 3 | Reducers run in registration order; handlers run in registration order | Kernel ordering |
| 4 | Every handler declares what it produces | Required `.produces::<M>()` |
| 5 | Every declared production has ≥1 consumer | Startup validation |
| 6 | `ctx.send::<M>()` matches a declaration | Runtime panic on mismatch |
| 7 | Handlers are sync, no IO, no OS clock/RNG | `&Context` only; time/rng via ctx |
| 8 | Handlers cannot mutate the global cache; only reducers can | `&T` access in handlers |
| 9 | All IO / heavy compute runs off the main thread | Actors via `app.spawn()` |
| 10 | Time is monotonic; no event scheduled in the past | Heap + `send_at`/assert |
| 11 | Replay reproduces identical `seq` stream | Log + replay check |
| 12 | Same-instant graph is acyclic (or bounded) | Cycle guard |
| 13 | Kernel/handlers/strategy identical backtest vs live | Only clock, `send_at`, sources, venue differ |
| 14 | No trading before `Armed`; `trading_enabled` gates order production | Boot lifecycle + control flag |
| 15 | No global mutable state — multiple kernels per process | Kernel owns all state; NO `OnceLock`/global runtime |
| 16 | Domain primitives (`Price`/`Quantity`/`Timestamp`) panic on overflow/NaN/negative | Newtype checks |

---

## 9. The Message Graph

Required `.produces` declarations form a self-documenting graph validated at
startup: every declared production must have a consumer. Same-instant
cycles are detected (or bounded) and rejected.

```rust
app = Kernel::new()

    .state(Portfolio::new())
        .reduce::<Fill>(update_portfolio)
    .state(OrderBook::new())
        .reduce::<NewOrder>(track_order)
        .reduce::<Fill>(remove_filled)

    .state(SmaStrat::new())
        .on::<Bar>(generate_signals)
        .produces::<NewOrder>()
        .produces::<Signal>()

    .on::<NewOrder>(check_risk)
        .produces::<ApprovedOrder>()
    .on::<Reject>(log_reject)

    .route::<ApprovedOrder>(&exchange);

// Control plane
app.on::<Command>(handle_control_command);
```

Validation at `app.run()`:

1. Check every declared production against the consumer registry; panic on missing consumer.
2. Detect same-instant cycles; panic (or apply max-iterations bound at runtime).
3. Otherwise enter the loop.

---

## 10. Examples

### 10.1 SMA Crossover (with MVP-style instrument filter)

```rust
struct SmaStrat { instrument: InstrumentId, fast: SMA, slow: SMA }

app.state(SmaStrat { instrument: AAPL, fast: SMA::new(10), slow: SMA::new(30) })
    .on::<Bar>(|state, ctx, bar| {
        if bar.instrument != state.instrument { return; }     // MVP filter
        state.fast.update(bar.close);
        state.slow.update(bar.close);
        if state.fast.value() > state.slow.value() {
            let cl_id = ClOrderId(ctx.seq());                  // deterministic id
            ctx.send(NewOrder { id: cl_id, instrument: bar.instrument, side: Side::Buy, quantity: 100.into() });
        }
    })
    .produces::<NewOrder>();
```

### 10.2 State transitions via reducers

```rust
app.state(Portfolio::new())
    .reduce::<Fill>(|p, fill| {
        p.cash  -= fill.cost;
        *p.positions.entry(fill.instrument).or_default() += fill.qty;
    });

app.state(OrderBook::new())
    .reduce::<NewOrder>(|book, order| book.add_pending(order))
    .reduce::<Fill>(|book, fill|   book.remove_filled(fill.cl_order_id));
```

### 10.3 Simulated exchange with latency (backtest = live parity)

```rust
let exchange = app.spawn(|inbox, outbox, ctx| {
    for approved in inbox { // ApprovedOrder
        let fill = match_order(&approved);
        outbox.send_at(ctx.now() + LATENCY_NS, fill); // fill lands between bars, in order
    }
});

app.route::<ApprovedOrder>(&exchange);
```

### 10.4 Background compute actor

```rust
let model = app.spawn(|inbox, outbox, ctx| {
    for features in inbox { outbox.send_at(ctx.now(), expensive_predict(features)); }
});

app.on::<Bar>(|ctx, bar| ctx.send_to(&model, Features::from_bar(bar)))
    .produces::<Signal>();

app.on::<Signal>(|ctx, sig| {
    if ctx.trading_enabled() && sig.confidence > 0.8 {
        ctx.send(NewOrder { /* ... */ });
    }
})
.produces::<NewOrder>();
```

### 10.5 Kill-switch via control plane

```rust
app.spawn(ControlPlane::from_socket("/var/run/kavod.sock"));

app.on::<Command>(|ctx, cmd| match cmd {
    Command::Halt => {
        ctx.set_trading_enabled(false);
        ctx.send(CancelAll);
        ctx.send(FlattenAll);
    }
    Command::EnableAlgo => ctx.set_trading_enabled(true),
    _ => {}
});

app.on::<NewOrder>(|ctx, order| {
    if !ctx.trading_enabled() { return; }   // safe no-op; kill-switch is in effect
    // risk checks ...
})
.produces::<ApprovedOrder>();
```

---

## 11. Open Items / Not Yet in Scope

- **Fine-grained routing** (`RouteKey`/selectors, per-instrument dispatch) —
  see `route_key_design.md`. The MVP uses broadcast-by-`TypeId` dispatch
  with closure filters. Routing is the most likely next extension.
- **Actor lifecycle** — supervision, restart policies, health checks (beyond the boot/kill-switch hooks above).
- **Log format** — serialization/checkpointing details of the inbound log.
- **Backpressure policy** — bounded vs unbounded live channels, overflow behavior.
- **Fill-model library** — slippage/latency/partial-fill models for the sim exchange (the design supports them; the library is TBD).
- **Networking** — channel/mailbox implementation details.
- **Margin / buying power** — risk-layer subsystem (the seed is `RiskEngine` checks + `SimExchange` fees; needs to grow until it rivals LEAN's per-broker reality modeling).

---

## 12. Decisions to confirm

1. **Handlers get read-only cache access; all cache mutation goes through
   reducers.** Your original draft gave handlers `get_mut`. The MVP enforces
   read-only to keep the mutation surface explicit, auditable, and to
   preserve the reducer abstraction's value. If a real use case needs
   handler-side writes (e.g. local accumulators that must be globally
   visible), we can revisit — most such cases are better modeled as either
   per-handler state or as a new reducer keyed off a produced message.

2. **The cache is keyed** (`ctx.get::<T>(&key)`; singletons use `Key = ()`).
   This reconciles your `State::Key` trait with the accessor. The examples
   in §10 show keyed reads (`OrderBook` keyed by `cl_order_id`). If the
   cache is genuinely one-instance-per-type for everything, the `State::Key`
   trait can be dropped and reads simplified to `ctx.get::<T>()`.

3. **The state-mutation surface is reducers only**, even for
   reconciliation. Actors reconcile by emitting state-setting messages
   (e.g. `PositionSnapshot`) which run through reducers during
   `Reconciling`. No direct cache writes from actors.