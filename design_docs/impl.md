# Kavod v7 Port Topology Implementation Design

> **Status:** Proposed implementation design for review
> **Semantic authority:** `design_docs/design-v7.md`
> **Scope:** Port Contracts, static Port Slots, generated Event and Command protocols, typed inboxes, Environment bindings, and Event selection
> **Priority:** The smallest Rust design that preserves v7 semantics without type erasure, trait-object dispatch, downcasting, or a user-visible routing key

---

## 1. Decision Summary

Kavod represents an application's complete static Port topology with one attribute macro declaration:

```rust
#[kavod::topology]
pub enum Ports {
    Primary(MarketData),
    Secondary(MarketData),
    Execution(Execution),
    Timer(Timer),
}
```

Each variant declares one logical Port Slot. Its field names one Port Contract, not an Event payload. Several Slots may use the same Contract. Declaration order establishes the frozen Slot order used for deterministic selection and manifest-local audit identity.

The macro generates five principal shapes:

```text
Ports          one source-qualified Port Event
PortsCommand   one destination-qualified Port Command
PortsBindings  all mode-specific Port implementations
PortsCapacities all frozen per-Slot inbox capacities
PortsQueues    all typed Event and Command inboxes
```

The first two are closed sum types. The latter three are product types. This distinction follows the runtime meaning:

- One Event comes from one Slot.
- One Command targets one Slot.
- Every Slot binding exists simultaneously.
- Every Slot capacity exists simultaneously.
- Every Slot owns its queues simultaneously.

Application code matches generated Event variants and stages generated Command variants. Environment construction receives one generated binding struct whose named fields make missing and duplicate bindings structurally impossible.

No Event or Command carries a separately supplied `SlotId`. The enum variant is its source or destination authority. Compact Slot ordinals remain private manifest-local metadata derived from variants and generated field correspondence.

## 2. Relationship To v7

This design implements the goals and invariants of `design-v7.md` with two deliberate semantic refinements that must be reflected in the next v7 revision if this design is selected.

First, Slot-to-application Event injection is total and generated. A Contract Event successfully removed from a Slot inbox cannot fail conversion into the topology Event enum. Encoding, capacity, clock, audit submission, and identifier failures remain reportable, but topology conversion is an invariant established by compilation rather than a runtime failure.

Second, `Environment::next_event` returns one owned, source-qualified Event candidate rather than a `NextEvent { slot: SlotId }`. Polling removes the selected inbox head before the post-poll Fatal boundary. The candidate is not accepted until that boundary admits acceptance. If Fatal wins, Kavod abandons the owned candidate with all other unaccepted Events.

This changes one capacity detail from v7: removing the candidate frees one Event-inbox position before Event acceptance. A concurrent live offer may use that position while the candidate waits at the post-poll boundary. This is intentional and must be tested as part of the selected inbox semantics.

All other relevant v7 rules remain unchanged:

- One accepted Event creates one synchronous turn.
- The Engine alone assigns Event index, freezes logical time, and establishes acceptance.
- The Slot queue and capability establish Event source authority.
- Commands remain deferred and ordered until handler return.
- Command preflight remains all-destinations-before-any-insertion.
- A Command is handed off only when inserted into its Slot's typed Command inbox.
- Live and simulation use the same Contracts, Slots, queue semantics, application Event and Command types, and Core turn protocol.
- All Core-managed storage is bounded and allocated before `RunStarted`.
- Fatal reporting, establishment, boundaries, and finalization remain those of v7.

### 2.1 Compile Feasibility Check

A manually expanded Rust 2024 prototype was compiled before finalizing this report. It validated:

- Associated Contract Event projections in generated Event enum fields.
- Associated Contract Command projections in generated Command enum fields.
- Two separate Slots using the same `MarketData` Contract.
- Other Slots using unrelated `Execution` and `Timer` Contracts.
- A generic `PortsBindings<...>` product with distinct field implementation types.
- Inference of `LiveEnvironment<PortsBindings<...>>` from a struct argument.
- Separate `LiveBindingSet` and `SimBindingSet` implementations over the same generated product.
- `AppEvent<Ports>` matching and `Context<Ports>::command(PortsCommand)` typing.

A negative compilation enabled a deliberately invalid Primary binding whose implementation provided `LivePort<Execution>` rather than `LivePort<MarketData>`. Rust rejected `LiveEnvironment::new(bindings)` because the generated binding product did not satisfy `LiveBindingSet`. This checks the central Contract-compatibility claim without relying on procedural-macro type introspection.

The check validates the language-level type relationships, not the unimplemented macro parser, queues, lifecycle, audit, or concurrency machinery. Those require the test plan in Section 28.

## 3. Requirements

The implementation must satisfy all of the following simultaneously:

1. A Contract defines one Event protocol and one Command protocol.
2. An application may instantiate one Contract in several distinct Slots.
3. Slots using the same Contract retain separate source, destination, capacity, queue, binding, and audit identity.
4. One application may combine Slots using unrelated Contracts.
5. Application Event handling and Command production remain statically typed.
6. Port implementations receive only their Contract Event and Command types.
7. A Port implementation cannot claim another Slot's Event source identity.
8. A Command destination cannot disagree with its payload type.
9. Live and simulated implementations may have different concrete Rust types.
10. Every declared Slot has exactly one compatible binding before start.
11. Core stores heterogeneous turn Commands without `dyn`, `Any`, or downcasting.
12. Environment runtime storage uses no erased implementation registry.
13. Runtime routing uses exhaustive generated matches and concrete field access.
14. No steady-state Core allocation is introduced.
15. Every loop and collection involved in topology processing has a declared bound.

