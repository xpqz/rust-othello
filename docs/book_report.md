# Opening book: what was built, what it measured, what was tried

Nov 21, 2026. Epic #150, closed with the book switched off. This report
draws together the design (`docs/design/book.md`), the issue notes
(`docs/prs/152.md` to `158.md` and `156.md`) and the committed run data
under `docs/tuning/`.

## Summary

- The book was built and wired into the engine behind a switch. The
  shipped asset has **7,729 positions** (85,019 bytes) from 137,548 WTHOR
  master games, each scored by a depth-19 search with our own engine.
- **At an equal per-move node cap it made play measurably worse.** With
  the book against without it, from 400 frozen book roots at 10,000,000
  nodes per move: score **0.4781, 95% interval [0.4575, 0.4988]**, about
  −30 to −1 Elo (348 W, 69 D, 383 L over 800 games).
- It therefore stays off (`standard()` has `book: false`). Why it loses
  has not been investigated.

## Why a book, and what it could buy

The design gave two reasons (`docs/design/book.md` § Why):

1. **Move quality.** A book value is a negamax over a tree of frequently
   played lines, with leaves up to 29 placements in. A move at ply 4 is
   chosen with information from searches about 25 plies further on. Live
   play at 3 s reached a median depth of only about 19 at ply 20.
