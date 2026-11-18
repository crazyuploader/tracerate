use tokio::task;

use crate::tester;
use crate::util;

pub struct Region {
    code: &'static str,
    city: &'static str,
    host: &'static str,
}

/// `ec2.<region>.api.aws` hosts are AWS's dual-stack (IPv4 + IPv6) endpoints, used
/// wherever an AWS region sits in the listed city. The Vultr hosts are IPv4-only;
/// with `-6` they are skipped.
pub const REGIONS: [Region; 18] = [
    Region {
        code: "IN",
        city: "Mumbai (South Asia)",
        host: "ec2.ap-south-1.api.aws",
    },
    Region {
        code: "IN",
        city: "Delhi NCR (South Asia)",
        host: "del-in-ping.vultr.com",
    },
    Region {
        code: "IN",
        city: "Hyderabad (South Asia)",
        host: "ec2.ap-south-2.api.aws",
    },
    Region {
        code: "SG",
        city: "Singapore (Southeast Asia)",
        host: "ec2.ap-southeast-1.api.aws",
    },
    Region {
        code: "JP",
        city: "Tokyo (East Asia)",
        host: "ec2.ap-northeast-1.api.aws",
    },
    Region {
        code: "KR",
        city: "Seoul (East Asia)",
        host: "ec2.ap-northeast-2.api.aws",
    },
    Region {
        code: "IL",
        city: "Tel Aviv (Middle East)",
        host: "ec2.il-central-1.api.aws",
    },
    Region {
        code: "GB",
        city: "London (Europe)",
        host: "ec2.eu-west-2.api.aws",
    },
    Region {
        code: "DE",
        city: "Frankfurt (Europe)",
        host: "ec2.eu-central-1.api.aws",
    },
    Region {
        code: "NL",
        city: "Amsterdam (Europe)",
        host: "ams-nl-ping.vultr.com",
    },
    Region {
        code: "ZA",
        city: "Johannesburg (Africa)",
        host: "jnb-za-ping.vultr.com",
    },
    Region {
        code: "US",
        city: "Newark (US East)",
        host: "nj-us-ping.vultr.com",
    },
    Region {
        code: "CA",
        city: "Toronto (Canada)",
        host: "tor-ca-ping.vultr.com",
    },
    Region {
        code: "US",
        city: "Chicago (US Central)",
        host: "il-us-ping.vultr.com",
    },
    Region {
        code: "US",
        city: "Seattle (US West)",
        host: "wa-us-ping.vultr.com",
    },
    Region {
        code: "US",
        city: "Los Angeles (US West)",
        host: "lax-ca-us-ping.vultr.com",
    },
    Region {
        code: "BR",
        city: "São Paulo (South America)",
        host: "ec2.sa-east-1.api.aws",
    },
    Region {
        code: "AU",
        city: "Sydney (Australia)",
        host: "ec2.ap-southeast-2.api.aws",
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
            // Under -4/-6, a host without an address of that family is skipped, not a timeout.
            if !tester::has_family_addr(host, 443).await {
                return None;
            }
            let ms = tcp_ping(host, 443, 3).await;
            Some((code, city, host, ms))
        });

        handles.push(handle);
    }

    let mut results = Vec::new();

    for handle in handles {
        if let Ok(Some((code, city, host, ms))) = handle.await {
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
