import socket
import time
import httpx


def get_ip_info() -> dict:
    """
    Best-effort ISP / IP / location lookup.

    Combines ipinfo.io (city precision, friendly ISP name) with
    Cloudflare's /meta endpoint (ASN, edge PoP code) so we can
    show "you hit Cloudflare BOM via Jio" in one line.
    """

    info = {
        "ip": None,
        "isp": None,
        "city": None,
        "country": None,
        "asn": None,
        "colo": None,
        "colo_city": None,
    }

    try:
        r = httpx.get("https://ipinfo.io/json", timeout=5)
        if r.status_code == 200:
            data = r.json()
            info["ip"] = data.get("ip")
            info["city"] = data.get("city")
            info["country"] = data.get("country")
            org = data.get("org") or ""
            if org.startswith("AS") and " " in org:
                asn, _, name = org.partition(" ")
                info["asn"] = asn
                info["isp"] = name
            elif org:
                info["isp"] = org
    except Exception:
        pass

    try:
        # Cloudflare blocks bare httpx requests; needs browser-ish headers.
        r = httpx.get(
            "https://speed.cloudflare.com/meta",
            timeout=5,
            headers={
                "User-Agent": "Mozilla/5.0",
                "Accept": "application/json",
                "Referer": "https://speed.cloudflare.com/",
            },
        )
        if r.status_code == 200:
            data = r.json()
            colo = data.get("colo")
            if isinstance(colo, dict):
                info["colo"] = colo.get("iata")
                info["colo_city"] = colo.get("city")
            elif isinstance(colo, str):
                info["colo"] = colo
            if not info["isp"]:
                info["isp"] = data.get("asOrganization")
            if not info["asn"] and data.get("asn"):
                info["asn"] = f"AS{data['asn']}"
            if not info["city"]:
                info["city"] = data.get("city")
            if not info["country"]:
                info["country"] = data.get("country")
            if not info["ip"]:
                info["ip"] = data.get("clientIp")
    except Exception:
        pass

    return info


def measure_dns(hostname: str = "speed.cloudflare.com") -> float:
    """
    DNS lookup time in ms via getaddrinfo.

    Note: OS-level DNS cache (systemd-resolved, nscd) may
    return a stale near-zero value on subsequent runs.
    First call after a cache flush is the honest one.
    """

    try:
        start = time.perf_counter()
        socket.getaddrinfo(hostname, None)
        elapsed = (time.perf_counter() - start) * 1000
        return round(elapsed, 2)
    except socket.gaierror:
        return 0.0
