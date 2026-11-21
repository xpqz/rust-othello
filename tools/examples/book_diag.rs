//! Book diagnostic analysis (`docs/design/opening-book-v2.md` § First
//! experiment), over `book_trace` journals.
//!
//! - `roots HISTORICAL`: the 12 early diagnostic roots, by a fixed rule
//!   (`early_roots`), then the first 12 historical book roots in file order.
//! - `diverge --book J --off R --roots FILE --version V --group G`: each
//!   game's first change against the book-off reference, in numeric pair
//!   and colour order. Both journals must cover every pair of the roots
//!   once per colour, with the same node cap and roots identity.
//! - `audit FILE --nodes A,B --out JOURNAL`: searches both children of up
//!   to 16 first disagreements with fresh tables and ProbCut off, journalling
//!   each position so an interrupted audit resumes.
//!
//! A symmetry-equivalent first change ends the comparison of that game: it
//! is reported as `equivalent` and never takes an audit slot.
use othello::board::{Board, final_score, is_terminal, legal_moves, pass, play};
use othello::score::terminal_score;
use othello::search::{NO_DEADLINE, SearchConfig, SearchLimits, SearchStatus, search_tt};
use othello::tt::Table;
use othello_tools::journal::{Journal, Record};
use othello_tools::sym::{canonical, transform};
use std::collections::{BTreeMap, BTreeSet};

/// A decision parsed from a `book_trace` event token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step {
    black: bool,
    /// `None` for a pass.
    square: Option<u8>,
    book: bool,
}

/// A game's first change against its book-off reference.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Disagreement {
    pair: usize,
    game: u8,
    /// Events before the change.
    index: usize,
    board: Board,
    book_move: u8,
    off_move: u8,
    /// The book move came from the book, not a search.
    from_book: bool,
    /// The two moves are images under a symmetry that fixes the board.
    equivalent: bool,
}

/// A `book_trace` journal: its controls and each game's steps.
#[derive(Debug)]
struct Trace {
    book: String,
    nodes: u64,
    roots: u64,
    engine: String,
    games: BTreeMap<(usize, u8), Vec<Step>>,
}

/// One child's audit value, from the mover's side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Value {
    score: i32,
    /// The best reply found, if the child was searched.
    reply: Option<u8>,
    /// The child search's status; `None` for a terminal child.
    status: Option<SearchStatus>,
    /// The completed heuristic iteration's depth; 0 for a terminal child,
    /// an exact solve, a W/D/L partial or a fallback.
    depth: u32,
    /// Exact: a terminal child or a solved search.
    solved: bool,
}

type Found = (String, String, Disagreement);

fn moves_of(board: Board) -> impl Iterator<Item = u8> {
    let moves = legal_moves(board.me, board.opp);
    (0..64u8).filter(move |s| moves & (1 << s) != 0)
}

fn sym_square(square: u8, kind: u8) -> u8 {
    transform(
        Board {
            me: 1 << square,
            opp: 0,
        },
        kind,
    )
    .me
    .trailing_zeros() as u8
}

/// The early roots: after 1 and 2 placements, every canonical position;
/// after 3 and 4, four each by a seeded shuffle (seed 171) of the
/// canonical positions in key order. Each as its least square sequence.
fn early_roots() -> Vec<Vec<u8>> {
    // Depth-first in square order meets each position first by its least sequence.
    fn walk(board: Board, line: &mut Vec<u8>, levels: &mut [BTreeMap<(u64, u64), Vec<u8>>]) {
        if line.len() == levels.len() {
            return;
        }
        for square in moves_of(board) {
            line.push(square);
            let child = play(board, square);
            levels[line.len() - 1]
                .entry(canonical(child))
                .or_insert_with(|| line.clone());
            walk(child, line, levels);
            line.pop();
        }
    }
    let mut levels = vec![BTreeMap::new(); 4];
    walk(Board::START, &mut Vec::new(), &mut levels);
    let mut rng = othello::rng::Rng::new(171);
    let mut roots = Vec::new();
    for (depth, level) in levels.into_iter().enumerate() {
        let mut lines: Vec<Vec<u8>> = level.into_values().collect();
        if depth >= 2 {
            for i in (1..lines.len()).rev() {
                lines.swap(i, (rng.next_u64() % (i as u64 + 1)) as usize);
            }
            lines.truncate(4);
        }
        roots.extend(lines);
    }
    roots
}

