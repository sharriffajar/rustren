use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;
use tiny_skia::{
    BlendMode, Color, ColorU8, FillRule, FilterQuality, GradientStop, LineCap, LineJoin,
    LinearGradient, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Point, PremultipliedColorU8,
    RadialGradient, Rect, Shader, SpreadMode, Stroke, StrokeDash, Transform,
};

thread_local! {
    /// Per-thread reusable zeroed Pixmap pool to avoid repeated 5+ MB heap alloc/zero/free per layer.
    static LOTTIE_SCRATCH_POOL: RefCell<Vec<Pixmap>> = const { RefCell::new(Vec::new()) };
    /// Scoped per-render color overrides (`sid` / `nm` / lowercase `#rrggbb` -> RGBA `[0..1]`).
    static LOTTIE_COLOR_OVERRIDES: RefCell<Option<HashMap<String, [f32; 4]>>> = const { RefCell::new(None) };
    /// Per-thread pre-parsed typed vector keyframe cache keyed by `prop as *const Value as usize`.
    static COMPILED_VEC_CACHE: RefCell<HashMap<usize, CompiledVecProp>> = RefCell::new(HashMap::new());
    /// Per-thread pre-parsed typed bezier shape keyframe cache keyed by `prop as *const Value as usize`.
    static COMPILED_SHAPE_CACHE: RefCell<HashMap<usize, CompiledShapeProp>> = RefCell::new(HashMap::new());
}

fn clear_compiled_prop_caches() {
    COMPILED_VEC_CACHE.with(|c| c.borrow_mut().clear());
    COMPILED_SHAPE_CACHE.with(|c| c.borrow_mut().clear());
}

#[derive(Clone)]
struct TypedVecKf {
    t: f32,
    hold: bool,
    s: Vec<f32>,
    e: Option<Vec<f32>>,
    ox: f32,
    oy: f32,
    ix: f32,
    iy: f32,
}

#[derive(Clone)]
enum CompiledVecProp {
    Static(Vec<f32>),
    Animated(Vec<TypedVecKf>),
}

type ShapePts = (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 2]>, bool);

#[derive(Clone)]
struct TypedShapeKf {
    t: f32,
    hold: bool,
    s: Option<ShapePts>,
    e: Option<ShapePts>,
    ox: f32,
    oy: f32,
    ix: f32,
    iy: f32,
}

#[derive(Clone)]
enum CompiledShapeProp {
    Static(Option<ShapePts>),
    Animated(Vec<TypedShapeKf>),
}

fn eval_bezier_params(ox: f32, oy: f32, ix: f32, iy: f32, t: f32) -> f32 {
    if (ox - oy).abs() < 1e-4 && (ix - iy).abs() < 1e-4 {
        return t;
    }
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    let mut u = t;
    for _ in 0..8 {
        let inv = 1.0 - u;
        let x = 3.0 * inv * inv * u * ox + 3.0 * inv * u * u * ix + u * u * u;
        if (x - t).abs() < 1e-4 {
            break;
        }
        if x < t {
            lo = u;
        } else {
            hi = u;
        }
        u = 0.5 * (lo + hi);
    }
    let inv = 1.0 - u;
    3.0 * inv * inv * u * oy + 3.0 * inv * u * u * iy + u * u * u
}

fn parse_override_hex(hex: &str) -> Option<[f32; 4]> {
    let s = hex.trim().trim_start_matches('#');
    match s.len() {
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&s[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&s[2..3], 16).ok()? * 17;
            Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
        }
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
        }
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            let a = u8::from_str_radix(&s[6..8], 16).ok()?;
            Some([
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
                a as f32 / 255.0,
            ])
        }
        _ => None,
    }
}

