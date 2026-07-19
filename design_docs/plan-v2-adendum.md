# Kavod v3 Addendum: Time, Sequence, and Acceptance

> **Status:** Draft for review
> **Supersedes:** design-v3.md §7.2, §12, §13, §15.12, §20.3–20.4, §22, §26.8 where they conflict with this document
> **Scope:** Conceptual model for engine time, sequence, single-site acceptance, source identity, sink submission, registration naming, and context capabilities. No implementation steps.

---

## 1. Motivation

design-v3.md correctly identifies that the kernel is a deterministic acceptance authority and that callbacks must not see wall clock or sequence. Two questions were left open or felt underspecified:

1. **What ticks the sequence, and where is time stamped?** §26.8 left the sequence increment policy open. §7.2 defined `dispatch_time` but the definition read ambiguously — "the scheduler timestamp of the message currently being handled" admits two interpretations (enqueue-time vs. dispatch-time) and obscured that there is exactly one stamping site.

2. **What is the right shape for the stamping site?** The existing implementation threads `&mut Sequencer` and `&mut Scheduler` separately through handler and actor output paths. This spreads acceptance across multiple call sites and obscures the invariant that seq, time, and (eventually) the ingress log record must be written atomically.

This addendum resolves both by introducing a single acceptance method — `Runtime::accept` — as the spine of the kernel. It also clarifies the vocabulary of time, introduces `SourceId` for replay and diagnostic attribution, unifies registration via a builder pattern with required names, and defines the `Sink` as the single submission surface for all productions.

The conceptual model draws on three industry references:

- **Aeron Cluster:** `clusterTime()` is frozen per log entry, stamped on the leader at append, carried in the log; the replicated state machine never reads wall clock; replay uses the same code path because time travels in the log.
- **LMAX Disruptor:** the business logic processor receives an ordered stream and never touches wall clock; `Sequence` is a ring-buffer cursor, not a business identifier.
- **NautilusTrader:** `TestClock` is pinned by the engine to the event's timestamp before each callback; `ts_event` (domain) and `ts_init` (engine/ingest) are distinct fields on every event. Live data is processed FIFO, not reordered by `ts_event`.

Kavod's existing model (message-centric frozen engine time, schedule sequence, domain time on payloads, arrival-order live ingress) is aligned with this consensus. This addendum sharpens the vocabulary and centralizes the stamping site; it does not change the determinism strategy.

---

## 2. The Spine: Single Acceptance Authority

The kernel is the single acceptance authority. Across backtest, live, and replay, every message that enters the scheduler passes through exactly one method:

```rust
impl Runtime {
    pub(crate) fn accept(
        &mut self,
        time: Timestamp,
        source: SourceId,
        msg: SharedMessage,
    ) -> Result<SeqNo, EngineError>;
}
```

`Runtime::accept` performs, atomically, in order:

1. Past-time validation: reject `time < runtime.dispatch_time`.
2. Sequence allocation: `sequencer.next()`.
3. Ingress log recordation (when the log is enabled): write `(time, seq, source, type_id, type_name, payload)`.
4. Scheduler push: `scheduler.push_shared(time, seq, msg)`.

"What ticks the sequence?" — `Runtime::accept`. "What stamps the time?" — `Runtime::accept`. "Where is the log written?" — `Runtime::accept`. There is no other site.

The scheduler remains a pure min-heap ordered by `(dispatch_time, SeqNo)`. It does not allocate sequence, validate past time, know about sources, or write logs. It is the downstream consequence of acceptance, not the acceptance authority.

### 2.1 Runtime Ownership

Conceptually:

```rust
struct Runtime {
    sequencer: Sequencer,
    scheduler: Scheduler,
    dispatch_time: Timestamp,   // engine logical now
    same_instant_count: usize,
    clock: Box<dyn Clock>,      // live ingress only; never in callbacks
    // log: IngressLog,         // when enabled
}
```

`Engine` owns `Runtime` and (in backtest) `Cache` as disjoint fields so that `&mut Runtime` and `&Cache` can be borrowed together during handler dispatch. In live, `Cache` lives on the logic thread; the kernel thread owns only `Runtime`.

---

## 3. Time Model

### 3.1 The Single Context Time

Every callback context exposes exactly one time:

```rust
ctx.now() -> Timestamp
```

`now()` is the **engine logical time of the current entry**, defined as:

