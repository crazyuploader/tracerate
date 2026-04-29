import socket
import time


# Linode publishes public speedtest endpoints in each region.
# Stable hostnames, anycast-free, good proxies for "real" geo distance.
REGIONS = [
    ("IN", "Mumbai",     "speedtest.mumbai1.linode.com"),
    ("SG", "Singapore",  "speedtest.singapore.linode.com"),
    ("JP", "Tokyo",      "speedtest.tokyo2.linode.com"),
    ("DE", "Frankfurt",  "speedtest.frankfurt.linode.com"),
    ("UK", "London",     "speedtest.london.linode.com"),
    ("US", "Newark",     "speedtest.newark.linode.com"),
    ("US", "Fremont",    "speedtest.fremont.linode.com"),
]


def _tcp_ping(host: str, port: int = 443, attempts: int = 3, timeout: float = 2.0) -> float:
    """
    TCP-connect latency in ms, returns the minimum of N attempts.
    Min is more representative of true RTT than mean — high samples
    are jitter, the floor is the actual round-trip.
    """

    samples = []
    for _ in range(attempts):
        try:
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.settimeout(timeout)
            start = time.perf_counter()
            sock.connect((host, port))
            samples.append((time.perf_counter() - start) * 1000)
        except (socket.timeout, socket.error):
            pass
        finally:
            try:
                sock.close()
            except Exception:
                pass

    if not samples:
        return 0.0
    return round(min(samples), 2)


def measure_regions() -> list[dict]:
    out = []
    for code, city, host in REGIONS:
        out.append({
            "code": code,
            "city": city,
            "host": host,
            "ms": _tcp_ping(host),
        })
    return out
