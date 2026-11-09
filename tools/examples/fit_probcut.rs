fn main() {}

#[cfg(test)]
mod tests {
    use othello::probcut::{ProbCut, SigmaModel};

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
    }

    #[test]
    fn split_is_grouped_and_stable() {
        let games: Vec<u64> = (0..100).collect();
        let (fit, held) = split_games(&games);
        let fit_set: std::collections::BTreeSet<_> = fit.iter().copied().collect();
        let held_set: std::collections::BTreeSet<_> = held.iter().copied().collect();
        assert!(fit_set.is_disjoint(&held_set));
        assert_eq!(fit_set.len() + held_set.len(), 100);
        assert_eq!((fit.len() as f64 / 100.0 - 0.7).abs() < 0.05, true);
        assert_eq!(split_games(&games), (fit, held));
    }

    #[test]
    fn dedup_keeps_one_row_per_key() {
        let rows = vec![
            Row {
                key: 1,
                game: 1,
                depth: 8,
                value: 10,
            },
            Row {
                key: 1,
                game: 2,
                depth: 8,
                value: 20,
            },
            Row {
                key: 2,
                game: 1,
                depth: 8,
                value: 30,
            },
        ];
        let unique = dedup_rows(rows);
        assert_eq!(unique.len(), 2);
        assert_eq!(unique.iter().find(|row| row.key == 1).unwrap().value, 10);
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

    #[allow(dead_code)]
    struct Row {
        key: u64,
        game: u64,
        depth: u32,
        value: i32,
    }
}
