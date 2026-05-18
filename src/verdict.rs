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

fn issues(
    download: f64,
    upload: f64,
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
    if upload < 10.0 {
        list.push(format!("Low upload: {} Mbps", upload));
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

pub fn analyze(
    result: &Value,
    bufferbloat: Option<&crate::bufferbloat::BufferbloatResult>,
) -> VerdictResult {
    let download = result
        .get("download_mbps")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let upload = result
        .get("upload_mbps")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
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
    let issue_list = issues(download, upload, ping, jitter, loss, bb_grade);

    VerdictResult {
        summary,
        issues: issue_list,
    }
}
