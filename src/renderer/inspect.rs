use anyhow::Result;
use cosmic_text::FontSystem;

use crate::animation::eval_property;
use crate::assets::AssetStore;
use crate::schema::{AnimProperty, Layer, LayerKind, Scene, TextSpan};

use super::layout::{
    apply_anchor, compute_flex_layout, effective_end_sec, resolve_child_timelines, resolve_xy,
    LayoutBox,
};
use super::text::{measure_text_size_and_lines, resolve_dynamic_text, resolve_text_box_width};
use super::with_thread_font_system;

#[derive(Debug, Clone, serde::Serialize)]
pub struct TextMeasureInfo {
    pub width: f32,
    pub height: f32,
    pub line_count: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LayerLayoutInfo {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub layer_type: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub opacity: f32,
    pub in_canvas: bool,
    pub clipped_by_canvas: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_measured: Option<TextMeasureInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<LayerLayoutInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutDiagnosticKind {
    CanvasOutOfBounds,
    CanvasEdgeClipped,
    ContainerOverflow,
    SiblingOverlap,
    UnexpectedTextWrap,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LayoutDiagnostic {
    pub kind: LayoutDiagnosticKind,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub message: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Evaluate computed bounding boxes and measured text dimensions at `time_sec`.
pub fn inspect_layout_at_time(
    scene: &Scene,
    assets: &AssetStore,
    time_sec: f32,
) -> Result<Vec<LayerLayoutInfo>> {
    with_thread_font_system(assets, |font_system, _| {
        let mut out = Vec::new();
        for (i, layer) in scene.layers.iter().enumerate() {
            if let Some(info) = inspect_layer_layout(
                layer,
                &format!("layers[{i}]"),
                scene.canvas.duration_secs,
                time_sec,
                assets,
                font_system,
                None,
                1.0,
                false,
                scene.canvas.width as f32,
                scene.canvas.height as f32,
            )? {
                out.push(info);
            }
        }
        Ok(out)
    })
}

/// Run automated layout troubleshooting diagnostics at `time_sec` (overlaps, clipping, overflow, unexpected text wraps).
pub fn diagnose_layout_at_time(
    scene: &Scene,
    assets: &AssetStore,
    time_sec: f32,
) -> Result<Vec<LayoutDiagnostic>> {
    let layout = inspect_layout_at_time(scene, assets, time_sec)?;
    let mut diagnostics = Vec::new();
    let canvas_w = scene.canvas.width as f32;
    let canvas_h = scene.canvas.height as f32;

    collect_diagnostics_for_siblings(scene, &layout, None, canvas_w, canvas_h, &mut diagnostics);
    Ok(diagnostics)
}

fn find_layer_by_path<'a>(scene: &'a Scene, path: &str) -> Option<&'a Layer> {
    let rest = path.strip_prefix("layers[")?;
    let mut parts = rest.split(".children[");
    let root_idx: usize = parts.next()?.strip_suffix(']')?.parse().ok()?;
    let mut current = scene.layers.get(root_idx)?;
    for part in parts {
        let idx: usize = part.strip_suffix(']')?.parse().ok()?;
        match &current.kind {
            LayerKind::Container { children, .. } => {
                current = children.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

fn is_container_backdrop(layer: &LayerLayoutInfo, container_w: f32, container_h: f32) -> bool {
    matches!(layer.layer_type.as_str(), "rect" | "circle")
        && layer.w >= container_w * 0.8
        && layer.h >= container_h * 0.8
}

fn collect_diagnostics_for_siblings(
    scene: &Scene,
    siblings: &[LayerLayoutInfo],
    parent: Option<&LayerLayoutInfo>,
    container_w: f32,
    container_h: f32,
    out: &mut Vec<LayoutDiagnostic>,
) {
    for child in siblings {
        if child.opacity <= 0.05 {
            continue;
        }

        if !child.in_canvas {
            out.push(LayoutDiagnostic {
                kind: LayoutDiagnosticKind::CanvasOutOfBounds,
                path: child.path.clone(),
                name: child.name.clone(),
                message: format!(
                    "Layer {} ({}) is completely outside canvas bounds ({:.1},{:.1} {:.1}x{:.1})",
                    child.path, child.layer_type, child.x, child.y, child.w, child.h
                ),
                x: child.x,
                y: child.y,
                w: child.w,
                h: child.h,
            });
        } else if (child.layer_type == "text" || child.layer_type == "container")
            && child.clipped_by_canvas
        {
            out.push(LayoutDiagnostic {
                kind: LayoutDiagnosticKind::CanvasEdgeClipped,
                path: child.path.clone(),
                name: child.name.clone(),
                message: format!(
                    "{} layer {} extends past canvas edge ({:.1},{:.1} {:.1}x{:.1})",
                    child.layer_type, child.path, child.x, child.y, child.w, child.h
                ),
                x: child.x,
                y: child.y,
                w: child.w,
                h: child.h,
            });
        }

        if let Some(p) = parent
            && (child.x < p.x - 1.5
                || child.y < p.y - 1.5
                || child.x + child.w > p.x + p.w + 1.5
                || child.y + child.h > p.y + p.h + 1.5)
            {
                out.push(LayoutDiagnostic {
                    kind: LayoutDiagnosticKind::ContainerOverflow,
                    path: child.path.clone(),
                    name: child.name.clone(),
                    message: format!(
                        "Child {} ({:.1},{:.1} {:.1}x{:.1}) overflows parent {} ({:.1},{:.1} {:.1}x{:.1})",
                        child.path, child.x, child.y, child.w, child.h,
                        p.path, p.x, p.y, p.w, p.h
                    ),
                    x: child.x,
                    y: child.y,
                    w: child.w,
                    h: child.h,
                });
            }

        if child.layer_type == "text"
            && let Some(tm) = &child.text_measured
                && tm.line_count > 1
                    && let Some(layer) = find_layer_by_path(scene, &child.path)
                        && let LayerKind::Text {
                            content,
                            dynamic,
                            max_width,
                            ..
                        } = &layer.kind
                            && dynamic.is_none() && !content.contains('\n') && max_width.is_none() {
                                out.push(LayoutDiagnostic {
                                    kind: LayoutDiagnosticKind::UnexpectedTextWrap,
                                    path: child.path.clone(),
                                    name: child.name.clone(),
                                    message: format!(
                                        "Text {} wrapped into {} lines without explicit max_width or newline ({:.1}x{:.1})",
                                        child.path, tm.line_count, child.w, child.h
                                    ),
                                    x: child.x,
                                    y: child.y,
                                    w: child.w,
                                    h: child.h,
                                });
                            }

        if !child.children.is_empty() {
            collect_diagnostics_for_siblings(
                scene,
                &child.children,
                Some(child),
                child.w,
                child.h,
                out,
            );
        }
    }

    for i in 0..siblings.len() {
        let a = &siblings[i];
        if a.opacity <= 0.05 || a.w <= 0.0 || a.h <= 0.0 || is_container_backdrop(a, container_w, container_h) {
            continue;
        }
        for b in &siblings[(i + 1)..] {
            if b.opacity <= 0.05 || b.w <= 0.0 || b.h <= 0.0 || is_container_backdrop(b, container_w, container_h) {
                continue;
            }
            if a.layer_type != "text" && b.layer_type != "text" {
                continue;
            }
            let overlap_x0 = a.x.max(b.x);
            let overlap_y0 = a.y.max(b.y);
            let overlap_x1 = (a.x + a.w).min(b.x + b.w);
            let overlap_y1 = (a.y + a.h).min(b.y + b.h);
            let overlap_w = overlap_x1 - overlap_x0;
            let overlap_h = overlap_y1 - overlap_y0;
            let min_h_tol = if a.layer_type == "text" && b.layer_type == "text" {
                (a.h.min(b.h) * 0.2).clamp(2.0, 16.0)
            } else {
                2.0
            };
            if overlap_w > 2.0 && overlap_h > min_h_tol {
                let target = if b.layer_type == "text" { b } else { a };
                out.push(LayoutDiagnostic {
                    kind: LayoutDiagnosticKind::SiblingOverlap,
                    path: target.path.clone(),
                    name: target.name.clone(),
                    message: format!(
                        "Sibling overlap ({:.1}x{:.1}px) between {} ({}) and {} ({})",
                        overlap_w, overlap_h, a.path, a.layer_type, b.path, b.layer_type
                    ),
                    x: overlap_x0,
                    y: overlap_y0,
                    w: overlap_w,
                    h: overlap_h,
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn inspect_layer_layout(
    layer: &Layer,
    path: &str,
    scene_duration: f32,
    time_sec: f32,
    assets: &AssetStore,
    font_system: &mut FontSystem,
    layout_override: Option<LayoutBox>,
    parent_opacity: f32,
    skip_window: bool,
    canvas_w: f32,
    canvas_h: f32,
) -> Result<Option<LayerLayoutInfo>> {
    if !skip_window {
        let end_sec = effective_end_sec(layer, scene_duration);
        if time_sec < layer.start_sec || time_sec > end_sec {
            return Ok(None);
        }
    }
    let local_t = time_sec;

    let boxes_intersect_canvas = |x: f32, y: f32, w: f32, h: f32| -> bool {
        x + w > 0.0 && y + h > 0.0 && x < canvas_w && y < canvas_h
    };
    let boxes_clipped_by_canvas = |x: f32, y: f32, w: f32, h: f32| -> bool {
        x < -0.5 || y < -0.5 || x + w > canvas_w + 0.5 || y + h > canvas_h + 0.5
    };

    let info = match &layer.kind {
        LayerKind::Rect {
            x,
            y,
            width,
            height,
            opacity,
            scale,
            ..
        } => {
            let (rx, ry) = resolve_xy(layer, layout_override, *x, *y, local_t);
            let base_w = layout_override.map(|b| b.w).unwrap_or(*width);
            let base_h = layout_override.map(|b| b.h).unwrap_or(*height);
            let s = eval_property(&layer.animations, AnimProperty::Scale, local_t, *scale);
            let w = eval_property(&layer.animations, AnimProperty::Width, local_t, base_w) * s;
            let h = eval_property(&layer.animations, AnimProperty::Height, local_t, base_h) * s;
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "rect".to_string(),
                x: rx,
                y: ry,
                w,
                h,
                opacity: op,
                in_canvas: boxes_intersect_canvas(rx, ry, w, h),
                clipped_by_canvas: boxes_clipped_by_canvas(rx, ry, w, h),
                text_measured: None,
                children: Vec::new(),
            }
        }
        LayerKind::Circle {
            cx,
            cy,
            r,
            opacity,
            scale,
            ..
        } => {
            let cx_base_offset = layout_override.map(|b| b.x + b.w * 0.5 - *cx).unwrap_or(0.0);
            let cy_base_offset = layout_override.map(|b| b.y + b.h * 0.5 - *cy).unwrap_or(0.0);
            let eff_cx = cx_base_offset
                + eval_property(&layer.animations, AnimProperty::Cx, local_t, *cx)
                + eval_property(&layer.animations, AnimProperty::X, local_t, 0.0);
            let eff_cy = cy_base_offset
                + eval_property(&layer.animations, AnimProperty::Cy, local_t, *cy)
                + eval_property(&layer.animations, AnimProperty::Y, local_t, 0.0);
            let s = eval_property(&layer.animations, AnimProperty::Scale, local_t, *scale);
            let eff_r = eval_property(&layer.animations, AnimProperty::R, local_t, *r) * s;
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            let (bx, by, bw, bh) = (eff_cx - eff_r, eff_cy - eff_r, eff_r * 2.0, eff_r * 2.0);
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "circle".to_string(),
                x: bx,
                y: by,
                w: bw,
                h: bh,
                opacity: op,
                in_canvas: boxes_intersect_canvas(bx, by, bw, bh),
                clipped_by_canvas: boxes_clipped_by_canvas(bx, by, bw, bh),
                text_measured: None,
                children: Vec::new(),
            }
        }
        LayerKind::Text {
            x,
            y,
            content,
            dynamic,
            spans,
            font_size,
            line_height,
            font_family,
            font_weight,
            text_align,
            max_width,
            letter_spacing,
            opacity,
            scale,
            ..
        } => {
            let (tx, ty) = resolve_xy(layer, layout_override, *x, *y, local_t);
            let effective_max_w = max_width.or_else(|| layout_override.map(|b| b.w));
            let s = eval_property(&layer.animations, AnimProperty::Scale, local_t, *scale);
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            let resolved_content =
                resolve_dynamic_text(layer, content, dynamic.as_ref(), local_t, scene_duration);
            let effective_spans: &[TextSpan] = if dynamic.is_some() { &[] } else { spans };
            let (mw, mh, line_count) = measure_text_size_and_lines(
                font_system,
                &resolved_content,
                effective_spans,
                *font_size,
                *line_height,
                *letter_spacing,
                font_family.as_deref(),
                *font_weight,
                effective_max_w,
                s,
            );
            let bw = resolve_text_box_width(layout_override, effective_max_w, mw, *text_align);
            let (tx, ty) = apply_anchor(tx, ty, bw, mh, layer.anchor);
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "text".to_string(),
                x: tx,
                y: ty,
                w: bw,
                h: mh,
                opacity: op,
                in_canvas: boxes_intersect_canvas(tx, ty, bw, mh),
                clipped_by_canvas: boxes_clipped_by_canvas(tx, ty, bw, mh),
                text_measured: Some(TextMeasureInfo {
                    width: mw,
                    height: mh,
                    line_count,
                }),
                children: Vec::new(),
            }
        }
        LayerKind::Image {
            src,
            x,
            y,
            width,
            height,
            opacity,
            scale,
            ..
        } => {
            let (ix, iy) = resolve_xy(layer, layout_override, *x, *y, local_t);
            let (sw, sh) = assets
                .images
                .get(src)
                .map(|img| (img.width() as f32, img.height() as f32))
                .unwrap_or((0.0, 0.0));
            let base_w = layout_override.map(|b| b.w).or(*width).unwrap_or(sw);
            let base_h = layout_override.map(|b| b.h).or(*height).unwrap_or(sh);
            let s = eval_property(&layer.animations, AnimProperty::Scale, local_t, *scale);
            let w = eval_property(&layer.animations, AnimProperty::Width, local_t, base_w) * s;
            let h = eval_property(&layer.animations, AnimProperty::Height, local_t, base_h) * s;
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "image".to_string(),
                x: ix,
                y: iy,
                w,
                h,
                opacity: op,
                in_canvas: boxes_intersect_canvas(ix, iy, w, h),
                clipped_by_canvas: boxes_clipped_by_canvas(ix, iy, w, h),
                text_measured: None,
                children: Vec::new(),
            }
        }
        LayerKind::Waveform {
            x,
            y,
            width,
            height,
            opacity,
            scale,
            ..
        } => {
            let (rx, ry) = resolve_xy(layer, layout_override, *x, *y, local_t);
            let base_w = layout_override.map(|b| b.w).unwrap_or(*width);
            let base_h = layout_override.map(|b| b.h).unwrap_or(*height);
            let s = eval_property(&layer.animations, AnimProperty::Scale, local_t, *scale);
            let w = eval_property(&layer.animations, AnimProperty::Width, local_t, base_w) * s;
            let h = eval_property(&layer.animations, AnimProperty::Height, local_t, base_h) * s;
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "waveform".to_string(),
                x: rx,
                y: ry,
                w,
                h,
                opacity: op,
                in_canvas: boxes_intersect_canvas(rx, ry, w, h),
                clipped_by_canvas: boxes_clipped_by_canvas(rx, ry, w, h),
                text_measured: None,
                children: Vec::new(),
            }
        }
        LayerKind::Particles {
            x,
            y,
            width,
            height,
            opacity,
            scale,
            ..
        } => {
            let (rx, ry) = resolve_xy(layer, layout_override, *x, *y, local_t);
            let base_w = layout_override.map(|b| b.w).unwrap_or(*width);
            let base_h = layout_override.map(|b| b.h).unwrap_or(*height);
            let s = eval_property(&layer.animations, AnimProperty::Scale, local_t, *scale);
            let w = eval_property(&layer.animations, AnimProperty::Width, local_t, base_w) * s;
            let h = eval_property(&layer.animations, AnimProperty::Height, local_t, base_h) * s;
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "particles".to_string(),
                x: rx,
                y: ry,
                w,
                h,
                opacity: op,
                in_canvas: boxes_intersect_canvas(rx, ry, w, h),
                clipped_by_canvas: boxes_clipped_by_canvas(rx, ry, w, h),
                text_measured: None,
                children: Vec::new(),
            }
        }
        LayerKind::Lottie {
            x,
            y,
            width,
            height,
            opacity,
            scale,
            ..
        } => {
            let (rx, ry) = resolve_xy(layer, layout_override, *x, *y, local_t);
            let base_w = layout_override.map(|b| b.w).or(*width).unwrap_or(200.0);
            let base_h = layout_override.map(|b| b.h).or(*height).unwrap_or(200.0);
            let s = eval_property(&layer.animations, AnimProperty::Scale, local_t, *scale);
            let w = eval_property(&layer.animations, AnimProperty::Width, local_t, base_w) * s;
            let h = eval_property(&layer.animations, AnimProperty::Height, local_t, base_h) * s;
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "lottie".to_string(),
                x: rx,
                y: ry,
                w,
                h,
                opacity: op,
                in_canvas: boxes_intersect_canvas(rx, ry, w, h),
                clipped_by_canvas: boxes_clipped_by_canvas(rx, ry, w, h),
                text_measured: None,
                children: Vec::new(),
            }
        }
        LayerKind::Video {
            src,
            x,
            y,
            width,
            height,
            opacity,
            scale,
            ..
        } => {
            let (vx, vy) = resolve_xy(layer, layout_override, *x, *y, local_t);
            let (sw, sh) = assets
                .videos
                .get(src)
                .map(|v| (v.width as f32, v.height as f32))
                .unwrap_or((0.0, 0.0));
            let base_w = layout_override.map(|b| b.w).or(*width).unwrap_or(sw);
            let base_h = layout_override.map(|b| b.h).or(*height).unwrap_or(sh);
            let s = eval_property(&layer.animations, AnimProperty::Scale, local_t, *scale);
            let w = eval_property(&layer.animations, AnimProperty::Width, local_t, base_w) * s;
            let h = eval_property(&layer.animations, AnimProperty::Height, local_t, base_h) * s;
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "video".to_string(),
                x: vx,
                y: vy,
                w,
                h,
                opacity: op,
                in_canvas: boxes_intersect_canvas(vx, vy, w, h),
                clipped_by_canvas: boxes_clipped_by_canvas(vx, vy, w, h),
                text_measured: None,
                children: Vec::new(),
            }
        }
        LayerKind::Svg {
            src,
            x,
            y,
            width,
            height,
            opacity,
            ..
        } => {
            let (sx, sy) = resolve_xy(layer, layout_override, *x, *y, local_t);
            let (sw, sh) = if width.is_none() || height.is_none() {
                assets
                    .svgs
                    .get(src)
                    .and_then(|bytes| {
                        resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()
                    })
                    .map(|t| (t.size().width(), t.size().height()))
                    .unwrap_or((0.0, 0.0))
            } else {
                (0.0, 0.0)
            };
            let base_w = layout_override.map(|b| b.w).or(*width).unwrap_or(sw);
            let base_h = layout_override.map(|b| b.h).or(*height).unwrap_or(sh);
            let w = eval_property(&layer.animations, AnimProperty::Width, local_t, base_w);
            let h = eval_property(&layer.animations, AnimProperty::Height, local_t, base_h);
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "svg".to_string(),
                x: sx,
                y: sy,
                w,
                h,
                opacity: op,
                in_canvas: boxes_intersect_canvas(sx, sy, w, h),
                clipped_by_canvas: boxes_clipped_by_canvas(sx, sy, w, h),
                text_measured: None,
                children: Vec::new(),
            }
        }
        LayerKind::Container {
            x,
            y,
            width,
            height,
            layout,
            opacity,
            cascade_opacity,
            timeline,
            stagger_secs,
            children,
            ..
        } => {
            let (cx, cy) = resolve_xy(layer, layout_override, *x, *y, local_t);
            let target_w = layout_override
                .map(|b| b.w)
                .or(*width)
                .map(|w| eval_property(&layer.animations, AnimProperty::Width, local_t, w));
            let target_h = layout_override
                .map(|b| b.h)
                .or(*height)
                .map(|h| eval_property(&layer.animations, AnimProperty::Height, local_t, h));
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            let (cw, ch, child_boxes) = compute_flex_layout(
                target_w,
                target_h,
                layout,
                children,
                font_system,
                local_t,
            )?;
            let (cx, cy) = apply_anchor(cx, cy, cw, ch, layer.anchor);
            let container_end = effective_end_sec(layer, scene_duration);
            let child_timelines = resolve_child_timelines(
                *timeline,
                layer.start_sec,
                container_end,
                local_t,
                scene_duration,
                *stagger_secs,
                children,
            );
            let child_parent_op = if *cascade_opacity { op } else { 1.0 };
            let mut child_infos = Vec::new();
            for (((idx, child), cbox), c_time) in children
                .iter()
                .enumerate()
                .zip(child_boxes)
                .zip(child_timelines)
            {
                let Some((ct, cdur, skip_win)) = c_time else {
                    continue;
                };
                let abs_box = LayoutBox {
                    x: cx + cbox.x,
                    y: cy + cbox.y,
                    w: cbox.w,
                    h: cbox.h,
                    parent_x: cx,
                    parent_y: cy,
                };
                if let Some(ci) = inspect_layer_layout(
                    child,
                    &format!("{path}.children[{idx}]"),
                    cdur,
                    ct,
                    assets,
                    font_system,
                    Some(abs_box),
                    child_parent_op,
                    skip_win,
                    canvas_w,
                    canvas_h,
                )? {
                    child_infos.push(ci);
                }
            }
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "container".to_string(),
                x: cx,
                y: cy,
                w: cw,
                h: ch,
                opacity: op,
                in_canvas: boxes_intersect_canvas(cx, cy, cw, ch),
                clipped_by_canvas: boxes_clipped_by_canvas(cx, cy, cw, ch),
                text_measured: None,
                children: child_infos,
            }
        }
        #[cfg(feature = "exp-3d")]
        LayerKind::Mesh3d {
            x,
            y,
            width,
            height,
            opacity,
            scale,
            ..
        } => {
            let (rx, ry) = resolve_xy(layer, layout_override, *x, *y, local_t);
            let base_w = layout_override.map(|b| b.w).unwrap_or(*width);
            let base_h = layout_override.map(|b| b.h).unwrap_or(*height);
            let s = eval_property(&layer.animations, AnimProperty::Scale, local_t, *scale);
            let w = eval_property(&layer.animations, AnimProperty::Width, local_t, base_w) * s;
            let h = eval_property(&layer.animations, AnimProperty::Height, local_t, base_h) * s;
            let op = eval_property(&layer.animations, AnimProperty::Opacity, local_t, *opacity)
                * parent_opacity;
            LayerLayoutInfo {
                path: path.to_string(),
                name: layer.name.clone(),
                layer_type: "mesh3d".to_string(),
                x: rx,
                y: ry,
                w,
                h,
                opacity: op,
                in_canvas: boxes_intersect_canvas(rx, ry, w, h),
                clipped_by_canvas: boxes_clipped_by_canvas(rx, ry, w, h),
                text_measured: None,
                children: Vec::new(),
            }
        }
    };
    Ok(Some(info))
}