fn resolve_color_override(item: &Value, base: [f32; 4]) -> [f32; 4] {
    LOTTIE_COLOR_OVERRIDES.with(|cell| {
        let borrow = cell.borrow();
        let Some(map) = borrow.as_ref() else {
            return base;
        };
        if let Some(sid) = item
            .get("c")
            .and_then(|c| c.get("sid"))
            .and_then(|v| v.as_str())
            .or_else(|| item.get("sid").and_then(|v| v.as_str()))
            && let Some(ov) = map.get(sid)
        {
            return [ov[0], ov[1], ov[2], ov[3] * base[3]];
        }
        if let Some(nm) = item.get("nm").and_then(|v| v.as_str())
            && let Some(ov) = map.get(nm)
        {
            return [ov[0], ov[1], ov[2], ov[3] * base[3]];
        }
        let hex_key = format!(
            "#{:02x}{:02x}{:02x}",
            (base[0].clamp(0.0, 1.0) * 255.0).round() as u8,
            (base[1].clamp(0.0, 1.0) * 255.0).round() as u8,
            (base[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        );
        if let Some(ov) = map.get(&hex_key) {
            return [ov[0], ov[1], ov[2], ov[3] * base[3]];
        }
        base
    })
}

fn acquire_zeroed_pixmap(w: u32, h: u32) -> Option<Pixmap> {
    let reused = LOTTIE_SCRATCH_POOL.with(|pool| {
        let mut p = pool.borrow_mut();
        if let Some(pos) = p.iter().position(|pm| pm.width() == w && pm.height() == h) {
            Some(p.swap_remove(pos))
        } else {
            None
        }
    });
    if let Some(pm) = reused {
        return Some(pm);
    }
    Pixmap::new(w, h)
}

fn clear_pixmap_rect(pm: &mut Pixmap, bounds: (u32, u32, u32, u32)) {
    let w = pm.width() as usize;
    let (x0, y0, x1, y1) = (
        bounds.0 as usize,
        bounds.1 as usize,
        bounds.2 as usize,
        bounds.3 as usize,
    );
    let row_bytes = (x1 - x0 + 1) * 4;
    let data = pm.data_mut();
    for y in y0..=y1 {
        let start = (y * w + x0) * 4;
        data[start..start + row_bytes].fill(0);
    }
}

fn release_zeroed_pixmap(mut pm: Pixmap, dirty: Option<(u32, u32, u32, u32)>) {
    if let Some(b) = dirty {
        clear_pixmap_rect(&mut pm, b);
    }
    LOTTIE_SCRATCH_POOL.with(|pool| {
        let mut p = pool.borrow_mut();
        if p.len() < 16 {
            p.push(pm);
        }
    });
}

/// Fast tight bounding box of all non-transparent pixels in a Pixmap: `(min_x, min_y, max_x, max_y)` inclusive.
fn find_alpha_bounds(pm: &Pixmap) -> Option<(u32, u32, u32, u32)> {
    let w = pm.width() as usize;
    let h = pm.height() as usize;
    let data = pm.data();
    let (_, pixels, _) = unsafe { data.align_to::<u32>() };
    if pixels.len() != w * h {
        return Some((0, 0, pm.width().saturating_sub(1), pm.height().saturating_sub(1)));
    }

    let mut min_y = h;
    let mut max_y = 0usize;
    let mut min_x = w;
    let mut max_x = 0usize;

    for y in 0..h {
        let row = &pixels[y * w..(y + 1) * w];
        // Fast scan for first non-zero pixel in row
        let mut first = None;
        for (x, &px) in row.iter().enumerate() {
            if px != 0 {
                first = Some(x);
                break;
            }
        }
        let Some(fx) = first else {
            continue;
        };
        if y < min_y {
            min_y = y;
        }
        max_y = y;
        if fx < min_x {
            min_x = fx;
        }
        // Scan backwards from right edge for last non-zero pixel
        let mut lx = fx;
        for x in (fx.max(max_x)..w).rev() {
            if row[x] != 0 {
                lx = x;
                break;
            }
        }
        if lx > max_x {
            max_x = lx;
        }
    }

    if min_y <= max_y && min_x <= max_x {
        Some((min_x as u32, min_y as u32, max_x as u32, max_y as u32))
    } else {
        None
    }
}

fn expand_bounds(
    bounds: (u32, u32, u32, u32),
    pad: u32,
    w: u32,
    h: u32,
) -> (u32, u32, u32, u32) {
    (
        bounds.0.saturating_sub(pad),
        bounds.1.saturating_sub(pad),
        (bounds.2 + pad).min(w.saturating_sub(1)),
        (bounds.3 + pad).min(h.saturating_sub(1)),
    )
}

fn blur_pixmap_subrect(pm: &mut Pixmap, bounds: (u32, u32, u32, u32), radius: f32) -> (u32, u32, u32, u32) {
    let pw = pm.width();
    let ph = pm.height();
    let pad = (radius * 2.0).ceil() as u32 + 2;
    let expanded = expand_bounds(bounds, pad, pw, ph);
    let (x0, y0, x1, y1) = expanded;
    let cw = (x1 - x0 + 1) as usize;
    let ch = (y1 - y0 + 1) as usize;
    let full_w = pw as usize;

    // If cropped rect covers nearly the whole canvas, blur in-place
    if cw * ch * 4 >= (full_w * ph as usize) * 3 {
        super::fx::fast_blur_rgba(pm.data_mut(), pw, ph, radius);
        return (0, 0, pw.saturating_sub(1), ph.saturating_sub(1));
    }

    let mut sub = vec![0u8; cw * ch * 4];
    let src = pm.data();
    let row_bytes = cw * 4;
    for row in 0..ch {
        let sy = y0 as usize + row;
        let s_off = (sy * full_w + x0 as usize) * 4;
        let d_off = row * row_bytes;
        sub[d_off..d_off + row_bytes].copy_from_slice(&src[s_off..s_off + row_bytes]);
    }

    super::fx::fast_blur_rgba(&mut sub, cw as u32, ch as u32, radius);

    let dst = pm.data_mut();
    for row in 0..ch {
        let sy = y0 as usize + row;
        let d_off = (sy * full_w + x0 as usize) * 4;
        let s_off = row * row_bytes;
        dst[d_off..d_off + row_bytes].copy_from_slice(&sub[s_off..s_off + row_bytes]);
    }
    expanded
}

fn blit_subrect(
    dst: &mut Pixmap,
    src: &Pixmap,
    bounds: (u32, u32, u32, u32),
    blend_mode: BlendMode,
) {
    let (x0, y0, x1, y1) = bounds;
    let cw = x1 - x0 + 1;
    let ch = y1 - y0 + 1;

    if blend_mode == BlendMode::SourceOver {
        let w = dst.width() as usize;
        let dst_px = dst.pixels_mut();
        let src_px = src.pixels();
        for y in y0 as usize..=y1 as usize {
            let row_start = y * w;
            for x in x0 as usize..=x1 as usize {
                let idx = row_start + x;
                let s = src_px[idx];
                let sa = s.alpha();
                if sa == 0 {
                    continue;
                }
                if sa == 255 {
                    dst_px[idx] = s;
                    continue;
                }
                let d = dst_px[idx];
                let inv_a = 255 - sa as u16;
                let out_r = (s.red() as u16 + ((d.red() as u16 * inv_a + 127) / 255)).min(255) as u8;
                let out_g = (s.green() as u16 + ((d.green() as u16 * inv_a + 127) / 255)).min(255) as u8;
                let out_b = (s.blue() as u16 + ((d.blue() as u16 * inv_a + 127) / 255)).min(255) as u8;
                let out_a = (sa as u16 + ((d.alpha() as u16 * inv_a + 127) / 255)).min(255) as u8;
                dst_px[idx] = PremultipliedColorU8::from_rgba(
                    out_r.min(out_a),
                    out_g.min(out_a),
                    out_b.min(out_a),
                    out_a,
                )
                .unwrap_or(s);
            }
        }
        return;
    }

    // Non-SourceOver blend mode: copy tight subrect to small Pixmap and use tiny_skia blend
    let Some(mut sub) = Pixmap::new(cw, ch) else {
        return;
    };
    let full_w = src.width() as usize;
    let row_bytes = cw as usize * 4;
    let sdata = src.data();
    let ddata = sub.data_mut();
    for row in 0..ch as usize {
        let sy = y0 as usize + row;
        let s_off = (sy * full_w + x0 as usize) * 4;
        let d_off = row * row_bytes;
        ddata[d_off..d_off + row_bytes].copy_from_slice(&sdata[s_off..s_off + row_bytes]);
    }
    let paint = PixmapPaint {
        opacity: 1.0,
        blend_mode,
        quality: FilterQuality::Nearest,
    };
    dst.draw_pixmap(x0 as i32, y0 as i32, sub.as_ref(), &paint, Transform::identity(), None);
}

/// Pure-Rust CPU Lottie (Bodymovin JSON) vector renderer on `tiny_skia`.
/// Supports:
/// - Composition `w`, `h`, `fr`, `ip`, `op`
/// - Shape layers (`ty: 4`), Image layers (`ty: 2`, embedded base64 or local path), Solid layers (`ty: 1`), Null layers (`ty: 3`), Precomp layers (`ty: 0`)
/// - Layer parenting (`ind` / `parent`), `ip`/`op` visibility, `ks` transform (`a`, `p`, `s`, `r`, `o`)
/// - Track mattes (`tt: 1..4` Alpha / AlphaInv / Luma / LumaInv, explicit `tp` or legacy previous-layer matte, `td` matte source suppression)
/// - Layer blend modes (`bm`) and Gaussian Blur effects (`ef` `ty: 29`)
/// - Static & keyframed properties with cubic-bezier easing, hold frames, and smooth bezier shape morphing
/// - Shapes: `gr` (group + `tr`), `rc` (rect), `el` (ellipse), `sr` (star/polygon), `sh` (bezier path + morph), `fl` (solid fill), `st` (solid stroke), `gf` (linear/radial gradient fill + alpha stops), `gs` (gradient stroke), `tm` (trim paths)
#[allow(dead_code)]
pub fn render_lottie_json(
    pixmap: &mut Pixmap,
    json_bytes: &[u8],
    time_sec: f32,
    loop_animation: bool,
    speed: f32,
    rx: f32,
    ry: f32,
    w: f32,
    h: f32,
    rot_deg: f32,
    opacity: f32,
) -> bool {
    let Ok(doc) = serde_json::from_slice::<Value>(json_bytes) else {
        return false;
    };
    let ok = render_lottie_doc(
        pixmap,
        &doc,
        time_sec,
        loop_animation,
        speed,
        rx,
        ry,
        w,
        h,
        rot_deg,
        opacity,
    );
    clear_compiled_prop_caches();
    ok
}

pub fn render_lottie_doc(
    pixmap: &mut Pixmap,
    doc: &Value,
    time_sec: f32,
    loop_animation: bool,
    speed: f32,
    rx: f32,
    ry: f32,
    w: f32,
    h: f32,
    rot_deg: f32,
    opacity: f32,
) -> bool {
    let dst_tf = Transform::from_translate(rx, ry)
        .post_concat(Transform::from_rotate_at(rot_deg, rx + w * 0.5, ry + h * 0.5));
    render_lottie_doc_ex(
        pixmap,
        doc,
        time_sec,
        loop_animation,
        speed,
        None,
        None,
        w,
        h,
        dst_tf,
        opacity,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn render_lottie_doc_ex(
    pixmap: &mut Pixmap,
    doc: &Value,
    time_sec: f32,
    loop_animation: bool,
    speed: f32,
    progress_override: Option<f32>,
    color_overrides: Option<&HashMap<String, String>>,
    w: f32,
    h: f32,
    dst_tf: Transform,
    opacity: f32,
) -> bool {
    let comp_w = doc.get("w").and_then(|v| v.as_f64()).unwrap_or(512.0) as f32;
    let comp_h = doc.get("h").and_then(|v| v.as_f64()).unwrap_or(512.0) as f32;
    let fr = doc.get("fr").and_then(|v| v.as_f64()).unwrap_or(30.0) as f32;
    let ip = doc.get("ip").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let op = doc.get("op").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32;

    if comp_w <= 0.0 || comp_h <= 0.0 || op <= ip {
        return false;
    }

    let dur_frames = (op - ip).max(1.0);
    let frame = if let Some(p) = progress_override {
        ip + p.clamp(0.0, 1.0) * (dur_frames - 0.001).max(0.0)
    } else {
        let raw_frame = time_sec.max(0.0) * speed.max(0.0) * fr;
        if loop_animation {
            ip + (raw_frame % dur_frames)
        } else {
            (ip + raw_frame).min(op - 0.001)
        }
    };

    let off_w = (w.ceil() as u32).max(1);
    let off_h = (h.ceil() as u32).max(1);
    let Some(mut off) = acquire_zeroed_pixmap(off_w, off_h) else {
        return false;
    };
    let sx = w / comp_w;
    let sy = h / comp_h;
    let local_tf = Transform::from_scale(sx, sy);

    let parsed_overrides = color_overrides.and_then(|m| {
        if m.is_empty() {
            return None;
        }
        let mut out = HashMap::with_capacity(m.len());
        for (k, v) in m {
            if let Some(rgba) = parse_override_hex(v) {
                let trimmed = k.trim();
                let norm_k = if let Some(hex_body) = trimmed.strip_prefix('#') {
                    let lower = hex_body.to_ascii_lowercase();
                    if lower.len() == 3 && lower.chars().all(|c| c.is_ascii_hexdigit()) {
                        let b = lower.as_bytes();
                        format!(
                            "#{}{}{}{}{}{}",
                            b[0] as char,
                            b[0] as char,
                            b[1] as char,
                            b[1] as char,
                            b[2] as char,
                            b[2] as char
                        )
                    } else {
                        format!("#{lower}")
                    }
                } else {
                    k.clone()
                };
                out.insert(norm_k, rgba);
            }
        }
        if out.is_empty() { None } else { Some(out) }
    });
    let prev_overrides =
        LOTTIE_COLOR_OVERRIDES.with(|cell| std::mem::replace(&mut *cell.borrow_mut(), parsed_overrides));

    let assets = doc.get("assets").and_then(|v| v.as_array());
    let result = if let Some(layers) = doc.get("layers").and_then(|v| v.as_array()) {
        render_layer_list(&mut off, layers, assets, frame, local_tf, 1.0);
        let dirty = find_alpha_bounds(&off);
        if dirty.is_some() {
            let paint = PixmapPaint {
                opacity: opacity.clamp(0.0, 1.0),
                quality: FilterQuality::Bilinear,
                ..PixmapPaint::default()
            };
            pixmap.draw_pixmap(0, 0, off.as_ref(), &paint, dst_tf, None);
        }
        release_zeroed_pixmap(off, dirty);
        true
    } else {
        release_zeroed_pixmap(off, None);
        false
    };

    LOTTIE_COLOR_OVERRIDES.with(|cell| *cell.borrow_mut() = prev_overrides);
    result
}

fn map_lottie_blend_mode(bm: i64) -> BlendMode {
    match bm {
        1 => BlendMode::Multiply,
        2 => BlendMode::Screen,
        3 => BlendMode::Overlay,
        4 => BlendMode::Darken,
        5 => BlendMode::Lighten,
        6 => BlendMode::ColorDodge,
        7 => BlendMode::ColorBurn,
        8 => BlendMode::HardLight,
        9 => BlendMode::SoftLight,
        10 => BlendMode::Difference,
        11 => BlendMode::Exclusion,
        12 => BlendMode::Hue,
        13 => BlendMode::Saturation,
        14 => BlendMode::Color,
        15 => BlendMode::Luminosity,
        _ => BlendMode::SourceOver,
    }
}

fn extract_gaussian_blur_radius(layer: &Value, frame: f32) -> f32 {
    let Some(effects) = layer.get("ef").and_then(|v| v.as_array()) else {
        return 0.0;
    };
    let mut total = 0.0f32;
    for ef in effects {
        let ty = ef.get("ty").and_then(|v| v.as_i64()).unwrap_or(-1);
        let en = ef.get("en").and_then(|v| v.as_i64()).unwrap_or(1);
        if ty == 29 && en != 0 {
            if let Some(params) = ef.get("ef").and_then(|v| v.as_array()) {
                if let Some(blur_val) = params.first().and_then(|p| p.get("v")) {
                    let b = eval_scalar(blur_val, frame, 0.0);
                    if b > 0.0 {
                        total += b * 0.35;
                    }
                }
            }
        }
    }
    total
}

fn render_layer_list(
    pixmap: &mut Pixmap,
    layers: &[Value],
    assets: Option<&Vec<Value>>,
    frame: f32,
    parent_tf: Transform,
    parent_opacity: f32,
) {
    // Cache matte source pixmaps + their dirty bounds per layer index in this composition
    let mut matte_cache: HashMap<usize, (Pixmap, Option<(u32, u32, u32, u32)>)> = HashMap::new();

    // Lottie layers are ordered top-to-bottom, so render in reverse (bottom-to-top).
    for idx in (0..layers.len()).rev() {
        let layer = &layers[idx];
        // Skip matte source layers in normal pass; they are rendered on demand as mattes.
        if layer.get("td").and_then(|v| v.as_i64()).unwrap_or(0) != 0 {
            continue;
        }
        if layer.get("hd").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let ip = layer.get("ip").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let op = layer.get("op").and_then(|v| v.as_f64()).unwrap_or(1e9) as f32;
        if frame < ip || frame >= op {
            continue;
        }

        let tt = layer.get("tt").and_then(|v| v.as_i64()).unwrap_or(0);
        let bm_raw = layer.get("bm").and_then(|v| v.as_i64()).unwrap_or(0);
        let blend_mode = map_lottie_blend_mode(bm_raw);
        let blur_radius = extract_gaussian_blur_radius(layer, frame);
        let needs_offscreen = (1..=4).contains(&tt) || bm_raw != 0 || blur_radius > 0.1;

        if !needs_offscreen {
            render_single_layer(pixmap, layer, layers, assets, frame, parent_tf, parent_opacity);
            continue;
        }

        let pw = pixmap.width();
        let ph = pixmap.height();
        let Some(mut layer_pm) = acquire_zeroed_pixmap(pw, ph) else {
            continue;
        };
        render_single_layer(&mut layer_pm, layer, layers, assets, frame, parent_tf, parent_opacity);

        let Some(mut dirty) = find_alpha_bounds(&layer_pm) else {
            // Layer produced zero visible pixels — return immediately without blur/matte/blit
            release_zeroed_pixmap(layer_pm, None);
            continue;
        };

        if blur_radius > 0.1 {
            let scale_est = ((parent_tf.sx * parent_tf.sx + parent_tf.ky * parent_tf.ky).sqrt()
                + (parent_tf.kx * parent_tf.kx + parent_tf.sy * parent_tf.sy).sqrt())
                * 0.5;
            let eff_radius = (blur_radius * scale_est.max(0.1)).min(64.0);
            if eff_radius > 0.2 {
                dirty = blur_pixmap_subrect(&mut layer_pm, dirty, eff_radius);
            }
        }

        let mut skip_blit = false;
        if (1..=4).contains(&tt) {
            let matte_idx = if let Some(tp) = layer.get("tp").and_then(|v| v.as_i64()) {
                layers
                    .iter()
                    .position(|l| l.get("ind").and_then(|v| v.as_i64()) == Some(tp))
            } else if idx > 0 {
                Some(idx - 1)
            } else {
                None
            };

            if let Some(m_idx) = matte_idx {
                let (matte_pm, matte_bounds) = matte_cache.entry(m_idx).or_insert_with(|| {
                    let mut m_pm = acquire_zeroed_pixmap(pw, ph)
                        .unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
                    let m_layer = &layers[m_idx];
                    let m_ip = m_layer.get("ip").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    let m_op = m_layer.get("op").and_then(|v| v.as_f64()).unwrap_or(1e9) as f32;
                    if frame >= m_ip && frame < m_op {
                        render_single_layer(&mut m_pm, m_layer, layers, assets, frame, parent_tf, 1.0);
                    }
                    let mb = find_alpha_bounds(&m_pm);
                    (m_pm, mb)
                });

                if matte_pm.width() == pw && matte_pm.height() == ph {
                    if matches!(tt, 1 | 3) {
                        // Positive alpha/luma matte: if matte is empty or disjoint, layer is completely clipped out
                        match *matte_bounds {
                            None => skip_blit = true,
                            Some(mb) => {
                                let ix0 = dirty.0.max(mb.0);
                                let iy0 = dirty.1.max(mb.1);
                                let ix1 = dirty.2.min(mb.2);
                                let iy1 = dirty.3.min(mb.3);
                                if ix0 > ix1 || iy0 > iy1 {
                                    skip_blit = true;
                                } else {
                                    apply_track_matte_rect(&mut layer_pm, matte_pm, tt, (ix0, iy0, ix1, iy1));
                                    // Blit only the intersected region (outside intersection is clipped)
                                    blit_subrect(pixmap, &layer_pm, (ix0, iy0, ix1, iy1), blend_mode);
                                    release_zeroed_pixmap(layer_pm, Some(dirty));
                                    continue;
                                }
                            }
                        }
                    } else {
                        // Inverted matte (tt == 2 | 4): only need to mask overlapping region
                        if let Some(mb) = *matte_bounds {
                            let ix0 = dirty.0.max(mb.0);
                            let iy0 = dirty.1.max(mb.1);
                            let ix1 = dirty.2.min(mb.2);
                            let iy1 = dirty.3.min(mb.3);
                            if ix0 <= ix1 && iy0 <= iy1 {
                                apply_track_matte_rect(&mut layer_pm, matte_pm, tt, (ix0, iy0, ix1, iy1));
                            }
                        }
                    }
                }
            }
        }

        if !skip_blit {
            blit_subrect(pixmap, &layer_pm, dirty, blend_mode);
        }
        release_zeroed_pixmap(layer_pm, Some(dirty));
    }

    // Return cached matte pixmaps to thread-local scratch pool
    for (_, (m_pm, m_dirty)) in matte_cache {
        release_zeroed_pixmap(m_pm, m_dirty);
    }
}

fn apply_track_matte_rect(
    target: &mut Pixmap,
    matte: &Pixmap,
    tt: i64,
    rect: (u32, u32, u32, u32),
) {
    let w = target.width() as usize;
    let dst = target.pixels_mut();
    let msrc = matte.pixels();
    let (x0, y0, x1, y1) = (
        rect.0 as usize,
        rect.1 as usize,
        rect.2 as usize,
        rect.3 as usize,
    );

    for y in y0..=y1 {
        let row = y * w;
        for x in x0..=x1 {
            let idx = row + x;
            let d = dst[idx];
            if d.alpha() == 0 {
                continue;
            }
            let m = msrc[idx];
            let factor_u16: u16 = match tt {
                1 => m.alpha() as u16,
                2 => 255 - m.alpha() as u16,
                3 => ((54u32 * m.red() as u32 + 183u32 * m.green() as u32 + 18u32 * m.blue() as u32) >> 8) as u16,
                4 => 255 - (((54u32 * m.red() as u32 + 183u32 * m.green() as u32 + 18u32 * m.blue() as u32) >> 8) as u16),
                _ => 255,
            };
            if factor_u16 == 0 {
                dst[idx] = PremultipliedColorU8::TRANSPARENT;
            } else if factor_u16 < 255 {
                let r = ((d.red() as u16 * factor_u16 + 127) / 255) as u8;
                let g = ((d.green() as u16 * factor_u16 + 127) / 255) as u8;
                let b = ((d.blue() as u16 * factor_u16 + 127) / 255) as u8;
                let a = ((d.alpha() as u16 * factor_u16 + 127) / 255) as u8;
                dst[idx] = PremultipliedColorU8::from_rgba(r.min(a), g.min(a), b.min(a), a)
                    .unwrap_or(PremultipliedColorU8::TRANSPARENT);
            }
        }
    }
}

fn render_single_layer(
    pixmap: &mut Pixmap,
    layer: &Value,
    all_layers: &[Value],
    assets: Option<&Vec<Value>>,
    frame: f32,
    parent_tf: Transform,
    parent_opacity: f32,
) {
    let (layer_tf, layer_op) = resolve_layer_transform(layer, all_layers, frame, parent_tf);
    let final_op = (parent_opacity * layer_op).clamp(0.0, 1.0);
    if final_op <= 0.001 {
        return;
    }

    let ty = layer.get("ty").and_then(|v| v.as_i64()).unwrap_or(-1);
    match ty {
        1 => {
            // Solid layer
            let sw = layer.get("sw").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let sh = layer.get("sh").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            if sw > 0.0 && sh > 0.0 {
                let hex = layer.get("sc").and_then(|v| v.as_str()).unwrap_or("#ffffff");
                if let Some((r, g, b)) = parse_hex_rgb(hex) {
                    if let Some(rect) = Rect::from_xywh(0.0, 0.0, sw, sh) {
                        let mut pb = PathBuilder::new();
                        pb.push_rect(rect);
                        if let Some(path) = pb.finish() {
                            let mut paint = Paint::default();
                            paint.set_color(
                                Color::from_rgba(r, g, b, final_op).unwrap_or(Color::WHITE),
                            );
                            paint.anti_alias = true;
                            pixmap.fill_path(&path, &paint, FillRule::Winding, layer_tf, None);
                        }
                    }
                }
            }
        }
        4 => {
            // Shape layer
            if let Some(shapes) = layer.get("shapes").and_then(|v| v.as_array()) {
                render_shape_group(pixmap, shapes, frame, layer_tf, final_op, None);
            }
        }
        2 => {
            // Image layer (embedded data:image/...;base64,... or local file path)
            if let (Some(ref_id), Some(asset_list)) = (
                layer.get("refId").and_then(|v| v.as_str()),
                assets,
            ) {
                if let Some(asset) = asset_list
                    .iter()
                    .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(ref_id))
                {
                    if let Some(img_pm) = decode_lottie_image_asset(asset) {
                        let target_w = asset
                            .get("w")
                            .and_then(|v| v.as_f64())
                            .map(|v| v as f32)
                            .unwrap_or(img_pm.width() as f32);
                        let target_h = asset
                            .get("h")
                            .and_then(|v| v.as_f64())
                            .map(|v| v as f32)
                            .unwrap_or(img_pm.height() as f32);
                        let sx = if img_pm.width() > 0 {
                            target_w / img_pm.width() as f32
                        } else {
                            1.0
                        };
                        let sy = if img_pm.height() > 0 {
                            target_h / img_pm.height() as f32
                        } else {
                            1.0
                        };
                        let draw_tf = Transform::from_scale(sx, sy).post_concat(layer_tf);
                        let paint = PixmapPaint {
                            opacity: final_op,
                            quality: FilterQuality::Bilinear,
                            ..PixmapPaint::default()
                        };
                        pixmap.draw_pixmap(0, 0, (*img_pm).as_ref(), &paint, draw_tf, None);
                    }
                }
            }
        }
        0 => {
            // Precomp layer
            if let (Some(ref_id), Some(asset_list)) = (
                layer.get("refId").and_then(|v| v.as_str()),
                assets,
            ) {
                let st = layer.get("st").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let sr = layer.get("sr").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                let sub_frame = (frame - st) / sr.max(0.001);
                if let Some(comp) = asset_list
                    .iter()
                    .find(|a| a.get("id").and_then(|v| v.as_str()) == Some(ref_id))
                {
                    if let Some(sub_layers) = comp.get("layers").and_then(|v| v.as_array()) {
                        render_layer_list(pixmap, sub_layers, assets, sub_frame, layer_tf, final_op);
                    }
                }
            }
        }
        _ => {}
    }
}

fn resolve_layer_transform(
    layer: &Value,
    all_layers: &[Value],
    frame: f32,
    root_tf: Transform,
) -> (Transform, f32) {
    let mut chain = vec![layer];
    let mut cur = layer;
    for _ in 0..16 {
        let Some(parent_ind) = cur.get("parent").and_then(|v| v.as_i64()) else {
            break;
        };
        if let Some(p) = all_layers
            .iter()
            .find(|l| l.get("ind").and_then(|v| v.as_i64()) == Some(parent_ind))
        {
            chain.push(p);
            cur = p;
        } else {
            break;
        }
    }

    let mut tf = root_tf;
    for node in chain.iter().rev() {
        if let Some(ks) = node.get("ks") {
            let (local_tf, _) = eval_ks(ks, frame);
            tf = local_tf.post_concat(tf);
        }
    }
    let own_op = layer
        .get("ks")
        .map(|ks| eval_ks(ks, frame).1)
        .unwrap_or(1.0);
    (tf, own_op)
}

fn eval_ks(ks: &Value, frame: f32) -> (Transform, f32) {
    let a = ks.get("a").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let p = ks.get("p").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let s = ks.get("s").map(|v| eval_vec2(v, frame, [100.0, 100.0])).unwrap_or([100.0, 100.0]);
    let r = ks.get("r").map(|v| eval_scalar(v, frame, 0.0)).unwrap_or(0.0);
    let o = ks.get("o").map(|v| eval_scalar(v, frame, 100.0)).unwrap_or(100.0) / 100.0;

    let tf = Transform::from_translate(-a[0], -a[1])
        .post_concat(Transform::from_scale(s[0] / 100.0, s[1] / 100.0))
        .post_concat(Transform::from_rotate(r))
        .post_concat(Transform::from_translate(p[0], p[1]));
    (tf, o.clamp(0.0, 1.0))
}

fn group_has_paint(items: &[Value]) -> bool {
    items.iter().any(|it| {
        !it.get("hd").and_then(|v| v.as_bool()).unwrap_or(false)
            && matches!(
                it.get("ty").and_then(|v| v.as_str()),
                Some("fl" | "gf" | "st" | "gs")
            )
    })
}

fn collect_unpainted_group_paths(items: &[Value], frame: f32, local_tf: Transform, out: &mut Vec<Path>) {
    let mut tf = local_tf;
    for item in items {
        if item.get("ty").and_then(|v| v.as_str()) == Some("tr") {
            let (tr_tf, _) = eval_ks(item, frame);
            tf = tr_tf.post_concat(tf);
        }
    }
    for item in items {
        if item.get("hd").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let ty = item.get("ty").and_then(|v| v.as_str()).unwrap_or("");
        let maybe_path = match ty {
            "gr" => {
                if let Some(sub) = item.get("it").and_then(|v| v.as_array()) {
                    if !group_has_paint(sub) {
                        collect_unpainted_group_paths(sub, frame, tf, out);
                    }
                }
                None
            }
            "rc" => build_rect_path(item, frame),
            "el" => build_ellipse_path(item, frame),
            "sr" => build_polystar_path(item, frame),
            "sh" => build_bezier_path(item, frame),
            _ => None,
        };
        if let Some(path) = maybe_path {
            if let Some(transformed) = path.transform(tf) {
                out.push(transformed);
            }
        }
    }
}

fn render_shape_group(
    pixmap: &mut Pixmap,
    items: &[Value],
    frame: f32,
    parent_tf: Transform,
    parent_op: f32,
    parent_trim: Option<(f32, f32, f32)>,
) {
    // Check for group transform (`tr`) and trim paths (`tm`) in this item list
    let mut tf = parent_tf;
    let mut group_op = parent_op;
    let mut trim_paths: Option<(f32, f32, f32)> = parent_trim;
    for item in items {
        match item.get("ty").and_then(|v| v.as_str()) {
            Some("tr") => {
                let (tr_tf, tr_op) = eval_ks(item, frame);
                tf = tr_tf.post_concat(tf);
                group_op = (group_op * tr_op).clamp(0.0, 1.0);
            }
            Some("tm") if !item.get("hd").and_then(|v| v.as_bool()).unwrap_or(false) => {
                let s = item.get("s").map(|v| eval_scalar(v, frame, 0.0)).unwrap_or(0.0);
                let e = item.get("e").map(|v| eval_scalar(v, frame, 100.0)).unwrap_or(100.0);
                let o = item.get("o").map(|v| eval_scalar(v, frame, 0.0)).unwrap_or(0.0);
                trim_paths = Some((s, e, o));
            }
            _ => {}
        }
    }

    // Collect geometry paths in this group (including unpainted nested groups for parent-level fills)
    let mut paths: Vec<Path> = Vec::new();
    for item in items {
        if item.get("hd").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let ty = item.get("ty").and_then(|v| v.as_str()).unwrap_or("");
        match ty {
            "gr" => {
                if let Some(sub) = item.get("it").and_then(|v| v.as_array()) {
                    if !group_has_paint(sub) {
                        collect_unpainted_group_paths(sub, frame, Transform::identity(), &mut paths);
                    }
                }
            }
            "rc" => {
                if let Some(path) = build_rect_path(item, frame) {
                    paths.push(path);
                }
            }
            "el" => {
                if let Some(path) = build_ellipse_path(item, frame) {
                    paths.push(path);
                }
            }
            "sr" => {
                if let Some(path) = build_polystar_path(item, frame) {
                    paths.push(path);
                }
            }
            "sh" => {
                if let Some(path) = build_bezier_path(item, frame) {
                    paths.push(path);
                }
            }
            _ => {}
        }
    }

    // Render items in reverse order (Lottie lists shapes/subgroups top-to-bottom)
    for item in items.iter().rev() {
        if item.get("hd").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let ty = item.get("ty").and_then(|v| v.as_str()).unwrap_or("");
        match ty {
            "gr" => {
                if let Some(sub) = item.get("it").and_then(|v| v.as_array()) {
                    if group_has_paint(sub) {
                        render_shape_group(pixmap, sub, frame, tf, group_op, trim_paths);
                    }
                }
            }
            "fl" => {
                let c = resolve_color_override(
                    item,
                    item.get("c").map(|v| eval_color(v, frame)).unwrap_or([1.0, 1.0, 1.0, 1.0]),
                );
                let o = item.get("o").map(|v| eval_scalar(v, frame, 100.0)).unwrap_or(100.0) / 100.0;
                let alpha = (c[3] * o * group_op).clamp(0.0, 1.0);
                if alpha > 0.001 && !paths.is_empty() {
                    let mut paint = Paint::default();
                    paint.set_color(Color::from_rgba(c[0], c[1], c[2], alpha).unwrap_or(Color::WHITE));
                    paint.anti_alias = true;
                    let rule = if item.get("r").and_then(|v| v.as_i64()) == Some(2) {
                        FillRule::EvenOdd
                    } else {
                        FillRule::Winding
                    };
                    fill_paths_combined(pixmap, &paths, &paint, rule, tf);
                }
            }
            "gf" => {
                let o = item.get("o").map(|v| eval_scalar(v, frame, 100.0)).unwrap_or(100.0) / 100.0;
                let alpha = (o * group_op).clamp(0.0, 1.0);
                if alpha > 0.001 && !paths.is_empty() {
                    if let Some(shader) = build_gradient_shader(item, frame, alpha) {
                        let paint = Paint {
                            shader,
                            anti_alias: true,
                            ..Paint::default()
                        };
                        let rule = if item.get("r").and_then(|v| v.as_i64()) == Some(2) {
                            FillRule::EvenOdd
                        } else {
                            FillRule::Winding
                        };
                        fill_paths_combined(pixmap, &paths, &paint, rule, tf);
                    }
                }
            }
            "st" => {
                let c = resolve_color_override(
                    item,
                    item.get("c").map(|v| eval_color(v, frame)).unwrap_or([1.0, 1.0, 1.0, 1.0]),
                );
                let o = item.get("o").map(|v| eval_scalar(v, frame, 100.0)).unwrap_or(100.0) / 100.0;
                let sw = item.get("w").map(|v| eval_scalar(v, frame, 2.0)).unwrap_or(2.0);
                let alpha = (c[3] * o * group_op).clamp(0.0, 1.0);
                if alpha > 0.001 && sw > 0.0 && !paths.is_empty() {
                    let mut paint = Paint::default();
                    paint.set_color(Color::from_rgba(c[0], c[1], c[2], alpha).unwrap_or(Color::WHITE));
                    paint.anti_alias = true;
                    let stroke = build_stroke_style(item, sw);
                    stroke_paths_with_trim(pixmap, &paths, &paint, &stroke, tf, trim_paths);
                }
            }
            "gs" => {
                let o = item.get("o").map(|v| eval_scalar(v, frame, 100.0)).unwrap_or(100.0) / 100.0;
                let sw = item.get("w").map(|v| eval_scalar(v, frame, 2.0)).unwrap_or(2.0);
                let alpha = (o * group_op).clamp(0.0, 1.0);
                if alpha > 0.001 && sw > 0.0 && !paths.is_empty() {
                    if let Some(shader) = build_gradient_shader(item, frame, alpha) {
                        let paint = Paint {
                            shader,
                            anti_alias: true,
                            ..Paint::default()
                        };
                        let stroke = build_stroke_style(item, sw);
                        stroke_paths_with_trim(pixmap, &paths, &paint, &stroke, tf, trim_paths);
                    }
                }
            }
            _ => {}
        }
    }
}

fn fill_paths_combined(
    pixmap: &mut Pixmap,
    paths: &[Path],
    paint: &Paint,
    rule: FillRule,
    tf: Transform,
) {
    if paths.len() == 1 {
        pixmap.fill_path(&paths[0], paint, rule, tf, None);
        return;
    }
    let mut pb = PathBuilder::new();
    for p in paths {
        pb.push_path(p);
    }
    if let Some(combined) = pb.finish() {
        pixmap.fill_path(&combined, paint, rule, tf, None);
    }
}

fn build_stroke_style(item: &Value, sw: f32) -> Stroke {
    let mut stroke = Stroke {
        width: sw,
        line_cap: match item.get("lc").and_then(|v| v.as_i64()) {
            Some(2) => LineCap::Round,
            Some(3) => LineCap::Square,
            _ => LineCap::Butt,
        },
        line_join: match item.get("lj").and_then(|v| v.as_i64()) {
            Some(2) => LineJoin::Round,
            Some(3) => LineJoin::Bevel,
            _ => LineJoin::Miter,
        },
        ..Stroke::default()
    };
    if let Some(ml) = item.get("ml").and_then(|v| v.as_f64()) {
        stroke.miter_limit = ml as f32;
    }
    stroke
}

fn stroke_paths_with_trim(
    pixmap: &mut Pixmap,
    paths: &[Path],
    paint: &Paint,
    stroke: &Stroke,
    tf: Transform,
    trim_paths: Option<(f32, f32, f32)>,
) {
    for path in paths {
        if let Some((ts, te, to)) = trim_paths {
            let lo = ts.min(te).clamp(0.0, 100.0) / 100.0;
            let hi = ts.max(te).clamp(0.0, 100.0) / 100.0;
            let span = (hi - lo).clamp(0.0, 1.0);
            if span <= 0.0005 {
                continue;
            }
            if span < 0.9995 {
                let b = path.bounds();
                let perim = (2.0 * (b.width() + b.height())).max(1.0);
                let seg_len = (perim * span).max(0.5);
                let off_frac = (lo + (to / 360.0)).rem_euclid(1.0);
                let mut trimmed = stroke.clone();
                trimmed.dash = StrokeDash::new(vec![seg_len, perim * 2.0], -perim * off_frac);
                pixmap.stroke_path(path, paint, &trimmed, tf, None);
                continue;
            }
        }
        pixmap.stroke_path(path, paint, stroke, tf, None);
    }
}

fn sample_color_stops(stops: &[(f32, f32, f32, f32)], pos: f32) -> (f32, f32, f32) {
    if stops.is_empty() {
        return (1.0, 1.0, 1.0);
    }
    if pos <= stops[0].0 {
        return (stops[0].1, stops[0].2, stops[0].3);
    }
    let last = stops[stops.len() - 1];
    if pos >= last.0 {
        return (last.1, last.2, last.3);
    }
    for w in stops.windows(2) {
        if pos >= w[0].0 && pos <= w[1].0 {
            let denom = (w[1].0 - w[0].0).max(1e-6);
            let t = ((pos - w[0].0) / denom).clamp(0.0, 1.0);
            return (
                w[0].1 + (w[1].1 - w[0].1) * t,
                w[0].2 + (w[1].2 - w[0].2) * t,
                w[0].3 + (w[1].3 - w[0].3) * t,
            );
        }
    }
    (last.1, last.2, last.3)
}

fn sample_alpha_stops(stops: &[(f32, f32)], pos: f32) -> f32 {
    if stops.is_empty() {
        return 1.0;
    }
    if pos <= stops[0].0 {
        return stops[0].1;
    }
    let last = stops[stops.len() - 1];
    if pos >= last.0 {
        return last.1;
    }
    for w in stops.windows(2) {
        if pos >= w[0].0 && pos <= w[1].0 {
            let denom = (w[1].0 - w[0].0).max(1e-6);
            let t = ((pos - w[0].0) / denom).clamp(0.0, 1.0);
            return w[0].1 + (w[1].1 - w[0].1) * t;
        }
    }
    last.1
}

fn build_gradient_shader(item: &Value, frame: f32, opacity: f32) -> Option<Shader<'static>> {
    let g_obj = item.get("g")?;
    let p_count = g_obj.get("p").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let k_prop = g_obj.get("k")?;
    let vals = eval_vec_n(k_prop, frame, &[]);
    if p_count == 0 || vals.len() < p_count * 4 {
        return None;
    }

    let mut color_stops: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(p_count);
    for i in 0..p_count {
        let base = i * 4;
        let off = vals[base].clamp(0.0, 1.0);
        let mut r = vals[base + 1];
        let mut g = vals[base + 2];
        let mut b = vals[base + 3];
        if r > 1.0 || g > 1.0 || b > 1.0 {
            r /= 255.0;
            g /= 255.0;
            b /= 255.0;
        }
        color_stops.push((off, r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)));
    }
    color_stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut alpha_stops: Vec<(f32, f32)> = Vec::new();
    let alpha_start = p_count * 4;
    if vals.len() >= alpha_start + 2 {
        let mut idx = alpha_start;
        while idx + 1 < vals.len() {
            alpha_stops.push((vals[idx].clamp(0.0, 1.0), vals[idx + 1].clamp(0.0, 1.0)));
            idx += 2;
        }
        alpha_stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    }

    let mut all_offsets: Vec<f32> = color_stops.iter().map(|s| s.0).collect();
    for a in &alpha_stops {
        all_offsets.push(a.0);
    }
    all_offsets.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    all_offsets.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

    let stops: Vec<GradientStop> = all_offsets
        .into_iter()
        .filter_map(|pos| {
            let (r, g, b) = sample_color_stops(&color_stops, pos);
            let a = (sample_alpha_stops(&alpha_stops, pos) * opacity).clamp(0.0, 1.0);
            let color = Color::from_rgba(r, g, b, a)?;
            Some(GradientStop::new(pos, color))
        })
        .collect();

    if stops.is_empty() {
        return None;
    }

    let s = item.get("s").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let e = item.get("e").map(|v| eval_vec2(v, frame, [100.0, 0.0])).unwrap_or([100.0, 0.0]);
    let grad_type = item.get("t").and_then(|v| v.as_i64()).unwrap_or(1);

    let dist = (e[0] - s[0]).hypot(e[1] - s[1]);
    if dist < 1e-4 {
        let (r, g, b) = sample_color_stops(&color_stops, 0.0);
        let a = (sample_alpha_stops(&alpha_stops, 0.0) * opacity).clamp(0.0, 1.0);
        return Color::from_rgba(r, g, b, a).map(Shader::SolidColor);
    }

    if grad_type == 2 {
        // Radial gradient
        let h_len = item
            .get("h")
            .map(|v| eval_scalar(v, frame, 0.0))
            .unwrap_or(0.0)
            .clamp(-99.0, 99.0)
            / 100.0;
        let h_ang = item
            .get("a")
            .map(|v| eval_scalar(v, frame, 0.0))
            .unwrap_or(0.0)
            .to_radians();
        let base_ang = (e[1] - s[1]).atan2(e[0] - s[0]);
        let fx = s[0] + h_len * dist * (base_ang + h_ang).cos();
        let fy = s[1] + h_len * dist * (base_ang + h_ang).sin();
        RadialGradient::new(
            Point::from_xy(fx, fy),
            Point::from_xy(s[0], s[1]),
            dist,
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        )
    } else {
        // Linear gradient
        LinearGradient::new(
            Point::from_xy(s[0], s[1]),
            Point::from_xy(e[0], e[1]),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        )
    }
}

fn build_rect_path(item: &Value, frame: f32) -> Option<Path> {
    let p = item.get("p").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let s = item.get("s").map(|v| eval_vec2(v, frame, [100.0, 100.0])).unwrap_or([100.0, 100.0]);
    let r = item.get("r").map(|v| eval_scalar(v, frame, 0.0)).unwrap_or(0.0).max(0.0);
    let w = s[0].abs();
    let h = s[1].abs();
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let x = p[0] - w * 0.5;
    let y = p[1] - h * 0.5;
    let rad = r.min(w * 0.5).min(h * 0.5);
    let mut pb = PathBuilder::new();
    if rad <= 0.01 {
        pb.push_rect(Rect::from_xywh(x, y, w, h)?);
    } else {
        let k = rad * 0.55228475;
        pb.move_to(x + rad, y);
        pb.line_to(x + w - rad, y);
        pb.cubic_to(x + w - rad + k, y, x + w, y + rad - k, x + w, y + rad);
        pb.line_to(x + w, y + h - rad);
        pb.cubic_to(x + w, y + h - rad + k, x + w - rad + k, y + h, x + w - rad, y + h);
        pb.line_to(x + rad, y + h);
        pb.cubic_to(x + rad - k, y + h, x, y + h - rad + k, x, y + h - rad);
        pb.line_to(x, y + rad);
        pb.cubic_to(x, y + rad - k, x + rad - k, y, x + rad, y);
        pb.close();
    }
    pb.finish()
}

fn build_ellipse_path(item: &Value, frame: f32) -> Option<Path> {
    let p = item.get("p").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let s = item.get("s").map(|v| eval_vec2(v, frame, [100.0, 100.0])).unwrap_or([100.0, 100.0]);
    let rx = s[0].abs() * 0.5;
    let ry = s[1].abs() * 0.5;
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    let kx = rx * 0.55228475;
    let ky = ry * 0.55228475;
    let cx = p[0];
    let cy = p[1];
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - ry);
    pb.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
    pb.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
    pb.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
    pb.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
    pb.close();
    pb.finish()
}

