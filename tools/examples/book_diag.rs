//! Book diagnostic analysis (`docs/design/opening-book-v2.md` § First
//! experiment), over `book_trace` journals.
//!
//! - `roots`: the 12 early diagnostic roots, by a fixed rule (below),
//!   followed by the first 12 historical book roots in file order.
//! - `diverge --book J --off R --version V --group G...`: each game's first
//!   disagreement between a book trace and the book-off reference with the
//!   same id.
//! - `audit FILE --nodes A,B`: searches both children of up to 16 first
//!   disagreements with fresh tables and ProbCut off.

use othello::board::Board;

/// A decision parsed from a `book_trace` event token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step {
    black: bool,
    /// `None` for a pass.
    square: Option<u8>,
    book: bool,
}

/// A game's first disagreement with its book-off reference.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Disagreement {
    id: String,
    /// Events before the disagreement.
    index: usize,
    board: Board,
    book_move: u8,
    off_move: u8,
    /// The book move came from the book, not a search.
    from_book: bool,
    /// The two moves are equivalent under a symmetry of the board.
    equivalent: bool,
}

/// The early roots: after 1 and 2 placements, every canonical position;
/// after 3 and 4, four each by a seeded shuffle (seed 171) of the
/// canonical positions in key order. Each as its least square sequence.
fn early_roots() -> Vec<Vec<u8>> {
    Vec::new()
}

/// Parses a `book_trace` events text into steps.
fn steps(_text: &str) -> Option<Vec<Step>> {
    None
}

/// The first disagreement between a book trace and its reference, both
/// from `opening`, or `None` if they agree throughout.
fn first_disagreement(
    _id: &str,
    _opening: &[u8],
    _book: &[Step],
    _off: &[Step],
) -> Option<Disagreement> {
    None
}

/// The mover's value of playing `square` from `board`, from a fresh search
/// of the child at `nodes` with ProbCut off: negated for a normal child,
/// kept for a child that must pass, exact for a terminal child.
fn child_value(_board: Board, _square: u8, _nodes: u64) -> (i32, Option<u8>) {
    (0, None)
}

/// Up to 16 disagreements, deduplicated by canonical board and move pair,
/// at most four per (version, group), in the given order.
fn select(_found: &[(String, String, Disagreement)]) -> Vec<(String, String, Disagreement)> {
    Vec::new()
}

/// `pair:game` records of a `book_trace` journal: id to events text.
fn trace_records(path: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let (_, records) = othello_tools::journal::read(std::path::Path::new(path))?;
    records
        .into_iter()
        .map(|r| {
            let text = r.line.split_once(" events ").map(|(_, t)| t.to_string());
            Ok((
                r.id.clone(),
                text.ok_or(format!("record {}: no events", r.id))?,
            ))
        })
        .collect()
}

fn square_name(square: u8) -> String {
    format!("{}{}", (b'a' + square % 8) as char, square / 8 + 1)
}

