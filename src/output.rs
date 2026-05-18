use colored::Colorize;

use crate::info::InfoResult;
use crate::regional::RegionResult;
use crate::verdict::VerdictResult;

pub fn print_header() {
    println!();
    println!(
        "{} {}",
        "tracerate".bold(),
        "· network diagnostics".dimmed()
    );
    println!();
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

pub fn render(
    info: &InfoResult,
    dns_ms: f64,
    r: &serde_json::Value,
    bb: Option<&crate::bufferbloat::BufferbloatResult>,
    regions: Option<&[RegionResult]>,
    summary: &VerdictResult,
    verbose: bool,
) {
    render_connection(info, dns_ms);
    render_speed(r);

    if let Some(bb) = bb {
        render_bufferbloat(bb, verbose);
    }

    if let Some(regions) = regions {
        render_regions(regions);
    }

    render_verdict(&summary.summary);
}

fn render_connection(info: &InfoResult, dns_ms: f64) {
    let isp = info.isp.as_deref().unwrap_or("unknown");
    let asn = info.asn.as_deref().unwrap_or("");
    let city = info.city.as_deref().unwrap_or("?");
    let country = info.country.as_deref().unwrap_or("?");
    let colo = info.colo.as_deref().unwrap_or("?");
    let colo_city = info.colo_city.as_deref().unwrap_or("");
    let ip = info.ip.as_deref().unwrap_or("?");

    let dns_color = if dns_ms < 50.0 {
        "dim"
    } else if dns_ms < 150.0 {
        "yellow"
    } else {
        "red"
    };

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
        format!("{:<5}", "ISP").dimmed(),
        isp.bold(),
        asn.to_string().dimmed()
    );
    println!(
        "  {}   {}, {}  {}  {}",
        format!("{:<5}", "Where").dimmed(),
        city,
        country,
        "→".dimmed(),
        edge
    );

    let dns_str = format!("{:.2} ms", dns_ms);
    let dns_colored = match dns_color {
        "yellow" => dns_str.bold().yellow(),
        "red" => dns_str.bold().red(),
        _ => dns_str.bold().cyan(),
    };

    println!(
        "  {}   {}   {} {}",
        format!("{:<5}", "IP").dimmed(),
        ip.yellow(),
        "· DNS".dimmed(),
        dns_colored
    );
    println!();
}

fn render_speed(r: &serde_json::Value) {
    section("Speed");

    let dl = r
        .get("download_mbps")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let ul = r.get("upload_mbps").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let ping = r.get("ping_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let jitter = r.get("jitter_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let loss = r.get("packet_loss").and_then(|v| v.as_f64()).unwrap_or(0.0);

    let scale = dl.max(ul).max(100.0);

    println!(
        "  {}   {}   {}",
        format!("{:<8}", "Download").dimmed(),
        bar(dl, scale, 20).cyan(),
        format!("{:.2} Mbps", dl).bold().cyan()
    );

    if r.get("upload_mbps").and_then(|v| v.as_f64()).is_some() {
        println!(
            "  {}   {}   {}",
            format!("{:<8}", "Upload").dimmed(),
            bar(ul, scale, 20).cyan(),
            format!("{:.2} Mbps", ul).bold().cyan()
        );
    }

    let loss_str = if loss > 0.0 {
        format!("· {:.1}% loss", loss).red().to_string()
    } else {
        "· 0% loss".dimmed().to_string()
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
    let max_city = regions.iter().map(|r| r.city.len()).max().unwrap_or(20);

    let mut ordered = regions.to_vec();
    ordered.sort_by(|a, b| {
        let a_val = if a.ms > 0.0 { a.ms } else { 1e9 };
        let b_val = if b.ms > 0.0 { b.ms } else { 1e9 };
        a_val.partial_cmp(&b_val).unwrap()
    });

    for r in &ordered {
        let ms = r.ms;
        let city_padded = format!("{:<width$}", r.city, width = max_city);
        if ms == 0.0 {
            println!(
                "  {}  {}  {}   {}",
                r.code.dimmed(),
                city_padded,
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
                "  {}  {}  {}   {}",
                r.code.dimmed(),
                city_padded,
                bar_colored,
                ms_colored
            );
        }
    }
    println!();
}

fn render_verdict(verdict: &str) {
    let (mark, color) = if verdict == "Connection looks healthy." {
        ("✔", "green")
    } else if verdict == "Low bandwidth, ISP speed is the bottleneck." {
        ("⚠", "yellow")
    } else {
        ("✘", "red")
    };

    let mark_colored = match color {
        "green" => mark.to_string().green(),
        "yellow" => mark.to_string().yellow(),
        "red" => mark.to_string().red(),
        _ => mark.to_string().normal(),
    };

    println!("  {}  {}", mark_colored, verdict.bold());
    println!();
}
