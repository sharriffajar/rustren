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
                    "{} layer memiliki font_size < {min_font_px}px (profil {profile_label}; contoh: {preview})",
                    items.len()
                ),
                "end_sec" => format!(
                    "{} layer memiliki end_sec > canvas.duration_secs (contoh: {preview})",
                    items.len()
                ),
                "missing_max_width" => format!(
                    "{} text layer di dalam container tanpa max_width (contoh: {preview})",
                    items.len()
                ),
                "out_of_canvas" => format!(
                    "{} layer berada di luar kanvas (contoh: {preview})",
                    items.len()
                ),
                "low_contrast" => format!(
                    "{} text layer memiliki kontras warna rendah (contoh: {preview})",
                    items.len()
                ),
                _ => format!("{} peringatan {category} (contoh: {preview})", items.len()),
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
                    "{path} di luar kanvas ({lx:.0},{ly:.0} {lw:.0}x{lh:.0})"
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
                        .push(format!("{path} text di dalam container tanpa max_width"));
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
                                "{path} kontras rendah ({ratio:.2}:1, teks {color})"
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
