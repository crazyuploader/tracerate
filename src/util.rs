/// Round a floating-point value to two decimal places.

///

/// # Examples

///

/// ```

/// let a = round2(1.235);

/// assert_eq!(a, 1.24);

///

/// let b = round2(-0.1234);

/// assert_eq!(b, -0.12);

/// ```
pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Rounds a floating-point value to one decimal place.
///
/// # Examples
///
/// ```
/// let v = round1(1.24);
/// assert_eq!(v, 1.2);
/// ```
pub fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Convert a byte count and elapsed seconds into megabits per second.
///
/// This returns the measured throughput in megabits per second computed as
/// `bytes * 8 / elapsed / 1_000_000`. If `elapsed` is less than or equal to
/// 0.0, the function returns `0.0`.
///
/// # Examples
///
/// ```
/// let mbps = bytes_to_mbps(1_000_000, 1.0);
/// assert_eq!(mbps, 8.0);
/// ```
pub fn bytes_to_mbps(bytes: u64, elapsed: f64) -> f64 {
    if elapsed <= 0.0 {
        return 0.0;
    }
    bytes as f64 * 8.0 / elapsed / 1_000_000.0
}

/// Converts a byte count to mebibytes (MiB).
///
/// Uses 1,048,576 bytes per MiB.
///
/// # Examples
///
/// ```
/// let mb = bytes_to_mb(1_048_576);
/// assert_eq!(mb, 1.0);
///
/// let mb = bytes_to_mb(524_288); // half of 1 MiB
/// assert_eq!(mb, 0.5);
/// ```
pub fn bytes_to_mb(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}
