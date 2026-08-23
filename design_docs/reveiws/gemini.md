# Kavod Core Design v12 — Adversarial Review Report

**Target Document:** `design_docs/design-v12.md` (Authoritative v12)  
**Scope:** Standalone semantic analysis in a vacuum (Axioms, Contracts, Typestate Grammar, Concurrency, Virtual Time, Journal, Lifecycle, and Finalization).

---

## 1. Executive Summary

An exhaustive adversarial review of **Kavod Core Design v12** was conducted to identify internal inconsistencies, contract contradictions, unhandled race conditions, and semantic failure modes.

### Overall Assessment
The foundational architecture of v12 is exceptionally rigorous. The **typestate certificate grammar (`RUN-GRAMMAR`)**, the **Journal write/encode/poison model (`JRN-POISON`, `JRN-ENCODE`)**, the **startup gate synchronization (`LIVE-START`, `SIM-START`)**, and the **monotonic virtual time progression (`SIM-TIME`, `SIM-SELECT`)** are formally sound, robust, and watertight against unauthorized state transitions.

However, the review identified **4 Major design defects/inconsistencies**, **4 Minor specification gaps**, and **1 Cosmetic typo**. The major issues center on:
1. A **Rust type/ownership incompatibility** between `dispatch_batch`'s slice signature and `Environment::dispatch` taking commands by value without a `Clone` bound.
2. A **semantic asymmetry and diagnostic misattribution** in `SIM-DISPATCH` when multi-command batches encounter a failing `SimPort::on_command`.
3. A **normative terminology collision** between `LIVE-SUPERVISION` and `LIVE-SHUTDOWN` regarding the term *"Publication"*, which risks shutdown deadline hangs.
4. A **race condition in error prioritization** where a pre-commitment dispatch capacity failure silently discards an earlier latched root-cause error.

---

## 2. Table of Findings

| ID | Location / Cites | Category | Severity | Summary |
|---|---|---|---|---|
| **M1** | Lines 405, 575–576, 778, 785 | API / Ownership | **Major** | `dispatch_batch(env, &[C])` cannot move `C` by value into `Environment::dispatch(command: C)` without `Clone`. |
| **M2** | Lines 689, 722–723, 778, 1026, 1037 | Virtual Time / Dispatch | **Major** | `SIM-DISPATCH` swallowing `on_command` errors causes multi-command batches to misattribute failure to position $k+1$ and creates Journal divergence with single-command batches. |
| **M3** | Lines 111, 917, 920, 948–949 | Concurrency / Glossary | **Major** | Glossary defines *"Publication"* strictly as latch Error entry; expected completions stay unpublished, but `LIVE-SHUTDOWN` waits for *"completion publications"*, threatening shutdown hangs. |
| **M4** | Lines 450–451, 744, 916, 943–945 | Concurrency / Error Model | **Major** | Pre-commitment `dispatch` inbox rejection returns local capacity error and leaves prior latched crash pending, which `RUN-FINALIZE` subsequently discards under A4. |
| **m1** | Lines 328–336, 368–371, 1107 | Macro / Syntax | **Minor** | `ports!` prose calls `Trading` a "naming stem", but declarative `macro_rules!` cannot concatenate identifiers and requires explicit identifier parameters. |
| **m2** | Lines 450–451, 943 | Concurrency / Latch | **Minor** | Ambiguity in `ENV-LATCH` regarding whether latch inspection strictly precedes fallible capacity checks in `dispatch`. |
| **m3** | Lines 920, 949 | Concurrency / Shutdown | **Minor** | Sequential joining of Ports in frozen Slot order during `shutdown` can block on an unresponsive thread before collecting finished threads. |
| **m4** | Lines 497–498, 526 | Journal Mechanism | **Minor** | Step 3 object boundary byte inspection (`buf[0] == b'{' && buf[len-1] == b'}'`) requires an explicit non-empty guard. |
| **C1** | Line 4 | Metadata | **Cosmetic** | Typo in path reference: `design_docs/reveiws/` instead of `reviews/`. |

---

## 3. Detailed Major Findings

---