/// A 64-bit FNV-1a digest, as `book_trace` writes into its header.
fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Parses a `book_trace` events text into steps.
fn steps(text: &str) -> Option<Vec<Step>> {
    text.split_whitespace()
        .map(|token| {
            let black = match token.as_bytes().first()? {
                b'b' => true,
                b'w' => false,
                _ => return None,
            };
            if &token[1..] == "-" {
                return Some(Step {
                    black,
                    square: None,
                    book: false,
                });
            }
            let f: Vec<&str> = token[1..].split(':').collect();
            let [key, legal, source, square, depth, status, nodes] = f[..] else {
                return None;
            };
            let valid = u64::from_str_radix(key, 16).is_ok()
                && legal.parse::<u32>().is_ok()
                && depth.parse::<u32>().is_ok()
                && ["c", "p", "f"].contains(&status)
                && nodes.parse::<u64>().is_ok();
            let square = square.parse::<u8>().ok().filter(|&s| s < 64 && valid)?;
            let book = match source {
                "b" => true,
                "s" => false,
                _ => return None,
            };
            Some(Step {
                black,
                square: Some(square),
                book,
            })
        })
        .collect()
}

/// Reads a `book_trace` journal's header and records; every id must be a
/// canonical `pair:game` and appear once.
fn parse_trace(header: &str, records: &[Record]) -> Result<Trace, String> {
    let f: Vec<&str> = header.split(' ').collect();
    let bad = || format!("not a book_trace v1 header: {header}");
    let [
        "header",
        "book_trace",
        "v1",
        "book",
        book,
        "asset",
        _,
        "nodes",
        nodes,
        "roots",
        roots,
        "engine",
        engine,
    ] = f[..]
    else {
        return Err(bad());
    };
    let mut games = BTreeMap::new();
    for record in records {
        let bad_record = || format!("bad record {}: {}", record.id, record.line);
        let (pair, game) = record.id.split_once(':').ok_or_else(bad_record)?;
        let key: (usize, u8) = (
            pair.parse().map_err(|_| bad_record())?,
            game.parse().map_err(|_| bad_record())?,
        );
        if record.id != format!("{}:{}", key.0, key.1) || key.1 > 1 {
            return Err(format!("non-canonical id {}", record.id));
        }
        let (score, events) = record
            .line
            .strip_prefix("score ")
            .and_then(|r| r.split_once(" events"))
            .ok_or_else(bad_record)?;
        score.parse::<i32>().map_err(|_| bad_record())?;
        let parsed = steps(events).ok_or_else(bad_record)?;
        if games.insert(key, parsed).is_some() {
            return Err(format!("duplicate id {}", record.id));
        }
    }
    Ok(Trace {
        book: book.into(),
        nodes: nodes.parse().map_err(|_| bad())?,
        roots: u64::from_str_radix(roots, 16).map_err(|_| bad())?,
        engine: engine.into(),
        games,
    })
}

/// The first change of a book trace against its reference, both from
/// `opening`, or `None` if they agree throughout. Replay stops at an
/// illegal move, which a checked `book_trace` journal never holds.
fn first_disagreement(
    pair: usize,
    game: u8,
    opening: &[u8],
    book: &[Step],
    off: &[Step],
) -> Option<Disagreement> {
    let mut board = Board::START;
    for &square in opening {
        board = play(board, square);
    }
    for (index, (b, o)) in book.iter().zip(off).enumerate() {
        match (b.square, o.square) {
            (Some(x), Some(y)) if x != y => {
                let equivalent =
                    (1..8).any(|k| transform(board, k) == board && sym_square(x, k) == y);
                return Some(Disagreement {
                    pair,
                    game,
                    index,
                    board,
                    book_move: x,
                    off_move: y,
                    from_book: b.book,
                    equivalent,
                });
            }
            (Some(x), Some(_)) if legal_moves(board.me, board.opp) & (1 << x) != 0 => {
                board = play(board, x)
            }
            (None, None) => board = pass(board),
            _ => return None,
        }
    }
    None
}

