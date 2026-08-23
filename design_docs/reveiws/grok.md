# Adversarial review of `design_docs/design-v12.md` (v12 semantics only)

**Scope.** The document in a vacuum. No implementation, no v11, no other files.
**Method.** Six independent attacks (axioms, latch/fatal/graph, live↔contract, sim↔contract, journal/trace/determinism, meta/binding-forms), then a pass that dropped anything that does not actually break the text.

**Verdict.** Not solid. The Run graph, latch state machine, and most ENV/SIM commitment stories compose. The live happy-path Stop, A4’s two sentences, and A2’s “turn ends at handoff” do not.

---

## CRITICAL

### C1. Live clean Stop cannot witness `Quiesced` as written

`Publication` is defined as entry of an Error into the latch.

`LIVE-SUPERVISION` publishes only `run(Err)` and completion **while `Running`**. Expected completion (after `Running` has ended) **stays unpublished** (A4).

`LIVE-SHUTDOWN` ends `Running` in the **same** instant as the latch close, then waits for **“completion publications”**, joining **“publishers”** (`publication follows the Port’s last work, destructors included`).

On a normal Stop the Port threads are still inside `run` when that instant fires. Their completions are therefore expected and **must not** latch-publish. They are not publishers. The wait then only ends at the deadline → detach → `Incomplete`.

`StopPending` + the clean-report edge then cannot reach `Stopped` on Live: `Error None` + `Incomplete` is `Core(ShutdownIncomplete)`.

“Completion tracking” in the Live mechanism is not a binding form (section 0: a rule in none of the four forms does not exist). Under the binding vocabulary, the only named completion signal is Publication, and the expected path is forbidden from using it.

This is not a wording nit. It takes down Live `Stopped` / `Quiesced` on the success path.

---

## HIGH

### H1. A2 vs the rest of the document: when does a turn end?

A2: “The turn ends at handoff — a destination Port’s processing of Commands already handed off runs outside it.”

That cannot be true together with:

- Glossary **Turn**: “one accepted Event (or the start), one handler call, one batch”
- `RUN-CHECKPOINT`: snapshot is **after** last handoff, still this turn
- Records: `TurnCompleted` is “**End of every non-Fatal turn**”
- Stop path: `StopRequested` → `shutdown` → `TurnCompleted(Stop)` still carry that turn’s `index`

Handoff, checkpoint, and “end of turn” are three instants. The dash clause is the real rule (Port processing is outside the serial loop). “Ends at handoff” is leftover clothes and, if taken as an axiom, falsifies the graph.

Empty-batch turns have **no** handoff. A2’s end condition is then undefined; the graph still checkpoints and completes.

Sim makes it worse: `SIM-DISPATCH` *is* `on_command`. That processing runs inside `dispatch`, inside `Prepared`, before `CommandsDispatched` — inside the turn everywhere except A2’s second sentence.

### H2. A4 contradicts itself, then contradicts the tables

A4 sentence 1 is **observation**: the first Error or Core condition the run **observes** is the cause.
A4 sentence 2 is **existence**: once a first Error or Core condition **exists** — or, on a run that ends without one, once the latch has **closed** — everything after is cleanup whose Errors are discarded.

Those are not the same.

**Existence without observation.** Continue, last checkpoint `None`, a Port publishes, then:

- `Core(IndexExhausted)` (`next_event` uncalled), or
- `Core(TimeRegression)`, or
- `EventAccepted` Journal fail, or
- `TurnOpen` `Core(CommandBoundExceeded)` while a Port published during the handler (`RUN-CHECKPOINT`: a turn that goes Fatal earlier takes no snapshot)

An Error already exists. Sentence 2 says the later Core/Journal condition is cleanup. Sentence 1 and `RUN-FINALIZE` make it the cause and discard the latch Error at close.

**Cleanup after a clean close.** Checkpoint `None` → `StopRequested` → `shutdown` closes an empty latch (`Quiesced`, `error: None`). No cause exists. `close()` then commits `TurnCompleted(Stop)`. That commit fails → `Journal(TurnCompleted)` Fatal, report reused.

Sentence 2, latch-closed clause: discard `J`, end without a cause → `Stopped`.
Edge table + `RUN-FINALIZE`: `J` is the cause → `Fatal`.

Environment Notes try to scope A4 to “shutdown work after the close.” That scope is **not in A4**.

### H3. A1 is false of every capability the design actually uses

A1: every appearance outside the owner is a **read-only view**.

Not true of:

- `Context::emit` (mutates the Run-owned batch) — and `APP-CONTEXT` cites A1 for this
- `SimCtx::{set_next,clear_next}` (mutate Environment-held arms)
- `LiveCtx::offer` (mutates Environment-owned fan-in)