### Finding M1: Ownership Incompatibility in `dispatch_batch(env, &[C])` vs By-Value `Environment::dispatch`
- **Severity:** **MAJOR**
- **Citations:** Line 227 (`Application::Command`), Line 322 (`PortContract::Command`), Line 405 (`Environment::dispatch`), Line 778 (Transitions Table), Lines 785–788.
- **The Issue:**
  In Section 3 and Section 4, `Command` has only a `Serialize` bound:
  ```rust
  type Command: Serialize; // Move-only, NOT Clone
  ```
  In Section 5 (`Environment` contract, line 405), `dispatch` consumes `command` by value:
  ```rust
  fn dispatch(&mut self, command: Self::Command) -> Result<(), Self::Error>;
  ```
  However, in Section 7 (*Enforcement table*, line 778), the private transition is specified as:
  ```rust
  TurnOpen | dispatch_batch(env, &[C]) | asserts nonempty; commits CommandsPrepared; runs the whole handoff loop in order; a dispatch Err carries { position, error } -> EffectsComplete
  ```
  and line 785 states:
  > *"`dispatch_batch` is one transition over one slice — with separate prepare and dispatch calls, two independent slices could commit a `CommandsDispatched` after a partial handoff."*
- **Why It Breaks:**
  In Rust, elements of type `C` cannot be moved out of a shared reference to a slice `&[C]` without `C: Clone` or `C: Copy`. Requiring `C: Clone` violates Kavod's zero-cost, move-only resource ownership model (e.g., commands carrying unique buffers, file descriptors, or non-cloneable identifiers).
- **Impact:**
  The transition signature as specified cannot compile if `C` is move-only and `dispatch` consumes `C` by value.
- **Recommended Fix:**
  Change the transition signature to take mutable/draining ownership of the command buffer or pass a draining cursor:
  `dispatch_batch(self, env: &mut E, batch: &mut BoundedBuffer<C>) -> Result<Certificate<EffectsComplete>, ...>`
  The transition first borrows `batch.as_slice()` to serialize and commit `CommandsPrepared`, then drains `batch` element-by-element by value into `env.dispatch(cmd)`.

---

### Finding M2: `SIM-DISPATCH` Multi-Command Batch Asymmetry and Diagnostic Misattribution
- **Severity:** **MAJOR**
- **Citations:** `SIM-DISPATCH` (line 1026), Sim Mechanism (lines 1037–1040), `Prepared` state (line 689), Records Table (lines 722–723), `EnvironmentFatal` (line 623), `DET-ENV` (lines 746–747).
- **The Issue:**
  In `SimEnvironment`, `dispatch` synchronously invokes `SimPort::on_command`. Per `SIM-DISPATCH`:
  > *"An `Err` from `on_command` is published (`ENV-ERRORS`) and the `dispatch` returns `Ok` — the invocation already committed."*
- **Trace Scenario:**
  Consider a turn where a handler emits a 2-command batch `[Cmd0, Cmd1]` (where `Cmd0` targets Port A and `Cmd1` targets Port B):
  1. `dispatch_batch` invokes `env.dispatch(Cmd0)`.
  2. `PortA::on_command(Cmd0)` returns `Err(PortAErr)`.
  3. `PortAErr` is published to the latch (`pending`), Port A's lifecycle ends (`SIM-LIFECYCLE`), and `dispatch(Cmd0)` returns `Ok(())`.
  4. `dispatch_batch` proceeds to index 1 and calls `env.dispatch(Cmd1)`.
  5. Per `ENV-LATCH` (line 450) and Sim Mechanism (line 1037), `dispatch(Cmd1)` checks the latch before commitment. Seeing `pending(PortAErr)`, it takes the error, marks it reported, and returns `Err(PortAErr)` without invoking Port B.
  6. `dispatch_batch` halts. `CommandsDispatched` is **never committed**.
  7. Engine exits Fatal with `FatalCause::Environment(EnvironmentFatal { error: PortAErr, operation: Dispatch { position: 1 } })`.

  Now contrast this with a 1-command batch `[Cmd0]` where `PortA::on_command(Cmd0)` returns `Err(PortAErr)`:
  1. `dispatch_batch` calls `env.dispatch(Cmd0)`.
  2. `PortA::on_command(Cmd0)` returns `Err(PortAErr)`. Error latches; `dispatch(Cmd0)` returns `Ok(())`.
  3. `dispatch_batch` completes the batch and **commits `CommandsDispatched`** to the Journal.
  4. Engine enters `EffectsComplete`, calls `checkpoint(env)` (`take_error()`), which returns `Some(PortAErr)`.
  5. Engine exits Fatal with `FatalCause::Environment(EnvironmentFatal { error: PortAErr, operation: Checkpoint })`.
