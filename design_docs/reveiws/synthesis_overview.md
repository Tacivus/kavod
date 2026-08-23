# v12 Synthesis Overview

## Scope and method

This overview evaluates every finding in Tiers 1 through 4 of `synthesis.md`
against `design-v12.md`. No other document was used.

The design is fundamentally sound. The run graph, first-observed-failure rule,
Journal commit model, shutdown report matrix, and deterministic-Core boundary do
not need redesign. Most findings come from five shared seams:

1. Some real rules are written in forms that section 0 says are non-binding.
2. The certificate mechanism has a few missing assignments and type-level details.
3. Error publication, thread completion, shutdown timing, and lifecycle terms need
   distinct names and exact ordering rules.
4. Several axioms and evidence claims are broader than the detailed rules they
   summarize.
5. The deliberately open Wiring section has not yet assigned all types, bounds,
   variants, and ordering authorities.

The ranked solutions under each finding are ordered from the best balance of
simplicity and robustness to more structural alternatives. The first solution in
each finding is the recommended one. Those first choices form one consistent fix
set. Lower choices are alternatives unless explicitly called cumulative; they are
not instructions to adopt conflicting semantics.

Validation labels mean:

- **Real defect:** implementation or conformance can actually diverge.
- **Real specification gap:** the intended behavior is coherent, but the binding
  text does not force it.
- **Real open work:** valid issue, already inside section 10's declared scope.
- **Judgment call:** the current design is coherent; changing it is optional.
- **Overstated kernel:** a smaller issue is real, but the severe claim is not.

## Recommended Wide Fixes

These are the large hammers. They solve most findings without changing the design's
architecture.

### A. Promote existing authority instead of duplicating it

Amend section 0 once so that the certificate transition table and Journal `commit`
step table are binding tables. Permit API blocks to prohibit derives. Add one Laws
guarantee for always-on assertions, and give the verification suites stable IDs.
This resolves Tier 1 item 5 and most of Tier 2's meta-rule batch without scattering
copies of the same rule.

### B. Use one Environment ordering and lifecycle vocabulary

Reserve **publication** for Errors entering the latch. Call normal thread signals
**completion notices**. Define latch linearization as implementation ordering whose
choice is witnessed by the operation result. Give shutdown one absolute deadline
budget rather than one budget per wait. Define Sim lifecycle as
`NotStarted -> Open -> Ended`. This resolves Tier 1 items 2, 3, 4, and 9 together.

### C. Make the existing transition proof executable

Keep the graph and records unchanged. Bind the certificate's initial `index` and
`last_time`, update both only on successful acceptance, retain the clean shutdown
report across a failed final commit, pass a drainable owned batch to dispatch, pass
a batch view to the empty edge, and represent checkpoint outcomes with two concrete
typed successors. This resolves Tier 1 items 1 and 6 as one mechanism repair.

### D. Narrow summaries to the rules they summarize

Scope A1 to ownership plus capabilities, A2 to serial turn execution, A3 to
Core-owned effectful operations, A4 to observed failures, and A9 to Core-owned run
outputs under the trust boundary. Apply the same discipline to replay and forensic
claims. This resolves Tier 1 items 7 and 8 and much of Tier 2.

### E. Close Wiring with one authority matrix

Use Slot declaration order as the sole ordering authority. In one binding Wiring
table, declare each public/opaque type, every configured nonzero bound, each Error
variant and mapping site, and the owner/test for every sizing obligation. This
resolves nearly all of Tier 3 without changing any runtime semantics.

### F. Harden record framing at the Journal boundary

After encoding, reject raw CR or LF bytes before appending the one Journal newline,
and also place the no-raw-newline rule in `TRUST-SERIALIZE`. Correct the serde
newtype note and state that evidence means serialized evidence. This closes the
actual JSONL hole and aligns the evidence language without changing normal records.

---

## Tier 1 - Fix Before Implementation

### 1. Certificate time bookkeeping is unwritten

**Validation: Real defect.** `RUN-GRAMMAR` says the certificate owns the accepted
index and last accepted time. The startup table only says to mint an `Initial`
certificate, `run_started(start_time)` lists no work, and only the successful next
index is explicitly installed by `accept_event`. Nothing binding installs the start
time or later accepted times into `last_time`. A conforming implementation therefore
cannot derive `Context::logical_time()` from the stated exhaustive tables. The clean
shutdown report also has to survive a failed `TurnCompleted(Stop)` commit, but the
mechanism does not say where it is retained. The `Initial` field meanings are also
unclear before `RunStarted` commits.

**Plainest version:** The certificate has a clock field, but the instructions never
say to put a time into it or update it. They also say to reuse a shutdown receipt
after a write failure without saying where the receipt was kept.

**Fifth-grader version:** The certificate is the run's scorecard. It must remember
the current turn number and time. The design describes the scorecard, but omits two
of the pencil marks needed to keep it accurate.

**Solutions, simplest and strongest first:**

1. **Bind all three assignments in the existing startup/transition tables.** Mint
   `Initial` with prospective index 0 and the frozen start time; say those values
   become accepted only when `RunStarted` commits. On successful `accept_event`,
   install both the derived next index and checked time. In `close`, retain the clean
   report before attempting `TurnCompleted(Stop)`, so a commit failure carries it to
   `RUN-FINALIZE`. This is the smallest complete fix.
2. Give `Initial` separate fields named `start_time` and no accepted-time claim,
   then let `run_started` construct the first ordinary certificate. This makes the
   pre-commit meaning clearer but adds a phase-specific representation.
3. Split certificate data into a shared Journal owner plus phase-specific metadata
   structs. This is strongest at the type level but adds machinery without changing
   behavior and is unnecessary for v12.

### 2. `ENV-LATCH` leaves "linearized" undefined

**Validation: Real specification gap; the blocker claim is false.** The commitment
table already says outcomes, not wall-clock instants, bind behavior, so a coherent
implementation exists. The actual gap is that `ENV-LATCH` never defines how a
concurrent publication is ordered against a call. The current phrase saying a prior
publication is returned as the operation's `Err` is also wrong for `take_error` and
`shutdown`, which have different result channels. `SIM-DISPATCH` omits the latch-first
return already required by its mechanism.

