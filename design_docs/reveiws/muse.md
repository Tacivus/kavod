# Adversarial Review: muse-spark-1.2, 2026-08-24
**Target:** design_docs/design-v12.md — quote its `Status:` line verbatim here.
> **Status:** Authoritative (v12). One section is open: Wiring & construction.
**Verdict:** Sound with fixes. The Core graph, Journal, and latch/Environment contracts are coherent and the determinism and failure-path tables are exhaustive under the document's own exhaustion rules. The review finds no contradiction that makes a guaranteed outcome unreachable or the design unsound; the surviving issues are one underspecified Live ordering, one unenforceable "never read" guarantee, and cosmetic marker/self-reference wording.

## Findings
### KAV-01 Live fan-in ordering underspecified — MAJOR, confidence high, ambiguity
- **Text attacked:** `LIVE-EVENTS` row ( §8, line 968): `Event fan-in is one bounded queue. Mapping into the Application Event sum precedes admission.` and `LIVE-SELECT` row ( §8, line 969): `next_event waits, without busy-spinning, until the latch is pending or one Event is available; the choice between them follows ENV-LATCH's publication ordering. The stamp is taken before the dequeue, and the dequeue is the consumption commitment`
- **Claim:** `bounded queue` is not defined in Glossary and no FIFO/LIFO/priority discipline is fixed. Two implementations both satisfy every binding row yet dequeue differently.
- **Witness:** Slots `Primary, Secondary` (§4 ports! example). Initial `State { balance:10 }`. `P1` offers `TradingEvent::Primary(Deposit{10})` and `P2` concurrently offers `TradingEvent::Secondary(Withdraw{10})`; both admitted before `next_event`. Impl A uses `crossbeam::channel` FIFO dequeues `Primary` first → handler `on_event(Deposit)` → `balance=20`. Impl B uses LIFO stack dequeues `Secondary` first → `balance=0`. Both satisfy `LIVE-EVENTS` (`offer` never waits, `Full/Closed` semantics) and `LIVE-SELECT` (stamp before dequeue, nothing fallible after). `DET-RUN`/`DET-ENV` condition equal `trace` ( §7, `DET-RUN` ) does not exclude this divergence because `trace` includes the dequeued `(Event,Timestamp)` sequence — the traces differ — but the spec's intent `A9`/`The Core introduces no choice` is silently delegated to the Environment without fixing the order. Replay via a `SimPort` cannot reproduce Live order without an extra discipline guarantee.
- **Fix sketch:** Add to `LIVE-EVENTS` or `LIVE-SELECT`: `The fan-in queue is FIFO; dequeue order is admission order.` or declare discipline explicitly as implementation choice and include it in `DET-ENV`'s trace.

### KAV-02 `PORT-STATE` "never reading the payload" is unenforceable — MAJOR, confidence high, unenforceable claim
- **Text attacked:** `PORT-STATE` ( §4, line 348): `A Port exclusively owns its mutable domain, protocol, and native state; wiring and the Environment relay its values, routing by the Slot sum's discriminant alone and never reading the payload.`
- **Claim:** The second sentence prohibits inspecting `C::Event`/`C::Command` payloads after discriminating. No available tier can enforce it: not unrepresentable (payload is `Serialize + Send`, inspectable), not asserted (no always-on check), not suite-tested (no `VERIFY-*` pins `PORT-STATE`, invariant index §Appendix lists `PORT-STATE` under Laws but no verification row covers it).
- **Witness:** Two Live `dispatch` implementations both pass `VERIFY-CONFORMANCE` and `VERIFY-LATCH`. Impl A: `match cmd { TradingCommand::Primary(c) => inbox_primary.send(c) ... }` — routes by discriminant only. Impl B: `match cmd { TradingCommand::Primary(c) => if c.amount==0 { log } ; inbox_primary.send(c) ... }` — reads payload (pure, observable only via side log). Both type-check, both satisfy `PORT-ROUTING`'s compiler-exhaustiveness, yet B violates the quoted sentence. Conformance cannot distinguish without an explicit payload-opacity test.
- **Fix sketch:** Downgrade clause to `TRUST-ROUTING` or add `VERIFY-ROUTING` payload-opacity test, or enforce via `PortContract` associated type that is not `Serialize` inside the routing path.