- **Inconsistencies & Contradictions:**
  1. **Diagnostic Misattribution:** In the 2-command batch, the exit diagnostic reports `Dispatch { position: 1 }`, falsely implicating `Cmd1` when `Cmd1` was never delivered and the error was generated entirely by `Cmd0` at position 0.
  2. **Journal Invariant Violation:** When batch length is 1, `CommandsDispatched` commits (evidencing *"Every prepared Command was handed off"*). When batch length is 2, `CommandsDispatched` does not commit, despite `Cmd0` reaching the exact same post-commitment failure state in both runs.
  3. **Cross-Environment Divergence (`DET-ENV`):** In Live Environment, both commands are enqueued into inboxes (`Ok`), `CommandsDispatched` commits, and Port A's asynchronous thread failure is observed at `Checkpoint`. In Sim, the same failure aborts dispatch at position 1 and suppresses `CommandsDispatched`.
- **Recommended Fix:**
  In `SimEnvironment::dispatch`, define synchronous handoff commitment such that if `on_command` fails, `dispatch` returns `Err(PortError)` directly at position $k$ (i.e. handoff failed to complete), or if treated as post-commitment, specify that `dispatch_batch` distinguishes between local dispatch rejection and a latched error from an earlier position in the same batch.

---

### Finding M3: Normative Terminology Collision in `LIVE-SUPERVISION` vs `LIVE-SHUTDOWN`
- **Severity:** **MAJOR**
- **Citations:** Glossary (line 111), `LIVE-SUPERVISION` (line 917), `LIVE-SHUTDOWN` (line 920), Live Mechanism (lines 932, 948–954).
- **The Issue:**
  1. **Glossary (line 111):** Normatively defines *Publication*:
     > *"- **Publication** — entry of an Error into the latch."*
  2. **`LIVE-SUPERVISION` (line 917):** Explicitly guarantees that normal thread completions stay unpublished:
     > *"...every completion is unambiguously premature — publishing atomically with its classification — or expected, staying unpublished (A4)."*
  3. **`LIVE-SHUTDOWN` (line 920) & Mechanism (line 948):** States:
     > *"It waits at most the shutdown deadline... for completion publications, joining publishers (prompt by construction: publication follows the Port's last work...)"*
- **Why It Breaks:**
  If a clean run executes `shutdown`, all Port threads exit normally (`Ok(())`). Under `LIVE-SUPERVISION`, normal exits **stay unpublished** (no Error enters the latch).
  If `LIVE-SHUTDOWN` waits for **"completion publications"** and only joins **"publishers"**, the Engine thread receives zero latch publications and will block until the shutdown deadline expires, turning every clean shutdown into a deadline timeout.
- **Root Cause:**
  Overloading the normative term "Publication" (strictly defined in the Glossary as latch Error entry) to also refer to internal thread termination signaling (`completion tracking`).
- **Recommended Fix:**
  In `LIVE-SHUTDOWN` and Mechanism, replace "completion publications / publishers" with "thread completion signals / finished threads". Explicitly state that thread termination notifies the internal `completion tracking` primitive, while *Publication* remains exclusively reserved for latch Error insertion.

---

