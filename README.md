# tracerate

A no-nonsense CLI internet speed tester, written in Rust. Ported from [tracerate](https://github.com/rushil-b-patel/tracerate) by rushil-b-patel.

## What it measures

- Download / upload speed (Mbps)
- Ping and packet loss
- Bufferbloat grade (A+ to F)
- DNS resolution time
- ISP and location detection
- Regional latency to 8 global servers

## Usage

| Command                   | Description                                                    |
| ------------------------- | -------------------------------------------------------------- |
| `tracerate`               | Full test (download, upload, bufferbloat, regional latency)    |
| `tracerate --quick`       | Fast test (10s download only, skips upload and extras)         |
| `tracerate --combined`    | Add a simultaneous download+upload test after sequential tests |
| `tracerate --duration 30` | Custom download/upload duration in seconds (default: 15)       |
| `tracerate --streams 8`   | Parallel streams for download/upload (default: 6)              |
| `tracerate --output json` | Machine-readable JSON output                                   |
| `tracerate --verbose`     | Verbose output (e.g. data used during bufferbloat)             |

## Build from source

```sh
git clone <repo>
cd tracerate
cargo build --release
./target/release/tracerate
```
