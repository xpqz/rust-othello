# Opening book follow-up

Status: revised proposal. The first deliverable is a diagnostic report.
A replacement asset and its shipping gate require a follow-up design
based on that evidence.

## What went wrong

The first book was reproducible and cheap to look up. Its 7,729 entries
gave it substantially more reach than the initial 3,771-entry book.
Reach was the metric used to judge the three extension rounds. There
was no strength comparison between the initial book and the extensions.
The final book scored 47.81% on the 400 frozen book roots at a common
10-million-node cap per move. Its 95% pair-bootstrap interval was
[45.75%, 49.88%]. It failed the registered shipping rule.
[Experiment report](../book_report.md) · [Gate record](../prs/156.md)

The leading hypothesis is that some stored choices were worse than the
moves live search would have found. We chose nominal depth 19 to fit
the build budget. The builder compared values backed up through unevenly
extended lines with values from depth-18 selective searches of exits.
A narrow win in that comparison gave the stored move the same runtime
authority as a clear win. Extension spent compute where the chosen
line left the tree, with no direct check that the new line improved play.
Taking the maximum of noisy drop-out evaluations can also favour exits.
The searches share a table within a node, so this is a selection effect
to measure, with no assumption of independent errors. These mechanisms
are hypotheses. The game results do not identify which
moves or kinds of positions caused the loss.
[Scoring and extension design](book.md)

The original clock match accepted 21 pairs in 4.8 hours and logged
397 void events. Load-triggered replay made progress depend on the
machine becoming quiet. The replacement node match completed under
ordinary use, but its per-move budgets could not carry saved work into
later searches. Its six- to eight-placement roots also bypassed the
first few moves. The result is evidence about this book at those
controls. Its effect from the normal start under the game clock remains
open. [Gate record](../prs/156.md)