### Finding M4: Pre-Commitment Admission Rejection Silently Discards Prior Latched Root Cause
- **Severity:** **MAJOR**
- **Citations:** `ENV-LATCH` (lines 450–451), `LIVE-DISPATCH` (line 916), Live Mechanism (lines 943–945), `RUN-FINALIZE` (line 744), Axiom A4 (line 143).
- **The Issue:**
  1. At $t_1$, background Port A crashes and publishes `Err(PortACrash)` to the latch (`pending`).
  2. Concurrently at $t_2$, Engine dispatches `Cmd0` to Port B.
  3. Port B's inbox is full; admission fails before commitment.
  4. Per `ENV-LATCH` (line 450):
     > *"An operation that fails before its commitment is not an observation point: it returns its own Error and a concurrent publication stays pending."*
  5. `dispatch(Cmd0)` returns `Err(InboxFull)`.
  6. Engine drops the certificate and executes `RUN-FINALIZE`.
  7. In `RUN-FINALIZE` (line 744):
     - First-observed cause is fixed as: `Environment(Dispatch { position: 0 })` with `InboxFull`.
     - `RUN-FINALIZE` calls `env.shutdown()`.
     - `shutdown()` closes the latch and retrieves `Some(PortACrash)`.
     - `RUN-FINALIZE` executes: *"discard the report's Error (A4: a cause exists)"*.
  8. Final exit reports `InboxFull`. The earlier root cause `PortACrash` is permanently discarded.
- **Why It Breaks:**
  Axiom A4 mandates: *"First failure wins. The first Error or fatal Core condition the run observes is the Fatal cause."*
  Here, `PortACrash` occurred before `InboxFull`, but because `dispatch` failed pre-commitment on capacity, the actual crash is discarded during finalization cleanup.
- **Recommended Fix:**
  Clarify in `LIVE-DISPATCH` and `ENV-LATCH` that `dispatch` inspects the latch **before** attempting fallible inbox admission. If the latch is pending, `dispatch` returns the latched error immediately without attempting admission, ensuring the true first failure is preserved.

---

## 4. Detailed Minor & Cosmetic Findings

---

### Finding m1: `ports!` Macro Syntax vs Declarative `macro_rules!` Stem Expansion
- **Severity:** **MINOR**
- **Citations:** Lines 328–336, Lines 368–371, Line 1107.
- **The Issue:**
  Section 4 prose (line 369) asserts:
  > *"The invocation's `Trading` is a naming stem: the expansion creates `TradingEvent` and `TradingCommand`, and no item named `Trading`."*
  However, Section 11 (line 1107) fixes `ports!` as a declarative `macro_rules!` macro. Standard `macro_rules!` cannot perform identifier concatenation (`$stem + "Event"`). The macro invocation explicitly supplies the identifiers:
  ```rust
  kavod::ports!(
      pub enum Trading<Event = TradingEvent, Command = TradingCommand> { ... }
  );
  ```
- **Recommended Fix:**
  Clarify in prose that `<Event = ..., Command = ...>` is required syntax in `macro_rules!` to explicitly name the generated types, and that `Trading` serves as a contract group identifier.

---

### Finding m2: Latch Pre-Check Ordering Ambiguity in `ENV-LATCH`
- **Severity:** **MINOR**
- **Citations:** `ENV-LATCH` (lines 450–451), Live Mechanism (line 943).
- **The Issue:**
  `ENV-LATCH` contains two adjacent rules that appear in tension:
  1. *"a publication linearized before an operation's own commitment is taken, marked reported, and returned as that operation's `Err`"*
  2. *"An operation that fails before its commitment is not an observation point: it returns its own Error and a concurrent publication stays pending."*
- **Recommended Fix:**
  Explicitly specify that an operation checks the latch at entry. Rule (2) applies strictly to publications that occur *concurrently during* an in-flight failing admission attempt.

---

### Finding m3: Sequential Join Loop under Unresponsive Port Threatens Shutdown Deadline
- **Severity:** **MINOR**
- **Citations:** `LIVE-SHUTDOWN` (line 920), Live Mechanism (lines 948–950).
- **The Issue:**
  Mechanism line 949 specifies: *"timed wait against the deadline from the monotonic clock for completion publications, joining publishers in Slot order."*
  If Slot 0 hangs (violating `BOUND-BLOCKING`) while Slot 1 exits in 1ms, a naive sequential join (`join(slot_0)` followed by `join(slot_1)`) will block on Slot 0 for the entire deadline duration before ever attempting to join Slot 1.
