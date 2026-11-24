use anyhow::{anyhow, Result};
use cosmic_text::{
    Align, Attrs, Buffer, Color as CosmicColor, Family, FontSystem, Metrics, Shaping, SwashCache,
    Weight,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, RwLock};
use tiny_skia::{
    BlendMode as SkiaBlendMode, FilterQuality, Paint, Pixmap, PixmapPaint, Rect, Transform,
};

use crate::animation::{apply_easing, eval_color_property, eval_property};
use crate::schema::{
    AnimProperty, DynamicTextSpec, FontWeightSpec, GradientTypeSpec, Layer, LayerKind,
    TextAlignSpec, TextAnimatorModeSpec, TextDirectionSpec, TextSpan,
};

use super::layout::{
    apply_anchor, build_pivot_transform_with_skew, effective_end_sec, resolve_xy, LayoutBox,
};
use super::parse_hex_color;
use super::svg_draw::build_gradient_shader;

pub(super) fn is_rtl_content(
    content: &str,
    spans: &[TextSpan],
    direction: TextDirectionSpec,
) -> bool {
    match direction {
        TextDirectionSpec::Rtl => true,
        TextDirectionSpec::Ltr => false,
        TextDirectionSpec::Auto => {
            let contains_rtl = |s: &str| {
                s.chars().any(|c| {
                    let u = c as u32;
                    (0x0590..=0x08FF).contains(&u)
                        || (0xFB1D..=0xFDFF).contains(&u)
                        || (0xFE70..=0xFEFF).contains(&u)
                })
            };
            contains_rtl(content) || spans.iter().any(|sp| contains_rtl(&sp.text))
        }
    }
}

