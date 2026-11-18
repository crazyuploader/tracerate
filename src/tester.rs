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

/// Size of the pieces a POST body is streamed in. Bytes are counted per piece
/// so throughput shows up while a request is still in flight, not only when a
/// whole chunk finishes.
const UPLOAD_PIECE_BYTES: usize = 64 * 1024;

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
/// The host is resolved once and one warm-up connect is discarded, so DNS
/// lookup and cold-path costs do not show up as latency or jitter. Then
/// `attempts` sequential TCP connects are made to the resolved address.
///
/// Returns a [`Ping`]; the latency fields are `None` when every attempt failed.
pub async fn ping(host: &str, port: u16, attempts: usize) -> Ping {
    let failed = |ip| Ping {
        ip,
        avg_ms: None,
        loss_pct: 100.0,
        jitter_ms: None,
        low_ms: None,
        high_ms: None,
    };
    let addr = match tokio::net::lookup_host((host, port)).await {
        Ok(mut addrs) => match addrs.next() {
            Some(a) => a,
            None => return failed(None),
        },
        Err(_) => return failed(None),
    };

    // Warm-up connect: not counted.
    let _ = tcp_connect_ms(addr).await;

    let mut results = Vec::new();

    for _ in 0..attempts {
        let result = tcp_connect_ms(addr).await;
        results.push(result);
    }

    let valid: Vec<f64> = results.into_iter().flatten().collect();
    if valid.is_empty() {
        return failed(Some(addr.ip()));
    }

    let avg = valid.iter().sum::<f64>() / valid.len() as f64;
    let loss = ((attempts - valid.len()) as f64 / attempts as f64) * 100.0;

    Ping {
        ip: Some(addr.ip()),
        avg_ms: Some(util::round2(avg)),
        loss_pct: util::round1(loss),
        jitter_ms: Some(util::round2(jitter(&valid))),
        low_ms: Some(util::round2(valid.iter().copied().fold(f64::MAX, f64::min))),
        high_ms: Some(util::round2(valid.iter().copied().fold(0.0, f64::max))),
    }
}

/// Result of [`ping`]. Times are milliseconds rounded to two decimals.
pub struct Ping {
    /// Address the host resolved to and was pinged at.
    pub ip: Option<std::net::IpAddr>,
    pub avg_ms: Option<f64>,
    /// TCP connect-failure rate, 0.0–100.0.
    pub loss_pct: f64,
    /// Mean absolute difference between consecutive successful samples.
    pub jitter_ms: Option<f64>,
    pub low_ms: Option<f64>,
    pub high_ms: Option<f64>,
}

/// Mean absolute difference between consecutive samples (the usual jitter
/// definition). A single outlier moves it far less than max − min does.
/// Returns 0.0 for fewer than two samples.
fn jitter(samples: &[f64]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let total: f64 = samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
    total / (samples.len() - 1) as f64
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
    tcp_connect_ms((host, port)).await
}

/// Times one TCP connect to `addr`, with a 3 second timeout.
async fn tcp_connect_ms<A: tokio::net::ToSocketAddrs>(addr: A) -> Option<f64> {
    let start = Instant::now();
    match tokio::time::timeout(std::time::Duration::from_secs(3), TcpStream::connect(addr)).await {
        Ok(Ok(_)) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            Some(elapsed)
        }
        _ => None,
    }
}

type ProgressCallback = Box<dyn Fn(u64, f64) + Send + Sync>;

/// Result of a download or upload measurement.
pub struct Transfer {
    /// Speed over the measurement window, rounded to two decimals.
    pub mbps: f64,
    /// Bytes moved inside the measurement window.
    pub bytes: u64,
    /// Bytes moved during the warm-up before the window; not part of `mbps`.
    pub warmup_bytes: u64,
    /// Length of the measurement window in seconds; shorter than requested after a stable stop.
    pub elapsed_s: f64,
}

