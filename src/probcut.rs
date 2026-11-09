//! ProbCut parameters and threshold arithmetic.
//!
//! Linear and StageTable use integer centidiscs. EdaxStyle is fixed-point in
//! thousandths for all six coefficients; `s` uses the same scale, and the
//! final sigma rounds up.

use crate::score::EVAL_BOUND;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigmaModel {
    Linear {
        s0: i32,
        gap: i32,
        empty: i32,
    },
    EdaxStyle {
        a_milli: i32,
        b_milli: i32,
        c_milli: i32,
        q2_milli: i32,
        q1_milli: i32,
        q0_milli: i32,
    },
    /// Six buckets for empties 18-23, 24-29, ..., 48+; lower uses bucket 0.
    StageTable {
        buckets: [(i32, i32); 6],
    },
}

#[derive(Clone, Copy, Debug)]
pub struct ProbCut {
    t_centi: u32,
    a_milli: i32,
    b: i32,
    sigma: SigmaModel,
    depths: (u32, u32),
    empties: (u32, u32),
}

pub fn shallow_depth(depth: u32) -> u32 {
    2 * (depth / 4) + (depth & 1)
}

fn bucket(empties: u32) -> usize {
    (empties.saturating_sub(18) / 6) as usize
}

fn ceil_div(n: i64, d: i64) -> i64 {
    let q = n / d;
    if n > 0 && n % d != 0 { q + 1 } else { q }
}

impl ProbCut {
    pub fn new(
        t_centi: u32,
        a_milli: i32,
        b: i32,
        sigma: SigmaModel,
        depths: (u32, u32),
        empties: (u32, u32),
    ) -> Result<Self, String> {
        if !(50..=400).contains(&t_centi) {
            return Err("t_centi out of range".into());
        }
        if !(500..=1500).contains(&a_milli) {
            return Err("a_milli out of range".into());
        }
        if depths.0 < 3 || depths.0 > depths.1 || depths.1 > 64 {
            return Err("invalid depth range".into());
        }
        if empties.0 > empties.1 || empties.1 > 64 {
            return Err("invalid empty range".into());
        }
        let pc = Self {
            t_centi,
            a_milli,
            b,
            sigma,
            depths,
            empties,
        };
        for depth in depths.0..=depths.1 {
            for empties in empties.0..=empties.1 {
                if pc.sigma_raw(depth, empties) < 1 {
                    return Err("sigma below 1 in supported domain".into());
                }
            }
        }
        Ok(pc)
    }

    pub fn supports(&self, depth: u32, empties: u32) -> bool {
        (self.depths.0..=self.depths.1).contains(&depth)
            && (self.empties.0..=self.empties.1).contains(&empties)
    }

    fn sigma_raw(&self, depth: u32, empties: u32) -> i64 {
        let gap = i64::from(depth - shallow_depth(depth));
        match self.sigma {
            SigmaModel::Linear { s0, gap: g, empty } => i64::from(s0)
                .saturating_add(i64::from(g).saturating_mul(gap))
                .saturating_add(i64::from(empty).saturating_mul(i64::from(empties))),
            SigmaModel::EdaxStyle {
                a_milli,
                b_milli,
                c_milli,
                q2_milli,
                q1_milli,
                q0_milli,
            } => {
                let s = i128::from(a_milli) * i128::from(empties)
                    + i128::from(b_milli) * i128::from(depth)
                    + i128::from(c_milli) * i128::from(shallow_depth(depth));
                let n = i128::from(q2_milli) * s * s
                    + i128::from(q1_milli) * s * 1000
                    + i128::from(q0_milli) * 1_000_000;
                let sigma = -((-n).div_euclid(1_000_000_000));
                sigma.clamp(i64::MIN as i128, i64::MAX as i128) as i64
            }
            SigmaModel::StageTable { buckets } => {
                let (s0, g) = buckets[bucket(empties).min(buckets.len() - 1)];
                i64::from(s0).saturating_add(i64::from(g).saturating_mul(gap))
            }
        }
    }

    pub fn sigma(&self, depth: u32, empties: u32) -> i64 {
        self.sigma_raw(depth, empties).max(1)
    }

    fn margin(&self, depth: u32, empties: u32) -> i64 {
        let sigma = self
            .sigma(depth, empties)
            .saturating_mul(i64::from(self.t_centi));
        ceil_div(sigma, 100)
    }

    pub fn high(&self, beta: i32, depth: u32, empties: u32) -> Option<i64> {
        let margin = self.margin(depth, empties);
        let n = i64::from(beta)
            .saturating_add(margin)
            .saturating_sub(i64::from(self.b));
        let high = ceil_div(n.saturating_mul(1000), i64::from(self.a_milli));
        (high <= i64::from(EVAL_BOUND)).then_some(high)
    }

    pub fn low(&self, alpha: i32, depth: u32, empties: u32) -> Option<i64> {
        let margin = self.margin(depth, empties);
        let n = i64::from(alpha)
            .saturating_sub(margin)
            .saturating_sub(i64::from(self.b));
        let low = n.saturating_mul(1000).div_euclid(i64::from(self.a_milli));
        (low >= -i64::from(EVAL_BOUND)).then_some(low)
    }
}
