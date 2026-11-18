//! Fixed-point BT.601 planar YUV420p conversion (Pro feature).

#[cfg(feature = "pro")]
pub(super) fn rgba_to_yuv420p_into(rgba: &[u8], width: u32, height: u32, out: &mut Vec<u8>) {
    rustren_pro::rgba_to_yuv420p_into(rgba, width, height, out);
}
