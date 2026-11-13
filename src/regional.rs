use tokio::task;

use crate::tester;
use crate::util;

struct Region {
    code: &'static str,
    city: &'static str,
    host: &'static str,
}

const REGIONS: [Region; 10] = [
    Region {
        code: "IN",
        city: "Mumbai (South Asia)",
        host: "bom-in-ping.vultr.com",
    },
    Region {
        code: "IN",
        city: "Delhi NCR (South Asia)",
        host: "del-in-ping.vultr.com",
    },
    Region {
        code: "IN",
        city: "Hyderabad (South Asia)",
        host: "ec2.ap-south-2.amazonaws.com",
    },
    Region {
        code: "SG",
        city: "Singapore (Southeast Asia)",
        host: "sgp-ping.vultr.com",
    },
    Region {
        code: "JP",
        city: "Tokyo (East Asia)",
        host: "hnd-jp-ping.vultr.com",
    },
    Region {
        code: "GB",
        city: "London (Europe)",
        host: "lon-gb-ping.vultr.com",
    },
    Region {
        code: "DE",
        city: "Frankfurt (Europe)",
        host: "fra-de-ping.vultr.com",
    },
    Region {
        code: "US",
        city: "Newark (US East)",
        host: "nj-us-ping.vultr.com",
    },
    Region {
        code: "US",
        city: "Seattle (US West)",
        host: "wa-us-ping.vultr.com",
    },
    Region {
        code: "AU",
        city: "Sydney (Australia)",
        host: "syd-au-ping.vultr.com",
    },
];

#[derive(Clone, serde::Serialize)]
pub struct RegionResult {
    pub code: String,
    pub city: String,
    pub host: String,
    /// `None` when every connect attempt failed or timed out.
    pub ms: Option<f64>,
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
/// Returns the minimum successful TCP connection time in milliseconds, rounded to two decimal places; returns `None` if no attempts succeeded.
///
/// # Examples
///
/// ```
/// let rt = tokio::runtime::Runtime::new().unwrap();
/// let ms = rt.block_on(tcp_ping("example.com", 443, 1));
/// assert!(ms.is_none_or(|m| m >= 0.0));
/// ```
async fn tcp_ping(host: &str, port: u16, attempts: usize) -> Option<f64> {
    let mut samples = Vec::new();

    for _ in 0..attempts {
        if let Some(ms) = tester::tcp_ping_once(host, port).await {
            samples.push(ms);
        }
    }

    samples.into_iter().min_by(f64::total_cmp).map(util::round2)
}
