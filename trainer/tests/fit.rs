use std::path::PathBuf;

use othello::board::Board;
use othello::pattern;
use othello::rng::Rng;
use othello_tools::sym;
use othello_trainer::fit::{self, Example, Fit, Options, Report, Stage, StageReport};
use othello_trainer::wthor::Game;

fn options() -> Options {
    Options {
        step: 0.05,
        floor: 1.0,
        patience: 1000,
        max_passes: 2000,
    }
}

#[test]
fn reference() {
    let examples = [
        Example {
            features: vec![0, 0],
            label: 4.0,
        },
        Example {
            features: vec![1],
            label: 2.0,
        },
        Example {
            features: vec![0],
            label: 1.0,
        },
        Example {
            features: vec![1, 1],
            label: 6.0,
        },
    ];
    let fit = fit::fit(3, &examples, &examples, &options());
    assert!((fit.values[0] - 1.8).abs() < 0.02, "{:?}", fit.values);
    assert!((fit.values[1] - 2.8).abs() < 0.02, "{:?}", fit.values);
    assert_eq!(fit.values[2], 0.0);
}

#[test]
fn shared() {
    let examples = [
        Example {
            features: vec![0, 0, 1],
            label: 5.0,
        },
        Example {
            features: vec![1],
            label: 1.0,
        },
        Example {
            features: vec![0],
            label: 2.0,
        },
    ];
    let fit = fit::fit(2, &examples, &examples, &options());
    assert!((fit.values[0] - 2.0).abs() < 0.02, "{:?}", fit.values);
    assert!((fit.values[1] - 1.0).abs() < 0.02, "{:?}", fit.values);
}

#[test]
fn damping() {
    let mut examples = vec![
        Example {
            features: vec![0],
            label: 10.0,
        };
        3
    ];
    examples.extend(vec![
        Example {
            features: vec![1],
            label: 10.0,
        };
        500
    ]);
    let fit = fit::fit(3, &examples, &examples, &Options::default());
    let options = Options::default();
    let base: f32 = 1.0 - 3.0 * options.step / options.floor;
    let bound = 10.0 * (1.0 - base.powi(options.max_passes as i32));
    assert!(bound <= 3.0, "{bound}");
    assert!(fit.values[1] > 9.0, "{:?}", fit.values);
    assert!(
        fit.values[0] <= bound + 0.001,
        "{} > {bound}",
        fit.values[0]
    );
    assert_eq!(fit.values[2], 0.0);
}

#[test]
fn quantisation() {
    assert!(fit::quantise(&[f32::NAN]).is_err());
    assert!(fit::quantise(&[f32::INFINITY]).is_err());
    assert!(fit::quantise(&[327.68]).is_err());
    assert!(fit::quantise(&[-327.68]).is_err());
    assert!(fit::quantise(&[327.67]).is_ok());
    assert!(fit::quantise(&[-327.67]).is_ok());
    assert!(fit::quantise(&[f32::NAN]).unwrap_err().contains('0'));
    assert_eq!(fit::quantise(&[1.23, -4.56]).unwrap(), vec![123, -456]);
    let values: Vec<f32> = (0..46).map(|index| (index as f32 - 23.0) / 7.0).collect();
    let features: Vec<u32> = (0..46).collect();
    let rounded: Vec<f32> = fit::quantise(&values)
        .unwrap()
        .iter()
        .map(|value| *value as f32 / 100.0)
        .collect();
    let difference = (fit::predict(&values, &features) - fit::predict(&rounded, &features)).abs();
    assert!(difference <= 0.23, "{difference}");
}

#[test]
fn tied_asset() {
    let stages: Vec<Vec<f32>> = (0..12)
        .map(|stage| {
            (0..pattern::TABLE_SLOTS)
                .map(|index| ((index + stage * 7) % 50) as f32 / 100.0)
                .collect()
        })
        .collect();
    let bytes = fit::asset(&stages).unwrap();
    assert_eq!(bytes.len(), 12 * pattern::TABLE_SLOTS * 2);
    for stage in 0..12 {
        for index in 0..pattern::TABLE_SLOTS {
            let canonical = fit::canonical(index as u32) as usize;
            let expected = (stages[stage][canonical] * 100.0).round() as i16;
            let offset = (stage * pattern::TABLE_SLOTS + index) * 2;
            let actual = i16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
            assert_eq!(actual, expected, "stage {stage} index {index}");
        }
    }
}

