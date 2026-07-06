# Kavod Routing Design (post-MVP)

**Status:** Extension, not required for the MVP kernel. The MVP kernel does
broadcast dispatch by `TypeId` only; handlers that need fine-grained
filtering (e.g. "only 1-minute bars for AAPL") receive every message of the
type and filter inside their closure. This document specifies how to upgrade
that to first-class, type-safe, per-key routing **without strings**, while
preserving the startup-validated produce/consume graph and the `.state /
.on / .produces` API shape.

The single most important rule of thumb that makes this work:

> **Route on stable, low-cardinality dimensions. Correlate dynamic, high-cardinality IDs through the keyed cache + reducers.**

Instruments, venues, timeframes, sides, strategies are stable. Order IDs
are dynamic. Never route on dynamic IDs.

---

## 1. Problem statement

Broadcast dispatch by `TypeId` is unsatisfying for medium-to-large universes
because:

- **Dispatch fanout.** Every `Bar` of every (instrument × timeframe) is
  delivered to every `Bar` handler, which early-returns. Cost grows as
  N(instruments) × M(timeframes) × H(handlers).
- **Invisible to the graph.** A handler that "consumes `Bar`" in the
  startup graph really consumes `Bar[AAPL, 1m]`. The validated DAG lies
  about the real wiring, and you cannot statically check that producer X has
  a consumer for `Bar[AAPL, 1m]` — only that someone consumes *some* `Bar`.
- **Stringly-typed alternatives are bad.** NautilusTrader routes on topics
  like `data.quotes.BINANCE.BTCUSDT-PERP`. Topic typos are silent
  non-delivery. We want the no-silent-drops / validated-graph property with
  typed keys instead.

The MVP closure-filter workaround is a legitimate escape hatch for the MVP
but should not be the *only* mechanism if routing will matter.

The kernel must stay domain-agnostic: it should not know what an
"Instrument" or a "Bar" is. All ergonomics live in the domain/wrapper layer.

---

## 2. Core concept

A **routing key** is a small struct of the dimensions anyone might dispatch
on. A **selector** is that same struct where each field is either `Exact(v)`
or `Any`. Dispatch = match a message's key against registered selectors.

```rust
trait Routed {
    type Key: Copy + Hash + Eq;
    fn route_key(&self) -> Self::Key;
}
```

Messages that don't implement `Routed` are broadcast (the MVP behavior,
unchanged). Messages that *do* name their routable dimensions and can be
indexed.

### Example keys (defined in the market-data / execution domain layers)

```rust
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct BarType   { instrument: InstrumentId, tf: Timeframe, dur: u16 }

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct QuoteKey  { instrument: InstrumentId, venue:   VenueId }

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct FillKey   { instrument: InstrumentId, venue:   VenueId, strategy: StrategyId }

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct OrderKey  { instrument: InstrumentId, venue:   VenueId, strategy: StrategyId }
```

A `Bar` is routed by `BarType`; a `Fill` is routed by `FillKey`. The kernel
sees only `Routed::Key` — it has no idea what an instrument or a venue is.

---

## 3. Selectors — the ergonomic layer

A selector is the routing key struct with every field wrapped in an
`Option<T>` where `None` means wildcard (`Any`):

```rust
#[derive(Default, Copy, Clone)]
struct BarTypeSelector {
    instrument: Option<InstrumentId>,
    tf:         Option<Timeframe>,
    dur:        Option<u16>,
}

impl BarTypeSelector {
    fn all() -> Self { Self::default() }                           // broadcast
    fn instrument(mut self, i: InstrumentId) -> Self { self.instrument = Some(i); self }
    fn tf(mut self, tf: Timeframe) -> Self       { self.tf = Some(tf); self }
    fn dur(mut self, d: u16) -> Self            { self.dur = Some(d); self }
}

impl BarType {
    fn selector() -> BarTypeSelector { BarTypeSelector::default() }
}
```

This is **domain sugar** that lives in the wrapper. Cosmetically:

