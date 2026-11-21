//! Book diagnostic traces (`docs/design/opening-book-v2.md` § First
//! experiment): node-capped games with the book off, the embedded book, or
//! another book asset, recording every engine decision in play order.
//!
//! A `Tracer` plays exactly as the production search agent would with the
//! same configuration. Each decision logs the board key, the number of
//! legal moves, the source (book or search), the move, and the search's
//! depth, status and reported nodes. Passes are logged as events too.
//!
//! Usage: `book_trace --roots FILE --book off|embedded|PATH --nodes N
//! --out JOURNAL [--pair-range A..B]`. A roots line is a move sequence of
//! any length, or `start` for the standard start. For each root the book
//! side plays black, then white, against book-off, with fresh tracers per
//! game. A pair is journalled game by game and kept only when complete.

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

/// One event of a game in play order; `black` names the side.
// RED scaffolding: constructed by `trace_game`, removed in GREEN.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Move { black: bool, decision: Decision },
    Pass { black: bool },
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
    /// `key:legal:source:square:depth:status:nodes`, key in hex, source `b`
    /// or `s`, status `c`, `p` or `f`.
    // RED scaffolding: used by `events_line`, removed in GREEN.
    #[allow(dead_code)]
    fn line(&self) -> String {
        String::new()
    }

    // RED scaffolding: used by the trace parser, removed in GREEN.
    #[allow(dead_code)]
    fn parse(_text: &str) -> Option<Decision> {
        None
    }
}

/// A game from `opening` between two tracers, with every event in order,
/// and black's final score in centidiscs.
fn trace_game(
    _opening: &[u8],
    black: &mut Tracer,
    white: &mut Tracer,
) -> Result<(i32, Vec<Event>), String> {
    let _ = (&mut black.log, &mut white.log);
    Err("trace_game: not implemented".into())
}

/// Events as journal text: `b<decision>` or `w<decision>` for moves,
/// `b-` or `w-` for passes.
fn events_line(_events: &[Event]) -> String {
    String::new()
}

/// Parses `events_line`'s text and replays it from `opening`, checking each
/// decision's key, legal count and move, and that each pass was forced.
// RED scaffolding: used by the resume check, removed in GREEN.
#[allow(dead_code)]
fn replay_events(_opening: &[u8], _text: &str) -> Result<Vec<Event>, String> {
    Err("replay_events: not implemented".into())
}

/// A roots file: one move sequence per line, of any length, or `start`;
/// `#` lines are comments. Every sequence must be legal.
fn load_roots(_text: &str) -> Result<Vec<Vec<u8>>, String> {
    Err("load_roots: not implemented".into())
}

/// A 64-bit FNV-1a digest of a book's bytes, for the journal header.
fn digest(_bytes: &[u8]) -> u64 {
    0
}

