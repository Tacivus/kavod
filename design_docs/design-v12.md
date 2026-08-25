# Kavod Core Design

> **Status:** Authoritative (v12), amended through the 2026-08 adversarial review round. One section is open: Wiring & construction.
> The round's record is `design_docs/reveiws/`; its reviews cite the pre-round tree, tagged `v12-as-reviewed`. v13 is reserved for the Wiring close.
> **Scope:** The deterministic Core shared by live and simulated execution.
> **Priority:** The smallest robust design implementable in days, whose rules can be enforced and tested.

Kavod Core is written under `#![forbid(unsafe_code)]`.

## 0. Reading this document

This document stands alone. It defines Kavod by what Kavod does.

Four forms bind:

1. **API blocks** — item names, type shapes, trait bounds, and variant sets are exact,
   and a doc comment binds the behavior it states, its wording free. Listed derives are
   required; further derives are free unless a guarantee prohibits them. Receiver style
   is free except where a block shows a consuming receiver, which binds. A block a
   section marks provisional binds its semantics only, until the Wiring section closes.
2. **Guarantee rows** — every normative rule outside an API block or a binding table is
   a table row with an ID. A rule in none of the four forms does not exist.
3. **Binding tables** — the Environment contract's commitment table and the Run's
   construction, startup, phase, edge, and record tables: every row is a guarantee row,
   and each table is exhaustive over its scope — work it does not list does not happen.
   The phase and edge tables are the run's non-Fatal graph; Fatal finalization is
   `RUN-FINALIZE`'s alone. A binding table's rows are cited by the table's name.
4. **Obligation rows** — the rows of the Obligations table, each with an ID: trusted
   rules, upheld by the named party and checked by the stated means.

Everything else is prose, and prose has exactly four jobs: **define** a term, **derive** a
consequence from the rules, **justify** a rule so it is not relitigated, or illustrate
**Mechanism**. A definition binds vocabulary — the Glossary is its home — and creates no
obligation by itself. Mechanism illustrates one replaceable realization of the binding
rules. It creates no obligation and is never authority over an API block, guarantee row,
binding table, or obligation row. Test any sentence by deleting it: if an implementer
obligation changes, the sentence was a rule in the wrong clothes — give it an ID or move
it; if nothing changes and it does none of the four jobs, cut it.

Placement rules, for this document and every future edit:

- **The Run owns interaction.** If a fact can be tested against one component alone, it
  lives in that component's section. If it says when an operation is called, what its
  result means for the run, or what happens next, it lives in the Run.
- **Citations point backward.** Section order is dependency order; a fact that needs a
  forward reference is in the wrong section. Navigation pointers are exempt: the
  Glossary's citations, this section's own citations, the open-section notice, the bounds registry, the ownership map,
  the invariant index, a contract's pointer to its shipped implementations, trust marks
  pointing into the Obligations table, and `VERIFY-*` enforcement marks pointing into the
  Enforced verification table.
