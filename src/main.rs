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
        value_parser = parse_duration,
        help = "Maximum seconds for each download/upload measurement; stops early once the speed is stable [default: 30s, 10s with --quick]"
    )]
    duration: Option<f64>,

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
        help = "Run download and upload simultaneously instead of one after the other"
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
        help = "Show extra detail: server IP, streams, ping range, test durations, bufferbloat data"
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

/// Same switch the report uses: off for --no-color, NO_COLOR, or a non-terminal stdout.
fn colors_on() -> bool {
    colored::control::SHOULD_COLORIZE.should_colorize()
}

/// Bar style in `color`; the bar and its message share the color.
/// `wide_msg` cuts the message to the terminal width, so a bar never wraps
/// and the line count used to erase the bars stays right.
fn bar_style(color: &str) -> indicatif::ProgressStyle {
    let c = if colors_on() {
        format!(".{color}")
    } else {
        String::new()
    };
    indicatif::ProgressStyle::default_bar()
        .template(&format!("{{prefix}}  {{bar:20{c}}}  {{wide_msg:{c}}}"))
        .expect("invalid progress bar template")
        .progress_chars("▰▱")
}

/// Creates a progress bar prefixed with `prefix` and configured for throughput-style updates.
///
/// The returned `indicatif::ProgressBar` has a total length of 1000, starts as a cyan
/// 20-character bar, and is initialized with the message "…".
///
/// # Examples
///
/// ```
/// let pb = make_progress_bar("  Downloading —");
/// assert_eq!(pb.length(), Some(1000));
/// ```
fn make_progress_bar(prefix: &str) -> indicatif::ProgressBar {
    let pb = indicatif::ProgressBar::new(1000)
        .with_style(bar_style("cyan"))
        .with_prefix(prefix.to_string());
    pb.set_message("…");
    pb
}

fn progress_msg(mbps: f64, elapsed: f64, bytes: u64) -> String {
    format!(
        "{:.2} Mbps  {:.1}s  {:.1} MB",
        mbps,
        elapsed,
        util::bytes_to_mb(bytes)
    )
}

/// Color for the live (last ~2s) speed relative to the run average so far.
fn speed_color(live_mbps: f64, avg_mbps: f64) -> &'static str {
    if live_mbps >= avg_mbps * 0.9 {
        "green"
    } else if live_mbps >= avg_mbps * 0.7 {
        "yellow"
    } else {
        "red"
    }
}

/// Leaves the bar on screen with the final speed and data used.
fn finish_bar(pb: &indicatif::ProgressBar, t: &tester::Transfer) {
    pb.finish_with_message(progress_msg(t.mbps, t.elapsed_s, t.bytes));
    // indicatif leaves the cursor on the finished line; move off it so the next bar starts below.
    if !pb.is_hidden() {
        eprintln!();
    }
}

