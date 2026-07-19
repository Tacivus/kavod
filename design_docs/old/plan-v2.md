# Kavod v3 Incremental Rewrite Plan

> **Target design:** `design_docs/design-v3.md`  
> **Starting point:** the current Phase 11 implementation  
> **Strategy:** remove the orchestration code that is fighting the borrow checker, preserve the proven primitives, and rebuild upward in small compile-safe phases

---

## 1. Purpose

This plan is not a greenfield implementation plan for the entire repository.

The domain primitives are already implemented and heavily tested. They should not be rewritten merely because the engine architecture is changing.

This plan focuses on the parts that are incorrect, incomplete, or structurally inconsistent with `design-v3.md`:

- Message ownership and fan-out.
- Sequence ownership.
- Scheduler payload ownership.
- Collision-safe cache storage.
- Separate reducer, handler, and actor contexts.
- Reducer and handler registries.
- Graph metadata and validation.
- Builder/runtime separation.
- The event loop.
- Inline backtest actors.
- Live actor infrastructure.
- Ingress logging and replay boundaries.

Every phase must leave the repository compiling and its applicable tests passing.

Do not implement several phases in one large change. The purpose of the phases is to make failures local and understandable.

---

## 2. Rewrite Rules

### 2.1 Preserve Proven Code

Do not rewrite stable arithmetic and domain code unless a concrete failing test or v3 requirement proves that a change is necessary.

The following modules are considered stable for this rewrite:

| Module | Action |
|---|---|
| `decimal.rs` | Preserve implementation and tests |
| `price.rs` | Preserve implementation and tests |
| `quantity.rs` | Preserve implementation and tests |
| `time.rs` | Preserve module structure |
| `time/timestamp.rs` | Preserve implementation and tests |
| `time/duration.rs` | Preserve implementation and tests |
| `clock.rs` | Preserve initially; stop exposing clocks through contexts |
| `clock/sim.rs` | Preserve initially |
| `clock/live.rs` | Preserve initially; use only at live ingress later |

The current clock abstraction may need later refinement for live timers, but that is not a prerequisite for rebuilding the deterministic backtest core.

### 2.2 Rewrite The Broken Layer

The following modules may be deleted and rewritten without preserving their internal structures:

| Current module | Rewrite status | Reason |
|---|---|---|
| `kernel.rs` | Full rewrite | Combines registration and execution, threads an outbox, exposes old contexts, and does not enforce v3 ingress semantics |
| `context.rs` | Full rewrite | One context has the wrong capabilities, contains a clock reference, exposes sequence, and depends on an outbox |
| `handler.rs` | Full rewrite | Duplicates stateful/stateless variants, coalesces groups, scans all handlers, and exposes private implementation types |
| `reducer.rs` | Full rewrite | Uses raw `Cache` rather than `ReducerCtx` and lacks dispatch-time capability |
| `graph.rs` | Full rewrite | Is coupled directly to registries, models old routes, panics instead of returning build errors, and overstates cycle knowledge |
| `cache.rs` | Internal rewrite | Stores only key hashes, so unequal colliding keys alias |
| `schedule.rs` | Adapt, not conceptual rewrite | Heap ordering is correct; payload ownership and naming must change |
| `message.rs` | Small rewrite | Add `Sync` and shared immutable payload support |
| `log.rs` | Split and defer | Keep sequence concepts, remove the disconnected log implementation, and design durable ingress logging later |
| `lib.rs` | Rewrite exports gradually | Remove stale public modules and expose only completed APIs |

### 2.3 No Temporary Compatibility Layer

Do not preserve the old public API with aliases such as:

```rust
type Context = HandlerCtx;
type Kernel = Engine;
```

Do not keep old methods such as:

```rust
Kernel::state(...)
Context::now()
Context::seq()
Cache::get_keyed(...)
```

The rewrite branch is allowed to break the old API. Compatibility wrappers would make the new design harder to understand and would create additional removal work.

### 2.4 Compile After Every Phase

At the end of every phase, run:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

If `clippy -D warnings` is temporarily blocked by intentionally unused scaffolding, document the exact warning and remove the scaffolding before completing the phase. Do not restore crate-wide `#![allow(dead_code)]` as a permanent workaround.

### 2.5 One Semantic Change Per Test

Tests should state one invariant clearly.

Avoid tests that register an entire trading graph merely to verify one cache accessor or one registry lookup.

Use unit tests for individual modules and integration tests for public engine behavior.

### 2.6 Stop At Design Gates

Do not silently decide an open v3 design question while implementing unrelated work.

The plan contains explicit design gates. When a gate is reached, resolve and record the decision before proceeding.

---

## 3. Current Behavior Inventory

### 3.1 Behavior Worth Preserving

The following current behavior is correct and should be recreated in the new architecture:

- Scheduler ordering by `(Timestamp, SeqNo)`.
- Earliest timestamp first.
- Lower sequence first for equal timestamps.
- Same-time breadth-first cascades.
- Reducers execute in registration order.
- Reducers execute before handlers.
- Handlers execute in deterministic registration order.
- Handler-group state persists across invocations.
- Separate handler groups have isolated state.
- Handlers read the global cache immutably.
- Undeclared handler production is rejected.
- Declared output types are available to graph validation.
- Future scheduling works through `send_at`.
- Past scheduling is rejected.

### 3.2 Behavior That Must Be Removed

The following current behavior conflicts with v3:

- A message with no consumer is silently ignored.
- `Context::new` can bypass production checking.
- `Context::now()` reads a clock during callback execution.
- `Context::seq()` exposes kernel sequence state.
- Handler output is accumulated in a raw `Vec` threaded through the registry.
- Cache identity uses only a `u64` hash.
- Handler-local state is required to implement global-cache `State`.
- Stateless and stateful handlers use duplicate enums and dispatch branches.
- Public methods return crate-private handler implementation types.
- Graph validation accepts old actor routes.
- Graph validation treats all declared cycles as same-time cycles.
- Build validation happens inside `run()` rather than in a separate build phase.
- `InboundLog` exists but is never populated.
- The same `Kernel` type performs both registration and execution.

### 3.3 Existing Tests To Recreate

Old tests may be deleted with the old modules, but the following invariants must be recreated before the core rewrite is considered complete:

| Existing invariant | New test location |
|---|---|
| Scheduler timestamp ordering | `schedule.rs` unit tests |
| Scheduler sequence tie-breaking | `schedule.rs` unit tests |
| Same-time breadth-first ordering | `schedule.rs` and engine integration tests |
| Reducer registration order | `reducer.rs` unit tests |
| Reducer-before-handler visibility | Engine integration tests |
| Handler state persistence | `handler.rs` unit tests |
| Handler group isolation | `handler.rs` unit tests |
| Stateless/stateful registration order | `handler.rs` unit tests |
| Undeclared send rejection | Handler context/registry tests |
| Orphan production rejection | Graph/builder tests |
| End-to-end message cascade | Engine integration tests |
| Future scheduling | Engine integration tests |
| Past scheduling rejection | Context and engine tests |

The existing `no_handler_for_type_is_noop` test must not be preserved. Replace it with a test proving that unconsumed ingress is rejected.

---

## 4. Dependency Graph

The rewrite dependency order is:

```text
Preserved domain primitives
        │
        ├── Message + Arc payload
        │       │
        │       ├── Sequence allocator
        │       │       │
        │       │       └── Scheduler adaptation
        │       │
        │       └── Actor fan-out later
        │
        ├── Collision-safe Cache
        │       │
        │       ├── ReducerCtx
        │       └── HandlerCtx
        │
        ├── ReducerRegistry
        ├── HandlerRegistry
        ├── GraphBuilder + validation
        ├── EngineConfig + errors
        ├── EngineBuilder
        ├── Engine runtime loop
        ├── Inline backtest actors
        ├── Live actor design gate
        ├── Live actor executor
        └── Ingress log + replay
```

Do not begin actor execution before the core event loop and graph are complete.

---

## Phase 0: Establish A Compile-Safe Rewrite Baseline

### Goal

Remove the old orchestration layer without changing the proven primitives.

The repository should temporarily contain only the stable leaf modules and any module currently being rebuilt.

### Starting Point

The current crate exports all modules from `lib.rs`, including the old kernel, context, handlers, reducers, and graph.

Those modules depend on each other, so deleting only one causes widespread compile errors.

### Steps

1. Run the complete current test suite and record the passing baseline.
2. Ensure the work is recoverable through version control before deleting files.
3. Remove these module exports from `lib.rs` together in one change:

```rust
context
graph
handler
kernel
reducer
```

4. Delete the corresponding old source files or replace them only when their rewrite phase begins.
5. Keep these modules exported:

```rust
cache
clock
decimal
log
message
price
quantity
schedule
time
```

6. Remove tests that only compile through the deleted orchestration modules.
7. Do not copy old implementation code into `legacy_*` modules.
8. Add a short comment to `lib.rs` only if necessary to explain that the orchestration modules are being reintroduced incrementally.
9. Run the remaining leaf-module tests.

### Tests

- All decimal tests still pass.
- All price tests still pass.
- All quantity tests still pass.
- All timestamp and duration tests still pass.
- Clock tests still pass.
- Cache tests still pass temporarily, even though the cache will be corrected later.
- Scheduler tests still pass temporarily.
- Sequence tests still pass temporarily.

### Completion Gate

The crate compiles without the old orchestration modules.

No stable domain primitive was rewritten.

### Do Not Do Yet

- Do not add `Engine`.
- Do not add contexts.
- Do not add actors.
- Do not redesign clocks.
- Do not add compatibility aliases.

---

## Phase 1: Immutable Shared Messages

### Goal

Establish the v3 message contract and ownership model before rebuilding scheduler or dispatch code.

### Current Gap

The current trait is:

```rust
pub trait Message: Send + Debug + Any + 'static {}
```

The scheduler stores `Box<dyn Message>`. A box cannot be cheaply shared with multiple actor subscribers or an in-memory diagnostic log.

### Steps

1. Update the trait to require `Sync`:

```rust
pub trait Message: Send + Sync + Debug + Any + 'static {}
```

2. Add a crate-private shared payload alias:

```rust
pub(crate) type SharedMessage = Arc<dyn Message>;
```

3. Keep `Message` free of timestamp and sequence methods.
4. Do not add a public message envelope.
5. Add a small crate-private helper for obtaining `TypeId` if it reduces repeated casting.
6. Add a small crate-private downcast helper only if it improves diagnostics. Avoid a large abstraction around `Any`.

### Tests

- A concrete message can be stored as `Arc<dyn Message>`.
- Cloning the shared payload does not clone or move the concrete payload.
- A shared message can cross a thread boundary.
- Two threads can hold immutable clones of the same payload.
- Downcasting a shared message reference recovers the concrete type.
- Downcasting to the wrong type returns `None` or produces the intended internal invariant error.

### Completion Gate

All message tests pass with `Send + Sync`.

No module still assumes that future fan-out requires cloning concrete messages.

### Do Not Do Yet

- Do not modify scheduler ordering.
- Do not implement actors.
- Do not design durable serialization.

---

## Phase 2: Separate Sequence From Logging

### Goal

Make scheduler sequence a small independent primitive rather than part of an unused logging module.

### Current Gap

`SeqNo` lives in `log.rs`, while `InboundLog` is disconnected from runtime behavior.

Sequence is required now. Durable logging is not yet designed.

### Steps

1. Create `src/sequence.rs`.
2. Move `SeqNo` into `sequence.rs`.
3. Add a kernel-owned allocator type:

```rust
pub(crate) struct Sequence {
    current: u64,
}
```

4. Provide one allocation method:

```rust
fn next(&mut self) -> SeqNo
```

5. Use `checked_add` and fail loudly on overflow.
6. Keep `SeqNo` ordered, copyable, and opaque.
7. Do not expose sequence allocation publicly.
8. Remove the old disconnected `InboundLog` implementation from the active module graph.
9. Delete `log.rs` if it contains no remaining active functionality.
10. Reintroduce an ingress log in a later dedicated phase after durable identity and serialization are designed.

### Tests

- Initial allocation is deterministic.
- Every call returns a strictly larger value.
- Values implement `Eq`, `Ord`, and `Copy` as required by the scheduler.
- Overflow uses checked failure rather than wrapping.
- Sequence cannot be constructed through an unrestricted public constructor.

### Completion Gate

Scheduler sequence no longer depends on `InboundLog`.

There is no dead logging object pretending to provide replay.

### Open Design Boundary

This phase allocates a unique sequence per scheduled message.

Do not add reducer, handler, or actor-operation sequence increments. That policy remains open in v3.

---

## Phase 3: Adapt The Scheduler To Shared Messages

### Goal

Preserve the proven heap ordering while replacing boxed ownership with shared immutable payloads.

### Current Strength

The current `BinaryHeap` ordering implementation is correct and heavily tested.

Do not rewrite the ordering algorithm merely to make the file look new.

### Steps

1. Rename the private queued type from `Event` to `ScheduledItem` if desired by the implementation.
2. Store:

```rust
struct ScheduledItem {
    dispatch_time: Timestamp,
    sequence: SeqNo,
    payload: SharedMessage,
}
```

3. Keep the item crate-private.
4. Keep payload out of `Eq` and `Ord`.
5. Preserve reversed `BinaryHeap` ordering:

```text
earlier dispatch time first
lower sequence first when times match
```

6. Replace `push_boxed` with a method accepting `SharedMessage`.
7. Keep a generic convenience method that wraps a concrete message in `Arc`.
8. Provide `is_empty` in addition to `len` if useful.
9. Do not put past-time validation inside the scheduler. The scheduler does not know the engine's current logical time.
10. Do not let the scheduler allocate sequence values. The engine owns the allocator.

### Tests To Preserve

- New scheduler is empty.
- Push/pop round-trip.
- Earliest timestamp pops first.
- Lower sequence breaks equal-time ties.
- Push order does not affect sorted pop order.
- Timestamp dominates sequence.
- Same-time breadth-first cascade behavior.
- Payload type and value do not affect ordering.
- Large out-of-order input pops monotonically.

### New Tests

- Popped payload is an `Arc` shared with another owner.
- The same payload can be sent to the scheduler and retained elsewhere.
- Generic concrete push and shared-payload push have identical ordering behavior.

### Completion Gate

All existing scheduler ordering invariants pass with shared payloads.

The scheduler contains no `Box<dyn Message>` and no outbox-specific method.

---

## Phase 4: Rebuild The Cache With Real Keys

### Goal

Fix the cache correctness bug before any new reducer or handler API depends on it.