/// Every game's first change, in numeric pair and colour order, after
/// checking that the reference is book-off, that both traces have the same
/// node cap, engine and the roots identity of `roots_text`, and that each covers
/// every pair of the roots once per colour.
fn compare(
    book: &Trace,
    off: &Trace,
    roots: &[Vec<u8>],
    roots_text: &str,
) -> Result<Vec<Disagreement>, String> {
    if off.book != "off" {
        return Err(format!("the reference is book {}, not off", off.book));
    }
    if (book.nodes, &book.engine) != (off.nodes, &off.engine) {
        return Err("the traces differ in node cap or engine".into());
    }
    let identity = digest(roots_text.as_bytes());
    if book.roots != identity || off.roots != identity {
        return Err("a trace was not made from these roots".into());
    }
    let ids: Vec<(usize, u8)> = (0..roots.len()).flat_map(|p| [(p, 0), (p, 1)]).collect();
    for trace in [book, off] {
        if !trace.games.keys().copied().eq(ids.iter().copied()) {
            return Err(format!(
                "book {} does not hold every game of the roots once",
                trace.book
            ));
        }
    }
    Ok(ids
        .iter()
        .filter_map(|&(p, g)| {
            first_disagreement(p, g, &roots[p], &book.games[&(p, g)], &off.games[&(p, g)])
        })
        .collect())
}

/// The mover's value of playing `square` from `board`: exact for a
/// terminal child; otherwise a fresh search at `nodes` with ProbCut off of
/// the position the next mover faces, negated when the opponent moves next
/// and kept when the opponent must pass.
fn child_value(board: Board, square: u8, nodes: u64) -> Value {
    let child = play(board, square);
    if is_terminal(child) {
        let score = -terminal_score(final_score(child));
        return Value {
            score,
            reply: None,
            status: None,
            depth: 0,
            solved: true,
        };
    }
    let passes = legal_moves(child.me, child.opp) == 0;
    let config = SearchConfig {
        probcut: None,
        ..SearchConfig::standard()
    };
    let limits = SearchLimits::new(64, NO_DEADLINE, nodes);
    let next = if passes { pass(child) } else { child };
    let r = search_tt(next, limits, &config, &mut Table::new(1 << 20));
    Value {
        score: if passes { r.score } else { -r.score },
        reply: Some(r.best),
        status: Some(r.status),
        depth: r.depth,
        solved: r.solved,
    }
}

/// Fit for a reversal decision: exact, or a completed heuristic iteration
/// (positive depth). A W/D/L partial or a fallback is not.
fn eligible(value: Value) -> bool {
    value.solved || (value.depth > 0 && value.status != Some(SearchStatus::Fallback))
}

/// Whether the preferred child, book or off, flips between the low and
/// high node caps: `None` unless all four values are eligible. A tie at
/// either cap is no reversal.
fn reversal(low: (Value, Value), high: (Value, Value)) -> Option<bool> {
    if ![low.0, low.1, high.0, high.1].into_iter().all(eligible) {
        return None;
    }
    let (a, b) = (
        low.0.score.cmp(&low.1.score),
        high.0.score.cmp(&high.1.score),
    );
    Some(a.is_ne() && b == a.reverse())
}

