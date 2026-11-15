//! Opening book sizing (design `docs/design/book.md`). Counts how often each
//! canonical position (D4 symmetry) occurs in WTHOR games, per ply; the
//! coverage of the gate opening sets; and the WTHOR share of all positions
//! at plies 1 to 6. With `--time D1,D2,..`, times the scoring searches for a
//! sample of frequent positions: one fixed-depth search of the position
//! (leaf) and one per non-book child at one less depth (drop-out).
//! usage: book_coverage [--time 16,18,20] [--sample 12] [--min 10]

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use othello::board::{Board, legal_moves, pass, play, squares};
use othello::search::{SearchConfig, SearchLimits, search_tt};
use othello::tt::{MAX_ENTRIES, Table};
use othello_tools::sym::canonical;

const WTHOR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../data/wthor");
const BENCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../bench");
const PLIES: usize = 30;
const THRESHOLDS: [u32; 7] = [1, 2, 5, 10, 20, 50, 100];

type Key = (u64, u64);

/// Positions (before each move) of every WTHOR game that replays legally.
fn games() -> (Vec<Vec<Board>>, usize) {
    let mut names: Vec<_> = std::fs::read_dir(WTHOR)
        .expect("data/wthor")
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wtb"))
        })
        .collect();
    names.sort();
    let (mut games, mut rejected) = (Vec::new(), 0);
    for name in names {
        let bytes = std::fs::read(&name).unwrap();
        for record in bytes[16..].as_chunks::<68>().0 {
            let moves = record[8..].iter().take_while(|&&byte| byte != 0);
            match replay(moves.map(|&byte| (byte / 10 - 1) * 8 + byte % 10 - 1)) {
                Some(boards) => games.push(boards),
                None => rejected += 1,
            }
        }
    }
    (games, rejected)
}

fn replay(moves: impl Iterator<Item = u8>) -> Option<Vec<Board>> {
    let mut board = Board::START;
    let mut boards = Vec::new();
    for square in moves {
        if legal_moves(board.me, board.opp) == 0 {
            board = pass(board);
        }
        if square >= 64 || legal_moves(board.me, board.opp) & (1 << square) == 0 {
            return None;
        }
        boards.push(board);
        board = play(board, square);
    }
    Some(boards)
}

fn parse_line(line: &str) -> Board {
    line.split_whitespace().fold(Board::START, |board, name| {
        let bytes = name.as_bytes();
        play(board, (bytes[1] - b'1') * 8 + bytes[0] - b'a')
    })
}

