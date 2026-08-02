# Kavod v7 Manual Port Topology Implementation Design

> **Status:** Proposed manual-first implementation design for review
> **Semantic authority:** `design_docs/design-v7.md`
> **Scope:** Port Contracts, static Port Slots, typed inboxes, live and simulated bindings, Environment integration, Event selection, and the later mechanical `macro_rules!` replacement
> **Priority:** Implement, inspect, and test one complete handwritten topology before introducing code generation

---

## 1. Reading Rule And Decision Summary

This document specifies ordinary Rust code first. Sections 2 through 29 describe the complete handwritten implementation for this representative topology:

```text
Slot 0: Primary   uses MarketData
Slot 1: Secondary uses MarketData
Slot 2: Execution uses Execution
Slot 3: Timer     uses Timer
```

The handwritten implementation is the authority for the eventual macro expansion. It must compile and pass all routing, lifecycle, capacity, simulation-ordering, and fault-injection tests before a macro is written.

Only Section 30 introduces `macro_rules!`. That macro must reproduce the already-tested handwritten types and exhaustive matches. It must not own Engine sequencing, queue algorithms, Fatal semantics, audit policy, Environment lifecycle rules, or simulation scheduling rules.

The selected implementation has these properties:

1. A `PortContract` associates one Event type with one Command type.
2. A separate `TradingTopology` marker names the complete topology.
3. `TradingPortEvent` is the closed source-qualified Port Event protocol.
4. `TradingPortCommand` is the closed destination-qualified Port Command protocol.
5. `TradingBindings<...>` is one named product that can conditionally implement either `LiveBindingSet` or `SimBindingSet`.
6. Every Slot has independent typed Event and Command inboxes.
7. Event polling identifies, but does not remove, one Slot inbox head.
8. Only the Engine admits removal and Event acceptance after the post-poll Fatal boundary.
9. Runtime routing uses exhaustive matches, concrete fields, and static dispatch.
10. All topology-specific code can be inspected without macro expansion tools.

This design restores the non-removing candidate semantics required by `design-v7.md`. It rejects the old `impl.md` owned-candidate design, which released Event capacity during polling before the Engine admitted acceptance.

## 2. Responsibility Boundaries

The implementation has four layers:

| Layer | Responsibility |
|---|---|
| Application | Contracts, protocol values, State, handler, deterministic encoders, concrete live and simulated Port implementations |
| Handwritten topology | Closed Event and Command enums, named products, manifest, exhaustive typed routing, field visitation |
| Generic Kavod runtime | Bounded queues, endpoint capabilities, Engine, Context, audit worker, Fatal inbox, notifier, live and simulation machinery |
| Environment | Mode-specific Port execution, Event candidate selection, logical clock, aggregate start, stop, and abort |

Generated or handwritten topology code performs only mechanical operations:

- Classify an Event or Command by exhaustive match.
- Allocate and split one typed queue pair per Slot.
- Inspect or remove one selected typed Event head.
- Count, preflight, and insert typed Commands.
- Visit every concrete binding in frozen declaration order.
- Dispatch one simulated callback to the selected concrete binding.
- Publish frozen manifest metadata.

Only the handwritten generic Engine performs semantic operations:

- Admit actions at Fatal boundaries.
- Assign Event and Command identities.
- Read and validate logical time.
- Submit audit evidence.
- Invoke the application handler.
- Decide partial-handoff consequences.
- Establish Fatal or Stopped.
- Execute Stop and Fatal finalization.

## 3. Core Contract Types

A Contract describes protocol meaning only:

```rust
pub trait PortContract: Sized + 'static {
    type Event: 'static;
    type Command: 'static;
}
```

Do not put `Send` on `PortContract`. Simulation may use non-`Send` values. Live construction adds `Send` bounds to the concrete Contract values it moves across threads.

One-sided Contracts use Contract-owned uninhabited types:

```rust
pub enum NoTimerCommands {}
```

Do not use `()`, `Option<T>`, or one global no-command marker. The uninhabited type remains part of that Contract's protocol identity.

## 4. Representative Application Contracts

The handwritten reference topology uses these ordinary application types:

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

```rust
pub struct Execution;

impl PortContract for Execution {
    type Event = ExecutionEvent;
    type Command = ExecutionCommand;
}
```

```rust
pub struct Timer;

impl PortContract for Timer {
    type Event = TimerEvent;
    type Command = TimerCommand;
}
```

Contracts own no Slot identity, queue, capacity, implementation, lifecycle state, or Environment mode.

## 5. Handwritten Topology Protocols

Topology identity, Event values, and Command values are separate types:

```rust
pub struct TradingTopology;
```

```rust
pub enum TradingPortEvent {
    Primary(<MarketData as PortContract>::Event),
    Secondary(<MarketData as PortContract>::Event),
    Execution(<Execution as PortContract>::Event),
    Timer(<Timer as PortContract>::Event),
}
```

```rust
pub enum TradingPortCommand {
    Primary(<MarketData as PortContract>::Command),
    Secondary(<MarketData as PortContract>::Command),
    Execution(<Execution as PortContract>::Command),
    Timer(<Timer as PortContract>::Command),
}
```

`TradingPortEvent` preserves source identity even though Primary and Secondary carry the same payload type. `TradingPortCommand` makes destination and payload type one value, so disagreement is unrepresentable:

```rust,compile_fail
TradingPortCommand::Timer(ExecutionCommand::Submit(order))
```

The Core-provided application protocol is:

```rust
pub enum AppEvent<T: Topology> {
    Ready,
    Port(T::Event),
}
```

The application handles:

```rust
match event {
    AppEvent::Ready => initialize(state, ctx),
    AppEvent::Port(TradingPortEvent::Primary(event)) => {
        on_primary(state, event, ctx)
    }
    AppEvent::Port(TradingPortEvent::Secondary(event)) => {
        on_secondary(state, event, ctx)
    }
    AppEvent::Port(TradingPortEvent::Execution(event)) => {
        on_execution(state, event, ctx)
    }
    AppEvent::Port(TradingPortEvent::Timer(event)) => {
        on_timer(state, event, ctx)
    }
}
```

This implementation selects total Slot injection: removing a `MarketDataEvent` from Primary's typed Event inbox always constructs `TradingPortEvent::Primary(event)`. No application callback or `TryFrom` participates. The conversion-failure branch permitted by v7 is unreachable for this topology implementation; all other pre-acceptance failure rules remain unchanged.

Do not derive `Clone`, `Copy`, `Debug`, equality, serialization, or other traits automatically on projection-heavy protocol enums. Add each derive only when all Contract associated-type bounds are intentional.

## 6. Private Slot Identity And Manifest

Runtime ordinals are checked private metadata:

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct SlotOrdinal(u16);

impl SlotOrdinal {
    #[doc(hidden)]
    pub const fn __new(value: u16) -> Self {
        Self(value)
    }

    #[doc(hidden)]
    pub const fn __index(self) -> usize {
        self.0 as usize
    }
}
```

The inner value is not constructible through the normal documented API. Because a topology may be handwritten in another crate, `__new` and `__index` are necessarily public, doc-hidden, convention-reserved SPI. `#[doc(hidden)]` is not a security boundary. Every runtime use validates the ordinal against the frozen topology before touching storage; source and destination authority still come from owned queue and enum capabilities.

The manifest is explicit and never uses Rust enum layout:

```rust
pub struct SlotManifestEntry {
    pub ordinal: SlotOrdinal,
    pub slot_name: &'static str,
    pub contract_name: &'static str,
}

pub const TRADING_MANIFEST: &[SlotManifestEntry] = &[
    SlotManifestEntry {
        ordinal: SlotOrdinal::__new(0),
        slot_name: "Primary",
        contract_name: "MarketData",
    },
    SlotManifestEntry {
        ordinal: SlotOrdinal::__new(1),
        slot_name: "Secondary",
        contract_name: "MarketData",
    },
    SlotManifestEntry {
        ordinal: SlotOrdinal::__new(2),
        slot_name: "Execution",
        contract_name: "Execution",
    },
    SlotManifestEntry {
        ordinal: SlotOrdinal::__new(3),
        slot_name: "Timer",
        contract_name: "Timer",
    },
];
```

Ordinals are local to the frozen manifest. They are not user-authored routing keys, Rust discriminants, cross-build identities, or business identities.

## 7. Capacity Products