/// A stable stop never ends a measurement before this many seconds.
const STABLE_MIN_S: f64 = 8.0;
/// Number of one-second samples of the running average that must agree.
const STABLE_SAMPLES: usize = 4;
/// Largest spread between those samples, as a fraction of the highest one.
const STABLE_SPREAD: f64 = 0.02;

/// True when the last `STABLE_SAMPLES` running-average samples sit within `STABLE_SPREAD` of each other.
fn is_stable(avgs: &[f64]) -> bool {
    if avgs.len() < STABLE_SAMPLES {
        return false;
    }
    let tail = &avgs[avgs.len() - STABLE_SAMPLES..];
    let hi = tail.iter().copied().fold(0.0, f64::max);
    let lo = tail.iter().copied().fold(f64::MAX, f64::min);
    hi > 0.0 && (hi - lo) / hi <= STABLE_SPREAD
}

/// Runs the warm-up and measurement window shared by download and upload,
/// then signals `stop`. With `stable_stop`, the window ends early once the
/// running average speed stops changing.
async fn measure_window(
    total_bytes: &AtomicU64,
    warmup_bytes: &AtomicU64,
    measure_start: &AtomicU64,
    stop: &AtomicBool,
    duration_s: f64,
    stable_stop: bool,
    on_progress: Option<ProgressCallback>,
) -> Transfer {
    tokio::time::sleep(std::time::Duration::from_secs_f64(1.5)).await;

    total_bytes.store(0, Ordering::Relaxed);
    let start_instant = Instant::now();
    measure_start.store(1, Ordering::Relaxed);

    let end_at = start_instant + std::time::Duration::from_secs_f64(duration_s);
    let mut avgs = Vec::new();

    while Instant::now() < end_at {
        let bytes = total_bytes.load(Ordering::Relaxed);
        let elapsed = start_instant.elapsed().as_secs_f64();
        if let Some(ref cb) = on_progress {
            cb(bytes, elapsed);
        }
        if stable_stop && elapsed >= (avgs.len() + 1) as f64 {
            avgs.push(util::bytes_to_mbps(bytes, elapsed));
            if elapsed >= STABLE_MIN_S && is_stable(&avgs) {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    stop.store(true, Ordering::Relaxed);

    let elapsed = start_instant.elapsed().as_secs_f64();
    let bytes = total_bytes.load(Ordering::Relaxed);
    let warmup_bytes = warmup_bytes.load(Ordering::Relaxed);

    if elapsed <= 0.0 || bytes == 0 {
        return Transfer {
            mbps: 0.0,
            bytes: 0,
            warmup_bytes,
            elapsed_s: elapsed,
        };
    }

    Transfer {
        mbps: util::round2(util::bytes_to_mbps(bytes, elapsed)),
        bytes,
        warmup_bytes,
        elapsed_s: elapsed,
    }
}

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
/// A [`Transfer`]. With `stable_stop`, the window can end before `duration_s` once the speed settles.
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
///     let t = download(template, 1.0, 1, false, None).await;
///     assert!(t.mbps >= 0.0);
/// }
/// ```
pub async fn download(
    url_template: &str,
    duration_s: f64,
    streams: usize,
    stable_stop: bool,
    on_progress: Option<ProgressCallback>,
) -> Transfer {
    let url = url_template.replace("{bytes}", "1000000000");
    // The client timeout covers the whole response body, so it must outlast
    // the warmup plus the measurement window. A fixed 30s killed every stream
    // mid-test for --duration above ~28s.
    let client = build_speed_client(duration_s.ceil() as u64 + 10);

    let stop = Arc::new(AtomicBool::new(false));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let warmup_bytes = Arc::new(AtomicU64::new(0));
    let measure_start = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();

    for _ in 0..streams {
        let client = client.clone();
        let url = url.clone();
        let stop = stop.clone();
        let total_bytes = total_bytes.clone();
        let warmup_bytes = warmup_bytes.clone();
        let measure_start = measure_start.clone();

        let handle = task::spawn(async move {
            // Reopen the request if the server finishes the body early (the
            // 1 GB request can end within the window on very fast links).
            while !stop.load(Ordering::Relaxed) {
                let response = match client.get(&url).send().await {
                    Ok(r) if r.status().is_success() => r,
                    // An error status would otherwise end the body at once and
                    // send the loop straight into another request.
                    _ => return,
                };

                let mut stream = response.bytes_stream();
                let mut got_data = false;

                while let Some(chunk) = stream.next().await {
                    got_data = true;
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }

                    let chunk = match chunk {
                        Ok(c) => c,
                        Err(_) => return,
                    };

                    let counter = if measure_start.load(Ordering::Relaxed) == 0 {
                        &warmup_bytes
                    } else {
                        &total_bytes
                    };
                    counter.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                }

                // An empty body would spin the loop without measuring anything.
                if !got_data {
                    return;
                }
            }
        });

        handles.push(handle);
    }

    let result = measure_window(
        &total_bytes,
        &warmup_bytes,
        &measure_start,
        &stop,
        duration_s,
        stable_stop,
        on_progress,
    )
    .await;

    for handle in handles {
        let _ = handle.await;
    }

    result
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
/// A [`Transfer`]. With `stable_stop`, the window can end before `duration_s` once the speed settles.
///
/// # Examples
///
/// ```
/// # use std::sync::Arc;
/// # use tokio;
/// # async fn _run() {
/// let t = crate::tester::upload("https://speed.cloudflare.com/__up", 1.0, 2, false, None).await;
/// assert!(t.mbps >= 0.0);
/// # }
/// ```
pub async fn upload(
    url: &str,
    duration_s: f64,
    streams: usize,
    stable_stop: bool,
    on_progress: Option<ProgressCallback>,
) -> Transfer {
    let data: Arc<Vec<u8>> = Arc::new(
        (0..UPLOAD_CHUNK_BYTES)
            .map(|_| rand::random::<u8>())
            .collect(),
    );

    let client = build_speed_client(60);

    let stop = Arc::new(AtomicBool::new(false));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let warmup_bytes = Arc::new(AtomicU64::new(0));
    let measure_start = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();

    for _ in 0..streams {
        let client = client.clone();
        let url = url.to_string();
        let stop = stop.clone();
        let total_bytes = total_bytes.clone();
        let warmup_bytes = warmup_bytes.clone();
        let measure_start = measure_start.clone();
        let data = data.clone();

        let handle = task::spawn(async move {
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                let len = data.len() as u64;
                // Bytes this request added to `total_bytes`, so a failed
                // request can take back what it never delivered.
                let sent = Arc::new(AtomicU64::new(0));

                // Stream the body in small pieces and count each one as it is
                // handed to the socket. Counting only after the response
                // arrives left the speed at 0 until the first full chunk
                // finished, which takes seconds on a slow uplink.
                let body_stream = futures::stream::unfold(0usize, {
                    let data = data.clone();
                    let stop = stop.clone();
                    let total_bytes = total_bytes.clone();
                    let warmup_bytes = warmup_bytes.clone();
                    let measure_start = measure_start.clone();
                    let sent = sent.clone();
                    move |offset| {
                        let data = data.clone();
                        let stop = stop.clone();
                        let total_bytes = total_bytes.clone();
                        let warmup_bytes = warmup_bytes.clone();
                        let measure_start = measure_start.clone();
                        let sent = sent.clone();
                        async move {
                            if offset >= data.len() || stop.load(Ordering::Relaxed) {
                                return None;
                            }
                            let end = (offset + UPLOAD_PIECE_BYTES).min(data.len());
                            let piece = data[offset..end].to_vec();
                            if measure_start.load(Ordering::Relaxed) != 0 {
                                total_bytes.fetch_add(piece.len() as u64, Ordering::Relaxed);
                                sent.fetch_add(piece.len() as u64, Ordering::Relaxed);
                            } else {
                                warmup_bytes.fetch_add(piece.len() as u64, Ordering::Relaxed);
                            }
                            Some((Ok::<_, std::io::Error>(piece), end))
                        }
                    }
                });

                let result = client
                    .post(&url)
                    .header(reqwest::header::CONTENT_LENGTH, len)
                    .body(reqwest::Body::wrap_stream(body_stream))
                    .send()
                    .await;

                // Requests cut short by `stop` fail by design; keep their bytes.
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                match result {
                    Ok(r) if r.status().is_success() => {}
                    // Failed or rejected: uncount the bytes and end this stream
                    // instead of retrying in a tight loop.
                    _ => {
                        total_bytes.fetch_sub(sent.load(Ordering::Relaxed), Ordering::Relaxed);
                        return;
                    }
                }
            }
        });

        handles.push(handle);
    }

    let result = measure_window(
        &total_bytes,
        &warmup_bytes,
        &measure_start,
        &stop,
        duration_s,
        stable_stop,
        on_progress,
    )
    .await;

    for handle in handles {
        let _ = handle.await;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- jitter ---

    #[test]
    fn jitter_fewer_than_two_samples_is_zero() {
        assert_eq!(jitter(&[]), 0.0);
        assert_eq!(jitter(&[5.0]), 0.0);
    }

    #[test]
    fn jitter_steady_samples_is_zero() {
        assert_eq!(jitter(&[4.0, 4.0, 4.0]), 0.0);
    }

    #[test]
    fn jitter_is_mean_consecutive_difference() {
        // |6-4| + |3-6| + |7-3| = 9, over 3 gaps
        assert_eq!(jitter(&[4.0, 6.0, 3.0, 7.0]), 3.0);
    }

    #[test]
    fn jitter_single_outlier_is_damped() {
        // max - min would report 40 ms here.
        let j = jitter(&[44.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0, 4.0]);
        assert!((j - 40.0 / 9.0).abs() < 1e-9);
    }

    // --- is_stable ---

    #[test]
    fn is_stable_needs_enough_samples() {
        assert!(!is_stable(&[50.0, 50.0, 50.0]));
    }

    #[test]
    fn is_stable_flat_tail_is_stable() {
        assert!(is_stable(&[10.0, 40.0, 50.0, 50.5, 50.2, 49.8]));
    }

    #[test]
    fn is_stable_rising_or_zero_is_not() {
        assert!(!is_stable(&[40.0, 44.0, 48.0, 52.0]));
        assert!(!is_stable(&[0.0, 0.0, 0.0, 0.0]));
    }

    // --- build_client ---

    #[test]
    fn build_client_does_not_panic_with_small_timeout() {
        let _client = build_client(1);
    }

    #[test]
    fn build_client_does_not_panic_with_large_timeout() {
        let _client = build_client(300);
    }

    #[test]
    fn build_client_zero_timeout_does_not_panic() {
        let _client = build_client(0);
    }

    // --- build_speed_client ---

    #[test]
    fn build_speed_client_does_not_panic() {
        let _client = build_speed_client(30);
    }

    #[test]
    fn build_speed_client_with_short_timeout() {
        let _client = build_speed_client(5);
    }

    #[test]
    fn build_speed_client_with_long_timeout() {
        let _client = build_speed_client(120);
    }

    // Regression: multiple clients can be constructed independently
    #[test]
    fn build_multiple_clients_succeed() {
        let _c1 = build_speed_client(10);
        let _c2 = build_speed_client(20);
        let _c3 = build_client(10);
    }

    // --- SERVER constant ---

    #[test]
    fn server_constant_name_is_cloudflare() {
        assert_eq!(SERVER.name, "Cloudflare");
    }

    #[test]
    fn server_constant_download_url_contains_placeholder() {
        assert!(SERVER.download_url.contains("{bytes}"));
    }

    #[test]
    fn server_constant_upload_url_is_nonempty() {
        assert!(!SERVER.upload_url.is_empty());
    }

    #[test]
    fn server_constant_host_is_nonempty() {
        assert!(!SERVER.host.is_empty());
    }
}