**Plainest version:** The design knows how races should end, but does not clearly say
who gets to call a photo finish or how the result tells us who won.

**Fifth-grader version:** An Error and an Engine operation can happen at almost the
same time. The implementation may choose their order while they overlap, but it must
make one choice and return a result that proves that choice.

**Solutions, simplest and strongest first:**

1. Add one ordering definition to `ENV-LATCH`: a publication completed before the
   call is ordered before its commitment; one begun after return is ordered after;
   a concurrent publication may be placed on either side; the operation result is
   the witness. Replace "returned as that operation's `Err`" with "surfaced through
   the result channel in that operation's commitment row." Add the pending-latch
   carve-out to `SIM-DISPATCH`. No mechanism changes.
2. Add a tiny latch outcome table covering `next_event`, `dispatch`, `take_error`,
   and `shutdown`. This is more verbose but makes every result channel explicit.
3. Require each shipped Environment's guarantee row to state its concrete lock/order
   realization and test it with scripted race schedules. This is more repetitive than
   the one contract definition, but keeps all ordering internal.

### 3. "Publication" collides with completion tracking

**Validation: Real defect under the document's own definitions.** The Glossary says
Publication means an Error entering the latch. `LIVE-SUPERVISION` says expected clean
completions stay unpublished, while `LIVE-SHUTDOWN` waits for completion publications.
A strict implementation would wait for signals that clean threads are forbidden to
send. The likely implementation intent is obvious, but section 0 makes vocabulary
definitions binding, so this cannot be dismissed as style.

**Plainest version:** One word is being used for both "an Error happened" and "a
thread finished normally." Those are different messages.

**Fifth-grader version:** The design has a red alarm and a normal "finished" bell,
but calls both of them the red alarm. Shutdown needs to wait for the normal bells.

**Solutions, simplest and strongest first:**

1. Reserve **publication** for latch Errors and rename normal signals **completion
   notices** everywhere. Make `LIVE-SHUTDOWN` wait for completion notices. Scope
   "while `Running`" over both `run(Err)` and clean `run` completion, and state that
   classification is fixed under the same lock as the `Running` transition/latch
   close. This is a one-vocabulary repair with no semantic change.
2. Rename both concepts more explicitly to `error publication` and `thread completion
   notice`. This is slightly wordier but even harder to misread.
3. Model completion as a second typed state machine beside the Error latch. This can
   improve implementation proofs, but the existing completion tracker is enough once
   its signal has a distinct name.

### 4. Live shutdown deadline semantics are under-written

**Validation: Real specification gap; the infinite-hang claim is overstated.** The
current mechanism clearly applies a deadline to waiting for completion signals, but
does not say whether waits share one deadline or each receive a fresh duration. It
also lists serialized joins after that wait and claims "prompt by construction." A
thread that violates `BOUND-BLOCKING` is outside the guarantee, so an unconditional
hang is not a valid attack. However, the timing claim is still too broad and Slot-order
joins can consume an unspecified amount of additional time.

**Plainest version:** Shutdown has a timer, but the rules do not say whether everyone
shares it or each person gets a new timer. They also promise faster return than the
mechanism can honestly prove.

**Fifth-grader version:** If cleanup has ten workers, there should be one finish time
for the whole class, not ten full timers used one after another. Joining a worker is
safe only after the worker has really finished.

**Solutions, simplest and strongest first:**

1. Define one absolute deadline computed at shutdown entry and shared by every timed
   wait. Completion notices wake the tracker; joins follow witnessed completion and
   never reset the budget. At the deadline, detach every unfinished thread. Replace
   "prompt by construction" with the honest claim: waiting is bounded structurally;
   post-notice teardown/join completion remains under `BOUND-BLOCKING` and is not a
   hard real-time guarantee.
2. For a stricter wall-clock policy, join only handles for which `is_finished()` is
   already true; recheck against the one deadline and detach the rest. This removes
   trust from join blocking but needs a bounded wake/recheck mechanism.
3. Introduce a small internal `Deadline` value that owns the absolute cutoff and
   supplies remaining time to every notice wait and join decision. This reduces timing
   arithmetic mistakes, but is more implementation structure than a single local
   deadline calculation.

### 5. The Enforcement layer's normative status is unclear

**Validation: Real specification defect.** `RUN-GRAMMAR` delegates to Enforcement,
but section 0 says ordinary prose is non-binding and only names a subset of tables as
binding. The same issue affects the Journal algorithm, always-on assertions, and test
suite bullets. Section 0 also allows arbitrary extra derives while the certificate
text prohibits `Clone`, `Copy`, and `Default`. These are actual contradictions in the
document's rule system, not runtime design flaws.

**Plainest version:** Important rules are written on pages that the document itself
says are only explanations.

**Fifth-grader version:** The rulebook says only boxed rules count, then puts some of
its most important rules outside the boxes. The fix is to mark the existing boxes as
official, not rewrite the game.

**Solutions, simplest and strongest first:**

1. Amend section 0 once: include the certificate transition table and Journal commit
   table in the binding-table list; allow API blocks to list forbidden derives; add
   one `ASSERT-ALWAYS` Laws row; assign stable IDs to the section 12 suites. Keep the
   rules in their current single homes. This is the recommended wide fix.
2. Move every normative sentence from Enforcement and Journal Mechanism into new
   guarantee rows. This follows the current section 0 literally but duplicates facts
   and invites drift.
3. Put explicit `Binding table` markers only around the two authoritative mechanism
   tables and `Define`/`Justify` markers around the remaining mechanism prose. This is
   precise but repeats section 0's classification locally.

### 6. The rendered transition mechanism cannot type-check

**Validation: Real implementation blocker, not a semantic flaw.** A borrowed `&[C]`
cannot transfer non-`Clone` Commands into `dispatch`. `no_commands()` cannot assert
anything without seeing the batch. `Checkpointed<answer>` asks Rust for a runtime
value in a type position. The graph and record ordering remain valid; only the
mechanism sketch is unimplementable as written.

