use std::time::Instant;
use tokio::net::TcpStream;
use tokio::task;

struct Region {
    code: &'static str,
    city: &'static str,
    host: &'static str,
}

const REGIONS: [Region; 8] = [
    Region {
        code: "IN",
        city: "Mumbai (South Asia)",
        host: "speedtest.mumbai1.linode.com",
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

async fn tcp_ping(host: &str, port: u16, attempts: usize) -> f64 {
    let mut samples = Vec::new();

    for _ in 0..attempts {
        let start = Instant::now();
        if let Ok(Ok(_)) = tokio::time::timeout(
            std::time::Duration::from_secs_f64(2.0),
            TcpStream::connect((host, port)),
        )
        .await
        {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            samples.push(elapsed);
        }
    }

    if samples.is_empty() {
        return 0.0;
    }

    round2(samples.iter().cloned().fold(f64::MAX, f64::min))
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