> The timestamp stamped on this message at acceptance into the scheduler. It is copied into the context. It is identical for the reducer, handler, and actor stages of the same dispatched message. It does not advance while callbacks run.

This replaces design-v3.md §7.2's `dispatch_time()`. The value is the same; the name and definition are sharper.

### 3.2 What `now()` Is Not

- **Not wall-clock time.** Callbacks never observe wall clock. The wall clock lives on `Runtime`, is read exactly once per live ingress (external `push_event` and actor-output drain), and that single read becomes the value passed to `Runtime::accept`. From that point the value is frozen.
- **Not enqueue-time for `send_at`.** When a handler at `now = T` calls `ctx.send_at(T_future, msg)`, the message is accepted into the scheduler at `T_future`. When it later pops and dispatches, `now()` for its reducers/handlers/actors shows `T_future`. The enqueue-time `T` is internal scheduler bookkeeping and is not exposed on any context.
- **Not a clock object.** Contexts hold a copied `Timestamp`, not a `&dyn Clock`. There is no `Clock` trait in any public or callback-facing API. The shared mutable `Clock` pattern used by Nautilus is deliberately rejected: it weakens capability isolation and admits accidental wall-clock reads during callbacks.

### 3.3 Domain Time

Domain time (e.g., `exchange_time`, the timestamp at which an event occurred at the venue) lives **on the payload**, as a regular field of the user's message type. It is never used as a scheduler key. It is never reordered against in live mode. The vocabulary — borrowed from Nautilus's `ts_event` — is `event_time`: a documented convention for source-ingress message fields.

There is no `Message::event_time()` trait method. Each message type names its own domain time field. The convention is documentation, not a kernel contract.

### 3.4 Live Ingress Stamping

In live mode, the wall clock is read **exactly once** per ingress:

- External `push_event`: the kernel reads `clock.now()` at the moment of receipt. That value is passed to `Runtime::accept` as `time`. It becomes the message's schedule time and the `now()` observed by all callbacks when it dispatches.
- Actor output drain: the kernel-thread drain reads `clock.now()` at the moment it observes an actor emission. That value is passed to `Runtime::accept`.

The clock is never re-read inside a callback. The pinned value is the only time the callback observes.

### 3.5 Backtest Time

In backtest, the caller supplies `time` directly to `push_event`. The historical source is the clock. `Runtime::accept` does not consult `Runtime.clock` in backtest; it trusts the supplied time. The simulated clock advances only when the scheduler pops a later timestamp and `Runtime.dispatch_time` is updated.

### 3.6 Stable Time Across Stages

For one dispatched message:

```text
message accepted at T → seq allocated
message pops, dispatch_time = T
  reducer runs:  ctx.now() == T
  handler runs:  ctx.now() == T
  actor runs:    ctx.now() == T
```

This is the Aeron `clusterTime()` pattern and the Nautilus pinned-`TestClock` pattern. It is not a stale snapshot — it is the explicit statement: "given this event at engine time T, what do you do?" Wall-clock variation between stages (live lag, thread scheduling) must not affect decisions. If it did, live and backtest would diverge and determinism would be broken.

### 3.7 Immediate vs Future Production

| Call | Accepted at | Later `now()` when processed |
|---|---|---|
| `ctx.send(msg)` | current `now` | that same current `now` (later sequence) |
| `ctx.send_at(T_future, msg)` | `T_future` (if valid) | `T_future` |

Same-time productions receive a later sequence and therefore process after messages already queued at that time (breadth-first).

---

## 4. Sequence Model

### 4.1 One Sequence, One Ticksite

There is exactly one sequencer. It lives on `Runtime`. It is incremented **exactly once per message**, inside `Runtime::accept`. No other code path allocates sequence.

This closes design-v3.md §26.8: **invocations of reducers, handlers, and actors do not consume a separate trace sequence.** The minimum required policy — "allocate one unique sequence whenever a message is accepted for scheduling" — is the maximum policy in v1.

### 4.2 What Does Not Get Sequenced

- Reducer invocations
- Handler invocations
- Actor invocations
- Actor callback invocations within one actor
- Cache mutations
- Production-declaration checks

All of these are deterministic consequences of accepting one message. They do not need their own identity for scheduling; the schedule sequence of the accepted message is sufficient.

### 4.3 Why Sequence Exists

`dispatch_time` admits ties. Multiple messages may be scheduled at the same engine time (a backtest source pushing fifty bars at T=100; two handlers emitting into the same timestamp; two `send_at` calls colliding at a future T). The scheduler requires a deterministic, acceptance-ordered tiebreaker that does not pollute the timestamp's domain meaning.

