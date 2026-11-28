<p align="center">
  <img src="assets/logo.png" alt="Rustren Logo" width="240" />
</p>

# rustren

Browserless 2.5D video render engine in Rust — built to be **fast** and **predictable for AI-generated video**.

> **Try Online (Zero Install):**
> - **Web Studio (PWA, Bring-Your-Own-Key (BYOK) AI Director + Browser WebM/PNG + MP4 via API):** https://nvm.web.id/rustren/studio/
> - **Hosted Render API (Experimental, AI-discoverable):** https://nvm.web.id/rustren/api/ ([`llms.txt`](https://nvm.web.id/rustren/llms.txt) · [Request API token](https://nvm.web.id/contact/))
> - **Public Asset Catalog (44 Free Assets: Backdrops, Overlays, Video Loops & CC0 Audio):** https://nvm.web.id/rustren/studio/assets/catalog/catalog.json
> - **Project Homepage:** https://nvm.web.id/rustren/

| Start Here | Best For | Entrypoint |
| :--- | :--- | :--- |
| **Visual Browser Editor** | Interactive editing, BYOK AI Director, Frame Notes, 44 free assets | [Rustren Studio](https://nvm.web.id/rustren/studio/) |
| **CLI (`rustren`)** | Local 150+ fps MP4 rendering, CI/CD, batch video, `rustren qa` | `cargo install rustren` |
| **AI Agent / MCP** | Claude Code, Cursor, Antigravity (pair with LottieFiles Creator MCP) | `rustren mcp` + [SKILL.md](SKILL.md) |
| **HTTP Render API** | Serverless / programmatic cloud MP4 & `/snapshot` rendering | `rustren serve` / [`llms.txt`](https://nvm.web.id/rustren/llms.txt) |

```
AI generates (JSON / HTML) → Compiler parses to IR → Schema validates → Previews (snapshot / layout) → Renderer executes → MP4 out
```

`rustren` compiles declarative **JSON** or **HTML** scenes into MP4 videos or PNG frame snapshots without spawning a headless browser, DOM, or Chromium engine. Pre-encode raster frames are deterministic and inspectable at every stage. See [SCHEMA.md](SCHEMA.md) for the full scene reference and [SKILL.md](SKILL.md) for the AI agent playbook.

## Teaser — Rustren Showreel (60s, 30fps, 16 Scenes)
**100% Rust, no browser. `JSON / HTML in → MP4 out.`**

- **Source JSON:** [examples/showreel.json](examples/showreel.json) (`93` root layers, `16` scenes including 2.5D spatial cards, isometric bento, card flips, surface shaders, and finale)
- **Output MP4:** [assets/showreel.mp4](assets/showreel.mp4) (`1280×720 @ 30fps`, `60.0s` / `1,800` frames, default `--crf 18 --preset medium`)
- **Hardware & Provenance:** `8 vCPU` Intel Xeon E5-2630 v4 @ 2.20GHz, `15 GiB` RAM (CPU-only, `libx264` via `ffmpeg`, measured on `rustren v0.4.0`, Nov 2026)

| Metric | Measured (`--preset medium --crf 18`) |
| :--- | :--- |
| **Render Time (Wall Clock)** | `11.71s` *(or `8.09s` on `veryfast`, `6.59s` on `ultrafast`)* |
| **Average Speed** | `153.7 fps` (`5.12×` real-time) |
| **MP4 File Size** | `2.2 MB` |
| **Duration / Frame** | `6.51 ms / frame` |
| **Frame Dedup Cache Hit** | `66.7%` (`1,200 hits / 600 misses`) |

<details>
<summary><strong>Preset Scaling Curve (<code>--crf 18</code>, 60s / 1,800 frames)</strong></summary>

<p align="center">
  <strong>Render time by preset</strong> (seconds, lower = faster)
</p>

```mermaid
xychart-beta
    title "Render Time by Preset (seconds) — 60s 720p30 @ CRF 18"
    x-axis [ultrafast, superfast, veryfast, faster, fast, medium]
    y-axis "Render time (s)" 0 --> 13
    line [6.59, 6.79, 8.09, 9.59, 10.27, 11.71]
```

<p align="center">
  <strong>MP4 file size by preset</strong> (MB, lower = smaller)
</p>

```mermaid
xychart-beta
    title "MP4 File Size by Preset (MB) — 60s 720p30 @ CRF 18"
    x-axis [ultrafast, superfast, veryfast, faster, fast, medium]
    y-axis "File size (MB)" 0 --> 9
    line [7.8, 3.4, 1.9, 2.0, 2.1, 2.2]
```

> Measured on `examples/showreel.json`, one run per preset, so differences of a few percent are within run-to-run noise. Read the two charts together: `veryfast` is the best trade-off here — it renders faster than `faster`/`fast`/`medium` **and** produces the smallest file (`1.9 MB`), while `ultrafast`/`superfast` render slightly faster but at a much larger file size.

| Preset | Render Time | Average Speed | MP4 File Size |
| :--- | :--- | :--- | :--- |
| `ultrafast` | `6.59s` | `273.1 fps` (`9.10×`) | `7.8 MB` |
| `superfast` | `6.79s` | `265.1 fps` (`8.84×`) | `3.4 MB` |
| `veryfast` | `8.09s` | `222.5 fps` (`7.42×`) | `1.9 MB` |
| `faster` | `9.59s` | `187.7 fps` (`6.26×`) | `2.0 MB` |
| `fast` | `10.27s` | `175.3 fps` (`5.84×`) | `2.1 MB` |
| `medium` *(default)* | `11.71s` | `153.7 fps` (`5.12×`) | `2.2 MB` |

</details>

[Watch 60s showreel video](https://github.com/user-attachments/assets/7089ed1a-c188-4d1b-9f3a-ac08d2ca9e64)

> Full 60s `1280×720 @ 30fps` MP4: [assets/showreel.mp4](assets/showreel.mp4)

## Zero-Encode Preview (`snapshot` & `layout`) — AI One-Shot Loop
Unlike traditional video pipelines that force you to encode a full MP4 just to check alignment, `rustren` lets AI agents and developers inspect any timestamp in milliseconds:

![Rustren contact sheet preview](assets/showreel_sheet.png)

```bash
# Render 16 keyframes across the 60s showreel into one 4x4 contact sheet (~0.95s, no FFmpeg)
rustren snapshot examples/showreel.json -t 1.5,3.3,5.5,7.0,8.8,11.0,14.5,18.5,23.5,28.0,32.5,37.5,42.0,47.0,51.5,57.5 --sheet --cols 4 -o assets/showreel_sheet.png

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
- **Deterministic Raster Frames & Inspectable Layout**: The same scene produces identical pre-encode RGBA frames across runs. *(MP4 byte output depends on the H.264 encoder, preset, and CRF; set `RUSTREN_HW_ENCODER=off` with pinned `--preset` and `--crf` for cross-machine reproducibility.)*

### Alternatives Considered
| Approach | Trade-offs vs `rustren` |
| --- | --- |
| **Headless Chromium / Playwright** | Full web CSS/JS fidelity, but heavy (~300 MB+ browser binary), slow cold starts, high RAM per worker, and harder to sandbox on public VPSs. |
| **Remotion (React + Chromium)** | Mature React ecosystem, but requires Node.js + Chromium runtime and executes arbitrary JS rather than a validated declarative schema. |
| **Raw `ffmpeg` filter_complex** | Zero browser overhead, but fragile for complex Flexbox layouts, rich typography, 2.5D transforms, and AI-authored iterative editing. |

### Non-Goals
`rustren` is a declarative video compiler, not a web browser: HTML input is a **declarative tag subset** that lowers to the same IR as JSON (no JavaScript execution, no external CSS stylesheets, no DOM runtime, and no browser CSS/layout parity).

## Multi-Format Input (JSON & HTML)
`rustren` compiles both declarative formats to one shared Intermediate Representation (IR) — JSON is the canonical form, and conformance tests guard HTML parity:
- **JSON (`.json`)**: Compact declarative scene schema for AI generation and programmatic pipelines.
- **HTML (`.html`, `.htm`)**: Declarative tag subset (`<scene>`, `<container>`, `<rect>`, `<circle>`, `<text>`, `<animate>`, `<profiles>`).

```bash
# Render from JSON or declarative HTML:
rustren render examples/showreel.json -o showreel.mp4
rustren render examples/cuboid_carousel.html -o cuboid.mp4
```

## Key Features
*(Full field-level reference in [SCHEMA.md](SCHEMA.md) and machine-readable [schema.json](schema.json).)*
- **Capability Registry & Version Gate**: `rustren capabilities` publishes renderer version (`1.2`), schema version (`1.2`), supported features, and HTTP API contracts.
- **Layout, 2.5D & Typography**: Flexbox (`taffy`), 2.5D perspective & `track_matte`, non-uniform `scale_x`/`scale_y` & custom `pivot`, rich text spans & RTL/BiDi, built-in per-char/word `text_animator`, dynamic text (typewriter/counter/timecode), `clip_feather`, `wave_warp` (transverse & longitudinal `stretch_x`/`stretch_y`), gradients, blur, shadow, and blend modes.
- **Composition Profiles & Timelines**: Multi-aspect-ratio `profiles` (`--composition <name>`) and global, local, or `series` timelines with overlap.
- **Anti-OOM Media & A/V Sync**: Windowed LRU `video` decoder for long clips (`playback_rate` including reverse, `loop`, animatable `focal_x`/`focal_y`, `luma_key`, trim/volume) and multi-track `audio` with fades and ducking.
- **Advanced Motion**: Multi-stop `keyframes[]`, precompiled math `expr`, custom `spring` physics, `cubic_bezier` + Penner easings, color interpolation, and SVG path/stroke animation.
- **AI Authoring & Debugging**: Zero-encode `snapshot` & `layout` diagnostics (CLI, MCP, and HTTP API), `validate` linting (`--strict-determinism`, `--fix`, `--error-format json`), `--profile` timing breakdown, and `SKILL.md` auto-installed to `~/.claude/skills/rustren/SKILL.md` by the 1-liner installers.

## Prerequisites & Installation
- **`ffmpeg`** (and `ffprobe` for `video` layers) available in `PATH`
- **Automatic GPU Hardware Encoding**: Probes for a working GPU H.264 encoder (`h264_nvenc`, `h264_videotoolbox`, `h264_qsv`) by default and falls back cleanly to `libx264` CPU encoding when unavailable. Override with `RUSTREN_HW_ENCODER=off` (force CPU) or an explicit encoder name.

### Quick Install (Prebuilt Binary — No Rust Required)
Always installs the latest GitHub Release automatically:

**Linux (`x86_64`) / macOS (`Apple Silicon`) / Android Termux (`aarch64`):**
```bash
curl -fsSL https://raw.githubusercontent.com/sharriffajar/rustren/main/scripts/install.sh | sh
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/sharriffajar/rustren/main/scripts/install.ps1 | iex
```

### Install via Cargo (`crates.io`)
```bash
cargo install rustren
```

### Build from Source
```bash
git clone https://github.com/sharriffajar/rustren.git
cd rustren
cargo build --release
# Binary will be at ./target/release/rustren
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

# Validate, auto-repair, and lint a scene before rendering
rustren validate scene.json --strict-determinism --error-format json
rustren validate scene.json --fix --in-place

# Render a single frame or contact sheet to PNG (no FFmpeg encode)
rustren snapshot scene.json -t 1.5,5.6,14.5 --sheet -o sheet.png

# Inspect computed layer bounding boxes & layout diagnostics
rustren layout scene.json -t 5.0

# Render scene to MP4 (supports --composition <profile>, --crf, --preset, --profile)
rustren render scene.json -o output.mp4 --composition portrait --crf 20 --preset fast --profile

# Explore bundled examples, detect audio beats, watch/diff, or launch local Studio/Preview
rustren examples
rustren beats track.mp3
rustren studio scene.json
rustren preview scene.json

# Start the HTTP render API server or MCP stdio server
rustren serve --port 8080
rustren mcp scene.json
```

## MCP Server for AI Agents (`rustren mcp`)
`rustren mcp scene.json` speaks JSON-RPC 2.0 over stdio, so an agent can close the
author → measure → repair loop without shelling out. Available in every build
(no `pro` feature gate).

| Tool | Purpose |
|---|---|
| `get_scene_summary`, `add_layer`, `remove_layer`, `add_animation` | Inspect/edit; edits are written back to disk atomically |
| `validate_scene` | Compile + lint + asset check, returns JSON diagnostics |
| `snapshot_scene` | Render `t`/`times` to PNG, optional `sheet` |
| `inspect_layout` | Computed boxes + measured text, optional `diagnose` |
| `diff_scenes` | Visual regression against another scene at `t` |
| `render_video` | Render the scene to MP4 (`crf`, `preset`, `composition`) |
| `detect_beats` | Audio beat timestamps for beat-sync |

## HTTP Render API (`rustren serve`)
Single-node, in-memory async job queue server (no external DB/persistence; horizontal scaling requires an external queue). Authenticate protected endpoints via `Authorization: Bearer <API_KEY>` or `x-api-key: <API_KEY>` when `RUSTREN_API_KEY` is set:
```bash
RUSTREN_API_KEY=your-secret rustren serve --host 0.0.0.0 --port 8080 --workers 2
```

| Method | Path | Auth | Description |
| --- | --- | --- | --- |
| `GET` | `/` or `/llms.txt` | Public | AI-readable Markdown API discovery guide |
| `GET` | `/health` | Public | Health check, worker count, active jobs |
| `GET` | `/capabilities` | Public | Renderer capability registry & HTTP API contracts |
| `GET` | `/schema` | Public | JSON Schema (`schema.json`) |
| `GET` | `/guide` | Public | Scene authoring guide (`SCHEMA.md`) |
| `POST` | `/validate` | API Key | Validate & lint a scene JSON payload |
| `POST` | `/layout` | API Key | Computed bounding boxes & overlap/clip diagnostics (`{"scene", "time"?}`) |
| `POST` | `/snapshot` | API Key | Zero-encode PNG frame or contact sheet (`{"scene", "times"?, "sheet"?, "cols"?}`) |
| `POST` | `/jobs` | API Key | Submit an async MP4 render job (`202 Accepted`) |
| `GET` | `/jobs/:id` | API Key | Poll job status (`queued`, `processing`, `completed`, `failed`) |
| `GET` | `/jobs/:id/video` | API Key | Download rendered `video/mp4` |

### Security & Resource Limits (`rustren serve`)
- **SSRF Protection** ([src/security.rs](src/security.rs)): Remote `http://`/`https://` URLs are DNS-resolved and validated before fetching, blocking non-public IP ranges including loopback (`127.0.0.0/8`, `::1`), private (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`), CGNAT (`100.64.0.0/10`), link-local/metadata (`169.254.0.0/16`, `fe80::/10`), multicast, broadcast, and unspecified addresses. Local filesystem `src` paths are **disabled by default** in `serve` mode (opt-in via `--allow-local-paths`).
- **Job & Server Limits**: Enforceable via CLI flags on `rustren serve`: `--max-duration-secs` (default `60.0`), `--max-width` (`1920`), `--max-height` (`1920`), `--max-pixels` (`1080*1920`), `--max-fps` (`60`), `--rate-limit` (`30` req/min/IP), and `--ttl-secs` (`3600`).

## SDK Types
Type definitions matching `schema.json` are included for AI pipelines:
- **TypeScript:** [sdk/typescript/rustren.d.ts](sdk/typescript/rustren.d.ts)
- **Python:** [sdk/python/rustren_types.py](sdk/python/rustren_types.py)

Both files are generated from the schema. After editing `schema.json` run:

```bash
python3 scripts/gen_sdk.py          # rewrite both SDK files
python3 scripts/gen_sdk.py --check  # exit 1 when they are stale
```

`cargo test` enforces the same parity (`tests/sdk_parity.rs`), so a stale SDK fails the build
even without Python.

## Examples
Browse [examples/](examples) for full scenes you can validate or render directly, including:
- [examples/showreel.json](examples/showreel.json) — 60-second, 16-scene v1.1 showreel (2.5D perspective, surface shaders, `dynamic` typewriter & timecode, `skew_x`, `anchor`, `stagger_secs`)
- [examples/showreel.html](examples/showreel.html) — declarative HTML showreel scene
- [examples/cuboid_carousel.html](examples/cuboid_carousel.html) — 200-frame 1080p declarative HTML card carousel
- [examples/rustren_showcase_15s.json](examples/rustren_showcase_15s.json) — 15-second multi-scene showcase
- [examples/benchmark_scene.json](examples/benchmark_scene.json) — animation benchmark scene
- [examples/demo.json](examples/demo.json) — minimal starter scene

## Documentation & Testing
- Full schema reference: [SCHEMA.md](SCHEMA.md) & [schema.json](schema.json)
- AI agent skill (workflow, rules, recipes): [SKILL.md](SKILL.md)
- Run unit & conformance tests:
```bash
cargo test --release
```

## License
`rustren` is **dual-licensed**:

- **Open Source — [GNU Affero General Public License v3.0 (`AGPL-3.0-only`)](LICENSE)**: Free to use, modify, and self-host for personal projects, research, and AGPL-compatible open-source software. If you run a modified or integrated version of `rustren` as a network service / SaaS, you must make the corresponding source code available under AGPL-3.0.
- **Commercial License (Whitelist)**: For companies, agencies, or SaaS products that want to embed or operate `rustren` in proprietary / closed-source commercial workflows without AGPL-3.0 obligations, see [COMMERCIAL-LICENSE.md](COMMERCIAL-LICENSE.md).

*(Note: Previous releases `v0.1.0`–`v0.3.1` remain under their original Apache-2.0 license.)*

## Changelog
See [CHANGELOG.md](CHANGELOG.md) for the full release history.
