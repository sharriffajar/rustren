# rustren

Browserless 2D video render engine in Rust — built to be **fast** and **predictable for AI-generated video**.

```
AI generates → Schema validates → Capability checks → Snapshot/Layout previews → Renderer executes → Regression tests verify
```

`rustren` compiles a single declarative JSON scene into an MP4 video or PNG frame snapshots without spawning a headless browser, DOM, or CSS engine. Every stage of the pipeline is explicit, inspectable, and deterministic.

## Teaser — Rustren Showreel (17s, 30fps)
**100% Rust, no browser. `JSON in → MP4 out.`**

- **Source JSON:** [examples/showreel.json](examples/showreel.json) (`44` root layers, `65 KB`)
- **Output MP4:** [assets/showreel.mp4](assets/showreel.mp4) (`1280×720 @ 30fps`, `17.0s` / `510` frames, `--crf 18 --preset medium`)
- **Hardware:** `4 vCPU` Intel Xeon E5-2630 v4 @ 2.20GHz, `15 GiB` RAM (CPU-only, no GPU)

| Metric | Measured (`examples/showreel.json`) |
| :--- | :--- |
| **Render Time (Wall Clock)** | `6.50s` |
| **Average Speed** | `78.5 fps` (`2.62×` real-time) |
| **Peak RAM (Max RSS)** | `305.6 MB` (`312,916 KB`) |
| **MP4 File Size** | `358.8 KB` (`367,401 bytes`) |
| **Total Bitrate** | `172.9 kbps` |
| **Duration / Frame** | `12.7 ms / frame` |

<video src="assets/showreel.mp4" controls muted playsinline width="720"></video>

## Zero-Encode Preview (`snapshot` & `layout`) — AI One-Shot Loop
Unlike traditional video pipelines that force you to encode a full MP4 just to check alignment, `rustren` lets AI agents and developers inspect any timestamp in milliseconds:

![Rustren contact sheet preview](assets/showreel_sheet.png)

```bash
# Render 6 keyframes across the 17s showreel into one 3x2 contact sheet (~0.22s, no FFmpeg)
rustren snapshot examples/showreel.json -t 1.5,3.2,5.6,7.0,11.0,14.5 --sheet --cols 3 -o assets/showreel_sheet.png

# Or inspect exact computed bounding boxes & measured text widths as JSON
rustren layout examples/showreel.json -t 14.5

# Watch mode: re-validate and re-render the active frame on every save (~12ms/frame)
rustren watch examples/showreel.json -t 1.5 -o preview.png

# Visual regression test: compare two scenes at timestamp t and fail if MAE > threshold
rustren diff baseline.json current.json -t 1.5 --threshold 0.01 --out diff.png
```

## Architecture & Why Browserless?
- **Pure-Rust Raster Pipeline**: Combines `tiny-skia` (2D vector/compositing), `cosmic-text` (text shaping & rich spans), `resvg` (SVG rendering), `taffy` (Flexbox layout), and `rayon` (parallel frame rendering), piped directly to `ffmpeg` for H.264 MP4 output.
- **Built-in Render Caches**: Static background prefix snapshotting, thread-local SVG raster/tree caches, and blurred-layer caching avoid re-computing unchanged pixels across frames.
- **Deterministic & Inspectable**: Same JSON produces identical frames across runs, with `snapshot` PNG previews and `layout` bounding-box inspection built into the CLI.

## Key Features
- **Capability Registry & Version Gate**: `rustren capabilities` publishes renderer version (`1.1`), schema version (`1.1`), and supported features.
- **Declarative Layout & Visuals**: Flexbox (`taffy`), `container` (`clip`, `cascade_opacity`, `stagger_secs`), `rect`, `circle`, `text` (rich text `spans` with per-span `font_size`, `letter_spacing`, `outline`, `gradient`, RTL/BiDi `direction: "rtl"`, `dynamic` typewriter/counter/timecode), `waveform` visualizer, `lottie` animation, `svg`, `image` & `video` (`fit: "fill" | "cover" | "contain"`), `anchor`, `skew_x`/`skew_y`, linear/radial multi-stop `gradient`, plus `blur`, `shadow`, and `blend_mode`.
- **Composition Profiles**: Define multi-format variants (`profiles: { "portrait": { ... }, "square": { ... } }`) in one JSON and render via `rustren render scene.json --composition portrait`.
- **Timeline & Sequencing**: Global `start_sec`/`end_sec`, `duration_secs`, `timeline: "local"`, and `timeline: "series"` with `overlap_secs`.
- **Media & A/V Sync**: `video` layer with `trim_start_sec`, `trim_end_sec`, `playback_rate`, `volume`, and `muted` synced to FFmpeg; multi-track `audio` with `fade_in_secs`, `fade_out_secs`, and volume ducking keyframes.
- **Advanced Motion**: `keyframes[]` multi-stop timelines, mathematical per-frame `expr` ("val + sin(t*2.0)*50"), custom `spring` (`stiffness`, `damping`, `mass`), `repeat` & `yoyo`, `cubic_bezier` + Penner easings, color interpolation (`color`, `border_color`), and path/stroke animation (`path_progress`, `stroke_dashoffset`).
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

# Validate and lint a scene JSON before rendering (add --strict-determinism for CI)
rustren validate scene.json
rustren validate scene.json --strict-determinism

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

### Security & Resource Limits (`rustren serve`)
- **SSRF Protection** ([src/security.rs](src/security.rs)): Remote `http://`/`https://` URLs are DNS-resolved and validated before fetching, blocking loopback (`127.0.0.0/8`, `::1`), private (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`), and link-local/metadata (`169.254.0.0/16`, `fe80::/10`) IP ranges. Local filesystem `src` paths are **disabled by default** in `serve` mode (opt-in via `--allow-local-paths`).
- **Job & Server Limits**: Enforceable via CLI flags on `rustren serve`: `--max-duration-secs` (default `60.0`), `--max-width` (`1920`), `--max-height` (`1920`), `--max-pixels` (`1080*1920`), `--max-fps` (`60`), `--rate-limit` (`30` req/min/IP), and `--ttl-secs` (`3600`).

## SDK Types
Type definitions matching `schema.json` are included for AI pipelines:
- **TypeScript:** [sdk/typescript/rustren.d.ts](sdk/typescript/rustren.d.ts)
- **Python:** [sdk/python/rustren_types.py](sdk/python/rustren_types.py)

## Examples
Browse [examples/](examples) for full scenes you can validate or render directly, including:
- [examples/showreel.json](examples/showreel.json) — 17-second v1.1 showreel (`dynamic` typewriter & timecode, `skew_x`, `anchor`, `stagger_secs`) used to generate the preview sheet above
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
