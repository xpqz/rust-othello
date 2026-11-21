//! Book diagnostic traces (`docs/design/opening-book-v2.md` § First
//! experiment): node-capped games with the book off or a book asset, recording every engine decision in play order.
//!
//! A `Tracer` plays exactly as the production search agent would with the
//! same configuration. Each decision logs the board key, the number of
//! legal moves, the source (book or search), the move, and the search's
//! depth, status and reported nodes. Passes are logged as events too.
//!
//! Usage: `book_trace --roots FILE --book off|PATH --nodes N
//! --out JOURNAL [--pair-range A..B]`. A roots line is a move sequence of
//! any length, or `start` for the standard start. For each root the book
//! side plays black, then white, against book-off, with fresh tracers per
//! game. A pair is journalled game by game and kept only when both games
//! replay to a finished position with the recorded score.

use othello::agents::Agent;
use othello::board::{Board, final_score, is_terminal, legal_moves, pass, play};
use othello::score::terminal_score;
use othello::search::{SearchConfig, SearchLimits, SearchStatus, search_tt};
use othello::tt::{Table, zobrist};

/// Bytes per asset record: `tt::zobrist` of the canonical board, the move
/// as a canonical square, and the value as i16, little-endian, sorted by key.
const RECORD: usize = 11;

/// Swaps file and rank: (f, r) -> (r, f).
fn transpose(mut x: u64) -> u64 {
    for (shift, mask) in [
        (28, 0x0f0f_0f0f_0000_0000u64),
        (14, 0x3333_0000_3333_0000),
        (7, 0x5500_5500_5500_5500),
    ] {
        let t = mask & (x ^ (x << shift));
        x ^= t ^ (t >> shift);
    }
    x
}

/// The image of `bits` under symmetry `kind`, numbered as in the engine's `pattern.rs`.
fn image(bits: u64, kind: u8) -> u64 {
    let mirror = |x: u64| x.reverse_bits().swap_bytes();
    match kind {
        0 => bits,
        1 => mirror(transpose(bits)),
        2 => bits.reverse_bits(),
        3 => transpose(bits).swap_bytes(),
        4 => mirror(bits),
        5 => bits.swap_bytes(),
        6 => transpose(bits),
        _ => transpose(bits).reverse_bits(),
    }
}

/// The stored move, mapped back to `board`'s orientation, and its value,
/// exactly as the retired engine module `othello::book` answered it.
/// The canonical board is the least `(me, opp)` over the eight images, as
/// in the builder. A board without a legal move is never answered.
fn book_lookup(asset: &[u8], board: Board) -> Option<(u8, i32)> {
    let moves = legal_moves(board.me, board.opp);
    let (records, rest) = asset.as_chunks::<RECORD>();
    if moves == 0 || !rest.is_empty() {
        return None;
    }
    let (kind, key) = (0..8)
        .map(|k| (k, (image(board.me, k), image(board.opp, k))))
        .min_by_key(|&(_, key)| key)?;
    let target = zobrist(Board {
        me: key.0,
        opp: key.1,
    });
    let index = records
        .binary_search_by_key(&target, |r| u64::from_le_bytes(r[..8].try_into().unwrap()))
        .ok()?;
    let record = records[index];
    if record[8] >= 64 {
        return None;
    }
    let inverse = [0, 3, 2, 1, 4, 5, 6, 7][kind as usize];
    let square = image(1 << record[8], inverse).trailing_zeros() as u8;
    let value = i16::from_le_bytes([record[9], record[10]]).into();
    (moves & (1 << square) != 0).then_some((square, value))
}