### Current Defect

The current cache stores:

```rust
HashMap<(TypeId, u64), Box<dyn Any + Send>>
```

Only the key hash is retained. Two unequal keys with the same hash alias each other.

`Eq` is never consulted.

### Target Representation

```rust
pub struct Cache {
    stores: HashMap<TypeId, Box<dyn Any + Send>>,
}
```

The value under `TypeId::of::<T>()` is:

```rust
HashMap<T::Key, T>
```

### State Trait

Implement the v3 contract:

```rust
pub trait State: Send + 'static {
    type Key: Eq + Hash + Send + 'static;

    fn key(&self) -> Self::Key;
}
```

Do not require `Clone` globally.

### Steps

1. Replace the hash-only storage implementation.
2. Add internal typed-store accessors:

```rust
fn store<T: State>(&self) -> Option<&HashMap<T::Key, T>>
fn store_mut<T: State>(&mut self) -> &mut HashMap<T::Key, T>
```

3. Implement primary keyed methods:

```rust
insert
try_insert
get
get_mut
remove
contains
```

4. Implement explicit singleton helpers:

```rust
get_singleton
get_singleton_mut
remove_singleton
```

5. Define `insert` as upsert and return the previous value.
6. Define `try_insert` as duplicate-rejecting insertion.
7. Define `len` as the number of stored state values, not the number of type stores.
8. Remove old `get_keyed` and `get_keyed_mut` names.
9. Document that a state's key is stable while stored.
10. Do not attempt a complicated runtime key-stability checker in this phase.

### Required Collision Test

Create a key type whose `Hash` implementation always emits the same value while `Eq` distinguishes its identifier.

Insert two states with distinct keys and identical hashes.

Prove:

- Both entries remain present.
- Reading each key returns the correct value.
- Mutating one does not mutate the other.
- Removing one does not remove the other.

This test is mandatory. The cache rewrite is incomplete without it.

### Additional Tests

- Insert and retrieve one keyed state.
- Insert multiple values of the same state type.
- Use equal key values across different state types without collision.
- Upsert returns the old value.
- `try_insert` rejects duplicates without replacing the original.
- Missing read returns `None`.
- Mutable read persists changes.
- Removal returns the owned state.
- Singleton helpers use `Key = ()`.
- Total length counts values across type stores.
- Removing the final value leaves no observable stale state.

### Completion Gate

The cache stores actual keys and passes forced-collision tests.

No new engine code depends on hash-only cache identity.

---

## Phase 5: Add Minimal Errors And Mode Configuration

### Goal

Introduce only the configuration and error types needed by the core builder.

Do not implement live behavior yet.

### Files

Create:

```text
src/config.rs
src/error.rs
```

### Mode

Add:

```rust
pub enum Mode {
    Backtest,
    Live,
    Replay,
}
```

Contexts must never expose this value.

### Initial EngineConfig

Keep the first version deliberately small:

```rust
pub struct EngineConfig {
    mode: Mode,
    initial_dispatch_time: Timestamp,
    max_events_per_instant: usize,
}
```

Provide an explicit backtest constructor so tests and callers do not manually assemble partially valid configuration:

```rust
EngineConfig::backtest(initial_dispatch_time)
```

The backtest runtime creates its internal `SimClock` from this value. Live and replay constructors may return unsupported-mode errors until their runtime phases are implemented.

Do not add actor defaults until actor registration exists.

### Errors

Use `thiserror` for public errors.

Add a minimal `BuildError` containing only errors the current phases can produce:

- Duplicate seeded state.
- Missing consumer.
- Duplicate registration identity if applicable.
- Unsupported mode while only backtest exists.

Add a minimal `EngineError` containing only runtime errors currently implemented:

- Unconsumed ingress.
- Event scheduled in the past.
- Same-instant limit exceeded.
- Sequence exhaustion if surfaced as an error rather than panic.

Do not add speculative variants for networking, replay codecs, or actor supervision.

### Tests

- Backtest config can be constructed.
- Backtest config preserves its initial dispatch time.
- Max-events value rejects zero if zero is invalid.
- Error messages include useful type names rather than only `TypeId`.
- Unsupported modes fail explicitly until implemented.

### Completion Gate

Core configuration is declarative and contains no clocks, channels, locks, schedulers, or executors.

---

## Phase 6: Implement ReducerCtx

### Goal

Create the reducer capability boundary independently of the reducer registry.

### Files

Create a context module structure:

```text
src/context/mod.rs
src/context/reducer.rs
```

Do not recreate a single `context.rs` containing one universal context.

### Target Type

Conceptually:

```rust
pub struct ReducerCtx<'a> {
    dispatch_time: Timestamp,
    cache: &'a mut Cache,
}
```

### API

Implement delegated cache methods:

```rust
dispatch_time
get
get_mut
get_singleton
get_singleton_mut
insert
try_insert
remove
remove_singleton
```

### Restrictions

`ReducerCtx` must not contain or expose:

- Scheduler.
- Sequence allocator.
- Output sink.
- `send`.
- `send_at`.
- Clock.
- Mode.

### Tests

- `dispatch_time` returns the constructor value.
- Keyed reads delegate correctly.
- Keyed mutable reads persist changes.
- Singleton reads and writes work.
- Insertion and removal work.
- Multiple sequential mutations work in one reducer context.

### Compile-Fail Test

Add `trybuild` as a development dependency, or use a compile-fail doctest if adding a dependency is undesirable.

Prove that reducer code cannot call:

```rust
ctx.send(...)
```

### Completion Gate

Reducer capabilities are fully represented by `ReducerCtx` without any registry or engine dependency.

---

## Phase 7: Implement Handler Output And HandlerCtx

### Goal

Replace the raw outbox with direct scheduling through a narrow internal capability.

### Files

Create:

```text
src/context/handler.rs
src/output.rs
```

`output.rs` may remain crate-private.

### Internal HandlerOutput

Conceptually:

```rust
pub(crate) struct HandlerOutput<'a> {
    scheduler: &'a mut Scheduler,
    sequence: &'a mut Sequence,
    dispatch_time: Timestamp,
}
```

The output capability directly allocates sequence and inserts into the scheduler.

It does not expose the scheduler publicly.

### HandlerCtx

Conceptually:

```rust
pub struct HandlerCtx<'a> {
    dispatch_time: Timestamp,
    cache: &'a Cache,
    output: &'a mut HandlerOutput<'a>,
    declared_productions: &'a ProductionSet,
}
```

The exact lifetime split may require separate context and runtime lifetimes. Keep those implementation details private.

### API

Implement:

```rust
dispatch_time
get
get_singleton
send
send_at
```

### `send` Semantics

`send(message)` must:

1. Verify that `M` is declared.
2. Use the current dispatch time.
3. Allocate a new scheduler sequence.
4. Wrap the message in `Arc`.
5. Push directly into the scheduler.

### `send_at` Semantics

`send_at(timestamp, message)` must:

1. Reject timestamps before `dispatch_time`.
2. Verify that `M` is declared.
3. Allocate a new scheduler sequence.
4. Push directly into the scheduler at the requested timestamp.

### Restrictions

`HandlerCtx` must not expose:

- `get_mut`.
- `insert`.
- `remove`.
- Clock.
- Sequence.
- Mode.
- Raw scheduler.
- Raw output vectors.

### Tests

