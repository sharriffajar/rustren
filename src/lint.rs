use crate::schema::{AnimProperty, AnimValue, Layer, LayerKind, Scene};
use std::collections::BTreeMap;

/// A machine-readable lint warning: a stable `code`, the path to the offending
/// node (JSON-Pointer-ish), a human message, and an optional suggested fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintWarning {
    pub code: &'static str,
    /// Path of the representative (first) offending node.
    pub path: String,
    pub message: String,
    pub fix: Option<String>,
    /// Every offending path when the warning summarizes more than one node.
    /// Empty for single-node warnings, where `path` is already enough.
    pub affected_paths: Vec<String>,
}

impl LintWarning {
    /// Render as the human-facing string used by the plain-text CLI output.
    pub fn to_text(&self) -> String {
        self.message.clone()
    }
}

/// Collect grouped, profile-aware lint warnings for a scene (plain text).
pub fn collect_lint_warnings(scene: &Scene) -> Vec<String> {
    collect_lint_warnings_structured(scene)
        .into_iter()
        .map(|w| w.to_text())
        .collect()
}

/// Collect structured lint warnings for a scene.
pub fn collect_lint_warnings_structured(scene: &Scene) -> Vec<LintWarning> {
    let min_font_px = if scene.canvas.height > scene.canvas.width {
        26.0
    } else {
        14.0
    };
    let profile_label = if scene.canvas.height > scene.canvas.width {
        "9:16"
    } else {
        "landscape/square"
    };

    let mut groups: BTreeMap<&'static str, Vec<(String, String)>> = BTreeMap::new();
    let bg_rgb = parse_rgb(&scene.canvas.background).unwrap_or((0.0, 0.0, 0.0, 1.0));

    lint_layers(
        &scene.layers,
        scene.canvas.width as f32,
        scene.canvas.height as f32,
        scene.canvas.duration_secs,
        min_font_px,
        false,
        "layers",
        bg_rgb,
        &mut groups,
    );

    let mut out = Vec::new();
    for (category, items) in groups {
        if items.len() <= 3 {
            for (path, msg) in items {
                out.push(LintWarning {
                    code: category,
                    path,
                    message: msg,
                    fix: fix_for(category),
                    affected_paths: Vec::new(),
                });
            }
        } else {
            let preview: String = items[..2]
                .iter()
                .map(|(_, m)| m.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let path = items[0].0.clone();
            let summary = match category {
                "font_size" => format!(
                    "{} layers have font_size < {min_font_px}px ({profile_label} profile; e.g.: {preview})",
                    items.len()
                ),
                "end_sec" => format!(
                    "{} layers have end_sec > canvas.duration_secs (e.g.: {preview})",
                    items.len()
                ),
                "missing_max_width" => format!(
                    "{} text layers inside a container are missing max_width (e.g.: {preview})",
                    items.len()
                ),
                "out_of_canvas" => format!(
                    "{} layers are outside the canvas bounds (e.g.: {preview})",
                    items.len()
                ),
                "low_contrast" => format!(
                    "{} text layers have low color contrast (e.g.: {preview})",
                    items.len()
                ),
                _ => format!("{} {category} warnings (e.g.: {preview})", items.len()),
            };
            out.push(LintWarning {
                code: category,
                path,
                message: summary,
                fix: fix_for(category),
                affected_paths: items.iter().map(|(p, _)| p.clone()).collect(),
            });
        }
    }

    // Phase 1 pre-flight 2.5D QA: EDGE_REVEAL & PACING_STAGNATION
    collect_25d_preflight_qa(scene, &mut out);

    out
}

fn collect_25d_preflight_qa(scene: &Scene, out: &mut Vec<LintWarning>) {
    let cw = scene.canvas.width as f32;
    let ch = scene.canvas.height as f32;
    let dur = scene.canvas.duration_secs;

    if let Some(world) = &scene.world {
        for (i, layer) in scene.layers.iter().enumerate() {
            if layer.space != crate::schema::LayerSpace::World {
                continue;
            }
            if layer.depth_slot != Some(crate::schema::DepthSlot::Backdrop) {
                continue;
            }
            let Some((lx, ly, lw, lh)) = static_layer_bounds(layer) else {
                continue;
            };
            let layer_z = layer.effective_depth_z();
            for &sample_t in &[0.0, dur * 0.5, dur] {
                let cam = world.eval_at(sample_t, dur);
                let tf = crate::renderer::camera25d::compute_plane_camera_transform(
                    cam, layer_z, cw, ch,
                );
                let x0 = lx * tf.sx + tf.tx;
                let y0 = ly * tf.sy + tf.ty;
                let x1 = (lx + lw) * tf.sx + tf.tx;
                let y1 = (ly + lh) * tf.sy + tf.ty;
                if x0 > 1.0 || y0 > 1.0 || x1 < cw - 1.0 || y1 < ch - 1.0 {
                    out.push(LintWarning {
                        code: "EDGE_REVEAL",
                        path: format!("layers[{i}]"),
                        message: format!(
                            "layers[{i}] (backdrop) exposes canvas void at t={sample_t:.2}s under world camera (projected [{x0:.0}..{x1:.0}, {y0:.0}..{y1:.0}] vs {cw:.0}x{ch:.0})"
                        ),
                        fix: Some("overscale the backdrop plane (e.g. 1.15x) or reduce camera truck/zoom amplitude".to_string()),
                        affected_paths: Vec::new(),
                    });
                    break;
                }
            }
        }
    }

    // PACING_STAGNATION: warn when a >8s contiguous window has zero layer cues and zero animations
    if dur > 8.0 && !scene.layers.is_empty() {
        let mut events: Vec<f32> = vec![0.0, dur];
        let mut has_any_anim = scene
            .world
            .as_ref()
            .map(|w| w.preset.is_some() || !w.animations.is_empty())
            .unwrap_or(false);
        for l in &scene.layers {
            events.push(l.start_sec.clamp(0.0, dur));
            if let Some(e) = l.end_sec {
                events.push(e.clamp(0.0, dur));
            }
            if !l.animations.is_empty() {
                has_any_anim = true;
            }
        }
        if !has_any_anim {
            events.sort_by(|a, b| a.total_cmp(b));
            for win in events.windows(2) {
                let gap = win[1] - win[0];
                if gap > 8.0 {
                    out.push(LintWarning {
                        code: "PACING_STAGNATION",
                        path: "canvas".to_string(),
                        message: format!(
                            "Static hold of {gap:.1}s ({:.1}s..{:.1}s) with zero layer transitions or animations (>8s retention risk)",
                            win[0], win[1]
                        ),
                        fix: Some("add a camera preset, layer transition, or secondary motion every 3-6 seconds".to_string()),
                        affected_paths: Vec::new(),
                    });
                    break;
                }
            }
        }
    }

    // 9:16 Safe-Zone HUD check (TikTok / Reels / Shorts top 15% and bottom 22% UI danger zones)
    if ch > cw {
        let top_safe = ch * 0.15;
        let bot_safe = ch * 0.78;
        for (i, layer) in scene.layers.iter().enumerate() {
            if let LayerKind::Text { y, .. } = &layer.kind
                && (*y < top_safe || *y > bot_safe)
            {
                out.push(LintWarning {
                    code: "SAFE_ZONE_9_16",
                    path: format!("layers[{i}]"),
                    message: format!(
                        "layers[{i}] text at y={y:.0} is outside 9:16 safe band [{top_safe:.0}..{bot_safe:.0}] (risks TikTok/Reels HUD occlusion)"
                    ),
                    fix: Some("place 9:16 readable text within 25%-70% vertical band; keep top/bottom 25% for decorative shapes".to_string()),
                    affected_paths: Vec::new(),
                });
            }
        }
    }
}

/// Suggested remediation for a warning category, or `None` when there is no
/// single obvious fix.
fn fix_for(code: &str) -> Option<String> {
    match code {
        "font_size" => Some("increase font_size above the profile minimum".to_string()),
        "end_sec" => Some("lower end_sec to <= canvas.duration_secs".to_string()),
        "missing_max_width" => Some("add max_width to the text layer".to_string()),
        "out_of_canvas" => Some("move or resize the layer so it intersects the canvas".to_string()),
        "low_contrast" => Some("change the text color or the background behind it".to_string()),
        "static_monolithic_svg" => Some(
            "split into modular Container/Include rigs with pivot_x/pivot_y or use a curated Lottie layer with progress/color_overrides"
                .to_string(),
        ),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn lint_layers(
    layers: &[Layer],
    canvas_w: f32,
    canvas_h: f32,
    duration_secs: f32,
    min_font_px: f32,
    in_container: bool,
    prefix: &str,
    default_bg: (f32, f32, f32, f32),
    groups: &mut BTreeMap<&'static str, Vec<(String, String)>>,
) {
    for (i, layer) in layers.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        if let Some(end) = layer.end_sec
            && end > duration_secs + 1e-3
        {
            groups.entry("end_sec").or_default().push((
                path.clone(),
                format!("{path} end_sec ({end}) > canvas.duration_secs ({duration_secs})"),
            ));
        }

        if let Some((lx, ly, lw, lh)) = static_layer_bounds(layer)
            && !has_xy_animation(layer)
            && (lx + lw <= 0.0 || ly + lh <= 0.0 || lx >= canvas_w || ly >= canvas_h)
        {
            groups.entry("out_of_canvas").or_default().push((
                path.clone(),
                format!("{path} outside canvas bounds ({lx:.0},{ly:.0} {lw:.0}x{lh:.0})"),
            ));
        }

        match &layer.kind {
            LayerKind::Text {
                x,
                y,
                font_size,
                color,
                max_width,
                opacity,
                ..
            } => {
                if *font_size < min_font_px {
                    groups.entry("font_size").or_default().push((
                        path.clone(),
                        format!("{path} font_size ({font_size}) < {min_font_px}px"),
                    ));
                }
                if in_container && max_width.is_none() {
                    groups.entry("missing_max_width").or_default().push((
                        path.clone(),
                        format!("{path} text inside container missing max_width"),
                    ));
                }
                if *opacity >= 0.2
                    && let Some(fg) = parse_rgb(color)
                {
                    let t_check = (layer.start_sec + 0.05)
                        .min(layer.end_sec.unwrap_or(duration_secs).max(layer.start_sec));
                    let bg = find_effective_bg_at(layers, i, *x, *y, t_check, default_bg);
                    let ratio = contrast_ratio(fg, bg);
                    if ratio < 2.2 {
                        groups.entry("low_contrast").or_default().push((
                            path.clone(),
                            format!("{path} low contrast ({ratio:.2}:1, text {color})"),
                        ));
                    }
                }
            }
            LayerKind::Container {
                children,
                background,
                opacity,
                ..
            } => {
                let child_bg = if let Some(bg_hex) = background.as_deref()
                    && let Some((r, g, b, a)) = parse_rgb(bg_hex)
                {
                    let alpha = (a * (*opacity)).clamp(0.0, 1.0);
                    let inv = 1.0 - alpha;
                    (
                        r * alpha + default_bg.0 * inv,
                        g * alpha + default_bg.1 * inv,
                        b * alpha + default_bg.2 * inv,
                        1.0,
                    )
                } else {
                    default_bg
                };
                lint_layers(
                    children,
                    canvas_w,
                    canvas_h,
                    duration_secs,
                    min_font_px,
                    true,
                    &format!("{path}.children"),
                    child_bg,
                    groups,
                );
            }
            LayerKind::Svg { src, .. } => {
                let trimmed = src.trim_start();
                if trimmed.starts_with('<')
                    && layer.animations.is_empty()
                    && layer.wave_warp.is_none()
                    && layer.uv_warp.is_none()
                {
                    let shape_count = trimmed.matches("<path").count()
                        + trimmed.matches("<circle").count()
                        + trimmed.matches("<rect").count()
                        + trimmed.matches("<polygon").count()
                        + trimmed.matches("<ellipse").count();
                    if trimmed.len() > 2048 || shape_count >= 12 {
                        groups.entry("static_monolithic_svg").or_default().push((
                            path.clone(),
                            format!(
                                "{path} is a complex static monolithic SVG ({shape_count} shapes, {} bytes); split into modular Container/Include rigs or use a curated Lottie layer with progress/color_overrides",
                                trimmed.len()
                            ),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

fn has_xy_animation(layer: &Layer) -> bool {
    layer.animations.iter().any(|a| {
        matches!(
            a.property,
            AnimProperty::X | AnimProperty::Y | AnimProperty::Cx | AnimProperty::Cy
        )
    })
}

fn static_layer_bounds(layer: &Layer) -> Option<(f32, f32, f32, f32)> {
    match &layer.kind {
        LayerKind::Rect {
            x,
            y,
            width,
            height,
            ..
        } => Some((*x, *y, *width, *height)),
        LayerKind::Circle { cx, cy, r, .. } => Some((*cx - *r, *cy - *r, *r * 2.0, *r * 2.0)),
        LayerKind::Svg {
            x,
            y,
            width: Some(w),
            height: Some(h),
            ..
        }
        | LayerKind::Image {
            x,
            y,
            width: Some(w),
            height: Some(h),
            ..
        }
        | LayerKind::Video {
            x,
            y,
            width: Some(w),
            height: Some(h),
            ..
        }
        | LayerKind::Waveform {
            x,
            y,
            width: w,
            height: h,
            ..
        }
        | LayerKind::Particles {
            x,
            y,
            width: w,
            height: h,
            ..
        }
        | LayerKind::Lottie {
            x,
            y,
            width: Some(w),
            height: Some(h),
            ..
        }
        | LayerKind::Container {
            x,
            y,
            width: Some(w),
            height: Some(h),
            ..
        } => Some((*x, *y, *w, *h)),
        #[cfg(feature = "exp-3d")]
        LayerKind::Mesh3d {
            x,
            y,
            width,
            height,
            ..
        } => Some((*x, *y, *width, *height)),
        LayerKind::Text {
            x,
            y,
            font_size,
            max_width: Some(w),
            ..
        } => Some((*x, *y, *w, *font_size * 1.2)),
        _ => None,
    }
}

fn is_active_at(layer: &Layer, t: f32, duration_secs: f32) -> bool {
    let end = layer
        .end_sec
        .or_else(|| layer.duration_secs.map(|d| layer.start_sec + d))
        .unwrap_or(duration_secs);
    t >= layer.start_sec && t <= end
}

fn find_effective_bg_at(
    siblings: &[Layer],
    before_idx: usize,
    px: f32,
    py: f32,
    t: f32,
    default_bg: (f32, f32, f32, f32),
) -> (f32, f32, f32, f32) {
    for layer in siblings[..before_idx].iter().rev() {
        if !is_active_at(layer, t, f32::MAX) {
            continue;
        }
        match &layer.kind {
            LayerKind::Rect {
                x,
                y,
                width,
                height,
                color,
                opacity,
                ..
            } => {
                if *opacity >= 0.85
                    && px >= *x
                    && px <= *x + *width
                    && py >= *y
                    && py <= *y + *height
                    && let Some(c) = parse_rgb(color)
                    && c.3 >= 0.85
                {
                    return c;
                }
            }
            LayerKind::Circle {
                cx,
                cy,
                r,
                color,
                opacity,
                ..
            } => {
                let eff_r = layer
                    .animations
                    .iter()
                    .find(|a| a.property == AnimProperty::R && t >= a.end_sec)
                    .and_then(|a| match a.to {
                        AnimValue::Number(v) => Some(v),
                        AnimValue::Color(_) => None,
                    })
                    .unwrap_or(*r);
                let dx = px - *cx;
                let dy = py - *cy;
                if *opacity >= 0.85
                    && (dx * dx + dy * dy) <= eff_r * eff_r
                    && let Some(c) = parse_rgb(color)
                    && c.3 >= 0.85
                {
                    return c;
                }
            }
            LayerKind::Container {
                x,
                y,
                children,
                opacity,
                ..
            } if *opacity >= 0.85 => {
                let inner =
                    find_effective_bg_at(children, children.len(), px - *x, py - *y, t, default_bg);
                if inner != default_bg {
                    return inner;
                }
            }
            #[cfg(feature = "exp-3d")]
            LayerKind::Mesh3d { .. } => {}
            _ => {}
        }
    }
    default_bg
}

fn parse_rgb(hex: &str) -> Option<(f32, f32, f32, f32)> {
    let s = hex.trim().trim_start_matches('#');
    match s.len() {
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
            let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
            let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
            Some((r, g, b, 1.0))
        }
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
            let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
            let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
            let a = u8::from_str_radix(&s[6..8], 16).ok()? as f32 / 255.0;
            Some((r, g, b, a))
        }
        _ => None,
    }
}

fn linearize(c: f32) -> f32 {
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn rel_luminance(c: (f32, f32, f32, f32)) -> f32 {
    0.2126 * linearize(c.0) + 0.7152 * linearize(c.1) + 0.0722 * linearize(c.2)
}

fn contrast_ratio(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> f32 {
    let la = rel_luminance(a);
    let lb = rel_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Reject unknown JSON fields that `serde` would otherwise silently ignore,
/// keeping `rustren validate` aligned with `schema.json`'s `additionalProperties: false`.
pub fn validate_no_unknown_fields(raw: &str) -> anyhow::Result<()> {
    let val: serde_json::Value = serde_json::from_str(raw)?;
    let Some(root) = val.as_object() else {
        return Ok(());
    };
    check_allowed_keys(
        root,
        "root",
        &[
            "schema_version",
            "min_renderer_version",
            "canvas",
            "world",
            "profiles",
            "fonts",
            "audio",
            "layers",
        ],
    )?;
    if let Some(world) = root.get("world").and_then(|v| v.as_object()) {
        check_allowed_keys(
            world,
            "world",
            &[
                "x",
                "y",
                "z",
                "roll",
                "focus_z",
                "dof_strength",
                "preset",
                "animations",
            ],
        )?;
    }
    if let Some(profiles) = root.get("profiles").and_then(|v| v.as_object()) {
        for (name, prof) in profiles {
            if let Some(po) = prof.as_object() {
                check_allowed_keys(
                    po,
                    &format!("profiles.{name}"),
                    &["width", "height", "fps", "duration_secs", "background"],
                )?;
            }
        }
    }
    if let Some(canvas) = root.get("canvas").and_then(|v| v.as_object()) {
        check_allowed_keys(
            canvas,
            "canvas",
            &["width", "height", "fps", "duration_secs", "background"],
        )?;
    }
    if let Some(audio) = root.get("audio").and_then(|v| v.as_array()) {
        for (i, item) in audio.iter().enumerate() {
            if let Some(obj) = item.as_object() {
                check_allowed_keys(
                    obj,
                    &format!("audio[{i}]"),
                    &[
                        "src",
                        "start_sec",
                        "volume",
                        "trim_start_sec",
                        "trim_end_sec",
                        "playback_rate",
                        "fade_in_secs",
                        "fade_out_secs",
                        "animations",
                    ],
                )?;
            }
        }
    }
    if let Some(layers) = root.get("layers").and_then(|v| v.as_array()) {
        check_layers_unknown_fields(layers, "layers")?;
    }
    Ok(())
}

fn check_allowed_keys(
    obj: &serde_json::Map<String, serde_json::Value>,
    path: &str,
    allowed: &[&str],
) -> anyhow::Result<()> {
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            anyhow::bail!("Unknown field '{path}.{key}' (not defined in schema)");
        }
    }
    Ok(())
}

/// Fields accepted on every layer regardless of `type`.
pub(crate) const COMMON_LAYER_FIELDS: &[&str] = &[
    "name",
    "type",
    "start_sec",
    "end_sec",
    "duration_secs",
    "overlap_secs",
    "position",
    "flex_grow",
    "margin",
    "margin_top",
    "margin_right",
    "margin_bottom",
    "margin_left",
    "blend_mode",
    "blur",
    "shadow",
    "anchor",
    "scale_x",
    "scale_y",
    "pivot_x",
    "pivot_y",
    "skew_x",
    "skew_y",
    "rotate_x",
    "rotate_y",
    "perspective",
    "backface_visible",
    "z_index",
    "z",
    "grain",
    "grain_mode",
    "vignette",
    "halftone",
    "wave_warp",
    "track_matte",
    "matte_layer",
    "space",
    "depth_slot",
    "depth_z",
    "uv_warp",
    "relight",
    "echo",
    "animations",
];

/// Type-specific layer fields. Keep in sync with `schema.json` `$defs`
/// (`schema_field_parity_matches_lint` enforces this).
pub(crate) const LAYER_KIND_FIELDS: &[(&str, &[&str])] = &[
    (
        "rect",
        &[
            "x",
            "y",
            "width",
            "height",
            "radius",
            "color",
            "gradient",
            "border_color",
            "border_width",
            "path_progress",
            "stroke_dashoffset",
            "opacity",
            "scale",
            "rotation",
        ],
    ),
    (
        "circle",
        &[
            "cx",
            "cy",
            "r",
            "color",
            "gradient",
            "border_color",
            "border_width",
            "path_progress",
            "stroke_dashoffset",
            "opacity",
            "scale",
        ],
    ),
    (
        "text",
        &[
            "x",
            "y",
            "content",
            "dynamic",
            "spans",
            "font_size",
            "line_height",
            "letter_spacing",
            "font_family",
            "font_weight",
            "text_align",
            "direction",
            "color",
            "gradient",
            "outline",
            "opacity",
            "max_width",
            "scale",
            "text_animator",
        ],
    ),
    (
        "image",
        &[
            "x", "y", "width", "height", "src", "fit", "opacity", "rotation", "scale",
        ],
    ),
    (
        "svg",
        &[
            "x",
            "y",
            "width",
            "height",
            "src",
            "path_progress",
            "stroke_dashoffset",
            "stroke_dasharray",
            "opacity",
            "rotation",
        ],
    ),
    (
        "video",
        &[
            "x",
            "y",
            "width",
            "height",
            "src",
            "fit",
            "trim_start_sec",
            "trim_end_sec",
            "playback_rate",
            "loop_video",
            "loop",
            "focal_x",
            "focal_y",
            "luma_key",
            "volume",
            "muted",
            "opacity",
            "rotation",
            "scale",
        ],
    ),
    (
        "waveform",
        &[
            "x", "y", "width", "height", "bars", "bar_gap", "color", "seed", "speed", "opacity",
            "rotation", "scale",
        ],
    ),
    (
        "lottie",
        &[
            "x",
            "y",
            "width",
            "height",
            "src",
            "loop_animation",
            "speed",
            "progress",
            "color_overrides",
            "opacity",
            "rotation",
            "scale",
        ],
    ),
    (
        "particles",
        &[
            "x", "y", "width", "height", "count", "seed", "speed", "gravity", "spread", "angle",
            "size", "color", "colors", "shape", "opacity", "rotation", "scale",
        ],
    ),
    (
        "mesh3d",
        &[
            "x",
            "y",
            "width",
            "height",
            "primitive",
            "src",
            "texture",
            "opacity",
            "scale",
            "rotation",
            "rotate_x",
            "rotate_y",
            "rotate_z",
            "fov",
            "color",
            "light_dir",
            "wireframe",
        ],
    ),
    ("include", &["src", "vars", "variables"]),
    (
        "container",
        &[
            "x",
            "y",
            "width",
            "height",
            "layout",
            "background",
            "gradient",
            "border_color",
            "border_width",
            "radius",
            "opacity",
            "scale",
            "rotation",
            "clip",
            "clip_feather",
            "cascade_opacity",
            "timeline",
            "stagger_secs",
            "children",
        ],
    ),
];

fn check_layers_unknown_fields(layers: &[serde_json::Value], prefix: &str) -> anyhow::Result<()> {
    const COMMON: &[&str] = COMMON_LAYER_FIELDS;
    for (i, item) in layers.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        let Some(obj) = item.as_object() else {
            continue;
        };
        let kind = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let Some((_, extra)) = LAYER_KIND_FIELDS.iter().find(|(k, _)| *k == kind) else {
            continue;
        };
        for key in obj.keys() {
            if !COMMON.contains(&key.as_str()) && !extra.contains(&key.as_str()) {
                anyhow::bail!(
                    "Unknown field '{path}.{key}' on type '{kind}' (not defined in schema)"
                );
            }
        }
        if let Some(shadow) = obj.get("shadow").and_then(|v| v.as_object()) {
            check_allowed_keys(
                shadow,
                &format!("{path}.shadow"),
                &["color", "offset_x", "offset_y", "blur"],
            )?;
        }
        if let Some(grad) = obj.get("gradient").and_then(|v| v.as_object()) {
            check_allowed_keys(
                grad,
                &format!("{path}.gradient"),
                &["type", "from", "to", "stops", "angle"],
            )?;
            if let Some(stops) = grad.get("stops").and_then(|v| v.as_array()) {
                for (si, st) in stops.iter().enumerate() {
                    if let Some(so) = st.as_object() {
                        check_allowed_keys(
                            so,
                            &format!("{path}.gradient.stops[{si}]"),
                            &["offset", "color"],
                        )?;
                    }
                }
            }
        }
        if let Some(outline) = obj.get("outline").and_then(|v| v.as_object()) {
            check_allowed_keys(outline, &format!("{path}.outline"), &["color", "width"])?;
        }
        if let Some(layout) = obj.get("layout").and_then(|v| v.as_object()) {
            check_allowed_keys(
                layout,
                &format!("{path}.layout"),
                &[
                    "direction",
                    "flex_wrap",
                    "justify_content",
                    "align_items",
                    "gap",
                    "row_gap",
                    "column_gap",
                    "padding",
                    "padding_top",
                    "padding_right",
                    "padding_bottom",
                    "padding_left",
                ],
            )?;
        }
        if let Some(spans) = obj.get("spans").and_then(|v| v.as_array()) {
            for (si, sp) in spans.iter().enumerate() {
                if let Some(so) = sp.as_object() {
                    check_allowed_keys(
                        so,
                        &format!("{path}.spans[{si}]"),
                        &["text", "color", "font_weight", "font_family", "font_size"],
                    )?;
                }
            }
        }
        if let Some(anims) = obj.get("animations").and_then(|v| v.as_array()) {
            for (ai, an) in anims.iter().enumerate() {
                if let Some(ao) = an.as_object() {
                    check_allowed_keys(
                        ao,
                        &format!("{path}.animations[{ai}]"),
                        &[
                            "property",
                            "expr",
                            "from",
                            "to",
                            "start_sec",
                            "end_sec",
                            "keyframes",
                            "easing",
                            "bezier",
                            "spring",
                            "repeat",
                            "yoyo",
                        ],
                    )?;
                    if let Some(kfs) = ao.get("keyframes").and_then(|v| v.as_array()) {
                        for (ki, kf) in kfs.iter().enumerate() {
                            if let Some(ko) = kf.as_object() {
                                check_allowed_keys(
                                    ko,
                                    &format!("{path}.animations[{ai}].keyframes[{ki}]"),
                                    &["time", "value", "easing", "bezier"],
                                )?;
                            }
                        }
                    }
                    if let Some(sp) = ao.get("spring").and_then(|v| v.as_object()) {
                        check_allowed_keys(
                            sp,
                            &format!("{path}.animations[{ai}].spring"),
                            &["stiffness", "damping", "mass"],
                        )?;
                    }
                }
            }
        }
        if let Some(children) = obj.get("children").and_then(|v| v.as_array()) {
            check_layers_unknown_fields(children, &format!("{path}.children"))?;
        }
    }
    Ok(())
}

/// Flag scene patterns that can hurt cross-machine determinism (network assets,
/// system font fallbacks, or infinite repeat cycles).
#[allow(dead_code)]
pub fn collect_determinism_warnings(scene: &Scene) -> Vec<String> {
    collect_determinism_warnings_structured(scene)
        .into_iter()
        .map(|w| w.to_text())
        .collect()
}

/// Structured variant of [`collect_determinism_warnings`].
pub fn collect_determinism_warnings_structured(scene: &Scene) -> Vec<LintWarning> {
    let mut out = Vec::new();
    let mut has_text = false;
    for (i, audio) in scene.audio.iter().enumerate() {
        if audio.src.starts_with("http://") || audio.src.starts_with("https://") {
            let path = format!("audio[{i}]");
            out.push(LintWarning {
                code: "determinism_remote_url",
                path,
                message: format!("determinism: audio[{i}] uses remote URL '{}'", audio.src),
                fix: Some("use a local asset path for deterministic renders".to_string()),
                affected_paths: Vec::new(),
            });
        }
    }
    collect_layer_determinism(&scene.layers, "layers", &mut has_text, &mut out);
    if has_text && scene.fonts.is_empty() {
        out.push(LintWarning {
            code: "determinism_system_fonts",
            path: "fonts".to_string(),
            message: "determinism: fonts[] is empty; text rendering relies on host system fonts"
                .to_string(),
            fix: Some("bundle the fonts used by text layers in fonts[]".to_string()),
            affected_paths: Vec::new(),
        });
    }
    out
}

fn collect_layer_determinism(
    layers: &[Layer],
    prefix: &str,
    has_text: &mut bool,
    out: &mut Vec<LintWarning>,
) {
    for (i, layer) in layers.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        for (ai, anim) in layer.animations.iter().enumerate() {
            if matches!(
                anim.repeat,
                Some(crate::schema::RepeatSpec::Mode(
                    crate::schema::RepeatMode::Infinite
                ))
            ) {
                out.push(LintWarning {
                    code: "determinism_infinite_repeat",
                    path: format!("{path}.animations[{ai}]"),
                    message: format!(
                        "determinism: {path}.animations[{ai}] uses repeat='infinite' (unbounded cycle)"
                    ),
                    fix: Some("use a finite repeat count for deterministic output".to_string()),
                    affected_paths: Vec::new(),
                });
            }
        }
        match &layer.kind {
            LayerKind::Text { .. } => {
                *has_text = true;
            }
            LayerKind::Image { src, .. }
            | LayerKind::Video { src, .. }
            | LayerKind::Svg { src, .. } => {
                if src.starts_with("http://") || src.starts_with("https://") {
                    out.push(LintWarning {
                        code: "determinism_remote_url",
                        path: path.clone(),
                        message: format!("determinism: {path} uses remote URL '{src}'"),
                        fix: Some("use a local asset path for deterministic renders".to_string()),
                        affected_paths: Vec::new(),
                    });
                }
            }
            LayerKind::Container { children, .. } => {
                collect_layer_determinism(children, &format!("{path}.children"), has_text, out);
            }
            _ => {}
        }
    }
}

/// Check that local (non-URL) assets referenced by the scene exist on disk.
/// Remote URLs are left to the render-time download path and are only flagged
/// by [`collect_determinism_warnings_structured`].
pub fn check_local_assets(scene: &Scene, base_dir: Option<&std::path::Path>) -> Vec<LintWarning> {
    fn resolve(src: &str, base_dir: Option<&std::path::Path>) -> std::path::PathBuf {
        crate::assets::resolve_local_asset(src, base_dir)
    }
    fn is_remote(src: &str) -> bool {
        src.starts_with("http://") || src.starts_with("https://")
    }

    let mut out = Vec::new();

    let check = |src: &str, path: String, kind: &'static str, out: &mut Vec<LintWarning>| {
        if src.trim().is_empty() || is_remote(src) || src.trim_start().starts_with('<') {
            return;
        }
        let resolved = resolve(src, base_dir);
        if !resolved.exists() {
            out.push(LintWarning {
                code: "missing_asset",
                path,
                message: format!("{kind} not found: {}", resolved.display()),
                fix: Some(format!("create the file or fix the src path for {src}")),
                affected_paths: Vec::new(),
            });
        }
    };

    for (i, font) in scene.fonts.iter().enumerate() {
        check(font, format!("fonts[{i}]"), "font file", &mut out);
    }
    for (i, audio) in scene.audio.iter().enumerate() {
        check(&audio.src, format!("audio[{i}]"), "audio file", &mut out);
    }
    check_layer_assets(&scene.layers, "layers", base_dir, &mut out);
    out
}

fn check_layer_assets(
    layers: &[Layer],
    prefix: &str,
    base_dir: Option<&std::path::Path>,
    out: &mut Vec<LintWarning>,
) {
    fn resolve(src: &str, base_dir: Option<&std::path::Path>) -> std::path::PathBuf {
        crate::assets::resolve_local_asset(src, base_dir)
    }
    fn is_remote(src: &str) -> bool {
        src.starts_with("http://") || src.starts_with("https://")
    }

    for (i, layer) in layers.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        let (src, kind): (Option<&str>, &'static str) = match &layer.kind {
            LayerKind::Image { src, .. } => (Some(src), "image"),
            LayerKind::Svg { src, .. } => (Some(src), "svg"),
            LayerKind::Video { src, .. } => (Some(src), "video"),
            LayerKind::Lottie { src, .. } => (Some(src), "lottie"),
            #[cfg(feature = "exp-3d")]
            LayerKind::Mesh3d { src: Some(src), .. } => (Some(src), "3D model"),
            LayerKind::Container { children, .. } => {
                check_layer_assets(children, &format!("{path}.children"), base_dir, out);
                (None, "")
            }
            _ => (None, ""),
        };
        if let Some(src) = src
            && !src.trim().is_empty()
            && !is_remote(src)
            && !src.trim_start().starts_with('<')
        {
            let resolved = resolve(src, base_dir);
            if !resolved.exists() {
                out.push(LintWarning {
                    code: "missing_asset",
                    path: path.clone(),
                    message: format!("{kind} not found: {}", resolved.display()),
                    fix: Some(format!("create the file or fix the src path for {src}")),
                    affected_paths: Vec::new(),
                });
            }
        }

        // `Mesh3d.texture` is a second, independent asset on the same layer.
        #[cfg(feature = "exp-3d")]
        if let LayerKind::Mesh3d {
            texture: Some(texture),
            ..
        } = &layer.kind
            && !texture.trim().is_empty()
            && !is_remote(texture)
        {
            let resolved = resolve(texture, base_dir);
            if !resolved.exists() {
                out.push(LintWarning {
                    code: "missing_asset",
                    path: path.clone(),
                    message: format!("mesh texture not found: {}", resolved.display()),
                    fix: Some(format!(
                        "create the file or fix the texture path for {texture}"
                    )),
                    affected_paths: Vec::new(),
                });
            }
        }
    }
}
