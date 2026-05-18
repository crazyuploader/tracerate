use serde::Deserialize;
use std::time::Instant;
use tokio::net::lookup_host;

#[derive(Debug, Deserialize)]
pub struct IpInfoResponse {
    pub ip: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub org: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CloudflareMeta {
    pub colo: Option<serde_json::Value>,
    pub as_organization: Option<String>,
    pub asn: Option<u64>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub client_ip: Option<String>,
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

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    if let Ok(response) = client.get("https://ipinfo.io/json").send().await {
        if response.status().is_success() {
            if let Ok(data) = response.json::<IpInfoResponse>().await {
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
        }
    }

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("User-Agent", "Mozilla/5.0".parse().unwrap());
    headers.insert("Accept", "application/json".parse().unwrap());
    headers.insert("Referer", "https://speed.cloudflare.com/".parse().unwrap());

    if let Ok(response) = client
        .get("https://speed.cloudflare.com/meta")
        .headers(headers)
        .send()
        .await
    {
        if response.status().is_success() {
            if let Ok(data) = response.json::<serde_json::Value>().await {
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
        }
    }

    info
}

pub async fn measure_dns(hostname: &str) -> f64 {
    let start = Instant::now();
    match lookup_host((hostname, 0)).await {
        Ok(_) => {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            round2(elapsed)
        }
        Err(_) => 0.0,
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