/// Runs the trace command; see the module documentation.
fn run(args: &[String]) -> Result<(), String> {
    let option = |flag: &str| {
        let index = args.iter().position(|arg| arg == flag)?;
        args.get(index + 1).cloned()
    };
    let usage = "usage: book_trace --roots FILE --book off|embedded|PATH --nodes N --out JOURNAL [--pair-range A..B]";
    let roots_path = option("--roots").ok_or(usage)?;
    let text = std::fs::read_to_string(&roots_path).map_err(|e| format!("{roots_path}: {e}"))?;
    let roots = load_roots(&text)?;
    let name = option("--book").ok_or(usage)?;
    let bytes = match name.as_str() {
        "off" | "embedded" => Vec::new(),
        path => std::fs::read(path).map_err(|e| format!("{path}: {e}"))?,
    };
    let book = match name.as_str() {
        "off" => Book::Off,
        "embedded" => Book::Embedded,
        _ => Book::Asset(&bytes),
    };
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
    let header = format!(
        "header book_trace v1 book {name} asset {:016x} nodes {nodes} roots {:016x} engine {}",
        digest(&bytes),
        digest(text.as_bytes()),
        othello_tools::provenance::git_commit()
    );
    let path = std::path::Path::new(&out);
    let (mut journal, done) = othello_tools::journal::Journal::open(path, &header)?;
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
            let (score, events) = trace_game(root, black, white)?;
            let line = format!("score {score} events {}", events_line(&events));
            journal.append(&format!("{pair}:{game}"), &line)?;
        }
        eprintln!("pair {pair} done");
    }
    Ok(())
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
    use othello::agents::search_agent;
    use othello::board::{legal_moves, pass, play};
    use othello::book::BOOK;
    use othello::tt::zobrist;
    use othello_tools::journal::read;

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

    /// A production agent that records the moves it plays.
    struct Recorder<A: Agent>(A, Vec<u8>);

    impl<A: Agent> Agent for Recorder<A> {
        fn choose(&mut self, board: Board) -> Option<u8> {
            let square = self.0.choose(board);
            self.1.extend(square);
            square
        }
    }

    /// The moves and colours of a production game, in play order.
    fn production(opening: &[u8], book: bool) -> (i32, Vec<(bool, u8)>) {
        let mut black = Recorder(search_agent(booked(book), limits()), Vec::new());
        let mut white = Recorder(search_agent(booked(false), limits()), Vec::new());
        let game = othello_tools::harness::play_game(opening, &mut black, &mut white).unwrap();
        // Rebuild the colour order by replaying the two move lists.
        let mut board = Board::START;
        for &square in opening {
            board = play(board, square);
        }
        let mut black_to_move = opening.len().is_multiple_of(2);
        let (mut b, mut w) = (black.1.iter(), white.1.iter());
        let mut order = Vec::new();
        while legal_moves(board.me, board.opp) != 0 || legal_moves(board.opp, board.me) != 0 {
            if legal_moves(board.me, board.opp) == 0 {
                board = pass(board);
                black_to_move = !black_to_move;
                continue;
            }
            let square = *if black_to_move { b.next() } else { w.next() }.unwrap();
            order.push((black_to_move, square));
            board = play(board, square);
            black_to_move = !black_to_move;
        }
        (game.score, order)
    }

    fn moves(events: &[Event]) -> Vec<(bool, u8)> {
        events
            .iter()
            .filter_map(|e| match *e {
                Event::Move { black, decision } => Some((black, decision.square)),
                Event::Pass { .. } => None,
            })
            .collect()
    }

    /// The trace's moves and colours equal a production game's, in order.
    #[test]
    fn trace_matches_production() {
        for (opening, book) in [(vec![19, 18], false), (vec![], true)] {
            let mode = if book { Book::Embedded } else { Book::Off };
            let (mut a, mut b) = (tracer(mode, NODES), tracer(Book::Off, NODES));
            let (score, events) = trace_game(&opening, &mut a, &mut b).unwrap();
            assert_eq!(
                (score, moves(&events)),
                production(&opening, book),
                "book {book}"
            );
        }
    }

    /// A traced game with a pass followed by another move by the same
    /// colour replays from its opening: keys, legal counts, moves and the
    /// forced pass all check out, through the persisted text.
    #[test]
    fn trace_replays_with_pass() {
        let found = (0..300u64).find_map(|k| {
            let opening = [19, [18, 20, 34][(k % 3) as usize]];
            let (mut a, mut b) = (tracer(Book::Off, 2_000 + k), tracer(Book::Off, 2_000));
            let (_, events) = trace_game(&opening, &mut a, &mut b).ok()?;
            let repeat = events.windows(2).any(|w| match (w[0], w[1]) {
                (Event::Pass { black: p }, Event::Move { black: m, .. }) => p != m,
                _ => false,
            });
            repeat.then_some((opening, events))
        });
        let (opening, events) = found.expect("a pass followed by the other colour moving twice");
        let replayed = replay_events(&opening, &events_line(&events)).unwrap();
        assert_eq!(replayed, events);
        // Each logged key is the board the mover saw.
        let mut board = Board::START;
        for &square in &opening {
            board = play(board, square);
        }
        for event in &events {
            match *event {
                Event::Move { decision, .. } => {
                    assert_eq!(decision.key, zobrist(board));
                    assert_eq!(
                        decision.legal,
                        legal_moves(board.me, board.opp).count_ones()
                    );
                    board = play(board, decision.square);
                }
                Event::Pass { .. } => board = pass(board),
            }
        }
    }

    #[test]
    fn replay_rejects_tampering() {
        let (mut a, mut b) = (tracer(Book::Off, NODES), tracer(Book::Off, NODES));
        let (_, events) = trace_game(&[19, 18], &mut a, &mut b).unwrap();
        let text = events_line(&events);
        assert!(replay_events(&[19, 20], &text).is_err(), "wrong opening");
        assert!(replay_events(&[19, 18], &text.replacen(":s:", ":b:", 1)).is_ok());
        let first = text.split(' ').next().unwrap().to_string();
        assert!(
            replay_events(&[19, 18], &text.replacen(&first, "b-", 1)).is_err(),
            "unforced pass"
        );
    }

    /// The embedded bytes through `book::lookup` give the same events as
    /// `book::play`; an empty asset never hits.
    #[test]
    fn asset_paths() {
        let (mut a, mut b) = (tracer(Book::Asset(BOOK), NODES), tracer(Book::Off, NODES));
        let via_lookup = trace_game(&[], &mut a, &mut b).unwrap();
        let (mut x, mut y) = (tracer(Book::Embedded, NODES), tracer(Book::Off, NODES));
        assert_eq!(via_lookup, trace_game(&[], &mut x, &mut y).unwrap());
        let Event::Move { decision, .. } = via_lookup.1[0] else {
            panic!("first event")
        };
        assert_eq!(decision.source, Source::Book);
        assert_eq!(
            (decision.depth, decision.nodes, decision.status),
            (0, 0, SearchStatus::Complete)
        );
        let (mut e, mut f) = (tracer(Book::Asset(&[]), NODES), tracer(Book::Off, NODES));
        let (_, events) = trace_game(&[], &mut e, &mut f).unwrap();
        for event in events {
            if let Event::Move { decision, .. } = event {
                assert_eq!(decision.source, Source::Search);
                assert!(decision.nodes > 0);
            }
        }
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

    /// Early roots: `start`, one to three placements, comments; illegal
    /// sequences are refused.
    #[test]
    fn roots_loader() {
        let roots = load_roots("# comment\nstart\nd3\nd3 c3 c4\n").unwrap();
        assert_eq!(roots, vec![vec![], vec![19], vec![19, 18, 26]]);
        assert!(load_roots("a1\n").is_err());
        assert!(load_roots("d3 d3\n").is_err());
    }

    #[test]
    fn digest_distinguishes() {
        assert_ne!(digest(b"abcd"), digest(b"abce"));
        assert_eq!(digest(b"abcd"), digest(b"abcd"));
        assert_ne!(digest(&[]), digest(BOOK));
    }

    fn temp(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("book_trace_{}_{label}", std::process::id()))
    }

    fn args(
        roots: &std::path::Path,
        book: &str,
        nodes: &str,
        out: &std::path::Path,
    ) -> Vec<String> {
        let (r, o) = (roots.to_str().unwrap(), out.to_str().unwrap());
        ["--roots", r, "--book", book, "--nodes", nodes, "--out", o]
            .map(String::from)
            .to_vec()
    }

    fn records(path: &std::path::Path) -> std::collections::BTreeMap<String, String> {
        read(path)
            .unwrap()
            .1
            .into_iter()
            .map(|r| (r.id, r.line))
            .collect()
    }

    /// Through the command: early roots trace, and every record replays.
    #[test]
    fn command_early_roots() {
        let (roots, out) = (temp("early_roots"), temp("early_out"));
        std::fs::write(&roots, "start\nd3\nd3 c3 c4\n").unwrap();
        let _ = std::fs::remove_file(&out);
        run(&args(&roots, "embedded", "20000", &out)).unwrap();
        let saved = records(&out);
        assert_eq!(saved.len(), 6);
        let openings: [&[u8]; 3] = [&[], &[19], &[19, 18, 26]];
        for (id, line) in &saved {
            let pair: usize = id.split(':').next().unwrap().parse().unwrap();
            let text = line.split(" events ").nth(1).unwrap();
            replay_events(openings[pair], text).unwrap();
        }
        let _ = (std::fs::remove_file(&roots), std::fs::remove_file(&out));
    }

    /// A pair interrupted after its first game, and a torn last record,
    /// resume to the same records as an uninterrupted run.
    #[test]
    fn command_resume() {
        let (roots, whole, cut) = (
            temp("resume_roots"),
            temp("resume_whole"),
            temp("resume_cut"),
        );
        std::fs::write(&roots, "d3\nd3 c3\nd3 c5\n").unwrap();
        for path in [&whole, &cut] {
            let _ = std::fs::remove_file(path);
        }
        run(&args(&roots, "off", "20000", &whole)).unwrap();
        let (head, all) = read(&whole).unwrap();
        let keep: String = all
            .iter()
            .filter(|r| ["0:0", "0:1", "1:0"].contains(&r.id.as_str()))
            .map(|r| format!("{}\t{}\n", r.id, r.line))
            .collect();
        let torn = format!("{head}\n{keep}2:0\tscore 1");
        std::fs::write(&cut, torn).unwrap();
        run(&args(&roots, "off", "20000", &cut)).unwrap();
        assert_eq!(records(&cut), records(&whole));
        let ids: Vec<String> = read(&cut).unwrap().1.into_iter().map(|r| r.id).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "one record per colour");
        for path in [&roots, &whole, &cut] {
            let _ = std::fs::remove_file(path);
        }
    }

    /// A different asset of the same size cannot resume a journal.
    #[test]
    fn command_asset_bound() {
        let (roots, asset, out) = (temp("bound_roots"), temp("bound_asset"), temp("bound_out"));
        std::fs::write(&roots, "d3\n").unwrap();
        std::fs::write(&asset, &BOOK[..BOOK.len() - 11]).unwrap();
        let _ = std::fs::remove_file(&out);
        run(&args(&roots, asset.to_str().unwrap(), "20000", &out)).unwrap();
        let mut changed = BOOK[..BOOK.len() - 11].to_vec();
        changed[0] ^= 1;
        std::fs::write(&asset, &changed).unwrap();
        let refused = run(&args(&roots, asset.to_str().unwrap(), "20000", &out));
        assert!(refused.unwrap_err().contains("mismatch"));
        for path in [&roots, &asset, &out] {
            let _ = std::fs::remove_file(path);
        }
    }

    /// A zero node cap would remove the bound on work: refused before any
    /// journal exists.
    #[test]
    fn command_zero_nodes() {
        let (roots, out) = (temp("zero_roots"), temp("zero_out"));
        std::fs::write(&roots, "d3\n").unwrap();
        let _ = std::fs::remove_file(&out);
        let refused = run(&args(&roots, "off", "0", &out)).unwrap_err();
        assert!(refused.contains("node cap"), "{refused}");
        assert!(!out.exists(), "no journal for a refused run");
        let _ = std::fs::remove_file(&roots);
    }
}
