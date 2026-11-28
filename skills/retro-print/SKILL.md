---
name: retro-print
description: Minimum quality floor for risograph prints, halftone dossiers, silkscreen posters, 1920s newsreels, and vintage film-textured videos built with rustren v0.6.0+. Enforces tactile halftone/film grain, ink-misregistration offsets, limited 2-3 ink palettes, and authentic print pacing.
---

# Retro Print & Film (`rustren` Schema 1.2)

## 1. Trigger & Scope
Use this skill for **tactile print & vintage film aesthetics**: Risograph prints, halftone case-file dossiers, silkscreen posters, 1920s newsreels, and silent-film title cards.
- **Do NOT use** for clean flat 2D kinetic promos (`simple-motion` forbids heavy texture/grain) or modern SaaS demos (`product-showcase`).

## 2. Style Presets (Pick One)
| Preset | Paper / Stock | Ink Palette (Max 3) | Texture Settings |
| :--- | :--- | :--- | :--- |
| **Risograph Print** | `#F4EFE6` | Fluorescent Pink `#FF48B0`, Risograph Blue `#0078BF`, Yellow `#FFE800` | `grain_mode: "film"`, `grain: 0.10`, `blend_mode: "multiply"` |
| **Halftone Dossier** | `#EAE3D2` | Charcoal `#1C1917`, Stamp Red `#B91C1C`, Ink Blue `#1D4ED8` | `halftone: 0.35`, `grain_mode: "film"`, `grain: 0.08`, `vignette: 0.25` |
| **1920s Newsreel / Silent Film** | `#141210` | Warm Ivory `#F5ECD7`, Sepia `#C2B280` | `grain_mode: "film"`, `grain: 0.18`, `vignette: 0.45` |

## 3. Non-Negotiable Quality Floor
1. **Strict 2–3 Ink Discipline**: Never use full-spectrum digital rainbows. Stick to 2–3 overprint inks + paper stock; use `blend_mode: "multiply"` so overlapping shapes create authentic overprint third colors.
2. **Intentional Misregistration**: Offset duplicate ink layers by `3–6px` (`x`/`y`) on stamps/headlines for tactile risograph/silkscreen registration feel.
3. **Tactile Texture Balance**: Apply `grain_mode: "film"` (`0.08–0.18`), `halftone` (`0.25–0.45`), and `vignette` (`0.20–0.45`) without crushing headline legibility (`>= 4.5:1` contrast).
4. **Print-Mechanical Pacing**: Use snappy stepped/stamp entrances (`150–250ms`, `ease_out`, `0–5%` overshoot) or classic title-card cuts rather than glossy modern rubbery bounce.

## 4. Pre-Delivery Visual QA Gate
1. Run `rustren validate` (0 errors).
2. Inspect a 6-frame contact sheet: verify halftone/film grain looks tactile (not muddy), overprint inks stay legible, and vignette does not darken edge typography.
