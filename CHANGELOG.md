# Changelog

All notable changes to `rustren` are documented in this file.
This project adheres to [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/).

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