- **Cite IDs.** Never section numbers, here or in tests.
- **Implementation sections realize the contract.** A Live or Simulated guarantee
  either names the Environment-contract row it realizes or defines that
  implementation's Port-facing API; a
  fact any conforming Environment implementor would need lives in the contract. Core
  sections build only on the contracts and never name an implementation — earlier
  mentions of the two shipped Environments are navigation only (the Scope line, the
  contract's pointer to its implementations, the bounds registry).

Every ID outside the Obligations table is **enforced**: violation is unrepresentable,
panics an always-on assertion, or is pinned by a named test suite. Obligation-row IDs
are **trusted**: upheld by a named party, checked by the stated means. Contract rows
bind whoever implements the contract: Kavod enforces them
in the implementations it ships and the Run boundary-checks what it can observe; a
bespoke implementation's conformance is a trusted obligation (Obligations table).
An ID whose row requires a verification suite is enforced by that suite's presence as
a required test target and by its passing result.

Enforcement has an order: **unrepresentable beats asserted beats tested.** Where ownership
or a certificate can carry a rule, it must. The first available tier owns the rule's
enforcement.

| ID | Enforcement definition |
|---|---|
| `ASSERT-INVARIANTS` | The asserted tier consists only of always-on, constant-time assertions that panic on violation; a debug-only assertion is not enforcement. Every asserted invariant has an owning guarantee and a named assertion site. |
| `BOUND-LOOPS` | Every Kavod-owned active loop is nonrecursive and enforced by its owner and bound: the run by the index domain, dispatch by batch length, Environment work by its owned budgets, Journal writing by record length. A blocking wait is not an active loop and implies no elapsed-time bound; work inside user code is trusted to be bounded (`TRUST-BLOCKING`). |

## 1. Glossary

One line per term. These definitions are normative; a *Define:* note elsewhere binds
the same way.

- **Application** — the user's pure transition logic: two handlers plus an initial State.
- **Handler** — `on_start` or `on_event`; runs once per turn.
- **Answer** — the Outcome a handler returns.
- **State** — all run-varying application data, owned by the Application.
- **Event** — one unit of input the Environment delivers to the run.
- **External Event** — an accepted Event delivered by `next_event`, as opposed to the
  start turn; External Events carry indices from 1.
- **Command** — one unit of intent a handler stages for the Environment to deliver.
- **Externally consequential** — of a Command's delivery or any other work: it causes
  an effect outside the process.
- **Turn** — one accepted Event (or the start), one handler call, one batch: the run's
  unit of progress.
- **Batch** — the ordered Commands one turn stages.
- **Candidate** — an Event returned by `next_event`: consumed, not yet accepted.
- **Accepted** — of a turn: its acceptance record committed — `RunStarted` for the start
  turn, `EventAccepted` for a candidate becoming one External Event. Only acceptance gives
  a turn its index and logical time.
- **Contract** — one Event protocol paired with one Command protocol.
- **Slot** — one named use of a Contract.
- **Port** — one Environment's implementation of one bound Slot.
- **Environment** — the Core's one boundary to the outside: waiting, Event selection,
  time, Command routing, lifecycle.
- **Journal** — the bounded JSON Lines writer the run's records pass through.
- **Sink** — the `std::io::Write` value the Journal writes into.
- **Record** — one flat JSON object evidencing one protocol step.
- **Commit** — encode, write, flush; only a successful flush commits.
- **Poisoned** — the Journal's permanent state after a sink failure; it accepts no
  further commits.
- **Error** — a typed value a component reports when an operation fails.
- **Fatal** — the run-level classification of the Error or Core condition that ended
  the run.
- **fail / failure** — plain English for "returned an Error"; no further meaning.
- **Sink failure** — a write or flush outcome of: a returned Error (`Interrupted`
  included), zero progress (`Ok(0)`), or an over-reported byte count.
- **Capability** — an owner-supplied value through which another component acts on the
  owner's fact, in exactly the ways the owner defines.
- **Commitment point** — the instant an operation's outcome becomes fixed. Before it,
  the operation's contractual effect has not occurred — subordinate effects its owner
  names may have, and they stand; after it, nothing is retried, revoked, or
  rolled back.
- **Handoff** — `dispatch`'s commitment: transfer of one Command value into its
  destination Port's exclusive inlet.
- **Admission** — entry of a value into a Kavod-owned queue or inbox; this describes
  the value's placement, not ownership of the container.
- **Publication** — the act of offering an Error to the latch; entry succeeds only per
  `ENV-LATCH`.
- **Latch** — the Environment's store for the first Error its own activity publishes
  (typically a Port). States: empty, pending, reported, closed — the close returns a
  pending Error through the report.
- **Run-scoped activity** — everything an Environment must end before the run is over:
  its own threads, timers, and callbacks, and whatever its Ports started
  (`TRUST-SPAWN`).
- **Quiescence** — the Environment's account of whether all run-scoped activity
  finished: `Quiesced` means it accounts every unit complete; `Incomplete` means it
  does not. Completion the Environment cannot itself witness relies on `TRUST-SPAWN`;
  a bespoke Environment's account also relies on `TRUST-ENV`.
- **ShutdownReport** — `shutdown`'s returned value: the run's Quiescence plus the
  Error the latch held at its close, if any.
- **Clean report** — a ShutdownReport of `Quiesced` carrying no Error.
- **Shutdown signal** — the Environment-delivered notice: no more input is coming;
  finish what you own and return.
- **Finite-source pattern** — a source that runs out of input offers one
  application-defined terminal Event and awaits the shutdown signal; the terminal Event's
  handler answers `Stop`. Ending a run is Application logic, expressed in the Event
  protocol like everything else.
- **Trace** — the run's full operation-result history: every Environment operation's
  returned value — Ok payloads and the ShutdownReport included — and every sink call's
  result (one write or flush call: a write's Ok count or a flush's success, or the
  failure's presence), with
  Error values erased and their presence and position kept. The accepted
  `(Event, Timestamp)` sequence is the trace's `next_event` successes — every one
  except possibly the last, which a Fatal can leave consumed but unaccepted.
- **Phase, edge, certificate** — the run's position in its graph, the transitions
  between positions, and the value whose possession proves the position.

## 2. Laws

Everything in this document is a consequence of nine axioms.

| # | Axiom | Statement |
|---|---|---|
| A1 | Single authority | Every fact has exactly one owner; every appearance outside its owner is a read-only view of it or an owner-supplied capability, and the owner defines every way the fact can change. |
| A2 | Serial turns | One Event, one handler call, one batch at a time; a turn completes, or the run goes Fatal, before the next Event is requested. A destination Port's processing of Commands already handed off runs outside the turn. |
| A3 | One commitment point | Every effectful operation commits at exactly one point, where its outcome becomes fixed. Work the rules give no commitment point — State mutation, a staged batch before its handoff — is its owner's private staging, standing or discarded by that owner's own rules. |
| A4 | First failure wins | The first Error or fatal Core condition the run observes is the Fatal cause; nothing observed later replaces it. Once an operation's failure outcome is fixed, that operation's remaining work is best-effort cleanup whose Errors are discarded. Once the Fatal cause is fixed, all later run work is likewise best-effort cleanup; on a run that ends without a Fatal cause, Environment- and Port-side cleanup instead begins when the latch closes. |
| A5 | Intent precedes effect | Where a record announces an action, it commits before the action begins; a completion record witnesses effects already committed. |
| A6 | Bounded everything | Every Kavod-owned container, count, identifier, and active loop has one accounting owner and a bound checked before use. Arithmetic on counts, capacities, times, and identities is checked. |
| A7 | Typed inside, rendered at the edge | Errors stay typed values while Kavod owns them. Text and bytes exist only at the serialization boundary. |
| A8 | Panics are bugs | A failing user component reports a typed Error. A panic — in Kavod or user code — is a bug: under the shipped profile the process aborts, and no exit represents it (`TRUST-ABORT`). |
| A9 | Determinism | The Core introduces no choice of its own: under `TRUST-PURE` and `TRUST-SERIALIZE`, every Core-owned run output (A1) is a function of the build, the Application, its initial State, the configuration, and the trace. |

**Failure.** A4's cleanup rule means Fatal performs no rollback: every effect that
reached its commitment point stays real. Consequences of this appear once, at each
effect's owner, and are derivable everywhere else.

**Panics.** Under `TRUST-ABORT`, Kavod ships with `panic = "abort"` and relies on
unwinding nowhere in shipped code; test code may catch panics under the test profile,
which unwinds. After a panic the evidence is the Journal's committed records, kept
current by flush-per-record commits.

**Guarantees**

| ID | Guarantee |
|---|---|
| `NO-UNSAFE` | Kavod Core compiles under `#![forbid(unsafe_code)]`. |
| `BOUND-STATIC` | Construction fixes the nonempty Port set and one Slot order; both remain unchanged for the Environment's lifetime. |
| `BOUND-NONZERO` | Every configured capacity uses a nonzero type, so zero is unrepresentable. |

**Bounds registry** (navigation; each bound's rules live with its owner):

| Bound | Owner |
|---|---|
| Command batch capacity (`max_commands_per_turn`) | The Run |
| Record bytes (`max_record_bytes`) | Journal |
| Index domain (`u64`) | The Run |
| Event queue, per-Port Command inboxes, completion-state entries and wakeups, shutdown deadline, time domain | Live Environment |
| Wakeup arms (one per Port), step budget per `next_event` call | Simulated Environment |

**Ownership map** (navigation):

| Component | Owns |
|---|---|
| Application | Pure transition logic; all run-varying data, inside State. |
| Port | All of its own domain, protocol, and native state. |
| Environment | Topology, waiting, Event selection, time stamping, routing, lifecycle. |
| Journal | The write mechanism: bounded encoding, one sink, poison. |
| The Run | The graph, the records, the certificate (index and time), Fatal classification. |

## 3. Application contract

The Application is a pure transition function over its State. Handlers are
user-implemented; Kavod owns the index and time types and `Context`.

### API

```rust
/// Both serialize as transparent u64 JSON values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct EventIndex(/* u64, private */);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Timestamp(/* u64 nanoseconds, private */);

impl EventIndex {
    /// The accepted turn's ordinal: 0 for the start turn, External Events from 1.
    pub fn as_u64(self) -> u64;
}

impl Timestamp {
    /// Builds a timestamp from a nanosecond count. The count's origin and
    /// meaning belong to the stamping Environment.
    pub fn from_nanos(nanos: u64) -> Self;
    /// The timestamp advanced by `elapsed`, or `None` if the duration or the
    /// sum overflows u64 nanoseconds (A6).
    pub fn checked_add(self, elapsed: std::time::Duration) -> Option<Self>;
    pub fn as_nanos(self) -> u64;
}

pub trait Application {
    type State;
    type Event: Serialize;
    type Command: Serialize;
    type Error;

    fn initial_state(&self) -> Self::State;

    fn on_start(
        &self,
        state: &mut Self::State,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Error>;

    fn on_event(
        &self,
        state: &mut Self::State,
        event: &Self::Event,
        ctx: &mut Context<'_, Self::Command>,
    ) -> Outcome<Self::Error>;
}

pub enum Outcome<E> {
    Continue,
    Stop,
    Fatal(E),
}

pub struct Context<'a, C> {
    /* batch buffer, overflow marker, index, logical time — private */
}

impl<'a, C> Context<'a, C> {
    /// The current accepted turn.
    pub fn index(&self) -> EventIndex;
    /// The current turn's accepted logical time (the start time at index 0).
    pub fn logical_time(&self) -> Timestamp;
    /// Exact Commands the batch can still store; zero once the overflow
    /// marker is set.
    pub fn remaining(&self) -> usize;
    /// Infallible; transfers one immutable Command.
    pub fn emit(&mut self, command: C);
}
```

### Guarantees

| ID | Guarantee |
|---|---|
| `APP-CONTEXT` | During a handler, `Context` reports its turn's index and logical time, and is the only capability Kavod supplies a handler (A1). |
| `APP-EMIT` | `emit` is infallible and transfers one immutable Command; while capacity remains it appends in call order. |
| `APP-OVERFLOW` | The first over-bound `emit` stores nothing and sets an overflow marker; every later `emit` stores nothing. A fresh handler invocation starts with the buffer empty and the marker clear. |
| `APP-FUTURE` | Work for a future turn returns through an External Event; `Context` offers no other channel. |
| `APP-STATE` | State mutation has no commitment point: whatever a handler wrote stands on every exit, Fatal included — a discarded batch (A4) rolls back no State. |

### Mechanism

`Context` wraps one fixed-capacity Command buffer of `max_commands_per_turn` entries,
allocated once at construction and reused every turn — cleared at handler entry
(`APP-OVERFLOW`) — plus the overflow marker. The
buffer never grows, so the turn loop does not allocate. `remaining()` is capacity minus
length, or zero when the marker is set.

| Step | `emit` |
|---|---|
| 1 | Marker set → store nothing (`APP-OVERFLOW`). |
| 2 | Buffer full → set the marker, store nothing. |
| 3 | Otherwise append in call order (`APP-EMIT`). |

### Notes

*Derive:* a handler cannot observe or influence anything outside State, its Event, and
Context — the signatures admit nothing else. Determinism then rests on State and the
payload types carrying no hidden authority, which is the Application author's trusted
obligation (Obligations table).

*Derive:* `Timestamp::checked_add` lets Port code compute a later time from an
Environment-supplied one; `from_nanos` lets an Environment implementation outside the
crate mint the timestamps it stamps.

*Define:* Port-domain timestamps — exchange time, receive time — are ordinary Event
payload fields with no Core meaning. Equal logical times are valid; index order breaks
ties.

*Justify:* `initial_state` is infallible by design: State is pure data, and anything
fallible needed to build it happens before engine construction, while constructing the
Application value itself. Its conduct is covered by `TRUST-BLOCKING`.

## 4. Port contract

A Contract pairs one Event protocol with one Command protocol. A Port is one
Environment-specific implementation of one bound Slot.

### API

```rust
pub trait PortContract {
    type Event: Serialize;
    type Command: Serialize;
}

/// Kavod-owned uninhabited type for absent directions; implements `Serialize`.
pub enum Never {}

kavod::ports!(
    pub enum Trading<Event = TradingEvent, Command = TradingCommand> {
        Primary(MarketData),
        Secondary(MarketData),
        Execution(Execution),
        Timer(Timer),
    }
);
```

### Guarantees

| ID | Guarantee |
|---|---|
| `PORT-STATE` | A Port exclusively owns its mutable domain, protocol, and native state; wiring and the Environment relay its values, routing by the Slot sum's discriminant alone and never reading the payload (`TRUST-ROUTING`; `TRUST-ENV` for a bespoke Environment). Processing after a Command's handoff belongs to the destination Port. |
| `PORT-SUMS` | The Slot-qualified Event and Command sums are closed and type-checked against their Contracts at the wiring: applying the frozen fan-in constructors and the fan-out match proves payload agreement for every variant. Distinct Slots of one Contract are distinct variants; that a hand-written sum's variants are exactly the bound Slots rides `PORT-ROUTING`'s trusted obligation. |
| `PORT-ROUTING` | Fan-in is one frozen variant constructor per inhabited Event direction; fan-out is one hand-written exhaustive destination match. The compiler proves exhaustiveness and payload agreement; each arm naming its semantically correct Slot is trusted (`TRUST-ROUTING`). Each Environment's Error sum carries one mapped variant per Slot's Port Error, at that Environment's own mapping site, placed finally when Wiring closes. |

### Mechanism

`ports!` is a `macro_rules!` macro. Its complete expansion for the example above:

```rust
#[derive(::serde::Serialize)]
pub enum TradingEvent {
    Primary(<MarketData as kavod::PortContract>::Event),
    Secondary(<MarketData as kavod::PortContract>::Event),
    Execution(<Execution as kavod::PortContract>::Event),
    Timer(<Timer as kavod::PortContract>::Event),
}

#[derive(::serde::Serialize)]
pub enum TradingCommand {
    Primary(<MarketData as kavod::PortContract>::Command),
    Secondary(<MarketData as kavod::PortContract>::Command),
    Execution(<Execution as kavod::PortContract>::Command),
    Timer(<Timer as kavod::PortContract>::Command),
}
```

That is the whole expansion: two enums, serde's default externally tagged
representation. The `Event =` and `Command =` idents are the generated enums' names,
exactly as the invocation writes them — `macro_rules!` concatenates no identifiers. The
invocation's `Trading` is documentation only: the expansion creates no item named
`Trading`. Hand-written equivalents are supported — same item names, variants, payload
types, and serialized bytes, so every rule reading the sums applies unchanged — and may
add derives freely. Generated derives use `::serde` paths, so consumers need a
direct dependency named `serde`. `Never`'s `Serialize` implementation is `match *self {}`,
and a `Never` arm is discharged by matching the uninhabited value.

### Notes

*Define:* every Contract is duplex; an absent direction uses `Never`.

*Justify:* the expansion above is exhaustive, so the two enums are inspectable by eye
and replaceable by hand — the macro is sugar, never authority.

## 5. Environment contract

The Environment is the Core's one boundary to the outside. This section is the
complete contract: an implementation satisfying every row here, under `ENV-SERIAL`'s
call pattern, is a conforming Environment. The Live and Simulated sections are the two
implementations Kavod ships; each adds only its own Port-facing API.

### API

```rust
pub trait Environment {
    type Event;
    type Command;
    type Error;

    fn start(&mut self) -> Result<Timestamp, Self::Error>;
    fn next_event(&mut self) -> Result<(Self::Event, Timestamp), Self::Error>;
    fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error>;
    /// Takes the first currently latched Error without waiting for one.
    fn take_error(&mut self) -> Option<Self::Error>;
    /// Raises the shutdown signal and closes Event admission as its one
    /// initiating step. The latch remains open through the Environment's bounded
    /// graceful-shutdown window; the final observation fixes the report and
    /// closes the latch.
    fn shutdown(self) -> ShutdownReport<Self::Error>;
}

pub struct ShutdownReport<E> {
    pub quiescence: Quiescence,
    /// The pending Error the latch held when it closed; `None` proves the
    /// latch was empty or already reported at the close.
    pub error: Option<E>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Quiescence {
    Quiesced,
    Incomplete,
}
```

### Commitment points

A3 applies on both sides of each row. The table binds outcomes, not instants: where a
commitment sits inside an implementation is that implementation's business — each names
its own (`ENV-ERRORS`) — and the returned value is the caller's only witness of it.

| Operation | Commitment point | `Err` means | Success means |
|---|---|---|---|
| `start` | Activation: run-scoped activity becomes live, after the start time is frozen; each implementation names the instant (`ENV-ERRORS`). | `ENV-START` holds; subordinate effects the implementation names may have occurred, and they stand. | Activation is committed irrevocably and the frozen start time returned. A Port failing afterward is a runtime failure, surfacing per `ENV-LATCH`. |
| `next_event` | Consumption of one candidate — each implementation names the instant (`ENV-ERRORS`); the call may wait for one, the only operation that waits for input. | No candidate was consumed; subordinate effects the implementation names may have occurred, and they stand. | Exactly one candidate is consumed, for good — never retried, revoked, or re-offered. |
| `dispatch` | Handoff of this one Command; the attempt never waits for future capacity. | This Command was not handed off. | Handoff stands; the destination Port owns all further processing (`PORT-STATE`). |
| `take_error` | One atomic snapshot of the latch. | — | `Some(error)` reports the pending first Error and marks the latch reported forever; `None` proves only that nothing was pending at the snapshot. The call never waits. |
| `shutdown` | Invocation: consuming the Environment makes shutdown irrevocable. The report's contents fix later within the call, at the final graceful-shutdown observation that closes the latch. | — | The report's quiescence: `Quiesced` means the Environment accounts every unit of run-scoped activity complete; completion it cannot itself witness relies on `TRUST-SPAWN`, and a bespoke Environment's account relies on `TRUST-ENV`. `Incomplete` means at least one unit remained unaccounted when the bounded wait ended. The report's Error is the latch's pending Error at the close (`ENV-LATCH`). |

### Guarantees

| ID | Guarantee |
|---|---|
| `ENV-SERIAL` | The contract assumes one serial caller: `start` at most once, and first if at all; then `next_event`, `dispatch`, and `take_error` one at a time; `shutdown` at most once, consuming the Environment. After `start` returns `Err` there is no later call: the Environment is quiesced (`ENV-START`) and safe to drop. After any other operation returns `Err`, or `take_error` returns `Some`, the only later call is `shutdown`. Implementations need no synchronization against concurrent contract calls. |
| `ENV-START` | When `start` returns `Err`, the Environment is quiesced and safe to drop: every Port either received no call, or its final call finished before the return and it will receive no further call. |
| `ENV-ERRORS` | A failure before an operation's commitment point returns as that operation's own `Err`. A failure after it must be published (`ENV-LATCH`), and the operation's success return stands. Each implementation names, in a guarantee row of its own section, the instants of its `start` activation and its `next_event` consumption. |
| `ENV-LATCH` | The latch holds at most the first published Error. States: empty → pending on the first publication; pending → reported when `take_error` returns it or an operation returns it as its `Err`; empty, pending, or reported → closed when shutdown's close runs — a pending Error at the close leaves through the report, the run's final latch observation. Every publication after the first, and every publication after the close, is discarded. The Environment chooses a logical order between each publication and `next_event` or `dispatch`'s commitment, `take_error`'s snapshot, or the close. For a call that reaches one of those observation points, a publication completed before the call began orders before the point, one begun after the call returned orders after the point, and one overlapping the call may order on either side. For the close, the anchors are the final observation that closes the latch, not the `shutdown` call: a publication completed before that observation begins orders before the close, and only one overlapping the observation may order on either side. An `Ok` return, `take_error`'s return, or the shutdown report witnesses that placement. A pending Error ordered before the point leaves the latch through the operation's result as fixed by the **Commitment points** table; one ordered after leaves the operation's result standing and stays pending. A call that would otherwise fail before commitment resolves any pending publication against the instant its own failure would fix, with the same anchors: a publication completed before the call began orders before that instant, and only one overlapping the call may order on either side of it. A pending Error ordered before that instant is returned and reported in preference to the operation's own Error; the operation's contractual effect did not occur, and its own Error is secondary and discarded. If the operation's own Error fixes first, it is returned and a publication ordered after it stays pending. A `next_event` call waiting for input returns and reports the Error that makes the latch pending. |
| `ENV-TIME` | One Environment authority — the single Event acceptor — stamps `Timestamp` on `start` and every `next_event`, and owns the count's origin and meaning. Stamped times never decrease across the run; equal stamps are valid. |
| `ENV-SHUTDOWN` | `shutdown` begins by raising the shutdown signal and closing Event admission — one initiating step whose internal order is the implementation's own. From the signal's initiating instant every Port not already ended has a means to observe it immediately (an already-ended Port's residue is `TRUST-SPAWN`'s), regardless of Commands already handed off but not yet processed; how an implementation orders the signal against those Commands is its Port-facing API. That instant begins the graceful-shutdown window. Throughout the window the latch remains open while the Environment performs its shutdown work and waits according to its run-scoped-activity accounting. This bounded quiescence policy applies only to waiting for activity still accounted outstanding, not to reclaiming activity already accounted complete; such reclamation remains subject to `TRUST-BLOCKING`. Already-handed-off residue is the destination Port's to drain or abandon (`PORT-STATE`), and the Environment itself initiates no further externally consequential work after raising the signal — shipped conduct upheld under `TRUST-SHUTDOWN`, bespoke under `TRUST-ENV`. When every unit is accounted complete or the wait bound expires, one final observation fixes quiescence and closes the latch into the report (`ENV-LATCH`). A publication ordered before that close follows the latch's ordinary first-wins rules; one ordered after it is discarded. |
| `ENV-SEPARATION` | The Environment orchestrates Ports and only that: Port domain state belongs to Ports (`PORT-STATE`), and handler invocation belongs to the Run. |
| `ENV-BOUNDS` | Every operation preserves the Environment's own declared bounds — the registry's rows for the shipped implementations; a bespoke implementation declares its own. `VERIFY-LIVE` and `VERIFY-SIM` pin the shipped bounds; a bespoke implementation upholds its declared bounds under `TRUST-ENV`. |

### Notes

*Define:* the shutdown signal carries exactly its glossary meaning — no more input is
coming; finish what you own and return. Disposition of pending work is Port authorship.

*Derive:* while the graceful-shutdown window is open, shutdown-work publications
follow `ENV-LATCH`'s ordinary first-wins state, and any Error pending at the final
observation leaves through the report. A publication after the close is discarded by
`ENV-LATCH` and remains cleanup under A4.

## 6. Journal

The Journal is a policy-free bounded JSON Lines writer: it encodes one value, writes
one line, flushes. The record schema belongs to the Run. The Journal's output is
human-readable forensic evidence, guaranteed exactly through its last committed record.

### API

```rust
pub struct Journal<W: std::io::Write> {
    /* writer, one reusable bounded encode buffer, poison marker */
}

impl<W: std::io::Write> Journal<W> {
    /// Reserves the encode buffer up front.
    pub fn new(writer: W, max_record_bytes: NonZeroUsize)
        -> Result<Self, JournalBuildError>;
    /// Encode into bounded storage, write one line, flush.
    /// Precondition: not poisoned.
    pub fn commit<R: Serialize>(&mut self, record: &R) -> Result<(), JournalError>;
    pub fn is_poisoned(&self) -> bool;
}

pub enum JournalBuildError {
    /// `max_record_bytes` leaves no room for the reserved newline byte.
    MaxBytesTooLarge,
    /// The reusable record buffer could not reserve its storage.
    AllocationFailed(std::collections::TryReserveError),
}

pub enum JournalError {
    Encode(serde_json::Error),
    /// The payload serialized to something other than one single-line JSON object.
    NotAnObject,
    BoundExceeded,
    Sink { operation: SinkOperation, error: std::io::Error },
}

#[derive(Debug, PartialEq, Eq)]
pub enum SinkOperation { Write, Flush }
```

### Guarantees

| ID | Guarantee |
|---|---|
| `JRN-FORMAT` | One record is one single-line serde JSON object plus one newline; line order is the sequence. `max_record_bytes` bounds the encoded object of every committed record; the newline is stored beyond it. |
| `JRN-ENCODE` | Encoding completes in the reusable bounded buffer before any byte of that record reaches the sink. The encode region is exactly `max_record_bytes + 1` bytes; construction computes that size with `max_record_bytes.checked_add(1)`, whose overflow is `MaxBytesTooLarge`. The encoded bytes are classified as one single-line JSON object exactly by starting with `{`, ending with `}`, and containing no newline byte; any other result is `NotAnObject`. A completed non-object of `max_record_bytes + 1` bytes is therefore `NotAnObject`; an object of that size leaves no room for the newline and is `BoundExceeded`. After that classification the newline is appended; no room for it is `BoundExceeded`. `Encode`, `NotAnObject`, and `BoundExceeded` write nothing and poison nothing. The bounded buffer's zero-progress rejection is `BoundExceeded`; every other encode failure is `Encode`. |
| `JRN-COMMIT` | Only a successful flush commits a record. Bytes past the last committed record are an uncertain suffix, even if they form complete lines — after a sink failure, and equally after any end of the process that arrives before a flush, an abort included. |
| `JRN-POISON` | Writing uses a loop bounded by record length and retries only a short successful write. Any sink failure permanently poisons the Journal, mapped to its typed Error: a write or flush Error as returned, zero progress as `WriteZero`, an over-reported count as `InvalidData`; `Interrupted` is never retried. A poisoned Journal performs no further sink operation; `commit` on it is a precondition violation and panics (A8). |
| `JRN-SINK` | `W: std::io::Write` is the whole persistence abstraction. A sink is fresh for one run or positioned immediately after a newline, exclusively owned by the Journal, and stores exactly the bytes given — the sink owner's obligation (`TRUST-SINK`). The contract ends at successful flush; durability beyond it, and writer destructor behavior, belong to the sink's owner. |

### Mechanism

The reusable bounded buffer implements `std::io::Write`, so `serde_json::to_writer`
encodes directly into it.

The following table is a nonbinding realization of `JRN-ENCODE`, `JRN-COMMIT`, and
`JRN-POISON`; those guarantee rows are the authority.

| Step | `commit` |
|---|---|
| 1 | Poisoned → invariant panic (A8). |
| 2 | Clear the buffer; encode into it. The buffer's zero-progress `WriteZero` → `BoundExceeded`; other serde failures → `Encode`. Nothing written, nothing poisoned (`JRN-ENCODE`). |
| 3 | Encoded bytes must start with `{`, end with `}`, and contain no newline byte — otherwise `NotAnObject`. Nothing written, nothing poisoned. |
| 4 | Append the newline; a record that left no room for it → `BoundExceeded`. |
| 5 | Write the buffer with a loop bounded by record length that retries only short successful writes: `Err` (including `Interrupted`), `Ok(0)` as `WriteZero`, and an over-reported count as `InvalidData` each poison and return `Sink { operation: Write, .. }` (`JRN-POISON`). |
| 6 | Flush. Failure → poison, `Sink { operation: Flush, .. }`. Success commits (`JRN-COMMIT`). |

A failed reservation is `AllocationFailed`, carrying the reservation Error.

### Notes

*Derive:* encode requirements fall on payload authors as trusted obligations
(Obligations table): deterministic, side-effect-free, bounded, nonpanicking `Serialize`
with stable map order. Map keys that cannot be JSON strings surface as `Encode`.
Non-finite floats follow `serde_json`. Lossy serialization is evidence only of the
fields it emits.

*Derive:* a named-field struct payload serializes as a JSON object, and `serde_json`
writes every newline inside an ordinary value escaped, so a caller committing
named-field structs of ordinary values can treat `NotAnObject` as unreachable. A newtype
serializes as its inner value — an object exactly when that value is one; tuple and unit
structs never are. Raw-passthrough and hand-written `Serialize` values can produce a
non-object or an interior newline; the variant serves them and direct Journal consumers
with arbitrary payloads.

*Derive:* memory sinks (a shared `Vec<u8>` handle) make tests and fault injection
direct. Because JSONL bytes alone cannot mark the committed boundary after a sink
failure, replay needs a cleanly completed Journal or an externally trusted boundary.

*Justify:* `Interrupted` poisons instead of retrying because the write loop's bound is
progress against record length and an interrupted write made none: retrying it would put
an unbounded spin inside a bounded loop (`BOUND-LOOPS`). A sink wanting `Interrupted`
retried wraps its writer.

## 7. The Run

The Run composes the contracts: one Engine drives one Application against one
Environment, evidencing every step through one Journal. Its shape is a graph. Phases
carry the work; edges carry the records; a transition *is* a commitment point — where
the edge carries a record, the next phase is unreachable until it commits. This is A5
and A3 closed over the whole run, and the Engine enforces it at compile time
(`RUN-GRAMMAR`).

### API

```rust
pub struct EngineConfig {
    pub max_commands_per_turn: NonZeroUsize,
    pub max_record_bytes: NonZeroUsize,
}

pub struct Engine<A, E, W> {
    /* private */
}

pub enum BuildError {
    CommandBuffer(std::collections::TryReserveError),
    Journal(JournalBuildError),
}

impl<A, E, W> Engine<A, E, W>
where
    A: Application,
    E: Environment<Event = A::Event, Command = A::Command>,
    W: std::io::Write,
{
    pub fn new(config: EngineConfig, app: A, env: E, writer: W)
        -> Result<Self, BuildError>;
    pub fn run(self) -> EngineExit<A::State, A::Error, E::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum RecordKind {
    RunStarted,
    EventAccepted,
    CommandsPrepared,
    CommandsDispatched,
    StopRequested,
    TurnCompleted,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub enum TurnOutcome {
    Continue,
    Stop,
}

pub enum EngineExit<S, AE, EE> {
    Stopped { state: S },
    Fatal {
        state: S,
        cause: FatalCause<AE, EE>,
        quiescence: Quiescence,
    },
}

pub enum FatalCause<AE, EE> {
    Application(AE),
    Environment(EnvironmentFatal<EE>),
    Journal(JournalFatal),
    Core(CoreError),
}

/// Names the operation where the Error was observed — not necessarily where it was
/// caused (`ENV-LATCH`).
pub struct EnvironmentFatal<EE> {
    pub error: EE,
    pub operation: EnvironmentOperation,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnvironmentOperation {
    Start,
    /// Where observed — possibly an unrelated already-latched Error, per
    /// ENV-LATCH.
    NextEvent,
    /// Where in the dispatch loop the Error was observed — possibly an
    /// unrelated already-latched Error, per ENV-LATCH.
    Dispatch { position: usize },
    /// The per-turn latch snapshot (RUN-CHECKPOINT) returned a pending Error.
    Checkpoint,
    /// The Stop-path shutdown report carried the latch's final pending Error.
    Shutdown,
}

pub struct JournalFatal {
    /// The kind of the record whose commit failed.
    pub record_kind: RecordKind,
    /// `Some` with the attempted outcome exactly when `record_kind` is
    /// `TurnCompleted`; `None` otherwise.
    pub outcome: Option<TurnOutcome>,
    pub error: JournalError,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CoreError {
    TimeRegression { previous: Timestamp, offered: Timestamp },
    IndexExhausted,
    CommandBoundExceeded,
    ShutdownIncomplete,
}
```

### Construction and startup

`Engine::new` runs before State creation and invokes no Application or Environment
method; failure is `BuildError`, and no run happened.

| Step | `Engine::new` | On failure |
|---|---|---|
| 1 | Reserve the complete Command batch for `max_commands_per_turn` with `try_reserve`. | `CommandBuffer` |
| 2 | Build the Journal from `max_record_bytes`. | `Journal` |

| Step | `run` startup | On failure |
|---|---|---|
| 1 | Create initial State, exactly once, before any fallible step — so every exit carries State. | A panic is a bug (A8). |
| 2 | `Environment::start`. | `Environment(Start)` Fatal with `Quiescence::Quiesced` — `ENV-START` already holds, so finalization skips `shutdown`. |
| 3 | Mint the certificate at `Initial`, consuming the Journal and the frozen start time. Its stored index is 0 and its stored time is the frozen start time; both are prospective values for the only outgoing edge, not accepted run state, and neither is available to `Context` until `RunStarted` commits. | — |
| 4 | Take the `RunStarted` edge, which reads its index and logical time from the certificate; the start turn proceeds per the graph at index 0. | Journal Fatal. |

### The graph

Non-normative sketch; the two tables below are the guarantee.

```
Initial ──RunStarted──▶ TurnOpen ──CommandsPrepared──▶ Prepared
                         ▲    │                            │
           EventAccepted │    │(empty batch)       CommandsDispatched
                         │    ▼                            ▼
                         │   EffectsComplete ◀─────────────┘
                         │         │(checkpoint)
                         │         ▼
                         │    Checkpointed ──StopRequested──▶ StopPending
                         │         │                              │
                         │  TurnCompleted(Continue)      TurnCompleted(Stop)
                         │         │                              │
                         └── BetweenTurns                         ▼
                                                               Closed

any failure: drop the certificate ──▶ RUN-FINALIZE
```

**Phases** — work in the listed order; each failure row names its `FatalCause`.

| Phase | Work, in order |
|---|---|
| `Initial` | None; startup takes the only edge out. |
| `TurnOpen` | Invoke the handler once with `Context` over the batch buffer — `on_start` at index 0, `on_event` otherwise, one turn protocol (A2). Then: overflow marker set → discard the batch → `Core(CommandBoundExceeded)`, beating every `Outcome` — the Core condition outranks the returned Outcome, and a returned `Fatal` payload is discarded with the batch (A4's cleanup rule). `Outcome::Fatal(error)` → discard the batch → `Application(error)`. Otherwise remember the answer and leave: empty batch by the recordless edge, nonempty by `CommandsPrepared`. |
| `Prepared` | Hand off each Command once, in order. `Err` at position k → `Environment(Dispatch { position: k })`: the prefix `[0, k)` stands handed off, the Command at k was not handed off — though the `Err` may be an unrelated already-latched Error ordered there by `ENV-LATCH`, not a rejection of that Command — and the suffix is discarded. |
| `EffectsComplete` | The checkpoint (`RUN-CHECKPOINT`): the latch snapshot. `Some(error)` → `Environment(Checkpoint)`. `None` takes the recordless edge, the remembered answer fixed in the phase. |
| `Checkpointed` | None; the fixed answer picks which of its two edges is available. |
| `BetweenTurns` | Take the `EventAccepted` edge. Its transition performs the index-domain check (`RUN-INDEX`): certificate index equals `u64::MAX` → `Core(IndexExhausted)`, `next_event` uncalled. Then it calls `next_event`; `Err` → `Environment(NextEvent)`. The successful return is the candidate the transition checks and records. |
| `StopPending` | `shutdown` — it consumes the Environment — then retain the report's quiescence for every later Fatal path before inspecting its Error. The retained quiescence survives the subsequent `TurnCompleted(Stop)` commit attempt. Error `Some` → `Environment(Shutdown)` with the retained quiescence: the report's Error outranks `Incomplete` as cause. Error `None` with `Incomplete` → `Core(ShutdownIncomplete)` with the retained quiescence. Error `None` with `Quiesced` → the `TurnCompleted(Stop)` edge; failure to commit that record finalizes with the retained `Quiesced`. |
| `Closed` | Return `EngineExit::Stopped { state }`. |

**Edges** — the rows bind record sequence and failure outcomes; a realization may fuse
adjacent edges under one source certificate. The Requires column names either a fact
established by the source phase or work the transition performs. Each recorded edge's
commit succeeds or fails as `Journal(JournalFatal)` carrying that record's kind and, for
`TurnCompleted`, its outcome. Work the transition performs before the commit can fail
as the Phases and Requires rows name; `EventAccepted` alone can fail after acquiring its
candidate as `Core(TimeRegression)`. The empty-batch recordless edge cannot fail. The
checkpoint edge commits nothing and fails only as `Environment(Checkpoint)`. On any
failure the certificate is dropped and `RUN-FINALIZE` runs.

| From | Record | Requires | To |
|---|---|---|---|
| `Initial` | `RunStarted` | the `Initial` certificate, whose prospective index and time were fixed by the run startup table | `TurnOpen` (certificate index 0 per `RUN-INDEX`; last accepted time the frozen start time) |
| `TurnOpen` | — | empty batch | `EffectsComplete` |
| `TurnOpen` | `CommandsPrepared` | nonempty batch | `Prepared` |
| `Prepared` | `CommandsDispatched` | every Command handed off | `EffectsComplete` |
| `EffectsComplete` | — | latch snapshot `None` | `Checkpointed` |
| `Checkpointed` | `TurnCompleted(Continue)` | the phase's fixed answer is `Continue` | `BetweenTurns` |
| `Checkpointed` | `StopRequested` | the phase's fixed answer is `Stop` | `StopPending` |
| `BetweenTurns` | `EventAccepted` | the transition's successful `next_event` return; `ENV-TIME`'s nondecrease, checked before the commit — violation is `Core(TimeRegression)`, nothing committed, and the candidate stays consumed | `TurnOpen` (certificate index advanced per `RUN-INDEX`; last accepted time the checked `Timestamp` returned with the candidate) |
| `StopPending` | `TurnCompleted(Stop)` | a clean report | `Closed` |

### Records

`RUN-RECORDS` fixes the wire shape; the table fixes each record's fields. Example:
`{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}`.

| Record | Fields after `record_kind`, `index` | Committed | Evidences |
|---|---|---|---|
| `RunStarted` | `schema_version`, `logical_time` | Before `on_start`. | Acceptance of the start turn, index 0, at the start time. |
| `EventAccepted` | `logical_time`, `event` | Before `on_event`. | Acceptance of one External Event. |
| `CommandsPrepared` | ordered `commands` | Before the first handoff of a nonempty batch. | The turn's complete Command intent. |
| `CommandsDispatched` | — | After the last handoff of a nonempty batch. | Every prepared Command was handed off. |
| `StopRequested` | — | After a `Stop` answer, before `shutdown`. | The Application requested shutdown. |
| `TurnCompleted` | `outcome` (`Continue`/`Stop`) | End of every non-Fatal turn. | The turn's outcome; a `Stop` outcome also witnesses the clean report. |

`EngineExit` is the run's only outcome channel: a fatal run's committed record sequence
simply ends at its last committed record — the sink may hold an uncertain physical
suffix (`JRN-COMMIT`) — and `CommandsPrepared` plus the typed `Dispatch { position }`
identify the exact handed-off prefix, while a `Journal(CommandsDispatched)` cause
certifies the whole batch was handed off. Records carry `record_kind`, `schema_version`,
indices, times, Events, Commands, and outcomes and nothing else (`RUN-RECORDS`), so
Journal bytes are Environment-independent given the trace (`DET-ENV`). The concrete
Rust record types are mechanism; `RUN-RECORDS` and the table bind the serialized form.

### Guarantees

| ID | Guarantee |
|---|---|
| `RUN-SERIAL` | The Engine owns the Environment and the Journal by value and is their only caller, delivering `ENV-SERIAL` by construction: one serial loop (A2), calls in the order the graph directs, and a consuming `shutdown` that makes a second lifecycle call unrepresentable. |
| `RUN-GRAMMAR` | Records are committed only through the graph's transitions, and the graph is enforced at compile time: possession of the certificate in phase P proves the Journal holds exactly the records of the certificate's path to P, and that the certificate holds exactly the phase data fixed by the run startup and edge tables. Every transition consumes its source certificate and returns its successor only after performing the edge requirement itself and successfully committing its listed record or records, if any. To any caller outside `RUN-ENFORCEMENT`'s boundary, a transition requirement is never a caller-supplied witness that can be forgotten, reused, contradicted, or forged: it is the phase itself or work the transition performs. An out-of-order record, a record whose kind disagrees with its payload, a `TurnCompleted` outcome disagreeing with the phase's fixed answer, a caller-supplied index, start time, or candidate, an accepted Event, index, or time disagreeing with its acceptance record, a skipped checkpoint, a `CommandsDispatched` without every handoff, a `TurnCompleted(Stop)` without a clean report, or a duplicated or fabricated certificate is unrepresentable to such a caller. This compile-time claim excludes the three runtime points, in-module transition conduct, and record omission by dropping, all of which `RUN-ENFORCEMENT` names as runtime- or test-enforced. The certificate does not implement `Clone`, `Copy`, or `Default`. |
| `RUN-ENFORCEMENT` | `RUN-GRAMMAR`'s enforcement boundary is exact. Three points remain runtime: the index arithmetic behind `accept_event`, whose domain check and overflow panic are fixed by `RUN-INDEX`, and the answer and batch the Engine passes from the turn it just ran to the single call sites of `classify` and the batch transition. One always-on assertion checks the induction base: the `Initial` certificate stores prospective index 0. `classify` consumes the answer and the `TurnOpen` certificate into one of two non-cloneable, answer-typed refinements of that phase; after that call no transition accepts an answer. The answer at that call site and its resulting outcome are pinned by `VERIFY-JOURNAL`. The batch transitions always-on assert empty or nonempty as their branch requires (`ASSERT-INVARIANTS`). In-module transition conduct remains expressible in ordinary code, and dropping a certificate and committing nothing remains expressible as the Fatal path, so required operation and record sequences are test-enforced; the wire format is also test-enforced. Certificate, phase, and transition types are module-private; every other illegal state listed by `RUN-GRAMMAR` is unrepresentable to callers outside that boundary. |
| `RUN-RECORDS` | A record is one flat JSON object — its top-level members are exactly its row's fields, in table order; values may nest, the top level may not. `record_kind` comes first, a bare tag string naming the kind; then `index`, the index of the turn the record belongs to — for `EventAccepted`, the newly accepted turn's. `outcome` is a bare tag string. `schema_version` is 1. `RunStarted` is the only possible first record, so every nonempty Journal begins with a versioned record. |
| `RUN-INDEX` | The `Initial` certificate's stored 0 is prospective. Thereafter the certificate's index is the latest accepted turn's ordinal: 0 once `RunStarted` commits, advancing exactly when `EventAccepted` commits. The bound is the index domain itself, checked before `next_event`: at certificate index `u64::MAX` the run ends `Core(IndexExhausted)` with no candidate consumed. Overflow past that check is an invariant panic. |
| `RUN-CHECKPOINT` | Every turn that reaches `EffectsComplete` takes the latch snapshot (`take_error`) exactly once — after its last handoff, before its completion record; a turn that goes Fatal earlier takes none. A pending Error there is `Environment(Checkpoint)` Fatal. On the Continue path a later publication stays pending for the next observing operation (`ENV-LATCH`); on the Stop path the next and final latch observation is shutdown's close, and once `StopPending` runs, its row is decisive on the report. |
| `RUN-FINALIZE` | Fatal finalization runs exactly once: fix the first-observed cause (A4); fix quiescence — `start` returned Ok and the Environment is unconsumed → call `shutdown` (`TRUST-BLOCKING`), take the report's quiescence, and discard the report's Error (A4: a cause exists); consumed, exactly when `StopPending` ran → use the report quiescence retained by that state, including after failure to commit `TurnCompleted(Stop)`; `start` returned `Err` → `Quiesced` (`ENV-START`); return `EngineExit::Fatal { state, cause, quiescence }`. |
| `DET-RUN` | Within one Environment type, under `TRUST-PURE` and `TRUST-SERIALIZE`: the same build (toolchain and full dependency set, `serde_json` included), Application, initial State, configuration, and trace reproduce the same handler calls, State transitions, Command intent, and Journal bytes through the last committed record, and exits equal in every Core-owned discriminant and Core-owned payload — equal outright when the Error values erased from the trace also correspond (`VERIFY-CONFORMANCE`). |
| `DET-ENV` | Across Environment types, under `DET-RUN`'s premises with only the Environment type free: equal traces produce equal handler calls, State transitions, Command intent, and Journal bytes through the last committed record, and exits equal in every Core-owned discriminant and payload — the `EngineExit` variant, `FatalCause` variant, `EnvironmentOperation` with its `position`, `RecordKind`, `TurnOutcome`, `JournalError` variant and `SinkOperation`, `CoreError` with its payloads, and `Quiescence`. Only Error values inside the exit may differ; they are erased from the trace. The row binds where equal traces exist: a failure shape only one Environment type can produce has no cross-type comparison, and the conformance suite compares the expressible overlap. |

### Enforcement

The mechanism behind `RUN-GRAMMAR`. All of it is module-private inside the engine;
`RecordKind` and `JournalFatal` are defined here and re-exported publicly.

The certificate:

```rust
pub(super) struct Certificate<W: std::io::Write, P> {
    journal: Journal<W>,   // the run's one Journal, consumed at minting
    index: EventIndex,     // prospective 0 in Initial; latest accepted ordinal thereafter
    last_time: Timestamp,  // prospective start time in Initial; last accepted time thereafter
    _phase: PhantomData<fn() -> P>,
}
```

*Derive:* `RUN-GRAMMAR` prohibits `Clone`, `Copy`, and `Default`; this mechanism omits
them as its local realization. Minting consumes the run's Journal, so a second grammar
over it is unconstructible; dropping the certificate destroys the Journal. The `fn() ->
P` marker keeps `Send`/`Sync` independent of the phase. The `Initial` phase exposes
neither `index()` nor `logical_time()`. Those getters exist only on phases from which
`Context` can be constructed.

*Derive:* the following nonbinding table renders the module-private transition
mechanism that realizes `RUN-GRAMMAR`; its Rust rendering is neither an API block nor a
binding table. `A` below is the type marker `Continue` or `Stop`. `classify` returns one
private `ClassifiedTurn` enum whose variants contain `TurnOpen<Continue>` and
`TurnOpen<Stop>` respectively; the Engine matches that enum once. These are typed
refinements of the binding table's `TurnOpen` phase, not additional graph phases.
`CommandBuffer<C>` is the private reusable fixed-capacity buffer described by the
Application mechanism, not a public type.

| Phase value | Transition | Does | Record | Returns |
|---|---|---|---|---|
| `Initial` | `run_started()` | reads the prospective index and time from the certificate; no caller supplies either value | `RunStarted` | `TurnOpen` at index 0 with the frozen start time accepted |
| `TurnOpen` | `classify(answer)` | after the **Phases** table's `TurnOpen` work has handled overflow and `Fatal`, consumes the non-Fatal answer and the certificate; a runtime match fixes the answer in the existing phase's type | — | `ClassifiedTurn::Continue(TurnOpen<Continue>)` or `ClassifiedTurn::Stop(TurnOpen<Stop>)` |
| `TurnOpen<A>` | `no_commands(&CommandBuffer<C>)` | asserts the actual reusable batch empty; infallible, no commit | — | `EffectsComplete<A>` |
| `TurnOpen<A>` | `dispatch_batch(env, &mut CommandBuffer<C>)` | asserts the actual reusable batch nonempty; commits `CommandsPrepared` from a shared view; drains each Command by value through the whole handoff loop in order; a dispatch `Err` carries `{ position, error }` and discards the undelivered suffix | `CommandsPrepared`, then `CommandsDispatched` only after the last handoff | `EffectsComplete<A>`, realizing the graph's `Prepared` state internally |
| `EffectsComplete<A>` | `checkpoint(env)` | the `take_error` snapshot; `Some` consumes the certificate into the `Environment(Checkpoint)` path | — | `Checkpointed<A>` |
| `Checkpointed<Continue>` | `complete_continue()` | the phase's only method | `TurnCompleted(Continue)` | `BetweenTurns` |
| `Checkpointed<Stop>` | `request_stop()` | the phase's only method | `StopRequested` | `StopPending` |
| `StopPending` | `close(env)` | `shutdown`; retains the report's quiescence before inspecting its Error; `Some` or `Incomplete` consumes the certificate into its Fatal path, and failure to commit after a clean report carries the retained `Quiesced` into finalization | `TurnCompleted(Stop)` | `Closed` |
| `BetweenTurns` | `accept_event(env)` | checks the index domain before interaction; calls `next_event`; derives the next index; checks `ENV-TIME`'s nondecrease before committing; the record and successful successor certificate use that returned Event, returned time, and derived index — operation `Err` is `Environment(NextEvent)`, and time violation is `TimeRegression` | `EventAccepted` | `TurnOpen` plus the accepted Event |

`dispatch_batch` is one transition over the actual reusable buffer. It first borrows the
buffer to commit `CommandsPrepared`, then drains each Command by value into `dispatch`;
the emptied allocation remains available for the next turn. With separate prepare and
dispatch calls, two independent buffers could commit a `CommandsDispatched` after a
partial handoff. The graph's `Prepared` state and both its edges bind the record
sequence, which is unchanged, as are the failure outcomes: a `CommandsPrepared` commit
failure precedes any handoff; `Err` at k keeps the prefix semantics and discards the
undelivered suffix; a `CommandsDispatched` commit failure follows every handoff.

One payload struct per record derives `Serialize`; its first field is a kind-typed
zero-sized value whose shared hand-written `Serialize` implementation emits the tag
supplied by `RecordPayload` — the serialized tag and a `JournalFatal`'s kind have one
source, and a kind/payload mismatch is unconstructible even in-module. `classify` fixes
the answer marker before either batch transition; `TurnOutcome` is chosen by the
transition exposed by that marker, never its caller. The same value supplies a
`TurnCompleted` payload and, if its commit fails, `JournalFatal.outcome`. A record's
`index` is the certificate's own arithmetic. `run_started` takes no index or
time argument: its payload reads both from the `Initial` certificate. `accept_event`'s
only argument is the Environment: it obtains the candidate itself, and its payload
carries that Event, its returned time, and the derived next index. Those same index and
time values become the certificate's index and last accepted time only on successful
commit, and the same Event is returned for the handler. Transitions that interact with
the Environment take it themselves: the Run owns interaction, and the requirement they
perform is the edge's.

*Derive — the proof boundary (`RUN-ENFORCEMENT`).*

- **Affinity, not linearity.** Dropping a certificate and committing nothing
  type-checks — that is the Fatal path by design — so a record *omitted* where the
  graph requires one is caught by golden-Journal tests, never the compiler.
- **Three points stay runtime.** The index arithmetic behind `accept_event` is
  guarded by `RUN-INDEX`'s domain check; overflow past it is an invariant panic. One
  always-on assert checks the induction base: the `Initial` certificate stores
  prospective index 0. The other two values are the answer passed to `classify` and
  the actual reusable buffer passed to the batch transition, one call site each.
  Every other caller-facing illegal state in `RUN-GRAMMAR`'s list is unrepresentable.
- **Residual always-on asserts:** `dispatch_batch` rejects an empty buffer; the
  recordless batch edge asserts the buffer it bypasses is empty — a nonempty batch
  there is a bug, not a silent drop.
- **Unforgeable means module-private.** The certificate, phases, and transitions hold
  their guarantees exactly as long as they stay behind their modules.
- **The wire format** (`RUN-RECORDS`) is pinned by byte-exact golden tests.

### Notes

*Derive — the certificate's corollaries.* After any Fatal, no commit is expressible:
the certificate is gone and the Journal was destroyed with it. The next Event is
acquirable only after `TurnCompleted(Continue)` commits: no other edge yields
`BetweenTurns`. No handler runs before its acceptance record: only `RunStarted` and
`EventAccepted` yield `TurnOpen`. `Stopped` implies a clean report: `Closed` is
reachable only through the closing transition's report of `Quiesced` with no Error.

*Derive:* the empty batch and the checkpoint take recordless edges because they
bracket no effect — nothing was prepared, nothing handed off, nothing observed but the
latch. A5 fixes every other record's position: acceptance and intent records precede
their effects; `CommandsDispatched` and `TurnCompleted` witness completed ones.

*Derive:* a Fatal's `EnvironmentOperation` and `position` name where the Error was
observed, never where it was caused: under `ENV-LATCH` an already-latched Error
surfaces at the next observing operation, so in a concurrent Environment the cause may
lie in any earlier turn since the last snapshot. Records and exits localize
observation; localizing the cause is Port evidence. Under `ENV-LATCH`, a publication
completed before a `next_event` or `dispatch` call is ordered ahead of that call's
observation point, so the pending Error returns first. A publication overlapping either
call can differ: if
the operation's own pre-commitment Error fixes first, it is returned, and the
publication ordered after it stays pending and leaves only through finalizing shutdown,
where its Error is discarded (`RUN-FINALIZE`).

*Derive:* `CommandsDispatched` can be a run's final record — the checkpoint that
follows it observed a pending Error.

*Derive:* an Application that wants stop-specific Port behavior emits it as Commands
before answering `Stop`; handoff to the destination inbox is guaranteed, and
processing is the destination Port's draining policy (`TRUST-DRAIN`). The shutdown
signal carries only its glossary meaning.

*Derive:* a candidate consumed by `next_event` becomes accepted only when
`EventAccepted` commits; a candidate lost to `TimeRegression` or a failed commit never
had an index — indices exist only inside the certificate — and its value survives only
in the trace.

*Derive:* Environment activation precedes `RunStarted`'s commit — the record carries
the frozen start time — so a run whose first commit fails exits Fatal with real
effects and no committed record; the exit carries the cause.

*Derive:* after an abort no exit exists (A8): the Journal then bounds the dispatch
uncertainty to the whole prepared batch — `CommandsPrepared` names the intent, nothing
names the handed-off prefix — and reconciliation runs on business keys (`TRUST-KEY`).

*Derive:* `Core(CommandBoundExceeded)` is an intent vacuum by design: the turn dies
before `CommandsPrepared`, its overflowing batch discarded unrecorded, so no record names
what the handler staged. The exit's cause and the turn's acceptance record locate the
failure; the staged Commands appear in no record.

*Derive:* an Environment may resolve races among concurrent sources however it likes;
the accepted trace records the resolution, and `DET-RUN` holds conditional on it.

*Justify:* the index domain is the run bound because it is the one non-arbitrary bound
available: it makes `RUN-INDEX`'s overflow argument a domain fact rather than a
configuration promise, and a harness that wants a tighter bound composes one in user
code — a counting Port, or an Environment wrapper.

## 8. Live Environment

The live Environment runs each bound Port on its own supervised thread and bridges
concurrent reality into the serial Environment contract. The Core's boundary is that
contract; this section ships one implementation of it — every guarantee below realizes
a named contract row or defines the live Port-facing API.

### API

Semantics here are normative; the exact `LiveCtx` signatures are provisional until the
open Wiring section settles construction.

```rust
pub trait LivePort<C: PortContract>: Send + 'static
where
    C::Event: Send + 'static,
    C::Command: Send + 'static,
{
    type Error: Send + 'static;
    fn run(self, ctx: LiveCtx<C>) -> Result<(), Self::Error>;
}

impl<C: PortContract> LiveCtx<C> {
    /// Block until one Command arrives or the shutdown signal is raised.
    /// Once raised, every call reports the signal; `try_recv` is the
    /// draining path.
    pub fn recv(&mut self) -> PortInput<C::Command>;
    /// Nonblocking: returns pending Commands first, then `Shutdown`.
    /// Once the signal is raised and the inbox is drained, every call returns
    /// `Some(PortInput::Shutdown)`; `None` means no Command is pending and the
    /// signal has not been raised.
    pub fn try_recv(&mut self) -> Option<PortInput<C::Command>>;
    /// Offer one Event through the Slot's frozen fan-in constructor.
    /// Never waits for future capacity; rejection returns the Event.
    pub fn offer(&mut self, event: C::Event) -> Result<(), OfferRejected<C::Event>>;
    /// Direct observation of lifecycle signaling.
    pub fn lifecycle(&self) -> Lifecycle;
}

pub enum PortInput<Cmd> { Command(Cmd), Shutdown }
/// Each variant returns the rejected Event to its offerer.
pub enum OfferRejected<Ev> { Full(Ev), Closed(Ev) }
pub enum Lifecycle { Running, Shutdown }
```

*Define:* Live completion state — the fixed, Slot-keyed set whose entry is
`Outstanding` until its supervisor shell enters a non-aborting terminal exit and
`Complete` thereafter. Completion state is cumulative from shell creation through
shutdown. `Complete` does not mean that the supervised thread was joined.

### Guarantees

| ID | Guarantee |
|---|---|
| `LIVE-THREADS` | Each bound Port runs in one supervised thread and owns its native client and all domain and protocol state. Everything crossing a Port-thread boundary — values moved in, Commands in, offered Events out, Port Errors out — is `Send + 'static`. |
| `LIVE-EVENTS` | Event fan-in is one bounded queue; dequeue order is admission order. Mapping into the Application Event sum precedes admission. `offer` never waits; `Full` or `Closed` returns the Event to the offering Port, which may retry under its own pacing while observing the lifecycle, or return an Error to latch. The fan-in closes when `shutdown` raises the signal: `offer` after it is `Closed`. |
| `LIVE-SELECT` | `next_event` waits, without busy-spinning, until the latch is pending or one Event is available; the choice between them follows `ENV-LATCH`'s publication ordering. The stamp is taken after the wait and immediately before the dequeue, and the dequeue is the consumption commitment (`ENV-ERRORS`): nothing fallible follows it. |
| `LIVE-TIME` | The single acceptor stamps from one monotonic clock, realizing `ENV-TIME`'s nondecrease structurally; duration conversion is checked (A6) and exhaustion is a typed Environment Error. |
| `LIVE-DISPATCH` | Each destination Port has one bounded Command inbox owned by the Live Environment; its `LiveCtx` is the only receiving capability. One non-waiting admission to that inbox is where `dispatch`'s handoff commits (the **Commitment points** table), with publication ordering governed by `ENV-LATCH`. |
| `LIVE-SUPERVISION` | Before the shutdown signal, `run(Err)` and `run(Ok)` completing prematurely each publish a typed Error to the latch and wake a blocked `next_event`; a test-profile unwind before the signal is a premature closure and publishes likewise, while after the signal it stays unpublished like an expected `run(Ok)`. Raising the signal ends `Running` at one linearized instant (`LIVE-SHUTDOWN`). After that instant, `run(Ok)` is expected and stays unpublished, while `run(Err)` still publishes: `ENV-LATCH` captures it before the final close and discards it after. Every required publication precedes that shell's transition to `Complete` (`LIVE-COMPLETION`), so shutdown cannot account the shell complete while missing its Error. |
| `LIVE-COMPLETION` | To witness the run's Quiescence under `ENV-SHUTDOWN`'s bounded quiescence policy, the Live Environment is the sole accounting owner of cumulative Live completion state. The fixed set has exactly one entry per bound Slot, matching the frozen supervisor set and order (`BOUND-STATIC`), is initialized before the start/cancel gate resolves, never grows, and is retained through shutdown. Each spawned shell exclusively owns one module-private, non-cloneable capability that changes only its Slot's entry from `Outstanding` to `Complete`, exactly once and infallibly, when that shell begins any non-aborting terminal exit: gate cancellation, return from `LivePort::run` with either result, or unwind under the test profile. While shutdown is waiting, the transition wakes it. The capability is unavailable to the Port value and `LiveCtx`. `Complete` is permanent and does not prove that the thread was joined. The fixed set is authority; any cached completed or outstanding count is only a checked derivative bounded by the set's length (A6). |
| `LIVE-LIFECYCLE` | The shutdown signal is `LiveCtx` authority — it consumes no queue or inbox capacity and is never hidden. Once raised, every `recv` reports it ahead of that Port's queued Commands — `ENV-SHUTDOWN`'s observability in its strongest form; `try_recv` yields queued Commands first and the signal after them, which is the draining path; `lifecycle` reads it directly. |
| `LIVE-START` | Every spawned supervisor shell waits at one start/cancel gate and cannot invoke `LivePort::run` while the gate is pending. Setup failure signals cancel at the gate, wakes and joins every shell, and returns `Err` with no Port code ever run — realizing `ENV-START`. After all fallible setup and start-time stamping succeed, signaling start at the gate is the commitment; no fallible startup work follows it. A Port failure after the start signal is a runtime failure, surfacing per `ENV-LATCH`. |
| `LIVE-SHUTDOWN` | `shutdown` realizes `ENV-SHUTDOWN`: in one linearized initiating instant it raises the signal, ends `Running`, closes the fan-in, wakes every Kavod-owned blocking point, and fixes one absolute shutdown deadline, the configured duration after that instant. Deadline addition is checked and saturates at the latest representable monotonic instant (A6). The latch remains open. Shutdown reads cumulative Live completion state (`LIVE-COMPLETION`); until every entry is `Complete` or the deadline expires, it waits only for outstanding entries, begins no join, gives every wait only the time remaining before the same deadline, and never restarts the deadline for a Slot, wakeup, or state transition. It then makes one final synchronized observation that decides every completion race and closes the latch into the `ShutdownReport` (`ENV-LATCH`): a completion transition and Error publication ordered before that observation count and are captured, while ones ordered after it do not count and are discarded. If every entry is `Complete`, shutdown joins every supervised thread and returns `Quiesced`; those joins may finish after the deadline because a blocking wait implies no elapsed-time bound (`BOUND-LOOPS`) and remaining teardown is trusted bounded (`TRUST-BLOCKING`). Otherwise shutdown detaches every unjoined supervised thread and returns `Incomplete`; their later Errors are post-close publications and are discarded. |

### Mechanism

One workable mechanism, replaceable wherever the guarantees hold: a bounded channel for
fan-in; one bounded SPSC inbox per destination Port; a supervisor-owned latch
(`Mutex` + `Condvar`, or an equivalent channel) that fan-in waiting and supervision
both wake; one start/cancel gate shared by the supervisor shells; a lifecycle cell the
`LiveCtx` blocking points check first.

| Step | `start` | 
|---|---|
| 1 | Adopt the frozen Slot order and configured capacities; create the queue, inboxes, latch, lifecycle cell, cumulative completion state, its bounded wake channel, and the pending gate. |
| 2 | Spawn one thread per bound Port in frozen Slot order; each shell waits at the gate. |
| 3 | Complete every remaining fallible setup step; stamp and freeze the start time (`LIVE-TIME`). |
| 4 | Any failure so far: signal cancel at the gate, wake and join every shell, return `Err` (`LIVE-START`). |
| 5 | Signal start at the gate — the commitment — and wake every shell; each invokes `LivePort::run`, classifies its result under `LIVE-SUPERVISION`, and maintains its entry under `LIVE-COMPLETION` on every terminal path. |
| 6 | Return the frozen start time. |

`next_event`: wait under `LIVE-SELECT`; a pending latch Error is taken, marked
reported, and returned (nothing consumed); otherwise stamp from the acceptor's
clock — a failed conversion is a typed Error, nothing consumed — then dequeue one
candidate and return it with that stamp; the dequeue is the consumption instant, and
nothing fallible follows it. `dispatch`: a pending latch
Error returns first (`ENV-LATCH`); otherwise route by the fan-out match
(`PORT-ROUTING`) and try one non-waiting admission — full or closed is a typed `Err`
with nothing handed off. `take_error`: one atomic snapshot per its commitment row.

*Justify:* one workable realization of `LIVE-SHUTDOWN` keeps the lifecycle state, latch,
and cumulative completion state under one lock. The initiating critical section raises
the lifecycle signal while leaving the latch open, and one module-private deadline
budget records that instant's monotonic time and owns the saturated shutdown deadline.
One bounded nonblocking wake token per entry only prompts a new observation; tokens
produced before initiation remain available. The completion-wait phase scans the fixed
set and consumes at most one token per entry, asking the budget only for the remaining
duration. When the set may be complete or the deadline expires, one final critical
section scans the authoritative set, decides `Quiesced` or `Incomplete`, and closes the
latch. A shell returning `Err` publishes before changing its entry to `Complete` under
that lock (`LIVE-SUPERVISION`), so the final scan cannot count the shell complete and
miss its Error. `Quiesced` exposes the handles for joining in frozen Slot order;
`Incomplete` drops every unjoined handle without waiting.

The supervision shell runs on the Port's own thread: wait at the gate; cancel returns
without invoking the Port; start invokes it and maps `Err` or premature completion into
a typed Error published first-wins to the latch — classification and publication under
the latch lock — waking the select. A non-cloneable terminal guard remains on the
shell's frame, outside the Port value and `LiveCtx`, for the shell's entire lifetime.
Its nonpanicking `Drop` publishes the premature-closure Error first when a pre-signal
test-profile unwind reaches it, then changes that shell's completion entry exactly once
and attempts one nonblocking wake. It runs after normal and `Err` paths and during test-profile
unwind; a shipped panic aborts instead.

### Notes

*Derive:* `Quiesced` here is a full witness — every supervised thread was joined, so
every Port finished entirely, destructors included. That is what settles user-owned
handles captured before binding: an exit of `Stopped`, or `Fatal` with `Quiesced`,
means terminal Port state is readable through them. Under the shipped abort profile a
Port panic ends the process before any join (A8); under the unwinding test profile a
panicked Port joins cleanly, so lifecycle tests read `Quiesced` as "joined", never
"succeeded".

*Define:* external cancellation is a Port: a signal-handling Port offers an Event —
SIGINT, say — whose handler answers `Stop`. `Engine::run` blocks and exposes no other
cancellation channel.

*Derive:* after `Incomplete`, a detached thread may still be running and in-process
reclamation is impossible; the caller renders the exit and terminates promptly, and a
supervisor above the process reclaims it (Obligations table). The evidence is
the Journal's committed records.

*Justify:* Port work has no Kavod-enforced elapsed-time bound and is trusted to
terminate (`TRUST-BLOCKING`). A blocking wait is not an active loop and implies no
elapsed-time bound (`BOUND-LOOPS`), so `LIVE-SHUTDOWN` applies its deadline to waiting
for outstanding Live completion state, not to joining after every entry is `Complete`.
This is the Live realization of `ENV-SHUTDOWN`'s bounded quiescence policy; the
Simulated Environment realizes the same contract without a trusted join tail. Port
blocking points observing the lifecycle remains a trusted obligation
(`TRUST-LIFECYCLE`) rather than a Kavod guarantee.

*Derive:* if every completion entry is `Complete` and post-completion teardown violates
`TRUST-BLOCKING` by never terminating, `shutdown` remains blocked in a join and produces
neither a `ShutdownReport` nor an `EngineExit`; it does not return `Incomplete`. The
contract promises no in-process recovery from that trusted nontermination.

*Justify:* the shell-owned guard's normal and `Err` behavior is what shipped code uses;
its unwind behavior exists only under the unwinding test profile and does not make
shipped code rely on unwinding (`TRUST-ABORT`). Because neither the Port nor `LiveCtx` can
reach the guard, only Kavod's module-private shell could forget it; lifecycle tests pin
that implementation boundary. `LIVE-START` needs no deadline while joining canceled
shells because its gate proves that no Port code ran. The shutdown deadline bounds Port
cooperation after activation, not joining by itself.

*Justify:* `try_recv` yields Commands before the signal so a draining Port can finish
queued work; the signal is never hidden — `lifecycle` and `recv` report it immediately —
so `ENV-SHUTDOWN`'s observability holds on every path.

## 9. Simulated Environment

The simulated Environment executes the same contract single-threaded under virtual
time; Ports advance only when stepped. This section ships the second implementation of
the Environment contract — every guarantee below realizes a named contract row or
defines the sim Port-facing API.

*Define:* Sim Port lifecycle — the Environment-owned method-eligibility state of one bound
SimPort: `NotStarted`, `Open`, or `Ended`.

### Lifecycle

| ID | Guarantee |
|---|---|
| `SIM-LIFECYCLE` | The Environment owns exactly one Sim Port lifecycle per bound Port. It begins `NotStarted`. Invoking `start` moves it to `Open` before Port code runs. A successful `start`, `on_command`, or `step` leaves it `Open`; the first `Err` returned by any of those methods moves it to `Ended` before the Environment does further work. Invoking `stop` is permitted only in `Open` and moves the lifecycle to `Ended` before Port code runs, whether it returns `Ok` or `Err`. Only `start` may be invoked in `NotStarted`; only `on_command`, `step`, or `stop` may be invoked in `Open`; no method may be invoked in `Ended`. |

### API

```rust
pub trait SimPort<C: PortContract> {
    type Error;
    fn start(&mut self, ctx: &mut SimCtx<'_, C>) -> Result<(), Self::Error>;
    fn on_command(
        &mut self,
        command: C::Command,
        ctx: &mut SimCtx<'_, C>,
    ) -> Result<(), Self::Error>;
    fn step(&mut self, ctx: &mut SimCtx<'_, C>)
        -> Result<Option<C::Event>, Self::Error>;
    /// Called at most once, only after `start` returned `Ok`; no method
    /// follows its return (`SIM-LIFECYCLE`).
    fn stop(&mut self) -> Result<(), Self::Error>;
}

impl<C: PortContract> SimCtx<'_, C> {
    pub fn now(&self) -> Timestamp;
    pub fn set_next(&mut self, time: Timestamp) -> Result<(), SimCtxError>;
    pub fn clear_next(&mut self);
}

pub enum SimCtxError {
    /// `set_next` requires `time >= now`; rejection changes nothing.
    TimeBeforeNow { now: Timestamp, requested: Timestamp },
}
```

### Guarantees

| ID | Guarantee |
|---|---|
| `SIM-STATE` | Each simulated Port owns all of its simulated domain state; the Environment holds no shared model and runs no concurrency — realizes `ENV-SEPARATION` and `PORT-STATE`. |
| `SIM-START` | `start` fixes the start time from the configured origin and sets `now` to it, then invokes each `NotStarted` Port's `start` in frozen Slot order (`SIM-LIFECYCLE`). After every invocation returns `Ok`, every Port is `Open` and successful return is the startup commitment (`ENV-ERRORS`). On the first `Err`, the failing Port is `Ended`; startup calls `stop` exactly once on the earlier `Open` prefix, in frozen Slot order, discarding those Errors, while every later Port remains `NotStarted` and receives no method. Every Port is then `NotStarted` or `Ended` and, under `TRUST-SIM-PORT`, `TRUST-SPAWN`, and `TRUST-BLOCKING`, no Port-started run-scoped activity remains; startup fails with the original Error — effects already made stay real (A4's cleanup rule), and the return satisfies `ENV-START`. |
| `SIM-TIME` | `now` starts at the configured origin and moves only by `next_event` advancing it to the selected arm's time; the returned candidate is stamped with `now`. Every armed time is `>= now` (`SIM-WAKEUP`) and selection takes the minimum, so stamps never decrease — realizes `ENV-TIME`. |
| `SIM-DISPATCH` | If `ENV-LATCH` orders a pending Error before this call's handoff, `dispatch` returns it as `Err` with no Port invocation and no handoff. Otherwise, `dispatch` synchronously routes to exactly one Port's `on_command`; the invocation is where `dispatch`'s handoff commits (the **Commitment points** table), and `now` does not advance. An `Err` from `on_command` is published (`ENV-ERRORS`) and `dispatch` returns `Ok` — the invocation already committed. |
| `SIM-WAKEUP` | Each Port has at most one revocable wakeup arm, initially disarmed, modifiable only through its own `SimCtx`: `set_next` requires `time >= now` — rejection changes nothing — and is last-call-wins; `clear_next` disarms. An arm is not an Event. |
| `SIM-SELECT` | `next_event` checks, in order at each selection: the latch — a pending Error that `ENV-LATCH` orders before this call's consumption returns as the call's `Err`, nothing selected or consumed; no armed Port (`SIM-COMPLETION`); the step budget (`SIM-STEPS`). It then selects the armed Port with the lowest time — equal times by round-robin: the selected Slot is the first lowest-time armed Slot met scanning from the cursor in frozen Slot order, wrapping; the cursor starts at Slot 0, persists across `next_event` calls, and moves to the selected Slot's successor after every selected `step`, including one returning `None` — advances `now` to the selected arm's time, clears the arm, and calls `step`. Only `step(Some)` creates the returned candidate, and its return is the consumption commitment (`ENV-ERRORS`); `step(None)` continues selection; `step(Err)` returns that Error. Every `Err` this call returns leaves the selections already made standing as the named subordinate effects (**Commitment points** table): each one's advanced `now`, cleared arm, and spent budget. |
| `SIM-STEPS` | Every `step` call consumes one unit of the configured step budget, fresh for each `next_event` invocation; `start`, `on_command`, and `stop` consume none. The budget is checked before selecting, advancing time, or clearing an arm; exhaustion is a typed Environment Error. |
| `SIM-COMPLETION` | `next_event` finding no armed Port — at entry or mid-selection — is a typed Environment Error: the run has nothing left to wait for. A run ends normally through the finite-source pattern. |
| `SIM-SHUTDOWN` | `shutdown` realizes `ENV-SHUTDOWN`: it closes Event admission, then delivers the sim shutdown signal by invoking `stop` exactly once on every `Open` Port (`SIM-LIFECYCLE`), in frozen Slot order, while the latch remains open. Every returned Error is mapped and published; first-wins applies, and shutdown continues through the remaining `Open` Ports. After every call returns, the final observation closes the latch into the report (`ENV-LATCH`). Every started lifecycle is then `Ended`, so the report carries `Quiesced`; completion of Port-started activity relies on `TRUST-SPAWN`, and return from each Port call relies on `TRUST-BLOCKING`. |

### Mechanism

Environment state: one Environment-owned lifecycle state stored with each Port
(`SIM-LIFECYCLE`), `now`, one `Option<Timestamp>` arm per Port, the round-robin cursor,
the latch, and a steps-used counter reset at each `next_event` entry. The lifecycle
representation is replaceable wherever `SIM-LIFECYCLE` holds. Selection always-on asserts
the selected Port is `Open` (`ASSERT-INVARIANTS`).
`dispatch`:
pending latch Error first (`ENV-LATCH`); otherwise route by the fan-out match and
invoke `on_command` — an `Err` from it lands in the latch and the `dispatch` still
returns `Ok`, because invocation already committed. `take_error`: one snapshot per its
commitment row; it is how an `Err` from a turn's final `on_command` reaches the run.

### Notes

*Derive, showing the method:* `on_command(Err)` is a failure after the handoff
commitment, so the Port's mutations stand, the Error latches, and the current
`dispatch` returns `Ok` — A3, applied through `ENV-LATCH`. Likewise `step(Err)` cannot
roll back the advanced `now`, the cleared arm, or Port mutations. Commands and earlier
equal-time turns may alter or cancel a later Port's arm before it fires; that is what
revocable means.

*Derive:* selection never meets an `Ended` Port's arm, so `SIM-SELECT` cannot breach
`SIM-LIFECYCLE`. A lifecycle Ends mid-run only through an `Err`: from `start`, startup
fails and no selection ever runs; from `step`, `next_event` returns the Error and
`ENV-SERIAL` permits only `shutdown` after it; from `on_command`, the Error is pending in
the latch, and every later observing operation returns it before reaching selection
(`ENV-LATCH`, `SIM-SELECT`'s check order). A stale arm is unreachable, not forbidden.

*Derive:* processing is synchronous and `SIM-SHUTDOWN` ends every remaining `Open`
lifecycle before its final observation, so the Environment's quiescence account is
structural and the report always carries `Quiesced`; completion beyond that account
rests on `TRUST-SPAWN`.

*Derive:* on the Stop path the checkpoint precedes `SIM-SHUTDOWN`, but each `stop` runs
before the final close. The report therefore carries the first `stop` Error published
during shutdown, if any, and otherwise carries `None`.

*Derive:* if a `stop` call violates `TRUST-BLOCKING` by never terminating,
`SIM-START`'s cleanup or `SIM-SHUTDOWN` remains blocked and produces neither an
operation result nor an `EngineExit`; the contract promises no in-process recovery from
that trusted nontermination.

*Derive:* replay is user wiring: a fixed or recorded trace presented by a user-written
`SimPort`, or a bespoke `Environment` built on `Timestamp::from_nanos`, with `DET-RUN`
as the counterfactual it relies on. Sim-Port determinism and bounded `step` work are
trusted obligations (`TRUST-SIM-PORT`).

*Derive:* byte-equal replay of a recorded run needs three preconditions: the
configured origin equals the recorded `RunStarted` logical time; the replay Port arms
each recorded stamp in order and answers each selected `step` with the recorded Event;
and the step budget covers every acquisition. The three are necessary, not sufficient:
equal-time selection depends on the cursor, and the cursor on when each arm was placed —
placement no recorded artifact captures — so a multi-Slot replay must also reproduce
each arm's placement, and a failure run must reproduce each Error's presence at its
trace position. A single-Port run with no Error in its trace needs the three alone.

*Derive:* a Port set that never arms is a run with nothing to wait for: the first
`next_event` ends `SIM-COMPLETION`. A start-`Err` run leaves an empty Journal — the
Error payload is the sole diagnostic.

*Justify:* the step budget exists because `step` may re-arm its own Port —
`set_next(now)` included — so selection alone cannot bound the acquisition loop.

## 10. Wiring & construction — OPEN

The one part of this document not ready for implementation. Decisions this section must
make, for both Environments and ideally with one shared answer where the question is
shared:

- The builder/registration API binding each Slot to one Port implementation in frozen
  Slot order — live: `LivePort` plus per-inbox and fan-in queue capacities; sim:
  `SimPort`.
- Where the frozen fan-in constructors and the hand-written fan-out match live, and how
  the builders receive them (`PORT-ROUTING`).
- Composition of each Environment's `Error` sum: Kavod-owned variants (live: dispatch
  inbox exhaustion — fan-in `Full` goes to the offering Port, never the Engine —
  thread-spawn failure, time-domain exhaustion, premature closure; sim: nothing-armed,
  step-budget exhaustion) plus one mapped variant per Slot's Port Error, at each
  Environment's mapping site (`PORT-ROUTING`).
- Final `LiveCtx` signatures, and how one is constructed against the chosen channels.
- `LiveConfig`: the shutdown deadline (nonzero milliseconds) and how the live time
  origin is anchored.
- `SimConfig`: the time origin and step budget (nonzero), and where it lives relative
  to `EngineConfig`.
- What fixes the Slot order: registration order, or the Slot sum's declaration order —
  declaration order is the candidate that keeps one authority.
- The crate's public re-export policy at `lib.rs`.
- Thread naming conventions, if any.

Constraints already fixed: every guarantee in the Environment, Live, and Simulated
sections; the commitment table; the `ShutdownReport`; the Run graph and certificate
grammar (`RUN-GRAMMAR`); `Send + 'static` boundaries; frozen Slot order as the only
ordering authority; a nonempty Port set (`BOUND-STATIC`); nonzero configured bounds
(A6, `BOUND-NONZERO`); everything frozen before `Engine::run`.

## 11. Crate layout

One crate, `kavod`, no feature gates — both Environments are std-only. Dependencies:
`serde` (with `derive`) and `serde_json`. `ports!` is `macro_rules!`, so no proc-macro
crate exists. This section is mechanism except the guarantee below and the public item
names, which the API blocks own.

```
kavod/src/
  lib.rs             #![forbid(unsafe_code)]; public re-exports (policy: Wiring, open)
  time.rs            EventIndex, Timestamp
  application.rs     Application, Outcome, Context
  port.rs            PortContract, Never, ports!
  environment.rs     Environment, Quiescence, ShutdownReport
  journal.rs         Journal, JournalError
  bounded_buffer.rs  crate-internal fixed-capacity storage backing the Command
                     batch and the Journal's encode buffer
  engine/
    mod.rs           wiring only: module declarations plus public re-exports
    engine.rs        Engine, EngineConfig, EngineExit, FatalCause, CoreError, EnvironmentFatal, EnvironmentOperation
    record.rs        record payloads, the certificate, transitions
                     (private; RecordKind, TurnOutcome, and JournalFatal re-exported)
  live/              LivePort, LiveCtx, live Environment      (planned: Wiring)
  sim/               SimPort, SimCtx, simulated Environment   (planned: Wiring)
```

### Guarantees

| ID | Guarantee |
|---|---|
| `CRATE-EXPORTS` | Every public item is reachable at a path without repeated segments. The engine module's `mod.rs` re-exports its children's public items rather than exposing the child modules, so no repeated-segment path exists. |

## 12. Obligations & verification

The rows of the **Obligations** table are trusted: upheld by the named party and checked
by the stated means. Every other ID is enforced under the reading rules. This table is
the complete trusted boundary — an obligation absent from it is enforced, not assumed.

**Obligations**

| ID | Obligation | Upholder | Verified by |
|---|---|---|---|
| `TRUST-PURE` | Handlers, State, and every Event and Command payload type — `Drop` impls included — carry no hidden authority (clocks, entropy, IO, globals, concurrency order, Environment dependence beyond `Context` and the delivered Event) and no aliased mutability; Ports share no state; all run-varying data lives in State | Application author | Two runs against the same scripted Environment and sink → identical Journal bytes and `DET-RUN`-equal exits |
| `TRUST-SIM-PORT` | Simulated Ports are deterministic, do bounded `step` work, and carry no hidden authority | Sim Port author | Repeatability tests |
| `TRUST-ENV` | A bespoke Environment — one Kavod does not ship — upholds every Environment-contract row | Environment author | `VERIFY-CONFORMANCE`; `VERIFY-LATCH`; review of bounds and every other property no execution trace can witness |
| `TRUST-BLOCKING` | User code — `initial_state`, handlers, Ports, serializers, writers, callbacks, destructors — is bounded and reports Errors instead of panicking | Their authors | Review; A8 defines the blast radius when violated |
| `TRUST-ABORT` | Shipped code relies on unwinding nowhere, and the final binary builds with `panic = "abort"` | Kavod implementer; build/deployment configuration | Code review; CI build-profile check |
| `TRUST-ROUTING` | One-to-one Slot routing and per-Slot Error mapping (`PORT-ROUTING`), with routing reading only the Slot sum's discriminant and never a routed payload (`PORT-STATE`) | Wiring author | Per-Slot tests; review |
| `TRUST-KEY` | Every externally consequential Command carries a stable business key, and its destination Port uses it to recognize a repeated or uncertain external effect | Application author; Port author | Per-Slot tests |
| `TRUST-SERIALIZE` | `Serialize` impls are deterministic, side-effect-free, bounded, nonpanicking, with stable map order | Payload authors | Golden-Journal tests |
| `TRUST-LIFECYCLE` | Live Port blocking points observe the lifecycle and cooperate with shutdown | Live Port author | Shutdown tests under load |
| `TRUST-DRAIN` | A Port whose protocol includes final Commands drains its inbox on shutdown (`try_recv`) before returning | Live Port author | Shutdown tests |
| `TRUST-SPAWN` | A Port ends every activity it started — threads, callbacks, timers — before its final Port method returns: `LivePort::run` for Live, or the method whose return leaves the Sim Port lifecycle `Ended` (`SIM-LIFECYCLE`) for Sim; run-scoped activity is otherwise unwitnessable | Port author | Review |
| `TRUST-EXIT` | The process terminates promptly after an `Incomplete` exit, under a supervisor that reclaims it | Caller / deployment | Operational review |
| `TRUST-SIZING` | `max_record_bytes` fits the largest record the run can stage: the batch under `max_commands_per_turn` and every inbound Event's encoding; payload authors bound their encodings | Deployment configuration; payload authors | Config review; per-Slot tests |
| `TRUST-INBOX` | Each per-Port inbox capacity covers the largest same-turn burst to that Port plus expected cross-turn residue — admission failure is Fatal, not backpressure | Deployment configuration | Config review |
| `TRUST-SINK` | The sink is fresh or positioned immediately after a newline, exclusively owned by the Journal, and stores exactly the bytes given | Sink owner | Review; memory-sink fault tests |
| `TRUST-SHUTDOWN` | After raising the shutdown signal, a shipped Environment initiates no further externally consequential work (`ENV-SHUTDOWN`) | Kavod implementer | Code review |
| `TRUST-MEMORY` | Transitive memory bounds of owned values | Value owner | Owner-defined |

### Enforced verification

| ID | Guarantee |
|---|---|
| `VERIFY-CONTEXT` | A Context suite verifies `APP-EMIT`, `APP-OVERFLOW`, and `APP-STATE`: Commands append in call order through exact capacity; the first over-bound `emit` stores nothing and sets the overflow marker; every later emission stores nothing; each fresh handler starts with an empty buffer and clear marker; and State mutations stand on every Fatal path. |
| `VERIFY-CONFORMANCE` | A conformance trace suite checks every scripted Environment call against the graph, including each Command handoff. Within each Environment type, it runs every scripted trace twice and compares every value in `DET-RUN`'s list. Across the two shipped Environments it compares every Core-owned discriminant and payload in `DET-ENV`'s list; run against a bespoke Environment, the same suite is its certification (`TRUST-ENV`). It compares the expressible cross-type overlap: a failure shape only one Environment type can produce has no cross-type case. |
| `VERIFY-JOURNAL` | A Golden-Journal suite pins every graph-required record sequence and every record byte-exactly, including each non-Fatal handler answer against its required outcome records at `classify`'s single runtime call site, and proves an encoding containing an interior newline byte is rejected as `NotAnObject` with nothing written (`RUN-GRAMMAR`, `RUN-RECORDS`, `RUN-ENFORCEMENT`, `JRN-ENCODE`). |
| `VERIFY-FAULTS` | A fault-injection suite exercises every edge: scripted sinks for Journal failures; scripted Environments for each operation's `Err`, an `Ok` `next_event` with a decreasing timestamp, and shutdown reports carrying `Some(error)` or `{ Incomplete, None }`; and an over-emitting Application, checking the resulting `FatalCause` and the exit's `quiescence` — including the `Quiesced` retained across a `TurnCompleted(Stop)` commit failure. For each post-`start` operation `Err`, it exercises the cross-product with a shutdown report carrying `Some(error)`, where the operation's Error remains the Fatal cause and the report's Error is discarded (A4, `RUN-FINALIZE`). It separately proves that a `start Err` performs no shutdown. |
| `VERIFY-GRAMMAR` | A compile-fail suite proves illegal transition sequences, a skipped checkpoint, a premature `TurnCompleted(Stop)`, any caller attempt to commit `CommandsDispatched` independently of the transition that performs every handoff, an outcome disagreeing with the fixed answer, and any attempt to use `Clone`, `Copy`, or `Default` on the certificate do not compile (`RUN-GRAMMAR`, `RUN-ENFORCEMENT`); an `include!`-based fixture crate reconstructs the Engine module and attacks from its visibility position, so each failure reaches the grammar restriction rather than module privacy. |
| `VERIFY-LIVE` | A Live lifecycle and shutdown suite proves: no `LivePort::run` begins before gate activation; failed startup cancels and joins every shell; every shell owns exactly one completion entry; a completion before shutdown remains visible at the final observation; normal return, `Err`, and test-profile unwind each make the entry `Complete` exactly once; Port code cannot reach or defer the terminal guard; shutdown raises the signal and closes fan-in while leaving the latch open; `run(Ok)` after the signal is expected and unpublished, while a typed `run(Err)` before the final close enters the report when it is the latch's first publication; every required Error publication precedes that shell's `Complete` transition; all waits share one deadline fixed at the initiating instant, including a duration whose addition saturates; during shutdown no join begins while an entry remains `Outstanding`; a completion concurrent with expiry and a publication concurrent with the final close are each classified by the final observation; successful shutdown returns `Quiesced` and joins every supervised thread — read as "joined", never "succeeded", under the unwinding test profile; deadline expiry without an Error returns `{ Incomplete, None }`; Error plus expiry returns `{ Incomplete, Some(error) }` carrying the first publication; deadline expiry detaches every unjoined thread; and a post-close publication is discarded. It also verifies `LIVE-EVENTS`, `LIVE-SELECT`, `LIVE-DISPATCH`, and shipped `ENV-BOUNDS`: `offer` succeeds through exact fan-in capacity, then `Full` returns the same Event without growth, and `Closed` returns it after shutdown; Events admitted in a known order are dequeued in that order; under an injected clock, an Event waking a blocked `next_event` receives a stamp no earlier than its admission instant; time-domain exhaustion before dequeue leaves the Event queued, while successful selection stamps before dequeue and performs no fallible work afterward; `dispatch` admits the same Command exactly once when capacity remains, while a full inbox returns a typed Error with no handoff or inbox growth; fan-in and inbox occupancy never exceed configured capacity; and completion-entry and wakeup storage never grows beyond one entry per bound Slot. It further proves: a premature completion — or a pre-signal test-profile unwind — publishes a typed Error that wakes a blocked `next_event`; a completion during the wait ends the wait promptly under an injected clock; and, verifying `LIVE-LIFECYCLE` and the `LiveCtx` signal semantics, `recv` reports a raised signal ahead of queued Commands, a Port blocked in `recv` observes `Shutdown` within the window, and after the signal and a drained inbox every `try_recv` returns `Some(PortInput::Shutdown)`, with `None` only when no Command is pending and no signal is raised. |
| `VERIFY-SIM` | A Sim lifecycle and shutdown suite verifies `SIM-LIFECYCLE`, `SIM-START`, and `SIM-SHUTDOWN` with per-Port call traces: all-success startup and shutdown; startup failure at every Slot position; `Err` from `on_command` and `step` followed by shutdown; and `stop` returning `Ok` or `Err` at every Slot position. It checks that startup cleanup stops only the successfully started prefix, exactly once and in frozen Slot order; the failing and not-yet-started Ports receive no `stop`; an `Ended` Port receives no later method; shutdown stops exactly the then-`Open` Ports once in frozen Slot order while the latch remains open; every `stop` Error is published before the final close, the first wins, and later Errors do not prevent remaining calls; all-`Ok` shutdown returns `{ Quiesced, None }`; and a `stop` Error that is the latch's first publication returns `{ Quiesced, Some(error) }`. It also verifies `SIM-WAKEUP`, `SIM-SELECT`, `SIM-STEPS`, `SIM-COMPLETION`, and shipped `ENV-BOUNDS`: one fixed wakeup arm per Port never grows; a rejected before-`now` arm changes nothing; later `set_next` calls replace earlier arms and `clear_next` disarms them; selection follows frozen Slot order and the persistent cursor across calls, including equal-time ties and a selected `step(None)`; exact step-budget boundaries permit the configured number of calls, while exhaustion performs no selection, time advance, arm clearing, Port call, or storage growth; and no armed Port at entry or mid-selection returns the completion Error. |
| `VERIFY-LATCH` | An Environment conformance suite proves `ENV-LATCH`'s before-call and after-return ordering constraints; for a publication overlapping an observing call, it accepts either placement and verifies that the call's result and resulting latch state agree with it. It proves that an already-pending Error wins over an operation's own pre-commitment Error, reports the latch permanently, leaves the operation's contractual effect absent, and discards the secondary Error; for an overlapping publication and such a local failure, it exercises both permitted orderings. A `next_event` blocked without input returns and reports the Error that wakes it. The suite also proves permanent first-Error reporting, final-Command simulated Error observation, the latch remaining open through graceful shutdown, a typed shutdown Error before the final close, either consistent placement for a publication racing that close, and post-close discard. Stop-path integration proves `{ Quiesced, None }` alone can reach `Stopped`, any `Some(error)` produces `Environment(Shutdown)` even with `Incomplete`, and `{ Incomplete, None }` produces `Core(ShutdownIncomplete)`. |

## Appendix A. Invariant index

Navigation only.

| ID | Section |
|---|---|
| `ASSERT-INVARIANTS`, `BOUND-LOOPS` | Reading this document |
| `APP-CONTEXT`, `APP-EMIT`, `APP-OVERFLOW`, `APP-FUTURE`, `APP-STATE` | Application contract |
| `PORT-STATE`, `PORT-SUMS`, `PORT-ROUTING` | Port contract |
| `ENV-SERIAL`, `ENV-START`, `ENV-ERRORS`, `ENV-LATCH`, `ENV-TIME`, `ENV-SHUTDOWN`, `ENV-SEPARATION`, `ENV-BOUNDS` | Environment contract |
| `JRN-FORMAT`, `JRN-ENCODE`, `JRN-COMMIT`, `JRN-POISON`, `JRN-SINK` | Journal |
| `RUN-SERIAL`, `RUN-GRAMMAR`, `RUN-ENFORCEMENT`, `RUN-RECORDS`, `RUN-INDEX`, `RUN-CHECKPOINT`, `RUN-FINALIZE`, `DET-RUN`, `DET-ENV` | The Run |
| `LIVE-THREADS`, `LIVE-EVENTS`, `LIVE-SELECT`, `LIVE-TIME`, `LIVE-DISPATCH`, `LIVE-SUPERVISION`, `LIVE-COMPLETION`, `LIVE-LIFECYCLE`, `LIVE-START`, `LIVE-SHUTDOWN` | Live Environment |
| `SIM-STATE`, `SIM-LIFECYCLE`, `SIM-START`, `SIM-TIME`, `SIM-DISPATCH`, `SIM-WAKEUP`, `SIM-SELECT`, `SIM-STEPS`, `SIM-COMPLETION`, `SIM-SHUTDOWN` | Simulated Environment |
| A1–A9, `NO-UNSAFE`, `BOUND-STATIC`, `BOUND-NONZERO` | Laws |
| `CRATE-EXPORTS` | Crate layout |
| `VERIFY-CONTEXT`, `VERIFY-CONFORMANCE`, `VERIFY-JOURNAL`, `VERIFY-FAULTS`, `VERIFY-GRAMMAR`, `VERIFY-LIVE`, `VERIFY-SIM`, `VERIFY-LATCH` | Obligations & verification |
| `TRUST-PURE`, `TRUST-SIM-PORT`, `TRUST-ENV`, `TRUST-BLOCKING`, `TRUST-ABORT`, `TRUST-ROUTING`, `TRUST-KEY`, `TRUST-SERIALIZE`, `TRUST-LIFECYCLE`, `TRUST-DRAIN`, `TRUST-SPAWN`, `TRUST-EXIT`, `TRUST-SIZING`, `TRUST-INBOX`, `TRUST-SINK`, `TRUST-SHUTDOWN`, `TRUST-MEMORY` (trusted) | Obligations & verification |
