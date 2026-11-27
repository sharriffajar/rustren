mod cache;
pub(crate) mod camera25d;
mod fx;
mod living_raster;
mod lottie;
pub mod inspect;
mod layout;
pub mod mesh3d;
mod svg_draw;
#[cfg(test)]
mod tests;
mod text;
mod yuv;

pub use cache::{
    clear_render_caches, get_cache_stats, get_frame_dedup_stats, get_text_cache_stats,
    record_frame_dedup_hits, recycle_frame_buffer, reset_cache_stats,
};
#[allow(unused_imports)]
pub use inspect::{
    LayerLayoutInfo, LayoutDiagnostic, LayoutDiagnosticKind, TextMeasureInfo,
    diagnose_layout_at_time, inspect_layout_at_time,
};

use anyhow::{Context, Result, anyhow};
use cosmic_text::{FontSystem, SwashCache};
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tiny_skia::{
    Color, FillRule, FilterQuality, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Rect,
    Transform,
};

use self::cache::{
    BLUR_CACHE_HITS, BLUR_CACHE_MISSES, GLOBAL_BLUR_CACHE, GLOBAL_SHADOW_CACHE, GLOBAL_WARP_CACHE,
    MAX_BLUR_CACHE_ENTRIES, MAX_SHADOW_CACHE_ENTRIES, MAX_SVG_RASTER_CACHE_ENTRIES,
    MAX_WARP_CACHE_ENTRIES, SVG_CACHE_HITS, SVG_CACHE_MISSES, THREAD_SIG_BUF, THREAD_SVG_CACHE,
    THREAD_SVG_RASTER_CACHE, cache_disabled, return_fx_scratch_pixmap, take_fx_scratch_pixmap,
    take_pooled_pixmap, with_thread_font_system,
};
#[cfg(feature = "pro")]
use self::cache::{
    FRAME_DEDUP_HITS, FRAME_DEDUP_MISSES, GLOBAL_FRAME_DEDUP_CACHE, MAX_FRAME_DEDUP_CACHE_ENTRIES,
    take_pooled_yuv_buffer,
};
#[cfg(feature = "pro")]
use self::yuv::rgba_to_yuv420p_into;

use crate::animation::{eval_color_property, eval_property};
use crate::assets::AssetStore;
#[cfg(feature = "exp-3d")]
use crate::schema::Mesh3dPrimitive;
use crate::schema::{
    AnimProperty, BlendModeSpec, FitSpec, Layer, LayerKind, ParticleShapeSpec, Scene,
    TrackMatteMode,
};

use self::fx::{
    apply_surface_effects_cropped, apply_wave_warp, build_shadow_pixmap_cropped,
    can_use_blur_cache,
    combine_shadow_and_foreground, crop_pixmap_padded, fast_blur_rgba, find_alpha_bounds,
    finished_cursor_blink_signature, hash_silhouette_alpha, is_extrinsic_pose_prop,
    layer_base_opacity, layer_pose_offset_and_scale, map_blend_mode, set_layer_base_scale,
    set_layer_opacity, splitmix64, warp_perspective_3d_cropped,
};
use self::layout::{
    LayoutBox, apply_anchor, build_pivot_transform_with_skew, compute_flex_layout,
    effective_end_sec, resolve_child_timelines, resolve_xy,
};
#[cfg(feature = "exp-3d")]
use self::mesh3d::{Mesh, render_mesh3d_to_pixmap};
use self::svg_draw::{
    build_progress_stroke_dash, build_rounded_rect, draw_styled_path, render_usvg_group_animated,
};

pub fn parse_hex_color(hex: &str, opacity_override: f32) -> Result<Color> {
    let trimmed = hex.trim();
    // Accept the CSS named colors that LLM-authored scenes tend to produce,
    // so `color="white"` renders instead of failing during the render pass.
    let owned;
    let trimmed = match crate::fix::named_color_to_hex(trimmed) {
        Some(hex) => {
            owned = hex;
            owned
        }
        None => trimmed,
    };
    if let Some(inner) = trimmed
        .strip_prefix("rgba(")
        .or_else(|| trimmed.strip_prefix("rgb("))
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() == 3 || parts.len() == 4 {
            let r: f32 = parts[0].parse()?;
            let g: f32 = parts[1].parse()?;
            let b: f32 = parts[2].parse()?;
            let a_raw: f32 = if parts.len() == 4 {
                parts[3].parse()?
            } else {
                1.0
            };
            let a_norm = if a_raw > 1.0 { a_raw / 255.0 } else { a_raw };
            let alpha = (a_norm * opacity_override.clamp(0.0, 1.0)).clamp(0.0, 1.0);
            return Color::from_rgba(
                (r / 255.0).clamp(0.0, 1.0),
                (g / 255.0).clamp(0.0, 1.0),
                (b / 255.0).clamp(0.0, 1.0),
                alpha,
            )
            .ok_or_else(|| anyhow!("Invalid RGBA values in '{}'", hex));
        }
        return Err(anyhow!("Invalid rgb/rgba color: {}", hex));
    }

    let hex = trimmed.trim_start_matches('#');
    // CSS shorthand hex: expand `#RGB`/`#RGBA` by doubling each nibble so the
    // schema's `{3}` alternative renders instead of failing.
    let expanded;
    let hex = match hex.len() {
        3 | 4 => {
            expanded = hex.chars().flat_map(|c| [c, c]).collect::<String>();
            expanded.as_str()
        }
        _ => hex,
    };
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
    Color::from_rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, alpha)
        .ok_or_else(|| anyhow!("Invalid RGBA values"))
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
    if layer.start_sec > 0.0
        || layer.grain > 0.0
        || layer.z_index != 0.0
        || !layer.animations.is_empty()
        || layer
            .wave_warp
            .as_ref()
            .is_some_and(|w| w.speed.abs() > 1e-6)
    {
        return false;
    }
    let end = layer
        .end_sec
        .or_else(|| layer.duration_secs.map(|d| layer.start_sec + d));
    if let Some(e) = end
        && e < scene_duration - 1e-5
    {
        return false;
    }
    match &layer.kind {
        LayerKind::Video { .. }
        | LayerKind::Waveform { .. }
        | LayerKind::Lottie { .. }
        | LayerKind::Particles { .. } => false,
        LayerKind::Text {
            text_animator: Some(_),
            ..
        } => false,
        LayerKind::Container { children, .. } => children
            .iter()
            .all(|c| is_layer_fully_static(c, scene_duration)),
        _ => true,
    }
}

/// Pre-renders canvas background and any leading 100%-static background layers into a reusable base `Pixmap`.
/// Always returns `Some(base_pixmap)` pre-filled with canvas background (and static prefix if safe).
pub fn build_static_prefix_pixmap(
    scene: &Scene,
    assets: &AssetStore,
) -> Result<(usize, Option<Pixmap>)> {
    if scene.world.is_some() {
        return Ok((0, None));
    }
    let prefix_len = scene
        .layers
        .iter()
        .take_while(|l| is_layer_fully_static(l, scene.canvas.duration_secs))
        .count();
    let dur = scene.canvas.duration_secs.max(0.0);
    let has_negative_z = if prefix_len > 0 {
        scene.layers[prefix_len..].iter().any(|l| {
            if l.z_index < 0.0 {
                return true;
            }
            if l.animations
                .iter()
                .any(|a| a.property == AnimProperty::ZIndex)
            {
                for s in 0..=16 {
                    let ts = (s as f32 / 16.0) * dur;
                    if eval_property(&l.animations, AnimProperty::ZIndex, ts, l.z_index) < 0.0 {
                        return true;
                    }
                }
            }
            false
        })
    } else {
        false
    };
    let effective_prefix = if has_negative_z { 0 } else { prefix_len };

    let width = scene.canvas.width;
    let height = scene.canvas.height;
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| anyhow!("Failed to allocate static base pixmap {}x{}", width, height))?;
    let bg = parse_hex_color(&scene.canvas.background, 1.0)?;
    pixmap.fill(bg);

    if effective_prefix > 0 {
        with_thread_font_system(assets, |font_system, swash_cache| {
            for layer in &scene.layers[..effective_prefix] {
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
    }

    Ok((effective_prefix, Some(pixmap)))
}

#[inline]
fn fnv1a_feed(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x100000001b3);
    }
}

fn hash_active_layer_state(layer: &Layer, time_sec: f32, parent_dur: f32, h: &mut u64) -> bool {
    if layer.grain > 0.0
        || layer.echo.is_some()
        || layer.track_matte != TrackMatteMode::None
        || layer
            .wave_warp
            .as_ref()
            .is_some_and(|w| w.speed.abs() > 1e-6)
    {
        return false;
    }

    for a in &layer.animations {
        fnv1a_feed(h, &[a.property as u8]);
        if matches!(a.property, AnimProperty::Color | AnimProperty::BorderColor) {
            if let Some(col) = eval_color_property(&layer.animations, a.property, time_sec, None) {
                fnv1a_feed(h, col.as_bytes());
            }
        } else {
            let val = eval_property(&layer.animations, a.property, time_sec, 0.0);
            fnv1a_feed(h, &((val * 100.0).round() as i32).to_le_bytes());
        }
        fnv1a_feed(h, &[0xfe]);
    }

    match &layer.kind {
        LayerKind::Video { .. } | LayerKind::Waveform { .. } | LayerKind::Lottie { .. } => {
            return false;
        }
        LayerKind::Particles { count, .. } if *count > 0 => {
            return false;
        }
        LayerKind::Text {
            text_animator: Some(_),
            ..
        } => {
            return false;
        }
        LayerKind::Text {
            content, dynamic, ..
        } if dynamic.is_some() => {
            let resolved =
                text::resolve_dynamic_text(layer, content, dynamic.as_ref(), time_sec, parent_dur);
            fnv1a_feed(h, resolved.as_bytes());
            fnv1a_feed(h, &[0xfd]);
        }
        LayerKind::Container {
            children,
            timeline,
            stagger_secs,
            ..
        } => {
            let container_end = effective_end_sec(layer, parent_dur);
            let child_timelines = resolve_child_timelines(
                *timeline,
                layer.start_sec,
                container_end,
                time_sec,
                parent_dur,
                *stagger_secs,
                children,
            );
            for (cidx, (child, c_time)) in children.iter().zip(child_timelines).enumerate() {
                let Some((ct, cdur, skip_win)) = c_time else {
                    continue;
                };
                if !skip_win {
                    let c_end = effective_end_sec(child, cdur);
                    if ct < child.start_sec || ct > c_end {
                        continue;
                    }
                }
                fnv1a_feed(h, &(cidx as u32).to_le_bytes());
                if !hash_active_layer_state(child, ct, cdur, h) {
                    return false;
                }
            }
        }
        _ => {}
    }

    true
}

