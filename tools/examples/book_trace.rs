//! Book diagnostic traces (`docs/design/opening-book-v2.md` § First
//! experiment): node-capped games with the book off, the embedded book, or
//! another book asset, recording every engine decision.
//!
//! A `Tracer` plays exactly as the production search agent would with the
//! same configuration, and logs for each decision the board key, the number
//! of legal moves, the source (book or search), the move, and the search's
//! depth, status and reported nodes. Passes call no agent and log nothing.

use othello::agents::Agent;
use othello::board::Board;
use othello::search::{SearchConfig, SearchLimits, SearchStatus};
use othello::tt::Table;

/// Which book a tracer consults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Book<'a> {
    Off,
    /// The asset embedded in the engine, through `book::play`.
    Embedded,
    /// Another asset's bytes, through `book::lookup`.
    Asset(&'a [u8]),
}

// RED scaffolding: constructed by `Tracer::choose`, removed in GREEN.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Book,
    Search,
}

/// One engine decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Decision {
    /// `tt::zobrist` of the board as the mover sees it.
    key: u64,
    legal: u32,
    source: Source,
    square: u8,
    depth: u32,
    status: SearchStatus,
    /// Nodes the search reports; an aborted iteration's nodes may be missing.
    nodes: u64,
}

/// A search agent with an optional book that logs its decisions.
struct Tracer<'a> {
    config: SearchConfig,
    limits: SearchLimits,
    table: Table,
    book: Book<'a>,
    log: Vec<Decision>,
}

fn tracer(book: Book<'_>, nodes: u64) -> Tracer<'_> {
    Tracer {
        config: SearchConfig::standard(),
        limits: SearchLimits::new(64, othello::search::NO_DEADLINE, nodes),
        table: Table::new(othello::tt::MAX_ENTRIES),
        book,
        log: Vec::new(),
    }
}

impl Agent for Tracer<'_> {
    fn choose(&mut self, _board: Board) -> Option<u8> {
        let _ = (
            &self.config,
            &self.limits,
            &mut self.table,
            self.book,
            &mut self.log,
        );
        None
    }

    fn reset(&mut self) {
        self.table.clear();
    }
}

impl Decision {
    /// `key:legal:source:square:depth:status:nodes`, source `b` or `s`,
    /// status `c`, `p` or `f`.
    fn line(&self) -> String {
        String::new()
    }

    // RED scaffolding: used by the resume check, removed in GREEN.
    #[allow(dead_code)]
    fn parse(_text: &str) -> Option<Decision> {
        None
    }
}

/// The book side of a run: `off`, `embedded`, or a path to an asset.
fn book_of<'a>(name: &str, bytes: &'a [u8]) -> Book<'a> {
    match name {
        "off" => Book::Off,
        "embedded" => Book::Embedded,
        _ => Book::Asset(bytes),
    }
}

/// One game's journal text: colours, score, then each decision with the
/// side that made it (`b` black, `w` white), in play order.
fn game_line(black: &Tracer, white: &Tracer, score: i32, moves: &[(char, Decision)]) -> String {
    let _ = (black.book, white.book);
    let decisions: Vec<String> = moves
        .iter()
        .map(|(side, d)| format!("{side}{}", d.line()))
        .collect();
    format!("score {score} decisions {}", decisions.join(" "))
}

/// Plays one game from `opening`, logging both sides' decisions in order.
fn traced_game(
    opening: &[u8],
    black: &mut Tracer,
    white: &mut Tracer,
) -> Result<(i32, Vec<(char, Decision)>), String> {
    let game = othello_tools::harness::play_game(opening, black, white)?;
    let mut moves: Vec<(char, Decision)> = black.log.iter().map(|d| ('b', *d)).collect();
    moves.extend(white.log.iter().map(|d| ('w', *d)));
    Ok((game.score, moves))
}

