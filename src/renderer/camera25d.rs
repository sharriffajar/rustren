use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Result, anyhow};
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use crate::animation::eval_property;
use crate::schema::{AnimProperty, CameraPreset, Layer, LayerKind, WorldCameraSpec};

/// Evaluated scalar 2.5D camera state at a given timestamp.
#[derive(Debug, Clone, Copy)]
pub struct CameraState {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub roll_deg: f32,
    pub focus_z: Option<f32>,
    pub dof_strength: f32,
    pub motion_blur: f32,
}

impl WorldCameraSpec {
    /// Evaluate camera state at `time_sec`, combining base values, optional
    /// high-level intent preset, and explicit keyframed `animations`.
    pub fn eval_at(&self, time_sec: f32, duration_secs: f32) -> CameraState {
        let p = if duration_secs > 1e-4 {
            (time_sec / duration_secs).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Smoothstep for gentle cinematic motion
        let smooth = p * p * (3.0 - 2.0 * p);

        let (mut px, py, mut pz, proll, mut pfocus) =
            (self.x, self.y, self.z, self.roll, self.focus_z);

        if let Some(preset) = self.preset {
            match preset {
                CameraPreset::PushIn => {
                    pz += smooth * 0.18;
                }
                CameraPreset::PullOut => {
                    pz += (1.0 - smooth) * 0.18;
                }
                CameraPreset::TruckLeft => {
                    px -= smooth * 96.0;
                }
                CameraPreset::TruckRight => {
                    px += smooth * 96.0;
                }
                CameraPreset::WhipLeft => {
                    // Sharp S-curve whip pan across 360px
                    let whip = p * p * p * (p * (p * 6.0 - 15.0) + 10.0);
                    px -= whip * 360.0;
                }
                CameraPreset::WhipRight => {
                    let whip = p * p * p * (p * (p * 6.0 - 15.0) + 10.0);
                    px += whip * 360.0;
                }
                CameraPreset::RackFocus => {
                    // Shift focus smoothly from foreground (0.65) to far (-0.5)
                    let fz = 0.65 + (-0.5 - 0.65) * smooth;
                    pfocus = Some(fz);
                }
                CameraPreset::PunchCut => {
                    // Instant punch-in at midpoint
                    if p >= 0.5 {
                        pz += 0.22;
                    }
                }
            }
        }

        let x = eval_property(&self.animations, AnimProperty::X, time_sec, px);
        let y = eval_property(&self.animations, AnimProperty::Y, time_sec, py);
        let z = eval_property(&self.animations, AnimProperty::ZIndex, time_sec, pz);
        let roll_deg = eval_property(&self.animations, AnimProperty::Rotation, time_sec, proll);

        CameraState {
            x,
            y,
            z,
            roll_deg,
            focus_z: pfocus,
            dof_strength: self.dof_strength.max(0.0),
            motion_blur: self.motion_blur.max(0.0),
        }
    }
}

/// Compute the 2.5D affine camera transform for a world-space plane at `layer_z`.
///
/// Normalized so that at rest (`cam.x = 0, cam.y = 0, cam.z = 0, cam.roll = 0`),
/// every plane has scale `1.0` (auto-scale compensation) so framing math never breaks.
pub fn compute_plane_camera_transform(
    cam: CameraState,
    layer_z: f32,
    canvas_w: f32,
    canvas_h: f32,
) -> Transform {
    let depth_factor = (1.0 + layer_z * 0.6).clamp(0.25, 2.2);
    let zoom = (1.0 + cam.z * depth_factor).clamp(0.2, 4.0);
    let dx = -cam.x * depth_factor;
    let dy = -cam.y * depth_factor;
    let cx = canvas_w * 0.5;
    let cy = canvas_h * 0.5;

    Transform::from_translate(-cx, -cy)
        .post_concat(Transform::from_scale(zoom, zoom))
        .post_concat(Transform::from_rotate(cam.roll_deg))
        .post_concat(Transform::from_translate(cx + dx, cy + dy))
}

/// Cache of pre-baked 1/4-resolution blurred twins for O(1) rack focus on static image planes.
static RACK_FOCUS_BLUR_CACHE: OnceLock<Mutex<HashMap<String, Arc<Pixmap>>>> = OnceLock::new();

thread_local! {
    /// Per-thread SIMD resizer for fast 1/4-res downsampling.
    static CAM25D_RESIZER: RefCell<Resizer> = RefCell::new(Resizer::new());
    /// Per-thread reusable 1/4-res blur scratch Pixmap for dynamic world layers.
    static CAM25D_QUARTER_SCRATCH: RefCell<Option<Pixmap>> = const { RefCell::new(None) };
}

fn downsample_and_blur_quarter_into(src: &Pixmap, small: &mut Pixmap) {
    let qw = small.width();
    let qh = small.height();
    small.data_mut().fill(0);
    let ok = CAM25D_RESIZER.with(|cell| {
        let mut resizer = cell.borrow_mut();
        if let Ok(src_img) = ImageRef::new(src.width(), src.height(), src.data(), PixelType::U8x4)
            && let Ok(mut dst_img) = Image::from_slice_u8(qw, qh, small.data_mut(), PixelType::U8x4)
        {
            let opts = ResizeOptions::new()
                .resize_alg(ResizeAlg::Convolution(FilterType::Bilinear))
                .use_alpha(false);
            return resizer.resize(&src_img, &mut dst_img, Some(&opts)).is_ok();
        }
        false
    });
    if !ok {
        let sx = qw as f32 / src.width() as f32;
        let sy = qh as f32 / src.height() as f32;
        let paint = PixmapPaint {
            opacity: 1.0,
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        };
        small.draw_pixmap(0, 0, src.as_ref(), &paint, Transform::from_scale(sx, sy), None);
    }
    super::fx::fast_blur_rgba(small.data_mut(), qw, qh, 3.0);
}

fn downsample_and_blur_quarter(src: &Pixmap) -> Option<Pixmap> {
    let qw = (src.width() / 4).max(1);
    let qh = (src.height() / 4).max(1);
    let mut small = Pixmap::new(qw, qh)?;
    downsample_and_blur_quarter_into(src, &mut small);
    Some(small)
}

/// Fast exact division by 255 for u32 in `0..=65535`.
#[inline(always)]
fn div255(v: u32) -> u32 {
    ((v + 128) * 257) >> 16
}

/// Sample premultiplied RGBA from `src` at fixed-point 16.16 `(u_fp, v_fp)` with bilinear filtering.
/// Returns `[r, g, b, a]` scaled by `op_q8` (`0..=256`).
#[inline(always)]
fn sample_bilinear_fp(
    src_data: &[u8],
    sw: usize,
    sw_m1: usize,
    sh_m1: usize,
    u_fp: i32,
    v_fp: i32,
) -> [u32; 4] {
    let sx0 = (u_fp >> 16).clamp(0, sw_m1 as i32) as usize;
    let sy0 = (v_fp >> 16).clamp(0, sh_m1 as i32) as usize;
    let sx1 = (sx0 + 1).min(sw_m1);
    let sy1 = (sy0 + 1).min(sh_m1);

    let i00 = (sy0 * sw + sx0) * 4;
    let i10 = (sy0 * sw + sx1) * 4;
    let i01 = (sy1 * sw + sx0) * 4;
    let i11 = (sy1 * sw + sx1) * 4;

    if (src_data[i00 + 3] | src_data[i10 + 3] | src_data[i01 + 3] | src_data[i11 + 3]) == 0 {
        return [0, 0, 0, 0];
    }

    let fx = ((u_fp >> 8) & 0xFF) as u32;
    let fy = ((v_fp >> 8) & 0xFF) as u32;
    let inv_fx = 256 - fx;
    let inv_fy = 256 - fy;
    let w00 = inv_fx * inv_fy;
    let w10 = fx * inv_fy;
    let w01 = inv_fx * fy;
    let w11 = fx * fy;

    [
        (src_data[i00] as u32 * w00
            + src_data[i10] as u32 * w10
            + src_data[i01] as u32 * w01
            + src_data[i11] as u32 * w11
            + 32768)
            >> 16,
        (src_data[i00 + 1] as u32 * w00
            + src_data[i10 + 1] as u32 * w10
            + src_data[i01 + 1] as u32 * w01
            + src_data[i11 + 1] as u32 * w11
            + 32768)
            >> 16,
        (src_data[i00 + 2] as u32 * w00
            + src_data[i10 + 2] as u32 * w10
            + src_data[i01 + 2] as u32 * w01
            + src_data[i11 + 2] as u32 * w11
            + 32768)
            >> 16,
        (src_data[i00 + 3] as u32 * w00
            + src_data[i10 + 3] as u32 * w10
            + src_data[i01 + 3] as u32 * w01
            + src_data[i11 + 3] as u32 * w11
            + 32768)
            >> 16,
    ]
}

#[inline(always)]
fn blend_src_over_premul(dst: &mut [u8], out_i: usize, sr: u32, sg: u32, sb: u32, sa: u32) {
    if sa == 0 {
        return;
    }
    if sa >= 255 {
        dst[out_i] = sr.min(255) as u8;
        dst[out_i + 1] = sg.min(255) as u8;
        dst[out_i + 2] = sb.min(255) as u8;
        dst[out_i + 3] = 255;
        return;
    }
    let inv_a = 255 - sa;
    dst[out_i] = (sr + div255(dst[out_i] as u32 * inv_a)).min(255) as u8;
    dst[out_i + 1] = (sg + div255(dst[out_i + 1] as u32 * inv_a)).min(255) as u8;
    dst[out_i + 2] = (sb + div255(dst[out_i + 2] as u32 * inv_a)).min(255) as u8;
    dst[out_i + 3] = (sa + div255(dst[out_i + 3] as u32 * inv_a)).min(255) as u8;
}

/// Compute screen-space bounding box of `bounds` projected through `cam_tf`, clamped to `dst`.
fn project_bounds_to_dst(
    bounds: (usize, usize, usize, usize),
    cam_tf: Transform,
    dst_w: usize,
    dst_h: usize,
    pad: i32,
) -> Option<(usize, usize, usize, usize)> {
    let (bx0, by0, bx1, by1) = bounds;
    let corners = [
        (bx0 as f32, by0 as f32),
        ((bx1 + 1) as f32, by0 as f32),
        (bx0 as f32, (by1 + 1) as f32),
        ((bx1 + 1) as f32, (by1 + 1) as f32),
    ];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (x, y) in corners {
        let tx = cam_tf.sx * x + cam_tf.kx * y + cam_tf.tx;
        let ty = cam_tf.ky * x + cam_tf.sy * y + cam_tf.ty;
        min_x = min_x.min(tx);
        min_y = min_y.min(ty);
        max_x = max_x.max(tx);
        max_y = max_y.max(ty);
    }
    let dx0 = (min_x.floor() as i32 - pad).clamp(0, dst_w as i32) as usize;
    let dy0 = (min_y.floor() as i32 - pad).clamp(0, dst_h as i32) as usize;
    let dx1 = (max_x.ceil() as i32 + pad).clamp(0, dst_w as i32) as usize;
    let dy1 = (max_y.ceil() as i32 + pad).clamp(0, dst_h as i32) as usize;
    if dx1 <= dx0 || dy1 <= dy0 {
        None
    } else {
        Some((dx0, dy0, dx1, dy1))
    }
}

/// Fast fixed-point 16.16 inverse-affine bilinear composite for any camera transform (including roll),
/// restricted to the projected dirty rect of non-transparent pixels.
/// Optionally fuses sharp + 1/4-res blurred twin in a single read/write pass over `dst`.
#[allow(clippy::too_many_arguments)]
fn composite_affine_fused(
    dst: &mut Pixmap,
    sharp: Option<(&Pixmap, f32)>,
    blurred: Option<(&Pixmap, f32)>,
    bounds: (usize, usize, usize, usize),
    cam_tf: Transform,
    layer_w: usize,
    layer_h: usize,
) -> bool {
    let Some(inv) = cam_tf.invert() else {
        return false;
    };
    let dst_w = dst.width() as usize;
    let dst_h = dst.height() as usize;
    // Extra padding when blurred twin is present so Gaussian blur halo isn't clipped
    let pad = if blurred.is_some() { 16 } else { 2 };
    let Some((dx0, dy0, dx1, dy1)) = project_bounds_to_dst(bounds, cam_tf, dst_w, dst_h, pad)
    else {
        return true;
    };

    const FP: f32 = 65536.0;
    let du_dx = (inv.sx * FP).round() as i32;
    let dv_dx = (inv.ky * FP).round() as i32;
    let max_u_fp = ((layer_w as i32) << 16) - 1;
    let max_v_fp = ((layer_h as i32) << 16) - 1;
    let dst_data = dst.data_mut();

    match (sharp, blurred) {
        (Some((sp, s_op)), None) => {
            let s_data = sp.data();
            let sw = sp.width() as usize;
            let sw_m1 = sw.saturating_sub(1);
            let sh_m1 = (sp.height() as usize).saturating_sub(1);
            let op_q8 = (s_op.clamp(0.0, 1.0) * 256.0 + 0.5) as u32;
            let full_op = op_q8 >= 256;

            for y in dy0..dy1 {
                let yf = y as f32 + 0.5;
                let xf = dx0 as f32 + 0.5;
                let mut u_fp = ((inv.sx * xf + inv.kx * yf + inv.tx - 0.5) * FP) as i32;
                let mut v_fp = ((inv.ky * xf + inv.sy * yf + inv.ty - 0.5) * FP) as i32;
                let row_off = y * dst_w * 4;

                for x in dx0..dx1 {
                    if u_fp >= 0 && u_fp <= max_u_fp && v_fp >= 0 && v_fp <= max_v_fp {
                        let [r, g, b, a] = sample_bilinear_fp(s_data, sw, sw_m1, sh_m1, u_fp, v_fp);
                        if a != 0 {
                            let out_i = row_off + x * 4;
                            if full_op {
                                blend_src_over_premul(dst_data, out_i, r, g, b, a);
                            } else {
                                blend_src_over_premul(
                                    dst_data,
                                    out_i,
                                    (r * op_q8 + 128) >> 8,
                                    (g * op_q8 + 128) >> 8,
                                    (b * op_q8 + 128) >> 8,
                                    (a * op_q8 + 128) >> 8,
                                );
                            }
                        }
                    }
                    u_fp = u_fp.wrapping_add(du_dx);
                    v_fp = v_fp.wrapping_add(dv_dx);
                }
            }
            true
        }
        (None, Some((bp, b_op))) => {
            let b_data = bp.data();
            let bw = bp.width() as usize;
            let bh = bp.height() as usize;
            let bw_m1 = bw.saturating_sub(1);
            let bh_m1 = bh.saturating_sub(1);
            let bx_scale = bw as f32 / layer_w.max(1) as f32;
            let by_scale = bh as f32 / layer_h.max(1) as f32;
            let b_du_dx = (inv.sx * bx_scale * FP).round() as i32;
            let b_dv_dx = (inv.ky * by_scale * FP).round() as i32;
            let max_bu_fp = ((bw as i32) << 16) - 1;
            let max_bv_fp = ((bh as i32) << 16) - 1;
            let op_q8 = (b_op.clamp(0.0, 1.0) * 256.0 + 0.5) as u32;
            let full_op = op_q8 >= 256;

            for y in dy0..dy1 {
                let yf = y as f32 + 0.5;
                let xf = dx0 as f32 + 0.5;
                let sx_f = inv.sx * xf + inv.kx * yf + inv.tx;
                let sy_f = inv.ky * xf + inv.sy * yf + inv.ty;
                let mut bu_fp = ((sx_f * bx_scale - 0.5) * FP) as i32;
                let mut bv_fp = ((sy_f * by_scale - 0.5) * FP) as i32;
                let row_off = y * dst_w * 4;

                for x in dx0..dx1 {
                    if bu_fp >= 0 && bu_fp <= max_bu_fp && bv_fp >= 0 && bv_fp <= max_bv_fp {
                        let [r, g, b, a] =
                            sample_bilinear_fp(b_data, bw, bw_m1, bh_m1, bu_fp, bv_fp);
                        if a != 0 {
                            let out_i = row_off + x * 4;
                            if full_op {
                                blend_src_over_premul(dst_data, out_i, r, g, b, a);
                            } else {
                                blend_src_over_premul(
                                    dst_data,
                                    out_i,
                                    (r * op_q8 + 128) >> 8,
                                    (g * op_q8 + 128) >> 8,
                                    (b * op_q8 + 128) >> 8,
                                    (a * op_q8 + 128) >> 8,
                                );
                            }
                        }
                    }
                    bu_fp = bu_fp.wrapping_add(b_du_dx);
                    bv_fp = bv_fp.wrapping_add(b_dv_dx);
                }
            }
            true
        }
        (Some((sp, s_op)), Some((bp, b_op))) => {
            let s_data = sp.data();
            let sw = sp.width() as usize;
            let sw_m1 = sw.saturating_sub(1);
            let sh_m1 = (sp.height() as usize).saturating_sub(1);
            let s_q8 = (s_op.clamp(0.0, 1.0) * 256.0 + 0.5) as u32;

            let b_data = bp.data();
            let bw = bp.width() as usize;
            let bh = bp.height() as usize;
            let bw_m1 = bw.saturating_sub(1);
            let bh_m1 = bh.saturating_sub(1);
            let bx_scale = bw as f32 / layer_w.max(1) as f32;
            let by_scale = bh as f32 / layer_h.max(1) as f32;
            let b_du_dx = (inv.sx * bx_scale * FP).round() as i32;
            let b_dv_dx = (inv.ky * by_scale * FP).round() as i32;
            let max_bu_fp = ((bw as i32) << 16) - 1;
            let max_bv_fp = ((bh as i32) << 16) - 1;
            let b_q8 = (b_op.clamp(0.0, 1.0) * 256.0 + 0.5) as u32;

            for y in dy0..dy1 {
                let yf = y as f32 + 0.5;
                let xf = dx0 as f32 + 0.5;
                let sx_f = inv.sx * xf + inv.kx * yf + inv.tx;
                let sy_f = inv.ky * xf + inv.sy * yf + inv.ty;
                let mut u_fp = ((sx_f - 0.5) * FP) as i32;
                let mut v_fp = ((sy_f - 0.5) * FP) as i32;
                let mut bu_fp = ((sx_f * bx_scale - 0.5) * FP) as i32;
                let mut bv_fp = ((sy_f * by_scale - 0.5) * FP) as i32;
                let row_off = y * dst_w * 4;

                for x in dx0..dx1 {
                    let out_i = row_off + x * 4;
                    if u_fp >= 0 && u_fp <= max_u_fp && v_fp >= 0 && v_fp <= max_v_fp {
                        let [sr, sg, sb, sa] =
                            sample_bilinear_fp(s_data, sw, sw_m1, sh_m1, u_fp, v_fp);
                        if sa != 0 {
                            blend_src_over_premul(
                                dst_data,
                                out_i,
                                (sr * s_q8 + 128) >> 8,
                                (sg * s_q8 + 128) >> 8,
                                (sb * s_q8 + 128) >> 8,
                                (sa * s_q8 + 128) >> 8,
                            );
                        }
                    }
                    if bu_fp >= 0 && bu_fp <= max_bu_fp && bv_fp >= 0 && bv_fp <= max_bv_fp {
                        let [br, bg, bb, ba] =
                            sample_bilinear_fp(b_data, bw, bw_m1, bh_m1, bu_fp, bv_fp);
                        if ba != 0 {
                            blend_src_over_premul(
                                dst_data,
                                out_i,
                                (br * b_q8 + 128) >> 8,
                                (bg * b_q8 + 128) >> 8,
                                (bb * b_q8 + 128) >> 8,
                                (ba * b_q8 + 128) >> 8,
                            );
                        }
                    }
                    u_fp = u_fp.wrapping_add(du_dx);
                    v_fp = v_fp.wrapping_add(dv_dx);
                    bu_fp = bu_fp.wrapping_add(b_du_dx);
                    bv_fp = bv_fp.wrapping_add(b_dv_dx);
                }
            }
            true
        }
        (None, None) => true,
    }
}

/// Composite an already-rendered world-space layer pixmap onto `dst` using the
/// 2.5D affine camera transform and O(1) dual-blur rack focus crossfade.
pub fn composite_world_layer(
    dst: &mut Pixmap,
    layer: &Layer,
    layer_pm: &Pixmap,
    cam: CameraState,
) -> Result<()> {
    let Some(bounds) = super::living_raster::find_alpha_bounds(layer_pm) else {
        return Ok(());
    };

    let layer_z = layer.effective_depth_z();
    let cam_tf = compute_plane_camera_transform(
        cam,
        layer_z,
        dst.width() as f32,
        dst.height() as f32,
    );
    let lw = layer_pm.width() as usize;
    let lh = layer_pm.height() as usize;

    let blur_mix = cam
        .focus_z
        .map(|fz| ((layer_z - fz).abs() * cam.dof_strength).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    if blur_mix <= 0.01 {
        if !composite_affine_fused(dst, Some((layer_pm, 1.0)), None, bounds, cam_tf, lw, lh) {
            let paint = PixmapPaint {
                opacity: 1.0,
                quality: FilterQuality::Bilinear,
                ..PixmapPaint::default()
            };
            dst.draw_pixmap(0, 0, layer_pm.as_ref(), &paint, cam_tf, None);
        }
        return Ok(());
    }

    let sharp_op = (1.0 - blur_mix).clamp(0.0, 1.0);
    let sharp_arg = if sharp_op > 0.01 {
        Some((layer_pm, sharp_op))
    } else {
        None
    };

    // Blurred 1/4-res twin (cached O(1) only for truly static image planes)
    let is_static_image = matches!(&layer.kind, LayerKind::Image { .. })
        && layer.animations.is_empty()
        && layer.uv_warp.is_none()
        && layer.relight.is_none();

    if is_static_image {
        let LayerKind::Image { src, .. } = &layer.kind else {
            unreachable!()
        };
        let key = format!("{}|{}x{}", src, layer_pm.width(), layer_pm.height());
        let cache = RACK_FOCUS_BLUR_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let blurred_arc: Arc<Pixmap> =
            if let Some(hit) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned() {
                hit
            } else {
                let built = Arc::new(
                    downsample_and_blur_quarter(layer_pm)
                        .ok_or_else(|| anyhow!("Failed to build 1/4-res rack focus blur twin"))?,
                );
                let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
                if guard.len() < 128 {
                    guard.insert(key, built.clone());
                }
                built
            };

        if !composite_affine_fused(
            dst,
            sharp_arg,
            Some((&blurred_arc, blur_mix)),
            bounds,
            cam_tf,
            lw,
            lh,
        ) {
            if let Some((sp, sop)) = sharp_arg {
                let paint = PixmapPaint {
                    opacity: sop,
                    quality: FilterQuality::Bilinear,
                    ..PixmapPaint::default()
                };
                dst.draw_pixmap(0, 0, sp.as_ref(), &paint, cam_tf, None);
            }
            let up_sx = layer_pm.width() as f32 / blurred_arc.width() as f32;
            let up_sy = layer_pm.height() as f32 / blurred_arc.height() as f32;
            let blur_tf = Transform::from_scale(up_sx, up_sy).post_concat(cam_tf);
            let blur_paint = PixmapPaint {
                opacity: blur_mix,
                quality: FilterQuality::Bilinear,
                ..PixmapPaint::default()
            };
            dst.draw_pixmap(0, 0, (*blurred_arc).as_ref(), &blur_paint, blur_tf, None);
        }
        return Ok(());
    }

    // Dynamic world layers: reuse thread-local 1/4-res scratch Pixmap (zero heap alloc per frame)
    let qw = (layer_pm.width() / 4).max(1);
    let qh = (layer_pm.height() / 4).max(1);
    CAM25D_QUARTER_SCRATCH.with(|cell| -> Result<()> {
        let mut slot = cell.borrow_mut();
        let needs_alloc = slot
            .as_ref()
            .is_none_or(|p| p.width() != qw || p.height() != qh);
        if needs_alloc {
            *slot = Some(
                Pixmap::new(qw, qh)
                    .ok_or_else(|| anyhow!("Failed to build 1/4-res rack focus blur twin"))?,
            );
        }
        let small = slot.as_mut().unwrap();
        downsample_and_blur_quarter_into(layer_pm, small);

        if !composite_affine_fused(
            dst,
            sharp_arg,
            Some((small, blur_mix)),
            bounds,
            cam_tf,
            lw,
            lh,
        ) {
            if let Some((sp, sop)) = sharp_arg {
                let paint = PixmapPaint {
                    opacity: sop,
                    quality: FilterQuality::Bilinear,
                    ..PixmapPaint::default()
                };
                dst.draw_pixmap(0, 0, sp.as_ref(), &paint, cam_tf, None);
            }
            let up_sx = layer_pm.width() as f32 / small.width() as f32;
            let up_sy = layer_pm.height() as f32 / small.height() as f32;
            let blur_tf = Transform::from_scale(up_sx, up_sy).post_concat(cam_tf);
            let blur_paint = PixmapPaint {
                opacity: blur_mix,
                quality: FilterQuality::Bilinear,
                ..PixmapPaint::default()
            };
            dst.draw_pixmap(0, 0, small.as_ref(), &blur_paint, blur_tf, None);
        }
        Ok(())
    })
}