fn build_polystar_path(item: &Value, frame: f32) -> Option<Path> {
    let p = item.get("p").map(|v| eval_vec2(v, frame, [0.0, 0.0])).unwrap_or([0.0, 0.0]);
    let pts = item.get("pt").map(|v| eval_scalar(v, frame, 5.0)).unwrap_or(5.0).round().max(3.0) as usize;
    let or = item.get("or").map(|v| eval_scalar(v, frame, 50.0)).unwrap_or(50.0);
    let ir = item.get("ir").map(|v| eval_scalar(v, frame, 25.0)).unwrap_or(25.0);
    let rot = item.get("r").map(|v| eval_scalar(v, frame, 0.0)).unwrap_or(0.0).to_radians();
    let is_star = item.get("sy").and_then(|v| v.as_i64()).unwrap_or(1) == 1;
    let total = if is_star { pts * 2 } else { pts };
    let mut pb = PathBuilder::new();
    for i in 0..total {
        let ang = rot - std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::TAU / (total as f32);
        let rad = if is_star && i % 2 == 1 { ir } else { or };
        let px = p[0] + rad * ang.cos();
        let py = p[1] + rad * ang.sin();
        if i == 0 {
            pb.move_to(px, py);
        } else {
            pb.line_to(px, py);
        }
    }
    pb.close();
    pb.finish()
}

