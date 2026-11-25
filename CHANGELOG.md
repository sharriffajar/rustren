# Changelog

All notable changes to `rustren` are documented in this file.
This project adheres to [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/).

## [0.5.1] - 2026-11-25

### Added
- **Longitudinal `wave_warp` Axes (`stretch_x` / `stretch_y`)**: Rubber-band stretched typography where vertical/horizontal strokes remain sharp without sinusoidal bending.
- **Full `font_weight` Range (`100`..`900` & `thin`..`black`)**: Supports numeric weights (`100`–`900`) and named weights (`thin`, `extralight`, `light`, `normal`, `medium`, `semibold`, `bold`, `extrabold`, `black`) in JSON and HTML scenes.

## [0.5.0] - 2026-11-24

### Added
- **Non-Uniform Scaling & Custom Pivot**: Independent `scale_x` / `scale_y` (plus `AnimProperty::ScaleX` / `ScaleY`) and `pivot_x` / `pivot_y` (`transform_origin`) on layers.
- **Built-in `text_animator`**: Per-character (`char`) and per-word (`word`) staggered text animations (`start_sec`, `stagger_secs`, `duration_secs`, `x_offset`, `y_offset`, `opacity_from`, `scale_from`, `easing`).
- **Feathered Container Clip (`clip_feather`)**: Soft alpha-feathered container clipping for rack-focus, soft vignettes, and half-screen scrims in pure JSON/HTML.
- **Built-in `wave_warp` Effect**: Deterministic pixel displacement (`amplitude`, `wavelength`, `speed`, `phase`, `axis`) for elastic / stretched-type / liquid motion.
- **Advanced `video` Layer Controls & Anti-OOM Windowed Decoder**: Bounded LRU windowed video decoding for long clips (1–3+ min) without OOM, plus `playback_rate` (negative for reverse), `loop`, animatable `focal_x` / `focal_y` (dynamic 9:16 reframing), and `luma_key` (black-background transparency).
- **Zero-Encode HTTP API Preview & AI Discovery**: Added `GET /` and `GET /llms.txt` (with `RUSTREN_PUBLIC_BASE_URL` and `RUSTREN_API_NOTICE`), `POST /snapshot` (single PNG or contact sheet), `POST /layout` (bounding boxes + diagnostics), and `http_api` contracts in `GET /capabilities`.
- **Installer Agent Skill Auto-Install**: `scripts/install.sh` and `scripts/install.ps1` automatically install `SKILL.md` into `~/.claude/skills/rustren/SKILL.md`.

### Fixed
- **Transparent PNG Premultiply Alpha**: `premultiply_rgba` on image load eliminates white halo bleed from `(255, 255, 255, 0)` pixels during resize/compositing.
- **Multiline Text `line_height` & `anchor: "center"`**: Consistent `line_height` on `\n` multiline text and accurate `anchor: "center"` / `text_align: "center"` without requiring `max_width`.
- **`track_matte` inside 2.5D Containers & `waveform` Color Animation**: `matte_layer` now inherits container `layout_override`; `LayerKind::Waveform` now evaluates `AnimProperty::Color`.
- **Linter & Layout Diagnostics**: Parent container `background` is now used for text contrast checks (eliminating false positives on dark pills over light canvases), and stacked text layers have a small vertical line-height overlap tolerance.

## [0.4.1] - 2026-11-22

### Added
- **MCP Actuation & Sensors**: `rustren mcp <scene>` scene actions (`add_layer`, `remove_layer`, `add_animation`) now persist atomically to disk, and new tools `validate_scene`, `snapshot_scene`, `inspect_layout`, `diff_scenes` and `render_video` let an agent author, measure, diff and render without leaving the protocol. MCP is available in every build (no `pro` feature gate).
- **Composition Variables in the Compiler**: `data-composition-variables` defaults are applied when compiling HTML, so CLI `validate`/`snapshot`/`render` match Studio behavior.

### Changed
- **Unified Asset Resolution**: `validate --check-assets`, `snapshot`, `layout`, `diff` and `render` now resolve relative asset paths identically (scene file directory first, then the working directory).