The generic Slot capacity is:

```rust
#[derive(Clone, Copy)]
pub struct SlotCapacity {
    pub event_items: usize,
    pub command_items: usize,
}
```

The handwritten topology has one named capacity per Slot:

```rust
pub struct TradingCapacities {
    pub primary: SlotCapacity,
    pub secondary: SlotCapacity,
    pub execution: SlotCapacity,
    pub timer: SlotCapacity,
}
```

The named product provides structural completeness without a map or public Slot key. Construction validates every capacity and all aggregate byte arithmetic before allocating.

## 8. Bounded Queue Primitive

The topology depends on one audited fixed-capacity SPSC ring implementation. Its algorithm is generic library code, not topology code.

The ring must provide these properties:

- Capacity is fixed and backing storage is allocated before start.
- Push and pop never allocate, overwrite, or silently drop.
- One endpoint is the sole producer and one is the sole consumer.
- FIFO order is preserved.
- `remaining_capacity` is exact for the sole producer.
- A failed push returns ownership of the value.
- Endpoint disconnection is distinguishable from full or empty.
- Payload destruction does not occur while another endpoint can access it.
- Shared backing remains alive if live workers outlive Fatal Engine exit.

Conceptually:

```rust
pub struct BoundedSpsc<T> {
    // Preallocated ring backing and synchronization state.
}

pub struct Producer<T> {
    // Unique producer capability.
}

pub struct Consumer<T> {
    // Unique consumer capability.
}
```

```rust
impl<T> BoundedSpsc<T> {
    pub fn try_new(
        capacity: usize,
    ) -> Result<(Producer<T>, Consumer<T>), QueueAllocationError>;
}
```

The required endpoint surface is:

```rust
pub enum PushError<T> {
    Full(T),
    Disconnected(T),
    Closed(T),
}

pub enum PopError {
    EmptyOpen,
    EmptyClosed,
    Disconnected,
}

impl<T> Producer<T> {
    pub fn remaining_capacity(&self) -> usize;
    pub fn try_push(&mut self, value: T) -> Result<(), PushError<T>>;
    pub fn close(&mut self);
}

impl<T> Consumer<T> {
    pub fn has_item(&self) -> bool;
    pub fn try_pop(&mut self) -> Result<T, PopError>;
    pub fn capacity(&self) -> usize;
}
```

The consumer retains its configured capacity so bounded abandonment does not depend on an external map. For Command envelopes, a specialized read-only `front_key` copies only the key. Endpoint drop and all error paths return or destroy each payload exactly once.

Neither endpoint is `Clone` in MVP. A queue crate may be used for the first prototype if it meets the observable contract. If allocator exhaustion must be returned rather than following the process allocator's OOM behavior, Kavod needs fallible backing allocation.

### 8.1 Event Staging Gate

A separate `if open { push() }` check is racy and forbidden. Event publication and global Event-staging closure must share one linearizable primitive.

```rust
pub enum StageResult<T> {
    Staged,
    RunClosed(T),
    FullWhileOpen(T),
    Disconnected(T),
}
```

The combined gate and queue publication operation must establish exactly one result:

- `Staged`: publication committed before closure.
- `RunClosed`: closure committed first; capacity is not reported.
- `FullWhileOpen`: full was observed while publication remained open; a Fatal report is required.
- `Disconnected`: the open queue lost its consumer; a Fatal report is required.

Strict publication-versus-closure ordering cannot be obtained from an unrelated `AtomicBool` and queue. The queue/gate implementation must integrate publication admission and closure, or use another audited synchronization primitive with the same linearization points.

All Slot Event producers share one run-scoped Event-staging phase authority so Engine Fatal or Stop closure has one logical commit point. Physical queue cleanup may follow that logical closure.

### 8.2 Command Production Closure

Core is the sole Command producer. All Command producers share one logical `CommandProductionGate`. Command-production closure occurs only between admitted Core actions and commits once for the complete topology. Physical sender closure follows through `Topology::__close_command_production`.

Each Command receiver operation returns one of:

```rust
pub enum CommandReceive<C> {
    Command(CommandEnvelope<C>),
    EmptyOpen,
    EmptyClosed,
}
```

The receiver rechecks its queue after observing logical closure so a Command published before the release-close transition cannot be missed.

### 8.3 Shared Run Services

`RunServices`, `LiveRunServices`, and `SimRunServices` are concrete bundles created during Engine construction. They contain shared preallocated backing for:

- Run phase and publication state.
- Event-staging and Command-production gates.
- Fatal reporting and its closure state.
- Ordinary audit submission and its closure state.
- Race-safe notifier state and Engine wake handle.
- Slot-local technical failure framing.

Every handle that a live worker may retain owns shared backing, commonly through construction-time `Arc` values. Closing an interface changes shared state; dropping Engine does not invalidate memory still reachable by a late worker. No handle cloning or allocation occurs after `RunStarted`; MVP gives one session to one worker per Slot.

## 9. Command Envelopes And Global Handoff Order

Typed per-Slot queues preserve per-Slot FIFO but do not by themselves preserve global interleaving across Slots. Simulation requires a total handoff key:

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HandoffKey {
    pub event_index: EventIndex,
    pub command_ordinal: CommandOrdinal,
}

pub struct CommandEnvelope<C> {
    pub key: HandoffKey,
    pub command: C,
}
```

Because turns execute serially and ordinals are unique within a turn, `(EventIndex, CommandOrdinal)` totally orders all handed-off Commands.

The queue consumer must support copying the front key without borrowing the payload across a callback:

```rust
fn front_key(&self) -> Option<HandoffKey>;
```

## 10. Split Per-Slot Endpoints

Queue ownership is split at construction:

```rust
pub struct CoreSlotQueues<C: PortContract> {
    events: Consumer<C::Event>,
    commands: Producer<CommandEnvelope<C::Command>>,
}

pub struct BindingSlotQueues<C: PortContract> {
    events: EventProducer<C::Event>,
    commands: Consumer<CommandEnvelope<C::Command>>,
}
```

`EventProducer<T>` wraps the raw producer with the shared Event-staging gate, notifier, authoritative Slot identity, and Fatal reporter. It exposes typed staging but no topology Event constructor or ordinal argument.

```rust
#[doc(hidden)]
pub fn __allocate_slot<C: PortContract>(
    slot: SlotOrdinal,
    capacity: SlotCapacity,
    services: &RunServices,
) -> Result<(CoreSlotQueues<C>, BindingSlotQueues<C>), ConstructionError>;
```

Endpoint ownership is:

| Endpoint | Sole owner |
|---|---|
| Event producer | Bound Port or SimEnvironment acting for that binding |
| Event consumer | Engine Core |
| Command producer | Engine Core |
| Command consumer | Bound Port or SimEnvironment acting for that binding |

Do not store one unsplit queue object and lend mutable references to concurrent owners.

### 10.1 Cross-Crate Queue Facade

The fields inside `CoreSlotQueues` and `BindingSlotQueues` remain private to Kavod. Handwritten topology code in an application crate uses only this narrow public-hidden facade:

```rust
impl<C: PortContract> CoreSlotQueues<C> {
    #[doc(hidden)]
    pub fn __event_ready(&self) -> bool;

    #[doc(hidden)]
    pub fn __take_event(&mut self) -> Result<C::Event, QueuePopError>;

    #[doc(hidden)]
    pub fn __command_remaining(&self) -> usize;

    #[doc(hidden)]
    pub fn __push_command(
        &mut self,
        command: CommandEnvelope<C::Command>,
    ) -> Result<(), TypedCommandPushError<C::Command>>;

    #[doc(hidden)]
    pub fn __close_command_production(&mut self);

    #[doc(hidden)]
    pub fn __abandon_events(&mut self);
}
```

```rust
impl<C: PortContract> BindingSlotQueues<C> {
    #[doc(hidden)]
    pub fn __into_live_session<E: BoundedPortError>(
        self,
        services: &LiveRunServices,
    ) -> LivePortSession<C, E>;

    #[doc(hidden)]
    pub fn __front_command_key(&self) -> Option<HandoffKey>;

    #[doc(hidden)]
    pub fn __pop_command(
        &mut self,
    ) -> Result<CommandEnvelope<C::Command>, QueuePopError>;

