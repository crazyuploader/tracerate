use serde::Deserialize;
use std::time::Instant;
use tokio::net::lookup_host;

use crate::util;

#[derive(Debug, Deserialize)]
pub struct IpInfoResponse {
    pub ip: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub org: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct InfoResult {
    pub ip: Option<String>,
    pub isp: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub asn: Option<String>,
    pub colo: Option<String>,
    pub colo_city: Option<String>,
}

/// Gather public IP and network metadata from external services and consolidate it into an `InfoResult`.
///
/// Queries https://ipinfo.io/json and https://speed.cloudflare.com/meta, merges available fields
/// (ip, isp, city, country, asn, colo, colo_city) from those services, and returns the combined result.
/// Network or JSON parsing failures are ignored and leave any unavailable fields as `None`.
///
/// # Examples
///
/// ```
/// # tokio_test::block_on(async {
/// let info = crate::info::get_ip_info().await;
/// // info.ip, info.isp, info.city, etc. may be `Some(_)` or `None` depending on availability.
/// println!("{:?}", info);
/// # });
/// ```
pub async fn get_ip_info() -> InfoResult {
    let mut info = InfoResult {
        ip: None,
        isp: None,
        city: None,
        country: None,
        asn: None,
        colo: None,
        colo_city: None,
    };

    let client = crate::tester::build_client(5);

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "User-Agent",
        "Mozilla/5.0".parse().expect("invalid static header value"),
    );
    headers.insert(
        "Accept",
        "application/json"
            .parse()
            .expect("invalid static header value"),
    );
    headers.insert(
        "Referer",
        "https://speed.cloudflare.com/"
            .parse()
            .expect("invalid static header value"),
    );

    let ipinfo_fut = async {
        match client.get("https://ipinfo.io/json").send().await {
            Ok(r) if r.status().is_success() => r.json::<IpInfoResponse>().await.ok(),
            _ => None,
        }
    };
    let cf_fut = async {
        match client
            .get("https://speed.cloudflare.com/meta")
            .headers(headers)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r.json::<serde_json::Value>().await.ok(),
            _ => None,
        }
    };

    let (ipinfo_data, cf_data) = tokio::join!(ipinfo_fut, cf_fut);

    if let Some(data) = ipinfo_data {
        info.ip = data.ip;
        info.city = data.city;
        info.country = data.country;

        if let Some(org) = data.org {
            if org.starts_with("AS") && org.contains(' ') {
                let mut parts = org.splitn(2, ' ');
                info.asn = parts.next().map(String::from);
                info.isp = parts.next().map(String::from);
            } else {
                info.isp = Some(org);
            }
        }
    }

    if let Some(data) = cf_data {
        if let Some(colo) = data.get("colo") {
            if let Some(colo_obj) = colo.as_object() {
                info.colo = colo_obj
                    .get("iata")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                info.colo_city = colo_obj
                    .get("city")
                    .and_then(|v| v.as_str())
                    .map(String::from);
            } else if let Some(colo_str) = colo.as_str() {
                info.colo = Some(colo_str.to_string());
            }
        }

        if info.isp.is_none() {
            info.isp = data
                .get("asOrganization")
                .and_then(|v| v.as_str())
                .map(String::from);
        }

        if info.asn.is_none() {
            if let Some(asn) = data.get("asn").and_then(|v| v.as_u64()) {
                info.asn = Some(format!("AS{}", asn));
            }
        }

        if info.city.is_none() {
            info.city = data.get("city").and_then(|v| v.as_str()).map(String::from);
        }

        if info.country.is_none() {
            info.country = data
                .get("country")
                .and_then(|v| v.as_str())
                .map(String::from);
        }

        if info.ip.is_none() {
            info.ip = data
                .get("clientIp")
                .and_then(|v| v.as_str())
                .map(String::from);
        }
    }

    info
}

/// Measure DNS lookup latency for a hostname in milliseconds.
///
/// The lookup duration is rounded to two decimal places; returns `None` on
/// resolution failure (distinguishing failure from a genuine near-zero result).
pub async fn measure_dns(hostname: &str) -> Option<f64> {
    let start = Instant::now();
    match lookup_host((hostname, 0)).await {
        Ok(_) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            Some(util::round2(elapsed))
        }
        Err(_) => None,
    }
}