- `dispatch_time` returns a stable copied value.
- `send` schedules at dispatch time.
- `send_at` schedules at the requested future time.
- `send_at` rejects a past timestamp.
- Declared production succeeds.
- Undeclared production fails.
- Two sends receive increasing sequences.
- Existing same-time items remain ahead of newly produced messages.
- Handler reads keyed cache state.
- Handler reads singleton cache state.

### Compile-Fail Tests

Prove that handler code cannot:

```rust
ctx.get_mut::<T>(...)
ctx.insert(...)
ctx.seq()
ctx.clock()
```

### Completion Gate

There is no `Vec<(Timestamp, Box<dyn Message>)>` in the synchronous handler path.

There is no `RefCell` and no internal channel used to schedule handler output.

---

## Phase 8: Rebuild ReducerRegistry

### Goal

Restore reducer registration and dispatch on top of `ReducerCtx`.

### Representation

```rust
type ErasedReducer =
    Box<dyn Fn(&mut ReducerCtx<'_>, &dyn Message) + Send>;

pub(crate) struct ReducerRegistry {
    by_type: HashMap<TypeId, Vec<ReducerEntry>>,
}
```

Store the concrete message type name in metadata for diagnostics.

### Steps

1. Register reducers by consumed message `TypeId`.
2. Erase concrete `M` through one checked downcast wrapper.
3. Require reducer closures to be `Send + 'static` so `Engine` can be moved to a backtest worker thread.
4. Dispatch only reducers matching the message type.
5. Construct one `ReducerCtx` per reducer invocation or reborrow one context safely.
6. Preserve registration order within each message type.
7. Expose subscription metadata for graph construction.
8. Do not expose the registry publicly.

### Public Registration Shape

The builder will eventually provide:

```rust
app.reduce::<Fill>(|ctx, fill| {
    // cache mutation
});
```

Do not add state-specific reducer builders in this phase.

### Tests

- Matching reducer fires.
- Wrong message type does not fire.
- Multiple reducers run in registration order.
- One reducer sees prior reducer mutations.
- Reducer can insert keyed state.
- Reducer can mutate singleton state.
- Reducer receives the supplied dispatch time.
- Registry reports consumed message types.
- Empty registry performs no callback.
- Downcast mismatch is treated as an internal invariant failure.

### Completion Gate

The reducer registry contains no raw `Fn(&mut Cache, &M)` public callback path.

All user reducers receive `ReducerCtx`.

---

## Phase 9: Rebuild Stateless Handler Registration

### Goal

Implement the smallest useful handler registry before adding state groups.

### Initial Representation

```rust
type ErasedHandler = Box<
    dyn Fn(Option<&mut (dyn Any + Send)>, &mut HandlerCtx<'_>, &dyn Message)
        + Send
>;

struct HandlerEntry {
    consumed: MessageType,
    state_slot: Option<StateSlot>,
    invoke: ErasedHandler,
    productions: ProductionSet,
}

struct HandlerRegistry {
    states: Vec<Box<dyn Any + Send>>,
    entries: Vec<HandlerEntry>,
    by_type: HashMap<TypeId, Vec<HandlerId>>,
}
```

The exact erased signature may vary, but there must be one handler invocation representation rather than separate stateless/stateful enums.

### Steps

1. Implement stateless registration only.
2. Append every entry to one flat `entries` vector.
3. Add its index to `by_type[TypeId::of::<M>()]`.
4. Preserve registration order in the index vector.
5. Add an opaque public registration handle exposing only `.produces::<T>()`.
6. Keep `HandlerEntry` private.
7. Dispatch only indexes from `by_type`.
8. Construct `HandlerCtx` with that entry's production declarations.
9. Expose consumed and produced metadata for graph building.

### Tests

- Matching stateless handler fires.
- Wrong message type does not fire.
- Multiple stateless handlers run in registration order.
- Handler can read cache.
- Handler can schedule a declared message.
- Handler cannot schedule an undeclared message.
- One handler's production declarations do not authorize another handler.
- Dispatch uses the type index rather than scanning unrelated handlers. Verify behavior, not performance internals.
- Opaque registration supports multiple `.produces` calls.

### Completion Gate

Stateless handler registration and dispatch work without groups, coalescing, or handler enums.

---

## Phase 10: Add Stateful Handler Groups

### Goal

Add private persistent handler state without coupling it to global-cache `State`.

### Public Shape

```rust
app.handler_group(SmaState::new(), |group| {
    group.on::<Bar>(...);
    group.on::<Reset>(...);
});
```

### Steps

1. Accept any group state satisfying:

```rust
S: Send + 'static
```

2. Do not require `S: State`.
3. Allocate one state slot per `handler_group` call.
4. During the scoped configuration closure, every registered handler records that slot.
5. Erase the stateful callback into the same `ErasedHandler` representation used by stateless handlers.
6. Downcast the state slot to `S` at invocation.
7. Keep entries flat and globally ordered by actual handler registration.
8. Prevent a group builder from outliving its configuration scope.
9. Keep state inaccessible outside its group callbacks.

### Tests

- Group state persists across multiple dispatched messages.
- Multiple handlers in one group observe each other's changes.
- Separate groups have isolated state.
- Separate groups using the same Rust state type remain isolated.
- Stateless and stateful handlers run in true registration order.
- Different message types can share one group state.
- A stateful handler's production declarations are enforced independently.
- Handler-group state does not implement `State` and still works.
- An impossible state-slot downcast fails as an internal invariant.

### Compile-Time Test

Prove that handler-group state is not reachable through `HandlerCtx::get` unless a separate value of that type was deliberately seeded in the global cache.

### Completion Gate

The handler registry has:

- One flat entry list.
- One state-slot arena.
- One message-type index.
- One erased callback shape.

There is no `HandlerFn` enum, no `Group` enum, and no stateless-group coalescing.

---

## Phase 11: Rebuild Graph Metadata And Validation

### Goal

Make graph validation a build-time operation independent of registry internals.

### Current Gap

The old graph directly queries handler and reducer registries, knows about old actor routes, emits only `TypeId` diagnostics, and rejects all type cycles.

### Graph Model

Introduce metadata types such as:

```rust
struct MessageType {
    id: TypeId,
    name: &'static str,
}

struct ConsumerDescriptor {
    kind: ConsumerKind,
    consumed: MessageType,
    registration_order: usize,
}

struct ProducerDescriptor {
    owner: ConsumerId,
    consumed: MessageType,
    produced: Vec<MessageType>,
}
```

The exact names may vary.

### Steps

1. Create a `GraphBuilder` that receives descriptors from registries.
2. Collect reducer subscriptions.
3. Collect handler subscriptions.
4. Collect handler production declarations per callback.
5. Validate that every declared production has at least one consumer.
6. Return `BuildError` rather than panic.
7. Include readable Rust type names in errors.
8. Produce an immutable `ValidatedGraph` containing the consumer type set needed at runtime.
9. Do not add old `route_consumers` arguments.
10. Leave actor consumers absent until actor registration is implemented.

### Cycle Policy In This Phase

Do not restore the old blanket cycle rejection as a hidden decision.

`design-v3.md` leaves exact static cycle semantics open because declarations do not distinguish `send` from `send_at`.

For the first core runtime:

- Implement orphan validation.
- Preserve the deterministic runtime same-instant bound later.
- Keep cycle metadata extensible.
- Add static cycle rejection only after a separate design decision.

