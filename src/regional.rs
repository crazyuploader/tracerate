use tokio::task;

use crate::tester;
use crate::util;

struct Region {
    code: &'static str,
    city: &'static str,
    host: &'static str,
}

const REGIONS: [Region; 9] = [
    Region {
        code: "IN",
        city: "Mumbai (South Asia)",
        host: "speedtest.mumbai1.linode.com",
    },
    Region {
        code: "IN",
        city: "Hyderabad (South Asia)",
        host: "ec2.ap-south-2.amazonaws.com",
    },
    Region {
        code: "SG",
        city: "Singapore (SouthEast Asia)",
        host: "speedtest.singapore.linode.com",
    },
    Region {
        code: "JP",
        city: "Tokyo (East Asia)",
        host: "speedtest.tokyo2.linode.com",
    },
    Region {
        code: "GB",
        city: "London (Europe)",
        host: "speedtest.london.linode.com",
    },
    Region {
        code: "DE",
        city: "Frankfurt (Europe)",
        host: "speedtest.frankfurt.linode.com",
    },
    Region {
        code: "US",
        city: "Newark (US East)",
        host: "speedtest.newark.linode.com",
    },
    Region {
        code: "US",
        city: "Fremont/Seattle (US West)",
        host: "speedtest.fremont.linode.com",
    },
    Region {
        code: "AU",
        city: "Sydney (Australia)",
        host: "speedtest.sydney.linode.com",
    },
];

#[derive(Clone, serde::Serialize)]
pub struct RegionResult {
    pub code: String,
    pub city: String,
    pub host: String,
    pub ms: f64,
}

pub async fn ping_regions() -> Vec<RegionResult> {
    let mut handles = Vec::new();

    for region in &REGIONS {
        let host = region.host;
        let code = region.code;
        let city = region.city;

        let handle = task::spawn(async move {
            let ms = tcp_ping(host, 443, 3).await;
            (code, city, host, ms)
        });

        handles.push(handle);
    }

    let mut results = Vec::new();

    for handle in handles {
        if let Ok((code, city, host, ms)) = handle.await {
            results.push(RegionResult {
                code: code.to_string(),
                city: city.to_string(),
                host: host.to_string(),
                ms,
            });
        }
    }

    results
}

/// Measure TCP connection latency to a host over multiple attempts and return the best result.
///
/// Returns the minimum successful TCP connection time in milliseconds, rounded to two decimal places; returns `0.0` if no attempts succeeded.
///
/// # Examples
///
/// ```
/// let rt = tokio::runtime::Runtime::new().unwrap();
/// let ms = rt.block_on(tcp_ping("example.com", 443, 1));
/// assert!(ms >= 0.0);
/// ```
async fn tcp_ping(host: &str, port: u16, attempts: usize) -> f64 {
    let mut samples = Vec::new();

    for _ in 0..attempts {
        if let Some(ms) = tester::tcp_ping_once(host, port).await {
            samples.push(ms);
        }
    }

    if samples.is_empty() {
        return 0.0;
    }

    util::round2(samples.iter().cloned().fold(f64::MAX, f64::min))
}
