use serde_json::Value;

#[derive(serde::Serialize)]
pub struct VerdictResult {
    pub summary: String,
    pub issues: Vec<String>,
}

fn diagnose(download: f64, ping: f64, jitter: f64, loss: f64, bufferbloat_delta: f64) -> String {
    if loss > 5.0 {
        return "Packet loss detected, connection is unstable.".to_string();
    }
    if bufferbloat_delta > 200.0 {
        return "Severe bufferbloat, router queue is overloaded.".to_string();
    }
    if ping > 100.0 && download >= 10.0 {
        return "High latency, likely congestion or poor routing.".to_string();
    }
    if jitter > 30.0 {
        return "High jitter, connection is unstable.".to_string();
    }
    if download < 10.0 {
        return "Low bandwidth, ISP speed is the bottleneck.".to_string();
    }
    "Connection looks healthy.".to_string()
}

/// Builds a list of human-readable issue messages based on measured network metrics and bufferbloat grade.
///
/// The returned list contains zero or more short diagnostic strings for conditions that exceed predefined thresholds.
///
/// Parameters:
/// - `upload`: optional measured upload speed in Mbps; `None` means upload was not measured.
/// - `upload_tested`: `true` if an upload measurement was attempted and `upload` should be considered; when `false`, low-upload is not reported even if `upload` is `None`.
///
/// # Returns
/// A `Vec<String>` with one message per detected issue; empty if no issues were detected.
///
/// # Examples
///
/// ```
/// let msgs = issues(12.0, Some(5.0), true, 50.0, 10.0, 0.0, "B");
/// assert!(msgs.contains(&"Low download: 12 Mbps".to_string()));
/// assert!(msgs.contains(&"Low upload: 5 Mbps".to_string()));
/// ```
fn issues(
    download: f64,
    upload: Option<f64>,
    upload_tested: bool,
    ping: f64,
    jitter: f64,
    loss: f64,
    bb_grade: &str,
) -> Vec<String> {
    let mut list = Vec::new();

    if loss > 5.0 {
        list.push(format!("Packet loss: {}%", loss));
    }
    if download < 25.0 {
        list.push(format!("Low download: {} Mbps", download));
    }
    if upload_tested && upload.unwrap_or(0.0) < 10.0 {
        list.push(format!("Low upload: {} Mbps", upload.unwrap_or(0.0)));
    }
    if ping > 80.0 {
        list.push(format!("High ping: {} ms", ping));
    }
    if jitter > 20.0 {
        list.push(format!("High jitter: {} ms", jitter));
    }
    if bb_grade == "C" || bb_grade == "D" || bb_grade == "F" {
        list.push(format!("Bufferbloat grade: {}", bb_grade));
    }

    list
}

