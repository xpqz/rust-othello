use std::collections::{BTreeMap, HashSet};
use std::fs;

use othello::board::{Board, legal_moves, play};

use crate::sym::canonical;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    pub squares: Vec<u8>,
    pub key: (u64, u64),
}

impl Opening {
    pub fn new(squares: Vec<u8>) -> Result<Self, String> {
        if !(4..=8).contains(&squares.len()) {
            return Err("opening length must be 4-8".into());
        }
        let mut board = Board::START;
        let mut key = None;
        for (ply, &square) in squares.iter().enumerate() {
            let moves = legal_moves(board.me, board.opp);
            if square >= 64 || moves & (1u64 << square) == 0 {
                return Err(format!("illegal square {}", square_name(square)));
            }
            board = play(board, square);
            if ply == 3 {
                key = Some(canonical(board));
            }
        }
        Ok(Self {
            squares,
            key: key.unwrap(),
        })
    }

    pub fn line(&self) -> String {
        self.squares
            .iter()
            .map(|&square| square_name(square))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub fn parse(text: &str) -> Result<Vec<Opening>, String> {
    let mut openings = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let squares = line
            .split_whitespace()
            .map(parse_square)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("line {}: {error}", index + 1))?;
        openings
            .push(Opening::new(squares).map_err(|error| format!("line {}: {error}", index + 1))?);
    }
    Ok(openings)
}

pub fn load(path: &str) -> Result<Vec<Opening>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    parse(&text)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetStats {
    pub openings: usize,
    pub duplicates: usize,
    pub lengths: Vec<(usize, usize)>,
}

pub fn stats(openings: &[Opening]) -> SetStats {
    let mut seen = HashSet::new();
    let mut duplicates = 0;
    let mut lengths = BTreeMap::new();
    for opening in openings {
        if !seen.insert(opening.squares.clone()) {
            duplicates += 1;
        }
        *lengths.entry(opening.squares.len()).or_insert(0) += 1;
    }
    SetStats {
        openings: openings.len(),
        duplicates,
        lengths: lengths.into_iter().collect(),
    }
}

pub fn check_splits(sets: &[&[Opening]; 3]) -> Result<(), String> {
    let mut seen = BTreeMap::new();
    for (index, set) in sets.iter().enumerate() {
        for opening in *set {
            if let Some(&first) = seen.get(&opening.key) {
                if first != index {
                    return Err(format!(
                        "group key overlap across splits: {}",
                        opening.line()
                    ));
                }
            } else {
                seen.insert(opening.key, index);
            }
        }
    }
    Ok(())
}

pub fn square_name(square: u8) -> String {
    let file = (b'a' + square % 8) as char;
    let rank = (b'1' + square / 8) as char;
    format!("{file}{rank}")
}

fn parse_square(text: &str) -> Result<u8, String> {
    let bytes = text.as_bytes();
    if bytes.len() != 2 {
        return Err(format!("invalid square {text}"));
    }
    let file = bytes[0]
        .checked_sub(b'a')
        .filter(|file| *file <= 7)
        .ok_or(format!("invalid square {text}"))?;
    let rank = bytes[1]
        .checked_sub(b'1')
        .filter(|rank| *rank <= 7)
        .ok_or(format!("invalid square {text}"))?;
    Ok(rank * 8 + file)
}