#[test]
fn canonical_pairs() {
    assert_eq!(fit::canonical(46_459), 35_577);
    assert_eq!(fit::canonical(59_049 + 27), 59_049 + 3);
    assert_eq!(fit::canonical(137_781 + 2_187), 137_781 + 1);
    assert_eq!(fit::canonical(157_464 + 2_187), 157_464 + 1);
    for index in 78_732..78_732 + 59_049 {
        assert_eq!(fit::canonical(index), index);
    }
}

#[test]
fn canonical_properties() {
    let mut offset = 0u32;
    for squares in pattern::FAMILIES {
        let end = offset + 3u32.pow(squares.len() as u32);
        for index in offset..end {
            let canonical = fit::canonical(index);
            assert!(canonical >= offset && canonical < end, "{index}");
            assert!(canonical <= index, "{index}");
            assert_eq!(fit::canonical(canonical), canonical, "{index}");
        }
        offset = end;
    }
}

#[test]
fn canonical_symmetry() {
    for board in boards() {
        let expected = multiset(board);
        for kind in 0..8 {
            assert_eq!(multiset(sym::transform(board, kind)), expected, "{kind}");
        }
    }
    assert!(boards().iter().any(|&board| {
        let raw = raw_multiset(board);
        (1..8).any(|kind| raw_multiset(sym::transform(board, kind)) != raw)
    }));
}

#[test]
fn early_stop() {
    let train: Vec<Example> = (0..50)
        .map(|_| Example {
            features: vec![0],
            label: 10.0,
        })
        .collect();
    let held: Vec<Example> = (0..50)
        .map(|_| Example {
            features: vec![0],
            label: 2.0,
        })
        .collect();
    let options = Options {
        step: 0.01,
        floor: 1.0,
        patience: 3,
        max_passes: 1000,
    };
    let fit: Fit = fit::fit(1, &train, &held, &options);
    let best = fit
        .curve
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.1.partial_cmp(&b.1.1).unwrap())
        .unwrap()
        .0;
    assert_eq!(fit.best, best);
    assert_eq!(fit.curve.len(), best + options.patience + 1);
    assert!(fit.curve.len() < options.max_passes);
    assert!((fit.values[0] - 2.0).abs() < 0.5, "{:?}", fit.values);
}