## 4. Port Contracts

A Port Contract associates the complete Event and Command protocols used by one kind of boundary:

```rust
pub trait PortContract: Sized + 'static {
    type Event: 'static;
    type Command: 'static;
}
```

Concrete bounds such as `Send`, audit encoding, and maximum encoded length belong on the final API according to the queue and encoder implementations. They are omitted from small examples where they obscure the topology relationship.

For example:

```rust
pub struct MarketData;

impl PortContract for MarketData {
    type Event = MarketDataEvent;
    type Command = MarketDataCommand;
}

pub enum MarketDataEvent {
    Quote(Quote),
    Trade(Trade),
    Connected,
    Disconnected,
}

pub enum MarketDataCommand {
    Subscribe(InstrumentId),
    Unsubscribe(InstrumentId),
}
```

A Contract describes protocol meaning only. It owns no Slot identity, runtime implementation, queue, capacity, lifecycle state, or Environment mode.

One-sided Contracts use explicit Contract-owned uninhabited protocol types:

```rust
pub enum MetricsCommands {}
```

Using an uninhabited type preserves direction and protocol identity without special cases such as `Option`, `()`, or a global no-command marker shared across unrelated Contracts.

## 5. Topology Declaration

The application declares the complete ordered Slot set once:

```rust
#[kavod::topology]
pub enum Ports {
    Primary(MarketData),
    Secondary(MarketData),
    Execution(Execution),
    Timer(Timer),
}
```

The declaration has the following meaning:

| Variant | Contract | Frozen Slot ordinal |
|---|---|---:|
| `Primary` | `MarketData` | 0 |
| `Secondary` | `MarketData` | 1 |
| `Execution` | `Execution` | 2 |
| `Timer` | `Timer` | 3 |

The ordinal is checked, compact, and local to the frozen run manifest. It is not a user-authored ID, Rust enum discriminant, cross-build identity, or routing authority.

The macro rejects malformed declarations, including:

- Named-field variants.
- Variants with zero or several Contract fields.
- Explicit discriminants.
- Duplicate variant names, which Rust also rejects.
- Unsupported enum generics.
- A Slot count exceeding Kavod's compile-time maximum.

Contract compatibility is checked by generated Rust trait bounds, not by procedural-macro type inspection.

## 6. Generated Event Protocol

The macro rewrites the topology declaration into the complete source-qualified Port Event enum:

```rust
pub enum Ports {
    Primary(<MarketData as PortContract>::Event),
    Secondary(<MarketData as PortContract>::Event),
    Execution(<Execution as PortContract>::Event),
    Timer(<Timer as PortContract>::Event),
}
```

This is an ordinary closed Rust enum. It contains the complete concrete Event value and preserves Slot source identity even where two variants carry the same Contract Event type.

The generated topology implementation derives private metadata through exhaustive matching:

```rust
impl Topology for Ports {
    type Command = PortsCommand;
    type Queues = __KavodPortsQueues;

    const SLOT_COUNT: usize = 4;

    fn event_slot(event: &Self) -> SlotOrdinal {
        match event {
            Self::Primary(_) => SlotOrdinal::new(0),
            Self::Secondary(_) => SlotOrdinal::new(1),
            Self::Execution(_) => SlotOrdinal::new(2),
            Self::Timer(_) => SlotOrdinal::new(3),
        }
    }
}
```

The exact trait decomposition may differ. The required facts are that the topology has one generated Command type, one generated typed queue product, one checked Slot count, and exhaustive Event and Command classification. The binding product associates itself with `Ports` through `LiveBindingSet` and `SimBindingSet`; `Topology` does not need a variadic generic binding type.

Application-visible accepted Events may use a Core-provided closed outer protocol:

```rust
pub enum AppEvent<P: Topology> {
    Ready,
    Port(P),
}
```

For the example application:

```rust
type TradingEvent = AppEvent<Ports>;
```

Application handling is direct and exhaustive:

```rust
match event {
    AppEvent::Ready => initialize(state, ctx),
    AppEvent::Port(Ports::Primary(event)) => on_primary(state, event, ctx),
    AppEvent::Port(Ports::Secondary(event)) => on_secondary(state, event, ctx),
    AppEvent::Port(Ports::Execution(event)) => on_execution(state, event, ctx),
    AppEvent::Port(Ports::Timer(event)) => on_timer(state, event, ctx),
}
```

This instantiated enum is the application's closed AppEvent protocol required by v7. `Ready` remains the only Engine Event. All future application work still returns through a declared Port Event.

## 7. Event Source Authority

A bound Port never submits the generated `Ports` enum. Its run-scoped session accepts only its Contract Event:

```rust
impl LivePort<MarketData> for PrimaryFeed {
    fn run(self, session: LivePortSession<MarketData>) {
        session.offer(MarketDataEvent::Connected);
    }
}
```

The `Primary` binding owns a typed Event inbox containing `MarketDataEvent`. Only generated topology code may remove from that inbox and wrap the value as `Ports::Primary`.

The source-authority chain is therefore:

```text
Primary binding capability
-> Primary typed Event inbox
-> generated Primary dequeue arm
-> Ports::Primary(event)
-> accepted AppEvent::Port(...)
```

Neither the implementation nor the Event payload supplies source metadata. A Secondary implementation using the same Contract receives a different session and queue. It cannot offer into Primary's queue through safe public APIs.

The accepted envelope may store source explicitly for audit convenience or derive it from the closed AppEvent. If stored, construction is private and generated from the variant so disagreement is unrepresentable through public APIs. Derivation is preferable because it avoids duplicate runtime state:

```rust
match &envelope.event {
    AppEvent::Ready => EventSource::Engine,
    AppEvent::Port(event) => EventSource::Port(Ports::event_slot(event)),
}
```

## 8. Generated Command Protocol

The macro generates one complete destination-qualified Command enum:

```rust
pub enum PortsCommand {
    Primary(<MarketData as PortContract>::Command),
    Secondary(<MarketData as PortContract>::Command),
    Execution(<Execution as PortContract>::Command),
    Timer(<Timer as PortContract>::Command),
}
```

Application code stages a complete directed value:

```rust
ctx.command(PortsCommand::Primary(
    MarketDataCommand::Subscribe(instrument),
));

ctx.command(PortsCommand::Execution(
    ExecutionCommand::Submit(order),
));
```

The destination and payload cannot disagree. For example, the following does not type-check:

```rust,compile_fail
PortsCommand::Timer(ExecutionCommand::Submit(order))
```

Context is generic over the frozen topology and accepts only its generated Command protocol:

```rust
pub struct Context<'a, P: Topology> {
    // Private turn-local authority and storage.
}

impl<P: Topology> Context<'_, P> {
    pub fn command(&mut self, command: P::Command) {
        // Encode and stage without Port insertion or IO.
    }
}
```

The turn-local bounded sequence stores one concrete type:

```rust
BoundedVec<PortsCommand>
```

This is not type erasure. Every payload remains a concrete enum field, exhaustive matching is compiler checked, and no downcast exists.

## 9. Generated Binding Product

All Port implementations exist simultaneously, so the macro generates a product type with one named field per Slot:

```rust
pub struct PortsBindings<
    PrimaryImpl,
    SecondaryImpl,
    ExecutionImpl,
    TimerImpl,
> {
    pub primary: PrimaryImpl,
    pub secondary: SecondaryImpl,
    pub execution: ExecutionImpl,
    pub timer: TimerImpl,
}
```

Live construction is ordinary struct construction:

```rust
let bindings = PortsBindings {
    primary: PrimaryFeed::new(),
    secondary: SecondaryFeed::new(),
    execution: Broker::new(),
    timer: SystemTimer::new(),
};

let capacities = PortsCapacities {
    primary: SlotCapacity::new(4_096, 128),
    secondary: SlotCapacity::new(4_096, 128),
    execution: SlotCapacity::new(1_024, 256),
    timer: SlotCapacity::new(1_024, 256),
};

let environment = LiveEnvironment::new(bindings, capacities)?;
```

The inferred binding type is:

```rust
PortsBindings<PrimaryFeed, SecondaryFeed, Broker, SystemTimer>
```

The complete Environment type is concrete:

```rust
LiveEnvironment<PortsBindings<PrimaryFeed, SecondaryFeed, Broker, SystemTimer>>
```

Users ordinarily rely on inference. An embedding program may define a type alias when it needs to name the Environment type.

The struct gives compile-time binding completeness:

- Every declared field must be initialized.
- A field cannot be initialized twice.
- No undeclared field can be supplied.
- Field identity does not depend on literal order.
- Rust reports missing and unknown fields directly.

This eliminates the tuple design's positional meaning, tuple-arity implementations, runtime duplicate checks, runtime missing checks, and O(N) binding lookup.

## 10. Binding Compatibility

The generated binding product implements live and simulation binding contracts under different concrete bounds:

```rust
impl<PrimaryImpl, SecondaryImpl, ExecutionImpl, TimerImpl> LiveBindingSet
    for PortsBindings<PrimaryImpl, SecondaryImpl, ExecutionImpl, TimerImpl>
where
    PrimaryImpl: LivePort<MarketData>,
    SecondaryImpl: LivePort<MarketData>,
    ExecutionImpl: LivePort<Execution>,
    TimerImpl: LivePort<Timer>,
{
    type Topology = Ports;
}
```

```rust
impl<PrimaryImpl, SecondaryImpl, ExecutionImpl, TimerImpl> SimBindingSet
    for PortsBindings<PrimaryImpl, SecondaryImpl, ExecutionImpl, TimerImpl>
where
    PrimaryImpl: SimPort<MarketData>,
    SecondaryImpl: SimPort<MarketData>,
    ExecutionImpl: SimPort<Execution>,
    TimerImpl: SimPort<Timer>,
{
    type Topology = Ports;
}
```

A wrong implementation type fails at compile time when Environment construction requires the applicable binding-set trait:

```rust,compile_fail
let bindings = PortsBindings {
    primary: Broker::new(), // Broker is not LivePort<MarketData>.
    secondary: SecondaryFeed::new(),
    execution: Broker::new(),
    timer: SystemTimer::new(),
};

let environment = LiveEnvironment::new(bindings, capacities)?;
```

One implementation type may occupy several compatible fields as separate values. Shared implementation state across several logical endpoints is deferred unless a concrete simulation requirement selects a grouped-binding API. It must not weaken one binding per Slot, typed endpoint authority, or deterministic callback ordering.

## 11. Generated Queue Product

The macro first generates one shared capacity product for live and simulation:

```rust
pub struct PortsCapacities {
    pub primary: SlotCapacity,
    pub secondary: SlotCapacity,
    pub execution: SlotCapacity,
    pub timer: SlotCapacity,
}
```

Each `SlotCapacity` contains that Slot's Event and Command item capacities and any required byte bounds. Struct construction requires one capacity entry per Slot without a map, string, ordinal, or user-visible key. The same deterministic capacity value can be supplied when constructing either Environment mode.

