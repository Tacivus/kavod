You are performing one adversarial design review of @design_docs/design-v12.md . It is the
sole input: DO NOT READ ANY OTHER FILE, and do not assume any prior review exists. The
document claims to stand alone; if you cannot resolve a question from its text alone, that
inability is itself a finding.

SCOPE
- Semantics of the design document only. Any existing implementation is obsolete and out
  of scope.
- Section 10 (Wiring) is declared open by the document itself. Its openness is not a
  finding. A closed section depending on an open decision counts only if the closed
  section's own text breaks without it.

HOW TO READ IT (before attacking)
1. Read section 0 first and extract the document's own reading rules: which forms bind,
   which tables claim exhaustiveness ("work it does not list does not happen"), how
   citations must work, what the Glossary binds, and the enforcement tiers
   (unrepresentable / asserted / suite-tested).
2. Hold the document to its own rules. A rule living only in nonbinding prose, a term used
   against its Glossary binding, a forward citation outside the stated exemptions, an
   "exhaustive" table missing a case its own rows imply, an ID whose claimed enforcement
   tier cannot actually enforce it — each is a finding.
3. Steelman before you strike. For every candidate finding, first construct the strongest
   reading under which the text is correct. Report the finding only if the defect survives
   that reading — or if the strong reading is one the text does not force, in which case
   the finding is an ambiguity and must be labeled as one.

ATTACK SURFACES (cover all; add your own)
- State machines: walk every state/transition table and every prose state machine through
  every input in every state. Hunt for unlisted transitions, unreachable-but-claimed
  states, reachable-but-forbidden ones.
- Failure paths: for every operation — Err before commitment, Err after commitment, and
  failure of the failure path (cleanup that fails, the final record's commit failing,
  errors during shutdown).
- Orderings and races: wherever the document admits concurrency or a choice of order,
  enumerate interleavings and check each against the guarantees — especially anything
  involving publication, commitment, close, completion, or the word "ordered".
- Arithmetic and bounds: byte arithmetic, capacity edges (0, max, max+1), index and time
  domains, every overflow claim.
- Determinism: try to construct two implementations that each satisfy every written word
  yet produce observably different results from equal inputs. Success means the spec is
  incomplete — show both implementations.
- Realizability: can the API blocks and stated mechanisms exist in real Rust (ownership,
  moves, trait bounds, serde behavior)? If you can execute code, verify serde/std behavior
  claims by running them and mark such findings "executed"; otherwise mark them "reasoned".
- Evidence claims: for every claim that a record, exit, or report "proves", "witnesses",
  or "identifies" something, construct the scenario where the artifact exists but the
  conclusion is false.
- Vocabulary: every Glossary term against every use of it; every ID citation against what
  the cited row actually says.

EVERY FINDING MUST CARRY
- The exact sentence(s) attacked, quoted, with their ID or table/row name.
- One concrete minimal witness with explicit values: a step-by-step trace, a pair of
  quoted contradictory rows, two conforming-but-divergent implementations, or one input
  under its two readings. No witness, no finding.
- A classification: contradiction | ambiguity (two conforming readings) | omission
  (unlisted work in an exhaustive scope) | false claim | unenforceable claim |
  unrealizable | self-conformance violation.
- A severity, assigned by consequence:
  CRITICAL — two binding rules contradict, a guaranteed outcome is unreachable, or the
  design is unsound under its own stated premises.
  MAJOR — two conforming implementations diverge observably, or a binding claim is
  provably false as written.
  MINOR — an ambiguity with one clearly intended reading, or a gap with no behavioral
  consequence yet.
  NIT — wording, citation form, cosmetics.
- A confidence level (high / medium / low), stated separately from severity.

CALIBRATION — READ TWICE
Every claim you make will be independently verified against the text. A rejected CRITICAL
costs your review more credibility than ten missed NITs. Do not inflate severity to be
heard. Never present one plausible bad implementation as the only possible behavior: if
the text permits a correct implementation, the finding is at most an ambiguity. If a
mechanism survives your attack, saying so specifically is as valuable as a finding. If
there are no real findings, say the document is sound and stop — do not manufacture
issues.

PROCESS
You may decompose into sub-agents per attack surface if available. Whatever the process,
the final output is one deduplicated report, and you must re-verify every sub-agent
finding against the text yourself before including it.

OUTPUT — one self-contained markdown report I can save to a file, in exactly this
structure:

# Adversarial Review: <your model/agent name>, <date>
**Target:** design_docs/design-v12.md — quote its `Status:` line verbatim here.
**Verdict:** 2–4 sentences: sound / sound with fixes / unsound, and why.

## Findings
Ordered by severity. Pick a short stable tag for yourself and number findings <TAG-01>,
<TAG-02>, … For each:
### <TAG-NN> <one-line title> — <SEVERITY>, confidence <level>, <classification>
- **Text attacked:** <IDs and exact quotes>
- **Claim:** <what is wrong, two sentences maximum>
- **Witness:** <the minimal concrete scenario>
- **Fix sketch:** <one sentence; optional>
If a severity level is empty, omit it. If there are no findings at all, write "No
findings."

## Attacked and held
The mechanisms you genuinely tried to break and could not — one line each naming the
attack that failed.

## Coverage
One line per document section: walked / skimmed / skipped.

## Questions the document cannot answer
Only questions whose answers are underivable from the text.

Do not summarize the document. No praise outside Verdict and Attacked-and-held. 