fn run(args: &[String]) -> Result<(), String> {
    let option = |flag: &str| {
        let index = args.iter().position(|arg| arg == flag)?;
        args.get(index + 1).cloned()
    };
    let usage = "usage: book_diag roots HISTORICAL | diverge --book J --off R --roots FILE --version V --group G | audit FILE --nodes A,B";
    match args.first().map(String::as_str) {
        Some("roots") => {
            let path = args.get(1).ok_or(usage)?;
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            for root in early_roots() {
                let names: Vec<String> = root.iter().map(|&s| square_name(s)).collect();
                println!("{}", names.join(" "));
            }
            for line in text.lines().filter(|l| !l.starts_with('#')).take(12) {
                println!("{line}");
            }
            Ok(())
        }
        Some("diverge") => {
            let (book, off) = (
                option("--book").ok_or(usage)?,
                option("--off").ok_or(usage)?,
            );
            let roots_path = option("--roots").ok_or(usage)?;
            let version = option("--version").ok_or(usage)?;
            let group = option("--group").ok_or(usage)?;
            let roots: Vec<Vec<u8>> = std::fs::read_to_string(&roots_path)
                .map_err(|e| format!("{roots_path}: {e}"))?
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                .map(|l| {
                    l.split_whitespace()
                        .filter(|t| *t != "start")
                        .map(|t| {
                            let b = t.as_bytes();
                            (b[1] - b'1') * 8 + (b[0] - b'a')
                        })
                        .collect()
                })
                .collect();
            let (book, off) = (trace_records(&book)?, trace_records(&off)?);
            for (id, text) in &book {
                let pair: usize = id
                    .split(':')
                    .next()
                    .and_then(|p| p.parse().ok())
                    .ok_or("bad id")?;
                let reference = off.get(id).ok_or(format!("{id}: no reference"))?;
                let (b, o) = (
                    steps(text).ok_or("bad trace")?,
                    steps(reference).ok_or("bad reference")?,
                );
                if let Some(d) = first_disagreement(id, &roots[pair], &b, &o) {
                    println!(
                        "{version}\t{group}\t{id}\t{}\t{:016x}:{:016x}\t{}\t{}\t{}\t{}",
                        d.index,
                        d.board.me,
                        d.board.opp,
                        square_name(d.book_move),
                        square_name(d.off_move),
                        if d.from_book { "book" } else { "search" },
                        if d.equivalent {
                            "equivalent"
                        } else {
                            "different"
                        }
                    );
                }
            }
            Ok(())
        }
        Some("audit") => {
            let path = args.get(1).ok_or(usage)?;
            let nodes: Vec<u64> = option("--nodes")
                .ok_or(usage)?
                .split(',')
                .map(|n| n.parse().map_err(|_| usage))
                .collect::<Result<_, _>>()?;
            if nodes.contains(&0) {
                return Err("node caps must be positive".into());
            }
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let mut found = Vec::new();
            for line in text.lines() {
                let f: Vec<&str> = line.split('\t').collect();
                let [
                    version,
                    group,
                    id,
                    index,
                    board,
                    book_move,
                    off_move,
                    source,
                    kind,
                ] = f[..]
                else {
                    return Err(format!("bad line: {line}"));
                };
                let (me, opp) = board.split_once(':').ok_or("bad board")?;
                let parse_square = |t: &str| {
                    let b = t.as_bytes();
                    (b[1] - b'1') * 8 + (b[0] - b'a')
                };
                found.push((
                    version.to_string(),
                    group.to_string(),
                    Disagreement {
                        id: id.into(),
                        index: index.parse().map_err(|_| "bad index")?,
                        board: Board {
                            me: u64::from_str_radix(me, 16).map_err(|_| "bad board")?,
                            opp: u64::from_str_radix(opp, 16).map_err(|_| "bad board")?,
                        },
                        book_move: parse_square(book_move),
                        off_move: parse_square(off_move),
                        from_book: source == "book",
                        equivalent: kind == "equivalent",
                    },
                ));
            }
            for (version, group, d) in select(&found) {
                for &n in &nodes {
                    let (book, book_reply) = child_value(d.board, d.book_move, n);
                    let (off, off_reply) = child_value(d.board, d.off_move, n);
                    let reply = |r: Option<u8>| r.map_or("-".into(), square_name);
                    println!(
                        "{version}\t{group}\t{}\t{n}\t{}\t{book}\t{}\t{}\t{off}\t{}",
                        d.id,
                        square_name(d.book_move),
                        reply(book_reply),
                        square_name(d.off_move),
                        reply(off_reply)
                    );
                }
            }
            Ok(())
        }
        _ => Err(usage.into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(message) = run(&args) {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use othello::board::{final_score, legal_moves, play};
    use othello::score::terminal_score;
    use othello_tools::sym::canonical;
    use std::collections::BTreeSet;

    #[test]
    fn early_roots_rule() {
        let roots = early_roots();
        let lengths: Vec<usize> = roots.iter().map(Vec::len).collect();
        assert_eq!(lengths, [1, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4]);
        let mut seen = BTreeSet::new();
        for root in &roots {
            let mut board = Board::START;
            for &square in root {
                assert_ne!(legal_moves(board.me, board.opp) & (1 << square), 0);
                board = play(board, square);
            }
            assert!(seen.insert(canonical(board)), "duplicate root {root:?}");
        }
        assert_eq!(roots[0], vec![19], "d3 is the least first move");
        assert_eq!(roots, early_roots(), "deterministic");
    }

    #[test]
    fn steps_parse() {
        let text = "b0000000000000001:4:b:19:0:c:0 w0000000000000002:3:s:18:5:p:900 b-";
        let parsed = steps(text).unwrap();
        assert_eq!(
            parsed,
            vec![
                Step {
                    black: true,
                    square: Some(19),
                    book: true
                },
                Step {
                    black: false,
                    square: Some(18),
                    book: false
                },
                Step {
                    black: true,
                    square: None,
                    book: false
                },
            ]
        );
        assert!(steps("x123").is_none());
    }

    fn mv(black: bool, square: u8, book: bool) -> Step {
        Step {
            black,
            square: Some(square),
            book,
        }
    }

    #[test]
    fn disagreement_found() {
        let book = [mv(true, 19, true), mv(false, 18, false), mv(true, 17, true)];
        let off = [
            mv(true, 19, false),
            mv(false, 18, false),
            mv(true, 26, false),
        ];
        assert!(first_disagreement("0:0", &[], &book[..2], &off[..2]).is_none());
        let d = first_disagreement("0:0", &[], &book, &off).unwrap();
        assert_eq!(
            (d.index, d.book_move, d.off_move, d.from_book),
            (2, 17, 26, true)
        );
        let mut board = Board::START;
        for square in [19, 18] {
            board = play(board, square);
        }
        assert_eq!(d.board, board);
    }

    /// From the start, d3 and c4 are images under a symmetry of the board.
    #[test]
    fn disagreement_equivalent() {
        let d =
            first_disagreement("0:0", &[], &[mv(true, 19, true)], &[mv(true, 26, false)]).unwrap();
        assert!(d.equivalent);
        let e = first_disagreement(
            "0:0",
            &[19],
            &[mv(false, 18, true)],
            &[mv(false, 20, false)],
        )
        .unwrap();
        assert!(!e.equivalent, "c3 and e3 after d3 are not symmetric");
    }

    /// A terminal child is scored exactly, from the mover's side.
    #[test]
    fn child_terminal() {
        // Black to move takes the last empty square and wins.
        let board = Board {
            me: !((1 << 63) | (1 << 62)),
            opp: 1 << 62,
        };
        let square = 63;
        assert_ne!(legal_moves(board.me, board.opp) & (1 << square), 0);
        let child = play(board, square);
        let (value, reply) = child_value(board, square, 1000);
        assert_eq!(value, -terminal_score(final_score(child)));
        assert_eq!(reply, None);
    }

    /// A normal child's value is the negated child search, with its reply.
    #[test]
    fn child_normal() {
        let (value, reply) = child_value(Board::START, 19, 20_000);
        let child = play(Board::START, 19);
        let config = othello::search::SearchConfig {
            probcut: None,
            ..othello::search::SearchConfig::standard()
        };
        let limits = othello::search::SearchLimits::new(64, othello::search::NO_DEADLINE, 20_000);
        let r = othello::search::search_tt(
            child,
            limits,
            &config,
            &mut othello::tt::Table::new(1 << 20),
        );
        assert_eq!((value, reply), (-r.score, Some(r.best)));
    }

    fn found(version: &str, group: &str, id: usize, square: u8) -> (String, String, Disagreement) {
        let board = play(Board::START, 19);
        let d = Disagreement {
            id: format!("{id}:0"),
            index: 1,
            board,
            book_move: square,
            off_move: 20,
            from_book: true,
            equivalent: false,
        };
        (version.into(), group.into(), d)
    }

    /// At most four per (version, group), sixteen in all, in the given
    /// order; a canonical board and move pair counts once overall.
    #[test]
    fn select_rule() {
        let groups = [
            ("r0", "early"),
            ("r0", "hist"),
            ("r3", "early"),
            ("r3", "hist"),
        ];
        let mut distinct = Vec::new();
        for (k, (v, g)) in groups.iter().enumerate() {
            for id in 0..6 {
                distinct.push(found(v, g, id, (k * 6 + id) as u8));
            }
        }
        let picked = select(&distinct);
        assert_eq!(picked.len(), 16);
        for (v, g) in groups {
            let mine: Vec<&String> = picked
                .iter()
                .filter(|p| p.0 == v && p.1 == g)
                .map(|p| &p.2.id)
                .collect();
            assert_eq!(
                mine,
                ["0:0", "1:0", "2:0", "3:0"],
                "{v} {g}: first four in order"
            );
        }
        let mut same = Vec::new();
        for (v, g) in groups {
            for id in 0..6 {
                same.push(found(v, g, id, 18));
            }
        }
        let once = select(&same);
        assert_eq!(once.len(), 1, "one board and move pair");
        assert_eq!((once[0].0.as_str(), once[0].2.id.as_str()), ("r0", "0:0"));
    }
}
