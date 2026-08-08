# Kavod Environment Design v8

> **Status:** Initial semantic draft
> **Authority:** `design_docs/design-v8.md`
> **Scope:** Port protocols and built-in live and simulated Environments
> **Priority:** The smallest statically typed design that realizes the Core Environment contract

---

## 1. Boundary

This document refines the Environment boundary in `design-v8.md`. Core lifecycle, operation commitment, command-batch prefix, audit, Fatal, and Engine rules remain defined there.

An Environment owns bindings, routing, queues, execution, logical-time production, and mode-specific bounds. A Port receives only its Contract protocol and mode-specific authority.

Rust syntax is illustrative. Unresolved choices are isolated in Section 9.

## 2. Contracts And Routing

```rust
trait PortContract {
    type Event;
    type Command;
}
```

A Port Contract owns no binding identity, capacity, lifecycle, or execution mode.

The Application Event is a closed source-qualified sum. The Application Command is a closed destination-qualified sum. Distinct bindings using one Contract remain distinct variants.

```rust
enum TradingEvent {
    Primary(MarketDataEvent),
    Secondary(MarketDataEvent),
    Execution(ExecutionEvent),
    Timer(TimerEvent),
}

enum TradingCommand {
    Primary(MarketDataCommand),
    Secondary(MarketDataCommand),
    Execution(ExecutionCommand),
    Timer(TimerCommand),
}
```

`AppCommand` denotes `Application::Command`. A Port Command is the associated `C::Command`; there is no third Command envelope.

Command routing consumes one AppCommand in batch order. Variant matching and typed handoff are one exhaustive operation:

```rust
match command {
    TradingCommand::Primary(command) => {
        handoff::<MarketData>(&mut bindings.primary, command)
    }
    TradingCommand::Secondary(command) => {
        handoff::<MarketData>(&mut bindings.secondary, command)
    }
    TradingCommand::Execution(command) => {
        handoff::<Execution>(&mut bindings.execution, command)
    }
    TradingCommand::Timer(command) => {
        handoff::<Timer>(&mut bindings.timer, command)
    }
}
```

The variant establishes destination and its payload establishes the Contract Command type. Routing does not group, reorder, retry, downcast, or consult payload metadata.

Event injection is the inverse operation. A binding wraps one typed `C::Event` in its fixed application Event variant. A Port cannot choose application source identity.

Binding registration order is frozen by construction. It provides deterministic binding identity and simulation tie breaking.

## 3. Construction, Bounds, And Errors

Live and simulation builders consume complete bindings and capacity configuration. They validate all bounds, allocate required bounded storage, and check aggregate arithmetic before run-scoped activity.

Configuration bounds at least:

- Binding count.
- Live Event ingress.
- Each Command inbox.
- First-failure retention.
- Encoded Environment-error bytes.
- Simulation pending-Command order.
- Simulation callback work.
- Logical-time domain.

Concrete Port errors implement `AuditEncode`. The binding wrapper immediately normalizes them into one bounded Environment error carrying binding identity, failure classification, and bounded detail. Encoding failure uses a fixed technical fallback.

The normalized representation supports the exact command-batch prefix information required by `design-v8.md`. The concrete Environment-error schema remains deferred.

## 4. Live Ports

### 4.1 Port And Context

```rust
trait LivePort<C: PortContract>: Send + 'static {
    type Error: AuditEncode + Send + 'static;

    fn run(
        self,
        ctx: LiveCtx<C>,
    ) -> Result<(), Self::Error>;
}
```

`run` is the sole live Port entry point and owns the Port for one run.

```rust
enum LiveInput<C> {
    Command(C),
    StopRequested,
    Aborted,
}

impl<C: PortContract> LiveCtx<C> {
    fn recv(&mut self) -> LiveInput<C::Command>;

    fn try_recv(&mut self) -> Option<LiveInput<C::Command>>;

    fn offer(
        &mut self,
        event: C::Event,
    ) -> Result<(), EventOfferError<C::Event>>;
}
```

`LiveCtx` is single-owner and exposes no logical clock, application protocol sum, State, Engine, Environment, AuditLog, or observable binding identity. It privately carries the binding-fixed source injection authority used by `offer`.

`Command` inputs preserve destination-inbox FIFO. Command claim and terminal control share the Environment commitment order. Once `StopRequested` or `Aborted` commits, queued but unclaimed Commands are abandoned and no later Command is returned. A Command claimed first remains Port-owned.