/// Which book a tracer consults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Book<'a> {
    Off,
    /// An asset's bytes, through `book_lookup`.
    Asset(&'a [u8]),
}

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
    fn choose(&mut self, board: Board) -> Option<u8> {
        let moves = legal_moves(board.me, board.opp);
        if moves == 0 {
            return None;
        }
        let hit = match self.book {
            Book::Off => None,
            Book::Asset(bytes) => book_lookup(bytes, board),
        };
        let (source, square, depth, status, nodes) = match hit {
            Some((square, _)) => (Source::Book, square, 0, SearchStatus::Complete, 0),
            None => {
                let r = search_tt(board, self.limits, &self.config, &mut self.table);
                (Source::Search, r.best, r.depth, r.status, r.nodes)
            }
        };
        self.log.push(Decision {
            key: zobrist(board),
            legal: moves.count_ones(),
            source,
            square,
            depth,
            status,
            nodes,
        });
        Some(square)
    }

    fn reset(&mut self) {
        self.table.clear();
    }
}

impl Decision {
    /// `key:legal:source:square:depth:status:nodes`, key in hex, source `b`
    /// or `s`, status `c`, `p` or `f`.
    fn line(&self) -> String {
        let source = if self.source == Source::Book {
            'b'
        } else {
            's'
        };
        let status = match self.status {
            SearchStatus::Complete => 'c',
            SearchStatus::Partial => 'p',
            SearchStatus::Fallback => 'f',
        };
        let Decision {
            key,
            legal,
            square,
            depth,
            nodes,
            ..
        } = *self;
        format!("{key:016x}:{legal}:{source}:{square}:{depth}:{status}:{nodes}")
    }

    fn parse(text: &str) -> Option<Decision> {
        let fields: Vec<&str> = text.split(':').collect();
        let [key, legal, source, square, depth, status, nodes] = fields[..] else {
            return None;
        };
        Some(Decision {
            key: u64::from_str_radix(key, 16).ok()?,
            legal: legal.parse().ok()?,
            source: match source {
                "b" => Source::Book,
                "s" => Source::Search,
                _ => return None,
            },
            square: square.parse().ok().filter(|&s: &u8| s < 64)?,
            depth: depth.parse().ok()?,
            status: match status {
                "c" => SearchStatus::Complete,
                "p" => SearchStatus::Partial,
                "f" => SearchStatus::Fallback,
                _ => return None,
            },
            nodes: nodes.parse().ok()?,
        })
    }
}

/// The board after `opening` and whether black is to move.
fn opened(opening: &[u8]) -> Result<(Board, bool), String> {
    let mut board = Board::START;
    for &square in opening {
        if square >= 64 || legal_moves(board.me, board.opp) & (1 << square) == 0 {
            return Err(format!("illegal opening move {square}"));
        }
        board = play(board, square);
    }
    Ok((board, opening.len().is_multiple_of(2)))
}

/// Black's terminal score in centidiscs.
fn score_for_black(board: Board, black_to_move: bool) -> i32 {
    let value = terminal_score(final_score(board));
    if black_to_move { value } else { -value }
}

/// A game from `opening` between two tracers, with every event in order,
/// and black's final score in centidiscs.
fn trace_game<'a>(
    opening: &[u8],
    black: &mut Tracer<'a>,
    white: &mut Tracer<'a>,
) -> Result<(i32, Vec<Event>), String> {
    let (mut board, mut black_to_move) = opened(opening)?;
    let mut events = Vec::new();
    loop {
        let moves = legal_moves(board.me, board.opp);
        if moves == 0 {
            if legal_moves(board.opp, board.me) == 0 {
                return Ok((score_for_black(board, black_to_move), events));
            }
            events.push(Event::Pass {
                black: black_to_move,
            });
            board = pass(board);
            black_to_move = !black_to_move;
            continue;
        }
        let agent = if black_to_move {
            &mut *black
        } else {
            &mut *white
        };
        let square = agent.choose(board).ok_or("agent returned no move")?;
        if moves & (1 << square) == 0 {
            return Err(format!("illegal move {square}"));
        }
        let decision = *agent.log.last().ok_or("no decision logged")?;
        events.push(Event::Move {
            black: black_to_move,
            decision,
        });
        board = play(board, square);
        black_to_move = !black_to_move;
    }
}