Sequence is that tiebreaker. It is a `u64` monotonic counter, allocated at one site, never exposed to business code. Folding ordering into the timestamp itself (Nautilus's `AtomicTime` unique-nanosecond approach) conflates "when this fires" with "in what order this was accepted"; Kavod keeps these orthogonal.

### 4.4 Sequence Is Not a Business Identifier

Sequence is kernel-internal scheduling and replay infrastructure. It is not exposed on any context. It is not a field on `Message`. It does not appear in any public API.

If business code requires deterministic monotonic identifiers (e.g., order IDs), a dedicated `next_id()` facility should be designed separately (design-v3.md §26.9, still open). It must not reuse schedule sequence.

### 4.5 Sequence Is Not a Trace Sequence

An optional operation or trace sequence — recording each reducer/handler/actor invocation for fine-grained diagnostics — remains a future possibility. It is not in v1. It would live as a kernel-internal side channel, never on a context, never observable by strategy. It is not required for determinism: the ingress log of accepted messages plus the deterministic callback execution order is sufficient to reproduce any run.

---

## 5. Source Identity

### 5.1 SourceId

Every acceptance carries a `SourceId` identifying who produced the message:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SourceId(&'static str);
```

Baked at registration time. Stored on `ReducerEntry` / `HandlerEntry` / `ActorEntry`. Forwarded by `Sink::submit` to `Runtime::accept`. Stamped into every ingress log record.

`SourceId` is not a runtime index. It is derived from a user-supplied `&'static str` name; it survives code edits and builds. An index assigned at registration would drift across builds as users add registrations in the middle of their builder sequence; a `&'static str` name is durable and replay-stable.

### 5.2 Sources

| Producer | SourceId |
|---|---|
| External `push_event` | `"external"` |
| Reducer | — reducers cannot send |
| Stateless handler | the handler's configured name |
| Handler in a group | group name, or per-handler override, or fallback composition (see §7.3) |
| Actor | the actor's configured name |
| Actor callback | actor name, or per-callback override, or fallback composition (see §7.3) |
| Replay re-injection | `"replay"` (synthetic) |

### 5.3 What SourceId Is For

- **Ingress log attribution:** every recorded entry carries its source. Diagnostic queries ("did this `Fill` come from `actor:sim-venue` or `handler:reconciler`?") are answerable from the log alone.
- **Replay re-injection:** replay feeds recorded entries back through `Runtime::accept` with the original `time`, `seq`, `source`, and `payload`. The producer identity is preserved; only the re-injection path may use a synthetic `"replay"` marker as needed.
- **Diagnostics:** divergence analysis, audit, and observability all rely on knowing who produced what.
- **Future capabilities:** source-scoped metrics, per-source rate limiting, per-source replay filtering all become possible without redesign.

### 5.4 What SourceId Is Not

- Not user-visible as a type. Users set `.name("...")` on the registration chain; the `SourceId` is internal.
- Not a routing key. Messages dispatch by `TypeId`, not by source. Source is metadata, not a destination.
- Not a business identifier. It identifies the registration that produced a message, not an order or trade.

---

## 6. The Sink: Single Submission Surface

### 6.1 The Sink Trait

Callbacks that may produce messages (handlers, actors) submit emissions through a single internal trait:

```rust
pub(crate) trait Sink {
    fn submit(&mut self, emission: Emission) -> Result<(), EngineError>;
}

pub(crate) enum Emission {
    Immediate(SharedMessage),
    At { time: Timestamp, payload: SharedMessage },
}
```

`Immediate` corresponds to `ctx.send(msg)` — the kernel stamps the time at acceptance. `At` corresponds to `ctx.send_at(time, msg)` — the caller has requested a specific future time, which `Runtime::accept` validates.

The `Sink` is the anti-corruption layer between callback code (which knows about messages and produces) and the kernel (which alone allocates sequence, stamps time, writes logs, and pushes the scheduler). Callbacks never touch `Runtime`, `Sequencer`, or `Scheduler` directly.

### 6.2 Sink Implementations

There is one `Sink` impl per execution mode. The trait makes the callback code identical across modes.

- **Backtest `DirectSink`:** holds `&mut Runtime`, the frozen `now: Timestamp`, and the `source: SourceId`. `submit` calls `Runtime::accept(now, source, msg)` for `Immediate`, or `Runtime::accept(time, source, msg)` for `At`. Synchronous, same OS thread, zero channel overhead.

- **Live `ChannelSink`** (deferred to Design Gate A onward): holds a `Sender<KernelInbox>` and the `source: SourceId`. `submit` sends the emission across a channel to the kernel-thread drain. The drain reads `clock.now()`, calls `Runtime::accept(clock.now(), source, msg)` for `Immediate`, or `Runtime::accept(time, source, msg)` for `At`.

### 6.3 Why One Sink Across Modes

The user-visible API — `ctx.send(msg)`, `ctx.send_at(t, msg)` — is identical across modes. Only the `Sink` impl differs. This is the uniformity layer: the same handler and actor code runs in backtest and live without generics or compile-time mode branching at the call site.

The sink abstraction is `&mut dyn Sink` (trait object). The dynamic dispatch cost is negligible at any realistic scale and is paid only on actual emissions, not on every dispatched message. The Sink type is internal; users never name it.

### 6.4 Actor Output Is Always Through the Sink

design-v3.md §15.12 specifies: the actor does not assign kernel sequence, final live dispatch timestamp, or scheduler priority. The kernel assigns those when receiving the output.

The Sink makes this enforcement structural rather than conventional. Actors hold `&mut dyn Sink`; they have no path to `Runtime`. In backtest the sink delegates synchronously; in live it crosses a channel. In both, the actor cannot stamp or sequence — `Runtime::accept` on the kernel side does, once.

### 6.5 Reducers Have No Sink

`ReducerCtx` exposes no `send` / `send_at` and contains no `Sink`. This statically enforces design-v3.md §10.5: reducers mutate cache and produce no messages.

---

## 7. Registration: Uniform Builder Pattern with Required Names

### 7.1 Principle

Every consumer registration returns a builder/registrar that exposes a fluent chain for configuration. There are **no positional configuration arguments** on the registration call itself — the callback is the only positional argument (alongside initial state for stateful consumers). All meta-configuration, including `name`, lives on the chain.

This unifies the four consumer kinds (reducer, stateless handler, handler group, actor) under one shape and provides a single home for future meta-config (metrics, replay flags, priority, etc.) without re-shaping the API.

### 7.2 Required Name

Every top-level consumer must be named. `build()` rejects any registration lacking a name with `BuildError::MissingName`. The name becomes the consumer's `SourceId` (or, for handlers/actor callbacks within a group/actor, the seed for fallback composition; see §7.3).

Names are `&'static str`. Uniqueness is enforced at `build()` alongside all other graph validation, not at registration time. This consolidates validation in one place.

### 7.3 Fallback Composition for Nested Registrations

Handlers within a handler group, and callbacks within an actor, may optionally specify their own name for finer-grained source attribution via `.name("...")` on the chain returned by `group.on::<M>()` or `actor.on::<M>()`.

When a per-handler or per-callback name is omitted, the source is auto-composed at `build()` time as:

```text
{owner_name}:{consumed_type_short_name}
```

Where `consumed_type_short_name` is the last segment of `std::any::type_name::<M>()`. Examples: `sma-strategy:Bar`, `sim-venue:SubmitOrder`.

The composed name is stable across builds (the user-supplied owner name is a `&'static str`; the type name is determined by the Rust type system). The composition format is fixed and not user-controllable.

### 7.4 Registration Shapes

**Reducer:**

```rust
app.reduce::<Fill>(|ctx, fill| { ... })
    .name("portfolio");
```

Returns `ReducerRegistrar<'_>`. Chain exposes `.name("...")`.

**Stateless handler:**

```rust
app.on::<Bar>(|ctx, bar| { ... })
    .name("sma-strategy")
    .produces::<Signal>();
```

Returns `HandlerRegistrar<'_>`. Chain exposes `.name("...")` and `.produces::<T>()`.

**Handler group:**

```rust
app.handler_group(SmaState::new(), |g| {
    g.name("sma-strategy");

    g.on::<Bar>(|state, ctx, bar| { ... })
        .produces::<Signal>();

    g.on::<Reset>(|state, _ctx, _r| { state.reset(); });
});
```

`handler_group` takes `(state, configure)`. The group's `.name("...")` is called inside the configure closure. All handlers in the group share the group's `SourceId`, with per-handler `.name("...")` optional and fallback composition when omitted.

**Actor:**

```rust
app.actor(SimVenue::new(), |a| {
    a.name("sim-venue");
    a.inbox_capacity(4096);

    a.on::<MarketData>(|venue, _ctx, m| { venue.book.apply(m); });

    a.on::<SubmitOrder>(|venue, ctx, o| { ... })
        .produces::<Fill>();
});
```

`actor` takes `(state, configure)`. The name is set inside the configure closure via `a.name("...")` — not as a positional first argument. Per-callback `.name("...")` is optional on the chain returned by `a.on::<M>()`.

### 7.5 Requiredness Summary

| Node | Name required at | Where to set |
|---|---|---|
| Reducer | `build()` | `.name("...")` on returned `ReducerRegistrar` |
| Stateless handler | `build()` | `.name("...")` on returned `HandlerRegistrar` |
| Handler group | `build()` | `g.name("...")` in group builder closure |
| Handler in group | optional (fallback if omitted) | `.name("...")` on returned `HandlerRegistrar` |
| Actor | `build()` | `a.name("...")` in actor builder closure |
| Actor callback | optional (fallback if omitted) | `.name("...")` on returned actor registrar |

### 7.6 Internal Config Structs

Each consumer kind has an internal config struct stored on its entry in the registry. These structs are the home for all meta-configuration. Today they hold `name: SourceId` (and, for actors, the existing `inbox_capacity`). Future fields (metrics tags, replay flags, priority, etc.) are added to these structs and exposed via chain methods when justified.

```rust
pub(crate) struct ReducerConfig {
    name: SourceId,
}

pub(crate) struct HandlerConfig {
    name: SourceId,
}

pub(crate) struct ActorConfig {
    name: SourceId,
    inbox_capacity: Option<NonZeroUsize>,
    overflow_policy: Option<ActorOverflowPolicy>,  // when resolved
}
```

The structs are not public. Users compose configuration via the chain; the build process collects the configured values into the registry entries. Only `name` (and existing actor capacity) is exposed today. Speculative methods for metrics, priority, or mode flags are deferred until a concrete need arises.

---

## 8. Contexts

### 8.1 ReducerCtx

```rust
pub struct ReducerCtx<'a> {
    now: Timestamp,
    cache: &'a mut Cache,
}
```

Capabilities: `now()`, full cache read/write. No `send`, no `send_at`, no `Sink`, no source, no clock, no sequence. Statically enforces design-v3.md §10.5.

### 8.2 HandlerCtx

```rust
pub struct HandlerCtx<'a> {
    now: Timestamp,
    cache: &'a Cache,
    sink: &'a mut dyn Sink,
    source: SourceId,              // baked at registration
    produces: &'a ProductionSet,   // baked at registration
}
```

Capabilities: `now()`, immutable cache, `send`, `send_at`. Production declarations are enforced on every send. Source is not user-readable; it is forwarded to the sink on every emission.

### 8.3 ActorCtx

```rust
pub struct ActorCtx<'a> {
    now: Timestamp,
    sink: &'a mut dyn Sink,
    source: SourceId,
    produces: &'a ProductionSet,
}
```

Capabilities: `now()`, `send`, `send_at`. No cache. No sequence. No clock. No mode.

### 8.4 Context Comparison

| Capability | HandlerCtx | ReducerCtx | ActorCtx |
|---|---:|---:|---:|
| `now()` | Yes | Yes | Yes |
| Read global cache | Yes | Yes | No |
| Mutate global cache | No | Yes | No |
| `send` / `send_at` | Yes | No | Yes |
| Handler private state | Function parameter | No | No |
| Actor private state | No | No | Function parameter |
| Clock access | No | No | No |
| Sequence access | No | No | No |
| Scheduler access | No | No | No |
| Source (user-readable) | No | No | No |
| Mode access | No | No | No |

---

## 9. Topology

### 9.1 Backtest

- Single OS thread. Kernel, logic (reducers/handlers), and inline actors share the thread.
- `Engine` owns `Runtime` and `Cache` as disjoint fields.
- `Sink = DirectSink` wrapping `&mut Runtime` per dispatch phase.
- Actor callbacks run inline after handlers for each dispatched message (design-v3.md §15.9).
- No channels, no locks, no GIL.

### 9.2 Live (Conceptual; Deferred Past Design Gate A)

- **Kernel thread** owns `Runtime`. One inbox drains external ingress and actor outputs. For each item: stamps via `Runtime::accept`, then dispatches.
- **Logic thread** owns `Cache`, reducers, handlers, and handler-group state. Receives dispatch envelopes from the kernel. Runs reducers then handlers. Ships productions back via `Sink` to the kernel inbox.
- **Per-actor thread** owns actor state and callbacks. Receives fan-out from the kernel. Runs callbacks serially. Ships emissions back via `Sink` to the kernel inbox.
- Dispatch is strictly one-at-a-time at the kernel → logic boundary (preserves invariants: reducers before handlers before actor delivery; no overlapping dispatches of distinct messages).
- Actors within one dispatch may run in parallel (each on its own thread).
- Live late data is processed in arrival order. Domain time stays on the payload. There is no live reorder-by-`event_time`.

### 9.3 Cache Ownership

The logic side owns the cache (backtest: main thread; live: logic thread). The kernel never reads or mutates cache. Routing decisions use the validated graph (`has_consumer(TypeId)`), not cache state. This is the Aeron separation: consensus/sequencing is orthogonal to RSM state.

---

## 10. Ingress Log

### 10.1 Record Shape

Every acceptance may write a record:

```rust
pub(crate) struct IngressRecord {
    time: Timestamp,
    seq: SeqNo,
    source: SourceId,
    type_id: TypeId,
    type_name: &'static str,
    payload: SharedMessage,
}
```

Written in `Runtime::accept`, atomically with sequence allocation. The durable serialization format remains open (design-v3.md §20.6 / §26.7). The record shape is committed so the stamping site is ready without re-architecture.

### 10.2 What Must Be Logged

Every nondeterministic live ingress message must be recorded before dispatch:

- Live market-data source output
- Exchange and broker actor output
- External control input
- Other actor-produced messages entering the kernel

Handler and backtest actor productions that re-enter via the same `Runtime::accept` path may also be recorded for full causal tapes; the minimum required set is nondeterministic live ingress.

### 10.3 Replay

Replay feeds recorded entries back through `Runtime::accept` with recorded `time`, `seq`, `source`, and `payload`. Live external actors are not contacted. Kernel handlers and reducers execute normally. Deterministic actor implementations may be rerun; external actor-bound messages may be compared against expected recorded behavior.

The same code path runs in live and replay. Time is never regenerated from wall clock during replay — it travels in the log.

---

## 11. Explicit Non-Decisions

The following remain open or deferred and are **not** decided by this addendum:

| Topic | Status |
|---|---|
| Durable ingress log encoding / schema evolution | Deferred (§26.7) |
| Deterministic business ID facility | Open (§26.9) |
| Op / trace sequence for invocations | Rejected for v1; optional diagnostics later |
| First-class timer product (`set_timer`, recurring, cancel) | Deferred; may wrap `send_at` + timer messages |
| Live actor overflow policy | Design Gate A |
| Live channel topology (shared vs per-actor) | Design Gate A |
| Actor readiness / shutdown / supervision | Design Gate A |
| PTP / hardware timestamping | Out of scope |
| Live reorder-by-exchange-time | Explicitly rejected as kernel default |
| Shared `Clock` trait on callbacks | Explicitly rejected |

---

## 12. Mapping to design-v3.md

| design-v3 section | Relationship to this addendum |
|---|---|
| §7 Time | Superseded by §3 here: `now()` replaces `dispatch_time()`; frozen-at-acceptance definition; no clock on contexts |
| §12 Contexts | Superseded by §8 here: sink/source/produces on HandlerCtx and ActorCtx |
| §13 Scheduler | Clarified: pure heap; no seq allocation; `Runtime::accept` owns acceptance |
| §15.12 Actor output | Clarified: always through `Sink`; kernel stamps at accept |
| §20.3 Sequence increment policy | **Resolved** by §4: one seq per acceptance in `Runtime::accept` |
| §20.4 Ingress log | Record shape committed in §10; durable format still deferred |
| §22 Implementation structure | Conceptual `Runtime` + `Sink` replace scattered sequencer/scheduler borrows |
| §26.8 Sequence increment policy | **Closed** for v1: no invocation trace sequence |
| §26.9 Deterministic IDs | Still open |

---

## 13. One-Line Thesis

Kavod's deterministic core is a single acceptance site — `Runtime::accept` — that stamps time, allocates sequence, records ingress, and schedules; callbacks see only a frozen `now()` and submit productions through a mode-abstracted `Sink`; domain time stays on payloads; live and replay share one code path because time and order travel in the log, not in wall clocks or public sequence APIs.
