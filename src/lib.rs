//! Othello engine conventions.
//!
//! - Score: WTHOR final score in centidiscs; the objective and heuristic-bound
//!   note live in [`score`].
//! - Start: black discs on d5/e4, white discs on d4/e5, black to move; black
//!   opens `c4`, `d3`, `e6`, `f5`.
//! - Squares: square `i` is `a1 + i`, row-major, with `a1` = bit 0 and `h8` =
//!   bit 63.
//! - Perft: a pass consumes one ply and is a node; `perft(b, 0) = 1`; a
//!   terminal position with `depth > 0` counts as one node. The terminal rule
//!   is this project's convention, not part of Aart Bik's table.
//! - Search: a pass costs no depth; terminal and pass handling precede the
//!   depth cutoff; fail-soft; callers handle pass and terminal roots;
//!   selective (ProbCut) results are approximate and their depth is nominal;
//!   `SearchStatus` is `Complete`, `Partial`, or `Fallback`; fallback is the
//!   first legal move by square index (lowest index).
//! - Endgame: the solver runs only at the root, at or below the configurable
//!   endgame threshold (default 14).
//! - Time control: default 3000 ms per move.
//! - TT: Zobrist keys with the full key stored, strict equal-depth heuristic
//!   reuse, and separate heuristic and endgame domains.
//! - Tools: pair bootstrap with a degeneracy policy, complete frozen
//!   evaluators, and tune/validation/final position sets grouped by the
//!   canonical D4 position after ply 4.
//! - Size (Phase 2): engine plus CLI at most 1,499 formatted lines that are
//!   neither blank nor comment-only (999 in release 1.0.0); tools at most
//!   1,000; blank and all comment-only lines
//!   (`//`, `///`, `//!`, block comments) are excluded.

#![forbid(unsafe_code)]

pub mod agents;
pub mod board;
pub mod cli;
pub mod eval;
pub mod perft;
pub mod probcut;
pub mod rng;
pub mod score;
pub mod search;
pub mod tt;