    #[doc(hidden)]
    pub fn __stage_sim_event(
        &mut self,
        event: C::Event,
    ) -> StageResult<C::Event>;
}
```

These types, `__allocate_slot`, and required helper functions live under `kavod::__private` or are re-exported there as `#[doc(hidden)] pub`. Macro expansion and handwritten external topologies cannot bypass Rust privacy. They never receive constructors for raw queue backing, gates, reporters, or notifier handles.

`BindingSlotQueues` stores the authoritative Slot identity established by `__allocate_slot`; `__into_live_session` derives identity from that endpoint and accepts no second ordinal. The same module provides generic `__sim_start_context`, `__sim_context`, and `__sim_stop_context` constructors. They validate the requested schedule index against the endpoint's stored identity and return only the authority appropriate to that callback. Topology code cannot construct contexts by filling private fields.

## 11. Handwritten Queue Products

The heterogeneous queue products are ordinary structs:

```rust
#[doc(hidden)]
pub struct TradingCoreQueues {
    primary: CoreSlotQueues<MarketData>,
    secondary: CoreSlotQueues<MarketData>,
    execution: CoreSlotQueues<Execution>,
    timer: CoreSlotQueues<Timer>,
}
```

```rust
#[doc(hidden)]
pub struct TradingBindingQueues {
    primary: BindingSlotQueues<MarketData>,
    secondary: BindingSlotQueues<MarketData>,
    execution: BindingSlotQueues<Execution>,
    timer: BindingSlotQueues<Timer>,
}
```

Allocation is explicit:

```rust
fn allocate_trading_queues(
    capacities: &TradingCapacities,
    services: &RunServices,
) -> Result<(TradingCoreQueues, TradingBindingQueues), ConstructionError> {
    let (primary_core, primary_binding) = __allocate_slot::<MarketData>(
        SlotOrdinal::__new(0),
        capacities.primary,
        services,
    )?;

    let (secondary_core, secondary_binding) = __allocate_slot::<MarketData>(
        SlotOrdinal::__new(1),
        capacities.secondary,
        services,
    )?;

    let (execution_core, execution_binding) = __allocate_slot::<Execution>(
        SlotOrdinal::__new(2),
        capacities.execution,
        services,
    )?;

    let (timer_core, timer_binding) = __allocate_slot::<Timer>(
        SlotOrdinal::__new(3),
        capacities.timer,
        services,
    )?;

    Ok((
        TradingCoreQueues {
            primary: primary_core,
            secondary: secondary_core,
            execution: execution_core,
            timer: timer_core,
        },
        TradingBindingQueues {
            primary: primary_binding,
            secondary: secondary_binding,
            execution: execution_binding,
            timer: timer_binding,
        },
    ))
}
```

`SlotCapacity` is deliberately `Copy` because it contains only validated scalar item limits. If byte-capacity configuration later becomes non-scalar, allocation takes references instead.

## 12. Topology Runtime SPI

The generic Engine needs a topology-associated implementation surface:

```rust
pub trait Topology: Sized + 'static {
    type Event: 'static;
    type Command: 'static;
    type Capacities: 'static;

    #[doc(hidden)]
    type CoreQueues: 'static;

    #[doc(hidden)]
    type BindingQueues: 'static;

    #[doc(hidden)]
    type CommandNeeds: 'static;

    const SLOT_COUNT: usize;
    const MANIFEST: &'static [SlotManifestEntry];

    fn event_slot(event: &Self::Event) -> SlotOrdinal;
    fn command_slot(command: &Self::Command) -> SlotOrdinal;

    #[doc(hidden)]
    fn __allocate_queues(
        capacities: &Self::Capacities,
        services: &RunServices,
    ) -> Result<(Self::CoreQueues, Self::BindingQueues), ConstructionError>;

    #[doc(hidden)]
    fn __event_ready(
        queues: &Self::CoreQueues,
        slot: SlotOrdinal,
    ) -> Result<bool, InvalidSlot>;

    #[doc(hidden)]
    fn __take_event(
        queues: &mut Self::CoreQueues,
        slot: SlotOrdinal,
    ) -> Result<Self::Event, TakeEventError>;

    #[doc(hidden)]
    fn __new_command_needs() -> Self::CommandNeeds;

    #[doc(hidden)]
    fn __count_command(
        needs: &mut Self::CommandNeeds,
        command: &Self::Command,
    ) -> Result<(), CountError>;

    #[doc(hidden)]
    fn __preflight_commands(
        queues: &Self::CoreQueues,
        needs: &Self::CommandNeeds,
    ) -> Result<(), CommandCapacityError>;

    #[doc(hidden)]
    fn __push_command(
        queues: &mut Self::CoreQueues,
        command: CommandEnvelope<Self::Command>,
    ) -> Result<(), CommandInsertError>;

    #[doc(hidden)]
    fn __close_command_production(queues: &mut Self::CoreQueues);

    #[doc(hidden)]
    fn __abandon_events(queues: &mut Self::CoreQueues);
}
```

The `#[doc(hidden)]` methods are a public cross-crate SPI, not an application API. Authority comes from ownership: application code never receives queue products or raw endpoints.

## 13. Handwritten Topology Implementation

Command preflight uses a concrete product rather than unstable generic const expressions:

```rust
#[doc(hidden)]
pub struct TradingCommandNeeds {
    primary: usize,
    secondary: usize,
    execution: usize,
    timer: usize,
}
```

The implementation begins:

```rust
impl Topology for TradingTopology {
    type Event = TradingPortEvent;
    type Command = TradingPortCommand;
    type Capacities = TradingCapacities;
    type CoreQueues = TradingCoreQueues;
    type BindingQueues = TradingBindingQueues;
    type CommandNeeds = TradingCommandNeeds;

    const SLOT_COUNT: usize = 4;
    const MANIFEST: &'static [SlotManifestEntry] = TRADING_MANIFEST;

    fn event_slot(event: &Self::Event) -> SlotOrdinal {
        match event {
            TradingPortEvent::Primary(_) => SlotOrdinal::__new(0),
            TradingPortEvent::Secondary(_) => SlotOrdinal::__new(1),
            TradingPortEvent::Execution(_) => SlotOrdinal::__new(2),
            TradingPortEvent::Timer(_) => SlotOrdinal::__new(3),
        }
    }

    fn command_slot(command: &Self::Command) -> SlotOrdinal {
        match command {
            TradingPortCommand::Primary(_) => SlotOrdinal::__new(0),
            TradingPortCommand::Secondary(_) => SlotOrdinal::__new(1),
            TradingPortCommand::Execution(_) => SlotOrdinal::__new(2),
            TradingPortCommand::Timer(_) => SlotOrdinal::__new(3),
        }
    }

    // Remaining methods are the exhaustive operations below.
}
```

### 13.1 Event Readiness And Removal

```rust
match slot.__index() {
    0 => Ok(queues.primary.__event_ready()),
    1 => Ok(queues.secondary.__event_ready()),
    2 => Ok(queues.execution.__event_ready()),
    3 => Ok(queues.timer.__event_ready()),
    _ => Err(InvalidSlot),
}
```

Removal derives source authority from the selected queue arm:

```rust
match slot.__index() {
    0 => queues
        .primary
        .__take_event()
        .map(TradingPortEvent::Primary)
        .map_err(TakeEventError::from),
    1 => queues
        .secondary
        .__take_event()
        .map(TradingPortEvent::Secondary)
        .map_err(TakeEventError::from),
    2 => queues
        .execution
        .__take_event()
        .map(TradingPortEvent::Execution)
        .map_err(TakeEventError::from),
    3 => queues
        .timer
        .__take_event()
        .map(TradingPortEvent::Timer)
        .map_err(TakeEventError::from),
    _ => Err(TakeEventError::InvalidSlot),
}
```

No Port supplies the ordinal or topology variant. A Port owns only its Slot-specific typed producer capability.

### 13.2 Command Counting And Preflight

```rust
fn __new_command_needs() -> TradingCommandNeeds {
    TradingCommandNeeds {
        primary: 0,
        secondary: 0,
        execution: 0,
        timer: 0,
    }
}
```

```rust
match command {
    TradingPortCommand::Primary(_) => {
        needs.primary = needs.primary.checked_add(1).ok_or(CountError)?;
    }
    TradingPortCommand::Secondary(_) => {
        needs.secondary = needs.secondary.checked_add(1).ok_or(CountError)?;
    }
    TradingPortCommand::Execution(_) => {
        needs.execution = needs.execution.checked_add(1).ok_or(CountError)?;
    }
    TradingPortCommand::Timer(_) => {
        needs.timer = needs.timer.checked_add(1).ok_or(CountError)?;
    }
}
```