### KAV-03 `JRN-ENCODE` three-byte classifier not exact — MINOR, confidence medium, false claim
- **Text attacked:** `JRN-ENCODE` ( §6, lines 530-532): `The encoded bytes are classified as one single-line JSON object exactly by starting with '{', ending with '}', and containing no newline byte; any other result is NotAnObject.` and `JRN-FORMAT` ( §6, line 519): `One record is one single-line serde JSON object plus one newline`
- **Claim:** "Exactly by" is false for hand-written `Serialize`/`RawValue` passthrough that the document explicitly contemplates. Among valid JSON values only objects start with `{`, but an arbitrary `Serialize` impl could emit bytes that pass the three checks yet are not a valid JSON object.
- **Witness (reasoned, not requiring `raw_value` validation):** Payload `struct Bad; impl Serialize for Bad { fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error>{ struct _A; impl Serialize for _A { fn serialize<S:Serializer>(&self,s:S)->Result<S::Ok,S::Error>{ s.serialize_bytes(b"{not json}") } } } }` is contrived but the document allows `RawValue` passthrough ( §6 Notes: `Raw-passthrough and hand-written Serialize values can produce a non-object or an interior newline`). `serde_json::value::RawValue` constructed from invalid JSON text `"{a:1}"` is rejected by `from_string` (executed, see verification), so the practical exploit is closed, but the normative wording still overstates exactness. A correct classifier must parse or validate JSON, not just check first/last byte.
- **Fix sketch:** Change to `... classified as NotAnObject unless the bytes parse as a JSON object with no interior newline; implementation may use the three-byte fast reject as sufficient for structs but the guarantee is parse-valid object.`

### KAV-04 `RUN-GRAMMAR` / `RUN-ENFORCEMENT` wording on caller-supplied witnesses — MINOR, confidence medium, ambiguity / self-conformance violation
- **Text attacked:** `RUN-GRAMMAR` mechanism ( §7, line 839): `Within RUN-ENFORCEMENT's boundary, a transition requirement is never a caller-supplied witness that can be forgotten, reused, contradicted, or forged: it is the phase itself or work the transition performs.` and `RUN-ENFORCEMENT` ( §7, lines 846-849): `Three points remain runtime: the index arithmetic behind accept_event, backed by one always-on assertion [...], and the answer and batch the Engine passes from the turn it just ran to the single call sites of classify and the batch transition.`
- **Claim:** The first sentence universally denies caller-supplied witnesses inside the boundary; the second admits two caller-supplied witnesses (`answer`, `batch`) exist at single call sites. The strong reading makes the first sentence false; the intended reading is that the boundary is defined to exclude those three points, but the text does not force that reading, so the scope of "within" is ambiguous and `RUN-GRAMMAR`'s list `An out-of-order record, ... a skipped checkpoint, ... is unrepresentable` includes omissions that the affinity note ( §7, line 842) says are `caught by golden-Journal tests, never the compiler`.
- **Witness:** `TurnOpen` at `index=5` with `batch=[cmd]` non-empty. Engine calls `no_commands(&buf)` (recordless edge) instead of `dispatch_batch`. Per `RUN-ENFORCEMENT` the transition asserts empty, so it panics (`ASSERT-INVARIANTS`) rather than being unrepresentable. The omitted `CommandsPrepared` is therefore not unrepresentable but asserted; the `RUN-GRAMMAR` list overclaims unrepresentability for that omission. Dropping the certificate instead of calling either transition is also representable (affinity).
- **Fix sketch:** Qualify `RUN-GRAMMAR` to `... within the boundary (excluding the three runtime points described in RUN-ENFORCEMENT) every non-Fatal illegal state is unrepresentable; omission via dropping is affinity, test-enforced (`VERIFY-JOURNAL`).`

