# Rustren Scene Schema (v1.1) — AI & Developer Guide

`rustren` is a browserless 2D video render engine. Scenes can be authored in declarative **JSON** (`.json`) or declarative **HTML** (`.html`).

> Run `rustren capabilities` to inspect supported features and versions (`renderer_version: "1.1"`, `schema_version: "1.1"`, `min_schema_version: "1.0"`) before generating scenes.

---

## Declarative HTML Syntax (Sprint 6)
In addition to JSON, `rustren` natively compiles declarative HTML files into the internal IR (`CompiledScene`):

```html
<scene width="1280" height="720" fps="30" duration="5.0" background="#0F172A">
    <!-- Responsive composition profiles -->
    <profiles>
        <profile name="portrait" width="1080" height="1920" />
        <profile name="square" width="720" height="720" />
    </profiles>

    <!-- Vector primitives -->
    <circle cx="640" cy="360" r="160" color="#38BDF8" opacity="0.3">
        <animate property="scale" from="0.5" to="1.5" start="0" end="2.0" easing="ease_out" />
    </circle>

    <!-- Flexbox Container -->
    <container x="200" y="240" width="880" height="240" radius="16" background="#1E293B" layout="column" padding="24" gap="12">
        <text font-size="40" color="#FFFFFF" font-weight="bold">
            Declarative HTML Video
            <animate property="opacity" from="0" to="1" start="0.2" end="0.8" easing="ease_out" />
        </text>
        <text font-size="20" color="#94A3B8">
            Rendered natively by rustren without Chromium!
        </text>
    </container>
</scene>
```

Supported tags: `<scene>`, `<profiles>`, `<profile>`, `<fonts>`, `<font>`, `<audio-tracks>`, `<audio>`, `<container>`, `<rect>`, `<circle>`, `<text>`, `<image>`, `<svg>`, `<video>`, `<waveform>`, `<lottie>`, and `<animate>`.

---

## One-Shot Rules
1. **Do not invent fields outside this schema** (see [schema.json](schema.json) for the official JSON Schema).
2. **Scene Timing**: Use `start_sec`, `end_sec`, or `duration_secs` (in seconds), and `timeline: "series"` on a container for automatic sequencing.
3. **`x` / `y` Animations**:
   - On root layers or `position: "absolute"` children inside a container: `from` and `to` are **absolute coordinates** (relative to the canvas for root layers, or relative to the container origin).
   - On `position: "relative"` flex children inside a `container`: `from` and `to` are **offsets** added to the computed `taffy` flex position; prefer `opacity`, `scale`, or `stagger_secs` when flex positioning should stay fixed.
