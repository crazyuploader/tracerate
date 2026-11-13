pub mod bufferbloat;
pub mod info;
pub mod output;
pub mod regional;
pub mod tester;
pub mod util;
pub mod verdict;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "tracerate",
    version,
    about = "A no-nonsense CLI internet speed tester"
)]
struct Cli {
    #[arg(
        long,
        default_value_t = false,
        help = "Skip upload, bufferbloat, and regional latency tests"
    )]
    quick: bool,

    #[arg(
        long,
        default_value_t = 15.0,
        value_parser = parse_duration,
        hide_default_value = true,
        help = "Duration in seconds for each download/upload measurement [default: 15s]"
    )]
    duration: f64,

    #[arg(
        long,
        default_value_t = 16,
        value_parser = parse_streams,
        help = "Number of parallel streams for download/upload (more streams = higher saturation)"
    )]
    streams: usize,

    #[arg(
        long,
        default_value = "pretty",
        value_parser = ["pretty", "json"],
        help = "Output format: 'pretty' for human-readable, 'json' for machine-readable"
    )]
    output: String,

    #[arg(
        long,
        default_value_t = false,
        help = "Run a combined download+upload test simultaneously after sequential tests"
    )]
    combined: bool,

    #[arg(
        long,
        default_value = tester::SERVER.download_url,
        value_parser = parse_download_url,
        help = "Download URL; must contain {bytes}. Also sets the ping/DNS host (bufferbloat still uses Cloudflare)"
    )]
    download_url: String,

    #[arg(
        long,
        default_value = tester::SERVER.upload_url,
        value_parser = parse_http_url,
        help = "Upload URL (POST target)"
    )]
    upload_url: String,

    #[arg(long, default_value_t = false, help = "Disable colored output")]
    no_color: bool,

    #[arg(
        short,
        long,
        default_value_t = false,
        help = "Show extra detail such as data transferred during bufferbloat test"
    )]
    verbose: bool,
}

fn parse_duration(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if (1.0..=300.0).contains(&v) {
        Ok(v)
    } else {
        Err("must be between 1 and 300 seconds".into())
    }
}

fn parse_http_url(s: &str) -> Result<String, String> {
    let u = reqwest::Url::parse(s).map_err(|e| e.to_string())?;
    match (u.scheme(), u.host_str()) {
        ("http" | "https", Some(_)) => Ok(s.to_string()),
        _ => Err("must be an http(s) URL with a host".into()),
    }
}

fn parse_download_url(s: &str) -> Result<String, String> {
    if !s.contains("{bytes}") {
        return Err("must contain {bytes}".into());
    }
    parse_http_url(&s.replace("{bytes}", "1"))?;
    Ok(s.to_string())
}

fn parse_streams(s: &str) -> Result<usize, String> {
    match s.parse::<usize>() {
        Ok(v) if (1..=32).contains(&v) => Ok(v),
        _ => Err("must be an integer between 1 and 32".into()),
    }
}

/// Creates a progress bar prefixed with `prefix` and configured for throughput-style updates.
///
/// The returned `indicatif::ProgressBar` has a total length of 1000, uses a cyan 20-character
/// bar with the given `prefix` followed by the current message, and is initialized with the
/// message "…".
///
/// # Examples
///
/// ```
/// let pb = make_progress_bar("  Downloading —");
/// assert_eq!(pb.length(), Some(1000));
/// ```
fn make_progress_bar(prefix: &str) -> indicatif::ProgressBar {
    let pb = indicatif::ProgressBar::new(1000);
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template(&format!("{}  {{bar:20.cyan}}  {{msg}}", prefix))
            .expect("invalid progress bar template")
            .progress_chars("▰▱"),
    );
    pb.set_message("…");
    pb
}

fn speed_progress_cb(
    pb: indicatif::ProgressBar,
    duration_s: f64,
) -> Box<dyn Fn(u64, f64) + Send + Sync> {
    Box::new(move |bytes: u64, elapsed: f64| {
        pb.set_position(((elapsed / duration_s).min(1.0) * 1000.0) as u64);
        if elapsed > 0.0 {
            pb.set_message(format!(
                "{:.2} Mbps  {:.1}s",
                util::bytes_to_mbps(bytes, elapsed),
                elapsed
            ));
        }
    })
}

