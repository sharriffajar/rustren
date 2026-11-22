//! Flexbox layout resolution via Taffy, anchor/pivot transforms, and child timeline scheduling.

use anyhow::Result;
use cosmic_text::FontSystem;
use taffy::prelude::*;
use tiny_skia::Transform;

use crate::animation::eval_property;
use crate::schema::{
    AlignSpec, AnimProperty, AnchorSpec, FlexDirectionSpec, Layer, LayerKind, LayoutSpec,
    PositionSpec, TextAlignSpec, TimelineMode,
};

use super::text::{is_rtl_content, measure_text_size};

#[derive(Clone, Copy)]
pub(super) struct LayoutBox {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) w: f32,
    pub(super) h: f32,
    pub(super) parent_x: f32,
    pub(super) parent_y: f32,
}

pub(super) fn apply_anchor(x: f32, y: f32, w: f32, h: f32, anchor: AnchorSpec) -> (f32, f32) {
    let (ax, ay) = anchor.factors();
    (x - ax * w, y - ay * h)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_pivot_transform_with_skew(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    scale: f32,
    rotation: f32,
    skew_x: f32,
    skew_y: f32,
) -> Transform {
    let px = x + w * 0.5;
    let py = y + h * 0.5;
    let mut t = Transform::identity();
    if (scale - 1.0).abs() > 1e-6 {
        t = t
            .post_translate(-px, -py)
            .post_scale(scale, scale)
            .post_translate(px, py);
    }
    if skew_x != 0.0 || skew_y != 0.0 {
        t = t
            .post_translate(-px, -py)
            .post_concat(Transform::from_skew(
                skew_x.to_radians().tan(),
                skew_y.to_radians().tan(),
            ))
            .post_translate(px, py);
    }
    if rotation != 0.0 {
        t = t.post_rotate_at(rotation, px, py);
    }
    t
}

pub(super) fn resolve_xy(
    layer: &Layer,
    layout_override: Option<LayoutBox>,
    layer_x: f32,
    layer_y: f32,
    time_sec: f32,
) -> (f32, f32) {
    if layer.position == PositionSpec::Absolute {
        let origin_x = layout_override.map(|b| b.parent_x).unwrap_or(0.0);
        let origin_y = layout_override.map(|b| b.parent_y).unwrap_or(0.0);
        (
            origin_x + eval_property(&layer.animations, AnimProperty::X, time_sec, layer_x),
            origin_y + eval_property(&layer.animations, AnimProperty::Y, time_sec, layer_y),
        )
    } else if let Some(b) = layout_override {
        (
            b.x + eval_property(&layer.animations, AnimProperty::X, time_sec, 0.0),
            b.y + eval_property(&layer.animations, AnimProperty::Y, time_sec, 0.0),
        )
    } else {
        (
            eval_property(&layer.animations, AnimProperty::X, time_sec, layer_x),
            eval_property(&layer.animations, AnimProperty::Y, time_sec, layer_y),
        )
    }
}

pub(super) fn effective_end_sec(layer: &Layer, parent_duration: f32) -> f32 {
    layer
        .end_sec
        .or_else(|| layer.duration_secs.map(|d| layer.start_sec + d))
        .unwrap_or(parent_duration)
}

pub(super) fn resolve_child_timelines(
    timeline: TimelineMode,
    container_start: f32,
    container_end: f32,
    parent_time: f32,
    parent_duration: f32,
    stagger_secs: f32,
    children: &[Layer],
) -> Vec<Option<(f32, f32, bool)>> {
    match timeline {
        TimelineMode::Global => {
            if stagger_secs <= 0.0 {
                vec![Some((parent_time, parent_duration, false)); children.len()]
            } else {
                (0..children.len())
                    .map(|i| Some((parent_time - (i as f32) * stagger_secs, parent_duration, false)))
                    .collect()
            }
        }
        TimelineMode::Local => {
            let base_t = parent_time - container_start;
            let local_dur = (container_end - container_start).max(0.0);
            if stagger_secs <= 0.0 {
                vec![Some((base_t, local_dur, false)); children.len()]
            } else {
                (0..children.len())
                    .map(|i| Some((base_t - (i as f32) * stagger_secs, local_dur, false)))
                    .collect()
            }
        }
        TimelineMode::Series => {
            let local_t = parent_time - container_start;
            let local_dur = (container_end - container_start).max(0.0);
            let mut cursor = 0.0f32;
            let mut out = Vec::with_capacity(children.len());
            for (i, child) in children.iter().enumerate() {
                let c_start =
                    (cursor + child.start_sec + (i as f32) * stagger_secs - child.overlap_secs)
                        .max(0.0);
                let c_dur = child
                    .duration_secs
                    .or_else(|| child.end_sec.map(|e| (e - child.start_sec).max(0.0)))
                    .unwrap_or((local_dur - c_start).max(0.0));
                let c_end = c_start + c_dur;
                cursor = c_end;
                if local_t >= c_start && local_t <= c_end {
                    out.push(Some((local_t - c_start, c_dur, true)));
                } else {
                    out.push(None);
                }
            }
            out
        }
    }
}

pub(super) fn map_align_items(spec: Option<AlignSpec>) -> Option<AlignItems> {
    spec.map(|s| match s {
        AlignSpec::Start | AlignSpec::SpaceBetween | AlignSpec::SpaceAround => AlignItems::Start,
        AlignSpec::Center => AlignItems::Center,
        AlignSpec::End => AlignItems::End,
        AlignSpec::Stretch => AlignItems::Stretch,
    })
}

pub(super) fn map_justify_content(spec: Option<AlignSpec>) -> Option<JustifyContent> {
    spec.map(|s| match s {
        AlignSpec::Start | AlignSpec::Stretch => JustifyContent::Start,
        AlignSpec::Center => JustifyContent::Center,
        AlignSpec::End => JustifyContent::End,
        AlignSpec::SpaceBetween => JustifyContent::SpaceBetween,
        AlignSpec::SpaceAround => JustifyContent::SpaceAround,
    })
}

fn build_container_style(
    layout: &LayoutSpec,
    cw: Dimension,
    ch: Dimension,
    pos: Position,
    inset: taffy::Rect<LengthPercentageAuto>,
    flex_grow: f32,
    (mt, mr, mb, ml): (f32, f32, f32, f32),
) -> Style {
    let (pt, pr, pb, pl) = layout.padding_trbl();
    let (col_gap, row_gap) = layout.gaps();
    Style {
        display: Display::Flex,
        position: pos,
        inset,
        flex_direction: match layout.direction {
            FlexDirectionSpec::Row => FlexDirection::Row,
            FlexDirectionSpec::Column => FlexDirection::Column,
        },
        flex_wrap: if layout.flex_wrap {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        },
        justify_content: map_justify_content(layout.justify_content),
        align_items: map_align_items(layout.align_items),
        gap: Size {
            width: LengthPercentage::Length(col_gap),
            height: LengthPercentage::Length(row_gap),
        },
        padding: taffy::Rect {
            left: LengthPercentage::Length(pl),
            right: LengthPercentage::Length(pr),
            top: LengthPercentage::Length(pt),
            bottom: LengthPercentage::Length(pb),
        },
        size: Size {
            width: cw,
            height: ch,
        },
        flex_grow,
        margin: taffy::Rect {
            left: LengthPercentageAuto::Length(ml),
            right: LengthPercentageAuto::Length(mr),
            top: LengthPercentageAuto::Length(mt),
            bottom: LengthPercentageAuto::Length(mb),
        },
        ..Default::default()
    }
}

pub(super) fn build_taffy_child_node(
    taffy: &mut TaffyTree<()>,
    child: &Layer,
    parent_width_hint: Option<f32>,
    font_system: &mut FontSystem,
    time_sec: f32,
) -> Result<taffy::NodeId> {
    let margin_trbl @ (mt, mr, mb, ml) = child.margin_trbl();
    let (pos, inset) = match child.position {
        PositionSpec::Relative => (Position::Relative, taffy::Rect::auto()),
        PositionSpec::Absolute => {
            let (ax, ay) = match &child.kind {
                LayerKind::Rect { x, y, .. }
                | LayerKind::Text { x, y, .. }
                | LayerKind::Image { x, y, .. }
                | LayerKind::Video { x, y, .. }
                | LayerKind::Waveform { x, y, .. }
                | LayerKind::Lottie { x, y, .. }
                | LayerKind::Particles { x, y, .. }
                | LayerKind::Svg { x, y, .. }
                | LayerKind::Container { x, y, .. } => (*x, *y),
                #[cfg(feature = "exp-3d")]
                LayerKind::Mesh3d { x, y, .. } => (*x, *y),
                LayerKind::Circle {
                    cx, cy, r, scale, ..
                } => {
                    let s = eval_property(&child.animations, AnimProperty::Scale, time_sec, *scale);
                    let eff_r = eval_property(&child.animations, AnimProperty::R, time_sec, *r) * s;
                    (*cx - eff_r, *cy - eff_r)
                }
            };
            (
                Position::Absolute,
                taffy::Rect {
                    left: LengthPercentageAuto::Length(ax),
                    top: LengthPercentageAuto::Length(ay),
                    right: LengthPercentageAuto::Auto,
                    bottom: LengthPercentageAuto::Auto,
                },
            )
        }
    };

    if let LayerKind::Container {
        width,
        height,
        layout,
        children,
        ..
    } = &child.kind
    {
        let cw = width
            .map(|w| Dimension::Length(eval_property(&child.animations, AnimProperty::Width, time_sec, w)))
            .unwrap_or(Dimension::Auto);
        let ch = height
            .map(|h| Dimension::Length(eval_property(&child.animations, AnimProperty::Height, time_sec, h)))
            .unwrap_or(Dimension::Auto);
        let (_pt, pr, _pb, pl) = layout.padding_trbl();
        let next_hint = width
            .map(|w| (w - pl - pr).max(0.0))
            .or_else(|| parent_width_hint.map(|pw| (pw - pl - pr).max(0.0)));
        let mut sub_nodes = Vec::with_capacity(children.len());
        for sub in children {
            sub_nodes.push(build_taffy_child_node(
                taffy,
                sub,
                next_hint,
                font_system,
                time_sec,
            )?);
        }
        let style = build_container_style(
            layout,
            cw,
            ch,
            pos,
            inset,
            child.flex_grow.unwrap_or(0.0),
            margin_trbl,
        );
        return Ok(taffy.new_with_children(style, &sub_nodes)?);
    }

    let (cw, ch) = match &child.kind {
        LayerKind::Rect { width, height, .. } => (
            Dimension::Length(eval_property(
                &child.animations,
                AnimProperty::Width,
                time_sec,
                *width,
            )),
            Dimension::Length(eval_property(
                &child.animations,
                AnimProperty::Height,
                time_sec,
                *height,
            )),
        ),
        LayerKind::Circle { r, scale, .. } => {
            let s = eval_property(&child.animations, AnimProperty::Scale, time_sec, *scale);
            let eff_r = eval_property(&child.animations, AnimProperty::R, time_sec, *r) * s;
            (Dimension::Length(eff_r * 2.0), Dimension::Length(eff_r * 2.0))
        }
        LayerKind::Waveform { width, height, .. }
        | LayerKind::Particles { width, height, .. } => {
            (Dimension::Length(*width), Dimension::Length(*height))
        }
        LayerKind::Container { .. } => unreachable!(),
        LayerKind::Image { width, height, .. }
        | LayerKind::Video { width, height, .. }
        | LayerKind::Lottie { width, height, .. }
        | LayerKind::Svg { width, height, .. } => (
            width.map(Dimension::Length).unwrap_or(Dimension::Auto),
            height.map(Dimension::Length).unwrap_or(Dimension::Auto),
        ),
        #[cfg(feature = "exp-3d")]
        LayerKind::Mesh3d { width, height, .. } => (
            Dimension::Length(*width),
            Dimension::Length(*height),
        ),
        LayerKind::Text {
            content,
            spans,
            direction,
            max_width,
            font_size,
            line_height,
            letter_spacing,
            font_family,
            font_weight,
            text_align,
            scale,
            ..
        } => {
            let is_rtl_text = is_rtl_content(content, spans, *direction);
            let s = eval_property(&child.animations, AnimProperty::Scale, time_sec, *scale);
            let wrap_w = max_width.or(parent_width_hint);
            let (measured_w, measured_h) = measure_text_size(
                font_system,
                content,
                spans,
                *font_size,
                *line_height,
                *letter_spacing,
                font_family.as_deref(),
                *font_weight,
                wrap_w,
                s,
            );
            let final_w = match (text_align, max_width) {
                (TextAlignSpec::Center | TextAlignSpec::Right, Some(mw)) => *mw,
                (TextAlignSpec::Start, Some(mw)) if is_rtl_text => *mw,
                (TextAlignSpec::End, Some(mw)) if !is_rtl_text => *mw,
                (_, Some(mw)) => measured_w.min(*mw),
                (_, None) => measured_w,
            };
            (Dimension::Length(final_w), Dimension::Length(measured_h))
        }
    };

    let style = Style {
        position: pos,
        inset,
        size: Size {
            width: cw,
            height: ch,
        },
        flex_grow: child.flex_grow.unwrap_or(0.0),
        margin: taffy::Rect {
            left: LengthPercentageAuto::Length(ml),
            right: LengthPercentageAuto::Length(mr),
            top: LengthPercentageAuto::Length(mt),
            bottom: LengthPercentageAuto::Length(mb),
        },
        ..Default::default()
    };
    Ok(taffy.new_leaf(style)?)
}

pub(super) fn compute_flex_layout(
    width: Option<f32>,
    height: Option<f32>,
    layout: &LayoutSpec,
    children: &[Layer],
    font_system: &mut FontSystem,
    time_sec: f32,
) -> Result<(f32, f32, Vec<LayoutBox>)> {
    let mut taffy: TaffyTree<()> = TaffyTree::new();
    let mut child_nodes = Vec::with_capacity(children.len());
    let (_pt, pr, _pb, pl) = layout.padding_trbl();
    let inner_width_hint = width.map(|w| (w - pl - pr).max(0.0));

    for child in children {
        child_nodes.push(build_taffy_child_node(
            &mut taffy,
            child,
            inner_width_hint,
            font_system,
            time_sec,
        )?);
    }

    let root_style = build_container_style(
        layout,
        width.map(Dimension::Length).unwrap_or(Dimension::Auto),
        height.map(Dimension::Length).unwrap_or(Dimension::Auto),
        Position::Relative,
        taffy::Rect::auto(),
        0.0,
        (0.0, 0.0, 0.0, 0.0),
    );

    let root = taffy.new_with_children(root_style, &child_nodes)?;
    taffy.compute_layout(
        root,
        Size {
            width: width
                .map(AvailableSpace::Definite)
                .unwrap_or(AvailableSpace::MaxContent),
            height: height
                .map(AvailableSpace::Definite)
                .unwrap_or(AvailableSpace::MaxContent),
        },
    )?;

    let root_layout = taffy.layout(root)?;
    let root_w = root_layout.size.width;
    let root_h = root_layout.size.height;

    let mut boxes = Vec::with_capacity(child_nodes.len());
    for node in child_nodes {
        let l = taffy.layout(node)?;
        boxes.push(LayoutBox {
            x: l.location.x,
            y: l.location.y,
            w: l.size.width,
            h: l.size.height,
            parent_x: 0.0,
            parent_y: 0.0,
        });
    }
    Ok((root_w, root_h, boxes))
}
