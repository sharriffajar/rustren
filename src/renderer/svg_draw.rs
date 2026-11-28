use anyhow::Result;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point,
    RadialGradient, Rect, Shader, SpreadMode, Stroke, StrokeDash, Transform,
};

use crate::schema::{GradientTypeSpec, LinearGradientSpec};

use super::parse_hex_color;

pub(super) fn build_rounded_rect(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r <= 0.0 {
        let rect = Rect::from_xywh(x, y, w, h)?;
        return Some(PathBuilder::from_rect(rect));
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_progress_stroke_dash(
    total_len: f32,
    progress: f32,
    dash_offset: Option<f32>,
    custom_dash: Option<&[f32]>,
) -> Option<StrokeDash> {
    if let Some(arr) = custom_dash
        && !arr.is_empty()
    {
        return StrokeDash::new(arr.to_vec(), dash_offset.unwrap_or(0.0));
    }
    let p = progress.clamp(0.0, 1.0);
    if p >= 1.0 - 1e-5 && dash_offset.is_none() {
        return None;
    }
    let len = total_len.max(0.001);
    if p <= 1e-5 {
        return StrokeDash::new(vec![0.0001, len * 2.0], dash_offset.unwrap_or(0.0));
    }
    let visible = len * p;
    let gap = (len - visible).max(0.0001);
    StrokeDash::new(vec![visible, gap + len], dash_offset.unwrap_or(0.0))
}

pub(super) fn compute_path_length(path: &tiny_skia::Path) -> f32 {
    use tiny_skia::PathSegment;
    let mut total = 0.0_f32;
    let mut cur = Point::from_xy(0.0, 0.0);
    let mut sub_start = cur;
    let dist = |a: Point, b: Point| ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();

    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                cur = p;
                sub_start = p;
            }
            PathSegment::LineTo(p) => {
                total += dist(cur, p);
                cur = p;
            }
            PathSegment::QuadTo(p1, p2) => {
                let mut prev = cur;
                for i in 1..=16 {
                    let t = i as f32 / 16.0;
                    let mt = 1.0 - t;
                    let pt = Point::from_xy(
                        mt * mt * cur.x + 2.0 * mt * t * p1.x + t * t * p2.x,
                        mt * mt * cur.y + 2.0 * mt * t * p1.y + t * t * p2.y,
                    );
                    total += dist(prev, pt);
                    prev = pt;
                }
                cur = p2;
            }
            PathSegment::CubicTo(p1, p2, p3) => {
                let mut prev = cur;
                for i in 1..=16 {
                    let t = i as f32 / 16.0;
                    let mt = 1.0 - t;
                    let pt = Point::from_xy(
                        mt.powi(3) * cur.x
                            + 3.0 * mt.powi(2) * t * p1.x
                            + 3.0 * mt * t.powi(2) * p2.x
                            + t.powi(3) * p3.x,
                        mt.powi(3) * cur.y
                            + 3.0 * mt.powi(2) * t * p1.y
                            + 3.0 * mt * t.powi(2) * p2.y
                            + t.powi(3) * p3.y,
                    );
                    total += dist(prev, pt);
                    prev = pt;
                }
                cur = p3;
            }
            PathSegment::Close => {
                total += dist(cur, sub_start);
                cur = sub_start;
            }
        }
    }
    total
}

fn usvg_spread_mode(mode: resvg::usvg::SpreadMethod) -> SpreadMode {
    match mode {
        resvg::usvg::SpreadMethod::Pad => SpreadMode::Pad,
        resvg::usvg::SpreadMethod::Reflect => SpreadMode::Reflect,
        resvg::usvg::SpreadMethod::Repeat => SpreadMode::Repeat,
    }
}

fn usvg_stops_to_skia(stops: &[resvg::usvg::Stop], opacity: f32) -> Vec<GradientStop> {
    let op = opacity.clamp(0.0, 1.0);
    stops
        .iter()
        .filter_map(|s| {
            let c = s.color();
            let a = (s.opacity().get() * op).clamp(0.0, 1.0);
            Color::from_rgba(
                c.red as f32 / 255.0,
                c.green as f32 / 255.0,
                c.blue as f32 / 255.0,
                a,
            )
            .map(|col| GradientStop::new(s.offset().get(), col))
        })
        .collect()
}

fn usvg_paint_to_tiny_paint(paint: &resvg::usvg::Paint, opacity: f32) -> Option<Paint<'static>> {
    let op = opacity.clamp(0.0, 1.0);
    match paint {
        resvg::usvg::Paint::Color(c) => {
            let col = Color::from_rgba(
                c.red as f32 / 255.0,
                c.green as f32 / 255.0,
                c.blue as f32 / 255.0,
                op,
            )?;
            let mut p = Paint::default();
            p.set_color(col);
            p.anti_alias = true;
            Some(p)
        }
        resvg::usvg::Paint::LinearGradient(lg) => {
            let stops = usvg_stops_to_skia(lg.stops(), op);
            let shader = LinearGradient::new(
                Point::from_xy(lg.x1(), lg.y1()),
                Point::from_xy(lg.x2(), lg.y2()),
                stops,
                usvg_spread_mode(lg.spread_method()),
                lg.transform(),
            )?;
            Some(Paint {
                shader,
                anti_alias: true,
                ..Paint::default()
            })
        }
        resvg::usvg::Paint::RadialGradient(rg) => {
            let stops = usvg_stops_to_skia(rg.stops(), op);
            let shader = RadialGradient::new(
                Point::from_xy(rg.fx(), rg.fy()),
                Point::from_xy(rg.cx(), rg.cy()),
                rg.r().get(),
                stops,
                usvg_spread_mode(rg.spread_method()),
                rg.transform(),
            )?;
            Some(Paint {
                shader,
                anti_alias: true,
                ..Paint::default()
            })
        }
        resvg::usvg::Paint::Pattern(_) => None,
    }
}

