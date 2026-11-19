---
name: rustren
description: Render MP4 videos from declarative HTML or JSON scenes with the browserless `rustren` CLI (Rust, ffmpeg). Use when the user wants to generate, animate, preview, or debug a video (promo, explainer, captions, charts, motion comics from still images, vertical TikTok/Reels, lower thirds). Covers the author -> validate -> snapshot -> render loop. Don't use for 3D, live streaming, or editing existing video timelines frame-by-frame in a GUI.
metadata:
  skill_version: "1.0.0"
  rustren_version: "0.3.0"
  schema_version: "1.1"
  renderer_version: "1.1"
---

# rustren — AI Agent Skill

`rustren` turns a text scene file into an MP4. No browser, no Chromium. You write the scene, check it cheaply, then render once.

Full field reference: [SCHEMA.md](SCHEMA.md) (human-readable) and [schema.json](schema.json) (machine-readable JSON Schema).
Working examples: [examples/](examples/).

## 1. Prerequisites (check first)

```bash
rustren --version          # binary present?
ffmpeg -version            # required; ffprobe also needed for `video` layers
rustren capabilities       # JSON: supported elements, easings, animation properties, schema version
```

If `rustren` is not on PATH, build it: `cargo build --release` (binary at `./target/release/rustren`).
Always run `rustren capabilities` before using a feature you are unsure about. It is the source of truth.

### Version check (do this once per session)

This skill was written for the versions in the frontmatter (`metadata:` block at the top of this file): `rustren_version`, `schema_version`, `renderer_version`.

```bash
rustren --version                                   # compare with metadata.rustren_version
rustren capabilities | grep -E '"(renderer|schema)_version"'   # compare with metadata.*_version
```

- **Same versions**: trust this file.
- **Installed rustren is newer** (higher `schema_version` or `renderer_version`): this file may be missing features or describe old behavior. Prefer `rustren capabilities`, [schema.json](schema.json), and `rustren validate` output over this file when they disagree, and tell the user the skill looks outdated.
- **Installed rustren is older** (lower than the frontmatter): features described here may not exist. Rely on `rustren capabilities` and tell the user to upgrade.
- If a command or flag in this file errors with "unexpected argument", run `rustren <command> --help` and follow that instead.

Versioning scheme of this file (`skill_version`): bump the patch for wording fixes, the minor for new recipes or sections, the major when the workflow or commands change incompatibly. Changes are recorded in the changelog at the bottom.

## 2. The golden loop (always follow this order)

Never go straight to `render`. A render can take many seconds; `validate` and `snapshot` take milliseconds.

```bash
# 1. Lint: schema errors, unknown fields, low contrast, tiny fonts, out-of-bounds
rustren validate scene.html

# 2. Look at key moments as PNG (cheap). Use a contact sheet for several timestamps.
rustren snapshot scene.html --t 0.5,3,6 --sheet -o sheet.png

# 3. If something looks wrong, get exact geometry instead of guessing
rustren layout scene.html --t 3 --diagnose

# 4. Only now render
rustren render scene.html -o out.mp4 --profile
```

Rules for the loop:
- `validate` exit code `0` means valid. Warnings do not fail it. Read them anyway and fix those that affect the result (see section 6).
- `snapshot` requires `--t` (comma-separated seconds). Without `--sheet` and with several timestamps, `-o` is treated as a directory/prefix.
- Look at the PNG (open the image) after every meaningful change. Do not claim a scene "looks right" without viewing a snapshot.
- Snapshot the **climax / transition moments**, not only `t=0`. Animations that start later are invisible at `t=0`.

## 3. Commands cheat sheet

| Command | Purpose | Key flags |
|---|---|---|
| `validate <file>` | Lint without rendering | `--strict-determinism`, `--composition <name>` |
| `snapshot <file>` | PNG of one or more timestamps | `--t 1.5,5` (required), `--sheet`, `--cols N`, `-o` |
| `layout <file>` | Computed boxes + text sizes as JSON | `--t <sec>`, `--diagnose` (overlap/clip/overflow) |
| `render <file>` | Produce MP4 | `-o`, `--crf` (default 18), `--preset` (default medium), `--profile`, `--composition` |
| `capabilities` | Supported features as JSON | none |
| `watch <file>` | Re-render the active frame to PNG on every save | |
| `diff <a> <b>` | Visual regression between two scenes at time `t` | |
| `beats <audio>` | Detect beat timestamps in seconds | use to sync cuts to music |
| `serve` / `mcp` / `studio` / `preview` | HTTP API, MCP stdio server, browser studio | see README |

Responsive output: define `<profiles>` in the scene (e.g. `portrait` 1080x1920, `square` 720x720) and pass `--composition portrait` to `validate`, `snapshot`, `layout`, `render`.

Faster renders: `--preset veryfast` (or `ultrafast`) trades file size for speed. Set `RUSTREN_HW_ENCODER=auto` to try a GPU H.264 encoder (`h264_nvenc`, `h264_videotoolbox`, `h264_qsv`); it falls back to `libx264` if none is found. Unset means CPU `libx264`.