Preflight compares all destinations before any insertion:

```rust
check_required(
    SlotOrdinal::__new(0),
    needs.primary,
    queues.primary.__command_remaining(),
)?;
check_required(
    SlotOrdinal::__new(1),
    needs.secondary,
    queues.secondary.__command_remaining(),
)?;
check_required(
    SlotOrdinal::__new(2),
    needs.execution,
    queues.execution.__command_remaining(),
)?;
check_required(
    SlotOrdinal::__new(3),
    needs.timer,
    queues.timer.__command_remaining(),
)?;
```

### 13.3 Command Insertion

Insertion moves the inner typed payload and preserves the handoff key:

```rust
let CommandEnvelope { key, command } = command;

match command {
    TradingPortCommand::Primary(command) => {
        normalize_command_push(
            queues.primary.__push_command(CommandEnvelope { key, command }),
        )
    }
    TradingPortCommand::Secondary(command) => {
        normalize_command_push(
            queues.secondary.__push_command(CommandEnvelope { key, command }),
        )
    }
    TradingPortCommand::Execution(command) => {
        normalize_command_push(
            queues.execution.__push_command(CommandEnvelope { key, command }),
        )
    }
    TradingPortCommand::Timer(command) => {
        normalize_command_push(
            queues.timer.__push_command(CommandEnvelope { key, command }),
        )
    }
}
```

`normalize_command_push` is generic over the inner Contract Command. It handles the typed push result inside each match arm and returns the common `Result<(), CommandInsertError>`. After successful all-destination preflight, `Full` is an invariant violation and panics. Disconnection or another non-Full insertion failure returns the common bounded error for Engine reporting. Earlier insertions remain handed off and later current-turn Commands are not attempted.

### 13.4 Closure And Abandonment

`__close_command_production` closes all four unique Command producers without invoking Port code. `__abandon_events` drains each Event consumer with a loop bounded by that Slot's configured Event capacity. Event staging must already be logically closed before abandonment.

## 14. Application, Context, And Turn Storage

The application associates itself with one topology:

```rust
pub trait Application: Sized + 'static {
    type Topology: Topology;
    type State: 'static;
    type FatalReason: 'static;

    const MAX_EVENT_ENCODING: usize;
    const MAX_COMMAND_ENCODING: usize;
    const MAX_FATAL_REASON_ENCODING: usize;

    fn initial_state(&self) -> Self::State;

    fn on_event(
        &self,
        state: &mut Self::State,
        event: &EventEnvelope<AppEvent<Self::Topology>>,
        ctx: &mut Context<'_, Self>,
    ) -> Outcome<Self::FatalReason>;

    fn encode_event(
        &self,
        event: &AppEvent<Self::Topology>,
        output: &mut EncodeBuffer,
    ) -> Result<(), EncodeError>;

    fn encode_command(
        &self,
        command: &<Self::Topology as Topology>::Command,
        output: &mut EncodeBuffer,
    ) -> Result<(), EncodeError>;

    fn encode_fatal_reason(
        &self,
        reason: &Self::FatalReason,
        output: &mut EncodeBuffer,
    ) -> Result<(), EncodeError>;
}
```

The Application value is the concrete encoder owner. Engine stores it, construction validates all three declared maxima, and `Context<'_, A>` holds an immutable reference to it while staging Commands. There is no additional encoder generic, registry, trait object, or unresolved ownership choice.

```rust
pub struct Context<'turn, A: Application> {
    application: &'turn A,
    commands: &'turn mut TurnCommands<
        <A::Topology as Topology>::Command,
    >,
    reporter: &'turn FatalReporter,
    production: &'turn CommandProductionGate,
}
```

Turn storage contains one concrete topology Command type:

```rust
pub struct StagedCommand<C> {
    ordinal: CommandOrdinal,
    command: Option<C>,
    encoded: EncodedCommand,
}
```

The `Option` lets the Engine move Commands out in ordinal order without shifting a vector. The bounded turn container is fully allocated before `RunStarted`.

`Context::command` performs only:

```text
check next ordinal
-> encode complete destination-qualified Command
-> reserve one bounded staged entry
-> store Command and encoding
-> return to handler
```

It performs no Port insertion or IO. The first Context failure reports Fatal and latches staging closed; later calls stage nothing. The handler is not preempted.

## 15. Handwritten Binding Product

All implementations exist simultaneously:

```rust
pub struct TradingBindings<
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

This struct has no mode bounds. Mode is selected when a conditional binding-set implementation applies.

## 16. Bounded Port Errors

Heterogeneous implementation errors are normalized at each concrete field call, before they enter common Environment or Fatal storage:

```rust
pub trait BoundedPortError: Sized + 'static {
    const MAX_ENCODED_LEN: usize;

    fn encode(
        self,
        output: &mut FailureBuffer,
    ) -> Result<(), FailureEncodingError>;
}
```

Construction validates every declared maximum against startup, runtime-report, stop, and terminal storage. Encoding failure uses a fixed bounded technical fallback. No Event or Command payload is erased to aggregate lifecycle errors.

## 17. Live Port API

A LivePort is active: after start, its worker owns the run-scoped session and may execute concurrently.

```rust
pub trait LivePort<C: PortContract>: Sized + Send + 'static
where
    C::Event: Send,
    C::Command: Send,
{
    type StartError: BoundedPortError;
    type RuntimeError: BoundedPortError + Send;
    type StopError: BoundedPortError;

    fn start(
        &mut self,
        session: LivePortSession<C, Self::RuntimeError>,
    ) -> Result<(), Self::StartError>;

    fn rollback_start(&mut self);

    fn stop(&mut self) -> Result<(), Self::StopError>;
}
```

Contracts:

- `start(Err)` returns only after that binding has reclaimed all capabilities and stopped every activity it began.
- `rollback_start` is called only for a binding whose `start` succeeded but whose aggregate Environment start later failed. It returns only after quiescence and has no recoverable result.
- `stop(Ok)` returns only when that binding will make no later use of run-scoped Event, Command, ordinary audit, or Fatal-reporting interfaces.
- `stop(Err)` is normalized into aggregate Environment stop failure; the Environment continues visiting remaining bindings.
- Abort is not a LivePort callback. Generic run control signals abort nonblockingly after Fatal establishment.

The session is typed by one Contract:

```rust
pub struct LivePortSession<C, E>
where
    C: PortContract,
    E: BoundedPortError,
{
    events: EventProducer<C::Event>,
    commands: Consumer<CommandEnvelope<C::Command>>,
    terminal: TerminalConsumer,
    failures: RuntimeFailureProducer<E>,
}
```

Its public behavior is conceptually:

```rust
impl<C, E> LivePortSession<C, E>
where
    C: PortContract,
    E: BoundedPortError,
{
    pub fn wait_until_published(&self) -> PublicationResult;
    pub fn offer(&mut self, event: C::Event) -> OfferResult<C::Event>;
    pub fn try_command(&mut self) -> Result<Option<C::Command>, RunClosed>;
    pub fn terminal_state(&self) -> TerminalState;
    pub fn report_fatal(&mut self, error: E) -> ReportResult;
}
```

The session exposes no Slot constructor, topology Event, AppState, Engine, handler, AuditWriter, logical acceptance clock, or Environment mode.

All sessions share preallocated state with these monotonic phases:

```text
Unpublished -> Running -> StopRequested -> Stopped
Unpublished -> StartupCancelled
Running or StopRequested -> Aborted
```

A worker launched by `start` first waits in `wait_until_published`. `Running` releases it; `StartupCancelled` tells it to terminate without using run-scoped interfaces. Event offers, Command reads, ordinary audit, and runtime Fatal reporting are unavailable in `Unpublished`. This makes successful aggregate `Environment::start` the single publication point rather than merely a documented convention. Blocking in this worker-side wait is outside Core's bounded-work guarantee.

## 18. Live Binding Set And Transactional Start

The generic Environment uses this hidden static-dispatch trait:

```rust
pub trait LiveBindingSet: Sized + 'static {
    type Topology: Topology;

    #[doc(hidden)]
    fn __validate_error_limits(
        limits: &FailureStorageLimits,
    ) -> Result<(), ConstructionError>;

    #[doc(hidden)]
    fn __start_all(
        &mut self,
        queues: <Self::Topology as Topology>::BindingQueues,
        services: &LiveRunServices,
    ) -> Result<(), EnvironmentStartError>;

    #[doc(hidden)]
    fn __stop_all(
        &mut self,
        errors: &mut StopErrorAccumulator,
    );
}
```

The handwritten conditional implementation is:

```rust
impl<P, S, X, Tm> LiveBindingSet for TradingBindings<P, S, X, Tm>
where
    P: LivePort<MarketData>,
    S: LivePort<MarketData>,
    X: LivePort<Execution>,
    Tm: LivePort<Timer>,
    <MarketData as PortContract>::Event: Send,
    <MarketData as PortContract>::Command: Send,
    <Execution as PortContract>::Event: Send,
    <Execution as PortContract>::Command: Send,
    <Timer as PortContract>::Event: Send,
    <Timer as PortContract>::Command: Send,
{
    type Topology = TradingTopology;

    // Methods use the direct field calls below.
}
```

`__start_all` destructures `TradingBindingQueues` by value, constructs one typed session per field through `BindingSlotQueues::__into_live_session`, and starts in manifest order:

```text
start Primary
-> start Secondary
-> start Execution
-> start Timer
```

All sessions remain behind one closed startup publication gate. If Execution fails, startup performs:

```text
failed Execution has already cleaned itself
-> rollback Secondary
-> rollback Primary
-> close all unpublished capabilities
-> return pre-run StartupError
```

The handwritten implementation tracks concrete started flags; it never stores heterogeneous references:

```rust
let mut primary_started = false;
let mut secondary_started = false;
let mut execution_started = false;

