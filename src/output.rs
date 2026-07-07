use colored::Colorize;

use crate::info::InfoResult;
use crate::regional::RegionResult;
use crate::util;
use crate::verdict::VerdictResult;

/// Prints a styled header for the tracerate CLI to stdout.
///
/// # Examples
///
/// ```
/// // Produces a blank line, a styled "tracerate · network diagnostics" line, and another blank line.
/// print_header();
/// ```
pub fn print_header() {
    println!();
    println!(
        "{} {}",
        "tracerate".bold(),
        "· network diagnostics".dimmed()
    );
    println!();
}

/// Format a speed, auto-scaling to Gbps at or above 1000 Mbps.
fn fmt_speed(mbps: f64) -> String {
    if mbps >= 1000.0 {
        format!("{:.2} Gbps", mbps / 1000.0)
            .bold()
            .cyan()
            .to_string()
    } else {
        format!("{:.2} Mbps", mbps).bold().cyan().to_string()
    }
}

fn bar(value: f64, max_value: f64, width: usize) -> String {
    if max_value <= 0.0 {
        return "▱".repeat(width);
    }
    let ratio = value.min(max_value) / max_value;
    let n = (ratio * width as f64).round() as usize;
    "▰".repeat(n) + &"▱".repeat(width - n)
}

fn section(title: &str) {
    println!("{}", title.bold().cyan());
    println!("{}", "─".repeat(56).dimmed());
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    info: &InfoResult,
    dns_ms: Option<f64>,
    r: &serde_json::Value,
    bb: Option<&crate::bufferbloat::BufferbloatResult>,
    regions: Option<&[RegionResult]>,
    summary: &VerdictResult,
    verbose: bool,
    started_at: &str,
    duration_s: u64,
) {
    render_connection(info, dns_ms, started_at, duration_s);
    render_speed(r);

    if let Some(bb) = bb {
        render_bufferbloat(bb, verbose);
    }

    if let Some(regions) = regions {
        render_regions(regions);
    }

    render_verdict(&summary.status, &summary.summary);
}

fn render_connection(info: &InfoResult, dns_ms: Option<f64>, started_at: &str, duration_s: u64) {
    let isp = info.isp.as_deref().unwrap_or("unknown");
    let asn = info.asn.as_deref().unwrap_or("");
    let city = info.city.as_deref().unwrap_or("?");
    let country = info.country.as_deref().unwrap_or("?");
    let colo = info.colo.as_deref().unwrap_or("?");
    let colo_city = info.colo_city.as_deref().unwrap_or("");
    let ip = info.ip.as_deref().unwrap_or("?");

    let edge = if colo_city.is_empty() {
        format!(
            "Cloudflare {} {}",
            colo.bold(),
            format!("({})", colo).dimmed()
        )
    } else {
        format!(
            "Cloudflare {} {}",
            colo.bold(),
            format!("({})", colo_city).dimmed()
        )
    };

    println!(
        "  {}   {}   {}",
        format!("{:<6}", "ISP").dimmed(),
        isp.bold(),
        asn.to_string().dimmed()
    );
    println!(
        "  {}   {}, {}  {}  {}",
        format!("{:<6}", "Where").dimmed(),
        city,
        country,
        "→".dimmed(),
        edge
    );

    // A failed lookup renders as an explicit "DNS failed", not a dim 0.0 ms.
    let dns_segment = match dns_ms {
        None => "DNS failed".red().to_string(),
        Some(ms) => {
            let dns_str = format!("{:.2} ms", ms);
            let dns_colored = if ms < 50.0 {
                dns_str.bold().cyan()
            } else if ms < 150.0 {
                dns_str.bold().yellow()
            } else {
                dns_str.bold().red()
            };
            format!("{} {}", "· DNS".dimmed(), dns_colored)
        }
    };

    println!(
        "  {}   {}   {}",
        format!("{:<6}", "IP").dimmed(),
        ip.yellow(),
        dns_segment
    );
    println!(
        "  {}   {}",
        format!("{:<6}", "Tested").dimmed(),
        format!("{}   ·  {}s", started_at, duration_s).dimmed()
    );
    println!();
}

