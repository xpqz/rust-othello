# Opening book: Design

Nov 16, 2026 · Approved, revision 3 with amendments 1 and 2 · SKIS epic #150, design issue #152

> Scope: an opening book built from the WTHOR archive and scored by our own
> engine, consulted only when the engine chooses a move to play. Search,
> the exact solver and every `search_tt` caller are untouched.
> `docs/PLAN.md` stays the source of truth for budgets, statistics and
> process. The evidence is `tools/examples/book_coverage.rs` and its log
> `docs/tuning/book_coverage.log` (`d76aaa6`). Two decisions belong to the
> user and are marked **User decision**. Revisions 2 and 3 answer the
> first two reviews; [Review response](#review-response) maps each finding to its change.

## Why, and what the book can and cannot buy

The engine searches from the first move, at a flat 3 s per move. A book
offers two things:

1. **Move quality.** A book value is not just one deeper search. It is a
   negamax over a tree of frequently played lines, whose leaves lie up to
   29 plies in. A move at ply 4 is chosen with information from searches
   25 plies further on. The diagnostic in #149 put the live search at a
   median depth of 19 at ply 20 in 3 s, so a single book search at depth
   20 is not much deeper than live play. The tree is where the depth comes
   from.
2. **Time.** Book moves cost no thinking time. Under today's flat 3 s per
   move, that time is lost.

The user, 15 Nov 2026: "Under today's flat 3 seconds per move, an opening
book's time savings are simply lost, so its main payoff needs the
time-management epic first, in terms of implementation." Then: "Do the
design work for the book now." So the book is designed and built now, and
its strength gate runs after #151 provides a game clock, at equal total
game time (see [Gate](#gate)). There is no flat-3 s gate.

## Evidence (committed)

`book_coverage` replays all 137,548 WTHOR records (none fail). It counts
canonical positions (D4 symmetry, `tools/src/sym.rs`) by ply, where row k is
the position after k moves. It records the board before each recorded move,
so the position after a game's final recorded move is not counted; within
the 30-ply window that matters only for games that end there (the builder
counts it, see [Nodes](#nodes-moves-and-the-partition)).

Positions at plies 0–29 occurring in at least N games:

| N | 1 | 2 | 5 | 10 | 20 | 50 | 100 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| positions | 1,639,683 | 162,777 | 41,295 | 18,877 | 8,977 | 3,607 | 1,829 |

Frequent positions stay plentiful to about ply 20 (856 positions with N ≥ 10
at ply 20, 130 at ply 29). Gate openings: only 37 of the 1,024
`openings_probcut3.txt` end positions, and 22 of 512 `openings_final.txt`,
occur in WTHOR at all. Ply 6 has 1,773 canonical positions; 825 occur in
WTHOR and 546 in at least 2 games.

Scoring cost: `standard()` (fitted ProbCut, threshold 18), table
`MAX_ENTRIES` cleared per search. 10 positions occurring in at least 10
games, at plies 10 and 20. "Drop-out" is one search per child that no WTHOR
game played from that position, at one less depth, with one table for the
set:

| ply | depth | leaf median | drop-out median (mean) |
| ---: | ---: | ---: | ---: |
| 10 | 18 | 1.00 s | 3.59 s (3.34 s) |
| 10 | 20 | 4.94 s | 19.08 s (15.30 s) |
| 20 | 18 | 1.08 s | 6.28 s (6.86 s) |
| 20 | 20 | 3.07 s | 18.49 s (22.64 s) |

The machine load was 2.8 at the start of the run, so these are sizing
estimates, not benchmarks.


## The book

### Orientation

- **Canonical orientation** of a board: the transform k in 0..8
  (`tools/src/sym.rs` numbering, the engine copies it) that minimises
  `(me, opp)`. When several transforms tie, which happens only on
  symmetric boards, the smallest k wins.
- Every move label below is a square in the canonical frame.

### Nodes, moves and the partition

- **Ply** counts placements only; a pass is not a ply.
- **Forced passes are contracted.** `normalize(b)` replaces a board whose
  mover has no legal move, while the opponent has one, by the passed
  board. Every counted position and every child key is normalized, so a
  node is always a board with a legal move, or a terminal board. There
  are no pass nodes.
- **Nodes:** canonical normalized positions after 0–29 placements that
  occur in at least `N = 10` WTHOR games.
  - The builder replays each game's recorded moves and counts the
    position after every placement, including the final recorded board,
    whether the game finished or not. Each position counts once per
    game.
  - That gives about 18,877 nodes; the builder's own count is logged.
  - N and the ply limit are fixed now.
- **Partition of a node's legal moves.** Every legal square s of node p,
  in its canonical frame, falls into exactly one class, by the canonical
  key of `normalize(play(p, s))`:
  - **book move:** that key is a node, whether or not any game played s
    from p. A transposition reached another way still counts.
  - **drop-out move:** that key is not a node.
- **Passing moves.** A move is `passed` when `play(p, s)` needed the
  pass. The same side then moves again in the child, so a passing book
  move is worth +v(child), the two sign changes cancelling. It is worth
  −v(child) otherwise.
- Several squares can reach the same canonical child, for example on
  symmetric boards. Each square is still listed once, with its own label.
- **Terminal nodes** (neither side can move): the value is
  `terminal_score(final_score(board))`, as in the engine, which awards
  empty squares to the winner. Such a node has no moves, needs no
  drop-out search, and is never emitted as a record.
- **Colour:** a node's key is the board alone, with no colour. The
  reachability walks carry the engine's role as explicit state. It flips
  on every placement and flips back across a contracted pass, so after a
  passing move the same side is to move. Colour is never inferred from
  disc count.

### Values

All values are centidiscs from the side to move.

- **Child value helper** `value(b, d)`, defined for any board:
  - if b is terminal: `terminal_score(final_score(b))`;
  - if b has no legal move: `−value(pass(b), d)`, because a pass consumes
    no ply, matching `negamax`;
  - otherwise: `search_tt(b, depth d).score`. `search_tt` is only ever
    called on a board with a legal move, as `root_setup` requires.
- **Drop-out value** `x(p)`: the drop-out moves are taken in ascending
  canonical square order.
  - Each move's value is `−value(play(p, s), D − 1)`, using the
    unnormalized child, so a forced pass inside it is handled by the
    helper at the same depth.
  - The table is cleared once, before p's first drop-out search, and kept
    warm across p's drop-out moves.
  - `x(p)` is the maximum of these values, taking the lower square on
    ties.
- If p has no drop-out moves, `x(p)` is absent.
- **Backed-up value**: `v(p) = max(x(p), max over book moves s of the
  move's value)`, where a book move is worth +v(child) if passed and
  −v(child) otherwise.
- **Book move**: the move attaining `v(p)`, with ties going to the lower
  canonical square. A square is either a book or a drop-out move, so no
  further tie rule is needed.
- A non-terminal node has at least one legal move, so the maximum is
  never taken over an empty set.
- **This is the book's own scoring algorithm.** A node whose moves are all
  drop-outs gets the maximum over its children searched one by one with a
  warm table. That is not claimed to equal `search_tt(p, D)`.
- **Order:** every book edge adds a disc, so nodes are backed up in
  descending order of disc count.

### Scoring search and determinism

- **Policy, written out field by field rather than taken from
  `standard()`:** `eval: pattern::evaluate`, `endgame_threshold: 18`,
  `ordering: true`, `probcut: Some(ProbCut::FITTED)`.
- **Depth:** `D` is fixed by the pilot rule below.
- **Table:** `MAX_ENTRIES`, with the lifecycle above.
- **Unit of work:** one node, or one (node, depth) pair in the pilot. Its
  searches depend only on the node's canonical board, the policy and the
  fixed order above, never on other units. Thread count, job order and
  resumption therefore cannot change a unit's result, and no hash-map
  iteration order is used anywhere.
- **Journal:** each line holds the node's canonical board, its drop-out
  scores by square and `x(p)`.
- **Journal header:** a policy version string naming D, N, the ply limit,
  the policy fields, the orientation and order rules, the WTHOR hash and
  the engine commit. Resuming checks the header exactly.

### Pilot and the D rule

Fixed now, before the pilot runs:

- **Sample:** 200 nodes drawn uniformly without replacement from the
  actual N = 10 node set. The draw is a seeded shuffle (seed 150) of the
  nodes sorted by canonical board.
- **Measurement:** each sampled node is scored at D = 18, 19 and 20 with
  the actual partition. Nodes with no drop-out moves are included and
  take about 0 s.
- **Resumable:** the pilot is journalled per (node, depth) unit,
  interruptible and restartable.
- **Projection** per D: the projected run time is (mean node time) ×
  (node count) ÷ 5 threads.
  - Its upper bound is the 97.5th percentile of that projection over
    10,000 bootstrap resamples of the sampled node times (seed 150): the
    upper end of a two-sided 95% percentile interval.
  - The bound is used as it stands, not added to the point estimate.
- **Rule:**
  - D is the largest of {18, 19, 20} whose upper bound is at most 36
    hours.
  - If even D = 18 exceeds 36 hours, the design comes back for review
    with a new N.
  - Nothing is changed automatically.

### Gate roots (frozen before scoring)

- **What the gate tests:** play from frequently reached human
  positions. It is a conditional gate: neither games from the standard
  start nor a frequency-weighted human opening distribution.
- **Roots:** 400 positions drawn uniformly without replacement, by a
  seeded shuffle (seed 151), from the node-set positions at plies 6–8
  that have a legal move. Pass and terminal nodes are internal and are
  never roots. The condition is checked again when the roots and the
  artifact are frozen.
  The pool is 232 + 402 + 645 = 1,279 positions in the sizing count; the
  builder's exact pool is logged.
- **Committed in the builder issue,** before any scoring:
  `bench/openings_book.txt`, holding each root's move sequence from a
  WTHOR game.
- **No replacements:** no root is ever replaced because of an outcome.
- **Every root is a node by construction, and roots are protected from
  overflow** (below). The shipped book therefore answers at every root
  for the side to move there.
- **The other colour's first move,** after the opponent's reply, is not
  guaranteed. Its hit and miss counts are logged on the final artifact.

### Asset, pruning and overflow

- **Room:** `assets/` totals 4,014,728 bytes against a 4 MiB guard, which
  leaves 179,576 bytes. The include guard already accepts any file under
  `assets/` (`tests/budget/rules.rs`).
- **Format:** `assets/book.bin`, sorted fixed-width records of 11 bytes,
  little-endian: `tt::zobrist` of the canonical board (u64), the book
  move as a canonical square (u8), and `v(p)` clamped to i16. That is at
  most 16,325 records. All 18,877 nodes would need 207,647 bytes, so
  pruning is required.
- **Reachability (lossless along connected book lines):**
  - **Roots:** the start position, with the engine as Black and as White,
    plus each of the 400 gate roots, with the engine to move and with the
    opponent to move.
  - **Walk:** from each root, at engine-to-move nodes the walk follows the
    book move only; at opponent nodes it follows every book move. It
    traverses forced passes and stops at drop-out moves.
  - **Stored:** the engine-to-move nodes reached that have a legal move.
  - **Guarantee:** while a game stays on a connected book line from a
    root, the stored book answers exactly as the full book would.
- **Re-entry:** lookup is stateless, so after the game leaves the book
  (a search move, or an opponent move outside the node set), any later
  position that is stored is answered again. Re-entry coverage is reduced
  by pruning, by choice. There is no "out of book" latch.
- **Overflow (a deliberate truncation, not a guarantee):** if the
  reachable set is still too large, nodes are removed in ascending
  (WTHOR count, zobrist key) order, and reachability is recomputed after
  each removal, until the set fits.
  - The start position and the 400 gate roots are mandatory and never
    removed.
  - If the mandatory records alone cannot fit, the build fails, and the
    design returns for review. That would take 401 records, about 4.4 KB,
    against 179,576 bytes, so it is not expected. Stored records keep their
  pre-truncation move and value. They are the full book's
  recommendations, and the engine leaves the book where a position is
  missing. No re-backup is done.
- **Logged on the final artifact, before any gate game:** the record
  count, every truncation, and per-root coverage (how many book plies are
  reachable).
- The size decision can be taken from these numbers without raising the
  guard now.
- **Manifest:** the builder also commits
  `docs/tuning/book_manifest.txt`, one line per record with the canonical
  board, square and value. Generation fails on two distinct boards with
  the same key. The manifest lets anyone verify legality, which
  `book.bin` alone cannot show.

### Extension rounds (amendment 3, revision 2)

- **Why:** in the #154 book, 1,843 of 3,771 stored records choose a
  drop-out move, and 187 of the 214 gate roots with one reachable record
  choose a drop-out at the root itself. Each such choice ends the connected
  book prefix. Extension scores the positions the book's own choices lead
  to, so that a connected line can continue past them. Coverage is not
  promised to grow monotonically: backup can change choices.
- **Extension nodes:** boards added to the node set with WTHOR count 0.
  They are canonical and normalized like every node, and the partition,
  values, backup, walk and overflow rules apply to them unchanged. Since
  the partition is by node-set membership, adding a node can turn an
  existing drop-out move into a book move.
- **Terms:** "key order" means ascending canonical `(me, opp)` pairs. A
  *book state* over a node set `M` is the backup over `M` (below) and its
  **untruncated** walk from the 802 declared walk roots, with no overflow
  removal. Truncation happens only when the final asset is emitted, and the
  full scored node set is kept separate from the emitted records.

#### Rounds and phases

Round r, for r = 1..R, starts from the node set `M_{r−1}` (`M_0` is the
#154 node set) and its book state.

- **Phase A.**
  - *Departures:* the walk's engine-to-move records whose backed-up move is
    a drop-out.
  - *Units* `A_r`: the distinct keys of `normalize(play(p, s))` over the
    departures, excluding terminal boards, sorted in key order. None is in
    `M_{r−1}`, by the partition.
  - *Context:* the immutable snapshot `C = M_{r−1} ∪ A_r`. Every unit is
    scored exactly as in #154, partitioned against `C`, so a move from one
    A node to another A node (a same-phase peer) is a book move and is not
    searched. The snapshot is fixed before any unit runs, so job order and
    worker count cannot change membership or the search sequence.
  - After A is complete, `M = C`, and the backup over `M` is recomputed.
- **Phase B.**
  - For each `c` in `A_r`, in key order: `m(c)` is `c`'s backed-up best move
    in the **post-A backup**, book or drop-out, with the existing value and
    lower-square tie rule. This is the move the scored book selects at `c`,
    whoever moves there: after a contracted pass that is the engine, and
    the next child need not be the engine's turn. Every non-terminal A
    unit has a legal move, so `m(c)` exists.
  - *Units* `B_r`: the distinct keys of `normalize(play(c, m(c)))` that are
    neither in `M` nor terminal, in key order. When `m(c)` is a book move,
    its child is already in `M` and `c` contributes no B unit.
  - *Context:* `M ∪ B_r`, scored and fixed as in phase A.
  - B is formed from every A unit, not only those still reached after the
    post-A backup. This keeps B a function of A's results alone.
  - After B is complete, `M_r = M ∪ B_r`, and the backup and walk give the
    book state after round r.
- **Registered: R = 3.** D = 19, the table and the scoring policy are those
  recorded in the #154 header, and nothing is changed automatically after a
  round. A round whose `A_r` is empty is complete with no change. Its empty
  phase journals are written, and every later round is then also empty.

#### Per-square values from a journal

Every journal, the #154 one included, keeps its existing record format:
`best | t1 t2 …`, where `best` is `value square` or `none`, and the tokens
are the raw search results in call order. The decoder below turns a
record into one parent value per drop-out square, given the context the
record was scored under.

- The drop-out squares are the node's legal squares whose normalized child
  key is not in that context, in ascending order. For each such square
  `s`, with `b = play(p, s)` unnormalized:
  - `b` terminal: the parent value is `−terminal_score(final_score(b))`,
    and no token is consumed;
  - `b` has no legal move but is not terminal: one token `t` is consumed,
    the search of `pass(b)`, and the parent value is `+t`;
  - otherwise: one token `t` is consumed, and the parent value is `−t`.
- The record is valid only if every token parses as an integer within
  ±`EVAL_BOUND`, the tokens are consumed exactly with none left over, and
  `best` equals the maximum of the decoded values with the lower square on
  ties (or `none` exactly when there is no drop-out square).
- **Reclassification:** in a later, larger node set, each decoded square
  whose child key is now a node becomes a book move. Its decoded value is
  ignored and the move backs up through the child as ±v(child). `x(p)` is
  the maximum of the decoded values of the squares that remain drop-outs,
  with the lower square on ties, and is absent if none remain. Several
  squares transposing to the same new child are each reclassified. No
  journal is rewritten and no search is repeated.

#### Journals, identity and resumption

- Each phase is one scoring job with its own journal, `r{r}a.journal` and
  `r{r}b.journal`, in a directory given on the command line.
- **Header** of a phase journal: the base header (policy fields, D, table,
  `eval.bin` hash, base engine commit, rules), then
  `extend R {R} round {r} phase {a|b}` and these SHA-256 digests, each with
  its count:
  - `base`: the base journal's records, in canonical form;
  - `prior`: all earlier phase journals, in phase order, each as a line
    `r{r}{a|b}` followed by its records in canonical form;
  - `roots`: the 802 walk roots (the existing root-set hash);
  - `units`: the sorted unit IDs;
  - `context`: the sorted IDs of the context snapshot.

  Finally, `extender {commit}` records the scorer's source commit, fixed
  at compile time from the `OTHELLO_COMMIT` environment value of the
  recorded build command (`unknown` when unset). A run writes its own
  value; a resume by a binary with another value is refused. A build
  replays the value the run recorded in `r1a` (and its R), so the
  builder's identity never enters, and phases with inconsistent values
  are refused. The `eval` field hashes the model compiled into the engine
  (`pattern::WEIGHTS`), not the file on disk.
- **Canonical form** of a journal's records: records sorted by ID, each as
  `id<TAB>line`, joined by newlines. Parallel append order therefore does
  not affect any digest.
- **Policy check:** before any work, the extender confirms that its own
  scoring policy fields, D, table, rules and `eval.bin` hash equal those in
  the base header, and refuses otherwise. A new evaluator cannot be mixed
  into this registered run.
- **Resumption:** re-running the same command validates, in order, the
  base journal, then each existing phase journal against its recomputed
  header. A mismatch refuses: changed roots, a changed earlier phase, or a
  different unit or context set. Every existing record is then checked by
  the decoder against its phase context, and the extender refuses foreign or
  duplicate units. A torn final line is discarded, as in #153's `Journal`.
  Only then are the missing units scored. A phase starts only after the
  previous one is complete.
- A SIGKILL at any point loses at most the units in progress. The logical
  records (sorted), the asset and the manifest are identical after any
  interruption, worker count or job order. Raw append order need not be.

#### Build

- `book build` takes the extension directory and the requested R. It uses
  the base journal and every complete round up to R; a round is complete
  when both phase journals hold exactly their units. A partial or absent
  phase after the last complete round is ignored for the build, and the
  manifest records it.
- The manifest header states `rounds requested {R} completed {k}`, and the
  partial phase and its unit count if there is one. Partial work is never
  presented as a round.
- Pruning and overflow are unchanged: removal in ascending
  `(count, zobrist(canonical board))` order with reachability recomputed.
  Extension nodes have count 0, so they go first. The mandatory roots stay
  protected.
- **Logged per round:** departures, `|A_r|`, `|B_r|`, same-phase peer edges,
  A units with a book move into the earlier node set, previous departures
  that now choose a book move, records, and the per-root coverage totals.
- With R = 0 the build is byte-identical to the #154 asset and manifest:
  the `rounds` header line applies only to builds with R > 0. The test
  compares bytes directly.

#### Sizing (guesses, not bounds)

- The reviewer's probe of the frozen data: 1,843 departures give 1,839
  distinct A1 units, none terminal. 56 of them have a legal reply into the
  base node set, and 4 A1 edges lead to another A1 unit.
- **Records:** later departures can grow, and backup changes can make
  unretained base nodes reachable. If each round adds about as many
  engine records as round 1's departures, three rounds give about 9,300
  against a room of 16,325. This is a guess. Overflow and the fixed R rule
  handle any excess, and R is not changed after looking at results.
- **Time:** the #154 scoring run at D = 19 searches each drop-out at
  depth 18. Its 72,553 s on 5 threads over 182,337 such searches, including
  table clearing and job overhead, average about 2 s of thread time each,
  under shared load. An extension node needs about 10 searches, so a phase
  of about 1,840 units takes about 2 h on 5 threads, and three rounds
  about 12 h, if later phases are of similar size.
- **Code:** an estimated 60–120 trainer lines, against 65 left under the
  2,100 guard. Raising the guard is planned under the user's standing
  permission ("You can raise the trainer LOC budget, but keep the core
  engine tight"). It will be done in #157 with `trainer/tests/budget.rs`,
  `AGENTS.md` and `docs/PLAN.md` kept consistent at one chosen ceiling.
  It is not done yet. The engine budget is unchanged.

### Builder home and reproducibility

- **The rule:** `docs/PLAN.md` says assets "must be reproducible from the
  trainer crate", and training and fitting pipelines live in the counted
  trainer. The trainer has 50 of its 1,200 lines left, and the builder
  needs an estimated 250–350.
- **Decided:** user decision, 16 Nov 2026: "Trainer can have 2000 LOC."
  The builder lives in the counted trainer, and the trainer guard is now
  2,000 lines (`trainer/tests/budget.rs`, `docs/PLAN.md`).
- **Reproducibility:** the asset is a pure function of the WTHOR files
  (whose hashes are in the header), N, the ply limit, D, the roots, the
  rules above and the engine commit.
- **Not tied to the evaluator** (amendment, user decision, 16 Nov 2026):
  - The build log and manifest header record the `eval.bin` hash the book
    was scored with, for provenance only. No test pins it.
  - A retrained evaluator keeps the existing book: lookup never uses the
    evaluator, and a stored move remains the result of the deep offline
    scoring. A stored value is then reported as the score of an older
    model.
  - Rebuilding after a retrain is optional, and is decided by a match
    against the existing book, not forced by a test.
  - The stored value is an offline score from the evaluator recorded in
    the manifest header. After a retrain it is not presented as an
    evaluation by the new model.
  - The build inputs (journal, header and manifest) are kept so that an
    old artifact can be reproduced.
  - Serving a completed book and continuing a scoring job are separate.
    A scoring journal still refuses to resume under different scoring
    inputs, including a different `eval.bin` hash, through its header.
  - The user, 16 Nov 2026: "The planned opening book depends on the
    current evaluation weights, so every retrain of the evaluator means
    another day-long book rebuild." The pin was the only thing forcing
    that.

## Engine

### Lookup

- `src/book.rs`, `pub fn book_move(board: Board) -> Option<(u8, i32)>`:
  1. It takes the canonical orientation, with the same tie rule, using
     bit operations. The engine has only the square transform in
     `pattern.rs`.
  2. It binary-searches the zobrist key in
     `include_bytes!("../assets/book.bin")`.
  3. It maps the stored square back through the inverse transform, after
     checking the square is below 64.
  4. It returns `None` unless the move is legal.
- **No answer** for a board with no legal move, or a terminal board: the
  caller's normal path decides.
- **Estimated size:** 45–60 engine lines with `play`. The engine is at
  1,218 of 1,500.

### Where it is consulted (the full consumer audit)

- `SearchConfig` gains `book: bool`. Only a new function reads it:
  `play(board, limits, config, table) -> SearchResult`. When `book` is
  set and `book_move` answers, `play` returns that move with the book
  value, `depth` 0, `nodes` 0, `Complete` and not solved. On a miss it
  calls `search_tt`, unchanged.
- **`search_tt` never reads `book`.** Every `search_tt` caller is
  therefore unchanged by construction, whatever `standard()` holds. That
  covers the CLI `search` command, the oracles and golden tests,
  `probcut_fit`, `probcut_screen`, the benches, `edax_gap`, `edax_phase`,
  `edax_ladder`'s exact check, `fallback_cost` and `solve_times`.

Playing paths, which inherit the shipped policy once it is enabled:

| Caller | Change |
| --- | --- |
| `SearchAgent::choose` (`src/agents.rs`) | calls `play` instead of `search_tt` |
| CLI `play` (`src/cli.rs:155`) | via `SearchAgent` |
| GUI `start_engine` (`gui/src/main.rs:192`) | calls `play` instead of `search_tt` |
| tools `match` agents (`tools/src/agents.rs`) | via `SearchAgent`; book agents are added with #151's clock syntax, not today's per-move parser |
| `engine_worker`, `edax_ladder` agent | via `SearchAgent` |

- **Explicit `book: false`:** `phase2_config`, `classic_config`,
  `pc:<t|off>` and the full-width oracle helpers. `frozen_off` is
  extended.
- `standard()` keeps `book: false` until the gate passes. With
  `book: false`, play is bit for bit today's.

## Gate

- **When:** after #151 merges, pre-registered in the gate issue before any
  game.
- **Setup:** #151's game clock, at equal total game time, with book on
  against book off and the same engine otherwise.
- **Starts:** the 400 frozen roots above, one pair per root, with colours
  swapped.
- **Pre-registered with the clock:** the actual clock settings, load and
  void rules, and timeout treatment.
- **Statistics:** pair bootstrap (10,000 resamples) with the degeneracy
  fallback, as for earlier gates. One look; enable only if the lower end
  of the interval is above 0.500.
- **Diagnostic (non-gated):** book moves per game for each side, and
  clock time saved.

## Tests

### Engine (RED, in the engine issue)

**Symmetry:**
- On an asymmetric fixture position, the stored move maps back exactly
  under all 8 transforms.
- On symmetric fixtures, including the start position, the returned move
  is legal, its value is identical across orientations, and it lies in
  the stored move's orbit under the board's stabiliser. Exact
  equivariance is impossible there, because the start board is invariant
  under a 180° turn that swaps its moves.

**Misses and bad data:**
- A position not in the book, an illegal stored move and an out-of-range
  stored square all return `None`.
- An empty book, an asset length that is not a multiple of 11 and i16
  value bounds are handled.
- A board with no legal move, and a terminal board, bypass the book.
- **Re-entry:** a game leaves the book through a search move, then
  reaches a stored position, and the book answers again.

**`play`:**
- With `book: false`, it equals `search_tt` exactly.
- On a hit it returns the book move with `nodes` 0 and the status above;
  on a miss it falls back to `search_tt`.
- `search_tt` gives identical results with `book` on and off.

**The shipped asset:**
- It is sorted, with no duplicate keys, and its length is a multiple of
  11.
- Every record matches the committed manifest.
- Every manifest move is legal in its board.

**Integration and frozen configurations:**
- A book hit and a book miss through `SearchAgent`, the CLI play path
  and the GUI engine call.
- Frozen configurations have `book: false`.

### Builder (in the builder issue)

**Partition:**
- An unobserved legal edge into a node reached by another transposition
  is a book move.
- Every legal move of every node is classified exactly once.
- Two squares reaching the same canonical child both appear.

**Passes, terminals and colour:**
- A contracted passing book move: worth +v(child), with the same side to
  move after it.
- A legal placement that produces an out-of-book forced-pass child: the
  value helper passes at the same depth and never calls `search_tt`
  without a legal move.
- A placement that produces a terminal child, valued by
  `terminal_score(final_score)` with empty squares awarded to the winner.
- Colour carried through a pass in the reachability walk.
- Pass and terminal nodes are never emitted.

**Values:**
- Backup on a hand-built DAG with transpositions.
- Drop-out excludes book moves and follows the square order.
- All the tie rules.

**Pruning:**
- Reachability keeps exactly the reachable union for the declared roots.
- Overflow removes in (count, key) order, with reachability recomputed.
- A low-count frozen root survives overflow.
- After a removal, a retained parent keeps its stored move even when
  that move's child was removed. The walk stops there, and the removed
  child is never emitted.
- The build fails when the mandatory records cannot fit.
- Truncated records keep their pre-truncation move and value.

**Determinism:**
- One worker versus several, reordered jobs, and kill-and-resume give
  identical per-node scores and identical asset and manifest bytes.
- Duplicate keys for distinct boards fail the build.

**Extension (amendment 3, in #157):**
- Departures: only walk records whose backed-up move is a drop-out, from
  the untruncated walk, in key order, on a hand-built DAG.
- Phase contexts: a same-phase peer edge is a book move and is not
  searched. Labels, journals' logical records and the build are identical
  under reversed unit order and one versus several workers.
- Phase B selection from the post-A backup: an existing book move beats
  the best drop-out (no B unit); all moves are book moves; tied values go
  to the lower square; a pass on the A edge or on the B edge, with the
  roles that follow.
- Terminal children and children already in the node set are never units.
- Decoder: terminal and forced-pass children interspersed in a token
  vector; missing, extra, non-integer and out-of-range tokens refused; a
  `best` that disagrees with the decoded vector refused.
- Reclassification: the old best square promoted to a book move, so a
  different runner-up becomes `x(p)`; every drop-out becomes a book move,
  so `x(p)` is absent; two squares transposing to the same new child; the
  value backs up through the new child, not only the old aggregate's
  removal.
- Identity: changed roots, a changed earlier phase, or changed unit or
  context sets refuse to resume; a different policy or `eval.bin` hash
  refuses to start; foreign and duplicate units refused; a torn tail
  discarded.
- Kills inside A, inside B and between them; a valid empty B; an empty
  A_r ending the rounds; a partial final round ignored by build and stated
  in the manifest.
- R = 0 builds the #154 asset and manifest byte-identically.
- Overflow removes extension nodes first, in `(count, zobrist)` order, and
  keeps the mandatory roots.

## Issues

1. **Builder** (after the user decision): node set, partition, scoring
   journal, backup, roots file, pruning, asset and manifest writer, then
   the pilot and the D rule.
2. **Build run:** the long scoring job, with the asset, manifest and log
   committed.
3. **Engine:** `book.rs`, `play`, the `book` switch, callers and tests.
4. **Gate and enablement:** after #151. A separate enablement commit
   sets `book: true` in `standard()`.

## Not in scope

- Edax's book: it is GPL, and #130 forbids vendoring Edax data.
- Other engines' books.
- Book learning from the engine's own games.
- Randomised book choice: play is deterministic.

## Review response

### First review, of `c751eb0`

| Finding | Change |
| --- | --- |
| A legal move can fall into neither category | Every legal move into the node set is a book move, whether observed or not; everything else is a drop-out move. Per-square labels; pass and terminal rules; colour carried explicitly; backup order defined. Builder tests added. |
| Reproducibility needs a fixed search order | Canonical orientation and its tie rule, ascending square order, a table cleared once per node, no hash-map iteration, and the rules pinned in the journal header. Determinism tests across workers, job order and resumption. The full-search equivalence claim is replaced: the book's scoring algorithm is defined by itself. |
| Pilot details | A seeded uniform sample of 200 from the actual node set, a resumable per-(node, depth) journal, a projection with a bootstrap upper bound, nodes with no drop-outs included, and a defined outcome when even D = 18 is too slow. |
| Pruning roots and gate disagree | Roots made explicit: the start position plus the 400 gate roots, both colours each. Gate roots drawn only from node-set positions at plies 6–8 and frozen before scoring. The gate's claim is stated as conditional. Coverage logged on the final artifact before play. |
| Overflow semantics | A deterministic (count, key) order with reachability recomputed after each removal. Stored records keep their pre-truncation recommendations. Described as a truncation, separately from lossless reachability. |
| Symmetry test impossible on symmetric positions | Transform ties go to the smallest k. Exact inverse tested on an asymmetric fixture; legality, value and stabiliser orbit tested on symmetric fixtures, including the start. |
| Builder home and budget | Remains an explicit user decision before builder GREEN, with the reviewer's preference recorded. |
| Non-blocking | Duplicate-key check and a board-bearing manifest; fixture tests for malformed data and bypasses; the sizing replay's boundary clarified; gate clock details deferred to the gate's pre-registration. |

### Second review, of `be755f9`

| Finding | Change |
| --- | --- |
| Scoring no-move children calls an incompatible root API | A child value helper over any board: terminal score, pass at the same depth, and `search_tt` only when a legal move exists. A pass parent's drop-out is at depth D. Terminal values use `terminal_score(final_score(board))`. Pass and terminal nodes are internal and never emitted. Fixtures added for out-of-book forced-pass and terminal children. |
| "Lossless for its roots" does not cover re-entry | The guarantee is narrowed to connected book lines. Re-entry through stateless lookup is kept, with reduced coverage, and no latch. A leave-and-re-enter test is added. |
| Overflow can drop gate roots | The start position and the 400 gate roots are mandatory and protected, and the build fails if they cannot fit. The coverage claim is restricted to the side to move at each root, with the other colour's first-move hits logged. A low-count frozen root test is added. |
| Bootstrap bound wording | The bound is the 97.5th percentile of the bootstrapped projections, used as it stands. |

### Amendment 1, after approval (for review with #153)

| Change | Reason |
| --- | --- |
| The builder home is decided: the counted trainer, with a 2,000-line budget. | User decision. |
| The `eval.bin` hash is recorded for provenance only, with no pin test; a retrain keeps the book, and rebuilding is optional. | User decision: a forced rebuild on every retrain is not wanted, and nothing in lookup depends on the evaluator. |

### Amendment 2, from the #153 RED review

| Change | Reason |
| --- | --- |
| Ply counts placements only. Forced passes are contracted by `normalize`, so there are no pass nodes. A passing book move is worth +v(child), and the walk's role flips back across the pass. | Accepted in the #153 RED review; the implementation instructions above now state it. |
| The second tie rule is removed; ties go to the lower square. | It could never apply. |
| An old book's values are offline scores, not new-model evaluations. Build inputs are kept, and journal resumption still checks the full header. | #153 RED review, on amendment 1. |

### Amendment 3, extension rounds (#157 builder, #158 run)

| Change | Reason |
| --- | --- |
| Extension rounds R = 3 of phase A (departure children) and phase B (the child of each A unit's backed-up best move), scored with the #154 policy at D = 19, one resumable journal per phase. | The #154 book's own choices end its connected prefix in half its stored records. User decision, 17 Nov 2026: "yes, draft the A+B amendment. It must be interruptible." |
| Extension nodes join the node set with count 0; reclassified drop-out squares back up through the new child without rescoring. | Keeps one partition and one backup rule. |
| Trainer guard to be raised in #157 for the extension code. | The user's standing permission; the engine is untouched. |

### Review of amendment 3, `4fae9b8`

| Finding | Change |
| --- | --- |
| Phase context and units conflated | Each phase has a sorted unit set and an immutable context snapshot (previous nodes plus that phase's units), fixed before any unit runs. Same-phase peer edges are book moves. Departures come from the untruncated walk; truncation only at emission. |
| Best drop-out is not the best reply | B takes each A unit's backed-up best move from the completed post-A backup, book or drop-out, with the existing tie rule. A book move contributes no unit. Roles after passes are stated mechanically. No monotonic coverage promised. |
| Journal tokens are raw search results | A decoder over the original context: ascending drop-out squares, terminal children without a token, forced-pass children +t, others −t, exact token count, domain check, `best` cross-checked. Every reclassified square is removed and `x(p)` recomputed. |
| Resume identity incomplete | Headers bind the base, prior-phase, root, unit and context digests (canonical sorted records), R, round, phase and the extender commit. Policy and `eval.bin` are checked against the base before work. Early stop, empty phases, partial rounds and the manifest's requested and completed rounds are defined. |
| Non-blocking | Sizes stated as guesses with their assumptions; the overflow key named as `(count, zobrist(canonical board))`; the guard raise described as planned, not done. |