Although examples pass this value to `LiveEnvironment::new`, it belongs to the frozen Application and Engine construction configuration required by v7, not to implementation-private live behavior. Live and simulation construction consume the same generated shape and cannot silently invent different Slot capacities.

Every Slot owns one typed Event inbox and one typed Command inbox. The topology macro generates their concrete product shape:

```rust
#[doc(hidden)]
pub struct __KavodPortsQueues {
    primary: SlotQueues<MarketData>,
    secondary: SlotQueues<MarketData>,
    execution: SlotQueues<Execution>,
    timer: SlotQueues<Timer>,
}
```

The generated queue product must be public enough to appear as the associated `Topology::Queues` type across crate boundaries. Its fields and constructors remain private, and its name is hidden from normal documentation. Application code cannot construct, replace, or access queue fields.

Conceptually:

```rust
struct SlotQueues<C: PortContract> {
    events: BoundedFifo<C::Event>,
    commands: BoundedFifo<CommandEnvelope<C::Command>>,
}
```

Exact concurrent queue types and endpoint splitting remain implementation details. The ownership rules are not:

| Queue endpoint | Producer | Consumer |
|---|---|---|
| Slot Event inbox | Bound Port session | Environment selector acting for Core |
| Slot Command inbox | Core | Bound Port implementation |

The queue product is allocated from `PortsCapacities` and fully validated during Engine construction before `Environment::start`. Queue fields never grow after `RunStarted`.

Using separate fields preserves per-Slot capacity even where several Slots share a Contract. Primary pressure cannot consume Secondary capacity.

## 12. Environment Shape

The common Environment contract becomes topology-associated and returns one owned Port Event:

```rust
pub trait Environment {
    type Topology: Topology;
    type StartError;
    type StopError;

    fn start(&mut self) -> Result<(), Self::StartError>;
    fn next_event(&mut self) -> Option<Self::Topology>;
    fn now(&self) -> LogicalTime;
    fn stop(&mut self) -> Result<(), Self::StopError>;
    fn abort(&mut self);
}
```

Here the generated `Ports` Event enum itself implements `Topology`, so `Option<Self::Topology>` is the complete source-qualified candidate. An equivalent decomposition may use `Topology::Event`; it does not change the design.

The live Environment is generic over one concrete generated binding product:

```rust
pub struct LiveEnvironment<B>
where
    B: LiveBindingSet,
{
    bindings: B,
    queues: <B::Topology as Topology>::Queues,
    // Clock, notifier, lifecycle, and selection state.
}
```

The simulation Environment has the same application-facing topology and queue protocols but different private scheduling state:

```rust
pub struct SimEnvironment<B>
where
    B: SimBindingSet,
{
    bindings: B,
    queues: <B::Topology as Topology>::Queues,
    // Virtual clock, cursors, callback counters, and selection state.
}
```

Engine is generic over the concrete Environment. Calls use monomorphized static dispatch. If an embedding executable must choose live or simulation at runtime, it may use one closed application-specific enum and exhaustive delegation; Kavod does not require a trait object.

## 13. Event Polling And Acceptance

One admitted Environment poll performs bounded work and returns at most one owned candidate:

```text
inspect runnable sources under the frozen selection policy
-> select at most one Slot
-> remove exactly one head Event from that Slot inbox
-> wrap it with the generated Slot Event variant
-> return Some(source-qualified Event)
```

Selection and removal do not assign Event index, read or freeze acceptance time, submit EventAccepted, or invoke application code.

Absent polling failure, `None` asserts that no Event candidate was selectable when the bounded nonblocking poll completed. A polling failure submits one bounded Fatal report and returns `None`, as in v7. The Engine processes the post-poll Fatal boundary before interpreting `None` as a reason to consider waiting.

The Engine then follows this sequence:

```text
process the post-poll Fatal boundary
-> if Fatal wins: abandon the owned candidate
-> otherwise read and validate Environment::now()
-> assign the next checked Event index
-> encode the complete Event and authoritative source
-> submit Sync(EventAccepted)
-> invoke on_event once
```

The generated wrapping operation is total. A removed `MarketDataEvent` from Primary's queue becomes `Ports::Primary(event)` with no user callback and no fallible conversion.

Failure before successful EventAccepted submission invokes no handler. The removed candidate is not retried. Successful EventAccepted submission establishes acceptance exactly as in v7.

After an admitted `None` with no Fatal report, the Engine still waits only through the race-safe run notifier. A staged queue remains signaled until a poll removes its selectable head. An owned candidate requires no signal because the Engine already holds it and immediately processes the post-poll boundary.

## 14. Event Capacity Consequence

An Event inbox position becomes available when polling removes its head, not when EventAccepted succeeds. This rule is observable at the concurrent live offer boundary.

The following race is valid:

```text
Primary Event inbox is full
-> admitted poll removes its head into an owned candidate
-> Primary Port offers another Event and uses the freed position
-> a Fatal report wins the post-poll boundary
-> Core abandons the owned candidate and closes Event staging
-> the newly staged Event is also abandoned as unaccepted
```

No accepted Event is lost. No Event is silently overwritten. The successful concurrent offer remains a real staging result even though terminal closure later abandons it.

This rule avoids a separate reservation token, retained-capacity guard, type-erased candidate handle, or second Slot key. It is the simplest semantics consistent with an owned candidate.

## 15. Command Staging And Audit Encoding

During the handler, each `Context::command` call performs bounded work only:

```text
validate next turn-local ordinal
-> encode the complete destination-qualified Command
-> reserve and write one turn-local entry
-> return to application code
```

The staged entry conceptually contains:

```rust
struct StagedCommand<C> {
    ordinal: CommandOrdinal,
    command: C,
    encoded: EncodedCommand,
}
```

For this topology, `C` is `PortsCommand`. The encoded representation includes the generated destination Slot identity and complete Contract Command value. It does not encode a separately supplied destination.

Encoding dispatch is exhaustive:

```rust
match command {
    PortsCommand::Primary(command) => encode_primary(command, output),
    PortsCommand::Secondary(command) => encode_secondary(command, output),
    PortsCommand::Execution(command) => encode_execution(command, output),
    PortsCommand::Timer(command) => encode_timer(command, output),
}
```

Application-provided Contract encoders remain subject to v7's deterministic behavior assumptions. The topology encoder adds frozen manifest identity and direction framing.

## 16. Command Preflight And Handoff

The generated Command variant determines its destination ordinal:

```rust
fn command_slot(command: &PortsCommand) -> SlotOrdinal {
    match command {
        PortsCommand::Primary(_) => SlotOrdinal::new(0),
        PortsCommand::Secondary(_) => SlotOrdinal::new(1),
        PortsCommand::Execution(_) => SlotOrdinal::new(2),
        PortsCommand::Timer(_) => SlotOrdinal::new(3),
    }
}
```

Core counts required capacity in a preallocated fixed-size array bounded by Kavod's maximum Slot count. It uses checked increment for every staged Command. Construction validates that the topology Slot count fits the array and that configured per-turn Command capacity fits its counter domains.

Preflight then compares each nonzero requirement with the corresponding generated queue field. Insufficient capacity reports Fatal before any current-turn Command insertion.

After successful CommandsPrepared submission, handoff consumes Commands in original turn-local ordinal order:

```rust
match command {
    PortsCommand::Primary(command) => queues.primary.commands.push(command),
    PortsCommand::Secondary(command) => queues.secondary.commands.push(command),
    PortsCommand::Execution(command) => queues.execution.commands.push(command),
    PortsCommand::Timer(command) => queues.timer.commands.push(command),
}
```

Each arm moves the concrete Contract Command into the destination's typed inbox. No payload cast, allocation, or virtual call occurs.

The v7 handoff and failure rules remain unchanged:

- A successful insertion is the handoff point.
- A full result after successful preflight is an invariant violation and panics.
- Any other insertion failure reports Fatal.
- Earlier successful insertions remain real.
- No later current-turn Command is attempted after the first failed insertion or CommandAccepted submission.
- CommandAccepted evidence follows each successful insertion.
- TurnCompleted follows only after all handoffs and evidence submissions succeed.

## 17. Live Port Contract

A LivePort implementation is generic over one Contract, not one application Slot:

```rust
pub trait LivePort<C: PortContract>: Sized + Send + 'static {
    // Exact worker ownership and lifecycle API remains open.
}
```

Primary and Secondary may use different implementation types:

```rust
impl LivePort<MarketData> for DirectFeed {}
impl LivePort<MarketData> for ConsolidatedFeed {}
```

They may also use separate values of the same implementation type. In either case, the generated binding field creates distinct queue and lifecycle authority.

The run-scoped session exposes:

- Typed `C::Command` input.
- Typed `C::Event` offer capability.
- Terminal-control input.
- Bounded Fatal-report output.
- No topology Event constructor.
- No AppState, Engine, handler, AuditWriter, or Environment-mode access.

Live workers may execute concurrently. Only successful insertion into the binding's Event inbox makes an Event selectable. Only Core insertion into the binding's Command inbox constitutes Command handoff.

## 18. SimPort Contract

A SimPort implementation is also Contract-generic and concrete:

```rust
pub trait SimPort<C: PortContract>: Sized + 'static {
    fn start(&mut self, ctx: &mut SimStartContext<C>);
    fn on_command(&mut self, ctx: &mut SimContext<C>, command: C::Command);
    fn step(&mut self, ctx: &mut SimContext<C>) -> Option<C::Event>;
    fn stop(&mut self, ctx: &mut SimStopContext<C>);
}
```

The signatures are illustrative; v7's callback semantics are controlling. Contexts cannot be retained. They expose virtual time and only the cursor or Event authorities appropriate to that callback.

Simulation polling uses the generated binding fields in frozen topology order unless deterministic Environment configuration selects another total policy. It first delivers handed-off Commands in global handoff order, then applies v7's cursor selection and same-time callback bound.

A `step` result still passes through its Slot's typed Event inbox insertion boundary before selection returns an owned topology Event. This preserves Event capacity, full-inbox Fatal reporting, and the common definition of staging. Simulation must not bypass the inbox merely because callback execution is synchronous.

Ready Commands are delivered before the first cursor selection. No cursor is stepped before the Ready turn completes. A simulated callback cannot recursively invoke the application handler.

## 19. Construction And Freezing

The generated binding struct proves at compile time that every named Slot field is present exactly once. Engine construction still performs all v7 runtime validation that Rust's type system does not prove:

- Every implementation satisfies mode-specific configuration requirements.
- Every Event and Command capacity is finite and nonzero where required.
- Total queue and turn-local storage arithmetic succeeds.
- The topology Slot count fits all fixed Core arrays and ordinal domains.
- Maximum Event, Command, Fatal Reason, and audit encodings fit configured storage.
- Audit ingress, pending storage, terminal reserve, and sequence domains are mutually compatible.
- Live notifier and worker resources are available.
- Simulation has one cursor slot per binding and valid same-time bounds.
- All required Core backing allocations succeed before start.

Failure is a construction error, not `EngineExit`. No run-scoped interface is published and no Port activity begins.

