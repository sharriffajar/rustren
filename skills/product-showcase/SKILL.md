---
name: product-showcase
description: Minimum quality floor for SaaS/UI demos, terminal and code walkthroughs, feature launches, and 2.5D product carousels built with rustren v0.6.0+ (schema 1.2). Enforces whip-pan camera handoffs, clean UI framing, readable code/terminal typography, and subtle film texture.
---

# Product & UI Showcase (`rustren` Schema 1.2)

## 1. Trigger & Scope
Use this skill for **SaaS product launches, UI/dashboard tours, CLI/terminal walkthroughs, code diffs, and 2.5D card/cuboid carousels**.
- **Do NOT use** for abstract flat-vector kinetic openers (use `simple-motion`), 2.5D photo/comic dioramas (use `living-diorama`), or news/chart explainers (use `editorial-data`).

## 2. Non-Negotiable Quality Floor
1. **Readable UI & Code Scale**: Never shrink full-screen dashboards or code blocks below legible mobile/desktop sizes. Zoom into the active UI region (`world` camera `push_in` or `scale_x`/`scale_y`) so the viewer immediately sees the feature.
2. **Snappy Camera Handoffs (`whip_left` / `whip_right`)**: Transition between UI cards or feature panels using `world` camera `whip_left` / `whip_right` or `truck_right` with `world.motion_blur: 1.0..1.4` on `space: "world"` layers.
3. **2.5D Depth & Subtle Film Finish**: Give floating UI cards clean rounded corners (`radius: 20..32`), subtle `rotate_y` (`±6°–14°`) with `perspective: 900`, and optional subtle `grain_mode: "film"` (`grain: 0.04..0.08`) to avoid sterile flat renders.
4. **Simplicity Threshold**: Animate at most **2 properties** simultaneously per UI card (e.g., `x` + `rotate_y`, or `scale` + `opacity`). Avoid spinning or bouncing UI windows.
5. **Hero-First Staging**: Lead with the product outcome (0–2s), then show 2–3 focused feature beats (2–3s each), and close with a crisp brand/CTA end-card.

## 3. Exact `rustren` Schema 1.2 Recipe (Whip-Pan UI Handoff)
```json
{
  "schema_version": "1.2",
  "world": { "preset": "whip_left", "motion_blur": 1.2 },
  "layers": [
    { "type": "rect", "space": "world", "depth_slot": "subject", "x": 120, "y": 240, "width": 840, "height": 520, "radius": 24, "color": "#0f172a", "perspective": 900, "rotate_y": -8 }
  ]
}
```

## 4. Pre-Delivery Visual QA Gate
1. Run `rustren validate` (0 errors; check `SAFE_ZONE_9_16` if vertical).
2. Inspect a contact sheet covering each feature panel + whip transition peak: verify code/UI text is sharp at rest, motion blur collapses to `0px` on holds, and no UI card clips awkwardly at frame edges.
