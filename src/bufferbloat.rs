use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
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
}

pub async fn measure_bufferbloat(duration: f64, attempts: usize) -> BufferbloatResult {
    let mut idle_samples = Vec::new();

    for _ in 0..attempts {
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
        };
    }

    let idle = idle_samples.iter().cloned().fold(f64::MAX, f64::min);

    let url = tester::SERVER.download_url.replace("{bytes}", "200000000");
    let stop = Arc::new(AtomicBool::new(false));

    let stop_clone = stop.clone();
    let saturate_handle = task::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap();

        loop {
            if stop_clone.load(Ordering::Relaxed) {
                break;
            }

            let response = match client.get(&url).send().await {
                Ok(r) => r,
                Err(_) => break,
            };

            let mut stream = response.bytes_stream();

            while let Some(chunk) = stream.next().await {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                let _ = chunk;
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let mut samples = Vec::new();
    let end_time = Instant::now() + std::time::Duration::from_secs_f64(duration);

    while Instant::now() < end_time {
        if let Some(ms) = tcp_ping_once(tester::SERVER.host, tester::SERVER.port).await {
            samples.push(ms);
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    stop.store(true, Ordering::Relaxed);
    let _ = saturate_handle.await;

    if samples.is_empty() {
        return BufferbloatResult {
            idle_ms: round2(idle),
            loaded_ms: 0.0,
            delta_ms: 0.0,
            grade: "?".to_string(),
        };
    }

    let loaded = samples.iter().cloned().fold(f64::MAX, f64::min);
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
