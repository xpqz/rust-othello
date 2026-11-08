use othello_tools::stats::{Interval, bootstrap_ci, elo_from_score, report_with};

fn approx(left: f64, right: f64) {
    assert!((left - right).abs() < 1e-9, "{left} vs {right}");
}

#[test]
fn elo_points() {
    approx(elo_from_score(0.5), 0.0);
    approx(elo_from_score(0.75), 400.0 * 3.0f64.log10());
    approx(elo_from_score(0.25), -400.0 * 3.0f64.log10());
    assert!(elo_from_score(1.0).is_infinite());
    assert!(elo_from_score(0.0).is_infinite());
}

#[test]
fn bootstrap_deterministic() {
    let scores = [
        1.0, 0.0, 0.5, 0.75, 0.25, 1.0, 0.0, 0.5, 0.5, 1.0, 0.0, 0.25,
    ];
    let first = bootstrap_ci(&scores, 1_000, 7);
    assert_eq!(first, bootstrap_ci(&scores, 1_000, 7));
    match first {
        Interval::Bootstrap { mean, low, high } => {
            assert!(low <= mean && mean <= high);
            assert!((0.0..=1.0).contains(&low));
            assert!((0.0..=1.0).contains(&high));
            let half = (40.0f64.ln() / (2.0 * scores.len() as f64)).sqrt();
            assert!(high - low < 2.0 * half);
        }
        other => panic!("expected bootstrap: {other:?}"),
    }
}

#[test]
fn degeneracy() {
    assert_eq!(bootstrap_ci(&[], 1_000, 7), Interval::Insufficient);
    match bootstrap_ci(&[0.5], 1_000, 7) {
        Interval::Uninformative { mean, low, high } => {
            approx(mean, 0.5);
            assert_eq!(low, 0.0);
            assert_eq!(high, 1.0);
        }
        other => panic!("expected uninformative: {other:?}"),
    }
    match bootstrap_ci(&[0.5; 20], 1_000, 7) {
        Interval::Uninformative { mean, .. } => approx(mean, 0.5),
        other => panic!("expected uninformative: {other:?}"),
    }
    match bootstrap_ci(&[1.0; 15], 1_000, 7) {
        Interval::Uninformative { mean, high, .. } => {
            approx(mean, 1.0);
            assert_eq!(high, 1.0);
        }
        other => panic!("expected uninformative: {other:?}"),
    }
}

#[test]
fn report_counts() {
    let summary = report_with(&[(1.0, 0.5), (0.0, 0.5), (0.5, 0.5)], 1_000, 7);
    assert_eq!(summary.pairs, 3);
    assert_eq!(summary.games, 6);
    assert_eq!(summary.wins, 1);
    assert_eq!(summary.draws, 4);
    assert_eq!(summary.losses, 1);
    approx(summary.mean, 0.5);
    assert!(!summary.interval.informative());
    assert_eq!(summary.elo, None);
}

#[test]
fn elo_range() {
    assert_eq!(Interval::Insufficient.elo_range(), None);
    assert_eq!(bootstrap_ci(&[0.5], 1_000, 7).elo_range(), None);
    let interval = bootstrap_ci(&[0.5; 10], 1_000, 7);
    let (low, high) = match interval {
        Interval::Uninformative { low, high, .. } => (low, high),
        other => panic!("expected uninformative: {other:?}"),
    };
    let (elo_low, elo_high) = interval.elo_range().unwrap();
    approx(elo_low, elo_from_score(low));
    approx(elo_high, elo_from_score(high));
}