pub fn compute_frame_temporal_signature(
    scene: &Scene,
    time_sec: f32,
    static_prefix_len: usize,
) -> Option<u64> {
    let mut h: u64 = 0xcbf29ce484222325;
    fnv1a_feed(&mut h, &scene.canvas.width.to_le_bytes());
    fnv1a_feed(&mut h, &scene.canvas.height.to_le_bytes());
    fnv1a_feed(&mut h, &(scene.layers.len() as u64).to_le_bytes());
    fnv1a_feed(&mut h, &(static_prefix_len as u64).to_le_bytes());
    fnv1a_feed(&mut h, scene.canvas.background.as_bytes());
    fnv1a_feed(&mut h, &[0xff]);

    let dur = scene.canvas.duration_secs;
    for (idx, layer) in scene.layers[static_prefix_len..].iter().enumerate() {
        let end_sec = effective_end_sec(layer, dur);
        if time_sec < layer.start_sec || time_sec > end_sec {
            continue;
        }
        fnv1a_feed(&mut h, &(idx as u32).to_le_bytes());
        if !hash_active_layer_state(layer, time_sec, dur, &mut h) {
            return None;
        }
    }
    Some(h)
}

#[cfg(test)]
pub fn render_frame(scene: &Scene, assets: &AssetStore, frame_idx: u32) -> Result<Vec<u8>> {
    render_frame_with_base(scene, assets, frame_idx, 0, None)
}

#[cfg(test)]
pub fn render_frame_with_base(
    scene: &Scene,
    assets: &AssetStore,
    frame_idx: u32,
    static_prefix_len: usize,
    static_base: Option<&Pixmap>,
) -> Result<Vec<u8>> {
    let time_sec = frame_idx as f32 / scene.canvas.fps as f32;
    let (bytes, _) = render_frame_at_time_with_base_profile(
        scene,
        assets,
        time_sec,
        static_prefix_len,
        static_base,
        false,
    )?;
    Ok(bytes)
}

#[cfg(feature = "pro")]
pub fn render_frame_arc_with_base_profiled(
    scene: &Scene,
    assets: &AssetStore,
    frame_idx: u32,
    static_prefix_len: usize,
    static_base: Option<&Pixmap>,
    collect_profile: bool,
) -> Result<(Arc<Vec<u8>>, Vec<u64>)> {
    let time_sec = frame_idx as f32 / scene.canvas.fps as f32;
    let sig = compute_frame_temporal_signature(scene, time_sec, static_prefix_len);
    if let Some(key) = sig
        && let Some(cached) = GLOBAL_FRAME_DEDUP_CACHE
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned()
    {
        FRAME_DEDUP_HITS.fetch_add(1, Ordering::Relaxed);
        return Ok((cached, Vec::new()));
    }
    FRAME_DEDUP_MISSES.fetch_add(1, Ordering::Relaxed);
    let (bytes, layer_ns) = render_frame_at_time_with_base_profile(
        scene,
        assets,
        time_sec,
        static_prefix_len,
        static_base,
        collect_profile,
    )?;
    let out_bytes = if scene.canvas.width.is_multiple_of(2) && scene.canvas.height.is_multiple_of(2)
    {
        let needed = (scene.canvas.width as usize * scene.canvas.height as usize * 3) / 2;
        let mut yuv = take_pooled_yuv_buffer(needed);
        rgba_to_yuv420p_into(&bytes, scene.canvas.width, scene.canvas.height, &mut yuv);
        cache::recycle_rgba_buffer(bytes);
        yuv
    } else {
        bytes
    };
    let arc = Arc::new(out_bytes);
    if let Some(key) = sig {
        let mut drained: Vec<Arc<Vec<u8>>> = Vec::new();
        {
            let mut guard = GLOBAL_FRAME_DEDUP_CACHE
                .write()
                .unwrap_or_else(|e| e.into_inner());
            if guard.len() >= MAX_FRAME_DEDUP_CACHE_ENTRIES {
                drained.extend(guard.drain().map(|(_, v)| v));
            }
            guard.insert(key, arc.clone());
        }
        for old_arc in drained {
            if let Ok(buf) = Arc::try_unwrap(old_arc) {
                recycle_frame_buffer(buf);
            }
        }
    }
    Ok((arc, layer_ns))
}

#[cfg(not(feature = "pro"))]
pub fn render_frame_arc_with_base_profiled(
    scene: &Scene,
    assets: &AssetStore,
    frame_idx: u32,
    static_prefix_len: usize,
    static_base: Option<&Pixmap>,
    collect_profile: bool,
) -> Result<(Arc<Vec<u8>>, Vec<u64>)> {
    let time_sec = frame_idx as f32 / scene.canvas.fps as f32;
    let (bytes, layer_ns) = render_frame_at_time_with_base_profile(
        scene,
        assets,
        time_sec,
        static_prefix_len,
        static_base,
        collect_profile,
    )?;
    Ok((Arc::new(bytes), layer_ns))
}

