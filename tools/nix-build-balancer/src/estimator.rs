//! Per-pname duration estimator: log-normal EWMA with a conservative
//! upper-quantile read-out.
//!
//! # Why this estimator
//!
//! Build durations for a given `pname` are positive, right-skewed, and heavy-
//! tailed. Modelling `D` directly as Gaussian is a bad fit (a normal mean is
//! pulled by single bad runs and is symmetric about its centre), but
//! `Y = ln(D)` is much more Gaussian-shaped — multiplicative jitter (cache
//! miss, disk pressure, swap, schedulers) becomes additive in log space.
//! The log-normal is the standard parametric choice for positive, heavy-
//! tailed observables; see Limpert, Stahel & Abbt (2001), *"Log-normal
//! distributions across the sciences and their applications"*, BioScience
//! 51(5):341–352.
//!
//! Once we're in log space we want **recency bias** in the location
//! estimate, because the scheduler needs to follow real changes in build
//! cost (compiler upgrade, dependency rewrite) within a handful of new
//! observations rather than waiting for old samples to age out of a fixed
//! window. The classic recurrence is the exponentially weighted moving
//! average (EWMA):
//!
//! ```text
//!   μ_k = μ_{k-1} + α · (y_k − μ_{k-1})        (α ∈ (0, 1])
//! ```
//!
//! The effective memory is ≈ 1/α observations; the half-life is
//! `ln(0.5)/ln(1−α)` — α=0.2 gives a half-life of ~3.1 samples. See the
//! Wikipedia article "Exponential smoothing" for the standard treatment.
//!
//! This is also the **steady-state form of a 1-D Kalman filter** with the
//! random-walk state model `x_k = x_{k-1} + w_k` (process noise w) and the
//! direct observation `z_k = x_k + v_k` (measurement noise v): the Kalman
//! gain K_k converges to a constant determined by the noise ratio Q/R, and
//! the update reduces to the recurrence above. So the choice between
//! "Kalman filter" and "EWMA" here is one of presentation, not of
//! mathematics — adding a separate trend or per-host state to the Kalman
//! formulation would buy real expressive power, but a vanilla 1-D filter
//! does not.
//!
//! # EW variance via West's recursion
//!
//! Tracking only μ is not enough: the scheduler wants a *conservative*
//! estimate (under-prediction over-admits and prematurely TTLs live
//! builds), which means we need a spread. We use the one-pass
//! weighted-variance recursion from D. H. D. West (1979), *"Updating Mean
//! and Variance Estimates: An Improved Method"*, CACM 22(9):532–535. With
//! the previous-mean residual `δ = y_k − μ_{k-1}`:
//!
//! ```text
//!   μ_k = μ_{k-1} + α · δ
//!   S_k = (1 − α) · (S_{k-1} + α · δ²)
//! ```
//!
//! The unweighted equivalent is Welford's algorithm (Knuth, TAOCP vol. 2
//! §4.2.2). Initial conditions `μ_0 = y_1`, `S_0 = 0` are the natural
//! choice: a single observation has no spread information.
//!
//! `S_k` is a biased-toward-zero estimator of σ² for small k (analogous to
//! the missing Bessel correction in the unweighted case). We don't apply
//! the closed-form bias correction `S / (1 − (1−α)^(2k))`; instead we
//! enforce a **variance floor** (see below), which doubles as a prior on
//! real-world jitter.
//!
//! # Variance floor
//!
//! Even when the observed durations have been near-identical, real builds
//! jitter. We enforce
//!
//! ```text
//!   S_eff = max(S, MIN_LN_VAR)
//! ```
//!
//! with `MIN_LN_VAR = (ln 1.2)² ≈ 0.0332`, corresponding to "assume ±20 %
//! multiplicative spread at 1σ even if the data so far suggests none".
//! That avoids ridiculous over-confidence on a freshly-observed pname.
//!
//! # Read-out
//!
//! The scheduler consumes a single number, so we return the upper-95 %
//! quantile of the fitted log-normal:
//!
//! ```text
//!   D̂ = exp(μ + z · √S_eff)        z ≈ 1.645 = Φ⁻¹(0.95)
//! ```
//!
//! This keeps the *conservative* semantics that the old unweighted-p95
//! estimator gave the scheduler. The number is also written into the
//! `admissions` row as `predicted_ms` and reused by the wall-clock TTL
//! `max(predicted_ms × 2, 60_000)`; both want an upper-tail estimate, not
//! a centre estimate.

