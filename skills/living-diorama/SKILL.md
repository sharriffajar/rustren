---
name: living-diorama
description: Minimum quality floor for 2.5D living dioramas, motion comics, and cinematic photo/illustration dramatizations built with rustren v0.6.0+ (schema 1.2). Enforces world-space depth planes, overscaled backdrops, rack-focus DoF, rigidity-masked uv_warp, 2D normal relighting, and zero-edge-reveal QA.
---

# Living Diorama (`rustren` Schema 1.2)

## 1. Trigger & Scope
Use this skill when animating **still images, photos, or layered illustrations** into cinematic 2.5D motion comics, atmospheric slideshows, or parallax dioramas.
- **Do NOT use** for flat 2D kinetic typography promos (use `simple-motion`), data/news explainers (use `editorial-data`), or SaaS UI demos (use `product-showcase`).

## 2. Non-Negotiable Quality Floor
1. **Overscale World Backdrops (`>= 115–125%`)**: Every `space: "world"` background/backdrop layer MUST be overscaled and centered so camera moves (`push_in`, `truck_left`, `truck_right`, `whip_left`, `whip_right`) never expose canvas edges (`lint` flags `EDGE_REVEAL`).
2. **True Depth Separation (`>= 3` Planes)**: Separate every shot into at least `backdrop` (`depth_slot: "backdrop"`), `subject` (`depth_slot: "subject"`), and `foreground` / `screen` overlay (`space: "screen"` for crisp HUD/subtitles).
3. **Purposeful Rack Focus & Motion Personality**: Default to **Premium / Cinematic Slow-Burn** (`350–600ms` transitions, `0%` overshoot, smooth `ease_in_out` or `cubic_bezier(0.4, 0, 0.2, 1)`). Use `focus_z` + `dof_strength` (`0.8–1.6`) to guide the viewer's eye, not to blur the hero subject.
4. **Living Micro-Motion (Anti-Slideshow)**: Never hold a still image frozen for >3s (`PACING_STAGNATION`). Combine slow camera drift with subtle `uv_warp` (hair, foliage, fabric, water) or flickering `relight` (lanterns, neon, sunbeams).
5. **Three Motion Layers**: **Primary** (camera move / rack focus), **Secondary** (parallax plane separation), **Ambient** (`uv_warp`, `relight` flicker, `grain_mode: "film"`).
6. **Built-in Style Presets**:
   - **Paper-cut Lightbox (`paper-lantern`)**: Multi-plane silhouette cutouts (`depth_slot: backdrop/subject/foreground`) + `dof_strength: 1.2` + warm `relight` backlight.
   - **Archive Light Table (`archive-lighttable`)**: Overhead 2.5D `world` camera (`truck_right`/`push_in`) gliding over archival photos/documents with subtle `grain_mode: "film"`.

## 3. Exact `rustren` Schema 1.2 Recipes

### 3.1 2.5D World Camera + Depth Planes
```json
{
  "schema_version": "1.2",
  "world": { "preset": "truck_right", "focus_z": 0.25, "dof_strength": 1.2 },
  "layers": [
    { "type": "image", "src": "bg.png", "space": "world", "depth_slot": "backdrop", "x": -120, "y": -80, "width": 1520, "height": 880 },
    { "type": "image", "src": "hero.png", "space": "world", "depth_slot": "subject", "x": 380, "y": 120, "width": 520, "height": 580 }
  ]
}
```

### 3.2 Living Raster (`uv_warp` + 2D `relight`)
```json
{
  "type": "image", "src": "hero.png", "space": "world", "depth_slot": "subject",
  "x": 380, "y": 120, "width": 520, "height": 580,
  "uv_warp": { "amplitude_x": 6.0, "amplitude_y": 2.0, "freq": 0.8, "wavelength": 48.0, "mask_src": "rigidity_mask.png" },
  "relight": {
    "ambient": 0.25,
    "normal_src": "hero_normal.png",
    "lights": [{ "x": 0.7, "y": 0.3, "z": 0.35, "color": "#ffaa44", "intensity": 1.3, "radius": 0.65, "flicker_amp": 0.2, "flicker_freq": 2.5 }]
  }
}
```

## 4. Pre-Delivery Visual QA Gate
1. Run `rustren validate` and confirm **0 `EDGE_REVEAL`** and **0 `PACING_STAGNATION`** warnings.
2. Render a 4-frame contact sheet (`0%`, `33%`, `66%`, `95%` of shot) and verify: no black/empty border slivers at camera extremes, hero subject stays sharp at focal plane, and `screen`-space subtitles remain crisp.
