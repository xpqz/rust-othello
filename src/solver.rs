//! Depth-free exact endgame solver.

use crate::board::{Board, final_score, legal_moves, pass, play};
use crate::score::{INF, terminal_score};
use crate::search::{Ctx, ordered_moves};
use crate::tt::{Bound, Domain, Entry, NO_MOVE, zobrist};

pub fn solve(board: Board, mut alpha: i32, beta: i32, ctx: &mut Ctx) -> Option<i32> {
    ctx.nodes += 1;
    if ctx.stop() {
        return None;
    }
    let moves = legal_moves(board.me, board.opp);
    if moves == 0 {
        if legal_moves(board.opp, board.me) == 0 {
            return Some(terminal_score(final_score(board)));
        }
        return solve(pass(board), -beta, -alpha, ctx).map(|value| -value);
    }
    let key = ctx.table.as_ref().map(|_| zobrist(board));
    let entry = key.and_then(|key| ctx.table.as_ref()?.probe(key)).copied();
    if let Some(value) = entry.and_then(|entry| entry.endgame_cutoff(alpha, beta)) {
        return Some(value);
    }
    let alpha_orig = alpha;
    let (order, len) = ordered_moves(board, moves, 2, None);
    let mut best = -INF;
    let mut chosen = NO_MOVE;
    for &square in &order[..len] {
        let value = -solve(play(board, square), -beta, -alpha, ctx)?;
        if value > best {
            best = value;
            chosen = square;
        }
        alpha = alpha.max(value);
        if alpha >= beta {
            break;
        }
    }
    if let (Some(table), Some(key)) = (ctx.table.as_mut(), key) {
        let flag = match (best <= alpha_orig, best >= beta) {
            (true, _) => Bound::Upper,
            (_, true) => Bound::Lower,
            _ => Bound::Exact,
        };
        let entry = Entry {
            key,
            value: best,
            depth: 0,
            flag,
            domain: Domain::Endgame,
            mv: chosen,
        };
        table.store(entry);
    }
    Some(best)
}

pub fn root(board: Board, moves: u64, best: u8, ctx: &mut Ctx) -> Option<(u8, i32)> {
    let (order, len) = ordered_moves(board, moves, 2, Some(best));
    let mut chosen = best;
    let mut score = -INF;
    let mut alpha = -INF;
    for &square in &order[..len] {
        let value = -solve(play(board, square), -INF, -alpha, ctx)?;
        if value > score {
            score = value;
            chosen = square;
        }
        alpha = alpha.max(value);
    }
    if let Some(table) = ctx.table.as_mut() {
        table.store(Entry {
            key: zobrist(board),
            value: score,
            depth: 0,
            flag: Bound::Exact,
            domain: Domain::Endgame,
            mv: chosen,
        });
    }
    Some((chosen, score))
}