fn build_bezier_path(item: &Value, frame: f32) -> Option<Path> {
    let ks = item.get("ks")?;
    let (v_pts, i_pts, o_pts, closed) = eval_shape_points(ks, frame)?;
    let n = v_pts.len();
    if n == 0 || i_pts.len() < n || o_pts.len() < n {
        return None;
    }
    let mut pb = PathBuilder::new();
    let p0 = v_pts[0];
    pb.move_to(p0[0], p0[1]);
    let segs = if closed { n } else { n.saturating_sub(1) };
    for idx in 0..segs {
        let next = (idx + 1) % n;
        let a = v_pts[idx];
        let oa = o_pts[idx];
        let b = v_pts[next];
        let ib = i_pts[next];
        pb.cubic_to(a[0] + oa[0], a[1] + oa[1], b[0] + ib[0], b[1] + ib[1], b[0], b[1]);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

fn extract_shape_points(shape_val: &Value) -> Option<(Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 2]>, bool)> {
    let v_arr = shape_val.get("v")?.as_array()?;
    let i_arr = shape_val.get("i")?.as_array()?;
    let o_arr = shape_val.get("o")?.as_array()?;
    let closed = shape_val.get("c").and_then(|v| v.as_bool()).unwrap_or(false);
    let parse_arr = |arr: &[Value]| -> Vec<[f32; 2]> {
        arr.iter()
            .map(|entry| {
                let a = entry.as_array();
                [
                    a.and_then(|x| x.first()).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                    a.and_then(|x| x.get(1)).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                ]
            })
            .collect()
    };
    Some((parse_arr(v_arr), parse_arr(i_arr), parse_arr(o_arr), closed))
}

fn lerp_pts(a: &[[f32; 2]], b: &[[f32; 2]], t: f32) -> Vec<[f32; 2]> {
    a.iter()
        .zip(b.iter())
        .map(|(p, q)| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t])
        .collect()
}