Logistello used played variations as material for searching deviations.
Edax expands alternatives near the scores of the current position and
the book root, then backs up the revised values. The lesson for this
engine is to spend offline work on competing moves and strong replies
that could change a decision. The existing packed lookup already
supports a compact book that answers only at selected positions.
[Buro, *Toward Opening Book Learning*](https://skatgame.net/mburo/ps/book.pdf) ·
[Edax deviation search](https://github.com/abulmo/edax-reversi/blob/master/src/book.c#L1865-L1907)

## First experiment

Compare the original book, the three-round book and book-off on the
same openings. Use the checked-in three-round binary asset and reproduce
the original from the zero-round build or its manifest. Check each
asset's bytes against its recorded identity before tracing. Keep the
book off in the default engine configuration.

- Freeze 24 diagnostic roots. Take 12 at 1–4 placements, spread across
  early opening families, and 12 from the original 400 roots. Include the
  standard start once as a separate smoke case. Pick roots by stable
  order or fixed seed before inspecting moves or hits. The historical
  roots are useful for diagnosis and are already exposed to results.
- Trace colour-swapped games for each book against book-off and replay
  the book-off reference for the same opening and colours. Use 250,000
  nodes per move, identical search settings and the existing node-mode
  table lifecycle. Pin one engine build, both assets, root order, node
  caps and seeds. For each book version and colour, compare its moves
  to the reference only up to the first divergence. Treat choices in the
  same orbit under the current board's stabiliser as equivalent and
  record exact-square differences separately.
- Replay at most two book-loss pairs from #156 at the original
  10-million-node cap, chosen by ascending pair ID. Check the reproduced
  scores against the journal. This connects the cheap traces to the
  failed experiment. The chosen losses are case studies.
- Audit up to 16 first disagreements across the two book versions
  and root groups, deduplicated by canonical board and pair of moves.
  Allocate up to four per version and root group in frozen root and
  colour order. First inspect the exact-control replays as additional
  context for this report. For each audited position, search both
  children at 500,000 and 2 million nodes each with fresh tables and
  ProbCut disabled. Use the same pass and terminal scoring rules as the
  builder and record the strongest reply, result status and score from
  the mover's perspective. Flag reversals between the budgets as
  unstable. Give exact outcomes precedence where both children solve.
- Where the journals supply comparable drop-out estimates, record
  the chosen exit's original score and the best alternative's score
  alongside the fresh scores. Report how often the original winning
  estimate falls on re-search. This can flag optimistic selection or a
  missed reply for follow-up. The fresh evaluations are heuristic on
  unsolved positions, and first disagreements from a small diagnostic
  sample cannot establish how much each mechanism cost in the match.

Report coverage and first-hit ply by root group and version, first
disagreements by ply and move type, whether the original and extended
books choose differently, and the audit evidence for deeper follow-up.
Report the observed game scores as descriptive data.

### Instrumentation

> **After #173:** the engine's `book::play` and `book::lookup` are gone.
> To reproduce, use `book_trace --book PATH`, where PATH is
> `docs/tuning/book_ext/book.bin` (r3, formerly embedded) or
> `docs/tuning/book_diag/book_r0.bin`. The example carries the lookup
> verbatim as `book_lookup`.

Add a dedicated experiment example using the existing board traversal,
seeded pair harness and journal where practical. A trace agent can call
`book::play` with the embedded asset and `book::lookup` with the original
asset, then use the same `search_tt` path on a miss. Tag hits explicitly.
`Agent::choose` alone exposes only the returned square. Record legal-move
count, source, move, board key, depth, status and reported search nodes
for each decision. The current midgame result can omit nodes spent in an
aborted iteration, so label that column as reported nodes. Detect lone
legal moves separately. Node mode can search them, while the playable
variant-3 clock bypasses search.

Size the example at roughly 100–150 lines plus a small replay and
journal fixture. It adds no engine or playable CLI lines. Confirm
that a no-trace fixture and its traced counterpart return the same
moves and scores, including a book hit, a miss, a pass and a lone move.
Treat a mismatch in the exact-control replay as a diagnostic finding
and resolve it before attributing losses to book choices.

### Completion rules

Use the existing resumable node-controlled workflow and two workers
by default. Load never voids a pair or triggers a replay. The proposed
ceiling is 30 active minutes across the diagnostic runs, including
repeats and restarts. The fixed root and audit counts bound the work
independently of wall time. Use small `--pair-range` batches and a
single deadline for each invocation, bounded by the remaining
allowance. Stop launching work when the allowance is gone. Rerun the
same command for an interrupted batch within the remaining allowance.
Report whatever is complete if time runs out. There is no load wait,
automatic retry, mandatory time slice or custom scheduler.

A trace unit is one colour-swapped pair, and an audit unit is one
position. Journal and flush complete units, verify the pinned header
on resume, and redo only incomplete units. Check SIGKILL recovery,
a truncated last record and a mismatched header before the run.
Compare deterministic results by unit ID. Load metadata and completion
order can differ. [Repository restart contract](../PLAN.md#interruptible-long-running-jobs)

## Decide the next experiment from the traces

If the audit shows consistently inferior book choices or missed
opponent replies, build a small book that spends verification on those
decisions. Calibrate the baseline's node effort in the relevant early
stages using a small profile of existing 51-second play. Nodes per
second alone cannot establish the effort per move. For a move-quality
gate, match the node cap to that measured effort and verify the
candidate substantially above it or with a stronger independent
search. The earlier 500,000/2-million verification rule and 500,000-node
gate do not establish an edge over production search.

If book choices mostly agree with play-strength search, the hypothesis
to test is that lookup saves effort for later moves. Compare equal
total work per game with a deterministic shared-budget policy for each
side. Reopening the virtual node clock (#168) is one option and requires
a separate design decision. It would need to charge work in aborted
iterations and exact and fallback paths, account for lookup overhead,
and let unused budget carry into subsequent moves. A virtual-clock
result applies to that model. Claims about the real 51-second clock
would require separate evidence. A short real-clock smoke test can
check accounting with no load rule and a hard stop. The diagnostic
stage has no dependency on that clock work.

Register a later gate only once the candidate, controls and plausible
effect determine an affordable sample size. Sample fresh early roots
from a declared opening distribution that exercises the book, with
the normal start as a smoke case. Report unconditional coverage across
that distribution. If an additional test selects covered roots, freeze
the selection rule and report the strength result as conditional on
coverage. Estimate hit rate before the strength run without reading
holdout scores. If coverage is inadequate, change the candidate or
state a narrower claim before freezing a new gate.

Use the historical pair variance and the chosen margin for a power
estimate. The old interval had a half-width of roughly two percentage
points at 400 pairs. At similar variance, 128 pairs would give a
half-width near 3.7 points. With a 47.5% lower-bound threshold, a book
that truly scores 50% would usually fail to clear it at that sample
size. Approve a gate only if a short timing probe shows that a sample
with useful power fits a fixed time and work budget. Freeze the rule,
openings and one-look analysis before the gate. No gate or shipping
decision follows from the diagnostic alone.
