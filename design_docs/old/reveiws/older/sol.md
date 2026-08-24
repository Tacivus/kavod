# Adversarial Semantic Review of Kavod Core Design v12

## Scope

This review considers only `design_docs/design-v12.md` as a self-contained specification. No implementation or other files were consulted. The explicitly open Wiring section was not treated as a defect merely for being incomplete.

## Verdict

The design is not yet semantically solid. The Run's state/edge graph is largely closed and coherent, but several cross-cutting axioms, lifecycle rules, determinism claims, and enforcement claims contradict the more detailed contracts.

## High-Severity Findings

### H-01: A4 defines two incompatible failure cutoffs

**References:** lines 143, 450, 708-711, 743-744.

A4 first says the Fatal cause is the first failure the Run **observes**, but then says cleanup begins once a failure **exists**.

Consider this execution:

1. A checkpoint returns `None`.
2. The Environment subsequently publishes Error `E`, leaving it pending and unobserved.
3. The `TurnCompleted(Continue)` Journal commit fails with `J`.

The first sentence of A4 and `RUN-FINALIZE` select `J`, because it was observed first. The second sentence says `E` existed first, making `J` a cleanup Error that must be discarded.

There is a second contradiction on the Stop path. A clean shutdown closes the latch before `TurnCompleted(Stop)` is committed. A4 says everything after a clean latch close is cleanup whose Errors are discarded, while the edge table classifies failure of that mandatory record as a new Journal Fatal.

**Repair:** Begin global cleanup only after the Run observes and fixes a Fatal cause. Scope post-close discarding specifically to Environment publications and shutdown work. State explicitly that mandatory Run records after a clean close can still produce the first Fatal cause.

### H-02: The commitment-point model is not total

**References:** lines 104-107, 142, 277, 431-450.

A commitment point is defined as the instant an operation's outcome becomes fixed, and A3 says every effectful operation has exactly one. Several normative rules do not fit that definition:

- Failed `start`, `next_event`, and `dispatch` calls never reach their listed activation, consumption, or handoff commitment, despite line 431 saying A3 applies to both outcomes.
- `ENV-LATCH` orders publication against an operation's commitment even when the operation returns its own precommitment Error and no successful-effect commitment exists.
- `shutdown` commits at "the call itself," but its `ShutdownReport` is not fixed then. Live quiescence depends on later thread completion through the deadline.
- `APP-STATE` explicitly says State mutation has no commitment point while also saying every mutation irrevocably stands.

These are not merely implementation choices. The central term has incompatible meanings: result linearization, successful effect transfer, and transactional commit.

**Repair:** Define a result-linearization point for every operation outcome. Separately define successful effect commitments such as activation, consumption, and handoff. Scope A3 to one of those concepts, and describe State writes as individually irrevocable rather than having no commitment semantics.

### H-03: A9 promises outputs that the Trace deliberately cannot determine

**References:** lines 125-130, 148, 594-633, 745-746.

Trace erases Error values, while `EngineExit` retains them. Two sink flushes can fail at the same position with different `std::io::Error` values. Their traces are equal because only failure presence is retained, but their `JournalFatal` exit values differ.

`DET-RUN` and `DET-ENV` already acknowledge this by weakening equality unless erased Error values correspond. A9 contains no such qualification.

**Repair:** Restrict A9 to the Core-owned output projection enumerated by `DET-RUN` and `DET-ENV`, or retain enough Error identity in Trace to determine the full exit.

### H-04: Sim Port lifecycle state is undefined

**References:** lines 1023-1024, 1031.

`SIM-START` and `SIM-SHUTDOWN` operate on Ports whose lifecycle is "open," but no binding rule defines:

- The initial lifecycle state.
- Whether lifecycle opens before or after successful `start`.
- Whether an unstarted Port is open.
- Whether successful `stop` ends the lifecycle.

For example, if A starts successfully, B returns `Err`, and C was never started, the cleanup rule does not determine whether C receives `stop`. Furthermore, `SIM-LIFECYCLE` says only an `Err` ends lifecycle, so a successful `stop` does not normatively close it even though shutdown reports `Quiesced`.