**Plainest version:** The transition table asks Rust to move values out of a read-only
borrow and to make a type from a runtime answer. Rust cannot do either.

**Fifth-grader version:** The plan says "give away the toys while only looking at
them," and "pick a box shape after opening the box." The data must be owned when moved,
and the two possible box shapes must be named ahead of time.

**Solutions, simplest and strongest first:**

1. Let `dispatch_batch` receive a mutable/drainable owned batch: serialize a borrowed
   view for `CommandsPrepared`, then drain Commands in order. Let `no_commands` receive
   an immutable batch view. Return a runtime enum containing either a concrete
   `CheckpointedContinue` certificate or a concrete `CheckpointedStop` certificate.
   Keep all records and failure semantics unchanged.
2. Move the batch into `dispatch_batch` and return reusable storage with each success
   or failure result. This makes ownership explicit but complicates every exit path.
3. Encode the remembered answer in the preceding phase type and duplicate the empty
   and dispatch transitions for Continue and Stop. This maximizes static proof but
   creates type and method repetition with little practical gain.

### 7. A9 overclaims determinism

**Validation: Real wording error.** `EngineExit` contains user Error values and State,
while the trace explicitly erases Error values. `DET-RUN` already limits unconditional
equality to Core-owned parts and adds a correspondence condition for erased Errors.
A9's statement that every run output is a function of its premises is therefore
broader than the detailed guarantee.

**Plainest version:** Kavod can promise that Kavod's own choices repeat. It cannot
promise that arbitrary user Error objects magically compare equal.

**Fifth-grader version:** The machine controls its own labels and steps, but users put
some objects inside the final box. The machine can guarantee its labels repeat, not
every hidden detail of user objects.

**Solutions, simplest and strongest first:**

1. Change A9 to "every Core-owned run output" and mark the remaining premise with
   `TRUST-PURE`. Keep `DET-RUN` as the exact equality definition.
2. Make A9 point directly to `DET-RUN` rather than paraphrasing it. This prevents
   future drift but makes the axiom less readable alone.
3. Define a formal "Core projection" of `EngineExit` in prose and state A9 over that
   projection. This is maximally precise, but adds terminology that `DET-RUN` already
   expresses adequately.

### 8. A4's second sentence is easy to misparse

**Validation: Real drafting hazard, not a contradiction.** The graph consistently
uses the first failure the run observes. `RUN-FINALIZE` expressly discards a later
latch Error after another cause exists. But A4's second sentence says "once a first
Error ... exists," which can be read as wall-clock existence anywhere rather than
observation by the run. Multiple reviewers took that reading, so the wording is not
safe enough for an axiom.

**Plainest version:** "First Error" must mean the first Error the Engine sees, not the
first Error that secretly happened somewhere.

**Fifth-grader version:** The referee can only choose the first foul reported to the
referee. A foul that happened earlier but was not reported does not replace the cause
already fixed by the rules.

**Solutions, simplest and strongest first:**

1. Restore "first **observed** Error or fatal Core condition" in the cleanup sentence.
   Add one forensic note: if an operation fails before commitment while an unrelated
   publication remains unobserved, the operation's own Error becomes the cause and
   finalization discards the publication. This names the intended edge case.
2. Define "first failure" in the Glossary as first observed by the serial Run and use
   that term throughout. This centralizes vocabulary but requires more replacements.
3. Add an explicit failure-precedence table. It would be unambiguous, but the graph
   and `RUN-FINALIZE` already provide that table in substance.

### 9. Sim Port lifecycle "open" is undefined

**Validation: Real defect.** `SIM-START` stops Ports whose lifecycle is open after a
startup failure, but no rule says when a lifecycle opens. A never-started Port could
therefore receive `stop`, and a Port that allocates only in `start` could panic. The
current `TRUST-SPAWN` wording also names `run`, which only Live Ports have.

**Plainest version:** Startup cleanup says "stop everyone who started," but never marks
who actually started.

**Fifth-grader version:** Each Port needs a three-state sign: not started, open, or
ended. Cleanup may call `stop` only on an open Port.

**Solutions, simplest and strongest first:**

1. Define `NotStarted -> Open -> Ended`. Enter Open when `start` is invoked; the first
   method `Err` or the first `stop` invocation ends the lifecycle; never-started and
   ended Ports receive nothing. During startup rollback, stop only Open Ports in Slot
   order. Rewrite `TRUST-SPAWN` as "before its lifecycle's last method returns," which
   covers live and sim.
2. Track a `started: bool` and `ended: bool` per Sim Port and state equivalent rules.
   This is the same semantics with less explicit state-machine vocabulary.
3. Give each Sim Port an RAII lifecycle guard created at `start` invocation. This can
   enforce cleanup bookkeeping but is more mechanism than the single-threaded sim
   needs.

---

## Tier 2 - Confirmed Batchable Repairs

### 1. Axiom glosses overreach

**Validation: Real wording errors.** A1's "read-only view" does not describe
capabilities such as `emit`, `set_next`, and `offer`. A2 says the turn ends at handoff,
but the turn still performs the completion record and checkpoint, while sim command
processing is synchronous inside dispatch. A3 says every effectful operation has a
commitment point, but `APP-STATE` explicitly has none and Port-internal work is outside
Core's commitment model. The detailed rows are coherent; the summaries are not.

**Plainest version:** The short rules say more than the detailed rules actually
promise.

**Fifth-grader version:** The chapter titles are too broad, even though the
instructions underneath are right. Make each title describe only the rules it owns.

**Solutions, simplest and strongest first:**

1. Repair all three in one Laws edit: A1 is one owner with explicit capabilities for
   mutation; A2 serializes acceptance, one handler, its batch, checkpoint, and
   completion while destination work after handoff belongs to the Port; A3 applies to
   Core-owned contract operations and recorded transitions, with `APP-STATE` and
   Port-internal effects explicitly outside it.
2. Keep the axiom text short and add exact scope clauses immediately below the Laws
   table. This is readable but makes the table misleading when quoted alone.
3. Remove the gloss portions and let the guarantee rows carry all detail. This avoids
   falsehood but weakens the axioms as a useful summary.

### 2. Replay's derive is necessary, not sufficient

