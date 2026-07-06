# Kavod Incremental Implementation Plan

## Overview

Each phase is independently testable with `cargo test`. The dependency graph flows:

```
Primitives → Messages → Clock → Seq+Log → Scheduler
                              → State+Cache → Context → Reducers → Handlers
                                                                       → Graph Valid → Kernel Loop
                                                                                      → Actors → Lifecycle → Backtest
```

---

## Phase 1: Domain Primitives

**Standalone, zero dependencies (except `rust_decimal`), immediately testable.**

- `Price(Decimal)` — non-negative, checked arithmetic
- `Quantity(Decimal)` — non-negative, checked arithmetic
- `Timestamp(u64)` — nanos since epoch, monotonic comparison
- `InstrumentId(u32)` — interning handle, `Copy + Eq + Hash`
- Add: `rust_decimal` crate

### Tests
- Construction from valid values
- Addition/subtraction overflow panics
- Negative value construction panics
- NaN construction panics
- Timestamp ordering (`ts1 < ts2`)
- `InstrumentId` equality and hashing

---

## Phase 2: Message Trait

**Trivial marker trait, enables the entire type system.**

- `pub trait Message: Send + 'static {}`
- A few example message structs for testing:
  - `Bar { ts: Timestamp, instrument: InstrumentId, open: f64, high: f64, low: f64, close: f64, volume: u64 }`
  - `Fill { ts: Timestamp, instrument: InstrumentId, qty: Quantity, price: Price, ... }`
  - `NewOrder { ... }`, `Signal { ... }`

### Tests
- Structs implement `Message`
- Downcasting via `TypeId` works
- Message can be boxed and sent across thread boundaries

---

## Phase 3: Clock Trait + Implementations

**Independent swappable component, testable in isolation.**

- `trait Clock: Send { fn now(&self) -> Timestamp; }`
- `LiveClock` — reads `std::time::SystemTime`, converts to nanos
- `SimClock` — `Cell<u64>`, manually advanced, default = 0

### Tests
- `SimClock` starts at 0, advances correctly via setter
- `LiveClock` returns a non-zero timestamp
- Trait objects (`Box<dyn Clock>`) work for swapping

---

## Phase 4: Sequence Counter + Inbound Log

**Determinism foundation.**

- `SeqNo(u64)` — monotonic, `next() -> SeqNo` increments
- `LogEntry { seq: SeqNo, ts: Timestamp, type_id: TypeId, payload: Box<dyn Message> }`
- `InboundLog` — `Vec<LogEntry>` with `append(entry)` and `iter()`
- Replay verification: feeding log back through produces identical seq stream

### Tests
- SeqNo monotonicity (each `next()` increases)
- Log append/iterate round-trip
- Replay produces identical entries

---

## Phase 5: Scheduler (Min-Heap)

**Drives the event loop. Time-ordered priority queue.**

- `Scheduler` wrapping `BinaryHeap<Event>` where `Event` orders by `(Timestamp, SeqNo)`
- `push(ts, seq, message)` — enqueue
- `pop() -> Option<(Timestamp, SeqNo, Box<dyn Message>)>` — earliest first
- `send_at(ts, message)` — future scheduling; panics if `ts < now`
- Same-instant BFS ordering: larger `seq` popped after everything already at that `ts`
- Max-iterations-per-instant bound to prevent infinite same-instant loops

### Tests
- Earliest timestamp popped first
- Same-timestamp tiebreak by seq (lower seq first)
- Future scheduling (event only pops when ts reached)
- Past scheduling panics
- BFS cascade: `A@T → B@T` resolves before `now` advances
- Max-iterations bound triggers

---

## Phase 6: State + Global Cache

**Typed, keyed storage layer.**

- `trait State: Clone + 'static { type Key: Hash + Eq; fn key(&self) -> Self::Key; }`
- `impl State for ()` for singleton types
- `Cache` — internal map of `(TypeId, u64)` hashed key → `Box<dyn Any>`
- Public API:
  - `cache.insert::<T: State>(value: T)` — upsert by key
  - `cache.get::<T: State>(key: &T::Key) -> Option<&T>` — read-only (handlers)
  - `cache.get_mut::<T: State>(key: &T::Key) -> Option<&mut T>` — mutable (reducers)
  - `cache.remove::<T: State>(key: &T::Key)`

### Tests
- Insert and retrieve by key
- Multiple instances of same type with different keys
- Singleton (`Key = ()`) access
- Missing key returns `None`
- `get_mut` actually mutates
- `get` returns immutable reference (compile-time checked)

---

## Phase 7: Context

**Narrowed kernel view passed to handlers. Bridges all subsystems.**

- `Context<'a>` holding references to:
  - `clock: &'a dyn Clock`
  - `cache: &'a Cache`
  - `seq: &'a Cell<SeqNo>`
  - `scheduler: &'a Scheduler`
  - `rng: &'a RefCell<impl Rng>`
  - `trading_enabled: &'a Cell<bool>`