### KAV-05 `RUN-INDEX` versus `RUN-STARTUP` prospective index wording — NIT, confidence medium, ambiguity
- **Text attacked:** Construction/§7 `run` startup row 3 (line 684): `Its stored index is 0 and its stored time is the frozen start time; both are prospective values for the only outgoing edge, not accepted run state, and neither is available to Context until RunStarted commits.` and `RUN-INDEX` (line 767): `The certificate's index is the latest accepted turn's ordinal: 0 for the start turn, advancing exactly when EventAccepted commits.`
- **Claim:** The startup row calls the mating `Initial` certificate's `0` prospective/not accepted, while `RUN-INDEX` calls `0` the latest accepted ordinal for the start turn. The same value is described as both non-accepted and accepted, which is confusing without the qualifier that acceptance occurs only after `RunStarted` commits.
- **Witness:** Observer reads `RUN-INDEX` alone and expects `Context::index()==0` available immediately after `mint`, contradicting startup row's `neither is available to Context until RunStarted commits` and `The Initial phase exposes neither index() nor logical_time()` ( §7, Enforcement note line 792).
- **Fix sketch:** Add to `RUN-INDEX`: `... 0 for the start turn after RunStarted commits; the Initial prospective 0 is not an accepted ordinal.`

## Attacked and held
- Certificate typestate preventing out-of-order records, duplicated `CommandsDispatched`, and caller-supplied `index`/`time`/`candidate` forgery (`RUN-GRAMMAR`/`RUN-ENFORCEMENT`) — holds via module-private types and single-call-site discipline.
- Journal poison and `JRN-COMMIT` flush-boundary: uncertain suffix after `Sink` failure and after abort, replay needs external boundary — holds.
- Latch first-wins, `reported`/`closed` states, `take_error`/`dispatch`/`next_event`/`close` observation-point ordering (`ENV-LATCH`) — holds including overlapping-publication either-side nondeterminism.
- Live `start/cancel` gate preventing `LivePort::run` before activation and joining canceled shells (`LIVE-START`) — holds.
- Live shutdown's one linearized `signal`/`Running`→`Closed`/`fan-in`/`latch` close instant and deadline/ `Complete` wake semantics (`LIVE-COMPLETION`/`LIVE-SHUTDOWN`) — holds as specified.
- Simulated `SIM-LIFECYCLE`/`SIM-START` cleanup of `Open` prefix only, and `step` budget + round-robin equal-time selection (`SIM-WAKEUP`/`SIM-SELECT`/`SIM-STEPS`) — holds, including `step(None)` cursor advance.
- Overflow-marker beating every `Outcome` and `APP-STATE` standing mutation (`TurnOpen` phase) — holds.
- `Timestamp::checked_add` overflow and `EventIndex`/`Timestamp` transparent `u64` serde (executed via `cargo run /tmp/test_ser`) — holds.

## Coverage
- §0 Reading this document — walked
- §1 Glossary — walked
- §2 Laws (A1-A9, `ASSERT-INVARIANTS`, `BOUND-*`, `NO-*`) — walked
- §3 Application contract (`APP-*`, `Context`) — walked
- §4 Port contract (`PORT-*`, `ports!` macro) — walked
- §5 Environment contract (`ENV-*`, commitment table) — walked
- §6 Journal (`JRN-*`) — walked
- §7 The Run (graph, phases, edges, records, `RUN-*`, `DET-*`, certificate mechanism) — walked
- §8 Live Environment (`LIVE-*`) — walked
- §9 Simulated Environment (`SIM-*`) — walked
- §10 Wiring & construction — walked (open, not a finding per scope)
- §11 Crate layout — skimmed
- §12 Obligations & verification (`TRUST-*`, `VERIFY-*`) — walked
- Appendix A Invariant index — skimmed

## Questions the document cannot answer
- From §10 open list: What fixes `Slot` order — registration order vs. `Slot` sum declaration order?
- Where do the frozen fan-in constructors and fan-out match live and how do builders receive them?
- How is each `Environment`'s `Error` sum composed (Kavod variants + per-`Slot` `Port` `Error` mapping)?
- What are the final `LiveCtx` signatures and under which channel primitive is `LiveCtx` built?
- What are `LiveConfig` (shutdown deadline, time origin anchor) and `SimConfig` (origin, step budget) concrete types and where does `SimConfig` live relative to `EngineConfig`?
- What is the crate's `lib.rs` public re-export policy and thread-naming convention, if any?
- From the body: What is the exact fan-in queue discipline (FIFO vs other) for `LIVE-EVENTS` (§8)?
- Does `JRN-ENCODE`'s three-byte check intend to be exact for arbitrary `Serialize` or only for the Run's own record structs (§6)?
