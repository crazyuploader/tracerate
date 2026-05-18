use futures::StreamExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpStream;
use tokio::task;

use crate::tester;

#[derive(serde::Serialize)]
pub struct BufferbloatResult {
    pub idle_ms: f64,
    pub loaded_ms: f64,
    pub delta_ms: f64,
    pub grade: String,
    pub data_used_mb: f64,
}

pub async fn measure_bufferbloat(duration: f64, streams: usize) -> BufferbloatResult {
    // Idle baseline: average of 8 TCP pings before any load
    let mut idle_samples = Vec::new();
    for _ in 0..8 {
        if let Some(ms) = tcp_ping_once(tester::SERVER.host, tester::SERVER.port).await {
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

    // Create client once, shared across tasks (same pattern as download test)
    let client = Arc::new(
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .default_headers({
                let mut h = reqwest::header::HeaderMap::new();
                for (k, v) in tester::REQUEST_HEADERS {
                    h.insert(k, v.parse().unwrap());
                }
                h
            })
            .build()
            .unwrap(),
    );

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
        if let Some(ms) = tcp_ping_once(tester::SERVER.host, tester::SERVER.port).await {
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
            idle_ms: round2(idle),
            loaded_ms: 0.0,
            delta_ms: 0.0,
            grade: "?".to_string(),
            data_used_mb: round2(data_used_mb),
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
        idle_ms: round2(idle),
        loaded_ms: round2(loaded),
        delta_ms: round2(delta),
        grade: grade.to_string(),
        data_used_mb: round2(data_used_mb),
    }
}

async fn tcp_ping_once(host: &str, port: u16) -> Option<f64> {
    let start = Instant::now();
    match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        TcpStream::connect((host, port)),
    )
    .await
    {
        Ok(Ok(_)) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            Some(elapsed)
        }
        _ => None,
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
