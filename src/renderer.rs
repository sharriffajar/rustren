use anyhow::{anyhow, Context, Result};
use cosmic_text::{
    Align, Attrs, Buffer, Color as CosmicColor, Family, FontSystem, Metrics, Shaping, SwashCache,
    Weight,
};
use std::cell::RefCell;
use taffy::prelude::*;
use tiny_skia::{
    BlendMode as SkiaBlendMode, Color, FillRule, FilterQuality, GradientStop, IntSize,
    LinearGradient, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Point, RadialGradient, Rect,
    Shader, SpreadMode, Stroke, StrokeDash, Transform,
};

#[allow(clippy::too_many_arguments)]
fn draw_rgba_frame(
    pixmap: &mut Pixmap,
    layer: &Layer,
    layout_override: Option<LayoutBox>,
    time_sec: f32,
    parent_opacity: f32,
    x: f32,
    y: f32,
    width: Option<f32>,
    height: Option<f32>,
    opacity: f32,
    rotation: f32,
    scale: f32,
    fit: FitSpec,
    src_w: u32,
    src_h: u32,
    raw_rgba: &[u8],
) -> Result<()> {
    let target_w = layout_override.map(|b| b.w).or(width).unwrap_or(src_w as f32);
    let target_h = layout_override.map(|b| b.h).or(height).unwrap_or(src_h as f32);

    let (x, y) = resolve_xy(layer, layout_override, x, y, time_sec);
    let w = eval_property(&layer.animations, AnimProperty::Width, time_sec, target_w);
    let h = eval_property(&layer.animations, AnimProperty::Height, time_sec, target_h);
    let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, opacity) * parent_opacity;
    let rot = eval_property(&layer.animations, AnimProperty::Rotation, time_sec, rotation);
    let sk_x = eval_property(&layer.animations, AnimProperty::SkewX, time_sec, layer.skew_x);
    let sk_y = eval_property(&layer.animations, AnimProperty::SkewY, time_sec, layer.skew_y);
    let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, scale);

    if w <= 0.0 || h <= 0.0 || op <= 0.0 || s <= 0.0 {
        return Ok(());
    }

    let (x, y) = apply_anchor(x, y, w, h, layer.anchor);
    let size = IntSize::from_wh(src_w, src_h).ok_or_else(|| anyhow!("Invalid media dimensions"))?;
    let src_pixmap = Pixmap::from_vec(raw_rgba.to_vec(), size)
        .ok_or_else(|| anyhow!("Failed to create Pixmap from media frame"))?;

    let paint = PixmapPaint {
        opacity: op.clamp(0.0, 1.0),
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    let pivot_xf = build_pivot_transform_with_skew(x, y, w, h, s, rot, sk_x, sk_y);

    match fit {
        FitSpec::Fill => {
            let sx = w / src_w as f32;
            let sy = h / src_h as f32;
            let transform = Transform::from_scale(sx, sy)
                .post_translate(x, y)
                .post_concat(pivot_xf);
            pixmap.draw_pixmap(0, 0, src_pixmap.as_ref(), &paint, transform, None);
        }
        FitSpec::Contain => {
            let u = (w / src_w as f32).min(h / src_h as f32);
            let draw_w = src_w as f32 * u;
            let draw_h = src_h as f32 * u;
            let ox = x + (w - draw_w) * 0.5;
            let oy = y + (h - draw_h) * 0.5;
            let transform = Transform::from_scale(u, u)
                .post_translate(ox, oy)
                .post_concat(pivot_xf);
            pixmap.draw_pixmap(0, 0, src_pixmap.as_ref(), &paint, transform, None);
        }
        FitSpec::Cover => {
            let pw = (w.ceil() as u32).max(1);
            let ph = (h.ceil() as u32).max(1);
            let mut box_pixmap = Pixmap::new(pw, ph)
                .ok_or_else(|| anyhow!("Failed to allocate cover pixmap {}x{}", pw, ph))?;
            let u = (w / src_w as f32).max(h / src_h as f32);
            let draw_w = src_w as f32 * u;
            let draw_h = src_h as f32 * u;
            let ox = (w - draw_w) * 0.5;
            let oy = (h - draw_h) * 0.5;
            let inner_xf = Transform::from_scale(u, u).post_translate(ox, oy);
            box_pixmap.draw_pixmap(
                0,
                0,
                src_pixmap.as_ref(),
                &PixmapPaint {
                    opacity: 1.0,
                    quality: FilterQuality::Bilinear,
                    ..PixmapPaint::default()
                },
                inner_xf,
                None,
            );
            let outer_xf = Transform::from_translate(x, y).post_concat(pivot_xf);
            pixmap.draw_pixmap(0, 0, box_pixmap.as_ref(), &paint, outer_xf, None);
        }
    }
    Ok(())
}

fn apply_anchor(x: f32, y: f32, w: f32, h: f32, anchor: AnchorSpec) -> (f32, f32) {
    let (ax, ay) = anchor.factors();
    (x - ax * w, y - ay * h)
}

fn build_pivot_transform_with_skew(
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

fn resolve_dynamic_text<'a>(
    layer: &Layer,
    content: &'a str,
    dynamic: Option<&DynamicTextSpec>,
    time_sec: f32,
    scene_duration: f32,
) -> std::borrow::Cow<'a, str> {
    let Some(spec) = dynamic else {
        return std::borrow::Cow::Borrowed(content);
    };
    let layer_end = effective_end_sec(layer, scene_duration);
    match spec {
        DynamicTextSpec::Typewriter {
            text,
            start_sec,
            end_sec,
            cursor,
        } => {
            let s = start_sec.unwrap_or(layer.start_sec);
            let e = end_sec.unwrap_or(layer_end);
            let p = if e > s {
                ((time_sec - s) / (e - s)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let total_chars = text.chars().count();
            let take_n = ((total_chars as f32) * p).round() as usize;
            let mut out: String = text.chars().take(take_n).collect();
            if let Some(cur) = cursor {
                if p < 1.0 || ((time_sec * 2.0) as u32).is_multiple_of(2) {
                    out.push_str(cur);
                }
            }
            std::borrow::Cow::Owned(out)
        }
        DynamicTextSpec::Counter {
            from,
            to,
            decimals,
            prefix,
            suffix,
            start_sec,
            end_sec,
            easing,
        } => {
            let s = start_sec.unwrap_or(layer.start_sec);
            let e = end_sec.unwrap_or(layer_end);
            let raw_p = if e > s {
                ((time_sec - s) / (e - s)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let p = apply_easing(raw_p, *easing);
            let val = from + (to - from) * p;
            std::borrow::Cow::Owned(format!("{prefix}{val:.decimals$}{suffix}", decimals = *decimals))
        }
        DynamicTextSpec::Timecode { format, offset_sec } => {
            let t = (time_sec + *offset_sec).max(0.0);
            let total_secs = t.floor() as u32;
            let mins = total_secs / 60;
            let secs = total_secs % 60;
            let centis = ((t - t.floor()) * 100.0).floor() as u32;
            let fmt = format.to_ascii_lowercase();
            let s = if fmt == "mm:ss.cc" || fmt == "mm:ss:cc" {
                format!("{mins:02}:{secs:02}.{centis:02}")
            } else if fmt == "hh:mm:ss" {
                let hrs = mins / 60;
                let m = mins % 60;
                format!("{hrs:02}:{m:02}:{secs:02}")
            } else {
                format!("{mins:02}:{secs:02}")
            };
            std::borrow::Cow::Owned(s)
        }
    }
}

use std::sync::atomic::{AtomicU64, Ordering};

static SVG_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static SVG_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
static BLUR_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static BLUR_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

pub fn reset_cache_stats() {
    SVG_CACHE_HITS.store(0, Ordering::Relaxed);
    SVG_CACHE_MISSES.store(0, Ordering::Relaxed);
    BLUR_CACHE_HITS.store(0, Ordering::Relaxed);
    BLUR_CACHE_MISSES.store(0, Ordering::Relaxed);
}

pub fn get_cache_stats() -> (u64, u64, u64, u64) {
    (
        SVG_CACHE_HITS.load(Ordering::Relaxed),
        SVG_CACHE_MISSES.load(Ordering::Relaxed),
        BLUR_CACHE_HITS.load(Ordering::Relaxed),
        BLUR_CACHE_MISSES.load(Ordering::Relaxed),
    )
}

use crate::animation::{apply_easing, eval_color_property, eval_property};
use crate::assets::AssetStore;
use crate::schema::{
    AlignSpec, AnchorSpec, AnimProperty, BlendModeSpec, DynamicTextSpec, FitSpec,
    FlexDirectionSpec, FontWeightSpec, GradientTypeSpec, Layer, LayerKind, LayoutSpec,
    LinearGradientSpec, PositionSpec, Scene, TextAlignSpec, TextSpan, TimelineMode,
};

pub fn parse_hex_color(hex: &str, opacity_override: f32) -> Result<Color> {
    let hex = hex.trim_start_matches('#');
    let (r, g, b, a) = match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16)?;
            let g = u8::from_str_radix(&hex[2..4], 16)?;
            let b = u8::from_str_radix(&hex[4..6], 16)?;
            (r, g, b, 255)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16)?;
            let g = u8::from_str_radix(&hex[2..4], 16)?;
            let b = u8::from_str_radix(&hex[4..6], 16)?;
            let a = u8::from_str_radix(&hex[6..8], 16)?;
            (r, g, b, a)
        }
        _ => return Err(anyhow!("Invalid hex color: #{}", hex)),
    };
    let alpha = ((a as f32 / 255.0) * opacity_override.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    Color::from_rgba(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        alpha,
    )
    .ok_or_else(|| anyhow!("Invalid RGBA values"))
}

fn build_rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
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