The terminal variants are Port-facing signals. `StopRequested` denotes private graceful-stop control, while `Aborted` denotes nongraceful closure after either Environment failure or explicit abort.

`offer` submits one typed Event through the binding's fixed source injection. Offer admission shares the Environment commitment order with terminal transitions and failure. It atomically succeeds with Event insertion, rejects after terminal commitment, or commits failure on ingress saturation. Exact offer-error variants remain deferred.

### 4.2 Live Storage And Commitment

The built-in LiveEnvironment uses:

- One global bounded source-qualified Event ingress.
- One typed bounded Command inbox per binding.
- One bounded first-failure latch.
- One commitment coordinator.
- One worker thread per binding.

The coordinator orders Event offer admission and delivery, each Command handoff and claim, worker-result reporting, first failure, and lifecycle transitions as required by `design-v8.md`. Event ingress saturation commits an Environment failure through the independent failure path.

`next_event` gives a committed failure precedence over undelivered Events. Otherwise it removes one Event, captures logical time, and commits the pair atomically.

### 4.3 Startup And Worker Exit

Live startup is:

```text
initialize preallocated queues and shared control
-> spawn every worker behind a closed publication gate
-> on spawn failure: cancel and join created workers; return Err
-> commit start Ok and Ready time
-> open the gate
-> each wrapper acknowledges entry immediately before invoking run
-> wait for every acknowledgement
-> return the committed Ok
```

Entry acknowledgement proves only that the worker passed the publication gate and reached the `LivePort::run` call site. Port connection and domain readiness remain Port protocol behavior.

A result after start commitment is a runtime result even if `Environment::start` has not physically returned. It cannot revoke the committed startup result.

On normal return, every wrapper reports its result before its thread exits. Result classification commits under the coordinator and is ordered against lifecycle transitions:

| Worker result | Environment meaning |
|---|---|
| `Ok` while Running | Unexpected worker exit |
| `Err` while Running | Port failure |
| `Ok` after `StopRequested` | Graceful binding completion |
| `Err` after `StopRequested` | Stop failure |
| Return after `Aborted` | No Core-visible result |

The first runtime failure follows the shared Environment commitment rule and causes other LiveCtx inputs to become `Aborted`.

### 4.4 Stop And Abort

Live stop first observes the commitment coordinator. With no failure it publishes private `StopRequested`; a prior failure retains `Aborted`. It then wakes every worker, abandons unclaimed Commands, and joins every worker. A failure that wins during stop upgrades remaining LiveCtx inputs to `Aborted`. Stop continues joining and returns the first committed failure; `Ok` requires every worker to complete successfully with no committed failure.

Live abort publishes `Aborted`, closes every LiveCtx capability, abandons queued inputs, signals every worker, and drops all `JoinHandle`s without waiting. LiveCtx backing has independent shared ownership and remains terminally closed. Detached workers retain no Environment-facing authority; process termination remains their ultimate cleanup.

## 5. Simulated Ports

### 5.1 Port And Contexts

```rust
trait SimPort<C: PortContract> {
    type Error: AuditEncode;

    fn start(
        &mut self,
        ctx: &mut SimStartCtx<'_, C>,
    ) -> Result<(), Self::Error>;

    fn on_command(
        &mut self,
        command: C::Command,
        ctx: &mut SimCtx<'_, C>,
    ) -> Result<(), Self::Error>;

    fn step(
        &mut self,
        ctx: &mut SimCtx<'_, C>,
    ) -> Result<Option<C::Event>, Self::Error>;

    fn stop(&mut self) -> Result<(), Self::Error>;
}
```

A SimPort is passive. The SimEnvironment invokes one callback at a time, and borrowed contexts cannot be retained.

Each binding owns one replaceable wake cursor:

```rust
impl<C: PortContract> SimStartCtx<'_, C> {
    fn now(&self) -> LogicalTime;
    fn set_next(&mut self, time: LogicalTime) -> Result<(), SimCtxError>;
    fn clear_next(&mut self);
}

impl<C: PortContract> SimCtx<'_, C> {
    fn now(&self) -> LogicalTime;
    fn set_next(&mut self, time: LogicalTime) -> Result<(), SimCtxError>;
    fn clear_next(&mut self);
}
```

