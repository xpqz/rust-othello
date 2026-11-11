use std::path::PathBuf;

use othello::board::Board;
use othello::pattern;
use othello::rng::Rng;
use othello_trainer::fit::{self, Example, Options, Report, Stage, StageReport};

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
fn damping() {
    let examples = [
        Example {
            features: vec![0],
            label: 1.0,
        },
        Example {
            features: vec![0],
            label: -1.0,
        },
        Example {
            features: vec![0],
            label: 1.0,
        },
    ];
    let fit = fit::fit(2, &examples, &examples, &Options::default());
    assert!(fit.values[0].abs() < 0.5, "{:?}", fit.values);
    assert_eq!(fit.values[1], 0.0);
}

#[test]
fn quantisation() {
    assert!(fit::quantise(&[f32::NAN]).is_err());
    assert!(fit::quantise(&[400.0]).is_err());
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
    let stage: Vec<f32> = (0..pattern::TABLE_SLOTS)
        .map(|index| index as f32 / 100.0)
        .collect();
    let bytes = fit::asset(&vec![stage; 12]).unwrap();
    assert_eq!(bytes.len(), 12 * pattern::TABLE_SLOTS * 2);
    for index in 0..pattern::TABLE_SLOTS {
        let canonical = fit::canonical(index as u32) as usize;
        let value = i16::from_le_bytes([bytes[2 * index], bytes[2 * index + 1]]);
        let tied = i16::from_le_bytes([bytes[2 * canonical], bytes[2 * canonical + 1]]);
        assert_eq!(value, tied, "{index} vs {canonical}");
    }
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
    bytes[last] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();
    assert!(fit::load_stage(&path, "header").unwrap().is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn reproducible() {
    let examples = [
        Example {
            features: vec![0, 1],
            label: 3.0,
        },
        Example {
            features: vec![1],
            label: 1.0,
        },
    ];
    let first = fit::fit(2, &examples, &examples, &options());
    let second = fit::fit(2, &examples, &examples, &options());
    assert_eq!(first.values, second.values);
    assert_eq!(first.curve, second.curve);
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
    for needle in ["relative", "10", "5", "20"] {
        assert!(text.contains(needle), "{needle}\n{text}");
    }
}

#[test]
fn report_fields() {
    let report = Report {
        inputs: vec![
            ("trainer".into(), "abc".into()),
            ("journal".into(), "def".into()),
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
        "trainer", "journal", "stage 0", "step", "asset", "classic", "label",
    ] {
        assert!(text.contains(needle), "{needle}\n{text}");
    }
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
        step: 0.02,
        floor: 5.0,
        patience: 30,
        max_passes: 200,
    };
    let fit = fit::fit(table, &train, &held, &options);
    let error: f32 = held
        .iter()
        .map(|example| (fit::predict(&fit.values, &example.features) - example.label).abs())
        .sum::<f32>()
        / held.len() as f32;
    assert!(error < 1.0, "{error}");
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
    let features: Vec<u32> = pattern::features(Board { me, opp })
        .iter()
        .map(|&feature| fit::canonical(feature))
        .collect();
    let label: f32 = features
        .iter()
        .map(|&feature| planted[feature as usize])
        .sum();
    Example { features, label }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("othello-fit-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
