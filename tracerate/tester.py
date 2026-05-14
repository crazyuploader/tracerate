import socket
import time
from typing import Callable
import httpx
import os


SERVER = {
    "name": "Cloudflare",
    "download_url": "https://speed.cloudflare.com/__down?bytes={bytes}",
    "upload_url" : "https://speed.cloudflare.com/__up",
    "host": "speed.cloudflare.com",
    "port": 443,
}

REQUEST_HEADERS = {
    "User-Agent": "Mozilla/5.0",
    "Accept": "*/*",
    "Referer": "https://speed.cloudflare.com/",
}

def ping(host: str, port: int, attempts: int = 5):
    """
    Measures latency and packet loss to the specified host and port using TCP ping.

    Returns:
        average_latency (float): The average latency in milliseconds.
        packet_loss (float): The percentage of packets lost.
        jitter (float): The jitter in milliseconds.
    """

    results = []

    for _ in range(attempts):
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.settimeout(3)
            try:
                start = time.perf_counter()
                s.connect((host, port))
                end = time.perf_counter()

                latency = (end - start) * 1000
                results.append(latency)
            finally:
                s.close()
        except (socket.timeout, socket.error):
            results.append(None)

    valid_results = [r for r in results if r is not None]
    if not valid_results:
        return None, 100.0, None

    average_latency = sum(valid_results) / len(valid_results)
    packet_loss = ((attempts - len(valid_results)) / attempts) * 100
    jitter = max(valid_results) - min(valid_results)
    return round(average_latency, 2), round(packet_loss, 1), round(jitter, 2)

def download(url: str, size_bytes: int, on_progress: Callable[[int], None] | None = None) -> float:
    """
    Downloads streams of data and measure speed in Mbps.

    Returns: speed in Mbps, or 0.0 if it fails.
    """
    url = url.format(bytes=size_bytes)
    try:
        total_bytes = 0
        start = time.perf_counter()
        with httpx.stream("GET", url, timeout=30, follow_redirects=True, headers=REQUEST_HEADERS) as response:
            response.raise_for_status()
            for chunk in response.iter_bytes():
                total_bytes += len(chunk)
                if on_progress:
                    on_progress(total_bytes)

        end = time.perf_counter()
        elapsed_time = (end - start)

        if elapsed_time == 0 or total_bytes == 0:
            return 0.0

        speed = (total_bytes * 8) / elapsed_time / 1_000_000
        return round(speed, 2)

    except (httpx.HTTPError, OSError):
        return 0.0

UPLOAD_MAX_BYTES = 25 * 1024 * 1024

def upload(url: str, size_bytes: int) -> float:
    """
    Uploads random bytes of data to a server and measure speed in Mbps.
    Caps payload at UPLOAD_MAX_BYTES so typical asymmetric links finish.

    Returns: speed in Mbps, or 0.0 if it fails.
    """
    size_bytes = min(size_bytes, UPLOAD_MAX_BYTES)
    data = os.urandom(size_bytes)
    timeout = httpx.Timeout(connect=10.0, read=60.0, write=300.0, pool=10.0)
    try:
        start = time.perf_counter()
        response = httpx.post(url, content=data, timeout=timeout, headers=REQUEST_HEADERS)
        response.raise_for_status()
        end = time.perf_counter()
        elapsed_time = end - start
        if elapsed_time == 0:
            return 0.0
        speed = (size_bytes * 8) / elapsed_time / 1_000_000
        return round(speed, 2)
    except (httpx.HTTPError, OSError):
        return 0.0