pub fn render_frame_at_time(scene: &Scene, assets: &AssetStore, time_sec: f32) -> Result<Vec<u8>> {
    let (bytes, _) =
        render_frame_at_time_with_base_profile(scene, assets, time_sec, 0, None, false)?;
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
        Some(base) => take_pooled_pixmap(base).unwrap_or_else(|| base.clone()),
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

    with_thread_font_system(assets, |font_system, swash_cache| {
        let slice = &scene.layers[static_prefix_len..];
        let dur = scene.canvas.duration_secs;
        let mut active_indices: Vec<usize> = slice
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                let end_sec = effective_end_sec(l, dur);
                if time_sec >= l.start_sec && time_sec <= end_sec {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        let needs_z_sort = active_indices.iter().any(|&i| {
            let l = &slice[i];
            l.z_index != 0.0
                || l.animations
                    .iter()
                    .any(|a| a.property == AnimProperty::ZIndex)
        });
        if needs_z_sort {
            let mut ranked: Vec<(usize, f32)> = active_indices
                .into_iter()
                .map(|i| {
                    let l = &slice[i];
                    (
                        i,
                        eval_property(&l.animations, AnimProperty::ZIndex, time_sec, l.z_index),
                    )
                })
                .collect();
            ranked.sort_by(|a, b| a.1.total_cmp(&b.1));
            active_indices = ranked.into_iter().map(|(i, _)| i).collect();
        }

        let world_cam = scene
            .world
            .as_ref()
            .map(|w| w.eval_at(time_sec, scene.canvas.duration_secs));

        for idx in active_indices {
            let layer = &slice[idx];
            let t0 = if collect_profile {
                Some(std::time::Instant::now())
            } else {
                None
            };
            let needs_living_raster = layer.uv_warp.is_some() || layer.relight.is_some();
            if (world_cam.is_some() && layer.space == crate::schema::LayerSpace::World)
                || needs_living_raster
            {
                thread_local! {
                    static WORLD_LAYER_SCRATCH: std::cell::RefCell<Option<Pixmap>> =
                        const { std::cell::RefCell::new(None) };
                }
                WORLD_LAYER_SCRATCH.with(|cell| -> Result<()> {
                    let mut slot = cell.borrow_mut();
                    let needs_alloc = slot
                        .as_ref()
                        .is_none_or(|p| p.width() != width || p.height() != height);
                    if needs_alloc {
                        *slot = Some(Pixmap::new(width, height).ok_or_else(|| {
                            anyhow!("Failed to allocate world layer pixmap {}x{}", width, height)
                        })?);
                    }
                    let layer_pm = slot.as_mut().unwrap();
                    layer_pm.data_mut().fill(0);
                    render_layer(
                        layer,
                        scene.canvas.duration_secs,
                        time_sec,
                        assets,
                        font_system,
                        swash_cache,
                        layer_pm,
                        None,
                        1.0,
                    )?;
                    if let Some(ref warp) = layer.uv_warp {
                        let mask = warp.mask_src.as_deref().and_then(|s| assets.images.get(s));
                        living_raster::apply_rigidity_uv_warp(layer_pm, warp, mask, time_sec);
                    }
                    if let Some(ref relight) = layer.relight {
                        let normal = relight
                            .normal_src
                            .as_deref()
                            .and_then(|s| assets.images.get(s));
                        let emission = relight
                            .emission_src
                            .as_deref()
                            .and_then(|s| assets.images.get(s));
                        living_raster::apply_2d_normal_relight(
                            layer_pm,
                            relight,
                            normal,
                            emission,
                            time_sec,
                        );
                    }
                    if let Some(cam) = world_cam
                        && layer.space == crate::schema::LayerSpace::World
                    {
                        camera25d::composite_world_layer(&mut pixmap, layer, layer_pm, cam)?;
                    } else {
                        let paint = PixmapPaint {
                            opacity: 1.0,
                            quality: FilterQuality::Nearest,
                            ..PixmapPaint::default()
                        };
                        pixmap.draw_pixmap(
                            0,
                            0,
                            layer_pm.as_ref(),
                            &paint,
                            Transform::identity(),
                            None,
                        );
                    }
                    Ok(())
                })?;
            } else {
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
            }
            if let Some(start) = t0 {
                layer_ns[static_prefix_len + idx] += start.elapsed().as_nanos() as u64;
            }
        }
        Ok(())
    })?;

    Ok((pixmap.take(), layer_ns))
}

#[allow(clippy::too_many_arguments)]
fn video_frame_index_at(
    local_t: f32,
    trim_start_sec: f32,
    trim_end_sec: Option<f32>,
    playback_rate: f32,
    loop_video: bool,
    fps: f32,
    frame_count: usize,
) -> usize {
    if frame_count == 0 {
        return 0;
    }
    let fps = if fps > 1e-6 { fps } else { 30.0 };
    let duration = frame_count as f32 / fps;
    let lo = trim_start_sec.clamp(0.0, duration);
    let hi = trim_end_sec.unwrap_or(duration).clamp(lo, duration);
    let span = (hi - lo).max(1.0 / fps);
    let rate = if playback_rate.abs() < 1e-6 {
        1.0
    } else {
        playback_rate
    };
    let mut pos = if rate >= 0.0 {
        lo + local_t * rate
    } else {
        hi + local_t * rate
    };
    if loop_video {
        let wrapped = (pos - lo).rem_euclid(span);
        // A reverse loop starts on the last frame of the segment, not the first.
        pos = if rate < 0.0 && wrapped < 1e-6 {
            hi
        } else {
            lo + wrapped
        };
    } else {
        pos = pos.clamp(lo, hi);
    }
    let max_idx = frame_count.saturating_sub(1) as isize;
    ((pos * fps).floor() as isize).clamp(0, max_idx) as usize
}

/// Convert a straight-alpha frame into premultiplied RGBA, using luminance as
/// the alpha key so black (or darker-than-threshold) pixels become transparent.
fn apply_luma_key(raw: &[u8], key: f32) -> Vec<u8> {
    let key = key.clamp(0.0, 1.0);
    let inv = (1.0 - key).max(1e-6);
    let mut out = Vec::with_capacity(raw.len());
    for px in raw.as_chunks::<4>().0 {
        let r = px[0] as f32 / 255.0;
        let g = px[1] as f32 / 255.0;
        let b = px[2] as f32 / 255.0;
        let luma = 0.299 * r + 0.587 * g + 0.114 * b;
        let a = (px[3] as f32 / 255.0) * ((luma - key) / inv).clamp(0.0, 1.0);
        out.push((r * a * 255.0).round() as u8);
        out.push((g * a * 255.0).round() as u8);
        out.push((b * a * 255.0).round() as u8);
        out.push((a * 255.0).round() as u8);
    }
    out
}

/// Soften the edge of a container clip mask so it fades out over `radius`
/// pixels instead of a hard cut (rack-focus / vignette / scrim looks).
/// Samples outside the mask count as transparent, so the outer boundary fades.
fn feather_mask_alpha(mask: &mut Mask, radius: f32) {
    let w = mask.width() as i32;
    let h = mask.height() as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let r = radius.round().clamp(1.0, 64.0) as i32;
    let cnt = (2 * r + 1) as u32;
    let data = mask.data_mut();
    let mut tmp = vec![0u8; data.len()];

    for y in 0..h {
        let row = (y * w) as usize;
        let mut sum: u32 = 0;
        for dx in -r..=r {
            if dx >= 0 && dx < w {
                sum += data[row + dx as usize] as u32;
            }
        }
        for x in 0..w {
            tmp[row + x as usize] = (sum / cnt) as u8;
            let out_x = x - r;
            if out_x >= 0 && out_x < w {
                sum -= data[row + out_x as usize] as u32;
            }
            let in_x = x + r + 1;
            if in_x >= 0 && in_x < w {
                sum += data[row + in_x as usize] as u32;
            }
        }
    }

    for x in 0..w {
        let mut sum: u32 = 0;
        for dy in 0..=2 * r {
            let sy = dy - r;
            if sy >= 0 && sy < h {
                sum += tmp[(sy * w + x) as usize] as u32;
            }
        }
        for y in 0..h {
            data[(y * w + x) as usize] = (sum / cnt) as u8;
            let out_y = y - r;
            if out_y >= 0 && out_y < h {
                sum -= tmp[(out_y * w + x) as usize] as u32;
            }
            let in_y = y + r + 1;
            if in_y >= 0 && in_y < h {
                sum += tmp[(in_y * w + x) as usize] as u32;
            }
        }
    }
}

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
    focal_x: f32,
    focal_y: f32,
    src_w: u32,
    src_h: u32,
    raw_rgba: &[u8],
) -> Result<()> {
    let target_w = layout_override
        .map(|b| b.w)
        .or(width)
        .unwrap_or(src_w as f32);
    let target_h = layout_override
        .map(|b| b.h)
        .or(height)
        .unwrap_or(src_h as f32);

    let (x, y) = resolve_xy(layer, layout_override, x, y, time_sec);
    let w = eval_property(&layer.animations, AnimProperty::Width, time_sec, target_w);
    let h = eval_property(&layer.animations, AnimProperty::Height, time_sec, target_h);
    let op =
        eval_property(&layer.animations, AnimProperty::Opacity, time_sec, opacity) * parent_opacity;
    let rot = eval_property(
        &layer.animations,
        AnimProperty::Rotation,
        time_sec,
        rotation,
    );
    let sk_x = eval_property(
        &layer.animations,
        AnimProperty::SkewX,
        time_sec,
        layer.skew_x,
    );
    let sk_y = eval_property(
        &layer.animations,
        AnimProperty::SkewY,
        time_sec,
        layer.skew_y,
    );
    let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, scale);

    let eff_sx = s
        * eval_property(
            &layer.animations,
            AnimProperty::ScaleX,
            time_sec,
            layer.scale_x,
        );
    let eff_sy = s
        * eval_property(
            &layer.animations,
            AnimProperty::ScaleY,
            time_sec,
            layer.scale_y,
        );

    if w <= 0.0 || h <= 0.0 || op <= 0.0 || eff_sx <= 0.0 || eff_sy <= 0.0 {
        return Ok(());
    }

    let (x, y) = apply_anchor(x, y, w, h, layer.anchor);
    let src_pixmap = tiny_skia::PixmapRef::from_bytes(raw_rgba, src_w, src_h)
        .ok_or_else(|| anyhow!("Failed to create PixmapRef from media frame"))?;

    let paint = PixmapPaint {
        opacity: op.clamp(0.0, 1.0),
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    let pivot_xf = build_pivot_transform_with_skew(
        x,
        y,
        w,
        h,
        eff_sx,
        eff_sy,
        rot,
        sk_x,
        sk_y,
        layer.pivot_x,
        layer.pivot_y,
    );

    // Axis-aligned SIMD fast-path (AVX2/SSE4.1/NEON via `fast_image_resize`) for Fill, Contain & Cover.
    if rot.abs() < 1e-4
        && sk_x.abs() < 1e-4
        && sk_y.abs() < 1e-4
        && (eff_sx - eff_sy).abs() < 1e-6
        && (layer.pivot_x - 0.5).abs() < 1e-6
        && (layer.pivot_y - 0.5).abs() < 1e-6
    {
        let s = eff_sx;
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        let (eff_sx, eff_sy, img_x0, img_y0, box_x0, box_y0, box_x1, box_y1) = match fit {
            FitSpec::Fill => {
                let half_w = w * 0.5 * s;
                let half_h = h * 0.5 * s;
                (
                    (w / src_w as f32) * s,
                    (h / src_h as f32) * s,
                    cx - half_w,
                    cy - half_h,
                    cx - half_w,
                    cy - half_h,
                    cx + half_w,
                    cy + half_h,
                )
            }
            FitSpec::Contain => {
                let u = (w / src_w as f32).min(h / src_h as f32);
                let half_dw = src_w as f32 * u * 0.5 * s;
                let half_dh = src_h as f32 * u * 0.5 * s;
                (
                    u * s,
                    u * s,
                    cx - half_dw,
                    cy - half_dh,
                    cx - half_dw,
                    cy - half_dh,
                    cx + half_dw,
                    cy + half_dh,
                )
            }
            FitSpec::Cover => {
                let u = (w / src_w as f32).max(h / src_h as f32);
                let half_w = w * 0.5 * s;
                let half_h = h * 0.5 * s;
                let crop_x = (src_w as f32 * u - w) * s * focal_x;
                let crop_y = (src_h as f32 * u - h) * s * focal_y;
                (
                    u * s,
                    u * s,
                    cx - half_w - crop_x,
                    cy - half_h - crop_y,
                    cx - half_w,
                    cy - half_h,
                    cx + half_w,
                    cy + half_h,
                )
            }
        };

        let canvas_w = pixmap.width();
        let canvas_h = pixmap.height();
        let dst_x0 = box_x0.max(0.0).floor() as i32;
        let dst_y0 = box_y0.max(0.0).floor() as i32;
        let dst_x1 = box_x1.min(canvas_w as f32).ceil() as i32;
        let dst_y1 = box_y1.min(canvas_h as f32).ceil() as i32;
        if dst_x1 <= dst_x0 || dst_y1 <= dst_y0 {
            return Ok(());
        }
        let dst_w = (dst_x1 - dst_x0) as u32;
        let dst_h = (dst_y1 - dst_y0) as u32;
        let src_crop_x = ((dst_x0 as f32 - img_x0) / eff_sx).clamp(0.0, (src_w - 1) as f32);
        let src_crop_y = ((dst_y0 as f32 - img_y0) / eff_sy).clamp(0.0, (src_h - 1) as f32);
        let src_crop_w = ((dst_w as f32) / eff_sx)
            .max(1e-3)
            .min(src_w as f32 - src_crop_x);
        let src_crop_h = ((dst_h as f32) / eff_sy)
            .max(1e-3)
            .min(src_h as f32 - src_crop_y);

        // 1:1 exact-size fast path: skip SIMD Bilinear convolution when no scaling/cropping is needed.
        if (eff_sx - 1.0).abs() < 1e-4
            && (eff_sy - 1.0).abs() < 1e-4
            && dst_w == src_w
            && dst_h == src_h
            && src_crop_x.abs() < 1e-3
            && src_crop_y.abs() < 1e-3
        {
            let blit_paint = PixmapPaint {
                opacity: op.clamp(0.0, 1.0),
                quality: FilterQuality::Nearest,
                ..PixmapPaint::default()
            };
            pixmap.draw_pixmap(
                dst_x0,
                dst_y0,
                src_pixmap,
                &blit_paint,
                Transform::identity(),
                None,
            );
            return Ok(());
        }

        if src_crop_w > 0.0 && src_crop_h > 0.0 {
            use fast_image_resize::images::{Image, ImageRef};
            use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
            use std::cell::RefCell;

            thread_local! {
                static SIMD_RESIZER: RefCell<(Resizer, Vec<u8>)> =
                    RefCell::new((Resizer::new(), Vec::new()));
            }

            if let Ok(src_img) = ImageRef::new(src_w, src_h, raw_rgba, PixelType::U8x4) {
                let opts = ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(FilterType::Bilinear))
                    .use_alpha(false)
                    .crop(
                        src_crop_x as f64,
                        src_crop_y as f64,
                        src_crop_w as f64,
                        src_crop_h as f64,
                    );
                let ok = SIMD_RESIZER.with(|cell| {
                    let mut borrow = cell.borrow_mut();
                    let (resizer, scratch) = &mut *borrow;
                    let needed = (dst_w as usize) * (dst_h as usize) * 4;
                    if scratch.len() < needed {
                        scratch.resize(needed, 0);
                    }
                    if let Ok(mut dst_img) =
                        Image::from_slice_u8(dst_w, dst_h, &mut scratch[..needed], PixelType::U8x4)
                        && resizer.resize(&src_img, &mut dst_img, Some(&opts)).is_ok()
                        && let Some(resized_ref) =
                            tiny_skia::PixmapRef::from_bytes(&scratch[..needed], dst_w, dst_h)
                    {
                        let blit_paint = PixmapPaint {
                            opacity: op.clamp(0.0, 1.0),
                            quality: FilterQuality::Nearest,
                            ..PixmapPaint::default()
                        };
                        pixmap.draw_pixmap(
                            dst_x0,
                            dst_y0,
                            resized_ref,
                            &blit_paint,
                            Transform::identity(),
                            None,
                        );
                        return true;
                    }
                    false
                });
                if ok {
                    return Ok(());
                }
            }
        }
    }

    match fit {
        FitSpec::Fill => {
            let sx = w / src_w as f32;
            let sy = h / src_h as f32;
            let transform = Transform::from_scale(sx, sy)
                .post_translate(x, y)
                .post_concat(pivot_xf);
            pixmap.draw_pixmap(0, 0, src_pixmap, &paint, transform, None);
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
            pixmap.draw_pixmap(0, 0, src_pixmap, &paint, transform, None);
        }
        FitSpec::Cover => {
            let u = (w / src_w as f32).max(h / src_h as f32);
            let draw_w = src_w as f32 * u;
            let draw_h = src_h as f32 * u;
            let ox = -(draw_w - w) * focal_x;
            let oy = -(draw_h - h) * focal_y;
            let direct_xf = Transform::from_scale(u, u)
                .post_translate(x + ox, y + oy)
                .post_concat(pivot_xf);
            let mut mask = Mask::new(pixmap.width(), pixmap.height())
                .ok_or_else(|| anyhow!("Failed to allocate cover clip mask"))?;
            if let Some(rect) = Rect::from_xywh(x, y, w, h) {
                let mut pb = PathBuilder::new();
                pb.push_rect(rect);
                if let Some(path) = pb.finish() {
                    mask.fill_path(&path, FillRule::Winding, false, pivot_xf);
                }
            }
            pixmap.draw_pixmap(0, 0, src_pixmap, &paint, direct_xf, Some(&mask));
        }
    }
    Ok(())
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
    render_layer_with_transform(
        layer,
        scene_duration,
        time_sec,
        assets,
        font_system,
        swash_cache,
        pixmap,
        layout_override,
        parent_opacity,
        Transform::identity(),
    )
}