- **Recommended Fix:**
  State explicitly that `shutdown` waits on the aggregate `completion tracking` primitive (e.g. `Condvar` or channel) until all threads complete or the deadline expires, and only joins threads confirmed finished.

---

### Finding m4: Journal Buffer Object Boundary Validation Edge Guard
- **Severity:** **MINOR**
- **Citations:** Lines 497–498, Line 526, Lines 543–546.
- **The Issue:**
  Section 6 (Mechanism, Step 3) specifies:
  > *"Step 3: Encoded bytes must start with `{` and end with `}` — otherwise `NotAnObject`. Nothing written, nothing poisoned."*
  If an encode failure produces 0 bytes, evaluating `buf[0]` and `buf[len - 1]` without `!buf.is_empty()` would cause an indexing panic.
- **Recommended Fix:**
  Clarify that Step 3 checks `!buf.is_empty() && buf[0] == b'{' && buf[buf.len() - 1] == b'}'`.

---

### Finding C1: Typo in Path Reference
- **Severity:** **COSMETIC**
- **Citations:** Line 4.
- **The Issue:**
  Line 4 references `design_docs/reveiws/` instead of `design_docs/reviews/`.

---

## 5. Architectural Verification of Sound Areas

The adversarial review verified that the following core components of v12 are mathematically and structurally sound:

1. **Typestate Grammar & Affine Certificate (`RUN-GRAMMAR`, `RUN-SERIAL`, `RUN-INDEX`):**
   - The compile-time typestate graph makes illegal transition sequences, skipped checkpoints, premature stop completions, and mismatched outcomes statically unrepresentable.
   - Consuming the `Journal` into the affine `Certificate` at minting ensures that dropping the certificate unconditionally destroys the Journal and diverts execution to `RUN-FINALIZE`.
   - The index bound check against `u64::MAX` in `BetweenTurns` structurally prevents arithmetic overflow before event consumption.
2. **Journal Invariants & Poisoning (`JRN-FORMAT`, `JRN-ENCODE`, `JRN-COMMIT`, `JRN-POISON`):**
   - Encoding directly into bounded preallocated storage guarantees that encoding failures (`Encode`, `NotAnObject`, `BoundExceeded`) produce zero sink I/O and do not poison the writer.
   - Any physical sink error, write zero, or interrupted write permanently poisons the Journal, guaranteeing forensic integrity up to the last flushed record.
3. **Startup Gate Synchronization (`LIVE-START`, `SIM-START`, `ENV-START`):**
   - The start/cancel gate prevents any supervisor shell from invoking `LivePort::run` during initial setup.
   - Startup failure cancels and joins all spawned shells, ensuring that an `Err` from `start` guarantees full quiescence (`Quiesced`) with zero Port code executed.
4. **Deterministic Single-Threaded Simulation (`SIM-STATE`, `SIM-TIME`, `SIM-WAKEUP`, `SIM-STEPS`):**
   - Virtual time monotonicity is structurally enforced by `SimCtx::set_next` checking `time >= now` and selection choosing the minimum armed time.
   - The per-turn step budget bounds loop execution, preventing infinite zero-time or self-rearming cycles.

---

## 6. Actionable Recommendations for v13

1. **Update `dispatch_batch` Signature:** Change `dispatch_batch(env, &[C])` to take `&mut BoundedBuffer<C>` to enable by-value draining into `Environment::dispatch` without requiring `C: Clone`.
2. **Harmonize Sim `on_command` Dispatch Semantics:** In `SIM-DISPATCH`, explicitly define whether `on_command` failure is a pre-commitment dispatch failure at position $k$ or specify how `dispatch_batch` reports the failing position.
3. **Disambiguate Publication vs Completion Tracking:** In `LIVE-SUPERVISION` and `LIVE-SHUTDOWN`, replace "completion publications" with "thread completion signals" to preserve the Glossary definition of *Publication*.
4. **Enforce Latch-First Inspection in `dispatch`:** In `LIVE-DISPATCH` and `ENV-LATCH`, require `dispatch` to check the latch before inspecting inbox capacity, preventing pre-commitment drops from masking earlier crashes.