**Validation: Real specification overclaim.** Replaying recorded Event values and
times with enough step budget does not reconstruct when every Port arm was placed.
In a multi-Slot tie, different arm placement can change the selected Slot and Journal
bytes even when the listed three conditions hold. The three conditions remain useful
necessities.

**Plainest version:** Replaying the alarm times is not enough if you do not also
replay when each alarm was set.

**Fifth-grader version:** Two students can have alarms for the same time. Which alarm
rings first can depend on when each student armed it, so a faithful replay must repeat
that behavior too.

**Solutions, simplest and strongest first:**

1. Change the note to "necessary, not sufficient." Add that multi-Slot replay must
   reproduce arm placement and failure replay must reproduce each Error's presence at
   its trace position; only the single-Port success case follows from the original
   three conditions alone.
2. State one general rule: byte-equal replay requires an equivalent deterministic
   SimPort program that reproduces the entire Environment trace, including scheduling
   state. Keep the three bullets merely as common setup checks.
3. For exact trace playback, use one aggregate replay SimPort or a bespoke scripted
   Environment that owns the global recorded schedule. This avoids reconstructing
   cross-Port arm timing, but is a more specialized replay harness.

### 3. `RawValue` can break JSONL framing

**Validation: Real defect.** The Journal checks only the first and last object bytes.
Serde raw JSON can contain physical newlines between tokens, so one accepted record
can occupy several lines while still starting with `{` and ending with `}`. That
violates `JRN-FORMAT`.

**Plainest version:** A record can hide extra line breaks inside itself and stop being
one line.

**Fifth-grader version:** The Journal promises one entry per notebook line. Raw JSON
can sneak line breaks into one entry, so the writer must reject those breaks.

**Solutions, simplest and strongest first:**

1. Scan the bounded encoded buffer for raw CR or LF before appending the Journal's one
   LF. Reject either with a small framing error, writing nothing and poisoning nothing.
   Also add the same prohibition to `TRUST-SERIALIZE`. The scan is linear in the
   already-bounded record and closes the hole at the owner.
2. Add only the `TRUST-SERIALIZE` obligation and pin it with tests. This is the fewest
   code changes, but leaves a basic framing guarantee dependent on payload authors.
3. Make the bounded encode writer reject raw CR/LF during serialization and map that
   rejection to the framing error. This avoids a second scan but complicates
   propagation through serde's writer error path.

### 4. The serde newtype derive is false

**Validation: Real factual error.** A newtype serializes transparently. A newtype
around a map can therefore serialize as a JSON object, exactly as `EventIndex` relies
on newtype transparency for scalar output. Rust struct shape alone does not determine
the top-level JSON shape.

**Plainest version:** A wrapper can serialize like the thing inside it. Not every
newtype fails the object check.

**Fifth-grader version:** Putting a map in a thin box does not stop it looking like a
map when serde opens the box.

**Solutions, simplest and strongest first:**

1. Replace the note with an output-based statement: named-field structs normally
   produce objects; transparent/newtype values produce whatever their inner value
   produces; `NotAnObject` depends on actual top-level encoded shape.
2. Make solution 1 and add Journal shape tests for named, tuple, unit, scalar-newtype,
   and map-newtype values. This is useful if the note is intended as user guidance.
3. Delete the derive entirely and document only `NotAnObject`. This cannot be wrong,
   but gives users less useful guidance.

### 5. `SIM-SELECT` does not state one exact tie-break algorithm

**Validation: Real specification gap.** "Round-robin in frozen Slot order" plus a
cursor strongly suggests cyclic scanning, but does not literally state scan direction
and wraparound. Two implementations can plausibly choose different equal-time Slots.
The cursor does not need recording because it is deterministic once the algorithm is
fixed.

**Plainest version:** "Round-robin" needs one exact sentence saying where scanning
starts and how it wraps.

**Fifth-grader version:** When alarms tie, start at the cursor, walk forward through
the Slot list, wrap at the end, and pick the first tied alarm.

**Solutions, simplest and strongest first:**

1. State exactly that selection finds the minimum armed time, then chooses the first
   Slot with that time in a cyclic scan beginning at the cursor and wrapping once.
2. Give the selection function as short pseudocode in the binding row. This is more
   explicit but less prose-friendly.
3. Define a private `select_slot(arms, cursor)` helper whose stated result is the
   cyclic-scan rule and pin it with exhaustive small-Slot tests. This is more mechanism
   than the one-sentence contract but keeps selection centralized.

### 6. Sim subordinate effects are named for only one `Err` path

**Validation: Real wording gap.** `step(Err)` names advanced time, cleared arm, and
spent budget as standing effects. Budget exhaustion or no-arms discovered after one
or more `step(None)` calls also leaves earlier time advances, arm clears, cursor moves,
and budget use in place. A4 already implies this, but the sim section names only one
case.

**Plainest version:** If one selection call does several steps and then fails, the
earlier steps are not undone, no matter which final error ended the call.

**Fifth-grader version:** Moving pieces and then running out of moves does not put the
pieces back where they started.

**Solutions, simplest and strongest first:**

1. Add one shared clause to `SIM-SELECT`, `SIM-STEPS`, and `SIM-COMPLETION` by citation:
   every subordinate effect from earlier selected steps stands on every later exit.
   Keep the actual list in `SIM-SELECT` only.
2. Add a small selection-outcome table listing candidate, step Error, exhaustion, and
   no-arms exits with standing effects. Clearer, but more text.
3. Return a private typed selection-progress value carrying the updated time, cursor,
   arms, and budget into every exit path. This makes standing effects structural but
   adds state plumbing to a single-threaded loop.

### 7. "Accepted" and count/ordinal vocabulary disagree

**Validation: Real terminology defect.** The Glossary limits Accepted to candidates,
while `RunStarted` says the start turn is accepted. `RUN-INDEX` calls the index an
accepted count even though the accepted start turn has index 0, so it is an ordinal,
not the number of accepted turns.

**Plainest version:** The start turn is accepted too, and index 0 is a position, not a
count of one accepted turn.

**Fifth-grader version:** The first turn sits in seat 0. Seat numbers are ordinals;
they are not the number of students already seated.

