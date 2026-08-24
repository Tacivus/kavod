# Environment Construction & Wiring — Option Survey (OPEN-1 / OPEN-2)

> **Status:** Survey, not a decision. Maps the option space for the two OPEN blocks in
> `design-final.md`; nothing here is chosen. Pseudocode is analyzed sketch, not
> implementation. Claims marked **(compile-proof)** rest on Rust type-system behavior
> this document has not compile-tested; treat each as a risk until a throwaway test
> confirms it.
> **2026-08-11 ecosystem sweep:** families C′ and J added; 3.K records the shapes
> rejected on sight; E6 added to §4 under the F12 partition; F9/F10 corrected against
> the amended design-final.md; 3.B's diagnostics cost dated; 3.D's receipt soundness
> conditions stated.
> **Scope:** builder/Slot binding, fan-in/fan-out placement, Error-sum composition,
> `LiveCtx`/`SimCtx` construction, `SimConfig`, graceful disposition, replay wiring —
> for both Environments, aiming at the one shared wiring answer OPEN-2 asks for.

## 1. The problem, decomposed

Both OPEN blocks reduce to five decisions plus peripheral knobs. Everything else in
§5/§6 is already fixed by invariants.

| # | Decision | Live | Sim |
|---|---|---|---|
| W1 | **Slot binding** — how the builder learns which Port fills which Slot, and how "every Slot bound exactly once" is proven | ✓ | ✓ |
| W2 | **Fan-in** — how each Port's `C::Event` gets wrapped into the App Event sum before admission | ✓ | ✓ |
| W3 | **Fan-out** — how a *generic* Environment destructures the *user's* Command enum to reach one typed destination | ✓ | ✓ |
| W4 | **Error sum** — how N heterogeneous Port Error types compose into one `E::Error` that lands in `EngineExit` | ✓ | ✓ |
| W5 | **Mode sharing** — how much of W1–W4 is written once and reused across live and sim | — | — |

Peripheral knobs (each one section below, none shape the families): capacities and
config structs, graceful disposition, `SimConfig` placement, replay wiring, thread
naming.

## 2. Forces

Facts that hold **regardless of which option is chosen** — derived from the frozen
design, the existing source, and Rust itself. The families in §3 are just different
ways of arranging these.

**F1 — Live erasure is free; the thread boundary does it.**
`LivePort::run(self, ctx)` consumes `self`, so `dyn LivePort` is not dyn-compatible
**(compile-proof)** — and it doesn't matter: the spawn closure `move || port.run(ctx)`
is already the erasure. The live Environment never stores a Port; it stores
`JoinHandle`s and channel ends. No family needs `Box<dyn LivePort>`.

**F2 — Sim erasure wants a boxed adapter trait.**
The sim scheduler needs uniform `&mut` access to N heterogeneous Ports (it steps them;
routing cannot capture them in a closure the scheduler also needs). `SimPort` methods
are `&mut self` with concrete arg types, so an object-safe internal adapter works:
`Box<dyn ErasedSimPort<AppEvent, AppCommand, PE>>` wrapping `(port, inject, err-map)`.
The adapter erases the per-Slot `Error` type via the captured map. Every family below
bottoms out here on the sim side; they differ only in how the adapter's pieces are
*supplied*.

