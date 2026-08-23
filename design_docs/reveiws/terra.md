# Adversarial Review: Kavod Core Design v12

**Scope:** Semantic review of `design_docs/design-v12.md` only. No implementation or other files were examined.

**Verdict:** The design is not semantically closed as written. The strongest issues are determinism, purity boundaries, simulated-environment conformance, and forensic-record semantics.

## Critical

### 1. A9 contradicts the defined Trace and public exit
**References:** `:125-130`, `:148`, `:594-612`, `:727`, `:745-746`

`Trace` explicitly erases Error values, retaining only their presence and position. A9 says every run output is a function of the trace, but `EngineExit` exposes the actual application, environment, and journal Error values.

Two runs can have identical traces where `next_event` returns an Error at the same position, but with different `E::Error` values. Their `EngineExit::Fatal` values differ. `DET-ENV` explicitly permits this outcome.

Either Error values must participate in trace equality, or A9 must exclude opaque Error payloads from "every run output."

## High

### 2. `initial_state` is outside the purity and authority boundary
**References:** `:31-35`, `:73-75`, `:224-243`, `:656-659`, `:1142`, `:1145`

A Handler is defined as only `on_start` or `on_event`; `initial_state` is neither. `TRUST-PURE` covers Handlers, State, Events, and Commands, but not the `Application` receiver or `initial_state`.

An application can satisfy the stated obligation while `initial_state` reads a clock, performs bounded I/O, or mutates interior state through `&self`. It runs inside `Engine::run`, before `RunStarted`, so this behavior has no record or typed failure channel.

The purity obligation needs to explicitly cover the `Application` object and `initial_state`.

### 3. `CommandsPrepared` cannot evidence complete command intent
**References:** `:718-723`, `:540-541`, `:1149`

`CommandsPrepared` is specified to evidence "the turn's complete Command intent." The Journal explicitly permits lossy serialization and says it evidences only emitted fields. `TRUST-SERIALIZE` requires determinism, not faithful or injective representation.

A command can serialize only its recipient while omitting an amount, order type, or business key. The Port receives the full typed command and may produce distinct external effects, while the Journal records are identical.

Define intent as serialized intent, or require serialized Commands to faithfully identify all semantically consequential fields.

### 4. Simulated shutdown violates immediate signal observability
**References:** `:452`, `:993-1004`, `:1031`

`ENV-SHUTDOWN` requires every Port to have a means to observe shutdown immediately from the shutdown instant. Sim defines the shutdown signal as calling `stop`, sequentially in Slot order.

`SimPort` has no lifecycle accessor or independent shutdown signal. A later Port cannot observe shutdown until earlier Ports' `stop` methods return. Bounded work is not immediate work.

Sim needs a shared observable shutdown state set before sequential cleanup, or the generic contract needs an explicit single-threaded exemption.

### 5. Sim has no specified Error mapping for `start` and `step`
**References:** `:344`, `:995-1003`, `:1024`, `:1028`, `:1081-1087`

`PORT-ROUTING` says sim maps each Slot's Port Error at the fan-out arm. That can map `on_command` failures, but `SimPort::start` and `SimPort::step` also return per-Port Errors. Those failures must become `Environment::start` and `Environment::next_event` Errors, respectively.

The only stated mapping site cannot cover those paths. Section 10 acknowledges Error-sum composition remains open, so these required outcomes are not yet semantically defined.

Specify one per-Slot Error mapper usable by all SimPort callbacks.

## Medium

### 6. `TRUST-PURE` contradicts Port ownership and assigns responsibility incorrectly
**References:** `:76`, `:188-190`, `:342`, `:912`, `:1022`, `:1142`

`TRUST-PURE` says "all run-varying data lives in State" and makes the Application author responsible. State is defined as application data, while Ports are required to own mutable domain, protocol, and native state.

Literal compliance forbids required Port state. It also assigns Port-sharing constraints to the Application author, despite other obligations recognizing separate Port and wiring authors.

Scope this rule to "run-varying application data" and assign Port-state obligations to the parties that control Ports and wiring.

### 7. The document violates its own backward-citation rule
**References:** `:43-48`, `:343-344`, `:558`, `:690-743`

The document requires citations to point backward, except for listed navigation cases. It has non-navigation forward dependencies, including:

- `PORT-SUMS` citing later `PORT-ROUTING`.
- The Run introduction citing later `RUN-GRAMMAR`.
- The `EffectsComplete` state citing later `RUN-CHECKPOINT`.

This weakens the claimed dependency-order discipline. Reorder the rules or explicitly define an exception for semantic forward references.

### 8. `ENV-ERRORS` requires implementation "binding rows" that do not exist
**References:** `:23-27`, `:449`, `:914-920`, `:1024-1028`

The reading rules define binding tables as the Environment commitment table and five Run tables. `ENV-ERRORS` requires each implementation to name commitment instants in a "binding row of its own section."

Live and Sim name those instants in ordinary guarantee rows, not designated binding tables. Under the document's own taxonomy, either the requirement is unsatisfied or "binding row" has an undefined broader meaning.

Use "guarantee row," or designate the Live and Sim guarantee tables as binding tables.

### 9. The claimed exact public API is not self-standing
**References:** `:16-20`, `:573-581`, `:879-900`, `:993-1010`, `:1072-1102`

The document says API blocks fix item names and type shapes exactly, but `Engine`, `LiveCtx`, and `SimCtx` are used without declarations anywhere in the document. `LiveCtx` is explicitly provisional, while `Engine` and `SimCtx` have no stated opaque type shape or construction semantics.

This is partly acknowledged by the open Wiring section, but it contradicts the top-level claim that the document stands alone as a complete contract.

### 10. `TRUST-ENV` cannot be verified by the stated trace suite alone
**References:** `:125-130`, `:447-454`, `:1144`, `:1161-1165`

`TRUST-ENV` says a bespoke Environment upholds every contract row and is verified by a conformance trace suite. The defined Trace contains operation results and sink-call results, not internal lifecycle state or external activity.

A trace cannot by itself establish facts such as no Port left mid-lifecycle after failed `start`, a single timestamp authority, or no externally consequential work after shutdown. The verification contract needs explicit probe instrumentation, white-box review, or a narrower certification claim.

## Excluded Concerns

I did not count these as findings because the document supplies a plausible reconciliation:

- Post-close shutdown errors are explicitly discarded by `ENV-LATCH`, A4, and the shutdown note.
- Dropping a certificate is intentionally a Fatal path, not a valid continuing graph path.
- Simulated synchronous `on_command` processing is compatible with A2 if "outside the turn" is read as an ownership boundary at handoff.
