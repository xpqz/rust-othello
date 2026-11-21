# Review of the opening-book follow-up proposal

Nov 21, 2026. This is a review of the **original** draft of
`opening-book-v2.md`. That draft proposed a 128-position verified
candidate and a 128-pair holdout gate at 500,000 nodes per move. The
revised draft, committed with this file, adopts the findings: the first
experiment is diagnostic only, and the fixed candidate and gate are
dropped. This file records why.

## Verdict on the original draft

The diagnosis was sound, and the trace and diagnostic stages were worth
doing. The candidate and gate as designed would likely have measured
search depth rather than the book, and would probably have come out
inconclusive by construction.

## What held up (checked against the code and records)

- **Diagnosis:** drop-outs were scored at depth 18 with ProbCut on,
  against values backed up through unevenly extended lines. The three
  extension rounds were judged on coverage only. The node check
  credited no saved time and started after 6–8 placements.
- **Machinery** the proposal relied on, all of which exists:
  - per-call node caps, with the last completed iteration returned, and
    the `Partial` and `Fallback` statuses;
  - ProbCut-off configurations;
  - `--pair-range`;
  - node control without a load rule;
  - a 0-round rebuild that reproduces the first book byte for byte.
- **Tracing** both books against the baseline, with the first differing
  choice per game, is the right first step and is cheap.
- **The first-disagreement diagnostics** separate unstable choices from
  strong replies that the backed-up lines missed.

## Major findings

1. **The verified book could only repeat a 2-million-node search, and
   the holdout played at 500,000 nodes.**
   - A move was kept only if fresh 0.5M- and 2M-node searches agreed
     with it, so the book held exactly what a 2M-node search plays.
   - A match at 500,000 nodes per move then gives the book side a
     four-times-deeper search in those positions. A pass would mainly
     show that deeper search wins in the opening.
   - At real controls (51 s per game, roughly 20 M nodes/s) live search
     already exceeds the verifier, so such a book adds no move quality.
     Its value is saved time, which the gate did not credit.
   - **Remedy:** verify well above play effort, or test where saved
     time counts.
2. **The holdout would rarely reach the book.**
   - The candidate was at most 128 positions after 0–8 placements,
     ranked by frequency in traces from 24 development roots. The
     holdout roots were fresh, started after 4–8 placements and came
     from separated opening families.
   - Few holdout games would pass through a stored position, so the
     "at least one book decision per game" requirement was likely to
     fail by design.
   - **Remedy:** start where the book lives, from early positions or the
     standard start with varied early moves, and measure coverage before
     the strength run.
3. **128 pairs was underpowered.**
   - The 400-pair node check had an interval half-width of about 0.02,
     so 128 pairs gives about 0.037. Passing a lower bound above 0.475
     needs an observed score of about 0.512 or more.
   - A book whose benefit is mainly time, not credited here, would sit
     near 0.500 and be inconclusive.
   - **Remedy:** size the gate from the historical pair variance with a
     timing probe.

## Minor findings

- "Skip single-legal-move positions, which the clock policy handles"
  does not hold in node-control matches (`pc:80 --nodes …`), which use no
  clock policy and search lone moves.
- The time-slice machinery (15-minute slices, persisted budgets,
  conservative debits, a supervisor) was heavy for an experiment of
  roughly 30 minutes of compute. The repository's resumable
  node-controlled jobs already survive interruption.
- A missing hypothesis: drop-out values are inflated by taking the
  maximum over noisy searches, which makes leaving the book look better
  than it is.
- Tracing needs instrumentation, because `Agent::choose` returns only a
  square. Recording source, nodes and key per decision needs a dedicated
  example built on `book::play`'s result. It is outside the budgets but
  was not sized.

## How the revised draft answers these

| Finding | Revised draft |
|---|---|
| Verified book repeats shallow search | The 500,000/2-million rule and 500,000-node gate are dropped. A move-quality gate must match the node cap to measured 51 s play effort and verify substantially above it. |
| Holdout misses the book | Diagnostic roots include 12 at 1–4 placements and the standard start. A later gate must sample a declared early-opening distribution that exercises the book, with coverage estimated before the strength run. |
| Underpowered gate | A gate is registered only after a power estimate from the historical variance and a timing probe fit the budget. The note that 128 pairs would usually fail a true 50% book is recorded. |
| Lone moves in node mode | Recorded explicitly: node mode searches them, and the variant-3 clock bypasses search. |
| Heavy scheduling | Replaced by resumable node-controlled batches, a 30-minute total ceiling across restarts, and no custom scheduler. |
| Maximum-of-noise hypothesis | Added, as a selection effect to measure. The searches share a table within a node, so no independence of errors is assumed. |
| Unsized instrumentation | A dedicated example of about 100–150 lines that tags hits and records reported nodes, with a traced/untraced equivalence fixture. |