- Methods:
  - `ctx.now() -> Timestamp`
  - `ctx.seq() -> SeqNo`
  - `ctx.send::<M: Message>(msg: M)` — produce message at current instant
  - `ctx.send_at::<M: Message>(ts: Timestamp, msg: M)` — schedule future message
  - `ctx.get::<T: State>(key: &T::Key) -> Option<&T>`
  - `ctx.rng() -> &mut impl Rng`
  - `ctx.trading_enabled() -> bool`
  - `ctx.send_to(&actor, msg)` — stub for Phase 12

### Tests
- `now()` returns clock time
- `send_at` with past `ts` panics
- `get` returns cached value
- `seq()` increments on subsequent calls
- `trading_enabled()` reflects flag state

---

## Phase 8: Reducers

**Cache mutation layer. Runs before handlers, produces no messages.**

- `Reducer<M: Message>` = `Box<dyn Fn(&mut Cache, &M)>`
- Registered per `(state_type, message_type)` pair
- Run in registration order for a given message type
- No access to context (no `send`, no clock — pure mutation)

### API
```rust
app.state(Portfolio::new())
    .reduce::<Fill>(|cache, fill| { /* mutate cache */ });
```

### Tests
- Reducer mutates cache on incoming message
- Multiple reducers for same message run in registration order
- Reducer cannot produce messages (compiler-enforced)
- Handler sees updated cache state after reducer ran

---

## Phase 9: Handlers

**Message consumer layer. May produce messages, may read cache.**

- `Handler<S: State, M: Message>` = `Box<dyn Fn(&mut S, &Context, &M)>`
- `.on::<M>(|ctx, msg| { ... })` — stateless handler
- `.state(S).on::<M>(|state, ctx, msg| { ... })` — with per-handler state
- `.produces::<M>()` — mandatory production declaration, stored in registry
- Handlers in the same `.state(S)` group execute in registration order
- `ctx.send::<M>()` must match a declared `.produces::<M>()` or panic at runtime

### Tests
- Handler fires on matching message type
- Per-handler state mutation persists between invocations
- Stateless handler works
- Missing `.produces` for `ctx.send` panics
- Undeclared `.produces` with no consumer caught later (Phase 10)
- Multiple handlers on same state group run in registration order

---

## Phase 10: Message Graph Validation

**Startup safety — structural faults caught before any capital is at risk.**

At `app.run()`:

1. Collect all `.produces::<M>()` declarations → set of `(handler_id, TypeId)`
2. Collect all subscriptions (`.on::<M>()`, `.reduce::<M>()`, `.route::<M>()`) → set of `TypeId`
3. For each produced `TypeId`, verify ≥1 consumer exists; panic on orphan
4. Build same-instant graph (edges where a handler produces `M` and another handler consumes `M` at same `ts`)
5. Detect cycles in same-instant graph (DFS); panic or warn with bounded max-iterations

### Tests
- Valid graph (every production has consumer) passes
- Missing consumer → panic with message naming the type
- Simple same-instant cycle (`A → B → A`) detected and rejected
- Long linear chain passes
- Actor route counts as consumer

---

## Phase 11: Kernel Event Loop

**The orchestrator. Ties scheduler + reducers + handlers + validation together.**

- `Kernel` struct owning:
  - `scheduler: Scheduler`
  - `cache: Cache`
  - `clock: Box<dyn Clock>`
  - `seq: Cell<SeqNo>`
  - `inbound_log: InboundLog`
  - Handler/reducer registries (built during construction)
  - `trading_enabled: Cell<bool>`
  - Lifecycle FSM (stub: Booting → Armed)
- `app.run()` entry point:
  1. Validate message graph (Phase 10)
  2. Enter single-threaded loop:
     - `pop` event from scheduler
     - Assert ≥1 consumer for event type
     - Run reducers for that type (in registration order)
     - Run handlers for that type (in registration order)
     - Messages produced via `ctx.send` / `ctx.send_at` re-enter scheduler
  3. Loop exits when scheduler is empty

### Tests
- End-to-end: `Bar → SMA handler → NewOrder → Risk handler → ApprovedOrder`
- Reducers run before handlers (confirmed via ordering assertion)
- Produced messages re-enter and get processed in same tick
- Same-instant cascade (`A@T → B@T`) completes before `now` advances
- Graph validation failure panics at `run()` (not earlier)

---

## Phase 12: Actors

**Off-main-thread components for IO and heavy compute.**

- `ActorHandle` — opaque handle for `send_to`
- `app.spawn(|inbox: Receiver<Box<dyn Message>>, outbox: Sender<(Timestamp, Box<dyn Message>)>, actor_ctx: ActorCtx| { ... })`
- Actor runs on a separate thread
- `inbox` — MPSC receiver; kernel/other actors send messages here
- `outbox` — sends `(timestamp, message)` back to kernel scheduler
- `ctx.send_to(&actor_handle, msg)` — point-to-point to actor
- `app.route::<M>(&actor)` — every `M` is also dispatched to this actor's inbox