`set_next` replaces the cursor and accepts equal time. Rejected past or out-of-domain time leaves the cursor unchanged, latches the first Context failure, and makes later Context mutations ineffective. The Environment checks the latch immediately after each callback; it takes precedence over the callback result and discards callback output.

Only `step` produces Events. `on_command` may request an immediate step through `set_next(ctx.now())`.

### 5.2 Startup And Command Handoff

Simulation startup establishes one prospective Ready time, then calls `start` once per binding in registration order. Every callback observes that time. Cursor mutations remain provisional until all callbacks succeed; success commits the Ready time and all cursors atomically, while error discards them and the SimEnvironment.

`command_batch` reserves capacity in the typed destination inbox and bounded global-order FIFO before each Command. Destination-inbox insertion is the handoff commitment; publication of the corresponding binding token is infallible at that same point. Both structures preserve FIFO, and failure before commitment changes neither. The method invokes no SimPort callback.

### 5.3 Event Selection

One simulated `next_event` performs bounded work. One configured callback budget covers every `on_command` and `step` and is checked before invocation:

```text
deliver all pending Commands in global handoff order
-> apply callback scheduling changes
-> select minimum (cursor time, registration order)
-> advance virtual time and clear the selected cursor
-> invoke the selected step once
-> if step returns Event: inject its source and return it
-> if step returns None: continue within the callback bound
```

Command callbacks run before cursor advancement. Callback failure returns an Environment error without changing the earlier Command handoff.

A Port must publish a replacement cursor for another step. If `next_event` is called with no pending Command or cursor, it returns an Environment error classified as no future Event. Normal source exhaustion is represented by an application-defined End Event; successful completion occurs when any handler returns Stop before quiescence.

Equal-time cursors use registration order. Ordering never depends on pointer identity, unstable iteration, or hidden state.

### 5.4 Stop And Abort

Simulation stop abandons all pending Commands and cursors, then calls every Port's `stop` in registration order. It continues after individual failures and returns the first failure in deterministic order.

Simulation abort abandons Commands and cursors and invokes no Port callback.

## 6. Logical Time

```rust
struct LogicalTime(u64);
```

`design-v8.md` owns LogicalTime semantics. Built-in Environments use a checked `u64` representation and validate conversion, configured maximum, and nondecrease before committing Ready or Event time. Equal values remain valid.

Builders configure the maximum logical time. The physical unit, epoch, and source or configured value of Ready time remain deferred.

## 7. Static Dispatch Requirements

Built-in Environments require exhaustive static routing. Command projection and simulated Event injection call concrete binding fields directly; no `Any`, downcast, runtime registry, or payload type erasure participates in the hot path. The zero-vtable representation of binding-fixed live Event injection remains deferred.

Generated code owns only binding storage, variant projection, source injection, registration identity, and concrete field calls. Generic Environment code implements the Core-defined lifecycle, commitment, failure, bound, waiting, and scheduling rules.

The exact generated artifacts and hidden macro bridge remain deferred.

## 8. Verification Obligations

Tests must establish:

- Every AppCommand variant reaches exactly one correctly typed binding.
- Every typed Port Event receives exactly one fixed source variant.
- Batch projection preserves original order and exact failure prefix.
- Live startup failure before commitment leaves no worker live.
- Live `start(Ok)` waits for every run-entry acknowledgement.
- Every normal worker return is reported exactly once.
- Terminal LiveInput preempts and abandons unclaimed Commands.
- Live stop joins all workers and abort waits for none.
- Detached workers cannot use closed LiveCtx capabilities.
- Simulation Commands run in global handoff order before cursor selection.
- Equal-time cursors use registration order.
- Only `step` produces simulated Events.
- Simulation stop abandons pending Commands.
- Invalid bound configuration fails before run-scoped activity.
- Runtime exhaustion fails before overflow, corruption, or partial item insertion.

## 9. Remaining Decisions

- Exact protocol macro syntax, generated artifacts, and hidden bridge traits.
- Concrete static binding and queue-product types.
- Exact `EventOfferError` variants and failed-offer ownership.
- Physical Port-worker panic propagation and process-termination mechanics; panic is never normalized into an Environment error.
- Logical-time unit, epoch, initial Ready baseline, and duration operations.
- Concrete normalized Environment-error schema and fixed fallbacks.
- Exact same-time callback bound and error classification.
- Concrete live queue, wakeup, and commitment-coordinator primitives.
- Zero-vtable binding-fixed source injection for duplicate uses of one Port Contract.