fn compile_shape_prop(prop: &Value) -> CompiledShapeProp {
    let Some(k) = prop.get("k") else {
        return CompiledShapeProp::Static(None);
    };
    let animated = prop.get("a").and_then(|v| v.as_i64()).unwrap_or(0) == 1
        || k.as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.get("t"))
            .is_some();
    if !animated {
        return CompiledShapeProp::Static(extract_shape_points(k));
    }
    let Some(kfs) = k.as_array() else {
        return CompiledShapeProp::Static(None);
    };
    let mut out = Vec::with_capacity(kfs.len());
    for kf in kfs {
        let t = kf.get("t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let hold = kf.get("h").and_then(|v| v.as_i64()) == Some(1);
        let s = kf
            .get("s")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(extract_shape_points);
        let e = kf
            .get("e")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(extract_shape_points);
        let ox = kf.pointer("/o/x").and_then(first_f32).unwrap_or(0.0);
        let oy = kf.pointer("/o/y").and_then(first_f32).unwrap_or(0.0);
        let ix = kf.pointer("/i/x").and_then(first_f32).unwrap_or(1.0);
        let iy = kf.pointer("/i/y").and_then(first_f32).unwrap_or(1.0);
        out.push(TypedShapeKf { t, hold, s, e, ox, oy, ix, iy });
    }
    CompiledShapeProp::Animated(out)
}