```rust
BarType::selector()
    .instrument(AAPL)
    .tf(Timeframe::Minute)
    .dur(1)

// examples of partial matches:
BarType::selector().instrument(AAPL)               // any timeframe of AAPL
BarType::selector().tf(Timeframe::Minute).dur(1)  // 1m bars of everything
BarType::selector()                                // every bar
```

The whole router generalizes by deriving the selector from the key:
`Key` has N fields → `Selector` has N `Option`s; `Any` matches anything,
`Exact(v)` matches `v`.

---

## 4. Dispatch mechanism

Two-tier table per `TypeId`:

```rust
keyed:     HashMap<(TypeId, ErasedKey), Vec<HandlerId>>,   // fully-specified selectors, O(1)
wildcard:  HashMap<TypeId, Vec<(Selector, HandlerId)>>,     // any selector with ≥1 Any field
broadcast: HashMap<TypeId, Vec<HandlerId>>,                 // selectors that are all-Any, or unrouted types
```

For an incoming message of type `M` with key `k`:

1. Fire `broadcast[M]` (always; all-`Any` subscribers).
2. Fire `keyed[(M, k)]` (exact-match subscribers).
3. For each `(sel, handler)` in `wildcard[M]`, if `sel.matches(k)`, fire `handler`.

Selectors that are *fully* specified (all `Exact`) go into `keyed`. Selectors
with any `Any` field go into `wildcard` (and scan-and-match). For typical
small fan-outs the `wildcard` scan is cheap; if it ever matters, index the
wildcard list by its most-selective specified field (e.g. a
`HashMap<InstrumentId, Vec<(Selector, HandlerId)>>` for
`instrument`-pinned wildcards).

`ErasedKey` is the stored, type-erased form of the typed key (a `u64` or
box-erased `dyn Any`+`Hash`). The user-facing API stays typed; only the
registry interior is erased.

---

## 5. Registration API — three flavors over one primitive

All three desugar to the same keyed/broadcast registration. Pick whichever
reads cleanest in context; the kernel primitive underneath is identical.

### 5.1 Key-as-argument (smallest change to MVP shape)

```rust
app.state(SmaStrat::new())
    .on_for::<Bar>(
        BarType::selector().instrument(AAPL).tf(Timeframe::Minute).dur(1),
        |state, ctx, bar| { /* ... */ },
    )
    .produces::<NewOrder>();
```

`.on::<Signal>(...)` (no key) stays for broadcast types. Wildcards work the
same way: pass any selector with `Any` fields.

### 5.2 Stream handle (most readable for shared streams)

Declare the data stream once, subscribe by reference. Reads clean and
composes — multiple handlers share one declared stream handle:

```rust
let aapl_1m = app.stream::<Bar>(BarType::selector()
    .instrument(AAPL).tf(Timeframe::Minute).dur(1));

let spy_1m   = app.stream::<Bar>(BarType::selector()
    .instrument(SPY) .tf(Timeframe::Minute).dur(1));

app.state(SmaStrat::new())        .on(&aapl_1m, |s, ctx, bar| { /* ... */ }).produces::<NewOrder>();
app.state(SmaStrat::new())        .on(&spy_1m,  |s, ctx, bar| { /* ... */ }).produces::<NewOrder>();
app.state(PortfolioStats::new())  .on(&aapl_1m, |s, _, _| { s.observe() });   // share handle
```

The handle is essentially a `(TypeId, Selector)` pair wrapped in a typed newtype;
it carries no runtime cost at declaration, and the validation graph names
each named stream.

### 5.3 Per-key fan-out (the ergonomic jackpot for the common case)

"Run this strategy independently per instrument" in one line — routing *and*
per-instrument state isolation handled for you. Maps directly onto your
keyed `State::Key = InstrumentId`:

```rust
app.per_instrument(universe)                              // one SmaStrat per id
   .on::<Bar>(Timeframe::Minute, |state, ctx, bar| { /* state == SmaStrat for bar.instrument */ })
   .produces::<NewOrder>();
```

This is the virtual-actor / entity pattern (Akka sharding, Orleans grains)
made deterministic and single-threaded. The most common real strategy shape
collapses to one line at registration.

---

