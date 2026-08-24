You are adjudicating and synthesizing twelve independent adversarial reviews of
@design_docs/design-v12.md . The reviews are:

@design_docs/reveiws/deepseek.md
@design_docs/reveiws/fable.md
@design_docs/reveiws/gemini.md
@design_docs/reveiws/glm.md
@design_docs/reveiws/grok.md
@design_docs/reveiws/kimi.md
@design_docs/reveiws/muse.md
@design_docs/reveiws/opus.md
@design_docs/reveiws/ox.md
@design_docs/reveiws/sol.md
@design_docs/reveiws/sonnet.md
@design_docs/reveiws/terra.md

All reviewers received the same instructions (@design_docs/reveiws/review-prompt.md) and
saw only the design document. Read that prompt first: its severity ladder, classification
vocabulary, and witness requirement are your vocabulary too.

GROUND TRUTH
The design document is the only arbiter. A review is testimony, never evidence. Every
verdict you issue must be re-derived from the document's text yourself — quote it. Two
failure modes are equally fatal here:
- Consensus laundering: ten reviews repeating a finding is not ten pieces of evidence.
  They shared a prompt and a document; a common misreading propagates. Verify the popular
  findings as skeptically as the lonely ones.
- Solo dismissal: a finding raised by exactly one review is where the most careful
  reviewer saw something the others missed — or hallucinated. These get MORE scrutiny
  time, not less. Never reject a finding because of its vote count, only because the text
  refutes it.
Where one review's Findings contradict another review's "Attacked and held" (one broke a
mechanism another certified as sound), that is a direct dispute: re-run the attack
yourself and say who was right.

PROCESS
You are encouraged to decompose into parallel sub-agents where available — e.g. one per
review for the inventory pass, one per theme for adjudication, one per disputed cluster
to re-run an attack. Whatever the decomposition, you personally re-verify every
sub-agent verdict against the design document's text before including it, and the final
output is one report in one voice.
1. Read the design document in full before opening any review.
2. Inventory: extract every finding from every review — tag, one-line title, severity,
   classification, the IDs/sections attacked. Every finding in this inventory must appear
   exactly once in your output's disposition index. Nothing is dropped silently.
3. Cluster by ROOT CAUSE, not by surface. Two findings attacking different sentences are
   the same finding if one fix resolves both; two findings quoting the same sentence are
   different findings if they need different fixes. When in doubt, ask: "would the fix
   for A make B moot?" Yes → same cluster.
4. Group clusters into THEMES — the mechanism or contract they stress (e.g. "shutdown
   ordering", "glossary self-conformance"), not the section number. A theme with five
   clusters that all trace to one underspecified contract should say so: the deliverable
   is the shape of the problem, not the pile of instances.
5. Adjudicate each cluster against the text:
   - Steelman the document first, exactly as the reviewers were told to steelman it. If
     the strongest reading survives the best witness in the cluster, the cluster is
     rejected or downgraded to ambiguity — regardless of how many reviews raised it or
     how confident they sounded.
   - Assign YOUR severity and classification from the review-prompt ladder. Reviewer
     labels are claims to check, not defaults to average. A "MAJOR" with no divergence
     witness is not a MAJOR; a "NIT" that makes two binding rules contradict is not a
     NIT. Say when you moved a severity and why in one clause.
   - Keep the single best witness (sharpest, most minimal); discard the rest. If every
     witness in the cluster is flawed but the underlying defect is real, construct a
     correct one yourself and mark it yours.
   - If you can execute code to settle a realizability/serde/std dispute, do so and mark
     the result "executed".
6. Write the fix direction per confirmed cluster — one or two sentences on what change
   resolves it, noting when one fix closes several clusters. Do not draft document text.

CALIBRATION
Your output will drive edits to an authoritative document. A wrongly-confirmed finding
costs an edit cycle; a wrongly-rejected one ships a defect. When genuinely uncertain
after working the text, say "unresolved" with the two readings — do not force a verdict.
Expect a large fraction of raised findings to die in adjudication; if none do, you are
averaging, not adjudicating.

OUTPUT — one self-contained markdown report:

# Review Synthesis: design-v12, <date>
**Inputs:** 12 reviews, <N> raw findings → <M> clusters → <K> confirmed.
**Verdict:** 3–5 sentences: the document's overall health, the one or two themes that
dominate the confirmed findings, and what the reviews collectively could not break.

## Confirmed findings
Grouped by theme, each theme ordered by severity. Number clusters SYN-01, SYN-02, …
### SYN-NN <title> — <SEVERITY>, <classification>
- **Sources:** <review: tag, review: tag, …> (or "sole: <review: tag>")
- **Text:** <IDs and exact quotes from the design document>
- **Adjudication:** <why it's real; note severity moves and merged findings in one
  clause each>
- **Witness:** <the one best witness>
- **Fix direction:** <one or two sentences>

## Disputes resolved
Findings-vs-held conflicts and severity fights worth recording: who claimed what, what
the text says, who was right. One short paragraph each.

## Rejected findings
One table: | finding(s) | claim | why the text refutes it |. Group identical rejections
into one row. Be specific enough that the rejection is final — this table is what stops
the issue from being re-raised next round.

## Unresolved
Clusters you could not settle from the text: the two readings and what would settle it.

## Held under fire
Mechanisms that multiple reviews attacked from different angles and nobody broke — one
line each. This is the document's verified core; it earns the same rigor as the
findings.

## Disposition index
One line per original finding: <review: tag> → SYN-NN | rejected (row ref) | unresolved.
Completeness check: every tag from every review appears exactly once.
