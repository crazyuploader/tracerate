def analyze(results: list[dict]) -> dict:
    """
    Takes raw test results and computes summary statistics.

    We separate computation from diagnosis intentionally,
    easier to test and reason about each part independently.

    Returns a dict with averages, variance, and the verdict string.
    """

    download_speeds = [
        r["download_mbps"]
        for r in results
        if r["download_mbps"] is not None and r["download_mbps"] > 0
    ]

    ping_times = [
        r["ping_ms"]
        for r in results
        if r["ping_ms"] is not None and r["ping_ms"] > 0
    ]

    upload_speeds = [
        r["upload_mbps"]
        for r in results
        if r["upload_mbps"] is not None and r["upload_mbps"] > 0
    ]


    avg_download = (
        round(sum(download_speeds) / len(download_speeds), 2)
        if download_speeds else 0.0
    )

    avg_ping = (
        round(sum(ping_times) / len(ping_times), 2)
        if ping_times else 0.0
    )

    avg_upload = (
        round(sum(upload_speeds) / len(upload_speeds), 2)
        if upload_speeds else None
    )

    variance_pct = 0.0

    if len(download_speeds) > 1 and avg_download > 0:
        spread = max(download_speeds) - min(download_speeds)
        variance_pct = round((spread / avg_download) * 100, 1)

    verdict = _diagnose(
        avg_download=avg_download,
        avg_ping=avg_ping,
        variance_pct=variance_pct,
        n_servers=len(results),
    )

    return {
        "avg_download_mbps": avg_download,
        "avg_upload_mbps": avg_upload,
        "avg_ping_ms": avg_ping,
        "variance_pct": variance_pct,
        "verdict": verdict,
    }


def _diagnose(
    avg_download: float,
    avg_ping: float,
    variance_pct: float,
    n_servers: int,
) -> str:
    """
    Maps numbers to a human readable diagnosis.

    Rules are checked in priority order,
    worst condition first, healthy last.
    """

    if n_servers > 1 and variance_pct > 30:
        return "Routing instability detected"

    if avg_ping > 100 and avg_download >= 10:
        return "Congestion detected"

    if avg_download < 10:
        return "ISP bandwidth is just low"

    return "Connection is healthy"
