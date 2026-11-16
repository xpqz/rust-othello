# Opening book: Design

Nov 16, 2026 · Draft, revision 2 · SKIS epic #150, design issue #152

> Scope: an opening book built from the WTHOR archive and scored by our own
> engine, consulted only when the engine chooses a move to play. Search,
> the exact solver and every `search_tt` caller are untouched.
> `docs/PLAN.md` stays the source of truth for budgets, statistics and
> process. The evidence is `tools/examples/book_coverage.rs` and its log
> `docs/tuning/book_coverage.log` (`d76aaa6`). Two decisions belong to the
> user and are marked **User decision**. Revision 2 answers the first
> review; [Review response](#review-response) maps each finding to its change.

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

- **Nodes:** canonical positions at plies 0–29 that occur in at least
  `N = 10` WTHOR games. The builder replays each game and counts the
  position after every move, including a finished game's final position,
  once per game per position. That gives about 18,877 nodes; the
  builder's own count is logged. N and the ply limit are fixed now.
- **Partition of a node's legal moves.** Every legal square s of node p,
  in its canonical frame, falls into exactly one class:
  - **book move:** the canonical child `play(p, s)` is a node, whether or
    not any game played s from p. A transposition reached another way
    still counts.
  - **drop-out move:** the child is not a node.
- Several squares can reach the same canonical child, for example on
  symmetric boards. Each square is still listed once, with its own label.
- **No legal move, but the opponent can move:** the node has one pass
  move. The passed position is a node or not, like any other child, and
  passing keeps the sign rule (−value of the child).
- **Neither side can move (terminal):** the value is the exact final
  score, `terminal_score` of the disc difference, and the node has no
  moves.
- **Colour:** a node's key is the board alone, with no colour. The two
  reachability walks below carry the engine's colour as explicit state,
  flipping it on every move including passes. Colour is never inferred
  from disc count.

### Values

All values are centidiscs from the side to move.

- **Drop-out value** `x(p)`: the drop-out moves are searched in ascending
  canonical square order. Each child gets one `search_tt` at fixed depth
  `D − 1`. The table is cleared once, before p's first drop-out search,
  and kept warm across p's drop-out moves. `x(p)` is the maximum of minus
  those scores, taking the lower square on ties.
- If p has no drop-out moves, `x(p)` is absent.
- **Backed-up value**: `v(p) = max(x(p), max over book moves s of
  −v(child(p, s)))`. A pass move counts as a book or drop-out move by its
  child, with the drop-out search on the passed position.
- **Book move**: the move attaining `v(p)`. Ties go to the lower canonical
  square, then to a book move over a drop-out move.
- A node has at least one move unless it is terminal, so the maximum is
  never taken over an empty set.
- **This is the book's own scoring algorithm.** A node whose moves are all
  drop-outs gets the maximum over its children searched one by one with a
  warm table. That is not claimed to equal `search_tt(p, D)`.
- **Order:** nodes are backed up in descending order of disc count. A pass
  keeps the disc count, so a passing node comes after its passed child.

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
- **Projection** per D: (mean pilot node time) × (node count) ÷ 5 threads,
  plus a 95% upper bound from a bootstrap over the sampled node times
  (10,000 resamples, seed 150).
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
  seeded shuffle (seed 151), from the node-set positions at plies 6–8.
  The pool is 232 + 402 + 645 = 1,279 positions in the sizing count; the
  builder's exact pool is logged.
- **Committed in the builder issue,** before any scoring:
  `bench/openings_book.txt`, holding each root's move sequence from a
  WTHOR game.
- **No replacements:** no root is ever replaced because of an outcome.
- **Every root is a node by construction,** so the book covers the
  first move of every gate game for both colours.

### Asset, pruning and overflow

- **Room:** `assets/` totals 4,014,728 bytes against a 4 MiB guard, which
  leaves 179,576 bytes. The include guard already accepts any file under
  `assets/` (`tests/budget/rules.rs`).
- **Format:** `assets/book.bin`, sorted fixed-width records of 11 bytes,
  little-endian: `tt::zobrist` of the canonical board (u64), the book
  move as a canonical square (u8), and `v(p)` clamped to i16. That is at
  most 16,325 records. All 18,877 nodes would need 207,647 bytes, so
  pruning is required.
- **Reachability (lossless for its roots):** the roots are the start
  position, with the engine as Black and as White, plus each of the 400
  gate roots with the engine to move and with the opponent to move. From
  each root, at engine-to-move nodes the walk follows the book move only;
  at opponent nodes it follows every book move. Stored positions are the
  engine-to-move nodes reached. On that set the stored book is exactly
  the full book for play from those roots.
- **Overflow (a deliberate truncation, not a guarantee):** if the
  reachable set is still too large, nodes are removed in ascending
  (WTHOR count, zobrist key) order, and reachability is recomputed after
  each removal, until the set fits. Stored records keep their
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

### Builder home and reproducibility

- **The rule:** `docs/PLAN.md` says assets "must be reproducible from the
  trainer crate", and training and fitting pipelines live in the counted
  trainer. The trainer has 50 of its 1,200 lines left, and the builder
  needs an estimated 250–350.
- **User decision,** required before builder GREEN: (a) raise the trainer
  budget, which the reviewer prefers as an explicit counted decision;
  (b) exempt book building; or (c) amend `docs/PLAN.md` to allow a tools
  example. PLAN and the epic wording follow whichever is chosen.
- **Reproducibility:** the asset is a pure function of the WTHOR files
  (whose hashes are in the header), N, the ply limit, D, the roots, the
  rules above and the engine commit. The book is tied to the evaluator,
  and a test pins the `eval.bin` hash recorded with the asset.

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
- Fixtures for a forced pass (sign and order), double no-move
  (terminal value), and colour carried through a pass in the
  reachability walk.

**Values:**
- Backup on a hand-built DAG with transpositions.
- Drop-out excludes book moves and follows the square order.
- All the tie rules.

**Pruning:**
- Reachability keeps exactly the reachable union for the declared roots.
- Overflow removes in (count, key) order, with reachability recomputed.
- Truncated records keep their pre-truncation move and value.

**Determinism:**
- One worker versus several, reordered jobs, and kill-and-resume give
  identical per-node scores and identical asset and manifest bytes.
- Duplicate keys for distinct boards fail the build.

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
