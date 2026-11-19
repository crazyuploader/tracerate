# tracerate

[![Build and Lint](https://github.com/crazyuploader/tracerate/actions/workflows/build-and-lint.yml/badge.svg)](https://github.com/crazyuploader/tracerate/actions/workflows/build-and-lint.yml)
[![Release](https://github.com/crazyuploader/tracerate/actions/workflows/release.yml/badge.svg)](https://github.com/crazyuploader/tracerate/actions/workflows/release.yml)
[![DeepSource](https://app.deepsource.com/gh/crazyuploader/tracerate.svg/?label=active+issues&show_trend=true)](https://app.deepsource.com/gh/crazyuploader/tracerate/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A no-nonsense CLI internet speed tester, written in Rust. Ported from [tracerate](https://github.com/rushil-b-patel/tracerate) by rushil-b-patel.

## What it measures

- Download / upload speed (Mbps, auto-scales to Gbps)
- Ping, jitter, and connection-failure rate
- Bufferbloat grade (A+ to F)
- DNS resolution time
- ISP and location detection
- Regional latency to 18 global servers

## Usage

| Command                        | Description                                                                                                  |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `tracerate`                    | Full test (download, upload, latency under load, regional latency)                                           |
| `tracerate --quick`            | Fast test (10s download only, skips upload and regional latency)                                             |
| `tracerate --combined`         | Run download and upload simultaneously instead of one after the other                                        |
| `tracerate --duration 30`      | Max download/upload duration in seconds, 1-300; stops early once speed is stable (default: 30)               |
| `tracerate --streams 8`        | Parallel streams for download/upload, 1-32 (default: 16)                                                     |
| `tracerate --output json`      | Machine-readable JSON output                                                                                 |
| `tracerate --verbose`          | Verbose output: server IP, streams, ping low/high, test durations                                            |
| `tracerate --fixed-duration`   | Always run the full --duration (no early stop once speed is stable)                                          |
| `tracerate --max-data 50`      | Stop speed tests after about 50 MB, for metered connections (uses at most 4 streams)                         |
| `tracerate -4` / `-6`          | Use IPv4 only / IPv6 only                                                                                    |
| `tracerate --download-url URL` | Custom download URL (must contain `{bytes}`); also sets the ping, DNS and latency-under-load host            |
| `tracerate --upload-url URL`   | Custom upload (POST) URL                                                                                     |
| `tracerate --no-color`         | Disable colored output (`NO_COLOR` is also respected)                                                        |

Exit codes: `0` on success, `1` if the download measured nothing, `2` on invalid arguments, `3` if the test finished but found a problem (packet loss, severe bufferbloat, high latency or jitter), `130` on Ctrl-C.

## Install

### Homebrew (macOS / Linux)

```sh
brew tap crazyuploader/tools
brew install --cask tracerate
```

### Prebuilt binaries

Download archives for Linux (AMD64/ARM64), macOS (Intel/ARM64), and Windows (AMD64) from the [releases page](https://github.com/crazyuploader/tracerate/releases).

## Build from source

```sh
git clone <repo>
cd tracerate
cargo build --release
./target/release/tracerate
```
