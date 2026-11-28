---
name: editorial-data
description: Minimum quality floor for news reels, data storytelling, animated charts, lower-thirds, and narrated caption videos built with rustren. Enforces strict typographic hierarchy, 9:16 safe-zone compliance, readable contrast, and clean voiceover/caption pacing.
---

# Editorial & Data Storytelling (`rustren`)

## 1. Trigger & Scope
Use this skill for **information-first videos**: news explainers, stat/chart breakdowns, lower-third broadcast graphics, and narrated TikTok/Reels/Shorts with synchronized captions.
- **Do NOT use** for abstract flat-vector kinetic openers (use `simple-motion`), 2.5D photo dioramas (use `living-diorama`), or SaaS UI demos (use `product-showcase`).

## 2. Non-Negotiable Quality Floor
1. **Strict Visual Hierarchy**: Never exceed **12 words** per headline card. Separate **Kicker / Tag** (`24–28px`, uppercase accent), **Headline** (`56–78px`, bold), and **Caption / Body** (`34–44px`) with clear vertical spacing.
2. **High-Contrast Legibility (`>= 4.5:1`)**: Always place text over a solid card, dark scrim, or high-contrast paper background. Never drop raw white text onto bright footage without a scrim (`clip_feather` or translucent `rect`).
3. **9:16 Safe-Zone Discipline**: Keep all headlines, charts, and captions strictly inside the **15%–78% vertical band** (`SAFE_ZONE_9_16`) and **75% inner width** so TikTok/Reels/Shorts UI buttons never cover copy.
4. **Corporate / Broadcast Motion Personality**: Use crisp, zero-clutter motion (`200–400ms`, `cubic_bezier(0.2, 0, 0, 1)`, `0–3%` overshoot). Bars, numbers, and captions should snap cleanly without excessive cartoon bounce.
5. **Audio & Caption Sync**: Align visual beats and `text_animator` / karaoke highlights to speech phrasing (~1–2.5s per caption chunk). Use audio ducking when pairing background music with voiceover.
6. **Built-in Style Presets**:
   - **Caption News (`caption-news`)**: Fast 9:16 news cards, bold lower-thirds, high-contrast karaoke captions inside `SAFE_ZONE_9_16`.
   - **Data Storytelling (`dataviz`)**: Animated bars, stat counters, and clean grid callouts on `#F8FAFC` or `#0F172A`.
   - **Visual Investigation (`visual-investigation`)**: Vox/NYT documentary style — highlighted document crops, drawn callout arrows (`path_progress`), and `world` pan/zoom.

## 3. Exact `rustren` Patterns
- **Animated Stat Counter / Bar Reveal**: Drive `width` or `scale_x` (`pivot_x: 0.0`) with `ease_out` (`0.4–0.6s`) + synchronized `text` dynamic counter.
- **Lower-Third Slide**: Mask a 2-line lower-third card with `track_matte: "alpha"` rect so text glides cleanly out of a vertical accent bar.
- **Scrim Over Footage**: Place a dark `rect` (`#0f172acc`) or feathered container (`clip_feather`) behind captions over `video`/`image` layers.

## 4. Pre-Delivery Visual QA Gate
1. Run `rustren validate` and confirm **0 `SAFE_ZONE_9_16`** and **0 low-contrast** warnings on readable text.
2. Inspect a 6-frame contact sheet across intro, peak data chart, caption swap, and outro: verify zero text clipping, zero overlapping caption lines, and instant scannability at mobile size.