**Repair:** Define `NotStarted -> Open -> Ended`. Successful `start` enters `Open`; any method Error enters `Ended`; one `stop` call on an open Port ends it regardless of its result. Startup failure should stop exactly the successfully started prefix.

### H-05: Port-state isolation is simultaneously enforced and trusted to the wrong party

**References:** lines 57-62, 342, 912, 1022, 1142.

`PORT-STATE`, `LIVE-THREADS`, and `SIM-STATE` are classified as enforced guarantees of exclusive Port state ownership. Safe user Port types can nevertheless share globals, native handles, or `Arc<Mutex<_>>`.

`TRUST-PURE` separately says "Ports share no state," but assigns that obligation to the Application author and attempts to check it using a scripted Environment. The Application author may not own any Port implementation, and that test need not exercise Ports.

The same row says all run-varying data lives in Application State, directly conflicting with the guarantees that Ports own run-varying protocol and native state.

**Repair:** Add a dedicated Port-isolation obligation upheld by each Port author and the wiring author. Change the State clause to "all run-varying Application data lives in State."

### H-06: The supposedly complete Environment contract permits an empty Port set

**References:** lines 171, 390-393, 447-454, 573-580, 1144.

`BOUND-STATIC` globally requires a nonempty Port set. Section 5 says satisfying every Environment row is sufficient for conformance, while `TRUST-ENV` requires bespoke implementations to uphold only those rows.

A bespoke Environment with zero Ports can return a start timestamp and a clean shutdown report. An Application that returns `Stop` from `on_start` then completes normally. Every Environment row is satisfied vacuously, but `BOUND-STATIC` is violated. The generic Engine has no topology witness with which to reject it.

**Repair:** Move the nonempty topology constraint into the Environment contract, require a construction certificate visible to Engine, or expand `TRUST-ENV` to include all applicable global guarantees.

## Medium-Severity Findings

### M-01: The Initial certificate claims acceptance before acceptance occurs

**References:** lines 658-659, 703, 720, 740, 756-760.

The certificate is minted in phase `Initial` before `RunStarted` commits. Yet every certificate contains an "accepted count" and "last accepted logical time," while `RunStarted` is what accepts the start turn.

An `Initial` certificate therefore either falsely represents the start as accepted or stores prospective values contradicting its field meanings and `RUN-GRAMMAR`.

**Repair:** Use a preacceptance token containing only the Journal and pending start time. Create the indexed and timed certificate only after `RunStarted` commits.

### M-02: `RUN-GRAMMAR` overstates certificate provenance

**References:** lines 740, 769-783, 800-809, 819-823.

The design says requirements are never loose values, but `run_started(start_time)` accepts a loose timestamp and `accept_event(time, &event)` accepts a loose pair. The certificate owns neither the Environment nor a single-use result from `start` or `next_event`.

Consequently, typestate alone cannot prove that the recorded values are the exact returned values or that acquisition occurred only in `BetweenTurns`. These runtime call-site dependencies are absent from the stated residual proof boundary.

**Repair:** Have transitions perform the Environment calls, consume private `Started` and `Candidate` witnesses, or narrow `RUN-GRAMMAR` to record ordering and list provenance as asserted or tested.

### M-03: Shutdown completion has two conflicting linearization definitions

**References:** lines 408-412, 450-452, 917, 920, 951-954, 1031.

`LIVE-SUPERVISION` says `run` returning while lifecycle is `Running` is premature and must publish an Error. It also says classification occurs atomically under the latch lock.

A Port can return while `Running`, be descheduled before acquiring that lock, and then lose the lock race to shutdown. The first clause requires publication; lock-time classification treats it as expected. The two readings produce different shutdown reports and exits.

Sim also defines each sequential `stop` call as the shutdown signal after the latch has already closed, while `ENV-SHUTDOWN` speaks of one instant from which every Port can immediately observe the signal.

**Repair:** Define the completion event's linearization point as lock-time classification, not raw method return. Define one lifecycle cut that closes admission and the latch and raises signal state before any shutdown callbacks execute.