Two further ownership collisions:

- **Arms:** `SIM-WAKEUP` says modifiable **only** through that Port’s `SimCtx`. `SIM-SELECT` clears the arm before `step`. Two mutators.
- **Inboxes:** `LIVE-DISPATCH` says the destination Port **owns** the inbox; `dispatch` commits by admitting into it. `PORT-STATE` says the Port exclusively owns its mutable domain. Environment write + Port `recv` is two writers of one “exclusive” container.

A1 as stated is not the design. The design is “one owner, others hold capabilities.” The axiom was not updated.

### H4. Live `next_event` / `dispatch` sequences are not `ENV-LATCH`-safe

`ENV-LATCH`: a publication linearized **before** `next_event`/`dispatch` commitment is taken and returned as that call’s `Err`; nothing consumed / nothing handed off.

`LIVE-SELECT`: stamp **then** dequeue; dequeue is consumption; nothing fallible follows. Mechanism: wait → take pending latch **or** stamp (fallible) → dequeue. No latch observation at the commitment instant.

Window: Event available, latch empty, stamp succeeds, Port publishes, dequeue consumes. That publication precedes commitment and must fail the call with **no** consume. The written sequence consumes.

Same window on `dispatch`: latch check → fan-out → admit.

Stamp-before-dequeue is not a second commitment. It is fallible work **between** latch observation and commitment. `LIVE-SELECT` / `LIVE-DISPATCH` are jointly satisfiable with `ENV-LATCH` only if stamp/fan-out + commit share a critical section with the latch. That is not stated. The sequence text is not a realization of the contract it cites.

### H5. `Quiesced` claims a witness Live cannot produce

Shutdown commitment: `Quiesced` witnesses **every** unit of run-scoped activity completed.

Glossary: that set is Environment threads, timers, callbacks, **and whatever Ports started** (`TRUST-SPAWN`).

`LIVE-SHUTDOWN`: `Quiesced` **exactly when every supervised thread was joined**.

`TRUST-SPAWN`: Port-started work is “otherwise **unwitnessable**.” A trusted row cannot discharge a binding-table witness. Live can report `Quiesced` while Port children still run.

Notes then overclaim: `Quiesced` ⇒ “every Port finished entirely, destructors included” and “terminal Port state is readable” through pre-bound handles. False if `TRUST-SPAWN` is broken; Live cannot tell.

`SIM-SHUTDOWN` always reports `Quiesced` — same witness inflation, cheaper because there are no threads.

### H6. The byte-equal sim replay *Derive* is false

Three stated preconditions (origin = `RunStarted` time; replay Port arms each recorded stamp in order and answers `step` with the recorded Event; budget covers every acquisition) are not sufficient:

1. **One arm, last-call-wins.** A `start()` loop of `set_next(stamp_i)` keeps only the last stamp. Sequential re-arm after each `step(Some)` is required and unstated.
2. **Competing arms / cursor.** Any other arm at a ≤ recorded time is selected first. Cursor is Environment state, not in the Journal and not in the accepted `(Event, Timestamp)` sequence. Equal-time winners then differ.
3. **`on_command` mutates arms.** Stated elsewhere; the recipe never says playback `on_command` must be a no-op or restore the next stamp.
4. **Journal ≠ trace.** A consumed-but-unaccepted tail (`TimeRegression` / `EventAccepted` commit fail) is in the trace, not in EventAccepted records. `DET-RUN` is a function of the **trace**. Playback from Journal stamps can hit `SIM-COMPLETION` instead of the original Fatal.

A *Derive* that does not follow from the rules is a spec bug.

### H7. A3 is universal; the document already has exceptions

A3: every effectful operation has **exactly one** commitment point.

`APP-STATE`: State mutation has **no** commitment point. Direct exception, never reflected in A3.

`shutdown`’s commitment row names two instants: “the call itself” **and** the close within it. `LIVE-SHUTDOWN` then does more work after the “one linearized instant” (wait, join, detach).

“A transition *is* a commit” vs `dispatch_batch`: one transition performs `CommandsPrepared`, N handoffs, then `CommandsDispatched`. Fine if A3 is per Environment/Journal operation; the Run prose pretends otherwise.

---

## MEDIUM

### M1. A8 / `NO-UNWIND` is an axiom that is actually a trusted obligation

A8: “the process **aborts**, and no exit represents it.”
Laws: the abort profile is `TRUST-ABORT`.
`NO-UNWIND` is an **enforced** Laws ID that ends “trusted (`TRUST-ABORT`).”

An axiom cannot be a deployment fact. Test profile unwinds; Live Notes then define `Quiesced` as “joined”, never “succeeded.” A8’s abort/no-exit story is not the specified test semantics.

