# Kavod Core Design v9

> **Status:** MVP semantic draft (supersedes v8)
> **Scope:** The deterministic Core shared by live and simulated execution
> **Priority:** The smallest robust design implementable in days, whose rules can be enforced and tested

---

## 1. Engineering Thesis

Kavod is a deterministic application Core. One Engine owns one Application State, accepts one ordered Event, invokes one synchronous transition, hands off its ordered Commands, completes the turn, and only then accepts another Event.

The same frozen Application runs in every Environment. An Environment owns topology, waiting, Event selection, logical time, lifecycle orchestration, routing, and execution mode behind one Core-facing contract.

Each runtime Port owns all of its mutable domain and native state. The Environment contains and orchestrates bound Ports but neither owns nor interprets their domain state. Live and simulated execution differ in how their Ports are driven, not in where domain state resides.

The Journal is the ordered evidence of Engine execution. Only the Engine writes it.

Kavod is informed by NASA's Power of Ten, TigerBeetle's Tiger Style, and SQLite's defensive testing culture. These are influences, not claims of compliance. The enforceable rules are:

| Principle | Kavod rule |
|---|---|
| Correctness before convenience | Add no feature without enforceable semantics |
| Single authority | Each fact has one owner and one semantic representation |
| Explicit execution | One Event, handler invocation, and turn at a time |
| Finite resources | Every Kavod-managed container, buffer, count, and identifier has a configured maximum |
| Bounded local work | Kavod-owned active loops are bounded and nonrecursive |
| Checked arithmetic | Counts, lengths, capacities, times, and identities never wrap or silently saturate |
| Explicit failure | A failed required operation establishes one Fatal cause |
| Assertions mean bugs | Invariant violations panic and are outside Engine outcomes |
| Defensive boundaries | Validate knowable failure conditions before irreversible actions |
| Evidence-driven engineering | Every bound and failure boundary supports direct and fault-injection testing |

v9 preserves the guarantees of v8 while replacing bespoke mechanisms with standard primitives: the Journal is serde JSON Lines over `std::io::Write`, Event and Command sums are plain enums written by hand or expanded by one declarative sugar macro, and the live Environment uses threads and a bounded Event queue. Simplicity is taken in mechanism, never in semantics. Robustness remains the first goal.

Resource bounds are semantic. Allocation strategy is an implementation choice.

Rust syntax is illustrative. Concrete APIs and storage remain implementation choices unless required by these semantics.

Kavod Core is written under `#![forbid(unsafe_code)]`.

## 2. Core Model

An Engine owns one run:

- One frozen Application.
- One concrete Application State.
- One Environment with matching Event and Command types.
- One Journal.
- One bounded turn-local Command batch.
- One checked Event-index domain.

Only the Engine passes State to application code. At most one handler call is active.

One accepted start or External Event creates one turn. The handler runs to normal return before the Engine processes its Outcome. A turn completes or the run is Fatal before another Event is requested.

An accepted External Event has one authoritative representation:

```rust
struct EventEnvelope<E> {
    index: EventIndex,
    logical_time: LogicalTime,
    event: E,
}
```

`EventIndex` and `LogicalTime` are `Serialize` newtypes with transparent `u64` JSON representations. `EventIndex` is checked. The start turn has index zero, and the first External Event has index one.

`LogicalTime` is an opaque `u64` count of nanoseconds with an Environment-owned origin. It is owned by the Environment's single Event acceptor: exactly one authority stamps it. Port-domain timestamps, such as exchange or receive time, are ordinary Event payload fields and carry no Core meaning.

Successful `Environment::start` and `Environment::next_event` times must never decrease. A regression is an Environment bug and is Core Fatal. Equal logical times are valid; Event index is the sole Event order.

Violations by user-supplied components, such as an Environment returning a regressed time, are graded failures reported through typed Errors and the Fatal path. Only Kavod-internal invariant violations panic.

The Core determinism contract is:

> Within one concrete Environment type, the same executable build, frozen Application, initial State, configuration, complete accepted `(Event, LogicalTime)` and Environment-result trace, and Journal-writer call-result trace produce the same handler calls, State transitions, ordered Command intent, journal bytes, and typed EngineExit.

Across live and simulated Environments, equal Engine configuration and capacities, equal accepted Event traces, and equal abstract operation-result traces produce the same handler calls, State transitions, ordered Command intent, Journal protocol sequence, and `NormalizedExit`. Concrete Environment Error text and therefore Fatal-record message bytes may differ by mode.

```rust
enum NormalizedExit {
    Stopped,
    Fatal(FatalClass),
}

/// The mode-independent classification of the primary FatalCause.
enum FatalClass {
    Application,
    Environment(EnvironmentOperation),
    Journal(JournalFailureKind, RecordKind),
    Core(CoreFailureKind),
}

enum JournalFailureKind {
    Encode,
    RecordBoundExceeded,
    SinkWrite,
    SinkFlush,
}

enum CoreFailureKind {
    TimeRegression,
    TurnBoundExceeded,
    CommandBoundExceeded,
}
```

