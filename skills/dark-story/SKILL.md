---
name: dark-story
description: Production style blueprint for Indonesian dark-story, horror folklore, shadow-puppet (wayang/kelir), and 2D cut-out rigging animations in rustren v0.6.0+ (schema 1.2). Enforces stepped 2-9fps puppet cadence, 6-plane kelir diorama staging, low-key luminance (27-30%), sepia/twilight palettes, and horror audio/LUFS targets.
---

# Dark-Story & Kelir Rigging (`rustren` Schema 1.2)

## 1. Trigger & Scope
Use this skill for **Indonesian dark-story, horror folklore, mystery, or shadow-puppet (wayang / kelir) 2D cut-out rigging animations**.
- **Do NOT use** for smooth 2.5D photo parallax (`living-diorama`), flat 2D kinetic promos (`simple-motion`), or news/data reels (`editorial-data`).

## 2. Canvas, Frame Rate & Stepped Motion Cadence
Match the restrained, uncanny rhythm of 2D puppet theater rather than smooth 30fps motion graphics:
- **Canvas**: `1920x1080` at `30 fps` (or `720x1280` for vertical, keeping subtitles inside `SAFE_ZONE_9_16` `15%–78%`), stereo audio `44.1 kHz`.
- **Stepped Rigging Cadence (`2–9 fps` effective)**: Hold character poses across `3–6` frames (`69%–94%` static hold frames). Since `rustren` has no `stepped` easing enum, implement stepped holds via `expr` (`floor(t * 6.0) / 6.0`) or tight keyframe pairs (`t` and `t + 0.001`).
- **Localized Rigging Over Camera Motion**: Keep backgrounds locked and drive storytelling through localized puppet pivots (`container` + `pivot_x`/`pivot_y` + `rotation`). Limit camera moves to slow Ken Burns push-ins (`1.00x -> 1.05x` over `5–10s`, `ease_in_out`), gentle horizontal tracking, or brief `3–6` frame micro-shakes on horror stingers.

## 3. 6-Plane Kelir Diorama Hierarchy
Stage every shot as a front-planar shadow theater (`0` 3D perspective distortion, `0` heavy black comic outlines):
- **Layer 0 — Backlit Sky / Radial Halo**: Radial gradient (`type: "radial"`) centered behind the focal subject (oil lamp / *blencong*) fading into dark vignette corners.
- **Layer 1 — Distant Atmospheric Plane**: Low-contrast hills, mist bands, palms, or harbor silhouettes (`blur: 4..8`).
- **Layer 2 — Midground Set**: Flat geometric village houses, wooden huts, bamboo, or interior walls in dark muted tones.
- **Layer 3 — Hero Rig Plane**: Lineless flat-vector cut-out puppets or deep silhouettes (`#130904`–`#1E120C`) with optional `1–2px` warm rim-light stroke. Rig hinged pivots via nested `container`s: `torso -> neck/head`, `shoulder -> elbow -> hand`, `hip -> knee -> foot`.
- **Layer 4 — Practical Light & FX**: Localized warm glows (oil lamps, lit windows, embers) using `blend_mode: "screen"` or `"lighten"` (never `"add"`, which is not in `BlendMode`) plus subtle floating dust/mist drift.
- **Layer 5 — Foreground Framing & Vignette**: Out-of-focus dark edge silhouettes (doorways, pillars, tall grass; `blur: 8..16`) plus radial `vignette`.

## 4. Low-Key Color Palettes & Lighting Bounds
- **Luminance Ceiling**: Mean frame luminance `68–77 / 255` (~27%–30% brightness), `35%–42%` deep shadow mass (`luma < 40`), high local dynamic contrast driven by the central backlight halo.
- **Mode A — Sepia / Blencong Amber (Default)**:
  - Deep silhouette / vignette: `#130904`, `#1E120C`
  - Midground wood / umber: `#442813`, `#472D1C`, `#6D4724`
  - Warm backlight halo: `#9A6B38`, `#C0A073`, `#CF9B59`
  - Single focal highlight (shirt, ledger, moon): `#D8CCB8` | Danger / fire accent: `#C87533`
- **Mode B — Twilight Mist / Desaturated Folk**:
  - Deep shadow: `#161917` | Timber midground: `#3E372E`, `#615C4B`
  - Misty dusk sky: `#7F908E`, `#B8BCB1` | Practical lamp gold: `#AF8945`

## 5. Shot Grammar, Subtitles & Audio Mix
- **Pacing**: `3.5–5.5s` median shot length (`8–15s` slow dread holds with `0.5–1.0s` cross-fades for tension). Rotate 4 shot templates: *Wide diorama tableau*, *Side-scrolling walk*, *Profile silhouette close-up*, and *Top-down prop insert*.
- **Subtitles**: Bottom-center clean sans-serif (`28–34px` at 1080p, `#F0ECE4` with `shadow` or `60%` dark translucent pill). Optional top-center chapter header (`16–20px`, `40%–55%` opacity).
- **Audio & Loudness**: Master to `-14.3` to `-15.8 LUFS` integrated, true peak `<= -1.5 dBTP`. Layer a continuous low nocturnal ambient bed (wind, crickets, drone), `2–5` deliberate `0.7–1.2s` pauses/min before reveals, and subtle SFX accents aligned to cuts/gestures.

## 6. Exact `rustren` Schema 1.2 Recipe (Radial Halo + Hinged Puppet Arm)
```json
{
  "schema_version": "1.2",
  "canvas": { "width": 1920, "height": 1080, "fps": 30, "duration_secs": 6.0, "background": "#130904" },
  "layers": [
    {
      "type": "rect", "x": 0, "y": 0, "width": 1920, "height": 1080,
      "gradient": { "type": "radial", "colors": ["#CF9B59", "#442813", "#130904"] },
      "vignette": 0.45
    },
    {
      "type": "container", "x": 820, "y": 420, "width": 280, "height": 420,
      "pivot_x": 0.5, "pivot_y": 0.2,
      "animations": [
        { "property": "rotation", "expr": "sin(floor(t * 6.0) / 6.0 * 1.5) * 4.0", "start_sec": 0.0, "end_sec": 6.0 }
      ],
      "children": [
        { "type": "svg", "src": "puppet_torso.svg", "x": 0, "y": 0, "width": 280, "height": 420 }
      ]
    }
  ]
}
```

## 7. Pre-Delivery Visual & Audio QA Gate
1. Run `rustren validate` (ensure overscaled backdrops if using `world` camera to avoid `EDGE_REVEAL`; `PACING_STAGNATION` will not fire as long as at least 1 subtle animation/flicker is active).
2. Run `rustren qa out.mp4` and verify loudness (`-14` to `-16 LUFS`, true peak `<= -1.5 dBTP`), zero unintentional black frames, readable silhouettes against the radial halo, and legible subtitles.