/// Up to 16 non-equivalent disagreements, at most four per (version,
/// group), in the given order; a board and move pair counts once, up to a
/// symmetry applied to all three.
fn select(found: &[Found]) -> Vec<Found> {
    let mut seen = BTreeSet::new();
    let mut quota: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    let mut picked = Vec::new();
    for entry in found.iter().filter(|e| !e.2.equivalent) {
        let d = &entry.2;
        let case = (0..8)
            .map(|k| {
                let b = transform(d.board, k);
                (
                    b.me,
                    b.opp,
                    sym_square(d.book_move, k),
                    sym_square(d.off_move, k),
                )
            })
            .min();
        let used = quota.entry((&entry.0, &entry.1)).or_default();
        if picked.len() < 16 && *used < 4 && seen.insert(case) {
            *used += 1;
            picked.push(entry.clone());
        }
    }
    picked
}

fn value_text(v: Value) -> String {
    let status = match v.status {
        None => 't',
        Some(SearchStatus::Complete) => 'c',
        Some(SearchStatus::Partial) => 'p',
        Some(SearchStatus::Fallback) => 'f',
    };
    let reply = v.reply.map_or("-".into(), square_name);
    format!(
        "{}:{reply}:{status}:{}:{}",
        v.score,
        v.depth,
        if v.solved { "exact" } else { "heur" }
    )
}

/// Audits the selection of `found` at each node cap into a resumable
/// journal at `out`, one record per position: per cap, the book and off
/// children's values, then whether the first and last caps reverse.
fn audit(found: &[Found], nodes: &[u64], out: &str) -> Result<(), String> {
    let picked = select(found);
    let lines: Vec<String> = picked
        .iter()
        .map(|(v, g, d)| disagreement_line(v, g, d))
        .collect();
    let caps: Vec<String> = nodes.iter().map(u64::to_string).collect();
    let header = format!(
        "header book_diag audit v1 nodes {} found {:016x}",
        caps.join(","),
        digest(lines.join("\n").as_bytes())
    );
    let (mut journal, _) = Journal::open(std::path::Path::new(out), &header)?;
    let ids: Vec<String> = picked
        .iter()
        .map(|(v, g, d)| format!("{v}:{g}:{}:{}", d.pair, d.game))
        .collect();
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for record in journal.records() {
        *count.entry(record.id.as_str()).or_default() += 1;
    }
    // Keep exactly one record per selected id; anything else is redone.
    let stale: BTreeSet<String> = count
        .iter()
        .filter(|&(id, &n)| n > 1 || !ids.iter().any(|i| i == id))
        .map(|(id, _)| id.to_string())
        .collect();
    let done: BTreeSet<String> = count
        .keys()
        .map(|id| id.to_string())
        .filter(|id| !stale.contains(id))
        .collect();
    if !stale.is_empty() {
        journal.discard(&stale)?;
    }
    for (id, (_, _, d)) in ids
        .iter()
        .zip(&picked)
        .filter(|(id, _)| !done.contains(*id))
    {
        let values: Vec<(Value, Value)> = nodes
            .iter()
            .map(|&n| {
                (
                    child_value(d.board, d.book_move, n),
                    child_value(d.board, d.off_move, n),
                )
            })
            .collect();
        let mut fields: Vec<String> = nodes
            .iter()
            .zip(&values)
            .map(|(n, (b, o))| format!("{n} book {} off {}", value_text(*b), value_text(*o)))
            .collect();
        let verdict = match reversal(values[0], values[values.len() - 1]) {
            Some(true) => "yes",
            Some(false) => "no",
            None => "ineligible",
        };
        fields.push(format!("reversal {verdict}"));
        journal.append(id, &fields.join(" | "))?;
    }
    Ok(())
}

fn square_name(square: u8) -> String {
    format!("{}{}", (b'a' + square % 8) as char, square / 8 + 1)
}

fn parse_square(text: &str) -> Option<u8> {
    let b = text.as_bytes();
    let valid = b.len() == 2 && (b'a'..=b'h').contains(&b[0]) && (b'1'..=b'8').contains(&b[1]);
    valid.then(|| (b[1] - b'1') * 8 + (b[0] - b'a'))
}

