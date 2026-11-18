mod common;

use common::{Sym, transform};
use othello::agents::{Agent, search_agent};
use othello::board::{Board, is_terminal, legal_moves, pass, play};
use othello::book::{BOOK, RECORD, book_move, lookup};
use othello::search::{SearchConfig, SearchLimits, SearchResult, SearchStatus, search_tt};
use othello::tt::{Table, zobrist};

const MANIFEST: &str = include_str!("../docs/tuning/book_manifest.txt");

/// (canonical board, canonical square, value) for each manifest record.
fn manifest() -> Vec<(Board, u8, i32)> {
    MANIFEST
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let (me, opp) = fields[0].split_once(':').unwrap();
            let board = Board {
                me: u64::from_str_radix(me, 16).unwrap(),
                opp: u64::from_str_radix(opp, 16).unwrap(),
            };
            (
                board,
                fields[1].parse().unwrap(),
                fields[2].parse().unwrap(),
            )
        })
        .collect()
}

fn image(board: Board, sym: Sym) -> Board {
    Board {
        me: transform(board.me, sym),
        opp: transform(board.opp, sym),
    }
}

fn square(sq: u8, sym: Sym) -> u8 {
    transform(1 << sq, sym).trailing_zeros() as u8
}

/// The trainer's key: the least `(me, opp)` over the eight images.
fn canonical(board: Board) -> Board {
    Sym::ALL
        .iter()
        .map(|&sym| image(board, sym))
        .min_by_key(|b| (b.me, b.opp))
        .unwrap()
}

