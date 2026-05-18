pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub fn bytes_to_mbps(bytes: u64, elapsed: f64) -> f64 {
    if elapsed <= 0.0 {
        return 0.0;
    }
    bytes as f64 * 8.0 / elapsed / 1_000_000.0
}

pub fn bytes_to_mb(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}
