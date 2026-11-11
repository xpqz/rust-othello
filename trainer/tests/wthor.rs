use std::path::PathBuf;

use othello::board::{Board, final_score, is_terminal, legal_moves};
use othello_trainer::check;
use othello_trainer::wthor::{self, Reject};

const GAME: [u8; 59] = [
    56, 64, 73, 66, 43, 34, 46, 36, 33, 65, 67, 22, 11, 75, 24, 74, 37, 42, 51, 53, 57, 48, 35, 23,
    86, 62, 76, 87, 71, 77, 88, 72, 32, 28, 14, 52, 61, 31, 82, 84, 25, 47, 26, 27, 38, 21, 78, 58,
    13, 85, 17, 15, 68, 12, 18, 16, 41, 63, 83,
];

const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const WRONG: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn record(moves: &[u8], black: u8, theoretical: u8) -> [u8; 68] {
    let mut record = [0; 68];
    record[6] = black;
    record[7] = theoretical;
    record[8..8 + moves.len()].copy_from_slice(moves);
    record
}

fn archive(records: &[[u8; 68]], year: u16, p3: u8) -> Vec<u8> {
    let mut bytes = vec![0; 16];
    bytes[4..8].copy_from_slice(&(records.len() as u32).to_le_bytes());
    bytes[10..12].copy_from_slice(&year.to_le_bytes());
    bytes[14] = p3;
    for record in records {
        bytes.extend_from_slice(record);
    }
    bytes
}

fn manifest_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("othello-wthor-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn manifest(hash: &str, name: &str) -> String {
    format!("{hash}  {name}\n")
}

#[test]
fn header_fields() {
    let db = wthor::read(&archive(&[record(&GAME, 8, 8)], 2025, 24)).unwrap();
    assert_eq!(db.header.games, 1);
    assert_eq!(db.header.year, 2025);
    assert_eq!(db.header.p3, 24);
    let db = wthor::read(&archive(&[record(&GAME, 8, 8)], 2025, 0)).unwrap();
    assert_eq!(db.header.p3, 22);
}

#[test]
fn header_rejections() {
    let mut p1 = archive(&[record(&GAME, 8, 8)], 2025, 24);
    p1[12] = 10;
    assert!(wthor::read(&p1).is_err());
    let mut p2 = archive(&[record(&GAME, 8, 8)], 2025, 24);
    p2[13] = 1;
    assert!(wthor::read(&p2).is_err());
}

#[test]
fn wrong_length() {
    let bytes = archive(&[record(&GAME, 8, 8)], 2025, 24);
    assert!(wthor::read(&bytes[..bytes.len() - 1]).is_err());
    let mut header = vec![0; 16];
    header[4..8].copy_from_slice(&1u32.to_le_bytes());
    assert!(wthor::read(&header).is_err());
}

#[test]
fn move_decoding() {
    let db = wthor::read(&archive(&[record(&[11, 88, 18, 81, 56], 0, 0)], 2025, 24)).unwrap();
    assert_eq!(db.games[0].moves, vec![0, 63, 7, 56, 37]);
}

#[test]
fn known_game() {
    let db = wthor::read(&archive(&[record(&GAME, 56, 56)], 2025, 24)).unwrap();
    let replay = wthor::replay(&db.games[0]).unwrap();
    assert_eq!(replay.passes, 4);
    assert_eq!(final_score(replay.final_board), -48);
    assert!(!replay.final_black);
    assert_eq!(replay.final_board.opp.count_ones(), 55);
    assert_eq!(replay.positions.len(), GAME.len());
    assert_eq!(replay.black.len(), replay.positions.len());
    assert_eq!(replay.positions[0], Board::START);
}

#[test]
fn illegal_move() {
    let db = wthor::read(&archive(&[record(&[11], 0, 0)], 2025, 24)).unwrap();
    assert_eq!(wthor::replay(&db.games[0]), Err(Reject::Illegal { ply: 0 }));
}

#[test]
fn invalid_byte() {
    for byte in [19, 99] {
        let db = wthor::read(&archive(&[record(&[byte], 0, 0)], 2025, 24)).unwrap();
        assert_eq!(wthor::replay(&db.games[0]), Err(Reject::Illegal { ply: 0 }));
    }
}

#[test]
fn incomplete_game() {
    let db = wthor::read(&archive(&[record(&GAME[..10], 0, 0)], 2025, 24)).unwrap();
    assert_eq!(wthor::replay(&db.games[0]), Err(Reject::Incomplete));
}

#[test]
fn positions_playable() {
    let db = wthor::read(&archive(&[record(&GAME, 56, 56)], 2025, 24)).unwrap();
    let replay = wthor::replay(&db.games[0]).unwrap();
    for position in &replay.positions {
        assert!(!is_terminal(*position));
        assert_ne!(legal_moves(position.me, position.opp), 0);
    }
}

#[test]
fn summary_counts() {
    let records = [
        record(&GAME, 56, 56),
        record(&GAME, 55, 56),
        record(&GAME, 56, 20),
        record(&[11], 0, 0),
        record(&GAME[..10], 0, 0),
        record(&[19], 0, 0),
    ];
    let db = wthor::read(&archive(&records, 2025, 24)).unwrap();
    let summary = check::summarise(&db);
    assert_eq!(summary.games_read, 6);
    assert_eq!(summary.replayed, 3);
    assert_eq!(summary.rejected_illegal, 2);
    assert_eq!(summary.rejected_incomplete, 1);
    assert_eq!(summary.real_agree, 2);
    assert_eq!(summary.theoretical_equal, 1);
    assert_eq!(summary.p3_values.get(&24), Some(&1));
}

#[test]
fn manifest_mismatch() {
    let path = std::env::temp_dir().join(format!("othello-wthor-{}", std::process::id()));
    std::fs::write(&path, b"abc").unwrap();
    assert!(check::verify_file(&path, ABC).is_ok());
    assert!(check::verify_file(&path, WRONG).is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn manifest_entries() {
    let dir = manifest_dir("entries");
    std::fs::write(dir.join("good.wtb"), b"abc").unwrap();
    std::fs::write(dir.join("bad.wtb"), b"def").unwrap();
    let bad = manifest(ABC, "good.wtb") + &manifest(WRONG, "bad.wtb");
    assert!(check::verify_manifest(&dir, &bad).is_err());
    std::fs::remove_file(dir.join("bad.wtb")).unwrap();
    assert!(check::verify_manifest(&dir, &manifest(ABC, "good.wtb")).is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_missing() {
    let dir = manifest_dir("missing");
    assert!(check::verify_manifest(&dir, &manifest(ABC, "missing.wtb")).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_unlisted() {
    let dir = manifest_dir("unlisted");
    std::fs::write(dir.join("good.wtb"), b"abc").unwrap();
    std::fs::write(dir.join("extra.wtb"), b"def").unwrap();
    assert!(check::verify_manifest(&dir, &manifest(ABC, "good.wtb")).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
