use crate::util;

#[derive(serde::Serialize)]
pub struct BufferbloatResult {
    pub idle_ms: f64,
    /// p90 latency across both directions under load.
    pub loaded_ms: f64,
    pub delta_ms: f64,
    pub grade: String,
    /// p90 latency while downloading; `None` when no sample was taken.
    pub download_loaded_ms: Option<f64>,
    /// p90 latency while uploading; `None` when upload was skipped or no sample was taken.
    pub upload_loaded_ms: Option<f64>,
}

/// Nearest-rank percentile of a sample list. `pct` is clamped to [0, 100]:
/// p0 returns the min, p100 the max. Returns 0.0 for an empty list so
/// callers can treat "no samples" as a neutral zero.
fn percentile(samples: &[f64], pct: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut ordered = samples.to_vec();
    ordered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = ordered.len();
    if pct <= 0.0 {
        return ordered[0];
    }
    if pct >= 100.0 {
        return ordered[n - 1];
    }
    // Nearest-rank: rank = ceil(pct/100 * n), then index = rank - 1.
    let rank = ((pct / 100.0) * n as f64).ceil() as usize;
    ordered[rank.clamp(1, n) - 1]
}

fn grade(delta: f64) -> &'static str {
    if delta < 5.0 {
        "A+"
    } else if delta < 30.0 {
        "A"
    } else if delta < 60.0 {
        "B"
    } else if delta < 200.0 {
        "C"
    } else if delta < 400.0 {
        "D"
    } else {
        "F"
    }
}

/// Grades bufferbloat from latency samples taken during the speed tests.
///
/// `idle_ms` is the best idle RTT (the minimum, so queueing noise stays out of
/// the baseline). Loaded latency is the p90 of the samples: robust to a couple
/// of lucky connects, still the latency a user feels under load. Returns
/// `None` without an idle baseline or any loaded sample.
///
/// TCP-connect sampling has a known limit: a lost SYN is retransmitted by the
/// kernel after >=1s, so packet loss shows up as inflated latency.
pub fn from_samples(
    idle_ms: Option<f64>,
    download: &[f64],
    upload: &[f64],
) -> Option<BufferbloatResult> {
    let idle = idle_ms?;
    let all: Vec<f64> = download.iter().chain(upload).copied().collect();
    if all.is_empty() {
        return None;
    }
    let p90 = |s: &[f64]| (!s.is_empty()).then(|| util::round2(percentile(s, 90.0)));
    let loaded = percentile(&all, 90.0);
    let delta = (loaded - idle).max(0.0);
    Some(BufferbloatResult {
        idle_ms: util::round2(idle),
        loaded_ms: util::round2(loaded),
        delta_ms: util::round2(delta),
        grade: grade(delta).to_string(),
        download_loaded_ms: p90(download),
        upload_loaded_ms: p90(upload),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- percentile ---

    #[test]
    fn percentile_empty_returns_zero() {
        assert_eq!(percentile(&[], 90.0), 0.0);
    }

    #[test]
    fn percentile_single_sample() {
        assert_eq!(percentile(&[42.0], 90.0), 42.0);
    }

    #[test]
    fn percentile_p0_returns_min() {
        assert_eq!(percentile(&[3.0, 1.0, 2.0], 0.0), 1.0);
    }

    #[test]
    fn percentile_p100_returns_max() {
        assert_eq!(percentile(&[3.0, 1.0, 2.0], 100.0), 3.0);
    }

    #[test]
    fn percentile_p50_of_four_is_second() {
        // nearest-rank: ceil(0.5 * 4) = 2 → index 1 of sorted [1,2,3,4]
        assert_eq!(percentile(&[4.0, 2.0, 1.0, 3.0], 50.0), 2.0);
    }

    #[test]
    fn percentile_p90_of_ten_is_ninth() {
        let samples: Vec<f64> = (1..=10).map(|v| v as f64).collect();
        // ceil(0.9 * 10) = 9 → index 8 → 9.0
        assert_eq!(percentile(&samples, 90.0), 9.0);
    }

    #[test]
    fn percentile_unsorted_input_not_mutated() {
        let samples = [5.0, 1.0, 3.0];
        percentile(&samples, 50.0);
        assert_eq!(samples, [5.0, 1.0, 3.0]);
    }

    // --- from_samples ---

    #[test]
    fn from_samples_needs_idle_and_loaded() {
        assert!(from_samples(None, &[50.0], &[]).is_none());
        assert!(from_samples(Some(20.0), &[], &[]).is_none());
    }

    #[test]
    fn from_samples_grades_delta_and_splits_directions() {
        let r = from_samples(Some(20.0), &[60.0; 10], &[]).unwrap();
        assert_eq!(r.loaded_ms, 60.0);
        assert_eq!(r.delta_ms, 40.0);
        assert_eq!(r.grade, "B");
        assert_eq!(r.download_loaded_ms, Some(60.0));
        assert_eq!(r.upload_loaded_ms, None);
    }

    // --- grade boundaries ---

    #[test]
    fn grade_boundaries() {
        assert_eq!(grade(0.0), "A+");
        assert_eq!(grade(4.99), "A+");
        assert_eq!(grade(5.0), "A");
        assert_eq!(grade(29.99), "A");
        assert_eq!(grade(30.0), "B");
        assert_eq!(grade(59.99), "B");
        assert_eq!(grade(60.0), "C");
        assert_eq!(grade(199.99), "C");
        assert_eq!(grade(200.0), "D");
        assert_eq!(grade(399.99), "D");
        assert_eq!(grade(400.0), "F");
        assert_eq!(grade(10_000.0), "F");
    }
}
