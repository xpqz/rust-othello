use othello::rng::Rng;

pub const RESAMPLES: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interval {
    Insufficient,
    Uninformative { mean: f64, low: f64, high: f64 },
    Bootstrap { mean: f64, low: f64, high: f64 },
}

impl Interval {
    pub fn informative(&self) -> bool {
        matches!(self, Interval::Bootstrap { .. })
    }

    pub fn bounds(&self) -> Option<(f64, f64)> {
        match *self {
            Interval::Insufficient => None,
            Interval::Uninformative { low, high, .. } | Interval::Bootstrap { low, high, .. } => {
                Some((low, high))
            }
        }
    }

    fn mean(&self) -> Option<f64> {
        match *self {
            Interval::Insufficient => None,
            Interval::Uninformative { mean, .. } | Interval::Bootstrap { mean, .. } => Some(mean),
        }
    }

    pub fn elo_range(&self) -> Option<(f64, f64)> {
        let (low, high) = self.bounds()?;
        (low > 0.0 && high < 1.0).then(|| (elo_from_score(low), elo_from_score(high)))
    }
}

pub fn bootstrap_ci(scores: &[f64], resamples: usize, seed: u64) -> Interval {
    let n = scores.len();
    if n == 0 {
        return Interval::Insufficient;
    }
    let mean = scores.iter().sum::<f64>() / n as f64;
    let variance = scores
        .iter()
        .map(|score| (score - mean).powi(2))
        .sum::<f64>()
        / n as f64;
    if n < 10 || variance == 0.0 || resamples == 0 {
        let half = (40.0f64.ln() / (2.0 * n as f64)).sqrt();
        return Interval::Uninformative {
            mean,
            low: (mean - half).max(0.0),
            high: (mean + half).min(1.0),
        };
    }
    let mut rng = Rng::new(seed);
    let mut means: Vec<f64> = (0..resamples)
        .map(|_| {
            (0..n)
                .map(|_| scores[(rng.next_u64() % n as u64) as usize])
                .sum::<f64>()
                / n as f64
        })
        .collect();
    means.sort_by(|left, right| left.partial_cmp(right).unwrap());
    Interval::Bootstrap {
        mean,
        low: percentile(&means, 0.025),
        high: percentile(&means, 0.975),
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

pub fn elo_from_score(score: f64) -> f64 {
    400.0 * (score / (1.0 - score)).log10()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub pairs: usize,
    pub games: usize,
    pub wins: usize,
    pub draws: usize,
    pub losses: usize,
    pub mean: f64,
    pub interval: Interval,
    pub elo: Option<(f64, f64)>,
}

pub fn report(games: &[(f64, f64)]) -> Report {
    report_with(games, RESAMPLES, 0)
}

pub fn report_with(games: &[(f64, f64)], resamples: usize, seed: u64) -> Report {
    let scores: Vec<f64> = games
        .iter()
        .map(|&(first, second)| (first + second) / 2.0)
        .collect();
    let interval = bootstrap_ci(&scores, resamples, seed);
    let (mut wins, mut draws, mut losses) = (0, 0, 0);
    for &(first, second) in games {
        for score in [first, second] {
            if score == 1.0 {
                wins += 1;
            } else if score == 0.5 {
                draws += 1;
            } else {
                losses += 1;
            }
        }
    }
    Report {
        pairs: games.len(),
        games: games.len() * 2,
        wins,
        draws,
        losses,
        mean: interval.mean().unwrap_or(f64::NAN),
        elo: interval.elo_range(),
        interval,
    }
}