thread_local! {
    static THREAD_FONT_SYS: RefCell<Option<(usize, FontSystem, SwashCache)>> = const { RefCell::new(None) };
    static THREAD_SVG_CACHE: RefCell<std::collections::HashMap<(u64, usize), resvg::usvg::Tree>> = RefCell::new(std::collections::HashMap::new());
    static THREAD_SVG_RASTER_CACHE: RefCell<std::collections::HashMap<(u64, usize, u32, u32), Pixmap>> = RefCell::new(std::collections::HashMap::new());
    static THREAD_BLUR_CACHE: RefCell<std::collections::HashMap<(u64, u32, u32), Option<(Pixmap, i32, i32)>>> = RefCell::new(std::collections::HashMap::new());
}

fn hash_bytes_fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn is_layer_fully_static(layer: &Layer, scene_duration: f32) -> bool {
    if layer.start_sec > 0.0 || !layer.animations.is_empty() {
        return false;
    }
    let end = layer
        .end_sec
        .or_else(|| layer.duration_secs.map(|d| layer.start_sec + d));
    if let Some(e) = end {
        if e < scene_duration - 1e-5 {
            return false;
        }
    }
    match &layer.kind {
        LayerKind::Video { .. } => false,
        LayerKind::Container { children, .. } => children
            .iter()
            .all(|c| is_layer_fully_static(c, scene_duration)),
        _ => true,
    }
}

/// Pre-renders any leading 100%-static background layers into a reusable base `Pixmap`.
/// Returns `(prefix_len, Some(base_pixmap))` if at least one static background layer exists.
pub fn build_static_prefix_pixmap(
    scene: &Scene,
    assets: &AssetStore,
) -> Result<(usize, Option<Pixmap>)> {
    let prefix_len = scene
        .layers
        .iter()
        .take_while(|l| is_layer_fully_static(l, scene.canvas.duration_secs))
        .count();
    if prefix_len == 0 {
        return Ok((0, None));
    }

    let width = scene.canvas.width;
    let height = scene.canvas.height;
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| anyhow!("Failed to allocate static base pixmap {}x{}", width, height))?;
    let bg = parse_hex_color(&scene.canvas.background, 1.0)?;
    pixmap.fill(bg);

    let font_count = assets.fonts.len();
    THREAD_FONT_SYS.with(|cell| -> Result<()> {
        let mut borrow = cell.borrow_mut();
        let needs_init = match &*borrow {
            Some((count, _, _)) => *count != font_count,
            None => true,
        };
        if needs_init {
            let mut fs = FontSystem::new();
            for font_bytes in &assets.fonts {
                fs.db_mut().load_font_data(font_bytes.clone());
            }
            *borrow = Some((font_count, fs, SwashCache::new()));
        }
        let (_, font_system, swash_cache) = borrow.as_mut().unwrap();
        for layer in &scene.layers[..prefix_len] {
            render_layer(
                layer,
                scene.canvas.duration_secs,
                0.0,
                assets,
                font_system,
                swash_cache,
                &mut pixmap,
                None,
                1.0,
            )?;
        }
        Ok(())
    })?;

    Ok((prefix_len, Some(pixmap)))
}

#[cfg(test)]
pub fn render_frame(scene: &Scene, assets: &AssetStore, frame_idx: u32) -> Result<Vec<u8>> {
    render_frame_with_base(scene, assets, frame_idx, 0, None)
}

pub fn render_frame_with_base(
    scene: &Scene,
    assets: &AssetStore,
    frame_idx: u32,
    static_prefix_len: usize,
    static_base: Option<&Pixmap>,
) -> Result<Vec<u8>> {
    let time_sec = frame_idx as f32 / scene.canvas.fps as f32;
    let (bytes, _) =
        render_frame_at_time_with_base_profile(scene, assets, time_sec, static_prefix_len, static_base, false)?;
    Ok(bytes)
}

pub fn render_frame_with_base_profiled(
    scene: &Scene,
    assets: &AssetStore,
    frame_idx: u32,
    static_prefix_len: usize,
    static_base: Option<&Pixmap>,
) -> Result<(Vec<u8>, Vec<u64>)> {
    let time_sec = frame_idx as f32 / scene.canvas.fps as f32;
    render_frame_at_time_with_base_profile(scene, assets, time_sec, static_prefix_len, static_base, true)
}

pub fn render_frame_at_time(scene: &Scene, assets: &AssetStore, time_sec: f32) -> Result<Vec<u8>> {
    let (bytes, _) = render_frame_at_time_with_base_profile(scene, assets, time_sec, 0, None, false)?;
    Ok(bytes)
}

pub fn render_frame_at_time_with_base_profile(
    scene: &Scene,
    assets: &AssetStore,
    time_sec: f32,
    static_prefix_len: usize,
    static_base: Option<&Pixmap>,
    collect_profile: bool,
) -> Result<(Vec<u8>, Vec<u64>)> {
    let width = scene.canvas.width;
    let height = scene.canvas.height;

    let mut pixmap = match static_base {
        Some(base) => base.clone(),
        None => {
            let mut p = Pixmap::new(width, height)
                .ok_or_else(|| anyhow!("Failed to allocate pixmap {}x{}", width, height))?;
            let bg = parse_hex_color(&scene.canvas.background, 1.0)?;
            p.fill(bg);
            p
        }
    };

    let mut layer_ns = if collect_profile {
        vec![0u64; scene.layers.len()]
    } else {
        Vec::new()
    };

    let font_count = assets.fonts.len();
    THREAD_FONT_SYS.with(|cell| -> Result<()> {
        let mut borrow = cell.borrow_mut();
        let needs_init = match &*borrow {
            Some((count, _, _)) => *count != font_count,
            None => true,
        };
        if needs_init {
            let mut fs = FontSystem::new();
            for font_bytes in &assets.fonts {
                fs.db_mut().load_font_data(font_bytes.clone());
            }
            *borrow = Some((font_count, fs, SwashCache::new()));
        }
        let (_, font_system, swash_cache) = borrow.as_mut().unwrap();

        for (idx, layer) in scene.layers[static_prefix_len..].iter().enumerate() {
            let t0 = if collect_profile {
                Some(std::time::Instant::now())
            } else {
                None
            };
            render_layer(
                layer,
                scene.canvas.duration_secs,
                time_sec,
                assets,
                font_system,
                swash_cache,
                &mut pixmap,
                None,
                1.0,
            )?;
            if let Some(start) = t0 {
                layer_ns[static_prefix_len + idx] += start.elapsed().as_nanos() as u64;
            }
        }
        Ok(())
    })?;

    Ok((pixmap.take(), layer_ns))
}

#[derive(Clone, Copy)]
struct LayoutBox {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    parent_x: f32,
    parent_y: f32,
}