fn record(board: Board, sq: u8, value: i16) -> Vec<u8> {
    let mut bytes = zobrist(board).to_le_bytes().to_vec();
    bytes.push(sq);
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// An asset from canonical records, sorted by key.
fn asset(records: &[(Board, u8, i16)]) -> Vec<u8> {
    let mut sorted = records.to_vec();
    sorted.sort_by_key(|r| zobrist(r.0));
    sorted
        .iter()
        .flat_map(|&(b, s, v)| record(b, s, v))
        .collect()
}

/// A record whose eight images are distinct boards.
fn asymmetric() -> (Board, u8, i32) {
    manifest()
        .into_iter()
        .find(|&(b, _, _)| {
            let mut images: Vec<(u64, u64)> = Sym::ALL
                .iter()
                .map(|&s| (image(b, s).me, image(b, s).opp))
                .collect();
            images.sort();
            images.dedup();
            images.len() == 8
        })
        .unwrap()
}

/// 46 plies of lowest-square moves from the start: far past any book line.
fn deep() -> Board {
    let mut board = Board::START;
    for _ in 0..46 {
        let moves = legal_moves(board.me, board.opp);
        board = if moves == 0 {
            pass(board)
        } else {
            play(board, moves.trailing_zeros() as u8)
        };
    }
    board
}

/// `answer` for `board` is legal, has the stored value, and maps to the
/// stored square under some symmetry that takes `board` to its canonical form.
fn assert_orbit(board: Board, answer: Option<(u8, i32)>, stored: u8, value: i32) {
    let (sq, v) = answer.expect("book answers");
    assert_ne!(
        legal_moves(board.me, board.opp) & (1 << sq),
        0,
        "illegal answer"
    );
    assert_eq!(v, value);
    let key = canonical(board);
    assert!(
        Sym::ALL
            .iter()
            .any(|&s| image(board, s) == key && square(sq, s) == stored),
        "answer {sq} is not in the stored square's orbit"
    );
}

fn booked() -> SearchConfig {
    SearchConfig {
        book: true,
        ..SearchConfig::standard()
    }
}

fn fresh(board: Board, config: &SearchConfig) -> SearchResult {
    search_tt(
        board,
        SearchLimits::depth(3),
        config,
        &mut Table::new(1 << 16),
    )
}

fn played(board: Board, config: &SearchConfig) -> SearchResult {
    othello::book::play(
        board,
        SearchLimits::depth(3),
        config,
        &mut Table::new(1 << 16),
    )
}

#[test]
fn asymmetric_orientations() {
    let (key, sq, value) = asymmetric();
    for sym in Sym::ALL {
        let board = image(key, sym);
        assert_eq!(book_move(board), Some((square(sq, sym), value)), "{sym:?}");
    }
}

#[test]
fn start_orbit() {
    let start = canonical(Board::START);
    let (_, sq, value) = manifest().into_iter().find(|r| r.0 == start).unwrap();
    for sym in Sym::ALL {
        let board = image(Board::START, sym);
        assert_orbit(board, book_move(board), sq, value);
    }
}

/// Every stored record, in all eight orientations, symmetric or not.
#[test]
fn manifest_orientations() {
    for (key, sq, value) in manifest() {
        for sym in Sym::ALL {
            let board = image(key, sym);
            assert_orbit(board, book_move(board), sq, value);
        }
    }
}

#[test]
fn miss_none() {
    let board = deep();
    assert!(manifest().iter().all(|r| r.0 != canonical(board)));
    assert_eq!(book_move(board), None);
    let (key, sq, value) = asymmetric();
    let only = asset(&[(key, sq, value as i16)]);
    assert_eq!(lookup(&only, board), None);
    assert_eq!(lookup(&only, key), Some((sq, value)));
}

#[test]
fn illegal_none() {
    let (key, sq, value) = asymmetric();
    let legal = legal_moves(key.me, key.opp);
    let occupied = (key.me | key.opp).trailing_zeros() as u8;
    let empty = (!(key.me | key.opp) & !legal).trailing_zeros() as u8;
    for bad in [occupied, empty] {
        assert_eq!(
            lookup(&asset(&[(key, bad, value as i16)]), key),
            None,
            "{bad}"
        );
    }
    assert_eq!(
        lookup(&asset(&[(key, sq, value as i16)]), key),
        Some((sq, value))
    );
}

#[test]
fn range_none() {
    let (key, _, value) = asymmetric();
    for bad in [64, 65, 255] {
        assert_eq!(
            lookup(&asset(&[(key, bad, value as i16)]), key),
            None,
            "{bad}"
        );
    }
}

#[test]
fn empty_asset() {
    assert_eq!(lookup(&[], Board::START), None);
    assert_eq!(lookup(&[], asymmetric().0), None);
}

/// An asset whose length is not a multiple of the record size answers nothing.
#[test]
fn ragged_asset() {
    let (key, sq, value) = asymmetric();
    let mut bytes = asset(&[(key, sq, value as i16)]);
    assert_eq!(lookup(&bytes, key), Some((sq, value)));
    bytes.push(0);
    assert_eq!(lookup(&bytes, key), None);
    assert_eq!(lookup(&bytes[..RECORD - 1], key), None);
}

#[test]
fn value_bounds() {
    let (key, sq, _) = asymmetric();
    for value in [i16::MIN, -1, 0, 1, i16::MAX] {
        let answer = lookup(&asset(&[(key, sq, value)]), key);
        assert_eq!(answer, Some((sq, i32::from(value))));
    }
}

/// A stuck board (pass) and a terminal board are never answered, even if
/// their key is stored.
#[test]
fn stuck_bypass() {
    let stuck = Board { me: 1 << 1, opp: 1 };
    assert_eq!(legal_moves(stuck.me, stuck.opp), 0);
    assert!(!is_terminal(stuck));
    let over = Board { me: 1, opp: 0 };
    assert!(is_terminal(over));
    for board in [stuck, over] {
        let bytes = asset(&[(canonical(board), 2, 100)]);
        for sym in Sym::ALL {
            assert_eq!(lookup(&bytes, image(board, sym)), None);
        }
    }
}

#[test]
fn play_off() {
    let config = SearchConfig::standard();
    assert!(!config.book);
    for board in [Board::START, asymmetric().0, deep()] {
        assert_eq!(played(board, &config), fresh(board, &config));
    }
}

#[test]
fn play_hit() {
    let (sq, value) = book_move(Board::START).expect("start is stored");
    let result = played(Board::START, &booked());
    assert_eq!((result.best, result.score), (sq, value));
    assert_eq!((result.depth, result.nodes), (0, 0));
    assert_eq!(result.status, SearchStatus::Complete);
    assert!(!result.solved);
}

#[test]
fn play_miss() {
    let board = deep();
    assert_eq!(
        played(board, &booked()),
        fresh(board, &SearchConfig::standard())
    );
}

#[test]
fn search_ignores_book() {
    for board in [Board::START, asymmetric().0, deep()] {
        assert_eq!(
            fresh(board, &booked()),
            fresh(board, &SearchConfig::standard())
        );
    }
}

/// One agent leaves the book with a search move, then is answered from the
/// book again: lookup is stateless.
#[test]
fn agent_reenters() {
    let mut agent = search_agent(booked(), SearchLimits::depth(3));
    let (key, _, _) = asymmetric();
    let stored = book_move(key).expect("stored").0;
    assert_eq!(agent.choose(key), Some(stored));
    let board = deep();
    assert_eq!(agent.choose(board), Some(fresh(board, &booked()).best));
    assert_eq!(agent.choose(key), Some(stored));
    assert_eq!(
        agent.choose(Board::START),
        Some(book_move(Board::START).unwrap().0)
    );
}

#[test]
fn agent_book_off() {
    let mut agent = search_agent(SearchConfig::standard(), SearchLimits::depth(3));
    let board = asymmetric().0;
    assert_eq!(
        agent.choose(board),
        Some(fresh(board, &SearchConfig::standard()).best)
    );
}

#[test]
fn asset_sorted() {
    assert_eq!(BOOK.len() % RECORD, 0);
    assert!(!BOOK.is_empty());
    let keys: Vec<u64> = BOOK
        .chunks(RECORD)
        .map(|r| u64::from_le_bytes(r[..8].try_into().unwrap()))
        .collect();
    assert!(
        keys.windows(2).all(|w| w[0] < w[1]),
        "unsorted or duplicate"
    );
}

#[test]
fn asset_manifest() {
    let mut rows: Vec<Vec<u8>> = manifest()
        .into_iter()
        .map(|(b, s, v)| record(b, s, v.clamp(i16::MIN.into(), i16::MAX.into()) as i16))
        .collect();
    rows.sort_by_key(|r| u64::from_le_bytes(r[..8].try_into().unwrap()));
    assert_eq!(BOOK, rows.concat().as_slice());
}

#[test]
fn manifest_legal() {
    for (board, sq, _) in manifest() {
        assert_eq!(board, canonical(board), "manifest board is not canonical");
        assert!(sq < 64);
        assert_ne!(
            legal_moves(board.me, board.opp) & (1 << sq),
            0,
            "illegal {sq}"
        );
    }
}