/// `(ln 1.2)²`. See module docs — the variance floor representing baseline
/// multiplicative jitter of about ±20 % at 1σ in log-space.
pub const MIN_LN_VAR: f64 = 0.033_188_371_796_572_25;

/// Default Φ⁻¹(0.95). Duplicated as a CLI default so the binary does not
/// need to import this constant.
pub const Z_P95: f64 = 1.645;

/// Default EWMA smoothing factor. Half-life ≈ 3.1 observations.
pub const ALPHA_DEFAULT: f64 = 0.2;

/// One-pass EWMA over `ln(durations)`, returning a conservative log-normal
/// upper-quantile estimate in milliseconds.
///
/// Pre-conditions:
///
/// - `durations` is in chronological order (oldest first). Order matters
///   for EWMA — reversing the slice generally changes the answer.
/// - `alpha ∈ (0, 1]`. The caller validates this at the CLI boundary; we
///   tolerate any finite value here but α outside the open interval is
///   either degenerate (no update) or undefined.
/// - `z ≥ 0`. Negative `z` would return a *lower* quantile, which the
///   scheduler does not want.
/// - `var_floor ≥ 0`. Production code passes [`MIN_LN_VAR`].
///
/// Behaviour:
///
/// - Empty / all non-positive → `None`. Caller falls back to the
///   policy-level `unknown_p95_ms`.
/// - One sample → `Some(d)`. With a single observation we have weak
///   evidence of the location and *no* evidence of the spread, so we
///   short-circuit the floor and return the observation itself. The
///   wall-clock TTL of `max(predicted_ms × 2, 60_000)` already gives
///   headroom; piling the floor on top makes the very first build of a
///   new pname routinely 35 % over-predicted.
/// - Two or more samples → `exp(μ + z · √max(S, var_floor))`, saturating
///   into `u64`.
pub fn predict_lognormal_ms(durations: &[u64], alpha: f64, z: f64, var_floor: f64) -> Option<u64> {
    // ln(0) = -∞ would poison the recurrence, and a zero-duration
    // "observation" is meaningless anyway.
    let mut it = durations.iter().copied().filter(|&d| d > 0);

    let first = it.next()?;
    let mut mean = (first as f64).ln();
    let mut var = 0.0_f64;
    let mut count: u32 = 1;

    for d in it {
        let y = (d as f64).ln();
        // West (1979): residual is computed against the *previous* mean,
        // and the variance update uses the same δ.
        let delta = y - mean;
        mean += alpha * delta;
        var = (1.0 - alpha) * (var + alpha * delta * delta);
        count += 1;
    }

    if count == 1 {
        return Some(first);
    }

    let s_eff = var.max(var_floor);
    let estimate = (mean + z * s_eff.sqrt()).exp();
    if estimate.is_finite() && estimate >= 0.0 {
        // u64::MAX as f64 is exactly representable; the cast saturates.
        Some(estimate.min(u64::MAX as f64) as u64)
    } else {
        // Pathological inputs (e.g. var = +∞ from a non-finite alpha) —
        // fall back to the first sample so the scheduler still sees a
        // plausible number rather than 0 or u64::MAX.
        Some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: u64, expected: f64, rel: f64) -> bool {
        // 1 ms absolute slack for the `as u64` truncation in the read-out.
        let diff = (actual as f64 - expected).abs();
        diff <= 1.0 + rel * expected.abs()
    }

    #[test]
    fn empty_history_is_none() {
        assert_eq!(
            predict_lognormal_ms(&[], ALPHA_DEFAULT, Z_P95, MIN_LN_VAR),
            None
        );
    }

    #[test]
    fn all_zero_history_is_none() {
        assert_eq!(
            predict_lognormal_ms(&[0, 0, 0], ALPHA_DEFAULT, Z_P95, MIN_LN_VAR),
            None
        );
    }

    #[test]
    fn single_sample_returns_sample_without_inflation() {
        // Single observation: no variance information, so we deliberately
        // skip the floor and return the observation as-is.
        let got = predict_lognormal_ms(&[10_000], ALPHA_DEFAULT, Z_P95, MIN_LN_VAR);
        assert_eq!(got, Some(10_000));
    }

    #[test]
    fn identical_samples_apply_variance_floor() {
        // 50 identical durations. EW variance collapses to 0, the floor
        // takes over, and the estimate is sample × exp(z·√MIN_LN_VAR).
        let durations = vec![30_000_u64; 50];
        let got = predict_lognormal_ms(&durations, ALPHA_DEFAULT, Z_P95, MIN_LN_VAR).unwrap();
        let expected = 30_000.0 * (Z_P95 * MIN_LN_VAR.sqrt()).exp();
        assert!(
            close(got, expected, 1e-6),
            "expected ≈ {expected:.1}, got {got}"
        );
        // Roughly the multiplier we documented: about 35 % over the sample.
        assert!(got > 30_000 && got < 50_000, "got {got}");
    }

    #[test]
    fn adapts_to_step_change_after_recovery_window() {
        // 200 obs at 2.4 minutes, then 30 new obs at 1.5 minutes. The old
        // p95 estimator would still report ~144 s after this (it would
        // need ~191 new samples to roll the percentile over). After 30 EW
        // updates with α=0.2 our mean has effectively converged to the
        // new value and the variance has decayed back to the floor, so
        // the conservative read-out settles at
        // 90_000 × exp(z·√MIN_LN_VAR) ≈ 121 500 ms.
        //
        // Note the *transient*: in the first few new samples the variance
        // term actually rises (the mean is hill-climbing and δ² is
        // large), so the prediction briefly *exceeds* the old steady
        // state. That's deliberate — the estimator is honestly widening
        // its uncertainty band while it tracks the change. The recovery
        // window is what we test, not the transient peak.
        let mut durations = vec![144_000_u64; 200];
        durations.extend(std::iter::repeat_n(90_000_u64, 30));
        let got = predict_lognormal_ms(&durations, 0.2, Z_P95, MIN_LN_VAR).unwrap();
        // Old steady-state estimate ≈ 144 000 × 1.35 = 194 400 ms; after
        // recovery we want to be well below that *and* below the raw old
        // sample value, so 130 000 ms is a clean threshold.
        assert!(
            got < 130_000,
            "after 30 new samples at 90 s the estimate should settle near \
             90 s × 1.35 = 121 500 ms, got {got}"
        );
        assert!(got > 100_000, "should not undershoot the floor, got {got}");
    }

    #[test]
    fn outlier_widens_then_recovers() {
        // Twenty 60 s builds, one 600 s outlier (10x), then thirty more
        // 60 s builds. The conservative bound necessarily *rises* right
        // after the outlier — variance jumps — but the EW variance term
        // decays geometrically as more normal samples arrive. After 30
        // recovery samples we're back near the floor-bounded steady
        // state for 60 s builds.
        let mut transient = vec![60_000_u64; 20];
        transient.push(600_000);
        let transient_pred = predict_lognormal_ms(&transient, 0.2, Z_P95, MIN_LN_VAR).unwrap();

        let mut recovered = transient.clone();
        recovered.extend(std::iter::repeat_n(60_000_u64, 30));
        let recovered_pred = predict_lognormal_ms(&recovered, 0.2, Z_P95, MIN_LN_VAR).unwrap();

        // Transient should clearly lift the prediction.
        assert!(
            transient_pred > 200_000,
            "outlier must lift the prediction, got {transient_pred}"
        );
        // After 30 normal samples the variance contribution decays and we
        // should be back near `60_000 × 1.35 ≈ 81_000` ms.
        assert!(
            recovered_pred < 100_000,
            "after 30 recovery samples the prediction should settle near \
             60 s × 1.35 = 81_000 ms, got {recovered_pred}"
        );
        // The recovered estimate must be strictly lower than the transient.
        assert!(
            recovered_pred < transient_pred,
            "recovery should reduce the estimate (transient={transient_pred}, \
             recovered={recovered_pred})"
        );
    }

    #[test]
    fn order_matters_for_ewma() {
        let oldest_first =
            predict_lognormal_ms(&[10_000, 10_000, 10_000, 100_000], 0.5, Z_P95, MIN_LN_VAR)
                .unwrap();
        let newest_first =
            predict_lognormal_ms(&[100_000, 10_000, 10_000, 10_000], 0.5, Z_P95, MIN_LN_VAR)
                .unwrap();
        // The first list ends with the big build → recent weight pulls the
        // estimate up. The second list ends with small builds → estimate
        // is closer to the small value.
        assert!(
            oldest_first > newest_first,
            "order should matter: oldest_first={oldest_first}, newest_first={newest_first}"
        );
    }

    #[test]
    fn alpha_one_is_last_sample_with_floor() {
        // α=1 means "trust the latest observation, forget everything
        // else". West's recursion then yields S = 0 always, so the
        // read-out is `last × exp(z·√MIN_LN_VAR)`.
        let got = predict_lognormal_ms(&[1_000, 2_000, 3_000], 1.0, Z_P95, MIN_LN_VAR).unwrap();
        let expected = 3_000.0 * (Z_P95 * MIN_LN_VAR.sqrt()).exp();
        assert!(close(got, expected, 1e-6), "got {got}, expected {expected}");
    }

    #[test]
    fn non_positive_durations_are_ignored() {
        // ln(0) is -∞ — the filter strips zeros so they don't poison the
        // recurrence. The result should match the all-positive subset.
        let with_zeros =
            predict_lognormal_ms(&[0, 10_000, 0, 12_000, 0], ALPHA_DEFAULT, Z_P95, MIN_LN_VAR);
        let without = predict_lognormal_ms(&[10_000, 12_000], ALPHA_DEFAULT, Z_P95, MIN_LN_VAR);
        assert_eq!(with_zeros, without);
    }

    #[test]
    fn lognormal_p95_matches_closed_form_on_stationary_data() {
        // Generate 500 samples drawn from a known log-normal so the EW
        // estimate has time to converge to the population parameters.
        // With μ_y = ln(60_000) and σ_y = 0.5, the true p95 is
        // exp(μ_y + 1.645·σ_y) ≈ 60_000 · exp(0.8225) ≈ 136_575.
        let mu_y = (60_000.0_f64).ln();
        let sigma_y = 0.5_f64;
        // Deterministic pseudo-random log-normal sequence — Box–Muller from
        // a Lehmer LCG so the test is reproducible and dependency-free.
        let mut state: u64 = 0xdead_beef_cafe_f00d;
        let mut next_u01 = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            // Top 53 bits → [0,1).
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut samples = Vec::with_capacity(500);
        for _ in 0..250 {
            let u1: f64 = next_u01().max(f64::MIN_POSITIVE);
            let u2: f64 = next_u01();
            let r = (-2.0 * u1.ln()).sqrt();
            let z0 = r * (2.0 * std::f64::consts::PI * u2).cos();
            let z1 = r * (2.0 * std::f64::consts::PI * u2).sin();
            samples.push((mu_y + sigma_y * z0).exp() as u64);
            samples.push((mu_y + sigma_y * z1).exp() as u64);
        }
        let got = predict_lognormal_ms(&samples, 0.05, Z_P95, MIN_LN_VAR).unwrap();
        let truth = (mu_y + Z_P95 * sigma_y).exp() as u64;
        // ±25 % on a stochastic test with 500 samples and α=0.05 is plenty
        // of slack to stay flaky-free across CI.
        let lo = (truth as f64 * 0.75) as u64;
        let hi = (truth as f64 * 1.25) as u64;
        assert!(
            got > lo && got < hi,
            "got {got}, truth ≈ {truth} (expected within ±25 %)"
        );
    }
}
