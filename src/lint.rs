use crate::schema::{AnimProperty, AnimValue, Layer, LayerKind, Scene};
use std::collections::BTreeMap;

/// Collect grouped, profile-aware lint warnings for a scene.
pub fn collect_lint_warnings(scene: &Scene) -> Vec<String> {
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

    let mut groups: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
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
            out.extend(items);
        } else {
            let preview = items[..2].join("; ");
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
            out.push(summary);
        }
    }
    out
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
    groups: &mut BTreeMap<&'static str, Vec<String>>,
) {
    for (i, layer) in layers.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        if let Some(end) = layer.end_sec {
            if end > duration_secs + 1e-3 {
                groups.entry("end_sec").or_default().push(format!(
                    "{path} end_sec ({end}) > canvas.duration_secs ({duration_secs})"
                ));
            }
        }

        if let Some((lx, ly, lw, lh)) = static_layer_bounds(layer) {
            if !has_xy_animation(layer)
                && (lx + lw <= 0.0 || ly + lh <= 0.0 || lx >= canvas_w || ly >= canvas_h)
            {
                groups.entry("out_of_canvas").or_default().push(format!(
                    "{path} outside canvas bounds ({lx:.0},{ly:.0} {lw:.0}x{lh:.0})"
                ));
            }
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
                    groups.entry("font_size").or_default().push(format!(
                        "{path} font_size ({font_size}) < {min_font_px}px"
                    ));
                }
                if in_container && max_width.is_none() {
                    groups
                        .entry("missing_max_width")
                        .or_default()
                        .push(format!("{path} text inside container missing max_width"));
                }
                if *opacity >= 0.2 {
                    if let Some(fg) = parse_rgb(color) {
                        let t_check = (layer.start_sec + 0.05).min(
                            layer.end_sec.unwrap_or(duration_secs).max(layer.start_sec),
                        );
                        let bg = find_effective_bg_at(layers, i, *x, *y, t_check, default_bg);
                        let ratio = contrast_ratio(fg, bg);
                        if ratio < 2.2 {
                            groups.entry("low_contrast").or_default().push(format!(
                                "{path} low contrast ({ratio:.2}:1, text {color})"
                            ));
                        }
                    }
                }
            }
            LayerKind::Container { children, .. } => {
                lint_layers(
                    children,
                    canvas_w,
                    canvas_h,
                    duration_secs,
                    min_font_px,
                    true,
                    &format!("{path}.children"),
                    default_bg,
                    groups,
                );
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
        | LayerKind::Container {
            x,
            y,
            width: Some(w),
            height: Some(h),
            ..
        } => Some((*x, *y, *w, *h)),
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
                if *opacity >= 0.85 && px >= *x && px <= *x + *width && py >= *y && py <= *y + *height
                {
                    if let Some(c) = parse_rgb(color) {
                        if c.3 >= 0.85 {
                            return c;
                        }
                    }
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
                if *opacity >= 0.85 && (dx * dx + dy * dy) <= eff_r * eff_r {
                    if let Some(c) = parse_rgb(color) {
                        if c.3 >= 0.85 {
                            return c;
                        }
                    }
                }
            }
            LayerKind::Container {
                x,
                y,
                children,
                opacity,
                ..
            } => {
                if *opacity >= 0.85 {
                    let inner = find_effective_bg_at(
                        children,
                        children.len(),
                        px - *x,
                        py - *y,
                        t,
                        default_bg,
                    );
                    if inner != default_bg {
                        return inner;
                    }
                }
            }
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
        &["schema_version", "min_renderer_version", "canvas", "fonts", "audio", "layers"],
    )?;
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
                    &["src", "start_sec", "volume", "trim_start_sec", "trim_end_sec", "playback_rate", "fade_in_secs", "fade_out_secs", "animations"],
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