fn eval_shape_points(prop: &Value, frame: f32) -> Option<ShapePts> {
    let key = prop as *const Value as usize;
    COMPILED_SHAPE_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if map.len() > 16384 {
            map.clear();
        }
        let compiled = map.entry(key).or_insert_with(|| compile_shape_prop(prop));
        match compiled {
            CompiledShapeProp::Static(pts) => pts.clone(),
            CompiledShapeProp::Animated(kfs) => {
                if kfs.is_empty() {
                    return None;
                }
                if frame <= kfs[0].t {
                    return kfs[0].s.clone();
                }
                let pos = kfs.partition_point(|kf| kf.t <= frame);
                let idx = pos.saturating_sub(1);
                if idx + 1 >= kfs.len() {
                    return kfs[idx]
                        .s
                        .clone()
                        .or_else(|| if idx > 0 { kfs[idx - 1].e.clone() } else { None });
                }
                let cur = &kfs[idx];
                let next = &kfs[idx + 1];
                let s_pts = cur.s.as_ref()?;
                if cur.hold || next.t <= cur.t {
                    return Some(s_pts.clone());
                }
                if let Some(e_pts) = cur.e.as_ref().or(next.s.as_ref()) {
                    if s_pts.0.len() == e_pts.0.len()
                        && s_pts.1.len() == e_pts.1.len()
                        && s_pts.2.len() == e_pts.2.len()
                    {
                        let raw_t = ((frame - cur.t) / (next.t - cur.t)).clamp(0.0, 1.0);
                        let eased = eval_bezier_params(cur.ox, cur.oy, cur.ix, cur.iy, raw_t);
                        return Some((
                            lerp_pts(&s_pts.0, &e_pts.0, eased),
                            lerp_pts(&s_pts.1, &e_pts.1, eased),
                            lerp_pts(&s_pts.2, &e_pts.2, eased),
                            s_pts.3,
                        ));
                    }
                }
                Some(s_pts.clone())
            }
        }
    })
}

fn eval_scalar(prop: &Value, frame: f32, default: f32) -> f32 {
    eval_vec_n(prop, frame, &[default])[0]
}

fn eval_vec2(prop: &Value, frame: f32, default: [f32; 2]) -> [f32; 2] {
    if prop.get("s").and_then(|v| v.as_bool()) == Some(true) {
        let x = prop
            .get("x")
            .map(|v| eval_scalar(v, frame, default[0]))
            .unwrap_or(default[0]);
        let y = prop
            .get("y")
            .map(|v| eval_scalar(v, frame, default[1]))
            .unwrap_or(default[1]);
        return [x, y];
    }
    let v = eval_vec_n(prop, frame, &default);
    [v[0], v.get(1).copied().unwrap_or(default[1])]
}

