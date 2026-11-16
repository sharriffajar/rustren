# rustren

Browserless 2D video render engine in Rust driven by an AI-friendly declarative JSON schema.

`rustren` compiles a single JSON scene description into an MP4 video or PNG frame snapshots without spawning a headless browser, DOM, or CSS engine.

## Zero-Encode Preview (`snapshot` & `layout`) — AI One-Shot Loop
Unlike traditional video pipelines that force you to encode a full MP4 just to check alignment, `rustren` lets AI agents and developers inspect any timestamp in milliseconds:

```bash
# Render 6 keyframes across a 17s showreel into one 3x2 contact sheet (~0.20s, no FFmpeg)
rustren snapshot scene.json -t 1.5,3.2,5.6,7.0,11.0,14.5 --sheet --cols 3 -o assets/showreel_sheet.png

# Or inspect exact computed bounding boxes & measured text widths as JSON
rustren layout scene.json -t 14.5
```

![Rustren contact sheet preview](assets/showreel_sheet.png)

## Architecture & Why Browserless?
- **Pure-Rust Raster Pipeline**: Combines `tiny-skia` (2D vector/compositing), `cosmic-text` (text shaping & rich spans), `resvg` (SVG rendering), `taffy` (Flexbox layout), and `rayon` (parallel frame rendering), piped directly to `ffmpeg` for H.264 MP4 output.
- **Built-in Render Caches**: Static background prefix snapshotting, thread-local SVG raster/tree caches, and blurred-layer caching avoid re-computing unchanged pixels across frames.
- **Deterministic & Inspectable**: Same JSON produces identical frames across runs, with `snapshot` PNG previews and `layout` bounding-box inspection built into the CLI.

## Key Features
- **Capability Registry & Version Gate**: `rustren capabilities` publishes renderer version (`1.1`), schema version (`1.1`), and supported features.
- **Declarative Layout & Visuals**: Flexbox (`taffy`), `container` (`clip`, `cascade_opacity`, `stagger_secs`), `rect`, `circle`, `text` (rich text `spans`, `letter_spacing`, `dynamic` typewriter/counter/timecode), `svg`, `image`, `anchor`, `skew_x`/`skew_y`, plus `blur`, `shadow`, and `blend_mode`.
- **Timeline & Sequencing**: Global `start_sec`/`end_sec`, `duration_secs`, `timeline: "local"`, and `timeline: "series"` with `overlap_secs`.
- **Media & A/V Sync**: `video` layer with `trim_start_sec`, `trim_end_sec`, `playback_rate`, `volume`, and `muted` synced to FFmpeg.
- **Advanced Motion**: Custom `spring` (`stiffness`, `damping`, `mass`), `repeat` & `yoyo`, `cubic_bezier` + Penner easings, color interpolation (`color`, `border_color`), and path/stroke animation (`path_progress`, `stroke_dashoffset`).
- **AI Authoring & Debugging Tools**: Instant single-frame / contact-sheet `snapshot`, computed bounding-box `layout` inspection, smart `validate` linting, and `--profile` performance breakdown.

## Prerequisites & Installation
- **Rust toolchain** (edition `2024`, Cargo)
- **`ffmpeg`** (and `ffprobe` for `video` layers) available in `PATH`

Build from source:
```bash
git clone https://github.com/sharriffajar/rustren.git
cd rustren
cargo build --release
# Binary will be at ./target/release/rustren
```

Or install into `~/.cargo/bin`:
```bash
cargo install --path .
```

## Quick Start
Save a minimal scene as `hello.json`:
```json
{
  "schema_version": "1.1",
  "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 3.0, "background": "#0F172A" },
  "layers": [
    {
      "type": "rect",
      "x": 640,
      "y": 360,
      "width": 520,
      "height": 180,
      "radius": 24,
      "color": "#2D4AF2",
      "anchor": "center",
      "skew_x": -12,
      "animations": [
        { "property": "scale", "from": 0.8, "to": 1.0, "start_sec": 0.0, "end_sec": 0.6, "easing": "spring" }
      ]
    },
    {
      "type": "text",
      "x": 640,
      "y": 360,
      "font_size": 64,
      "font_weight": "bold",
      "color": "#FFFFFF",
      "anchor": "center",
      "dynamic": { "mode": "typewriter", "text": "rustren", "start_sec": 0.1, "end_sec": 0.9 }
    }
  ]
}
```

Validate, preview a frame, and render to MP4:
```bash
rustren validate hello.json
rustren snapshot hello.json -t 1.0 -o preview.png
rustren render hello.json -o hello.mp4
```

## CLI Commands
```bash
# Inspect renderer capabilities (JSON)
rustren capabilities

# Validate and lint a scene JSON before rendering
rustren validate scene.json

# Render a single frame or contact sheet to PNG (no FFmpeg encode)
rustren snapshot scene.json -t 1.5 -o frame.png
rustren snapshot scene.json -t 1.5,5.6,14.5 --sheet -o sheet.png

# Inspect computed layer bounding boxes & measured text dimensions
rustren layout scene.json -t 5.0

# Render scene JSON to MP4 (default: --crf 18 --preset medium)
rustren render scene.json -o output.mp4

# Render with custom encoding options and per-layer/cache profiling
rustren render scene.json -o output.mp4 --crf 20 --preset fast --profile

# Start the HTTP render API server
rustren serve --port 8080
```

## HTTP Render API (`rustren serve`)
Start the async job queue server (with SSRF protection, rate limiting, and configurable canvas/duration limits):
```bash
RUSTREN_API_KEY=your-secret rustren serve --host 0.0.0.0 --port 8080 --workers 2
```
Endpoints:
- `GET /health` — health check
- `GET /schema` — JSON Schema (`schema.json`)
- `GET /guide` — authoring guide (`SCHEMA.md`)
- `GET /capabilities` — renderer capability registry
- `POST /validate` — validate & lint a scene JSON payload
- `POST /jobs` — submit an async render job
- `GET /jobs/:id` — poll job status
- `GET /jobs/:id/video` — download rendered MP4

## Examples
Browse [examples/](examples) for full scenes you can validate or render directly, including:
- [examples/rustren_showcase_15s.json](examples/rustren_showcase_15s.json) — 15-second multi-scene showcase
- [examples/benchmark_scene.json](examples/benchmark_scene.json) — animation benchmark scene
- [examples/demo.json](examples/demo.json) — minimal starter scene

## Documentation & Testing
- Full schema reference: [SCHEMA.md](SCHEMA.md) & [schema.json](schema.json)
- Run unit & conformance tests:
```bash
cargo test --release
```

## License
Licensed under the [Apache License, Version 2.0](LICENSE).
