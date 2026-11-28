---
name: sketch-explainer
description: Minimum quality floor for whiteboard explainers, chalkboard lessons, sketchnotes, one-line drawings, and marker-on-paper videos built with rustren. Enforces progressive stroke-draw reveals (path_progress / stroke_dashoffset), narration-paced drawing, clean paper/board contrast, and zero pop-in blocks.
---

# Sketch & Board Explainer (`rustren`)

## 1. Trigger & Scope
Use this skill for **hand-drawn / board-based explainers**: whiteboard lessons, chalkboard diagrams, sketchnotes, one-line drawings, stick-figure explainers, and marker-on-paper tutorials.
- **Do NOT use** for flat kinetic promos (`simple-motion`), data/news reels (`editorial-data`), 2.5D photo dioramas (`living-diorama`), or textured print/film (`retro-print`).

## 2. Style Presets (Pick One)
| Preset | Canvas Anchor | Ink / Stroke | Accent | Feel |
| :--- | :--- | :--- | :--- | :--- |
| **Whiteboard** | `#F8FAFC` | `#0F172A` (marker) | `#E11D48`, `#2563EB` | Clean classroom / SaaS explainer |
| **Chalkboard** | `#1B2A24` | `#F1F5F9` (chalk) | `#FACC15`, `#38BDF8` | Academic, warm, subtle `grain: 0.06` |
| **One-Line / Sketchnote** | `#F7F3E9` (warm paper) | `#1E293B` | `#EA580C`, `#0D9488` | Editorial notebook, continuous line |

## 3. Non-Negotiable Quality Floor
1. **Draw-On, Never Pop-In**: Every diagram, arrow, box, and icon MUST enter via progressive stroke drawing (`path_progress: 0.0 -> 1.0` or animated `stroke_dashoffset`), never abrupt `opacity` pop-ins.
2. **Narration-Paced Reveal**: Draw 1 concept at a time (`0.6–1.4s` per stroke group) with `0.3–0.6s` breathing pauses so the viewer reads as it draws.
3. **Clear Spatial Flow**: Arrange concepts left-to-right or top-to-bottom connected by drawn arrows. Never crowd >4 diagram nodes on screen simultaneously without panning (`world` camera `truck_right`) or wiping clean.
4. **Legible Stroke Weight**: Keep SVG `stroke-width` between `4px–10px` (on `720x1280` or `1920x1080`) with `stroke-linecap="round"` and `stroke-linejoin="round"`.

## 4. Exact `rustren` Recipe (Progressive Stroke Draw)
```json
{
  "type": "svg", "src": "diagram_step1.svg",
  "x": 120, "y": 320, "width": 480, "height": 320,
  "path_progress": 0.0, "start_sec": 0.5, "end_sec": 6.0,
  "animations": [
    { "property": "path_progress", "from": 0.0, "to": 1.0, "start_sec": 0.5, "end_sec": 1.6, "easing": "ease_in_out" }
  ]
}
```

## 5. Pre-Delivery Visual QA Gate
1. Run `rustren validate` (0 errors; check `SAFE_ZONE_9_16` if vertical).
2. Inspect a 6-frame contact sheet across draw-start, mid-draw, and full-board hold: verify strokes draw smoothly without snapping, labels never overlap drawn lines, and contrast is crisp.
