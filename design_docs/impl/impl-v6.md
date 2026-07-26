# Kavod v6 Implementation Design

> **Status:** Early API and implementation direction
> **Authority:** `design_docs/design-v6.md` remains normative
> **Scope:** Protocol descriptors, application graph construction, and private runtime erasure

## 1. Selected Direction

Kavod v6 will use:

1. Per-Port closed Event and Command enums.
2. Typed descriptor constants for individual protocol variants.
3. Leaf-payload callback signatures with no application-side enum matching.
4. Explicit stable protocol and variant identities.
5. Dense run-local indices after graph validation.
6. Narrow private type erasure for heterogeneous payload, callback, Component-state, and endpoint storage.
7. Checked downcasts only. An impossible mismatch is Engine-fatal.

The division of responsibility is:

```text
descriptor constants      define semantic identity and payload typing
dense runtime indices     provide efficient frozen lookup
private type erasure      stores heterogeneous runtime values
stable explicit IDs       identify protocols and variants in audit evidence
```

`TypeId`, Rust type names, enum discriminants, pointer addresses, and dense runtime indices are never durable or audit identities.

## 2. Public Protocol Shape

A logical Port associates one immutable identity with one Event protocol and one Command protocol:

```rust
pub trait PortSpec: 'static {
    const ID: PortId;

    type Events: Send + 'static;
    type Commands: Send + 'static;
}
```

Example:

```rust
pub struct MarketData;

impl PortSpec for MarketData {
    const ID: PortId = PortId::new("market-data");

    type Events = MarketDataEvent;
    type Commands = MarketDataCommand;
}

pub enum MarketDataEvent {
    Bar(Bar),
    Tick(Tick),
    Correction(Bar),
}

pub enum MarketDataCommand {
    Subscribe(Subscription),
    RequestSnapshot(InstrumentId),
}
```

The same payload type may appear in several variants or several Port protocols. Routing never uses the payload type alone.

One-sided Ports use explicit uninhabited types rather than `()`:

```rust
pub enum NoEvents {}
pub enum NoCommands {}
```

## 3. Manual Descriptor Primitive

The non-macro semantic primitive is a typed, opaque variant descriptor:

```rust
pub struct EventVariant<E, T> {
    // Private representation.
    _protocol: PhantomData<fn(E) -> E>,
    _payload: PhantomData<fn(T) -> T>,
}
```

Conceptually, one descriptor supplies:

```text
protocol identity
variant identity
payload schema identity
payload Rust type
injection into the protocol enum
projection from the protocol enum
audit encoding entry
manifest membership
```

The public representation stays opaque. In particular, it does not expose:

```text
TypeId
function pointers
manifest offsets
dense runtime indices
codec vtables
raw route keys
```

Event descriptors are Port-neutral so one protocol enum can be reused by several logical Ports. The Port is supplied at registration or staging:

```rust
component
    .events(PrimaryFeed)
    .on(MarketDataEvent::BAR, Strategy::on_primary_bar);

component
    .events(BackupFeed)
    .on(MarketDataEvent::BAR, Strategy::on_backup_bar);
```

The builder statically requires:

```rust
PrimaryFeed: PortSpec<Events = MarketDataEvent>
BackupFeed: PortSpec<Events = MarketDataEvent>
```

Message and Command variants use corresponding typed descriptors:

```rust
pub struct MessageVariant<M, T> { /* private */ }
pub struct CommandVariant<C, T> { /* private */ }
```

## 4. Generated Ergonomic Layer

After the manual descriptor contract is compile-proven, a derive should generate descriptors and the complete protocol manifest from one enum declaration:

```rust
#[derive(KavodProtocol)]
#[kavod(
    protocol_id = "market-data.events",
    version = 1,
)]
pub enum MarketDataEvent {
    #[kavod(id = 1)]
    Bar(Bar),

    #[kavod(id = 2)]
    Tick(Tick),

    #[kavod(id = 3)]
    Correction(Bar),
}
```

The derive generates opaque constants:

```rust
MarketDataEvent::BAR
MarketDataEvent::TICK
MarketDataEvent::CORRECTION
```

It also generates, from the same enum syntax tree:

```text
an exhaustive classifier
typed injection and projection
the complete variant manifest
stable ID metadata
payload schema metadata
audit encoding adapters
diagnostic names
```

Stable IDs are explicit. They are not inferred from declaration order or Rust names. Removed IDs are never reused.