Normalization maps `EngineExit::Stopped` to `Stopped` and maps each primary `FatalCause` to its `FatalClass`, retaining every mode-independent discriminant: the `EnvironmentOperation` including dispatch position, the Journal failure and record classification, and the Core failure variant. Concrete error values and `Display` text are discarded. Secondary finalization Errors do not replace the normalized primary classification.

Live Environments may produce nondeterministic traces when concurrent sources race. That nondeterminism is explicit in the accepted Environment trace; the Core remains deterministic conditional on that trace. Application behavior and journal content must not independently depend on hidden clocks, entropy, IO, environment variables, process-global mutable state, concurrent task order, pointer identity, unstable iteration, or Environment mode. Application-owned serialization must likewise avoid unstable iteration order.

## 3. Application

Conceptually:

```rust
trait Application {
    type State;
    type Event: Serialize;
    type Command: Serialize;
    type FatalReason: Display;

    fn initial_state(&self) -> Self::State;

    /// The start turn (index zero). It may stage Commands.
    fn on_start(
        &self,
        state: &mut Self::State,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::FatalReason> {
        Outcome::Continue
    }

    fn on_event(
        &self,
        state: &mut Self::State,
        event: &EventEnvelope<Self::Event>,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::FatalReason>;
}

enum Outcome<F> {
    Continue,
    Stop,
    Fatal(F),
}
```

The Application, its deterministic configuration, and all Engine capacities are frozen before `Engine::run`.

All run-varying mutable application data resides in State. The frozen Application and accepted Event and Command logical values remain immutable.

The handler may mutate complete State and stage Commands through Context. It receives no Environment, Journal, external IO, clock, entropy, or concurrency authority.

`Context::emit` is infallible and appends immutable Commands to the bounded current-turn batch in call order. The batch stores at most `max_commands_per_turn` Commands. The first emit beyond the bound stores nothing and sets an overflow marker; later emits also store nothing. After the handler returns, a set marker is Core Fatal and discards the batch, taking precedence over the returned Outcome. Commands are never silently dropped, coalesced, or reordered.

Conceptually, `Context::emit(&mut self, command: Command)` transfers ownership of one Command. Context exposes the immutable current Event index and LogicalTime, so `on_start` can observe the accepted start time without a synthetic `CoreEvent::Ready` wrapper, and `remaining()`, the exact number of additional Commands the current batch can still store; it is zero once the overflow marker is set.

Absent a higher-precedence Core, Environment, or Journal failure, `Continue` completes the turn and `Stop` dispatches the current batch before graceful shutdown. `Fatal` discards the batch and ends normal execution.

Internal application structure has ordinary Rust semantics. Work for a future turn returns through an External Event.

## 4. Ports, Contracts, And Slots

A Port Contract pairs one Event protocol with one Command protocol. Every Contract is duplex; a direction with no messages uses the uninhabited `Never` type:

```rust
trait PortContract {
    type Event: Serialize;
    type Command: Serialize;
}

/// Kavod-provided uninhabited type for an absent direction.
enum Never {}

struct MarketData;

impl PortContract for MarketData {
    type Event = MarketDataEvent;
    type Command = Never; // Event-only Port.
}
```

Kavod implements `Serialize` for `Never` by exhaustive matching. No `Never` value can be constructed.

A runtime Port is one mode-specific implementation of one bound Slot. A live Port may own a native client, connection state, and protocol state. A simulated Port may own an order book, replay cursor, or timer state. This state belongs exclusively to the Port. Terminal Port state is recovered through user-owned handles captured before binding, never through the Engine.

A Slot is one named use of a Contract. Distinct Slots of one Contract are distinct variants. An Application uses one closed, source-qualified Event sum and one closed, destination-qualified Command sum whose variants are its Slots:

```rust
kavod::ports! {
    pub enum TradingEvent / TradingCommand {
        Primary:   MarketData,
        Secondary: MarketData,
        Execution: Execution,
        Timer:     Timer,
    }
}
```

`ports!` is a declarative `macro_rules!` macro. It expands to exactly the two paired sums, with matching variant names and serde's default externally tagged `Serialize` representation:

```rust
#[derive(::serde::Serialize)]
pub enum TradingEvent {
    Primary(<MarketData as PortContract>::Event),
    Secondary(<MarketData as PortContract>::Event),
    Execution(<Execution as PortContract>::Event),
    Timer(<Timer as PortContract>::Event),
}

#[derive(::serde::Serialize)]
pub enum TradingCommand {
    Primary(<MarketData as PortContract>::Command),
    Secondary(<MarketData as PortContract>::Command),
    Execution(<Execution as PortContract>::Command),
    Timer(<Timer as PortContract>::Command),
}
```

The macro is pure syntax sugar with no semantic authority. Hand-writing enums with the same variants and serde representation is fully supported and observationally identical. The macro generates no routing, no topology type, and nothing that participates in the Engine or Environment contracts. Macro consumers already require serde for their protocol payloads and must have a direct dependency named `serde`; this keeps generated derives hygienic without custom derive machinery. The MVP Engine may initially use hand-written sums; the macro is implemented after the Engine runs end-to-end.

Wiring remains explicit and compiler-checked:

- Fan-in uses a variant constructor per inhabited Event direction at wiring time. An enum variant constructor is already a function: `TradingEvent::Primary` maps one `MarketDataEvent` into one `TradingEvent`.
- Fan-out is one hand-written `match` in the Environment's dispatch path, with exhaustiveness enforced by the compiler. An arm with a `Never` payload is discharged by matching the uninhabited value: `TradingCommand::Primary(never) => match never {}`.
- Each binding also supplies a mapping from the bound Port's `Error` into the Environment's `Error` sum, so heterogeneous Port failures become one typed Environment Error.

Adding a Slot is one line in `ports!`, one fan-in wiring line when its Event direction is inhabited, one exhaustive dispatch arm, and one Error-mapping arm.

The compiler guarantees that wiring matches are exhaustive and that routed payload types agree with their Port Contracts. It does not prove that a routing arm selects the semantically correct Slot or that user-written wiring is free of side effects. Correct one-to-one routing is a trusted Environment-configuration obligation subject to direct per-Slot wiring tests. Wiring code owns no domain state; observable mutable domain state belongs to the bound Ports.

Command handoff has one commitment point per Environment mode, defined in Section 5. Subsequent processing belongs to the bound Port. An externally consequential Command carries an Application-owned stable business key sufficient to recognize a repeated or uncertain external effect. Correct key scope and uniqueness are Application obligations.

## 5. Environment

Conceptually:

```rust
trait Environment {
    type Event;
    type Command;
    type Error: Display;

    /// Erect run-scoped machinery and freeze the start turn's LogicalTime.
    fn start(&mut self) -> Result<LogicalTime, Self::Error>;

    /// Wait for and return the next authoritative Event and LogicalTime.
    fn next_event(
        &mut self,
    ) -> Result<(Self::Event, LogicalTime), Self::Error>;

    /// Attempt one Command handoff.
    fn dispatch(
        &mut self,
        command: Self::Command,
    ) -> Result<(), Self::Error>;

    /// Consume the Environment and release all run-scoped machinery.
    fn shutdown(self, mode: ShutdownMode) -> Result<(), Self::Error>;
}

enum ShutdownMode {
    Graceful,
    Abort,
}
```

`Engine<A, E>` requires equal Application and Environment Event and Command types.

After successful Engine construction, the Engine is the only caller and guarantees this call order: `start` exactly once; then `next_event` and `dispatch` interleaved, one call at a time; then `shutdown` at most once. There is no Environment lifecycle state machine.

- `start` returning `Err` leaves no run-scoped activity live. The Environment cleans up after itself and is safe to drop.
- `next_event` owns waiting, source selection, and time stamping. It waits until it returns one authoritative `(Event, LogicalTime)` pair or one Error. A returned Event is a committed candidate: it is consumed exactly once and is never retried or revoked. It becomes an accepted Event only after `EventAccepted` is successfully flushed.
- `dispatch` attempts one Command handoff without waiting for future capacity. `Ok` commits the handoff. `Err` guarantees that this Command was not handed off; the Engine does not retry it.
- `shutdown(Graceful)` stops Event delivery, rejects new Commands, and resolves the configured graceful disposition of handed-off Commands. `shutdown(Abort)` stops Event delivery and new Command handoff without initiating further externally consequential work.
- `shutdown` consumes the Environment, quiesces all run-scoped activity, and returns when it is safe to drop even when it returns `Err`. Its Error reports failure to achieve the requested disposition, not failure to quiesce.
- Any Error from a normal fallible Environment operation is Fatal to the run. An Abort cleanup Error is retained as a secondary finalization Error and never replaces an existing primary cause.

Each Environment retains at most one first Port failure; later Port failures do not replace it. Every Environment operation has one commitment point. A Port failure observed before the active operation's commitment makes that operation return `Err`. A Port failure observed after commitment does not revoke it: the active operation returns its ordinary result, and the retained failure is returned by the next fallible Environment boundary before that boundary performs another commitment.

For Command dispatch:

- A failure before handoff returns `Err` and guarantees that the current Command was not handed off.
- A Port processing failure after handoff does not revoke the handoff; the dispatch whose handoff succeeded returns `Ok`.
- The retained Port failure is returned by the next `dispatch`, `next_event`, or graceful `shutdown` before that operation performs another handoff or Event commitment.

This rule is identical across live and simulated Environments. Live handoff commits at destination-inbox admission. Simulated handoff commits when the selected Port's `on_command` invocation begins.

Every Environment operation preserves the Environment's own configured bounds, including channel capacities, queue depths, thread counts, wakeup storage, and shutdown work. Successfully returned Events and handed-off Commands are never silently overwritten, coalesced, or duplicated. Neither Environment mode may invoke the Application handler.

### 5.1 Live Environment

A live Port is conceptually:

```rust
trait LivePort<C: PortContract>: Send + 'static {
    type Error: Display + Send + 'static;

    fn run(
        self,
        ctx: LiveCtx<C>,
    ) -> Result<(), Self::Error>;
}
```

The concrete `LiveCtx` API is an implementation choice. Semantically it provides reception of Commands handed off to this Slot, nonblocking inspection of pending Commands, offering of one typed Port Event through the Slot's frozen fan-in constructor, and direct observation of run lifecycle signaling.

