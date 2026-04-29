import json

import typer
from rich.console import Console
from rich.progress import (
    BarColumn,
    DownloadColumn,
    Progress,
    TextColumn,
    TransferSpeedColumn,
)

from tracerate.bufferbloat import measure_bufferbloat
from tracerate.info import get_ip_info, measure_dns
from tracerate.regional import measure_regions
from tracerate.tester import (
    SERVER,
    measure_download,
    measure_ping,
    measure_upload,
)
from tracerate.verdict import analyze


app = typer.Typer(help="tracerate — a no-nonsense CLI internet speed tester")
console = Console()


@app.command()
def run(
    quick: bool = typer.Option(
        default=False,
        help="Skip upload + bufferbloat + regional probes; use 10MB download.",
    ),
    bytes_mb: int = typer.Option(
        default=25,
        help="Download size in MB.",
    ),
    output: str = typer.Option(
        default="pretty",
        help="Output format: pretty or json.",
    ),
):
    if output not in ("pretty", "json"):
        typer.echo("--output must be 'pretty' or 'json'", err=True)
        raise typer.Exit(code=1)

    download_bytes = (10 if quick else bytes_mb) * 1024 * 1024
    test_upload = not quick
    test_extras = not quick

    if output == "pretty":
        console.print()
        console.print("  [bold]tracerate[/bold] [dim]· network diagnostics[/dim]")
        console.print()

    with console.status("[dim]Looking up your ISP...[/dim]", spinner="dots"):
        info = get_ip_info()
        dns_ms = measure_dns(SERVER["host"])

    with console.status("[dim]Measuring latency...[/dim]", spinner="dots"):
        ping_ms, loss_pct = measure_ping(SERVER["host"], SERVER["port"])

    download_mbps = _run_download(download_bytes, quiet=(output == "json"))

    upload_mbps = None
    if test_upload:
        with console.status("[dim]Uploading 10 MB...[/dim]", spinner="dots"):
            upload_mbps = measure_upload(SERVER["up"])

    bufferbloat = None
    if test_extras:
        with console.status("[dim]Probing bufferbloat (saturating link)...[/dim]", spinner="dots"):
            bufferbloat = measure_bufferbloat()

    regions = []
    if test_extras:
        with console.status("[dim]Pinging regional servers...[/dim]", spinner="dots"):
            regions = measure_regions()

    result = {
        "name": SERVER["name"],
        "ping_ms": ping_ms,
        "packet_loss_pct": loss_pct,
        "download_mbps": download_mbps,
        "upload_mbps": upload_mbps,
        "error": None,
    }
    summary = analyze(result, bufferbloat=bufferbloat)

    if output == "json":
        print(json.dumps({
            "info": info,
            "dns_ms": dns_ms,
            "result": result,
            "bufferbloat": bufferbloat,
            "regions": regions,
            "summary": summary,
        }, indent=2))
        return

    _render(info, dns_ms, result, bufferbloat, regions, summary)


def _run_download(download_bytes: int, quiet: bool) -> float:
    """Run the main download with a live progress bar."""

    if quiet:
        return measure_download(SERVER["down"], download_bytes)

    with Progress(
        TextColumn("  [dim]Downloading[/dim]"),
        BarColumn(bar_width=30, complete_style="cyan", finished_style="cyan"),
        DownloadColumn(),
        TransferSpeedColumn(),
        console=console,
        transient=True,
    ) as progress:
        task = progress.add_task("dl", total=download_bytes)

        def on_progress(total: int) -> None:
            progress.update(task, completed=total)

        return measure_download(SERVER["down"], download_bytes, on_progress=on_progress)


# ────────────────────────── rendering ──────────────────────────

_DIVIDER = "[dim]" + "─" * 56 + "[/dim]"


def _bar(value: float, max_value: float, width: int = 20,
         filled: str = "▰", empty: str = "▱") -> str:
    if max_value <= 0:
        return empty * width
    ratio = min(value, max_value) / max_value
    n = int(round(ratio * width))
    return filled * n + empty * (width - n)


def _section(title: str) -> None:
    console.print(f"  [bold cyan]{title}[/bold cyan]")
    console.print(f"  {_DIVIDER}")


def _render(info, dns_ms, r, bb, regions, summary):
    _render_connection(info, dns_ms)
    _render_speed(r)

    if bb is not None:
        _render_bufferbloat(bb)

    if regions:
        _render_regions(regions)

    _render_verdict(summary["verdict"])