fn eval_color(prop: &Value, frame: f32) -> [f32; 4] {
    let v = eval_vec_n(prop, frame, &[1.0, 1.0, 1.0, 1.0]);
    let mut r = v[0];
    let mut g = v.get(1).copied().unwrap_or(1.0);
    let mut b = v.get(2).copied().unwrap_or(1.0);
    let a = v.get(3).copied().unwrap_or(1.0);
    if r > 1.0 || g > 1.0 || b > 1.0 {
        r /= 255.0;
        g /= 255.0;
        b /= 255.0;
    }
    [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a.clamp(0.0, 1.0)]
}

fn compile_vec_prop(prop: &Value) -> CompiledVecProp {
    let Some(k) = prop.get("k") else {
        return CompiledVecProp::Static(Vec::new());
    };
    let animated = prop.get("a").and_then(|v| v.as_i64()).unwrap_or(0) == 1
        || k.as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.get("t"))
            .is_some();
    if !animated {
        return CompiledVecProp::Static(value_to_floats(k, &[]));
    }
    let Some(kfs) = k.as_array() else {
        return CompiledVecProp::Static(Vec::new());
    };
    let mut out = Vec::with_capacity(kfs.len());
    for kf in kfs {
        let t = kf.get("t").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let hold = kf.get("h").and_then(|v| v.as_i64()) == Some(1);
        let s = kf
            .get("s")
            .map(|v| value_to_floats(v, &[]))
            .unwrap_or_default();
        let e = kf.get("e").map(|v| value_to_floats(v, &[]));
        let ox = kf.pointer("/o/x").and_then(first_f32).unwrap_or(0.0);
        let oy = kf.pointer("/o/y").and_then(first_f32).unwrap_or(0.0);
        let ix = kf.pointer("/i/x").and_then(first_f32).unwrap_or(1.0);
        let iy = kf.pointer("/i/y").and_then(first_f32).unwrap_or(1.0);
        out.push(TypedVecKf { t, hold, s, e, ox, oy, ix, iy });
    }
    CompiledVecProp::Animated(out)
}

fn eval_vec_n(prop: &Value, frame: f32, default: &[f32]) -> Vec<f32> {
    let key = prop as *const Value as usize;
    COMPILED_VEC_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if map.len() > 32768 {
            map.clear();
        }
        let compiled = map.entry(key).or_insert_with(|| compile_vec_prop(prop));
        match compiled {
            CompiledVecProp::Static(v) => {
                if v.is_empty() {
                    default.to_vec()
                } else {
                    v.clone()
                }
            }
            CompiledVecProp::Animated(kfs) => {
                if kfs.is_empty() {
                    return default.to_vec();
                }
                if frame <= kfs[0].t {
                    return if kfs[0].s.is_empty() {
                        default.to_vec()
                    } else {
                        kfs[0].s.clone()
                    };
                }
                let pos = kfs.partition_point(|kf| kf.t <= frame);
                let idx = pos.saturating_sub(1);
                if idx + 1 >= kfs.len() {
                    if !kfs[idx].s.is_empty() {
                        return kfs[idx].s.clone();
                    }
                    if idx > 0 {
                        if let Some(prev_e) = &kfs[idx - 1].e {
                            if !prev_e.is_empty() {
                                return prev_e.clone();
                            }
                        }
                    }
                    return default.to_vec();
                }
                let cur = &kfs[idx];
                let next = &kfs[idx + 1];
                let s_vals = if cur.s.is_empty() {
                    default
                } else {
                    cur.s.as_slice()
                };
                if cur.hold || next.t <= cur.t {
                    return s_vals.to_vec();
                }
                let e_vals = cur
                    .e
                    .as_deref()
                    .filter(|v| !v.is_empty())
                    .or_else(|| (!next.s.is_empty()).then_some(next.s.as_slice()))
                    .unwrap_or(s_vals);
                let raw_t = ((frame - cur.t) / (next.t - cur.t)).clamp(0.0, 1.0);
                let eased = eval_bezier_params(cur.ox, cur.oy, cur.ix, cur.iy, raw_t);
                let len = s_vals.len().max(e_vals.len());
                (0..len)
                    .map(|i| {
                        let a = s_vals.get(i).copied().unwrap_or(0.0);
                        let b = e_vals.get(i).copied().unwrap_or(a);
                        a + (b - a) * eased
                    })
                    .collect()
            }
        }
    })
}

fn value_to_floats(v: &Value, default: &[f32]) -> Vec<f32> {
    if let Some(n) = v.as_f64() {
        return vec![n as f32];
    }
    if let Some(arr) = v.as_array() {
        let out: Vec<f32> = arr
            .iter()
            .filter_map(|x| x.as_f64().map(|f| f as f32))
            .collect();
        if !out.is_empty() {
            return out;
        }
    }
    default.to_vec()
}

fn eval_lottie_bezier(kf: &Value, t: f32) -> f32 {
    let ox = kf.pointer("/o/x").and_then(first_f32).unwrap_or(0.0);
    let oy = kf.pointer("/o/y").and_then(first_f32).unwrap_or(0.0);
    let ix = kf.pointer("/i/x").and_then(first_f32).unwrap_or(1.0);
    let iy = kf.pointer("/i/y").and_then(first_f32).unwrap_or(1.0);
    if (ox - oy).abs() < 1e-4 && (ix - iy).abs() < 1e-4 {
        return t;
    }
    // Solve cubic bezier x(u) = t via Newton-Raphson / binary search
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    let mut u = t;
    for _ in 0..8 {
        let x = cubic(u, ox, ix);
        if (x - t).abs() < 1e-4 {
            break;
        }
        if x < t {
            lo = u;
        } else {
            hi = u;
        }
        u = 0.5 * (lo + hi);
    }
    cubic(u, oy, iy)
}

fn cubic(u: f32, p1: f32, p2: f32) -> f32 {
    let inv = 1.0 - u;
    3.0 * inv * inv * u * p1 + 3.0 * inv * u * u * p2 + u * u * u
}

fn first_f32(v: &Value) -> Option<f32> {
    v.as_f64()
        .map(|f| f as f32)
        .or_else(|| v.as_array()?.first()?.as_f64().map(|f| f as f32))
}

fn parse_hex_rgb(hex: &str) -> Option<(f32, f32, f32)> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() == 6 {
        let r = u8::from_str_radix(&h[0..2], 16).ok()? as f32 / 255.0;
        let g = u8::from_str_radix(&h[2..4], 16).ok()? as f32 / 255.0;
        let b = u8::from_str_radix(&h[4..6], 16).ok()? as f32 / 255.0;
        return Some((r, g, b));
    }
    None
}

static LOTTIE_IMAGE_CACHE: OnceLock<Mutex<HashMap<u64, Option<Arc<Pixmap>>>>> = OnceLock::new();

fn decode_lottie_image_asset(asset: &Value) -> Option<Arc<Pixmap>> {
    let p = asset.get("p").and_then(|v| v.as_str())?;
    if p.is_empty() {
        return None;
    }
    let u = asset.get("u").and_then(|v| v.as_str()).unwrap_or("");

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    u.hash(&mut hasher);
    p.hash(&mut hasher);
    let key = hasher.finish();

    let cache = LOTTIE_IMAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(&key) {
            return cached.clone();
        }
    }

    let decoded = decode_image_bytes(u, p).and_then(|bytes| {
        let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let (w, h) = rgba.dimensions();
        let mut pm = Pixmap::new(w, h)?;
        let dst = pm.pixels_mut();
        for (i, px) in rgba.pixels().enumerate() {
            dst[i] = ColorU8::from_rgba(px[0], px[1], px[2], px[3]).premultiply();
        }
        Some(Arc::new(pm))
    });

    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, decoded.clone());
    }
    decoded
}