Lifecycle signaling is Context authority. Graceful-stop and Abort signals are not application Events, Port Events, or Commands, and they consume no Event or Command inbox capacity. The exact disposition of previously handed-off Commands under Graceful and Abort shutdown is a concrete Environment policy subject to the common `Environment::shutdown` contract.

A live Environment runs each bound Port in one supervised thread. Each Port owns its native client and all of its domain and protocol state.

Event fan-in uses one configured bounded queue. A Port offers only its Contract Event type; the frozen binding maps it into the Application Event sum before queue admission. Queue-full and disconnect are reported to the offering Port without silently dropping, overwriting, coalescing, or duplicating the Event and without silently waiting for future capacity. A Port that cannot make progress returns an Error, which the Environment latches as a Port failure.

`next_event` waits until either the first Port failure is latched or one queued Event is available:

```text
loop:
    check the first-failure latch
        when set: return Err(failure)
    check or wait for the Event queue
        when an Event is available: stamp and return it
```

The wait is awakened by either condition and does not busy-spin. Event selection and first-failure observation have one Environment-defined order: a failure latched before an Event is committed preempts that queued candidate, and a failure latched afterward cannot revoke the committed Event and is reported by the next fallible Environment operation. The single Event acceptor stamps `LogicalTime` from one monotonic clock, making time regression structurally impossible in correct operation. Monotonic-duration conversion is checked; exhaustion is an Environment Error.

Each Command destination Port owns one configured bounded Command inbox. `dispatch` is the frozen hand-written destination match and performs one nonblocking inbox admission. Successful admission is the Command handoff commitment. Queue-full, disconnect, or a previously latched Port failure returns Error before admission. Native Port processing occurs later inside the owning Port and does not alter the handoff commitment.

Port completion is supervised. `run` returning `Err` latches a typed Port failure. A Port thread panic is contained at the Port boundary and latches a typed `PortPanicked` Environment failure. Unexpected `run` completion while the Environment remains Running latches a premature-Port-closure failure; a finite Event source offers its application-defined terminal Event and then waits for shutdown. Each of these conditions wakes a blocked `next_event`.

`start` spawns the supervised Port threads. A `start` failure after some threads were spawned signals and joins them before returning `Err`.

`shutdown` publishes lifecycle state through every Port's Context, closes Engine-facing admission, and joins every supervised Port thread. It continues signaling and joining Ports after observing one shutdown failure and retains the first Error. Port implementations must make every blocking point lifecycle-aware and must cooperate with shutdown. Blocking duration is not an active-loop bound and Kavod promises no wall-clock termination deadline. Live Event and Command types, Port errors, and values moved into Port threads must be `Send + 'static`.

### 5.2 Simulated Environment

A simulated Port is conceptually:

```rust
trait SimPort<C: PortContract> {
    type Error: Display;

