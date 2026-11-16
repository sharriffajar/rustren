# rustren

Browserless 2D video render engine berbasis Rust dengan skema JSON deklaratif yang ramah AI.

## Fitur Utama
- **Capability Registry & Version Gate**: `rustren capabilities` mempublikasikan versi renderer (`1.1`), versi schema (`1.1`), dan fitur yang didukung.
- **Declarative Layout & Visuals**: Flexbox (`taffy`), `container` (`clip`, `cascade_opacity`), `rect`, `circle`, `text` (rich text `spans`, `letter_spacing`), `svg`, `image`, serta efek `blur`, `shadow`, dan `blend_mode`.
- **Timeline & Sequencing**: Global `start_sec`/`end_sec`, `duration_secs`, `timeline: "local"`, dan `timeline: "series"` dengan `overlap_secs`.
- **Media & A/V Sync**: Layer `video` dengan `trim_start_sec`, `trim_end_sec`, `playback_rate`, `volume`, dan `muted` yang tersinkronisasi ke FFmpeg.
- **Advanced Motion**: Custom `spring` (`stiffness`, `damping`, `mass`), `repeat` & `yoyo`, `cubic_bezier` + penner easings, interpolasi warna (`color`, `border_color`), serta animasi path/stroke (`path_progress`, `stroke_dashoffset`).

## Perintah CLI
```bash
# Cek kapabilitas renderer (JSON)
rustren capabilities

# Validasi scene JSON sebelum render
rustren validate scene.json

# Render scene JSON ke MP4 (default: --crf 18 --preset medium)
rustren render scene.json -o output.mp4

# Render dengan opsi encoding kustom
rustren render scene.json -o output.mp4 --crf 20 --preset fast

# Jalankan HTTP API server
rustren serve --port 8080
```

## Dokumentasi & Pengujian
- Panduan lengkap skema: [SCHEMA.md](SCHEMA.md) & [schema.json](schema.json)
- Jalankan conformance & unit tests:
```bash
cargo test --release
```
