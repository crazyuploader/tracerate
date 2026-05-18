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
    // Idle baseline: average of 8 TCP pings before any load
    let mut idle_samples = Vec::new();
    for _ in 0..8 {
        if let Some(ms) = tester::tcp_ping_once(tester::SERVER.host, tester::SERVER.port).await {
            idle_samples.push(ms);
        }
    }

    if idle_samples.is_empty() {
        return BufferbloatResult {
            idle_ms: 0.0,
            loaded_ms: 0.0,
            delta_ms: 0.0,
            grade: "?".to_string(),
            data_used_mb: 0.0,
        };
    }

    let idle = idle_samples.iter().sum::<f64>() / idle_samples.len() as f64;

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

    // Wait for link to fully saturate before measuring
    tokio::time::sleep(std::time::Duration::from_secs_f64(1.5)).await;

    // Collect loaded latency samples, compute average
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

    let loaded = samples.iter().sum::<f64>() / samples.len() as f64;
    let delta = (loaded - idle).max(0.0);

    let grade = if delta < 5.0 {
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
    };

    BufferbloatResult {
        idle_ms: util::round2(idle),
        loaded_ms: util::round2(loaded),
        delta_ms: util::round2(delta),
        grade: grade.to_string(),
        data_used_mb: util::round2(data_used_mb),
    }
}
