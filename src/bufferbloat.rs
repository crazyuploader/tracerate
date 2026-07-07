use futures::StreamExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::task;

use crate::tester;
use crate::util;

#[derive(serde::Serialize)]
pub struct BufferbloatResult {
    pub idle_ms: f64,
    pub loaded_ms: f64,
    pub delta_ms: f64,
    pub grade: String,
    pub data_used_mb: f64,
}

/// Cap on parallel saturation streams. Beyond this the probe itself starts
/// competing for upstream bandwidth on slow links and skews the measurement;
/// matches the streams=6 ceiling used by the main download test.
const MAX_SATURATION_STREAMS: usize = 6;

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

/// Measures TCP ping latency before and during concurrent download saturation and produces a `BufferbloatResult` summarizing idle and loaded latencies, their difference, a grade, and data transferred.
///
/// The function samples up to eight TCP pings to establish an idle baseline, then spawns `streams` concurrent download workers to saturate the link for the specified `duration` (in seconds) while sampling TCP ping latency under load. If no idle samples are obtained, the function returns a result with all numeric fields set to `0.0` and `grade` set to `"?"`. If no loaded samples are collected during the saturation window, the returned result contains the rounded idle latency and rounded data usage while `loaded_ms`, `delta_ms` are `0.0` and `grade` is `"?"`.
///
/// # Examples
///
/// ```
/// # use tokio_test::block_on;
/// # async fn run_example() {
/// let result = super::measure_bufferbloat(3.0, 2).await;
/// // basic invariant checks
/// assert!(result.idle_ms >= 0.0);
/// assert!(result.loaded_ms >= 0.0);
/// assert!(result.delta_ms >= 0.0);
/// assert!(result.data_used_mb >= 0.0);
/// # }
/// # block_on(run_example());
/// ```
pub async fn measure_bufferbloat(duration: f64, streams: usize) -> BufferbloatResult {
    // Clamp streams so the probe itself can't become the bottleneck.
    let streams = streams.clamp(1, MAX_SATURATION_STREAMS);

    // Idle baseline: 8 concurrent TCP pings before any load
    let idle_samples: Vec<f64> = futures::future::join_all(
        (0..8).map(|_| tester::tcp_ping_once(tester::SERVER.host, tester::SERVER.port)),
    )
    .await
    .into_iter()
    .flatten()
    .collect();

    if idle_samples.is_empty() {
        return BufferbloatResult {
            idle_ms: 0.0,
            loaded_ms: 0.0,
            delta_ms: 0.0,
            grade: "?".to_string(),
            data_used_mb: 0.0,
        };
    }

    // Best idle RTT is the baseline; the average would bake queueing noise
    // into the reference point.
    let idle = idle_samples.iter().cloned().fold(f64::MAX, f64::min);

    // Saturate with multiple concurrent download streams
    let url = tester::SERVER.download_url.replace("{bytes}", "200000000");
    let stop = Arc::new(AtomicBool::new(false));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let mut saturate_handles = Vec::new();

    let client = Arc::new(tester::build_speed_client(60));

    for _ in 0..streams {
        let stop_clone = stop.clone();
        let url_clone = url.clone();
        let bytes_clone = total_bytes.clone();
        let client = client.clone();

        let handle = task::spawn(async move {
            loop {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }

                let response = match client.get(&url_clone).send().await {
                    Ok(r) => r,
                    Err(_) => break,
                };

                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    if stop_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Ok(c) = chunk {
                        bytes_clone.fetch_add(c.len() as u64, Ordering::Relaxed);
                    }
                }
            }
        });

        saturate_handles.push(handle);
    }

    // Short warmup so streams ramp past TCP slow-start before we sample.
    tokio::time::sleep(std::time::Duration::from_secs_f64(0.3)).await;

    // TCP-connect sampling under load has a known limitation: a lost SYN is
    // silently retransmitted by the kernel (after >=1s), so packet loss shows
    // up as inflated latency rather than a missed sample.
    let mut samples = Vec::new();
    let end_time = Instant::now() + std::time::Duration::from_secs_f64(duration);

    while Instant::now() < end_time {
        if let Some(ms) = tester::tcp_ping_once(tester::SERVER.host, tester::SERVER.port).await {
            samples.push(ms);
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    stop.store(true, Ordering::Relaxed);
    for handle in saturate_handles {
        let _ = handle.await;
    }

    let data_used_mb = total_bytes.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0);

    if samples.is_empty() {
        return BufferbloatResult {
            idle_ms: util::round2(idle),
            loaded_ms: 0.0,
            delta_ms: 0.0,
            grade: "?".to_string(),
            data_used_mb: util::round2(data_used_mb),
        };
    }

    // p90 of loaded samples: robust to a couple of lucky connects, still
    // reflects the latency a user actually feels under load.
    let loaded = percentile(&samples, 90.0);
    let delta = (loaded - idle).max(0.0);

    BufferbloatResult {
        idle_ms: util::round2(idle),
        loaded_ms: util::round2(loaded),
        delta_ms: util::round2(delta),
        grade: grade(delta).to_string(),
        data_used_mb: util::round2(data_used_mb),
    }
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