pub(super) fn resolve_dynamic_text<'a>(
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
            if let Some(cur) = cursor
                && (p < 1.0 || ((time_sec * 2.0) as u32).is_multiple_of(2)) {
                    out.push_str(cur);
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
            std::borrow::Cow::Owned(format!(
                "{prefix}{val:.decimals$}{suffix}",
                decimals = *decimals
            ))
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

fn is_emoji_char(c: char) -> bool {
    matches!(
        c as u32,
        0x2600..=0x27BF | 0x1F300..=0x1FAFF | 0xFE0F | 0x200D
    )
}

fn find_color_emoji_family(font_system: &FontSystem) -> Option<&'static str> {
    ["Noto Color Emoji", "Apple Color Emoji", "Segoe UI Emoji"].into_iter().find(|&candidate| font_system
            .db()
            .faces()
            .any(|f| f.families.iter().any(|(name, _)| name == candidate))).map(|v| v as _)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_text_buffer(
    font_system: &mut FontSystem,
    content: &str,
    spans: &[TextSpan],
    font_size: f32,
    line_height: Option<f32>,
    font_family: Option<&str>,
    font_weight: FontWeightSpec,
    text_align: TextAlignSpec,
    direction: TextDirectionSpec,
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
    let is_rtl = is_rtl_content(content, spans, direction);
    let align = match text_align {
        TextAlignSpec::Left => Align::Left,
        TextAlignSpec::Center => Align::Center,
        TextAlignSpec::Right => Align::Right,
        TextAlignSpec::Start => {
            if is_rtl {
                Align::Right
            } else {
                Align::Left
            }
        }
        TextAlignSpec::End => {
            if is_rtl {
                Align::Left
            } else {
                Align::Right
            }
        }
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

const MAX_TEXT_MEASURE_CACHE_ENTRIES: usize = 2048;
const MAX_TEXT_RASTER_CACHE_ENTRIES: usize = 1024;

type TextRasterEntry = Option<(Arc<Pixmap>, f32, f32, bool)>;

static TEXT_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static TEXT_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

static GLOBAL_TEXT_RASTER_CACHE: LazyLock<
    RwLock<HashMap<(u64, usize, u32, u32), TextRasterEntry>>,
> = LazyLock::new(|| RwLock::new(HashMap::new()));

pub(super) fn clear_text_raster_cache() {
    GLOBAL_TEXT_RASTER_CACHE.write().unwrap_or_else(|e| e.into_inner()).clear();
}

pub(super) fn reset_text_cache_stats() {
    TEXT_CACHE_HITS.store(0, Ordering::Relaxed);
    TEXT_CACHE_MISSES.store(0, Ordering::Relaxed);
    clear_text_raster_cache();
}

pub(super) fn get_text_cache_stats() -> (u64, u64) {
    (
        TEXT_CACHE_HITS.load(Ordering::Relaxed),
        TEXT_CACHE_MISSES.load(Ordering::Relaxed),
    )
}

#[inline]
fn blend_glyph_rect_premul(
    dst: &mut [u8],
    pm_w: usize,
    pm_h: usize,
    gx: i32,
    gy: i32,
    w: u32,
    h: u32,
    col: CosmicColor,
) {
    let sa = col.a() as u32;
    if sa == 0 || w == 0 || h == 0 {
        return;
    }
    let (pr, pg, pb) = if sa == 255 {
        (col.r() as u32, col.g() as u32, col.b() as u32)
    } else {
        (
            (col.r() as u32 * sa + 127) / 255,
            (col.g() as u32 * sa + 127) / 255,
            (col.b() as u32 * sa + 127) / 255,
        )
    };
    let inv_a = 255 - sa;
    let x_start = gx.max(0) as usize;
    let y_start = gy.max(0) as usize;
    let x_end = (gx + w as i32).min(pm_w as i32).max(0) as usize;
    let y_end = (gy + h as i32).min(pm_h as i32).max(0) as usize;
    for py in y_start..y_end {
        let row_off = py * pm_w;
        for px in x_start..x_end {
            let idx = (row_off + px) * 4;
            let da = dst[idx + 3] as u32;
            if sa == 255 || da == 0 {
                dst[idx] = pr as u8;
                dst[idx + 1] = pg as u8;
                dst[idx + 2] = pb as u8;
                dst[idx + 3] = sa as u8;
            } else {
                dst[idx] = (pr + (dst[idx] as u32 * inv_a + 127) / 255).min(255) as u8;
                dst[idx + 1] = (pg + (dst[idx + 1] as u32 * inv_a + 127) / 255).min(255) as u8;
                dst[idx + 2] = (pb + (dst[idx + 2] as u32 * inv_a + 127) / 255).min(255) as u8;
                dst[idx + 3] = (sa + (da * inv_a + 127) / 255).min(255) as u8;
            }
        }
    }
}

thread_local! {
    static THREAD_TEXT_MEASURE_CACHE: std::cell::RefCell<HashMap<u64, (f32, f32, usize)>> =
        std::cell::RefCell::new(HashMap::new());
}

#[allow(clippy::too_many_arguments)]
pub(super) fn measure_text_size_and_lines(
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
) -> (f32, f32, usize) {
    let key = {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut feed = |bytes: &[u8]| {
            for &b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        };
        feed(&(font_system.db().faces().count() as u64).to_le_bytes());
        feed(content.as_bytes());
        feed(&[0xff]);
        for sp in spans {
            feed(sp.text.as_bytes());
            feed(&[0xfe]);
            if let Some(fw) = sp.font_weight {
                feed(&[match fw {
                    FontWeightSpec::Normal => 1,
                    FontWeightSpec::Bold => 2,
                }]);
            }
            if let Some(ref ff) = sp.font_family {
                feed(ff.as_bytes());
            }
            feed(&[0xfd]);
            feed(&sp.font_size.map(f32::to_bits).unwrap_or(0).to_le_bytes());
        }
        feed(&font_size.to_bits().to_le_bytes());
        feed(&line_height.map(f32::to_bits).unwrap_or(0).to_le_bytes());
        feed(&letter_spacing.to_bits().to_le_bytes());
        if let Some(ff) = font_family {
            feed(ff.as_bytes());
        }
        feed(&[match font_weight {
            FontWeightSpec::Normal => 1,
            FontWeightSpec::Bold => 2,
        }]);
        feed(&max_width.map(f32::to_bits).unwrap_or(u32::MAX).to_le_bytes());
        feed(&scale.to_bits().to_le_bytes());
        h
    };
    if let Some(cached) = THREAD_TEXT_MEASURE_CACHE.with(|c| c.borrow().get(&key).copied()) {
        return cached;
    }

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
        TextDirectionSpec::Auto,
        CosmicColor::rgba(255, 255, 255, 255),
        1.0,
        max_width,
        None,
        scale,
    ) else {
        return (0.0, lh.ceil(), 1);
    };

    let ls = letter_spacing * scale;
    let mut max_w: f32 = 0.0;
    let mut line_count: usize = 0;
    for run in buffer.layout_runs() {
        let extra = (run.glyphs.len().saturating_sub(1) as f32) * ls.max(0.0);
        max_w = max_w.max((run.line_w + extra).max(0.0));
        line_count += 1;
    }
    if line_count == 0 {
        line_count = 1;
    }
    let measured = (max_w.ceil(), (line_count as f32 * lh).ceil(), line_count);
    THREAD_TEXT_MEASURE_CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() >= MAX_TEXT_MEASURE_CACHE_ENTRIES {
            map.clear();
        }
        map.insert(key, measured);
    });
    measured
}