/// A roots file as `book_trace` reads it: move sequences or `start`.
fn load_roots(text: &str) -> Result<Vec<Vec<u8>>, String> {
    let lines = text.lines().map(str::trim);
    lines
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let tokens = l.split_whitespace().filter(|t| *t != "start");
            tokens
                .map(|t| parse_square(t).ok_or(format!("bad root {l}")))
                .collect()
        })
        .collect()
}

fn read_trace(path: &str) -> Result<Trace, String> {
    let (header, records) = othello_tools::journal::read(std::path::Path::new(path))?;
    parse_trace(&header, &records)
}

/// A disagreements line, as `diverge` writes and `audit` reads.
fn disagreement_line(version: &str, group: &str, d: &Disagreement) -> String {
    format!(
        "{version}\t{group}\t{}:{}\t{}\t{:016x}:{:016x}\t{}\t{}\t{}\t{}",
        d.pair,
        d.game,
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
    )
}

fn parse_disagreement(line: &str) -> Option<(String, String, Disagreement)> {
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
        return None;
    };
    let (pair, game) = id.split_once(':')?;
    let (me, opp) = board.split_once(':')?;
    let d = Disagreement {
        pair: pair.parse().ok()?,
        game: game.parse().ok()?,
        index: index.parse().ok()?,
        board: Board {
            me: u64::from_str_radix(me, 16).ok()?,
            opp: u64::from_str_radix(opp, 16).ok()?,
        },
        book_move: parse_square(book_move)?,
        off_move: parse_square(off_move)?,
        from_book: source == "book",
        equivalent: kind == "equivalent",
    };
    Some((version.into(), group.into(), d))
}

