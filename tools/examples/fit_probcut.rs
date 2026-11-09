fn main() {}

#[cfg(test)]
mod tests {
    use othello::probcut::{ProbCut, SigmaModel};
    use std::path::Path;

    #[test]
    fn ols_recovers_synthetic_fit() {
        let (a, b, sigma) = ols(&synthetic(0x5EED, 0.9, 20.0, 100.0, 4000));
        assert!((a - 0.9).abs() < 0.01, "{a}");
        assert!((b - 20.0).abs() < 5.0, "{b}");
        assert!((sigma - 100.0).abs() < 5.0, "{sigma}");
    }

    #[test]
    fn decision_counts_hand_table() {
        let pc = ProbCut::new(
            150,
            1000,
            0,
            SigmaModel::Linear {
                s0: 100,
                gap: 0,
                empty: 0,
            },
            (3, 12),
            (18, 52),
        )
        .unwrap();
        let rows = [
            (300, 90, 0, 100),
            (-300, -90, -100, 0),
            (300, 110, 0, 100),
            (-300, -110, -100, 0),
            (250, 100, 0, 100),
            (-250, -100, -100, 0),
        ];
        let counts = decision_counts(&rows, &pc, 8, 30);
        assert_eq!(counts.high_fired, 3);
        assert_eq!(counts.false_high, 1);
        assert_eq!(counts.low_fired, 3);
        assert_eq!(counts.false_low, 1);
    }

    #[test]
    fn clopper_pearson_upper_bounds() {
        assert!((cp_upper(0, 200) - 0.0149).abs() < 0.001);
        assert!((cp_upper(0, 75) - 0.0389).abs() < 0.002);
        assert!((cp_upper(10, 200) - 0.08334).abs() < 0.001);
        assert!((cp_upper(20, 200) - 0.14199).abs() < 0.001);
        assert!((cp_upper(5, 75) - 0.13507).abs() < 0.001);
    }

    #[test]
    fn split_is_grouped_and_stable() {
        let games: Vec<u64> = (0..100).collect();
        let (fit, held) = split_games(&games);
        let fit_set: std::collections::BTreeSet<_> = fit.iter().copied().collect();
        let held_set: std::collections::BTreeSet<_> = held.iter().copied().collect();
        assert!(fit_set.is_disjoint(&held_set));
        assert_eq!(fit_set.len() + held_set.len(), 100);
        assert!((fit.len() as f64 / 100.0 - 0.7).abs() < 0.05);
        assert_eq!(split_games(&games), (fit, held));
    }

    #[test]
    fn dedup_keeps_one_position_per_key() {
        let rows = vec![
            Position {
                key: 1,
                game: 1,
                window: None,
            },
            Position {
                key: 1,
                game: 2,
                window: None,
            },
            Position {
                key: 2,
                game: 1,
                window: Some((-100, 100)),
            },
        ];
        let unique = dedup_positions(rows);
        assert_eq!(unique.len(), 2);
        assert_eq!(unique.iter().find(|row| row.key == 1).unwrap().game, 1);
    }

    #[test]
    fn pipeline_interrupt_resume_and_header() {
        use std::fs;
        use std::os::unix::process::ExitStatusExt;
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        let unique = |name: &str| {
            std::env::temp_dir().join(format!(
                "fit_probcut_{}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                name
            ))
        };
        let example = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("examples/fit_probcut");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/fit_probcut.rs");
        assert!(
            example.metadata().unwrap().modified().unwrap()
                >= source.metadata().unwrap().modified().unwrap(),
            "fit_probcut example binary is missing or stale; run cargo build --release --example fit_probcut"
        );
        let run = |dir: &Path, seed: u64| {
            Command::new(&example)
                .args([
                    "--dir",
                    dir.to_str().unwrap(),
                    "--games",
                    "4",
                    "--positions",
                    "8",
                    "--depth",
                    "4",
                    "--seed",
                    &seed.to_string(),
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .unwrap()
        };
        let reference = unique("reference");
        let resumed = unique("resumed");
        fs::create_dir_all(&reference).unwrap();
        fs::create_dir_all(&resumed).unwrap();
        assert!(run(&reference, 7).status.success());
        let mut child = Command::new(&example)
            .args([
                "--dir",
                resumed.to_str().unwrap(),
                "--games",
                "4",
                "--positions",
                "8",
                "--depth",
                "4",
                "--seed",
                "7",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let measure = resumed.join("measure.journal");
        let start = Instant::now();
        loop {
            let lines = fs::read_to_string(&measure).map_or(0, |text| text.lines().count());
            if lines > 2 {
                child.kill().unwrap();
                break;
            }
            if child.try_wait().unwrap().is_some() || start.elapsed() > Duration::from_secs(30) {
                panic!("pipeline finished before interruption");
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(child.wait().unwrap().signal(), Some(9));
        assert!(run(&resumed, 7).status.success());
        let reference_fit = fs::read(reference.join("fit.log")).unwrap();
        assert_eq!(fs::read(resumed.join("fit.log")).unwrap(), reference_fit);

        let text = fs::read_to_string(&measure).unwrap();
        let mut cut = text[..text.len().saturating_sub(4)].to_string();
        cut.push_str("torn");
        fs::write(&measure, cut).unwrap();
        assert!(run(&resumed, 7).status.success());
        assert_eq!(fs::read(resumed.join("fit.log")).unwrap(), reference_fit);

        let mismatch = run(&resumed, 8);
        assert!(!mismatch.status.success());
        assert!(String::from_utf8_lossy(&mismatch.stderr).contains("header"));
        let depth = Command::new(&example)
            .args([
                "--dir",
                resumed.to_str().unwrap(),
                "--games",
                "4",
                "--positions",
                "40",
                "--depth",
                "5",
                "--unit-delay-ms",
                "20",
                "--seed",
                "7",
            ])
            .output()
            .unwrap();
        assert!(!depth.status.success());
        assert!(String::from_utf8_lossy(&depth.stderr).contains("header"));
        fs::remove_dir_all(reference).unwrap();
        fs::remove_dir_all(resumed).unwrap();
    }

    struct Position {
        key: u64,
        game: u64,
        window: Option<(i32, i32)>,
    }

    fn synthetic(seed: u64, a: f64, b: f64, sigma: f64, count: usize) -> Vec<(f64, f64)> {
        let mut rng = othello::rng::Rng::new(seed);
        (0..count)
            .map(|_| {
                let x = (rng.next_u64() % 12_801) as f64 - 6_400.0;
                let noise = normal(&mut rng) * sigma;
                (x, a * x + b + noise)
            })
            .collect()
    }

    fn normal(rng: &mut othello::rng::Rng) -> f64 {
        let u1 = (rng.next_u64() as f64 + 1.0) / (u64::MAX as f64 + 2.0);
        let u2 = (rng.next_u64() as f64 + 1.0) / (u64::MAX as f64 + 2.0);
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}
