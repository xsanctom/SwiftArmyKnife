# Swift Army Knife

A tiny, native macOS app for quick file chores. Drop **files or a whole folder** and
pick what you want:

- **Video** — Convert to MP4 · Compress · Extract audio · Make GIF
- **Image** — Convert (JPG/PNG/WebP) · Resize · Compress
- **Spreadsheet** — CSV ⇄ XLSX

…with a live progress bar, cancel, and an Advanced sheet for tuning. Drop many files
or a folder to batch-convert; each output lands next to its source, suffixed, and
never overwrites the original. The menu adapts to what you drop.

A SwiftUI shell over a Rust engine (via [swift-bridge](https://github.com/chinedufn/swift-bridge)).
**Apple-silicon only.**

## Setup

### 1. Runtime dependencies (to *use* the app)

| Tool | Used for | Ships with macOS? |
|---|---|---|
| `ffmpeg` / `ffprobe` | all video ops, WebP images | no — install |
| `sips` | HEIC decode, image convert/resize/compress | ✅ built in |
| `python3` + `openpyxl` | CSV ⇄ XLSX | no — install |

Install everything in one line (via [Homebrew](https://brew.sh)):

```sh
brew install ffmpeg python && /opt/homebrew/bin/python3 -m pip install --break-system-packages openpyxl
```

> `--break-system-packages` is needed because Homebrew's Python is
> [PEP 668](https://peps.python.org/pep-0668/) "externally managed". It installs
> `openpyxl` into that Python's site-packages, which is what the app calls
> (`/opt/homebrew/bin/python3`). If you'd rather not touch the system Python, a
> virtualenv works too — just point the app's Python path at it.

If `ffmpeg` is missing the app tells you and offers this command; if `python3`/`openpyxl`
are missing, only spreadsheet conversions are affected (media still work).

### 2. Build dependencies (to *build* from source)

- **Xcode** (from the App Store) — for the Swift compiler + macOS SDK.
- **Rust** — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

(swift-bridge and the crates are fetched automatically by Cargo.)

## Build & run

```sh
./build.sh
open build/SwiftArmyKnife.app
```

`build.sh` compiles the Rust core (release), generates the swift-bridge glue, compiles
the SwiftUI app, and assembles the `.app` bundle.

## How it works

```
Finder drag ─▶ SwiftUI window ──swift-bridge──▶ Rust core ──spawn──▶ ffmpeg / sips / python3
                    ▲                                │
                    └────── progress (poll) ─────────┘
```

- **Swift** (`app/`) — window, drag-and-drop (files + folders), presets, Advanced sheet,
  progress/cancel UI, batch orchestration.
- **Rust** (`core/`) — probes each file, builds the command sequence for the chosen op,
  spawns and supervises the right tool (ffmpeg / sips / python3), parses progress, and
  manages the collision-free output path.

Tool choices worth knowing:

- **HEIC/HEIF/AVIF** are decoded with `sips` (ffmpeg mis-decodes tiled HEIC to a single
  tile). Image ops use `sips` generally so EXIF orientation is preserved.
- **Spreadsheets** go through a small `python3` + `openpyxl` script (both directions,
  one script). Old binary `.xls` isn't supported (openpyxl can't read it).

## Test

```sh
cd core && cargo test
```

The Rust core is fully testable headless — argv building, output naming, progress
parsing, cancellation, and an end-to-end pass against real ffmpeg.

## Scope

Video, image, and spreadsheet (CSV ⇄ XLSX) files, single or batched. Old binary `.xls`,
audio-file and PDF tools, and a bundled (notarized) ffmpeg are possible future additions.