if let Err(error) = start_one(&mut self.primary, primary_session) {
    return Err(error);
}
primary_started = true;

if let Err(error) = start_one(&mut self.secondary, secondary_session) {
    if primary_started {
        rollback_one(&mut self.primary);
    }
    return Err(error);
}
secondary_started = true;

// Execution and Timer repeat the same pattern. Each failure arm calls
// rollback_one for successful earlier fields in exact reverse order.
```

`start_one` and `rollback_one` are generic library helpers that own session-state checks, error normalization, cancellation, and quiescence contracts. The topology code owns only concrete forward calls, boolean progress, and concrete reverse calls. It uses no trait object, erased pointer, stored heterogeneous borrow, or unsafe code. After failure, generic cleanup closes every endpoint that was never moved into a successful worker.

After every field succeeds, opening the publication gate is the single publication point and `Environment::start` returns success. No Port Event may be accepted before Ready completes, although live Events may stage after publication and wait in their bounded inboxes.

`LiveEnvironment::stop` commits and broadcasts `StopRequested` through shared services before calling `__stop_all`. The binding method then visits every field in declaration order through generic `stop_one`, normalizes failures into bounded storage, and does not stop after the first failure. It has no lifecycle-transition authority of its own.

## 19. Live Environment

```rust
pub struct LiveEnvironment<B>
where
    B: LiveBindingSet,
{
    bindings: B,
    pending_queues: Option<<B::Topology as Topology>::BindingQueues>,
    services: LiveRunServices,
    lifecycle: LiveLifecycle,
    last_selected: Option<SlotOrdinal>,
    clock: LiveLogicalClock,
}
```

`pending_queues` exists only before successful start. `start` consumes it through `B::__start_all`. A failed start leaves the Environment inert and non-restartable.

`stop` creates a preallocated `StopErrorAccumulator`, calls `B::__stop_all`, and converts an empty accumulator to `Ok(())` or a nonempty accumulator to one bounded `Environment::StopError` after every field has been visited. Error order is declaration order; the first is primary within the stop error and later retained entries are diagnostic. This Environment error is then reported through the run Fatal inbox by Engine at the v7 post-stop boundary.

The simplest frozen live selection policy is bounded round-robin in manifest order. With no previous selection, scanning begins at Slot zero. Otherwise it begins after `last_selected`. One poll inspects at most `SLOT_COUNT` Event heads and returns at most one candidate. `last_selected` updates when a candidate is selected, not when accepted; this policy state is a Core-visible Environment result and is frozen for the run. Fixed-priority scanning is not selected because it permits starvation.

`LiveEnvironment::abort` performs only a bounded nonblocking transition on generic run control. It invokes no binding callback, waits for no worker, retries nothing, and reports no second Fatal. Shared preallocated queue backing remains alive through worker-owned endpoints if cleanup continues after Engine exit.

## 20. Simulated Port API

A SimPort is a passive synchronous state machine:

```rust
pub trait SimPort<C: PortContract>: Sized + 'static {
    type StartError: BoundedPortError;
    type RuntimeError: BoundedPortError;
    type StopError: BoundedPortError;

    fn start(
        &mut self,
        ctx: &mut SimStartContext<'_, C>,
    ) -> Result<(), Self::StartError>;

    fn on_command(
        &mut self,
        ctx: &mut SimContext<'_, C>,
        command: C::Command,
    ) -> Result<(), Self::RuntimeError>;

    fn step(
        &mut self,
        ctx: &mut SimContext<'_, C>,
    ) -> Result<Option<C::Event>, Self::RuntimeError>;

    fn stop(
        &mut self,
        ctx: &mut SimStopContext<'_, C>,
    ) -> Result<(), Self::StopError>;
}
```

Borrowed contexts cannot be retained safely. Authority is restricted:

| Context | Authority |
|---|---|
| `SimStartContext<C>` | Read `now`, replace or clear this Slot's cursor |
| `SimContext<C>` | Read `now`, replace or clear this Slot's cursor |
| `SimStopContext<C>` | Read `now`; privately drain or abandon already handed-off Commands; no Event or cursor publication |

`set_next(time)` rejects time before `now`; equal time is valid. Context stores the first Core failure internally, so ignoring a returned `Result` cannot hide failure. During startup it makes aggregate start fail transactionally. During a run it reports Fatal.

The concrete contexts hold only borrowed authority:

```rust
pub struct SimStartContext<'a, C: PortContract> {
    now: LogicalTime,
    cursor: &'a mut Option<LogicalTime>,
    failure: &'a mut Option<SimContextFailure>,
    marker: PhantomData<fn() -> C>,
}

pub struct SimContext<'a, C: PortContract> {
    now: LogicalTime,
    cursor: &'a mut Option<LogicalTime>,
    failure: &'a mut Option<SimContextFailure>,
    reporter: &'a FatalReporter,
    marker: PhantomData<fn() -> C>,
}

pub struct SimStopContext<'a, C: PortContract> {
    now: LogicalTime,
    commands: &'a mut Consumer<CommandEnvelope<C::Command>>,
    disposition: &'a mut StopCommandDisposition,
    marker: PhantomData<fn() -> C>,
}
```

Runtime `set_next` and `clear_next` write the first failure into the callback-local `failure` slot rather than querying global Fatal state. The generic callback wrapper examines that slot immediately after return and reports it exactly once.

`SimStopContext` cannot publish Events or cursors. Its concrete public methods are:

```rust
impl<C: PortContract> SimStopContext<'_, C> {
    pub fn now(&self) -> LogicalTime;

    pub fn try_command(
        &mut self,
    ) -> Result<Option<C::Command>, RunClosed>;

    pub fn abandon_remaining(&mut self);
}
```

Reading each Command records a drained disposition; `abandon_remaining` drains the bounded remainder and records abandonment. After `SimPort::stop` returns, the generic stop wrapper verifies the queue is empty and a disposition exists for every entry. Undisposed Commands produce a bounded `UndisposedStopCommands` Environment stop error. Kavod does not interpret draining or abandonment as external effect. This handles Commands produced by the Stop turn without adding another normal Environment poll.

When a callback both returns an implementation error and leaves a Context failure, the Context/Core failure is reported first because it committed during callback execution; the implementation error is then retained as a secondary report when capacity permits. Startup uses the same deterministic precedence in its bounded startup error.

## 21. Generic Simulation State

Homogeneous simulation metadata does not need generated fields:

```rust
pub struct SimSchedule {
    cursors: Box<[Option<LogicalTime>]>,
    now: LogicalTime,
    callbacks_at_now: usize,
    max_callbacks_at_one_time: usize,
}
```

Mode dispatch returns small non-payload classifications:

```rust
pub enum SimDispatchResult {
    Completed,
    FatalReported,
}