fn decode_image_bytes(u: &str, p: &str) -> Option<Vec<u8>> {
    if p.starts_with("data:") {
        let (_, payload) = p.split_once(',')?;
        decode_base64(payload)
    } else {
        let full = if u.is_empty() {
            p.to_string()
        } else {
            format!("{}{}", u, p)
        };
        std::fs::read(full).ok()
    }
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u8 = 0;
    for b in input.bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_lottie_animated_circle_moves_pixels() {
        let lottie_json = r#"{
            "v": "5.7.4", "fr": 30, "ip": 0, "op": 60, "w": 200, "h": 200,
            "layers": [{
                "ty": 4, "ind": 1, "ip": 0, "op": 60,
                "ks": {
                    "o": { "a": 0, "k": 100 },
                    "r": { "a": 0, "k": 0 },
                    "p": {
                        "a": 1,
                        "k": [
                            { "t": 0, "s": [40, 100], "e": [160, 100] },
                            { "t": 30, "s": [160, 100] }
                        ]
                    },
                    "a": { "a": 0, "k": [0, 0] },
                    "s": { "a": 0, "k": [100, 100] }
                },
                "shapes": [{
                    "ty": "gr",
                    "it": [
                        { "ty": "el", "p": { "a": 0, "k": [0, 0] }, "s": { "a": 0, "k": [50, 50] } },
                        { "ty": "fl", "c": { "a": 0, "k": [0.97, 0.45, 0.08, 1] }, "o": { "a": 0, "k": 100 } }
                    ]
                }]
            }]
        }"#;

        let mut pm0 = Pixmap::new(200, 200).unwrap();
        assert!(render_lottie_json(&mut pm0, lottie_json.as_bytes(), 0.0, true, 1.0, 0.0, 0.0, 200.0, 200.0, 0.0, 1.0));
        let mut pm1 = Pixmap::new(200, 200).unwrap();
        assert!(render_lottie_json(&mut pm1, lottie_json.as_bytes(), 1.0, true, 1.0, 0.0, 0.0, 200.0, 200.0, 0.0, 1.0));

        // Pixel at (40, 100) should be orange at t=0 and transparent at t=1
        assert!(pm0.pixel(40, 100).unwrap().alpha() > 200);
        assert_eq!(pm1.pixel(40, 100).unwrap().alpha(), 0);
        // Pixel at (160, 100) should be orange at t=1
        assert!(pm1.pixel(160, 100).unwrap().alpha() > 200);
    }

    #[test]
    fn test_eval_vec_n_bodymovin_legacy_and_hold_boundary() {
        let prop = serde_json::json!({
            "k": [
                {"t": 0, "s": [10.0], "e": [50.0]},
                {"t": 20}
            ]
        });
        assert!((eval_scalar(&prop, 0.0, 0.0) - 10.0).abs() < 0.01);
        assert!((eval_scalar(&prop, 25.0, 0.0) - 50.0).abs() < 0.01);

        let hold = serde_json::json!({
            "a": 1,
            "k": [
                {"t": 0, "s": [1.0], "h": 1},
                {"t": 10, "s": [2.0], "h": 1},
                {"t": 20, "s": [3.0]}
            ]
        });
        assert!((eval_scalar(&hold, 0.0, 0.0) - 1.0).abs() < 0.01);
        assert!((eval_scalar(&hold, 10.0, 0.0) - 2.0).abs() < 0.01);
        assert!((eval_scalar(&hold, 20.0, 0.0) - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_render_lottie_embedded_base64_image_layer() {
        let png_b64 = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
        let lottie_json = serde_json::json!({
            "v": "5.7.7",
            "fr": 30,
            "ip": 0,
            "op": 30,
            "w": 100,
            "h": 100,
            "assets": [{
                "id": "image_0",
                "w": 50,
                "h": 50,
                "u": "",
                "p": png_b64,
                "e": 1
            }],
            "layers": [{
                "ty": 2,
                "ind": 1,
                "refId": "image_0",
                "ip": 0,
                "op": 30,
                "ks": {
                    "o": { "a": 0, "k": 100 },
                    "r": { "a": 0, "k": 0 },
                    "p": { "a": 0, "k": [25, 25] },
                    "a": { "a": 0, "k": [0, 0] },
                    "s": { "a": 0, "k": [100, 100] }
                }
            }]
        })
        .to_string();

        let mut pm = Pixmap::new(100, 100).unwrap();
        assert!(render_lottie_json(
            &mut pm,
            lottie_json.as_bytes(),
            0.0,
            true,
            1.0,
            0.0,
            0.0,
            100.0,
            100.0,
            0.0,
            1.0
        ));
        let inside = pm.pixel(50, 50).unwrap();
        assert!(inside.red() > 240 && inside.green() < 10 && inside.alpha() > 240);
        assert_eq!(pm.pixel(10, 10).unwrap().alpha(), 0);
    }

    #[test]
    fn test_render_lottie_gradient_fill_and_track_matte_tp() {
        let lottie_json = serde_json::json!({
            "v": "5.7.0", "fr": 30, "ip": 0, "op": 30, "w": 100, "h": 100,
            "layers": [
                {
                    "ty": 4, "ind": 10, "td": 1, "ip": 0, "op": 30,
                    "ks": { "o": {"a":0,"k":100}, "p": {"a":0,"k":[50,50]}, "a": {"a":0,"k":[0,0]}, "s": {"a":0,"k":[100,100]}, "r": {"a":0,"k":0} },
                    "shapes": [{
                        "ty": "gr",
                        "it": [
                            { "ty": "rc", "p": {"a":0,"k":[0,0]}, "s": {"a":0,"k":[40,40]}, "r": {"a":0,"k":0} },
                            { "ty": "fl", "c": {"a":0,"k":[1,1,1,1]}, "o": {"a":0,"k":100} }
                        ]
                    }]
                },
                {
                    "ty": 4, "ind": 20, "tt": 1, "tp": 10, "ip": 0, "op": 30,
                    "ks": { "o": {"a":0,"k":100}, "p": {"a":0,"k":[50,50]}, "a": {"a":0,"k":[0,0]}, "s": {"a":0,"k":[100,100]}, "r": {"a":0,"k":0} },
                    "shapes": [{
                        "ty": "gr",
                        "it": [
                            { "ty": "rc", "p": {"a":0,"k":[0,0]}, "s": {"a":0,"k":[100,100]}, "r": {"a":0,"k":0} },
                            {
                                "ty": "gf", "t": 1, "r": 1,
                                "s": {"a":0,"k":[-50,0]}, "e": {"a":0,"k":[50,0]},
                                "o": {"a":0,"k":100},
                                "g": {"p": 2, "k": {"a":0,"k":[0, 1, 0, 0, 1, 0, 0, 1]}}
                            }
                        ]
                    }]
                }
            ]
        }).to_string();

        let mut pm = Pixmap::new(100, 100).unwrap();
        assert!(render_lottie_json(&mut pm, lottie_json.as_bytes(), 0.0, false, 1.0, 0.0, 0.0, 100.0, 100.0, 0.0, 1.0));
        // Inside matte [30..70, 30..70] should be visible purple-ish gradient
        let center = pm.pixel(50, 50).unwrap();
        assert!(center.alpha() > 240 && center.red() > 80 && center.blue() > 80);
        // Outside matte (10, 10) must be clipped to transparent
        assert_eq!(pm.pixel(10, 10).unwrap().alpha(), 0);
    }

    #[test]
    fn test_lottie_progress_and_color_overrides() {
        let doc = serde_json::json!({
            "v": "5.7.0", "fr": 30, "ip": 0, "op": 30, "w": 100, "h": 100,
            "layers": [{
                "ty": 4, "ind": 1, "ip": 0, "op": 30,
                "ks": {
                    "o": {"a":0,"k":100},
                    "p": {"a":1,"k":[
                        {"t":0,"s":[20,50],"e":[80,50]},
                        {"t":30,"s":[80,50]}
                    ]},
                    "a": {"a":0,"k":[0,0]},
                    "s": {"a":0,"k":[100,100]},
                    "r": {"a":0,"k":0}
                },
                "shapes": [{
                    "ty": "gr",
                    "it": [
                        { "ty": "rc", "p": {"a":0,"k":[0,0]}, "s": {"a":0,"k":[20,20]}, "r": {"a":0,"k":0} },
                        { "ty": "fl", "nm": "AccentFill", "c": {"a":0,"k":[1,0,0,1], "sid":"accent_slot"}, "o": {"a":0,"k":100} }
                    ]
                }]
            }]
        });

        let mut overrides = HashMap::new();
        overrides.insert("accent_slot".to_string(), "#00FF00".to_string());

        let mut pm = Pixmap::new(100, 100).unwrap();
        // Scrubbing progress=1.0 overrides time_sec=0.0, moving rect to x=80 and coloring it green via sid
        assert!(render_lottie_doc_ex(
            &mut pm,
            &doc,
            0.0,
            false,
            1.0,
            Some(1.0),
            Some(&overrides),
            100.0,
            100.0,
            Transform::identity(),
            1.0,
        ));
        let px = pm.pixel(80, 50).unwrap();
        assert!(px.alpha() > 240 && px.green() > 240 && px.red() < 15);
        assert_eq!(pm.pixel(20, 50).unwrap().alpha(), 0);

        // Test fallback matching via `nm`
        let mut nm_overrides = HashMap::new();
        nm_overrides.insert("AccentFill".to_string(), "#0000FF".to_string());
        let mut pm_nm = Pixmap::new(100, 100).unwrap();
        assert!(render_lottie_doc_ex(
            &mut pm_nm,
            &doc,
            0.0,
            false,
            1.0,
            Some(0.0),
            Some(&nm_overrides),
            100.0,
            100.0,
            Transform::identity(),
            1.0,
        ));
        let px_nm = pm_nm.pixel(20, 50).unwrap();
        assert!(px_nm.alpha() > 240 && px_nm.blue() > 240 && px_nm.red() < 15);

        // Test fallback matching via 3-digit hex key `#F00` (expanded to `#ff0000`)
        let mut hex3_overrides = HashMap::new();
        hex3_overrides.insert("#F00".to_string(), "#00FFFF".to_string());
        let mut pm_hex3 = Pixmap::new(100, 100).unwrap();
        assert!(render_lottie_doc_ex(
            &mut pm_hex3,
            &doc,
            0.0,
            false,
            1.0,
            Some(0.0),
            Some(&hex3_overrides),
            100.0,
            100.0,
            Transform::identity(),
            1.0,
        ));
        let px_h3 = pm_hex3.pixel(20, 50).unwrap();
        assert!(px_h3.alpha() > 240 && px_h3.green() > 240 && px_h3.blue() > 240 && px_h3.red() < 15);

        // Test non-identity parent_transform (90deg rotation around canvas center (50, 50))
        let rot_tf = Transform::from_rotate_at(90.0, 50.0, 50.0);
        let mut pm_rot = Pixmap::new(100, 100).unwrap();
        assert!(render_lottie_doc_ex(
            &mut pm_rot,
            &doc,
            0.0,
            false,
            1.0,
            Some(0.0),
            None,
            100.0,
            100.0,
            rot_tf,
            1.0,
        ));
        // Point (20, 50) rotated 90deg clockwise around (50, 50) lands at (50, 20)
        let px_rot = pm_rot.pixel(50, 20).unwrap();
        assert!(px_rot.alpha() > 240 && px_rot.red() > 240);
        assert_eq!(pm_rot.pixel(20, 50).unwrap().alpha(), 0);
    }
}