/// Events as journal text: `b<decision>` or `w<decision>` for moves,
/// `b-` or `w-` for passes.
fn events_line(events: &[Event]) -> String {
    let side = |black: bool| if black { 'b' } else { 'w' };
    let tokens: Vec<String> = events
        .iter()
        .map(|e| match *e {
            Event::Move { black, decision } => format!("{}{}", side(black), decision.line()),
            Event::Pass { black } => format!("{}-", side(black)),
        })
        .collect();
    tokens.join(" ")
}

/// Parses `events_line`'s text and replays it from `opening`, checking each
/// decision's key, legal count and move, and that each pass was forced.
fn replay_events(opening: &[u8], text: &str) -> Result<Vec<Event>, String> {
    let (mut board, mut black_to_move) = opened(opening)?;
    let mut events = Vec::new();
    for token in text.split(' ').filter(|t| !t.is_empty()) {
        let turn = if black_to_move { "b" } else { "w" };
        let Some(rest) = token.strip_prefix(turn) else {
            return Err(format!("event {token} out of turn"));
        };
        let moves = legal_moves(board.me, board.opp);
        if rest == "-" {
            if moves != 0 || legal_moves(board.opp, board.me) == 0 {
                return Err(format!("unforced pass {token}"));
            }
            events.push(Event::Pass {
                black: black_to_move,
            });
            board = pass(board);
        } else {
            let d = Decision::parse(rest).ok_or(format!("bad decision {token}"))?;
            let fits = d.key == zobrist(board) && d.legal == moves.count_ones();
            if !fits || moves & (1 << d.square) == 0 {
                return Err(format!("decision {token} does not match the position"));
            }
            events.push(Event::Move {
                black: black_to_move,
                decision: d,
            });
            board = play(board, d.square);
        }
        black_to_move = !black_to_move;
    }
    Ok(events)
}

/// The board after `events` from `opening`, and whether black is to move.
fn after(opening: &[u8], events: &[Event]) -> Option<(Board, bool)> {
    let (mut board, mut black_to_move) = opened(opening).ok()?;
    for event in events {
        board = match *event {
            Event::Move { decision, .. } => play(board, decision.square),
            Event::Pass { .. } => pass(board),
        };
        black_to_move = !black_to_move;
    }
    Some((board, black_to_move))
}

/// Whether `events` from `opening` end at a terminal position.
fn finished(opening: &[u8], events: &[Event]) -> bool {
    after(opening, events).is_some_and(|(board, _)| is_terminal(board))
}

/// Black's final score in centidiscs after `events` from `opening`, which
/// must have finished.
fn final_black(opening: &[u8], events: &[Event]) -> i32 {
    after(opening, events).map_or(0, |(board, black)| score_for_black(board, black))
}

/// Whether a saved record is a finished game from `root` whose score is
/// the replayed final score.
fn trusted(root: &[u8], line: &str) -> bool {
    let Some((score, text)) = line
        .strip_prefix("score ")
        .and_then(|l| l.split_once(" events "))
    else {
        return false;
    };
    let Ok(events) = replay_events(root, text) else {
        return false;
    };
    finished(root, &events) && score.parse() == Ok(final_black(root, &events))
}

/// A roots file: one move sequence per line, of any length, or `start`;
/// `#` lines are comments. Every sequence must be legal.
fn load_roots(text: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut roots = Vec::new();
    let lines = text.lines().map(str::trim);
    for line in lines.filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let mut squares = Vec::new();
        for name in line.split_whitespace().filter(|_| line != "start") {
            let b = name.as_bytes();
            let valid =
                b.len() == 2 && (b'a'..=b'h').contains(&b[0]) && (b'1'..=b'8').contains(&b[1]);
            if !valid {
                return Err(format!("bad square {name} in root {line}"));
            }
            squares.push((b[1] - b'1') * 8 + (b[0] - b'a'));
        }
        opened(&squares).map_err(|e| format!("root {line}: {e}"))?;
        roots.push(squares);
    }
    Ok(roots)
}

