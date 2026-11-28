---
name: simple-motion
description: Minimum quality floor for flat 2D kinetic motion-graphics videos built with rustren (Reels/TikTok/Shorts promos, kinetic typography openers, social explainers). Enforces flat tactile palettes, unique per-beat motifs, dense-to-calm pacing contrast, protected text islands, off-screen transitions, and pre-delivery visual QA.
---

# Simple Motion (`rustren`)

## 1. Trigger & Scope

Use this skill whenever building **flat 2D motion-graphics videos** in `rustren`, including:
- Vertical social promos (`720x1280` 9:16 Reels, TikTok, YouTube Shorts).
- Kinetic typography openers, feature teasers, and product launch reels.
- Minimalist flat-vector explainers and brand stingers.

> **Quality Floor Mandate**: Everything in this document defines the **absolute minimum quality bar** for simple 2D motion graphics. You may exceed this bar with richer choreography or effects, but you must **never ship below it**. Any output violating the 6 Laws, triggering the Hard Reject List, or failing the Pre-Delivery Visual QA Gate is rejected.

---

## 2. Non-Negotiable Design Floor (The 6 Laws)

### Law 1 — Disciplined Flat Tactile Palette
Use a strict **flat tactile palette**: **0% gradients, 0% drop shadows, 0% outer glows**.
- **Base Anchor**: `#F3F0E6` (warm cream paper).
- **Primary Ink**: `#222933` (deep charcoal).
- **Inverted Ink**: `#FFFFFF` (pure white on dark/accent fills).
- **5 Flat Accents**: Hold 5 saturated, matte accent hexes consistent across the entire timeline (e.g. Coral `#E35B54`, Mustard `#EEB843`, Cobalt `#4B67D1`, Mint `#A7DBD8`, Plum `#8A1C7C`).

### Law 2 — 1 Beat = 1 Unique Hero Motif
Every beat must introduce **one distinct hero visual motif** (e.g., 4-color segmented ring, 6-petal U-bend crawling snake outline, 4-layer oversized organic sweep, curved trim-path lines, overlapping giant circles, falling flat confetti, full-bleed diagonal wipe, orbiting perimeter shapes, isolated oval matte squash, staggered card drop, bottom-up mask reveal). **Never reuse the same hero motif twice** in a single video.

### Law 3 — Dense-to-Calm Pacing Contrast
- Keep each beat to **1.0–2.5 seconds** (tight, scannable rhythm; ~1 keyword per 1–2s).
- Alternate **High-Energy (dense)** beats with **Calm (breather / isolated focus)** beats. Never chain 3+ hyper-busy beats without a visual breather.

### Law 4 — Protected Center Text Island & Peripheral Budget
On a `720x1280` (9:16) canvas:
- **Protected Text Island**: `x: 100..620`, `y: 440..800`. Decorative shapes must **never** collide with or obscure legibility inside this zone while copy is on screen.
- **Peripheral Shape Budget**:
  - **High-Energy (dense) beats**: `5–7` peripheral geometric accents framing the edges.
  - **Calm (breather) beats**: `0–2` peripheral shapes max.

### Law 5 — Zero Pop-In & Zero Hard-Vanish
No element may abruptly appear at full size or snap-delete on an active frame.
- **Off-Screen Entry/Exit**: All wipes and slides must start **100% off-screen** (`x >= W + 100`, `x <= -W - 100`, `y <= -H - 200`, or `y >= H + 200`) and exit **100% off-screen**, or scale cleanly from/to `0.0`.
- **Overshoot**: Primary entrances use `10–12%` `ease_out_back` overshoot (`0.0 -> 1.12 -> 0.96 -> 1.0`) for tactile snap.

### Law 6 — Zero Dead Holds & Zero Text Ghosting
- **No Dead Holds**: During every reading hold (>1.0s), apply subtle continuous life (`±10–16px` idle float, `±4–6°` slow rotation/wiggle, elastic scale breath, or crawling stroke dash).
- **No Text Ghosting**: Outgoing text and incoming text must **never** overlap or crossfade in the same screen space. Separate them with either a **≥2-frame full-coverage occluder** (wipe/card) or a **clean empty gap** (≥2 frames) where outgoing text is already at `scale: 0` / `opacity: 0`.

---

## 2.5 Motion Personality & Craft Rules
Pick **one** archetype per video and apply it consistently:

| Archetype | Duration | Easing | Overshoot | Best For |
| :--- | :--- | :--- | :--- | :--- |
| **Playful** | `150–300ms` | `ease_out_back` | `10–20%` | Social promos, consumer apps, upbeat reels |
| **Premium** | `350–600ms` | `cubic_bezier(0.4,0,0.2,1)` | `0%` | Luxury, editorial, minimal brand stingers |
| **Corporate** | `200–400ms` | `cubic_bezier(0.2,0,0,1)` | `0–3%` | B2B SaaS, clean announcements |
| **Energetic** | `100–250ms` | `ease_out_expo` | `15–30%` | High-tempo launches, sports, hype teasers |

