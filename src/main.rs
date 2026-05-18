pub mod bufferbloat;
pub mod info;
pub mod output;
pub mod regional;
pub mod tester;
pub mod verdict;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "tracerate",
    version = "1.1.0",
    about = "A no-nonsense CLI internet speed tester"
)]
struct Cli {
    #[arg(long, default_value_t = false, help = "Skip upload, bufferbloat, and regional latency tests")]
    quick: bool,

    #[arg(long, default_value_t = 15.0, hide_default_value = true, help = "Duration in seconds for each download/upload measurement [default: 15s]")]
    duration: f64,

    #[arg(long, default_value_t = 6, help = "Number of parallel streams for download/upload (more streams = higher saturation)")]
    streams: usize,

    #[arg(long, default_value = "pretty", help = "Output format: 'pretty' for human-readable, 'json' for machine-readable")]
    output: String,

    #[arg(short, long, default_value_t = false, help = "Show extra detail such as data transferred during bufferbloat test")]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if cli.output != "pretty" && cli.output != "json" {
        eprintln!("--output must be 'pretty' or 'json'");
        std::process::exit(1);
    }

    let duration_s = if cli.quick { 10.0 } else { cli.duration };
    let test_upload = !cli.quick;
    let test_extras = !cli.quick;
    let quiet = cli.output == "json";

    if cli.output == "pretty" {
        output::print_header();
    }

    let spinner = indicatif::ProgressBar::new_spinner();
    if cli.output == "pretty" {
        spinner.set_style(
            indicatif::ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        spinner.set_message("Looking up your ISP...");
        spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    }

    let info = info::get_ip_info().await;
    let dns_ms = info::measure_dns(tester::SERVER.host).await;

    if cli.output == "pretty" {
        spinner.set_message("Measuring latency...");
    }

    let (ping_ms, loss_pct, jitter_ms) =
        tester::ping(tester::SERVER.host, tester::SERVER.port, 5).await;

    let (download_mbps, download_bytes) = if quiet {
        tester::download(tester::SERVER.download_url, duration_s, cli.streams, None).await
    } else {
        spinner.finish_and_clear();

        let pb = indicatif::ProgressBar::new(1000);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("  Downloading —  {bar:20.cyan}  {msg}")
                .unwrap()
                .progress_chars("▰▱"),
        );
        pb.set_message("…");

        let pb_clone = pb.clone();
        let result = tester::download(
            tester::SERVER.download_url,
            duration_s,
            cli.streams,
            Some(Box::new(move |total_bytes, elapsed| {
                let ratio = (elapsed / duration_s).min(1.0);
                pb_clone.set_position((ratio * 1000.0) as u64);
                if elapsed > 0.0 {
                    let speed_mbps = (total_bytes as f64 * 8.0) / elapsed / 1_000_000.0;
                    pb_clone.set_message(format!("{:.2} Mbps  {:.1}s", speed_mbps, elapsed));
                }
            })),
        )
        .await;

        pb.finish_and_clear();
        result
    };

    // New spinner for post-download phases (original was finish_and_clear'd above)
    let mut spinner = if !quiet {
        let s = indicatif::ProgressBar::new_spinner();
        s.set_style(
            indicatif::ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        s.enable_steady_tick(std::time::Duration::from_millis(100));
        s
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

            let pb = indicatif::ProgressBar::new(1000);
            pb.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template("  Uploading   —  {bar:20.cyan}  {msg}")
                    .unwrap()
                    .progress_chars("▰▱"),
            );
            pb.set_message("…");

            let pb_clone = pb.clone();
            let (speed, bytes) = tester::upload(
                tester::SERVER.upload_url,
                duration_s,
                cli.streams,
                Some(Box::new(move |total_bytes, elapsed| {
                    let ratio = (elapsed / duration_s).min(1.0);
                    pb_clone.set_position((ratio * 1000.0) as u64);
                    if elapsed > 0.0 {
                        let speed_mbps = (total_bytes as f64 * 8.0) / elapsed / 1_000_000.0;
                        pb_clone.set_message(format!("{:.2} Mbps  {:.1}s", speed_mbps, elapsed));
                    }
                })),
            )
            .await;

            pb.finish_and_clear();

            let s = indicatif::ProgressBar::new_spinner();
            s.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .unwrap(),
            );
            s.enable_steady_tick(std::time::Duration::from_millis(100));
            spinner = s;

            (Some(speed), bytes)
        }
    } else {
        (None, 0)
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
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
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
