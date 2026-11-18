//! Fixed-point BT.601 planar YUV420p conversion (Pro feature).

#[cfg(feature = "pro")]
pub(super) fn rgba_to_yuv420p_into(rgba: &[u8], width: u32, height: u32, out: &mut Vec<u8>) {
    let w = width as usize;
    let h = height as usize;
    let y_len = w * h;
    let uv_len = (w / 2) * (h / 2);
    let total_len = y_len + uv_len * 2;
    if out.len() != total_len {
        out.resize(total_len, 0);
    }
    let (y_plane, uv_planes) = out.split_at_mut(y_len);
    let (u_plane, v_plane) = uv_planes.split_at_mut(uv_len);

    let row_stride = w * 4;
    let half_w = w / 2;
    let half_h = h / 2;
    for by in 0..half_h {
        let r0_off = (by * 2) * row_stride;
        let r1_off = r0_off + row_stride;
        let y0_off = (by * 2) * w;
        let y1_off = y0_off + w;
        let uv_row_off = by * half_w;

        for bx in 0..half_w {
            let i00 = r0_off + bx * 8;
            let i01 = i00 + 4;
            let i10 = r1_off + bx * 8;
            let i11 = i10 + 4;

            let (r00, g00, b00) = (rgba[i00] as i32, rgba[i00 + 1] as i32, rgba[i00 + 2] as i32);
            let (r01, g01, b01) = (rgba[i01] as i32, rgba[i01 + 1] as i32, rgba[i01 + 2] as i32);
            let (r10, g10, b10) = (rgba[i10] as i32, rgba[i10 + 1] as i32, rgba[i10 + 2] as i32);
            let (r11, g11, b11) = (rgba[i11] as i32, rgba[i11 + 1] as i32, rgba[i11 + 2] as i32);

            let y_x0 = bx * 2;
            y_plane[y0_off + y_x0] =
                (((66 * r00 + 129 * g00 + 25 * b00 + 128) >> 8) + 16) as u8;
            y_plane[y0_off + y_x0 + 1] =
                (((66 * r01 + 129 * g01 + 25 * b01 + 128) >> 8) + 16) as u8;
            y_plane[y1_off + y_x0] =
                (((66 * r10 + 129 * g10 + 25 * b10 + 128) >> 8) + 16) as u8;
            y_plane[y1_off + y_x0 + 1] =
                (((66 * r11 + 129 * g11 + 25 * b11 + 128) >> 8) + 16) as u8;

            let r_sum = r00 + r01 + r10 + r11;
            let g_sum = g00 + g01 + g10 + g11;
            let b_sum = b00 + b01 + b10 + b11;

            let uv_idx = uv_row_off + bx;
            u_plane[uv_idx] =
                (((-38 * r_sum - 74 * g_sum + 112 * b_sum + 512) >> 10) + 128).clamp(16, 240)
                    as u8;
            v_plane[uv_idx] =
                (((112 * r_sum - 94 * g_sum - 18 * b_sum + 512) >> 10) + 128).clamp(16, 240)
                    as u8;
        }
    }
}