pub enum SimStepResult {
    NoEvent,
    EventStaged(SlotOrdinal),
    FatalReported,
}
```

Topology-specific callback helpers normalize and report runtime, Context, queue, and invalid-Slot failures exactly once before returning `FatalReported`. The Environment then stops the current poll and lets Engine establish Fatal at the post-poll boundary.

The topology-specific match contains no error policy. Each arm calls one generic typed wrapper:

```rust
__call_sim_start::<MarketData, P>(/* binding, context inputs */)
__call_sim_command::<MarketData, P>(/* binding, envelope, context inputs */)
__call_sim_step::<MarketData, P>(/* binding, endpoint, context inputs */)
__call_sim_stop::<MarketData, P>(/* binding, endpoint, context inputs */)
```

These library helpers are monomorphized over `C` and `P`, so they can encode `P::StartError`, `P::RuntimeError`, and `P::StopError` without erasure. They own callback-local failure precedence, normalization, Fatal reporting, callback-count checks, Event staging result handling, and stop-disposition validation. Handwritten or generated code only selects the concrete field and calls the helper.

Construction allocates exactly `Topology::SLOT_COUNT` cursor entries and validates all index and callback-count domains before start.

The selected equal-time cursor policy is:

```text
minimum (cursor time, Slot ordinal)
```

Before invoking `step`, SimEnvironment advances `now` to the selected time and clears that cursor. The callback must publish a replacement for later work.

`callbacks_at_now` resets to zero only when `now` advances strictly. Equal-time Commands and steps across any number of polls share the same checked count. The Environment checks and increments the counter before dequeuing a Command or invoking a callback; exhaustion reports Fatal and begins no later callback in that poll.

## 22. Simulated Binding Set

```rust
pub trait SimBindingSet: Sized + 'static {
    type Topology: Topology;

    #[doc(hidden)]
    fn __validate_error_limits(
        limits: &FailureStorageLimits,
    ) -> Result<(), ConstructionError>;

    #[doc(hidden)]
    fn __start_all(
        &mut self,
        queues: &mut <Self::Topology as Topology>::BindingQueues,
        schedule: &mut SimSchedule,
        services: &SimRunServices,
    ) -> Result<(), EnvironmentStartError>;

    #[doc(hidden)]
    fn __next_command_key(
        queues: &<Self::Topology as Topology>::BindingQueues,
    ) -> Option<(SlotOrdinal, HandoffKey)>;

    #[doc(hidden)]
    fn __deliver_command(
        &mut self,
        queues: &mut <Self::Topology as Topology>::BindingQueues,
        slot: SlotOrdinal,
        schedule: &mut SimSchedule,
        services: &SimRunServices,
    ) -> SimDispatchResult;

    #[doc(hidden)]
    fn __step(
        &mut self,
        queues: &mut <Self::Topology as Topology>::BindingQueues,
        slot: SlotOrdinal,
        schedule: &mut SimSchedule,
        services: &SimRunServices,
    ) -> SimStepResult;

    #[doc(hidden)]
    fn __stop_all(
        &mut self,
        queues: &mut <Self::Topology as Topology>::BindingQueues,
        schedule: &SimSchedule,
        errors: &mut StopErrorAccumulator,
    );
}
```

The handwritten conditional implementation is:

```rust
impl<P, S, X, Tm> SimBindingSet for TradingBindings<P, S, X, Tm>
where
    P: SimPort<MarketData>,
    S: SimPort<MarketData>,
    X: SimPort<Execution>,
    Tm: SimPort<Timer>,
{
    type Topology = TradingTopology;

    // Methods use direct field calls and exhaustive matches.
}
```

This implementation is unrelated to the `LiveBindingSet` implementation. A concrete `TradingBindings<P, S, X, Tm>` implements only the mode whose complete `where` clause is satisfied. A mixed live/sim product implements neither. A product whose every field supports both modes may implement both traits.

Each conditional implementation's `__validate_error_limits` directly checks the `MAX_ENCODED_LEN` of every field's Start, Runtime, and Stop error types using checked aggregate arithmetic. The builders call it before queue allocation or Port activity.

### 22.1 Global Command Merge

`__next_command_key` peeks all four typed queue heads and returns the smallest `HandoffKey`. It must not drain one Slot completely before another.

```text
inspect Primary front key
inspect Secondary front key
inspect Execution front key
inspect Timer front key
-> choose the minimum key
```

Equal keys are an invariant violation. `__deliver_command` pops only the selected typed queue and calls only its corresponding `on_command` method. The loop is bounded by the total configured Command inbox capacity and checked same-time callback limit.

### 22.2 Simulated Step

`__step` exhaustively selects one binding. `Some(event)` is offered through that Slot's typed Event producer. It must enter the same bounded Event inbox used in live mode before a candidate is returned. `None` means private progress only.

Simulation startup calls every `start` in declaration order. Because callbacks are synchronous, retain no context, and may start no external activity, failed startup marks and discards the entire SimEnvironment; no rollback callback is required.

Simulation stop visits all bindings with restricted stop contexts and accumulates bounded failures. Simulation abort clears generic scheduling state and invokes no SimPort callback.

## 23. Simulated Environment Poll

`SimEnvironment` owns the passive bindings and their endpoint half for the complete run:

```rust
pub struct SimEnvironment<B>
where
    B: SimBindingSet,
{
    bindings: B,
    queues: <B::Topology as Topology>::BindingQueues,
    schedule: SimSchedule,
    services: SimRunServices,
    lifecycle: SimLifecycle,
}
```

Its constructor is private to `build_sim`. `start` calls `B::__start_all`, marks a failed instance inert and non-restartable, and publishes the running phase only after every callback succeeds and every Context failure check passes. `stop` passes the binding queues into `B::__stop_all`, converts the accumulator into one bounded `Environment::StopError`, and succeeds only if every binding completed its private shutdown contract. `abort` closes/abandons generic queues and schedule state without invoking a callback.

One admitted `SimEnvironment::next_event` does bounded work in this order:

```text
while a Command head exists:
    choose globally smallest HandoffKey
    check same-time callback bound
    invoke exactly one on_command
    on failure report Fatal and stop this poll

if no Fatal was reported:
    choose minimum (cursor time, Slot ordinal)
    if one exists:
        advance now and clear cursor
        check same-time callback bound
        invoke at most one step
        if Some(Event): stage it through the Slot Event inbox

inspect Event heads
-> return at most one non-owning candidate
```

Ready Commands therefore reach SimPorts before the first cursor selection. No cursor is stepped before the Ready turn completes because the Engine performs no Port poll until Ready completes.

## 24. Non-Owning Candidate And Environment Trait

The candidate contains only private derived metadata:

```rust
pub struct NextEvent<T: Topology> {
    slot: SlotOrdinal,
    marker: PhantomData<fn() -> T>,
}
```

Implement `Copy` and `Clone` manually so derives do not accidentally require `T: Copy`.

Environment receives only a read-only Event-head facade:

```rust
pub struct EventHeads<'a, T: Topology> {
    queues: &'a T::CoreQueues,
}
```

```rust
impl<T: Topology> EventHeads<'_, T> {
    pub fn is_ready(&self, slot: SlotOrdinal) -> Result<bool, InvalidSlot>;

    pub fn candidate(
        &self,
        slot: SlotOrdinal,
    ) -> Result<Option<NextEvent<T>>, InvalidSlot>;
}
```

There is no dequeue method on `EventHeads`.

```rust
pub trait Environment {
    type Topology: Topology;
    type StartError;
    type StopError;

    fn start(&mut self) -> Result<(), Self::StartError>;

    fn next_event(
        &mut self,
        heads: EventHeads<'_, Self::Topology>,
    ) -> Option<NextEvent<Self::Topology>>;