## 4. Minimal scene (HTML)

The root tag is `<scene>`, **not** `<html>`. Layout uses attributes (`x`, `y`, `width`, `layout`, `padding`, `gap`...), **not** CSS `style="..."`.

```html
<scene width="1280" height="720" fps="30" duration="5.0" background="#0F172A">
  <container x="200" y="240" width="880" height="240" radius="16"
             background="#1E293B" layout="column" padding="24" gap="12">
    <text font-size="40" color="#FFFFFF" font-weight="bold" max-width="832">
      Hello from rustren
      <animate property="opacity" from="0" to="1" start="0.2" end="0.8" easing="ease_out" />
    </text>
  </container>
</scene>
```

Supported tags: `scene`, `profiles`, `profile`, `fonts`, `font`, `audio-tracks`, `audio`, `container`, `rect`, `circle`, `text`, `image`, `svg`, `video`, `waveform`, `lottie`, `animate`.

`<animate>` can be a child of the layer it animates, or a sibling that names it with `target="<id>"` (give the layer an `id`).

## 5. Rules that prevent most failures

1. **Do not invent fields or tags.** Unknown fields are rejected by `validate`. If unsure, check [SCHEMA.md](SCHEMA.md) or `rustren capabilities`.
2. **Z-order is document order**: first element is at the bottom, last is on top.
3. **Text in a container needs a max width** (`max-width` in HTML, `max_width` in JSON) so wrapping and height estimation are correct.
   - Naming: JSON fields are `snake_case`. In HTML prefer `kebab-case` attributes (`font-size`, `max-width`, `cascade-opacity`, `letter-spacing`); copy the spelling used in [examples/](examples/).
4. **Layering inside a `container`**: flex layout stacks children. For overlays (image + banner + effect lines on the same area) set `position="absolute"` with `x`/`y` on each overlapping child. Inside a container, absolute `x`/`y` are relative to that container, not the canvas.
5. **Animating `width`/`height` inside a container** (progress bars): the rect needs `position="absolute"`, `x=0`, `y=0` or flex layout overrides it.
6. **Time**: animation `start`/`end` are in seconds on the global clock unless the container has `timeline="local"` or `"series"`. Layer `end` is inclusive; for a hard cut use `next_start - 1/fps`.
7. **Fade whole groups**: container `opacity` does not cascade by default. Set `cascade-opacity="true"` (HTML) / `"cascade_opacity": true` (JSON) on the container (and nested containers) to fade a whole scene together.
8. **Missing assets fail the render** with a non-zero exit code (`image`, `video`, `svg`, `audio`, `fonts`). Prefer local paths. Remote URLs are fetched with SSRF protection and are flagged by `validate --strict-determinism`.
9. **Cache-friendly authoring** (faster renders): see the "Cache-aware authoring" section below for the full checklist. Short version: static backgrounds first, animate `opacity`/`x`/`y`/`rotation`, reuse identical `src` strings, and check `render --profile`.
10. **Safe sizes for 9:16 (1080x1920)**: keep body text >= 28px and keep key content inside the safe area. `validate` warns on font_size < 14.

## 6. Reading `validate` output

Typical warnings and the fix:

| Warning | Fix |
|---|---|
| `font_size < 14px` | Raise font size (unless it is decorative). |
| `text inside container missing max_width` | Add `max-width` (HTML) / `max_width` (JSON) equal to container width minus padding. |
| low contrast | Change text/background colors. |
| out-of-canvas bounds | Run `layout --diagnose`, then move or resize. |
| remote URL / empty `fonts` / `repeat=infinite` (only with `--strict-determinism`) | Use local assets, bundle fonts, use finite repeats. |

## 7. Recipe: dramatize existing still images (motion comic / slideshow / Ken Burns)

Goal: make still pages or panels feel cinematic without redrawing them.

Structure per beat (repeat for each image):
- A `container` the size of the canvas with `clip="true"` (acts as the "window").
- An `image` inside with `fit="cover"`, larger than the window, absolutely positioned.
- `animate` its `scale` and `y`/`x` to pan and zoom. Use `easing="ease_out"` for settling moves, `easing="spring"` for punchy impacts.
- Fade the window's `opacity` in/out to cross-fade between beats.

Drama toolkit (check property and easing names against `rustren capabilities`):
- **Camera shake**: several short `animate` blocks on the window's `x` (for example `0 -> -14 -> 12 -> -8 -> 0`, ~0.08s each). Add `rotation` for extra violence.
- **Impact flash**: a full-canvas white `container` whose `opacity` jumps `0 -> 1` in ~0.04s then eases back to `0` over ~0.3s.
- **Speed lines**: an `svg` with radial `<path>` strokes, fade its `opacity` in on the beat.
- **Banner text**: a semi-opaque dark `container` strip with large bold `text`, `scale` from 1.4 to 1.0 with `spring`.
- **Letterbox**: two thin dark `rect`s at top and bottom, drawn last, with subtitles inside the bottom bar.
- **Beat drop**: a short black beat with a single line of centered text before the climax.
- **Audio**: add `<audio-tracks>` containing `<audio src="track.mp3" start="0" volume="0.8" />` entries for music and effects. Use `rustren beats track.mp3` to place cuts on beats.

