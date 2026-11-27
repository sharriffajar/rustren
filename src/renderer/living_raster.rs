use image::RgbaImage;
use tiny_skia::{Pixmap, PremultipliedColorU8};

use crate::schema::{RelightSpec, UvWarpSpec};

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

    let src_bytes = pm.data().to_vec();
    let dst = pm.data_mut();
    let wl = spec.wavelength.max(4.0);
    let freq_t = time_sec * spec.speed * std::f32::consts::TAU * spec.freq;
    let wf = (w - 1) as f32;
    let hf = (h - 1) as f32;

    for y in 0..h {
        let yf = y as f32;
        let wy_wave = (freq_t + yf / wl * std::f32::consts::TAU + spec.phase).sin();
        for x in 0..w {
            let xf = x as f32;
            let weight = if let Some(m) = mask {
                let mx = ((xf / wf) * (m.width().saturating_sub(1)) as f32).round() as u32;
                let my = ((yf / hf) * (m.height().saturating_sub(1)) as f32).round() as u32;
                m.get_pixel(mx.min(m.width() - 1), my.min(m.height() - 1))[0] as f32 / 255.0
            } else {
                1.0
            };
            if weight <= 0.002 {
                continue;
            }
            let wx_wave = (freq_t + xf / wl * std::f32::consts::TAU + spec.phase).cos();
            let su = (xf + weight * spec.amplitude_x * wy_wave).clamp(0.0, wf);
            let sv = (yf + weight * spec.amplitude_y * wx_wave).clamp(0.0, hf);

            let x0 = su.floor() as usize;
            let y0 = sv.floor() as usize;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let fx = su - x0 as f32;
            let fy = sv - y0 as f32;
            let w00 = (1.0 - fx) * (1.0 - fy);
            let w10 = fx * (1.0 - fy);
            let w01 = (1.0 - fx) * fy;
            let w11 = fx * fy;

            let i00 = (y0 * w + x0) * 4;
            let i10 = (y0 * w + x1) * 4;
            let i01 = (y1 * w + x0) * 4;
            let i11 = (y1 * w + x1) * 4;
            let out_i = (y * w + x) * 4;

            for c in 0..4 {
                let v = src_bytes[i00 + c] as f32 * w00
                    + src_bytes[i10 + c] as f32 * w10
                    + src_bytes[i01 + c] as f32 * w01
                    + src_bytes[i11 + c] as f32 * w11;
                dst[out_i + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
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

/// Half-res 2D normal & emission relighting pass.
///
/// Modulates premultiplied RGBA pixels by `ambient + sum_i max(0, dot(N, L_i)) * atten * intensity * flicker`
/// and adds optional emission map glow.
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

    struct EvalLight {
        lx: f32,
        ly: f32,
        lz: f32,
        r: f32,
        g: f32,
        b: f32,
        inv_radius_sq: f32,
        intensity: f32,
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
            EvalLight {
                lx: l.x,
                ly: l.y,
                lz: l.z.max(0.05),
                r,
                g,
                b,
                inv_radius_sq: 1.0 / (rad * rad),
                intensity: l.intensity.max(0.0) * flicker,
            }
        })
        .collect();

    let ambient = spec.ambient.clamp(0.0, 2.0);
    let wf = (w.max(2) - 1) as f32;
    let hf = (h.max(2) - 1) as f32;
    let pixels = pm.pixels_mut();

    for y in 0..h {
        let ny_uv = y as f32 / hf;
        for x in 0..w {
            let idx = y * w + x;
            let px = pixels[idx];
            let a = px.alpha();
            if a == 0 {
                continue;
            }
            let nx_uv = x as f32 / wf;

            let (nx, ny, nz) = if let Some(nm) = normal_map {
                let mx = (nx_uv * (nm.width().saturating_sub(1)) as f32).round() as u32;
                let my = (ny_uv * (nm.height().saturating_sub(1)) as f32).round() as u32;
                let p = nm.get_pixel(mx.min(nm.width() - 1), my.min(nm.height() - 1));
                let x_n = (p[0] as f32 / 127.5) - 1.0;
                let y_n = (p[1] as f32 / 127.5) - 1.0;
                let z_n = ((p[2] as f32 / 127.5) - 1.0).max(0.1);
                let len = (x_n * x_n + y_n * y_n + z_n * z_n).sqrt().max(1e-4);
                (x_n / len, y_n / len, z_n / len)
            } else {
                (0.0, 0.0, 1.0)
            };

            let mut lr = ambient;
            let mut lg = ambient;
            let mut lb = ambient;

            for light in &lights {
                let dx = light.lx - nx_uv;
                let dy = light.ly - ny_uv;
                let dz = light.lz;
                let d2 = dx * dx + dy * dy;
                let atten = (1.0 / (1.0 + d2 * light.inv_radius_sq)).clamp(0.0, 1.0);
                let inv_len = 1.0 / (d2 + dz * dz).sqrt().max(1e-4);
                let ndotl = (nx * dx + ny * dy + nz * dz) * inv_len;
                if ndotl > 0.0 {
                    let contrib = ndotl * atten * light.intensity;
                    lr += light.r * contrib;
                    lg += light.g * contrib;
                    lb += light.b * contrib;
                }
            }

            let mut er = 0.0f32;
            let mut eg = 0.0f32;
            let mut eb = 0.0f32;
            if let Some(em) = emission_map {
                let mx = (nx_uv * (em.width().saturating_sub(1)) as f32).round() as u32;
                let my = (ny_uv * (em.height().saturating_sub(1)) as f32).round() as u32;
                let p = em.get_pixel(mx.min(em.width() - 1), my.min(em.height() - 1));
                let scale = a as f32 / 255.0;
                er = p[0] as f32 * scale;
                eg = p[1] as f32 * scale;
                eb = p[2] as f32 * scale;
            }

            let out_r = ((px.red() as f32 * lr + er).round().clamp(0.0, a as f32)) as u8;
            let out_g = ((px.green() as f32 * lg + eg).round().clamp(0.0, a as f32)) as u8;
            let out_b = ((px.blue() as f32 * lb + eb).round().clamp(0.0, a as f32)) as u8;
            pixels[idx] = PremultipliedColorU8::from_rgba(out_r, out_g, out_b, a).unwrap_or(px);
        }
    }
}
