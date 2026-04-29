import socket
import threading
import time

import httpx

from tracerate.tester import SERVER


def _saturate_download(stop_flag: threading.Event, url: str) -> None:
    """Stream a large download until told to stop. Discards bytes."""
    try:
        with httpx.stream("GET", url, timeout=60, follow_redirects=True) as response:
            for _ in response.iter_bytes():
                if stop_flag.is_set():
                    break
    except Exception:
        pass


def _sample_ping(host: str, port: int) -> float | None:
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(2)
    try:
        start = time.perf_counter()
        sock.connect((host, port))
        return (time.perf_counter() - start) * 1000
    except (socket.timeout, socket.error):
        return None
    finally:
        try:
            sock.close()
        except Exception:
            pass


def measure_bufferbloat(duration: float = 5.0, baseline_attempts: int = 8) -> dict:
    """
    Saturate the link with a download in a background thread,
    sample ping repeatedly during the saturation, compare to idle.

    Why this matters: bufferbloat is when your modem queues
    packets under load. Throughput stays high but latency
    explodes — exactly what kills video calls and gaming.

    Idle and loaded both use min-of-samples — the floor is
    the true RTT, higher samples are jitter.

    Grade scale follows the Waveform bufferbloat test convention.
    """

    idle_samples = []
    for _ in range(baseline_attempts):
        ms = _sample_ping(SERVER["host"], SERVER["port"])
        if ms is not None:
            idle_samples.append(ms)
        time.sleep(0.05)

    if not idle_samples:
        return {"idle_ms": 0.0, "loaded_ms": 0.0, "delta_ms": 0.0, "grade": "?"}

    idle = min(idle_samples)

    url = SERVER["down"].format(bytes=200_000_000)
    stop = threading.Event()
    worker = threading.Thread(target=_saturate_download, args=(stop, url), daemon=True)
    worker.start()

    time.sleep(0.5)

    samples = []
    end_time = time.time() + duration
    while time.time() < end_time:
        ms = _sample_ping(SERVER["host"], SERVER["port"])
        if ms is not None:
            samples.append(ms)
        time.sleep(0.2)

    stop.set()
    worker.join(timeout=2)

    if not samples:
        return {
            "idle_ms": round(idle, 2),
            "loaded_ms": 0.0,
            "delta_ms": 0.0,
            "grade": "?",
        }

    loaded = min(samples)
    delta = max(0.0, loaded - idle)

    if   delta < 5:    grade = "A+"
    elif delta < 30:   grade = "A"
    elif delta < 60:   grade = "B"
    elif delta < 200:  grade = "C"
    elif delta < 400:  grade = "D"
    else:              grade = "F"

    return {
        "idle_ms": round(idle, 2),
        "loaded_ms": round(loaded, 2),
        "delta_ms": round(delta, 2),
        "grade": grade,
    }