fn resolve_xy(
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

fn map_blend_mode(mode: BlendModeSpec) -> SkiaBlendMode {
    match mode {
        BlendModeSpec::Normal => SkiaBlendMode::SourceOver,
        BlendModeSpec::Multiply => SkiaBlendMode::Multiply,
        BlendModeSpec::Screen => SkiaBlendMode::Screen,
        BlendModeSpec::Overlay => SkiaBlendMode::Overlay,
        BlendModeSpec::Darken => SkiaBlendMode::Darken,
        BlendModeSpec::Lighten => SkiaBlendMode::Lighten,
        BlendModeSpec::ColorDodge => SkiaBlendMode::ColorDodge,
        BlendModeSpec::ColorBurn => SkiaBlendMode::ColorBurn,
        BlendModeSpec::HardLight => SkiaBlendMode::HardLight,
        BlendModeSpec::SoftLight => SkiaBlendMode::SoftLight,
        BlendModeSpec::Difference => SkiaBlendMode::Difference,
        BlendModeSpec::Exclusion => SkiaBlendMode::Exclusion,
    }
}

fn box_blur_h(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let win = (r * 2 + 1) as u32;
    for y in 0..h {
        let row = y * w * 4;
        let mut sum = [0u32; 4];
        for dx in -(r as isize)..=(r as isize) {
            let sx = dx.clamp(0, (w - 1) as isize) as usize;
            let idx = row + sx * 4;
            for c in 0..4 {
                sum[c] += src[idx + c] as u32;
            }
        }
        for x in 0..w {
            let out = row + x * 4;
            for c in 0..4 {
                dst[out + c] = (sum[c] / win) as u8;
            }
            let rem_x = (x as isize - r as isize).clamp(0, (w - 1) as isize) as usize;
            let add_x = (x as isize + r as isize + 1).clamp(0, (w - 1) as isize) as usize;
            let rem_idx = row + rem_x * 4;
            let add_idx = row + add_x * 4;
            for c in 0..4 {
                sum[c] = sum[c] + src[add_idx + c] as u32 - src[rem_idx + c] as u32;
            }
        }
    }
}

fn box_blur_v(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let win = (r * 2 + 1) as u32;
    for x in 0..w {
        let mut sum = [0u32; 4];
        for dy in -(r as isize)..=(r as isize) {
            let sy = dy.clamp(0, (h - 1) as isize) as usize;
            let idx = (sy * w + x) * 4;
            for c in 0..4 {
                sum[c] += src[idx + c] as u32;
            }
        }
        for y in 0..h {
            let out = (y * w + x) * 4;
            for c in 0..4 {
                dst[out + c] = (sum[c] / win) as u8;
            }
            let rem_y = (y as isize - r as isize).clamp(0, (h - 1) as isize) as usize;
            let add_y = (y as isize + r as isize + 1).clamp(0, (h - 1) as isize) as usize;
            let rem_idx = (rem_y * w + x) * 4;
            let add_idx = (add_y * w + x) * 4;
            for c in 0..4 {
                sum[c] = sum[c] + src[add_idx + c] as u32 - src[rem_idx + c] as u32;
            }
        }
    }
}

/// 3-pass separable box blur approximating a Gaussian blur on premultiplied RGBA.
/// For large radii (>= 20.0), uses 2x downsampling + bilinear upsampling for ~4x speedup.
fn fast_blur_rgba(data: &mut [u8], w: u32, h: u32, radius: f32) {
    if radius >= 20.0 && w >= 64 && h >= 64 {
        let dw = (w / 2).max(1) as usize;
        let dh = (h / 2).max(1) as usize;
        let sw = w as usize;
        let sh = h as usize;
        let mut down = vec![0u8; dw * dh * 4];
        for dy in 0..dh {
            let sy0 = (dy * 2).min(sh - 1);
            let sy1 = (sy0 + 1).min(sh - 1);
            for dx in 0..dw {
                let sx0 = (dx * 2).min(sw - 1);
                let sx1 = (sx0 + 1).min(sw - 1);
                let i00 = (sy0 * sw + sx0) * 4;
                let i10 = (sy0 * sw + sx1) * 4;
                let i01 = (sy1 * sw + sx0) * 4;
                let i11 = (sy1 * sw + sx1) * 4;
                let out = (dy * dw + dx) * 4;
                for c in 0..4 {
                    down[out + c] = ((data[i00 + c] as u16
                        + data[i10 + c] as u16
                        + data[i01 + c] as u16
                        + data[i11 + c] as u16
                        + 2)
                        >> 2) as u8;
                }
            }
        }
        fast_blur_rgba_direct(&mut down, dw as u32, dh as u32, radius * 0.5);
        // Bilinear upsample back to w x h
        for y in 0..sh {
            let fy = ((y as f32 + 0.5) * 0.5 - 0.5).clamp(0.0, (dh - 1) as f32);
            let y0 = fy.floor() as usize;
            let y1 = (y0 + 1).min(dh - 1);
            let wy = fy - y0 as f32;
            let wy0 = 1.0 - wy;
            for x in 0..sw {
                let fx = ((x as f32 + 0.5) * 0.5 - 0.5).clamp(0.0, (dw - 1) as f32);
                let x0 = fx.floor() as usize;
                let x1 = (x0 + 1).min(dw - 1);
                let wx = fx - x0 as f32;
                let wx0 = 1.0 - wx;
                let w00 = wx0 * wy0;
                let w10 = wx * wy0;
                let w01 = wx0 * wy;
                let w11 = wx * wy;
                let i00 = (y0 * dw + x0) * 4;
                let i10 = (y0 * dw + x1) * 4;
                let i01 = (y1 * dw + x0) * 4;
                let i11 = (y1 * dw + x1) * 4;
                let out = (y * sw + x) * 4;
                for c in 0..4 {
                    data[out + c] = (down[i00 + c] as f32 * w00
                        + down[i10 + c] as f32 * w10
                        + down[i01 + c] as f32 * w01
                        + down[i11 + c] as f32 * w11)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
        }
        return;
    }
    fast_blur_rgba_direct(data, w, h, radius);
}

fn fast_blur_rgba_direct(data: &mut [u8], w: u32, h: u32, radius: f32) {
    let r = (radius * 0.5).round() as usize;
    if r == 0 || w == 0 || h == 0 {
        return;
    }
    let (w, h) = (w as usize, h as usize);
    let mut tmp = vec![0u8; data.len()];
    for _ in 0..3 {
        box_blur_h(data, &mut tmp, w, h, r);
        box_blur_v(&tmp, data, w, h, r);
    }
}

fn find_alpha_bounds(pixmap: &Pixmap) -> Option<(u32, u32, u32, u32)> {
    let w = pixmap.width();
    let h = pixmap.height();
    let data = pixmap.data();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;

    for y in 0..h {
        let row_start = (y as usize) * (w as usize) * 4;
        let row = &data[row_start..row_start + (w as usize) * 4];
        // Fast check: scan row for any non-zero alpha
        let mut row_min = w;
        let mut row_max = 0u32;
        for (x, px) in row.chunks_exact(4).enumerate() {
            if px[3] != 0 {
                let xu = x as u32;
                if xu < row_min {
                    row_min = xu;
                }
                row_max = xu + 1;
            }
        }
        if row_min < w {
            if y < min_y {
                min_y = y;
            }
            max_y = y + 1;
            if row_min < min_x {
                min_x = row_min;
            }
            if row_max > max_x {
                max_x = row_max;
            }
        }
    }

    if min_x < max_x && min_y < max_y {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

fn crop_pixmap_padded(
    src: &Pixmap,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    pad: u32,
) -> Result<(Pixmap, i32, i32)> {
    let content_w = x1 - x0;
    let content_h = y1 - y0;
    let crop_w = (content_w + pad * 2).max(1);
    let crop_h = (content_h + pad * 2).max(1);
    let mut out = Pixmap::new(crop_w, crop_h)
        .ok_or_else(|| anyhow!("Failed to allocate cropped pixmap {}x{}", crop_w, crop_h))?;
    let sw = src.width() as usize;
    let dw = crop_w as usize;
    let src_data = src.data();
    let dst_data = out.data_mut();
    let row_bytes = (content_w as usize) * 4;
    for row in 0..(content_h as usize) {
        let s_off = ((y0 as usize + row) * sw + (x0 as usize)) * 4;
        let d_off = ((pad as usize + row) * dw + (pad as usize)) * 4;
        dst_data[d_off..d_off + row_bytes].copy_from_slice(&src_data[s_off..s_off + row_bytes]);
    }
    Ok((out, x0 as i32 - pad as i32, y0 as i32 - pad as i32))
}

fn build_shadow_pixmap_cropped(
    src: &Pixmap,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    color_hex: &str,
    blur_radius: f32,
) -> Result<(Pixmap, i32, i32)> {
    let sc = parse_hex_color(color_hex, 1.0)?;
    let pad = if blur_radius > 0.0 {
        (blur_radius * 1.6).ceil() as u32 + 2
    } else {
        0
    };
    let content_w = x1 - x0;
    let content_h = y1 - y0;
    let crop_w = (content_w + pad * 2).max(1);
    let crop_h = (content_h + pad * 2).max(1);
    let mut shadow = Pixmap::new(crop_w, crop_h)
        .ok_or_else(|| anyhow!("Failed to allocate shadow pixmap {}x{}", crop_w, crop_h))?;
    let sw = src.width() as usize;
    let dw = crop_w as usize;
    let src_data = src.data();
    let dst_data = shadow.data_mut();
    let sr = sc.red();
    let sg = sc.green();
    let sb = sc.blue();
    let sa = sc.alpha();

    for row in 0..(content_h as usize) {
        let s_off = ((y0 as usize + row) * sw + (x0 as usize)) * 4;
        let d_off = ((pad as usize + row) * dw + (pad as usize)) * 4;
        let s_row = &src_data[s_off..s_off + (content_w as usize) * 4];
        let d_row = &mut dst_data[d_off..d_off + (content_w as usize) * 4];
        for (s, d) in s_row.chunks_exact(4).zip(d_row.chunks_exact_mut(4)) {
            let a = (s[3] as f32 / 255.0) * sa;
            if a > 0.0 {
                d[0] = (sr * a * 255.0).round().clamp(0.0, 255.0) as u8;
                d[1] = (sg * a * 255.0).round().clamp(0.0, 255.0) as u8;
                d[2] = (sb * a * 255.0).round().clamp(0.0, 255.0) as u8;
                d[3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    if blur_radius > 0.0 {
        fast_blur_rgba(shadow.data_mut(), crop_w, crop_h, blur_radius);
    }
    Ok((shadow, x0 as i32 - pad as i32, y0 as i32 - pad as i32))
}

fn effective_end_sec(layer: &Layer, parent_duration: f32) -> f32 {
    layer
        .end_sec
        .or_else(|| layer.duration_secs.map(|d| layer.start_sec + d))
        .unwrap_or(parent_duration)
}

fn resolve_child_timelines(
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

#[allow(clippy::too_many_arguments)]
fn render_layer(
    layer: &Layer,
    scene_duration: f32,
    time_sec: f32,
    assets: &AssetStore,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    pixmap: &mut Pixmap,
    layout_override: Option<LayoutBox>,
    parent_opacity: f32,
) -> Result<()> {
    render_layer_scoped(
        layer,
        scene_duration,
        time_sec,
        assets,
        font_system,
        swash_cache,
        pixmap,
        layout_override,
        parent_opacity,
        false,
    )
}

fn layer_base_opacity(kind: &LayerKind) -> f32 {
    match kind {
        LayerKind::Rect { opacity, .. }
        | LayerKind::Circle { opacity, .. }
        | LayerKind::Text { opacity, .. }
        | LayerKind::Image { opacity, .. }
        | LayerKind::Svg { opacity, .. }
        | LayerKind::Video { opacity, .. }
        | LayerKind::Container { opacity, .. } => *opacity,
    }
}

fn set_layer_opacity(kind: &mut LayerKind, val: f32) {
    match kind {
        LayerKind::Rect { opacity, .. }
        | LayerKind::Circle { opacity, .. }
        | LayerKind::Text { opacity, .. }
        | LayerKind::Image { opacity, .. }
        | LayerKind::Svg { opacity, .. }
        | LayerKind::Video { opacity, .. }
        | LayerKind::Container { opacity, .. } => *opacity = val,
    }
}

fn can_use_blur_cache(layer: &Layer, layout_override: Option<LayoutBox>) -> bool {
    layout_override.is_none()
        && layer.blur > 0.0
        && layer.shadow.is_none()
        && !matches!(layer.kind, LayerKind::Video { .. } | LayerKind::Container { .. })
        && layer
            .animations
            .iter()
            .all(|a| a.property == AnimProperty::Opacity)
}

#[allow(clippy::too_many_arguments)]
fn render_layer_scoped(
    layer: &Layer,
    scene_duration: f32,
    time_sec: f32,
    assets: &AssetStore,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    pixmap: &mut Pixmap,
    layout_override: Option<LayoutBox>,
    parent_opacity: f32,
    skip_window_check: bool,
) -> Result<()> {
    if !skip_window_check {
        let end_sec = effective_end_sec(layer, scene_duration);
        if time_sec < layer.start_sec || time_sec > end_sec {
            return Ok(());
        }
    }

    let needs_fx = layer.shadow.is_some()
        || layer.blur > 0.0
        || layer.blend_mode != BlendModeSpec::Normal;
    if !needs_fx {
        return render_layer_inner(
            layer,
            scene_duration,
            time_sec,
            assets,
            font_system,
            swash_cache,
            pixmap,
            layout_override,
            parent_opacity,
        );
    }

    if parent_opacity <= 0.0 {
        return Ok(());
    }

    let blend = map_blend_mode(layer.blend_mode);
    let (pw, ph) = (pixmap.width(), pixmap.height());

    if can_use_blur_cache(layer, layout_override) {
        let eff_op = eval_property(
            &layer.animations,
            AnimProperty::Opacity,
            time_sec,
            layer_base_opacity(&layer.kind),
        ) * parent_opacity;
        if eff_op <= 0.0 {
            return Ok(());
        }

        let mut un_faded = layer.clone();
        un_faded
            .animations
            .retain(|a| a.property != AnimProperty::Opacity);
        set_layer_opacity(&mut un_faded.kind, 1.0);
        let sig = format!("{:?}|{:.3}", un_faded.kind, layer.blur);
        let key = (hash_bytes_fnv1a(sig.as_bytes()), pw, ph);

        return THREAD_BLUR_CACHE.with(|cell| -> Result<()> {
            let mut cache = cell.borrow_mut();
            if !cache.contains_key(&key) {
                BLUR_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
                let mut layer_pixmap = Pixmap::new(pw, ph)
                    .ok_or_else(|| anyhow!("Failed to allocate layer fx pixmap {}x{}", pw, ph))?;
                render_layer_inner(
                    &un_faded,
                    scene_duration,
                    time_sec,
                    assets,
                    font_system,
                    swash_cache,
                    &mut layer_pixmap,
                    None,
                    1.0,
                )?;
                let entry = if let Some((x0, y0, x1, y1)) = find_alpha_bounds(&layer_pixmap) {
                    let pad = (layer.blur * 1.6).ceil() as u32 + 2;
                    let (mut blur_pixmap, bx, by) =
                        crop_pixmap_padded(&layer_pixmap, x0, y0, x1, y1, pad)?;
                    let (bw, bh) = (blur_pixmap.width(), blur_pixmap.height());
                    fast_blur_rgba(blur_pixmap.data_mut(), bw, bh, layer.blur);
                    Some((blur_pixmap, bx, by))
                } else {
                    None
                };
                cache.insert(key, entry);
            } else {
                BLUR_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            }
            if let Some((blur_pixmap, bx, by)) = cache.get(&key).unwrap() {
                let paint = PixmapPaint {
                    opacity: eff_op.clamp(0.0, 1.0),
                    blend_mode: blend,
                    quality: FilterQuality::Bilinear,
                };
                pixmap.draw_pixmap(
                    0,
                    0,
                    blur_pixmap.as_ref(),
                    &paint,
                    Transform::from_translate(*bx as f32, *by as f32),
                    None,
                );
            }
            Ok(())
        });
    }
    let mut layer_pixmap = Pixmap::new(pw, ph)
        .ok_or_else(|| anyhow!("Failed to allocate layer fx pixmap {}x{}", pw, ph))?;
    render_layer_inner(
        layer,
        scene_duration,
        time_sec,
        assets,
        font_system,
        swash_cache,
        &mut layer_pixmap,
        layout_override,
        parent_opacity,
    )?;

    let Some((x0, y0, x1, y1)) = find_alpha_bounds(&layer_pixmap) else {
        return Ok(());
    };

    if let Some(sh) = &layer.shadow {
        let (shadow_pixmap, sx, sy) =
            build_shadow_pixmap_cropped(&layer_pixmap, x0, y0, x1, y1, &sh.color, sh.blur)?;
        let spaint = PixmapPaint {
            opacity: 1.0,
            blend_mode: blend,
            quality: FilterQuality::Bilinear,
        };
        pixmap.draw_pixmap(
            0,
            0,
            shadow_pixmap.as_ref(),
            &spaint,
            Transform::from_translate(sx as f32 + sh.offset_x, sy as f32 + sh.offset_y),
            None,
        );
    }

    let paint = PixmapPaint {
        opacity: 1.0,
        blend_mode: blend,
        quality: FilterQuality::Bilinear,
    };
    if layer.blur > 0.0 {
        let pad = (layer.blur * 1.6).ceil() as u32 + 2;
        let (mut blur_pixmap, bx, by) = crop_pixmap_padded(&layer_pixmap, x0, y0, x1, y1, pad)?;
        let (bw, bh) = (blur_pixmap.width(), blur_pixmap.height());
        fast_blur_rgba(blur_pixmap.data_mut(), bw, bh, layer.blur);
        pixmap.draw_pixmap(
            0,
            0,
            blur_pixmap.as_ref(),
            &paint,
            Transform::from_translate(bx as f32, by as f32),
            None,
        );
    } else {
        pixmap.draw_pixmap(0, 0, layer_pixmap.as_ref(), &paint, Transform::identity(), None);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_layer_inner(
    layer: &Layer,
    scene_duration: f32,
    time_sec: f32,
    assets: &AssetStore,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    pixmap: &mut Pixmap,
    layout_override: Option<LayoutBox>,
    parent_opacity: f32,
) -> Result<()> {

    match &layer.kind {
        LayerKind::Rect {
            x,
            y,
            width: w,
            height: h,
            radius,
            color,
            gradient,
            border_color,
            border_width,
            path_progress,
            stroke_dashoffset,
            opacity,
            scale,
            rotation,
        } => {
            let base_w = layout_override.map(|b| b.w).unwrap_or(*w);
            let base_h = layout_override.map(|b| b.h).unwrap_or(*h);

            let (x, y) = resolve_xy(layer, layout_override, *x, *y, time_sec);
            let w = eval_property(&layer.animations, AnimProperty::Width, time_sec, base_w);
            let h = eval_property(&layer.animations, AnimProperty::Height, time_sec, base_h);
            let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity) * parent_opacity;
            let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
            let rot = eval_property(&layer.animations, AnimProperty::Rotation, time_sec, *rotation);
            let eff_color = eval_color_property(&layer.animations, AnimProperty::Color, time_sec, Some(color))
                .unwrap_or_else(|| color.clone());
            let eff_border_color = eval_color_property(
                &layer.animations,
                AnimProperty::BorderColor,
                time_sec,
                border_color.as_deref(),
            );
            let prog = eval_property(&layer.animations, AnimProperty::PathProgress, time_sec, *path_progress).clamp(0.0, 1.0);
            let has_offset_anim = layer.animations.iter().any(|a| a.property == AnimProperty::StrokeDashoffset);
            let dash_offset = if stroke_dashoffset.is_some() || has_offset_anim {
                Some(eval_property(&layer.animations, AnimProperty::StrokeDashoffset, time_sec, stroke_dashoffset.unwrap_or(0.0)))
            } else {
                None
            };

            if w <= 0.0 || h <= 0.0 || op <= 0.0 || s <= 0.0 {
                return Ok(());
            }

            let eff_radius = eval_property(&layer.animations, AnimProperty::Radius, time_sec, *radius).max(0.0);
            let (x, y) = apply_anchor(x, y, w, h, layer.anchor);
            let sk_x = eval_property(&layer.animations, AnimProperty::SkewX, time_sec, layer.skew_x);
            let sk_y = eval_property(&layer.animations, AnimProperty::SkewY, time_sec, layer.skew_y);
            if let Some(path) = build_rounded_rect(x, y, w, h, eff_radius) {
                let xform = build_pivot_transform_with_skew(x, y, w, h, s, rot, sk_x, sk_y);
                let perimeter = 2.0 * (w + h);
                let stroke_dash = build_progress_stroke_dash(perimeter, prog, dash_offset, None);
                draw_styled_path(
                    pixmap,
                    &path,
                    x,
                    y,
                    w,
                    h,
                    Some(&eff_color),
                    gradient.as_ref(),
                    eff_border_color.as_deref(),
                    *border_width,
                    stroke_dash,
                    op,
                    xform,
                )?;
            }
        }
        LayerKind::Circle {
            cx,
            cy,
            r,
            color,
            border_color,
            border_width,
            path_progress,
            stroke_dashoffset,
            opacity,
            scale,
        } => {
            let cx_base_offset = layout_override.map(|b| b.x + b.w * 0.5 - *cx).unwrap_or(0.0);
            let cy_base_offset = layout_override.map(|b| b.y + b.h * 0.5 - *cy).unwrap_or(0.0);

            let cx = cx_base_offset
                + eval_property(&layer.animations, AnimProperty::Cx, time_sec, *cx)
                + eval_property(&layer.animations, AnimProperty::X, time_sec, 0.0);
            let cy = cy_base_offset
                + eval_property(&layer.animations, AnimProperty::Cy, time_sec, *cy)
                + eval_property(&layer.animations, AnimProperty::Y, time_sec, 0.0);
            let scale_val = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
            let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity) * parent_opacity;
            let r = eval_property(&layer.animations, AnimProperty::R, time_sec, *r) * scale_val;
            let eff_color = eval_color_property(&layer.animations, AnimProperty::Color, time_sec, Some(color))
                .unwrap_or_else(|| color.clone());
            let eff_border_color = eval_color_property(
                &layer.animations,
                AnimProperty::BorderColor,
                time_sec,
                border_color.as_deref(),
            );
            let prog = eval_property(&layer.animations, AnimProperty::PathProgress, time_sec, *path_progress).clamp(0.0, 1.0);
            let has_offset_anim = layer.animations.iter().any(|a| a.property == AnimProperty::StrokeDashoffset);
            let dash_offset = if stroke_dashoffset.is_some() || has_offset_anim {
                Some(eval_property(&layer.animations, AnimProperty::StrokeDashoffset, time_sec, stroke_dashoffset.unwrap_or(0.0)))
            } else {
                None
            };

            if r <= 0.0 || op <= 0.0 {
                return Ok(());
            }

            if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
                let mut paint = Paint::default();
                paint.set_color(parse_hex_color(&eff_color, op)?);
                paint.anti_alias = true;
                pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);

                if let Some(b_hex) = eff_border_color.as_deref() {
                    if *border_width > 0.0 {
                        let mut b_paint = Paint::default();
                        b_paint.set_color(parse_hex_color(b_hex, op)?);
                        b_paint.anti_alias = true;
                        let circumference = 2.0 * std::f32::consts::PI * r;
                        let stroke = Stroke {
                            width: *border_width,
                            dash: build_progress_stroke_dash(circumference, prog, dash_offset, None),
                            ..Stroke::default()
                        };
                        pixmap.stroke_path(&path, &b_paint, &stroke, Transform::identity(), None);
                    }
                }
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
            letter_spacing,
            font_family,
            font_weight,
            text_align,
            color,
            gradient,
            outline,
            opacity,
            max_width,
            scale,
        } => {
            let effective_max_w = max_width.or_else(|| layout_override.map(|b| b.w));

            let (x, y) = resolve_xy(layer, layout_override, *x, *y, time_sec);
            let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity) * parent_opacity;
            let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);

            if op <= 0.0 || s <= 0.0 {
                return Ok(());
            }

            let eff_color = eval_color_property(&layer.animations, AnimProperty::Color, time_sec, Some(color))
                .unwrap_or_else(|| color.clone());
            let c = parse_hex_color(&eff_color, op)?;
            let text_color = CosmicColor::rgba(
                (c.red() * 255.0) as u8,
                (c.green() * 255.0) as u8,
                (c.blue() * 255.0) as u8,
                (c.alpha() * 255.0) as u8,
            );

            let resolved_content =
                resolve_dynamic_text(layer, content, dynamic.as_ref(), time_sec, scene_duration);
            let effective_spans: &[TextSpan] = if dynamic.is_some() { &[] } else { spans };
            let (mw, mh) = measure_text_size(
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
            let bw = effective_max_w.unwrap_or(mw);
            let (x, y) = apply_anchor(x, y, bw, mh, layer.anchor);
            let sk_x = eval_property(&layer.animations, AnimProperty::SkewX, time_sec, layer.skew_x);
            let sk_y = eval_property(&layer.animations, AnimProperty::SkewY, time_sec, layer.skew_y);
            let text_xform = build_pivot_transform_with_skew(x, y, bw, mh, 1.0, 0.0, sk_x, sk_y);

            let buffer = build_text_buffer(
                font_system,
                &resolved_content,
                effective_spans,
                *font_size,
                *line_height,
                font_family.as_deref(),
                *font_weight,
                *text_align,
                text_color,
                op,
                Some(effective_max_w.unwrap_or(pixmap.width() as f32)),
                Some(pixmap.height() as f32),
                s,
            )?;

            let ls = *letter_spacing * s;
            let use_fx_buffer = gradient.is_some()
                || outline.as_ref().is_some_and(|o| o.width > 0.0);

            if !use_fx_buffer {
                let mut draw_pixel = |gx: i32, gy: i32, w: u32, h: u32, color: CosmicColor| {
                    if w == 0 || h == 0 {
                        return;
                    }
                    let a = color.a() as f32 / 255.0;
                    if a <= 0.0 {
                        return;
                    }
                    if let Some(rect) = Rect::from_xywh(gx as f32, gy as f32, w as f32, h as f32) {
                        let mut paint = Paint::default();
                        paint.set_color(
                            Color::from_rgba(
                                color.r() as f32 / 255.0,
                                color.g() as f32 / 255.0,
                                color.b() as f32 / 255.0,
                                a,
                            )
                            .unwrap_or(Color::WHITE),
                        );
                        pixmap.fill_rect(rect, &paint, text_xform, None);
                    }
                };

                if ls.abs() < 1e-4 {
                    buffer.draw(font_system, swash_cache, text_color, |px, py, w, h, col| {
                        draw_pixel(x as i32 + px, y as i32 + py, w, h, col);
                    });
                } else {
                    for run in buffer.layout_runs() {
                        let glyph_count = run.glyphs.len();
                        let extra_w = (glyph_count.saturating_sub(1) as f32) * ls;
                        let align_shift = match text_align {
                            TextAlignSpec::Left => 0.0,
                            TextAlignSpec::Center => -extra_w * 0.5,
                            TextAlignSpec::Right => -extra_w,
                        };
                        for (idx, glyph) in run.glyphs.iter().enumerate() {
                            let physical_glyph = glyph.physical((0., 0.), 1.0);
                            let glyph_color = glyph.color_opt.unwrap_or(text_color);
                            let shift_x = (align_shift + (idx as f32) * ls).round() as i32;
                            swash_cache.with_pixels(
                                font_system,
                                physical_glyph.cache_key,
                                glyph_color,
                                |px, py, col| {
                                    draw_pixel(
                                        x as i32 + physical_glyph.x + shift_x + px,
                                        y as i32 + run.line_y as i32 + physical_glyph.y + py,
                                        1,
                                        1,
                                        col,
                                    );
                                },
                            );
                        }
                    }
                }
            } else {
                let pad = outline
                    .as_ref()
                    .map(|o| o.width.ceil().max(0.0) as i32)
                    .unwrap_or(0);
                let tb_w = (bw.ceil() as i32 + pad * 2 + 4).max(1) as u32;
                let tb_h = (mh.ceil() as i32 + pad * 2 + 4).max(1) as u32;
                let mut fill_pm = Pixmap::new(tb_w, tb_h)
                    .ok_or_else(|| anyhow!("Failed to allocate text fx pixmap {}x{}", tb_w, tb_h))?;

                let mut plot_local = |lx: i32, ly: i32, w: u32, h: u32, col: CosmicColor| {
                    if w == 0 || h == 0 || col.a() == 0 {
                        return;
                    }
                    if let Some(rect) = Rect::from_xywh(
                        (lx + pad) as f32,
                        (ly + pad) as f32,
                        w as f32,
                        h as f32,
                    ) {
                        let mut p = Paint::default();
                        p.set_color(
                            Color::from_rgba(
                                col.r() as f32 / 255.0,
                                col.g() as f32 / 255.0,
                                col.b() as f32 / 255.0,
                                col.a() as f32 / 255.0,
                            )
                            .unwrap_or(Color::WHITE),
                        );
                        fill_pm.fill_rect(rect, &p, Transform::identity(), None);
                    }
                };

                if ls.abs() < 1e-4 {
                    buffer.draw(font_system, swash_cache, text_color, |px, py, w, h, col| {
                        plot_local(px, py, w, h, col);
                    });
                } else {
                    for run in buffer.layout_runs() {
                        let glyph_count = run.glyphs.len();
                        let extra_w = (glyph_count.saturating_sub(1) as f32) * ls;
                        let align_shift = match text_align {
                            TextAlignSpec::Left => 0.0,
                            TextAlignSpec::Center => -extra_w * 0.5,
                            TextAlignSpec::Right => -extra_w,
                        };
                        for (idx, glyph) in run.glyphs.iter().enumerate() {
                            let physical_glyph = glyph.physical((0., 0.), 1.0);
                            let glyph_color = glyph.color_opt.unwrap_or(text_color);
                            let shift_x = (align_shift + (idx as f32) * ls).round() as i32;
                            swash_cache.with_pixels(
                                font_system,
                                physical_glyph.cache_key,
                                glyph_color,
                                |px, py, col| {
                                    plot_local(
                                        physical_glyph.x + shift_x + px,
                                        run.line_y as i32 + physical_glyph.y + py,
                                        1,
                                        1,
                                        col,
                                    );
                                },
                            );
                        }
                    }
                }

                if let Some(grad) = gradient {
                    if let Some(shader) = build_gradient_shader(grad, pad as f32, pad as f32, bw, mh, op)? {
                        let mut gp = Paint {
                            shader,
                            blend_mode: SkiaBlendMode::SourceIn,
                            ..Paint::default()
                        };
                        gp.anti_alias = true;
                        if let Some(r) = Rect::from_xywh(0.0, 0.0, tb_w as f32, tb_h as f32) {
                            fill_pm.fill_rect(r, &gp, Transform::identity(), None);
                        }
                    }
                }

                let dst_xform = Transform::from_translate(x - pad as f32, y - pad as f32)
                    .post_concat(text_xform);

                if let Some(out_spec) = outline {
                    if out_spec.width > 0.0 {
                        let oc = parse_hex_color(&out_spec.color, op)?;
                        let mut out_pm = Pixmap::new(tb_w, tb_h).ok_or_else(|| {
                            anyhow!("Failed to allocate text outline pixmap {}x{}", tb_w, tb_h)
                        })?;
                        let r_px = out_spec.width.ceil() as i32;
                        let r2 = (out_spec.width * out_spec.width) + 0.5;
                        let src_d = fill_pm.data();
                        let dst_d = out_pm.data_mut();
                        let uw = tb_w as i32;
                        let uh = tb_h as i32;
                        for oy in 0..uh {
                            for ox in 0..uw {
                                let mut max_a = 0u8;
                                for dy in -r_px..=r_px {
                                    let sy = oy + dy;
                                    if sy < 0 || sy >= uh {
                                        continue;
                                    }
                                    for dx in -r_px..=r_px {
                                        if (dx * dx + dy * dy) as f32 > r2 {
                                            continue;
                                        }
                                        let sx = ox + dx;
                                        if sx < 0 || sx >= uw {
                                            continue;
                                        }
                                        let a = src_d[((sy * uw + sx) * 4 + 3) as usize];
                                        if a > max_a {
                                            max_a = a;
                                            if max_a == 255 {
                                                break;
                                            }
                                        }
                                    }
                                    if max_a == 255 {
                                        break;
                                    }
                                }
                                if max_a > 0 {
                                    let a_f = (max_a as f32 / 255.0) * oc.alpha();
                                    let idx = ((oy * uw + ox) * 4) as usize;
                                    dst_d[idx] = (oc.red() * a_f * 255.0).round().clamp(0.0, 255.0) as u8;
                                    dst_d[idx + 1] = (oc.green() * a_f * 255.0).round().clamp(0.0, 255.0) as u8;
                                    dst_d[idx + 2] = (oc.blue() * a_f * 255.0).round().clamp(0.0, 255.0) as u8;
                                    dst_d[idx + 3] = (a_f * 255.0).round().clamp(0.0, 255.0) as u8;
                                }
                            }
                        }
                        pixmap.draw_pixmap(
                            0,
                            0,
                            out_pm.as_ref(),
                            &PixmapPaint::default(),
                            dst_xform,
                            None,
                        );
                    }
                }

                pixmap.draw_pixmap(
                    0,
                    0,
                    fill_pm.as_ref(),
                    &PixmapPaint::default(),
                    dst_xform,
                    None,
                );
            }
        }
        LayerKind::Image {
            x,
            y,
            width,
            height,
            src,
            fit,
            opacity,
            rotation,
            scale,
        } => {
            let img = assets
                .images
                .get(src)
                .ok_or_else(|| anyhow!("Image not preloaded: {}", src))?;
            draw_rgba_frame(
                pixmap,
                layer,
                layout_override,
                time_sec,
                parent_opacity,
                *x,
                *y,
                *width,
                *height,
                *opacity,
                *rotation,
                *scale,
                *fit,
                img.width(),
                img.height(),
                img.as_raw(),
            )?;
        }
        LayerKind::Video {
            x,
            y,
            width,
            height,
            src,
            fit,
            trim_start_sec,
            trim_end_sec,
            playback_rate,
            opacity,
            rotation,
            scale,
            ..
        } => {
            let video = assets
                .videos
                .get(src)
                .ok_or_else(|| anyhow!("Video not preloaded: {}", src))?;
            let local_t = (time_sec - layer.start_sec).max(0.0);
            let mut video_t = *trim_start_sec + local_t * playback_rate.max(0.1);
            if let Some(te) = trim_end_sec {
                video_t = video_t.min(*te);
            }
            let frame_idx = ((video_t * video.fps).floor() as usize)
                .min(video.frames.len().saturating_sub(1));
            draw_rgba_frame(
                pixmap,
                layer,
                layout_override,
                time_sec,
                parent_opacity,
                *x,
                *y,
                *width,
                *height,
                *opacity,
                *rotation,
                *scale,
                *fit,
                video.width,
                video.height,
                &video.frames[frame_idx],
            )?;
        }
        LayerKind::Svg {
            x,
            y,
            width,
            height,
            src,
            path_progress,
            stroke_dashoffset,
            stroke_dasharray,
            opacity,
            rotation,
        } => {
            let svg_bytes = assets
                .svgs
                .get(src)
                .ok_or_else(|| anyhow!("SVG not preloaded: {}", src))?;

            let svg_key = (hash_bytes_fnv1a(svg_bytes), svg_bytes.len());
            THREAD_SVG_CACHE.with(|cell| -> Result<()> {
                let mut cache = cell.borrow_mut();
                if !cache.contains_key(&svg_key) {
                    let parsed =
                        resvg::usvg::Tree::from_data(svg_bytes, &resvg::usvg::Options::default())
                            .context("Failed to parse SVG")?;
                    cache.insert(svg_key, parsed);
                }
                let tree = cache.get(&svg_key).unwrap();

                let svg_size = tree.size();
                let target_w = layout_override
                    .map(|b| b.w)
                    .or(*width)
                    .unwrap_or(svg_size.width());
                let target_h = layout_override
                    .map(|b| b.h)
                    .or(*height)
                    .unwrap_or(svg_size.height());

                let (x, y) = resolve_xy(layer, layout_override, *x, *y, time_sec);
                let w = eval_property(&layer.animations, AnimProperty::Width, time_sec, target_w);
                let h = eval_property(&layer.animations, AnimProperty::Height, time_sec, target_h);
                let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
                    * parent_opacity;
                let rot = eval_property(&layer.animations, AnimProperty::Rotation, time_sec, *rotation);
                let prog = eval_property(&layer.animations, AnimProperty::PathProgress, time_sec, *path_progress)
                    .clamp(0.0, 1.0);
                let has_offset_anim = layer
                    .animations
                    .iter()
                    .any(|a| a.property == AnimProperty::StrokeDashoffset);
                let dash_offset = if stroke_dashoffset.is_some() || has_offset_anim {
                    Some(eval_property(
                        &layer.animations,
                        AnimProperty::StrokeDashoffset,
                        time_sec,
                        stroke_dashoffset.unwrap_or(0.0),
                    ))
                } else {
                    None
                };

                if w <= 0.0 || h <= 0.0 || op <= 0.0 {
                    return Ok(());
                }

                let (x, y) = apply_anchor(x, y, w, h, layer.anchor);
                let sk_x = eval_property(&layer.animations, AnimProperty::SkewX, time_sec, layer.skew_x);
                let sk_y = eval_property(&layer.animations, AnimProperty::SkewY, time_sec, layer.skew_y);
                let sx = w / svg_size.width().max(1.0);
                let sy = h / svg_size.height().max(1.0);
                let animated_stroke =
                    prog < 1.0 || dash_offset.is_some() || stroke_dasharray.is_some();

                let pw = (w.ceil() as u32).max(1);
                let ph = (h.ceil() as u32).max(1);

                if !animated_stroke {
                    let raster_key = (svg_key.0, svg_key.1, pw, ph);
                    return THREAD_SVG_RASTER_CACHE.with(|rcell| -> Result<()> {
                        let mut rcache = rcell.borrow_mut();
                        if !rcache.contains_key(&raster_key) {
                            SVG_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
                            let mut svg_pixmap = Pixmap::new(pw, ph)
                                .ok_or_else(|| anyhow!("Failed to allocate SVG pixmap {}x{}", pw, ph))?;
                            let scale_xform = Transform::from_scale(sx, sy);
                            resvg::render(tree, scale_xform, &mut svg_pixmap.as_mut());
                            rcache.insert(raster_key, svg_pixmap);
                        } else {
                            SVG_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                        }
                        let cached = rcache.get(&raster_key).unwrap();
                        let paint = PixmapPaint {
                            opacity: op.clamp(0.0, 1.0),
                            quality: FilterQuality::Bilinear,
                            ..PixmapPaint::default()
                        };
                        let pivot_xf = build_pivot_transform_with_skew(
                            x, y, w, h, 1.0, rot, sk_x, sk_y,
                        );
                        let xform = Transform::from_translate(x, y).post_concat(pivot_xf);
                        pixmap.draw_pixmap(0, 0, cached.as_ref(), &paint, xform, None);
                        Ok(())
                    });
                }

                let mut svg_pixmap = Pixmap::new(pw, ph)
                    .ok_or_else(|| anyhow!("Failed to allocate SVG pixmap {}x{}", pw, ph))?;
                let scale_xform = Transform::from_scale(sx, sy);
                render_usvg_group_animated(
                    tree.root(),
                    &mut svg_pixmap,
                    scale_xform,
                    prog,
                    dash_offset,
                    stroke_dasharray.as_deref(),
                );

                let paint = PixmapPaint {
                    opacity: op.clamp(0.0, 1.0),
                    quality: FilterQuality::Bilinear,
                    ..PixmapPaint::default()
                };
                let xform = if rot != 0.0 {
                    Transform::from_translate(x, y).post_rotate_at(rot, x + w * 0.5, y + h * 0.5)
                } else {
                    Transform::from_translate(x, y)
                };
                pixmap.draw_pixmap(0, 0, svg_pixmap.as_ref(), &paint, xform, None);
                Ok(())
            })?;
        }
        LayerKind::Container {
            x,
            y,
            width,
            height,
            layout,
            background,
            gradient,
            border_color,
            border_width,
            radius,
            opacity,
            scale,
            rotation,
            clip,
            cascade_opacity,
            timeline,
            stagger_secs,
            children,
        } => {
            let (cx, cy) = resolve_xy(layer, layout_override, *x, *y, time_sec);
            let has_w_anim = layer
                .animations
                .iter()
                .any(|a| a.property == AnimProperty::Width);
            let has_h_anim = layer
                .animations
                .iter()
                .any(|a| a.property == AnimProperty::Height);
            let target_w = if let Some(b) = layout_override {
                Some(if has_w_anim {
                    eval_property(&layer.animations, AnimProperty::Width, time_sec, b.w)
                } else {
                    b.w
                })
            } else {
                width.map(|w| eval_property(&layer.animations, AnimProperty::Width, time_sec, w))
            };
            let target_h = if let Some(b) = layout_override {
                Some(if has_h_anim {
                    eval_property(&layer.animations, AnimProperty::Height, time_sec, b.h)
                } else {
                    b.h
                })
            } else {
                height.map(|h| eval_property(&layer.animations, AnimProperty::Height, time_sec, h))
            };
            let op =
                eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
                    * parent_opacity;
            let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
            let rot = eval_property(&layer.animations, AnimProperty::Rotation, time_sec, *rotation);

            if op <= 0.0 || s <= 0.0 {
                return Ok(());
            }

            let (cw, ch, child_boxes) = compute_flex_layout(
                target_w,
                target_h,
                layout,
                children,
                font_system,
                time_sec,
            )?;

            if cw <= 0.0 || ch <= 0.0 {
                return Ok(());
            }

            let (cx, cy) = apply_anchor(cx, cy, cw, ch, layer.anchor);
            let sk_x = eval_property(&layer.animations, AnimProperty::SkewX, time_sec, layer.skew_x);
            let sk_y = eval_property(&layer.animations, AnimProperty::SkewY, time_sec, layer.skew_y);
            let xform = build_pivot_transform_with_skew(cx, cy, cw, ch, s, rot, sk_x, sk_y);

            let eff_bg = eval_color_property(
                &layer.animations,
                AnimProperty::Color,
                time_sec,
                background.as_deref(),
            );
            let eff_border = eval_color_property(
                &layer.animations,
                AnimProperty::BorderColor,
                time_sec,
                border_color.as_deref(),
            );

            let eff_radius = eval_property(&layer.animations, AnimProperty::Radius, time_sec, *radius).max(0.0);
            if eff_bg.is_some() || gradient.is_some() || eff_border.is_some() {
                if let Some(path) = build_rounded_rect(cx, cy, cw, ch, eff_radius) {
                    draw_styled_path(
                        pixmap,
                        &path,
                        cx,
                        cy,
                        cw,
                        ch,
                        eff_bg.as_deref(),
                        gradient.as_ref(),
                        eff_border.as_deref(),
                        *border_width,
                        None,
                        op,
                        xform,
                    )?;
                }
            }

            let container_end = effective_end_sec(layer, scene_duration);
            let child_timelines = resolve_child_timelines(
                *timeline,
                layer.start_sec,
                container_end,
                time_sec,
                scene_duration,
                *stagger_secs,
                children,
            );

            let needs_offscreen =
                *clip || rot != 0.0 || sk_x != 0.0 || sk_y != 0.0 || (s - 1.0).abs() > 1e-6;
            if needs_offscreen {
                let pw = (cw.ceil() as u32).max(1);
                let ph = (ch.ceil() as u32).max(1);
                let mut child_pixmap = Pixmap::new(pw, ph)
                    .ok_or_else(|| anyhow!("Failed to allocate container pixmap {}x{}", pw, ph))?;
                for ((child, cbox), c_time) in children
                    .iter()
                    .zip(child_boxes.into_iter())
                    .zip(child_timelines.into_iter())
                {
                    let Some((ct, cdur, skip_win)) = c_time else {
                        continue;
                    };
                    let local_box = LayoutBox {
                        x: cbox.x,
                        y: cbox.y,
                        w: cbox.w,
                        h: cbox.h,
                        parent_x: 0.0,
                        parent_y: 0.0,
                    };
                    render_layer_scoped(
                        child,
                        cdur,
                        ct,
                        assets,
                        font_system,
                        swash_cache,
                        &mut child_pixmap,
                        Some(local_box),
                        if *cascade_opacity { op } else { 1.0 },
                        skip_win,
                    )?;
                }
                if *clip {
                    if let Some(mask_path) = build_rounded_rect(0.0, 0.0, cw, ch, eff_radius) {
                        if let Some(mut mask) = Mask::new(pw, ph) {
                            mask.fill_path(&mask_path, FillRule::Winding, true, Transform::identity());
                            child_pixmap.apply_mask(&mask);
                        }
                    }
                }
                let paint = PixmapPaint {
                    opacity: 1.0,
                    quality: FilterQuality::Bilinear,
                    ..PixmapPaint::default()
                };
                let composite_xform = Transform::from_translate(cx, cy).post_concat(xform);
                pixmap.draw_pixmap(0, 0, child_pixmap.as_ref(), &paint, composite_xform, None);
            } else {
                for ((child, cbox), c_time) in children
                    .iter()
                    .zip(child_boxes.into_iter())
                    .zip(child_timelines.into_iter())
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
                    render_layer_scoped(
                        child,
                        cdur,
                        ct,
                        assets,
                        font_system,
                        swash_cache,
                        pixmap,
                        Some(abs_box),
                        if *cascade_opacity { op } else { 1.0 },
                        skip_win,
                    )?;
                }
            }
        }
    }

    Ok(())
}

fn map_align_items(spec: Option<AlignSpec>) -> Option<AlignItems> {
    spec.map(|s| match s {
        AlignSpec::Start | AlignSpec::SpaceBetween | AlignSpec::SpaceAround => AlignItems::Start,
        AlignSpec::Center => AlignItems::Center,
        AlignSpec::End => AlignItems::End,
        AlignSpec::Stretch => AlignItems::Stretch,
    })
}

fn map_justify_content(spec: Option<AlignSpec>) -> Option<JustifyContent> {
    spec.map(|s| match s {
        AlignSpec::Start | AlignSpec::Stretch => JustifyContent::Start,
        AlignSpec::Center => JustifyContent::Center,
        AlignSpec::End => JustifyContent::End,
        AlignSpec::SpaceBetween => JustifyContent::SpaceBetween,
        AlignSpec::SpaceAround => JustifyContent::SpaceAround,
    })
}

#[allow(clippy::too_many_arguments)]
fn is_emoji_char(c: char) -> bool {
    matches!(
        c as u32,
        0x2600..=0x27BF | 0x1F300..=0x1FAFF | 0xFE0F | 0x200D
    )
}

fn find_color_emoji_family(font_system: &FontSystem) -> Option<&'static str> {
    for candidate in ["Noto Color Emoji", "Apple Color Emoji", "Segoe UI Emoji"] {
        if font_system
            .db()
            .faces()
            .any(|f| f.families.iter().any(|(name, _)| name == candidate))
        {
            return Some(candidate);
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn build_text_buffer(
    font_system: &mut FontSystem,
    content: &str,
    spans: &[TextSpan],
    font_size: f32,
    line_height: Option<f32>,
    font_family: Option<&str>,
    font_weight: FontWeightSpec,
    text_align: TextAlignSpec,
    default_color: CosmicColor,
    opacity: f32,
    max_width: Option<f32>,
    max_height: Option<f32>,
    scale: f32,
) -> Result<Buffer> {
    let lh = line_height.unwrap_or(font_size * 1.25) * scale;
    let metrics = Metrics::new((font_size * scale).max(1.0), lh.max(1.0));
    let mut buffer = Buffer::new(font_system, metrics);
    buffer.set_size(font_system, max_width, max_height);
    let family = match font_family {
        Some(name) => Family::Name(name),
        None => Family::SansSerif,
    };
    let weight = match font_weight {
        FontWeightSpec::Normal => Weight::NORMAL,
        FontWeightSpec::Bold => Weight::BOLD,
    };
    let align = match text_align {
        TextAlignSpec::Left => Align::Left,
        TextAlignSpec::Center => Align::Center,
        TextAlignSpec::Right => Align::Right,
    };
    let default_attrs = Attrs::new().family(family).weight(weight).color(default_color);
    if spans.is_empty() {
        let emoji_fam = if font_family.is_none() && content.chars().any(is_emoji_char) {
            find_color_emoji_family(font_system)
        } else {
            None
        };
        if let Some(efam) = emoji_fam {
            let mut auto_spans: Vec<(&str, Attrs)> = Vec::new();
            let mut run_start = 0usize;
            let mut cur_is_emoji: Option<bool> = None;
            for (idx, ch) in content.char_indices() {
                let em = is_emoji_char(ch);
                match cur_is_emoji {
                    Some(prev) if prev != em => {
                        let a = if prev {
                            default_attrs.family(Family::Name(efam))
                        } else {
                            default_attrs
                        };
                        auto_spans.push((&content[run_start..idx], a));
                        run_start = idx;
                        cur_is_emoji = Some(em);
                    }
                    None => cur_is_emoji = Some(em),
                    _ => {}
                }
            }
            if run_start < content.len() {
                let a = if cur_is_emoji.unwrap_or(false) {
                    default_attrs.family(Family::Name(efam))
                } else {
                    default_attrs
                };
                auto_spans.push((&content[run_start..], a));
            }
            buffer.set_rich_text(font_system, auto_spans, default_attrs, Shaping::Advanced);
        } else {
            buffer.set_text(font_system, content, default_attrs, Shaping::Advanced);
        }
    } else {
        let mut rich_spans = Vec::with_capacity(spans.len());
        for sp in spans {
            let mut a = default_attrs;
            if let Some(fam) = sp.font_family.as_deref() {
                a = a.family(Family::Name(fam));
            }
            if let Some(w) = sp.font_weight {
                a = a.weight(match w {
                    FontWeightSpec::Normal => Weight::NORMAL,
                    FontWeightSpec::Bold => Weight::BOLD,
                });
            }
            if let Some(fs) = sp.font_size {
                let sfs = fs * scale;
                let slh = line_height.unwrap_or(fs * 1.25) * scale;
                a = a.metrics(Metrics::new(sfs, slh));
            }
            if let Some(col_hex) = sp.color.as_deref() {
                let c = parse_hex_color(col_hex, opacity)?;
                a = a.color(CosmicColor::rgba(
                    (c.red() * 255.0) as u8,
                    (c.green() * 255.0) as u8,
                    (c.blue() * 255.0) as u8,
                    (c.alpha() * 255.0) as u8,
                ));
            }
            rich_spans.push((sp.text.as_str(), a));
        }
        buffer.set_rich_text(font_system, rich_spans, default_attrs, Shaping::Advanced);
    }
    for line in buffer.lines.iter_mut() {
        line.set_align(Some(align));
    }
    buffer.shape_until_scroll(font_system, false);
    Ok(buffer)
}

#[allow(clippy::too_many_arguments)]
fn measure_text_size(
    font_system: &mut FontSystem,
    content: &str,
    spans: &[TextSpan],
    font_size: f32,
    line_height: Option<f32>,
    letter_spacing: f32,
    font_family: Option<&str>,
    font_weight: FontWeightSpec,
    max_width: Option<f32>,
    scale: f32,
) -> (f32, f32) {
    let lh = line_height.unwrap_or(font_size * 1.25) * scale;
    let Ok(buffer) = build_text_buffer(
        font_system,
        content,
        spans,
        font_size,
        line_height,
        font_family,
        font_weight,
        TextAlignSpec::Left,
        CosmicColor::rgba(255, 255, 255, 255),
        1.0,
        max_width,
        None,
        scale,
    ) else {
        return (0.0, lh.ceil());
    };

    let ls = letter_spacing * scale;
    let mut max_w: f32 = 0.0;
    let mut line_count: usize = 0;
    for run in buffer.layout_runs() {
        let extra = (run.glyphs.len().saturating_sub(1) as f32) * ls;
        max_w = max_w.max((run.line_w + extra).max(0.0));
        line_count += 1;
    }
    if line_count == 0 {
        line_count = 1;
    }
    (max_w.ceil(), (line_count as f32 * lh).ceil())
}

fn build_taffy_child_node(
    taffy: &mut TaffyTree<()>,
    child: &Layer,
    parent_width_hint: Option<f32>,
    font_system: &mut FontSystem,
    time_sec: f32,
) -> Result<taffy::NodeId> {
    let margin = child.margin.unwrap_or(0.0);
    let (pos, inset) = match child.position {
        PositionSpec::Relative => (Position::Relative, taffy::Rect::auto()),
        PositionSpec::Absolute => {
            let (ax, ay) = match &child.kind {
                LayerKind::Rect { x, y, .. }
                | LayerKind::Text { x, y, .. }
                | LayerKind::Image { x, y, .. }
                | LayerKind::Video { x, y, .. }
                | LayerKind::Svg { x, y, .. }
                | LayerKind::Container { x, y, .. } => (*x, *y),
                LayerKind::Circle { cx, cy, r, .. } => (*cx - *r, *cy - *r),
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
        let next_hint = width
            .map(|w| (w - layout.padding * 2.0).max(0.0))
            .or_else(|| parent_width_hint.map(|pw| (pw - layout.padding * 2.0).max(0.0)));
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
        let style = Style {
            display: Display::Flex,
            position: pos,
            inset,
            flex_direction: match layout.direction {
                FlexDirectionSpec::Row => FlexDirection::Row,
                FlexDirectionSpec::Column => FlexDirection::Column,
            },
            justify_content: map_justify_content(layout.justify_content),
            align_items: map_align_items(layout.align_items),
            gap: Size {
                width: LengthPercentage::Length(layout.gap),
                height: LengthPercentage::Length(layout.gap),
            },
            padding: taffy::Rect {
                left: LengthPercentage::Length(layout.padding),
                right: LengthPercentage::Length(layout.padding),
                top: LengthPercentage::Length(layout.padding),
                bottom: LengthPercentage::Length(layout.padding),
            },
            size: Size {
                width: cw,
                height: ch,
            },
            flex_grow: child.flex_grow.unwrap_or(0.0),
            margin: taffy::Rect {
                left: LengthPercentageAuto::Length(margin),
                right: LengthPercentageAuto::Length(margin),
                top: LengthPercentageAuto::Length(margin),
                bottom: LengthPercentageAuto::Length(margin),
            },
            ..Default::default()
        };
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
        LayerKind::Circle { r, .. } => (Dimension::Length(*r * 2.0), Dimension::Length(*r * 2.0)),
        LayerKind::Container { .. } => unreachable!(),
        LayerKind::Image { width, height, .. }
        | LayerKind::Video { width, height, .. }
        | LayerKind::Svg { width, height, .. } => (
            width.map(Dimension::Length).unwrap_or(Dimension::Auto),
            height.map(Dimension::Length).unwrap_or(Dimension::Auto),
        ),
        LayerKind::Text {
            content,
            spans,
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
            left: LengthPercentageAuto::Length(margin),
            right: LengthPercentageAuto::Length(margin),
            top: LengthPercentageAuto::Length(margin),
            bottom: LengthPercentageAuto::Length(margin),
        },
        ..Default::default()
    };
    Ok(taffy.new_leaf(style)?)
}

fn compute_flex_layout(
    width: Option<f32>,
    height: Option<f32>,
    layout: &LayoutSpec,
    children: &[Layer],
    font_system: &mut FontSystem,
    time_sec: f32,
) -> Result<(f32, f32, Vec<LayoutBox>)> {
    let mut taffy: TaffyTree<()> = TaffyTree::new();
    let mut child_nodes = Vec::with_capacity(children.len());
    let inner_width_hint = width.map(|w| (w - layout.padding * 2.0).max(0.0));

    for child in children {
        child_nodes.push(build_taffy_child_node(
            &mut taffy,
            child,
            inner_width_hint,
            font_system,
            time_sec,
        )?);
    }

    let root_style = Style {
        display: Display::Flex,
        flex_direction: match layout.direction {
            FlexDirectionSpec::Row => FlexDirection::Row,
            FlexDirectionSpec::Column => FlexDirection::Column,
        },
        justify_content: map_justify_content(layout.justify_content),
        align_items: map_align_items(layout.align_items),
        gap: Size {
            width: LengthPercentage::Length(layout.gap),
            height: LengthPercentage::Length(layout.gap),
        },
        padding: taffy::Rect {
            left: LengthPercentage::Length(layout.padding),
            right: LengthPercentage::Length(layout.padding),
            top: LengthPercentage::Length(layout.padding),
            bottom: LengthPercentage::Length(layout.padding),
        },
        size: Size {
            width: width.map(Dimension::Length).unwrap_or(Dimension::Auto),
            height: height.map(Dimension::Length).unwrap_or(Dimension::Auto),
        },
        ..Default::default()
    };

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

#[allow(clippy::too_many_arguments)]
fn build_progress_stroke_dash(
    total_len: f32,
    progress: f32,
    dash_offset: Option<f32>,
    custom_dash: Option<&[f32]>,
) -> Option<StrokeDash> {
    if let Some(arr) = custom_dash {
        if !arr.is_empty() {
            return StrokeDash::new(arr.to_vec(), dash_offset.unwrap_or(0.0));
        }
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

fn compute_path_length(path: &tiny_skia::Path) -> f32 {
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

fn render_usvg_group_animated(
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
                render_usvg_group_animated(sub, pixmap, root_xform, progress, dash_offset, custom_dash);
            }
            resvg::usvg::Node::Path(path) => {
                if !path.is_visible() {
                    continue;
                }
                let xform = root_xform.pre_concat(path.abs_transform());
                if let Some(fill) = path.fill() {
                    if let Some(p) = usvg_paint_to_tiny_paint(fill.paint(), fill.opacity().get()) {
                        let rule = match fill.rule() {
                            resvg::usvg::FillRule::NonZero => FillRule::Winding,
                            resvg::usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                        };
                        pixmap.fill_path(path.data(), &p, rule, xform, None);
                    }
                }
                if let Some(stroke) = path.stroke() {
                    if let Some(p) = usvg_paint_to_tiny_paint(stroke.paint(), stroke.opacity().get()) {
                        let path_len = compute_path_length(path.data());
                        let dash = build_progress_stroke_dash(path_len, progress, dash_offset, custom_dash);
                        let skia_stroke = Stroke {
                            width: stroke.width().get(),
                            dash,
                            ..Stroke::default()
                        };
                        pixmap.stroke_path(path.data(), &p, &skia_stroke, xform, None);
                    }
                }
            }
            _ => {}
        }
    }
}

fn build_gradient_shader(
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
        GradientTypeSpec::Radial => RadialGradient::new(
            Point::from_xy(cx, cy),
            Point::from_xy(cx, cy),
            w.max(h) * 0.5,
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        ),
    };
    Ok(shader)
}

#[allow(clippy::too_many_arguments)]
fn draw_styled_path(
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

    if let Some(b_hex) = border_color {
        if border_width > 0.0 {
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
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TextMeasureInfo {
    pub width: f32,
    pub height: f32,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_measured: Option<TextMeasureInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<LayerLayoutInfo>,
}

/// Evaluate computed bounding boxes and measured text dimensions at `time_sec`.
pub fn inspect_layout_at_time(
    scene: &Scene,
    assets: &AssetStore,
    time_sec: f32,
) -> Result<Vec<LayerLayoutInfo>> {
    let font_count = assets.fonts.len();
    THREAD_FONT_SYS.with(|cell| -> Result<Vec<LayerLayoutInfo>> {
        let mut borrow = cell.borrow_mut();
        let needs_init = match &*borrow {
            Some((count, _, _)) => *count != font_count,
            None => true,
        };
        if needs_init {
            let mut fs = FontSystem::new();
            for font_bytes in &assets.fonts {
                fs.db_mut().load_font_data(font_bytes.clone());
            }
            *borrow = Some((font_count, fs, SwashCache::new()));
        }
        let (_, font_system, _) = borrow.as_mut().unwrap();

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
            let (mw, mh) = measure_text_size(
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
            let bw = effective_max_w.unwrap_or(mw);
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
                text_measured: Some(TextMeasureInfo { width: mw, height: mh }),
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
                .zip(child_boxes.into_iter())
                .zip(child_timelines.into_iter())
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
                text_measured: None,
                children: child_infos,
            }
        }
    };
    Ok(Some(info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_parse_hex_color() {
        let c = parse_hex_color("#FF0000", 0.5).unwrap();
        assert!((c.red() - 1.0).abs() < 1e-5);
        assert!((c.alpha() - 0.5).abs() < 1e-5);
        assert!(parse_hex_color("#123", 1.0).is_err());
    }

    #[test]
    fn test_render_frame_with_container_and_absolute() {
        let json = r##"{
            "canvas": { "width": 320, "height": 240, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
            "layers": [
                {
                    "type": "container",
                    "x": 10, "y": 10, "width": 300, "height": 220,
                    "gradient": { "from": "#111111", "to": "#222222", "angle": 45 },
                    "border_color": "#da5c2c", "border_width": 2,
                    "children": [
                        { "type": "rect", "width": 50, "height": 50, "color": "#38BDF8" },
                        { "type": "rect", "position": "absolute", "x": 200, "y": 20, "width": 40, "height": 40, "color": "#22C55E" }
                    ]
                }
            ]
        }"##;
        let scene: Scene = serde_json::from_str(json).unwrap();
        let assets = AssetStore {
            fonts: vec![],
            images: HashMap::new(),
            videos: HashMap::new(),
            svgs: HashMap::new(),
            audio_paths: vec![],
        };
        let frame = render_frame(&scene, &assets, 0).unwrap();
        assert_eq!(frame.len(), 320 * 240 * 4);
    }
}