/// Creates a spinner-style indicatif progress bar configured with a cyan spinner and message.
///
/// The spinner is set to tick steadily every 100 milliseconds.
///
/// # Examples
///
/// ```
/// let spinner = make_spinner();
/// spinner.set_message("Starting...");
/// spinner.finish_and_clear();
/// ```
fn make_spinner() -> indicatif::ProgressBar {
    let s = indicatif::ProgressBar::new_spinner();
    s.set_style(
        indicatif::ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .expect("invalid spinner template"),
    );
    s.enable_steady_tick(std::time::Duration::from_millis(100));
    s
}

/// CLI entrypoint that runs the tracerate measurement flow and prints the results.
///
/// This function parses command-line arguments, performs latency, download, upload,
/// optional combined, bufferbloat, and regional tests according to the CLI flags,
/// assembles a JSON result and a human-friendly summary, and outputs either pretty
/// formatted text or a JSON object depending on the `--output` option.
///
/// # Examples
///
/// ```no_run
/// // Run the compiled binary from a shell to execute the full measurement flow:
/// // $ tracerate --quick --output json
/// ```
#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if cli.no_color {
        colored::control::set_override(false);
    }

    // Restore the cursor a half-drawn progress bar may have hidden, then exit 130.
    tokio::spawn(async {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprint!("\r\x1b[2K\x1b[?25h");
            std::process::exit(130);
        }
    });

    // Validated by the clap parsers above, so these cannot fail.
    let server_url = reqwest::Url::parse(&cli.download_url.replace("{bytes}", "1"))
        .expect("download_url validated by clap");
    let server_host = server_url
        .host_str()
        .unwrap_or(tester::SERVER.host)
        .to_string();
    let server_port = server_url
        .port_or_known_default()
        .unwrap_or(tester::SERVER.port);

    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).ok();

    if cli.quick && cli.combined {
        eprintln!(
            "error: --combined cannot be used with --quick (upload is skipped in quick mode)"
        );
        std::process::exit(1);
    }

    let duration_s = if cli.quick { 10.0 } else { cli.duration };
    let test_upload = !cli.quick;
    let test_extras = !cli.quick;
    let test_combined = cli.combined;
    let quiet = cli.output == "json";

    if cli.output == "pretty" {
        output::print_header();
    }

    let spinner = if cli.output == "pretty" {
        let s = make_spinner();
        s.set_message("Looking up your ISP and measuring latency...");
        s
    } else {
        indicatif::ProgressBar::hidden()
    };

    let test_start = chrono::Local::now();
    let test_start_instant = std::time::Instant::now();

    // ISP lookup, DNS timing, and the ping baseline are independent — run
    // them concurrently.
    let (info, dns_ms, (ping_ms, loss_pct, jitter_ms)) = tokio::join!(
        info::get_ip_info(),
        info::measure_dns(&server_host),
        tester::ping(&server_host, server_port, 5),
    );

    let (download_mbps, download_bytes) = if quiet {
        tester::download(&cli.download_url, duration_s, cli.streams, None).await
    } else {
        spinner.finish_and_clear();

        let pb = make_progress_bar("  Downloading —");
        let result = tester::download(
            &cli.download_url,
            duration_s,
            cli.streams,
            Some(speed_progress_cb(pb.clone(), duration_s)),
        )
        .await;

        pb.finish_and_clear();
        result
    };

    // New spinner for post-download phases (original was finish_and_clear'd above)
    let mut spinner = if !quiet {
        make_spinner()
    } else {
        indicatif::ProgressBar::hidden()
    };

    let (upload_mbps, upload_bytes) = if test_upload {
        if quiet {
            let (s, b) = tester::upload(&cli.upload_url, duration_s, cli.streams, None).await;
            (Some(s), b)
        } else {
            spinner.finish_and_clear();

            let pb = make_progress_bar("  Uploading   —");
            let (speed, bytes) = tester::upload(
                &cli.upload_url,
                duration_s,
                cli.streams,
                Some(speed_progress_cb(pb.clone(), duration_s)),
            )
            .await;

            pb.finish_and_clear();
            spinner = make_spinner();

            (Some(speed), bytes)
        }
    } else {
        (None, 0)
    };

    let (combined_dl_mbps, combined_ul_mbps, combined_bytes) = if test_combined {
        if quiet {
            let ((dl, dl_b), (ul, ul_b)) = tokio::join!(
                tester::download(&cli.download_url, duration_s, cli.streams, None),
                tester::upload(&cli.upload_url, duration_s, cli.streams, None),
            );
            (Some(dl), Some(ul), Some(dl_b + ul_b))
        } else {
            spinner.finish_and_clear();

            // Single bar: dl callback drives display, ul callback updates shared atomic.
            // Avoids MultiProgress cursor-tracking bugs during the 1.5s warmup sleep.
            let ul_bytes_shared = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
            let ul_bytes_for_cb = ul_bytes_shared.clone();

            let pb = make_progress_bar("  Combined   —");

            let pb_clone = pb.clone();
            let ul_bytes_for_dl = ul_bytes_shared.clone();

            let ((dl, dl_b), (ul, ul_b)) = tokio::join!(
                tester::download(
                    &cli.download_url,
                    duration_s,
                    cli.streams,
                    Some(Box::new(move |dl_bytes, elapsed| {
                        let ul_bytes = ul_bytes_for_dl.load(std::sync::atomic::Ordering::Relaxed);
                        let ratio = (elapsed / duration_s).min(1.0);
                        pb_clone.set_position((ratio * 1000.0) as u64);
                        if elapsed > 0.0 {
                            let total_mbps =
                                (dl_bytes + ul_bytes) as f64 * 8.0 / elapsed / 1_000_000.0;
                            pb_clone
                                .set_message(format!("{:.2} Mbps  {:.1}s", total_mbps, elapsed));
                        }
                    })),
                ),
                tester::upload(
                    &cli.upload_url,
                    duration_s,
                    cli.streams,
                    Some(Box::new(move |ul_bytes, _elapsed| {
                        ul_bytes_for_cb.store(ul_bytes, std::sync::atomic::Ordering::Relaxed);
                    })),
                ),
            );

            pb.finish_and_clear();
            spinner = make_spinner();

            (Some(dl), Some(ul), Some(dl_b + ul_b))
        }
    } else {
        (None, None, None)
    };

    let bufferbloat = if test_extras {
        spinner.set_message("Probing bufferbloat (saturating link)...");
        Some(bufferbloat::measure_bufferbloat(5.0, 4).await)
    } else {
        None
    };

    let regions = if test_extras {
        spinner.set_message("Pinging regional servers...");
        Some(regional::ping_regions().await)
    } else {
        None
    };

    let result = serde_json::json!({
        "name": tester::SERVER.name,
        "ping_ms": ping_ms,
        "packet_loss": loss_pct,
        "jitter_ms": jitter_ms,
        "download_mbps": download_mbps,
        "download_bytes": download_bytes,
        "upload_mbps": upload_mbps,
        "upload_bytes": upload_bytes,
        "combined_download_mbps": combined_dl_mbps,
        "combined_upload_mbps": combined_ul_mbps,
        "combined_bytes": combined_bytes,
        "error": null,
    });

    let summary = verdict::analyze(&result, bufferbloat.as_ref());

    spinner.finish_and_clear();

    let started_at = test_start.format("%Y-%m-%d %H:%M:%S").to_string();
    let test_duration = test_start_instant.elapsed().as_secs_f64().round() as u64;

    // Total failure (no download data at all) exits non-zero so scripts/cron can tell.
    let failed = download_mbps <= 0.0;

    if cli.output == "json" {
        let output = serde_json::json!({
            "info": info,
            "dns_ms": dns_ms,
            "started_at": started_at,
            "duration_s": test_duration,
            "result": result,
            "bufferbloat": bufferbloat,
            "regions": regions,
            "summary": summary,
        });
        match serde_json::to_string_pretty(&output) {
            Ok(s) => println!("{}", s),
            Err(e) => {
                eprintln!("error: failed to serialize output: {}", e);
                std::process::exit(1);
            }
        }
        if failed {
            std::process::exit(1);
        }
        return;
    }

    output::render(
        &info,
        dns_ms,
        &result,
        bufferbloat.as_ref(),
        regions.as_deref(),
        &summary,
        cli.verbose,
        &started_at,
        test_duration,
    );

    if failed {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- make_progress_bar ---

    #[test]
    fn parse_duration_bounds() {
        for ok in ["1", "15", "300", "2.5"] {
            assert!(parse_duration(ok).is_ok(), "{ok}");
        }
        for bad in ["0", "-1", "301", "NaN", "inf", "abc"] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn url_parsers() {
        assert!(parse_download_url("https://h.example/d?b={bytes}").is_ok());
        assert!(parse_download_url("https://h.example/d").is_err());
        assert!(parse_download_url("ftp://h.example/{bytes}").is_err());
        assert!(parse_http_url("https://h.example/up").is_ok());
        assert!(parse_http_url("nope").is_err());
    }

    #[test]
    fn parse_streams_bounds() {
        assert_eq!(parse_streams("1"), Ok(1));
        assert_eq!(parse_streams("32"), Ok(32));
        for bad in ["0", "33", "-1", "x"] {
            assert!(parse_streams(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn make_progress_bar_has_length_1000() {
        let pb = make_progress_bar("  Test —");
        assert_eq!(pb.length(), Some(1000));
    }

    #[test]
    fn make_progress_bar_initial_position_is_zero() {
        let pb = make_progress_bar("  Test —");
        assert_eq!(pb.position(), 0);
    }

    #[test]
    fn make_progress_bar_empty_prefix_does_not_panic() {
        let pb = make_progress_bar("");
        assert_eq!(pb.length(), Some(1000));
    }

    #[test]
    fn make_progress_bar_long_prefix_does_not_panic() {
        let prefix = "A".repeat(80);
        let pb = make_progress_bar(&prefix);
        assert_eq!(pb.length(), Some(1000));
    }

    // --- speed_progress_cb ---

    #[test]
    fn speed_progress_cb_sets_position_proportionally() {
        let pb = indicatif::ProgressBar::new(1000);
        let cb = speed_progress_cb(pb.clone(), 10.0);
        // 1 second into a 10-second duration → position = 100
        cb(1_000_000, 1.0);
        assert_eq!(pb.position(), 100);
    }

    #[test]
    fn speed_progress_cb_position_clamped_to_1000_when_over_duration() {
        let pb = indicatif::ProgressBar::new(1000);
        let cb = speed_progress_cb(pb.clone(), 5.0);
        // elapsed > duration: clamp to 1.0 → position = 1000
        cb(5_000_000, 10.0);
        assert_eq!(pb.position(), 1000);
    }

    #[test]
    fn speed_progress_cb_zero_elapsed_does_not_set_message() {
        let pb = indicatif::ProgressBar::new(1000);
        let cb = speed_progress_cb(pb.clone(), 10.0);
        // elapsed == 0 → position set to 0, but message not updated
        cb(0, 0.0);
        assert_eq!(pb.position(), 0);
    }

    #[test]
    fn speed_progress_cb_half_duration_gives_position_500() {
        let pb = indicatif::ProgressBar::new(1000);
        let cb = speed_progress_cb(pb.clone(), 20.0);
        // 10s / 20s = 0.5 → 500
        cb(0, 10.0);
        assert_eq!(pb.position(), 500);
    }

    #[test]
    fn speed_progress_cb_returns_callable_closure() {
        let pb = indicatif::ProgressBar::new(1000);
        let cb = speed_progress_cb(pb.clone(), 10.0);
        // Should be callable multiple times without panicking
        cb(0, 0.0);
        cb(1_000_000, 1.0);
        cb(5_000_000, 5.0);
        cb(10_000_000, 10.0);
    }

    // --- make_spinner ---

    #[test]
    fn make_spinner_creates_spinner_bar() {
        let s = make_spinner();
        // A spinner created with new_spinner() has no fixed length
        s.finish_and_clear();
    }

    #[test]
    fn make_spinner_can_set_and_finish() {
        let s = make_spinner();
        s.set_message("Testing...");
        s.finish_and_clear();
    }

    #[test]
    fn make_spinner_does_not_panic() {
        let _s = make_spinner();
        // Drop without finishing — should not panic
    }
}