4. **`width` / `height` Animations Inside a `container` (e.g. progress bar)**: Set `position: "absolute"`, `x: 0`, `y: 0` on that `rect` so its size is not overridden by `taffy` flex layout.
5. **Grid & Radial Glow Backgrounds**: Use an `svg` layer with `<pattern>` and `<radialGradient>` at the bottom of `layers[]`.
6. **Multi-line Text**: Always specify `max_width` on `type: "text"` so word-wrapping and `taffy` height estimation are accurate.
7. **Cache**: Read the [Cache Engine](#cache-engine-keys--how-to-write-cache-friendly-json) table below. Place static background layers at the start of `layers[]`, and prefer animating `opacity` / `x` / `y` / `rotation` to get cache **HITs** (avoid animating SVG `width`, `path_progress`, or text `scale` unless needed).

### 7. `keyframes[]`, `dynamic` Text, `stagger_secs`, `outline`/`gradient` & `fit`
Use these primitives to keep AI-generated JSON compact and expressive:
```json
{
  "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 4.0, "background": "#0F172A" },
  "layers": [
    {
      "type": "rect",
      "x": 640,
      "y": 360,
      "width": 600,
      "height": 260,
      "radius": 28,
      "color": "#000000",
      "anchor": "center",
      "skew_x": -10,
      "gradient": {
        "type": "radial",
        "stops": [
          { "offset": 0.0, "color": "#2D4AF2" },
          { "offset": 1.0, "color": "#0F172A" }
        ]
      },
      "animations": [
        {
          "property": "opacity",
          "keyframes": [
            { "time": 0.0, "value": 0.0 },
            { "time": 0.4, "value": 1.0, "easing": "ease_out" },
            { "time": 3.5, "value": 1.0 },
            { "time": 4.0, "value": 0.0, "easing": "ease_in" }
          ]
        }
      ]
    },
    {
      "type": "text",
      "x": 640,
      "y": 340,
      "font_size": 56,
      "font_weight": "bold",
      "anchor": "center",
      "gradient": { "from": "#22D3EE", "to": "#FFFFFF", "angle": 0 },
      "outline": { "color": "#000000", "width": 3.0 },
      "dynamic": { "mode": "typewriter", "text": "JSON in -> MP4 out", "start_sec": 0.2, "end_sec": 1.2 }
    },
    {
      "type": "text",
      "x": 1220,
      "y": 680,
      "font_size": 18,
      "color": "#94A3B8",
      "anchor": "bottom_right",
      "dynamic": { "mode": "timecode", "format": "mm:ss.cc" }
    }
  ]
}
```

---

## Root Structure
```json
{
  "schema_version": "1.1",
  "min_renderer_version": "1.0",
  "canvas": {
    "width": 1080,
    "height": 1920,
    "fps": 30,
    "duration_secs": 15.0,
    "background": "#070B14"
  },
  "fonts": ["/path/to/Inter-Bold.ttf"],
  "audio": [
    {
      "src": "/path/to/audio.mp3",
      "start_sec": 0.0,
      "volume": 0.8,
      "trim_start_sec": 0.0,
      "trim_end_sec": 15.0,
      "playback_rate": 1.0
    }
  ],
  "layers": [ ... ]
}
```

## Common Layer Properties
- `name` (optional): string
- `type` (required): `"container"` | `"rect"` | `"circle"` | `"text"` | `"svg"` | `"image"` | `"video"` | `"waveform"` | `"lottie"` | `"particles"`
- `position` (optional, default `"relative"`): `"relative"` | `"absolute"`
- `anchor` (optional, default `"top_left"`): `"top_left"` | `"top_center"` | `"top_right"` | `"center_left"` | `"center"` | `"center_right"` | `"bottom_left"` | `"bottom_center"` | `"bottom_right"`
- `skew_x`, `skew_y` (optional, default `0.0`): f32 (degrees)
- `rotate_x`, `rotate_y` (optional, default `0.0`): f32 (2.5D perspective tilt in degrees)
- `perspective` (optional, default `800.0`): f32 (focal distance in px for 2.5D ray-plane homography)
- `backface_visible` (optional, default `true`): bool (culls layer when facing away if `false`)
- `z_index` *(alias: `z`)* (optional, default `0.0`): f32 (painter's depth sort order within root or container)
- `grain` (optional, default `0.0`): f32 (`0.0..1.0` deterministic film grain intensity)
- `vignette` (optional, default `0.0`): f32 (`0.0..1.0` radial edge darkening)
- `halftone` (optional, default `0.0`): f32 (`> 0.0` rotated 45-deg halftone dot screen)
- `start_sec` (optional, default `0.0`): f32
- `end_sec` (optional, default `canvas.duration_secs`): f32
- `duration_secs` (optional): f32 (alternative `end_sec = start_sec + duration_secs`, useful for `series` sequencing)
- `overlap_secs` (optional, default `0.0`): f32 (backward overlap between items in `timeline: "series"` containers)
- `flex_grow` (optional): f32
- `margin` (optional): f32
- `blend_mode` (optional, default `"normal"`): `"normal"` | `"multiply"` | `"screen"` | `"overlay"` | `"darken"` | `"lighten"` | `"difference"`
- `blur` (optional, default `0.0`): f32 (Gaussian blur radius in px)
- `shadow` (optional): `{ "offset_x": 0.0, "offset_y": 8.0, "blur": 16.0, "color": "#00000080" }`
- `animations` (optional): array of Animation

## Layer Types

### 1. `container` (Flexbox via `taffy` & Nested Timeline)
- `x`, `y` (default `0.0`), `width`, `height` (optional, auto-sizes to content if omitted)
- `radius` (default `0.0`), `background` (optional hex `#RGB` / `#RRGGBB` / `#RRGGBBAA`)
- `gradient` (optional): `{ "from": "#22D3EE", "to": "#818CF8", "angle": 90 }`
- `border_color` (optional), `border_width` (default `0.0`), `opacity` (default `1.0`), `scale` (default `1.0`), `rotation` (default `0.0`, degrees), `clip` (default `false`), `cascade_opacity` (default `false`)
- `timeline` (optional, default `"global"`): `"global"` | `"local"` (child animation times relative to container `start_sec`) | `"series"` (children sequenced automatically via `duration_secs` & `overlap_secs`)
- `stagger_secs` (optional, default `0.0`): f32 (progressive delay `i * stagger_secs` applied to each child)
- `layout`:
  - `direction`: `"row"` | `"column"`
  - `justify_content`: `"start"` | `"center"` | `"end"` | `"space_between"` | `"space_around"` | `"stretch"` (aliases: `"flex_start"`, `"flex_end"`)
  - `align_items`: `"start"` | `"center"` | `"end"` | `"space_between"` | `"space_around"` | `"stretch"` (aliases: `"flex_start"`, `"flex_end"`)
  - `gap` (default `0.0`), `padding` (default `0.0`)
- `children`: array of Layer

### 2. `rect`
- `x`, `y` (default `0.0`), `width`, `height` (required)
- `radius` (default `0.0`), `color` (required hex; if `gradient` is also set, **`gradient` takes precedence**), `scale` (default `1.0`), `rotation` (default `0.0`, degrees)
- `gradient`, `border_color`, `border_width`, `path_progress` (default `1.0`), `stroke_dashoffset` (optional), `opacity`

### 3. `circle`
- `cx`, `cy` (default `0.0`), `r` (required), `color` (required hex), `border_color` (optional), `border_width` (default `0.0`), `path_progress` (default `1.0`), `stroke_dashoffset` (optional), `opacity` (default `1.0`), `scale` (default `1.0`)
- When placed as a `relative` child inside a `container`, its flex box size is `2r × 2r` and `cx`/`cy` is placed at the flex box center.

### 4. `text`
- `x`, `y` (default `0.0`), `content` (optional if `spans` or `dynamic` is provided)
- `dynamic` (optional): declarative dynamic text:
  - `{"mode": "typewriter", "text": "...", "start_sec": 0.0, "end_sec": 1.0, "cursor": "|"}`
  - `{"mode": "counter", "from": 0, "to": 100, "decimals": 0, "prefix": "", "suffix": "%", "easing": "ease_out"}`
  - `{"mode": "timecode", "format": "mm:ss", "offset_sec": 0.0}` (`"mm:ss"`, `"mm:ss.cc"`, `"hh:mm:ss"`)
- `spans` (optional): array of `{ "text": "...", "color": "#...", "font_weight": "bold", "font_family": "...", "font_size": 48.0 }` for multi-style rich text
- `font_size` (default `48.0`), `line_height` (optional, default `1.25 × font_size`), `letter_spacing` (default `0.0`, px), `font_family` (optional), `font_weight`: `"normal"` | `"bold"`
- `text_align`: `"left"` | `"center"` | `"right"`
- `color` (default `"#FFFFFF"`), `gradient` (optional, overrides `color` fill), `outline` (optional: `{ "color": "#000000", "width": 2.0 }`), `opacity` (default `1.0`), `scale` (default `1.0`), `max_width` (optional, recommended)

### 5. `svg`
- `x`, `y` (default `0.0`), `width`, `height` (optional), `src` (required: file path, URL, or inline `<svg ...>...</svg>`), `path_progress` (default `1.0`), `stroke_dashoffset` (optional), `stroke_dasharray` (optional array of f32), `opacity` (default `1.0`), `rotation` (default `0.0`, degrees)

### 6. `image`
- `x`, `y` (default `0.0`), `width`, `height` (optional), `src` (required: file path or URL), `fit` (default `"fill"`: `"fill"` | `"cover"` | `"contain"`), `opacity` (default `1.0`), `rotation` (default `0.0`, degrees), `scale` (default `1.0`)

### 7. `video`
- `x`, `y` (default `0.0`), `width`, `height` (optional, defaults to source video dimensions), `src` (required: file path or URL for MP4/MOV/WEBM), `fit` (default `"fill"`: `"fill"` | `"cover"` | `"contain"`)
- `trim_start_sec` (default `0.0`), `trim_end_sec` (optional), `playback_rate` (default `1.0`, affects both video & audio), `volume` (default `1.0`), `muted` (default `false`)
- `opacity` (default `1.0`), `rotation` (default `0.0`), `scale` (default `1.0`)

## `animations[]`
```json
{
  "property": "opacity",
  "from": 0.0,
  "to": 1.0,
  "start_sec": 0.0,
  "end_sec": 0.5,
  "easing": "spring"
}
```
- `property`: `"x"` | `"y"` | `"width"` | `"height"` | `"opacity"` | `"scale"` | `"rotation"` | `"skew_x"` | `"skew_y"` | `"rotate_x"` | `"rotate_y"` | `"perspective"` | `"z_index"` | `"cx"` | `"cy"` | `"r"` | `"radius"` | `"color"` *(alias: `"background"`)* | `"border_color"` | `"path_progress"` | `"stroke_dashoffset"` | `"volume"`
- `from`, `to`: number (`f32`) or hex color (`"#RRGGBB"` / `"#RRGGBBAA"`) for `color` / `background` / `border_color`
- `keyframes` *(optional alternative to `from`/`to`/`start_sec`/`end_sec`)*: array of `{ "time": 0.0, "value": 0.0, "easing": "ease_out" }` for multi-stop timelines in one block (e.g., fade-in → hold → fade-out)
- `easing`: `"linear"` | `"ease_in"` | `"ease_out"` | `"ease_in_out"` | `"ease_in_quad"` | `"ease_out_quad"` | `"ease_in_out_quad"` | `"ease_in_expo"` | `"ease_out_expo"` | `"ease_in_out_expo"` | `"ease_in_back"` | `"ease_out_back"` | `"ease_in_out_back"` | `"ease_out_bounce"` | `"cubic_bezier"` | `"spring"`
- `bezier` *(optional, for `easing: "cubic_bezier"`)*: `[x1, y1, x2, y2]`
- `spring` *(optional, for `easing: "spring"`)*: `{ "stiffness": 120.0, "damping": 14.0, "mass": 1.0 }`
- `repeat` *(optional)*: integer (`>= 0`) or `"infinite"` to repeat the `[start_sec, end_sec]` cycle
- `yoyo` *(optional, default `false`)*: if `true`, odd cycles reverse direction (`to` → `from`)

## Time, Animation & Inheritance Semantics
- **Animation Times (`animations[].start_sec` / `end_sec`)**: Evaluated in **global** video time by default (not relative to the layer's `start_sec`), unless inside a `timeline: "local"` or `"series"` container.
- **Clamping & Multi-keyframe**: Before the first animation starts, the property is clamped to `from`. Between two sequential animations on the same property (e.g. fade-in `0→1` then fade-out `1→0`), the value holds at the previous animation's `to`. After the last animation finishes, it clamps to `to`.
- **Layer `end_sec`**: **Inclusive** (`time_sec <= end_sec` is rendered), quantized per frame (`frame_idx / fps`). For a clean hard cut without a 1-frame overlap, use `end_sec = next_start - 1/fps`.
- **Z-order**: Order in `layers[]` and `children[]` is back-to-front (first element at the bottom, last element on top).
- **Container Inheritance**:
  - Container `opacity` does **not** cascade to `children` by default (`cascade_opacity: false`), though if `opacity <= 0` children are skipped. Set `"cascade_opacity": true` to fade an entire scene/container together (enable `cascade_opacity: true` on nested containers as well if they should inherit).
  - `start_sec` / `end_sec` on `children` are evaluated according to the container's `timeline` mode (`"global"`, `"local"`, or `"series"`), and when the parent container is outside its active window, all `children` are skipped.
  - `cx`, `cy`, `r` apply to `circle` (enabling pulse & ripple animations).
  - `rotation` applies to `container`, `rect`, `image`, `video`, and `svg`.
  - `scale` applies to `container`, `rect`, `text`, `image`, `video`, and `circle`.
- **Spring Overshoot, Clamping & Error Handling**:
  - `spring` can slightly overshoot `to` before settling. Animated `opacity` is clamped to `[0.0, 1.0]`, and layers with `width <= 0`, `height <= 0`, `r <= 0`, `scale <= 0`, or `opacity <= 0` are not drawn. If two animations on the same property overlap in time, the later one in the array wins.
  - Missing or unreadable `src` (`image`, `video`, `svg`, `audio`, `fonts`) fails immediately with a non-zero exit code. Unknown `font_family` falls back to system sans-serif.

## Size, Layout & Text Rules
- **`container`**: `width` and `height` are **optional** (auto-sizes to children if omitted). When `position: "absolute"` is used on a child inside a `container`, `x`/`y` are **relative to the `container`**, not the root canvas.
- **Text**:
  - When `fonts: []`, the engine automatically falls back to system sans-serif.
  - `font_family` matches the font's internal family name. `font_weight: "bold"` requests bold weight from `cosmic-text`.
  - `text_align` aligns text within `max_width` (or canvas width if omitted). Supports `\n` newlines and system color emojis.
- **Gradients & Colors**: Colors use hex strings (`#RGB`, `#RRGGBB`, `#RRGGBBAA`). `gradient` supports `"type": "linear" | "radial"` and multi-stop `"stops": [{ "offset": 0.0, "color": "#..." }, ...]` (or 2-stop `"from"` / `"to"`). `angle` is measured clockwise in degrees from the positive X-axis (`0°` = left-to-right, `90°` = top-to-bottom).
- **SVG**: Rendered via `resvg`/`usvg` (supports `<path>`, `<rect>`, `<circle>`, `<pattern>`, `<radialGradient>`, `<linearGradient>`, etc.).

## Output Specs & Performance
- **Format**: MP4 (H.264 High Profile, `yuv420p`, `30fps` default).
- **CLI Inspection Tools**:
  - `rustren validate <file.json>` (`--strict-determinism` flags remote URLs, empty `fonts[]`, and `repeat: "infinite"`): checks schema compatibility, rejects unknown fields, and emits grouped lint warnings (profile-aware font size, out-of-canvas bounds, low text contrast).
  - `rustren snapshot <file.json> -t 1.5,5.6 --sheet -o sheet.png`: renders individual timestamps or a contact sheet directly to PNG.
  - `rustren layout <file.json> -t 5.0`: outputs computed bounding boxes and measured text dimensions as JSON.
  - `rustren render <file.json> -o out.mp4 --profile`: prints per-layer CPU time and cache hit/miss stats.

## Safe Sizing & Typography Guide (9:16 / 1080×1920)
- **Safe margins**: `>= 60` px left/right, `>= 120` px top/bottom (avoids Shorts/Reels/TikTok UI overlays).
- **Typography**:
  - Main title: `font_size >= 64`
  - Subtitle / card heading: `font_size >= 44`
  - Body text: `font_size >= 40` (avoid `< 34` on mobile)
  - Small label / badge: `font_size >= 26`

## Coordinate System & Anchor
- **Origin**: `(0, 0)` at **top-left**, `+x` right, `+y` down, units in **px**.
- **Anchor & Pivot**: Default `anchor` is `"top_left"` (use `"center"` to position by center). `circle` is centered at `(cx, cy)`. `rect`, `container`, `image`, `svg`, and `text` apply `rotation`/`skew_x`/`skew_y` around the center of their bounding box.

## Complete Scene Example (1 Scene + Progress Bar)
```json
{
  "canvas": { "width": 1080, "height": 1920, "fps": 30, "duration_secs": 5.0, "background": "#070B14" },
  "fonts": [],
  "audio": [],
  "layers": [
    {
      "type": "container",
      "name": "scene_1",
      "x": 60,
      "y": 300,
      "width": 960,
      "opacity": 0.0,
      "cascade_opacity": true,
      "start_sec": 0.0,
      "end_sec": 4.9667,
      "layout": { "direction": "column", "gap": 28, "padding": 40 },
      "background": "#111A2E",
      "radius": 24,
      "animations": [
        { "property": "opacity", "from": 0.0, "to": 1.0, "start_sec": 0.0, "end_sec": 0.4, "easing": "ease_out" },
        { "property": "opacity", "from": 1.0, "to": 0.0, "start_sec": 4.5, "end_sec": 4.9, "easing": "ease_in" }
      ],
      "children": [
        { "type": "text", "content": "MAIN HEADLINE", "font_size": 72, "font_weight": "bold", "color": "#22D3EE", "max_width": 880 },
        { "type": "text", "content": "Concise supporting copy readable on mobile screens.", "font_size": 44, "color": "#FFFFFF", "max_width": 880 }
      ]
    },
    {
      "type": "rect",
      "name": "progress",
      "x": 60,
      "y": 1790,
      "width": 0,
      "height": 8,
      "color": "#22D3EE",
      "radius": 4,
      "animations": [
        { "property": "width", "from": 0.0, "to": 960.0, "start_sec": 0.0, "end_sec": 5.0, "easing": "linear" }
      ]
    }
  ]
}
```

## Quick Layout Recipes
- **2-Column Comparison Card (`row` + `flex_grow`)**:
```json
{
  "type": "container",
  "x": 60,
  "y": 800,
  "width": 960,
  "layout": { "direction": "row", "gap": 24 },
  "children": [
    {
      "type": "container",
      "flex_grow": 1,
      "background": "#111A2E",
      "radius": 24,
      "layout": { "direction": "column", "gap": 12, "padding": 32 },
      "children": [
        { "type": "text", "content": "#7", "font_size": 76, "font_weight": "bold", "color": "#22D3EE", "text_align": "center", "max_width": 400 },
        { "type": "text", "content": "GDP PPP", "font_size": 36, "color": "#94A3B8", "text_align": "center", "max_width": 400 }
      ]
    },
    {
      "type": "container",
      "flex_grow": 1,
      "background": "#111A2E",
      "radius": 24,
      "layout": { "direction": "column", "gap": 12, "padding": 32 },
      "children": [
        { "type": "text", "content": "#1", "font_size": 76, "font_weight": "bold", "color": "#818CF8", "text_align": "center", "max_width": 400 },
        { "type": "text", "content": "Largest in ASEAN", "font_size": 36, "color": "#94A3B8", "text_align": "center", "max_width": 400 }
      ]
    }
  ]
}
```
- **Bullet List (`column`)**:
```json
{
  "type": "container",
  "x": 60,
  "y": 600,
  "width": 960,
  "background": "#111A2E",
  "radius": 24,
  "layout": { "direction": "column", "gap": 20, "padding": 36 },
  "children": [
    { "type": "text", "content": "• Concise first bullet point", "font_size": 42, "color": "#FFFFFF", "max_width": 888 },
    { "type": "text", "content": "• Second bullet with accent color", "font_size": 42, "color": "#22D3EE", "max_width": 888 }
  ]
}
```

### 7. `keyframes[]`, `dynamic` Text, `stagger_secs`, `outline`/`gradient` & `fit`
Use these primitives to keep AI-generated JSON compact and expressive:
```json
{
  "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 4.0, "background": "#0F172A" },
  "layers": [
    {
      "type": "rect",
      "x": 640,
      "y": 360,
      "width": 600,
      "height": 260,
      "radius": 28,
      "color": "#000000",
      "anchor": "center",
      "skew_x": -10,
      "gradient": {
        "type": "radial",
        "stops": [
          { "offset": 0.0, "color": "#2D4AF2" },
          { "offset": 1.0, "color": "#0F172A" }
        ]
      },
      "animations": [
        {
          "property": "opacity",
          "keyframes": [
            { "time": 0.0, "value": 0.0 },
            { "time": 0.4, "value": 1.0, "easing": "ease_out" },
            { "time": 3.5, "value": 1.0 },
            { "time": 4.0, "value": 0.0, "easing": "ease_in" }
          ]
        }
      ]
    },
    {
      "type": "text",
      "x": 640,
      "y": 340,
      "font_size": 56,
      "font_weight": "bold",
      "anchor": "center",
      "gradient": { "from": "#22D3EE", "to": "#FFFFFF", "angle": 0 },
      "outline": { "color": "#000000", "width": 3.0 },
      "dynamic": { "mode": "typewriter", "text": "JSON in -> MP4 out", "start_sec": 0.2, "end_sec": 1.2 }
    },
    {
      "type": "text",
      "x": 1220,
      "y": 680,
      "font_size": 18,
      "color": "#94A3B8",
      "anchor": "bottom_right",
      "dynamic": { "mode": "timecode", "format": "mm:ss.cc" }
    }
  ]
}
```

---

## v1.1 Feature Recipes (Copy-Paste Ready)

### 1. SVG Path Draw-On (`path_progress`)
Use `path_progress` (`0.0` → `1.0`) directly on an `svg` layer—the engine automatically measures path lengths (no manual `stroke_dasharray` guessing). SVG gradient fills continue rendering during the animation.
```json
{
  "type": "svg",
  "x": 44,
  "y": 230,
  "width": 612,
  "height": 150,
  "path_progress": 0.0,
  "src": "<svg viewBox='0 0 612 150' xmlns='http://www.w3.org/2000/svg'><path d='M0 130 Q 150 40, 300 95 T 612 42' fill='none' stroke='#22D3EE' stroke-width='5' stroke-linecap='round'/></svg>",
  "animations": [
    { "property": "path_progress", "from": 0.0, "to": 1.0, "start_sec": 0.5, "end_sec": 1.5, "easing": "ease_in_out_expo" }
  ]
}
```

### 2. Morphing Container (`radius`, `background`, `clip`)
A `container` with `"clip": true` can morph smoothly from a pill button into a full card while swapping `children` via `start_sec`/`end_sec`:
```json
{
  "type": "container",
  "x": 790,
  "y": 496,
  "width": 340,
  "height": 88,
  "radius": 44,
  "background": "#141418",
  "clip": true,
  "shadow": { "offset_x": 0, "offset_y": 24, "blur": 48, "color": "#00000045" },
  "animations": [
    { "property": "x", "from": 790.0, "to": 650.0, "start_sec": 0.5, "end_sec": 0.8, "easing": "spring", "spring": { "stiffness": 210, "damping": 20, "mass": 1 } },
    { "property": "width", "from": 340.0, "to": 620.0, "start_sec": 0.5, "end_sec": 0.8, "easing": "spring", "spring": { "stiffness": 210, "damping": 20, "mass": 1 } },
    { "property": "radius", "from": 44.0, "to": 32.0, "start_sec": 0.5, "end_sec": 0.8, "easing": "ease_out" },
    { "property": "background", "from": "#141418", "to": "#FFFFFF", "start_sec": 0.5, "end_sec": 0.8, "easing": "ease_out" }
  ],
  "children": []
}
```

### 3. Color Interpolation & `cubic_bezier`
Animate `color` / `background` / `border_color` with hex strings in `from` and `to`:
```json
{
  "type": "text",
  "x": 72,
  "y": 28,
  "content": "Week",
  "font_size": 28,
  "font_weight": "bold",
  "color": "#64748B",
  "max_width": 120,
  "animations": [
    {
      "property": "color",
      "from": "#64748B",
      "to": "#FFFFFF",
      "start_sec": 1.0,
      "end_sec": 1.25,
      "easing": "cubic_bezier",
      "bezier": [0.22, 1.0, 0.36, 1.0]
    }
  ]
}
```

### 4. Rich Text (`spans`), `letter_spacing` & Color Emoji
- Use `spans` to mix colors, weights, and font sizes in one paragraph.
- For emoji text (`😊`, `🔥`, `❤️`), **leave `font_family` unset** so the engine falls back to system color emoji fonts (`Noto Color Emoji` / `Apple Color Emoji`).
```json
{
  "type": "text",
  "x": 80,
  "y": 120,
  "font_size": 48,
  "letter_spacing": 1.5,
  "max_width": 900,
  "spans": [
    { "text": "Render ", "color": "#FFFFFF" },
    { "text": "100% deterministic ", "color": "#22D3EE", "font_weight": "bold" },
    { "text": "without a browser.", "color": "#94A3B8" }
  ]
}
```

### 5. Relative Sequencing (`timeline: "series"` & `"local"`)
- `timeline: "series"`: children run sequentially based on `duration_secs` (overlapping backward by `overlap_secs` for crossfades), with each child's animation times starting at `0.0` (local to that child).
```json
{
  "type": "container",
  "timeline": "series",
  "x": 0,
  "y": 0,
  "width": 1080,
  "height": 1920,
  "children": [
    {
      "type": "container",
      "duration_secs": 3.0,
      "background": "#0F172A",
      "children": [
        {
          "type": "text",
          "content": "Slide 1",
          "max_width": 800,
          "animations": [{ "property": "opacity", "from": 0.0, "to": 1.0, "start_sec": 0.0, "end_sec": 0.4 }]
        }
      ]
    },
    {
      "type": "container",
      "duration_secs": 3.0,
      "overlap_secs": 0.3,
      "background": "#1E293B",
      "children": [
        {
          "type": "text",
          "content": "Slide 2 (crossfade 0.3s)",
          "max_width": 800,
          "animations": [{ "property": "opacity", "from": 0.0, "to": 1.0, "start_sec": 0.0, "end_sec": 0.3 }]
        }
      ]
    }
  ]
}
```

### 6. Video Clip Layer & Visual Effects (`video`, `blur`, `blend_mode`)
```json
{
  "type": "video",
  "x": 0,
  "y": 0,
  "width": 1920,
  "height": 1080,
  "src": "/path/to/clip.mp4",
  "trim_start_sec": 2.0,
  "trim_end_sec": 10.0,
  "playback_rate": 1.25,
  "volume": 0.8,
  "muted": false
}
```
```json
{
  "type": "circle",
  "cx": 1500,
  "cy": 280,
  "r": 240,
  "color": "#22D3EE",
  "opacity": 0.25,
  "blur": 60.0,
  "blend_mode": "screen"
}
```

### 7. `keyframes[]`, `dynamic` Text, `stagger_secs`, `outline`/`gradient` & `fit`
Use these primitives to keep AI-generated JSON compact and expressive:
```json
{
  "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 4.0, "background": "#0F172A" },
  "layers": [
    {
      "type": "rect",
      "x": 640,
      "y": 360,
      "width": 600,
      "height": 260,
      "radius": 28,
      "color": "#000000",
      "anchor": "center",
      "skew_x": -10,
      "gradient": {
        "type": "radial",
        "stops": [
          { "offset": 0.0, "color": "#2D4AF2" },
          { "offset": 1.0, "color": "#0F172A" }
        ]
      },
      "animations": [
        {
          "property": "opacity",
          "keyframes": [
            { "time": 0.0, "value": 0.0 },
            { "time": 0.4, "value": 1.0, "easing": "ease_out" },
            { "time": 3.5, "value": 1.0 },
            { "time": 4.0, "value": 0.0, "easing": "ease_in" }
          ]
        }
      ]
    },
    {
      "type": "text",
      "x": 640,
      "y": 340,
      "font_size": 56,
      "font_weight": "bold",
      "anchor": "center",
      "gradient": { "from": "#22D3EE", "to": "#FFFFFF", "angle": 0 },
      "outline": { "color": "#000000", "width": 3.0 },
      "dynamic": { "mode": "typewriter", "text": "JSON in -> MP4 out", "start_sec": 0.2, "end_sec": 1.2 }
    },
    {
      "type": "text",
      "x": 1220,
      "y": 680,
      "font_size": 18,
      "color": "#94A3B8",
      "anchor": "bottom_right",
      "dynamic": { "mode": "timecode", "format": "mm:ss.cc" }
    }
  ]
}
```

---

## Cache Engine: Keys & How to Write Cache-Friendly JSON

| Cache | Scope & Key | How to Hit (and What Causes Misses) |
| :### 7. `keyframes[]`, `dynamic` Text, `stagger_secs`, `outline`/`gradient` & `fit`
Use these primitives to keep AI-generated JSON compact and expressive:
```json
{
  "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 4.0, "background": "#0F172A" },
  "layers": [
    {
      "type": "rect",
      "x": 640,
      "y": 360,
      "width": 600,
      "height": 260,
      "radius": 28,
      "color": "#000000",
      "anchor": "center",
      "skew_x": -10,
      "gradient": {
        "type": "radial",
        "stops": [
          { "offset": 0.0, "color": "#2D4AF2" },
          { "offset": 1.0, "color": "#0F172A" }
        ]
      },
      "animations": [
        {
          "property": "opacity",
          "keyframes": [
            { "time": 0.0, "value": 0.0 },
            { "time": 0.4, "value": 1.0, "easing": "ease_out" },
            { "time": 3.5, "value": 1.0 },
            { "time": 4.0, "value": 0.0, "easing": "ease_in" }
          ]
        }
      ]
    },
    {
      "type": "text",
      "x": 640,
      "y": 340,
      "font_size": 56,
      "font_weight": "bold",
      "anchor": "center",
      "gradient": { "from": "#22D3EE", "to": "#FFFFFF", "angle": 0 },
      "outline": { "color": "#000000", "width": 3.0 },
      "dynamic": { "mode": "typewriter", "text": "JSON in -> MP4 out", "start_sec": 0.2, "end_sec": 1.2 }
    },
    {
      "type": "text",
      "x": 1220,
      "y": 680,
      "font_size": 18,
      "color": "#94A3B8",
      "anchor": "bottom_right",
      "dynamic": { "mode": "timecode", "format": "mm:ss.cc" }
    }
  ]
}
```

--- | :### 7. `keyframes[]`, `dynamic` Text, `stagger_secs`, `outline`/`gradient` & `fit`
Use these primitives to keep AI-generated JSON compact and expressive:
```json
{
  "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 4.0, "background": "#0F172A" },
  "layers": [
    {
      "type": "rect",
      "x": 640,
      "y": 360,
      "width": 600,
      "height": 260,
      "radius": 28,
      "color": "#000000",
      "anchor": "center",
      "skew_x": -10,
      "gradient": {
        "type": "radial",
        "stops": [
          { "offset": 0.0, "color": "#2D4AF2" },
          { "offset": 1.0, "color": "#0F172A" }
        ]
      },
      "animations": [
        {
          "property": "opacity",
          "keyframes": [
            { "time": 0.0, "value": 0.0 },
            { "time": 0.4, "value": 1.0, "easing": "ease_out" },
            { "time": 3.5, "value": 1.0 },
            { "time": 4.0, "value": 0.0, "easing": "ease_in" }
          ]
        }
      ]
    },
    {
      "type": "text",
      "x": 640,
      "y": 340,
      "font_size": 56,
      "font_weight": "bold",
      "anchor": "center",
      "gradient": { "from": "#22D3EE", "to": "#FFFFFF", "angle": 0 },
      "outline": { "color": "#000000", "width": 3.0 },
      "dynamic": { "mode": "typewriter", "text": "JSON in -> MP4 out", "start_sec": 0.2, "end_sec": 1.2 }
    },
    {
      "type": "text",
      "x": 1220,
      "y": 680,
      "font_size": 18,
      "color": "#94A3B8",
      "anchor": "bottom_right",
      "dynamic": { "mode": "timecode", "format": "mm:ss.cc" }
    }
  ]
}
```

--- | :### 7. `keyframes[]`, `dynamic` Text, `stagger_secs`, `outline`/`gradient` & `fit`
Use these primitives to keep AI-generated JSON compact and expressive:
```json
{
  "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 4.0, "background": "#0F172A" },
  "layers": [
    {
      "type": "rect",
      "x": 640,
      "y": 360,
      "width": 600,
      "height": 260,
      "radius": 28,
      "color": "#000000",
      "anchor": "center",
      "skew_x": -10,
      "gradient": {
        "type": "radial",
        "stops": [
          { "offset": 0.0, "color": "#2D4AF2" },
          { "offset": 1.0, "color": "#0F172A" }
        ]
      },
      "animations": [
        {
          "property": "opacity",
          "keyframes": [
            { "time": 0.0, "value": 0.0 },
            { "time": 0.4, "value": 1.0, "easing": "ease_out" },
            { "time": 3.5, "value": 1.0 },
            { "time": 4.0, "value": 0.0, "easing": "ease_in" }
          ]
        }
      ]
    },
    {
      "type": "text",
      "x": 640,
      "y": 340,
      "font_size": 56,
      "font_weight": "bold",
      "anchor": "center",
      "gradient": { "from": "#22D3EE", "to": "#FFFFFF", "angle": 0 },
      "outline": { "color": "#000000", "width": 3.0 },
      "dynamic": { "mode": "typewriter", "text": "JSON in -> MP4 out", "start_sec": 0.2, "end_sec": 1.2 }
    },
    {
      "type": "text",
      "x": 1220,
      "y": 680,
      "font_size": 18,
      "color": "#94A3B8",
      "anchor": "bottom_right",
      "dynamic": { "mode": "timecode", "format": "mm:ss.cc" }
    }
  ]
}
```

--- |
| **Static Background Snapshot** | Prefix `layers[0..k]` per scene | **Hit**: Place all 100% static background layers (`start_sec: 0`, `end_sec` omitted or `>= duration_secs`, `animations: []`, non-`video`) at the **start of `layers[]`**. **Miss**: Putting an animated layer at index `0` stops the prefix scan for later static layers. |
| **Blurred-Layer Cache** (`THREAD_BLUR_CACHE`) | `(fnv1a(kind_without_opacity, blur), canvas_w, canvas_h)` per thread | **Hit**: Use `blur > 0` without `shadow` on a root non-`container`/non-`video` layer, and animate only `opacity` (or keep static). **Miss**: Adding `shadow`, placing inside a `container`, or animating `x`/`y`/`r`/`width`/`height`. |
| **SVG Raster Cache** (`THREAD_SVG_RASTER_CACHE`) | `(fnv1a(svg_bytes), byte_len, pw, ph)` per thread | **Hit**: Use `svg` without `path_progress`/`stroke_dashoffset`/`stroke_dasharray`, and do not animate `width`/`height`. Animating `x`, `y`, `rotation`, and `opacity` **still hits**! |
| **SVG Tree Cache** (`THREAD_SVG_CACHE`) | `(fnv1a(svg_bytes), byte_len)` per thread | **Hit**: Repeated inline `<svg>` or `src` values are parsed once per thread (still helps when `path_progress` is animated). |
| **AssetStore & Disk Cache** | Disk: `SHA-256(url)` · RAM: `src` string | **Hit**: Reuse the exact same `src` string across layers so images/SVGs/videos are decoded once. |
| **Font & Glyph Cache** (`THREAD_FONT_SYS`) | `(fonts.len(), glyph_cache_key)` per thread | **Hit**: Animating text `opacity`/`x`/`y` always hits glyph cache. **Miss**: Animating text `scale` rasterizes glyphs at new sizes every frame. |

### Additional Coordinate Rules
- **`circle` inside a `container`**: When using `position: "absolute"` on a `circle` inside a `container`, `cx`/`cy` in `animations` remain **relative to the `container`'s top-left corner**, not the root canvas.


---

## v0.3.0 Additions (Track Mattes & Component Reuse)

### Track Mattes (`track_matte` & `matte_layer`)
Every `Layer` supports After Effects-style track mattes:
- `track_matte`: `"none"` (default) | `"alpha"` | `"alpha_inverted"` | `"luma"` | `"luma_inverted"`
- `matte_layer`: Nested `Layer` used as the alpha/luma mask.
- **HTML syntax**: `<text track-matte="alpha"><matte><rect ... /></matte>Masked Text</text>`

### Component Reuse (`include`)
- **HTML**: `<include src="card.html" bg="#238636" label="Card A" />` (supports `{{var}}` and `{{var|default}}`, max depth `8`).
- **JSON**: `{ "type": "include", "src": "card.json", "vars": { "bg": "#238636" } }`.