**Solutions, simplest and strongest first:**

1. Define Accepted over either a committed `RunStarted` or committed `EventAccepted`
   record. Call `EventIndex` the accepted turn's ordinal everywhere. Keep values 0, 1,
   2, and so on unchanged.
2. Keep "count" but define it as zero-based count. That is mathematically possible but
   needlessly surprising.
3. Add a defined term `AcceptedOrdinal` and state that `EventIndex` is its concrete
   representation. This is more formal than simply replacing "count," but preserves
   every existing value and API.

### 8. `ports!` cannot synthesize identifiers from a naming stem

**Validation: Real implementation error.** Stable `macro_rules!` cannot concatenate
`Trading` with `Event` and `Command` to create identifiers. The invocation already
contains explicit `Event = TradingEvent` and `Command = TradingCommand` names, so no
new mechanism is needed.

**Plainest version:** The macro must use the two names the caller already supplied;
it cannot build those names by gluing words together.

**Fifth-grader version:** The form already has boxes for the Event name and Command
name. Use those boxes instead of asking the macro to invent names from `Trading`.

**Solutions, simplest and strongest first:**

1. State that the identifiers after `Event =` and `Command =` are the exact output
   enum names. Remove the naming-stem claim and either keep `Trading` as a wiring label
   with a stated purpose or remove it if it serves none.
2. Change syntax to `ports!(pub enum TradingEvent, TradingCommand { ... })`. This is
   even more direct but less descriptive.
3. Use a more explicit `macro_rules!` form with separate `event enum` and `command
   enum` clauses. It preserves the dependency plan but makes invocations longer.

### 9. Evidence holes need honest names

**Validation: Four real evidence-description gaps, not one broken state machine.**

- `CommandBoundExceeded` discards staged Commands and the returned Outcome before an
  intent record exists. The exit proves overflow, not what the handler tried to emit.
- A dispatch `Err` can be an older latched Error, so `Prepared` and
  `EnvironmentFatal` must distinguish observation site from cause site.
- An uncertain physical Journal suffix can result from any termination before a
  successful flush, not only a returned sink Error.
- If all Commands were handed off and the `CommandsDispatched` commit failed, the
  exact full prefix is identified by `JournalFatal.record_kind`, not by a committed
  `CommandsDispatched` record.

**Plainest version:** The behavior is mostly right, but a few sentences claim the
evidence tells us more than it really does.

**Fifth-grader version:** A receipt can prove "the basket overflowed" without listing
everything that was in it. It can also prove where an Error was noticed without
proving where the Error began. Say exactly what each receipt proves.

**Solutions, simplest and strongest first:**

1. Do one evidence-honesty pass with no schema change: document the overflow intent
   vacuum; change dispatch wording to observation-site language; generalize uncertain
   suffix to every non-flush termination; and explicitly state that
   `Journal(CommandsDispatched)` after `CommandsPrepared` proves full handoff despite
   the missing completion record. This is sufficient and preserves the graph.
2. Add staged command count to `CoreError::CommandBoundExceeded` while still omitting
   payloads. This improves diagnosis modestly but changes a public payload.
3. Add a fixed-size overflow summary to the Core Error, such as configured capacity
   and stored-prefix length, without retaining Command payloads. This remains bounded
   but gives less forensic value than a committed intent record would.

### 10. Meta-rule self-conformance batch

**Validation: Mostly real editorial/specification debt.** None requires runtime
redesign, but each weakens the document's claim that placement, citation, vocabulary,
and enforcement rules are mechanically checkable.

| Subfinding | Validation | Minimal correction |
|---|---|---|
| `PORT-ROUTING` names sim/live in the generic contract and gives the wrong sim mapping site | Real | Move implementation-specific mapping sites to Live/Sim; sim maps `start`, `step`, and `on_command`/fan-out paths. |
| Appendix A omits A1-A9 | Real navigation omission | Add the axioms or explicitly say the index lists guarantee/obligation IDs only. |
| Citations use section numbers or generic labels | Real local violations | Replace them with IDs or binding-table names. |
| `BOUND-*` prefixes mix enforced and trusted rows | Real clarity problem | Let table location determine trust and document that, or rename trusted sizing/blocking rows consistently. |
| Glossary is absent from forward-reference exemptions | Real meta-rule gap | Exempt Glossary definitions explicitly. |
| `ENV-ERRORS` says "binding row" | Real informal vocabulary | Say "guarantee row or named binding-table row." |
| `ENV-SHUTDOWN` uses queue-flavored universal wording | Real wording mismatch | State implementation-neutral shutdown effects, then let Live and Sim name queue/stop realizations. |
| `BOUND-STATIC` puts live thread count in a universal Laws row and has vague freeze points | Real scope problem | Bind static Slot topology globally for shipped wiring; bind one-thread-per-Slot in `LIVE-THREADS`; name construction freeze once. |
| "States" conflicts with Glossary "Phase"; `S` means phase and State elsewhere | Real naming hazard | Use "Phases" and rename the phantom parameter `P`. |
| `JournalFatal.record_kind` lacks a binding doc comment | Real API documentation gap | Say it is the kind whose commit failed. |
| StopPending says "reported Error" for a pending-at-close Error | Real vocabulary error | Say "report-carried pending Error." |
| `JRN-POISON` calls `Ok(0)`/over-reporting a "failure" | Real defined-term mismatch | Say "sink Error or invalid sink result." |
| `RUN-INDEX`'s residual-assert sentence is garbled | Real explanation gap | Name each assertion and its exact condition separately. |
| Sim arms are never said to start disarmed | Real initialization gap | Add "all arms start disarmed" to `SIM-WAKEUP`. |
| A dead Port's arm cannot be selected, but this is unstated | Real derive omission, not a bug | State that every Port-ending `Err` ends the run before another selection, so stale arms are unreachable. |
| Sim latch-first `next_event` behavior is only a citation | Real local clarity gap | State the returned Error/no-consumption outcome in `SIM-SELECT`. |
| Journal object test omits the empty-buffer guard | Real panic hazard in the rendered algorithm | Check nonempty before first/last-byte access. |
| Poisoning on `Interrupted` lacks justification | Real rationale gap | Explain the no-retry choice as bounded, deterministic sink-call evidence. |
| "Observationally identical" hand-written Port sums is undefined | Real term gap | Define equality over variant/payload mapping and serialized representation relevant to Core. |
| `PORT-STATE` says wiring never interprets values | Real overstatement | Say wiring inspects only the outer Slot tag and does not inspect/transform payloads. |
| `DET-ENV` compares report Error presence already fixed by equal traces | Real redundant/dead comparand | Remove it from the output comparison list. |
| `TRUST-PURE` asks for an "identical exit" that generic State/Error values cannot promise | Real contradiction | Use `DET-RUN`-equal Core-owned content and corresponding erased Errors. |

