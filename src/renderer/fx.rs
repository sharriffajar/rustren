use anyhow::{anyhow, Result};
use tiny_skia::{BlendMode as SkiaBlendMode, Pixmap};

use crate::animation::eval_property;
use crate::schema::{
    AnimProperty, BlendModeSpec, DynamicTextSpec, Layer, LayerKind, PositionSpec, WaveAxisSpec,
    WaveWarpSpec,
};

use super::layout::{resolve_xy, LayoutBox};
use super::parse_hex_color;

pub(super) fn map_blend_mode(mode: BlendModeSpec) -> SkiaBlendMode {
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
pub(super) fn fast_blur_rgba(data: &mut [u8], w: u32, h: u32, radius: f32) {
    if radius >= 8.0 && w >= 64 && h >= 64 {
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

pub(super) fn find_alpha_bounds(pixmap: &Pixmap) -> Option<(u32, u32, u32, u32)> {
    let w = pixmap.width();
    let h = pixmap.height();
    let data = pixmap.data();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let wu = w as usize;

    for y in 0..h {
        let row_start = (y as usize) * wu * 4;
        let row = &data[row_start..row_start + wu * 4];
        let mut chunks = row.chunks_exact(8);
        let has_nonzero_chunk = chunks.by_ref().any(|c| {
            u64::from_ne_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]) != 0
        });
        let has_nonzero = has_nonzero_chunk || chunks.remainder().iter().any(|&b| b != 0);
        if !has_nonzero {
            continue;
        }
        // Fast check from left and right edges for non-zero alpha
        let mut left = 0usize;
        while left < wu {
            if row[left * 4 + 3] != 0 {
                break;
            }
            left += 1;
        }
        if left < wu {
            let mut right = wu;
            while right > left + 1 {
                if row[(right - 1) * 4 + 3] != 0 {
                    break;
                }
                right -= 1;
            }
            if y < min_y {
                min_y = y;
            }
            max_y = y + 1;
            if (left as u32) < min_x {
                min_x = left as u32;
            }
            if (right as u32) > max_x {
                max_x = right as u32;
            }
        }
    }

    if min_x < max_x && min_y < max_y {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

/// Zero out only the `[x0..x1, y0..y1]` dirty rectangle of a reusable scratch `Pixmap`.
pub(super) fn clear_pixmap_rect(pixmap: &mut Pixmap, x0: u32, y0: u32, x1: u32, y1: u32) {
    let pw = pixmap.width();
    let ph = pixmap.height();
    let cx0 = x0.min(pw) as usize;
    let cy0 = y0.min(ph) as usize;
    let cx1 = x1.min(pw) as usize;
    let cy1 = y1.min(ph) as usize;
    if cx1 <= cx0 || cy1 <= cy0 {
        return;
    }
    let stride = (pw as usize) * 4;
    let row_bytes = (cx1 - cx0) * 4;
    let data = pixmap.data_mut();
    for y in cy0..cy1 {
        let off = y * stride + cx0 * 4;
        data[off..off + row_bytes].fill(0);
    }
}

pub(super) fn crop_pixmap_padded(
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

/// FNV-1a hash over the alpha channel of the `[x0..x1, y0..y1]` region.
///
/// Used as part of the shadow-cache key: anti-aliased edges make two frames with the
/// *same* integer alpha bounds have *different* coverage. Keying on bounds alone let a
/// frame reuse a silhouette built from another frame's edge coverage, which produced
/// ±1..4 RGB drift that depended on thread scheduling. Hashing the mask makes the key
/// imply an identical silhouette.
pub(super) fn hash_silhouette_alpha(src: &Pixmap, x0: u32, y0: u32, x1: u32, y1: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let sw = src.width() as usize;
    let data = src.data();
    let cx1 = (x1 as usize).min(sw);
    let cx0 = (x0 as usize).min(cx1);
    let rows = (y1 as usize).min(src.height() as usize);
    for y in (y0 as usize)..rows {
        let base = (y * sw + cx0) * 4;
        let end = (y * sw + cx1) * 4;
        for i in (base..end).step_by(4) {
            h ^= data[i + 3] as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
    }
    h
}

pub(super) fn build_shadow_pixmap_cropped(
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
        for (s, d) in s_row.as_chunks::<4>().0.iter().zip(d_row.chunks_exact_mut(4)) {
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

#[inline]
pub(super) fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

static GRAIN_LUT_256: std::sync::LazyLock<[i8; 256 * 256]> = std::sync::LazyLock::new(|| {
    let mut lut = [0i8; 256 * 256];
    for (i, slot) in lut.iter_mut().enumerate() {
        let h = splitmix64((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xA5A5_5A5A);
        *slot = (((h & 0xFFFF) as i32 * 254) / 65535 - 127) as i8;
    }
    lut
});

/// Per-column lookup tables for surface effects.
///
/// Every term the per-pixel loop needs depends only on the column offset inside the
/// cropped box (or on the box origin plus a per-frame grain offset), never on the row.
/// Recomputing them for all `w * h` pixels per frame cost ~35 CPU cycles/pixel; building
/// them once per `(w, bx, grain_ox, step_uv)` keeps the arithmetic identical while
/// removing the redundant work. Bounded to one entry, so memory stays flat.
#[derive(Default)]
struct SurfaceXLut {
    key: Option<(usize, i32, i32, u32)>,
    width: usize,
    vx: Vec<f32>,
    xstep: Vec<f32>,
    txs: Vec<usize>,
}

impl SurfaceXLut {
    fn ensure(&mut self, w: usize, bx: i32, grain_ox: i32, step_uv: f32) {
        let key = (w, bx, grain_ox, step_uv.to_bits());
        if self.key == Some(key) && self.width == w {
            return;
        }
        self.key = Some(key);
        self.width = w;
        self.vx.clear();
        self.xstep.clear();
        self.txs.clear();
        self.vx.reserve(w);
        self.xstep.reserve(w);
        self.txs.reserve(w);
        let half_w = (w as f32 * 0.5).max(1.0);
        for x in 0..w {
            let nx = (x as f32 + 0.5 - half_w) / half_w;
            self.vx.push(nx * nx);
            self.xstep.push(x as f32 * step_uv);
            self.txs.push(((bx + x as i32 + grain_ox) & 255) as usize);
        }
    }
}

thread_local! {
    static SURFACE_X_LUT: std::cell::RefCell<SurfaceXLut> =
        std::cell::RefCell::new(SurfaceXLut::default());
}

/// Apply deterministic per-pixel surface effects (vignette, halftone, film grain)
/// strictly within the cropped bounding box `(bx, by, pw, ph)`.
pub(super) fn apply_surface_effects_cropped(
    pm: &mut Pixmap,
    bx: i32,
    by: i32,
    grain: f32,
    vignette: f32,
    halftone: f32,
    time_sec: f32,
) {
    let w = pm.width() as usize;
    let h = pm.height() as usize;
    if w == 0 || h == 0 || (grain <= 0.0 && vignette <= 0.0 && halftone <= 0.0) {
        return;
    }
    let vig = vignette.clamp(0.0, 1.0);
    let gr = grain.clamp(0.0, 1.0);
    let has_ht = halftone > 0.0;

    let frame_seed = (time_sec * 30.0).round() as i64 as u64;
    let frame_hash = splitmix64(frame_seed.wrapping_mul(83_492_791));
    let grain_ox = (frame_hash & 255) as i32;
    let grain_oy = ((frame_hash >> 8) & 255) as i32;
    let lut = &*GRAIN_LUT_256;

    let cell = if has_ht {
        if halftone <= 1.0 {
            (halftone * 10.0).max(3.0)
        } else {
            halftone.clamp(2.0, 32.0)
        }
    } else {
        1.0
    };
    let step_uv = 0.707_106_77 / cell;
    let ht_scale = cell * 1.5;

    SURFACE_X_LUT.with(|cell_ref| {
        let mut xlut = cell_ref.borrow_mut();
        xlut.ensure(w, bx, grain_ox, step_uv);
        let vx: &[f32] = &xlut.vx;
        let xstep: &[f32] = &xlut.xstep;
        let txs: &[usize] = &xlut.txs;
        let data = pm.data_mut();

        // Pure-integer fast path when only film grain is active.
        if vig <= 0.0 && !has_ht && gr > 0.0 {
            let scale_q8 = (gr * 48.0 * 65536.0 / (127.0 * 255.0)).round() as i32;
            for y in 0..h {
                let ty = (((by + y as i32 + grain_oy) & 255) as usize) << 8;
                let row = &mut data[y * w * 4..(y + 1) * w * 4];
                for (x, px) in row.chunks_exact_mut(4).enumerate() {
                    let a = px[3];
                    if a == 0 {
                        continue;
                    }
                    let delta = (lut[ty | txs[x]] as i32 * scale_q8 * (a as i32)) / 65536;
                    let ai = a as i32;
                    px[0] = (px[0] as i32 + delta).clamp(0, ai) as u8;
                    px[1] = (px[1] as i32 + delta).clamp(0, ai) as u8;
                    px[2] = (px[2] as i32 + delta).clamp(0, ai) as u8;
                }
            }
            return;
        }

        // Fast integer path for vignette (+ optional film grain) without halftone.
        if vig > 0.0 && !has_ht {
            let half_h = (h as f32 * 0.5).max(1.0);
            let scale_q8 = if gr > 0.0 {
                (gr * 48.0 * 65536.0 / (127.0 * 255.0)).round() as i32
            } else {
                0
            };
            let vig_k = vig * 0.375;
            for y in 0..h {
                let ny = (y as f32 + 0.5 - half_h) / half_h;
                let ny_sq = ny * ny;
                let ty = (((by + y as i32 + grain_oy) & 255) as usize) << 8;
                let row = &mut data[y * w * 4..(y + 1) * w * 4];
                for (x, px) in row.chunks_exact_mut(4).enumerate() {
                    let a = px[3];
                    if a == 0 {
                        continue;
                    }
                    let fac_q8 = ((1.0 - ((vx[x] + ny_sq) * vig_k).min(vig)).max(0.0) * 256.0) as i32;
                    let delta = if scale_q8 != 0 {
                        (lut[ty | txs[x]] as i32 * scale_q8 * (a as i32)) >> 16
                    } else {
                        0
                    };
                    let ai = a as i32;
                    px[0] = (((px[0] as i32 * fac_q8) >> 8) + delta).clamp(0, ai) as u8;
                    px[1] = (((px[1] as i32 * fac_q8) >> 8) + delta).clamp(0, ai) as u8;
                    px[2] = (((px[2] as i32 * fac_q8) >> 8) + delta).clamp(0, ai) as u8;
                }
            }
            return;
        }

        let half_h = (h as f32 * 0.5).max(1.0);
        let wx0 = bx as f32 + 0.5;
        let gr_amp = gr * (48.0 / (127.0 * 255.0));

        for y in 0..h {
            let ny = (y as f32 + 0.5 - half_h) / half_h;
            let ny_sq = ny * ny;
            let wyf = (by + y as i32) as f32 + 0.5;
            let u_row = (wx0 + wyf) * step_uv;
            let v_row = (wyf - wx0) * step_uv;
            let ty = (((by + y as i32 + grain_oy) & 255) as usize) << 8;
            let row = &mut data[y * w * 4..(y + 1) * w * 4];
            for (x, px) in row.chunks_exact_mut(4).enumerate() {
                let a = px[3];
                if a == 0 {
                    continue;
                }
                let af = a as f32;
                let mut r = px[0] as f32;
                let mut g = px[1] as f32;
                let mut b = px[2] as f32;

                if vig > 0.0 {
                    let r2 = (vx[x] + ny_sq) * 0.5;
                    let factor = (1.0 - vig * (r2 * 0.75).clamp(0.0, 1.0)).clamp(0.0, 1.0);
                    r *= factor;
                    g *= factor;
                    b *= factor;
                }

                if has_ht {
                    let u = u_row + xstep[x];
                    let v = v_row - xstep[x];
                    let fx = u - u.floor() - 0.5;
                    let fy = v - v.floor() - 0.5;
                    let dist = (fx * fx + fy * fy).sqrt() * std::f32::consts::SQRT_2;
                    let luma = ((0.299 * r + 0.587 * g + 0.114 * b) / af).clamp(0.0, 1.0);
                    let dot_r = luma.sqrt() * 0.95;
                    let cov = ((dot_r - dist) * ht_scale + 0.5).clamp(0.15, 1.0);
                    r *= cov;
                    g *= cov;
                    b *= cov;
                }

                if gr > 0.0 {
                    let delta = (lut[ty | txs[x]] as f32) * gr_amp * af;
                    r = (r + delta).clamp(0.0, af);
                    g = (g + delta).clamp(0.0, af);
                    b = (b + delta).clamp(0.0, af);
                }

                px[0] = r.round().clamp(0.0, af) as u8;
                px[1] = g.round().clamp(0.0, af) as u8;
                px[2] = b.round().clamp(0.0, af) as u8;
            }
        }
    });
}

/// Deterministic per-pixel sine displacement over the cropped foreground.
/// `ox`/`oy` are the canvas-space origin of `pm`, so the wave stays stable
/// when the crop window changes.
pub(super) fn apply_wave_warp(
    pm: &mut Pixmap,
    ox: i32,
    oy: i32,
    spec: &WaveWarpSpec,
    time_sec: f32,
) {
    if spec.amplitude.abs() < 1e-6 || spec.wavelength.abs() < 1e-6 {
        return;
    }
    let w = pm.width() as usize;
    let h = pm.height() as usize;
    if w == 0 || h == 0 {
        return;
    }
    let src = pm.data().to_vec();
    let dst = pm.data_mut();
    let k = std::f32::consts::TAU / spec.wavelength;
    let phase = spec.phase + spec.speed * time_sec;
    let row_bytes = w * 4;

    #[inline(always)]
    fn lerp_rgba(p0: &[u8], p1: &[u8], w0: u32, w1: u32, out: &mut [u8]) {
        out[0] = ((p0[0] as u32 * w0 + p1[0] as u32 * w1 + 128) >> 8) as u8;
        out[1] = ((p0[1] as u32 * w0 + p1[1] as u32 * w1 + 128) >> 8) as u8;
        out[2] = ((p0[2] as u32 * w0 + p1[2] as u32 * w1 + 128) >> 8) as u8;
        out[3] = ((p0[3] as u32 * w0 + p1[3] as u32 * w1 + 128) >> 8) as u8;
    }

    #[inline(always)]
    fn sample_1d_params(f: f32, limit: usize) -> Option<(usize, usize, u32, u32)> {
        if f < 0.0 || f >= limit as f32 {
            return None;
        }
        let i0 = f as usize;
        let i1 = (i0 + 1).min(limit - 1);
        let w1 = ((f - i0 as f32) * 256.0).round() as u32;
        let w0 = 256 - w1;
        Some((i0, i1, w0, w1))
    }

    match spec.axis {
        WaveAxisSpec::StretchX => {
            let x_lut: Vec<Option<(usize, usize, u32, u32)>> = (0..w)
                .map(|x| {
                    let wave = (k * (ox + x as i32) as f32 + phase).sin();
                    sample_1d_params(x as f32 - spec.amplitude * wave, w)
                })
                .collect();

            for y in 0..h {
                let row_off = y * row_bytes;
                let src_row = &src[row_off..row_off + row_bytes];
                let dst_row = &mut dst[row_off..row_off + row_bytes];
                if src_row.iter().all(|&b| b == 0) {
                    dst_row.fill(0);
                    continue;
                }
                for (x, entry) in x_lut.iter().enumerate() {
                    let d = &mut dst_row[x * 4..x * 4 + 4];
                    if let Some(&(x0, x1, w0, w1)) = entry.as_ref() {
                        lerp_rgba(&src_row[x0 * 4..], &src_row[x1 * 4..], w0, w1, d);
                    } else {
                        d.fill(0);
                    }
                }
            }
        }
        WaveAxisSpec::X => {
            for y in 0..h {
                let row_off = y * row_bytes;
                let src_row = &src[row_off..row_off + row_bytes];
                let dst_row = &mut dst[row_off..row_off + row_bytes];
                if src_row.iter().all(|&b| b == 0) {
                    dst_row.fill(0);
                    continue;
                }
                let shift = spec.amplitude * (k * (oy + y as i32) as f32 + phase).sin();
                for x in 0..w {
                    let d = &mut dst_row[x * 4..x * 4 + 4];
                    if let Some((x0, x1, w0, w1)) = sample_1d_params(x as f32 - shift, w) {
                        lerp_rgba(&src_row[x0 * 4..], &src_row[x1 * 4..], w0, w1, d);
                    } else {
                        d.fill(0);
                    }
                }
            }
        }
        WaveAxisSpec::StretchY => {
            let y_lut: Vec<Option<(usize, usize, u32, u32)>> = (0..h)
                .map(|y| {
                    let wave = (k * (oy + y as i32) as f32 + phase).sin();
                    sample_1d_params(y as f32 - spec.amplitude * wave, h)
                })
                .collect();

            for (y, entry) in y_lut.iter().enumerate() {
                let dst_row = &mut dst[y * row_bytes..(y + 1) * row_bytes];
                if let Some(&(y0, y1, w0, w1)) = entry.as_ref() {
                    let r0 = &src[y0 * row_bytes..(y0 + 1) * row_bytes];
                    let r1 = &src[y1 * row_bytes..(y1 + 1) * row_bytes];
                    for x in 0..w {
                        let o = x * 4;
                        lerp_rgba(&r0[o..], &r1[o..], w0, w1, &mut dst_row[o..o + 4]);
                    }
                } else {
                    dst_row.fill(0);
                }
            }
        }
        WaveAxisSpec::Y => {
            let shifts: Vec<f32> = (0..w)
                .map(|x| spec.amplitude * (k * (ox + x as i32) as f32 + phase).sin())
                .collect();

            for y in 0..h {
                let dst_row = &mut dst[y * row_bytes..(y + 1) * row_bytes];
                let yf = y as f32;
                for (x, &shift) in shifts.iter().enumerate() {
                    let o = x * 4;
                    let d = &mut dst_row[o..o + 4];
                    if let Some((y0, y1, w0, w1)) = sample_1d_params(yf - shift, h) {
                        lerp_rgba(
                            &src[y0 * row_bytes + o..],
                            &src[y1 * row_bytes + o..],
                            w0,
                            w1,
                            d,
                        );
                    } else {
                        d.fill(0);
                    }
                }
            }
        }
    }
}

/// Perform exact analytical ray-plane inverse homography warp for 2.5D `scale`, `rotate_x`,
/// `rotate_y`, and `perspective`, operating strictly on the tight cropped `src` bounding box.
/// Returns `Ok(None)` when culled (backface invisible or outside bounds).
#[allow(clippy::too_many_arguments)]
pub(super) fn warp_perspective_3d_cropped(
    src: &Pixmap,
    bx: i32,
    by: i32,
    pivot_x: f32,
    pivot_y: f32,
    scale: f32,
    rotate_x_deg: f32,
    rotate_y_deg: f32,
    perspective: f32,
    backface_visible: bool,
    canvas_w: u32,
    canvas_h: u32,
) -> Result<Option<(Pixmap, i32, i32)>> {
    if scale <= 0.0 {
        return Ok(None);
    }
    let rx = rotate_x_deg.to_radians();
    let ry = rotate_y_deg.to_radians();
    let d = if perspective > 1.0 { perspective } else { 800.0 };

    let (srx, crx) = rx.sin_cos();
    let (sry, cry) = ry.sin_cos();

    let eu = (cry, 0.0, -sry);
    let ev = (srx * sry, crx, srx * cry);
    let n = (crx * sry, -srx, crx * cry);

    if !backface_visible && n.2 <= 1e-5 {
        return Ok(None);
    }
    if n.2.abs() <= 1e-5 {
        return Ok(None);
    }

    let sw = src.width() as i32;
    let sh = src.height() as i32;
    let x0 = bx as f32;
    let y0 = by as f32;
    let x1 = (bx + sw) as f32;
    let y1 = (by + sh) as f32;
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];

    let mut min_px = f32::INFINITY;
    let mut min_py = f32::INFINITY;
    let mut max_px = f32::NEG_INFINITY;
    let mut max_py = f32::NEG_INFINITY;

    for (cx_corner, cy_corner) in corners {
        let u = (cx_corner - pivot_x) * scale;
        let v = (cy_corner - pivot_y) * scale;
        let x3 = u * eu.0 + v * ev.0;
        let y3 = u * eu.1 + v * ev.1;
        let z3 = u * eu.2 + v * ev.2;
        let denom = (d + z3).max(d * 0.08);
        let px = pivot_x + d * x3 / denom;
        let py = pivot_y + d * y3 / denom;
        min_px = min_px.min(px);
        min_py = min_py.min(py);
        max_px = max_px.max(px);
        max_py = max_py.max(py);
    }

    let cw_i = canvas_w as i32;
    let ch_i = canvas_h as i32;
    let dst_x0 = (min_px.floor() as i32 - 2).clamp(-cw_i, cw_i * 2);
    let dst_y0 = (min_py.floor() as i32 - 2).clamp(-ch_i, ch_i * 2);
    let dst_x1 = (max_px.ceil() as i32 + 2).clamp(-cw_i, cw_i * 2);
    let dst_y1 = (max_py.ceil() as i32 + 2).clamp(-ch_i, ch_i * 2);

    if dst_x1 <= dst_x0 || dst_y1 <= dst_y0 {
        return Ok(None);
    }

    let dst_w = (dst_x1 - dst_x0) as u32;
    let dst_h = (dst_y1 - dst_y0) as u32;
    let mut out = Pixmap::new(dst_w, dst_h)
        .ok_or_else(|| anyhow!("Failed to allocate 2.5D warped pixmap {}x{}", dst_w, dst_h))?;

    let src_data = src.data();
    let dst_data = out.data_mut();
    let sw_i = sw;
    let sh_i = sh;
    let sw_u = sw as usize;
    let dw_u = dst_w as usize;
    let ox_in_src = pivot_x - bx as f32;
    let oy_in_src = pivot_y - by as f32;

    let inv_s = 1.0 / scale.max(1e-4);
    let dn2 = d * n.2;
    let eu0_s = eu.0 * inv_s;
    let eu1_s = eu.1 * inv_s;
    let eu2_sd = eu.2 * d * inv_s;
    let ev0_s = ev.0 * inv_s;
    let ev1_s = ev.1 * inv_s;
    let ev2_sd = ev.2 * d * inv_s;
    let const_u = ox_in_src - 0.5 - eu2_sd;
    let const_v = oy_in_src - 0.5 - ev2_sd;
    let sx_scr0 = dst_x0 as f32 + 0.5 - pivot_x;

    let sample_src_u32 = |ix: i32, iy: i32| -> [u32; 4] {
        if ix < 0 || iy < 0 || ix >= sw_i || iy >= sh_i {
            [0; 4]
        } else {
            let idx = ((iy as usize) * sw_u + (ix as usize)) * 4;
            [
                src_data[idx] as u32,
                src_data[idx + 1] as u32,
                src_data[idx + 2] as u32,
                src_data[idx + 3] as u32,
            ]
        }
    };

    #[inline(always)]
    fn bilinear_weights(fx: f32, fy: f32, x_lo: i32, y_lo: i32) -> (u32, u32, u32, u32) {
        let wx_i = (((fx - x_lo as f32) * 256.0) as i32).clamp(0, 255) as u32;
        let wy_i = (((fy - y_lo as f32) * 256.0) as i32).clamp(0, 255) as u32;
        let wx0 = 256 - wx_i;
        let wy0 = 256 - wy_i;
        (wx0 * wy0, wx_i * wy0, wx0 * wy_i, wx_i * wy_i)
    }

    let step_denom = n.0;
    let step_u = eu0_s;
    let step_v = ev0_s;

    for dy in 0..(dst_h as i32) {
        let sy_scr = (dst_y0 + dy) as f32 + 0.5 - pivot_y;
        let base_denom = sy_scr * n.1 + dn2;
        let base_u = sy_scr * eu1_s + eu2_sd;
        let base_v = sy_scr * ev1_s + ev2_sd;
        let row_out_base = (dy as usize) * dw_u * 4;

        let mut denom = sx_scr0 * n.0 + base_denom;
        let mut num_u = sx_scr0 * eu0_s + base_u;
        let mut num_v = sx_scr0 * ev0_s + base_v;

        for dx in 0..(dst_w as i32) {
            let cur_denom = denom;
            let cur_u = num_u;
            let cur_v = num_v;
            denom += step_denom;
            num_u += step_u;
            num_v += step_v;

            if cur_denom.abs() <= 1e-5 {
                continue;
            }
            let t = dn2 / cur_denom;
            if t <= 0.05 {
                continue;
            }
            let fx = t * cur_u + const_u;
            let fy = t * cur_v + const_v;
            if fx < -1.0 || fy < -1.0 || fx >= sw_i as f32 || fy >= sh_i as f32 {
                continue;
            }

            let x_lo = ((fx + 2.0) as i32) - 2;
            let y_lo = ((fy + 2.0) as i32) - 2;
            let out_idx = row_out_base + (dx as usize) * 4;

            if x_lo >= 0 && x_lo + 1 < sw_i && y_lo >= 0 && y_lo + 1 < sh_i {
                let r0 = ((y_lo as usize) * sw_u + (x_lo as usize)) * 4;
                let r1 = r0 + sw_u * 4;
                let p00 = u32::from_ne_bytes(src_data[r0..r0 + 4].try_into().unwrap());
                let p10 = u32::from_ne_bytes(src_data[r0 + 4..r0 + 8].try_into().unwrap());
                let p01 = u32::from_ne_bytes(src_data[r1..r1 + 4].try_into().unwrap());
                let p11 = u32::from_ne_bytes(src_data[r1 + 4..r1 + 8].try_into().unwrap());
                if (p00 | p10 | p01 | p11) == 0 {
                    continue;
                }
                if p00 == p10 && p00 == p01 && p00 == p11 {
                    dst_data[out_idx..out_idx + 4].copy_from_slice(&p00.to_ne_bytes());
                    continue;
                }
                let (w00, w10, w01, w11) = bilinear_weights(fx, fy, x_lo, y_lo);
                for ch in 0..4 {
                    let v = src_data[r0 + ch] as u32 * w00
                        + src_data[r0 + 4 + ch] as u32 * w10
                        + src_data[r1 + ch] as u32 * w01
                        + src_data[r1 + 4 + ch] as u32 * w11;
                    dst_data[out_idx + ch] = ((v + 32768) >> 16) as u8;
                }
            } else {
                let (w00, w10, w01, w11) = bilinear_weights(fx, fy, x_lo, y_lo);
                let c00 = sample_src_u32(x_lo, y_lo);
                let c10 = sample_src_u32(x_lo + 1, y_lo);
                let c01 = sample_src_u32(x_lo, y_lo + 1);
                let c11 = sample_src_u32(x_lo + 1, y_lo + 1);
                for ch in 0..4 {
                    let v = c00[ch] * w00 + c10[ch] * w10 + c01[ch] * w01 + c11[ch] * w11;
                    dst_data[out_idx + ch] = ((v + 32768) >> 16) as u8;
                }
            }
        }
    }

    Ok(Some((out, dst_x0, dst_y0)))
}

pub(super) fn layer_base_scale(kind: &LayerKind) -> f32 {
    match kind {
        LayerKind::Rect { scale, .. }
        | LayerKind::Circle { scale, .. }
        | LayerKind::Text { scale, .. }
        | LayerKind::Image { scale, .. }
        | LayerKind::Video { scale, .. }
        | LayerKind::Waveform { scale, .. }
        | LayerKind::Lottie { scale, .. }
        | LayerKind::Particles { scale, .. }
        | LayerKind::Container { scale, .. } => *scale,
        #[cfg(feature = "exp-3d")]
        LayerKind::Mesh3d { scale, .. } => *scale,
        LayerKind::Svg { .. } => 1.0,
    }
}

pub(super) fn set_layer_base_scale(kind: &mut LayerKind, val: f32) {
    match kind {
        LayerKind::Rect { scale, .. }
        | LayerKind::Circle { scale, .. }
        | LayerKind::Text { scale, .. }
        | LayerKind::Image { scale, .. }
        | LayerKind::Video { scale, .. }
        | LayerKind::Waveform { scale, .. }
        | LayerKind::Lottie { scale, .. }
        | LayerKind::Particles { scale, .. }
        | LayerKind::Container { scale, .. } => *scale = val,
        #[cfg(feature = "exp-3d")]
        LayerKind::Mesh3d { scale, .. } => *scale = val,
        LayerKind::Svg { .. } => {}
    }
}

pub(super) fn layer_pose_offset_and_scale(
    layer: &Layer,
    layout_override: Option<LayoutBox>,
    time_sec: f32,
) -> (f32, f32, f32) {
    let (base_x, base_y) = match &layer.kind {
        LayerKind::Circle { cx, cy, .. } => (*cx, *cy),
        LayerKind::Rect { x, y, .. }
        | LayerKind::Text { x, y, .. }
        | LayerKind::Image { x, y, .. }
        | LayerKind::Svg { x, y, .. }
        | LayerKind::Video { x, y, .. }
        | LayerKind::Waveform { x, y, .. }
        | LayerKind::Lottie { x, y, .. }
        | LayerKind::Particles { x, y, .. }
        | LayerKind::Container { x, y, .. } => (*x, *y),
        #[cfg(feature = "exp-3d")]
        LayerKind::Mesh3d { x, y, .. } => (*x, *y),
    };
    let (cur_x, cur_y) = resolve_xy(layer, layout_override, base_x, base_y, time_sec);
    let (ref_x, ref_y) = if layer.position == PositionSpec::Absolute {
        (
            layout_override.map(|b| b.parent_x).unwrap_or(0.0) + base_x,
            layout_override.map(|b| b.parent_y).unwrap_or(0.0) + base_y,
        )
    } else if let Some(b) = layout_override {
        (b.x, b.y)
    } else {
        (base_x, base_y)
    };
    let eff_scale = eval_property(
        &layer.animations,
        AnimProperty::Scale,
        time_sec,
        layer_base_scale(&layer.kind),
    );
    (cur_x - ref_x, cur_y - ref_y, eff_scale)
}

pub(super) fn layer_base_opacity(kind: &LayerKind) -> f32 {
    match kind {
        LayerKind::Rect { opacity, .. }
        | LayerKind::Circle { opacity, .. }
        | LayerKind::Text { opacity, .. }
        | LayerKind::Image { opacity, .. }
        | LayerKind::Svg { opacity, .. }
        | LayerKind::Video { opacity, .. }
        | LayerKind::Waveform { opacity, .. }
        | LayerKind::Lottie { opacity, .. }
        | LayerKind::Particles { opacity, .. }
        | LayerKind::Container { opacity, .. } => *opacity,
        #[cfg(feature = "exp-3d")]
        LayerKind::Mesh3d { opacity, .. } => *opacity,
    }
}

pub(super) fn set_layer_opacity(kind: &mut LayerKind, val: f32) {
    match kind {
        LayerKind::Rect { opacity, .. }
        | LayerKind::Circle { opacity, .. }
        | LayerKind::Text { opacity, .. }
        | LayerKind::Image { opacity, .. }
        | LayerKind::Svg { opacity, .. }
        | LayerKind::Video { opacity, .. }
        | LayerKind::Waveform { opacity, .. }
        | LayerKind::Lottie { opacity, .. }
        | LayerKind::Particles { opacity, .. }
        | LayerKind::Container { opacity, .. } => *opacity = val,
        #[cfg(feature = "exp-3d")]
        LayerKind::Mesh3d { opacity, .. } => *opacity = val,
    }
}

pub(super) fn is_extrinsic_pose_prop(prop: AnimProperty) -> bool {
    matches!(
        prop,
        AnimProperty::Opacity
            | AnimProperty::X
            | AnimProperty::Y
            | AnimProperty::Scale
            | AnimProperty::RotateX
            | AnimProperty::RotateY
            | AnimProperty::RotateZ
            | AnimProperty::Perspective
            | AnimProperty::ZIndex
    )
}

fn layer_kind_fx_static(kind: &LayerKind, time_sec: f32) -> bool {
    match kind {
        LayerKind::Video { .. }
        | LayerKind::Waveform { .. }
        | LayerKind::Lottie { .. }
        | LayerKind::Particles { .. } => false,
        LayerKind::Text {
            dynamic,
            text_animator,
            ..
        } => {
            if text_animator.is_some() {
                false
            } else {
                match dynamic {
                    None => true,
                    Some(DynamicTextSpec::Typewriter {
                        end_sec: Some(e), ..
                    })
                    | Some(DynamicTextSpec::Counter {
                        end_sec: Some(e), ..
                    }) => time_sec >= *e,
                    _ => false,
                }
            }
        }
        LayerKind::Svg {
            path_progress,
            stroke_dashoffset,
            stroke_dasharray,
            ..
        } => *path_progress >= 1.0 && stroke_dashoffset.is_none() && stroke_dasharray.is_none(),
        LayerKind::Container {
            children, timeline, ..
        } => {
            if *timeline != crate::schema::TimelineMode::Global {
                return false;
            }
            children.iter().all(|c| {
                c.start_sec <= time_sec
                    && c.end_sec.is_none()
                    && c.duration_secs.is_none()
                    && is_layer_fx_geometry_static(c, time_sec)
            })
        }
        _ => true,
    }
}

fn is_layer_fx_geometry_static(layer: &Layer, time_sec: f32) -> bool {
    if layer.grain > 0.0 {
        return false;
    }
    if !layer.animations.iter().all(|a| {
        a.expr.is_none()
            && a.repeat.is_none()
            && a.keyframes.is_empty()
            && time_sec >= a.end_sec
    }) {
        return false;
    }
    layer_kind_fx_static(&layer.kind, time_sec)
}

pub(super) fn finished_cursor_blink_signature(layer: &Layer, time_sec: f32) -> u64 {
    match &layer.kind {
        LayerKind::Text {
            dynamic:
                Some(DynamicTextSpec::Typewriter {
                    end_sec: Some(e),
                    cursor: Some(cur),
                    ..
                }),
            ..
        } if time_sec >= *e => {
            let blink_on = ((time_sec * 2.0) as u32).is_multiple_of(2);
            let mut h = if blink_on {
                0x9E37_79B9_7F4A_7C15u64
            } else {
                0xC2B2_AE3D_27D4_EB4Fu64
            };
            for &b in cur.as_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        }
        LayerKind::Container { children, .. } => {
            let mut acc = 0u64;
            for (idx, child) in children.iter().enumerate() {
                let ch = finished_cursor_blink_signature(child, time_sec);
                if ch != 0 {
                    acc ^= ch.rotate_left((idx as u32 % 31) + 1);
                }
            }
            acc
        }
        _ => 0,
    }
}

pub(super) fn can_use_blur_cache(
    layer: &Layer,
    _layout_override: Option<LayoutBox>,
    _eff_op: f32,
    time_sec: f32,
) -> bool {
    if layer.wave_warp.is_some() {
        return false;
    }
    let has_fx = layer.blur > 0.0
        || layer.shadow.is_some()
        || layer.vignette > 0.0
        || layer.halftone > 0.0
        || layer.rotate_x != 0.0
        || layer.rotate_y != 0.0
        || layer
            .animations
            .iter()
            .any(|a| matches!(a.property, AnimProperty::RotateX | AnimProperty::RotateY));
    if !has_fx || layer.grain > 0.0 {
        return false;
    }
    if layer.blend_mode != BlendModeSpec::Normal && layer.shadow.is_some() {
        return false;
    }
    if !layer.animations.iter().all(|a| {
        is_extrinsic_pose_prop(a.property)
            || (a.expr.is_none()
                && a.repeat.is_none()
                && a.keyframes.is_empty()
                && time_sec >= a.end_sec)
    }) {
        return false;
    }
    layer_kind_fx_static(&layer.kind, time_sec)
}

/// Composite an optional cropped shadow pixmap beneath a cropped foreground pixmap into one tight pixmap.
#[allow(clippy::too_many_arguments)]
pub(super) fn combine_shadow_and_foreground(
    shadow: Option<(&Pixmap, f32, f32)>,
    fg: Pixmap,
    fx: i32,
    fy: i32,
) -> Result<(Pixmap, i32, i32)> {
    let Some((sh_pm, sx, sy)) = shadow else {
        return Ok((fg, fx, fy));
    };
    let sw = sh_pm.width() as f32;
    let sh_h = sh_pm.height() as f32;
    let fw = fg.width() as i32;
    let fh = fg.height() as i32;

    let min_x = (sx.floor() as i32).min(fx);
    let min_y = (sy.floor() as i32).min(fy);
    let max_x = ((sx + sw).ceil() as i32).max(fx + fw);
    let max_y = ((sy + sh_h).ceil() as i32).max(fy + fh);

    let cw = (max_x - min_x).max(1) as u32;
    let ch = (max_y - min_y).max(1) as u32;
    let mut combined = Pixmap::new(cw, ch)
        .ok_or_else(|| anyhow!("Failed to allocate combined FX pixmap {}x{}", cw, ch))?;

    let paint = tiny_skia::PixmapPaint::default();
    combined.draw_pixmap(
        0,
        0,
        sh_pm.as_ref(),
        &paint,
        tiny_skia::Transform::from_translate(sx - min_x as f32, sy - min_y as f32),
        None,
    );
    combined.draw_pixmap(
        0,
        0,
        fg.as_ref(),
        &paint,
        tiny_skia::Transform::from_translate((fx - min_x) as f32, (fy - min_y) as f32),
        None,
    );
    Ok((combined, min_x, min_y))
}