#[test]
fn stage_restart() {
    let dir = temp_dir("stage");
    let path = dir.join("stage_00");
    let stage = Stage {
        values: vec![1.0, 2.0, 3.0],
        counts: vec![4, 5, 6],
        curve: vec![(0.5, 0.6)],
    };
    fit::save_stage(&path, "header", &stage).unwrap();
    assert_eq!(
        fit::load_stage(&path, "header").unwrap(),
        Some(stage.clone())
    );
    assert!(fit::load_stage(&path, "other").unwrap().is_none());

    std::fs::write(format!("{}.tmp", path.display()), b"partial").unwrap();
    assert_eq!(
        fit::load_stage(&path, "header").unwrap(),
        Some(stage.clone())
    );
    std::fs::remove_file(&path).unwrap();
    assert!(fit::load_stage(&path, "header").unwrap().is_none());

    fit::save_stage(&path, "header", &stage).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] = b'x';
    std::fs::write(&path, bytes).unwrap();
    assert!(fit::load_stage(&path, "header").unwrap().is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn inspect_output() {
    let mut stage = Stage {
        values: vec![0.0; pattern::TABLE_SLOTS],
        counts: vec![0; pattern::TABLE_SLOTS],
        curve: Vec::new(),
    };
    stage.values[0] = -2.0;
    stage.counts[0] = 10;
    stage.values[1] = 3.0;
    stage.counts[1] = 5;
    stage.values[2] = 1.0;
    stage.counts[2] = 20;
    let text = fit::inspect(&stage, 0, 2);
    let high = text.find("+2.57").unwrap_or_else(|| panic!("{text}"));
    let mid = text.find("+0.57").unwrap_or_else(|| panic!("{text}"));
    let low = text.find("-2.43").unwrap_or_else(|| panic!("{text}"));
    assert!(high < mid && mid < low, "{text}");
    assert!(!text.contains("count 0"), "{text}");
}

#[test]
fn report_fields() {
    let report = Report {
        inputs: vec![
            ("trainer".into(), "abc".into()),
            ("journal".into(), "def".into()),
            ("toolchain".into(), "rustc 1.99".into()),
        ],
        constants: options(),
        stages: vec![StageReport {
            stage: 0,
            curve: vec![(1.0, 1.1), (0.5, 0.6)],
            held: 0.6,
            rounded: 0.61,
            zero: 1.2,
            classic: 0.9,
            correlation: 0.5,
            nonzero: 0.25,
            largest: 3.0,
        }],
        label_quality: 0.4,
        asset_sha: "0123".into(),
    };
    let text = fit::report(&report);
    for needle in [
        "trainer",
        "journal",
        "toolchain",
        "stage 0",
        "step",
        "1.10",
        "0.60",
        "asset",
        "classic",
        "label",
    ] {
        assert!(text.contains(needle), "{needle}\n{text}");
    }
}

#[test]
fn split() {
    let game = |moves: Vec<u8>, real: u8, theoretical: u8| Game {
        real,
        theoretical,
        moves,
    };
    let first = game(vec![11, 12, 13], 0, 0);
    let second = game(vec![11, 12, 13], 56, 24);
    assert_eq!(fit::held_out(&first), fit::held_out(&second));
    let held = (0u16..300)
        .filter(|&seed| fit::held_out(&game(seed.to_le_bytes().to_vec(), 0, 0)))
        .count();
    assert!((5..=25).contains(&held), "{held}");
}

#[test]
fn planted() {
    let table = pattern::TABLE_SLOTS;
    let planted: Vec<f32> = (0..table as u32)
        .map(|index| {
            let family = family_of(index);
            if family != 9 && family != 10 {
                return 0.0;
            }
            let canonical = fit::canonical(index);
            ((canonical % 17) as f32 - 8.0) / 8.0
        })
        .collect();
    let mut train = Vec::new();
    let mut held = Vec::new();
    for family in [9usize, 10] {
        let configs = 3u32.pow(pattern::FAMILIES[family].len() as u32);
        for config in 0..configs {
            for repeat in 0..6 {
                let example = example(family, config, repeat, &planted);
                if repeat == 5 {
                    held.push(example);
                } else {
                    train.push(example);
                }
            }
        }
    }
    let options = Options {
        step: 0.05,
        floor: 1.0,
        patience: 100,
        max_passes: 500,
    };
    let fit = fit::fit(table, &train, &held, &options);
    let error: f32 = held
        .iter()
        .map(|example| (fit::predict(&fit.values, &example.features) - example.label).abs())
        .sum::<f32>()
        / held.len() as f32;
    let zero: f32 = held.iter().map(|example| example.label.abs()).sum::<f32>() / held.len() as f32;
    assert!(error < 0.05, "{error}");
    assert!(error < 0.05 * zero, "{error} vs {zero}");
}

fn family_of(index: u32) -> usize {
    let mut offset = 0u32;
    for (family, squares) in pattern::FAMILIES.iter().enumerate() {
        offset += 3u32.pow(squares.len() as u32);
        if index < offset {
            return family;
        }
    }
    11
}

fn example(family: usize, config: u32, repeat: u64, planted: &[f32]) -> Example {
    let squares = pattern::FAMILIES[family];
    let mut rng = Rng::new(((family as u64) << 32) ^ ((config as u64) << 8) ^ repeat);
    let mut me = rng.next_u64();
    let mut opp = rng.next_u64() & !me;
    for (position, &square) in squares.iter().enumerate() {
        let bit = 1u64 << square;
        me &= !bit;
        opp &= !bit;
        let digit = (config / 3u32.pow(position as u32)) % 3;
        if digit == 1 {
            me |= bit;
        } else if digit == 2 {
            opp |= bit;
        }
    }
    let targets: Vec<usize> = pattern::instances()
        .iter()
        .enumerate()
        .filter(|(_, instance)| instance.family == 9 || instance.family == 10)
        .map(|(index, _)| index)
        .collect();
    let features: Vec<u32> = pattern::features(Board { me, opp })
        .iter()
        .enumerate()
        .filter(|(index, _)| targets.contains(index))
        .map(|(_, &feature)| fit::canonical(feature))
        .collect();
    let label: f32 = features
        .iter()
        .map(|&feature| planted[feature as usize])
        .sum();
    Example { features, label }
}

fn multiset(board: Board) -> Vec<u32> {
    let mut values: Vec<u32> = pattern::features(board)
        .iter()
        .map(|&feature| fit::canonical(feature))
        .collect();
    values.sort();
    values
}

fn raw_multiset(board: Board) -> Vec<u32> {
    let mut values = pattern::features(board).to_vec();
    values.sort();
    values
}

fn boards() -> Vec<Board> {
    let mut boards = vec![Board::START];
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/corpus.txt"))
        .unwrap();
    boards.extend(
        text.lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let me = u64::from_str_radix(parts.next()?, 16).ok()?;
                let opp = u64::from_str_radix(parts.next()?, 16).ok()?;
                Some(Board { me, opp })
            })
            .filter(|board| (board.me | board.opp).count_ones() >= 40)
            .take(3),
    );
    boards
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("othello-fit-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