### M-04: Live completion notification is not a termination witness

**References:** lines 111, 920, 947-964.

A supervised thread necessarily sends its completion notification before the thread itself terminates. It can then be descheduled before return or thread-local teardown. Calling `join` based on that notification can block past the shutdown deadline.

The phrase "prompt by construction" does not establish a bound, and "completion publication" also conflicts with the glossary, where Publication exclusively means inserting an Error into the latch.

**Repair:** Treat completion notifications only as wakeups. Join only handles independently witnessed as terminated, and detach every unconfirmed handle at the deadline. State whether the deadline bounds the complete shutdown call or only its quiescence wait.

### M-05: The fixed transition set has no coherent normative status

**References:** lines 23-27, 405, 748-783, 810-812, 1098-1100.

Section 0 does not list the Enforcement transition table among the Run's binding tables, but the open Wiring constraints call its transition set fixed.

If the table is nonbinding, the claimed fixed set does not exist under the document's own authority rules. If it is binding, two transitions cannot perform their stated requirements:

- `dispatch_batch(env, &[C])` cannot move arbitrary non-`Clone` Commands into consuming `dispatch`.
- `no_commands()` receives no batch, while the shown certificate stores no batch, so it cannot assert that the handler's actual batch is empty.

**Repair:** Explicitly designate this as a binding table. Pass an owning/drainable batch to dispatch and the actual batch, or an asserted empty-batch witness, to `no_commands`.

### M-06: The turn has two different normative endpoints

**References:** lines 83-84, 141, 693, 708-711, 725, 743.

A2 says the turn ends at handoff. The binding graph performs a checkpoint and commits `TurnCompleted` afterward. On Stop, it additionally commits `StopRequested`, executes shutdown, and only then commits `TurnCompleted(Stop)`. Empty-batch turns have no handoff at all.

**Repair:** Say that the Command-delivery phase ends at the final handoff, while the turn ends when `TurnCompleted` commits.

### M-07: Sim wakeup arms lack a binding initial state

**References:** lines 1027, 1035-1036, 1065-1066.

`SIM-WAKEUP` says each Port has at most one arm but never says arms begin disarmed. The derivation that a Port set which never arms immediately reaches `SIM-COMPLETION` assumes that missing rule.

An initially disarmed arm and an initially armed-at-origin arm both satisfy "at most one," but produce different first `next_event` outcomes.

**Repair:** Add to `SIM-START` or `SIM-WAKEUP` that every arm is initially disarmed before the first Port `start` call.

### M-08: Exact API blocks omit required type declarations

**References:** lines 16-20, 320-326, 368-374, 573-582, 993-1015.

The exact Run API provides an inherent `impl` for `Engine<A,E,W>` without declaring `Engine`. The nonprovisional Sim API similarly implements `SimCtx` without declaring it.

`Never` is declared without `Serialize`, even though absent directions require it to satisfy `PortContract`. Its implementation appears only in Mechanism prose, which is nonbinding under Section 0.

**Repair:** Add opaque public declarations for `Engine` and `SimCtx`. Put `Never`'s `Serialize` implementation and required macro expansion semantics in an API block or guarantee row.

### M-09: Error destructors are outside the trust boundary

**References:** lines 143, 228, 401, 688, 744, 1024-1031, 1142-1145.

Application and Environment Error types are unconstrained. The design deliberately discards Error values during overflow precedence, latch replacement, shutdown cleanup, and finalization, invoking their destructors.

`TRUST-PURE` covers `Drop` for State, Events, and Commands, but not Errors. `BOUND-BLOCKING` covers boundedness and panic behavior, not hidden authority or side effects. An Error destructor can therefore perform unrecorded work and affect determinism or cleanup semantics.

**Repair:** Add an obligation for every Error type and its destructor, or explicitly classify and trace destructor effects as subordinate cleanup effects.

### M-10: The specified Journal mechanism does not ensure JSON Lines framing

**References:** lines 511-512, 519-527, 1149.

