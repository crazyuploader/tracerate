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
    version = "1.1.0",
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
        hide_default_value = true,
        help = "Duration in seconds for each download/upload measurement [default: 15s]"
    )]
    duration: f64,

    #[arg(
        long,
        default_value_t = 6,
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
        short,
        long,
        default_value_t = false,
        help = "Show extra detail such as data transferred during bufferbloat test"
    )]
    verbose: bool,
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

/// Creates a callback that updates the given progress bar with throughput (Mbps) and elapsed time.

///

/// The returned closure takes `(bytes, elapsed_seconds)` and:

/// - sets the bar position proportional to `elapsed_seconds / duration_s` (clamped to the bar range 0..=1000),

/// - updates the bar message to "`{Mbps} Mbps  {seconds}s`" when `elapsed_seconds > 0`.

///

/// # Examples

///

/// ```

/// use indicatif::ProgressBar;

///

/// let pb = ProgressBar::new(1000);

/// let cb = speed_progress_cb(pb.clone(), 10.0);

///

/// // simulate 1_000_000 bytes after 1 second into a 10s duration

/// cb(1_000_000, 1.0);

/// assert_eq!(pb.position(), 100); // (1.0 / 10.0) * 1000 == 100

/// ```
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

    let duration_s = if cli.quick { 10.0 } else { cli.duration };
    let test_upload = !cli.quick;
    let test_extras = !cli.quick;
    let test_combined = !cli.quick && cli.combined;
    let quiet = cli.output == "json";

    if cli.output == "pretty" {
        output::print_header();
    }

    let spinner = if cli.output == "pretty" {
        let s = make_spinner();
        s.set_message("Looking up your ISP...");
        s
    } else {
        indicatif::ProgressBar::hidden()
    };

    let info = info::get_ip_info().await;
    let dns_ms = info::measure_dns(tester::SERVER.host).await;

    spinner.set_message("Measuring latency...");

    let (ping_ms, loss_pct, jitter_ms) =
        tester::ping(tester::SERVER.host, tester::SERVER.port, 5).await;

    let (download_mbps, download_bytes) = if quiet {
        tester::download(tester::SERVER.download_url, duration_s, cli.streams, None).await
    } else {
        spinner.finish_and_clear();

        let pb = make_progress_bar("  Downloading —");
        let result = tester::download(
            tester::SERVER.download_url,
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
            let (s, b) =
                tester::upload(tester::SERVER.upload_url, duration_s, cli.streams, None).await;
            (Some(s), b)
        } else {
            spinner.finish_and_clear();

            let pb = make_progress_bar("  Uploading   —");
            let (speed, bytes) = tester::upload(
                tester::SERVER.upload_url,
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
                tester::download(tester::SERVER.download_url, duration_s, cli.streams, None),
                tester::upload(tester::SERVER.upload_url, duration_s, cli.streams, None),
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
                    tester::SERVER.download_url,
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
                    tester::SERVER.upload_url,
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
        Some(bufferbloat::measure_bufferbloat(5.0, 8).await)
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

    if cli.output == "json" {
        let output = serde_json::json!({
            "info": info,
            "dns_ms": dns_ms,
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
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- make_progress_bar ---

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