**F3 — Fan-in constructors are already generated and already frozen.**
Tuple-variant constructors are fn items: `TradingEvent::Primary` *is* a
`fn(MdEvent) -> TradingEvent`. `PORT-ROUTING`'s "one frozen variant constructor per
inhabited direction" is satisfied by capturing that fn item at bind time; no new
generation is involved. Concretely it ends up as a captured closure inside each Port's
`LiveCtx` offer path (live) or adapter (sim) — uniform across families. The only open
question is how the builder *receives* it: explicit argument, Slot-token constant, or
macro plumbing. Note: `LiveCtx<C>` must not be generic over the App Event sum, so the
offer path is a boxed `dyn Fn(C::Event) -> Result<(), OfferRejected> + Send` capturing
constructor + queue handle **(compile-proof on the `Send` closure bound)**. The
rejected alternative, recorded: fan-in via `AppEvent: From<C::Event>` bounds
(libp2p's idiom for hand-written sums) — `From` is type-directed, so two
same-Contract Slots are indistinguishable; the variant constructor is value-directed
and is the fix. The same type-vs-value asymmetry recurs on the error side (§4, E2
vs E6).

**F4 — Fan-out is the real fork, and the Command enum is already a routing table.**
`TradingCommand::Primary(c)` names its destination in its discriminant. All W3
mechanisms are answers to one question: *who writes the match over the user's enum
that a generic Environment cannot write?*
  (a) the user, against typed send handles — compiler proves match exhaustiveness and
      payload agreement; arm-correctness stays a trusted per-Slot-tested obligation.
      This is `PORT-ROUTING` verbatim.
  (b) the user, as one prism per binding (`fn(Cmd) -> Result<C::Command, Cmd>`) —
      still hand-written, but no whole-match exhaustiveness proof; totality becomes a
      runtime build check.
  (c) a macro — amends `PORT-ROUTING`'s "hand-written" clause (§3.3, tier 2) and
      §3.2's "the macro generates no routing".
  (d) the type level (HList/Coproduct) — same amendment as (c) by other means.

**F5 — Slot order has exactly one defensible authority: enum declaration order.**
Registration call order must never matter (A1). Any family that registers
incrementally stores by a per-Slot index derived from declaration order; families that
bind via generated struct/method position get this for free. Today `ports!` generates
*nothing* that represents Slot identity (its expansion is exactly the two enums, and
§3.4 says "Nothing else is generated"), so **every family below except D-bare and G
requires extending `ports!` output** — a §3.4 (tier 3) edit, much cheaper than a
`PORT-ROUTING` (tier 2) amendment. Keep the two amendments distinct.

**F6 — `macro_rules!` cannot mint identifiers.**
Slot variants are `CamelCase`, and `macro_rules!` can neither case-convert nor
concatenate idents (`paste` did that; it is archived, successor `pastey` — either is
a dependency). What it *can* do: generate an enum **and** a match/impl over the same
variants, provided one invocation supplies the idents — which `ports!` does. So
generated per-Slot *methods or fields* must reuse the variant ident verbatim
(`#[allow(non_snake_case)]`, so `.Primary(port)` / `wiring.Primary = …`), take
user-supplied lowercase names in the `ports!` invocation, go positional (tuple
struct), or move to a proc-macro (amends §9's "no proc-macro crate"). This one
mechanical limitation is most of the ergonomic difference between families B/C and
A/D.

**F7 — Pure std suffices for every wait in §5; existing std channels do not.**
Every blocking point is "condition A OR condition B over state one lock can own":
select waits on latch-or-Event; a Port's `recv` waits on Command-or-lifecycle where
lifecycle must consume no capacity (`LIVE-LIFECYCLE`). `std::sync::mpsc` cannot
express either — a blocked `recv` wakes only for a message, total sender disconnect,
or timeout; std's `select!` was deprecated in 1.32 and removed without replacement.
So the channels are hand-rolled `Mutex`+`Condvar` structures (the textbook
two-condvar bounded queue; `Condvar::wait_while` takes an arbitrary predicate over
the locked state, so "Event OR latch OR lifecycle" is just fields plus `notify_all`)
— which need no multi-channel select at all. Perspective on the dependency question:
since Rust 1.67, std mpsc *is* vendored crossbeam-channel internally; adding the
crate would buy only the API std chose not to expose (`select!`, cloneable
receivers), not a better engine — and F7 shows the API isn't needed.

**F8 — The error-sum choice is visible in `EngineExit`.**
`E::Error` flows straight into `EngineExit<S, AF, EE>`. A single-parameter sum keeps
the exit type readable; a generated per-Slot-generic sum makes every `EngineExit`
mention N type arguments. This is the most user-visible consequence of W4.

**F9 — Shipped wiring lives inside the kavod crate; bespoke Environments are
user-land.** *(Corrected 2026-08-11 — the original claim, "`Timestamp::new` is
`pub(crate)`, only in-crate code can stamp time", predates the §10 answers.)*
design-final.md now ships public `Timestamp::from_nanos` (§2.1) and explicitly
sanctions bespoke third-party `Environment` impls (§6.2); this survey's own §7
replay resolution depends on both. What stands is §10's boundary: in the shipped
Environments `ENV-LATCH`/`ENV-CALLS` are enforced, and no shipped family may quietly
convert them into user obligations — which still disqualifies 3.F as a product (see
its entry).

**F10 — A public origin needs a public `Timestamp` constructor.** *(Resolved:
`Timestamp::from_nanos(u64)` is in design-final.md §2.1 — §10 answer 5.)*
`SimConfig { origin }` has its type; nothing further is forced. Kept as the record
of why the addition happened.

**F11 — `Send` bounds land only where values cross threads.**
Injectors, ports, and error-map closures cross into Port threads (live) and must be
`Send + 'static`. Anything invoked only from the Engine's thread — the router closure
of family D, prisms of family E, the whole sim Environment — needs no `Send`.
`SimPort`s may be `!Send`; the sim builder should not require it.

**F12 — Rust cannot mint a nominal type from N generic parameters without codegen.**
A library holding N heterogeneous types can only carry all N in its public
signatures, erase them, force them equal, or have the *user* name the sum and let
the library construct into it. §4's mechanism table is therefore a complete
partition, not an enumeration — E3 carries, E4 erases, E5 unifies, E1/E2/E6 delegate
the naming — and every ecosystem candidate lands in an existing cell (terrors
carries *and* erases; frunk's Coproduct carries structurally). There is no fifth
door; the 2026-08-11 falsification hunt (§4) found none.

## 3. Families

Coherent API shapes. Each: sketch, what is proven where, live/sim instantiation,
amendments required. The error mechanism (§4), disposition (§5), and replay (§7) are
orthogonal — any family composes with any of them.

### 3.A — Runtime-checked builder + generated Slot tokens

`ports!` additionally emits one zero-sized token per Slot implementing a kavod `Slot`
trait: `Contract`, `AppEvent`/`AppCommand`, `const INDEX`, `const NAME`, and
`fn inject(C::Event) -> AppEvent` (the captured F3 constructor).

```rust
let env = LiveEnv::builder(live_cfg)
    .bind(slots::Primary,   coinbase, BindCfg { inbox: nz(64) }, TradingErr::Coinbase)
    .bind(slots::Secondary, kraken,   BindCfg { inbox: nz(64) }, TradingErr::Kraken)
    .bind(slots::Execution, fix,      BindCfg { inbox: nz(16) }, TradingErr::Fix)
    .bind(slots::Timer,     timer,    BindCfg { inbox: nz(8)  }, TradingErr::Timer)
    .build()?;   // WiringError::Unbound(NAME) / DoubleBound(NAME)
```

`bind` erases immediately (F1/F2), storing by `INDEX`; call order is irrelevant (F5).
Totality and distinctness are **runtime** `build()` checks. Fan-out still needs an F4
answer — this family pairs naturally with F4(a) as family D's router, F4(b) prisms
passed as a fifth `bind` argument, or F4(c) generated `project` on the token.
Amendments: §3.4 token generation only (plus `PORT-ROUTING` iff the F4(c) pairing is
chosen).
*Precedent: the ecosystem's default shape — turmoil registers hosts by name on a
built `Sim` value, madsim via `NodeBuilder`, bevy via `add_systems`; totality is
runtime-or-absent in all of them (axum's missing route is a 404 at request time).*

### 3.B — Typestate builder (compile-time totality)

`ports!` generates a builder whose type parameters flip `Unbound → Bound` per Slot;
`build()` exists only when all are `Bound` **(compile-proof)**.

```rust
let env = Trading::live(live_cfg)          // TradingLive<U, U, U, U>
    .Primary(coinbase, BindCfg { .. }, TradingErr::Coinbase)   // <B, U, U, U>
    .Secondary(kraken, ..)
    .Execution(fix, ..)
    .Timer(timer, ..)
    .build();                               // only impl'd on <B, B, B, B>
```

Totality and no-double-bind proven at compile time; unbound Slot = "no method
`build`" error. That error's notoriety is dated (2026-08-11 sweep): gate `build` on
a `where State: IsComplete` bound trait instead of method absence and hang
`#[diagnostic::on_unimplemented]` (stable since 1.78) on it — bon v3 ships exactly
this recipe — and one bound trait per Slot makes the error name the missing Slot.
The remaining typestate cost is authoring effort, not user diagnostics. Method
names hit F6: `.Primary(…)` verbatim or proc-macro. Internally identical to A after
the types collapse. A const-generic re-skin — one `const BOUND: bool` per Slot — is
stable (const-builder, const_typed_builder) and still B; the single-bitmask version
needs `generic_const_exprs`, nightly and incomplete. Amendments: §3.4; same F4
pairing question as A.
*Precedent: typed-builder and bon (proc-macro typestate; "missing field ⇒ no
`build()`", bon adds nameable `IsSet`/`IsComplete` state traits), libp2p's
`SwarmBuilder` with ~10 phase types, and hand-rolled marker-type builders — the
last fully `macro_rules!`-expressible when names are supplied, which is F6's exact
boundary.*

### 3.C — Struct-literal wiring (construction as the totality proof)

`ports!` generates a wiring struct with one field per Slot; Rust's "all fields or no
struct" rule is the totality proof — no typestate machinery, no runtime check.

```rust
let env = LiveEnv::new(live_cfg, TradingLiveWiring {
    Primary:   LiveBinding::new(coinbase, nz(64), TradingErr::Coinbase),
    Secondary: LiveBinding::new(kraken,   nz(64), TradingErr::Kraken),
    Execution: LiveBinding::new(fix,      nz(16), TradingErr::Fix),
    Timer:     LiveBinding::new(timer,    nz(8),  TradingErr::Timer),
})?;
```

`LiveBinding<C, PE>` erases the Port at construction (boxed `FnOnce` shim + captured
err-map), so the struct is generic only over `PE`, not over N port types. A generated
`fn erase(self) -> Vec<Lane<…>>` attaches each field's injector positionally (F3 —
fan-in generation is invariant-compatible). Field names hit F6: verbatim
`non_snake_case` fields, or a tuple struct — positional, dodging F6 entirely, but two
adjacent same-Contract Slots can be swapped silently (exactly the trust class of a
wrong match arm, now without a per-arm name to review). Amendments: §3.4; F4 pairing
still open (a struct method `route` hand-written by the user pairs well).
*Precedent: the strongest in the survey. libp2p's `#[derive(NetworkBehaviour)]` is
this family at production scale: a struct of typed sub-behaviours, a generated event
enum with one variant per field wrapping in declaration order, `From` impls per field
when the user hand-writes the sum — and totality by construction (`E0063`), stated in
its docs' own terms via `with_behaviour(|_| MyBehaviour { ping, mdns })`. RTIC's
`init` returning `(Shared, Local)` is the same proof. libp2p needs its proc-macro
mainly for the field→variant case conversion that `ports!` sidesteps by naming Slots
explicitly (F6).*

### 3.C′ — Slot-trait wiring (trait-impl completeness)

C's trait-shaped dual (2026-08-11 sweep): a per-App trait with one body-less item
pair per Slot; writing the `impl` is the totality proof.

```rust
#[allow(non_snake_case)]
trait TradingLiveWiring {
    type Primary: LivePort<MarketData>;
    fn Primary(&mut self) -> Bound<Self::Primary>;   // Bound = port + BindCfg (+ err map)
    type Secondary: LivePort<MarketData>;
    fn Secondary(&mut self) -> Bound<Self::Secondary>;
    // … Execution, Timer
}
```

E0046 names *every* missing Slot in one error at the impl site; a double-bound Slot
is a duplicate-item error (E0201) **(compile-proof)**; impl items are unordered, so
F5 is free; items are keyed by name, so same-Contract Slots cannot collide. Two
things C cannot say: the binding is a *factory with a receiver* — matching
`run(self, ctx)`'s consumption, letting Live and Sim construct differently — and the
per-Slot associated type states payload agreement as a bound rather than leaving it
to field-type inference. The load-bearing rule: zero default bodies — a default
silently deletes that Slot's obligation, which is exactly why tonic *removed* its
generated `unimplemented!` stubs (hyperium/tonic#221) and made required-items the
default. F6 applies to item names as it does to C's fields; erasure still happens at
collection (F1/F2 unchanged). Fan-out unchanged — pairs with D; the tempting variant
where the same macro also emits the router match is G's `PORT-ROUTING` amendment
resurfacing, rejected on the same grounds. Amendments: §3.4 only.
*Precedent: tonic's generated service traits (above); tarpc `#[tarpc::service]`;
iced's `Application` (four required associated types, four required methods). This
is also the object-algebra / tagless-final shape under its academic name — Live and
Sim as two interpretations of one wiring trait.*

### 3.D — User-authored router against typed send handles

The `PORT-ROUTING`-verbatim family. The user writes the one exhaustive destination
match — as a closure, fn, or trait impl — against a mode-generic `Router` that only
exposes token-typed sends. Requires A's Slot tokens for the typed handle.

```rust
fn route<R: Router<Trading>>(cmd: TradingCommand, r: &mut R) -> Result<Routed, RouteError> {
    match cmd {
        TradingCommand::Primary(c)   => r.send(slots::Primary, c),
        TradingCommand::Secondary(c) => r.send(slots::Secondary, c),
        TradingCommand::Execution(c) => r.send(slots::Execution, c),
        TradingCommand::Timer(c)     => r.send(slots::Timer, c),
    }
}
let env = LiveEnv::builder(cfg).bind(…)… .router(route).build()?;
```

The compiler proves match exhaustiveness and payload agreement per arm; the trusted
residue is exactly `PORT-ROUTING`'s: an arm naming the wrong same-Contract token.
Refinement: `Routed` is a sealed receipt constructible only by `send`, and `send`
consumes `cmd` — so every control path must call `send` exactly once
**(compile-proof)**; "forgot to send" and "sent twice" both become type errors,
shrinking the trusted residue to wrong-token only.

The receipt's soundness conditions, stated because each is load-bearing (2026-08-11
sweep): `Routed` is a concrete struct with a private field — not a trait, so there
is no impl to fabricate — and neither `Clone`, `Copy`, nor `Default`; the Command
moves by value and is not `Clone`. Receipts are then created only by `send`, and
`mem::forget` can destroy one but never mint one, so conservation closes forgot-send
and double-send even against `forget` **(compile-proof)**. If `Routed` ever grew
`Clone`, a per-call invariant-lifetime brand (HRTB closure, no unsafe) would close
cross-call stashing; today the no-`Clone` rule alone does. Ceiling check: every
session-type crate surveyed (session_types, ferrite, rumpsteak) enforces linearity
only by a Drop-panic at *runtime* — rumpsteak's own papers retreat to "affine" — so
the receipt is the strongest exactly-once story stable Rust offers. The
anti-variant, recorded: send handles as *escaping values* (xtra's `Address<A>`
shape) — a held handle is a second handoff path outside `ENV-CALLS`' serial
discipline and `PORT-HANDOFF`'s single commitment point; `route` receiving `&mut R`
is precisely what keeps typed sends inside the dispatch turn.

The prize is W5: `route` is generic over `R: Router<App>`, so the *same hand-written
match* backs both Environments — live `send` = inbox admission (`LIVE-DISPATCH`), sim
`send` = adapter `on_command` invocation (`SIM-DISPATCH`). One match, per-Slot tested
once, two modes. Internally `send` reaches the lane by `INDEX` + downcast; a mismatch
is unreachable by construction (invariant panic, A8).
Amendments: §3.4 tokens only. `PORT-ROUTING` intact — arguably this family is that
invariant, spelled as an API.
*Precedent: ractor is the nearest shape (one message enum per actor, matched inside
one handler); stakker shows `macro_rules!`-only dispatch is viable when the user
names the target explicitly. No surveyed crate hands the user a typed router over
erased lanes — unsurprising, since no other design has a `PORT-ROUTING`-style
hand-written-match invariant to satisfy.*

### 3.E — Per-binding prisms

No whole match anywhere; each binding carries its own extractor, and the Environment
tries prisms in Slot order (or jumps via a generated discriminant index).

```rust
.bind(slots::Primary, coinbase, cfg, TradingErr::Coinbase,
      |cmd| match cmd { TradingCommand::Primary(c) => Ok(c), other => Err(other) })
```

Hand-written per Slot, maximally granular, no macro beyond A's tokens — but
exhaustiveness is gone: a Command matching no prism is a *runtime* `NoRoute`
Environment Error, and nothing stops two prisms from claiming overlapping variants
(first-accept-wins in Slot order). The weakest static story in the survey; listed
because it is the smallest incremental API and the closest to "one closure per
binding" from the prompt. Amendments: §3.4 tokens only.
*Precedent: `derive_more::TryInto` generates exactly these per-variant partial
extractions; frunk's `uninject` is the same operation with static indices. The
ecosystem uses them as conversions, not as routing tables.*

### 3.F — Toolkit / bring-your-own-Environment

Kavod ships the parts (fan-in queue, inbox, latch, supervisor shell, sim scheduler);
the user assembles their own `Environment` impl and hand-writes `dispatch`.
**Disqualified by F9 once** (corrected 2026-08-11): a kavod-*shipped* toolkit would
silently move `ENV-LATCH`/`ENV-CALLS` from "enforced" to "trusted", violating §10's
boundary sentence. The original second ground — user code cannot stamp `Timestamp`s
— no longer holds: `Timestamp::from_nanos` is public and design-final.md §6.2
explicitly blesses bespoke third-party `Environment` impls, which §7's resolution
relies on. Out as a product; endorsed as user-land — the §5.1 process-per-Port
stance. Recorded so the rejection is explicit, not an oversight.

### 3.G — Whole-topology macro

One declarative invocation, RTIC-style; the macro generates binding, routing, and the
error sum in one place.

```rust
kavod::live_env!(Trading {
    config: live_cfg,
    Primary:   { port: coinbase, inbox: 64, err: TradingErr::Coinbase },
    Secondary: { port: kraken,   inbox: 64, err: TradingErr::Kraken },
    Execution: { port: fix,      inbox: 16, err: TradingErr::Fix },
    Timer:     { port: timer,    inbox: 8,  err: TradingErr::Timer },
});
```

Because the expansion contains a real `match` over the App Command enum, an omitted
Slot is a non-exhaustive-match **compile** error and a duplicate is a duplicate-arm
error — compile-time totality without typestate, via `macro_rules!` alone. Costs:
the routing is generated (`PORT-ROUTING` amendment), the API surface is a macro
grammar rather than types (IDE/doc opacity, worst-in-class error locality), and the
Slot list is repeated (drift between `ports!` and `live_env!` is caught only by that
match). Amendments: `PORT-ROUTING` + §3.2's "generates no routing" (the clause is
about `ports!`; a second macro violates its spirit if not its letter).
*Precedent: RTIC's `#[app]` — a whole-program macro proving exactly-once resource
binding and per-task access at compile time. Its costs there are this row's costs:
closed world, a macro grammar as the API, generated names opaque to tooling.*

### 3.H — Type-level HList

Bindings as a heterogeneous cons-list (hand-rolled, ~100 lines, no dep);
the Environment is generic over the whole list, fully monomorphic, zero boxing;
totality and distinctness are trait-resolution proofs **(compile-proof)**. Requires a
generated Coproduct bridge from the Command enum (F4(d) — `PORT-ROUTING` amendment),
inflicts HList type names on every Environment type error, and makes
`Engine<A, E, W>`'s `E` a tongue-twister. Maximum static guarantees, maximum
opacity; the performance it buys (no `Box`, static dispatch) is irrelevant at
"one dispatch per Command per turn". Surveyed for completeness of the creative axis.
*Precedent: frunk's `Coproduct` (folding requires one handler per variant — a real
compiler totality proof); libp2p routes per-connection events positionally through
nested `Either`/`Select` types, so structural routing does ship at production scale —
and its ergonomics are that ecosystem's standing complaint.*

### 3.I — Mode-GAT shared bindings struct

The "write the topology once, instantiate per mode" dream:

```rust
trait Mode { type Port<C: PortContract>; }
struct TradingPorts<M: Mode> {
    Primary:   M::Port<MarketData>,
    Secondary: M::Port<MarketData>,
    …
}
```

The consequence chain kills it: `M::Port<MarketData>` must be *one* type, but Primary
and Secondary bind *different* concrete Ports of the same Contract → the GAT must be
a boxed trait object → `dyn LivePort` needs a `run(self: Box<Self>)` receiver
amendment (F1), and `dyn SimPort<C, Error = E>` pins one Error per Contract, forcing
error unification the other families avoid **(compile-proof chain)**. Documented
because it *looks* like the obvious W5 answer and is not; the shareable artifact is
smaller than the ports struct — it is the routing (family D) and the tokens (family
A), not the bindings.
*Precedent: TigerBeetle's `ReplicaType(StateMachine, MessageBus, Storage, AOF)` is
this idea, workable in Zig because comptime generics need no object safety — Rust's
dyn rules are exactly where 3.I dies. The ecosystem's other one-topology-two-modes
answer — madsim/shuttle/loom-style cfg swap with an ambient environment — is
unavailable by axiom: kavod's Environment is a value and hidden authority is banned
(§1.3).*

### 3.J — Coherence-keyed binding traits

The DI-container shape, absent from the original survey: kavod exposes a generic
`Binds<S: Slot>` trait; the user writes one impl per Slot on their wiring type; a
generated umbrella bound gates construction.

```rust
impl Binds<slots::Primary> for MyWiring {
    type Port = Coinbase;
    fn bind(&mut self) -> Bound<Coinbase> { … }
}
// generated: trait TradingBound: Binds<slots::Primary> + Binds<slots::Secondary> + … {}
```

Totality is one E0277 unsatisfied-bound error per missing Slot at the construction
call **(compile-proof)**; exactly-once is coherence itself — a second
`Binds<Primary>` impl for one type is E0119 **(compile-proof)**; impls are unordered
items, so F5 is free; Slot tokens are nominal keys, so same-Contract Slots cannot
collide. Disqualified for kavod by *eager construction*: the bound proves the impls
exist, not that anything calls them. The Environment needs all N lanes at build, and
the collector is either generated per-App (a §3.4 extension then doing the real
proving), hand-written and unproven (totality decays to A's runtime check), or made
total by consuming a complete record — which is C/C′ with extra steps. The DI
precedents never face this because their resolution is lazy, at use sites; kavod has
no lazy site. Second wrinkle: N `Binds<S>` impls cannot each consume one wiring
value by `self`, so bindings must be factories, not owned Ports.
*Precedent: shaku's `HasComponent<I>` — totality via call-site bounds, uniqueness
via coherence, keyed by interface type and therefore carrying exactly the
same-Contract collision Slot tokens would fix; waiter_di's
`#[provides(profiles::Dev)]` — provider impls parameterized by a profile type, the
mode-as-type-parameter precedent §11.5 records. teloc is family H wearing DI clothes
(its resolver is literally frunk's `Selector`); dill, syrette, and coi are family A
containers.*

### 3.K — Shapes rejected on sight (recorded per 3.F's precedent)

- **TypeId-keyed registry** (anymap, http `Extensions`, bevy Resources): type keys
  collide on same-Contract Slots, and `Extensions` *silently replaces* on duplicate
  insert — a worse failure than an error. Nominal keys fix the collision and leave
  A minus its totality check. Dominated.
- **Link-time / life-before-main registration** (linkme's `#[distributed_slice]`,
  inventory): registration decoupled from any value the Engine holds, with order
  and visibility as linker properties — the definitional ambient authority §1.3
  bans; proc-macro and platform-dependent besides.
- **Pre-spawned Ports** (user spawns, hands over channels/`JoinHandle`s): every
  supervision crate surveyed (bastion, supervisor, rust_supervisor) takes a
  factory, never a live handle — restart needs the recipe, and kavod's analogue is
  harder: `run(self, ctx)` would be consumed before the Environment could thread
  `ctx`, and a pre-spawned OS thread is meaningless in Sim, so W5 dies outright.
  Distinct from §3.2's user-*retained* handles for terminal Port state, which
  stand: the user keeps a handle; kavod keeps the spawn.
- **E0004 erased lane table** (enum-map's shape: generated `SlotId` enum, one total
  match building the lane array): compile-time totality with neither typestate nor
  struct-literal — but an enum-indexed array's value type cannot depend on the key,
  so Slot↔Contract agreement is severed (`Primary => secondary_lane` compiles), a
  miswiring channel no typed family has. Viable only beneath an already-typed
  layer, which then does the real work. (The crate itself is proc-macro + unsafe;
  the pattern, not the dependency, was evaluated.)
- **Escaping send handles** — see 3.D's anti-variant note: a user-held typed sender
  is a second handoff path outside `ENV-CALLS`/`PORT-HANDOFF`.

## 4. Error-sum mechanisms (orthogonal to family)

The OPEN text fixes the shape — Kavod-owned variants plus one mapped variant per
Slot's Port Error. Kavod-owned variants, derived from §5/§6: live —
`InboxFull{slot}`, `InboxClosed{slot}`, `PrematureClosure{slot}`, `SpawnFailed{slot}`,
`TimeDomainExhausted`; sim — `SimQuiescent`, `StepBudgetExhausted`,
`TimeDomainExhausted`. (Fan-in queue full is *not* here: `LIVE-EVENTS` reports it to
the offering Port.) The open question is the mapping mechanism:

| # | Mechanism | Shape | Cost |
|---|---|---|---|
| E1 | Map-fn at bind | `LiveError<PE>` single user param; `bind(…, map: impl Fn(P::Error) -> PE + Send)` | One closure per bind; `EngineExit<S, AF, LiveError<PE>>` stays readable |
| E2 | `From` bounds | `bind` requires `PE: From<P::Error>` | Zero args (thiserror-idiomatic); ambiguous when two Slots share an Error type — fixed by Kavod wrapping `Port { slot: SlotId, error: PE }` so `PE` never encodes the Slot |
| E3 | Generated per-Slot generics | `TradingLiveError<E1, E2, E3, E4>` | Precise; viral into every `EngineExit` mention (F8) — likely disqualifying |
| E4 | Erased | `Port { slot: SlotId, error: Box<dyn Any + Send> }` | No user sum needed; consumer must downcast — against A7's spirit (typed *inside*) |
| E5 | Forced unification | all Ports share one Error type | Pushes mapping into every Port impl; hostile to third-party Ports |
| E6 | Attributed constructor | `PE: FromSlotError<P::Error>` — `fn from_slot_error(slot: SlotId, e: P::Error) -> PE` | One impl per (PE, Port-Error-type) pair; the Slot travels as a value, so same-Error Slots share one impl and no wrapper reaches the exit type |

E6 (2026-08-11 sweep) is E2's shape with the attribution moved from a kavod-side
wrapper into a constructor *argument* — the ecosystem's standard fix for `From`'s
same-type ambiguity, value-directed exactly as F3's variant constructors are. It is
the only mechanism found that satisfies every constraint at once: no per-bind
closure argument (E1's wiring noise), no `Port { slot, error }` wrapper in the exit
type (E2's), no bounds owed by Port Errors or `PE`, and
`EngineExit<S, AF, LiveError<PE>>` shows one user-named parameter. Precedent:
winnow's `FromExternalError` (attribution as a constructor argument, no
`Display`/`Debug` owed by either side), snafu's `IntoError` selectors, serde's
`de::Error`. E1/E2/E6 are one family under F12 — the user names the sum — and E6
leads it. All three keep live and sim sums as *separate types* over one shared user
`PE` — consistent with §1.3's "mode-specific failure content may differ only inside
`EngineExit`". `SlotId` (a generated per-App Slot-name enum, or `INDEX` + `NAME`)
gives forensic attribution without touching the Journal (failures are never
serialized).

One freeze-time reconciliation, flagged: `PORT-ROUTING`'s letter places Error
mapping in the fan-out match — "each arm mapping its Port Error" — while E1/E2/E6
attach it at bind. Both sites are real (dispatch-path Errors arise in arms; latched
`run` Errors arise at supervision, off any arm), so freezing the mechanism must
either assign each site its owner or amend `PORT-ROUTING`'s wording to name the
mapping once.

Ecosystem check: **no surveyed crate composes a closed, typed error sum across
heterogeneous components** — a claim that survived a deliberate 2026-08-11
falsification hunt. kameo has per-actor `type Error` but supervision erases into a
downcastable `PanicError`; bevy's fallible systems land in `BevyError`, an explicit
anyhow-alike; smithy-rs generates closed per-operation sums, but from an IDL with
whole-world codegen and an open `Unknown` variant client-side. The structural
unions came closest and each concedes a constraint: terrors' `OneOf` is a
type-level set as a *proof layer* over `Box<dyn Any>` + downcast + `unsafe impl`
(E4 inside, E3-viral in signatures, type-directed so same-Error Slots need
newtypes); frunk's `Coproduct` is the one genuinely downcast-free, bounds-free
structural sum, and its nested type is *less* readable than E3's flat parameters;
error_set is a proc-macro generating user-side E2/E3 artifacts; `std::error::Request`
is nightly, lives on `dyn Error` (Display owed), and is a generalized downcast — E4
in politer clothes. The dominant idioms remain erased boxes plus downcast (tower
`BoxError`, libp2p `ConnectionDenied`, ractor's `ActorProcessingErr`) and
trait-classified errors (embedded-hal `Error::kind()`); generated closed sums
appear for *events* (libp2p's variant-per-field enum), never for errors. F12 says
why: without codegen there is no way to mint the nominal sum, so the ecosystem's
four doors are exactly E3, E4, E5, and the user-named family — kavod's departure is
choosing the fourth door and keeping it typed. Classification buys nothing here
because kavod never interprets Port Errors — it only carries them to `EngineExit`.

## 5. Graceful disposition (OPEN-1)

> **Resolved 2026-08-11: G4 plus a whole-shutdown deadline; design-final.md
> amended.** Disposition is Port-owned (signal delivered ahead of queued Commands;
> residue drained via `try_recv` or abandoned) — no Environment knob. The only
> Graceful configuration is `LiveConfig`'s `shutdown_deadline` (`NonZeroU64` ms):
> expiry detaches stragglers and returns `ShutdownTimeout { slot }` → Fatal. Abort
> and failed-`start` cleanup detach without waiting. See §5.1 for the settled model
> and the escalations rejected on the way. The options below stand as the record of
> the space.

`ENV-SHUTDOWN` (pre-amendment): Graceful "resolves the configured disposition of
already-handed-off Commands". Handed-off means *in an inbox, not yet received*. The
disposition is concretely the ordering of queued Commands vs the Graceful signal at
each Port's `recv`:

| # | Option | Semantics | Config surface |
|---|---|---|---|
| G1 | Signal-first, global | `recv` yields `Graceful` immediately; queued Commands dropped | one enum in `LiveConfig` |
| G2 | Drain-first, global | `recv` yields queued Commands, then `Graceful` | same enum |
| G3 | Per-Slot choice | G1/G2 per binding | field in `BindCfg` |
| G4 | Port-owned policy | always signal-first; a draining Port `try_recv`s the residue itself after seeing `Graceful` | none — the "configuration" is which Port code was bound |

G4 reads "configured disposition" as satisfied by Port authorship (the Port owns its
domain; whether an in-flight order-cancel must still go out is domain knowledge, which
is an argument that the Environment *shouldn't* own this knob). G1–G3 read it as an
Environment knob. G4 needs no config surface but must be stated in §5's semantics;
G2's drain-then-signal is the only option where `Graceful` delivery is delayed by
queue depth, which interacts with `BOUND-BLOCKING`'s "no wall-clock promise".
Precedent: ractor's mailbox delivers Signal/Stop ahead of queued user messages —
signal-first (G1/G4's delivery order) with the drain decision left to the actor.

### 5.1 Settled shutdown model, and rejected escalations (2026-08-11)

**Model.** Graceful: publish the signal (ahead of queued Commands, G4), wait at most
`shutdown_deadline` on the supervision shells' completion signals
(`Condvar::wait_timeout` — `JoinHandle` has no timed join), join finishers, detach
stragglers, `Err` precedence latched-Port-failure > `ShutdownTimeout{slot}` >
shutdown's own Errors. Abort: publish, close admission, drop handles, return —
no wait. Failed `start` cleans up with Abort's discipline. After a timeout or Abort
the process is **condemned**: the caller renders `EngineExit` and terminates
promptly; the durable forensic artifact is the Journal's committed prefix — the
same stance §1.5 takes for panics. This converts `BOUND-BLOCKING`'s shutdown-
cooperation *trust* into an enforced bound (A6) while leaving Port work itself
unbounded and trusted.

**Escalations examined for "exit returned ⇒ all threads dead", and why each fails:**

- **Fork isolation** (run the Engine in a child process, crash the child): dead on
  arrival — `EngineExit::Fatal { state, cause }` cannot cross a process boundary,
  because `State`, `A::Fatal`, and `E::Error` carry no `Serialize` bounds *by
  design* (A7; failures are never serialized), and the parent's copy of State is
  stale. Also: `fork` needs `libc` + `unsafe` against `#![forbid(unsafe_code)]`,
  forking a threaded process strands locks held by non-forking threads, Windows has
  no fork, and a library forking its host process is a layering violation.
- **Thread cancellation** (`pthread_cancel` / `TerminateThread`): unsound, not
  merely unsafe — a cancelled thread can die holding any lock, the allocator's
  included, poisoning the very process the kill was meant to clean; post-kill
  forensics is untrustworthy, defeating its own purpose.
- **Process-per-Port as a kavod feature**: puts serialization (and `Deserialize`
  bounds the contract deliberately lacks) on the hot path, hands Kavod-owned
  bounded inboxes to kernel pipe buffers (against A6's accounting-owner rule), and
  needs a process-spawning runtime std doesn't offer. Rejected as core machinery,
  **endorsed as a user-land pattern**: a proxy `LivePort` that spawns and owns a
  child process it can `kill()` — isolation bought exactly at the Port that wraps a
  hostile native SDK, by the user who chose it.

The underlying trilemma is universal — a thread in a blocking syscall can only be
waited on, abandoned to process death (tokio's `shutdown_timeout` makes the same
choice), or moved behind a process boundary the kernel can kill. The model picks
abandonment-with-deadline at the core and leaves the process boundary available
per-Port.

## 6. Peripheral config

- **Config structs rhyme with `EngineConfig`**: plain structs of `NonZero*` pub
  fields, first argument of the builder/constructor. `LiveConfig { event_queue,
  disposition? }`, `SimConfig { origin, max_steps_per_event }`. Per-Slot capacity
  (inbox) belongs in the per-bind `BindCfg`, not the global struct — the bound's
  accounting owner is per-Port (§1.6). Precedent: turmoil's `Builder` holds exactly
  the env-owned knobs (epoch, capacities, seed) and nothing engine-shaped.
- **`SimConfig` placement**: a sim-crate struct passed to the sim builder — *not* an
  `EngineConfig` field. §1.6's table already assigns the time domain and
  `max_steps_per_event` to the sim Environment; putting them in `EngineConfig` would
  move a bound across its accounting owner (A1). Requires F10's public `Timestamp`
  constructor.
- **Thread naming**: Slot idents are `&'static str` constants in every tokened
  family; `thread::Builder::name(format!("kavod-{slot}"))` at spawn, with an optional
  override in `BindCfg`. Decision-complete in one sentence; no family affects it.

## 7. Replay wiring (OPEN-2)

> **Resolved 2026-08-11: out of scope.** Kavod ships no replay mechanism. The
> determinism claim stays counterfactual (§1.3: *if* the same trace is presented,
> the same run results). Users wanting replay implement `Environment` directly
> (R2's shape) or write a trace-emitting `SimPort` (R1's shape); the options below
> stay as reference for them. Ripples for design-final.md: `SIM-COMPLETION`'s
> sentence "fixed-input replay wiring therefore accepts a constructor for that
> Event" and OPEN-2's replay bullet should be struck; and third-party `Environment`
> impls require a public `Timestamp` constructor (F10) — pending decision.

| # | Option | Shape | Fit |
|---|---|---|---|
| R1 | Provided `TraceSimPort<C>` | a kavod-shipped `SimPort` holding `Vec<(Timestamp, C::Event)>`; arms a wakeup per entry, emits in order | Per-Slot traces; the terminal Event is simply the last trace entry, satisfying `SIM-COMPLETION`'s constructor requirement with no extra machinery |
| R2 | Ports-free `ReplayEnv<AppEvent>` | a third `Environment` impl over `Vec<(AppEvent, Timestamp)>`; `dispatch` records Commands for assertion | The §10 conformance-suite vehicle: replays an *accepted trace* exactly, no Ports, no scheduler; `SimQuiescent`-equivalent when the trace ends without `Stop` |
| R3 | Journal-bytes replay | deserialize a recorded Journal back into Events | Requires `Event: DeserializeOwned` — a bound amendment the design deliberately avoided (`Serialize`-only); out unless the contract changes |

R1 and R2 are complements, not competitors: R1 replays *inputs into the sim
scheduler* (wakeup interleaving still exercised); R2 replays *the accepted trace
itself* (bit-exact §1.3 comparisons, and the natural home for "same trace twice →
identical Journal bytes"). R2's open sub-question is `dispatch` semantics: discard,
record for assertion, or compare against an expected Command trace and latch on
divergence.

Ecosystem check: wherever the sim owns randomness, **the seed is the trace** (madsim,
TigerBeetle, FDB, turmoil) and no event-trace API exists; shuttle replays *scheduling
decisions*, not events. Kavod has no ambient randomness, so its trace is real data —
the closest precedents are proptest-state-machine (a `Vec<Transition>` applied
stepwise against a reference model) and stateright (a counterexample `Path`
re-assertable by name), both R2-shaped. Separately, str0m's `Output::Timeout(Instant)`
— one next-deadline per machine serving as both timer registration and quiescence
signal — is `SIM-WAKEUP`'s single revocable arm, independently arrived at.

## 8. Comparison

| | A tokens | B typestate | C struct | C′ trait | D router | E prisms | G macro | H HList | I GAT |
|---|---|---|---|---|---|---|---|---|---|
| Totality proof | runtime | compile | compile | compile | runtime | runtime | compile | compile | compile |
| Fan-out author | (pairs w/ D/E) | (pairs) | (pairs) | (pairs) | **user, CT-exhaustive** | user, no proof | macro | type-level | macro/dyn |
| `PORT-ROUTING` intact | ✓ | ✓ | ✓ | ✓ | **✓ verbatim** | ✓ (weakened proof) | ✗ | ✗ | ✗ |
| Amendments | §3.4 | §3.4 | §3.4 | §3.4 | §3.4 | §3.4 | §3.2+3.3 | §3.2+3.3 | §3.3+F1 |
| `macro_rules!` feasible | ✓ | ✓ (F6 names) | ✓ (F6 names) | ✓ (F6 names) | ✓ | ✓ | ✓ | painful | ✓ |
| W5 sharing | tokens | per-mode builders | per-mode structs | per-mode impls | **one router, both modes** | per-mode prisms | two macros | per-mode | one struct (broken) |
| Error fit | E1/E2/E6 | E1/E2/E6 | E1/E2/E6 | E1/E2/E6 | E1/E2/E6 | E1/E2/E6 | any | E3-ish | forces E5 |
| API opacity | low | medium (authoring; diagnostics fixed, see 3.B) | low | low | low | low | high | very high | high |

Families compose: **A is the substrate** (tokens + runtime-checked builder), **D sits
on A** (the router is what the builder's `.router()` receives), **B/C/C′ replace A's
runtime totality check with a compile-time one** at F6's naming cost, **E replaces D**
at the cost of the exhaustiveness proof. G/H/I are complete alternatives. J and the
3.K shapes are absent from the table: J's compile-time totality collapses at eager
collection (its entry), and 3.K's entries are dominated on sight.

## 9. Direction (updated after the 2026-08-11 answers)

The MVP constraint from §10's answers — *every per-app artifact hand-writable now,
macro-able later* — selects the bundle the original leaning already favored:

- **A + D + E6 (E1/E2 as fallbacks)**: hand-written Slot tokens (~9 mechanical
  lines per Slot: a ZST, a `Slot` impl naming Contract/sums/`INDEX`/`NAME`, and
  `inject` wrapping the variant constructor), the hand-written router (which is
  `PORT-ROUTING` anyway), a hand-written user error enum with one `FromSlotError`
  impl per Port Error type (E6 — the Slot value disambiguates same-Error Slots
  inside the impl, so binds carry no mapping arguments at all). The builder and
  lanes are library code, written once in-crate. Nothing per-app needs any macro;
  every per-app item is exactly the mechanical shape a later `ports!` extension can
  emit — the same "hand-written equivalents are observationally identical"
  philosophy §3.2 already states for the enums.
- **Out for MVP**: B (hand-writing a typestate builder is precisely the ten-hour yak
  the answers reject, even with 3.B's diagnostics fix), G and H (the
  macro/type-machinery *is* the API), I (broken by the F1/dyn chain regardless), J
  (totality collapses at eager collection). **C and C′ remain the upgrade paths** if
  runtime totality checks ever bite — C's per-app cost is a plain struct plus an
  erase impl; C′'s is a trait impl whose E0046 error names every missing Slot and
  whose factory shape matches `run(self, ctx)` consumption; both hand-writable.
- All §10 questions are answered. The remaining OPEN-1/OPEN-2 work is freezing the
  A+D surface itself: `Slot` trait + token shape, `Router`/`send` signatures and the
  `Routed` receipt (with 3.D's soundness conditions), `LiveCtx` construction against
  the hand-rolled channels, `LiveConfig`/`SimConfig` fields, the two error sums, and
  the mapping-site wording `PORT-ROUTING` needs (§4's reconciliation flag) — plus
  compile tests for every **(compile-proof)** claim before design-final.md freezes
  them.
- Tier-3 reinforcements from the sweep, free at implementation time: the
  lane-collection site destructures its record with no `..` (E0027 — adding a Slot
  breaks the build loudly); inline-const asserts (stable 1.79) for
  per-instantiation const predicates; and a mode-generic builder *skeleton* is
  internally feasible — erased lanes are homogeneous per mode — the salvageable
  remainder of family I.

## 10. Questions — answers of 2026-08-11

1. **`PORT-ROUTING` / macros** — *answered:* macros are acceptable in principle, but
   the MVP must be fully hand-writable; no large macro investment for an API that
   may still change. Consequence: the chosen API must treat any macro as optional
   sugar over hand-writable items (§9). Families whose API *is* the macro or the
   type machinery (G, H) are out for MVP.
2. **Dependencies / proc-macro** — *answered:* no proc-macro for the MVP; only if
   objectively the best solution, and nothing in the chosen bundle needs one
   (F6/F7: `macro_rules!` suffices later, std suffices now).
3. **Replay** — *answered:* out of scope entirely; see §7's resolution note. Users
   build replay themselves against the public `Environment` trait — which raises
   F10/§7's ripple: public `Environment` implementors need a public `Timestamp`
   constructor (pending below).
4. **Graceful disposition** — *answered:* G4, extended with the whole-shutdown
   deadline and no-wait Abort (§5.1); `ENV-SHUTDOWN`, `LIVE-SHUTDOWN`, `LIVE-START`,
   `ENV-LATCH`, §4.2's shutdown/start rows, §5.2, and §1.6's live bounds row are
   amended in design-final.md accordingly.
5. **Public `Timestamp` constructor** — *answered:* `Timestamp::from_nanos(u64)`
   added to §2.1 of design-final.md, making external `Environment` impls possible
   and giving `SimConfig { origin }` its type.

## 11. Ecosystem synthesis

Per-family precedents are inline above; eight cross-cutting facts from the research
(original sweep: turmoil, madsim, shuttle, loom, TigerBeetle, FDB,
quinn-proto/str0m/firezone, stateright, libp2p, actix/ractor/kameo/stakker, bevy,
axum/tower, RTIC/embassy, typed-builder/bon; 2026-08-11 sweep adds
shaku/waiter_di/teloc/dill, tonic/tarpc/iced, xtra/coerce/riker/hannibal,
hecs/shipyard/legion, terrors/eros/error_set/frunk-Coproduct/snafu/winnow,
enum-map, linkme/inventory, session_types/ferrite/rumpsteak, bastion,
const_typed_builder/bon-v3):

1. **Dual-mode systems come in three shapes**: ambient environment behind a
   compile-time swap (madsim's package rename, shuttle/loom's cfg shadow modules,
   FDB's `g_network` global); environment-as-constructed-value with an explicit
   step loop (turmoil's `Sim`, TigerBeetle's `Cluster`); and environment-inverted
   sans-io (quinn-proto, str0m, stateright's actors), where the caller's loop *is*
   the environment. Kavod is axiomatically in the second family — the Environment is
   a value the Engine owns — wrapped around an Application that is already
   sans-io-shaped (time as argument, no IO, effects as returned Commands).
   Fittingly, explicit quiescence detection appears only in the value-style systems
   (turmoil's "all clients completed", TigerBeetle's `pending()` reason strings);
   `SimQuiescent` is in that company.
2. **Struct-literal totality is production-proven; typestate is proc-macro
   country.** libp2p and RTIC both ship "constructing the struct is the completeness
   proof" at scale; typed-builder/bon show typestate's ceiling and its ergonomic
   cost. Every proc-macro in the surveyed set earns its keep doing the one thing
   `macro_rules!` cannot: minting identifiers (F6).
3. **Closed sums are for events, not errors** (§4's ecosystem check). Generated
   variant-per-component event enums have a direct precedent (libp2p); kavod's typed
   error sum has none — A7 is a genuine departure, priced accordingly.
4. **Supervision-as-typed-value has precedent; restart does not apply.** ractor's
   `SupervisionEvent`, stakker's `StopCause`, and kameo's `ActorStopReason` all
   report component death as data — `LIVE-SUPERVISION`'s latch is conventional. The
   ecosystem's restart idiom (factory closures: turmoil `host`, madsim `init`) has
   no kavod analogue by design: first failure latches and wins (A4).
5. **Mode-as-type-parameter is shipped practice**: waiter_di's profile types select
   among provider impls at compile time — the one value-style
   one-topology-two-modes precedent found beyond cfg-swap, and the W5 story J and
   C′ would use.
6. **Required-items is a product decision, not an accident**: tonic removed its
   generated default stubs (hyperium/tonic#221) specifically so the compiler
   enumerates unbound handlers — E0046 as deliberate UX, C′'s precedent.
7. **Stable Rust's exactly-once ceiling is the receipt**: every session-type crate
   surveyed enforces linearity by runtime Drop-panic (rumpsteak's literature says
   "affine" outright); D's receipt + by-value Command is the strongest static form
   available.
8. **Type-keyed registries fail closed-world nominal topologies twice**: they
   collide on same-Contract Slots, and the ecosystem's canonical one (http
   `Extensions`) *silently replaces* on duplicate insert — nominal Slot keys are
   non-negotiable, which every surviving family already has.
