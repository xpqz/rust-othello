# Design review — 7 November 2026

Reviewed `docs/DESIGN.md`, all 913 lines of `docs/PLAN.md`, and all 54 SKIS
issues: nine phase epics (#52–60) and 45 tasks (#61–105), all currently open.
There is no implementation to benchmark or test yet. This is a design and
execution-plan review, not a claim about implemented defects. The designs and
issues have not been changed by this review.

The architecture is a sound basis for a small classical engine. Side-relative
bitboards, independent move-generation checks, plain alpha-beta before
optimisation, a serial harness, and explicit cancellation are good choices.
Revision 2 addresses substantial problems in the original design. However,
several contracts still contradict one another, the statistical validation has
leakage and coverage risks, and the line estimates are not yet evidence that
the complete platform fits. Resolve the P1 findings before implementing search
and the harness.

Priority: **P1** changes a central correctness or acceptance contract; **P2**
should be resolved in the affected phase before relying on its results.

## Findings

### 1. P1 — A forced-pass root cannot satisfy the SearchResult contract

References: [PLAN:113](/Users/stefan/work/crust/othello/docs/PLAN.md:113),
[PLAN:313](/Users/stefan/work/crust/othello/docs/PLAN.md:313),
[PLAN:482](/Users/stefan/work/crust/othello/docs/PLAN.md:482).
Issues: #73, #74, #83, #85.

`SearchResult.best` is specified as `None` only for a terminal root. But a
nonterminal forced-pass root has no legal square. The agent contract correctly
requires `None` there. Returning an opponent move after internally passing
would be an illegal move for the actual caller. A legal fallback also cannot
exist at that root.

Make `best = None` mean that the root has no legal placement, covering both
pass and terminal. Distinguish these through the board's legal moves/terminal
test, or through an explicit action if that proves useful. Keep the backed-up
score from searching the passed position. Specify terminal `depth`, `solved`,
and timeout behavior at a forced-pass root as well. Test both kinds of `None`
through search, the agent adapter, CLI, and harness.

This follows the [official Othello rules](https://www.worldothello.org/about/about-othello/othello-rules/official-rules/english):
a player must pass when unable to move and cannot pass when a move exists.

### 2. P1 — Deeper heuristic TT values violate fixed-depth equivalence

References: [PLAN:152](/Users/stefan/work/crust/othello/docs/PLAN.md:152),
[PLAN:314](/Users/stefan/work/crust/othello/docs/PLAN.md:314),
[PLAN:676](/Users/stefan/work/crust/othello/docs/PLAN.md:676).
Issues: #82, #87–89.

The plan says a heuristic `Exact` value is exact for its remaining depth, then
allows its use whenever `entry.depth >= depth`. A deeper horizon can have a
different value from a shallower one. Consequently a warm TT can change the
answer to a fixed-depth query, contradicting TT-on/off equivalence and the
minimax oracle contract. Bounds at a different heuristic horizon have the same
problem; they are not automatically bounds on the shallower minimax value.

A small independent array-board calculation demonstrates the issue without
any hashing: with the supplied positional table plus `100 * disc_diff`, the
initial position has minimax values **297 at depth 1** and **0 at depth 2**.
That evaluator is an illustrative allowed Phase 2 configuration, not an
implemented engine. A cached depth-2 `Exact(0)` cannot answer the depth-1 query
while preserving its specified result.

Choose the contract explicitly. For reproducible strict fixed-depth search,
reuse heuristic scores/bounds only at equal remaining depth; other legal TT
moves can still order the search. Alternatively permit deeper substitutions
in playing mode and weaken the equivalence claim for that mode. Add a test
that searches deep, then shallow, using the same table. Cold-table tests alone
will miss this. Include evaluator and endgame-threshold configuration in the
table's lifetime rules.

### 3. P1 — Disjoint opening seeds do not create a held-out position set

References: [PLAN:327](/Users/stefan/work/crust/othello/docs/PLAN.md:327),
[PLAN:580](/Users/stefan/work/crust/othello/docs/PLAN.md:580),
[PLAN:804](/Users/stefan/work/crust/othello/docs/PLAN.md:804).
Issues: #80, #81, #99, #100.

Different seeds can generate the same opening sequence, transposed position,
or symmetric position. This is particularly likely with 4–8-ply openings. An
independent enumeration found only **236 distinct four-ply positions**, or
**60 up to D4 symmetry**, despite 244 opening paths. Seed disjointness does not
establish the plan's stronger claim that held-out openings are disjoint.

Generate and freeze position sets, with a declared identity rule. Prefer
canonical D4 board keys for the split, reject overlap, and record duplicate
counts and opening-length distributions. If only independent random draws are
intended, describe them as such rather than claiming unseen positions. Define
whether prefix-related openings belong in the same split too.

After inspecting a failed validation run and changing the engine in response,
that set has influenced tuning. Reserve another final set or explicitly record
that the earlier set has become developmental.

### 4. P2 — The TT conflates search domain with exactness and discards solver bounds

References: [PLAN:145](/Users/stefan/work/crust/othello/docs/PLAN.md:145),
[PLAN:155](/Users/stefan/work/crust/othello/docs/PLAN.md:155),
[PLAN:715](/Users/stefan/work/crust/othello/docs/PLAN.md:715).
Issues: #87–91.

`solved` is doing two jobs: distinguishing terminal searches from heuristic
searches, and identifying an exact value rather than a bound. Therefore a
terminal-search fail-high/fail-low is stored as `solved=false` and then ignored
by the solver. Much of a narrow-window search's reusable work is thrown away.
The depth-free solver also has no specified value for `Entry.depth` when it
stores these entries. Accidental consumption by heuristic search would make
the contracts harder to reason about.

Represent the domain separately from `Exact/Lower/Upper`: for example an
endgame tag or a reserved draft convention. Reuse proven terminal-search
bounds in compatible terminal-search windows while rejecting heuristic bounds.
Keep `SearchResult.solved` for the claim about the exact root result. A draw
returned inside `(-1, 1)` is an exact zero, so blanket wording that every WDL
entry must be Lower/Upper is unnecessarily restrictive.

For comparison, [Edax's endgame implementation](https://github.com/abulmo/edax-reversi/blob/master/src/endgame.c)
uses bound-bearing hash data during narrow-window endgame searches. This does
not prove a speedup here; measure it on the intended corpus. The current
ignore-all-bounds policy is conservative but an important performance tradeoff.

### 5. P2 — Freezing weights alone does not freeze the baseline evaluator

References: [PLAN:217](/Users/stefan/work/crust/othello/docs/PLAN.md:217),
[PLAN:557](/Users/stefan/work/crust/othello/docs/PLAN.md:557),
[PLAN:765](/Users/stefan/work/crust/othello/docs/PLAN.md:765).
Issues: #72, #79, #95, #99, #100.

Phase 7 changes the positional table and feature definitions, not just their
weights. Feeding frozen Phase 2 weights through those new definitions changes
the baseline. A few pinned evaluation examples can fail to detect drift on
other positions. The search pseudocode also hardcodes `evaluate(b)` while the
harness requires two evaluators to coexist.

Freeze the complete Phase 2 evaluation function, including its table, scaling,
and rounding, and pass an evaluator function pointer/configuration into search
from the start. Share code where its semantics truly remain fixed. Record the
full configuration in each run.

Using current search for both players is useful for evaluator A/B tests, but
does not measure improvements or regressions in search itself. For the
"previous frozen configuration" release comparison, retain a versioned engine
artifact or explicitly limit that claim to evaluator regressions.

### 6. P2 — Harness state and invalid actions need production contracts

References: [PLAN:482](/Users/stefan/work/crust/othello/docs/PLAN.md:482),
[PLAN:543](/Users/stefan/work/crust/othello/docs/PLAN.md:543),
[PLAN:571](/Users/stefan/work/crust/othello/docs/PLAN.md:571),
[PLAN:688](/Users/stefan/work/crust/othello/docs/PLAN.md:688).
Issues: #74, #77, #80, #81, #102.

Opaque `FnMut` agents can retain RNG state, TT contents, and counters. Merely
swapping closures for the second game does not define fresh paired conditions
or schedule-independent randomness. Also, the exact-50% acceptance for
"identical agents" is valid for deterministic identical policies replaying
the same opening, but is not guaranteed for independent stochastic agents.

Specify an agent factory or a reset lifecycle per game. Derive opening,
agent, and bootstrap RNG streams separately from stable run/pair/game IDs;
log enough information to reconstruct them. Clearing TT contents is concrete;
"aging" is insufficient if old entries remain usable and alter the timing
comparison. Limit exact paired-equality tests to deterministic agents, with a
separate statistical sanity check for random agents.

Validate returned actions in release matches as well as debug builds. Reject
both an illegal square and `None` when legal moves exist; neither may silently
become a pass. The cost is negligible compared with search, and corrupted
release match results would undermine tuning.

The game loop also needs an explicit absolute side-to-move bit outside the
side-relative `Board`. Toggle it on every placement and pass, including replay
of an odd-length opening. Use it for selecting the black/white agent, printing
colours, and attributing the terminal result.

### 7. P2 — A percentile bootstrap needs a degeneracy policy

References: [PLAN:549](/Users/stefan/work/crust/othello/docs/PLAN.md:549),
[PLAN:552](/Users/stefan/work/crust/othello/docs/PLAN.md:552).
Issues: #78, #81, #100.

If all observed pair scores equal 1, every bootstrap resample also equals 1.
The reported interval is `[1,1]`; mapping its endpoints to Elo produces an
infinite lower bound. Repeatedly resampling cannot recover uncertainty missing
from an empirical sample. The same zero-width issue occurs for any constant
pair-score sample. With only one observed winning pair, for example, `[1,1]`
plainly does not support a general 95% coverage claim.

Specify a finite-sample fallback for small or degenerate samples, or report the
bootstrap interval as uninformative there and withhold the strength claim.
A conservative bounded-mean interval is an option that preserves pair units.
Do not treat fractional pair scores as binomial successes merely to apply
Wilson. Define zero pairs, one pair, all draws, all wins/losses, quantile
selection, and Elo endpoints. Infinity may be a displayed point estimate;
it must not masquerade as established strength certainty.

### 8. P2 — Feature selection against the original baseline does not isolate gains

References: [PLAN:789](/Users/stefan/work/crust/othello/docs/PLAN.md:789).
Issues: #97, #99.

After feature A helps, A+B can still beat the Phase 2 baseline even when B
hurts relative to A. A recorded win over the original baseline therefore does
not establish that each retained feature helps. Trying many weight candidates
on the same 200 pairs also makes a nominal 95% interval a weak selection gate:
the selected winner benefits from repeated testing.

Compare each candidate with the immediately preceding accepted configuration
under otherwise identical search settings. Use an ablation when attributing
gain to a feature. Keep Phase 2 as the long-term anchor, predeclare a practical
selection protocol, and reserve untouched validation for the final chosen
configuration. A positive Elo interval is just a monotonic transform of the
same score interval, not independent confirmation.

### 9. P2 — The size budget needs an early feasibility gate and a precise scope

References: [PLAN:18](/Users/stefan/work/crust/othello/docs/PLAN.md:18),
[PLAN:262](/Users/stefan/work/crust/othello/docs/PLAN.md:262),
[PLAN:385](/Users/stefan/work/crust/othello/docs/PLAN.md:385),
[PLAN:850](/Users/stefan/work/crust/othello/docs/PLAN.md:850).
Issues: #65, #97, #104.

The module budgets sum correctly to 805, but 140 lines for search and 75 for
the CLI are optimistic estimates. Search includes root selection, negamax,
ordering, time controls, iterative deepening, cancellation, solver, and TT
integration. The CLI includes interactive input, argument errors, board output,
perft, corpus benchmarking, matches, reports, and replayable logs. Their
complete readable size has not been demonstrated.

At 1,000 ms per move, 400 games can consume roughly 6–7 hours of serial search
time if most of the remaining 52–56 placements use the budget. Many candidate
runs plus a final ladder are a material development cost. Set short tuning
time controls explicitly and reserve the full default budget for selected
checks; parallel equal-time matches need CPU-contention controls.

Enforce cumulative size at every phase and prove a complete vertical slice
early. Count formatted production source across all Rust targets, not just
whatever happens to live under `src/`; prohibit moving implementation into
tests or `include!` files to escape the count. Define the treatment of support
scripts and future model assets. The current doc-comment-counting rule is
clear but unusual; document it as the actual metric.

The user requirement says **under 1000**, whereas the plan and issues permit
`<=1000`. Use `<=999` for a literal interpretation. Removing/disabling TT only
saves lines if its implementation is removed; a runtime flag does not.

The first optional scope cuts should include unused production D4 transforms
(keep correctness transforms in tests until runtime augmentation is required),
WDL-before-margin machinery, optional stability, parallelism, and duplicate
reporting features. Preserve legal play and score correctness. Rebenchmark
before removing TT or ordering while promising unchanged endgame capability.

### 10. P2 — Some phase gates require future work, and abort tests overreach

References: [PLAN:380](/Users/stefan/work/crust/othello/docs/PLAN.md:380),
[PLAN:625](/Users/stefan/work/crust/othello/docs/PLAN.md:625),
[PLAN:676](/Users/stefan/work/crust/othello/docs/PLAN.md:676),
[PLAN:905](/Users/stefan/work/crust/othello/docs/PLAN.md:905).
Issues: #62, #65, #67, #72, #85, #89, #92.

Phase 0 tests `terminal_score(Board)` and evaluation bounds before the board
score and evaluator are delivered in later phases. Decide which minimal board
and score implementation lands in Phase 0, or stage the tests with their real
dependencies. Avoid fake production stubs merely to make a gate green.
Phase 4 requires an endgame-solver timeout test although the solver is not
implemented until Phase 6; move that integration check to Phase 6.

"Aborted searches store nothing" is broader than "the interrupted node and
its unfinished ancestors do not store results." Completed sibling subtrees
can safely have stored entries before an ancestor times out. Requiring the
entire table to remain unchanged implies buffering or rollback, adding code
and cost without protecting score correctness. State the local rule and test
that no incomplete result is stored; retained completed results are allowed.

The universal definition of done also needs exceptions for docs, test-only,
scaffold, and measurement issues. Its current requirement that changes be in
production modules and have a failing behavior test conflicts with several
explicit tasks. Keep meaningful checks appropriate to each issue.

### 11. P2 — Deadline tests assume timing guarantees they cannot provide

References: [PLAN:122](/Users/stefan/work/crust/othello/docs/PLAN.md:122),
[PLAN:619](/Users/stefan/work/crust/othello/docs/PLAN.md:619),
[PLAN:625](/Users/stefan/work/crust/othello/docs/PLAN.md:625),
[PLAN:749](/Users/stefan/work/crust/othello/docs/PLAN.md:749).
Issues: #83–85, #93, #101.

A 1 ms budget need not abort: an easy root may complete. OS scheduling can also
make a 50 ms wall-clock assertion fail despite correct polling. Polling only
inside recursion omits pre-search work such as TT allocation/clearing and
expensive ordering performed before the next recursive call.

Specify when the clock starts, which setup costs are included, and an overrun
tolerance. Test cancellation deterministically with an expired deadline or a
small public node limit, then use tolerant wall-clock smoke tests separately.
Keep root-terminal and forced-pass exceptions explicit. Add cancellation to
`solve_wdl`'s return type; its proposed bare `i32` contradicts the sole
`Option` cancellation channel.

The median 12-empty solve-time goal alone does not establish how often the
default 1 s search actually solves a position. Report completion rate and
p90/p99 latency as well as median, plus fallback strength after an interrupted
first solve. The legal-first fallback is safe but may be very weak. Stop
iterative deepening once the root is proven exact rather than repeating solves.

### 12. P2 — The two documents and some issues still give conflicting instructions

References: [DESIGN:184](/Users/stefan/work/crust/othello/docs/DESIGN.md:184),
[DESIGN:248](/Users/stefan/work/crust/othello/docs/DESIGN.md:248),
[DESIGN:328](/Users/stefan/work/crust/othello/docs/DESIGN.md:328),
[PLAN:3](/Users/stefan/work/crust/othello/docs/PLAN.md:3).
Issues: #57, #63, #89, #105.

PLAN is explicitly the source of truth, which helps, but DESIGN still offers
the old depth-before-terminal pseudocode, Wilson-based game statistics, static
X/C penalties, and unresolved decisions the plan has already made. Following
its examples can reintroduce the very bugs Revision 2 fixes. Update those
sections or visibly mark them superseded and point to the contracts.

The plan's pseudocode also mutates an immutable `alpha` parameter; change it to
`mut alpha` before using it as implementation guidance. Replace "mate-in-1"
with a concrete Othello test: a legal placement causing a double-no-move
terminal with a verified score. Define the initial board constants, corpus
serialization, expected-score provenance, and what "printer round-trips a
board" actually requires. These are small decisions that prevent divergent
implementations.

SKIS largely matches the revised plan. One residual mismatch is that epic #57
states hard node-reduction percentages while task #89 and the plan allow
evidence-based adjustment. Keep the epic gate consistent. Issues currently
link to their phase epic; keep the cross-phase dependency order explicit when
selecting work, since the links themselves are undirected.

## Coverage and design choices worth keeping

- **Rules and representation:** the direction masks and side-relative `play`
  convention are coherent. Specify illegal-square preconditions and validate
  all CLI input before shifting/indexing. Handle EOF cleanly. A tied terminal
  with empty squares must score zero under the chosen winner-gets-empties rule.
- **Independent correctness evidence:** the naive reference, perft, symmetry,
  and shallow minimax oracle complement one another. Add hand-built perft
  pass/early-terminal fixtures: the initial-position table through depth 10
  does not exercise early game termination. Aart Bik reports terminal leaves
  first appearing at depth 11 in his reference.
- **Confirmed perft values:** depths 1–10 match
  [Aart Bik's original table](https://aartbik.blogspot.com/2009/02/perft-for-reversi.html),
  including 3,005,288 and 24,571,284. Keep a direct source link in the tests.
- **Evaluation:** scaled mobility, signed frontier, dynamic corner risk, and
  antisymmetry tests are appropriate. Specify phase endpoints and feature
  weights before implementation. With an exact threshold of 12, a disc-diff
  feature active only below roughly 10 empties is normally bypassed by the
  solver; tune the heuristic on positions where it actually runs. Treat the
  stability approximation as optional rather than a required milestone.
- **Performance:** measure fixed-depth time, completed depth in a fixed budget,
  nodes, and solve completion. Precompute mobility-ordering keys once per move
  if comparator recomputation is expensive. Recomputing a square-by-square
  Zobrist hash at every node and polling `Instant` every node are reasonable
  first implementations, not established routes to 1 M nodes/s. Profile before
  adding more machinery. Node reduction targets alone cannot establish speed.
- **TT integrity:** a full 64-bit hash detects table-index collisions but not
  two positions with the same full hash. Explicitly accept that tiny residual
  risk or store/check both bitboards if exact-oracle mode requires deterministic
  collision safety. Forced index-collision tests do not exercise full-key
  aliases. Validate stored move legality before using it for root output.
- **Benchmark design:** freeze positions and configuration, separate midgame
  and endgame results, include forced passes and difficult endgames, and record
  warm/cold TT conditions. Pair the same-position timing comparisons and use
  repeated release runs. Avoid oversubscribed equal-time strength experiments.
- **Safety and portability:** `forbid(unsafe_code)` in the library does not
  cover the separately compiled binary target. Apply crate policy there too.
  Constrain CLI depth before converting to `u8` in TT entries. Define seed-zero
  handling for xorshift and guard malformed input and empty statistics sets.
- **Product boundary:** the documents describe an engine/library, CLI, and
  experiment harness. That is a coherent first platform. Service hosting,
  multiplayer, GUI, persisted games, network protocols, model training, and 6×6
  generalisation are future scope. The current fixed 8×8 representation does
  not make 6×6 support free. A learned evaluator will need a loader/lifetime
  decision and model-data budget beyond replacing one function's arithmetic.

## Recommended execution order

1. Reconcile pass/root results, TT depth/domain semantics, and abort stores in
   the plan and the affected SKIS tasks before implementation.
2. Define the production LOC metric, executable phase gates, and cumulative
   budgets. Establish the board/oracles, then a complete playable fixed-depth
   slice with actual formatted counts.
3. Build a trustworthy serial harness: explicit colour state, fresh agent
   lifecycle, release action validation, frozen evaluator, position-based
   tune/validation split, and interval edge cases.
4. Add cancellation and measured ordering/TT improvements. Build terminal
   solving with reusable terminal bounds and deterministic oracle checks.
5. Tune incremental candidates against the current accepted configuration,
   then run the selected engine once on untouched validation at declared time
   controls. Publish actual size, timing distributions, solve completion, and
   strength evidence before claiming all targets are met.