### Tests

- Empty graph validates.
- Terminal consumer with no outputs validates.
- Linear production chain validates.
- Reducer counts as consumer.
- Handler counts as consumer.
- Orphan handler production returns `BuildError` with the produced type name.
- Multiple producers of one consumed type validate.
- Multiple consumers of one produced type validate.
- A consumer set can answer runtime `has_consumer(TypeId)`.
- No test references actor routes.

### Completion Gate

Graph validation no longer depends on the concrete fields of handler or reducer registries.

Build diagnostics name user message types.

---

## Phase 12: Build EngineBuilder And Seed State

### Goal

Separate registration from execution before adding the event loop.

### Files

Create:

```text
src/engine.rs
src/builder.rs
```

The public runtime type is `Engine`.

The old public `Kernel` type is not restored.

### EngineBuilder Ownership

The builder owns:

```text
EngineConfig
seed Cache
ReducerRegistry
HandlerRegistry
GraphBuilder inputs
```

### Public API

Implement:

```rust
Engine::builder(config)
builder.seed(state)
builder.reduce::<M>(callback)
builder.on::<M>(callback)
builder.handler_group(state, configure)
builder.build()
```

### Steps

1. `seed` uses `Cache::try_insert`.
2. Duplicate state returns a configuration error.
3. Registration methods delegate to private registries.
4. `build()` gathers graph metadata.
5. `build()` validates the graph.
6. `build()` freezes all registries.
7. `build()` constructs an `Engine` containing runtime state but no queued events.
8. Initialize runtime logical time and `SimClock` from the backtest config's initial dispatch time.
9. Support backtest mode only initially.
10. Return explicit unsupported-mode errors for live and replay until those phases exist.
11. Do not allow registrations on `Engine`.

### Engine Skeleton

Conceptually:

```rust
pub struct Engine {
    config: EngineConfig,
    scheduler: Scheduler,
    sequence: Sequence,
    cache: Cache,
    reducers: ReducerRegistry,
    handlers: HandlerRegistry,
    graph: ValidatedGraph,
    dispatch_time: Timestamp,
    clock: Box<dyn Clock>,
}
```

Exact field grouping may change when `Runtime` is introduced.

### Tests

- Empty builder builds.
- Seeded keyed state appears in the built engine.
- Seeded singleton state appears in the built engine.
- Duplicate seed returns `BuildError`.
- Valid registrations build.
- Orphan production prevents build.
- Builder cannot be reused after consuming `build()`.
- Built engine has no registration methods.
- Live/replay modes report unsupported status explicitly for now.

### Completion Gate

Topology validation happens at `build()`, never at `run()`.

The type system separates configuration from execution.

---

## Phase 13: Implement Core Engine Ingress

### Goal

Allow validated external input to enter the scheduler without implementing dispatch yet.

### API

```rust
engine.push_event(dispatch_time, message) -> Result<(), EngineError>
```

### Steps

1. Look up the message type in `ValidatedGraph`.
2. Reject input with no consumer before scheduling it.
3. Compare the requested timestamp with the engine's current logical dispatch time.
4. Reject past input.
5. Allocate exactly one scheduler sequence.
6. Wrap the message in `Arc`.
7. Push it into the scheduler.
8. Keep the enqueue implementation private and reusable by handler and actor output paths later.
9. Do not log ingress yet. Logging is a later phase with unresolved serialization.

### Tests

- Consumed input is accepted.
- Unconsumed input is rejected.
- Input at current time is accepted.
- Input in the future is accepted.
- Input before current logical time is rejected.
- Two same-time inputs receive increasing sequence and preserve insertion order.
- Failed input does not consume scheduler capacity or mutate cache.

### Completion Gate

There is one kernel-owned path for validated scheduling and sequence assignment.

No caller assigns sequence directly.

---

## Phase 14: Implement The Deterministic Core Event Loop

### Goal

Run reducers and handlers end to end using direct scheduling and fixed dispatch time.

### Runtime Structure

Split ownership along borrow boundaries if useful:

```rust
struct Runtime {
    scheduler: Scheduler,
    sequence: Sequence,
    cache: Cache,
    dispatch_time: Timestamp,
    clock: Box<dyn Clock>,
    same_instant_count: usize,
}
```

`Engine` separately owns registries and the validated graph.

Do not pass `&mut Runtime` to user callbacks.

### Loop Order

For each queued message:

```text
pop earliest item
verify timestamp monotonicity
set runtime dispatch_time from the item
advance SimClock internally if retained
assert the item has a consumer
run reducers
run handlers
continue to next scheduled item
```

Actors are added in a later phase.

### Steps

1. Pop one `ScheduledItem` at a time.
2. Set `dispatch_time` from the scheduled item, not from `Clock::now()`.
3. Do not read the clock during callbacks.
4. Build `ReducerCtx` with mutable cache and copied dispatch time.
5. Complete all reducers.
6. End the mutable cache borrow.
7. Build handler output from disjoint scheduler and sequence fields.
8. Dispatch handlers with immutable cache and copied dispatch time.
9. Let handler output schedule directly.
10. Do not recursively dispatch produced messages.
11. Track the configured same-instant count.
12. Reject a count beyond the configured bound.
13. Exit cleanly when the scheduler is empty.

### Required End-To-End Tests

- Empty engine exits cleanly.
- Reducer-only message updates cache.
- Handler-only message runs.
- Reducers run before handlers for the same message.
- Multiple reducers preserve order.
- Multiple handlers preserve order.
- Handler observes completed reducer state.
- Handler sends a same-time message that is processed later.
- A same-time causal chain completes before future time advances.
- Existing equal-time ingress stays ahead of newly produced output.
- `send_at` inserts between surrounding future messages correctly.
- Past scheduling is rejected.
- Same-instant bound triggers deterministically.
- Unconsumed input is rejected before run.
- Per-pop consumer assertion catches impossible internal inconsistency.
- Final dispatch time equals the last processed scheduler timestamp.

### Clock Tests

- Backtest context time equals the scheduler timestamp.
- Callback execution speed does not affect dispatch time.
- Two handlers for one message see exactly the same dispatch time.
- A produced same-time message sees the inherited dispatch time when later processed.

### Completion Gate

The deterministic backtest core works without actors, outboxes, `RefCell`, internal channels, or public sequence access.

---

## Phase 15: Core API And Capability Hardening

### Goal

Make the completed non-actor core difficult to misuse before adding actors.

### Steps

1. Remove all obsolete public exports.
2. Remove `#![allow(dead_code)]`.
3. Ensure `HandlerEntry`, state slots, registry IDs, and scheduler items are private.
4. Re-export only intended public types from `lib.rs`.
5. Add an `Engine: Send` compile-time assertion.
6. Ensure `Engine` is not unnecessarily required to be `Sync`.
7. Add compile-fail tests for context capabilities.
8. Add rustdoc examples for builder, reducer, handler, and handler group APIs.
9. Verify every public error has a readable diagnostic.
10. Verify there are no references to `Kernel`, old `Context`, `route`, or `send_to`.

### Public Integration Tests

Create tests under `tests/` that use only public exports:

```text
tests/build_validation.rs
tests/cache_reducers.rs
tests/handler_groups.rs
tests/scheduling.rs
tests/end_to_end.rs
tests/ui/
```

### Required Public Tests

