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

#[cfg(test)]
mod tests {
    use super::*;

    // --- round2 ---

    #[test]
    fn round2_rounds_up_at_half() {
        // 1.235 * 100 = 123.5 → rounds to 124 → 1.24
        assert_eq!(round2(1.235), 1.24);
    }

    #[test]
    fn round2_rounds_down() {
        assert_eq!(round2(1.234), 1.23);
    }

    #[test]
    fn round2_negative_value() {
        // -0.1234 * 100 = -12.34 → rounds to -12 → -0.12
        assert_eq!(round2(-0.1234), -0.12);
    }

    #[test]
    fn round2_zero() {
        assert_eq!(round2(0.0), 0.0);
    }

    #[test]
    fn round2_already_two_decimals() {
        assert_eq!(round2(3.14), 3.14);
    }

    #[test]
    fn round2_large_value() {
        assert_eq!(round2(999.999), 1000.0);
    }

    #[test]
    fn round2_clearly_rounds_up_past_midpoint() {
        // 1.236 * 100 = 123.6 → clearly rounds to 124 → 1.24
        assert_eq!(round2(1.236), 1.24);
    }

    // --- round1 ---

    #[test]
    fn round1_rounds_up_at_half() {
        // 1.25 * 10 = 12.5 → rounds to 13 → 1.3
        assert_eq!(round1(1.25), 1.3);
    }

    #[test]
    fn round1_rounds_down() {
        assert_eq!(round1(1.24), 1.2);
    }

    #[test]
    fn round1_zero() {
        assert_eq!(round1(0.0), 0.0);
    }

    #[test]
    fn round1_negative() {
        // -0.15 * 10 = -1.5 → rounds to -2 → -0.2
        assert_eq!(round1(-0.15), -0.2);
    }

    #[test]
    fn round1_already_one_decimal() {
        assert_eq!(round1(5.5), 5.5);
    }

    #[test]
    fn round1_whole_number() {
        assert_eq!(round1(100.0), 100.0);
    }

    // --- bytes_to_mbps ---

    #[test]
    fn bytes_to_mbps_one_megabyte_per_second() {
        // 1_000_000 bytes * 8 / 1.0 s / 1_000_000 = 8.0 Mbps
        assert_eq!(bytes_to_mbps(1_000_000, 1.0), 8.0);
    }

    #[test]
    fn bytes_to_mbps_zero_elapsed_returns_zero() {
        assert_eq!(bytes_to_mbps(1_000_000, 0.0), 0.0);
    }

    #[test]
    fn bytes_to_mbps_negative_elapsed_returns_zero() {
        assert_eq!(bytes_to_mbps(1_000_000, -1.0), 0.0);
    }

    #[test]
    fn bytes_to_mbps_zero_bytes() {
        assert_eq!(bytes_to_mbps(0, 5.0), 0.0);
    }

    #[test]
    fn bytes_to_mbps_125_kilobytes_per_second_is_1_mbps() {
        // 125_000 bytes * 8 / 1.0 / 1_000_000 = 1.0 Mbps
        assert_eq!(bytes_to_mbps(125_000, 1.0), 1.0);
    }

    #[test]
    fn bytes_to_mbps_scales_with_duration() {
        // 2_000_000 bytes over 2 seconds = same as 1_000_000 / 1 second
        assert_eq!(bytes_to_mbps(2_000_000, 2.0), bytes_to_mbps(1_000_000, 1.0));
    }

    #[test]
    fn bytes_to_mbps_large_transfer() {
        // 1 GiB (1_073_741_824 bytes) over 10 seconds
        let mbps = bytes_to_mbps(1_073_741_824, 10.0);
        assert!(mbps > 800.0 && mbps < 1000.0);
    }

    // --- bytes_to_mb ---

    #[test]
    fn bytes_to_mb_one_mib() {
        assert_eq!(bytes_to_mb(1_048_576), 1.0);
    }

    #[test]
    fn bytes_to_mb_half_mib() {
        assert_eq!(bytes_to_mb(524_288), 0.5);
    }

    #[test]
    fn bytes_to_mb_zero() {
        assert_eq!(bytes_to_mb(0), 0.0);
    }

    #[test]
    fn bytes_to_mb_two_mib() {
        assert_eq!(bytes_to_mb(2_097_152), 2.0);
    }

    #[test]
    fn bytes_to_mb_fractional_result() {
        // 1 byte / 1_048_576 should be a very small positive value
        let result = bytes_to_mb(1);
        assert!(result > 0.0);
        assert!(result < 0.000_002);
    }
}