#[allow(clippy::too_many_arguments)]
fn render_layer_with_transform(
    layer: &Layer,
    scene_duration: f32,
    time_sec: f32,
    assets: &AssetStore,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    pixmap: &mut Pixmap,
    layout_override: Option<LayoutBox>,
    parent_opacity: f32,
    parent_transform: Transform,
) -> Result<()> {
    if let Some(echo) = layer.echo
        && echo.count > 0
    {
        let mut echo_layer = layer.clone();
        echo_layer.echo = None;
        for i in (1..=echo.count).rev() {
            let echo_t = (time_sec - (i as f32 * echo.step_sec)).max(0.0);
            let echo_opacity = parent_opacity * echo.decay.powi(i as i32);
            render_layer_scoped(
                &echo_layer,
                scene_duration,
                echo_t,
                assets,
                font_system,
                swash_cache,
                pixmap,
                layout_override,
                echo_opacity,
                false,
                parent_transform,
            )?;
        }
    }

    if layer.track_matte != TrackMatteMode::None
        && let Some(ref matte) = layer.matte_layer
    {
        let w = pixmap.width();
        let h = pixmap.height();
        let mut layer_pm = cache::ScratchPixmap::new(w, h)
            .ok_or_else(|| anyhow::anyhow!("Failed to allocate matte layer pixmap"))?;
        let mut matte_pm = cache::ScratchPixmap::new(w, h)
            .ok_or_else(|| anyhow::anyhow!("Failed to allocate matte mask pixmap"))?;

        render_layer_scoped(
            layer,
            scene_duration,
            time_sec,
            assets,
            font_system,
            swash_cache,
            &mut layer_pm,
            layout_override,
            parent_opacity,
            false,
            Transform::identity(),
        )?;
        render_layer_scoped(
            matte,
            scene_duration,
            time_sec,
            assets,
            font_system,
            swash_cache,
            &mut matte_pm,
            layout_override,
            1.0,
            false,
            Transform::identity(),
        )?;

        let layer_data = layer_pm.data_mut();
        let matte_data = matte_pm.data();
        for (lp, mp) in layer_data
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(matte_data.chunks_exact(4))
        {
            let factor = match layer.track_matte {
                TrackMatteMode::Alpha => mp[3] as f32 / 255.0,
                TrackMatteMode::AlphaInverted => 1.0 - (mp[3] as f32 / 255.0),
                TrackMatteMode::Luma => {
                    (0.299 * mp[0] as f32 + 0.587 * mp[1] as f32 + 0.114 * mp[2] as f32) / 255.0
                }
                TrackMatteMode::LumaInverted => {
                    let a = mp[3] as f32 / 255.0;
                    let luma = (0.299 * mp[0] as f32 + 0.587 * mp[1] as f32 + 0.114 * mp[2] as f32)
                        / 255.0;
                    (a - luma).max(0.0)
                }
                TrackMatteMode::None => 1.0,
            };
            lp[0] = (lp[0] as f32 * factor).round().clamp(0.0, 255.0) as u8;
            lp[1] = (lp[1] as f32 * factor).round().clamp(0.0, 255.0) as u8;
            lp[2] = (lp[2] as f32 * factor).round().clamp(0.0, 255.0) as u8;
            lp[3] = (lp[3] as f32 * factor).round().clamp(0.0, 255.0) as u8;
        }

        pixmap.draw_pixmap(
            0,
            0,
            layer_pm.as_ref(),
            &PixmapPaint {
                quality: if parent_transform.is_identity() {
                    FilterQuality::Nearest
                } else {
                    FilterQuality::Bilinear
                },
                ..PixmapPaint::default()
            },
            parent_transform,
            None,
        );
        return Ok(());
    }

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
        parent_transform,
    )
}