pub(super) fn render_usvg_group_animated(
    group: &resvg::usvg::Group,
    pixmap: &mut Pixmap,
    root_xform: Transform,
    progress: f32,
    dash_offset: Option<f32>,
    custom_dash: Option<&[f32]>,
) {
    for node in group.children() {
        match node {
            resvg::usvg::Node::Group(sub) => {
                render_usvg_group_animated(
                    sub,
                    pixmap,
                    root_xform,
                    progress,
                    dash_offset,
                    custom_dash,
                );
            }
            resvg::usvg::Node::Path(path) => {
                if !path.is_visible() {
                    continue;
                }
                let xform = root_xform.pre_concat(path.abs_transform());
                if let Some(fill) = path.fill()
                    && let Some(p) = usvg_paint_to_tiny_paint(fill.paint(), fill.opacity().get())
                {
                    let rule = match fill.rule() {
                        resvg::usvg::FillRule::NonZero => FillRule::Winding,
                        resvg::usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                    };
                    pixmap.fill_path(path.data(), &p, rule, xform, None);
                }
                if let Some(stroke) = path.stroke()
                    && let Some(p) =
                        usvg_paint_to_tiny_paint(stroke.paint(), stroke.opacity().get())
                {
                    let path_len = compute_path_length(path.data());
                    let dash =
                        build_progress_stroke_dash(path_len, progress, dash_offset, custom_dash);
                    let skia_stroke = Stroke {
                        width: stroke.width().get(),
                        dash,
                        ..Stroke::default()
                    };
                    pixmap.stroke_path(path.data(), &p, &skia_stroke, xform, None);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn build_gradient_shader(
    grad: &LinearGradientSpec,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    opacity: f32,
) -> Result<Option<Shader<'static>>> {
    let stops = if !grad.stops.is_empty() {
        let mut out = Vec::with_capacity(grad.stops.len());
        for st in &grad.stops {
            out.push(GradientStop::new(
                st.offset.clamp(0.0, 1.0),
                parse_hex_color(&st.color, opacity)?,
            ));
        }
        out
    } else {
        let c_from = parse_hex_color(grad.from.as_deref().unwrap_or("#000000"), opacity)?;
        let c_to = parse_hex_color(grad.to.as_deref().unwrap_or("#FFFFFF"), opacity)?;
        vec![GradientStop::new(0.0, c_from), GradientStop::new(1.0, c_to)]
    };

    let cx = x + w * 0.5;
    let cy = y + h * 0.5;
    let shader = match grad.kind {
        GradientTypeSpec::Linear => {
            let rad = grad.angle.to_radians();
            let dx = rad.cos() * w * 0.5;
            let dy = rad.sin() * h * 0.5;
            LinearGradient::new(
                Point::from_xy(cx - dx, cy - dy),
                Point::from_xy(cx + dx, cy + dy),
                stops,
                SpreadMode::Pad,
                Transform::identity(),
            )
        }
        GradientTypeSpec::Radial => {
            let r_cx = x + w * grad.cx.unwrap_or(0.5);
            let r_cy = y + h * grad.cy.unwrap_or(0.5);
            let r_fx = x + w * grad.fx.unwrap_or(grad.cx.unwrap_or(0.5));
            let r_fy = y + h * grad.fy.unwrap_or(grad.cy.unwrap_or(0.5));
            let r_rad = (w.max(h) * grad.r.unwrap_or(0.5)).max(0.001);
            RadialGradient::new(
                Point::from_xy(r_fx, r_fy),
                Point::from_xy(r_cx, r_cy),
                r_rad,
                stops,
                SpreadMode::Pad,
                Transform::identity(),
            )
        }
    };
    Ok(shader)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_styled_path(
    pixmap: &mut Pixmap,
    path: &tiny_skia::Path,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    solid_color: Option<&str>,
    gradient: Option<&LinearGradientSpec>,
    border_color: Option<&str>,
    border_width: f32,
    stroke_dash: Option<StrokeDash>,
    opacity: f32,
    transform: Transform,
) -> Result<()> {
    if let Some(grad) = gradient {
        if let Some(shader) = build_gradient_shader(grad, x, y, w, h, opacity)? {
            let paint = Paint {
                shader,
                anti_alias: true,
                ..Paint::default()
            };
            pixmap.fill_path(path, &paint, FillRule::Winding, transform, None);
        }
    } else if let Some(hex) = solid_color {
        let mut paint = Paint::default();
        paint.set_color(parse_hex_color(hex, opacity)?);
        paint.anti_alias = true;
        pixmap.fill_path(path, &paint, FillRule::Winding, transform, None);
    }

    if let Some(b_hex) = border_color
        && border_width > 0.0
    {
        let mut paint = Paint::default();
        paint.set_color(parse_hex_color(b_hex, opacity)?);
        paint.anti_alias = true;
        let stroke = Stroke {
            width: border_width,
            dash: stroke_dash,
            ..Stroke::default()
        };
        pixmap.stroke_path(path, &paint, &stroke, transform, None);
    }
    Ok(())
}