- Build and run a reducer-only engine.
- Build and run a stateless-handler engine.
- Build and run a handler-group engine.
- Reject an orphan production at build.
- Reject unconsumed external input.
- Complete a multi-message cascade.
- Prove two independent engines do not share state.
- Move an engine into a spawned thread and run it.

### Completion Gate

The non-actor v3 core is complete, documented, and tested through public APIs.

Do not begin actors while core tests are failing.

---

## Phase 16: Add Actor Configuration Metadata Only

### Goal

Introduce actor declarations without executing actors or creating channels.

### Design Boundary

Actor configuration contains values. It does not contain a mailbox, channel, thread, or executor.

### Files

Create:

```text
src/actor/mod.rs
src/actor/config.rs
src/actor/registry.rs
```

### Initial ActorConfig

Implement only settled fields:

```rust
pub struct ActorConfig {
    inbox_capacity: Option<NonZeroUsize>,
}
```

Do not choose a default overflow policy in code. That remains a design gate for live execution.

### Registration Metadata

An actor declaration needs:

- Stable unique name.
- Registration order.
- Private state type.
- Subscribed message types.
- Per-callback production declarations.
- Declarative configuration.

### Public Shape

Conceptually:

```rust
app.actor("sim-venue", SimVenue::new(), |actor| {
    actor.inbox_capacity(4_096);
    actor.on::<MarketData>(...);
});
```

The exact method syntax may be adjusted, but users never construct an inbox.

### Graph Integration

1. `actor.on::<M>()` counts as a consumer of `M`.
2. Actor productions count as graph productions.
3. There is no `route::<M>()`.
4. There is no `send_to`.
5. There are no public actor handles.

### Tests

- Duplicate actor names fail build.
- Actor `.on::<M>()` satisfies an orphan handler production of `M`.
- Actor production with no consumer fails build.
- Actor subscription metadata preserves registration order.
- Inbox capacity rejects zero.
- Actor configuration contains no channel/runtime type.
- No actor callback executes yet.

### Completion Gate

Actors participate in graph validation without introducing execution complexity.

---

## Phase 17: Implement ActorCtx And Executor-Neutral Output

### Goal

Define actor callback capabilities before choosing backtest or live execution mechanisms.

### Files

Create:

```text
src/context/actor.rs
src/actor/output.rs
```

### ActorCtx

Conceptually:

```rust
pub struct ActorCtx<'a> {
    dispatch_time: Timestamp,
    output: &'a mut dyn ActorOutputSink,
    declared_productions: &'a ProductionSet,
}
```

### API

Implement:

```rust
dispatch_time
send
send_at
```

### Restrictions

ActorCtx must not expose:

- Cache.
- Handler-group state.
- Sequence.
- Scheduler.
- Clock.
- Mode.
- Channels.
- Inbox capacity.

### Output Sink Contract

The sink reports an actor emission to the selected executor.

It does not let the actor assign kernel sequence.

For immediate `send`, it carries no user-selected final live dispatch timestamp. The kernel stamps receipt.

For `send_at`, it carries the explicitly requested timestamp for later validation.

### Tests

- `dispatch_time` returns the current message's dispatch time.
- Declared immediate output reaches a fake sink.
- Declared scheduled output reaches a fake sink with the requested time.
- Undeclared actor output fails.
- ActorCtx has no cache API.
- ActorCtx has no sequence or clock API.

### Completion Gate

Actor callback code can be tested without threads, channels, scheduler access, or cache access.

---

## Phase 18: Implement Inline Backtest Actors

### Goal

Add deterministic actor execution to backtests before any live threading.

### Representation

Use the same broad pattern as handler state:

- Actor state arena or typed erased actor objects.
- Flat actor registration order.
- Type-indexed actor callback subscriptions.
- Per-callback production declarations.

The exact internal representation may differ from handlers because each actor is also a lifecycle unit.

### Dispatch Order

For every scheduler message:

```text
reducers
handlers
subscribed inline actors
next scheduler pop
```

### Steps

1. Build actor runtimes from actor declarations in backtest mode.
2. Do not create threads or channels.
3. Deliver the shared `Arc` payload to subscribed actors after all handlers finish.
4. Execute actors in actor registration order.
5. Execute matching callbacks within an actor in callback registration order.
6. Pass actor state as `&mut A`.
7. Pass copied message dispatch time through `ActorCtx`.
8. Route immediate actor output through the kernel's validated ingress/scheduling path.
9. In inline backtest execution, immediate output is received at the current dispatch time.
10. Route `send_at` through past-time validation.
11. Do not recursively dispatch actor output.

### Tests

- Actor receives a subscribed message.
- Actor ignores an unsubscribed type.
- Actor state persists across messages.
- Separate actor states are isolated.
- Actors execute after all handlers.
- Actors execute in actor registration order.
- Actor callbacks execute in registration order.
- Immediate actor output is scheduled at current dispatch time.
- Scheduled actor output uses requested future time.
- Actor output gets a kernel-assigned sequence.
- Actor output is checked against `.produces`.
- Actor-required global state is delivered through an owned message snapshot.
- Simulated venue processes market data before a later order.
- Live venue implementation is not forced to subscribe to market data.
- Same-time actor feedback is bounded by the runtime guard.

### Completion Gate

The complete backtest graph supports reducers, handlers, and deterministic inline actors.

No actor thread exists in backtest mode.

---

## Phase 19: Backtest Actor Integration And Parity Tests

### Goal

Prove the actor model with realistic but minimal examples before designing live infrastructure.

### Simulated Venue Test Component

Create test-only messages and actor state:

```text
MarketData
SubmitOrder
Fill
SimVenueState
```

The simulated venue:

- Subscribes to `MarketData`.
- Maintains a private book projection.
- Subscribes to `SubmitOrder`.
- Produces `Fill` through `send_at` with deterministic latency.

### Required Tests

- Market data updates both reducer-owned cache projection and actor-private venue projection independently.
- Handler sees reducer-updated cache before actor delivery.
- Sim venue sees market events in scheduler order.
- Order executes against the latest causally preceding market state.
- Fill latency places a fill between surrounding market events correctly.
- Fill reducer updates portfolio before fill handlers run.
- Repeating the same run produces identical output ordering.
- Running two backtests on separate threads produces isolated state.

### Snapshot Alternative Test

Add one test actor that consumes an explicitly owned snapshot message rather than subscribing to all source events.

Prove that no cache borrow crosses into the actor.

### Completion Gate

The backtest actor model is sufficient for a simulated venue and heavy deterministic computation.

At this point the backtest core is usable even if live actors remain unimplemented.

---

## Design Gate A: Finalize Live Actor Semantics

Do not implement live actor threads until these decisions are recorded in `design-v3.md` or a dedicated actor ADR.

### Required Decisions

1. Is live actor inbox capacity required per actor, inherited from explicit global defaults, or both?
2. What is the initial overflow policy?
3. Is explicit unbounded capacity allowed?
4. How are queue depth, oldest age, and high-water metrics exposed?
5. What does an actor callback return on infrastructure failure?
6. What happens to the engine when one actor faults?
7. How does actor readiness gate engine startup?
8. Does shutdown drain queued messages or stop immediately?
9. What timeout applies when joining actors?
10. Which channel/runtime library is used internally?
11. How do source actors emit messages without a triggering engine message?
12. How do live venues receive unsolicited exchange responses?
13. How does the live loop wait for actor output and scheduled timers?
14. What happens when `ActorCtx::send_at` arrives after its requested time?