fn check_layers_unknown_fields(layers: &[serde_json::Value], prefix: &str) -> anyhow::Result<()> {
    const COMMON: &[&str] = &[
        "name", "type", "start_sec", "end_sec", "duration_secs", "overlap_secs", "position",
        "flex_grow", "margin", "blend_mode", "blur", "shadow", "anchor", "skew_x", "skew_y",
        "animations",
    ];
    for (i, item) in layers.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        let Some(obj) = item.as_object() else {
            continue;
        };
        let kind = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let extra: &[&str] = match kind {
            "rect" => &[
                "x", "y", "width", "height", "radius", "color", "gradient", "border_color",
                "border_width", "path_progress", "stroke_dashoffset", "opacity", "scale",
                "rotation",
            ],
            "circle" => &[
                "cx", "cy", "r", "color", "border_color", "border_width", "path_progress",
                "stroke_dashoffset", "opacity", "scale",
            ],
            "text" => &[
                "x", "y", "content", "dynamic", "spans", "font_size", "line_height",
                "letter_spacing", "font_family", "font_weight", "text_align", "color",
                "gradient", "outline", "opacity", "max_width", "scale",
            ],
            "image" => &["x", "y", "width", "height", "src", "fit", "opacity", "rotation", "scale"],
            "svg" => &[
                "x", "y", "width", "height", "src", "path_progress", "stroke_dashoffset",
                "stroke_dasharray", "opacity", "rotation",
            ],
            "video" => &[
                "x", "y", "width", "height", "src", "fit", "trim_start_sec", "trim_end_sec",
                "playback_rate", "volume", "muted", "opacity", "rotation", "scale", "bars", "bar_gap", "seed", "speed",
            ],
            "container" => &[
                "x", "y", "width", "height", "layout", "background", "gradient", "border_color",
                "border_width", "radius", "opacity", "scale", "rotation", "clip",
                "cascade_opacity", "timeline", "stagger_secs", "children",
            ],
            _ => continue,
        };
        for key in obj.keys() {
            if !COMMON.contains(&key.as_str()) && !extra.contains(&key.as_str()) {
                anyhow::bail!("Unknown field '{path}.{key}' on type '{kind}' (not defined in schema)");
            }
        }
        if let Some(shadow) = obj.get("shadow").and_then(|v| v.as_object()) {
            check_allowed_keys(shadow, &format!("{path}.shadow"), &["color", "offset_x", "offset_y", "blur"])?;
        }
        if let Some(grad) = obj.get("gradient").and_then(|v| v.as_object()) {
            check_allowed_keys(grad, &format!("{path}.gradient"), &["type", "from", "to", "stops", "angle"])?;
            if let Some(stops) = grad.get("stops").and_then(|v| v.as_array()) {
                for (si, st) in stops.iter().enumerate() {
                    if let Some(so) = st.as_object() {
                        check_allowed_keys(so, &format!("{path}.gradient.stops[{si}]"), &["offset", "color"])?;
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
                &["direction", "justify_content", "align_items", "gap", "padding"],
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
                            "property", "from", "to", "start_sec", "end_sec", "keyframes",
                            "easing", "bezier", "spring", "repeat", "yoyo",
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
pub fn collect_determinism_warnings(scene: &Scene) -> Vec<String> {
    let mut out = Vec::new();
    let mut has_text = false;
    for (i, audio) in scene.audio.iter().enumerate() {
        if audio.src.starts_with("http://") || audio.src.starts_with("https://") {
            out.push(format!("determinism: audio[{i}] uses remote URL '{}'", audio.src));
        }
    }
    collect_layer_determinism(&scene.layers, "layers", &mut has_text, &mut out);
    if has_text && scene.fonts.is_empty() {
        out.push("determinism: fonts[] is empty; text rendering relies on host system fonts".to_string());
    }
    out
}

fn collect_layer_determinism(
    layers: &[Layer],
    prefix: &str,
    has_text: &mut bool,
    out: &mut Vec<String>,
) {
    for (i, layer) in layers.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        for (ai, anim) in layer.animations.iter().enumerate() {
            if matches!(anim.repeat, Some(crate::schema::RepeatSpec::Mode(crate::schema::RepeatMode::Infinite))) {
                out.push(format!(
                    "determinism: {path}.animations[{ai}] uses repeat='infinite' (unbounded cycle)"
                ));
            }
        }
        match &layer.kind {
            LayerKind::Text { .. } => {
                *has_text = true;
            }
            LayerKind::Image { src, .. } | LayerKind::Video { src, .. } | LayerKind::Svg { src, .. } => {
                if src.starts_with("http://") || src.starts_with("https://") {
                    out.push(format!("determinism: {path} uses remote URL '{src}'"));
                }
            }
            LayerKind::Container { children, .. } => {
                collect_layer_determinism(children, &format!("{path}.children"), has_text, out);
            }
            _ => {}
        }
    }
}