fn value(args: &[String], flag: &str) -> Option<String> {
    let at = args.iter().position(|arg| arg == flag)?;
    Some(
        args.get(at + 1)
            .unwrap_or_else(|| panic!("{flag} needs a value"))
            .clone(),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (games, rejected) = games();
    let mut counts: Vec<HashMap<Key, u32>> = vec![HashMap::new(); PLIES];
    for game in &games {
        for (ply, &board) in game.iter().take(PLIES).enumerate() {
            *counts[ply].entry(canonical(board)).or_default() += 1;
        }
    }
    println!("book_coverage v1 games {} rejected {rejected}", games.len());
    println!("ply: positions occurring in at least {THRESHOLDS:?} games");
    let mut total = [0usize; 7];
    for (ply, count) in counts.iter().enumerate() {
        let row = THRESHOLDS.map(|t| count.values().filter(|&&n| n >= t).count());
        (0..7).for_each(|i| total[i] += row[i]);
        println!("{ply:3} {row:?}");
    }
    println!("plies 0..{PLIES}: {total:?}");
    for set in ["openings_probcut3.txt", "openings_final.txt"] {
        let text = std::fs::read_to_string(format!("{BENCH}/{set}")).unwrap();
        let ends: Vec<u32> = text
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(|line| {
                counts[8]
                    .get(&canonical(parse_line(line)))
                    .copied()
                    .unwrap_or(0)
            })
            .collect();
        let seen = ends.iter().filter(|&&n| n > 0).count();
        let five = ends.iter().filter(|&&n| n >= 5).count();
        println!(
            "{set}: {} openings, end position in WTHOR {seen}, in >= 5 games {five}",
            ends.len()
        );
    }
    let mut boards = vec![Board::START];
    for (ply, count) in counts.iter().enumerate().take(7).skip(1) {
        let mut level = HashSet::new();
        let mut next = Vec::new();
        for board in boards {
            for square in squares(legal_moves(board.me, board.opp)) {
                let child = play(board, square);
                if level.insert(canonical(child)) {
                    next.push(child);
                }
            }
        }
        let seen = level.iter().filter(|key| count.contains_key(key)).count();
        let two = level
            .iter()
            .filter(|key| count.get(key).is_some_and(|&n| n >= 2))
            .count();
        println!(
            "ply {ply}: positions {}, in WTHOR {seen}, in >= 2 games {two}",
            level.len()
        );
        boards = next;
    }
    if let Some(depths) = value(&args, "--time") {
        let sample: usize = value(&args, "--sample").map_or(12, |text| text.parse().unwrap());
        let min: u32 = value(&args, "--min").map_or(10, |text| text.parse().unwrap());
        let depths: Vec<u32> = depths.split(',').map(|d| d.parse().unwrap()).collect();
        timing(&games, &counts, &depths, sample, min);
    }
}

/// For plies 10 and 20, the first `sample` positions (game order) occurring in
/// at least `min` games. Explicit scoring policy: the standard evaluator and
/// fitted ProbCut, threshold 18, fresh table per search.
fn timing(
    games: &[Vec<Board>],
    counts: &[HashMap<Key, u32>],
    depths: &[u32],
    sample: usize,
    min: u32,
) {
    let config = SearchConfig::standard();
    let mut table = Table::new(MAX_ENTRIES);
    println!(
        "timing: standard() (fitted ProbCut, threshold 18), table {MAX_ENTRIES}, cleared per leaf search and per drop-out set"
    );
    for ply in [10, 20] {
        let mut seen = HashSet::new();
        let picks: Vec<Board> = games
            .iter()
            .filter_map(|game| game.get(ply).copied())
            .filter(|&board| counts[ply][&canonical(board)] >= min && seen.insert(canonical(board)))
            .take(sample)
            .collect();
        for &depth in depths {
            let (mut leaf, mut dropout, mut children) = (Vec::new(), Vec::new(), 0);
            for &board in &picks {
                table.clear();
                let start = Instant::now();
                search_tt(board, SearchLimits::depth(depth), &config, &mut table);
                leaf.push(start.elapsed().as_secs_f64());
                let book: HashSet<Key> = games
                    .iter()
                    .filter_map(|game| {
                        let at = game
                            .iter()
                            .position(|&b| canonical(b) == canonical(board))?;
                        game.get(at + 1).map(|&next| canonical(next))
                    })
                    .collect();
                let start = Instant::now();
                table.clear();
                for square in squares(legal_moves(board.me, board.opp)) {
                    let child = play(board, square);
                    if !book.contains(&canonical(child)) {
                        children += 1;
                        search_tt(child, SearchLimits::depth(depth - 1), &config, &mut table);
                    }
                }
                dropout.push(start.elapsed().as_secs_f64());
            }
            let median = |v: &mut Vec<f64>| {
                v.sort_by(f64::total_cmp);
                v[v.len() / 2]
            };
            let (leaf_sum, drop_sum): (f64, f64) = (leaf.iter().sum(), dropout.iter().sum());
            println!(
                "ply {ply} depth {depth}: positions {} leaf median {:.2}s mean {:.2}s; drop-out ({children} children) median {:.2}s mean {:.2}s",
                picks.len(),
                median(&mut leaf),
                leaf_sum / picks.len() as f64,
                median(&mut dropout),
                drop_sum / picks.len() as f64,
            );
        }
    }
}
