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
| `tracerate --quick` | Fast test (download only, skips upload and extras) |
| `tracerate --size 500` | Custom download size in MB (default: 100) |
| `tracerate --output json` | Machine readable output |