2. **Time.** Book moves cost no thinking time. Under the old flat 3 s per
   move that saving was lost, so the payoff was expected to need the game
   clock (#151) first.

## Data and evidence

- **Source:** WTHOR, 137,548 games, none rejected
  (`docs/tuning/book_coverage.log`).
- **Frequent positions:** canonical positions (D4 symmetry) after 0–29
  placements, by how many games reach them:

  | Seen in at least N games | 1 | 2 | 5 | **10** | 20 | 50 | 100 |
  |---|---:|---:|---:|---:|---:|---:|---:|
  | positions | 1,639,683 | 162,777 | 41,295 | **18,877** | 8,977 | 3,607 | 1,829 |

  N = 10 was chosen, giving **18,877 nodes**.
- **Reach into test openings:** only 37 of the 1,024 ProbCut gate
  openings and 22 of the 512 final-check openings occur in WTHOR at all.
  From arbitrary 8-move openings the book is rarely reachable. From the
  standard start it is reached on move one.

## Design, in brief (`docs/design/book.md`, approved with amendments 1–3)

- **Nodes:** canonical, pass-contracted positions seen in at least 10
  games. Every legal move is either a **book move**, whose child is a
  node, or a **drop-out**, which is scored by a search at depth D − 1.
- **Values:** backed up by negamax through book moves; the lower square
  wins ties.
- **Gate roots:** 400 positions drawn uniformly by a seeded shuffle (seed
  151) from nodes at plies 6–8 that have a legal move,
  `bench/openings_book.txt`, frozen before any scoring.
- **Asset:** sorted 11-byte records (zobrist key, canonical move, value),
  pruned to the positions reachable from the roots by walking the book
  move at engine nodes and every book move at opponent nodes. The room is
  179,576 bytes (16,325 records) under the 4 MiB asset guard.
- **Engine lookup** (`src/book.rs`, #155): canonical orientation by bit
  transforms, binary search, the move mapped back and checked for
  legality. `book::play` answers from the book only when
  `SearchConfig::book` is set. `search_tt` never reads it.

## What was built and run

### Builder and scoring (#153, #154)

- **Builder:** in the trainer crate, with an interruptible journal and
  resume (#153).
- **Depth pilot,** under a pre-registered rule (the largest D whose upper
  projection fits 36 h on 5 threads). Projected hours on 5 threads:

  | D | mean | upper (97.5%) |
  |---|---:|---:|
  | 18 | 10.49 | 11.90 |
  | **19** | 19.33 | 21.96 |
  | 20 | 43.00 | 48.96 |

  The rule picked **D = 19**.
- **Scoring:** all 18,877 nodes in 72,552.9 s (20.15 h, 182,337
  searches), on a shared machine
  (`docs/tuning/book_run/score.{journal,log}`).
- **First book:** **3,771 records** (41,481 bytes), with nothing removed
  for room. The start move is d3 (+81 centidiscs). Values in centidiscs
  run from −2,119 to 2,602, median 321.

### The coverage problem and extension rounds (#157, #158)

- **What went wrong:** 1,843 of the 3,771 stored records (49%) chose a
  drop-out move, one that leaves the scored node set, which ends the
  connected book line. 214 of the 401 roots reached only the root record
  itself; 187 of those chose a drop-out at the root.
- **The fix tried, amendment 3:** extension rounds. Each round scores the
  children of the book's own drop-out choices (phase A), then the child of
  each new node's best move (phase B), and backs up again. Three rounds
  were registered and run; journals are in `docs/tuning/book_ext/`.

| Round | Departures | A units | B units | Nodes after |
|---|---:|---:|---:|---:|
| 1 | 1,843 | 1,839 | 1,816 | 22,532 |
| 2 | 2,128 | 2,127 | 2,115 | 26,774 |
| 3 | 2,385 | 2,384 | 2,370 | 31,528 |

| Rounds | Records | Bytes | Roots reaching only themselves (engine first) | Median records per root (first / second) | Start |
|---|---:|---:|---:|---|---|
| 0 | 3,771 | 41,481 | 214 | 1 / 1 | d3 +81 |
| 1 | 5,172 | 56,892 | 52 | 3 / 3 | d3 +73 |
| 2 | 6,644 | 73,084 | 19 | 4 / 5 | d3 +103 |
| **3** | **7,729** | **85,019** | **10** | **6 / 7** | d3 +71 |

- **Result:** the three-round book was approved and replaced
  `assets/book.bin`. Rebuilding with 0 rounds reproduces the first book
  byte for byte.
- **Cost:** the extension run took about 13.8 h.

## How it was tested for strength

| Attempt | Setup | Outcome |
|---|---|---|
| Clock gate (#156, first registration) | `pc:80:c51000v3+book` vs `pc:80:c51000v3`: the same 51 s game clock and early-stopping policy, 400 roots, `--jobs 4 --max-load 6` | **Stopped, never scored.** After 4.8 h only 21 of 400 pairs were accepted (397 void events): background load on a daily-use machine kept load above 6. Journal kept as a record: `docs/tuning/book_gate_stopped.*` |
| **Node check (#156, re-registered)** | `pc:80+book` vs `pc:80`, **10,000,000 nodes per move**, 400 roots, colours swapped, seed 1561. Deterministic, so no load rule | **0.4781 [0.4575, 0.4988]**: fails the not-worse margin (0.475) and lies wholly below 0.500. `docs/tuning/book_nodes.{journal,log,summary}` |

**Scope of the result,** as required by the review:

- same per-move node cap, not equal realized node use;
- these 400 frozen roots only;
- **not** the 51 s game clock, where book moves would also save time.

The node check deliberately gives the book no credit for saved time, so
it tests only the quality of the book's moves.

## What has not been tried

Why the book loses was not investigated. Plausible causes, none tested:

1. **Optimistic drop-out values.** A node's drop-out value is the maximum
   over several independent searches, so noise inflates it. That can make
   the book prefer leaving its own tree, or prefer lines whose value was
   flattered by noise.
2. **The engine's own blind spots.** The book is scored by the same
   engine at depth 19. Where that search misjudges a line, the book
   commits to it confidently.
3. **Lines unsuited to this engine at this effort.** Positions that look
   good after deep scoring may be harder to play at 10 M nodes per move
   than the ones the engine would steer to itself.
4. **A defect** not covered by the lookup tests, such as orientation,
   value sign or the backup. The tests cover symmetry, legality, asset
   integrity and the play paths, and an independent review reconstructed
   the asset, but strength was never checked move by move.

Possible next steps, none started:

- **Diagnose before rebuilding:** take the games the book lost, and
  compare book moves against a deep search, and drop-out against book
  values.
- **Test under the game clock,** where the time saving counts. That needs
  a load-independent clock: the "node clock" (#168, closed unbuilt),
  because wall-clock gates do not complete on a machine in daily use.
- **Shrink the book** to positions with many games or clear value
  margins, or **re-score drop-outs** with a correction for the
  maximum-of-noise bias.

## Files

| Topic | Where |
|---|---|
| Design and amendments | `docs/design/book.md` |
| Issue notes and reviews | `docs/prs/152.md`–`158.md`, `docs/reviews/152.md`–`158.md` |
| Coverage evidence | `docs/tuning/book_coverage.log`, `tools/examples/book_coverage.rs` |
| Depth pilot, scoring, first build | `docs/tuning/book_run/` |
| Extension rounds | `docs/tuning/book_ext/` |
| Shipped asset and manifest | `assets/book.bin`, `docs/tuning/book_manifest.txt` |
| Builder | `trainer/src/book.rs`, `trainer/tests/book*.rs` |
| Engine lookup | `src/book.rs`, `tests/book.rs` |
| Gate roots | `bench/openings_book.txt` |
| Strength check | `docs/tuning/book_nodes.*`, `docs/tuning/book_gate_stopped.*` (unscored) |
