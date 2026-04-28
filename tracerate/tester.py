import time
import socket
import httpx
import os


SERVERS = [
    {
        "name": "Cloudflare",
        "down": "https://speed.cloudflare.com/__down?bytes={bytes}",
        "up": "https://speed.cloudflare.com/__up",
        "host": "speed.cloudflare.com",
        "port": 443,
    },
    {
        "name": "Hetzner",
        "down": "https://speed.hetzner.de/100MB.bin",
        "up": None,
        "host": "speed.hetzner.de",
        "port": 443,
    },
    {
        "name": "Frappe",
        "down": "https://speedtest.frappe.io/api/method/frappe.utils.speed_test.download?size={bytes}",
        "up": None,
        "host": "speedtest.frappe.io",
        "port": 443,
    },
]

def measure_ping(host: str, port: int, attempts: int = 5) -> tuple[float, float]:
    """
    Measures latency and packet loss to a host using TCP connect time.

    Why TCP and not ICMP?
    ICMP (what 'ping' command uses) requires root on Linux.
    TCP connect to port 443 works without any special permissions.

    Returns: (average_latency_ms, packet_loss_percent)
    """

    results = []

    for _ in range(attempts):
        try:
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.settimeout(3)
            start = time.perf_counter()
            sock.connect((host, port))
            elapsed = (time.perf_counter() - start) * 1000
            results.append(elapsed)
        except (socket.timeout, socket.error):
            pass
        finally:
            sock.close()

    if not results:
        return 0.0, 0.0

    average_latency = sum(results) / len(results)
    packet_loss = ((attempts - len(results)) / attempts) * 100

    return round(average_latency, 2), round(packet_loss, 1)

def measure_download(url: str, bytes_to_download: int) -> float:
    """
    Downloads a file in chunks and measures speed in Mbps.

    We stream the response — meaning we process each chunk
    as it arrives and discard it. We never hold the full
    file in memory.

    Returns: speed in Mbps, or 0.0 if it failed.
    """

    url = url.format(bytes=bytes_to_download)
    try:
        total_bytes = 0
        start = time.perf_counter()

        with httpx.stream("GET", url, timeout=30, follow_redirects=True) as response:
            for chunk in response.iter_bytes():
                total_bytes += len(chunk)
        elapsed = time.perf_counter() - start

        if elapsed == 0 or total_bytes == 0:
            return 0.0

        speed_mbps = (total_bytes * 8) / elapsed / 1_000_000
        return round(speed_mbps, 2)

    except Exception:
        return 0.0

def measure_upload(url: str, bytes_to_upload: int = 10_000_000) -> float:
    """
    Uploads random bytes to a server and measures speed in Mbps.

    Why random bytes?
    Some servers or network equipment compress data in transit.
    Random bytes can't be compressed — gives you an honest measurement.

    Returns: speed in Mbps, or 0.0 if it failed.
    """

    data = os.urandom(bytes_to_upload)
    try:
        start = time.perf_counter()
        response = httpx.post(url, content=data, timeout=30)
        elapsed = time.perf_counter() - start
        if elapsed == 0:
            return 0.0
        speed_mbps = (bytes_to_upload * 8) / elapsed / 1_000_000
        return round(speed_mbps, 2)
    except Exception:
        return 0.0

def run_tests(
    n_servers: int,
    bytes_to_download: int,
    test_upload: bool,
) -> list[dict]:
    """
    Runs ping + download (+ optionally upload) against N servers.
    Returns a list of result dicts, one per server.
    """

    selected = SERVERS[:n_servers]
    results = []

    for server in selected:
        print(f"Testing {server['name']}...")

        result = {
            "name": server["name"],
            "ping_ms": None,
            "packet_loss_pct": None,
            "download_mbps": None,
            "upload_mbps": None,
            "error": None,
        }

        ping, loss = measure_ping(server["host"], server["port"])
        result["ping_ms"] = ping
        result["packet_loss_pct"] = loss

        if server["down"]:
            result["download_mbps"] = measure_download(
                server["down"], bytes_to_download
            )

        if test_upload and server["up"]:
            result["upload_mbps"] = measure_upload(server["up"])

        results.append(result)

    return results