/// `book_trace --roots FILE --book off|embedded|PATH --nodes N --out JOURNAL
/// [--pair-range A..B]`: for each root, the book side plays black then
/// white against book-off, with fresh tracers for every game.
fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let option = |flag: &str| {
        let index = args.iter().position(|arg| arg == flag)?;
        args.get(index + 1).cloned()
    };
    let usage = "usage: book_trace --roots FILE --book off|embedded|PATH --nodes N --out JOURNAL [--pair-range A..B]";
    let roots = othello_tools::positions::load(&option("--roots").ok_or(usage)?)?;
    let name = option("--book").ok_or(usage)?;
    let bytes = match name.as_str() {
        "off" | "embedded" => Vec::new(),
        path => std::fs::read(path).map_err(|e| format!("{path}: {e}"))?,
    };
    let book = book_of(&name, &bytes);
    let nodes: u64 = option("--nodes")
        .and_then(|n| n.parse().ok())
        .ok_or(usage)?;
    let out = option("--out").ok_or(usage)?;
    let (first, past) = match option("--pair-range") {
        Some(range) => {
            let (a, b) = range.split_once("..").ok_or(usage)?;
            (a.parse().map_err(|_| usage)?, b.parse().map_err(|_| usage)?)
        }
        None => (0, roots.len()),
    };
    let digest = othello_tools::provenance::file_hash(&option("--roots").unwrap())?;
    let header = format!(
        "header book_trace v1 book {name} bytes {} nodes {nodes} roots {digest:016x} engine {}",
        bytes.len(),
        othello_tools::provenance::git_commit()
    );
    let (mut journal, done) =
        othello_tools::journal::Journal::open(std::path::Path::new(&out), &header)?;
    for (pair, root) in roots.iter().enumerate().take(past).skip(first) {
        if done.contains(&format!("{pair}:1")) {
            continue;
        }
        for game in 0..2 {
            let (mut side, mut off) = (tracer(book, nodes), tracer(Book::Off, nodes));
            let (black, white) = if game == 0 {
                (&mut side, &mut off)
            } else {
                (&mut off, &mut side)
            };
            let (score, moves) = traced_game(&root.squares, black, white)?;
            let line = format!(
                "opening {} {}",
                root.line(),
                game_line(black, white, score, &moves)
            );
            journal.append(&format!("{pair}:{game}"), &line)?;
        }
        eprintln!("pair {pair} done");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use othello::agents::search_agent;
    use othello::board::{empty_count, legal_moves};
    use othello::book::BOOK;
    use othello_tools::harness::play_game;

    const NODES: u64 = 20_000;

    fn limits() -> SearchLimits {
        SearchLimits::new(64, othello::search::NO_DEADLINE, NODES)
    }

    fn booked(book: bool) -> SearchConfig {
        SearchConfig {
            book,
            ..SearchConfig::standard()
        }
    }

    /// Book off, the tracer plays the production agent's game exactly.
    #[test]
    fn off_matches_agent() {
        let opening = [19, 18];
        let (mut a, mut b) = (tracer(Book::Off, NODES), tracer(Book::Off, NODES));
        let traced = play_game(&opening, &mut a, &mut b).unwrap();
        let (mut x, mut y) = (
            search_agent(booked(false), limits()),
            search_agent(booked(false), limits()),
        );
        assert_eq!(traced, play_game(&opening, &mut x, &mut y).unwrap());
        assert!(
            a.log
                .iter()
                .chain(&b.log)
                .all(|d| d.source == Source::Search)
        );
    }

    /// With the embedded book, the tracer plays the book-enabled agent's
    /// game exactly, and the start is a logged book hit.
    #[test]
    fn embedded_matches_agent() {
        let (mut a, mut b) = (tracer(Book::Embedded, NODES), tracer(Book::Off, NODES));
        let traced = play_game(&[], &mut a, &mut b).unwrap();
        let (mut x, mut y) = (
            search_agent(booked(true), limits()),
            search_agent(booked(false), limits()),
        );
        assert_eq!(traced, play_game(&[], &mut x, &mut y).unwrap());
        let first = a.log[0];
        assert_eq!(first.source, Source::Book);
        assert_eq!(
            (first.depth, first.nodes, first.status),
            (0, 0, SearchStatus::Complete)
        );
        assert_eq!(first.key, othello::tt::zobrist(Board::START));
    }

    /// The embedded asset's bytes through `book::lookup` give the same
    /// decisions as `book::play`.
    #[test]
    fn asset_matches_embedded() {
        let (mut a, mut b) = (tracer(Book::Asset(BOOK), NODES), tracer(Book::Off, NODES));
        let via_lookup = play_game(&[], &mut a, &mut b).unwrap();
        let (mut x, mut y) = (tracer(Book::Embedded, NODES), tracer(Book::Off, NODES));
        assert_eq!(via_lookup, play_game(&[], &mut x, &mut y).unwrap());
        assert_eq!(a.log, x.log);
    }

    /// An empty asset never hits: the tracer searches every move.
    #[test]
    fn empty_asset_searches() {
        let (mut a, mut b) = (tracer(Book::Asset(&[]), NODES), tracer(Book::Off, NODES));
        play_game(&[], &mut a, &mut b).unwrap();
        assert!(
            a.log
                .iter()
                .all(|d| d.source == Source::Search && d.nodes > 0)
        );
    }

    /// Every logged decision had a legal move; passes log nothing; lone
    /// moves are searched in node mode and logged with one legal move.
    #[test]
    fn decisions_and_passes() {
        let found = (0..200u8).find_map(|k| {
            let opening = [19, [18, 20, 34][usize::from(k % 3)]];
            let (mut a, mut b) = (
                tracer(Book::Off, 2_000 + u64::from(k)),
                tracer(Book::Off, 2_000),
            );
            let game = play_game(&opening, &mut a, &mut b).ok()?;
            let decisions = a.log.len() + b.log.len();
            let lone = a.log.iter().chain(&b.log).any(|d| d.legal == 1);
            (game.passes > 0 && lone).then_some((game, decisions, a, b))
        });
        let (game, decisions, a, b) = found.expect("a game with a pass and a lone move");
        assert_eq!(decisions as u32 + game.passes + 2, game.plies);
        for d in a.log.iter().chain(&b.log) {
            assert!(d.legal >= 1);
            assert_eq!(d.source, Source::Search);
        }
        let _ = (empty_count, legal_moves);
    }

    #[test]
    fn line_roundtrip() {
        let d = Decision {
            key: 0x0123_4567_89ab_cdef,
            legal: 7,
            source: Source::Book,
            square: 19,
            depth: 0,
            status: SearchStatus::Complete,
            nodes: 0,
        };
        let s = Decision {
            source: Source::Search,
            depth: 9,
            status: SearchStatus::Partial,
            nodes: 123_456,
            ..d
        };
        for decision in [d, s] {
            assert_eq!(Decision::parse(&decision.line()), Some(decision));
        }
        assert_eq!(Decision::parse("garbage"), None);
    }
}