Same shape: section 0 says every ID outside Obligations is **enforced**, but `PORT-ROUTING` / `PORT-SUMS` → `TRUST-ROUTING`, `JRN-SINK` → `TRUST-SINK`, `NO-UNWIND` → `TRUST-ABORT`.

### M2. Dead-Port arms vs `SIM-LIFECYCLE`

Nothing clears or ignores a wakeup when a Port dies. `SIM-SELECT` filters **armed**, not live. `stop()` has no `SimCtx`. `on_command`/`start`/`step` can `set_next` then `Err`, leaving an arm on a corpse. Selecting it calls `step` → forbidden further method.

Under Engine + latch-first this is likely unreachable (`step(Err)` is this `next_event`’s `Err`; `on_command(Err)` is pending before the next select). **That unreachability is not written.** Implementers get no rule: clear on death / skip dead / “unreachable, do not care.”

### M3. Equal-time cursor is not one algorithm

`SIM-SELECT` says lowest time; equals by round-robin; cursor starts at Slot 0; persists; moves to the selected Slot’s successor after every selected `step`, including `None`.

Missing: how the cursor **chooses** among min-time Ports (scan-from-cursor-and-wrap is the usual reading, not written); successor of the last Slot (wrap implied, not written).

Two shipped-sim implementations can be internally deterministic and still disagree on equal-time winners → different Events → different traces. A9 / `SIM-SELECT` as a binding row needs one function.

### M4. Sim `next_event` latch path is citation-only

`LIVE-SELECT` + live Mechanism: pending latch is taken, marked reported, returned; nothing consumed.

`SIM-SELECT`: “checks, in order … the latch (`ENV-LATCH`)” — not take/report/return. Sim Mechanism specifies pending-latch-first on **`dispatch`** only.

`ENV-LATCH` still binds. An implementer of §9 Mechanism alone can skip it — and then M2 becomes reachable.

### M5. `LiveCtx` drop can make inbox `Closed` the first observed cause

Shell: invoke `run(self, ctx)`, then classify/publish. `run` returning drops `LiveCtx` **before** the latch publish. That drop ends the SPSC receiver.

`LIVE-SHUTDOWN` closes fan-in, **not** Command inboxes. Inbox `Closed` is therefore receiver drop, which races any later `dispatch` (still allowed: premature is not yet observed; `ENV-SERIAL` still permits `dispatch`).

`ENV-LATCH`: a pre-commitment `dispatch` `Err` is **not** an observation point; the concurrent premature Error stays pending and is discarded at finalize. First observed cause can be inbox `Closed`, not premature closure.

Section 10’s planned Live Error sum lists dispatch **exhaustion**, spawn failure, time exhaustion, premature closure — **not** inbox `Closed`.

### M6. A9 overclaims; `DET-ENV` names an exit field the exit does not carry

A9: every run output is a function of build, Application, initial State, configuration, and the trace.

The glossary **erases Error values**. `DET-RUN` then weakens A9: Core-owned bits follow the trace; full `EngineExit` equality needs the erased values to “also correspond.” Physical sink bytes after a failed write/flush are a run output and are explicitly **not** determined (`JRN-COMMIT` uncertain suffix; trace keeps only failure *presence*). `DET-RUN` is the real rule; A9’s second clause is false as written.

`DET-ENV` compares exits on “the report’s Error presence.” `EngineExit` has no such field. `RUN-FINALIZE` **discards** the report’s Error on the normal Fatal path. Presence lives in the trace’s `ShutdownReport`, except implied on `Stopped` / `Environment(Shutdown)` / `ShutdownIncomplete`.

### M7. Checkpoint “brackets no effect” is false

Run Notes *Derive*: empty-batch and checkpoint edges “bracket no effect — nothing was prepared, nothing handed off, nothing observed but the latch.”

`take_error` is an effectful operation with a commitment point: `Some` **marks the latch reported forever**. The recordless edge is a design choice (keep Journal Environment-independent). The derive is wrong, and it is what people will use to argue about A5 placement.

Same landmine: phase name `EffectsComplete` + A5 “completion record witnesses effects already committed” reads as Port-effect completion. It is handoff + latch snapshot. Live Commands can still be executing after `TurnCompleted(Continue)`. Internally consistent if “effect” = handoff; the names fight A2.

### M8. Status / four-form machine / `Never`

