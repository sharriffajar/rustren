use std::cell::RefCell;

use image::RgbaImage;
use tiny_skia::{Pixmap, PremultipliedColorU8};

use crate::schema::{RelightSpec, UvWarpSpec};

thread_local! {
    /// Per-thread reusable scratch buffer for UV warp source copy (avoids per-frame multi-MB allocs).
    static UV_WARP_SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// Per-thread reusable x-lookup tables to avoid per-frame small Vec allocations.
    static X_INDEX_BUF_A: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static X_INDEX_BUF_B: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static WX_WAVE_BUF: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

/// Fast tight bounding box of non-transparent pixels in a Pixmap: `(min_x, min_y, max_x, max_y)` inclusive.
pub(crate) fn find_alpha_bounds(pm: &Pixmap) -> Option<(usize, usize, usize, usize)> {
    let w = pm.width() as usize;
    let h = pm.height() as usize;
    if w == 0 || h == 0 {
        return None;
    }
    let data = pm.data();
    let (_, pixels, _) = unsafe { data.align_to::<u32>() };
    if pixels.len() != w * h {
        return Some((0, 0, w - 1, h - 1));
    }

    let mut min_y = h;
    let mut max_y = 0usize;
    let mut min_x = w;
    let mut max_x = 0usize;

    for y in 0..h {
        let row = &pixels[y * w..(y + 1) * w];
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
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

/// Rigidity-masked procedural UV warp (replaces CPU skeletal mesh rigging for
/// swaying curtains, drifting hair/cloth, rustling foliage, and breathing motion).
///
/// `mask` is an optional 1-channel weight map (sampled from R channel: 0 = pinned, 255 = full motion).
pub fn apply_rigidity_uv_warp(
    pm: &mut Pixmap,
    spec: &UvWarpSpec,
    mask: Option<&RgbaImage>,
    time_sec: f32,
) {
    let w = pm.width() as usize;
    let h = pm.height() as usize;
    if w < 2 || h < 2 || (spec.amplitude_x.abs() < 1e-3 && spec.amplitude_y.abs() < 1e-3) {
        return;
    }

    let Some((b_x0, b_y0, b_x1, b_y1)) = find_alpha_bounds(pm) else {
        return;
    };

    let pad_x = spec.amplitude_x.abs().ceil() as usize + 2;
    let pad_y = spec.amplitude_y.abs().ceil() as usize + 2;
    let x0 = b_x0.saturating_sub(pad_x);
    let y0 = b_y0.saturating_sub(pad_y);
    let x1 = (b_x1 + pad_x).min(w - 1);
    let y1 = (b_y1 + pad_y).min(h - 1);

    let wl = spec.wavelength.max(4.0);
    let inv_wl_tau = std::f32::consts::TAU / wl;
    let freq_t = time_sec * spec.speed * std::f32::consts::TAU * spec.freq;
    let wf = (w - 1) as f32;
    let hf = (h - 1) as f32;
    let amp_x = spec.amplitude_x;
    let amp_y = spec.amplitude_y;
    let phase = spec.phase;
    let has_y_warp = amp_y.abs() >= 1e-3;

    // Precompute cos wave table across active columns
    WX_WAVE_BUF.with(|wx_cell| {
        let mut wx_table = wx_cell.borrow_mut();
        if has_y_warp {
            wx_table.resize(w, 0.0);
            for x in x0..=x1 {
                wx_table[x] = (freq_t + (x as f32) * inv_wl_tau + phase).cos() * amp_y;
            }
        }

        // Precompute mask column byte offsets (R channel of RGBA mask)
        X_INDEX_BUF_A.with(|mx_cell| {
            let mut mx_table = mx_cell.borrow_mut();
            let mask_info = mask.map(|m| {
                let mw = m.width() as usize;
                let mh = m.height() as usize;
                let mw_m1 = mw.saturating_sub(1);
                let mh_m1 = mh.saturating_sub(1);
                mx_table.resize(w, 0);
                let scale_x = mw_m1 as f32 / wf;
                for x in x0..=x1 {
                    let mx = ((x as f32) * scale_x).round() as usize;
                    mx_table[x] = mx.min(mw_m1) * 4;
                }
                (m.as_raw().as_slice(), mw * 4, mh_m1, mh_m1 as f32 / hf)
            });

            // Copy only the active rows [y0..=y1] into thread-local scratch buffer
            UV_WARP_SCRATCH.with(|scratch_cell| {
                let mut src_buf = scratch_cell.borrow_mut();
                let needed = (y1 - y0 + 1) * w * 4;
                if src_buf.len() < needed {
                    src_buf.resize(needed, 0);
                }
                let src_start = y0 * w * 4;
                src_buf[..needed].copy_from_slice(&pm.data()[src_start..src_start + needed]);
                let src_bytes = &src_buf[..needed];
                let dst = pm.data_mut();
                let y0_f = y0 as f32;
                let y1_f = y1 as f32;
                let max_local_y = y1 - y0;
                const INV_255: f32 = 1.0 / 255.0;

                if !has_y_warp {
                    // Fast 1D horizontal-only linear interpolation path (2 taps instead of 4)
                    for y in y0..=y1 {
                        let yf = y as f32;
                        let wy_amp = (freq_t + yf * inv_wl_tau + phase).sin() * amp_x;
                        if wy_amp.abs() < 1e-3 {
                            continue;
                        }
                        let mask_row = mask_info.map(|(raw, stride, mh_m1, scale_y)| {
                            let my = (yf * scale_y).round() as usize;
                            let row_off = my.min(mh_m1) * stride;
                            &raw[row_off..row_off + stride]
                        });
                        let src_row = (y - y0) * w * 4;
                        let dst_row = y * w * 4;

                        for x in x0..=x1 {
                            let weight = if let Some(mrow) = mask_row {
                                let r_val = mrow[mx_table[x]];
                                if r_val == 0 {
                                    continue;
                                }
                                r_val as f32 * INV_255
                            } else {
                                1.0
                            };

                            let su = ((x as f32) + weight * wy_amp).clamp(0.0, wf);
                            let sx0 = su as usize;
                            let sx1 = (sx0 + 1).min(w - 1);

                            let i0 = src_row + sx0 * 4;
                            let i1 = src_row + sx1 * 4;
                            let out_i = dst_row + x * 4;

                            if (src_bytes[i0 + 3] | src_bytes[i1 + 3]) == 0 {
                                dst[out_i..out_i + 4].fill(0);
                                continue;
                            }

                            let fx = ((su - sx0 as f32) * 256.0 + 0.5) as u32;
                            let inv_fx = 256 - fx;
                            dst[out_i] = ((src_bytes[i0] as u32 * inv_fx
                                + src_bytes[i1] as u32 * fx
                                + 128)
                                >> 8) as u8;
                            dst[out_i + 1] = ((src_bytes[i0 + 1] as u32 * inv_fx
                                + src_bytes[i1 + 1] as u32 * fx
                                + 128)
                                >> 8) as u8;
                            dst[out_i + 2] = ((src_bytes[i0 + 2] as u32 * inv_fx
                                + src_bytes[i1 + 2] as u32 * fx
                                + 128)
                                >> 8) as u8;
                            dst[out_i + 3] = ((src_bytes[i0 + 3] as u32 * inv_fx
                                + src_bytes[i1 + 3] as u32 * fx
                                + 128)
                                >> 8) as u8;
                        }
                    }
                    return;
                }

                // 2D fixed-point bilinear path (16-bit weights: fx_i, fy_i in 0..=256)
                for y in y0..=y1 {
                    let yf = y as f32;
                    let wy_amp = (freq_t + yf * inv_wl_tau + phase).sin() * amp_x;
                    let mask_row = mask_info.map(|(raw, stride, mh_m1, scale_y)| {
                        let my = (yf * scale_y).round() as usize;
                        let row_off = my.min(mh_m1) * stride;
                        &raw[row_off..row_off + stride]
                    });
                    let dst_row = y * w * 4;

                    for x in x0..=x1 {
                        let weight = if let Some(mrow) = mask_row {
                            let r_val = mrow[mx_table[x]];
                            if r_val == 0 {
                                continue;
                            }
                            r_val as f32 * INV_255
                        } else {
                            1.0
                        };

                        let su = ((x as f32) + weight * wy_amp).clamp(0.0, wf);
                        let sv = (yf + weight * wx_table[x]).clamp(y0_f, y1_f);

                        let sx0 = su as usize;
                        let sy0_local = (sv - y0_f) as usize;
                        let sx1 = (sx0 + 1).min(w - 1);
                        let sy1_local = (sy0_local + 1).min(max_local_y);

                        let i00 = (sy0_local * w + sx0) * 4;
                        let i10 = (sy0_local * w + sx1) * 4;
                        let i01 = (sy1_local * w + sx0) * 4;
                        let i11 = (sy1_local * w + sx1) * 4;

                        let out_i = dst_row + x * 4;
                        if (src_bytes[i00 + 3]
                            | src_bytes[i10 + 3]
                            | src_bytes[i01 + 3]
                            | src_bytes[i11 + 3])
                            == 0
                        {
                            dst[out_i..out_i + 4].fill(0);
                            continue;
                        }

                        let fx = ((su - sx0 as f32) * 256.0 + 0.5) as u32;
                        let fy = (((sv - y0_f) - sy0_local as f32) * 256.0 + 0.5) as u32;
                        let inv_fx = 256 - fx;
                        let inv_fy = 256 - fy;
                        let w00 = inv_fx * inv_fy;
                        let w10 = fx * inv_fy;
                        let w01 = inv_fx * fy;
                        let w11 = fx * fy;

                        dst[out_i] = ((src_bytes[i00] as u32 * w00
                            + src_bytes[i10] as u32 * w10
                            + src_bytes[i01] as u32 * w01
                            + src_bytes[i11] as u32 * w11
                            + 32768)
                            >> 16) as u8;
                        dst[out_i + 1] = ((src_bytes[i00 + 1] as u32 * w00
                            + src_bytes[i10 + 1] as u32 * w10
                            + src_bytes[i01 + 1] as u32 * w01
                            + src_bytes[i11 + 1] as u32 * w11
                            + 32768)
                            >> 16) as u8;
                        dst[out_i + 2] = ((src_bytes[i00 + 2] as u32 * w00
                            + src_bytes[i10 + 2] as u32 * w10
                            + src_bytes[i01 + 2] as u32 * w01
                            + src_bytes[i11 + 2] as u32 * w11
                            + 32768)
                            >> 16) as u8;
                        dst[out_i + 3] = ((src_bytes[i00 + 3] as u32 * w00
                            + src_bytes[i10 + 3] as u32 * w10
                            + src_bytes[i01 + 3] as u32 * w01
                            + src_bytes[i11 + 3] as u32 * w11
                            + 32768)
                            >> 16) as u8;
                    }
                }
            });
        });
    });
}

fn parse_hex_rgb_f32(hex: &str) -> (f32, f32, f32) {
    let h = hex.trim_start_matches('#');
    if h.len() >= 6 {
        let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(255) as f32 / 255.0;
        let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(220) as f32 / 255.0;
        let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(170) as f32 / 255.0;
        return (r, g, b);
    }
    (1.0, 0.88, 0.67)
}

#[inline(always)]
fn modulate_pixel(
    px: PremultipliedColorU8,
    lr_q8: u32,
    lg_q8: u32,
    lb_q8: u32,
    er_u8: u32,
    eg_u8: u32,
    eb_u8: u32,
) -> PremultipliedColorU8 {
    let a = px.alpha() as u32;
    if a == 0 {
        return px;
    }
    let out_r = (((px.red() as u32 * lr_q8 + 128) >> 8) + ((er_u8 * a + 127) / 255)).min(a) as u8;
    let out_g = (((px.green() as u32 * lg_q8 + 128) >> 8) + ((eg_u8 * a + 127) / 255)).min(a) as u8;
    let out_b = (((px.blue() as u32 * lb_q8 + 128) >> 8) + ((eb_u8 * a + 127) / 255)).min(a) as u8;
    PremultipliedColorU8::from_rgba(out_r, out_g, out_b, a as u8).unwrap_or(px)
}

/// Half-res 2D normal & emission relighting pass.
///
/// Evaluates smooth lighting (`ambient + sum_i max(0, dot(N, L_i)) * atten * intensity * flicker`)
/// on a 2x2 half-resolution grid (matching half-res normal maps) while modulating full-resolution
/// premultiplied RGBA pixels so edges and alpha remain crisp at ~4x lower CPU cost.
pub fn apply_2d_normal_relight(
    pm: &mut Pixmap,
    spec: &RelightSpec,
    normal_map: Option<&RgbaImage>,
    emission_map: Option<&RgbaImage>,
    time_sec: f32,
) {
    let w = pm.width() as usize;
    let h = pm.height() as usize;
    if w == 0 || h == 0 {
        return;
    }

    let Some((x0, y0, x1, y1)) = find_alpha_bounds(pm) else {
        return;
    };

    struct EvalLight {
        lx: f32,
        ly: f32,
        lz: f32,
        lz_sq: f32,
        r: f32,
        g: f32,
        b: f32,
        inv_radius_sq: f32,
    }

    let lights: Vec<EvalLight> = spec
        .lights
        .iter()
        .map(|l| {
            let (r, g, b) = parse_hex_rgb_f32(&l.color);
            let flicker = if l.flicker_amp > 0.0 && l.flicker_freq > 0.0 {
                let s1 = (time_sec * l.flicker_freq * std::f32::consts::TAU).sin();
                let s2 = (time_sec * l.flicker_freq * 2.37 * std::f32::consts::TAU).cos();
                (1.0 + l.flicker_amp * (0.65 * s1 + 0.35 * s2)).max(0.05)
            } else {
                1.0
            };
            let rad = l.radius.max(0.05);
            let lz = l.z.max(0.05);
            let intensity = l.intensity.max(0.0) * flicker;
            EvalLight {
                lx: l.x,
                ly: l.y,
                lz,
                lz_sq: lz * lz,
                r: r * intensity,
                g: g * intensity,
                b: b * intensity,
                inv_radius_sq: 1.0 / (rad * rad),
            }
        })
        .collect();

    let ambient = spec.ambient.clamp(0.0, 2.0);
    let inv_wf = 1.0 / (w.max(2) - 1) as f32;
    let inv_hf = 1.0 / (h.max(2) - 1) as f32;
    const INV_127_5: f32 = 1.0 / 127.5;

    X_INDEX_BUF_A.with(|nm_x_cell| {
        let mut nm_x = nm_x_cell.borrow_mut();
        let nm_info = normal_map.map(|nm| {
            let nw = nm.width() as usize;
            let nh = nm.height() as usize;
            let nw_m1 = nw.saturating_sub(1);
            let nh_m1 = nh.saturating_sub(1);
            nm_x.resize(w, 0);
            let sx = nw_m1 as f32 * inv_wf;
            for x in x0..=x1 {
                nm_x[x] = ((x as f32) * sx).round().min(nw_m1 as f32) as usize * 4;
            }
            (nm.as_raw().as_slice(), nw * 4, nh_m1, nh_m1 as f32 * inv_hf)
        });

        X_INDEX_BUF_B.with(|em_x_cell| {
            let mut em_x = em_x_cell.borrow_mut();
            let em_info = emission_map.map(|em| {
                let ew = em.width() as usize;
                let eh = em.height() as usize;
                let ew_m1 = ew.saturating_sub(1);
                let eh_m1 = eh.saturating_sub(1);
                em_x.resize(w, 0);
                let sx = ew_m1 as f32 * inv_wf;
                for x in x0..=x1 {
                    em_x[x] = ((x as f32) * sx).round().min(ew_m1 as f32) as usize * 4;
                }
                (em.as_raw().as_slice(), ew * 4, eh_m1, eh_m1 as f32 * inv_hf)
            });

            let pixels = pm.pixels_mut();

            // Process in 2x2 tiles for ~4x fewer sqrt/div lighting evaluations while keeping full-res albedo/alpha
            let mut y = y0;
            while y <= y1 {
                let has_y1 = y + 1 <= y1;
                let y_sample = if has_y1 { y as f32 + 0.5 } else { y as f32 };
                let ny_uv = y_sample * inv_hf;

                let nm_row = nm_info.map(|(raw, stride, nh_m1, sy)| {
                    let my = (y_sample * sy).round().min(nh_m1 as f32) as usize;
                    let off = my * stride;
                    &raw[off..off + stride]
                });
                let em_row = em_info.map(|(raw, stride, eh_m1, sy)| {
                    let my = (y_sample * sy).round().min(eh_m1 as f32) as usize;
                    let off = my * stride;
                    &raw[off..off + stride]
                });

                let row0 = y * w;
                let row1 = if has_y1 { (y + 1) * w } else { row0 };

                let mut x = x0;
                while x <= x1 {
                    let has_x1 = x + 1 <= x1;
                    let idx00 = row0 + x;
                    let p00 = pixels[idx00];
                    let p10 = if has_x1 {
                        pixels[idx00 + 1]
                    } else {
                        PremultipliedColorU8::TRANSPARENT
                    };
                    let p01 = if has_y1 {
                        pixels[row1 + x]
                    } else {
                        PremultipliedColorU8::TRANSPARENT
                    };
                    let p11 = if has_x1 && has_y1 {
                        pixels[row1 + x + 1]
                    } else {
                        PremultipliedColorU8::TRANSPARENT
                    };

                    if (p00.alpha() | p10.alpha() | p01.alpha() | p11.alpha()) == 0 {
                        x += 2;
                        continue;
                    }

                    let nx_uv = (if has_x1 { x as f32 + 0.5 } else { x as f32 }) * inv_wf;

                    let mut lr = ambient;
                    let mut lg = ambient;
                    let mut lb = ambient;

                    if !lights.is_empty() {
                        if let Some(nrow) = nm_row {
                            let off = nm_x[x];
                            let x_n = nrow[off] as f32 * INV_127_5 - 1.0;
                            let y_n = nrow[off + 1] as f32 * INV_127_5 - 1.0;
                            let z_n = (nrow[off + 2] as f32 * INV_127_5 - 1.0).max(0.1);
                            let inv_n = 1.0 / (x_n * x_n + y_n * y_n + z_n * z_n).sqrt().max(1e-4);
                            let nx = x_n * inv_n;
                            let ny = y_n * inv_n;
                            let nz = z_n * inv_n;

                            for light in &lights {
                                let dx = light.lx - nx_uv;
                                let dy = light.ly - ny_uv;
                                let dot_raw = nx * dx + ny * dy + nz * light.lz;
                                if dot_raw > 0.0 {
                                    let d2 = dx * dx + dy * dy;
                                    let atten = 1.0 / (1.0 + d2 * light.inv_radius_sq);
                                    let inv_len = 1.0 / (d2 + light.lz_sq).sqrt().max(1e-4);
                                    let contrib = dot_raw * inv_len * atten;
                                    lr += light.r * contrib;
                                    lg += light.g * contrib;
                                    lb += light.b * contrib;
                                }
                            }
                        } else {
                            // Flat normal (0, 0, 1): skip normal decoding and normalization sqrt
                            for light in &lights {
                                let dx = light.lx - nx_uv;
                                let dy = light.ly - ny_uv;
                                let d2 = dx * dx + dy * dy;
                                let atten = 1.0 / (1.0 + d2 * light.inv_radius_sq);
                                let inv_len = 1.0 / (d2 + light.lz_sq).sqrt().max(1e-4);
                                let contrib = light.lz * inv_len * atten;
                                lr += light.r * contrib;
                                lg += light.g * contrib;
                                lb += light.b * contrib;
                            }
                        }
                    }

                    let lr_q8 = (lr.clamp(0.0, 16.0) * 256.0 + 0.5) as u32;
                    let lg_q8 = (lg.clamp(0.0, 16.0) * 256.0 + 0.5) as u32;
                    let lb_q8 = (lb.clamp(0.0, 16.0) * 256.0 + 0.5) as u32;

                    let (er, eg, eb) = if let Some(erow) = em_row {
                        let off = em_x[x];
                        (erow[off] as u32, erow[off + 1] as u32, erow[off + 2] as u32)
                    } else {
                        (0, 0, 0)
                    };

                    if p00.alpha() != 0 {
                        pixels[idx00] = modulate_pixel(p00, lr_q8, lg_q8, lb_q8, er, eg, eb);
                    }
                    if has_x1 && p10.alpha() != 0 {
                        pixels[idx00 + 1] = modulate_pixel(p10, lr_q8, lg_q8, lb_q8, er, eg, eb);
                    }
                    if has_y1 && p01.alpha() != 0 {
                        pixels[row1 + x] = modulate_pixel(p01, lr_q8, lg_q8, lb_q8, er, eg, eb);
                    }
                    if has_x1 && has_y1 && p11.alpha() != 0 {
                        pixels[row1 + x + 1] =
                            modulate_pixel(p11, lr_q8, lg_q8, lb_q8, er, eg, eb);
                    }

                    x += 2;
                }
                y += 2;
            }
        });
    });
}