    fn now(&self) -> LogicalTime;
    fn stop(&mut self) -> Result<(), Self::StopError>;
    fn abort(&mut self);
}
```

A polling failure reports through the Environment's bounded Fatal reporter and returns `None`. The Engine processes the post-poll Fatal boundary before interpreting `None` as idle.

The selected head remains present because Engine is its sole consumer. Producers may append but cannot remove it. Polling therefore does not release Event capacity.

## 25. Engine Ownership And Construction

```rust
pub struct Engine<A, E>
where
    A: Application,
    E: Environment<Topology = A::Topology>,
{
    application: A,
    state: A::State,
    environment: E,
    queues: <A::Topology as Topology>::CoreQueues,
    turn_commands: TurnCommands<<A::Topology as Topology>::Command>,
    last_time: Option<LogicalTime>,
    next_event_index: EventIndex,
    // Audit worker, Fatal inbox, gates, notifier, terminal reserve.
}
```

Public builders allocate and couple both endpoint halves so callers cannot mix queues from separate runs:

```rust
pub fn build_live<A, B>(
    application: A,
    bindings: B,
    capacities: <A::Topology as Topology>::Capacities,
    config: EngineConfig,
) -> Result<Engine<A, LiveEnvironment<B>>, ConstructionError>
where
    A: Application,
    B: LiveBindingSet<Topology = A::Topology>;
```

```rust
pub fn build_sim<A, B>(
    application: A,
    bindings: B,
    capacities: <A::Topology as Topology>::Capacities,
    config: SimEngineConfig,
) -> Result<Engine<A, SimEnvironment<B>>, ConstructionError>
where
    A: Application,
    B: SimBindingSet<Topology = A::Topology>;
```

Do not expose a public Environment constructor that accepts separately created Core and binding queue products.

## 26. Exact Event Path

The Engine follows v7 in this order:

```text
process pre-poll Fatal boundary
-> admit one bounded Environment poll
-> Environment selects but does not remove one head
-> process post-poll Fatal boundary
-> if Fatal: abandon candidate metadata; queued Event remains unaccepted
-> if None: perform race-safe idle check before waiting
-> if candidate admitted:
     read and validate Environment::now
     remove exactly one selected Event through Topology::__take_event
     wrap it with authoritative Slot variant
     assign checked Event index and frozen time
     encode complete AppEvent and source
     submit Sync(EventAccepted)
     invoke on_event exactly once
```

Logical-time domain validation or regression occurs before removal, so the selected Event remains queued. `Environment::now` itself is an infallible read; validation may fail. Invalid candidate or removal failure reports Fatal and invokes no handler. Index, encoding, capacity, or EventAccepted submission failure after removal abandons that removed Event and invokes no handler. An accepted Event is never retried.

There is no Fatal boundary between successful `EventAccepted` submission and handler invocation. Reports committed during that admitted action are processed after normal handler return.

## 27. Exact Command Path

After handler return:

```text
translate Outcome::Fatal into one Application Fatal report
-> process post-handler Fatal boundary
-> if Commands exist:
     create zeroed topology-specific CommandNeeds
     count every staged Command with checked arithmetic
     preflight every destination before any insertion
     submit Sync(CommandsPrepared) with complete ordered intent
     if counting, preflight, encoding, or submission fails:
         report Fatal; process boundary; insert no current-turn Command
     for each ordinal in order:
         create HandoffKey(EventIndex, CommandOrdinal)
         admit one insertion plus CommandAccepted action
         move Command from staged entry
         insert through Topology::__push_command
         submit NoSync(CommandAccepted)
         if insertion or evidence submission fails:
             report Fatal; process boundary; retain earlier handoffs;
             skip every later current-turn Command
-> Continue: submit Sync(TurnCompleted) and poll again
-> Stop: enter the exact v7 Stop path
```

Each insertion and its following `CommandAccepted` submission are one admitted action. A failure does not begin another normal action before the next Fatal boundary. `TurnCompleted` is attempted only after every insertion and evidence submission succeeds; its failure leaves all handoffs real but does not establish turn completion.

The successful queue insertion is the handoff point. It proves neither Port processing nor external effect. Every externally consequential Command still requires application-owned business identity or idempotency information.

## 28. Startup, Stop, Fatal, And Notifier Integration

### 28.1 Startup

Construction validates topology shape, all queue and byte capacities, encoder maxima, lifecycle storage, simulation cursors, notifier resources, audit storage, terminal reserve, and identifier domains before activity begins. Runtime topology validation requires manifest length equal to `SLOT_COUNT`, contiguous unique ordinals from zero, and representability in `SlotOrdinal`. General construction cannot instantiate every Event or Command payload, especially uninhabited protocols, so correspondence between classification/routing arms and manifest entries is established by exhaustive handwritten code plus compile and runtime routing tests, not by a fictitious runtime reflection pass. Mode-specific binding error maxima are validated through `__validate_error_limits`.

Runtime startup remains:

```text
transactional Environment::start
-> immediate Sync(RunStarted) attempt with no intervening Fatal boundary
-> first Fatal boundary
-> admit Ready acceptance action:
     read and validate Environment::now
     reserve checked Event index zero
     construct AppEvent::Ready with Engine source and frozen time
     encode the complete Event
     submit Sync(EventAccepted)
     invoke on_event exactly once
-> process Ready Outcome and Commands through the common turn path
```

Ready never uses an Event inbox or `next_event`. Successful Ready acceptance initializes `last_time`, and the next available checked Event index becomes one. Encoding, index, clock validation, or `EventAccepted` submission failure reports Fatal and invokes no handler. Ready Stop enters the ordinary Stop path after processing Ready Commands. Ready Commands are handed off before any Port poll, so SimEnvironment delivers them before its first cursor selection.

`Environment::start` failure is a distinct pre-run `StartupError`, not a construction error or `EngineExit`. Construction has already succeeded, but failed start leaves no run-scoped activity or interface live.

### 28.2 Stop

After ordinary current-turn Command processing:

```text
admit closure of Event staging and Command production
-> abandon unaccepted Events
-> boundary
-> admit NoSync(EnvironmentStopStarted) plus aggregate Environment::stop
     if evidence submission fails: report Fatal; do not call stop
     if stop fails: report Fatal
-> process post-stop Fatal boundary
-> admit NoSync(EnvironmentStopped); on failure report Fatal
-> boundary
-> admit ordinary-audit closure
-> boundary
-> admit reserved append and Sync(TurnCompleted with Stop)
     on append or synchronization failure report Fatal
