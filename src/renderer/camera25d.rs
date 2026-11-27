use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Result, anyhow};
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
    // Depth sensitivity: foreground planes (layer_z > 0) move & scale faster than backdrop (layer_z < 0).
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

fn downsample_and_blur_quarter(src: &Pixmap) -> Option<Pixmap> {
    let qw = (src.width() / 4).max(1);
    let qh = (src.height() / 4).max(1);
    let mut small = Pixmap::new(qw, qh)?;
    let sx = qw as f32 / src.width() as f32;
    let sy = qh as f32 / src.height() as f32;
    let paint = PixmapPaint {
        opacity: 1.0,
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    small.draw_pixmap(0, 0, src.as_ref(), &paint, Transform::from_scale(sx, sy), None);
    super::fx::fast_blur_rgba(small.data_mut(), qw, qh, 3.0);
    Some(small)
}

/// Composite an already-rendered world-space layer pixmap onto `dst` using the
/// 2.5D affine camera transform and O(1) dual-blur rack focus crossfade.
pub fn composite_world_layer(
    dst: &mut Pixmap,
    layer: &Layer,
    layer_pm: &Pixmap,
    cam: CameraState,
) -> Result<()> {
    let layer_z = layer.effective_depth_z();
    let cam_tf = compute_plane_camera_transform(
        cam,
        layer_z,
        dst.width() as f32,
        dst.height() as f32,
    );

    let blur_mix = cam
        .focus_z
        .map(|fz| ((layer_z - fz).abs() * cam.dof_strength).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    if blur_mix <= 0.01 {
        let paint = PixmapPaint {
            opacity: 1.0,
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        };
        dst.draw_pixmap(0, 0, layer_pm.as_ref(), &paint, cam_tf, None);
        return Ok(());
    }

    // Sharp component
    let sharp_op = (1.0 - blur_mix).clamp(0.0, 1.0);
    if sharp_op > 0.01 {
        let paint = PixmapPaint {
            opacity: sharp_op,
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        };
        dst.draw_pixmap(0, 0, layer_pm.as_ref(), &paint, cam_tf, None);
    }

    // Blurred 1/4-res twin (cached O(1) for static image planes)
    let cache_key = match &layer.kind {
        LayerKind::Image { src, .. } if layer.animations.is_empty() => {
            Some(format!("{}|{}x{}", src, layer_pm.width(), layer_pm.height()))
        }
        _ => None,
    };

    let blurred_arc: Arc<Pixmap> = if let Some(key) = cache_key {
        let cache = RACK_FOCUS_BLUR_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
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
        }
    } else {
        Arc::new(
            downsample_and_blur_quarter(layer_pm)
                .ok_or_else(|| anyhow!("Failed to build 1/4-res rack focus blur twin"))?,
        )
    };

    let up_sx = layer_pm.width() as f32 / blurred_arc.width() as f32;
    let up_sy = layer_pm.height() as f32 / blurred_arc.height() as f32;
    let blur_tf = Transform::from_scale(up_sx, up_sy).post_concat(cam_tf);
    let blur_paint = PixmapPaint {
        opacity: blur_mix,
        quality: FilterQuality::Bilinear,
        ..PixmapPaint::default()
    };
    dst.draw_pixmap(0, 0, (*blurred_arc).as_ref(), &blur_paint, blur_tf, None);
    Ok(())
}
