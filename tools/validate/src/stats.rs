//! Error distributions, and an interval for an agreement rate.

use serde_json::{Value, json};

/// The distribution of signed errors: a bias shows in `mean`, and the tail in
/// `p99` and `max`, which one RMSE would hide. Percentiles are nearest rank
/// over the absolute errors. Empty when `errors` is.
pub fn distribution(errors: &[f64]) -> Value {
    let n = errors.len();
    if n == 0 {
        return json!({"n": 0});
    }
    let mut abs: Vec<f64> = errors.iter().map(|e| e.abs()).collect();
    abs.sort_by(f64::total_cmp);
    let rank = |p: f64| abs[((p * n as f64).ceil() as usize).clamp(1, n) - 1];
    json!({
        "n": n,
        "mean": errors.iter().sum::<f64>() / n as f64,
        "rmse": (errors.iter().map(|e| e * e).sum::<f64>() / n as f64).sqrt(),
        "p50": rank(0.50),
        "p95": rank(0.95),
        "p99": rank(0.99),
        "max": abs[n - 1],
    })
}

/// The 95 % Wilson interval of `hits` out of `n`, which stays honest at a
/// rate near 1, where the normal approximation reaches past it.
pub fn wilson(hits: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    const Z: f64 = 1.959_964;
    let (n, p) = (n as f64, hits as f64 / n as f64);
    let d = 1.0 + Z * Z / n;
    let c = (p + Z * Z / (2.0 * n)) / d;
    let h = Z * (p * (1.0 - p) / n + Z * Z / (4.0 * n * n)).sqrt() / d;
    ((c - h).max(0.0), (c + h).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_reads_bias_and_tail() {
        let d = distribution(&[1.0, -1.0, 1.0, -3.0]);
        assert_eq!(d["mean"], -0.5);
        assert_eq!(d["p50"], 1.0);
        assert_eq!(d["max"], 3.0);
        assert_eq!(d["rmse"], 3.0_f64.sqrt());
    }

    #[test]
    fn wilson_stays_inside_the_unit_interval() {
        let (lo, hi) = wilson(300, 300);
        assert!(lo > 0.987 && lo < 0.99, "{lo}");
        assert_eq!(hi, 1.0);
        assert_eq!(wilson(0, 0), (0.0, 1.0));
    }
}