**Plainest version:** The document's rules about how to write rules are not followed
in several small places.

**Fifth-grader version:** The design has a grammar and filing system. Some labels are
in the wrong drawer, some names are inconsistent, and a few instructions point to a
chapter instead of the exact rule. One cleanup pass fixes them.

**Solutions, simplest and strongest first:**

1. Apply all corrections above as one mechanical self-conformance pass after the
   section 0 binding-form amendment. Use no semantic changes and no new subsystem.
2. Add a review checklist keyed to section 0: placement, backward citation, ID form,
   owner, enforcement mode, implementation neutrality, and Glossary terms. Run it on
   every later design edit.
3. Build a documentation linter for IDs, Appendix membership, and forbidden section
   citations. This can prevent recurrence, but only after the simple text cleanup.

---

## Tier 3 - Wiring-Close Checklist

### 1. Error-sum composition is incomplete

**Validation: Real open work.** Section 10 already promises final Error sums, but the
current generic `PORT-ROUTING` text places sim mapping only at fan-out even though
Port Errors can arise from `start`, `on_command`, and `step`. Premature closure needs
a concrete typed value. A closed command inbox is operationally different from a full
one and should not be reported as capacity exhaustion.

**Plainest version:** Wiring has not finished naming every way each Environment can
fail, and one existing label points to the wrong place.

**Fifth-grader version:** Make one checklist of every door where an Error can enter,
then give each kind the right label. "Mailbox full" and "mailbox gone" are not the
same problem.

**Solutions, simplest and strongest first:**

1. Add one binding Error-mapping matrix in Wiring. Rows are operation/failure source;
   columns are live/sim variant, commitment side, and Slot mapping. Include sim
   `start`, `on_command`, and `step`; define premature closure; separate inbox `Full`
   from `Closed`. This single table closes all three gaps.
2. Define each Environment Error enum beside its API and cite a shared mapping
   guarantee. This is conventional Rust documentation but makes cross-environment
   auditing harder.
3. Generate each Environment's closed Error enum and mapping skeleton from the same
   explicit Slot declaration using `macro_rules!`, leaving Kavod-owned variants
   handwritten. This preserves typed per-Slot variants but increases macro surface.

### 2. Bounds and their scope are incomplete

**Validation: Real open work.** The live aggregate Event and Error paths need explicit
`Send + 'static` bounds. `BOUND-NONZERO` requires nonzero configured capacities, but
Wiring must expose those types. Nonempty Port enforcement has no construction site,
and `BOUND-STATIC` currently appears to cover bespoke Environments that may not use
Kavod Ports at all.

**Plainest version:** The design has the safety rules, but the unfinished builders do
not yet show where Rust and configuration enforce them.

**Fifth-grader version:** Every builder needs the right-sized boxes, at least one Port,
and thread-safe labels where values cross threads. Custom Environments need to promise
their own bounds rather than pretend they use Kavod's builder.

**Solutions, simplest and strongest first:**

1. In the Wiring authority table, use `NonZero*` fields for every capacity/budget;
   place `Send + 'static` on the live aggregate Event and Error types; reject an empty
   shipped Port builder; scope `BOUND-STATIC` to shipped wired Environments and leave
   bespoke topology/bounds to `TRUST-ENV` plus `ENV-BOUNDS`.
2. Encode nonempty Port count and capacities in builder typestate. Stronger, but it
   expands generic types and is unnecessary when construction-time checks suffice.
3. Generate a fixed builder with one required field per declared Slot, then retain
   runtime `NonZero*` capacities. This makes nonempty/completeness structural but adds
   generated builder API.

### 3. Slot-order authority is undecided

**Validation: Real open work with broad consequences.** Startup, shutdown, sim ties,
error mapping, and several tests depend on one frozen order. Section 10 names
declaration order as the candidate but does not decide it. Registration order creates
a second authority and makes refactoring builders behaviorally significant.

**Plainest version:** The design needs one official Slot order before any ordering
rule can be tested.

**Fifth-grader version:** Use the order written in the Slot enum everywhere. Do not
let the order in which builder methods happen secretly change behavior.

**Solutions, simplest and strongest first:**

1. Choose Slot declaration order as the sole authority. Make builders bind by Slot
   identity and materialize Ports in that order regardless of registration call order.
2. Require registrations to appear in declaration order and assert it once at build.
   Simpler internally, but needlessly constrains callers.
3. Have the Slot declaration macro emit one private ordinal/iterator and require every
   startup, shutdown, tie-break, and mapping table to consume it. This enforces the
   same declaration order structurally but adds macro-generated mechanism.

### 4. Environment sizing obligations are missing

**Validation: Real open work, with one overstatement.** Inbox under-sizing directly
causes dispatch Fatal. Step-budget under-sizing causes a typed Environment Error.
Shutdown-budget under-sizing can produce `Incomplete`, and fan-in under-sizing causes
`Full`, which a Port may retry or convert to an Error rather than immediately forcing
Fatal. The synthesis is right that all need owners and checks, but fan-in Full is not
unconditionally a run-Fatal by itself.

**Plainest version:** Several knobs can stop progress or end a run when too small, but
only one knob says who must size it correctly.

**Fifth-grader version:** Inbox size, Event queue size, shutdown time, and sim step
budget all need a named adult responsible for choosing enough capacity.

**Solutions, simplest and strongest first:**