### Fixed
- Silently-dropped HTML `snake_case` attributes (e.g. `font_size=`) now raise a `snake_case_attribute` warning with line/column, and `rustren validate --fix` renames them to kebab-case.
- `validate --fix` no longer skips attributes after a closing `</svg>` tag.

## [0.4.0] - 2026-11-22

### Added
- **Auto-Repair Scenes (`rustren validate --fix` / `--in-place`)**: Automatically fixes common AI-generated mistakes in both JSON and HTML scenes (stripping numeric units like `px`, expanding shorthand hex `#RGB` colors, normalizing named colors, moving invalid `position` values into `anchor`).
- **Structured Agent Diagnostics (`--error-format json`)**: Machine-readable JSON diagnostics with exact source line, column, and field paths across `validate`, `render`, `snapshot`, `layout`, and `diff`.
- **Built-in Examples Explorer (`rustren examples`)**: List and inspect bundled showcase scenes directly from the CLI.
- **SDK Parity Generator**: `scripts/gen_sdk.py` and `tests/sdk_parity.rs` keep `schema.json`, `sdk/python/rustren_types.py`, and `sdk/typescript/rustren.d.ts` synchronized.
- **1-Liner Installers**: `scripts/install.sh` (Linux/macOS) and `scripts/install.ps1` (Windows) for zero-Rust prebuilt binary installation.
- **Experimental CPU 3D (`--features exp-3d`)**: Opt-in software 3D rasterizer (`<mesh3d>`) with procedural primitives (`cube`, `sphere`, `pyramid`, `torus`), `.glb`/`.gltf` model loading, UV texture mapping, anti-aliasing, and Blinn-Phong specular/rim lighting.
- **Commercial License Guide**: Added [COMMERCIAL-LICENSE.md](COMMERCIAL-LICENSE.md).

### Changed
- **Dual License**: Switched from `Apache-2.0` to dual `AGPL-3.0-only` / Commercial License (`v0.1.0`–`v0.3.1` remain `Apache-2.0`).
- **Full Engine Enabled by Default**: `default = ["pro"]` in `Cargo.toml` — `cargo install rustren` now includes Planar YUV420p, temporal frame dedup, buffer pooling, `serve`, `mcp`, and `studio` out of the box.
- **Linux Release Compatibility**: Release binaries are built on `ubuntu-22.04` (glibc 2.35) for broad Linux compatibility.

## [0.3.1] - 2026-11-19

### Added
- **SIMD Image Scaling (`fast_image_resize`)**: AVX2/SSE4.1/NEON fast-path for axis-aligned `Fill`, `Contain`, and `Cover` image/video layers.
- **Automatic GPU Hardware Encoder Probe**: Safe 1-frame dry-run probe (`h264_nvenc`, `h264_videotoolbox`, `h264_qsv`) with automatic fallback to `libx264` (`RUSTREN_HW_ENCODER=off` forces CPU).
- **Bundled Allo Font Family**: 18 static weights/obliques (SIL OFL 1.1) embedded in `FontSystem`.
- **AI Agent Skill (`SKILL.md`)**: Versioned agent workflow guide with CI drift guard (`test_skill_md_versions_match_code`).

### Fixed
- Constant-time API key comparison (`subtle::ConstantTimeEq`) and bounded `rate_limits` sweeper in `rustren serve`.
- Poison-safe mutex/rwlock recovery across renderer caches and tightened shadow/warp cache keys.
- Release profile switched to `lto = "thin"`, `codegen-units = 1`, `strip = true`.

## [0.3.0] - 2026-11-18

### Added
- **Web Studio UI (`rustren studio [file]` / `rustren preview <file>`)**: Auto-opens browser, drag-and-drop `.json`/`.html`, direct canvas manipulation, clickable diamond keyframes, and background MP4 export.
- **Audio Beat Detection (`rustren beats <audio.mp3>`)**: Extracts beat timestamps as JSON for beat-sync automation.
- **MCP Server (`rustren mcp <scene.json>`)**: Stdio JSON-RPC 2.0 server exposing `get_scene_summary`, `add_layer`, `remove_layer`, `add_animation`, and `detect_beats`.
- **2.5D & Surface Primitives**: `rotate_x`, `rotate_y`, `perspective`, `z_index`, backface culling, `echo`, `grain`, `vignette`, `halftone`, `particles`, and `track_matte`.