fn run(args: &[String]) -> Result<(), String> {
    let option = |flag: &str| {
        let index = args.iter().position(|arg| arg == flag)?;
        args.get(index + 1).cloned()
    };
    let usage = "usage: book_diag roots HISTORICAL | diverge --book J --off R --roots FILE --version V --group G | audit FILE --nodes A,B --out JOURNAL";
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
            let roots_path = option("--roots").ok_or(usage)?;
            let roots_text =
                std::fs::read_to_string(&roots_path).map_err(|e| format!("{roots_path}: {e}"))?;
            let roots = load_roots(&roots_text)?;
            let book = read_trace(&option("--book").ok_or(usage)?)?;
            let off = read_trace(&option("--off").ok_or(usage)?)?;
            let (version, group) = (
                option("--version").ok_or(usage)?,
                option("--group").ok_or(usage)?,
            );
            for d in compare(&book, &off, &roots, &roots_text)? {
                println!("{}", disagreement_line(&version, &group, &d));
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
            if nodes.is_empty() || nodes.contains(&0) {
                return Err("node caps must be positive".into());
            }
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let found = text
                .lines()
                .map(|l| parse_disagreement(l).ok_or(format!("bad line: {l}")))
                .collect::<Result<Vec<_>, _>>()?;
            audit(&found, &nodes, &option("--out").ok_or(usage)?)
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
        assert!(first_disagreement(0, 0, &[], &book[..2], &off[..2]).is_none());
        let d = first_disagreement(4, 1, &[], &book, &off).unwrap();
        assert_eq!((d.pair, d.game, d.index), (4, 1, 2));
        assert_eq!(
            (d.book_move, d.off_move, d.from_book, d.equivalent),
            (17, 26, true, false)
        );
        assert_eq!(d.board, play(play(Board::START, 19), 18));
    }

    /// An equivalent first change ends the comparison there: later
    /// differences are not reported.
    #[test]
    fn disagreement_equivalent() {
        let book = [mv(true, 19, true), mv(false, 18, false)];
        let off = [mv(true, 26, false), mv(false, 44, false)];
        let d = first_disagreement(0, 0, &[], &book, &off).unwrap();
        assert_eq!((d.index, d.equivalent), (0, true));
        let e = first_disagreement(0, 0, &[19], &[mv(false, 18, true)], &[mv(false, 20, false)])
            .unwrap();
        assert!(!e.equivalent, "c3 and e3 after d3 are not symmetric");
    }

    fn found(
        version: &str,
        group: &str,
        pair: usize,
        board: Board,
        moves: (u8, u8),
    ) -> (String, String, Disagreement) {
        let d = Disagreement {
            pair,
            game: 0,
            index: 1,
            board,
            book_move: moves.0,
            off_move: moves.1,
            from_book: true,
            equivalent: false,
        };
        (version.into(), group.into(), d)
    }

    /// At most four per (version, group), sixteen in all, in the given
    /// order; equivalents never take a slot; a board and move pair counts
    /// once, also when transformed by a symmetry.
    #[test]
    fn select_rule() {
        let groups = [
            ("r0", "early"),
            ("r0", "hist"),
            ("r3", "early"),
            ("r3", "hist"),
        ];
        let board = play(Board::START, 19);
        let legal: Vec<u8> = (0..64)
            .filter(|s| legal_moves(board.me, board.opp) & (1 << s) != 0)
            .collect();
        let mut distinct = Vec::new();
        for (k, (v, g)) in groups.iter().enumerate() {
            for pair in 0..6 {
                let child = play(board, legal[(k * 6 + pair) % legal.len()]);
                let moves: Vec<u8> = (0..64)
                    .filter(|s| legal_moves(child.me, child.opp) & (1 << s) != 0)
                    .collect();
                distinct.push(found(v, g, pair, child, (moves[0], moves[moves.len() - 1])));
            }
        }
        let mut with_equivalent = vec![{
            let mut e = found("r0", "early", 99, board, (18, 20));
            e.2.equivalent = true;
            e
        }];
        with_equivalent.extend(distinct);
        let picked = select(&with_equivalent);
        assert!(!picked.is_empty());
        assert!(
            picked.iter().all(|p| !p.2.equivalent),
            "equivalents take no slot"
        );
        assert!(picked.len() <= 16);
        for (v, g) in groups {
            assert!(picked.iter().filter(|p| p.0 == v && p.1 == g).count() <= 4);
        }
        // The same case under a symmetry: counted once.
        let kind = 3;
        let original = found("r0", "early", 0, board, (18, 20));
        let image = found(
            "r3",
            "hist",
            1,
            transform(board, kind),
            (sym_square(18, kind), sym_square(20, kind)),
        );
        let once = select(&[original, image]);
        assert_eq!(once.len(), 1);
        assert_eq!((once[0].0.as_str(), once[0].2.pair), ("r0", 0));
    }

    /// A terminal child is scored exactly from the mover's side.
    #[test]
    fn child_terminal() {
        let board = Board {
            me: !((1 << 63) | (1 << 62)),
            opp: 1 << 62,
        };
        assert_ne!(legal_moves(board.me, board.opp) & (1 << 63), 0);
        let child = play(board, 63);
        let value = child_value(board, 63, 1000);
        assert_eq!(value.score, -terminal_score(final_score(child)));
        assert_eq!(
            (value.reply, value.status, value.depth, value.solved),
            (None, None, 0, true)
        );
    }

    fn fresh(board: Board, nodes: u64) -> othello::search::SearchResult {
        let config = SearchConfig {
            probcut: None,
            ..SearchConfig::standard()
        };
        let limits = SearchLimits::new(64, NO_DEADLINE, nodes);
        search_tt(board, limits, &config, &mut Table::new(1 << 20))
    }

    /// A normal child: the negated search of the opponent's position, with
    /// its reply, status and solved flag.
    #[test]
    fn child_normal() {
        let value = child_value(Board::START, 19, 20_000);
        let r = fresh(play(Board::START, 19), 20_000);
        assert_eq!(
            value,
            Value {
                score: -r.score,
                reply: Some(r.best),
                status: Some(r.status),
                depth: r.depth,
                solved: r.solved
            }
        );
    }

    /// A child where the opponent must pass: the mover moves again, so the
    /// value is the search of the passed position, not negated.
    #[test]
    fn child_forced_pass() {
        let found = (0..20_000u64).find_map(|seed| {
            let mut rng = othello::rng::Rng::new(seed);
            let mut board = Board::START;
            while !othello::board::is_terminal(board) {
                let moves = legal_moves(board.me, board.opp);
                for square in (0..64).filter(|s| moves & (1 << s) != 0) {
                    let child = play(board, square);
                    if legal_moves(child.me, child.opp) == 0
                        && legal_moves(child.opp, child.me) != 0
                    {
                        return Some((board, square));
                    }
                }
                board = if moves == 0 {
                    pass(board)
                } else {
                    let list: Vec<u8> = (0..64).filter(|s| moves & (1 << s) != 0).collect();
                    play(board, list[(rng.next_u64() % list.len() as u64) as usize])
                };
            }
            None
        });
        let (board, square) = found.expect("a move after which the opponent must pass");
        let passed = pass(play(board, square));
        let value = child_value(board, square, 20_000);
        let r = fresh(passed, 20_000);
        assert_eq!(value.score, r.score, "the mover moves again: not negated");
        assert_eq!(
            value.reply,
            Some(r.best),
            "the reply is the mover's next move"
        );
        assert_eq!(
            (value.status, value.depth, value.solved),
            (Some(r.status), r.depth, r.solved)
        );
    }

    fn value(score: i32, status: Option<SearchStatus>, depth: u32, solved: bool) -> Value {
        Value {
            score,
            reply: None,
            status,
            depth,
            solved,
        }
    }

    /// Reversals use completed heuristic iterations, which a node-capped
    /// search reports as `Partial` at positive depth, and exact values; a
    /// W/D/L partial or a fallback makes the decision unavailable.
    #[test]
    fn reversal_rule() {
        use SearchStatus::{Complete, Fallback, Partial};
        let p = |score, depth| value(score, Some(Partial), depth, false);
        let (wdl, fallback) = (
            value(64, Some(Partial), 0, false),
            value(5, Some(Fallback), 0, false),
        );
        let (exact, terminal) = (
            value(-128, Some(Complete), 0, true),
            value(256, None, 0, true),
        );
        assert!(eligible(p(30, 9)) && eligible(exact) && eligible(terminal));
        assert!(!eligible(wdl) && !eligible(fallback));
        assert_eq!(
            reversal((p(30, 9), p(10, 9)), (p(5, 11), p(20, 11))),
            Some(true)
        );
        assert_eq!(
            reversal((p(30, 9), p(10, 9)), (p(40, 11), p(20, 11))),
            Some(false)
        );
        assert_eq!(
            reversal((p(30, 9), p(10, 9)), (p(20, 11), p(20, 11))),
            Some(false),
            "a tie"
        );
        assert_eq!(
            reversal((p(30, 9), p(10, 9)), (exact, terminal)),
            Some(true)
        );
        assert_eq!(reversal((p(30, 9), wdl), (p(5, 11), p(20, 11))), None);
        assert_eq!(reversal((p(30, 9), p(10, 9)), (fallback, p(20, 11))), None);
    }

    fn header(book: &str, nodes: u64, roots: u64) -> String {
        format!(
            "header book_trace v1 book {book} asset 0 nodes {nodes} roots {roots:016x} engine x"
        )
    }

    /// One game record from `d3`: white plays `square`.
    fn record(id: &str, square: u8) -> Record {
        let board = play(Board::START, 19);
        let key = othello::tt::zobrist(board);
        let legal = legal_moves(board.me, board.opp).count_ones();
        Record {
            id: id.into(),
            line: format!("score 0 events w{key:016x}:{legal}:s:{square}:1:c:10"),
        }
    }

    fn trace(book: &str, nodes: u64, roots: &str, records: Vec<Record>) -> Result<Trace, String> {
        parse_trace(&header(book, nodes, digest(roots.as_bytes())), &records)
    }

    /// The comparison is bound to the same roots and controls, needs every
    /// game exactly once, and orders results by numeric pair.
    #[test]
    fn compare_checks() {
        let roots_text = "d3\n".repeat(11);
        let roots = load_roots(&roots_text).unwrap();
        let full = |differ: &[usize]| -> Vec<Record> {
            let differ = differ.to_vec();
            (0..11)
                .flat_map(|p| {
                    let differ = differ.clone();
                    (0..2).map(move |g| {
                        record(
                            &format!("{p}:{g}"),
                            if g == 0 && differ.contains(&p) {
                                20
                            } else {
                                18
                            },
                        )
                    })
                })
                .collect()
        };
        let off = trace("off", 1000, &roots_text, full(&[])).unwrap();
        let book = trace("embedded", 1000, &roots_text, full(&[10, 2])).unwrap();
        let found = compare(&book, &off, &roots, &roots_text).unwrap();
        let order: Vec<(usize, u8)> = found.iter().map(|d| (d.pair, d.game)).collect();
        assert_eq!(order, [(2, 0), (10, 0)], "numeric pair order");
        let other_nodes = trace("embedded", 2000, &roots_text, full(&[])).unwrap();
        assert!(
            compare(&other_nodes, &off, &roots, &roots_text).is_err(),
            "node caps differ"
        );
        let other_engine = parse_trace(
            &header("embedded", 1000, digest(roots_text.as_bytes()))
                .replace("engine x", "engine y"),
            &full(&[]),
        )
        .unwrap();
        assert!(
            compare(&other_engine, &off, &roots, &roots_text).is_err(),
            "engines differ"
        );
        let other_roots = trace("embedded", 1000, "d3\n", full(&[])).unwrap();
        assert!(
            compare(&other_roots, &off, &roots, &roots_text).is_err(),
            "roots identity differs"
        );
        let not_off = trace("embedded", 1000, &roots_text, full(&[])).unwrap();
        assert!(
            compare(&book, &not_off, &roots, &roots_text).is_err(),
            "reference must be book-off"
        );
        let mut short = full(&[]);
        short.pop();
        let missing = trace("embedded", 1000, &roots_text, short).unwrap();
        assert!(
            compare(&missing, &off, &roots, &roots_text).is_err(),
            "a missing game"
        );
        let mut duplicated = full(&[]);
        duplicated.push(record("3:0", 18));
        assert!(
            trace("embedded", 1000, &roots_text, duplicated).is_err(),
            "a duplicate id"
        );
        let mut alias = full(&[]);
        alias[0] = record("00:0", 18);
        assert!(
            trace("embedded", 1000, &roots_text, alias).is_err(),
            "a non-canonical id"
        );
    }

    /// An interrupted audit resumes: completed positions are kept, and the
    /// result equals an uninterrupted audit.
    #[test]
    fn audit_resumes() {
        let board = play(Board::START, 19);
        let list = vec![
            found("r0", "early", 0, board, (18, 20)),
            found("r0", "early", 1, play(board, 18), (17, 26)),
        ];
        let dir = std::env::temp_dir();
        let whole = dir.join(format!("book_diag_{}_whole", std::process::id()));
        let cut = dir.join(format!("book_diag_{}_cut", std::process::id()));
        for path in [&whole, &cut] {
            let _ = std::fs::remove_file(path);
        }
        audit(&list, &[1000, 2000], whole.to_str().unwrap()).unwrap();
        let (head, all) = othello_tools::journal::read(&whole).unwrap();
        assert_eq!(all.len(), 2, "one record per position");
        std::fs::write(&cut, format!("{head}\n{}\t{}\n", all[0].id, all[0].line)).unwrap();
        audit(&list, &[1000, 2000], cut.to_str().unwrap()).unwrap();
        assert_eq!(othello_tools::journal::read(&cut).unwrap().1, all);
        assert!(
            audit(&list, &[1000, 3000], cut.to_str().unwrap()).is_err(),
            "other caps: header mismatch"
        );
        for path in [&whole, &cut] {
            let _ = std::fs::remove_file(path);
        }
    }
}