`JRN-FORMAT` promises one JSON object per line. The mechanism verifies only that encoded bytes begin with `{` and end with `}`. A valid raw JSON fragment can contain literal line breaks as insignificant whitespace while still passing that test.

Conversely, a valid object with leading or trailing JSON whitespace is semantically an object but receives `NotAnObject`.

**Repair:** Reject literal CR and LF bytes inside the encoded object and define the canonical whitespace policy, or detect the top-level serializer form structurally rather than inspecting only the first and last bytes.

### M-11: Journal-based abort reconciliation does not guarantee that keys were recorded

**References:** lines 540-541, 722, 854-856, 1148-1149.

Lossy serialization is explicitly permitted. `TRUST-KEY` requires an externally consequential Command to carry a business key, but does not require the serialized Command to emit it.

After an abort there is no exit and the batch is gone. If serialization omitted the key, `CommandsPrepared` cannot support the promised reconciliation on business keys.

**Repair:** Require every externally consequential Command's serialized representation to include its stable key and routing identity, or define an independent durable key registry.

### M-12: Two bounds obligations are unsatisfiable or overstated

**References:** lines 168, 171, 230, 975-977, 1145, 1152.

`BOUND-BLOCKING` requires `initial_state` and destructors to report Errors instead of panicking, but neither has a typed Error return channel.

`BOUND-STATIC` says Slot registration fixes "the thread count," while `TRUST-SPAWN` explicitly allows Ports to create threads, timers, and callbacks.

**Repair:** Require infallible APIs and destructors to be bounded, nonpanicking, and nonfailing; require typed Errors only where a channel exists. Scope `BOUND-STATIC` to Kavod-supervised Port threads.

## Low-Severity Findings

| ID | References | Finding | Repair |
|---|---|---|---|
| L-01 | lines 78-88, 210, 257-260, 720, 742 | `Accepted` is defined only for candidates with `EventAccepted`, but the start turn is repeatedly called accepted. `RUN-INDEX` also calls index 0 an accepted count even though one turn has been accepted. | Define accepted turns through either acceptance record and call the index an ordinal or count of accepted External Events. |
| L-02 | lines 415-420, 594-600, 744, 746 | `DET-ENV` lists the shutdown report's Error presence among equal exit payloads, but `EngineExit` contains no report or Error-presence field. | Move this item to the trace-equality premise. |
| L-03 | lines 146, 224-228, 398-401 | A7 says text and bytes exist only at serialization, but Error, Event, and Command types may themselves be `String`, `Vec<u8>`, or contain them. | Say Kavod transports typed values opaquely and does not render Errors before the edge. |
| L-04 | lines 543-546 | A named-field struct need not serialize as an object when it has `serde(transparent)` or a custom `Serialize` implementation. | Limit the derivation to default-derived named-field structs without representation-changing attributes. |
| L-05 | lines 43-48, 343-344, 437-450, 690-743, 740-815 | The document repeatedly violates its own backward-citation and ID-only rules, including forward row references and `RUN-GRAMMAR` pointing to later nonbinding Enforcement prose. | Reorder dependencies or explicitly exempt within-table references and named binding tables. |
| L-06 | lines 154-158, 513-515, 727-733, 970-973 | Committed records are described as post-abort evidence even though sink durability and retention are explicitly excluded. The Journal also does not contain Fatal cause or quiescence. | Qualify this as logical evidence if retained by the sink, and require the rendered `EngineExit` to evidence Fatal cause and quiescence. |

## Areas That Held Up

- The Run's state and edge tables form a closed exit graph once the cross-cutting A4 and commitment issues are set aside.
- Fatal finalization does not double-call shutdown and correctly reuses a consumed Stop-path report.
- Dispatch prefix semantics, index exhaustion, time-regression handling, and consumed-but-unaccepted candidates are internally coherent.
- Journal write, flush, poison, and uncertain-suffix semantics are otherwise consistent.
- Sim event selection is deterministic once lifecycle and initial-arm state are defined.
- Explicitly unresolved Wiring choices were not counted as defects merely because they remain open.
