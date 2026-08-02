# Canonical State And Reducers

> **Status:** Settled for the Kavod application-state boundary
> **Scope:** Canonical shared state, Component-private state, Reducer authority, dynamic state, and state dependencies
> **ControlPlane reconciliation:** `7_control_plane_lifecycle_supervision.md` adds Engine-owned authoritative ControlPlane state and permits ordinary Reducers to project accepted ControlEvents into `AppState`. ControlPlane state is not canonical application state.

## Conclusion

Kavod uses one application-defined concrete `AppState` as its canonical shared state.

```text
Engine physically owns AppState
        |
        +-- Reducer callback receives mutable access
        |
        +-- ordinary Component callback receives immutable access
```

Reducers are restricted callbacks, not state containers, Components, projectors, or logical state owners.

Reducers have no private mutable state. Every behavior-affecting mutable value used by a Reducer belongs to canonical `AppState`; a Reducer must not retain mutable callback captures as a hidden third state class.

The application logically owns `AppState`. Each Component logically owns its private state. The Engine physically owns both and lends the appropriate access during callback execution.

Kavod does not use a `TypeId` cache, state-slot registry, string-keyed resources, per-container ownership, or declared field-level reads and writes.

## State Classes

Kavod has two application-state classes.

### Canonical Shared State

Canonical shared state is the application's single concrete `AppState`.

It contains information that must be readable across Component boundaries, such as:

- Instruments.
- Orders and order status.
- Positions.
- Portfolio cash and balances.
- Completed bars.
- Broker-observed reconciliation state.
- Shared risk state.

Only Reducer callbacks may mutate it. Ordinary Component callbacks receive immutable access.

### Component-Private State

Component-private state belongs to one Component instance and is inaccessible to other Components.

Examples include:

- Strategy indicators.
- Signal accumulators.
- Strategy finite-state machines.
- In-progress bar aggregation.
- Component-specific configuration and counters.

Callbacks registered on the same Component instance may share that Component's private state. If independently registered Components require direct access to the same state, that state is canonical shared state rather than private state.

## Ownership

| State | Logical owner | Physical owner | Mutation authority |
|---|---|---|---|
| Canonical `AppState` | Application | Engine | Reducer callbacks |
| Component-private state | Component instance | Engine | That Component's callbacks |
| Environment implementation state | Live Port implementation or simulated model | Environment-owned worker or model storage | Owning live implementation or simulated-model callbacks |
| ControlPlane lifecycle state | Engine ControlPlane | Engine | ControlPlane |
| Kernel ordering state | Kernel | Engine | Kernel |

A Reducer does not own the state it changes. It is one registered transition callback with temporary mutable access to application-owned canonical state.

There is no separate projector ownership model.

Applications may project ControlEvents into an application-defined runtime view such as logical Port status. That projection is deterministic decision state; it does not replace or mutate the ControlPlane's authoritative lifecycle, quarantine, placement, or incarnation state.

## Reducer Semantics

A Reducer:

- Consumes one typed Port Event, ControlEvent, or Message.
- Receives mutable access to the complete `AppState`.
- May read or change any part of `AppState`.
- May update several related fields in one callback.
- Emits no Messages, Port Commands, or ControlCommands.
- Performs no external IO or blocking work.
- Cannot retain an `AppState` reference after the callback returns.
- Has no private mutable state or behavior-affecting mutable capture.
- Returns no generic application result; expected domain outcomes are represented through state and later ordinary application logic.

For each delivered Event or Message whose callback dispatch completes before fatal establishment:

1. Matching Reducer callbacks execute in stable registration order.
2. Each Reducer completes before the next begins.
3. Matching ordinary Component callbacks execute only after all matching Reducers complete.
4. Ordinary Components therefore observe the fully reduced state for that input.

If fatal establishment terminates dispatch between callbacks, the Engine is poisoned. No later ordinary Component bypasses the unfinished matching Reducers, and Kavod makes no state-consistency guarantee for the incomplete turn.

The settled turn-scheduling design preserves this per-input guarantee. It does not claim that an ordinary Component observes canonical-state transitions caused by other Messages still waiting in the turn FIFO. Related updates that a decision requires together must be represented by one complete aggregate Event or Message.

## Multiple Reducers

Any registered Reducer may mutate any part of `AppState`.

Several Reducers may consume the same input and update the same or different state. Their registration order is semantically observable because later Reducers see earlier mutations.

This is legitimate when callbacks represent ordered, compatible transitions. For example, one execution report may update:

- Order lifecycle state.
- Position and cash state.
- Execution deduplication state.

It is a design smell when unrelated Reducers assign conflicting values and correctness depends on undocumented last-write-wins behavior.

State fields that must remain mutually coherent for one delivered input belong in one cohesive Reducer transition, normally implemented by invoking one cohesive `AppState` domain operation wholly within that callback. For example, applying one fill should update execution deduplication, order state, position, cash, and fees as one transition when those values share an invariant.

