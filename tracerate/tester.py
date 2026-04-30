import os
import socket
import time
from typing import Callable

import httpx


SERVER = {
    "name": "Cloudflare",
    "down": "https://speed.cloudflare.com/__down?bytes={bytes}",
    "up": "https://speed.cloudflare.com/__up",
    "host": "speed.cloudflare.com",
    "port": 443,
}

_REQUEST_HEADERS = {
    "User-Agent": "Mozilla/5.0",
    "Accept": "*/*",
    "Referer": "https://speed.cloudflare.com/",
}


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
            try:
                sock.close()
            except Exception:
                pass

    if not results:
        return 0.0, 0.0

    average_latency = sum(results) / len(results)
    packet_loss = ((attempts - len(results)) / attempts) * 100
    jitter = round(max(results) - min(results), 2)

    return round(average_latency, 2), round(packet_loss, 1), round(jitter, 2)


def measure_download(
    url: str,
    bytes_to_download: int,
    on_progress: Callable[[int], None] | None = None,
) -> float:
    """
    Downloads a file in chunks and measures speed in Mbps.

    We stream the response and discard each chunk —
    bytes are never held in memory.

    `on_progress(total_bytes)` is called per chunk if provided,
    so the caller can drive a progress bar.

    Returns: speed in Mbps, or 0.0 if it failed.
    """

    url = url.format(bytes=bytes_to_download)
    try:
        total_bytes = 0
        start = time.perf_counter()

        with httpx.stream(
            "GET",
            url,
            timeout=30,
            follow_redirects=True,
            headers=_REQUEST_HEADERS,
        ) as response:
            response.raise_for_status()
            for chunk in response.iter_bytes():
                total_bytes += len(chunk)
                if on_progress is not None:
                    on_progress(total_bytes)

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
    Random bytes can't be compressed — gives an honest measurement.

    Returns: speed in Mbps, or 0.0 if it failed.
    """

    data = os.urandom(bytes_to_upload)
    try:
        start = time.perf_counter()
        httpx.post(url, content=data, timeout=30)
        elapsed = time.perf_counter() - start
        if elapsed == 0:
            return 0.0
        speed_mbps = (bytes_to_upload * 8) / elapsed / 1_000_000
        return round(speed_mbps, 2)
    except Exception:
        return 0.0