## 6. Actor routing (the same primitive)

The same keys route messages to actors point-to-point. Replaces the MVP
`app.route::<M>(&actor)` (broadcast) with keyed variants:

```rust
// route ApprovedOrders to the right venue by venue field:
app.route_for::<ApprovedOrder>(OrderKey::selector().venue(BINANCE),  &binance_actor);
app.route_for::<ApprovedOrder>(OrderKey::selector().venue(COINBASE), &coinbase_actor);
```

Multiple route registrations for the same type compose: the union of
matching selectors receives the message (the kernel asserts at least one
matches — no silent drops).

---

## 7. Worked example: orders and fills

Fills are the killer case because different consumers want *different*
dimensions of the same message:

```rust
// Portfolio wants EVERY fill (all-Any = broadcast).
app.state(Portfolio::new()).reduce::<Fill>(update_cash);

// Per-instrument position tracker — keyed by instrument, any venue/strategy.
app.per_instrument(universe)
   .reduce::<Fill>(|pos, fill| { pos.qty += fill.qty; });

// Strategy wants only its own fills — keyed by strategy id.
app.state(MyStrat::new())
   .on_for::<Fill>(FillKey::selector().strategy(my_id), |s, ctx, fill| {
       // react to my order's fill
   });

// Venue actor routing for order commands — keyed by venue.
app.route_for::<ApprovedOrder>(OrderKey::selector().venue(BINANCE),  &binance_actor);
app.route_for::<ApprovedOrder>(OrderKey::selector().venue(COINBASE), &coinbase_actor);
```

### Critical: do NOT route on dynamic IDs

`order_id` / `cl_order_id` are runtime-generated and unbounded. Routing on
them would mean a subscription per order and would destroy the static
validation. Correlating a `Fill` back to the `NewOrder` that created it is a
**state** problem, not a routing problem:

```rust
// OrderBook cache is keyed by cl_order_id. A reducer updates it.
app.state(OrderBook::new())
    .reduce::<NewOrder>(|book, o|     book.add_pending(o))             // key = o.id
    .reduce::<Fill>(|book, fill|       book.remove_filled(fill.cl_order_id));

// A handler that needs the originating order reads the cache:
app.on::<Fill>(|ctx, fill| {
    let order = ctx.get::<OrderBook>(&fill.cl_order_id).expect("prior NewOrder must have reduced");
    // ...
});
```

So routing fans out to interested handlers by **stable** dimension
(instrument, venue, strategy); the **keyed cache + reducers** handle
**dynamic** correlation. Keeping these two jobs separate is what stops the
routing layer from turning into a mess.

---

## 8. Validation interplay

The MVP strong guarantee — every produced message type has a consumer —
extends cleanly if you **register the universe at startup**:

- **Wildcard fields (`Any`) trivially satisfy connectivity** — they match
  anything.
- **Fully or partially pinned selectors over a registered universe** can be
  validated at startup over the finite `(type × pinned-key)` space:
  - "Every `ApprovedOrder[venue=X]` a strategy might produce has a route," for each `venue` you actually trade.
  - "Every `Bar[i, 1m]` for `i in universe` has a consumer," for each timeframe the data feeds will produce.

So you keep the strong startup guarantee, just extended across the known
key set. The kernel needs:
1. A *universe registration*: the set of `(InstrumentId, VenueId, Timeframe, …)` values that producers may emit.
2. A *producer declaration*: which keys (or selectors) each handler/actor may emit.
3. A *consumer declaration*: the existing `.produces` + selectors wired via `.on_for` / `.on(&stream)`.
4. A *connectivity check* over `(type × key)`: every `producer[type, selector]` is matched by some `consumer[type, selector]` under the universe.

This is a genuine advantage over Nautilus's runtime-only topic matching
and over Barter's closed `EngineEvent` enum.

When the universe is *not* registered up front (some discovery flows),
connectivity falls back to type-level validation + broadcast fallback
(`Any` selectors always match). That's weaker but still safe (no silent
drops; you know you dropped).

---

## 9. Deriving boilerplate away