fn speed_progress_cb(
    pb: indicatif::ProgressBar,
    duration_s: f64,
) -> Box<dyn Fn(u64, f64) + Send + Sync> {
    // (elapsed, bytes) samples covering the last two seconds, and the color in use.
    // Two seconds, not one, so a jittery link does not flip colors every tick.
    let state = std::sync::Mutex::new((std::collections::VecDeque::<(f64, u64)>::new(), "cyan"));
    Box::new(move |bytes: u64, elapsed: f64| {
        pb.set_position(((elapsed / duration_s).min(1.0) * 1000.0) as u64);
        if elapsed <= 0.0 {
            return;
        }
        let avg = util::bytes_to_mbps(bytes, elapsed);
        pb.set_message(progress_msg(avg, elapsed, bytes));

        let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
        let (window, color) = &mut *guard;
        window.push_back((elapsed, bytes));
        while window.len() > 1 && window[1].0 <= elapsed - 2.0 {
            window.pop_front();
        }
        // Speeds are noisy while streams ramp up; judge drops only after 5s.
        let (t0, b0) = window[0];
        if elapsed >= 5.0 && elapsed > t0 {
            // saturating: a failed upload request takes its bytes back.
            let live = util::bytes_to_mbps(bytes.saturating_sub(b0), elapsed - t0);
            let new = speed_color(live, avg);
            if new != *color {
                *color = new;
                pb.set_style(bar_style(new));
            }
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
            .template(if colors_on() {
                "{spinner:.cyan} {msg}"
            } else {
                "{spinner} {msg}"
            })
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

    let duration_s = cli.duration.unwrap_or(if cli.quick { 10.0 } else { 30.0 });
    let test_upload = !cli.quick;
    let test_extras = !cli.quick;
    let test_combined = cli.combined;
    let quiet = cli.output == "json";
    // Same check indicatif uses to decide whether bars are drawn.
    let console_shown = !quiet && std::io::IsTerminal::is_terminal(&std::io::stderr());
    // Hide the cursor while bars draw: indicatif pads lines to full width, leaving it blinking at the far right.
    if console_shown {
        eprint!("\x1b[?25l");
    }

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
    let (info, dns_ms, ping) = tokio::join!(
        info::get_ip_info(),
        info::measure_dns(&server_host),
        tester::ping(&server_host, server_port, 10),
    );

    spinner.finish_and_clear();

    let new_bar = |prefix: &str| {
        if quiet {
            indicatif::ProgressBar::hidden()
        } else {
            make_progress_bar(prefix)
        }
    };

    // Stable stop is off for --combined: one direction ending early would
    // leave the other measured without the load the mode exists for.
    let (download, upload) = if test_combined {
        let pb = new_bar("  Combined    —");
        // Download callback drives the bar with dl+ul bytes; upload callback only shares its count.
        let ul_bytes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let ul_for_dl = ul_bytes.clone();
        let cb = speed_progress_cb(pb.clone(), duration_s);
        let (dl, ul) = tokio::join!(
            tester::download(
                &cli.download_url,
                duration_s,
                cli.streams,
                false,
                Some(Box::new(move |b, e| cb(
                    b + ul_for_dl.load(std::sync::atomic::Ordering::Relaxed),
                    e
                ))),
            ),
            tester::upload(
                &cli.upload_url,
                duration_s,
                cli.streams,
                false,
                Some(Box::new(move |b, _| {
                    ul_bytes.store(b, std::sync::atomic::Ordering::Relaxed)
                })),
            ),
        );
        finish_bar(
            &pb,
            &tester::Transfer {
                mbps: dl.mbps + ul.mbps,
                bytes: dl.bytes + ul.bytes,
                warmup_bytes: 0,
                elapsed_s: dl.elapsed_s,
            },
        );
        (dl, Some(ul))
    } else {
        let pb = new_bar("  Downloading —");
        let dl = tester::download(
            &cli.download_url,
            duration_s,
            cli.streams,
            true,
            Some(speed_progress_cb(pb.clone(), duration_s)),
        )
        .await;
        finish_bar(&pb, &dl);

        let ul = if test_upload {
            let pb = new_bar("  Uploading   —");
            let ul = tester::upload(
                &cli.upload_url,
                duration_s,
                cli.streams,
                true,
                Some(speed_progress_cb(pb.clone(), duration_s)),
            )
            .await;
            finish_bar(&pb, &ul);
            Some(ul)
        } else {
            None
        };
        (dl, ul)
    };
    let download_mbps = download.mbps;
    let speed_bytes = download.bytes
        + download.warmup_bytes
        + upload.as_ref().map_or(0, |u| u.bytes + u.warmup_bytes);

    // Speed bars stay on screen while the remaining tests run; erased before the report.
    let bar_lines = if test_combined || !test_upload { 1 } else { 2 } + 1;
    let spinner = if !quiet {
        if console_shown {
            eprintln!();
        }
        make_spinner()
    } else {
        indicatif::ProgressBar::hidden()
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

    let upload_mbps = upload.as_ref().map(|u| u.mbps);
    let combined_bytes =
        test_combined.then(|| download.bytes + upload.as_ref().map_or(0, |u| u.bytes));
    let total_data_mb =
        util::bytes_to_mb(speed_bytes) + bufferbloat.as_ref().map_or(0.0, |b| b.data_used_mb);

    let result = serde_json::json!({
        "name": tester::SERVER.name,
        "mode": if test_combined { "combined" } else { "sequential" },
        "server_host": server_host,
        "server_ip": ping.ip.map(|ip| ip.to_string()),
        "streams": cli.streams,
        "ping_ms": ping.avg_ms,
        "ping_low_ms": ping.low_ms,
        "ping_high_ms": ping.high_ms,
        "packet_loss": ping.loss_pct,
        "jitter_ms": ping.jitter_ms,
        "download_mbps": download_mbps,
        "download_bytes": download.bytes,
        "download_duration_s": util::round1(download.elapsed_s),
        "upload_mbps": upload_mbps,
        "upload_bytes": upload.as_ref().map_or(0, |u| u.bytes),
        "upload_duration_s": upload.as_ref().map(|u| util::round1(u.elapsed_s)),
        "combined_download_mbps": combined_bytes.map(|_| download_mbps),
        "combined_upload_mbps": combined_bytes.and(upload_mbps),
        "combined_bytes": combined_bytes,
        "warmup_bytes": download.warmup_bytes + upload.as_ref().map_or(0, |u| u.warmup_bytes),
        "total_data_mb": util::round1(total_data_mb),
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

    // shortcut: below ~40 columns the prefix and bar alone wrap, leaving fragments; fine for real terminals.
    if console_shown {
        eprint!("\x1b[{bar_lines}A\x1b[J\x1b[?25h");
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
    fn speed_color_tiers() {
        assert_eq!(speed_color(100.0, 100.0), "green");
        assert_eq!(speed_color(90.0, 100.0), "green");
        assert_eq!(speed_color(80.0, 100.0), "yellow");
        assert_eq!(speed_color(50.0, 100.0), "red");
    }

    #[test]
    fn speed_progress_cb_survives_shrinking_bytes() {
        let pb = indicatif::ProgressBar::new(1000);
        let cb = speed_progress_cb(pb.clone(), 10.0);
        cb(5_000_000, 5.0);
        cb(1_000_000, 6.0);
    }

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
