def analyze(result: dict, bufferbloat: dict | None = None) -> dict:
    """
    Takes a single test result (+ optional bufferbloat data)
    and returns a summary with averages and a verdict string.

    Verdict priority: worst-condition first, healthy last.
    """

    download = result.get("download_mbps") or 0.0
    upload = result.get("upload_mbps")
    ping = result.get("ping_ms") or 0.0
    jitter = result.get("jitter_ms") or 0.0
    loss = result.get("packet_loss_pct") or 0.0

    verdict = _diagnose(
        download=download,
        ping=ping,
        jitter=jitter,
        loss=loss,
        bufferbloat_delta=(bufferbloat or {}).get("delta_ms", 0.0),
    )

    return {
        "download_mbps": download,
        "upload_mbps": upload,
        "ping_ms": ping,
        "jitter_ms": jitter,
        "packet_loss_pct": loss,
        "verdict": verdict,
    }


def _diagnose(download: float, ping: float, jitter: float, loss: float, bufferbloat_delta: float) -> str:
    if loss > 5:
        return "Packet loss detected"

    if bufferbloat_delta > 200:
        return "Severe bufferbloat — calls and gaming will lag"

    if ping > 100 and download >= 10:
        return "Congestion detected"

    if jitter > 30:
        return "High jitter — calls may drop out"

    if download < 10:
        return "ISP bandwidth is just low"

    return "Connection is healthy"