### Tests
- Actor receives message sent via `send_to`
- Actor sends result back to kernel (appears in scheduler)
- `route::<M>` delivers every `M` to actor
- Actor runs on separate thread (can verify thread ID differs)

---

## Phase 13: Lifecycle + Control Plane

**Safety gating and operator control.**

### Lifecycle FSM
```
Booting → Reconciling → Armed
```
- **Booting:** graph validation, state construction, actor spawn
- **Reconciling:** actors reconcile external state; order-producing handlers are **inert**
- **Armed:** fully operational; orders may flow
- Transition: all registered venue actors report reconciled → `Armed`

### Control Plane
- `Command` message enum:
  - `EnableAlgo` — sets `trading_enabled = true`
  - `DisableAlgo` — sets `trading_enabled = false`
  - `CloseAllPositions` — sends `CancelAll` + `FlattenAll`
  - `Halt` — disables trading + cancel all + flatten all
- `ctx.set_trading_enabled(bool)` — mutates the kernel flag
- `ctx.trading_enabled() -> bool` — handlers gate order production on this

### Tests
- FSM transitions correctly
- `ctx.send::<NewOrder>()` in Reconciling is suppressed
- `trading_enabled = false` gates `ctx.send::<NewOrder>()` in Armed
- `Command::Halt` disables trading and queues cancel/flatten
- `Command::EnableAlgo` re-enables trading

---

## Phase 14: Backtest Substrate

**The first runnable mode. End-to-end deterministic backtesting.**

### Lazy Data Source
- `trait Source { fn next(&mut self) -> Option<(Timestamp, Box<dyn Message>)>; }`
- `CsvBarSource` — reads CSV, returns one `Bar` at a time, sorted by ts
- K-way merge: scheduler holds next pending event per source, pops from source on exhaustion

### SimExchange Actor
- Receives `ApprovedOrder`, model-matches (fill-or-kill, market order for MVP)
- Produces `Fill` via `outbox.send_at(ts + latency, fill)`
- Configurable latency model: `Fixed(u64)` or `Uniform(u64, u64)` nanos

### End-to-End Backtest
1. Load bar data from CSV
2. Register SMA crossover strategy
3. Register SimExchange actor
4. `app.run()` — processes all bars, generates fills
5. Collect results (PnL, positions)

### Tests
- SMA crossover backtest produces expected signals
- No look-ahead bias: fills land with latency, processed between bars in correct order
- Deterministic replay: same input → same output (byte-identical results)
- Empty data source → clean exit
- Multiple instruments interleaved correctly

---

## Phase 15: Live Substrate (Future)

**Identical kernel, different periphery.**

- `LiveClock` integration (already built in Phase 3)
- Feed actors pushing events into kernel (rather than being pulled)
- `BrokerExchange` actor (real FIX/REST connection) — stub initially
- Sandbox/paper trading mode: `LiveClock + real data feed + SimExchange`
- Crash-only recovery: restart reads inbound log, replays, reconciles

---

## Dependency Graph (Visual)

```
┌──────────────────────────────────────────────────────────────────────────┐
│  Phase 1: Domain Primitives                                              │
│  Phase 2: Message Trait                                                  │
└──────────────────────────────────────────────────────────────────────────┘
                    │
        ┌───────────┼───────────┐
        ▼                       ▼
┌───────────────┐       ┌──────────────────┐
│ Phase 3: Clock│       │ Phase 6: State    │
│               │       │ + Cache           │
└───────┬───────┘       └────────┬─────────┘
        │                        │
        ▼                        ▼
┌───────────────┐       ┌──────────────────┐
│ Phase 4: Seq  │       │ Phase 7: Context  │
│ + Inbound Log │       │                  │
└───────┬───────┘       └────────┬─────────┘
        │                        │
        ▼                        │
┌───────────────┐                │
│ Phase 5:      │                │
│ Scheduler     │                │
└───────┬───────┘                │
        │                        │
        └────────────┬───────────┘
                     ▼
            ┌──────────────────┐
            │ Phase 8: Reducers│
            └────────┬─────────┘
                     ▼
            ┌──────────────────┐
            │ Phase 9: Handlers│
            └────────┬─────────┘
                     ▼
            ┌──────────────────────┐
            │ Phase 10: Graph       │
            │ Validation            │
            └───────────┬──────────┘
                        ▼
            ┌──────────────────────┐
            │ Phase 11: Kernel     │
            │ Event Loop           │
            └───────────┬──────────┘
                        ▼
            ┌──────────────────────┐
            │ Phase 12: Actors     │
            └───────────┬──────────┘
                        ▼
            ┌──────────────────────┐
            │ Phase 13: Lifecycle  │
            │ + Control Plane      │
            └───────────┬──────────┘
                        ▼
            ┌──────────────────────┐
            │ Phase 14: Backtest   │
            │ Substrate            │
            └───────────┬──────────┘
                        ▼
            ┌──────────────────────┐
            │ Phase 15: Live       │
            │ Substrate (Future)   │
            └──────────────────────┘
```