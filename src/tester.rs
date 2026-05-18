use futures::StreamExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpStream;
use tokio::task;

use crate::util;

pub struct Server {
    pub name: &'static str,
    pub download_url: &'static str,
    pub upload_url: &'static str,
    pub host: &'static str,
    pub port: u16,
}

pub const SERVER: Server = Server {
    name: "Cloudflare",
    download_url: "https://speed.cloudflare.com/__down?bytes={bytes}",
    upload_url: "https://speed.cloudflare.com/__up",
    host: "speed.cloudflare.com",
    port: 443,
};

const REQUEST_HEADERS: [(&str, &str); 3] = [
    ("User-Agent", "Mozilla/5.0"),
    ("Accept", "*/*"),
    ("Referer", "https://speed.cloudflare.com/"),
];

const UPLOAD_CHUNK_BYTES: usize = 4 * 1024 * 1024;

pub fn build_client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .expect("failed to build HTTP client")
}

pub fn build_speed_client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            for (k, v) in REQUEST_HEADERS {
                h.insert(k, v.parse().expect("invalid static header value"));
            }
            h
        })
        .build()
        .expect("failed to build HTTP client")
}

pub async fn ping(host: &str, port: u16, attempts: usize) -> (Option<f64>, f64, Option<f64>) {
    let mut results = Vec::new();

    for _ in 0..attempts {
        let result = tcp_ping_once(host, port).await;
        results.push(result);
    }

    let valid: Vec<f64> = results.into_iter().flatten().collect();
    if valid.is_empty() {
        return (None, 100.0, None);
    }

    let avg = valid.iter().sum::<f64>() / valid.len() as f64;
    let loss = ((attempts - valid.len()) as f64 / attempts as f64) * 100.0;
    let jitter = valid
        .iter()
        .cloned()
        .fold((f64::MAX, f64::MIN), |(min, max), v| {
            (min.min(v), max.max(v))
        });
    let jitter = jitter.1 - jitter.0;

    (
        Some(util::round2(avg)),
        util::round1(loss),
        Some(util::round2(jitter)),
    )
}

/// Measures a single TCP round-trip to `host:port`. Returns `None` on timeout or error.
pub async fn tcp_ping_once(host: &str, port: u16) -> Option<f64> {
    let start = Instant::now();
    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
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

type ProgressCallback = Box<dyn Fn(u64, f64) + Send + Sync>;

pub async fn download(
    url_template: &str,
    duration_s: f64,
    streams: usize,
    on_progress: Option<ProgressCallback>,
) -> (f64, u64) {
    let url = url_template.replace("{bytes}", "1000000000");
    let client = build_speed_client(30);

    let stop = Arc::new(AtomicBool::new(false));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let measure_start = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();

    for _ in 0..streams {
        let client = client.clone();
        let url = url.clone();
        let stop = stop.clone();
        let total_bytes = total_bytes.clone();
        let measure_start = measure_start.clone();

        let handle = task::spawn(async move {
            let response = match client.get(&url).send().await {
                Ok(r) => r,
                Err(_) => return,
            };

            let mut stream = response.bytes_stream();

            while let Some(chunk) = stream.next().await {
                if stop.load(Ordering::Relaxed) {
                    return;
                }

                let mstart = measure_start.load(Ordering::Relaxed);
                if mstart == 0 {
                    continue;
                }

                let chunk = match chunk {
                    Ok(c) => c,
                    Err(_) => return,
                };

                total_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
        });

        handles.push(handle);
    }

    tokio::time::sleep(std::time::Duration::from_secs_f64(1.5)).await;

    total_bytes.store(0, Ordering::Relaxed);
    let start_instant = Instant::now();
    measure_start.store(1, Ordering::Relaxed);

    let end_at = start_instant + std::time::Duration::from_secs_f64(duration_s);

    while Instant::now() < end_at {
        if let Some(ref cb) = on_progress {
            let bytes = total_bytes.load(Ordering::Relaxed);
            let elapsed = start_instant.elapsed().as_secs_f64();
            cb(bytes, elapsed);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    stop.store(true, Ordering::Relaxed);

    let elapsed = start_instant.elapsed().as_secs_f64();
    let bytes_transferred = total_bytes.load(Ordering::Relaxed);

    for handle in handles {
        let _ = handle.await;
    }

    if elapsed <= 0.0 || bytes_transferred == 0 {
        return (0.0, 0);
    }

    (
        util::round2(util::bytes_to_mbps(bytes_transferred, elapsed)),
        bytes_transferred,
    )
}

pub async fn upload(
    url: &str,
    duration_s: f64,
    streams: usize,
    on_progress: Option<ProgressCallback>,
) -> (f64, u64) {
    let data: Arc<Vec<u8>> = Arc::new(
        (0..UPLOAD_CHUNK_BYTES)
            .map(|_| rand::random::<u8>())
            .collect(),
    );

    let client = build_speed_client(60);

    let stop = Arc::new(AtomicBool::new(false));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let measure_start = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();

    for _ in 0..streams {
        let client = client.clone();
        let url = url.to_string();
        let stop = stop.clone();
        let total_bytes = total_bytes.clone();
        let measure_start = measure_start.clone();
        let data = data.clone();

        let handle = task::spawn(async move {
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                let chunk = data.as_ref().clone();
                let len = chunk.len() as u64;

                let result = client.post(&url).body(chunk).send().await;

                if result.is_ok()
                    && measure_start.load(Ordering::Relaxed) != 0
                    && !stop.load(Ordering::Relaxed)
                {
                    total_bytes.fetch_add(len, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    tokio::time::sleep(std::time::Duration::from_secs_f64(1.5)).await;

    total_bytes.store(0, Ordering::Relaxed);
    let start_instant = Instant::now();
    measure_start.store(1, Ordering::Relaxed);

    let end_at = start_instant + std::time::Duration::from_secs_f64(duration_s);

    while Instant::now() < end_at {
        if let Some(ref cb) = on_progress {
            let bytes = total_bytes.load(Ordering::Relaxed);
            let elapsed = start_instant.elapsed().as_secs_f64();
            cb(bytes, elapsed);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    stop.store(true, Ordering::Relaxed);

    let elapsed = start_instant.elapsed().as_secs_f64();
    let bytes_transferred = total_bytes.load(Ordering::Relaxed);

    for handle in handles {
        let _ = handle.await;
    }

    if elapsed <= 0.0 || bytes_transferred == 0 {
        return (0.0, 0);
    }

    (
        util::round2(util::bytes_to_mbps(bytes_transferred, elapsed)),
        bytes_transferred,
    )
}