#[allow(clippy::too_many_arguments)]
pub(super) fn measure_text_size(
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
    let (w, h, _) = measure_text_size_and_lines(
        font_system,
        content,
        spans,
        font_size,
        line_height,
        letter_spacing,
        font_family,
        font_weight,
        max_width,
        scale,
    );
    (w, h)
}

pub(super) fn resolve_text_box_width(
    layout_override: Option<LayoutBox>,
    effective_max_w: Option<f32>,
    measured_w: f32,
    text_align: TextAlignSpec,
) -> f32 {
    if let Some(b) = layout_override {
        b.w
    } else {
        match text_align {
            TextAlignSpec::Center | TextAlignSpec::Right => effective_max_w.unwrap_or(measured_w),
            _ => measured_w.min(effective_max_w.unwrap_or(measured_w)),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rasterize_text_buffer<F>(
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    text_color: CosmicColor,
    letter_spacing: f32,
    text_align: TextAlignSpec,
    is_rtl_text: bool,
    mut plot_local: F,
) where
    F: FnMut(i32, i32, u32, u32, CosmicColor),
{
    if letter_spacing.abs() < 1e-4 {
        buffer.draw(font_system, swash_cache, text_color, |px, py, w, h, col| {
            plot_local(px, py, w, h, col);
        });
    } else {
        for run in buffer.layout_runs() {
            let glyph_count = run.glyphs.len();
            let extra_w = (glyph_count.saturating_sub(1) as f32) * letter_spacing;
            let align_shift = match text_align {
                TextAlignSpec::Left => 0.0,
                TextAlignSpec::Center => -extra_w * 0.5,
                TextAlignSpec::Right => -extra_w,
                TextAlignSpec::Start => {
                    if is_rtl_text {
                        -extra_w
                    } else {
                        0.0
                    }
                }
                TextAlignSpec::End => {
                    if is_rtl_text {
                        0.0
                    } else {
                        -extra_w
                    }
                }
            };
            for (idx, glyph) in run.glyphs.iter().enumerate() {
                let physical_glyph = glyph.physical((0., 0.), 1.0);
                let glyph_color = glyph.color_opt.unwrap_or(text_color);
                let shift_x = (align_shift + (idx as f32) * letter_spacing).round() as i32;
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
}

#[allow(clippy::too_many_arguments)]
fn compute_text_raster_key(
    font_face_count: usize,
    content: &str,
    spans: &[TextSpan],
    font_size: f32,
    line_height: Option<f32>,
    letter_spacing: f32,
    font_family: Option<&str>,
    font_weight: FontWeightSpec,
    text_align: TextAlignSpec,
    direction: TextDirectionSpec,
    is_rtl: bool,
    eff_color: &str,
    gradient: Option<&crate::schema::LinearGradientSpec>,
    outline: Option<&crate::schema::TextOutlineSpec>,
    buf_w: f32,
    scale: f32,
    op_key: u8,
    use_fx_buffer: bool,
    bw: f32,
    mh: f32,
) -> (u64, usize, u32, u32) {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    feed(&(font_face_count as u64).to_le_bytes());
    feed(content.as_bytes());
    feed(&[0xff]);
    for sp in spans {
        feed(sp.text.as_bytes());
        feed(&[0xfe]);
        if let Some(ref c) = sp.color {
            feed(c.as_bytes());
        }
        feed(&[0xfd]);
        if let Some(fw) = sp.font_weight {
            feed(&[match fw {
                FontWeightSpec::Normal => 1,
                FontWeightSpec::Bold => 2,
            }]);
        }
        if let Some(ref ff) = sp.font_family {
            feed(ff.as_bytes());
        }
        feed(&[0xfc]);
        feed(&sp.font_size.map(f32::to_bits).unwrap_or(0).to_le_bytes());
    }
    feed(&font_size.to_bits().to_le_bytes());
    feed(&line_height.map(f32::to_bits).unwrap_or(0).to_le_bytes());
    feed(&letter_spacing.to_bits().to_le_bytes());
    if let Some(ff) = font_family {
        feed(ff.as_bytes());
    }
    feed(&[match font_weight {
        FontWeightSpec::Normal => 1,
        FontWeightSpec::Bold => 2,
    }]);
    feed(&[match text_align {
        TextAlignSpec::Left => 1,
        TextAlignSpec::Center => 2,
        TextAlignSpec::Right => 3,
        TextAlignSpec::Start => 4,
        TextAlignSpec::End => 5,
    }]);
    feed(&[match direction {
        TextDirectionSpec::Auto => 1,
        TextDirectionSpec::Ltr => 2,
        TextDirectionSpec::Rtl => 3,
    }]);
    feed(&[u8::from(is_rtl), op_key, u8::from(use_fx_buffer)]);
    feed(eff_color.as_bytes());
    feed(&[0xfb]);
    if let Some(g) = gradient {
        feed(&[match g.kind {
            GradientTypeSpec::Linear => 1,
            GradientTypeSpec::Radial => 2,
        }]);
        if let Some(ref f) = g.from {
            feed(f.as_bytes());
        }
        feed(&[0xfa]);
        if let Some(ref t) = g.to {
            feed(t.as_bytes());
        }
        feed(&g.angle.to_bits().to_le_bytes());
        for st in &g.stops {
            feed(&st.offset.to_bits().to_le_bytes());
            feed(st.color.as_bytes());
        }
    }
    feed(&[0xf9]);
    if let Some(o) = outline {
        feed(o.color.as_bytes());
        feed(&o.width.to_bits().to_le_bytes());
    }
    feed(&buf_w.to_bits().to_le_bytes());
    feed(&scale.to_bits().to_le_bytes());
    (h, content.len(), bw.to_bits(), mh.to_bits())
}

struct TextAnimatorUnit {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn word_ordinal(line: &str, byte_start: usize) -> usize {
    let upto = &line[..byte_start.min(line.len())];
    let mut idx = 0usize;
    let mut prev_ws = true;
    for ch in upto.chars() {
        if ch.is_whitespace() {
            prev_ws = true;
        } else {
            if prev_ws {
                idx += 1;
            }
            prev_ws = false;
        }
    }
    if prev_ws { idx } else { idx.saturating_sub(1) }
}

fn collect_text_units(
    buffer: &Buffer,
    mode: TextAnimatorModeSpec,
    font_size: f32,
) -> Vec<TextAnimatorUnit> {
    let mut units: Vec<TextAnimatorUnit> = Vec::new();
    let mut index_of: HashMap<(usize, usize), usize> = HashMap::new();
    for run in buffer.layout_runs() {
        let line_top = run.line_top;
        let line_bottom = run.line_top + run.line_height.max(font_size);
        for glyph in run.glyphs.iter() {
            let key = match mode {
                TextAnimatorModeSpec::Char => (run.line_i, glyph.start),
                TextAnimatorModeSpec::Word => (run.line_i, word_ordinal(run.text, glyph.start)),
            };
            let pg = glyph.physical((0.0, 0.0), 1.0);
            let gx0 = pg.x as f32 - 2.0;
            let gx1 = pg.x as f32 + glyph.w.max(1.0) + 2.0;
            match index_of.get(&key) {
                Some(&i) => {
                    let u = &mut units[i];
                    u.x0 = u.x0.min(gx0);
                    u.y0 = u.y0.min(line_top);
                    u.x1 = u.x1.max(gx1);
                    u.y1 = u.y1.max(line_bottom);
                }
                None => {
                    index_of.insert(key, units.len());
                    units.push(TextAnimatorUnit {
                        x0: gx0,
                        y0: line_top,
                        x1: gx1,
                        y1: line_bottom,
                    });
                }
            }
        }
    }
    units
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_text_layer(
    layer: &Layer,
    scene_duration: f32,
    time_sec: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    pixmap: &mut Pixmap,
    layout_override: Option<LayoutBox>,
    parent_opacity: f32,
) -> Result<()> {
    let LayerKind::Text {
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
        direction,
        color,
        gradient,
        outline,
        opacity,
        max_width,
        scale,
        text_animator,
    } = &layer.kind
    else {
        return Ok(());
    };

    let effective_max_w = max_width.or_else(|| layout_override.map(|b| b.w));

    let (x, y) = resolve_xy(layer, layout_override, *x, *y, time_sec);
    let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
        * parent_opacity;
    let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
    let scale_x = eval_property(
        &layer.animations,
        AnimProperty::ScaleX,
        time_sec,
        layer.scale_x,
    );
    let scale_y = eval_property(
        &layer.animations,
        AnimProperty::ScaleY,
        time_sec,
        layer.scale_y,
    );

    if op <= 0.0 || s <= 0.0 || scale_x <= 0.0 || scale_y <= 0.0 {
        return Ok(());
    }

    let eff_color = eval_color_property(&layer.animations, AnimProperty::Color, time_sec, Some(color))
        .unwrap_or_else(|| color.clone());
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
    let bw = resolve_text_box_width(layout_override, effective_max_w, mw, *text_align);
    let (x, y) = apply_anchor(x, y, bw, mh, layer.anchor);
    let sk_x = eval_property(&layer.animations, AnimProperty::SkewX, time_sec, layer.skew_x);
    let sk_y = eval_property(&layer.animations, AnimProperty::SkewY, time_sec, layer.skew_y);
    let text_xform = build_pivot_transform_with_skew(
        x,
        y,
        bw,
        mh,
        scale_x,
        scale_y,
        0.0,
        sk_x,
        sk_y,
        layer.pivot_x,
        layer.pivot_y,
    );

    let is_rtl_text = is_rtl_content(&resolved_content, effective_spans, *direction);
    let use_fx_buffer = gradient.is_some() || outline.as_ref().is_some_and(|o| o.width > 0.0);
    let aligned_nontrivial = match text_align {
        TextAlignSpec::Left => false,
        TextAlignSpec::Center | TextAlignSpec::Right => true,
        TextAlignSpec::Start => is_rtl_text,
        TextAlignSpec::End => !is_rtl_text,
    };
    let buf_w = if let Some(mw) = effective_max_w {
        mw
    } else if let Some(b) = layout_override {
        b.w
    } else if aligned_nontrivial {
        // Center/right alignment must be measured against the text box itself;
        // using the canvas width here shifts the glyphs away from the anchor.
        bw.max(1.0)
    } else {
        (bw + 16.0).max(64.0)
    };
    let (op_key, raster_op) = if use_fx_buffer {
        let q = (op.clamp(0.0, 1.0) * 255.0).round() as u8;
        (q, q as f32 / 255.0)
    } else {
        (255u8, 1.0)
    };

    let cache_key = compute_text_raster_key(
        font_system.db().faces().count(),
        &resolved_content,
        effective_spans,
        *font_size,
        *line_height,
        *letter_spacing,
        font_family.as_deref(),
        *font_weight,
        *text_align,
        *direction,
        is_rtl_text,
        &eff_color,
        gradient.as_ref(),
        outline.as_ref(),
        buf_w,
        s,
        op_key,
        use_fx_buffer,
        bw,
        mh,
    );

    let cached = if super::cache::cache_disabled("text") {
        None
    } else {
        GLOBAL_TEXT_RASTER_CACHE
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&cache_key)
            .cloned()
    };
    let entry: TextRasterEntry = if let Some(hit) = cached {
        TEXT_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
        hit
    } else {
        TEXT_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
        let c = parse_hex_color(&eff_color, raster_op)?;
        let text_color = CosmicColor::rgba(
            (c.red() * 255.0) as u8,
            (c.green() * 255.0) as u8,
            (c.blue() * 255.0) as u8,
            (c.alpha() * 255.0) as u8,
        );
        let buffer = build_text_buffer(
            font_system,
            &resolved_content,
            effective_spans,
            *font_size,
            *line_height,
            font_family.as_deref(),
            *font_weight,
            *text_align,
            *direction,
            text_color,
            raster_op,
            Some(buf_w),
            None,
            s,
        )?;
        let ls = *letter_spacing * s;

        let built: TextRasterEntry = if !use_fx_buffer {
            let mut spans_drawn: Vec<(i32, i32, u32, u32, CosmicColor)> = Vec::with_capacity(256);
            let mut min_x = i32::MAX;
            let mut min_y = i32::MAX;
            let mut max_x = i32::MIN;
            let mut max_y = i32::MIN;

            rasterize_text_buffer(
                &buffer,
                font_system,
                swash_cache,
                text_color,
                ls,
                *text_align,
                is_rtl_text,
                |lx, ly, w, h, col| {
                    if w == 0 || h == 0 || col.a() == 0 {
                        return;
                    }
                    min_x = min_x.min(lx);
                    min_y = min_y.min(ly);
                    max_x = max_x.max(lx + w as i32);
                    max_y = max_y.max(ly + h as i32);
                    spans_drawn.push((lx, ly, w, h, col));
                },
            );

            if spans_drawn.is_empty() || max_x <= min_x || max_y <= min_y {
                None
            } else {
                let tw = (max_x - min_x) as u32;
                let th = (max_y - min_y) as u32;
                let mut pm = Pixmap::new(tw, th)
                    .ok_or_else(|| anyhow!("Failed to allocate text cache pixmap {}x{}", tw, th))?;
                let pm_w = tw as usize;
                let pm_h = th as usize;
                let dst = pm.data_mut();
                for (lx, ly, w, h, col) in spans_drawn {
                    blend_glyph_rect_premul(dst, pm_w, pm_h, lx - min_x, ly - min_y, w, h, col);
                }
                Some((Arc::new(pm), min_x as f32, min_y as f32, true))
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
            let pm_w = tb_w as usize;
            let pm_h = tb_h as usize;

            {
                let dst = fill_pm.data_mut();
                rasterize_text_buffer(
                    &buffer,
                    font_system,
                    swash_cache,
                    text_color,
                    ls,
                    *text_align,
                    is_rtl_text,
                    |lx, ly, w, h, col| {
                        blend_glyph_rect_premul(dst, pm_w, pm_h, lx + pad, ly + pad, w, h, col);
                    },
                );
            }

            if let Some(grad) = gradient
                && let Some(shader) =
                    build_gradient_shader(grad, pad as f32, pad as f32, bw, mh, raster_op)?
                {
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

            let final_pm = if let Some(out_spec) = outline.as_ref().filter(|o| o.width > 0.0) {
                let oc = parse_hex_color(&out_spec.color, raster_op)?;
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
                            dst_d[idx + 1] =
                                (oc.green() * a_f * 255.0).round().clamp(0.0, 255.0) as u8;
                            dst_d[idx + 2] =
                                (oc.blue() * a_f * 255.0).round().clamp(0.0, 255.0) as u8;
                            dst_d[idx + 3] = (a_f * 255.0).round().clamp(0.0, 255.0) as u8;
                        }
                    }
                }
                out_pm.draw_pixmap(
                    0,
                    0,
                    fill_pm.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
                out_pm
            } else {
                fill_pm
            };

            Some((Arc::new(final_pm), -(pad as f32), -(pad as f32), false))
        };

        if !super::cache::cache_disabled("text") {
            let mut guard = GLOBAL_TEXT_RASTER_CACHE.write().unwrap_or_else(|e| e.into_inner());
            if guard.len() >= MAX_TEXT_RASTER_CACHE_ENTRIES {
                guard.clear();
            }
            guard.insert(cache_key, built.clone());
        }
        built
    };

    if let Some(anim) = text_animator.as_ref() {
        if let Some((pm_arc, ox, oy, needs_op)) = entry {
            let c = parse_hex_color(&eff_color, raster_op)?;
            let text_color = CosmicColor::rgba(
                (c.red() * 255.0) as u8,
                (c.green() * 255.0) as u8,
                (c.blue() * 255.0) as u8,
                (c.alpha() * 255.0) as u8,
            );
            let buffer = build_text_buffer(
                font_system,
                &resolved_content,
                effective_spans,
                *font_size,
                *line_height,
                font_family.as_deref(),
                *font_weight,
                *text_align,
                *direction,
                text_color,
                raster_op,
                Some(buf_w),
                None,
                s,
            )?;
            let units = collect_text_units(&buffer, anim.mode, *font_size);
            let base_x = if use_fx_buffer {
                x + ox
            } else {
                (x as i32) as f32 + ox
            };
            let base_y = if use_fx_buffer {
                y + oy
            } else {
                (y as i32) as f32 + oy
            };
            let base_paint_op = if needs_op { op.clamp(0.0, 1.0) } else { 1.0 };
            let pm = pm_arc.as_ref().as_ref();
            let pm_w = pm.width() as i32;
            let pm_h = pm.height() as i32;
            let layer_t = (time_sec - layer.start_sec).max(0.0);
            for (idx, unit) in units.iter().enumerate() {
                let unit_start = anim.start_sec + idx as f32 * anim.stagger_secs;
                let raw = if anim.duration_secs > 1e-6 {
                    ((layer_t - unit_start) / anim.duration_secs).clamp(0.0, 1.0)
                } else if layer_t >= unit_start {
                    1.0
                } else {
                    0.0
                };
                let progress = apply_easing(raw, anim.easing);
                let unit_op = base_paint_op * lerp(anim.opacity_from, 1.0, progress);
                if unit_op <= 0.0 {
                    continue;
                }
                let unit_scale = lerp(anim.scale_from, 1.0, progress).max(0.0);
                if unit_scale <= 0.0 {
                    continue;
                }
                let off_x = anim.x_offset * (1.0 - progress);
                let off_y = anim.y_offset * (1.0 - progress);

                let sx0 = (unit.x0 - ox).floor().max(0.0) as i32;
                let sy0 = (unit.y0 - oy).floor().max(0.0) as i32;
                let sx1 = (unit.x1 - ox).ceil().min(pm_w as f32) as i32;
                let sy1 = (unit.y1 - oy).ceil().min(pm_h as f32) as i32;
                if sx1 <= sx0 || sy1 <= sy0 {
                    continue;
                }
                let sw = (sx1 - sx0) as u32;
                let sh = (sy1 - sy0) as u32;
                let Some(mut temp) = Pixmap::new(sw, sh) else {
                    continue;
                };
                let src = pm.data();
                let dst = temp.data_mut();
                let row_bytes = (sw * 4) as usize;
                for row in 0..sh {
                    let d = row as usize * row_bytes;
                    let s_off =
                        ((((sy0 as u32 + row) * pm.width()) + sx0 as u32) * 4) as usize;
                    dst[d..d + row_bytes].copy_from_slice(&src[s_off..s_off + row_bytes]);
                }

                let layout_x0 = sx0 as f32 + ox;
                let layout_y0 = sy0 as f32 + oy;
                let cx = layout_x0 + sw as f32 * 0.5;
                let cy = layout_y0 + sh as f32 * 0.5;
                let place = Transform::from_translate(layout_x0, layout_y0);
                let scale_t = if (unit_scale - 1.0).abs() > 1e-6 {
                    Transform::from_translate(cx, cy)
                        .post_concat(Transform::from_scale(unit_scale, unit_scale))
                        .post_concat(Transform::from_translate(-cx, -cy))
                } else {
                    Transform::identity()
                };
                let xform = Transform::from_translate(base_x + off_x, base_y + off_y)
                    .post_concat(text_xform)
                    .post_concat(Transform::from_translate(-ox, -oy))
                    .post_concat(scale_t)
                    .post_concat(place);
                let paint = PixmapPaint {
                    opacity: unit_op,
                    quality: FilterQuality::Bilinear,
                    ..PixmapPaint::default()
                };
                pixmap.draw_pixmap(0, 0, temp.as_ref(), &paint, xform, None);
            }
        }
        return Ok(());
    }

    if let Some((pm_arc, ox, oy, needs_op)) = entry {
        let base_x = if use_fx_buffer { x + ox } else { (x as i32) as f32 + ox };
        let base_y = if use_fx_buffer { y + oy } else { (y as i32) as f32 + oy };
        let dst_xform = if sk_x == 0.0
            && sk_y == 0.0
            && (scale_x - 1.0).abs() < 1e-6
            && (scale_y - 1.0).abs() < 1e-6
        {
            Transform::from_translate(base_x, base_y)
        } else {
            Transform::from_translate(base_x, base_y).post_concat(text_xform)
        };
        let paint = PixmapPaint {
            opacity: if needs_op { op.clamp(0.0, 1.0) } else { 1.0 },
            quality: if sk_x == 0.0
                && sk_y == 0.0
                && (scale_x - 1.0).abs() < 1e-6
                && (scale_y - 1.0).abs() < 1e-6
            {
                FilterQuality::Nearest
            } else {
                FilterQuality::Bilinear
            },
            ..PixmapPaint::default()
        };
        pixmap.draw_pixmap(0, 0, pm_arc.as_ref().as_ref(), &paint, dst_xform, None);
    }
    Ok(())
}
