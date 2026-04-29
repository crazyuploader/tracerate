import json
import typer

from rich.console import Console
from rich.table import Table
from rich.text import Text
from rich import print as rprint

from tracerate.tester import run_tests, SERVERS
from tracerate.verdict import analyze

app = typer.Typer(
    help="tracerate — a no-nonsense CLI internet speed tester"
)

console = Console()

@app.command()
def run(
    servers: int = typer.Option(
        default=1,
        help="Number of servers to test against (1-3)",
    ),
    quick: bool = typer.Option(
        default=False,
        help="Skip upload, use 10MB download instead of 25MB",
    ),
    bytes_mb: int = typer.Option(
        default=25,
        help="Download size in MB per server",
    ),
    output: str = typer.Option(
        default="pretty",
        help="Output format: pretty or json",
    ),
):
    max_servers = len(SERVERS)
    if servers < 1 or servers > max_servers:
        typer.echo(
            f"--servers must be between 1 and {max_servers}",
            err=True
        )
        raise typer.Exit(code=1)

    if output not in ("pretty", "json"):
        typer.echo("--output must be 'pretty' or 'json'", err=True)
        raise typer.Exit(code=1)

    if quick:
        download_bytes = 10 * 1024 * 1024
    else:
        download_bytes = bytes_mb * 1024 * 1024

    test_upload = not quick

    results = run_tests(
        n_servers=servers,
        bytes_to_download=download_bytes,
        test_upload=test_upload,
    )
    summary = analyze(results)

    if output == "json":
        _print_json(results, summary)
    else:
        _print_pretty(results, summary)

def _print_json(results: list[dict], summary: dict) -> None:
    """
    Prints raw JSON to stdout.
    Useful for piping into other tools or scripts.
    """

    output = {
        "servers": results,
        "summary": summary,
    }

    print(json.dumps(output, indent=2))

def _print_pretty(results: list[dict], summary: dict) -> None:
    """
    Prints a rich formatted table with per-server results
    and a summary + verdict at the bottom.
    """

    console.print()

    table = Table(
        title="tracerate results",
        show_header=True,
        header_style="bold cyan",
    )

    table.add_column("Server",       style="bold")
    table.add_column("Ping (ms)",    justify="right")
    table.add_column("Packet Loss",  justify="right")
    table.add_column("Download",     justify="right")
    table.add_column("Upload",       justify="right")

    for r in results:
        ping     = f"{r['ping_ms']} ms"      if r["ping_ms"]        else "—"
        loss     = f"{r['packet_loss_pct']}%" if r["packet_loss_pct"] is not None else "—"
        download = f"{r['download_mbps']} Mbps" if r["download_mbps"] else "—"
        upload   = f"{r['upload_mbps']} Mbps"   if r["upload_mbps"]   else "—"

        if r["packet_loss_pct"] and r["packet_loss_pct"] > 0:
            loss = f"[red]{loss}[/red]"

        if r["download_mbps"]:
            if r["download_mbps"] >= 25:
                download = f"[green]{download}[/green]"
            else:
                download = f"[yellow]{download}[/yellow]"

        table.add_row(r["name"], ping, loss, download, upload)

    console.print(table)


    console.print()
    console.print(f"  [bold]Avg Download:[/bold]  {summary['avg_download_mbps']} Mbps")
    console.print(f"  [bold]Avg Ping:[/bold]      {summary['avg_ping_ms']} ms")

    if summary["avg_upload_mbps"] is not None:
        console.print(f"  [bold]Avg Upload:[/bold]    {summary['avg_upload_mbps']} Mbps")

    if summary["variance_pct"] > 0:
        console.print(f"  [bold]Variance:[/bold]      {summary['variance_pct']}%")

    console.print()

    verdict = summary["verdict"]

    if verdict == "Connection is healthy":
        verdict_text = f"[bold green]✓ {verdict}[/bold green]"
    elif verdict == "ISP bandwidth is just low":
        verdict_text = f"[bold yellow]⚠ {verdict}[/bold yellow]"
    else:
        verdict_text = f"[bold red]✗ {verdict}[/bold red]"

    console.print(f"  [bold]Verdict:[/bold]       {verdict_text}")
    console.print()