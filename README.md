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
| `tracerate`                    | Full test (download, upload, bufferbloat, regional latency)                                                  |
| `tracerate --quick`            | Fast test (10s download only, skips upload and extras)                                                       |
| `tracerate --combined`         | Add a simultaneous download+upload test after sequential tests                                               |
| `tracerate --duration 30`      | Custom download/upload duration in seconds, 1-300 (default: 15)                                              |
| `tracerate --streams 8`        | Parallel streams for download/upload, 1-32 (default: 16)                                                     |
| `tracerate --output json`      | Machine-readable JSON output                                                                                 |
| `tracerate --verbose`          | Verbose output (e.g. data used during bufferbloat)                                                           |
| `tracerate --download-url URL` | Custom download URL (must contain `{bytes}`); also sets the ping/DNS host. Bufferbloat still uses Cloudflare |
| `tracerate --upload-url URL`   | Custom upload (POST) URL                                                                                     |
| `tracerate --no-color`         | Disable colored output (`NO_COLOR` is also respected)                                                        |

Exit codes: `0` on success, `1` if the download measured nothing or arguments are invalid, `130` on Ctrl-C.

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