The derive is convenience and correspondence-error prevention. The underlying descriptor and graph semantics do not depend on proc-macro behavior that cannot be expressed manually.

## 5. Application Graph Syntax

The preferred Component registration syntax is:

```rust
application.component(Strategy::new(config), |component| {
    component
        .events(MarketData)
        .on(MarketDataEvent::BAR, Strategy::on_bar)
        .produces_message(AppMessage::SIGNAL)
        .produces_command(Execution, ExecutionCommand::SUBMIT);
});
```

The handler receives the leaf payload directly:

```rust
impl Strategy {
    fn on_bar(
        &mut self,
        bar: &Bar,
        state: &AppState,
        ctx: &mut ComponentCtx<'_>,
    ) {
        // No MarketDataEvent match is required here.
    }
}
```

Several callbacks on one Component share the same private state instance:

```rust
application.component(Strategy::new(config), |component| {
    component
        .events(MarketData)
        .on(MarketDataEvent::BAR, Strategy::on_bar);

    component
        .events(MarketData)
        .on(MarketDataEvent::TICK, Strategy::on_tick);
});
```

Reducer registration uses the same descriptors but a restricted callback signature:

```rust
application.reducers(|reducers| {
    reducers
        .events(MarketData)
        .on(MarketDataEvent::BAR, reduce_bar);
});

fn reduce_bar(
    state: &mut AppState,
    bar: &Bar,
    ctx: ReducerCtx,
) {
    state.apply_bar(bar);
}
```

Reducers receive no output or shutdown capability.

Engine Events remain built-in typed cases:

```rust
component.on_engine(Ready, Bootstrap::on_ready);
component
    .on_engine(ShutdownRequested, ShutdownPolicy::on_request)
    .may_shutdown();
```

Exact naming, callback IDs, and builder ownership remain to be finalized.

## 6. Callback Output Syntax

Variant-specific output authority is preferred because it follows least authority:

```rust
component
    .events(MarketData)
    .on(MarketDataEvent::BAR, Strategy::on_bar)
    .produces_message(AppMessage::SIGNAL)
    .produces_command(Execution, ExecutionCommand::SUBMIT);
```

The callback uses the same descriptors:

```rust
ctx.message(AppMessage::SIGNAL, signal);

ctx.command(
    Execution,
    ExecutionCommand::SUBMIT,
    submission,
);
```

The descriptor binds the payload type. Passing the wrong payload is a compile error.

All semantic context methods return `()`. On the first undeclared output, illegal shutdown, encoding failure, identifier exhaustion, or configured bound failure:

```text
the attempted operation stages nothing
the attempted operation consumes no ordinal
the run closes fatally
the callback context becomes inert
later semantic calls from that callback stage nothing
the active callback may return or unwind
the incomplete turn never reaches TurnComputed
```

This prevents callback behavior from branching on technical infrastructure failures.

## 7. Live And Simulation Staging

The preferred live staging API uses the Event descriptor and leaf payload:

```rust
io.stage(MarketDataEvent::BAR, bar)?;
```

The `LivePort` context supplies the logical Port. A worker cannot stage an Event for another Port through that context.

Simulation uses the same protocol descriptor:

```rust
sim.stage_at(
    time,
    MarketData,
    MarketDataEvent::BAR,
    bar,
);
```

Simulation Command endpoints may also register leaf-payload callbacks:

```rust
endpoint.on_command(
    ExecutionCommand::SUBMIT,
    ExchangeModel::on_submit,
);

fn on_submit(
    model: &mut ExchangeModel,
    submission: &Submission,
    ctx: &mut SimulationCtx<'_>,
) {
    // No ExecutionCommand match is required here.
}
```

Live and simulation share descriptor meaning, Port identity, and application semantics. They do not share physical runtime mechanics.

## 8. Graph Compilation

Application construction collects typed registrations and protocol manifests. Before `Ready`, it must:

```text
validate unique Port IDs
validate unique protocol and variant IDs
validate descriptor membership in the declared protocol
validate every source-qualified Event variant has a consumer
validate Ready and ShutdownRequested have consumers
validate every produced Message has a consumer
validate every Command destination and variant
validate callback-local output and shutdown declarations
validate stable Reducer and Component order
validate schemas and audit encoders
validate exactly one compatible Environment binding per Port
validate all finite limits and identifier domains
freeze the graph
assign dense run-local indices
```