1. Add one `BOUND-ENV-SIZING` obligation with four named clauses: inbox burst/residue,
   fan-in burst/retry policy, shutdown teardown envelope, and steps per acquisition.
   Name deployment plus relevant Port authors as upholders and add focused boundary
   tests. Keep `BOUND-INBOX` as a citation or fold it into this one row, but not both.
2. Add separate `BOUND-FANIN`, `BOUND-SHUTDOWN`, and `BOUND-STEPS` rows beside
   `BOUND-INBOX`. This gives precise IDs but creates more small rules.
3. Let Wiring compute conservative bounded defaults from explicit per-Slot workload
   declarations, while still requiring nonzero hard caps. This reduces manual sizing
   but adds configuration metadata and cannot replace deployment review.

### 5. API-block completeness is unfinished

**Validation: Real open work.** `Engine`, `LiveCtx`, and `SimCtx` are used in binding
APIs without their own opaque struct declarations. `Never: Serialize` exists only in
prose. `EventIndex` construction authority should be exact. `TurnOutcome` and
`RecordPayload` are referenced by the normative Enforcement mechanism but undeclared.

**Plainest version:** Some type names are used before the official API says those
types exist.

**Fifth-grader version:** Every named tool needs an entry in the parts list, even when
its inside is private.

**Solutions, simplest and strongest first:**

1. Complete the API blocks with opaque `Engine`, `LiveCtx`, and `SimCtx` declarations;
   bind `impl Serialize for Never`; make `EventIndex`'s field private and its minting
   crate-internal; declare private `TurnOutcome` and `RecordPayload` shapes in the
   promoted Enforcement binding block.
2. Remove private mechanism names such as `RecordPayload` from normative prose and
   leave only behavior. This reduces declarations but weakens the stated compile-time
   mechanism.
3. Consolidate all opaque public handles in one Wiring API block and all private proof
   types in one Enforcement API block. This is easier to audit but moves more text
   than the targeted declarations.

### 6. `TRUST-PURE` omits subjects and assigns one duty to the wrong owner

**Validation: Real trust-boundary gap.** The Application object itself can hold hidden
authority used through `&self`; `initial_state` can consult hidden authority; user
Error destructors can have effects. "Ports share no state" cannot be upheld solely by
the Application author. Running twice in one process may also preserve the same hidden
global state and miss a dependency.

**Plainest version:** The purity promise covers handler inputs but forgets the object
holding the handlers, initial-state creation, Error cleanup, and the people who build
Ports.

**Fifth-grader version:** Check every place secret randomness or shared state could
hide, and make the person who owns that place responsible for it.

**Solutions, simplest and strongest first:**

1. Expand the trust boundary in one coordinated edit: Application value,
   `initial_state`, State/Event/Command, and user Error `Drop` behavior belong to the
   relevant value/Application authors; Port state separation belongs to Port and
   Wiring authors. Verify repeatability in separate processes with perturbed ambient
   environment, comparing `DET-RUN`-equal Core-owned output rather than generic exit
   equality.
2. Split this into `TRUST-APP-PURE`, `TRUST-PORT-ISOLATION`, and
   `TRUST-ERROR-DROP`. Ownership is clearer, but there are more rows.
3. Attempt to type-restrict all hidden authority. Rust trait bounds cannot prove lack
   of clocks, globals, or I/O, so review and repeatability testing remain necessary.

### 7. Lossy serialization weakens evidence claims

**Validation: Real evidence-contract gap.** The Journal can only preserve fields a
`Serialize` implementation emits. `CommandsPrepared` therefore cannot prove complete
semantic intent for omitted fields. Post-abort reconciliation relies on Journal bytes,
but `TRUST-KEY` does not currently require the business key to appear in those bytes.

**Plainest version:** If the serializer leaves a field out, the Journal never knew it.
The business key must be among the fields actually written.

**Fifth-grader version:** A receipt only proves what is printed on it. If recovery
needs an order number, the order number must be printed.

**Solutions, simplest and strongest first:**

1. Define `CommandsPrepared` as the complete **serialized** Command intent and extend
   `TRUST-KEY` to require the stable business key in the serialized Command encoding.
   Add a golden per-Slot assertion for that key. This is both honest and operationally
   useful.
2. Require non-lossy serialization of every Command field. Stronger, but "every field"
   may include implementation details irrelevant to reconciliation.
3. Add a separate mandatory top-level business-key field to every command record.
   This centralizes recovery data but changes the wire schema and duplicates payload
   information.

---

## Tier 4 - Design Judgment Calls

### 1. Live liveness has no owner

**Validation: Genuine judgment call, not a safety defect.** A live Environment may
wait forever when every Port waits for Commands and the Engine waits for an Event.
`BOUND-LOOPS` explicitly says a blocking wait has no elapsed-time bound, and external
cancellation is intentionally modeled as a Port. The design is coherent, but no named
party must ensure that a live wiring has a progress or cancellation path.

**Plainest version:** The system can safely wait forever because nobody is officially
responsible for making something happen.

**Fifth-grader version:** The Engine and all Ports can wait for each other. Assign
someone to ensure one Port can produce an Event, Error, timeout, or cancellation.

**Solutions, simplest and strongest first:**

1. Add `TRUST-LIVENESS`: the Wiring/deployment owner ensures each live topology has a
   credible path to Event, Error, or cancellation while a run may block. Verify with a
   topology review and an end-to-end cancellation test. Do not add a second Engine
   cancellation channel.
2. Require Wiring to designate one ordinary Slot as the liveness/cancellation source.
   This is more checkable but may be artificial for naturally event-driven systems.
3. Ship a watchdog/cancellation Port that users may bind like any other Port. This
   preserves the architecture while offering a standard solution, but should remain
   optional.

### 2. An oversized inbound Event can kill the run before the Application sees it

**Validation: Genuine judgment call with a real operational risk.** A5 requires
`EventAccepted` to commit before `on_event`. If the Event cannot fit the record bound,
the Journal fails and the Application cannot reject it. This is consistent with the
design, but a remote source may trigger a run-ending local bound failure unless the
Port/payload owner enforces size.

**Plainest version:** A too-large Event breaks the receipt before the Application can
look at the Event.