    fn start(
        &mut self,
        ctx: &mut SimCtx<'_, C>,
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

The concrete `SimCtx` API is an implementation choice. Semantically it provides:

```rust
impl<C: PortContract> SimCtx<'_, C> {
    fn now(&self) -> LogicalTime;
    fn set_next(&mut self, time: LogicalTime) -> Result<(), SimCtxError>;
    fn clear_next(&mut self);
}
```

Each simulated Port owns all of its simulated domain state. The Environment owns no shared simulation model and provides no transaction or rollback mechanism.

`start` fixes the Environment's start LogicalTime, then invokes every bound Port's `start` in frozen Slot order with `now` equal to that time; the first Error fails `Environment::start`.

`dispatch` executes synchronously. Its frozen exhaustive routing match selects one bound Port and calls that Port's `on_command` directly. No simulated Command inbox exists, and `now` does not advance during `dispatch`. Invocation of `on_command` is the Command handoff commitment. If `on_command` returns `Err`, the handoff remains committed, all Port mutations remain real, and the Environment latches the Error as a post-handoff Port failure; the current `dispatch` returns `Ok`, and the next fallible Environment operation returns the latched Error before performing another handoff or Event commitment.

Each bound simulated Port has at most one next-wakeup arm:

- `set_next(time)` requires `time >= now`.
- Repeated `set_next` calls are last-call-wins.
- `clear_next` removes the current arm.
- An arm is a revocable Port wakeup, not a committed Event.
- No Event exists until `step` returns `Some(event)`.
- Commands and earlier equal-time turns may alter or cancel a later Port's wakeup.
- `SimCtx` can modify only the arm belonging to its bound Port.

`next_event` repeatedly:

```text
check the first-failure latch
    when set: return Err(failure)
select the armed Port with the lowest LogicalTime
    when no Port is armed: return Err(SimQuiescent)
advance now to the selected LogicalTime
clear the selected Port's arm
invoke that Port's step
    on Err: return the Port failure
    on Some(Event): apply the frozen fan-in constructor and return the Event
    on None: continue under max_steps_per_event
```

Equal-time armed Ports use deterministic round-robin selection in frozen Slot order. After every selected `step`, including one returning `None`, the equal-time cursor advances to the Slot after the selected Port, so a Port that re-arms itself at the current time cannot permanently starve another armed equal-time Port.

Every `step` invocation, including one returning `Some`, consumes one unit of the configured `max_steps_per_event` budget. The bound is checked before invoking a step that would exceed it; exhaustion is a simulated Environment Error under `EnvironmentOperation::NextEvent`.

`shutdown` invokes every bound Port's `stop` in frozen Slot order, continues after observing one Error, and retains the first Error. Simulated Command processing is fully synchronous, so Graceful and Abort coincide for simulated Ports and `stop` takes no `ShutdownMode`.

The simulated Environment uses no concurrency. Port determinism, bounded work, and avoidance of hidden authority are trusted Port obligations subject to repeatability tests.

Normal simulation completion is an application-defined External Event whose handler returns `Stop`. An Environment Error is always Fatal: reaching the end of input data with no armed Port is `SimQuiescent`, not a successful exit. Environments that replay fixed input therefore accept a constructor for the terminal application Event at wiring time.

## 6. Journal

The Journal is ordered, human-readable evidence of Engine execution: forensics, not a crash-proof write-ahead log. It is JSON Lines. Each record is one `serde_json` object on one line. The line number is the sequence and the newline is the frame. There is no bespoke encoding, framing, sequence domain, pending store, or synchronization policy.

Conceptually:

```rust
struct Journal<W: std::io::Write> {
    writer: W,
    // One reusable bounded record buffer, one bounded Fatal-message buffer,
    // and a poison marker.
}

#[derive(Serialize)]
enum CompletedOutcome {
    Continue,
    Stop,
}

enum RecordKind {
    RunStarted,
    EventAccepted,
    CommandsPrepared,
    CommandsDispatched,
    StopRequested,
    TurnCompleted,
    Fatal,
}

#[derive(Serialize)]
enum FatalKind {
    Application,
    Environment,
    Journal,
    Core,
}

#[derive(Serialize)]
enum Record<'a, E: Serialize, C: Serialize> {
    RunStarted {
        schema_version: u32,
        logical_time: LogicalTime,
    },
    EventAccepted {
        index: EventIndex,
        logical_time: LogicalTime,
        event: &'a E,
    },
    CommandsPrepared {
        index: EventIndex,
        commands: &'a [C],
    },
    CommandsDispatched {
        index: EventIndex,
    },
    StopRequested {
        index: EventIndex,
    },
    TurnCompleted {
        index: EventIndex,
        outcome: CompletedOutcome,
    },
    Fatal {
        schema_version: u32,
        index: Option<EventIndex>,
        kind: FatalKind,
        message: &'a str,
    },
}
```

`schema_version` identifies the record schema of one build. `RunStarted` and `Fatal` are the only possible first records of a journal, so every journal begins with a versioned record.

`W: std::io::Write` is the persistence abstraction. A File, `Vec<u8>`, network stream, or any other byte sink may be used. Memory sinks make Journal tests and fault injection direct. The Engine consumes `W`; a test or embedder that needs the bytes after the run passes a user-owned shared handle, such as an `Rc<RefCell<Vec<u8>>>` wrapper implementing `Write`.

Writing a record is:

```text
encode through a max_record_bytes-limited writer into the reusable buffer
-> write the complete encoded object and newline to W
-> flush W
```

`max_record_bytes` bounds the encoded JSON object and excludes the one terminating newline byte. The bounded encoder rejects bytes beyond the bound before extending the buffer. Encoding failure or bound exhaustion occurs before any bytes for that record reach `W`; it is Journal Fatal.

The Journal writes directly from the complete record buffer, without a second `BufWriter`. Its own partial-write loop permits at most one successful progress call per output byte plus one terminal call. It does not retry `Interrupted`. `Ok(0)` while bytes remain becomes `io::ErrorKind::WriteZero`. Either condition is a sink write failure. A write or flush Error poisons the Journal permanently. Bytes after the last successfully flushed record are an uncertain trailing suffix and are not Journal records, even if they happen to contain a complete line. The Journal makes no further explicit `Write::write` or `Write::flush` call, including a Fatal write, through a poisoned writer; behavior of the user-owned writer's destructor is outside this contract.

`flush` after every record is the complete Journal persistence guarantee: durability beyond the operating system's write interface, including power-loss durability, is outside the MVP contract. A sink is fresh for one run or is initially positioned immediately after a newline. Journal sequence is run-relative. A run that does not need evidence may use `std::io::sink()` as `W`; encoding cost remains.

Records evidencing handler invocation and Command handoff are flushed before those actions: `EventAccepted` before the handler is invoked, and `CommandsPrepared`, containing the complete application-defined serialized intent, before the first dispatch. Environment Event acquisition commits earlier and may be followed by validation or Journal failure without structural evidence of the candidate Event.

Application Events and Commands appear through their application-defined `Serialize` representations. Kavod does not claim that a custom or lossy serializer captures fields it omits. Application and Environment primary failures remain typed in `EngineExit` and are rendered through `Display` only at the Journal boundary. Deterministic, side-effect-free `Serialize` and `Display` implementations are trusted Application and Environment obligations. JSON constrains all serialized payloads: non-finite floating-point values and map keys that are not representable as JSON strings are encoding failures, and map iteration order must be stable across runs.

Fatal text is formatted immediately when the primary cause is established, into a separate buffer bounded by `max_fatal_message_bytes`, with deterministic truncation at a UTF-8 boundary. Construction proves that the maximally escaped message plus the largest Fatal envelope fits within `max_record_bytes`, so a well-formed Fatal record always encodes. A `Display` implementation that returns `fmt::Error` falls back to a fixed static descriptor. Both buffers reserve their complete configured capacity before run-scoped activity begins.

The Fatal record is attempted at most once, only while the Journal is unpoisoned. Construction makes its encoding infallible; if the Journal is already poisoned or the write or flush fails, the primary cause still reaches `EngineExit` and the Error is retained as `journal_error`. Finalization is never re-entered.

An Application whose Event and Command types also implement `serde::de::DeserializeOwned` may use a separately owned Journal-reading schema to construct a replay script for the simulated Environment. A reader must reject a line after `max_record_bytes + 1` bytes without first allocating the complete line. Replay is enabled by the JSONL format but is not required for the MVP. After a sink failure, bytes beyond the last successfully flushed record are an uncertain suffix: JSONL bytes alone cannot identify the committed record prefix, so replay requires a cleanly completed journal or an externally trusted committed boundary.

## 7. Execution

### 7.1 Construction And Startup

Conceptually:

```rust
impl<A: Application, E: Environment<Event = A::Event, Command = A::Command>, W: std::io::Write>
    Engine<A, E, W>
{
    fn new(
        app: A,
        env: E,
        writer: W,
        config: EngineConfig,
    ) -> Result<Self, ConstructionError>;

    fn run(self) -> EngineExit<A::State, A::FatalReason, E::Error>;
}
```

Engine and Environment bounds use nonzero types, making zero invalid at construction. The Engine reserves the complete Command batch, record buffer, and Fatal-message buffer before it creates State or starts the Environment. Construction also proves that the maximally escaped Fatal message and its envelope fit within `max_record_bytes`. Checked layout or allocation failure returns `ConstructionError`; it is not a runtime Fatal and invokes no Application or Environment method.

Runtime startup is:

```text
create initial State exactly once
-> Environment::start
   on Error: Fatal(Environment::Start); Environment is safe to drop
-> write RunStarted with the accepted start LogicalTime
   on Error: Fatal
-> invoke on_start for turn index zero
-> process the ordinary turn result
```

Successful `RunStarted` flush accepts the start turn and establishes the current accepted index as zero. Before that point, there is no current accepted index. No External Event is requested before the start turn completes.

### 7.2 External Event

```text
verify accepted External Event count < max_turns
   on exhaustion: Core Fatal
-> Environment::next_event
   on Error: Fatal(Environment::NextEvent)
-> validate LogicalTime is not less than the previous accepted time
   on regression: Core Fatal
-> checked assignment of the next EventIndex
-> construct the EventEnvelope
-> write EventAccepted
-> invoke on_event exactly once
-> process the ordinary turn result
```

Failure before a successful `EventAccepted` flush invokes no handler. An Event returned by the Environment is a committed candidate and is consumed once even if time validation or Journal writing subsequently fails. Successful `EventAccepted` flush accepts the Event and establishes its index as the current accepted turn.

`max_turns` bounds accepted External Events, excluding the start turn. Exhaustion is Fatal: it prevents an unbounded Event/Command feedback loop, including a loop that advances EventIndex indefinitely at one LogicalTime.

### 7.3 Turn Result

After normal handler return:

```text
Context overflow marker set: Core Fatal; discard batch
-> Outcome::Fatal: Fatal(Application); discard batch
-> when Commands exist:
     write CommandsPrepared with complete ordered intent
     dispatch each Command once in order
       on Error at zero-based position k: Fatal(Environment::Dispatch(k));
         discard undispatched suffix
     write CommandsDispatched
-> Outcome::Continue:
     write TurnCompleted(Continue)
     begin next-Event acquisition; its bound check may fail before next_event
-> Outcome::Stop:
     write StopRequested
     Environment::shutdown(Graceful)
       on Error: Fatal(Environment::ShutdownGraceful)
     write TurnCompleted(Stop)
     return EngineExit::Stopped
```

The `CommandsPrepared` record plus the typed `EngineExit` cause identifies the exact successful prefix `[0, k)` after a partial batch failure. A dispatch Error means the Command at zero-based position `k` was not handed off. The Journal alone need not contain the position if Fatal recording fails.

Another Event is requested only after `TurnCompleted(Continue)` is flushed.

### 7.4 Fatal

The first failure observed by the Engine is the primary cause:

```rust
enum FatalCause<AF, EE> {
    Application(AF),
    Environment {
        error: EE,
        operation: EnvironmentOperation,
    },
    Journal(JournalFailure),
    Core(CoreFailure),
}

enum EnvironmentOperation {
    Start,
    NextEvent,
    Dispatch { position: usize },
    ShutdownGraceful,
}

enum JournalFailure {
    Encode {
        record: RecordKind,
        error: serde_json::Error,
    },
    RecordBoundExceeded {
        record: RecordKind,
    },
    Sink {
        record: RecordKind,
        operation: JournalOperation,
        error: std::io::Error,
    },
}

enum JournalOperation {
    Write,
    Flush,
}

enum CoreFailure {
    TimeRegression {
        previous: LogicalTime,
        offered: LogicalTime,
    },
    TurnBoundExceeded,
    CommandBoundExceeded,
}

enum EngineExit<S, AF, EE> {
    Stopped {
        state: S,
    },
    Fatal {
        state: S,
        cause: FatalCause<AF, EE>,
        shutdown_error: Option<EE>,
        journal_error: Option<JournalFailure>,
    },
}
```

`RecordKind` is the closed set of Record variants. `FatalKind` maps directly from the four `FatalCause` variants. Application, Environment, serializer, and Journal sink Errors remain typed in `EngineExit` for as long as the Engine owns them. The Fatal JSONL record uses that closed classification and a bounded `Display` rendering.

The Fatal record's `index` is `Some(i)` exactly when a successful `RunStarted` or `EventAccepted` flush has established current accepted turn `i`; it is `None` before the start turn is accepted. A consumed candidate Event whose `EventAccepted` write fails does not become current.

Fatal finalization is:

```text
stop normal execution
-> if the Environment started and has not shut down:
     Environment::shutdown(Abort)
     retain any Error as shutdown_error without replacing the primary cause
-> if the Journal is unpoisoned:
     render and write the Fatal record, best effort, at most once
     retain any Error as journal_error without replacing the primary cause
-> return EngineExit::Fatal with State, typed primary cause,
     shutdown_error, and journal_error
```

Finalization failure never replaces the primary cause. After Fatal, no handler, dispatch, or graceful action begins; only mandatory abort cleanup and the Fatal-record attempt may run. `EngineExit` returns the current State; State mutations, consumed candidate Events, dispatched Command prefixes, and successfully flushed Journal records remain real.

## 8. Bounds And Configuration

Conceptually:

```rust
struct EngineConfig {
    /// Accepted External Events per run. Exhaustion is Core Fatal.
    max_turns: NonZeroU64,

    /// Commands stored in one turn batch. Exhaustion is Core Fatal.
    max_commands_per_turn: NonZeroUsize,

    /// Encoded JSON-object bytes, excluding newline. Exhaustion is Fatal.
    max_record_bytes: NonZeroUsize,

    /// Formatted primary Fatal-cause bytes before JSON escaping.
    /// The escaped message plus the Fatal envelope must fit max_record_bytes.
    max_fatal_message_bytes: NonZeroUsize,
}

enum ConstructionError {
    CommandStorage(TryReserveError),
    JournalRecordStorage(TryReserveError),
    FatalMessageStorage(TryReserveError),
    /// The escaped Fatal message and envelope cannot fit max_record_bytes.
    FatalMessageTooLarge,
}
```

Bounds use nonzero types so that zero is unrepresentable. Checked storage layout and reservation occur before run-scoped activity and may return `ConstructionError`.

Each bound has one accounting owner and is checked before corrupting one item, record, or identity:

- The Engine owns `max_turns`, `max_commands_per_turn`, `max_record_bytes`, and `max_fatal_message_bytes`.
- The live Environment owns its Event-queue capacity, Port count, Command-inbox capacities, first-failure storage, thread count, logical-time domain, and shutdown-work bounds.
- The simulated Environment owns its Port count, wakeup-arm storage, equal-time cursor, logical-time domain, and `max_steps_per_event`.
- Each Port owns the bounds of its domain-state containers, native-client buffers, identifiers, counters, internal loops, and Port-local shutdown work.
- Every concrete bound documents its unit, check point, accounting owner, and exhaustion Error.
- Values with transitive owned memory remain governed by their owning Application, Port, Environment, or Journal sink.

All Kavod-managed containers and buffers are bounded. The Command batch never stores beyond `max_commands_per_turn`. The Journal encoding buffer never stores beyond `max_record_bytes`. Live queues never store beyond their Environment capacities, and simulated wakeup storage holds at most one arm per bound Port. `CommandsPrepared` serializes the complete turn batch into one record, so `max_record_bytes` must accommodate the largest batch the Application can stage under `max_commands_per_turn`; this sizing relation is a configuration obligation.

Kavod-owned active loops are bounded and nonrecursive. The run loop performs at most one start turn plus `max_turns` External turns; the dispatch loop is bounded by the current batch length; simulated internal progression is bounded by `max_steps_per_event`; and Environment polling, queue, and shutdown loops are bounded by Environment-owned iteration or work budgets. Blocking waits are not active loops and have no implied elapsed-time bound.

All capacity and identity arithmetic is checked before use. `max_turns` may equal `u64::MAX`; exhaustion is checked before attempting an index beyond the configured maximum, so EventIndex overflow is unreachable and remains an invariant assertion rather than an Engine outcome. Environment time conversion and other identities have the same checked-arithmetic obligation under their owning bounds.

Engine control flow advances only when the active Application, Environment, or Journal-writer call returns. Work delegated to user-defined handlers, Ports, serializers, formatters, destructors, Environment callbacks, and `Write` implementations is outside Kavod's active-loop bound; these implementations must be bounded and nonpanicking. Blocking is not a progress guarantee.

Panic on the Engine thread and process termination end the Engine semantic model. The Environment remains safe to drop after `start`; an Engine-owned cleanup guard invokes abort cleanup during unwinding. Catching an Engine panic and attempting to resume the run is unsupported. A panic confined to a supervised live Port thread is not an Engine-thread panic: the live Environment contains it at the Port boundary, latches a typed `PortPanicked` failure, wakes the Engine, and follows the ordinary Fatal path. Under `panic = "abort"` no cleanup guard or Port-panic containment runs; the design assumes unwinding.

## 9. Verification Obligations

Tests must establish:

- One handler invocation per flushed `EventAccepted` record.
- No overlapping turns or State access.
- Checked Event order, nondecreasing LogicalTime, and Core Fatal on regression.
- Deferred, ordered, bounded Command staging.
- Context overflow is Core Fatal, stores no out-of-bound Command, discards the batch, and takes precedence over the returned Outcome.
- Live dispatch commits exactly at bounded destination-inbox admission; queue-full, disconnect, and a previously latched Port failure commit nothing.
- Simulated dispatch invokes exactly one routed Port synchronously; the invocation commits handoff; a returned Port Error is latched as a post-handoff failure and is reported before the next Environment commitment.
- Simulated wakeups are revocable and Slot-scoped; Event commitment occurs only when `step` returns `Some`; equal-time wakeups use round-robin selection without starvation; `max_steps_per_event` is enforced before excess work; `SimQuiescent` is reported when no Port remains armed.
- Live `next_event` wakes for either Event availability or the first Port failure and never busy-spins; lifecycle signaling reaches Ports through Context and consumes no Event or Command inbox capacity.
- Live Port Error, panic, and premature closure latch a typed failure and wake the Environment without invoking the Application; Port panics never unwind the Engine thread.
- Dispatch stops at the first Error; `CommandsPrepared` and the typed Fatal dispatch position identify the exact successful prefix.
- No retry or rollback after an Event or Command commitment.
- `on_start` returning `Stop` or `Fatal` follows the ordinary turn and Fatal paths at index zero.
- The Environment call order of Section 5 is enforced; no handler, dispatch, or graceful action begins after Fatal.
- Every journal is well-formed JSON Lines; a non-finite float is an Encode failure.
- Fatal message truncation is deterministic, and a representable Fatal record always encodes under the construction check.
- Fan-in and fan-out wiring is exhaustive and type-checked, and per-Slot routing correctness is directly tested.
- Exact Journal sequences for startup, Continue, Stop, and every Fatal boundary.
- Journal encode, size, write, zero-progress, interrupted-write, and flush failures; sink failure poisons the Journal and permits no later explicit sink call; Fatal-record writing is best effort and never re-entered; primary and finalization Errors reach `EngineExit` typed.
- Graceful and Abort shutdown dispositions, graceful shutdown as a possible primary Fatal, and Abort failure as a typed secondary Error.
- Construction storage failure occurs before State creation and invokes no Application or Environment method.
- `max_turns`, `max_commands_per_turn`, `max_record_bytes`, `max_fatal_message_bytes`, and every Environment bound at, below, and above their boundaries.
- Repeated equal abstract traces under equal Engine configuration and capacities produce equal handler calls, State transitions, ordered Command intent, Journal protocol sequences, and `NormalizedExit` across live and simulated Environments. Repeated traces within one Environment type additionally produce equal Journal bytes and typed EngineExit values.
- Failure behavior on both sides of every irreversible operation.
- `ports!` expansion has the same Rust variants and serialized bytes as the corresponding hand-written Event and Command sums.

## Appendix A. Deltas From v8

| v8 mechanism | v9 replacement |
|---|---|
| `AuditEncode` and derive macro, `AuditBuffer`, framing, Audit sequence domain | serde `Serialize` and JSON Lines; newline is the frame and line order is the sequence |
| `AuditWriter`, pending store, terminal reserve, `FatalAudit`, synchronization policy table | `Journal<W: std::io::Write>`, flush per record, poison after sink failure, at most one best-effort Fatal record |
| Port protocol macro with generated Environment routing | `PortContract` remains descriptive; `macro_rules!` `ports!` expands only the paired sums; routing remains hand-written |
| `CoreEvent<E>` and `Ready` variant | Optional `on_start` hook for the index-zero start turn |
| Environment lifecycle state machine and separate `stop`/`abort` methods | Engine-enforced call order and one consuming `shutdown(mode)` with typed graceful or Abort disposition |
| `command_batch` with prefix result | Per-Command `dispatch`; exact prefix identified by `CommandsPrepared` plus typed failure position |
| Context latched staging Error | Infallible `emit` with bounded storage and one overflow marker |
| Event-index exhaustion Fatal and reserve-before-fetch | `max_turns` makes EventIndex overflow unreachable; checked assertion remains |
| Audit-specific construction and pending-byte bounds | Engine-owned turn, Command, and Journal-line bounds plus Environment-owned queue and work bounds |
