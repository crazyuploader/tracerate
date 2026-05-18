use futures::StreamExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpStream;
use tokio::task;

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

pub const UPLOAD_MAX_BYTES: usize = 25 * 1024 * 1024;

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

    (Some(round2(avg)), round1(loss), Some(round2(jitter)))
}

async fn tcp_ping_once(host: &str, port: u16) -> Option<f64> {
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
) -> f64 {
    let url = url_template.replace("{bytes}", "1000000000");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            for (k, v) in REQUEST_HEADERS {
                h.insert(k, v.parse().unwrap());
            }
            h
        })
        .build()
        .unwrap();

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
        return 0.0;
    }

    let speed_mbps = (bytes_transferred as f64 * 8.0) / elapsed / 1_000_000.0;
    round2(speed_mbps)
}

pub async fn upload(url: &str, size_bytes: usize) -> f64 {
    let size_bytes = size_bytes.min(UPLOAD_MAX_BYTES);
    let data: Vec<u8> = (0..size_bytes).map(|_| rand::random()).collect();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap();

    let start = Instant::now();

    let result = client
        .post(url)
        .headers({
            let mut h = reqwest::header::HeaderMap::new();
            for (k, v) in REQUEST_HEADERS {
                h.insert(k, v.parse().unwrap());
            }
            h
        })
        .body(data)
        .send()
        .await;

    match result {
        Ok(response) => {
            if response.status().is_success() {
                let elapsed = start.elapsed().as_secs_f64();
                if elapsed == 0.0 {
                    return 0.0;
                }
                let speed = (size_bytes as f64 * 8.0) / elapsed / 1_000_000.0;
                round2(speed)
            } else {
                0.0
            }
        }
        Err(_) => 0.0,
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
