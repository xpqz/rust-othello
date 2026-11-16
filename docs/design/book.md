# Opening book: Design

Nov 16, 2026 · Draft, revision 1 · SKIS epic #150, design issue #152

> Scope: an opening book built from the WTHOR archive and scored by our own
> engine, consulted only when the engine chooses a move to play. Search,
> the exact solver and every `search_tt` caller are untouched.
> `docs/PLAN.md` stays the source of truth for budgets, statistics and
> process. The evidence is `tools/examples/book_coverage.rs` and its log
> `docs/tuning/book_coverage.log` (`d76aaa6`). Two decisions belong to the
> user and are marked **User decision**.

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

`book_coverage` replays all 137,548 WTHOR records (none fail; unfinished
games count up to their last move). It counts canonical positions
(D4 symmetry, `tools/src/sym.rs`) by ply, where row k is the position
after k moves.

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

### Nodes and edges

- **Nodes:** canonical positions at plies 0–29 that occur in at least
  `N = 10` WTHOR games: 18,877 positions. N and the ply limit are fixed in
  the builder issue before any scoring.
- **Edges:** a move from node p to node c, both in the book, played in at
  least one WTHOR game. Transpositions merge by canonical key, so the book
  is a DAG.
- **Positions with no legal move** pass, so their child is the passed
  position. WTHOR records that fail to replay are excluded.

### Values

All values are from the side to move, in the engine's centidisc scale.

- **Drop-out value** `x(p)`: the best value over p's legal moves whose
  child is not a book node. Each such child is searched at fixed depth
  `D − 1`, and the value is the maximum of minus those scores. If every
  child is a book node, there is no drop-out. This is the standard
  correction that keeps a book from trusting only human moves.
- **Backed-up value**: `v(p) = max(x(p), max over book children c of −v(c))`.
- A node with no book children therefore gets the full depth-D search
  value, and no separate leaf rule is needed.
- **Book move** at p: the move that attains `v(p)`. If that is a drop-out
  move, the engine leaves the book there. Ties go to the lower square in
  the canonical frame.

### Scoring search (explicit, not `standard()`)

- **Proposed policy:** `pattern::evaluate`, `endgame_threshold` 18,
  `ordering` true and `probcut: Some(ProbCut::FITTED)`. These are the
  values the shipped engine plays with, written out field by field.
- **Fixed depth:** `D = 20`, so drop-out children are searched at depth 19.
- **Table:** `MAX_ENTRIES`, cleared before each node's drop-out set.
- **Determinism:** each node's searches depend only on the node and the
  policy, so results do not depend on thread count or order.

### Cost

- 18,877 nodes at a mean drop-out cost of about 15–23 s at D = 20 is about
  80–120 CPU-hours, or roughly 16–24 hours on 5 threads. At D = 18 it is
  about 5–6 hours.
- The table above counts as drop-out only children that no WTHOR game
  played. With N = 10 more children fall outside the book, so the real
  cost is higher.
- The builder issue therefore starts with a pilot of 200 nodes, timed and
  logged, and chooses D from it. The rule: the largest D in {18, 19, 20}
  whose projected run is at most 36 hours on 5 threads. The rule is fixed
  before the pilot runs.

### Asset

- **Room:** `assets/` totals 4,014,728 bytes against a 4 MiB guard, which
  leaves 179,576 bytes. The include guard already accepts any file under
  `assets/` (`tests/budget/rules.rs`).
- **Format:** `assets/book.bin`, sorted fixed-width records of 11 bytes
  each: `tt::zobrist` of the canonical board (u64), the book move in the
  canonical frame (u8), and `v(p)` (i16). That is at most 16,325 records,
  stored little-endian.
- **Reachability pruning:** for each colour, the engine only needs the
  nodes reachable when it plays the book move at its own turns and the
  opponent plays any book child at theirs. Only the union of the two sets
  is stored.
- **Overflow:** if the pruned set still exceeds the room, the
  lowest-count nodes are dropped until it fits. This rule is fixed now.
- **User decision:** keep the 4 MiB asset guard, which this design fits
  under, or raise it for a larger book.

### Builder home and reproducibility

- **The rule:** `docs/PLAN.md` says assets "must be reproducible from the
  trainer crate". The trainer has 50 of its 1,200 lines left. The builder
  needs node collection, a resumable threaded scoring journal, the DAG
  backup, pruning and the asset writer: an estimated 250–350 lines.
- **User decision:**
  - (a) raise the trainer budget for the builder;
  - (b) exempt book building, as was done for the ProbCut fitting tools,
    and keep it in the trainer outside the count; or
  - (c) build it as an uncounted `tools/examples` program, with
    `docs/PLAN.md` amended.
