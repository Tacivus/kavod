# Adversarial Review: Claude Opus 5 (1M), 2026-08-23

**Target:** design_docs/design-v12.md — `> **Status:** Authoritative (v12). One section is open: Wiring & construction.`

**Verdict:** Sound with fixes. The graph, the certificate grammar, `RUN-FINALIZE`'s quiescence arms, the latch state machine, `SIM-LIFECYCLE`, and the Journal's byte arithmetic all survived direct attack — I could not construct a Fatal path matching zero or two finalization arms, an undefined (latch state, input) pair, a reachable `commit` on a poisoned Journal, or an off-by-one in the record-bytes bound. The two MAJOR findings are both in the Environment contract, and both are gaps rather than contradictions: §5 declares itself "the complete contract" but leaves two orderings free that change `EngineExit` for identical Port behavior. The remaining findings are self-conformance: the document's own §0 rules — the three prose jobs, the three enforcement tiers, the Glossary bindings, the no-forward-citation rule — are violated in enough places that "hold it to its own rules" is the highest-yield fix pass available.

## Findings

### KV-01 The contract fixes no order between raising the shutdown signal and closing the latch — MAJOR, confidence high, omission

- **Text attacked:**
  - `Environment::shutdown` doc comment (binding under §0 form 1: "a doc comment binds the behavior it states"): "Publishes the shutdown signal, closes admission and the latch, and applies the Environment's bounded quiescence policy."
  - `ENV-SHUTDOWN`: "`shutdown` stops Event delivery, closes Event admission, closes the latch into the report (`ENV-LATCH`), and raises the shutdown signal."
  - `ENV-LATCH`: "Every publication after the first, and every publication after the close, is discarded."
  - §5 preamble: "This section is the complete contract: an implementation satisfying every row here, under `ENV-SERIAL`'s call pattern, is a conforming Environment."
- **Claim:** Neither list is stated as ordered, and no row fixes the order — yet a Port Error caused *by* the signal is captured under signal-then-close and discarded under close-then-signal. The two orders produce different `EngineExit` variants from identical Port behavior.
- **Witness:** Slots `A` (source) and `B` (a logging Port whose `run` returns `Err(FlushFailed)` when its final flush fails after it observes the signal). Application answers `Stop` at turn 3; the turn-3 checkpoint returns `None`; `StopRequested` commits; `StopPending` runs `shutdown`.
  - **Env S1, close-then-signal:** latch closes empty → report `{Quiesced, None}`; signal raised; `B` wakes, flush fails, publishes `FlushFailed` *after the close* → discarded (`ENV-LATCH`). Clean report → `TurnCompleted(Stop)` commits → `EngineExit::Stopped { state }`. Journal ends `…, StopRequested, TurnCompleted`.
  - **Env S2, signal-then-close:** signal raised; `B` wakes inside the window, flush fails, publishes `FlushFailed` → latch pending; latch closes → report `{Quiesced, Some(FlushFailed)}`. Per the `StopPending` row ("Error `Some` → `Environment(Shutdown)`") → `EngineExit::Fatal { cause: Environment(EnvironmentFatal { error: FlushFailed, operation: Shutdown }), quiescence: Quiesced }`. Journal ends at `StopRequested`.
  Divergence covers the `EngineExit` variant, the `FatalCause` variant, `EnvironmentOperation`, and the committed record sequence — all Core-owned. `DET-ENV` cannot catch it: the two runs have unequal traces (the ShutdownReport differs), so the freedom sits upstream of the premise `DET-ENV` quantifies over.
- **Scope note:** Both *shipped* Environments happen to close first — `LIVE-SHUTDOWN` collapses it into "one linearized instant"; `SIM-SHUTDOWN` closes before calling `stop`. This bites only the `TRUST-ENV` / `VERIFY-CONFORMANCE` audience, which is precisely the audience §5 claims to serve completely.
- **Fix sketch:** Add one clause to `ENV-SHUTDOWN` fixing the order (close-then-signal, matching both shipped implementations) and make the doc comment's list match, or state explicitly that a post-close publication is discarded regardless of the signal's placement.

### KV-02 Nothing forces an operation to surface an already-pending latched Error in preference to its own pre-commitment failure — MAJOR, confidence high (gap) / medium (severity), ambiguity

- **Text attacked:** `ENV-LATCH`, three consecutive sentences: "For a call that reaches one of those observation points, a publication completed before the call began orders before the point… A pending Error ordered before the point leaves the latch through the operation's result as fixed by the **Commitment points** table… An operation that fails before its commitment is not an observation point: it returns its own Error and a concurrent publication stays pending." Commitment points, `dispatch`: "`Err` means: This Command was not handed off."
- **Claim:** Sentence 1 is conditioned on *reaching* an observation point, and sentence 3 exempts any call that fails before its commitment — so a call that fails for its own reason while an Error is already pending has two conforming behaviors, and under one of them the first Port Error reaches no record and no exit.
- **Witness:** t0: Port `A` publishes `E`; the publication **completes**; latch pending. t1 > t0: the Run, in phase `Prepared`, calls `dispatch(c_0)` for destination `B`; `B`'s inbox is full (a pre-commitment failure with its own typed Error `InboxFull`).
  - **Reading A** (sentences 1–2): `E` completed before the call began ⇒ orders before the handoff point ⇒ leaves through the result ⇒ `Err(E)`, latch **reported**. Exit: `Environment(Dispatch { position: 0 })` carrying `E`; finalizing `shutdown` report carries `None`.
  - **Reading B** (sentence 3): the call never reaches its commitment point ⇒ not an observation point ⇒ returns `Err(InboxFull)`, latch stays **pending**. Exit: `Environment(Dispatch { position: 0 })` carrying `InboxFull`; the finalizing `shutdown` report carries `Some(E)`, which `RUN-FINALIZE` **discards** ("take the report's quiescence, and discard the report's Error"). `E` — the actual root cause — appears in no record and no exit.
  Sentence 3's qualifier is "a **concurrent** publication", which does not cover a publication completed *before* the call began, so neither sentence governs this case cleanly. The same shape recurs on `next_event`: `ENV-LATCH`'s "A `next_event` call waiting for input returns once the latch is pending" is intransitive and does not say the pending Error is what it returns.
- **Counter-argument, stated fairly:** the two readings agree on every Core-owned discriminant (`FatalCause` variant, `EnvironmentOperation`, `position`, `Quiescence`), and `DET-ENV` explicitly frees Error values, so no determinism row is breached. A reader who weighs only Core-owned outputs would rate this MINOR. I rate it MAJOR because the divergence silently defeats the latch's stated purpose — delivering the first published Error — and `VERIFY-LATCH` claims to prove "permanent first-Error reporting" without being able to decide between the two readings.
- **Fix sketch:** One clause in `ENV-LATCH`: an operation must return a pending Error that orders before its commitment point in preference to any Error of its own, and a `next_event` woken by the latch returns that Error.