After successful construction, the topology, binding field correspondence, queue capacities, selection policy, encoders, and manifest ordinals are frozen before `Environment::start`.

## 20. Startup, Stop, And Abort

The generated product does not create per-Port application lifecycle. Environment lifecycle remains aggregate as required by v7.

`Environment::start` visits every concrete binding under one transactional startup operation. A failure leaves no run-scoped activity or interface live and returns a pre-run startup error.

`Environment::stop` visits every binding according to the Environment's frozen aggregate stop policy. Success means every binding completed its private shutdown contract and will make no later use of run-scoped Event, Command, ordinary audit, or Fatal-reporting interfaces.

`Environment::abort` remains one bounded, nonblocking, best-effort aggregate action after Fatal establishment. Generated field access permits static calls to each concrete binding. Core does not await or retry cleanup.

Implementation-specific failures must be mapped at each typed binding boundary into Kavod's bounded startup, stop, or Fatal-report representation. This mapping is ordinary monomorphized code. Port Event and Command payloads are never erased to solve heterogeneous failure aggregation.

The exact implementation-error adapter API remains open because v7 leaves concrete Environment error representation undecided. It must preserve bounded representation, report ordering, aggregate stop semantics, and terminal failure precedence.

## 21. Audit Identity

Topology declaration order supplies checked manifest-local Slot ordinals. `RunStarted` records the correspondence required to interpret later compact records:

```text
Slot 0 -> Primary -> MarketData
Slot 1 -> Secondary -> MarketData
Slot 2 -> Execution -> Execution
Slot 3 -> Timer -> Timer
```

EventAccepted records derive source from the generated Event variant. CommandsPrepared and CommandAccepted derive destination from the generated Command variant. Technical Port reports derive Slot identity from the generated binding field being invoked.

The implementation must never serialize or depend on:

- Rust enum discriminant layout.
- `TypeId`.
- Rust type names as authority.
- Pointer or function addresses.
- Struct field offsets.
- Procedural-macro expansion order other than declared variant order.

Reordering topology variants changes the frozen manifest for a later build. Cross-build compatibility remains outside the v7 claim.

## 22. Bounds And Allocation

The topology design introduces no steady-state Core allocation.

Before `RunStarted`, construction allocates and validates:

- One Event queue per generated Slot field.
- One Command queue per generated Slot field.
- Turn-local `PortsCommand` storage.
- Complete bounded Event and Command encoding storage.
- Fixed per-Slot Command preflight counters.
- Environment notifier and lifecycle state.
- Simulation cursor and same-time accounting state.

`Ports` is sized as the maximum of its concrete Event variants plus its enum tag. `PortsCommand` is sized as the maximum of its concrete Command variants plus its enum tag. Queue storage remains Contract-specific, so one unusually large Contract payload does not enlarge unrelated Slot queue entries.

Kavod declares a compile-time maximum Slot count and separately validates the application's configured count. Core loops over generated or fixed-size structures with that bound. No generic recursion is required at runtime.

Checked arithmetic covers Slot count, queue capacities, total allocated bytes, preflight counts, Event index, Command ordinal, audit sequence, simulation schedule ordinal, and encoded framing sizes.

## 23. Macro Implementation

`#[kavod::topology]` requires a small procedural-macro companion crate because an attribute macro must rewrite enum field types and generate companion identifiers such as `PortsCommand`, `PortsBindings`, `PortsCapacities`, and the hidden queue product.

The macro performs syntactic work only:

1. Parse one enum whose variants each contain one Contract type.
2. Preserve visibility and supported documentation attributes.
3. Rewrite each Event variant field to `<Contract as PortContract>::Event`.
4. Generate the parallel Command enum using `<Contract as PortContract>::Command`.
5. Generate the generic binding product with one snake-case field per Slot.
6. Generate the named capacity product.
7. Generate the public-but-hidden typed queue product with private fields.
8. Generate exhaustive Event and Command classification.
9. Generate direct queue routing and binding visitation.
10. Generate manifest names, Contract correspondence, and checked ordinals.
11. Generate live and simulation binding-set implementations with concrete trait bounds.

The expansion is linear in Slot count. It does not generate partial-builder states or combinations of bindings.

For robust diagnostics, the macro should emit errors at the topology variant that caused them. Generated companion-name collisions should be ordinary compile errors initially; optional explicit companion names may be added only if a real multi-topology module requirement appears.

Generated private modules and helper names must use a collision-resistant prefix. Public API should expose only the Event enum, Command enum, binding struct, required capacity/configuration types, and documented traits. Types that Rust visibility requires for public associated types may be `#[doc(hidden)] pub` while retaining private fields and constructors.

## 24. Why Not A Tuple

A heterogeneous tuple is type-safe, but it is inferior for this static topology:

- Tuple positions duplicate topology order as an independent correspondence.
- Missing and duplicate logical bindings require runtime validation.
- Diagnostics refer to tuple indexes rather than Slot names.
- Generic operation requires tuple-arity trait implementations or recursive type lists.
- Direct Command and Event routing requires traversal or another lookup layer.
- Users cannot naturally name implementation roles in type aliases.

The generated struct already follows from the topology declaration and gives named, direct, compile-time-complete correspondence.

## 25. Why Not Slot Keys

An API such as:

```rust
ctx.command(PRIMARY, command)
```

requires a separate key value and a generic method whose accepted Command type depends on that key's static type. It can be made safe with marker types, but it duplicates information already represented by a closed Command variant.

The selected API is simpler:

```rust
ctx.command(PortsCommand::Primary(command))
```