### Explicit Non-Decision

Do not use an unbounded queue merely because it avoids choosing overflow behavior.

An unbounded queue is itself an overflow policy and must be explicit.

---

## Phase 20: Finalize Declarative Actor Runtime Configuration

> **Blocked by Design Gate A**

### Goal

Represent the resolved live policies as values without exposing runtime mechanisms.

### ActorConfig

After the gate, add finalized fields such as:

```rust
pub struct ActorConfig {
    inbox_capacity: InboxCapacity,
    overflow_policy: ActorOverflowPolicy,
}
```

`InboxCapacity` is a configuration value, not a mailbox.

### Resolution

Implement explicit precedence:

```text
per-actor override
    > mode-specific actor defaults
    > engine defaults
```

Live `build()` fails if required values are unresolved.

### Tests

- Per-actor capacity overrides defaults.
- Explicit defaults are inherited.
- Missing required live config returns `BuildError`.
- Zero capacity is rejected.
- Effective config is available for diagnostics.
- No public type contains a channel sender or receiver.
- Backtest executor remains behaviorally unaffected by inbox capacity.

### Completion Gate

Users configure limits and policies but cannot instantiate transport objects.

---

## Phase 21: Implement Reactive Live Actor Threads

> **Blocked by Design Gate A**

### Goal

Run message-driven actors on runtime-owned threads.

This phase covers actors that react to kernel messages. Source actors are separate.

### Steps

1. Create one dedicated thread per actor initially.
2. Create a private bounded or explicitly unbounded inbox according to resolved config.
3. Keep all channel types crate-private.
4. Move actor state and callbacks onto the actor thread.
5. Deliver `Arc<dyn Message>` clones after handlers complete.
6. Preserve FIFO delivery from the single kernel producer.
7. Execute callbacks serially per actor.
8. Send actor emissions through a runtime-owned output channel.
9. Include stable actor identity in output metadata.
10. Do not let actor threads assign scheduler sequence.
11. Implement the chosen overflow behavior explicitly.
12. Record queue-depth metrics in the runtime.

### Tests

- Actor callback runs on a different thread from the kernel.
- One actor processes inputs FIFO.
- Actor state is never concurrently borrowed.
- Multiple actors receive the same shared payload.
- Handler execution finishes before actor delivery begins.
- Actor output returns to the kernel.
- Kernel assigns actor output sequence.
- Configured overflow behavior is deterministic and visible.
- Actor disconnection is surfaced.
- No message is silently dropped.

### Completion Gate

Reactive live actors work without exposing channels, mailboxes, locks, or thread handles publicly.

---

## Phase 22: Implement Live Ingress Pump And Receipt Time

> **Blocked by Design Gate A**

### Goal

Integrate actor output, external input, and future scheduler times into a live event loop.

### Steps

1. Add one kernel ingress receiver for actor/source output.
2. Read `LiveClock` exactly once when accepting each immediate live ingress message.
3. Use that receipt timestamp as dispatch time.
4. Allocate sequence at kernel acceptance.
5. Validate consumers before scheduling.
6. Validate explicit actor `send_at` timestamps.
7. Wait for whichever occurs first:

```text
actor/source ingress
next scheduled timer
shutdown signal
actor failure signal
```

8. Do not repeatedly read wall time inside callbacks.
9. Keep callback `dispatch_time` fixed.
10. Ensure an empty scheduler does not terminate while live actors/sources remain active.

### Deterministic Test Infrastructure

Use a controllable fake live clock rather than sleeping in most tests.

Avoid timing assertions based on small real-wall-time windows.

### Tests

- Immediate live ingress is stamped once.
- Payload domain time remains independent.
- Callback wall-clock delay does not change dispatch time.
- Two actor outputs receive order according to kernel arrival.
- Future scheduled messages fire at or after their requested time.
- Past actor `send_at` follows the selected fault policy.
- Live loop waits while actors are active.
- Shutdown wakes a waiting loop.

### Completion Gate

Live ingress and timers share one kernel-owned ordering boundary.

---

## Phase 23: Implement Source Actors

> **Blocked by the source actor decision in Design Gate A**

### Goal

Support actors that originate messages without first receiving a kernel message.

Examples include:

- Market-data feeds.
- Control-plane listeners.
- Exchange response readers.

### Required Separation

`ActorCtx::dispatch_time` is defined by a triggering message.

A source loop with no triggering message must not invent a fake callback dispatch time.

Use a separate source-output capability if required by the finalized design.

### Steps

1. Implement the chosen startup/run hook.
2. Provide an output capability that sends immediate ingress for kernel receipt stamping.
3. Support source shutdown.
4. Surface source failure through actor fault reporting.
5. Preserve actor-private connection state.
6. Prevent source code from reading global cache.

### Tests

- Source actor can emit without an input message.
- Kernel assigns receipt time and sequence.
- Source has no cache access.
- Source stops on shutdown.
- Source failure is surfaced.
- Backtest mode does not start live source threads.

### Completion Gate

Live market-data and venue-response ingress can be modeled without abusing subscribed callback contexts.

---

## Phase 24: Actor Readiness, Failure, And Shutdown

> **Blocked by Design Gate A**

### Goal

Make actor lifecycle behavior explicit and testable.

### Steps

1. Assign internal actor lifecycle states.
2. Require readiness before live engine operation begins if selected by design.
3. Distinguish expected domain messages from infrastructure faults.
4. Catch actor panics at thread boundaries when possible.
5. Report actor name and current message type on failure.
6. Implement the selected engine response to actor failure.
7. Stop accepting new work during shutdown.
8. Apply selected drain behavior.
9. Signal actor shutdown through private runtime control.
10. Join threads with selected timeout semantics.
11. Do not implement automatic restart unless separately designed.

### Tests

- Engine waits for required readiness.
- Readiness failure prevents operation.
- Actor panic becomes a visible fault.
- Expected venue rejection remains a normal message, not an actor fault.
- Shutdown stops delivery.
- Drain policy is honored.
- Join timeout is surfaced.
- No actor thread survives successful shutdown.

### Completion Gate

Actor lifecycle has no hidden restart, retry, drop, or shutdown behavior.

---

## Design Gate B: Durable Ingress And Replay

Before implementing a real ingress log, decide:

1. Stable message type identity across builds.
2. Serialization registration.
3. Schema versioning.
4. Storage format.
5. Flush and durability guarantees.
6. Whether handler/actor internal productions are traced separately.
7. Whether operation sequencing is separate from scheduler sequencing.
8. How outbound commands to disabled replay actors are verified.
9. Whether deterministic backtest actors rerun during replay.

Do not treat `TypeId` or `Debug` output as a durable replay format.

---

## Phase 25: Implement Ingress Logging

> **Blocked by Design Gate B**

### Goal

Record nondeterministic live ingress before dispatch.

### Logged Sources

- Live market-data source output.
- Live venue/exchange actor output.
- External control input.
- Other nondeterministic actor output.

### Steps

1. Create a dedicated ingress-log module.
2. Use stable type identity from Design Gate B.
3. Serialize payload before acknowledging durable acceptance if required.
4. Record dispatch time assigned at receipt.
5. Record scheduler sequence.
6. Record source identity.
7. Append before scheduler dispatch.
8. Surface logging failure according to the selected live failure policy.
9. Keep runtime `Arc<dyn Message>` ownership independent of durable bytes.