Stable IDs identify audit evidence. Dense indices index frozen runtime arrays.

## 9. Private Runtime Representation

The runtime may erase heterogeneous values only after typed validation:

```rust
struct ErasedPayload {
    route: DenseRouteIndex,
    value: Box<dyn Any + Send>,
}

struct CompiledRoute<S> {
    stable_identity: StableRouteIdentity,
    expected_type: TypeId,
    reducers: Box<[ErasedReducer<S>]>,
    components: Box<[ErasedComponentCallback<S>]>,
    encoder: ErasedEncoder,
}
```

This is illustrative storage, not frozen implementation syntax. Per-value boxing should be measured and may later be replaced with bounded arenas or typed per-Port storage.

The hot path is conceptually:

```text
dense route lookup
-> checked payload downcast or generated projection
-> Reducers in stable order
-> Components in stable order
```

The runtime never routes by payload `TypeId`. `TypeId` is only a redundant, process-local checked type witness at the private erasure boundary.

All downcasts remain checked. Kavod will not use:

```text
downcast_unchecked
transmute
unreachable_unchecked
raw pointer casts for protocol dispatch
```

## 10. Failure Rules

Failures are classified by where they can still be handled truthfully:

| Failure | Treatment |
|---|---|
| Duplicate stable ID | Application build error |
| Missing Event consumer | Application build error |
| Wrong descriptor for a Port protocol | Compile or build error |
| Wrong payload at a public call | Compile error |
| Missing or incompatible binding | Environment build error |
| Descriptor or manifest encoding unavailable | Build or run-start error, depending on boundary |
| Checked downcast mismatch after validation | Fatal internal invariant violation |
| Selected projector returns no payload | Fatal internal invariant violation |
| Encoder failure before `InputAccepted` | Candidate not accepted; fatal |
| Encoder failure during Message or Command production | Output not staged; incomplete turn fatal |
| Unknown descriptor in an audit file | Typed inspection error |
| Unknown dense runtime route after freeze | Fatal internal invariant violation |

An internal mismatch never silently skips a callback and never becomes an application Event.

## 11. Audit Identity

Audit records use explicit stable identities:

```text
logical Port ID, where applicable
protocol ID
protocol version
variant ID
payload schema ID
stable callback registration reference
```

They never use:

```text
TypeId
type_name
enum discriminant
declaration position
descriptor address
function address
dense route index
hash-table hash
```

`RunStarted` records the frozen manifest, graph order, bindings, schema identities, and strong content identity required by `design-v6.md`.

Stable schema identities improve forensic tooling but do not introduce replay, restoration, or recovery authority.

## 12. Verification Gates

Before freezing this API:

1. Implement one protocol and its descriptors manually.
2. Implement the same protocol through derive generation.
3. Prove both produce the same canonical manifest.
4. Property-test injection and projection for every variant.
5. Test the same payload type in two variants of one protocol.
6. Test the same payload type through two logical Ports.
7. Differentially compare the erased dispatcher with a direct exhaustive-match reference dispatcher.
8. Force route-key hash collisions and prove equality, not hash output, controls routing.
9. Inject descriptor/payload mismatches through test-only seams and require fatal failure before callback invocation.
10. Mutation-test removal of the Port or variant dimension from route keys.
11. Compile-fail wrong payload, wrong Port, wrong direction, and escaping callback-reference cases.
12. Run Miri over projection, erased drops, callback invocation, panic cleanup, and private-state access.
13. Keep impossible-state assertions active in production.

## 13. Remaining API Decisions

The following remain open:

1. Exact `Application` and nested registrar ownership syntax.
2. Explicit callback ID syntax versus stable builder-assigned registration references.
3. Whether Port marker values or typed `PortKey<P>` values identify logical Ports.
4. Whether multiple logical instances of one `PortSpec` are supported in the MVP.
5. Exact Message protocol declaration and registration syntax.
6. Whether Command authority is always variant-specific or may optionally authorize a complete Port Command protocol.
7. Exact manual protocol-definition API used before derive generation.
8. Exact audit encoding and schema-trait APIs.
9. Whether typed per-Port queues can avoid Event payload boxing in the initial implementation.
10. Exact live worker and shared simulation-world interfaces.

These decisions must preserve the authority, audit, closure, boundedness, and failure semantics in `design_docs/design-v6.md`.