Likewise, returning `NextEvent { slot: SlotId }` forces later lookup into heterogeneous typed storage. Returning `Ports::Primary(event)` carries the complete source-qualified value without erasure or lookup.

Private compact ordinals remain useful for arrays and audit records, but they are always derived from generated variants or fields.

## 26. Why Not Type Erasure Or Trait Objects

A registry such as `Vec<Box<dyn Port>>` or `BTreeMap<SlotId, Box<dyn Any>>` would simplify heterogeneous storage but would weaken the selected properties:

- Contract compatibility would move toward runtime checks.
- Payload access would require erased methods or downcasts.
- Source and destination authority could diverge from concrete payload type.
- Queue storage would no longer visibly retain Contract types.
- Environment calls would use virtual dispatch.
- Invalid registry states would become representable after construction.

The closed enums and generated products solve the actual static problem directly. Runtime extensibility is neither required nor compatible with v7's frozen Application shape.

## 27. Adversarial Review

### 27.1 Shared Contracts Do Not Collapse Slot Identity

Primary and Secondary carry the same `MarketDataEvent` and `MarketDataCommand` types, but they occupy distinct enum variants, binding fields, queue fields, capacities, and manifest entries. No routing decision uses payload `TypeId`.

**Result:** Satisfied.

### 27.2 Different Contracts Remain Statically Typed

Execution and Timer variants project their own associated protocols. Generated binding bounds reject an implementation attached to the wrong field when constructing a live or simulation Environment.

**Result:** Satisfied.

### 27.3 Binding Completeness Is Structural

`PortsBindings` has exactly one field per topology variant. Safe ordinary construction cannot omit, duplicate, or add a binding. This is stronger and simpler than construction-time set validation.

**Result:** Satisfied.

### 27.4 Event Source Cannot Be Payload-Supplied

Port sessions accept only Contract Events. Generated dequeue code wraps the Event according to the queue field. The bound implementation never submits a topology variant or ordinal.

**Result:** Satisfied, provided topology Event offer methods are not exposed to Port sessions.

### 27.5 Command Destination Cannot Disagree

Destination is the Command enum variant. Preflight, encoding, handoff, and evidence all derive from that same variant.

**Result:** Satisfied, provided no Core API accepts a separate destination argument.

### 27.6 Event Conversion Cannot Fail

Every generated queue field has one generated wrapping arm whose input is exactly the Contract Event type. There is no application-defined `TryFrom` call.

**Result:** Satisfied. v7's conversion-failure wording must be revised.

### 27.7 Owned Polling Preserves Acceptance Authority

Polling removes and source-qualifies a candidate but does not assign index or time, audit acceptance, or invoke the handler. The Engine's next Fatal boundary still decides whether acceptance begins.

**Result:** Satisfied with the explicitly changed capacity-release point.

### 27.8 Popped Candidates Are Not Retried

Clock, encoding, identifier, capacity, or EventAccepted failure after polling abandons the candidate and invokes no handler. Fatal finalization follows v7. Requeueing would change FIFO and pressure history and is forbidden.

**Result:** Satisfied.

### 27.9 Command Preflight Remains Atomic Across Destinations

The generated destination classifier counts every staged Command before insertion. Generated queue fields expose exact available capacities. Only after all checks and CommandsPrepared submission does ordinal-order insertion begin.

**Result:** Satisfied.

### 27.10 Concurrent Consumption Cannot Invalidate Preflight

Core remains the sole Command producer. Live Port consumption only increases available capacity after preflight. A subsequent full result remains an invariant violation.

**Result:** Satisfied.

### 27.11 Simulation Cannot Bypass Common Queue Semantics

SimPort Event results enter their typed Slot inbox before selection returns them. Commands are delivered from the same typed inbox representation in global handoff order. Direct callback-to-handler delivery is prohibited.

**Result:** Satisfied, provided the simulation implementation does not optimize away the semantic insertion boundary.

### 27.12 No Hidden Environment-Mode Branch Reaches Application Code

The Application uses `AppEvent<Ports>` and `PortsCommand` in both modes. Binding structs differ only in concrete implementation fields and mode-specific Environment traits.

**Result:** Satisfied.

### 27.13 Macro Output Does Not Become Audit Authority

Generated declaration order feeds the frozen manifest. Raw compiler layout and names are never serialized as standalone authority.

**Result:** Satisfied.

### 27.14 No Steady-State Allocation Is Required

Enums move inline values, generated products have static field layouts, and all bounded queue and turn storage is allocated before start.

**Result:** Satisfied, subject to concrete bounded queue and encoder implementations.

### 27.15 Core Work Is Bounded

Slot count, turn Command count, queue capacities, encodings, and simulation callbacks all have configured maxima. Generated matches have finite arms. Fixed arrays avoid generic-const-expression dependence and runtime growth.

**Result:** Satisfied.

### 27.16 Heterogeneous Lifecycle Errors Need A Bounded Adapter

Concrete implementations may naturally have different private error types. Environment startup, stop, and Fatal reporting need one bounded representation. Each generated field must map its implementation error into that representation without changing Event or Command protocol typing.

**Result:** Feasible but intentionally open because v7 leaves exact Environment error representation undecided. This must be settled with LivePort and SimPort lifecycle signatures.

### 27.17 Large Topologies Increase Generated Generic Arity

`PortsBindings` has one type parameter per Slot. This is linear and compiler-supported but can increase diagnostics and monomorphization cost. Kavod already requires a finite maximum Slot count.

**Result:** Acceptable. Establish and test a conservative maximum rather than claiming unbounded topology size.

### 27.18 Public Construction Must Not Permit Queue Mismatch