#[allow(clippy::too_many_arguments)]
fn is_mesh3d_layer(layer: &Layer) -> bool {
    #[cfg(feature = "exp-3d")]
    {
        matches!(layer.kind, LayerKind::Mesh3d { .. })
    }
    #[cfg(not(feature = "exp-3d"))]
    {
        let _ = layer;
        false
    }
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
    parent_transform: Transform,
) -> Result<()> {
    if !skip_window_check {
        let end_sec = effective_end_sec(layer, scene_duration);
        if time_sec < layer.start_sec || time_sec > end_sec {
            return Ok(());
        }
    }

    let rot_x = eval_property(
        &layer.animations,
        AnimProperty::RotateX,
        time_sec,
        layer.rotate_x,
    );
    let rot_y = eval_property(
        &layer.animations,
        AnimProperty::RotateY,
        time_sec,
        layer.rotate_y,
    );
    let persp = eval_property(
        &layer.animations,
        AnimProperty::Perspective,
        time_sec,
        layer.perspective,
    );
    let has_3d = (rot_x.abs() > 1e-4 || rot_y.abs() > 1e-4) && !is_mesh3d_layer(layer);
    if has_3d && !layer.backface_visible {
        let n_z = rot_x.to_radians().cos() * rot_y.to_radians().cos();
        if n_z <= 1e-5 {
            return Ok(());
        }
    }

    let has_surface_fx = layer.grain > 0.0 || layer.vignette > 0.0 || layer.halftone > 0.0;
    let needs_fx = layer.shadow.is_some()
        || layer.blur > 0.0
        || layer.blend_mode != BlendModeSpec::Normal
        || layer.wave_warp.is_some()
        || has_surface_fx
        || has_3d;
    if !needs_fx {
        let is_vector_transform_native = matches!(
            layer.kind,
            LayerKind::Rect { .. }
                | LayerKind::Circle { .. }
                | LayerKind::Container { .. }
                | LayerKind::Lottie { .. }
        );
        if !parent_transform.is_identity() && !is_vector_transform_native {
            let mut leaf_pm = cache::ScratchPixmap::new(pixmap.width(), pixmap.height())
                .ok_or_else(|| anyhow!("Failed to allocate transformed leaf pixmap"))?;
            render_layer_inner(
                layer,
                scene_duration,
                time_sec,
                assets,
                font_system,
                swash_cache,
                &mut leaf_pm,
                layout_override,
                parent_opacity,
                Transform::identity(),
            )?;
            let paint = PixmapPaint {
                opacity: 1.0,
                quality: FilterQuality::Bilinear,
                ..PixmapPaint::default()
            };
            pixmap.draw_pixmap(0, 0, leaf_pm.as_ref(), &paint, parent_transform, None);
            return Ok(());
        }
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
            parent_transform,
        );
    }

    if parent_opacity <= 0.0 {
        return Ok(());
    }

    let eff_op = eval_property(
        &layer.animations,
        AnimProperty::Opacity,
        time_sec,
        layer_base_opacity(&layer.kind),
    ) * parent_opacity;
    if eff_op <= 0.0 {
        return Ok(());
    }

    let blend = map_blend_mode(layer.blend_mode);
    let (pw, ph) = (pixmap.width(), pixmap.height());

    if can_use_blur_cache(layer, layout_override, eff_op, time_sec) {
        let (dx, dy, eff_scale) = layer_pose_offset_and_scale(layer, layout_override, time_sec);
        let sig_hash = THREAD_SIG_BUF.with(|cell| {
            let mut buf = cell.borrow_mut();
            buf.clear();
            let blink_sig = finished_cursor_blink_signature(layer, time_sec);
            let _ = write!(
                &mut *buf,
                "{:?}|{:?}|{:?}|{:x}|{:x}|{:x}|{:?}|{:x}|{:x}|{}|{:x}|",
                layer.kind,
                layer.position,
                layer.anchor,
                layer.skew_x.to_bits(),
                layer.skew_y.to_bits(),
                layer.blur.to_bits(),
                layer.shadow,
                layer.vignette.to_bits(),
                layer.halftone.to_bits(),
                layer.animations.len(),
                blink_sig,
            );
            if let Some(b) = layout_override {
                // Bit-exact: the cached pixmap is reused verbatim, so a layout box that
                // differs below 0.001 px must not alias onto the same entry.
                let _ = write!(
                    &mut *buf,
                    "{:x},{:x},{:x},{:x},{:x},{:x}",
                    b.x.to_bits(),
                    b.y.to_bits(),
                    b.w.to_bits(),
                    b.h.to_bits(),
                    b.parent_x.to_bits(),
                    b.parent_y.to_bits()
                );
            }
            hash_bytes_fnv1a(buf.as_bytes())
        });
        let key = (sig_hash, pw, ph);

        let cached = {
            if cache_disabled("blur") {
                None
            } else {
                let guard = GLOBAL_BLUR_CACHE.read().unwrap_or_else(|e| e.into_inner());
                guard.get(&key).cloned()
            }
        };

        let entry = if let Some(hit) = cached {
            BLUR_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            hit
        } else {
            BLUR_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
            let mut un_faded = layer.clone();
            un_faded
                .animations
                .retain(|a| !is_extrinsic_pose_prop(a.property));
            set_layer_opacity(&mut un_faded.kind, 1.0);
            set_layer_base_scale(&mut un_faded.kind, 1.0);

            let mut layer_pixmap = take_fx_scratch_pixmap(pw, ph)?;
            render_layer_inner(
                &un_faded,
                scene_duration,
                time_sec,
                assets,
                font_system,
                swash_cache,
                &mut layer_pixmap,
                layout_override,
                1.0,
                Transform::identity(),
            )?;
            let bounds = find_alpha_bounds(&layer_pixmap);
            let built = if let Some((x0, y0, x1, y1)) = bounds {
                let base_px = (x0 + x1) as f32 * 0.5;
                let base_py = (y0 + y1) as f32 * 0.5;
                let sh_entry = if let Some(sh) = &layer.shadow {
                    let (sh_pm, sx, sy) = build_shadow_pixmap_cropped(
                        &layer_pixmap,
                        x0,
                        y0,
                        x1,
                        y1,
                        &sh.color,
                        sh.blur,
                    )?;
                    Some((sh_pm, sx as f32 + sh.offset_x, sy as f32 + sh.offset_y))
                } else {
                    None
                };

                let (mut fg_pm, fx, fy) = if layer.blur > 0.0 {
                    let pad = (layer.blur * 1.6).ceil() as u32 + 2;
                    let (mut blur_pm, bx, by) =
                        crop_pixmap_padded(&layer_pixmap, x0, y0, x1, y1, pad)?;
                    let (bw, bh) = (blur_pm.width(), blur_pm.height());
                    fast_blur_rgba(blur_pm.data_mut(), bw, bh, layer.blur);
                    (blur_pm, bx, by)
                } else {
                    crop_pixmap_padded(&layer_pixmap, x0, y0, x1, y1, 0)?
                };

                if layer.vignette > 0.0 || layer.halftone > 0.0 {
                    apply_surface_effects_cropped(
                        &mut fg_pm,
                        fx,
                        fy,
                        0.0,
                        layer.vignette,
                        layer.halftone,
                        0.0,
                    );
                }

                let sh_ref = sh_entry.as_ref().map(|(pm, sx, sy)| (pm, *sx, *sy));
                let (comb_pm, c_bx, c_by) = combine_shadow_and_foreground(sh_ref, fg_pm, fx, fy)?;
                Some((Arc::new(comb_pm), c_bx, c_by, base_px, base_py))
            } else {
                None
            };
            return_fx_scratch_pixmap(layer_pixmap, bounds);
            if !cache_disabled("blur") {
                let mut guard = GLOBAL_BLUR_CACHE.write().unwrap_or_else(|e| e.into_inner());
                if guard.len() >= MAX_BLUR_CACHE_ENTRIES {
                    guard.clear();
                }
                guard.insert(key, built.clone());
            }
            built
        };

        if let Some((cached_pm, bx, by, base_px, base_py)) = entry {
            let paint = PixmapPaint {
                opacity: eff_op.clamp(0.0, 1.0),
                blend_mode: blend,
                quality: FilterQuality::Bilinear,
            };
            let nearest_paint = PixmapPaint {
                opacity: eff_op.clamp(0.0, 1.0),
                blend_mode: blend,
                quality: FilterQuality::Nearest,
            };
            if has_3d {
                // Bit-exact pose key: rounded quantisation aliased subtly different
                // rotations onto one cached warp, so a cache hit could emit different
                // pixels than the uncached path. Identical pose floats still hit, which
                // is the case this cache exists for (static 3D layers).
                let wkey = (
                    sig_hash,
                    rot_x.to_bits(),
                    rot_y.to_bits(),
                    persp.to_bits(),
                    eff_scale.to_bits(),
                );
                let warped_cached = {
                    if cache_disabled("warp") {
                        None
                    } else {
                        let guard = GLOBAL_WARP_CACHE.read().unwrap_or_else(|e| e.into_inner());
                        guard.get(&wkey).cloned()
                    }
                };
                let warped_entry = if let Some(hit) = warped_cached {
                    hit
                } else {
                    let built = warp_perspective_3d_cropped(
                        cached_pm.as_ref(),
                        bx,
                        by,
                        base_px,
                        base_py,
                        eff_scale,
                        rot_x,
                        rot_y,
                        persp,
                        layer.backface_visible,
                        pw,
                        ph,
                    )?
                    .map(|(pm, wx, wy)| (Arc::new(pm), wx, wy));
                    if !cache_disabled("warp") {
                        let mut guard =
                            GLOBAL_WARP_CACHE.write().unwrap_or_else(|e| e.into_inner());
                        if guard.len() >= MAX_WARP_CACHE_ENTRIES {
                            guard.clear();
                        }
                        guard.insert(wkey, built.clone());
                    }
                    built
                };
                if let Some((w_pm, wx, wy)) = warped_entry {
                    pixmap.draw_pixmap(
                        wx + dx.round() as i32,
                        wy + dy.round() as i32,
                        w_pm.as_ref().as_ref(),
                        &nearest_paint,
                        Transform::identity(),
                        None,
                    );
                }
            } else if (eff_scale - 1.0).abs() > 1e-4 {
                let xform = Transform::from_translate(bx as f32 - base_px, by as f32 - base_py)
                    .post_scale(eff_scale, eff_scale)
                    .post_translate(base_px + dx, base_py + dy);
                pixmap.draw_pixmap(0, 0, cached_pm.as_ref().as_ref(), &paint, xform, None);
            } else {
                pixmap.draw_pixmap(
                    bx + dx.round() as i32,
                    by + dy.round() as i32,
                    cached_pm.as_ref().as_ref(),
                    &nearest_paint,
                    Transform::identity(),
                    None,
                );
            }
        }
        return Ok(());
    }

    let mut layer_pixmap = take_fx_scratch_pixmap(pw, ph)?;
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
        Transform::identity(),
    )?;

    let bounds = find_alpha_bounds(&layer_pixmap);
    let Some((x0, y0, x1, y1)) = bounds else {
        return_fx_scratch_pixmap(layer_pixmap, None);
        return Ok(());
    };
    let pivot_x = (x0 + x1) as f32 * 0.5;
    let pivot_y = (y0 + y1) as f32 * 0.5;

    let sh_entry: Option<(Arc<Pixmap>, f32, f32)> = if let Some(sh) = &layer.shadow {
        let bw_u = x1 - x0;
        let bh_u = y1 - y0;
        let silhouette_key = if eff_op >= 0.99 && layer.skew_x == 0.0 && layer.skew_y == 0.0 {
            match &layer.kind {
                LayerKind::Container {
                    width: Some(cw),
                    height: Some(ch),
                    background,
                    gradient,
                    radius,
                    rotation,
                    ..
                } if (background.is_some() || gradient.is_some())
                    && *rotation == 0.0
                    && ((bw_u as f32) - *cw).abs() <= 6.0
                    && ((bh_u as f32) - *ch).abs() <= 6.0 =>
                {
                    let eff_rad =
                        eval_property(&layer.animations, AnimProperty::Radius, time_sec, *radius)
                            .max(0.0);
                    Some((
                        bw_u,
                        bh_u,
                        eff_rad.to_bits(),
                        hash_bytes_fnv1a(sh.color.as_bytes()),
                        sh.blur.to_bits(),
                        hash_silhouette_alpha(&layer_pixmap, x0, y0, x1, y1),
                    ))
                }
                LayerKind::Rect {
                    radius, rotation, ..
                } if *rotation == 0.0 => {
                    let eff_rad =
                        eval_property(&layer.animations, AnimProperty::Radius, time_sec, *radius)
                            .max(0.0);
                    Some((
                        bw_u,
                        bh_u,
                        eff_rad.to_bits(),
                        hash_bytes_fnv1a(sh.color.as_bytes()),
                        sh.blur.to_bits(),
                        hash_silhouette_alpha(&layer_pixmap, x0, y0, x1, y1),
                    ))
                }
                _ => None,
            }
        } else {
            None
        };

        let cached_sh = if cache_disabled("shadow") {
            None
        } else {
            silhouette_key.and_then(|k| {
                GLOBAL_SHADOW_CACHE
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&k)
                    .cloned()
            })
        };
        let pad = if sh.blur > 0.0 {
            (sh.blur * 1.6).ceil() as i32 + 2
        } else {
            0
        };
        if let Some(sh_arc) = cached_sh {
            Some((
                sh_arc,
                (x0 as i32 - pad) as f32 + sh.offset_x,
                (y0 as i32 - pad) as f32 + sh.offset_y,
            ))
        } else {
            let (sh_pm, sx, sy) =
                build_shadow_pixmap_cropped(&layer_pixmap, x0, y0, x1, y1, &sh.color, sh.blur)?;
            let sh_arc = Arc::new(sh_pm);
            if let Some(k) = silhouette_key
                && !cache_disabled("shadow")
            {
                let mut guard = GLOBAL_SHADOW_CACHE
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                if guard.len() >= MAX_SHADOW_CACHE_ENTRIES {
                    guard.clear();
                }
                guard.insert(k, sh_arc.clone());
            }
            Some((sh_arc, sx as f32 + sh.offset_x, sy as f32 + sh.offset_y))
        }
    } else {
        None
    };

    let wave_pad = layer
        .wave_warp
        .as_ref()
        .map(|w| w.amplitude.abs().ceil() as u32 + 1)
        .unwrap_or(0);
    let (mut fg_pm, fg_x, fg_y) = if layer.blur > 0.0 {
        let pad = (layer.blur * 1.6).ceil() as u32 + 2 + wave_pad;
        let (mut blur_pm, bx, by) = crop_pixmap_padded(&layer_pixmap, x0, y0, x1, y1, pad)?;
        let (bw, bh) = (blur_pm.width(), blur_pm.height());
        fast_blur_rgba(blur_pm.data_mut(), bw, bh, layer.blur);
        (blur_pm, bx, by)
    } else {
        crop_pixmap_padded(&layer_pixmap, x0, y0, x1, y1, wave_pad)?
    };
    return_fx_scratch_pixmap(layer_pixmap, bounds);

    if has_surface_fx {
        apply_surface_effects_cropped(
            &mut fg_pm,
            fg_x,
            fg_y,
            layer.grain,
            layer.vignette,
            layer.halftone,
            time_sec,
        );
    }

    if let Some(spec) = layer.wave_warp.as_ref() {
        apply_wave_warp(&mut fg_pm, fg_x, fg_y, spec, time_sec);
    }

    let paint = PixmapPaint {
        opacity: 1.0,
        blend_mode: blend,
        quality: FilterQuality::Nearest,
    };
    if has_3d {
        let sh_ref = sh_entry
            .as_ref()
            .map(|(pm, sx, sy)| (pm.as_ref(), *sx, *sy));
        let (comb_pm, c_bx, c_by) = combine_shadow_and_foreground(sh_ref, fg_pm, fg_x, fg_y)?;
        if let Some((w_fg, wx, wy)) = warp_perspective_3d_cropped(
            &comb_pm,
            c_bx,
            c_by,
            pivot_x,
            pivot_y,
            1.0,
            rot_x,
            rot_y,
            persp,
            layer.backface_visible,
            pw,
            ph,
        )? {
            let xform = Transform::from_translate(wx as f32, wy as f32).post_concat(parent_transform);
            pixmap.draw_pixmap(0, 0, w_fg.as_ref(), &paint, xform, None);
        }
    } else {
        if let Some((sh_arc, sx, sy)) = sh_entry {
            let sh_xform =
                Transform::from_translate(sx.round(), sy.round()).post_concat(parent_transform);
            pixmap.draw_pixmap(0, 0, sh_arc.as_ref().as_ref(), &paint, sh_xform, None);
        }
        let fg_xform = Transform::from_translate(fg_x as f32, fg_y as f32).post_concat(parent_transform);
        pixmap.draw_pixmap(0, 0, fg_pm.as_ref(), &paint, fg_xform, None);
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
    parent_transform: Transform,
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
            let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
                * parent_opacity;
            let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
            let eff_sx = s
                * eval_property(
                    &layer.animations,
                    AnimProperty::ScaleX,
                    time_sec,
                    layer.scale_x,
                );
            let eff_sy = s
                * eval_property(
                    &layer.animations,
                    AnimProperty::ScaleY,
                    time_sec,
                    layer.scale_y,
                );
            let rot = eval_property(
                &layer.animations,
                AnimProperty::Rotation,
                time_sec,
                *rotation,
            );
            let eff_color = eval_color_property(
                &layer.animations,
                AnimProperty::Color,
                time_sec,
                Some(color),
            )
            .unwrap_or_else(|| color.clone());
            let eff_border_color = eval_color_property(
                &layer.animations,
                AnimProperty::BorderColor,
                time_sec,
                border_color.as_deref(),
            );
            let prog = eval_property(
                &layer.animations,
                AnimProperty::PathProgress,
                time_sec,
                *path_progress,
            )
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

            if w <= 0.0 || h <= 0.0 || op <= 0.0 || eff_sx <= 0.0 || eff_sy <= 0.0 {
                return Ok(());
            }

            let eff_radius =
                eval_property(&layer.animations, AnimProperty::Radius, time_sec, *radius).max(0.0);
            let (x, y) = apply_anchor(x, y, w, h, layer.anchor);
            let sk_x = eval_property(
                &layer.animations,
                AnimProperty::SkewX,
                time_sec,
                layer.skew_x,
            );
            let sk_y = eval_property(
                &layer.animations,
                AnimProperty::SkewY,
                time_sec,
                layer.skew_y,
            );
            if let Some(path) = build_rounded_rect(x, y, w, h, eff_radius) {
                let xform = build_pivot_transform_with_skew(
                    x,
                    y,
                    w,
                    h,
                    eff_sx,
                    eff_sy,
                    rot,
                    sk_x,
                    sk_y,
                    layer.pivot_x,
                    layer.pivot_y,
                );
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
                    xform.post_concat(parent_transform),
                )?;
            }
        }
        LayerKind::Circle {
            cx,
            cy,
            r,
            color,
            gradient,
            border_color,
            border_width,
            path_progress,
            stroke_dashoffset,
            opacity,
            scale,
        } => {
            let cx_base_offset = layout_override
                .map(|b| b.x + b.w * 0.5 - *cx)
                .unwrap_or(0.0);
            let cy_base_offset = layout_override
                .map(|b| b.y + b.h * 0.5 - *cy)
                .unwrap_or(0.0);

            let cx = cx_base_offset
                + eval_property(&layer.animations, AnimProperty::Cx, time_sec, *cx)
                + eval_property(&layer.animations, AnimProperty::X, time_sec, 0.0);
            let cy = cy_base_offset
                + eval_property(&layer.animations, AnimProperty::Cy, time_sec, *cy)
                + eval_property(&layer.animations, AnimProperty::Y, time_sec, 0.0);
            let scale_val = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
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
            let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
                * parent_opacity;
            let r = eval_property(&layer.animations, AnimProperty::R, time_sec, *r) * scale_val;
            let eff_color = eval_color_property(
                &layer.animations,
                AnimProperty::Color,
                time_sec,
                Some(color),
            )
            .unwrap_or_else(|| color.clone());
            let eff_border_color = eval_color_property(
                &layer.animations,
                AnimProperty::BorderColor,
                time_sec,
                border_color.as_deref(),
            );
            let prog = eval_property(
                &layer.animations,
                AnimProperty::PathProgress,
                time_sec,
                *path_progress,
            )
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

            if r <= 0.0 || op <= 0.0 || scale_x <= 0.0 || scale_y <= 0.0 {
                return Ok(());
            }

            let xform = build_pivot_transform_with_skew(
                cx - r,
                cy - r,
                r * 2.0,
                r * 2.0,
                scale_x,
                scale_y,
                0.0,
                0.0,
                0.0,
                layer.pivot_x,
                layer.pivot_y,
            );

            if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
                let circumference = 2.0 * std::f32::consts::PI * r;
                let stroke_dash = build_progress_stroke_dash(circumference, prog, dash_offset, None);
                draw_styled_path(
                    pixmap,
                    &path,
                    cx - r,
                    cy - r,
                    r * 2.0,
                    r * 2.0,
                    Some(&eff_color),
                    gradient.as_ref(),
                    eff_border_color.as_deref(),
                    *border_width,
                    stroke_dash,
                    op,
                    xform.post_concat(parent_transform),
                )?;
            }
        }
        LayerKind::Text { .. } => {
            text::render_text_layer(
                layer,
                scene_duration,
                time_sec,
                font_system,
                swash_cache,
                pixmap,
                layout_override,
                parent_opacity,
            )?;
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
                0.5,
                0.5,
                img.width(),
                img.height(),
                img.as_raw(),
            )?;
        }
        LayerKind::Waveform {
            x,
            y,
            width,
            height,
            bars,
            bar_gap,
            color,
            seed,
            speed,
            opacity,
            rotation,
            scale,
        } => {
            let (rx, ry) = resolve_xy(layer, layout_override, *x, *y, time_sec);
            let base_w = layout_override.map(|b| b.w).unwrap_or(*width);
            let base_h = layout_override.map(|b| b.h).unwrap_or(*height);
            let sc = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
            let w = eval_property(&layer.animations, AnimProperty::Width, time_sec, base_w) * sc;
            let h = eval_property(&layer.animations, AnimProperty::Height, time_sec, base_h) * sc;
            let op = (eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
                * parent_opacity)
                .clamp(0.0, 1.0);
            let rot = eval_property(
                &layer.animations,
                AnimProperty::Rotation,
                time_sec,
                *rotation,
            );

            let num_bars = (*bars).max(1);
            let gap = (*bar_gap).max(0.0);
            let total_gap = gap * (num_bars.saturating_sub(1) as f32);
            let bar_w = ((w - total_gap) / (num_bars as f32)).max(1.0);

            let eff_color = eval_color_property(
                &layer.animations,
                AnimProperty::Color,
                time_sec,
                Some(color),
            )
            .unwrap_or_else(|| color.clone());
            let col = parse_hex_color(&eff_color, op)?;
            let mut paint = Paint::default();
            paint.set_color(col);
            paint.anti_alias = true;

            let tf = Transform::from_rotate_at(rot, rx + w * 0.5, ry + h * 0.5);

            for i in 0..num_bars {
                let phase = ((*seed as f32) * 0.137 + (i as f32) * 0.73)
                    + time_sec * (*speed) * (2.5 + ((i % 5) as f32) * 0.45);
                let amp = 0.18 + 0.82 * (0.5 + 0.5 * phase.sin() * (phase * 0.41).cos()).abs();
                let bh = (h * amp).max(2.0);
                let bx = rx + (i as f32) * (bar_w + gap);
                let by = ry + (h - bh) * 0.5;
                if let Some(rect) = Rect::from_xywh(bx, by, bar_w, bh) {
                    let mut pb = PathBuilder::new();
                    pb.push_rect(rect);
                    if let Some(path) = pb.finish() {
                        pixmap.fill_path(&path, &paint, FillRule::Winding, tf, None);
                    }
                }
            }
        }
        LayerKind::Lottie {
            x,
            y,
            width,
            height,
            src,
            loop_animation,
            speed,
            progress,
            color_overrides,
            opacity,
            rotation,
            scale,
        } => {
            let (rx, ry) = resolve_xy(layer, layout_override, *x, *y, time_sec);
            let target_w = layout_override.map(|b| b.w).or(*width).unwrap_or(200.0);
            let target_h = layout_override.map(|b| b.h).or(*height).unwrap_or(200.0);
            let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
            let eff_sx = eval_property(
                &layer.animations,
                AnimProperty::ScaleX,
                time_sec,
                layer.scale_x,
            );
            let eff_sy = eval_property(
                &layer.animations,
                AnimProperty::ScaleY,
                time_sec,
                layer.scale_y,
            );
            let w = eval_property(&layer.animations, AnimProperty::Width, time_sec, target_w) * s;
            let h = eval_property(&layer.animations, AnimProperty::Height, time_sec, target_h) * s;
            let (rx, ry) = apply_anchor(rx, ry, w, h, layer.anchor);
            let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
                * parent_opacity;
            let rot = eval_property(
                &layer.animations,
                AnimProperty::Rotation,
                time_sec,
                *rotation,
            );
            let sk_x = eval_property(
                &layer.animations,
                AnimProperty::SkewX,
                time_sec,
                layer.skew_x,
            );
            let sk_y = eval_property(
                &layer.animations,
                AnimProperty::SkewY,
                time_sec,
                layer.skew_y,
            );
            let has_progress_anim = layer
                .animations
                .iter()
                .any(|a| a.property == AnimProperty::Progress);
            let progress_override = if progress.is_some() || has_progress_anim {
                Some(
                    eval_property(
                        &layer.animations,
                        AnimProperty::Progress,
                        time_sec,
                        progress.unwrap_or(0.0),
                    )
                    .clamp(0.0, 1.0),
                )
            } else {
                None
            };

            if op > 0.0 && w > 0.0 && h > 0.0 && eff_sx > 0.0 && eff_sy > 0.0 {
                let xform = build_pivot_transform_with_skew(
                    rx,
                    ry,
                    w,
                    h,
                    eff_sx,
                    eff_sy,
                    rot,
                    sk_x,
                    sk_y,
                    layer.pivot_x,
                    layer.pivot_y,
                );
                let dst_tf = Transform::from_translate(rx, ry)
                    .post_concat(xform)
                    .post_concat(parent_transform);
                let rendered = assets
                    .lotties
                    .get(src)
                    .map(|doc| {
                        lottie::render_lottie_doc_ex(
                            pixmap,
                            doc,
                            time_sec,
                            *loop_animation,
                            *speed,
                            progress_override,
                            color_overrides.as_ref(),
                            w,
                            h,
                            dst_tf,
                            op,
                        )
                    })
                    .unwrap_or(false);
                if !rendered {
                    let mut paint = Paint::default();
                    paint.set_color_rgba8(70, 130, 180, (140.0 * op) as u8);
                    paint.anti_alias = true;
                    let tf = xform.post_concat(parent_transform);
                    if let Some(rect) = Rect::from_xywh(rx, ry, w, h) {
                        let mut pb = PathBuilder::new();
                        pb.push_rect(rect);
                        if let Some(path) = pb.finish() {
                            pixmap.fill_path(&path, &paint, FillRule::Winding, tf, None);
                        }
                    }
                }
            }
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
            loop_video,
            focal_x,
            focal_y,
            luma_key,
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
            let frame_idx = video_frame_index_at(
                local_t,
                *trim_start_sec,
                *trim_end_sec,
                *playback_rate,
                *loop_video,
                video.fps,
                video.frame_count,
            );
            let eff_focal_x = eval_property(
                &layer.animations,
                AnimProperty::FocalX,
                time_sec,
                *focal_x,
            )
            .clamp(0.0, 1.0);
            let eff_focal_y = eval_property(
                &layer.animations,
                AnimProperty::FocalY,
                time_sec,
                *focal_y,
            )
            .clamp(0.0, 1.0);
            let frame = video.frame(frame_idx)?;
            let keyed_frame = luma_key
                .as_ref()
                .map(|k| apply_luma_key(frame.as_slice(), *k));
            let frame_data: &[u8] = keyed_frame.as_deref().unwrap_or(frame.as_slice());
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
                eff_focal_x,
                eff_focal_y,
                video.width,
                video.height,
                frame_data,
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
                if let std::collections::hash_map::Entry::Vacant(e) = cache.entry(svg_key) {
                    let parsed =
                        resvg::usvg::Tree::from_data(svg_bytes, &resvg::usvg::Options::default())
                            .context("Failed to parse SVG")?;
                    e.insert(parsed);
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
                let op =
                    eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
                        * parent_opacity;
                let rot = eval_property(
                    &layer.animations,
                    AnimProperty::Rotation,
                    time_sec,
                    *rotation,
                );
                let prog = eval_property(
                    &layer.animations,
                    AnimProperty::PathProgress,
                    time_sec,
                    *path_progress,
                )
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
                let sk_x = eval_property(
                    &layer.animations,
                    AnimProperty::SkewX,
                    time_sec,
                    layer.skew_x,
                );
                let sk_y = eval_property(
                    &layer.animations,
                    AnimProperty::SkewY,
                    time_sec,
                    layer.skew_y,
                );
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
                        if cache_disabled("svg") || !rcache.contains_key(&raster_key) {
                            SVG_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
                            let mut svg_pixmap = Pixmap::new(pw, ph).ok_or_else(|| {
                                anyhow!("Failed to allocate SVG pixmap {}x{}", pw, ph)
                            })?;
                            let scale_xform = Transform::from_scale(sx, sy);
                            resvg::render(tree, scale_xform, &mut svg_pixmap.as_mut());
                            if rcache.len() >= MAX_SVG_RASTER_CACHE_ENTRIES {
                                rcache.clear();
                            }
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
                            x,
                            y,
                            w,
                            h,
                            scale_x,
                            scale_y,
                            rot,
                            sk_x,
                            sk_y,
                            layer.pivot_x,
                            layer.pivot_y,
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
                let pivot_xf = build_pivot_transform_with_skew(
                    x,
                    y,
                    w,
                    h,
                    scale_x,
                    scale_y,
                    rot,
                    sk_x,
                    sk_y,
                    layer.pivot_x,
                    layer.pivot_y,
                );
                let xform = Transform::from_translate(x, y).post_concat(pivot_xf);
                pixmap.draw_pixmap(0, 0, svg_pixmap.as_ref(), &paint, xform, None);
                Ok(())
            })?;
        }
        LayerKind::Particles {
            x,
            y,
            width,
            height,
            count,
            seed,
            speed,
            gravity,
            spread,
            angle,
            size,
            color,
            colors,
            shape,
            opacity,
            rotation,
            scale,
        } => {
            let (rx, ry) = resolve_xy(layer, layout_override, *x, *y, time_sec);
            let base_w = layout_override.map(|b| b.w).unwrap_or(*width);
            let base_h = layout_override.map(|b| b.h).unwrap_or(*height);
            let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
            let w = eval_property(&layer.animations, AnimProperty::Width, time_sec, base_w) * s;
            let h = eval_property(&layer.animations, AnimProperty::Height, time_sec, base_h) * s;
            let op = (eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
                * parent_opacity)
                .clamp(0.0, 1.0);
            let rot = eval_property(
                &layer.animations,
                AnimProperty::Rotation,
                time_sec,
                *rotation,
            );
            if op <= 0.0 || w <= 0.0 || h <= 0.0 || *count == 0 {
                return Ok(());
            }

            let (rx, ry) = apply_anchor(rx, ry, w, h, layer.anchor);
            let ox = rx + w * 0.5;
            let oy = ry + h * 0.5;
            let local_t = (time_sec - layer.start_sec).max(0.0);

            const DEFAULT_CONFETTI_COLORS: [&str; 5] =
                ["#f59e0b", "#38bdf8", "#ec4899", "#22c55e", "#a855f7"];
            let palette: Vec<Color> = if !colors.is_empty() {
                colors
                    .iter()
                    .map(|c| parse_hex_color(c, op))
                    .collect::<Result<Vec<_>>>()?
            } else if *shape == ParticleShapeSpec::Confetti && color == "#FFFFFF" {
                DEFAULT_CONFETTI_COLORS
                    .iter()
                    .map(|c| parse_hex_color(c, op))
                    .collect::<Result<Vec<_>>>()?
            } else {
                vec![parse_hex_color(color, op)?]
            };

            let mut paint = Paint {
                anti_alias: true,
                ..Paint::default()
            };

            for i in 0..*count {
                let h0 = splitmix64(*seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
                let h1 = splitmix64(h0);
                let h2 = splitmix64(h1);
                let h3 = splitmix64(h2);

                let u0 = ((h0 & 0xFFFF) as f32) / 65535.0;
                let u1 = ((h1 & 0xFFFF) as f32) / 65535.0;
                let u2 = ((h2 & 0xFFFF) as f32) / 65535.0;
                let u3 = ((h3 & 0xFFFF) as f32) / 65535.0;

                let theta = (angle + (u0 - 0.5) * spread).to_radians();
                let v0 = (80.0 + u1 * 180.0) * speed;
                let vx = theta.cos() * v0;
                let vy = theta.sin() * v0;

                let px0 = ox + (u2 - 0.5) * w * 0.12;
                let py0 = oy + (u3 - 0.5) * h * 0.12;
                let px = px0 + vx * local_t;
                let py = py0 + vy * local_t + 0.5 * gravity * local_t * local_t;
                let ps = (size * (0.65 + 0.7 * u2) * s).max(1.5);

                paint.set_color(palette[(h1 as usize) % palette.len()]);
                match shape {
                    ParticleShapeSpec::Circle => {
                        if let Some(path) = PathBuilder::from_circle(px, py, ps * 0.5) {
                            pixmap.fill_path(
                                &path,
                                &paint,
                                FillRule::Winding,
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                    ParticleShapeSpec::Rect | ParticleShapeSpec::Confetti => {
                        let p_rot = rot + (u2 - 0.5) * 360.0 + (u3 - 0.5) * 540.0 * local_t;
                        let (rw, rh) = if *shape == ParticleShapeSpec::Confetti {
                            let flutter = (p_rot * 2.0).to_radians().cos().abs().max(0.25);
                            (ps * 1.4, ps * 0.65 * flutter)
                        } else {
                            (ps, ps)
                        };
                        if let Some(rect) = Rect::from_xywh(px - rw * 0.5, py - rh * 0.5, rw, rh) {
                            let mut pb = PathBuilder::new();
                            pb.push_rect(rect);
                            if let Some(path) = pb.finish() {
                                let tf = Transform::from_rotate_at(p_rot, px, py);
                                pixmap.fill_path(&path, &paint, FillRule::Winding, tf, None);
                            }
                        }
                    }
                }
            }
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
            clip_feather,
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
            let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
                * parent_opacity;
            let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
            let eff_sx = s
                * eval_property(
                    &layer.animations,
                    AnimProperty::ScaleX,
                    time_sec,
                    layer.scale_x,
                );
            let eff_sy = s
                * eval_property(
                    &layer.animations,
                    AnimProperty::ScaleY,
                    time_sec,
                    layer.scale_y,
                );
            let rot = eval_property(
                &layer.animations,
                AnimProperty::Rotation,
                time_sec,
                *rotation,
            );

            if op <= 0.0 || eff_sx <= 0.0 || eff_sy <= 0.0 {
                return Ok(());
            }

            let (cw, ch, child_boxes) =
                compute_flex_layout(target_w, target_h, layout, children, font_system, time_sec)?;

            if cw <= 0.0 || ch <= 0.0 {
                return Ok(());
            }

            let (cx, cy) = apply_anchor(cx, cy, cw, ch, layer.anchor);
            let sk_x = eval_property(
                &layer.animations,
                AnimProperty::SkewX,
                time_sec,
                layer.skew_x,
            );
            let sk_y = eval_property(
                &layer.animations,
                AnimProperty::SkewY,
                time_sec,
                layer.skew_y,
            );
            let xform = build_pivot_transform_with_skew(
                cx,
                cy,
                cw,
                ch,
                eff_sx,
                eff_sy,
                rot,
                sk_x,
                sk_y,
                layer.pivot_x,
                layer.pivot_y,
            );

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

            let eff_radius =
                eval_property(&layer.animations, AnimProperty::Radius, time_sec, *radius).max(0.0);
            if (eff_bg.is_some() || gradient.is_some() || eff_border.is_some())
                && let Some(path) = build_rounded_rect(cx, cy, cw, ch, eff_radius)
            {
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
                    xform.post_concat(parent_transform),
                )?;
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
            let mut active_children: Vec<(&Layer, LayoutBox, f32, f32, f32)> = children
                .iter()
                .zip(child_boxes)
                .zip(child_timelines)
                .filter_map(|((child, cbox), c_time)| {
                    c_time.map(|(ct, cdur, _skip_win)| {
                        let z = eval_property(
                            &child.animations,
                            AnimProperty::ZIndex,
                            ct,
                            child.z_index,
                        );
                        (child, cbox, ct, cdur, z)
                    })
                })
                .collect();
            if active_children.iter().any(|(_, _, _, _, z)| *z != 0.0) {
                active_children.sort_by(|a, b| a.4.total_cmp(&b.4));
            }

            if *clip {
                let pw = (cw.ceil() as u32).max(1);
                let ph = (ch.ceil() as u32).max(1);
                let mut child_pixmap = cache::ScratchPixmap::new(pw, ph)
                    .ok_or_else(|| anyhow!("Failed to allocate container pixmap {}x{}", pw, ph))?;
                for (child, cbox, ct, cdur, _) in active_children {
                    let local_box = LayoutBox {
                        x: cbox.x,
                        y: cbox.y,
                        w: cbox.w,
                        h: cbox.h,
                        parent_x: 0.0,
                        parent_y: 0.0,
                    };
                    render_layer_with_transform(
                        child,
                        cdur,
                        ct,
                        assets,
                        font_system,
                        swash_cache,
                        &mut child_pixmap,
                        Some(local_box),
                        if *cascade_opacity { op } else { 1.0 },
                        Transform::identity(),
                    )?;
                }
                if let Some(mask_path) = build_rounded_rect(0.0, 0.0, cw, ch, eff_radius)
                    && let Some(mut mask) = Mask::new(pw, ph)
                {
                    mask.fill_path(&mask_path, FillRule::Winding, true, Transform::identity());
                    if *clip_feather > 0.0 {
                        feather_mask_alpha(&mut mask, *clip_feather);
                    }
                    child_pixmap.apply_mask(&mask);
                }
                let paint = PixmapPaint {
                    opacity: 1.0,
                    quality: FilterQuality::Bilinear,
                    ..PixmapPaint::default()
                };
                let composite_xform = Transform::from_translate(cx, cy)
                    .post_concat(xform)
                    .post_concat(parent_transform);
                pixmap.draw_pixmap(0, 0, child_pixmap.as_ref(), &paint, composite_xform, None);
            } else {
                let child_parent_xform = xform.post_concat(parent_transform);
                for (child, cbox, ct, cdur, _) in active_children {
                    let abs_box = LayoutBox {
                        x: cx + cbox.x,
                        y: cy + cbox.y,
                        w: cbox.w,
                        h: cbox.h,
                        parent_x: cx,
                        parent_y: cy,
                    };
                    render_layer_with_transform(
                        child,
                        cdur,
                        ct,
                        assets,
                        font_system,
                        swash_cache,
                        pixmap,
                        Some(abs_box),
                        if *cascade_opacity { op } else { 1.0 },
                        child_parent_xform,
                    )?;
                }
            }
        }
        #[cfg(feature = "exp-3d")]
        LayerKind::Mesh3d {
            x,
            y,
            width,
            height,
            primitive,
            src,
            texture,
            opacity,
            scale,
            rotate_x,
            rotate_y,
            rotate_z,
            fov,
            color,
            light_dir,
            wireframe,
        } => {
            let (rx, ry) = resolve_xy(layer, layout_override, *x, *y, time_sec);
            let op = eval_property(&layer.animations, AnimProperty::Opacity, time_sec, *opacity)
                .clamp(0.0, 1.0)
                * parent_opacity;
            let s = eval_property(&layer.animations, AnimProperty::Scale, time_sec, *scale);
            let w = *width * s;
            let h = *height * s;

            let rot_x = eval_property(
                &layer.animations,
                AnimProperty::RotateX,
                time_sec,
                *rotate_x,
            );
            let rot_y = eval_property(
                &layer.animations,
                AnimProperty::RotateY,
                time_sec,
                *rotate_y,
            );
            let rot_z = eval_property(
                &layer.animations,
                AnimProperty::RotateZ,
                time_sec,
                *rotate_z,
            );

            if op <= 0.0 || w <= 0.0 || h <= 0.0 {
                return Ok(());
            }

            let (rx, ry) = apply_anchor(rx, ry, w, h, layer.anchor);

            // Select mesh: a local `.glb`/`.gltf` when `src` is set, otherwise the
            // procedural primitive. Imported models are cached in `AssetStore`
            // so they parse once instead of on every frame.
            let mesh: std::sync::Arc<Mesh> = match src {
                Some(path) if !path.trim().is_empty() => assets
                    .mesh3d
                    .get(path)
                    .cloned()
                    .ok_or_else(|| anyhow!("Mesh3d model not preloaded: {path}"))?,
                _ => std::sync::Arc::new(match primitive {
                    Mesh3dPrimitive::Cube => Mesh::cube(),
                    Mesh3dPrimitive::Sphere => Mesh::sphere(16, 32),
                    Mesh3dPrimitive::Pyramid => Mesh::pyramid(),
                    Mesh3dPrimitive::Torus => Mesh::torus(0.5, 0.2, 24, 16),
                }),
            };

            // Texture is resolved lazily by path and cached in a `OnceLock`, so
            // repeated frames reuse one decoded RGBA buffer.
            let mesh_texture = match texture {
                Some(path) if !path.trim().is_empty() => {
                    Some(crate::renderer::mesh3d::load_texture(path)?)
                }
                _ => None,
            };

            let light_dir = light_dir.unwrap_or([0.5, -1.0, 0.5]);

            render_mesh3d_to_pixmap(
                &mut pixmap.as_mut(),
                &mesh,
                rx,
                ry,
                w,
                h,
                *fov,
                color,
                light_dir,
                *wireframe,
                op,
                s,
                rot_x,
                rot_y,
                rot_z,
                mesh_texture.as_deref(),
            );
        }
    }

    Ok(())
}