The only domain-side cost is writing `Key`, its `Selector`, a `matches`
impl, and a `route_key` impl per message type. A derive removes it:

```rust
#[derive(RouteKey)]
struct BarType { instrument: InstrumentId, tf: Timeframe, dur: u16 }
// generates:
//   struct BarTypeSelector { instrument: Option<InstrumentId>, tf: Option<Timeframe>, dur: Option<u16> }
//   impl BarTypeSelector { fn matches(&self, k: &BarType) -> bool { ... } }
//   impl Routed for Bar { type Key = BarType; fn route_key(&self) -> BarType { self.r#type } }
```

`RouteKey` is a small, dependency-free proc-macro. One line per message type.

---

## 10. Migration path from MVP

The MVP and this extension compose cleanly, so there's no rewrite:

1. **MVP**: `app.on::<Bar>(...)` broadcasts (no `RouteKey`). Closures filter in-handler.
2. **Add `Routed`** for the types where routing matters first (Bar, Fill, ApprovedOrder).
   - `app.on::<Bar>(h)` continues to mean broadcast (i.e. an all-`Any` selector).
   - `app.on_for::<Bar>(sel, h)` is the new keyed entry.
   - `app.route::<ApprovedOrder>(&actor)` continues to broadcast; `app.route_for::<ApprovedOrder>(sel, &actor)` adds keyed actor routing.
3. **Add `per_instrument`** ergonomic for the dominant per-instrument state group pattern.
4. **Universe registration + keyed validation** enables the strong static-graph guarantee across the known key set — opt-in.
5. **`derive(RouteKey)`** removes the per-type boilerplate.

Each step is additive; nothing in the MVP becomes incorrect. The MVP closure
filter remains the escape hatch for ad-hoc conditions that the key system
can't express (e.g. "only bars where `open < close`" — that's a predicate,
not a routing key — and intentionally stays in user closures).

---

## 11. What NOT to do

These were considered and rejected:

- **Type-level instruments** (each symbol its own type / const generic). Maximally type-safe and zero-cost, but instruments are loaded at runtime from config/data, so you can't name them at compile time, and you'd get monomorphization bloat near-instantly.

- **Arbitrary predicate filters** (`.on::<Bar>().filter(|b| ...)`). Destroys the startup-validated static graph (you can't know statically what a predicate matches) and hurts determinism/analyzability. Prefer exact keys or selector key-sets over predicates.

- **Stringly-typed topics** (Nautilus-style). Typos are silent non-delivery; you can't derive `Hash`/`Eq` semantics cleanly across multiple hierarchy levels; loses the validated graph you've worked for.

- **Dynamic-ID routing** (`order_id` in the routing key). Subscriptions per order, no static validation, OOM risk on long runs. Use the keyed cache + reducers for that.

---

## 12. Open questions for this extension

- **Multi-dimension `Route` structs** (e.g. `{ venue, instrument }` with per-field wildcards). `BarType` is already multi-field and covered by selectors automatically. Cross-venue strategies may want explicit per-field wildcards like `Route::selector().venue(BINANCE)` — the derive supports this for free; just define the key struct with the fields you need.
- **Hierarchical wildcard buckets** (a small trie of selectors indexed by prefix for very large wildcard sets). Not needed at the MVP-extension scope; revisit if the `wildcard` scan ever shows up in a profile.
- **Composing selectors** at runtime (e.g. operator-driven runtime re-subscription). Out of scope of the static graph; would need a "live subscription" path that bypasses startup validation. Defer.

---

## 13. Summary

- Kernel stays domain-agnostic: it only needs `Routed::Key: Hash + Eq` and the selector/matches primitive.
- Domain layer (`BarType`, `FillKey`, etc.) declares the keys; a derive removes boilerplate.
- Three registration flavors (`on_for`, stream handle, `per_instrument`) desugar to the same keyed/broadcast primitive; MVP closure filter remains the ad-hoc escape hatch.
- Route on *stable* dimensions; *correlate dynamic IDs through the cache* — the hard and fast rule.
- Universe registration lets the startup-validated graph extend across the known key set — keeping the headline guarantee Kavod already promises.