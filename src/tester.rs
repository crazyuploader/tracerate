use futures::StreamExt;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
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

/// Unspecified address of the IP family forced by `-4`/`-6`. Every HTTP client
/// binds to it and every ping picks a matching address. Global because clients
/// are built in several modules that would otherwise all need a new parameter.
pub static IP_FAMILY: OnceLock<IpAddr> = OnceLock::new();

/// True when `addr` matches the forced IP family, or no family is forced.
fn family_ok(addr: &SocketAddr) -> bool {
    IP_FAMILY
        .get()
        .is_none_or(|f| f.is_ipv4() == addr.is_ipv4())
}

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
        .local_address(IP_FAMILY.get().copied())
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
        .local_address(IP_FAMILY.get().copied())
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
        Ok(mut addrs) => match addrs.find(family_ok) {
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

/// True when `host` resolves to an address of the forced IP family, or of any
/// family when none is forced. False also when the lookup fails, so a dead host
/// is skipped the same way.
pub async fn has_family_addr(host: &str, port: u16) -> bool {
    match tokio::net::lookup_host((host, port)).await {
        Ok(mut addrs) => addrs.any(|a| family_ok(&a)),
        Err(_) => false,
    }
}

/// Measures a single TCP round-trip time to `host:port`, in milliseconds.
///
/// The host is resolved before the timer starts, to an address of the forced
/// IP family if any. `None` on resolve failure, timeout, or connect error.
pub async fn tcp_ping_once(host: &str, port: u16) -> Option<f64> {
    let addr = tokio::net::lookup_host((host, port))
        .await
        .ok()?
        .find(family_ok)?;
    tcp_connect_ms(addr).await
}

/// Times one TCP connect to `addr`, with a 3 second timeout.
async fn tcp_connect_ms(addr: SocketAddr) -> Option<f64> {
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
    /// TCP connect times (ms) to the probe address, taken during the window.
    pub loaded_ms: Vec<f64>,
}

/// How a download or upload measurement runs.
pub struct Window {
    /// Longest the measurement may run, in seconds.
    pub duration_s: f64,
    /// End early once the running average speed settles.
    pub stable_stop: bool,
    /// End once warm-up plus measured bytes reach this many bytes.
    pub max_bytes: Option<u64>,
    /// Address pinged every 200 ms during the window, for latency under load.
    pub probe: Option<SocketAddr>,
}

/// The data cap is checked only after this many seconds, so a tiny window
/// cannot turn a few bytes into an absurd speed.
const CAP_MIN_S: f64 = 1.0;

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
/// then signals `stop`. The window ends at `w.duration_s`, or earlier on a
/// stable stop or when `w.max_bytes` is used up.
async fn measure_window(
    total_bytes: &AtomicU64,
    warmup_bytes: &AtomicU64,
    measure_start: &AtomicU64,
    stop: &AtomicBool,
    w: &Window,
    on_progress: Option<ProgressCallback>,
) -> Transfer {
    tokio::time::sleep(std::time::Duration::from_secs_f64(1.5)).await;

    total_bytes.store(0, Ordering::Relaxed);
    let start_instant = Instant::now();
    measure_start.store(1, Ordering::Relaxed);

    // Latency under load: ping while the transfer saturates the link.
    let loaded = Arc::new(Mutex::new(Vec::new()));
    let sampler = w.probe.map(|addr| {
        let loaded = loaded.clone();
        task::spawn(async move {
            loop {
                if let Some(ms) = tcp_connect_ms(addr).await {
                    loaded.lock().unwrap_or_else(|e| e.into_inner()).push(ms);
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        })
    });

    let end_at = start_instant + std::time::Duration::from_secs_f64(w.duration_s);
    let mut avgs = Vec::new();

    while Instant::now() < end_at {
        let bytes = total_bytes.load(Ordering::Relaxed);
        let elapsed = start_instant.elapsed().as_secs_f64();
        if let Some(ref cb) = on_progress {
            cb(bytes, elapsed);
        }
        if let Some(max) = w.max_bytes {
            if elapsed >= CAP_MIN_S && bytes + warmup_bytes.load(Ordering::Relaxed) >= max {
                break;
            }
        }
        if w.stable_stop && elapsed >= (avgs.len() + 1) as f64 {
            avgs.push(util::bytes_to_mbps(bytes, elapsed));
            if elapsed >= STABLE_MIN_S && is_stable(&avgs) {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    stop.store(true, Ordering::Relaxed);
    // Abort rather than await: a connect stuck under load could hold us 3s.
    if let Some(h) = sampler {
        h.abort();
    }

    let elapsed = start_instant.elapsed().as_secs_f64();
    let bytes = total_bytes.load(Ordering::Relaxed);
    let warmup_bytes = warmup_bytes.load(Ordering::Relaxed);
    let loaded_ms = std::mem::take(&mut *loaded.lock().unwrap_or_else(|e| e.into_inner()));

    if elapsed <= 0.0 || bytes == 0 {
        return Transfer {
            mbps: 0.0,
            bytes: 0,
            warmup_bytes,
            elapsed_s: elapsed,
            loaded_ms,
        };
    }

    Transfer {
        mbps: util::round2(util::bytes_to_mbps(bytes, elapsed)),
        bytes,
        warmup_bytes,
        elapsed_s: elapsed,
        loaded_ms,
    }
}

/// Measures download throughput by concurrently fetching a URL template across multiple streams for a fixed duration.
///
/// The `url_template` must contain the substring `"{bytes}"` which will be replaced with a large value to trigger a full-range download. The function spawns `streams` concurrent workers that read response bodies and accumulate received bytes; if `on_progress` is provided it will be invoked periodically with the current total bytes and elapsed seconds.
///
/// # Parameters
///
/// - `url_template`: URL template containing `"{bytes}"` to be substituted before requests.
/// - `streams`: Number of concurrent download streams to run.
/// - `w`: duration, stable stop, data cap, and latency probe.
/// - `on_progress`: Optional callback invoked periodically as `cb(total_bytes, elapsed_secs)`.
///
/// # Returns
///
/// A [`Transfer`]. The window can end before `w.duration_s`; see [`Window`].
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
///     let w = Window { duration_s: 1.0, stable_stop: false, max_bytes: None, probe: None };
///     let t = download(template, 1, &w, None).await;
///     assert!(t.mbps >= 0.0);
/// }
/// ```
pub async fn download(
    url_template: &str,
    streams: usize,
    w: &Window,
    on_progress: Option<ProgressCallback>,
) -> Transfer {
    let url = url_template.replace("{bytes}", "1000000000");
    // The client timeout covers the whole response body, so it must outlast
    // the warmup plus the measurement window. A fixed 30s killed every stream
    // mid-test for --duration above ~28s.
    let client = build_speed_client(w.duration_s.ceil() as u64 + 10);

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
        w,
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
/// A [`Transfer`]. The window can end before `w.duration_s`; see [`Window`].
///
/// # Examples
///
/// ```
/// # use std::sync::Arc;
/// # use tokio;
/// # async fn _run() {
/// let w = crate::tester::Window { duration_s: 1.0, stable_stop: false, max_bytes: None, probe: None };
/// let t = crate::tester::upload("https://speed.cloudflare.com/__up", 2, &w, None).await;
/// assert!(t.mbps >= 0.0);
/// # }
/// ```
pub async fn upload(
    url: &str,
    streams: usize,
    w: &Window,
    on_progress: Option<ProgressCallback>,
) -> Transfer {
    let data: Arc<Vec<u8>> = Arc::new(
        (0..UPLOAD_CHUNK_BYTES)
            .map(|_| rand::random::<u8>())
            .collect(),
    );

    let client = build_speed_client(60);

    // A server that rejects uploads only answers after the body is sent, so the
    // bytes still in flight when the window ends would read as real speed.
    // One small POST up front catches a bad --upload-url.
    match client.post(url).body(vec![0u8; 1024]).send().await {
        Ok(r) if r.status().is_success() => {}
        _ => {
            return Transfer {
                mbps: 0.0,
                bytes: 0,
                warmup_bytes: 0,
                elapsed_s: 0.0,
                loaded_ms: Vec::new(),
            }
        }
    }

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
        w,
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