/// Create a human-readable verdict and list of issues from test metrics and optional bufferbloat results.
///
/// Arguments:
/// - `result`: JSON object containing test metrics (e.g., `download_mbps`, `upload_mbps`, `ping_ms`, `jitter_ms`, `packet_loss`). Missing numeric fields default to 0.0; presence of `upload_mbps` is used to determine whether upload was tested.
/// - `bufferbloat`: Optional bufferbloat analysis used to supply `delta_ms` and `grade` for bufferbloat-related diagnostics.
///
/// # Returns
/// A `VerdictResult` containing:
/// - `summary`: a single-sentence verdict derived from the provided metrics and bufferbloat delta.
/// - `issues`: a vector of human-readable issue messages based on thresholds and available measurements.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// let result = json!({
///     "download_mbps": 50.0,
///     "upload_mbps": 5.0,
///     "ping_ms": 30.0,
///     "jitter_ms": 5.0,
///     "packet_loss": 0.0
/// });
/// // No bufferbloat data available
/// let verdict = crate::verdict::analyze(&result, None);
/// assert!(verdict.summary.len() > 0);
/// assert!(verdict.issues.iter().any(|s| s.contains("Low upload")));
/// ```
pub fn analyze(
    result: &Value,
    bufferbloat: Option<&crate::bufferbloat::BufferbloatResult>,
) -> VerdictResult {
    let download = result
        .get("download_mbps")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let upload = result.get("upload_mbps").and_then(|v| v.as_f64());
    let upload_tested = upload.is_some();
    let ping = result
        .get("ping_ms")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let jitter = result
        .get("jitter_ms")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let loss = result
        .get("packet_loss")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let delta = bufferbloat.map(|bb| bb.delta_ms).unwrap_or(0.0);
    let bb_grade = bufferbloat.map(|bb| bb.grade.as_str()).unwrap_or("?");

    let summary = diagnose(download, ping, jitter, loss, delta);
    let issue_list = issues(
        download,
        upload,
        upload_tested,
        ping,
        jitter,
        loss,
        bb_grade,
    );

    VerdictResult {
        summary,
        issues: issue_list,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bufferbloat::BufferbloatResult;
    use serde_json::json;

    fn make_bb(delta_ms: f64, grade: &str) -> BufferbloatResult {
        BufferbloatResult {
            idle_ms: 10.0,
            loaded_ms: 10.0 + delta_ms,
            delta_ms,
            grade: grade.to_string(),
            data_used_mb: 5.0,
        }
    }

    // --- analyze: summary path (diagnose) ---

    #[test]
    fn analyze_healthy_connection() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert_eq!(v.summary, "Connection looks healthy.");
        assert!(v.issues.is_empty());
    }

    #[test]
    fn analyze_packet_loss_dominates_summary() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 10.0,
            "jitter_ms": 2.0,
            "packet_loss": 10.0
        });
        let v = analyze(&r, None);
        assert_eq!(v.summary, "Packet loss detected, connection is unstable.");
    }

    #[test]
    fn analyze_severe_bufferbloat_in_summary() {
        let r = json!({
            "download_mbps": 100.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let bb = make_bb(201.0, "F");
        let v = analyze(&r, Some(&bb));
        assert_eq!(v.summary, "Severe bufferbloat, router queue is overloaded.");
    }

    #[test]
    fn analyze_high_latency_summary() {
        let r = json!({
            "download_mbps": 50.0,
            "ping_ms": 120.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert_eq!(v.summary, "High latency, likely congestion or poor routing.");
    }

    #[test]
    fn analyze_high_jitter_summary() {
        let r = json!({
            "download_mbps": 50.0,
            "ping_ms": 50.0,
            "jitter_ms": 35.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert_eq!(v.summary, "High jitter, connection is unstable.");
    }

    #[test]
    fn analyze_low_bandwidth_summary() {
        let r = json!({
            "download_mbps": 5.0,
            "ping_ms": 50.0,
            "jitter_ms": 5.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert_eq!(v.summary, "Low bandwidth, ISP speed is the bottleneck.");
    }

    // High ping only fires if download >= 10; below that, low bandwidth wins
    #[test]
    fn analyze_high_ping_below_10mbps_gives_low_bandwidth() {
        let r = json!({
            "download_mbps": 5.0,
            "ping_ms": 150.0,
            "jitter_ms": 0.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert_eq!(v.summary, "Low bandwidth, ISP speed is the bottleneck.");
    }

    // --- analyze: upload_tested behavior ---

    #[test]
    fn analyze_no_upload_field_does_not_report_low_upload() {
        // upload_mbps absent → upload_tested=false → no low-upload issue even though 0 < 10
        let r = json!({
            "download_mbps": 100.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert!(!v.issues.iter().any(|s| s.contains("Low upload")));
    }

    #[test]
    fn analyze_upload_present_and_low_reports_issue() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 5.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert!(v.issues.iter().any(|s| s.contains("Low upload")));
    }

    #[test]
    fn analyze_upload_at_threshold_10_mbps_not_reported() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 10.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert!(!v.issues.iter().any(|s| s.contains("Low upload")));
    }

    // --- analyze: issues list ---

    #[test]
    fn analyze_low_download_reported_when_below_25() {
        let r = json!({
            "download_mbps": 10.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert!(v.issues.iter().any(|s| s.contains("Low download")));
    }

    #[test]
    fn analyze_download_at_25_not_reported() {
        let r = json!({
            "download_mbps": 25.0,
            "upload_mbps": 15.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert!(!v.issues.iter().any(|s| s.contains("Low download")));
    }

    #[test]
    fn analyze_high_ping_reported_above_80() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 90.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert!(v.issues.iter().any(|s| s.contains("High ping")));
    }

    #[test]
    fn analyze_ping_at_80_not_reported() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 80.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert!(!v.issues.iter().any(|s| s.contains("High ping")));
    }

    #[test]
    fn analyze_high_jitter_reported_above_20() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 15.0,
            "jitter_ms": 25.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert!(v.issues.iter().any(|s| s.contains("High jitter")));
    }

    #[test]
    fn analyze_packet_loss_reported_above_5() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 6.0
        });
        let v = analyze(&r, None);
        assert!(v.issues.iter().any(|s| s.contains("Packet loss")));
    }

    #[test]
    fn analyze_packet_loss_at_5_not_reported() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 5.0
        });
        let v = analyze(&r, None);
        assert!(!v.issues.iter().any(|s| s.contains("Packet loss")));
    }

    #[test]
    fn analyze_bufferbloat_grade_c_reported() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let bb = make_bb(80.0, "C");
        let v = analyze(&r, Some(&bb));
        assert!(v.issues.iter().any(|s| s.contains("Bufferbloat grade")));
    }

    #[test]
    fn analyze_bufferbloat_grade_d_reported() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let bb = make_bb(150.0, "D");
        let v = analyze(&r, Some(&bb));
        assert!(v.issues.iter().any(|s| s.contains("Bufferbloat grade")));
    }

    #[test]
    fn analyze_bufferbloat_grade_f_reported() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let bb = make_bb(250.0, "F");
        let v = analyze(&r, Some(&bb));
        assert!(v.issues.iter().any(|s| s.contains("Bufferbloat grade")));
    }

    #[test]
    fn analyze_bufferbloat_grade_a_not_reported() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let bb = make_bb(5.0, "A");
        let v = analyze(&r, Some(&bb));
        assert!(!v.issues.iter().any(|s| s.contains("Bufferbloat grade")));
    }

    #[test]
    fn analyze_bufferbloat_grade_b_not_reported() {
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 20.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let bb = make_bb(30.0, "B");
        let v = analyze(&r, Some(&bb));
        assert!(!v.issues.iter().any(|s| s.contains("Bufferbloat grade")));
    }

    // --- analyze: missing fields default gracefully ---

    #[test]
    fn analyze_missing_all_fields_defaults_to_low_bandwidth() {
        // All fields absent → download=0, ping=0, jitter=0, loss=0 → download<10 fires
        let r = json!({});
        let v = analyze(&r, None);
        assert_eq!(v.summary, "Low bandwidth, ISP speed is the bottleneck.");
    }

    #[test]
    fn analyze_upload_zero_when_tested_reported() {
        // upload_mbps present as 0 → upload_tested=true, 0 < 10 → reported
        let r = json!({
            "download_mbps": 100.0,
            "upload_mbps": 0.0,
            "ping_ms": 15.0,
            "jitter_ms": 2.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert!(v.issues.iter().any(|s| s.contains("Low upload")));
    }

    // Regression: multiple issues can be reported simultaneously
    #[test]
    fn analyze_multiple_issues_reported_together() {
        let r = json!({
            "download_mbps": 10.0,
            "upload_mbps": 5.0,
            "ping_ms": 90.0,
            "jitter_ms": 25.0,
            "packet_loss": 0.0
        });
        let v = analyze(&r, None);
        assert!(v.issues.iter().any(|s| s.contains("Low download")));
        assert!(v.issues.iter().any(|s| s.contains("Low upload")));
        assert!(v.issues.iter().any(|s| s.contains("High ping")));
        assert!(v.issues.iter().any(|s| s.contains("High jitter")));
    }
}