def _render_connection(info: dict, dns_ms: float) -> None:
    isp       = info.get("isp")       or "unknown"
    asn       = info.get("asn")       or ""
    city      = info.get("city")      or "?"
    country   = info.get("country")   or "?"
    colo      = info.get("colo")      or "?"
    colo_city = info.get("colo_city")
    ip        = info.get("ip")        or "?"

    dns_color = "dim" if dns_ms < 50 else ("yellow" if dns_ms < 150 else "red")
    edge = f"Cloudflare [bold]{colo}[/bold]" + (f" [dim]({colo_city})[/dim]" if colo_city else "")

    console.print(f"  [dim]ISP    [/dim]  [bold]{isp}[/bold]   [dim]{asn}[/dim]")
    console.print(f"  [dim]Where  [/dim]  {city}, {country}  [dim]→[/dim]  {edge}")
    console.print(f"  [dim]IP     [/dim]  {ip}   [dim]·  DNS[/dim] [{dns_color}]{dns_ms} ms[/{dns_color}]")
    console.print()


def _render_speed(r: dict) -> None:
    _section("Speed")

    dl = r.get("download_mbps") or 0.0
    ul = r.get("upload_mbps") or 0.0
    ping = r.get("ping_ms") or 0.0
    loss = r.get("packet_loss_pct") or 0.0

    scale = max(dl, ul, 100.0)

    console.print(f"   [dim]Download[/dim]  [cyan]{_bar(dl, scale)}[/cyan]   [bold]{dl:>7.2f}[/bold] [dim]Mbps[/dim]")
    if r.get("upload_mbps") is not None:
        console.print(f"   [dim]Upload  [/dim]  [cyan]{_bar(ul, scale)}[/cyan]   [bold]{ul:>7.2f}[/bold] [dim]Mbps[/dim]")

    loss_part = f"[red]· {loss}% loss[/red]" if loss > 0 else "[dim]· 0% loss[/dim]"
    console.print(f"   [dim]Ping    [/dim]  [bold]{ping}[/bold] [dim]ms[/dim]   {loss_part}")
    console.print()


_GRADE_COLOR = {
    "A+": "green", "A": "green",
    "B": "cyan",
    "C": "yellow", "D": "yellow",
    "F": "red", "?": "dim",
}


def _render_bufferbloat(bb: dict) -> None:
    _section("Bufferbloat")
    grade = bb["grade"]
    color = _GRADE_COLOR.get(grade, "dim")
    console.print(f"   [dim]Idle  [/dim]  [bold]{bb['idle_ms']}[/bold] [dim]ms[/dim]")
    console.print(
        f"   [dim]Loaded[/dim]  [bold]{bb['loaded_ms']}[/bold] [dim]ms[/dim]"
        f"   [dim]Δ[/dim] [bold]+{bb['delta_ms']}[/bold] [dim]ms[/dim]"
        f"   [dim]Grade[/dim] [{color}][bold]{grade}[/bold][/{color}]"
    )
    console.print()


def _render_regions(regions: list[dict]) -> None:
    _section("Regional latency")
    reachable = [r for r in regions if r["ms"] > 0]
    scale = max((r["ms"] for r in reachable), default=200.0)

    ordered = sorted(regions, key=lambda r: r["ms"] if r["ms"] > 0 else 1e9)
    for r in ordered:
        ms = r["ms"]
        if ms == 0:
            bar = "[dim]" + "▱" * 12 + "[/dim]"
            ms_str = "[dim]timeout[/dim]"
        else:
            color = "cyan" if ms < 80 else ("yellow" if ms < 180 else "red")
            bar = f"[{color}]{_bar(ms, scale, width=12)}[/{color}]"
            ms_str = f"[bold]{ms:>6.0f}[/bold] [dim]ms[/dim]"
        console.print(f"   [dim]{r['code']}[/dim]  {r['city']:<11}  {bar}   {ms_str}")
    console.print()


def _render_verdict(verdict: str) -> None:
    if verdict == "Connection is healthy":
        mark, color = "✔", "green"
    elif verdict in ("ISP bandwidth is just low",):
        mark, color = "⚠", "yellow"
    else:
        mark, color = "✘", "red"

    console.print(f"  [{color}]{mark}[/{color}]  [bold]{verdict}[/bold]")
    console.print()
