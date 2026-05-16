# tracerate

A no-nonsense CLI internet speed tester.

## What it measures
- Download / upload speed (Mbps)
- Ping and packet loss
- Bufferbloat grade (A+ to F)
- DNS resolution time
- ISP and location detection
- Regional latency to 8 global servers

## Install
pip install tracerate

## Usage

| Command | Description |
|---|---|
| `tracerate` | Full test (download, upload, bufferbloat, regional latency) |
| `tracerate --quick` | Fast test (10s download only, skips upload and extras) |
| `tracerate --duration 30` | Custom download duration in seconds (default: 15) |
| `tracerate --streams 8` | Parallel download streams (default: 6) |
| `tracerate --output json` | Machine readable output |