Performance note for big images: a full-canvas `image` with `fit="cover"` that fully covers the canvas takes a faster single-pass path. Images smaller than the canvas use a clipped single-pass path. Keep source images near the output resolution when possible; very large sources cost more per frame.

Copyright: only animate images the user owns or is licensed to use. Ask the user to supply them; do not scrape.

## 8. Recipe: explainer / promo / captions

- Sequence scenes with a parent `container timeline="series"` and give each child a `duration`; no manual start math.
- Reuse components with `include` (see "Component Reuse" in [SCHEMA.md](SCHEMA.md)).
- Staggered lists: `stagger` (HTML) / `stagger_secs` (JSON) on the container.
- Rich text: `spans` with per-span color, `letter_spacing`, color emoji. Typewriter and glow effects exist; see `examples/glow_and_typewriter_demo.html`.
- Data visuals: see `examples/charts_2d_showcase.html`, `examples/blocks_data_showcase.html`.

Look in [examples/](examples/) first and copy the closest file instead of writing from scratch.

## 9. Debugging playbook

| Symptom | Do this |
|---|---|
| Only text appears, images missing or misplaced | Overlapping children need `position="absolute"`; check `src` path; run `layout --t <sec>`. |
| Text clipped or overlapping | `layout --t <sec> --diagnose`; add `max_width`; reduce font size. |
| Animation not visible | Snapshot at a time inside its `[start, end]`; check the property name against `capabilities`. |
| Render is slow | `render --profile`; look at the top layers; use `--preset veryfast`; try `RUSTREN_HW_ENCODER=auto`; keep sources near output resolution. |
| Render fails immediately | Read stderr: usually a missing asset path, unknown field, or `ffmpeg` not on PATH. |
| Output differs from before | `rustren diff old.html new.html --t <sec>`. |

## 10. Cache-aware authoring (faster renders, same output)

`rustren` caches aggressively. Write scenes that let the caches hit. Authoritative table: "Cache Engine" in [SCHEMA.md](SCHEMA.md).

| Cache | Do this (hit) | Avoid this (miss) |
|---|---|---|
| Static background snapshot | Put 100% static layers (no animations, no `video`, active the whole duration) at the **start** of the layer list | An animated layer at index 0 stops the static prefix from being detected |
| Blurred layer | `blur` without `shadow` on a root layer; animate only `opacity` | `shadow` with blur, blur inside a `container`, animating `x`/`y`/`width`/`height` |
| SVG raster | Static `svg`; animating `x`, `y`, `rotation`, `opacity` still hits | Animating SVG `width`/`height`, `path_progress`, `stroke_dashoffset` |
| SVG tree | Repeat the same inline SVG or `src` freely | n/a |
| Asset store | Reuse the **exact same `src` string** so media is decoded once | Slightly different paths/URLs for the same file |
| Text / glyph | Animate text `opacity`, `x`, `y` | Animating text `scale` (re-rasterizes glyphs every frame) |
| Whole-frame dedup (Pro build) | Hold frames where nothing changes | Constant tiny motion everywhere |

How to verify it worked:

```bash
rustren render scene.html -o out.mp4 --profile
```

`--profile` prints hit/miss counts per cache and the slowest root layers (cumulative CPU time). Typical reading:
- Low hit rate on a cache means one of the "avoid" rows above applies to the top layers. Fix those first.
- One root layer dominating CPU time is usually a very large `image`, a heavy `blur`, or an animated SVG. Reduce source size, simplify, or split static from animated parts.
- `static_prefix_skipped: 0/N` means no static prefix was detected; move static layers to the top of the list.

Cache-friendly trade-off: do not contort the creative design to chase hit rates. Only restructure when `--profile` shows a real bottleneck.

Asset downloads are cached on disk (`--cache-dir`, default `.rustren_cache`). Reuse the same cache directory across runs; do not commit it.

## 11. Machine interfaces

- **CLI**: every command above; use exit codes (`0` ok, non-zero failure).
- **MCP**: `rustren mcp` starts a stdio JSON-RPC server (Model Context Protocol) so an agent can call tools without shelling out.
- **HTTP**: `rustren serve --host 0.0.0.0 --port 8080` with `RUSTREN_API_KEY` set; queue-based render service.
- **Discovery**: `rustren capabilities` (JSON), [schema.json](schema.json) (JSON Schema).

## 12. Output etiquette for agents

- Tell the user the exact output path and show a snapshot or contact sheet, not only "done".
- Report what you actually ran and its result. If `validate` printed warnings you chose not to fix, say so.
- Do not commit generated MP4/PNG files unless asked.

## Changelog

- **1.0.0** (rustren 0.3.0, schema 1.1): first release. Golden loop, command cheat sheet, authoring rules, motion-comic and explainer recipes, cache-aware authoring, debugging playbook, version check.