/// Renders the "Speed" diagnostic section from a JSON value and prints formatted, colored output to stdout.
///
/// Expects `r` to be a JSON object containing numeric fields typically produced by the speed test:
/// `download_mbps`, `upload_mbps`, `download_bytes`, `upload_bytes`, `combined_download_mbps`,
/// `combined_upload_mbps`, `combined_bytes`, `ping_ms`, `jitter_ms`, and `packet_loss`. Missing fields
/// are treated as zero; upload and combined metrics are shown only when present.
///
/// # Examples
///
/// ```
/// use serde_json::json;
///
/// let data = json!({
///     "download_mbps": 85.3,
///     "upload_mbps": 12.7,
///     "download_bytes": 134217728,
///     "upload_bytes": 67108864,
///     "combined_download_mbps": 85.3,
///     "combined_upload_mbps": 12.7,
///     "combined_bytes": 201326592,
///     "ping_ms": 12.34,
///     "jitter_ms": 0.56,
///     "packet_loss": 0.0
/// });
///
/// // Prints the formatted Speed section to stdout
/// render_speed(&data);
/// ```
fn render_speed(r: &serde_json::Value) {
    section("Speed");

    let dl = r
        .get("download_mbps")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let ul = r.get("upload_mbps").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let dl_bytes = r
        .get("download_bytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let ul_bytes = r.get("upload_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
    let ping = r.get("ping_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let jitter = r.get("jitter_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let loss = r.get("packet_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);

    let scale = dl.max(ul).max(100.0);

    println!(
        "  {}   {}   {}   {}",
        format!("{:<8}", "Download").dimmed(),
        bar(dl, scale, 20).cyan(),
        fmt_speed(dl),
        format!("({:.1} MB)", util::bytes_to_mb(dl_bytes)).dimmed()
    );

    if r.get("upload_mbps").and_then(|v| v.as_f64()).is_some() {
        let ratio = if dl > 0.0 { ul / dl } else { 0.0 };
        println!(
            "  {}   {}   {}   {}   {}",
            format!("{:<8}", "Upload").dimmed(),
            bar(ul, scale, 20).cyan(),
            fmt_speed(ul),
            format!("({:.1} MB)", util::bytes_to_mb(ul_bytes)).dimmed(),
            format!("↑/↓ {:.2}x", ratio).dimmed()
        );
    }

    let combined_dl = r.get("combined_download_mbps").and_then(|v| v.as_f64());
    let combined_ul = r.get("combined_upload_mbps").and_then(|v| v.as_f64());
    let combined_bytes = r
        .get("combined_bytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if let (Some(cdl), Some(cul)) = (combined_dl, combined_ul) {
        let total = cdl + cul;
        println!(
            "  {}   {}   {}   {}",
            format!("{:<8}", "Combined").dimmed(),
            bar(total, scale * 2.0, 20).cyan(),
            fmt_speed(total),
            format!("({:.1} MB)", util::bytes_to_mb(combined_bytes)).dimmed(),
        );
    }

    // "conn. fail" is honest labeling: this is the TCP connect-failure rate,
    // not true packet loss (a lost SYN is retried by the kernel).
    let loss_str = if loss > 0.0 {
        format!("· {:.1}% conn. fail", loss)
            .bold()
            .red()
            .to_string()
    } else {
        "· 0% conn. fail".green().to_string()
    };

    println!(
        "  {}   {}   {}",
        format!("{:<8}", "Ping").dimmed(),
        format!("{:.2} ms", ping).bold().cyan(),
        loss_str
    );
    println!(
        "  {}   {}",
        format!("{:<8}", "Jitter").dimmed(),
        format!("{:.2} ms", jitter).bold().cyan()
    );
    println!();
}

fn render_bufferbloat(bb: &crate::bufferbloat::BufferbloatResult, verbose: bool) {
    section("Bufferbloat");

    let grade_color = match bb.grade.as_str() {
        "A+" | "A" => "green",
        "B" => "cyan",
        "C" | "D" => "yellow",
        "F" => "red",
        _ => "dim",
    };

    let grade_str = match grade_color {
        "green" => bb.grade.bold().green().to_string(),
        "cyan" => bb.grade.bold().cyan().to_string(),
        "yellow" => bb.grade.bold().yellow().to_string(),
        "red" => bb.grade.bold().red().to_string(),
        _ => bb.grade.dimmed().to_string(),
    };

    println!(
        "  {}   {}",
        format!("{:<6}", "Idle").dimmed(),
        format!("{:.2} ms", bb.idle_ms).bold().cyan()
    );
    println!(
        "  {}   {}   {} {}   {} {}",
        format!("{:<6}", "Loaded").dimmed(),
        format!("{:.2} ms", bb.loaded_ms).bold().cyan(),
        "Δ".dimmed(),
        format!("+{:.2} ms", bb.delta_ms).bold().cyan(),
        "Grade".dimmed(),
        grade_str
    );
    if verbose {
        println!(
            "  {}   {:.1} MB used",
            format!("{:<6}", "Data").dimmed(),
            bb.data_used_mb
        );
    }
    println!();
}

fn render_regions(regions: &[RegionResult]) {
    section("Regional latency");

    let reachable: Vec<&RegionResult> = regions.iter().filter(|r| r.ms > 0.0).collect();
    let scale = reachable.iter().map(|r| r.ms).fold(200.0, f64::max);

    let mut ordered = regions.to_vec();
    ordered.sort_by(|a, b| {
        let a_val = if a.ms > 0.0 { a.ms } else { 1e9 };
        let b_val = if b.ms > 0.0 { b.ms } else { 1e9 };
        a_val.partial_cmp(&b_val).unwrap()
    });

    for r in &ordered {
        let ms = r.ms;
        // Split "City (Region)" into aligned city and region columns.
        let (city_part, region_part) = match r.city.split_once(" (") {
            Some((city, region)) => (city, region.trim_end_matches(')')),
            None => (r.city.as_str(), ""),
        };
        let city_padded = format!("{:<16}", city_part);
        let region_padded = format!("{:<14}", region_part).dimmed();
        if ms == 0.0 {
            println!(
                "  {}  {}  {}  {}  {}",
                r.code.dimmed(),
                city_padded,
                region_padded,
                "▱".repeat(12).dimmed(),
                "timeout".dimmed()
            );
        } else {
            let color = if ms < 80.0 {
                "cyan"
            } else if ms < 180.0 {
                "yellow"
            } else {
                "red"
            };

            let bar_str = bar(ms, scale, 12);
            let ms_str = format!("{:>3.0} ms", ms);

            let bar_colored = match color {
                "cyan" => bar_str.cyan(),
                "yellow" => bar_str.yellow(),
                "red" => bar_str.red(),
                _ => bar_str.normal(),
            };

            let ms_colored = match color {
                "cyan" => ms_str.bold().cyan().to_string(),
                "yellow" => ms_str.bold().yellow().to_string(),
                "red" => ms_str.bold().red().to_string(),
                _ => ms_str.bold().to_string(),
            };

            println!(
                "  {}  {}  {}  {}  {}",
                r.code.dimmed(),
                city_padded,
                region_padded,
                bar_colored,
                ms_colored
            );
        }
    }
    println!();
}

/// Print the final verdict line: a status mark plus the diagnosis message.
/// `status` is one of "healthy" (✔ green), "low_bandwidth" (⚠ yellow),
/// or anything else (✘ red).
fn render_verdict(status: &str, message: &str) {
    let mark_colored = match status {
        "healthy" => "✔".green(),
        "low_bandwidth" => "⚠".yellow(),
        _ => "✘".red(),
    };

    println!("  {}  {}", mark_colored, message.bold());
    println!();
}
