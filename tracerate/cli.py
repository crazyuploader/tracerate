import typer
import json
from rich.console import Console

from tracerate.info import get_ip_info, measure_dns
from tracerate.tester import (
    SERVER,
    UPLOAD_MAX_BYTES,
    ping,
    download,
    upload,
)
from rich.progress import (
    BarColumn,
    DownloadColumn,
    Progress,
    TextColumn,
    TransferSpeedColumn,
)
from tracerate.verdict import analyze
from tracerate.regional import ping_regions
from tracerate.bufferbloat import bufferbloat as measure_bufferbloat

app = typer.Typer(help="tracerate - a no-nonsense CLI internet speed tester")
console = Console()


def run_download(download_bytes: int, quiet: bool) -> float:
    """Run the main download with a live progress bar."""

    if quiet:
        return download(SERVER["download_url"], download_bytes)

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

        return download(SERVER["download_url"], download_bytes, on_progress=on_progress)


@app.command()
def run(
        quick: bool = typer.Option(default=False, help="Skip upload, bufferbloat, and regional probes. And only use 50MB download."),
        size: int = typer.Option(default=100, help="Download size in MB."),
        output: str = typer.Option(default="pretty", help="Output format: pretty or json.")
    ):
        if output not in ("pretty", "json"):
                typer.echo("--output must be 'pretty' or 'json'", err=True)
                raise typer.Exit(code=1)

        size_effective = 50 if quick else size
        download_bytes = size_effective * 1024 * 1024
        test_upload = not quick
        test_extras = not quick

        if output == "pretty":
            console.print()
            console.print("[bold]tracerate[/bold] [dim]· network diagnostics[/dim]")
            console.print()

        with console.status("[dim]Looking up your ISP...[/dim]", spinner="dots"):
            info = get_ip_info()
            dns_ms = measure_dns(SERVER["host"])

        with console.status("[dim]Measuring latency...[/dim]", spinner="dots"):
            ping_ms, loss_pct, jitter_ms = ping(SERVER["host"], SERVER["port"])

        download_mbps = run_download(download_bytes, quiet=(output == "json"))

        upload_mbps = None
        if test_upload:
            upload_mb = min(size_effective, UPLOAD_MAX_BYTES // (1024 * 1024))
            with console.status(f"[dim]Uploading {upload_mb} MB...[/dim]", spinner="dots"):
                upload_mbps = upload(SERVER["upload_url"], download_bytes)

        bufferbloat = None
        if test_extras:
            with console.status("[dim]Probing bufferbloat (saturating link)...[/dim]", spinner="dots"):
                bufferbloat = measure_bufferbloat()

        regions=[]
        if test_extras:
            with console.status("[dim]Pinging regional servers...[/dim]", spinner="dots"):
                regions = ping_regions()

        result = {
            "name": SERVER["name"],
            "ping_ms": ping_ms,
            "packet_loss": loss_pct,
            "jitter_ms": jitter_ms,
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

        render(info, dns_ms, result, bufferbloat, regions, summary)


_DIVIDER = "[dim]" + "─" * 56 + "[/dim]"

def bar(value: float, max_value: float, width: int = 20, filled: str = "▰", empty: str = "▱") -> str:
    if max_value <= 0:
        return empty * width
    ratio = min(value, max_value) / max_value
    n = int(round(ratio * width))
    return filled * n + empty * (width - n)

def section(title: str) -> None:
    console.print(f"  [bold cyan]{title}[/bold cyan]")
    console.print(f"  {_DIVIDER}")


def render(info, dns_ms, r, bb, regions, summary):
    render_connection(info, dns_ms)
    render_speed(r)

    if bb is not None:
        render_bufferbloat(bb)

    if regions:
        render_regions(regions)

    render_verdict(summary["summary"])


def render_connection(info: dict, dns_ms: float) -> None:
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


def render_speed(r: dict) -> None:
    section("Speed")

    dl = r.get("download_mbps") or 0.0
    ul = r.get("upload_mbps") or 0.0
    ping = r.get("ping_ms") or 0.0
    jitter = r.get("jitter_ms") or 0.0
    loss = r.get("packet_loss") or 0.0

    scale = max(dl, ul, 100.0)

    console.print(f"   [dim]Download[/dim]  [cyan]{bar(dl, scale)}[/cyan]   [bold]{dl:>7.2f}[/bold] [dim]Mbps[/dim]")
    if r.get("upload_mbps") is not None:
        console.print(f"   [dim]Upload  [/dim]  [cyan]{bar(ul, scale)}[/cyan]   [bold]{ul:>7.2f}[/bold] [dim]Mbps[/dim]")

    loss_part = f"[red]· {loss}% loss[/red]" if loss > 0 else "[dim]· 0% loss[/dim]"
    console.print(f"   [dim]Ping    [/dim]  [bold]{ping}[/bold] [dim]ms[/dim]   {loss_part}")
    console.print(f"   [dim]Jitter  [/dim]  [bold]{jitter}[/bold] [dim]ms[/dim]")
    console.print()


_GRADE_COLOR = {
    "A+": "green", "A": "green",
    "B": "cyan",
    "C": "yellow", "D": "yellow",
    "F": "red", "?": "dim",
}


def render_bufferbloat(bb: dict) -> None:
    section("Bufferbloat")
    grade = bb["grade"]
    color = _GRADE_COLOR.get(grade, "dim")
    console.print(f"   [dim]Idle  [/dim]  [bold]{bb['idle_ms']}[/bold] [dim]ms[/dim]")
    console.print(
        f"   [dim]Loaded[/dim]  [bold]{bb['loaded_ms']}[/bold] [dim]ms[/dim]"
        f"   [dim]Δ[/dim] [bold]+{bb['delta_ms']}[/bold] [dim]ms[/dim]"
        f"   [dim]Grade[/dim] [{color}][bold]{grade}[/bold][/{color}]"
    )
    console.print()


def render_regions(regions: list[dict]) -> None:
    section("Regional latency")
    reachable = [r for r in regions if r["ms"] > 0]
    scale = max((r["ms"] for r in reachable), default=200.0)

    ordered = sorted(regions, key=lambda r: r["ms"] if r["ms"] > 0 else 1e9)
    for r in ordered:
        ms = r["ms"]
        if ms == 0:
            bar_str = "[dim]" + "▱" * 12 + "[/dim]"
            ms_str = "[dim]timeout[/dim]"
        else:
            color = "cyan" if ms < 80 else ("yellow" if ms < 180 else "red")
            bar_str = f"[{color}]{bar(ms, scale, width=12)}[/{color}]"
            ms_str = f"[bold]{ms:>6.0f}[/bold] [dim]ms[/dim]"
        console.print(f"   [dim]{r['code']}[/dim]  {r['city']:<11}  {bar_str}   {ms_str}")
    console.print()


def render_verdict(verdict: str) -> None:
    if verdict == "Connection looks healthy.":
        mark, color = "✔", "green"
    elif verdict in ("Low bandwidth, ISP speed is the bottleneck.",):
        mark, color = "⚠", "yellow"
    else:
        mark, color = "✘", "red"

    console.print(f"  [{color}]{mark}[/{color}]  [bold]{verdict}[/bold]")
    console.print()