-> final boundary atomically commits Stopped or observes a racing Fatal report
```

Every line marked as an admitted action reaches its defined success or failure boundary before another normal action begins. A successfully appended Stop record may therefore be followed by Fatal if terminal synchronization fails or a report wins the final boundary. Only returned `EngineExit::Stopped` proves observed successful stop synchronization and final Stopped commitment.

### 28.3 Fatal

Only Engine establishes Fatal. On establishment it logically closes Fatal reporting, ordinary audit submission, Event staging, and Command production at the one v7 boundary. Topology helpers perform physical queue closure and abandonment only after that decision.

Environment abort is generic infrastructure control. It invokes no LivePort or SimPort callback.

Fatal finalization is implemented completely in the generic Engine:

```text
interfaces are already logically closed and reports are frozen in commit order
-> call Environment::abort once
-> finish the accepted ordinary audit prefix
-> append one reserved Sync(Fatal) containing frozen reports
-> make exactly one final synchronization attempt
-> join AuditWorker
-> return EngineExit::Fatal with State, reports, and audit disposition
```

If the final synchronization succeeds, audit disposition is `Synced`. If it fails, the worker appends fixed `FatalSyncFailed` only to the returned preallocated pending buffer, performs no additional writer call, submits no new report, preserves the original primary cause, joins, and returns `FatalAudit::Unsynchronized { pending, sync_error }`.

### 28.4 Race-Safe Notification

The notifier is level-assisted rather than an ordering channel. Runnable levels are:

- Any Event inbox nonempty.
- Any simulation cursor published.
- Fatal inbox nonempty.
- Any other declared Environment work source.

Before sleeping, Engine clears the coalesced notification token, rescans every level with acquire ordering, and sleeps only if all remain idle and no new token committed. Publication first commits work, then sets the token and wakes Engine. Spurious and coalesced wakes are valid.

The notifier carries no Event order, source, logical time, or Fatal authority.

## 29. Verification And Manual Implementation Order

Implement in this order:

1. Contract markers and protocol types.
2. `TradingTopology`, `TradingPortEvent`, and `TradingPortCommand` by hand.
3. Checked ordinals, manifest, and named capacities.
4. Bounded SPSC queue and integrated Event staging/closure primitive.
5. Split endpoint types and four-Slot queue products.
6. Every `Topology` method and exhaustive match by hand.
7. Turn-local Command storage, encoding, counting, preflight, and handoff.
8. `TradingBindings` and conditional `LiveBindingSet` implementation.
9. Transactional live startup, reverse rollback, aggregate stop, and abort signal.
10. Conditional `SimBindingSet`, contexts, cursor scheduling, global Command merge, and step dispatch.
11. `LiveEnvironment` and `SimEnvironment` non-removing polling.
12. Generic Engine integration at every v7 Fatal and audit boundary.
13. Compile-pass, compile-fail, runtime, concurrency, and fault-injection tests.
14. Inspect the complete handwritten code and only then add `topology!`.

Required compile-pass tests:

- A separate application crate implements the complete handwritten public-hidden SPI.
- Two Slots use one Contract with different implementation types.
- Two Slots use one Contract with separate values of the same implementation type.
- Unrelated Contracts coexist.
- One binding product shape works with separately constructed live and simulation values.
- An implementation type that supports both modes can participate in either conditional set.
- Uninhabited one-sided protocols compile.
- Non-`Send` protocols compile for simulation.

Required compile-fail tests:

- Wrong Contract implementation in a live field.
- Wrong Contract implementation in a simulation field.
- Mixed live and simulation fields satisfy neither complete binding set.
- Wrong Command payload for a destination variant.
- Missing, duplicate, or unknown binding and capacity fields.
- Port session attempts to offer the topology Event rather than its Contract Event.
- Non-`Send` Event or Command protocols are rejected by live bindings.

Required runtime and fault-injection tests:

- Every Event queue maps to exactly its corresponding Event variant.
- Every Command variant maps to exactly its corresponding typed queue.
- Primary and Secondary retain distinct identity with identical payload bytes.
- Poll selection does not remove or release capacity.
- Fatal winning the post-poll boundary leaves the candidate queued until abandonment.
- Clock failure leaves the candidate queued.
- EventAccepted failure after removal does not invoke the handler or retry the Event.
- Offer-versus-closure exercises both linearization outcomes.
- Full Event inbox while open reports Fatal; closure winner returns `RunClosed`.
- Command preflight failure inserts no current-turn Command.
- `Full` after successful preflight is tested as an invariant panic.
- Disconnection during insertion retains earlier handoffs and skips later Commands.
- Interleaved simulated Commands are delivered by `HandoffKey`, not Slot order.
- Equal-time cursors use Slot ordinal as tie-breaker.
- Ready Commands are delivered before the first simulated step.
- Ready acceptance succeeds and fails independently at clock, index, encoding, audit submission, handler Outcome, Command, Stop, and Fatal boundaries.
- Same-time callback exhaustion reports Fatal before the excess callback.
- Same-time callback counts persist across polls and reset only after strict time advancement.
- Partial live startup rolls back successful earlier fields in reverse order.
- Worker activity racing publication or startup cancellation cannot use unpublished interfaces.
- Aggregate stop visits every binding after an earlier stop failure.
- Simulated Stop gives every already handed-off Command a private drain or abandonment disposition.
- Abort invokes no Port callback and does not wait.
- Every live-worker-held capability safely observes terminal shared state after Engine exit.
- Notifier publication in every rescan-to-sleep race prevents a lost wake.
- Command receivers distinguish empty/open from empty/closed and cannot miss a pre-closure publication.
- Queue endpoint-drop tests prove each payload is destroyed exactly once.
- Malformed manifest length, ordinal, duplication, and representability are rejected.
- Allocation guard detects any Core allocation after `RunStarted`, including Fatal and Stop.
- Every admitted v7 action boundary has a Fatal race test.

## 30. Mechanical `macro_rules!` Replacement

Only after the handwritten implementation passes Section 29 may it be replaced by:

```rust
kavod::topology! {
    pub mod trading_ports {
        primary: Primary(super::MarketData),
        secondary: Secondary(super::MarketData),
        execution: Execution(super::Execution),
        timer: Timer(super::Timer),
    }
}
```

Each declaration supplies:

```text
field_name: EventAndCommandVariant(ContractType)
```

Contract paths must resolve from inside the generated child module, so parent-module Contracts use `super::Contract` and crate-level Contracts may use `crate::path::Contract`. The field name is explicit because stable `macro_rules!` does not perform identifier case conversion. Fixed names inside the generated module avoid companion-identifier synthesis:

```text
trading_ports::Topology
trading_ports::Event
trading_ports::Command
trading_ports::Bindings<...>
trading_ports::Capacities
trading_ports::__CoreQueues
trading_ports::__BindingQueues
trading_ports::__CommandNeeds
```

The public macro pattern is:

```rust
#[macro_export]
macro_rules! topology {
    (
        $visibility:vis mod $module:ident {
            $(
                $field:ident : $variant:ident($contract:ty)
            ),+ $(,)?
        }
    ) => {
        // Recurse through private @collect and @emit arms of this same exported
        // macro, assign declaration-order ordinals, and emit the handwritten
        // expansion.
    };
}
```

Private `@collect` and `@emit` arms of the same exported macro associate each declaration with its zero-based ordinal and recurse through `$crate::topology!`. This avoids relying on an inaccessible helper macro after cross-crate expansion. The user never writes an ordinal. The final emission repeats over records containing:

```text
(ordinal expression, field, variant, Contract)
```

The macro must generate exactly:

1. Empty topology marker.
2. Source-qualified Event enum.
3. Destination-qualified Command enum.
4. Named generic binding product.
5. Named capacity product.
6. Hidden Core queue product.
7. Hidden binding queue product.
8. Hidden Command-needs product.
9. Explicit manifest entries and ordinals.
10. Complete `Topology` implementation.
11. Conditional `LiveBindingSet` implementation.
12. Conditional `SimBindingSet` implementation.
13. Direct live lifecycle visitation.
14. Direct simulation command-head merge and callback dispatch.

The macro must not generate:

- Queue algorithms.
- Event staging/closure synchronization.
- Engine turn sequencing.
- Audit submission or synchronization policy.
- Fatal reporting or establishment.
- Startup state machines or error-normalization rules.
- Stop or Fatal precedence.
- Notifier algorithms.
- Simulation clock or callback-bound algorithms.

Those remain generic library code called by the emitted field-by-field glue.

Exported macro expansion refers to Kavod internals through `$crate::__private`, not a hard-coded crate name. Required support types and traits are `#[doc(hidden)] pub` because expansion occurs in the application crate. Their fields and constructors remain private wherever ownership permits.

Macro tests compare behavior with the handwritten reference topology:

- Same manifest and ordinals.
- Same Event and Command routing.
- Same live and simulation trait-bound acceptance and rejection.
- Same capacity allocation and independent queues.
- Same lifecycle visitation order.
- Same simulation Command merge and cursor dispatch.
- No `dyn`, `Any`, downcast, runtime registry, or user-visible routing key.
- Expansion remains linear in Slot count.

Do not delete the handwritten reference topology after introducing the macro. Keep it as the readable oracle used to test generated behavior.

## 31. Final Review Checklist

Before implementation is accepted, verify:

- Polling never removes an Event.
- Source is derived from the dequeue arm, not candidate or payload data.
- Destination is derived from the Command variant, not a separate argument.
- Port sessions expose only Contract protocols.
- Core is mechanically the sole Command producer.
- Every current-turn destination is preflighted before any insertion.
- Simulation merges per-Slot Command heads by global handoff key.
- Simulated Events pass through the common Event inbox.
- Live startup has a real reverse rollback path.
- Successful stop proves no later run-scoped capability use.
- Abort invokes no user callback.
- Queue storage survives non-joining Fatal abort.
- Every runtime container and loop has a validated finite bound.
- No Core-managed allocation occurs after `RunStarted`.
- Generated/manual glue contains no semantic Fatal or audit policy.
- The macro is optional: deleting it leaves a complete implementable manual design.

## 32. Conclusion

The implementation path is deliberately ordinary:

```text
write Contracts
-> write one topology marker
-> write Event and Command enums
-> write named capacity, binding, and queue products
-> write exhaustive routing matches
-> implement conditional live and simulation binding traits
-> integrate with one generic Engine
-> test all v7 boundaries
-> mechanically replace repetition with macro_rules!
```

The macro is not the architecture. The handwritten Rust types, ownership boundaries, queue semantics, Environment behavior, and Engine protocol are the architecture. `topology!` is accepted only when it demonstrably emits that same reviewed implementation.
