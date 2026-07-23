# Swift Army Knife

A tiny, native macOS app for quick video chores. Drop a video onto the window and
pick what you want: **Convert to MP4 · Compress · Extract audio · Make GIF** — with
a live progress bar, cancel, and an Advanced sheet for tuning. Output lands next to
the source, suffixed, and never overwrites the original.

A SwiftUI shell over a Rust engine (via [swift-bridge](https://github.com/chinedufn/swift-bridge)),
wrapping `ffmpeg`/`ffprobe`. Apple-silicon only.

## How it works

```
Finder drag ─▶ SwiftUI window ──swift-bridge──▶ Rust core ──spawn──▶ ffmpeg/ffprobe
                    ▲                                │
                    └────── progress (poll) ─────────┘
```

- **Swift** (`app/`) — the window, drag-and-drop, presets, Advanced sheet, progress UI.
- **Rust** (`core/`) — probes the file, builds the ffmpeg command sequence, spawns and
  supervises it, parses progress, and manages the collision-free output path. Detection
  is content-based (ffprobe), so a mislabeled or audio-only file is handled correctly.

## Requirements

- macOS on Apple silicon
- [`ffmpeg`](https://ffmpeg.org): `brew install ffmpeg` (the app detects it and, if it's
  missing, shows an install prompt instead of failing silently)
- To build: Xcode + [Rust](https://rustup.rs)

## Build & run

```sh
./build.sh
open build/SwiftArmyKnife.app
```

`build.sh` compiles the Rust core (release), generates the swift-bridge glue, compiles
the SwiftUI app, and assembles the `.app` bundle.

## Test

```sh
cd core && cargo test
```

The Rust core is fully testable headless — command building, output naming, progress
parsing, cancellation, and an end-to-end pass against real ffmpeg.

## v1 scope

Video only. Batch/folder drop, image and PDF tools, and a bundled (notarized) ffmpeg
are possible future additions.
