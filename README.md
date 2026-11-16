# rustren

Browserless 2D video render engine in Rust driven by an AI-friendly declarative JSON schema.

## Key Features
- **Capability Registry & Version Gate**: `rustren capabilities` publishes renderer version (`1.1`), schema version (`1.1`), and supported features.
- **Declarative Layout & Visuals**: Flexbox (`taffy`), `container` (`clip`, `cascade_opacity`, `stagger_secs`), `rect`, `circle`, `text` (rich text `spans`, `letter_spacing`, `dynamic` typewriter/counter/timecode), `svg`, `image`, `anchor`, `skew_x`/`skew_y`, plus `blur`, `shadow`, and `blend_mode`.
- **Timeline & Sequencing**: Global `start_sec`/`end_sec`, `duration_secs`, `timeline: "local"`, and `timeline: "series"` with `overlap_secs`.
- **Media & A/V Sync**: `video` layer with `trim_start_sec`, `trim_end_sec`, `playback_rate`, `volume`, and `muted` synced to FFmpeg.
- **Advanced Motion**: Custom `spring` (`stiffness`, `damping`, `mass`), `repeat` & `yoyo`, `cubic_bezier` + Penner easings, color interpolation (`color`, `border_color`), and path/stroke animation (`path_progress`, `stroke_dashoffset`).
- **AI Authoring & Debugging Tools**: Instant single-frame / contact-sheet `snapshot`, computed bounding-box `layout` inspection, smart `validate` linting, and `--profile` performance breakdown.

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

## Documentation & Testing
- Full schema reference: [SCHEMA.md](SCHEMA.md) & [schema.json](schema.json)
- Run unit & conformance tests:
```bash
cargo test --release
```