Multiple Reducers for one payload are appropriate for independent projections or deliberately ordered application transitions. Stable registration order makes dependencies deterministic, but Kavod does not inspect field access or prove that a later Reducer observed the intended earlier mutation. Depending on sibling Reducer order is application coupling that must be explicit and tested; it is not coherence supplied by the kernel.

Kavod does not introduce one logical writer per state container. The application is the single logical owner, the kernel thread is the single physical writer, and Reducers are the ordered mutation sites.

## Atomicity

A Reducer may transition several parts of `AppState` before another callback runs.

This provides atomic visibility:

- No ordinary Component observes a Reducer halfway through.
- No callback overlaps another callback.
- The next Reducer sees the completed mutations of the previous Reducer.

It does not provide rollback. A panic after partial mutation terminates the Engine according to its failure policy; Kavod does not resume execution as though the transition never happened.

Application state with strong internal invariants should expose cohesive domain operations. For example, applying a fill should update cash, position quantity, fees, realized PnL, order state, and deduplication through one operation rather than unrelated assignments spread across callbacks.

## Dynamic State

`AppState` has one statically known Rust type, but its contents are dynamic.

Typical structures include:

```text
Instruments: InstrumentId -> Instrument
Orders: ClientOrderId -> Order
Positions: (AccountId, InstrumentId) -> Position
Bars: BarType -> BarHistory
Accounts: AccountId -> AccountState
```

Discovering a new instrument, creating an order, opening a position, or beginning a new timeframe does not change the application graph or state type. It inserts an entity into an application-defined collection.

Multiple configured instances of one state family are represented through ordinary fields, nested structures, or maps. No newtypes or separate cache slots are required merely to distinguish accounts, venues, strategies, or timeframes.

Kavod core does not distinguish low-cardinality configured keys from unbounded runtime IDs inside `AppState`. That distinction may matter for routing and configuration, but storage organization belongs to the application.

Orders, positions, bars, and instruments are state entities, not graph nodes.

## Component Composition

A reusable Component need not know the application's state layout.

The preferred composition pattern is:

```text
Reusable Component
    -> emits typed Message
    -> application registers Reducer for that Message
    -> Reducer maps it into the application's AppState
```

For example, a reusable bar aggregator may emit `BarCompleted` when one closure is independently actionable, or `BarsClosed` when consumers require coherent visibility of every closure caused by one input. The application decides whether its Reducer:

- Stores every completed bar.
- Retains only the latest bar.
- Partitions bars by strategy or timeframe.
- Does not store bars at all.

This preserves Component modularity without requiring Components to register new cache slot types.

Application-specific Components may read the complete `AppState` directly.

## State Dependencies

Kavod does not require field-level read or write declarations.

Every ordinary Component callback may read the complete immutable `AppState`. Every Reducer callback may read and mutate the complete `AppState`.

A declaration such as "reads positions" would not enforce anything if the callback still received the full state root. Genuine enforcement would require narrow generated views or field lenses, adding substantial complexity without supporting a current semantic requirement.

The graph therefore records:

- Which Port Event, ControlEvent, or Message invokes a Reducer.
- Which Port Event, ControlEvent, or Message invokes an ordinary Component callback.
- That Reducers may mutate canonical state.
- That ordinary Components may read canonical state.
- Which Messages, Port Commands, and ControlCommands ordinary callbacks may produce.

It does not record individual `AppState` fields or dynamic entity reads.

This weakens field-level dependency inspection but keeps the graph truthful. Kavod must not present unenforced read annotations as authoritative metadata.

## Dependency Errors

A single concrete `AppState` removes top-level missing-state dependencies.

### Construction-Time Structural Errors

- No initial `AppState` was supplied.
- A callback was registered against an incompatible application state type.
- Initial state fails an application-defined validation performed during construction.
- Existing Event, Message, Command, or Port graph validation fails.

### Runtime Domain Conditions

- An order ID is unknown.
- An instrument has not been loaded.
- A position does not exist.
- A bar series has insufficient history.
- An execution report is duplicated.
- A state transition is illegal.
- Broker-observed state disagrees with local state.

These depend on runtime data and remain application domain concerns.

## State Identity And Evolution

`TypeId`, Rust type names, memory addresses, and registration order are not durable state identities.

For the current in-memory kernel, `AppState` needs no runtime lookup identity because it is one concrete value owned by the Engine.

If diagnostic state encoding or state hashes are later introduced, the entire `AppState` is one application schema unit. Stable identity would belong to the application state schema, not to every field or collection.

The application would own:

- State layout.
- Domain invariants.
- Deterministic encoding.
- Schema versioning.
- Migration semantics, if migration is ever supported.
- Stable collection ordering where behavior or encoding observes iteration.