Users construct implementation and capacity fields, not queue products. Queue-product constructors and endpoint splitting remain private. Environment construction creates queues from the frozen topology and capacities.

**Result:** Satisfied if queue fields and constructors remain crate-private.

### 27.19 Public Associated-Type Visibility Must Not Expose Authority

Rust forbids a public trait implementation from leaking a private associated type. The generated queue product therefore must be `#[doc(hidden)] pub`, but all fields, endpoint constructors, and mutation methods remain private to Kavod. Public visibility of the type name does not grant queue authority or permit construction.

**Result:** Satisfied by the public-but-hidden queue product; making the type itself private would not compile across an application crate boundary.

## 28. Verification Plan

The implementation is not complete until all of the following tests exist.

### 28.1 Compile-Pass Tests

- Two Slots use one Contract with different implementation types.
- Two Slots use one Contract with the same implementation type as separate values.
- One topology combines several unrelated Contracts.
- Live and simulation binding products use the same generated topology.
- Application matching receives the expected concrete Event types.
- Context accepts every valid generated Command variant.
- Uninhabited one-sided protocols compile.

### 28.2 Compile-Fail Tests

- A binding field uses an implementation of the wrong Contract.
- A Command variant receives another Contract's Command type.
- A topology variant does not contain exactly one Contract.
- A topology exceeds the maximum Slot count.
- A binding struct literal omits a Slot field.
- A binding struct literal names an undeclared field.
- A Port session attempts to offer the topology Event enum instead of its Contract Event.

### 28.3 Runtime Construction Tests

- Checked capacity arithmetic rejects overflow.
- Every generated queue receives its configured independent capacity.
- Manifest ordinals follow declaration order.
- Maximum Event and Command encodings fit preallocated storage.
- Startup failure leaves no run-scoped interface live.

### 28.4 Runtime Routing Tests

- Every Event queue wraps into exactly its corresponding Event variant.
- Every Command variant inserts into exactly its corresponding Command queue.
- Primary and Secondary values with identical payload bytes retain different source and destination identity.
- Global Command production order is preserved across destination interleaving.
- Per-Slot FIFO order is preserved.

### 28.5 Fault-Injection Tests

- Event offer loses the staging-closure race.
- Event inbox is full while staging remains open.
- A poll pops a candidate and Fatal wins the post-poll boundary.
- A concurrent offer uses capacity freed by candidate popping.
- Clock validation fails after candidate popping.
- EventAccepted encoding or submission fails after candidate popping.
- Command staging and encoding fail during the handler.
- Command preflight fails before insertion.
- CommandsPrepared submission fails before insertion.
- Command insertion or CommandAccepted submission fails after earlier handoffs.
- Aggregate stop fails after completed handoffs.
- Fatal finalization runs with every generated binding shape.

### 28.6 Macro Tests

- Generated names, visibility, and documentation are correct.
- Compiler errors point to the originating topology variant.
- Companion-name collisions fail clearly.
- Variant attributes are preserved where supported.
- Expanded code contains no trait objects, `Any`, or downcasts.
- Generated matches remain exhaustive when a Slot is added.

## 29. Selected Decisions And Open Details

The following decisions are selected by this report:

1. One attribute enum declares the complete ordered Slot topology.
2. A topology variant names a Contract.
3. The generated topology enum is the complete source-qualified Port Event type.
4. A generated parallel enum is the complete destination-qualified Command type.
5. A generated generic struct contains all concrete implementation bindings.
6. A generated named struct contains all per-Slot capacities.
7. A generated public-but-hidden struct with private fields contains all typed queues.
8. Struct fields, not tuple positions or runtime keys, establish binding correspondence.
9. Binding and capacity completeness are compile-time structural completeness.
10. Environment polling returns one owned source-qualified Event candidate.
11. Polling removal frees Event-inbox capacity before acceptance.
12. Slot-to-topology Event conversion is total and generated.
13. Core uses private bounded ordinals only as derived manifest metadata and array indexes.
14. Live and simulation use separate binding traits over the same generated binding product.
15. Runtime Engine and Environment calls use static dispatch.

The following details remain open without weakening the selected topology:

- Exact `PortContract` bounds and audit encoder traits.
- Exact bounded FIFO implementation and live synchronization primitive.
- Exact LivePort worker ownership and session signatures.
- Exact SimPort context signatures.
- Exact bounded startup and stop error adapters.
- Exact capacity configuration syntax.
- Exact generated companion-name override syntax, if needed.
- The numeric maximum Slot count.

These open details must preserve every invariant in this report and `design-v7.md`. They do not justify introducing a runtime registry, user-visible Slot key, erased payload, trait-object Port implementation, fallible topology injection, or recursive Core processing.

## 30. Conclusion

The selected topology design maps v7's static application model directly into Rust's native closed types:

```text
Contract associated types
-> generated source Event enum
-> generated destination Command enum
-> generated implementation product
-> generated capacity product
-> generated typed queue product
-> monomorphized live or simulation Environment
```

It supports repeated and unrelated Contracts, proves binding completeness through struct construction, preserves per-Slot authority and capacity, and gives Core exhaustive static routing without type erasure or dynamic dispatch.

The principal semantic change from the current v7 text is owned Event selection: polling pops one Event, wraps it with authoritative source identity, and returns it as an unaccepted candidate. That choice removes `SlotId` and heterogeneous lookup while preserving the Engine's exclusive authority over acceptance, turn execution, Fatal establishment, and audit evidence.

Subject to the compile-pass, compile-fail, bounded-storage, and fault-injection tests above, this is the smallest robust implementation shape for v7's Port and Slot requirements.