- **Three Motion Layers**: Every beat needs **Primary** (hero text/action), **Secondary** (peripheral shapes reacting), and **Ambient** (continuous slow drift/crawl).
- **Distance-Scaled Duration**: `100px = base`, `200px = 1.3×`, `400px = 1.6×` (large sweeps take longer than tiny accents).
- **Simplicity Threshold**: `1` animated property = direct, `2` = polished, `3+` simultaneously on one element = noisy.

---

## 3. Anti-Slop Hard Reject List

Reject and fix immediately before showing the user if **any** of these 5 defects appear:
1. **Linear / Un-eased Motion**: Any entrance/exit using `linear` timing instead of purposeful easing (`ease_out_back`, `ease_in_out`, `ease_out`).
2. **Dead Frames (>1.5s Static)**: Any hold longer than 1.5s with zero visual movement (feels like a PowerPoint slideshow).
3. **Layout, Overlap & Safe-Zone Violations**: Clipped text, ghosting/overlapping words, weak contrast, or shapes colliding with the center text island.
4. **Wall-of-Text / No Focal Hierarchy**: >12 words per beat or flat typographic weight where headline and subtitle compete equally.
5. **Unmotivated FX / Transition Soup**: Gratuitous `echo` spam, always-on `motion_blur`, or sudden mid-frame pop-ins.

---

## 4. Exact `rustren` Primitive Recipes (Schema v1.2)

### 4.1 Crawling Multi-Color Outline (`svg` + `stroke_dasharray` + `stroke_dashoffset`)
Stack 4 single-color `svg` layers sharing the exact same smooth closed path, offset by `seg = total_len / (repeats * 4)`:

```json
{
  "type": "svg",
  "src": "flower_snake_0.svg",
  "x": -640, "y": -360, "width": 2000, "height": 2000,
  "pivot_x": 0.5, "pivot_y": 0.5,
  "stroke_dasharray": [319.0, 949.0],
  "stroke_dashoffset": -45.0,
  "start_sec": 3.0, "end_sec": 4.05,
  "animations": [
    { "property": "stroke_dashoffset", "from": -45.0, "to": -1180.0, "start_sec": 3.0, "end_sec": 4.05, "easing": "linear" }
  ]
}
```

### 4.2 Selective 1D `motion_blur` (Whip-In Text ONLY)
Apply `motion_blur` (`0.6–0.8`) **only** to fast horizontal whip-in text with **constant `y`**. Never put `motion_blur` on `world` camera when vectors must stay crisp.

```json
{
  "type": "text",
  "content": "COLORFUL",
  "x": 1150, "y": 560, "anchor": "center",
  "font_size": 86, "font_weight": 800, "color": "#222933",
  "motion_blur": 0.70,
  "start_sec": 4.18, "end_sec": 5.95,
  "animations": [
    { "property": "x", "from": 1150, "to": 360, "start_sec": 4.18, "end_sec": 4.46, "easing": "ease_out" }
  ]
}
```

### 4.3 Bottom-Up Mask Reveal (`track_matte: "alpha"`)
Use an inline `matte_layer` rect so the ending brand wordmark rises cleanly from below without clipping artifacts:

```json
{
  "type": "text",
  "content": "RUSTREN",
  "x": 360, "y": 760, "anchor": "center",
  "font_size": 98, "font_weight": 800, "color": "#222933", "letter_spacing": 8.0,
  "start_sec": 25.60, "end_sec": 27.00,
  "track_matte": "alpha",
  "matte_layer": { "type": "rect", "x": 40, "y": 555, "width": 640, "height": 150, "color": "#FFFFFF" },
  "animations": [
    { "property": "y", "from": 760, "to": 595, "start_sec": 25.60, "end_sec": 26.22, "easing": "ease_out_back" }
  ]
}
```

### 4.4 Engine Gotchas (Mandatory Checks)
- **`svg` scaling**: `svg` layers must animate `scale_x` and `scale_y` (not uniform `scale`) with `"pivot_x": 0.5, "pivot_y": 0.5`.
- **`circle` coordinates**: On `circle` layers, `x` and `y` are **offsets added to `cx` and `cy`** — never put absolute canvas coordinates in both `cx/cy` and `x/y` or the circle will double-shift off-screen.
- **`text` rotation**: `text` layers do not accept a top-level `"rotation"` field; use `perspective` + `rotate_y` for 3D card flips or keep hero text cleanly horizontal.

---

## 5. Mandatory Pre-Delivery Visual QA Gate

Before delivering any `simple-motion` video:
1. **12-Beat Contact Sheet**: Extract 1 hero frame per beat into a 6×2 contact sheet and verify palette discipline, unique motif per beat, dense-to-calm contrast, and clean center text island (`x: 100..620, y: 440..800`).
2. **Transition-Boundary Audit**: Verify zero text ghosting, zero color flash, zero pop-in, and clean bottom-up mask reveal on the ending brand mark.