**Fifth-grader version:** The clerk must write the package in the log before opening
it. If the package description is too large for the log, the run stops first.

**Solutions, simplest and strongest first:**

1. Keep A5 and expand `BOUND-SIZING`: each Port/payload author must bound the encoded
   Event including record envelope overhead, and deployment must set
   `max_record_bytes` above the largest bound. Add a maximum-size test per Slot.
2. Require each ingress Port to construct Events from bounded payload types before
   `offer`. This rejects oversized remote data at the protocol owner and makes the
   bound structural, but is more intrusive in Event definitions.
3. Split genuinely large external data into bounded application-level Event chunks,
   each independently accepted and journaled. This preserves A5 and Journal ownership
   but adds protocol-level reassembly where large payloads are actually needed.

### 3. Optional forensic enhancements

**Validation: Judgment calls, not defects.** A committed-byte count would help trim a
suffix after a returned sink failure but cannot help after process abort when no exit
exists. A run ID/per-record schema version helps multi-run appended sinks, but the
current sink contract and `RunStarted` already support a simpler run boundary. Slot
identity in the Journal would improve Journal-only diagnosis, but the typed exit
already identifies the mapped Slot and the current observation/cause split is
deliberate.

**Plainest version:** More forensic metadata could be useful, but the current evidence
claims do not require it and each new field permanently expands the wire format.

**Fifth-grader version:** Extra labels can make investigation easier, but do not add
them until someone truly needs to answer a question the current Journal and exit
cannot answer together.

**Solutions, simplest and strongest first:**

1. Make no v12 schema change. Add one explicit evidence-scope note: Journal plus exit
   is the full returned-failure evidence; Journal alone intentionally lacks some cause
   identity; abort has no trusted committed boundary. This is the KISS choice.
2. If suffix recovery is an immediate requirement, add `committed_bytes` only to the
   returned Journal-failure information and clearly state that it is unavailable after
   abort.
3. If one sink will intentionally contain many independently consumed runs, add a run
   ID and per-record schema version together. Do not add them speculatively.

### 4. Offer retry can neglect inbox draining

**Validation: Genuine obligation gap.** `LIVE-EVENTS` permits a Port to retry `offer`
under its own pacing while observing lifecycle. `TRUST-LIFECYCLE` only names blocking
points, and `TRUST-DRAIN` concerns shutdown inbox draining. A tight Full/retry loop can
observe lifecycle yet still fail to alternate with draining its own Commands.

**Plainest version:** A Port can spend all its time retrying a full Event queue and
never read the Commands waiting for it.

**Fifth-grader version:** Do not keep pushing on a full outgoing mailbox while ignoring
your incoming mailbox. Retry loops need a fair chance to check both.

**Solutions, simplest and strongest first:**

1. Widen `TRUST-LIFECYCLE` to all retry and polling loops, and require retry policy to
   interleave lifecycle checks and protocol-required inbox draining. Verify under
   sustained fan-in backpressure.
2. Add a separate `TRUST-PORT-FAIRNESS` obligation for retry/drain interleaving. More
   precise, but another small rule.
3. Provide an optional bounded Port-side retry helper that requires a lifecycle/drain
   callback between attempts. This standardizes fair retry behavior but adds helper
   API for policy that can remain in Port code.

### 5. SimPort `stop` Errors are always discarded

**Validation: Genuine policy choice, currently internally consistent.**
`SIM-SHUTDOWN` closes the latch before calling `stop`, so A4 classifies every stop
Error as cleanup and discards it. This mirrors the rule that post-close live shutdown
Errors do not become the run cause. The downside is that a clean Stop can return
`Stopped` even when a Sim Port reports cleanup failure.

**Plainest version:** Sim cleanup can fail, but the current rules intentionally throw
that Error away after shutdown has already been declared clean.

**Fifth-grader version:** Once the referee closes the game, cleanup complaints do not
change the final score. Decide whether that is the evidence story you want.

**Solutions, simplest and strongest first:**

1. Keep the current policy and say it explicitly in the `stop` API and
   `SIM-SHUTDOWN`; add a test proving a stop Error is discarded. This preserves A4 and
   live/sim shutdown symmetry.
2. Make `stop` infallible and place any cleanup fallibility inside earlier Port methods
   or trusted cleanup obligations. This removes a misleading Error channel but may be
   too restrictive for some simulations.
3. Signal shutdown, call stops, publish the first stop Error, and close the latch only
   afterward so it enters the report. This is a contained semantic change, but it
   makes sim shutdown differ from the current post-close cleanup model and is not
   recommended for v12.

---

## Consistent Recommended Disposition

### Fix now without changing semantics

1. Apply Wide Fix A: promote the two mechanism tables, assertion rule, suite IDs, and
   prohibited-derive syntax.
2. Apply Wide Fix B: latch ordering definition, completion-notice rename, one shutdown
   deadline budget, and explicit Sim lifecycle.
3. Apply Wide Fix C: certificate assignments/report retention and type-correct batch
   and checkpoint transitions.
4. Apply Wide Fix D: narrow A1-A4/A9, replay, evidence, and terminology claims.
5. Apply Wide Fix F: reject raw record newlines and correct serde/evidence statements.
6. Run the Tier 2 meta-rule cleanup as one mechanical pass.

### Close in Wiring, not elsewhere

1. Choose Slot declaration order.
2. Add one Error mapping matrix.
3. Add one topology/bounds/API authority table.
4. Add one Environment sizing obligation with named owners and tests.
5. Repair the purity/isolation trust boundary by actual owner.

### Keep as policy for v12

1. Add a liveness owner, but no second cancellation architecture.
2. Keep A5 and control Event size through per-Slot sizing obligations.
3. Do not add speculative forensic fields.
4. Require fair offer-retry/drain behavior as a Port obligation.
5. Keep post-close Sim `stop` Errors discarded, but state and test it explicitly.

This disposition preserves the design's strongest properties: one owner per fact,
one serial run grammar, first observed failure, intent before effect, bounded owned
work, typed Errors, Environment-independent Core records, and no rollback. It fixes
real holes by making existing intent binding and executable rather than adding new
subsystems.