Kavod core owns only the callback boundary at which state may be observed diagnostically. MVP tracing records a `ReducerMutationBoundaryCompleted` marker after a Reducer completes with mutable access; it records no old value, new value, field-level diff, state serialization, or state hash.

Component-private state must be considered separately if later diagnostic replay or state verification needs to account for all behavior-affecting state.

## Comparable Patterns

Robust state machines commonly use one concrete state root transitioned by ordered inputs. NautilusTrader similarly uses one concrete cache containing typed maps keyed by dynamic domain IDs rather than a `TypeId` typemap.

Event-sourced systems often separate aggregates and projections because they require independent persistence streams, asynchronous materialization, or rebuildable read models. Kavod does not currently have those requirements and should not import that ownership model.

ECS and simulation frameworks often use typed resource registries to support independently composed systems and conflict scheduling. Kavod's reusable Components can instead communicate through Messages, while application Reducers adapt those Messages into one application-owned state model.

## Comparison Of Viable Models

| Model | Strengths | Failure modes | Decision |
|---|---|---|---|
| `TypeId` typed cache | Easy heterogeneous registration | Service locator semantics, one value per type, weak durable identity, casting internally | Rejected |
| Stable keyed state slots | Multiple instances and explicit identity | More registration, handles, schemas, and dependency machinery than Kavod needs | Rejected |
| Dedicated projector-owned state | Clear update responsibility | Misrepresents Reducers as owning objects and complicates cross-state transitions | Rejected |
| One concrete `AppState` | Fully typed, dynamic internal data, simple ownership, cohesive transitions, clear schema boundary | Broad read/write authority and hidden field dependencies | Preferred |

## State Rules

1. Each application has exactly one concrete canonical `AppState`.
2. The application logically owns `AppState`; the Engine physically owns it.
3. Only Reducer callbacks receive mutable canonical-state access.
4. Ordinary Component callbacks receive immutable canonical-state access.
5. Components may mutate only their own private state.
6. Reducers are callbacks, not state owners or projector objects.
7. Reducers execute before ordinary Components for each delivered input.
8. Multiple Reducers execute in stable registration order.
9. Dynamic entities live inside application-defined collections in `AppState`.
10. State fields and dynamic entities are not application graph nodes.
11. Field-level reads and writes are not declared or validated.
12. Canonical state must not use interior mutability to bypass Reducer-only mutation.
13. Live Port implementations and simulated models cannot borrow or mutate application state.
14. Reducers have no private mutable state or behavior-affecting mutable captures.
15. Canonical fields that must remain mutually coherent for one input are transitioned within one cohesive Reducer callback, normally through one cohesive `AppState` operation.
16. Multiple same-input Reducers do not create an inferred field-dependency or coherence guarantee.

## Explicit Non-Goals

Kavod does not currently provide:

- A generic cache schema.
- State-slot registration.
- Field-level dependency graphs.
- Per-container writer ownership.
- Automatic state migration.
- State restoration or recovery.
- Transactional Reducer rollback.
- Proof that arbitrary application Rust code cannot mutate through an escape hatch.
- State APIs designed around speculative parallel execution.
- A generic callback return or application-error channel for Reducers or Components.

## Rejected Alternatives

- **`BTreeMap<TypeId, Box<dyn Any>>`:** deterministic iteration does not repair weak semantic identity or service-locator access.
- **String-keyed typed state:** requires registration and casting machinery that a concrete root avoids.
- **One graph node per dynamic entity:** would make the graph runtime-mutable and unbounded.
- **Mandatory read declarations:** unenforceable while callbacks receive the complete `AppState`.
- **One logical writer per container:** unnecessary because Reducers are callbacks over one application-owned state root.
- **Projector Components as state owners:** imports an abstraction that does not match Kavod's callback model.

## Dependencies On Other Discussions

- The turn-scheduling design settles Reducer visibility as local to each delivered input and uses explicit aggregate domain facts for correlated derived updates. It adds no generic Reducer phase or turn-wide state-settlement guarantee.
- Fatal establishment poisons the Engine; partially mutated state is not rolled back or reusable and the Engine never resumes. A contained Port worker panic follows the separate ControlPlane failure-reporting policy and does not interrupt deterministic application execution merely because the worker failed.
- The observability design defers state hashes, state encoding, and state-value recording; the MVP records only the successful Reducer mutation boundary.
- The Component identity discussion is relevant only if behavior-affecting private state is later recorded or compared.

## Open Questions

No unresolved question blocks the canonical-state model.

The following details remain deliberately deferred:

- The exact callback and context API used to expose immutable or mutable `AppState`.
- Whether initial state validation has a standardized hook or remains application construction logic.
- Whether diagnostic state hashing will ever require a stable application-state schema identifier.
- How Component-private state participates in any later diagnostic replay or state comparison.