### Tests

- Every live ingress is logged exactly once.
- Log order matches assigned sequence.
- Logged dispatch time matches callback dispatch time.
- Internal handler production is not misclassified as external ingress unless explicitly traced.
- Logging failure prevents unrecorded live processing when durability is required.

### Completion Gate

The live ingress boundary is a real determinism boundary rather than an unused in-memory vector.

---

## Phase 26: Implement Replay Mode

> **Blocked by Design Gate B and Phase 25**

### Goal

Recreate kernel-observed live ingress without external IO.

### Steps

1. Load recorded ingress entries in order.
2. Validate stable message identities and schemas.
3. Inject recorded dispatch times and payloads.
4. Do not start live external actors.
5. Install replay stubs where graph consumers are required.
6. Run reducers and handlers normally.
7. Apply the decided policy for deterministic actors.
8. Compare outbound external commands if selected by design.
9. Verify scheduler output ordering.

### Tests

- Replay performs no external IO.
- Recorded actor outputs appear at recorded dispatch times.
- Replay is identical across repeated runs.
- Missing codec/schema errors are explicit.
- Corrupt log records fail loudly.
- Optional outbound verification detects divergence.

### Completion Gate

Recorded live ingress deterministically reproduces kernel behavior without contacting external systems.

---

## Phase 27: Final Cleanup And Documentation

### Goal

Remove rewrite scaffolding and ensure repository documentation matches reality.

### Steps

1. Remove every obsolete file and old API reference.
2. Remove temporary unsupported-mode branches that are now implemented.
3. Remove dead compatibility comments.
4. Confirm `design-v3.md` matches actual behavior.
5. Update crate-level documentation.
6. Add one public example for each mode actually implemented.
7. Add one public simulated-venue example.
8. Document all actor config defaults and resolution rules.
9. Document every runtime failure policy.
10. Run all tests, clippy, and rustdoc.

### Final Verification

```bash
cargo fmt --check
cargo test
cargo test --doc
cargo clippy --all-targets -- -D warnings
cargo doc --no-deps
```

### Final Completion Gate

The implementation satisfies every finalized invariant in `design-v3.md`.

Every unresolved design question remains explicitly documented rather than silently encoded in implementation behavior.

---

## Test Strategy By Layer

### Leaf Unit Tests

Keep exhaustive unit tests beside:

- Sequence.
- Scheduler.
- Cache.
- Contexts.
- Reducer registry.
- Handler registry.
- Graph validation.
- Actor config resolution.
- Actor registry.

### Public Integration Tests

Use `tests/` for behavior visible to crate users:

- Builder validation.
- Seeding.
- Reducer/handler ordering.
- Message cascades.
- Handler-group state.
- Inline actors.
- Mode-specific venue installation.
- Live ingress.
- Replay.

### Compile-Fail Tests

Use `trybuild` or compile-fail doctests to prove capability restrictions:

- Reducer cannot send.
- Handler cannot mutate cache.
- Actor cannot access cache.
- Contexts cannot access sequence.
- Contexts cannot access clock.
- Contexts cannot access mode.
- Runtime internals are not publicly constructible.

### Determinism Tests

For deterministic scenarios, run the same setup multiple times and compare:

- Message type order.
- Dispatch timestamps.
- Scheduler sequence order where observable to test internals.
- Final cache state.
- Handler-group state effects.
- Actor outputs.

Do not rely only on final state. Different event orders can accidentally produce the same final value.

### Concurrency Tests

Avoid fragile sleep-based tests.

Use barriers, fake clocks, controlled channels, and explicit synchronization to test live actors.

Tests must complete with bounded timeouts so deadlocks fail rather than hang indefinitely.

---

## Phase Completion Checklist

Every phase must satisfy all applicable items:

- The phase's public behavior is documented.
- The phase has focused unit tests.
- Existing preserved tests still pass.
- No ignored test hides unfinished behavior.
- No `todo!()` exists on an active code path.
- No broad `allow(dead_code)` hides incomplete integration.
- No public type exposes an internal runtime mechanism.
- Error messages identify relevant message or actor types.
- `cargo fmt --check` passes.
- `cargo test` passes.
- `cargo clippy --all-targets -- -D warnings` passes.
- The next phase does not begin until the completion gate is met.

---

## Explicitly Deferred Work

Do not add these features during the core rewrite unless their design gate has been completed:

- Point-to-point actor messaging.
- `send_to`.
- Explicit actor routes.
- Public actor handles.
- Fine-grained keyed routing.
- Dynamic subscriptions after build.
- Automatic actor restart.
- Automatic actor retries.
- Silent mailbox drop policies.
- Parallel actors inside one backtest.
- Deterministic business ID generation from scheduler sequence.
- Public sequence access.
- Deterministic RNG.
- Trading lifecycle and control plane.
- Durable serialization before its design gate.
- Checkpointing.
- Persistent cache snapshots.
- Exact static same-time cycle analysis.

---

## Recommended Milestones

### Milestone 1: Correct Foundations

Includes Phases 0 through 4.

Result:

- Stable primitives preserved.
- Messages are shareable.
- Sequence is independent.
- Scheduler uses `Arc` payloads.
- Cache is collision-safe.

### Milestone 2: Capability-Safe Registries

Includes Phases 5 through 11.

Result:

- Separate contexts.
- Direct handler scheduling.
- Reducer registry.
- Flat handler registry.
- Private handler groups.
- Build-time graph validation.

### Milestone 3: Deterministic Core Engine

Includes Phases 12 through 15.

Result:

- Separate builder and engine.
- Seeded global cache.
- Validated ingress.
- Deterministic event loop.
- Public capability tests.

This is the first major stable stopping point.

### Milestone 4: Deterministic Backtest Actors

Includes Phases 16 through 19.

Result:

- Declarative actor registration.
- ActorCtx.
- Actor graph integration.
- Inline deterministic actor execution.
- Simulated venue support.

This is the second major stable stopping point.

### Milestone 5: Live Actor Runtime

Includes Design Gate A and Phases 20 through 24.

Result:

- Explicit actor runtime configuration.
- Private runtime queues.
- Live actor threads.
- Receipt-time ingress.
- Source actors.
- Lifecycle and shutdown.

### Milestone 6: Replay Boundary

Includes Design Gate B and Phases 25 through 27.

Result:

- Durable ingress logging.
- Replay mode.
- Final documentation and cleanup.

---

## First Implementation Sequence

The recommended immediate sequence is:

```text
Phase 0  Remove old orchestration modules
Phase 1  Update Message and add Arc payload
Phase 2  Extract Sequence from dead logging
Phase 3  Adapt Scheduler to Arc
Phase 4  Fix Cache storage and collision behavior
Phase 5  Add minimal config/errors
Phase 6  Implement ReducerCtx
Phase 7  Implement HandlerCtx and direct output
Phase 8  Rebuild reducers
Phase 9  Rebuild stateless handlers
Phase 10 Add handler groups
Phase 11 Add graph validation
Phase 12 Add EngineBuilder
Phase 13 Add validated ingress
Phase 14 Add event loop
Phase 15 Harden the public core
```

Stop after Phase 15 and review the complete non-actor engine before starting actor registration.

That review should verify that the borrow-checker workarounds that motivated the rewrite are gone rather than merely moved into new files.