### KV-03 Four IDs claim the "enforced" status with no enforcer in any of the three tiers — MINOR, confidence high, unenforceable claim

- **Text attacked:** §0: "Every ID outside the Obligations table is **enforced**: violation is unrepresentable, panics an always-on assertion, or is pinned by a named test suite." §12: "This table is the complete trusted boundary — an obligation absent from it is enforced, not assumed."
- **Claim:** `ENV-BOUNDS`, `ASSERT-INVARIANTS`, `BOUND-LOOPS`, and `NO-UNWIND`'s first clause are code-review rules; none is unrepresentable, none is backed by a named assertion, and none is named by any of the seven `VERIFY-*` rows. Review is a means §12 reserves for Obligations rows (`TRUST-BLOCKING` | "Verified by: Review").
- **Witness:** `ENV-BOUNDS` — "Every operation preserves the Environment's own declared bounds — the registry's rows for the shipped implementations". Enumerate the named suites: `VERIFY-CONFORMANCE` (compares `DET-ENV`'s discriminant list), `VERIFY-JOURNAL` (record sequences and bytes), `VERIFY-FAULTS` (scripted sinks and Environment `Err`s), `VERIFY-GRAMMAR` (compile-fail), `VERIFY-LIVE` (lifecycle/completion/deadline), `VERIFY-SIM` (lifecycle call traces), `VERIFY-LATCH` (latch ordering). None names bounds preservation. A Live implementation whose fan-in queue grows past its configured capacity violates `ENV-BOUNDS` with every named suite green, and no Obligations row covers it. `ASSERT-INVARIANTS` is worse-placed still: no assertion can assert a property of the assertions, and `debug_assert!` compiles.
- **Steelman that partly survives:** `BOUND-LOOPS` reads as a registry whose four clauses are each enforced at their owner (`JRN-POISON`, batch length, `RUN-INDEX`, `SIM-STEPS`), and `ASSERT-INVARIANTS` can be read as a *definition* of the assertion tier rather than a behavioral guarantee — in which case it belongs in §0 or the Glossary, not in a guarantee table. Neither steelman reaches `ENV-BOUNDS`.
- **Severity note:** MINOR as scored (no implementation diverges). If §0's tier list and §12's "complete trusted boundary" are themselves read as binding, this is MAJOR — a provably false completeness claim.
- **Fix sketch:** Either add `TRUST-*` rows for these (upholder: Kavod implementer; verified by: review) or name a suite; `ENV-BOUNDS` for a bespoke Environment already rides `TRUST-ENV`.

### KV-04 Three of four `CoreError` outcomes are produced by binding rows and pinned by no named suite — MINOR, confidence high, omission

- **Text attacked:** `VERIFY-FAULTS`: "A fault-injection suite exercises every edge: scripted sinks for Journal failures and scripted Environments for each operation's `Err` and for a shutdown report carrying `Some(error)`, checking the resulting `FatalCause`."
- **Claim:** The headline says "every edge", but the enumeration that follows reaches neither `Core(ShutdownIncomplete)`, `Core(TimeRegression)`, nor `Core(CommandBoundExceeded)`.
- **Witness:** `Core(ShutdownIncomplete)` requires a report of `{ quiescence: Incomplete, error: None }` — quiescence is not an operation `Err`, and the report carries `None`, not `Some(error)`. `VERIFY-LIVE` covers the Environment producing `Incomplete` ("deadline expiry returns `Incomplete` while detaching every unjoined thread") but says nothing about the Run classifying that report into `FatalCause::Core(ShutdownIncomplete)`; `VERIFY-SIM` cannot cover it ("the report always carries `Quiesced`"); `VERIFY-LATCH` proves only the negative ("`Stopped` follows only a clean report"). Likewise `Core(TimeRegression)` needs a scripted Environment returning `Ok` with a *decreasing* stamp — not an `Err` — and `Core(CommandBoundExceeded)` needs an over-emitting handler, which is neither a scripted sink nor a scripted Environment.
- **Note:** `Core(IndexExhausted)` is a separate and benign case: it is unreachable by construction (2^64 successful flushes, and `RUN-ENFORCEMENT`'s mint assertion blocks a test shortcut), which is the correct use of the unrepresentable tier rather than a suite gap.
- **Fix sketch:** Extend `VERIFY-FAULTS`' enumeration to name a nonmonotonic-stamp Environment, an over-emitting Application, and a `{Incomplete, None}` report.

### KV-05 "Mechanism" is an undeclared fourth prose category, and three Mechanism passages carry implementer obligations — MINOR, confidence high, self-conformance violation

- **Text attacked:** §0: "Everything else is prose, and prose has exactly three jobs: **define** a term, **derive** a consequence from the rules, or **justify** a rule… Test any sentence by deleting it: if an implementer obligation changes, the sentence was a rule in the wrong clothes." §0 again: "A rule in none of the four forms does not exist."
- **Claim:** Sections 3, 6, 7, 8, 9 and 11 carry `### Mechanism` prose that is none of the three jobs, and at least three such passages fail the deletion test — one of them with an observable divergence.
- **Witness (executed):** §6 Mechanism: "`new` computes `max_record_bytes.checked_add(1)` to size the buffer for the object plus the newline." This is the *only* statement of the encode region's size; `JRN-ENCODE` and `JRN-FORMAT` never fix it. Two implementations satisfying every sentence of both rows then diverge. With `max_record_bytes = 60` and a raw-passthrough non-object payload encoding to exactly 61 bytes (`Vec<u8>` of thirty `1`s → `[1,1,…,1]`):
  - Impl A (encode region = max+1, the Mechanism paragraph's shape) → `Err(NotAnObject)`
  - Impl B (encode region = max, newline byte reserved separately) → `Err(BoundExceeded)`
  Controls agree, isolating the divergence to this case: a 61-byte *object* gives `BoundExceeded` under both; a 60-byte object commits under both. Impl B satisfies `JRN-FORMAT` (newline still stored beyond `max_record_bytes`, total storage still max+1); step 4's "no room for it" branch is merely dead. `JournalError`'s variant is Core-owned and is on `DET-ENV`'s compared list.
  Two further deletion-test failures, without divergence: §4 Mechanism, "`Never`'s `Serialize` implementation is `match *self {}`" — `PortContract` binds `type Command: Serialize`, `Never` is Kavod-owned, and `Never`'s API block lists no derives, so without this prose sentence the documented absent-direction pattern does not compile (`error[E0277]`) and no downstream crate can repair it (`error[E0117]`). §11, "the engine module's `mod.rs` re-exports its children's public items rather than exposing the child modules" — delete it and public paths become `kavod::engine::engine::Engine`, in a section that has declared itself "mechanism".
- **Fix sketch:** Either name Mechanism as a fourth prose job in §0 with an explicit nonbinding stipulation, or promote these three sentences (encode-region size into `JRN-ENCODE`; `Never: Serialize` into the API block's derive list; the re-export rule into a guarantee row).

### KV-06 A4's post-close cleanup clause and the `StopPending` row give opposite outcomes for a failed `TurnCompleted(Stop)` commit — MINOR, confidence high, ambiguity

- **Text attacked:** A4: "Once the Fatal cause is fixed, all later run work is likewise best-effort cleanup whose Errors are discarded; **on a run that ends without a Fatal cause, that run-wide cleanup instead begins when the latch closes.**" `StopPending`: "Error `None` with `Quiesced` → the `TurnCompleted(Stop)` edge; failure to commit that record finalizes with the retained `Quiesced`."
- **Claim:** The `TurnCompleted(Stop)` commit is run work performed *after* the latch closed on a run that has no Fatal cause at that instant; A4 classifies such work as best-effort cleanup whose Errors are discarded, while the `StopPending` row turns its failure into a Fatal exit.
- **Witness:** `on_event` at index 7 answers `Stop`. Checkpoint `None`. `StopRequested` commits. `shutdown` runs; report `{Quiesced, None}` — the latch closed with nothing pending and no Fatal cause exists. The Engine commits `TurnCompleted(Stop)`; the sink's `flush` returns `Err(BrokenPipe)`.
  - R1 (`StopPending` literal): `Fatal { cause: Journal(JournalFatal { record_kind: TurnCompleted, error: Sink { operation: Flush, .. } }), quiescence: Quiesced }`.
  - R2 (A4 literal, applied at the instant the latch closes): the flush is post-close run work ⇒ best-effort cleanup ⇒ Error discarded ⇒ `Stopped { state }`, Journal ending at `StopRequested`. R2 is self-consistent — discarding *makes* the run end without a Fatal cause, satisfying A4's own antecedent.
- **Steelman (which I accept):** the antecedent is retrospective, so for this run the clause simply does not fire, and `RUN-GRAMMAR` ("returns its successor only after… successfully committing its listed record") plus the Edges table make `Closed` unreachable without the commit. The graph is decisive — but by two *other* rows, not by A4, and nothing marks A4's clause as retrospective. As written it cannot be evaluated prospectively at the instant it names.
- **Fix sketch:** Scope A4's clause to Environment- and Port-side cleanup ("work outside the Run's own graph"), which is the job it actually does (`ENV-SHUTDOWN` note, `SIM-SHUTDOWN`'s discarded `stop` Errors).

### KV-07 "Bounded quiescence policy" is load-bearing, undefined, and its natural reading is contradicted by the Live realization — MINOR, confidence medium, ambiguity

- **Text attacked:** `shutdown` doc comment (binding): "applies the Environment's **bounded quiescence policy**." `ENV-SHUTDOWN`: "applies its own bounded quiescence policy." Against `LIVE-SHUTDOWN`: "those joins may finish after the deadline because a blocking wait implies no elapsed-time bound (`BOUND-LOOPS`)", and §8 Notes: "if every completion entry is `Complete` and post-completion teardown violates `TRUST-BLOCKING` by never terminating, `shutdown` remains blocked in a join and produces neither a `ShutdownReport` nor an `EngineExit`."
- **Claim:** A "bounded" policy permits an unbounded shutdown; the reconciliation exists only in §8's *Justify*, and §5 declares itself the complete contract.
- **Witness:** A bespoke Environment author reads §5 alone. Two conforming policies: P1 bounds only the wait for outstanding completion state (Live's shape — may block forever in teardown); P2 bounds total elapsed time in `shutdown` including teardown. Both satisfy the words "applies its own bounded quiescence policy"; P1 can hang the process, P2 cannot. The term appears in no Glossary line, despite §1's "One line per term."
- **Fix sketch:** One Glossary line, or one clause in `ENV-SHUTDOWN`: the bound applies to waiting for outstanding activity, not to reclaiming activity already witnessed complete.

### KV-08 The second of `RUN-ENFORCEMENT`'s "three points" has no enforcement tier — MINOR, confidence high, omission

- **Text attacked:** `RUN-ENFORCEMENT`: "Three points remain runtime: the index arithmetic behind `accept_event`, **backed by one always-on assertion** that a freshly minted certificate has the start index, and **the answer and batch** the Engine passes from the turn it just ran to the single call sites of `classify` and the batch transition." And: "The batch transitions **always-on assert** empty or nonempty as their branch requires (`ASSERT-INVARIANTS`)."
- **Claim:** Point 1 gets an assertion and point 3 (the batch) gets an assertion; point 2 — the *answer* handed to `classify` — gets nothing in any of §0's three tiers.
- **Witness:** The Engine holds an `Outcome` and can pass any value at that call site; nothing is unrepresentable, nothing asserts agreement with the turn just run, and no `VERIFY-*` row names it. `VERIFY-GRAMMAR` proves only that "an outcome disagreeing with **the fixed answer**" fails to compile — and the fixed answer is by definition whatever `classify` was handed, so a wrong answer passed in produces a fully type-correct, compile-clean run that commits `TurnCompleted(Stop)` for a handler that answered `Continue`. §0 ("Every ID outside the Obligations table is enforced") and §12 ("an obligation absent from it is enforced, not assumed") both classify this as enforced; it is assumed.
- **Fix sketch:** Either name it in an Obligations row, or add a golden-Journal case to `VERIFY-JOURNAL` pinning the committed `outcome` against the handler's returned `Outcome`.

### KV-09 The Edges preamble says both recordless edges "cannot fail"; the checkpoint edge's transition is fallible — MINOR, confidence high, ambiguity

- **Text attacked:** Edges preamble: "Work its transition performs before the commit can fail as the Phases and Requires rows name; `EventAccepted` alone can fail after acquiring its candidate as `Core(TimeRegression)`. **The two recordless edges commit nothing and cannot fail.**" Against `RUN-GRAMMAR`: "a transition requirement is never a caller-supplied witness that can be forgotten, reused, contradicted, or forged: it is the phase itself or **work the transition performs**."
- **Claim:** The `EffectsComplete → Checkpointed` edge's Requires is "latch snapshot `None`"; `RUN-GRAMMAR` forbids that being a caller-supplied witness, so the transition must perform `take_error` itself — and with the latch pending it consumes the certificate into `Environment(Checkpoint)` and returns no successor. It failed.
- **Witness:** Latch pending at `EffectsComplete`, index 4. `TurnOpen → EffectsComplete` is genuinely infallible (private table, `no_commands`: "asserts the actual reusable batch empty; **infallible**, no commit"). `EffectsComplete → Checkpointed` is not (private table, `checkpoint(env)`: "the `take_error` snapshot; `Some` consumes the certificate into the `Environment(Checkpoint)` path"). One of the two edges the preamble calls infallible has a Fatal outcome. The surviving reading — "cannot fail *to commit*" — is available, but the preceding sentence has just established work-failure as a distinct category, and under that reading "cannot fail" is redundant with "commit nothing".
- **Fix sketch:** "The two recordless edges commit nothing; the empty-batch edge cannot fail, and the checkpoint edge fails only as `Environment(Checkpoint)`."

### KV-10 `LIVE-COMPLETION`'s set does not match the supervisor set after a partial spawn — MINOR, confidence medium, omission

- **Text attacked:** `LIVE-COMPLETION`: "The fixed set has exactly one entry per bound Slot, **matching the frozen supervisor set and order** (`BOUND-STATIC`), is initialized before the start/cancel gate resolves… **Each spawned shell** exclusively owns one module-private, non-cloneable capability that changes only its Slot's entry from `Outstanding` to `Complete`." Live `start` step 2: "Spawn one thread per bound Port in frozen Slot order"; step 4: "Any failure so far: publish cancel, wake and join every shell, return `Err`."
- **Claim:** Thread-spawn failure mid-sequence leaves an entry with no owning shell, permanently `Outstanding` — a terminal state the guarantee's accounting does not contemplate.
- **Witness:** Three bound Slots 0/1/2. Step 1 creates a three-entry set, all `Outstanding`. Step 2 spawns shell 0, spawns shell 1, and the spawn for Slot 2 fails (§10 confirms "thread-spawn failure" is a live Kavod-owned Error variant). Step 4 publishes cancel; shells 0 and 1 exit via gate cancellation → entries 0 and 1 `Complete`; both joined; `start` returns `Err`. Entry set = {0,1,2}; supervisor set = {0,1}. "matching the frozen supervisor set" is false, and entry 2 can never become `Complete` because only a *spawned* shell owns the capability.
- **Consequence is nil:** `start` returned `Err`, so `ENV-SERIAL` forbids any later call, `shutdown` is never invoked, the set is never read, and `RUN-FINALIZE` supplies `Quiesced` from `ENV-START`. The gap is in the guarantee's completeness, not in behavior.
- **Fix sketch:** Say the set matches the *bound Slot* set (which `BOUND-STATIC` fixes) and that entries without a spawned shell are meaningful only after a successful `start`.

### KV-11 `EngineExit` alone cannot separate a failed `TurnCompleted(Continue)` from a failed `TurnCompleted(Stop)` — MINOR, confidence high, omission

- **Text attacked:** §7 Records: "`EngineExit` is the run's only outcome channel". `JournalFatal`: "/// The kind of the record whose commit failed. `pub record_kind: RecordKind`". `RecordKind` has one `TurnCompleted` variant; the record itself carries an `outcome` field, the exit does not.
- **Claim:** Two operationally opposite runs produce byte-identical exits.
- **Witness:** Same Environment, same config.
  - Run A: index 3 answers `Continue`; checkpoint `None`; committing `TurnCompleted(Continue)` fails with `Sink { operation: Flush, .. }`. `RUN-FINALIZE` arm 1: Environment unconsumed → finalization calls `shutdown` → `Quiesced`. Exit: `Fatal { cause: Journal(JournalFatal { record_kind: TurnCompleted, error: Sink{Flush} }), quiescence: Quiesced }`.
  - Run B: index 3 answers `Stop`; `StopRequested` commits; `shutdown` returns a clean report; committing `TurnCompleted(Stop)` fails with `Sink { operation: Flush, .. }`. `RUN-FINALIZE` arm 2, retained `Quiesced`. Exit: **identical in every field**.
  Run B completed all its application business and shut the Environment down deliberately; Run A died mid-stream with an Environment only finalization stopped. The caller's decision (resume vs. do not resume) differs.
- **Steelman:** the Journal disambiguates — Run B's last committed record is `StopRequested`, which can never precede a `TurnCompleted(Continue)`. The gap is in the exit taken alone, which is what an in-process caller with a write-only or remote sink has, and the exit is declared the only outcome channel.
- **Fix sketch:** Carry the outcome in `JournalFatal` (a `TurnCompleted { outcome }` kind, or a separate `RecordKind` per outcome).

### KV-12 The per-Port Command inbox has two owners — MINOR, confidence medium, ambiguity

- **Text attacked:** `LIVE-DISPATCH`: "**Each destination Port owns one bounded Command inbox**; one non-waiting admission to it is where `dispatch`'s handoff commits." Glossary: "**Admission** — entry of a value into a **Kavod-owned** queue or inbox." Bounds registry: "per-Port Command inboxes | **Live Environment**". Ownership map: Port owns "All of its own domain, protocol, and native state". A1: "Every fact has exactly one owner."
- **Claim:** A binding guarantee row says the Port owns the inbox; the binding Glossary definition of the operation performed on it says Kavod owns it; both navigation tables side with Kavod.
- **Witness:** Ask A1's question of one inbox: who defines every way it can change? Under `LIVE-DISPATCH` the Port; under the Glossary and the bounds registry the Live Environment (which sizes it, admits into it, and closes it at shutdown). The Port's only access is `LiveCtx::recv`/`try_recv` — an owner-supplied capability, which is A1's word for *not* being the owner.
- **Fix sketch:** "Each destination Port has one bounded Command inbox, owned by the Environment" — matching the registry and the `Capability` glossary line.

### KV-13 `TRUST-ENV`'s stated verification means cannot check the obligation it states — MINOR, confidence medium, unenforceable claim

- **Text attacked:** `TRUST-ENV`: "A bespoke Environment — one Kavod does not ship — **upholds every Environment-contract row** | Environment author | **The conformance trace suite run against it**." `VERIFY-CONFORMANCE`: "compares every Core-owned discriminant and payload in `DET-ENV`'s list."
- **Claim:** `DET-ENV`'s list (`EngineExit`, `FatalCause`, `EnvironmentOperation` + `position`, `RecordKind`, `JournalError` + `SinkOperation`, `CoreError`, `Quiescence`, plus Journal bytes) cannot witness several contract rows the obligation covers.
- **Witness:** `ENV-BOUNDS` ("Every operation preserves the Environment's own declared bounds") — a bespoke Environment with an unbounded fan-in queue produces identical discriminants and identical Journal bytes on every trace in the suite. Same for `ENV-SEPARATION`, and for `ENV-SHUTDOWN`'s "The Environment itself initiates no further externally consequential work". Compounding it, `VERIFY-LATCH` is *also* described as "An Environment conformance suite", so the definite description "The conformance trace suite" does not resolve to one suite.
- **Fix sketch:** Name both suites in `TRUST-ENV`'s Verified-by cell and add "review" for the rows no trace can witness.

### KV-14 A Core section names both shipped Environments, and the fact it asserts is not derivable from any binding row — MINOR, confidence high, self-conformance violation

- **Text attacked:** §7 Notes: "**Both shipped Environments check the latch before `next_event` selection and `dispatch` handoff**, so an Error pending when either call begins returns first." §0: "Core sections build only on the contracts and **never name an implementation** — earlier mentions of the two shipped Environments are navigation only (the Scope line, the contract's pointer to its implementations, the bounds registry)"; "**Citations point backward.** … a fact that needs a forward reference is in the wrong section."
- **Claim:** The sentence sits in a Core section, names both implementations, is none of the three exempt navigation pointers, and forward-depends on §8/§9 — and the fact is not even derivable from the binding rows it depends on.
- **Witness:** `LIVE-SELECT` says only "the choice between them follows `ENV-LATCH`'s publication ordering". The latch-first behavior appears only in §8's Mechanism, which is explicitly nonbinding ("One workable mechanism, replaceable wherever the guarantees hold"). So a replacement Live mechanism satisfying `LIVE-SELECT` falsifies the §7 derive. Worse, the sentence's framing ("Both *shipped* Environments…") reads latch-first checking as an implementation choice, when KV-02 shows the contract does not force it for anyone — which is exactly the wrong signal to send the `TRUST-ENV` audience.
- **Fix sketch:** Delete the sentence and let KV-02's fix put the requirement in `ENV-LATCH`, where a Core section can cite it.

### KV-15 `BOUND-STATIC` names registration as the Slot-order authority; §10's preferred answer is declaration order — MINOR, confidence medium, ambiguity

- **Text attacked:** `BOUND-STATIC`: "**Slot registration at construction fixes** the Port set — nonempty — and the Slot order: static, not configured, and **fixed nowhere else**." §10: "What fixes the Slot order: registration order, or the Slot sum's declaration order — **declaration order is the candidate that keeps one authority**."
- **Claim:** If declaration order wins, the order is fixed by the `ports!` invocation, not by registration, and `BOUND-STATIC`'s two clauses are both wrong — a closed binding row already prejudges an open decision, and against the option §10 itself prefers.
- **Witness:** §10's own phrase "keeps one authority" concedes that the registration-order option produces two authorities, which is the state `BOUND-STATIC` rules out. Frozen Slot order is load-bearing in five closed rows (`SIM-SELECT`'s cursor, `SIM-START`, `SIM-SHUTDOWN`, `LIVE-START`'s spawn order, `LIVE-SHUTDOWN`'s join order), so the two answers are observably different for the same user program.
- **Steelman:** "fixes" can mean "locks in" (registration must be in declaration order) and "fixed nowhere else" can mean "not at any later time or by configuration" — under which there is no contradiction. That reading is available; it is not forced.
- **Fix sketch:** Reword `BOUND-STATIC` to "Construction fixes the Port set and the Slot order" and let §10 choose the source.

### KV-16 `LIVE-SELECT` bounds the stamp on one side only — MINOR, confidence medium, ambiguity

- **Text attacked:** `LIVE-SELECT`: "**The stamp is taken before the dequeue**, and the dequeue is the consumption commitment (`ENV-ERRORS`): nothing fallible follows it." `LIVE-TIME`: "The single acceptor stamps from one monotonic clock, realizing `ENV-TIME`'s nondecrease structurally."
- **Claim:** Nothing relates the stamp to when the Event arrived; "before the dequeue" admits an arbitrarily early read.
- **Witness:** `next_event` entered at monotonic 1_000 ns; no Event available; Event `E` admitted at 5_000_001_000 ns; the call dequeues immediately after. **L_late** reads the clock immediately before the dequeue → `Timestamp(5_000_001_000)`. **L_early** reads it on entry, before the wait → `Timestamp(1_000)`. Both satisfy "the stamp is taken before the dequeue", both stamp from one monotonic clock, both are nondecreasing across the run. The `EventAccepted` record's `logical_time` differs by five seconds for the same Event, and `Context::logical_time()` differs for the same handler call, in a Journal advertised as "human-readable forensic evidence".
- **Steelman:** `Timestamp::from_nanos`'s doc says "The count's origin and meaning belong to the stamping Environment", so there may be nothing to violate — but that clause is about the *origin*, not the *instant*, and the sentence reads as if it pins one.
- **Fix sketch:** "The stamp is taken immediately before the dequeue, after the wait."

### KV-17 `Prepared` is a binding phase no certificate can occupy — MINOR, confidence medium, ambiguity

- **Text attacked:** Glossary: "**Phase, edge, certificate** — the run's position in its graph, the transitions between positions, and **the value whose possession proves the position**." `RUN-GRAMMAR`: "Every transition **consumes its source certificate** and returns its successor…" Private table: "`EffectsComplete<A>`, **realizing the graph's `Prepared` state internally**."
- **Claim:** The binding Phases table lists `Prepared` and the binding Edges table has an edge whose `From` is `Prepared`, but the realization goes `TurnOpen<A>` straight to `EffectsComplete<A>` — so no certificate value in `Prepared` ever exists, and that edge has no source certificate to consume.
- **Witness:** Nonempty batch of 3 Commands at index 2. Under the Edges table the run passes through `Prepared`; under `dispatch_batch` it never holds a `Prepared` certificate. `RUN-GRAMMAR`'s "possession of the certificate in phase P proves…" is vacuous for P = `Prepared`, and the Glossary's definition of certificate implies each phase has one.
- **Steelman:** the reconciling prose says "The graph's `Prepared` state and both its edges bind the record sequence, which is unchanged" — i.e. the Edges table binds record sequence and failure outcomes, not certificate existence. That re-reading appears nowhere in the binding forms.
- **Fix sketch:** One clause in the Edges preamble: rows bind record sequence and failure outcomes; a realization may fuse adjacent edges that share a source certificate.

### KV-18 `ENV-SHUTDOWN` uses "externally consequential" outside its Glossary scope — MINOR, confidence high, ambiguity

- **Text attacked:** Glossary: "**Externally consequential** — **of a Command**: its delivery causes an effect outside the process." `ENV-SHUTDOWN`: "The Environment itself initiates no further **externally consequential work**, and applies its own bounded quiescence policy."
- **Claim:** The definition is scoped to Commands; the binding row applies the term to Environment *work*, for which the Glossary supplies no meaning — and `TRUST-ENV` makes that an author-facing obligation with no fixed boundary.
- **Witness:** A bespoke Environment's `shutdown` flushes a metrics socket during teardown. Is that "externally consequential work"? Under the Glossary line the term does not apply to Environment work at all, so the row is unevaluable; under the obvious extension ("work whose performance causes an effect outside the process") it is forbidden. Nothing in the document decides it.
- **Fix sketch:** Broaden the Glossary line: "of a Command's delivery, or of any work: it causes an effect outside the process."

### KV-19 "The returned value witnesses that placement" is not checkable for any `Err` return — MINOR, confidence medium, unenforceable claim

- **Text attacked:** `ENV-LATCH`: "…one overlapping the call may order on either side; **the returned value witnesses that placement**." `VERIFY-LATCH`: "for a publication overlapping an observing call, it accepts either placement and **verifies that the call's result and resulting latch state agree with it**."
- **Claim:** For `Err` returns the placement is legible only from the value's *provenance*, which the contract exposes nowhere — `Environment::Error` is an opaque associated type with no required discriminability between "an Error I published" and "an Error this operation minted".
- **Witness:** Latch empty. The Run calls `next_event`. During the call, Event `E` arrives *and* Port `A` publishes `P`. The Environment's duration conversion overflows (`LIVE-TIME`) and it returns `Err(TimeExhausted)`, nothing consumed, `P` pending. The alternative conforming run — `P` ordered before the consumption point — returns `Err(P)` with the latch reported. Both are one `Err` of one opaque type. `PORT-ROUTING`'s per-Slot mapped variants give the two *shipped* Environments enough structure for a test; no row imposes that on a bespoke Environment, so against one `VERIFY-LATCH`'s agreement check has nothing to compare and cannot fail.
- **Scope:** The claim *is* forced and checkable for `Ok` returns (an `Ok` is incompatible with sentence 2 plus the Commitment points table) and for `take_error`'s `Some`/`None`. The defect is confined to `Err`.
- **Fix sketch:** Require `Environment::Error` to distinguish a republished latch Error from an operation-minted one, or scope the sentence to `Ok` returns.

### KV-20 `ENV-SERIAL`'s "`start` exactly once" is violated by the `Engine::new` failure path and by a constructed-but-never-run Engine — MINOR, confidence high, ambiguity

- **Text attacked:** `ENV-SERIAL`: "The contract assumes one serial caller: **`start` exactly once**, first; then `next_event`, `dispatch`, and `take_error` one at a time; **`shutdown` at most once**, consuming the Environment." Construction table preamble: "`Engine::new` … invokes no Application or Environment method; failure is `BuildError`, and no run happened."
- **Claim:** The row deliberately contrasts quantifiers in one sentence, so the literal reading is forced — but `Engine::new` takes `env` by value and drops it with `start` called **zero** times, and nothing in the document covers dropping an Environment that was never started (`ENV-START` covers only drop-after-`start`-`Err`).
- **Witness (executed):** `EngineConfig { max_commands_per_turn: NonZeroUsize::new(usize::MAX).unwrap(), max_record_bytes: NonZeroUsize::new(4096).unwrap() }` → step 1's `try_reserve(usize::MAX)` returns `Err` ("memory allocation failed because the computed capacity exceeded the collection's maximum") → `BuildError::CommandBuffer`, Environment dropped with zero `start` calls. Second path: `max_record_bytes = usize::MAX` → `checked_add(1)` → `None` → `JournalBuildError::MaxBytesTooLarge`. Third path needs no failure at all: `run(self)` is opt-in, so a caller may simply drop the Engine.
- **Fix sketch:** "`start` at most once, and first if at all."

### KV-21 The "complete expansion" of `ports!` is not a valid expansion — NIT, confidence high, false claim (executed)

- **Text attacked:** §4 Mechanism: "`ports!` is a `macro_rules!` macro. **Its complete expansion** for the example above:" followed by `Primary(<MarketData as $crate::PortContract>::Event),`. §4 Notes: "*Justify:* the expansion above is exhaustive, so the two enums are inspectable by eye and **replaceable by hand**."
- **Witness (executed):** `$crate` is a macro-*body* token; expansion replaces it with a crate path. Compiling the block verbatim: `error: expected identifier, found `$`` at `Timer(<Timer as $crate::PortContract>::Command),`. Substituting `kavod::PortContract` compiles clean, including `#[derive(::serde::Serialize)]` over the associated-type projections. The `Justify` explicitly invites a reader to copy the block by hand, which does not compile.
- **Fix sketch:** Print the expansion with `kavod::` (or label the block "the macro body").

### KV-22 "A transition *is* a commit" is false for two of the nine edges — NIT, confidence high, false claim

- **Text attacked:** §7 opening: "Phases carry the work; edges carry the records; **a transition *is* a commit** — the next phase is unreachable until the edge's record commits." Edges preamble: "**The two recordless edges commit nothing** and cannot fail."
- **Witness:** `TurnOpen → EffectsComplete` (empty batch) and `EffectsComplete → Checkpointed` (checkpoint) are transitions that are not commits. The §7 Notes even supply the correction the opening should have carried: "the empty batch and the checkpoint take recordless edges because they bracket no effect".

### KV-23 `Checkpointed`'s Phases row says "the only edge out"; the Edges table gives it two — NIT, confidence high, ambiguity

- **Text attacked:** Phases: "| `Checkpointed` | None; the fixed answer picks **the only edge out**. |" Edges: "| `Checkpointed` | `TurnCompleted(Continue)` | … | `BetweenTurns` |" and "| `Checkpointed` | `StopRequested` | … | `StopPending` |".
- **Witness:** `Checkpointed` appears in the Edges `From` column twice, so the definite description has no referent under the binding tables. It resolves only against the nonbinding private table's `Checkpointed<Continue>` / `Checkpointed<Stop>`, and the declaration "These are typed refinements … not additional graph phases" is made for `TurnOpen`'s refinements only. Contrast the parallel `Initial` row, where the phase genuinely has one outgoing edge. Intended reading is clear: the fixed answer selects which of the two is available.

### KV-24 A binding row cites a section by name because its target has no ID — NIT, confidence high, self-conformance violation

- **Text attacked:** §0: "**Cite IDs.** Never section numbers, here or in tests." `SIM-COMPLETION`: "A run ends normally through the finite-source pattern **(Ports Notes)**."
- **Witness:** The target is §4's un-IDed "*Define:* the finite-source pattern". A by-name pointer is not a section number, but it is also not an ID, and the Glossary — the declared home for definitions — has no "finite-source pattern" line, so the row cannot cite an ID even in principle.

### KV-25 "Subordinate effects stand" is attributed to A4's cleanup rule; the rule is in the Glossary — NIT, confidence medium, false claim (citation content)

- **Text attacked:** Commitment points, `next_event` / `Err` means: "No candidate was consumed; **subordinate effects the implementation names stand (A4's cleanup rule)**."
- **Witness:** A4's cleanup rule reads "that operation's remaining work is best-effort cleanup **whose Errors are discarded**" — it says nothing about effects standing. The actual source is the Glossary's **Commitment point**: "Before it, the operation's contractual effect has not occurred — **subordinate effects its owner names may have, and they stand**." `SIM-SELECT` cites it correctly ("as the named subordinate effects (**Commitment points** table)"). The same misattribution drives §2's "**Failure.** A4's cleanup rule means Fatal performs no rollback", where the work is done by A3 plus the Commitment-point definition.

### KV-26 `VERIFY-GRAMMAR`, a binding row, states its obligation against a private decomposition the graph does not require — NIT, confidence medium, self-conformance violation

- **Text attacked:** `VERIFY-GRAMMAR`: "…any caller attempt to commit `CommandsDispatched` **independently of the fused batch transition**…"
- **Witness:** A non-fusing realization satisfies every binding row: module-private `prepare(env, &CommandBuffer<C>) -> Prepared<A>` (commits `CommandsPrepared`) and `Prepared<A>::dispatch_all(env, &mut CommandBuffer<C>) -> EffectsComplete<A>` (every handoff, then `CommandsDispatched`), both over the one reusable buffer. Same two edges in order, `RUN-GRAMMAR` honored at each, identical record sequence and identical failure outcomes — and it defeats the stated hazard ("With separate prepare and dispatch calls, two independent buffers could commit a `CommandsDispatched` after a partial handoff") because there is one buffer, not two. Yet it has no "fused batch transition" for the compile-fail suite to name.
- **Fix sketch:** "…independently of the transition that performs every handoff."

### KV-27 A binding doc comment uses "publishes" for the shutdown signal; the Glossary reserves it for the latch — NIT, confidence medium, self-conformance violation

- **Text attacked:** Glossary: "**Publication** — entry of an **Error** into the latch." `Environment::shutdown` doc comment: "**Publishes the shutdown signal**, closes admission and the latch". Also `LIVE-START` steps 4/5: "publish cancel", "Publish start".
- **Witness:** `ENV-SHUTDOWN` uses the disciplined verb for the same act — "**raises** the shutdown signal" — which shows the document knows the distinction and drops it inside the binding API block, where §0 says the doc comment binds.

### KV-28 `SIM-STATE` names no contract row and defines no Port-facing API — NIT, confidence high, self-conformance violation

- **Text attacked:** §0: "A Live or Simulated guarantee **either names the Environment-contract row it realizes or defines that implementation's Port-facing API**", restated at both section heads.
- **Witness:** `SIM-STATE`: "Each simulated Port owns all of its simulated domain state; the Environment holds no shared model and runs no concurrency." No ID cited, and it touches no `SimPort`/`SimCtx` item — it restates `PORT-STATE` and `ENV-SEPARATION` without saying so.

### KV-29 `Trace` ascribes an "Ok count" to `flush`, which returns none — NIT, confidence high, ambiguity

- **Text attacked:** Glossary, **Trace**: "and every sink call's result (**one write or flush call: its Ok count**, or its failure's presence)".
- **Witness:** `std::io::Write::flush(&mut self) -> io::Result<()>`. A successful flush has no count. Since `DET-RUN` and `DET-ENV` both quantify over "the trace", and `JRN-COMMIT` makes the flush the commit point, flush results are exactly the ones that must be in it. The disjunctive reading is obviously intended; the wording does not distribute.

### KV-30 "Frozen" is declared the only ordering authority and has no Glossary line, in three senses — NIT, confidence high, omission

- **Text attacked:** §10 Constraints: "**frozen Slot order as the only ordering authority**." §1: "One line per term. These definitions are normative."
- **Witness:** Three senses, none defined: `PORT-SUMS`, "applying the **frozen fan-in constructors**" (compile-time-fixed items); `SIM-SELECT`, "scanning from the cursor in **frozen Slot order**" (fixed at construction); run startup step 3, "consuming the Journal and the **frozen start time**" (fixed at one runtime instant).

### KV-31 `SIM-SELECT` states wrapping for the scan but not for the cursor advance — NIT, confidence high, ambiguity

- **Text attacked:** `SIM-SELECT`: "the selected Slot is the first lowest-time armed Slot met scanning from the cursor in frozen Slot order, **wrapping**; the cursor … **moves to the selected Slot's successor** after every selected `step`".
- **Witness:** Three Slots 0/1/2, cursor = 2, arms `S0@10, S1@10, S2@10`. Scan from 2 selects `S2`; `step` returns `Some(E)`. "Wrapping" is attached to the scan clause, so the literal successor of Slot 2 is Slot 3, which does not exist. No schedule diverges — the next scan wraps anyway, so cursor `N` and cursor `0` select identically — but the arithmetic is unstated.

### KV-32 `LIVE-SHUTDOWN` claims a four-way linearized instant; the Mechanism serializes two — NIT, confidence medium, omission

- **Text attacked:** `LIVE-SHUTDOWN`: "in **one linearized instant** it publishes the signal, ends `Running`, closes the fan-in, and closes the latch". Mechanism: "the lifecycle cell flips and the latch closes **under the latch lock**, one linearized instant".
- **Witness:** Fan-in is "one bounded channel" with its own synchronization; the latch is a separate `Mutex` + `Condvar`. The Mechanism names only two of the four as atomic. The strongest reading rescues it — `LIVE-EVENTS` *defines* the fan-in close as the signal being raised ("The fan-in closes when `shutdown` publishes the signal"), and `offer` is a `LiveCtx` method that would consult the lifecycle cell, so the flip is the close — which is why this is a NIT and not a defect in the guarantee. The Mechanism, whose job is to show the guarantee is realizable, does not say so.

## Attacked and held

- **`RUN-FINALIZE`'s quiescence arms.** I enumerated all 17 Fatal-producing points — startup step 2, startup step 4, `TurnOpen`×2, `Journal(CommandsPrepared)`, `Dispatch{k}`, `Journal(CommandsDispatched)`, `Environment(Checkpoint)`, `Journal(TurnCompleted)` on Continue, `Journal(StopRequested)`, `IndexExhausted`, `Environment(NextEvent)`, `TimeRegression`, `Journal(EventAccepted)`, `Environment(Shutdown)`, `Core(ShutdownIncomplete)`, `Journal(TurnCompleted)` after a clean report. Every one matches exactly one of the three arms; the guards are pairwise disjoint. No path matches zero, none matches two.
- **`ENV-LATCH`'s latch machine is total.** Both discard clauses are load-bearing and both are present: "Every publication after the first" covers publication into `pending` and into `reported`; "every publication after the close" separately covers a first-ever publication arriving after a close from `empty`. The hole I expected is plugged. `take_error` in `reported` and in `closed` are unreachable — the first because `ENV-SERIAL` allows only `shutdown` after `take_error` returns `Some`, the second because `shutdown(self)` consumes the Environment.
- **`SIM-LIFECYCLE` is a total (state, method) matrix.** All twelve pairs decided. A `NotStarted` Port at `shutdown` is unreachable: its only producer is a `SIM-START` failure, after which `ENV-SERIAL` forbids `shutdown` entirely.
- **Selection never meets an `Ended` Port.** I tried routing a Command to a Port whose `on_command` had already failed: `SIM-DISPATCH`'s pending-Error check fires first, and reaching `next_event` with a stale arm requires surviving the checkpoint, which `RUN-CHECKPOINT` prevents. "A stale arm is unreachable, not forbidden" is exact.
- **Journal byte arithmetic has no off-by-one.** Buffer = `max_record_bytes + 1`. An object of exactly `max` commits (max+1 bytes written); one of `max+1` fills the buffer, passes classification, and fails at step 4; one of `max+2` fails at step 2 by zero progress. Both boundary cases yield `BoundExceeded`, so `JRN-FORMAT`'s "bounds the encoded object of every committed record" holds with no seam. `checked_add(1)` overflows only at `usize::MAX`, matching `MaxBytesTooLarge` exactly (executed: `usize::MAX-1 → Some`, `usize::MAX → None`).
- **`JRN-POISON`'s panic is unreachable from the Run.** Every commit failure drops the certificate, which destroys the Journal; the only two-record transition is split by the Edges table; `Encode`/`NotAnObject`/`BoundExceeded` "poison nothing". The panic exists solely for direct public `Journal` consumers, who have `is_poisoned()`.
- **`RUN-INDEX`'s domain edge is exact.** Indices 1..=`u64::MAX` for External Events plus the start turn at 0 = exactly the `u64` domain, and the check sits before `next_event`, so no candidate is consumed. Index 0 ⟺ the start turn never wraps.
- **The record wire format is exactly as claimed (executed).** Derived struct field order is declaration order, and a payload built in Records-table order produces `{"record_kind":"RunStarted","index":0,"schema_version":1,"logical_time":100}` byte-identical to the document's example. `EventIndex`/`Timestamp` derive to transparent `u64`s despite private fields. `outcome` from a fieldless enum emits a bare tag string.
- **`JRN-ENCODE`'s newline classification is sound against real `serde_json` (executed).** Over `"line1\nline2"`, `"tab\there"`, `"héllo → 日本語 🎉"`, U+0085/U+2028/U+2029, `"\r"`: newlines and control characters are escaped, non-ASCII is emitted raw as UTF-8, no output contains byte 0x0A. A raw byte scan is safe because 0x0A can never appear inside a UTF-8 multi-byte sequence.
- **Every API block is realizable in real Rust (executed — a full skeleton was compiled and run).** `Certificate<W,P>` owning the `Journal<W>` while `dispatch_batch` commits `CommandsPrepared` from a shared view and then drains by value type-checks (disjoint borrows; the payload drops before the mutable borrow). `run(self)` destructuring so `close(env)` can move the Environment while other transitions borrow it works. `Context<'a,C>`'s overflow marker is readable after the handler returns under NLL. The `ports!` matcher is writable as `macro_rules!`; the invocation creates no item named `Trading`; `#[derive(::serde::Serialize)]` over associated-type projections needs no extra bounds; a `Never` arm is both permitted and omittable on rustc 1.96.1 with no `unreachable_patterns` warning.
- **Variant reachability is complete.** All 6 `RecordKind`, 5 `EnvironmentOperation`, 4 `CoreError`, 4 `FatalCause`, 4 `JournalError`, 2 `JournalBuildError`, 2 `BuildError` have a reachable path. No guaranteed outcome is unreachable. `EngineExit::Stopped` can never carry `Incomplete`.
- **The `Running`/latch-close race is genuinely linearized.** I could not construct a lost update: a premature classification before the instant publishes into a still-open latch; one after does not publish at all. The completion entry flipping later than the publication is harmless because shutdown waits on the entry, not the publication.
- **`LIVE-SHUTDOWN`'s deadline machine is total.** All-Complete at the close, completion during the wait, expiry with an outstanding entry, and a completion racing expiry (settled by "one final synchronized observation") each have an outcome, and the `Quiesced`-then-join tail is explicitly excused with its non-terminating consequence derived rather than hidden.
- **`ENV-LATCH`'s ordering freedom cannot change Journal bytes.** The choice manifests only in returned values, and returned values *are* the trace; `RUN-RECORDS` admits no Error value onto the wire. Given equal traces, Journal bytes are equal without needing `DET-ENV`'s escape clause — and that clause is not circular: it excludes only shapes one type cannot express.
- **`SIM-STEPS`' off-by-one.** The unit-accounting sentence defines the predicate and the check sentence only fixes the gate's position, so a budget of *B* forces exactly *B* `step` calls per `next_event`. Walked B=1 both ways. No divergent conforming reading.
- **`SIM-TIME`'s monotonicity is inductive, not asserted.** Selection takes the minimum arm, `now` becomes that minimum, every surviving arm is ≥ it, `set_next` requires `time >= now`, and `dispatch` does not advance `now`.
- **`ENV-ERRORS`' naming duty is discharged four times out of four** — `LIVE-START`, `LIVE-SELECT`, `SIM-START`, `SIM-SELECT`.
- **A5 vs `RunStarted`/`EventAccepted`.** Activation and candidate consumption both precede their records, but the records announce the *handler call*, not those effects, and both consequences are derived explicitly. No A5 breach.
- **Appendix A is exact.** All 76 IDs (A1–A9 plus 67 named) appear in exactly one row; none indexed twice, none missing, none undefined. No citation by section *number* appears anywhere in the document.
- **`Stopped` ⇒ clean report holds structurally.** `Closed` has exactly one incoming edge whose Requires is "a clean report", which is why `EngineExit::Stopped` carries no `quiescence` field.
- **`Dispatch { position: k }` prefix semantics survive the `ENV-LATCH` case.** The Commitment points table fixes `dispatch` `Err` ⟹ "This Command was not handed off", so the prefix `[0,k)` claim holds even when the Error is an unrelated latched one.
- **The Environment commitment table's exhaustiveness survives.** Its scope is the five trait operations' outcomes; concurrent Port work is governed by the Guarantees table, not smuggled past "work it does not list does not happen".

## Coverage

- §0 Reading this document — walked (used as the primary weapon)
- §1 Glossary — walked (every term checked against its uses)
- §2 Laws — walked
- §3 Application contract — walked
- §4 Port contract — walked (macro expansion executed)
- §5 Environment contract — walked (both MAJORs here)
- §6 Journal — walked (arithmetic and serde behavior executed)
- §7 The Run — walked (graph, records, enforcement, all Notes)
- §8 Live Environment — walked
- §9 Simulated Environment — walked
- §10 Wiring & construction — skimmed (declared open; consulted only where closed text depends on it)
- §11 Crate layout — walked
- §12 Obligations & verification — walked (every `VERIFY-*` row mapped against every enforced ID)
- Appendix A — walked (full ID reconciliation)

## Questions the document cannot answer

1. Does the shutdown signal reach Ports before or after the latch closes? (KV-01) The document states the actions in opposite orders in two binding places and fixes neither.
2. When an operation fails for its own reason while an Error is already pending in the latch, which Error does it return, and does the latch stay pending? (KV-02, KV-19)
3. What does "bounded" require of a bespoke Environment's quiescence policy — bounded waiting, or bounded total elapsed time in `shutdown`? (KV-07)
4. How large is the Journal's encode region: `max_record_bytes` or `max_record_bytes + 1`? Only nonbinding Mechanism prose says. (KV-05)
5. Who owns a per-Port Command inbox — the destination Port (`LIVE-DISPATCH`) or the Live Environment (bounds registry, Glossary `Admission`)? (KV-12)
6. Which suite, assertion, or unrepresentability enforces `ENV-BOUNDS` and `ASSERT-INVARIANTS`? (KV-03)
7. What fixes the Slot order — registration or declaration? `BOUND-STATIC` says registration; §10 prefers declaration. (KV-15)
8. Relative to an Event's arrival, when may a Live Environment read its clock for that Event's stamp? (KV-16)