- “One section is open: Wiring” is false. `LiveCtx` signatures are provisional; `PORT-ROUTING` defers the Error sum “placed finally when Wiring closes”; `BOUND-STATIC` nonempty has no construction API yet.
- A1–A9 are normative and sit in **none** of the four binding forms. Either they are a silent fifth form, or they “do not exist.” “Everything in this document is a consequence of nine axioms” is also false (JSONL, this graph, this checkpoint, `u64` as the run bound, live one-thread-per-Port, sim round-robin are choices).
- `PortContract` requires `Event: Serialize` and `Command: Serialize`. `Never` has no impls in the API block. The only `Serialize` impl is Mechanism (`match *self {}`). Mechanism cannot create the obligation the type’s purpose needs.
- Section 0’s binding-table list omits the certificate transition table. `RUN-GRAMMAR` incorporates “work its own transition performs.” If that table is prose, the unrepresentability claim has no binding work list.

### M9. Forensic prefix claim does not cover Journal-after-full-handoff

“`CommandsPrepared` plus the typed `Dispatch { position }` identify the exact handed-off prefix.”

If `CommandsDispatched` **commit** fails, every Command was handed off, there is no `position`, and the witness record does not exist. The cause is `JournalFatal { record_kind: CommandsDispatched }`, which implies full handoff if you already know the graph. The sentence as written does not.

After abort, the Notes correctly say the uncertainty is the whole prepared batch (`TRUST-KEY`). That path is fine.

---

## LOW

- **`BOUND-SIZING` off-by-one.** Glossary / `RUN-RECORDS`: a record is the JSON object. `JRN-FORMAT`: object **plus** newline; `max_record_bytes` bounds the object. `BOUND-SIZING` says `max_record_bytes` must fit “the largest record.”
- **`fail` / `failure`.** Glossary: “returned an Error; no further meaning.” A4’s title and the graph use it for Core conditions too.
- **`BOUND-*` prefix collision.** `BOUND-BLOCKING`, `BOUND-SIZING`, `BOUND-INBOX` are trusted obligation rows next to enforced `BOUND-LOOPS` / `BOUND-STATIC` / `BOUND-NONZERO`.
- **A6 vs `remaining()`.** “capacity minus length” — unchecked sub, no `checked_` rule.
- **`TRUST-MEMORY`** is vacuous (“Owner-defined”).
- **`PORT-ROUTING`** names “sim: the fan-out arm; live: supervision” inside a Core guarantee. Placement rules forbid this (exemption list is Scope, ENV pointer, bounds registry).
- **u64 timestamps as JSON numbers.** Live nanos are past 2⁵³. Byte-determinism under `serde_json` still holds; any f64 consumer loses them. Forensic footgun, not a contradiction.
- **Finite-source + `Continue`.** Next `next_event` is `SIM-COMPLETION` Fatal. Specified, easy to misuse.
- **Enforcement residue list** names three runtime points and omits that `accept_event(time, &event)` is Engine-supplied from the just-consumed candidate (representable mismatch, one call site).

---

## Attacked and solid (do not “fix”)

- Latch SM: `empty → pending → reported|closed`; one-shot first Error; `already reported` ⇒ `report.error` is `None`; observer already has the cause.
- `ENV-LATCH` Rule B + finalize discard of a concurrent unpublished Error: A4-as-observation, not a reversal — **once H2’s wording is fixed to observation**.
- Waiting `next_event` liveness vs take-and-return (Live).
- `start` `Err`: latch never closed; `ENV-SERIAL` forbids `shutdown`; hardcoded `Quiesced`. Close is final only when `shutdown` runs.
- Every named Fatal path maps to exactly one of `RUN-FINALIZE`’s three quiescence arms. `StopPending` vs case 1 mutually exclusive (`shutdown` consumes).
- Graph: eight states reachable; Fatal is drop-cert, not an edge; recordless edges cannot fail; `dispatch_batch` failure outcomes match `Prepared`; `u64::MAX` accept then `IndexExhausted` or Stop; start-turn / zero-event Stop valid.
- Live Stop window (checkpoint `None`, Port publishes before close) → `Environment(Shutdown)` is A4-as-observation and an admitted `DET-ENV` non-overlap with sim’s structural `None`.
- `SIM-START` / `SIM-DISPATCH` / `step(Err)` vs `on_command(Err)` commitment split: asymmetric and consistent.
- Failed sim `next_event` that advanced `now`: no later stamp (`ENV-SERIAL`); `ENV-TIME` is about stamped returns.
- A5 vs activation/consumption: records announce **acceptance** / intent, not `start()` / consume. Empty Journal + real effects is derived, not a clash.
- Schema version only on `RunStarted`; encode failures do not poison; Interrupted not retried; no public `EventIndex` constructor; `Stopped` ⇒ clean report is one-way; `CoreError` covers every Core Fatal path.
- Slot order / live time origin / nonempty Port set: construction, not closed-runtime holes. `DET-ENV` non-overlap for live-only / sim-only failure shapes is admitted.
