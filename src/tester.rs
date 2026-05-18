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

/// Builds a `reqwest::Client` configured with the given request timeout.
///
/// `timeout_secs` is the per-request timeout in seconds.
///
/// Panics if the client cannot be constructed.
///
/// # Examples
///
/// ```
/// let client = build_client(30);
/// // use `client` to make requests...
/// ```
pub fn build_client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .expect("failed to build HTTP client")
}

/// Builds an HTTP client preconfigured with standard request headers and a request timeout.
///
/// The client's default headers are populated from `REQUEST_HEADERS`. If any static header
/// value is invalid or the client builder fails, the function will panic.
///
/// # Examples
///
/// ```
/// let client = build_speed_client(30);
/// // use `client` to make requests...
/// ```
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

/// Measures TCP latency, packet loss, and jitter to a given host and port by performing multiple connection attempts.
///
/// Performs `attempts` sequential TCP connection attempts and aggregates the results:
/// - `host` and `port` specify the target address to connect to.
/// - `attempts` is the number of connection attempts to perform.
///
/// # Returns
///
/// A tuple containing:
/// - `Option<f64>`: average round-trip time in milliseconds rounded to two decimals, or `None` if all attempts failed.
/// - `f64`: packet loss percentage rounded to one decimal (`0.0`–`100.0`).
/// - `Option<f64>`: jitter in milliseconds (difference between max and min successful latencies) rounded to two decimals, or `None` if no attempts succeeded.
///
/// # Examples
///
/// ```
/// # tokio_test::block_on(async {
/// let (avg, loss, jitter) = crate::tester::ping("example.com", 80, 3).await;
/// // avg and jitter are `Option<f64>`; loss is always a `f64`.
/// assert!(loss >= 0.0 && loss <= 100.0);
/// # });
/// ```
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

/// Measures a single TCP round-trip time to `host:port`.
///
/// # Returns
///
/// `Some(elapsed_ms)` with the elapsed time in milliseconds when the TCP connection completes successfully within 3 seconds, `None` on timeout or any connection error.
///
/// # Examples
///
/// ```
/// // Run the async function using a small runtime.
/// let rt = tokio::runtime::Runtime::new().unwrap();
/// let elapsed = rt.block_on(async { crate::tcp_ping_once("example.com", 80).await });
/// // elapsed is either `None` (timeout/error) or a non-negative millisecond value
/// assert!(elapsed.is_none() || elapsed.unwrap() >= 0.0);
/// ```
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

/// Measures download throughput by concurrently fetching a URL template across multiple streams for a fixed duration.
///
/// The `url_template` must contain the substring `"{bytes}"` which will be replaced with a large value to trigger a full-range download. The function spawns `streams` concurrent workers that read response bodies and accumulate received bytes; if `on_progress` is provided it will be invoked periodically with the current total bytes and elapsed seconds.
///
/// # Parameters
///
/// - `url_template`: URL template containing `"{bytes}"` to be substituted before requests.
/// - `duration_s`: Measurement duration in seconds.
/// - `streams`: Number of concurrent download streams to run.
/// - `on_progress`: Optional callback invoked periodically as `cb(total_bytes, elapsed_secs)`.
///
/// # Returns
///
/// A tuple where the first element is the measured download speed in megabits per second (rounded to two decimal places) and the second element is the total number of bytes transferred during the measurement window.
///
/// # Examples
///
/// ```
/// # use tester::{download};
/// # use std::time::Duration;
/// # use tokio;
/// type ProgressCallback = fn(u64, f64);
///
/// #[tokio::test]
/// async fn example_download() {
///     // Use Cloudflare speed test template as an example.
///     let template = "https://speed.cloudflare.com/__down?bytes={bytes}";
///     let (mbps, bytes) = download(template, 1.0, 1, None).await;
///     // Result is a tuple (mbps, bytes); values may be zero in constrained environments.
///     assert!(bytes >= 0);
///     assert!(mbps >= 0.0);
/// }
/// ```
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

/// Performs concurrent HTTP POST uploads of random data to the given `url` for approximately `duration_s`, using `streams` parallel workers and optionally reporting progress via `on_progress`.
///
/// The function spawns `streams` tasks that repeatedly POST 4 MiB chunks to `url`, measures bytes transferred during the measurement window, and returns the measured upload speed and total bytes.
///
/// # Parameters
/// - `url`: destination upload endpoint.
/// - `duration_s`: measurement duration in seconds.
/// - `streams`: number of concurrent upload tasks.
/// - `on_progress`: optional callback invoked periodically with `(bytes_uploaded, elapsed_seconds)`.
///
/// # Returns
/// A tuple `(mbps, total_bytes)` where `mbps` is the measured upload speed in megabits per second (rounded to two decimals) and `total_bytes` is the total number of bytes uploaded.
///
/// # Examples
///
/// ```
/// # use std::sync::Arc;
/// # use tokio;
/// # async fn _run() {
/// let (mbps, bytes) = crate::tester::upload("https://speed.cloudflare.com/__up", 1.0, 2, None).await;
/// assert!(mbps >= 0.0);
/// assert!(bytes >= 0);
/// # }
/// ```
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