- **Either way:**
  - The scoring run is interruptible and restartable (`docs/PLAN.md`
    § Interruptible long-running jobs), journalled per node and keyed by
    the canonical position.
  - The asset is a pure function of the WTHOR files (whose hashes are
    in the header), N, the ply limit, D and the engine commit.
  - The book is tied to the evaluator and must be rebuilt whenever the
    evaluator changes. A test pins the asset's hash to the
    `eval.bin` hash recorded in the build log.

## Engine

### Lookup

- `src/book.rs`, `pub fn book_move(board: Board) -> Option<(u8, i32)>`:
  1. It forms the 8 D4 transforms of the board with bit operations (the
     engine has only the square transform in `pattern.rs`) and takes the
     minimum `(me, opp)`, keeping the transform used.
  2. It binary-searches the zobrist key of that canonical board in
     `include_bytes!("../assets/book.bin")`.
  3. It maps the stored move back through the inverse transform.
  4. It returns `None` unless that move is legal. A 64-bit key collision
     can only ever produce a legal move.
- **Estimated size:** 45–60 engine lines with the play helper below. The
  engine is at 1,218 of 1,500.

### Where it is consulted (the full consumer audit)

- `SearchConfig` gains `book: bool`. Only a new function reads it:
  `play(board, limits, config, table) -> SearchResult`. When `book` is
  set and `book_move` answers, `play` returns that move with the book
  value, `depth` 0, `nodes` 0, `Complete` and not solved. Otherwise it
  calls `search_tt`.
- **`search_tt` never reads `book`.** Every `search_tt` caller is
  therefore unchanged by construction, whatever `standard()` holds. That
  covers the CLI `search` command, the oracles and golden tests,
  `probcut_fit`, `probcut_screen`, the benches, `edax_gap`, `edax_phase`,
  `edax_ladder`'s exact check, `fallback_cost` and `solve_times`. A test
  asserts that `search_tt` results are identical with `book` on and off.

Playing paths, which inherit the shipped policy once it is enabled:

| Caller | Change |
| --- | --- |
| `SearchAgent::choose` (`src/agents.rs`) | calls `play` instead of `search_tt` |
| CLI `play` (`src/cli.rs:155`) | via `SearchAgent` |
| GUI `start_engine` (`gui/src/main.rs:192`) | calls `play` instead of `search_tt` |
| tools `match` agents (`tools/src/agents.rs`) | via `SearchAgent`; new `bk:<on\|off>[:<ms>]` gate agents |
| `engine_worker`, `edax_ladder` agent | via `SearchAgent` |

Frozen and explicit configurations set `book: false`: `phase2_config`,
`classic_config`, `pc:<t|off>` and the full-width oracle helpers. `frozen_off`
is extended. `standard()` keeps `book: false` until the gate passes.

With `book: false`, play is bit for bit today's.

## Gate

- **When:** after #151 merges, using its game clock at equal total
  game time, with book on against book off, same engine otherwise.
  Pre-registered in the gate issue before any game.
- **Openings:** 400 drawn without replacement from the 546 canonical
  ply-6 positions that occur in at least 2 WTHOR games, by a fixed seed.
  Pairs swap colours.
- **Openings are in the book's sample by design:** the book is meant for
  positions people actually reach. The current fresh sets would leave
  the book almost unused (37 of 1,024).
- **Statistics:** pair bootstrap (10,000 resamples) with the degeneracy
  fallback, as for earlier gates. One look; enable only if the lower end
  of the interval is above 0.500.
- **Diagnostic (non-gated):** the mean number of book moves per game for
  each side, and the clock time saved.

## Tests (RED, in the engine issue)

- `book_move` on a fixture book: it finds a stored position under all 8
  symmetries and returns the move mapped back.
- It returns `None` for a position not in the book, and for an illegal
  stored move.
- `play` with `book: false` equals `search_tt` exactly; with `book: true`
  and a hit it returns the book move with `nodes` 0.
- `search_tt` ignores `book`.
- The asset loads, is sorted, holds no duplicate keys, and every stored
  move is legal in its position.
- Frozen configurations have `book: false`.

Builder tests (in the builder issue):

- Backup on a small hand-built DAG with transpositions.
- Drop-out excludes book children.
- Pruning keeps exactly the reachable union.
- Overflow drops the lowest-count nodes.
- Kill-and-resume gives byte-identical results.

## Issues

1. **Builder** (after the user decisions): node set, scoring journal,
   backup, pruning, asset writer, pilot and the D rule.
2. **Build run:** the long scoring job, the asset and its log committed.
3. **Engine:** `book.rs`, `play`, the `book` switch, callers, `bk:` agents
   and tests.
4. **Gate and enablement:** after #151. A separate enablement commit
   sets `book: true` in `standard()`.

## Not in scope

- Edax's book: it is GPL, and #130 forbids vendoring Edax data.
- Other engines' books.
- Book learning from the engine's own games.
- Randomised book choice: play is deterministic.