/// A 64-bit FNV-1a digest of a book's bytes, for the journal header.
fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Runs the trace command; see the module documentation.
fn run(args: &[String]) -> Result<(), String> {
    let option = |flag: &str| {
        let index = args.iter().position(|arg| arg == flag)?;
        args.get(index + 1).cloned()
    };
    let usage = "usage: book_trace --roots FILE --book off|PATH --nodes N --out JOURNAL [--pair-range A..B]";
    let roots_path = option("--roots").ok_or(usage)?;
    let text = std::fs::read_to_string(&roots_path).map_err(|e| format!("{roots_path}: {e}"))?;
    let roots = load_roots(&text)?;
    let nodes: u64 = option("--nodes")
        .and_then(|n| n.parse().ok())
        .ok_or(usage)?;
    if nodes == 0 {
        return Err("the node cap must be positive: 0 would remove the bound on work".into());
    }
    let name = option("--book").ok_or(usage)?;
    let bytes = match name.as_str() {
        "off" => Vec::new(),
        path => std::fs::read(path).map_err(|e| format!("{path}: {e}"))?,
    };
    let book = match name.as_str() {
        "off" => Book::Off,
        _ => Book::Asset(&bytes),
    };
    let out = option("--out").ok_or(usage)?;
    let (first, past) = match option("--pair-range") {
        Some(range) => {
            let (a, b) = range.split_once("..").ok_or(usage)?;
            (a.parse().map_err(|_| usage)?, b.parse().map_err(|_| usage)?)
        }
        None => (0, roots.len()),
    };
    if first >= past || past > roots.len() {
        return Err(format!(
            "invalid pair range {first}..{past} for {} roots",
            roots.len()
        ));
    }
    let header = format!(
        "header book_trace v1 book {name} asset {:016x} nodes {nodes} roots {:016x} engine {}",
        digest(&bytes),
        digest(text.as_bytes()),
        othello_tools::provenance::git_commit()
    );
    let path = std::path::Path::new(&out);
    let (mut journal, _) = othello_tools::journal::Journal::open(path, &header)?;
    // A pair counts only when each of its two games has exactly one record,
    // finished, with its score checked; any other record (a duplicate, a
    // foreign id, or part of an incomplete pair) is discarded and replayed.
    let mut copies: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for record in journal.records() {
        copies
            .entry(record.id.clone())
            .or_default()
            .push(record.line.clone());
    }
    let complete = |pair: usize| {
        (0..2).all(
            |g| match copies.get(&format!("{pair}:{g}")).map(Vec::as_slice) {
                Some([line]) => trusted(&roots[pair], line),
                _ => false,
            },
        )
    };
    let stale: std::collections::BTreeSet<String> = copies
        .keys()
        .filter(|id| {
            let parsed = id
                .split_once(':')
                .and_then(|(p, g)| Some((p.parse::<usize>().ok()?, g.parse::<u8>().ok()?)));
            // Only the canonical spelling `{pair}:{game}` counts.
            parsed.is_none_or(|(p, g)| {
                **id != format!("{p}:{g}") || g > 1 || p >= roots.len() || !complete(p)
            })
        })
        .cloned()
        .collect();
    journal.discard(&stale)?;
    for (pair, root) in roots.iter().enumerate().take(past).skip(first) {
        if complete(pair) {
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
    use othello_tools::journal::read;

    const NODES: u64 = 20_000;
    /// The archived three-round asset, byte-identical to the retired
    /// embedded book.
    const R3: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/tuning/book_ext/book.bin"
    );
    const MANIFEST: &str = include_str!("../../docs/tuning/book_manifest.txt");
    /// `start` traced with `--book embedded` at 20,000 nodes before removal.
    const FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/tuning/book_diag/embedded_start_20k.journal"
    );

    /// An asset record for `board`: the zobrist key of its canonical
    /// image, the canonical square and the value, little-endian.
    fn record(board: Board, square: u8, value: i16) -> Vec<u8> {
        let (me, opp) = othello_tools::sym::canonical(board);
        let mut bytes = zobrist(Board { me, opp }).to_le_bytes().to_vec();
        bytes.push(square);
        bytes.extend(value.to_le_bytes());
        bytes
    }

    /// (canonical board, canonical square, value) per manifest record.
    fn manifest() -> Vec<(Board, u8, i16)> {
        let lines = MANIFEST.lines().filter(|l| !l.starts_with('#'));
        lines
            .map(|line| {
                let f: Vec<&str> = line.split(' ').collect();
                let (me, opp) = f[0].split_once(':').unwrap();
                let board = Board {
                    me: u64::from_str_radix(me, 16).unwrap(),
                    opp: u64::from_str_radix(opp, 16).unwrap(),
                };
                let value: i32 = f[2].parse().unwrap();
                (
                    board,
                    f[1].parse().unwrap(),
                    value.clamp(i16::MIN.into(), i16::MAX.into()) as i16,
                )
            })
            .collect()
    }

    fn sym_square(square: u8, kind: u8) -> u8 {
        let bit = othello_tools::sym::transform(
            Board {
                me: 1 << square,
                opp: 0,
            },
            kind,
        );
        bit.me.trailing_zeros() as u8
    }

    /// The archived asset is the manifest's encoding, sorted by key, and
    /// the lookup answers every record in all eight orientations with its
    /// value and an image of its move under a symmetry taking the stored
    /// board to the asked one.
    #[test]
    fn lookup_manifest() {
        let asset = std::fs::read(R3).unwrap();
        let records = manifest();
        assert_eq!(records.len(), 7729);
        let mut rows: Vec<Vec<u8>> = records.iter().map(|&(b, s, v)| record(b, s, v)).collect();
        rows.sort_by_key(|r| u64::from_le_bytes(r[..8].try_into().unwrap()));
        assert_eq!(asset, rows.concat());
        for &(board, square, value) in &records {
            for kind in 0..8 {
                let image = othello_tools::sym::transform(board, kind);
                let (found, v) = book_lookup(&asset, image).expect("a stored position");
                assert_eq!(v, i32::from(value));
                let mapped = (0..8).any(|j| {
                    othello_tools::sym::transform(board, j) == image
                        && sym_square(square, j) == found
                });
                assert!(mapped, "{board:?} kind {kind}: {found}");
            }
        }
    }

    /// No answer from an empty or ragged asset, for a board without a
    /// legal move, for an out-of-range or illegal stored square, or for a
    /// position not stored.
    #[test]
    fn lookup_misses() {
        let asset = std::fs::read(R3).unwrap();
        assert!(book_lookup(&asset, Board::START).is_some());
        assert_eq!(book_lookup(&[], Board::START), None);
        assert_eq!(book_lookup(&asset[..asset.len() - 1], Board::START), None);
        let stuck = Board { me: 1, opp: 0 };
        assert_eq!(book_lookup(&record(stuck, 1, 0), stuck), None);
        assert_eq!(
            book_lookup(&record(Board::START, 64, 0), Board::START),
            None
        );
        assert_eq!(
            book_lookup(&record(Board::START, 0, 0), Board::START),
            None,
            "a1 is illegal"
        );
        let (me, opp) = othello_tools::sym::canonical(Board::START);
        let canonical = Board { me, opp };
        let legal = legal_moves(canonical.me, canonical.opp).trailing_zeros() as u8;
        let hit = book_lookup(&record(Board::START, legal, 7), Board::START);
        assert_eq!(hit.map(|(_, v)| v), Some(7), "a synthetic hit");
        let mut deep = Board::START;
        for _ in 0..30 {
            let moves = legal_moves(deep.me, deep.opp);
            deep = if moves == 0 {
                pass(deep)
            } else {
                play(deep, moves.trailing_zeros() as u8)
            };
        }
        assert_eq!(book_lookup(&asset, deep), None, "not stored");
    }

    /// `--book embedded` is retired: refused before any journal exists.
    #[test]
    fn command_embedded_retired() {
        let (roots, out) = (temp("embedded_roots"), temp("embedded_out"));
        std::fs::write(&roots, "d3\n").unwrap();
        let _ = std::fs::remove_file(&out);
        assert!(run(&args(&roots, "embedded", "20000", &out)).is_err());
        assert!(!out.exists(), "no journal for a refused run");
        let _ = std::fs::remove_file(&roots);
    }

    /// The archived asset as a file reproduces the game records traced
    /// with the embedded book before its removal.
    #[test]
    fn archived_reproduces_embedded() {
        let (roots, out) = (temp("archived_roots"), temp("archived_out"));
        std::fs::write(&roots, "start\n").unwrap();
        let _ = std::fs::remove_file(&out);
        run(&args(&roots, R3, "20000", &out)).unwrap();
        let fixture = read(std::path::Path::new(FIXTURE)).unwrap().1;
        assert_eq!(read(&out).unwrap().1, fixture);
        for path in [&roots, &out] {
            let _ = std::fs::remove_file(path);
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
    fn production(opening: &[u8], nodes: u64) -> (i32, Vec<(bool, u8)>) {
        let cap = SearchLimits::new(64, othello::search::NO_DEADLINE, nodes);
        let mut black = Recorder(search_agent(SearchConfig::standard(), cap), Vec::new());
        let mut white = Recorder(search_agent(SearchConfig::standard(), cap), Vec::new());
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

    /// A book-off trace's moves and colours equal a production game's, in
    /// order; `archived_reproduces_embedded` covers the book side.
    #[test]
    fn trace_matches_production() {
        for opening in [vec![19, 18], vec![]] {
            let (mut a, mut b) = (tracer(Book::Off, NODES), tracer(Book::Off, NODES));
            let (score, events) = trace_game(&opening, &mut a, &mut b).unwrap();
            assert_eq!((score, moves(&events)), production(&opening, NODES));
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

    /// A lone legal move: book-off logs it as a search with one legal move,
    /// and plays the production node-limited agent's game, that move
    /// included.
    #[test]
    fn lone_move_searched() {
        let found = (0..300u64).find_map(|k| {
            let opening = [19, [18, 20, 34][(k % 3) as usize]];
            let nodes = 2_000 + k;
            let (mut a, mut b) = (tracer(Book::Off, nodes), tracer(Book::Off, nodes));
            let (score, events) = trace_game(&opening, &mut a, &mut b).ok()?;
            let lone = events
                .iter()
                .any(|e| matches!(e, Event::Move { decision, .. } if decision.legal == 1));
            lone.then_some((opening, nodes, score, events))
        });
        let (opening, nodes, score, events) = found.expect("a game with a lone move");
        for event in &events {
            if let Event::Move { decision, .. } = event
                && decision.legal == 1
            {
                assert_eq!(decision.source, Source::Search);
                assert!(decision.nodes > 0, "node mode searches lone moves");
            }
        }
        assert_eq!((score, moves(&events)), production(&opening, nodes));
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
        let tokens: Vec<&str> = text.split(' ').collect();
        let short = tokens[..tokens.len() - 2].join(" ");
        let partial = replay_events(&[19, 18], &short).unwrap();
        assert!(
            !finished(&[19, 18], &partial),
            "a legal prefix is not a finished game"
        );
        assert!(finished(&[19, 18], &events));
    }

    /// A book hit is logged as a complete depth-0 decision with no nodes;
    /// an empty asset never hits.
    #[test]
    fn asset_paths() {
        let asset = std::fs::read(R3).unwrap();
        let (mut a, mut b) = (tracer(Book::Asset(&asset), NODES), tracer(Book::Off, NODES));
        let via_lookup = trace_game(&[], &mut a, &mut b).unwrap();
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
                // A fallback after a node-capped solve reports no nodes:
                // the aborted solve's work is not counted.
                let fallback = decision.status == SearchStatus::Fallback;
                assert!(fallback || decision.nodes > 0, "{decision:?}");
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
        assert_ne!(digest(&[]), digest(&std::fs::read(R3).unwrap()));
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
        run(&args(&roots, R3, "20000", &out)).unwrap();
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

    /// Fully written records that cannot be trusted are replayed: a legal
    /// but unfinished trace, and a wrong final score. Valid pairs are kept.
    #[test]
    fn command_resume_validates() {
        let (roots, whole, bad) = (temp("valid_roots"), temp("valid_whole"), temp("valid_bad"));
        std::fs::write(&roots, "d3\nd3 c3\nd3 c5\n").unwrap();
        for path in [&whole, &bad] {
            let _ = std::fs::remove_file(path);
        }
        run(&args(&roots, "off", "20000", &whole)).unwrap();
        let (head, all) = read(&whole).unwrap();
        let body: String = all
            .iter()
            .map(|r| {
                let line = match r.id.as_str() {
                    "1:0" => {
                        let tokens: Vec<&str> = r.line.split(' ').collect();
                        tokens[..tokens.len() - 2].join(" ")
                    }
                    "2:1" => {
                        let score = r.line.split(' ').nth(1).unwrap();
                        let changed = score.parse::<i32>().unwrap() + 200;
                        r.line
                            .replacen(&format!("score {score} "), &format!("score {changed} "), 1)
                    }
                    _ => r.line.clone(),
                };
                format!("{}\t{line}\n", r.id)
            })
            .collect();
        std::fs::write(&bad, format!("{head}\n{body}")).unwrap();
        run(&args(&roots, "off", "20000", &bad)).unwrap();
        assert_eq!(records(&bad), records(&whole));
        for path in [&roots, &whole, &bad] {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Duplicate and foreign records cannot make a pair complete: they are
    /// removed, and the journal ends with exactly one record per game.
    #[test]
    fn command_resume_strays() {
        let (roots, whole, extra) = (
            temp("stray_roots"),
            temp("stray_whole"),
            temp("stray_extra"),
        );
        std::fs::write(&roots, "d3\nd3 c3\n").unwrap();
        for path in [&whole, &extra] {
            let _ = std::fs::remove_file(path);
        }
        run(&args(&roots, "off", "20000", &whole)).unwrap();
        let text = std::fs::read_to_string(&whole).unwrap();
        let first = text.lines().nth(1).unwrap().to_string();
        let foreign = first.replacen("0:0\t", "0:2\t", 1);
        let alias = first.replacen("0:0\t", "00:0\t", 1);
        std::fs::write(&extra, format!("{text}{first}\n{foreign}\n{alias}\n")).unwrap();
        run(&args(&roots, "off", "20000", &extra)).unwrap();
        let mut ids: Vec<String> = read(&extra).unwrap().1.into_iter().map(|r| r.id).collect();
        ids.sort();
        assert_eq!(ids, ["0:0", "0:1", "1:0", "1:1"]);
        assert_eq!(records(&extra), records(&whole));
        for path in [&roots, &whole, &extra] {
            let _ = std::fs::remove_file(path);
        }
    }

    /// A pair range must lie within the roots: refused before any journal.
    #[test]
    fn command_bad_range() {
        let (roots, out) = (temp("range_roots"), temp("range_out"));
        std::fs::write(&roots, "d3\nd3 c3\n").unwrap();
        for range in ["2..3", "0..3", "1..1", "x..2"] {
            let _ = std::fs::remove_file(&out);
            let mut command = args(&roots, "off", "20000", &out);
            command.extend(["--pair-range".into(), range.into()]);
            assert!(run(&command).is_err(), "{range} accepted");
            assert!(!out.exists(), "{range}: journal created");
        }
        let _ = std::fs::remove_file(&roots);
    }

    /// A different asset of the same size cannot resume a journal.
    #[test]
    fn command_asset_bound() {
        let (roots, asset, out) = (temp("bound_roots"), temp("bound_asset"), temp("bound_out"));
        std::fs::write(&roots, "d3\n").unwrap();
        let book = std::fs::read(R3).unwrap();
        std::fs::write(&asset, &book[..book.len() - 11]).unwrap();
        let _ = std::fs::remove_file(&out);
        run(&args(&roots, asset.to_str().unwrap(), "20000", &out)).unwrap();
        let mut changed = book[..book.len() - 11].to_vec();
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
