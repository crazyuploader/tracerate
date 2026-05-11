import socket
import time
from typing import Callable

SERVER = {
    "name": "Cloudflare",
    "download_url": "https://speed.cloudflare.com/__down?bytes={bytes}",
    "upload_url" : "https://speed.cloudflare.com/__up",
    "host": "speed.cloudflare.com",
    "port": 443,
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
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(3)

        try:
            start = time.perf_counter()
            s.connect((host, port))
            end = time.perf_counter()

            latency = (end - start) * 1000
            results.append(latency)

        except (socket.timeout, socket.error):
            results.append(None)
            pass
        finally:
            s.close()

    valid_results = [r for r in results if r is not None]
    if not valid_results:
        return None, 100.0, None

    average_latency = sum(valid_results) / len(valid_results)
    packet_loss = ((attempts - len(valid_results)) / attempts) * 100
    jitter = max(valid_results) - min(valid_results)

    return average_latency, packet_loss, jitter
