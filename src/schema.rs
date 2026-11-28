use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Scene {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    pub canvas: Canvas,
    #[serde(default)]
    pub world: Option<WorldCameraSpec>,
    #[serde(default)]
    pub profiles: std::collections::HashMap<String, CompositionProfile>,
    #[serde(default)]
    pub fonts: Vec<String>,
    #[serde(default)]
    pub audio: Vec<AudioTrack>,
    pub layers: Vec<Layer>,
}

/// Film grain rendering mode:
/// - `fine` (default, v1.1 compatible): 1x1 per-pixel noise at 30fps (highest detail, larger H.264 bitrate)
/// - `film`: coarse 2x2 grain at 12fps cadence (organic 16mm look, much faster libx264 encode & smaller MP4)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrainMode {
    #[default]
    Fine,
    Film,
}

/// Coordinate space for a layer: `screen` (default, v1.1 behavior, unaffected by 2.5D camera)
/// or `world` (opt-in v1.2 2.5D diorama plane).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerSpace {
    #[default]
    Screen,
    World,
}

/// Semantic depth slots for 2.5D diorama planes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepthSlot {
    Backdrop,
    Far,
    Mid,
    Subject,
    Foreground,
}

impl DepthSlot {
    pub fn default_z(self) -> f32 {
        match self {
            DepthSlot::Backdrop => -1.0,
            DepthSlot::Far => -0.5,
            DepthSlot::Mid => 0.0,
            DepthSlot::Subject => 0.25,
            DepthSlot::Foreground => 0.65,
        }
    }
}

/// High-level 2.5D camera motion presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraPreset {
    PushIn,
    PullOut,
    TruckLeft,
    TruckRight,
    WhipLeft,
    WhipRight,
    RackFocus,
    PunchCut,
}

/// Opt-in 2.5D scalar virtual camera (`schema_version: "1.2"`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WorldCameraSpec {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub z: f32,
    #[serde(default)]
    pub roll: f32,
    #[serde(default)]
    pub focus_z: Option<f32>,
    #[serde(default = "default_dof_strength")]
    pub dof_strength: f32,
    #[serde(default)]
    pub motion_blur: f32,
    #[serde(default)]
    pub preset: Option<CameraPreset>,
    #[serde(default)]
    pub animations: Vec<Animation>,
}

fn default_dof_strength() -> f32 {
    1.2
}

/// Rigidity-masked procedural UV warp (`schema_version: "1.2"`).
#[derive(Debug, Clone, Deserialize)]
pub struct UvWarpSpec {
    #[serde(default)]
    pub mask_src: Option<String>,
    #[serde(default = "default_uv_amp")]
    pub amplitude_x: f32,
    #[serde(default)]
    pub amplitude_y: f32,
    #[serde(default = "default_uv_freq")]
    pub freq: f32,
    #[serde(default = "default_uv_wavelength")]
    pub wavelength: f32,
    #[serde(default)]
    pub phase: f32,
    #[serde(default = "default_scale")]
    pub speed: f32,
}

fn default_uv_amp() -> f32 {
    6.0
}
fn default_uv_freq() -> f32 {
    0.5
}
fn default_uv_wavelength() -> f32 {
    120.0
}

/// 2D point light for half-res normal/emission relighting (`schema_version: "1.2"`).
#[derive(Debug, Clone, Deserialize)]
pub struct Light2DSpec {
    #[serde(default = "default_pivot")]
    pub x: f32,
    #[serde(default = "default_pivot")]
    pub y: f32,
    #[serde(default = "default_light_z")]
    pub z: f32,
    #[serde(default = "default_light_color")]
    pub color: String,
    #[serde(default = "default_light_intensity")]
    pub intensity: f32,
    #[serde(default = "default_light_radius")]
    pub radius: f32,
    #[serde(default)]
    pub flicker_amp: f32,
    #[serde(default)]
    pub flicker_freq: f32,
}

fn default_light_z() -> f32 {
    0.35
}
fn default_light_color() -> String {
    "#ffddaa".to_string()
}
fn default_light_intensity() -> f32 {
    1.2
}
fn default_light_radius() -> f32 {
    0.65
}

/// Half-res 2D normal & emission relighting pass (`schema_version: "1.2"`).
#[derive(Debug, Clone, Deserialize)]
pub struct RelightSpec {
    #[serde(default)]
    pub normal_src: Option<String>,
    #[serde(default)]
    pub emission_src: Option<String>,
    #[serde(default = "default_ambient")]
    pub ambient: f32,
    #[serde(default)]
    pub lights: Vec<Light2DSpec>,
}

fn default_ambient() -> f32 {
    0.25
}

fn default_schema_version() -> String {
    "1.0".to_string()
}

impl Scene {
    pub fn apply_profile(&mut self, profile_name: &str) -> anyhow::Result<()> {
        let Some(p) = self.profiles.get(profile_name) else {
            anyhow::bail!(
                "Profile '{}' not found in scene.profiles. Available: {:?}",
                profile_name,
                self.profiles.keys().collect::<Vec<_>>()
            );
        };
        if let Some(w) = p.width {
            self.canvas.width = w;
        }
        if let Some(h) = p.height {
            self.canvas.height = h;
        }
        if let Some(fps) = p.fps {
            self.canvas.fps = fps;
        }
        if let Some(d) = p.duration_secs {
            self.canvas.duration_secs = d;
        }
        if let Some(bg) = &p.background {
            self.canvas.background = bg.clone();
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompositionProfile {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub duration_secs: Option<f32>,
    pub background: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_secs: f32,
    #[serde(default = "default_bg")]
    pub background: String,
}

fn default_bg() -> String {
    "#000000".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct AudioTrack {
    pub src: String,
    #[serde(default)]
    pub start_sec: f32,
    #[serde(default = "default_opacity")]
    pub volume: f32,
    #[serde(default)]
    pub trim_start_sec: Option<f32>,
    #[serde(default)]
    pub trim_end_sec: Option<f32>,
    #[serde(default = "default_scale")]
    pub playback_rate: f32,
    #[serde(default)]
    pub fade_in_secs: f32,
    #[serde(default)]
    pub fade_out_secs: f32,
    #[serde(default)]
    pub animations: Vec<Animation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Layer {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub start_sec: f32,
    pub end_sec: Option<f32>,
    #[serde(default)]
    pub duration_secs: Option<f32>,
    #[serde(default)]
    pub overlap_secs: f32,
    #[serde(default)]
    pub position: PositionSpec,
    #[serde(default)]
    pub flex_grow: Option<f32>,
    #[serde(default)]
    pub margin: Option<f32>,
    #[serde(default)]
    pub margin_top: Option<f32>,
    #[serde(default)]
    pub margin_right: Option<f32>,
    #[serde(default)]
    pub margin_bottom: Option<f32>,
    #[serde(default)]
    pub margin_left: Option<f32>,
    #[serde(default)]
    pub blend_mode: BlendModeSpec,
    #[serde(default)]
    pub blur: f32,
    #[serde(default)]
    pub motion_blur: f32,
    #[serde(default)]
    pub shadow: Option<ShadowSpec>,
    #[serde(default)]
    pub anchor: AnchorSpec,
    #[serde(default = "default_scale")]
    pub scale_x: f32,
    #[serde(default = "default_scale")]
    pub scale_y: f32,
    #[serde(default = "default_pivot")]
    pub pivot_x: f32,
    #[serde(default = "default_pivot")]
    pub pivot_y: f32,
    #[serde(default)]
    pub skew_x: f32,
    #[serde(default)]
    pub skew_y: f32,
    #[serde(default)]
    pub rotate_x: f32,
    #[serde(default)]
    pub rotate_y: f32,
    #[serde(default)]
    pub perspective: f32,
    #[serde(default = "default_true")]
    pub backface_visible: bool,
    #[serde(default, alias = "z")]
    pub z_index: f32,
    #[serde(default)]
    pub grain: f32,
    #[serde(default)]
    pub grain_mode: GrainMode,
    #[serde(default)]
    pub vignette: f32,
    #[serde(default)]
    pub halftone: f32,
    #[serde(default)]
    pub wave_warp: Option<WaveWarpSpec>,
    #[serde(default)]
    pub track_matte: TrackMatteMode,
    #[serde(default)]
    pub matte_layer: Option<Box<Layer>>,
    #[serde(default)]
    pub space: LayerSpace,
    #[serde(default)]
    pub depth_slot: Option<DepthSlot>,
    #[serde(default)]
    pub depth_z: Option<f32>,
    #[serde(default)]
    pub uv_warp: Option<UvWarpSpec>,
    #[serde(default)]
    pub relight: Option<RelightSpec>,
    #[serde(default)]
    pub echo: Option<EchoSpec>,
    #[serde(flatten)]
    pub kind: LayerKind,
    #[serde(default)]
    pub animations: Vec<Animation>,
}

impl Layer {
    /// Effective normalized depth for 2.5D world-space planes.
    pub fn effective_depth_z(&self) -> f32 {
        self.depth_z
            .or_else(|| self.depth_slot.map(|s| s.default_z()))
            .unwrap_or(0.0)
    }
}

impl Layer {
    /// Return effective `(top, right, bottom, left)` margins with side overrides falling back to `margin`.
    pub fn margin_trbl(&self) -> (f32, f32, f32, f32) {
        let base = self.margin.unwrap_or(0.0);
        (
            self.margin_top.unwrap_or(base),
            self.margin_right.unwrap_or(base),
            self.margin_bottom.unwrap_or(base),
            self.margin_left.unwrap_or(base),
        )
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackMatteMode {
    #[default]
    None,
    Alpha,
    AlphaInverted,
    Luma,
    LumaInverted,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WaveAxisSpec {
    #[default]
    X,
    Y,
    StretchX,
    StretchY,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct WaveWarpSpec {
    pub amplitude: f32,
    pub wavelength: f32,
    #[serde(default)]
    pub speed: f32,
    #[serde(default)]
    pub phase: f32,
    #[serde(default)]
    pub axis: WaveAxisSpec,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EchoSpec {
    #[serde(default = "default_echo_count")]
    pub count: u32,
    #[serde(default = "default_echo_step")]
    pub step_sec: f32,
    #[serde(default = "default_echo_decay")]
    pub decay: f32,
    #[serde(default)]
    pub tint_colors: Vec<String>,
}

fn default_echo_count() -> u32 {
    5
}
fn default_echo_step() -> f32 {
    0.03
}
fn default_echo_decay() -> f32 {
    0.65
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineMode {
    #[default]
    Global,
    #[serde(alias = "parallel")]
    Local,
    Series,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlendModeSpec {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShadowSpec {
    #[serde(default = "default_shadow_color")]
    pub color: String,
    #[serde(default)]
    pub offset_x: f32,
    #[serde(default)]
    pub offset_y: f32,
    #[serde(default)]
    pub blur: f32,
}

fn default_shadow_color() -> String {
    "#00000080".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextSpan {
    pub text: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub font_weight: Option<FontWeightSpec>,
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default)]
    pub font_size: Option<f32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FitSpec {
    #[default]
    Fill,
    Cover,
    Contain,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextOutlineSpec {
    #[serde(default = "default_text_outline_color")]
    pub color: String,
    #[serde(default = "default_text_outline_width")]
    pub width: f32,
}

fn default_text_outline_color() -> String {
    "#000000".to_string()
}

fn default_text_outline_width() -> f32 {
    2.0
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextAnimatorModeSpec {
    #[default]
    Char,
    Word,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextAnimatorSpec {
    #[serde(default)]
    pub mode: TextAnimatorModeSpec,
    #[serde(default)]
    pub start_sec: f32,
    #[serde(default = "default_text_animator_stagger")]
    pub stagger_secs: f32,
    #[serde(default = "default_text_animator_duration")]
    pub duration_secs: f32,
    #[serde(default)]
    pub y_offset: f32,
    #[serde(default)]
    pub x_offset: f32,
    #[serde(default = "default_opacity")]
    pub opacity_from: f32,
    #[serde(default = "default_scale")]
    pub scale_from: f32,
    #[serde(default)]
    pub easing: Easing,
}

fn default_text_animator_stagger() -> f32 {
    0.05
}

fn default_text_animator_duration() -> f32 {
    0.4
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnchorSpec {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl AnchorSpec {
    pub fn factors(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::TopCenter => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::CenterLeft => (0.0, 0.5),
            Self::Center => (0.5, 0.5),
            Self::CenterRight => (1.0, 0.5),
            Self::BottomLeft => (0.0, 1.0),
            Self::BottomCenter => (0.5, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum DynamicTextSpec {
    Typewriter {
        text: String,
        #[serde(default)]
        start_sec: Option<f32>,
        #[serde(default)]
        end_sec: Option<f32>,
        #[serde(default)]
        cursor: Option<String>,
    },
    Counter {
        from: f32,
        to: f32,
        #[serde(default)]
        decimals: usize,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        suffix: String,
        #[serde(default)]
        start_sec: Option<f32>,
        #[serde(default)]
        end_sec: Option<f32>,
        #[serde(default)]
        easing: Easing,
    },
    Timecode {
        #[serde(default = "default_timecode_format")]
        format: String,
        #[serde(default)]
        offset_sec: f32,
    },
}

fn default_timecode_format() -> String {
    "mm:ss".to_string()
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PositionSpec {
    #[default]
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GradientTypeSpec {
    #[default]
    Linear,
    Radial,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GradientStopSpec {
    pub offset: f32,
    pub color: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LinearGradientSpec {
    #[serde(default, rename = "type")]
    pub kind: GradientTypeSpec,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub stops: Vec<GradientStopSpec>,
    #[serde(default)]
    pub angle: f32,
    #[serde(default)]
    pub cx: Option<f32>,
    #[serde(default)]
    pub cy: Option<f32>,
    #[serde(default)]
    pub fx: Option<f32>,
    #[serde(default)]
    pub fy: Option<f32>,
    #[serde(default)]
    pub r: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayerKind {
    Rect {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        width: f32,
        height: f32,
        #[serde(default)]
        radius: f32,
        color: String,
        #[serde(default)]
        gradient: Option<LinearGradientSpec>,
        #[serde(default)]
        border_color: Option<String>,
        #[serde(default)]
        border_width: f32,
        #[serde(default = "default_opacity")]
        path_progress: f32,
        #[serde(default)]
        stroke_dashoffset: Option<f32>,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default = "default_scale")]
        scale: f32,
        #[serde(default)]
        rotation: f32,
    },
    Circle {
        #[serde(default)]
        cx: f32,
        #[serde(default)]
        cy: f32,
        r: f32,
        color: String,
        #[serde(default)]
        gradient: Option<LinearGradientSpec>,
        #[serde(default)]
        border_color: Option<String>,
        #[serde(default)]
        border_width: f32,
        #[serde(default = "default_opacity")]
        path_progress: f32,
        #[serde(default)]
        stroke_dashoffset: Option<f32>,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default = "default_scale")]
        scale: f32,
    },
    Text {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        #[serde(default)]
        content: String,
        #[serde(default)]
        dynamic: Option<DynamicTextSpec>,
        #[serde(default)]
        spans: Vec<TextSpan>,
        #[serde(default = "default_font_size")]
        font_size: f32,
        #[serde(default)]
        line_height: Option<f32>,
        #[serde(default)]
        letter_spacing: f32,
        #[serde(default)]
        font_family: Option<String>,
        #[serde(default)]
        font_weight: FontWeightSpec,
        #[serde(default)]
        text_align: TextAlignSpec,
        #[serde(default)]
        direction: TextDirectionSpec,
        #[serde(default = "default_text_color")]
        color: String,
        #[serde(default)]
        gradient: Option<LinearGradientSpec>,
        #[serde(default)]
        outline: Option<TextOutlineSpec>,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default)]
        max_width: Option<f32>,
        #[serde(default = "default_scale")]
        scale: f32,
        #[serde(default)]
        text_animator: Option<TextAnimatorSpec>,
    },
    Image {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        width: Option<f32>,
        height: Option<f32>,
        src: String,
        #[serde(default)]
        fit: FitSpec,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default)]
        rotation: f32,
        #[serde(default = "default_scale")]
        scale: f32,
    },
    Svg {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        width: Option<f32>,
        height: Option<f32>,
        src: String,
        #[serde(default = "default_opacity")]
        path_progress: f32,
        #[serde(default)]
        stroke_dashoffset: Option<f32>,
        #[serde(default)]
        stroke_dasharray: Option<Vec<f32>>,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default)]
        rotation: f32,
    },
    Video {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        width: Option<f32>,
        height: Option<f32>,
        src: String,
        #[serde(default)]
        fit: FitSpec,
        #[serde(default)]
        trim_start_sec: f32,
        #[serde(default)]
        trim_end_sec: Option<f32>,
        #[serde(default = "default_scale")]
        playback_rate: f32,
        #[serde(default, alias = "loop")]
        loop_video: bool,
        #[serde(default = "default_pivot")]
        focal_x: f32,
        #[serde(default = "default_pivot")]
        focal_y: f32,
        #[serde(default)]
        luma_key: Option<f32>,
        #[serde(default = "default_opacity")]
        volume: f32,
        #[serde(default)]
        muted: bool,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default)]
        rotation: f32,
        #[serde(default = "default_scale")]
        scale: f32,
    },
    Waveform {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        width: f32,
        height: f32,
        #[serde(default = "default_waveform_bars")]
        bars: usize,
        #[serde(default = "default_waveform_gap")]
        bar_gap: f32,
        #[serde(default = "default_color_white")]
        color: String,
        #[serde(default = "default_waveform_seed")]
        seed: u64,
        #[serde(default = "default_scale")]
        speed: f32,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default)]
        rotation: f32,
        #[serde(default = "default_scale")]
        scale: f32,
    },
    Lottie {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        width: Option<f32>,
        height: Option<f32>,
        src: String,
        #[serde(default)]
        loop_animation: bool,
        #[serde(default = "default_scale")]
        speed: f32,
        #[serde(default)]
        progress: Option<f32>,
        #[serde(default)]
        color_overrides: Option<std::collections::HashMap<String, String>>,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default)]
        rotation: f32,
        #[serde(default = "default_scale")]
        scale: f32,
    },
    Particles {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        #[serde(default = "default_particles_box")]
        width: f32,
        #[serde(default = "default_particles_box")]
        height: f32,
        #[serde(default = "default_particles_count")]
        count: usize,
        #[serde(default = "default_waveform_seed")]
        seed: u64,
        #[serde(default = "default_scale")]
        speed: f32,
        #[serde(default = "default_particles_gravity")]
        gravity: f32,
        #[serde(default = "default_particles_spread")]
        spread: f32,
        #[serde(default = "default_particles_angle")]
        angle: f32,
        #[serde(default = "default_particles_size")]
        size: f32,
        #[serde(default = "default_color_white")]
        color: String,
        #[serde(default)]
        colors: Vec<String>,
        #[serde(default)]
        shape: ParticleShapeSpec,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default)]
        rotation: f32,
        #[serde(default = "default_scale")]
        scale: f32,
    },
    Container {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        #[serde(default)]
        width: Option<f32>,
        #[serde(default)]
        height: Option<f32>,
        #[serde(default)]
        layout: LayoutSpec,
        #[serde(default)]
        background: Option<String>,
        #[serde(default)]
        gradient: Option<LinearGradientSpec>,
        #[serde(default)]
        border_color: Option<String>,
        #[serde(default)]
        border_width: f32,
        #[serde(default)]
        radius: f32,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default = "default_scale")]
        scale: f32,
        #[serde(default)]
        rotation: f32,
        #[serde(default)]
        clip: bool,
        #[serde(default)]
        clip_feather: f32,
        #[serde(default)]
        cascade_opacity: bool,
        #[serde(default)]
        timeline: TimelineMode,
        #[serde(default)]
        stagger_secs: f32,
        #[serde(default)]
        children: Vec<Layer>,
    },
    #[cfg(feature = "exp-3d")]
    Mesh3d {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        width: f32,
        height: f32,
        #[serde(default = "default_mesh3d_primitive")]
        primitive: Mesh3dPrimitive,
        #[serde(default)]
        src: Option<String>,
        #[serde(default)]
        texture: Option<String>,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default = "default_scale")]
        scale: f32,
        #[serde(default)]
        rotate_x: f32,
        #[serde(default)]
        rotate_y: f32,
        #[serde(default)]
        rotate_z: f32,
        #[serde(default = "default_mesh3d_fov")]
        fov: f32,
        #[serde(default = "default_mesh3d_color")]
        color: String,
        #[serde(default)]
        light_dir: Option<[f32; 3]>,
        #[serde(default)]
        wireframe: bool,
    },
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LayoutSpec {
    #[serde(default)]
    pub direction: FlexDirectionSpec,
    #[serde(default)]
    pub flex_wrap: bool,
    #[serde(default)]
    pub justify_content: Option<AlignSpec>,
    #[serde(default)]
    pub align_items: Option<AlignSpec>,
    #[serde(default)]
    pub gap: f32,
    #[serde(default)]
    pub row_gap: Option<f32>,
    #[serde(default)]
    pub column_gap: Option<f32>,
    #[serde(default)]
    pub padding: f32,
    #[serde(default)]
    pub padding_top: Option<f32>,
    #[serde(default)]
    pub padding_right: Option<f32>,
    #[serde(default)]
    pub padding_bottom: Option<f32>,
    #[serde(default)]
    pub padding_left: Option<f32>,
}

impl LayoutSpec {
    /// Return effective `(top, right, bottom, left)` padding with side overrides falling back to `padding`.
    pub fn padding_trbl(&self) -> (f32, f32, f32, f32) {
        let base = self.padding;
        (
            self.padding_top.unwrap_or(base),
            self.padding_right.unwrap_or(base),
            self.padding_bottom.unwrap_or(base),
            self.padding_left.unwrap_or(base),
        )
    }

    /// Return effective `(column_gap, row_gap)` with axis overrides falling back to `gap`.
    pub fn gaps(&self) -> (f32, f32) {
        (
            self.column_gap.unwrap_or(self.gap),
            self.row_gap.unwrap_or(self.gap),
        )
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FlexDirectionSpec {
    #[default]
    Row,
    Column,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignSpec {
    #[serde(alias = "flex_start")]
    Start,
    Center,
    #[serde(alias = "flex_end")]
    End,
    SpaceBetween,
    SpaceAround,
    Stretch,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParticleShapeSpec {
    #[default]
    Circle,
    Rect,
    Confetti,
}

#[cfg(feature = "exp-3d")]
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mesh3dPrimitive {
    #[default]
    Cube,
    Sphere,
    Pyramid,
    Torus,
}

#[cfg(feature = "exp-3d")]
fn default_mesh3d_primitive() -> Mesh3dPrimitive {
    Mesh3dPrimitive::Cube
}

#[cfg(feature = "exp-3d")]
fn default_mesh3d_fov() -> f32 {
    45.0
}

#[cfg(feature = "exp-3d")]
fn default_mesh3d_color() -> String {
    "#38BDF8".to_string()
}

fn default_true() -> bool {
    true
}

fn default_particles_box() -> f32 {
    400.0
}

fn default_particles_count() -> usize {
    40
}

fn default_particles_gravity() -> f32 {
    120.0
}

fn default_particles_spread() -> f32 {
    360.0
}

fn default_particles_angle() -> f32 {
    -90.0
}

fn default_particles_size() -> f32 {
    8.0
}

fn default_waveform_bars() -> usize {
    24
}

fn default_waveform_gap() -> f32 {
    4.0
}

fn default_waveform_seed() -> u64 {
    42
}

fn default_color_white() -> String {
    "#FFFFFF".to_string()
}

fn default_opacity() -> f32 {
    1.0
}

fn default_scale() -> f32 {
    1.0
}

fn default_pivot() -> f32 {
    0.5
}

fn default_font_size() -> f32 {
    48.0
}

fn default_text_color() -> String {
    "#FFFFFF".to_string()
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct SpringConfig {
    #[serde(default = "default_spring_stiffness")]
    pub stiffness: f32,
    #[serde(default = "default_spring_damping")]
    pub damping: f32,
    #[serde(default = "default_spring_mass")]
    pub mass: f32,
}

fn default_spring_stiffness() -> f32 {
    120.0
}

fn default_spring_damping() -> f32 {
    14.0
}

fn default_spring_mass() -> f32 {
    1.0
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    Infinite,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RepeatSpec {
    Count(u32),
    Mode(RepeatMode),
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum AnimValue {
    Number(f32),
    Color(String),
}

impl From<f32> for AnimValue {
    fn from(v: f32) -> Self {
        Self::Number(v)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Keyframe {
    pub time: f32,
    pub value: AnimValue,
    #[serde(default)]
    pub easing: Easing,
    #[serde(default)]
    pub bezier: Option<[f32; 4]>,
}

fn default_anim_zero() -> AnimValue {
    AnimValue::Number(0.0)
}

#[derive(Debug, Clone, Deserialize)]
pub struct Animation {
    pub property: AnimProperty,
    #[serde(default)]
    pub expr: Option<String>,
    #[serde(default = "default_anim_zero")]
    pub from: AnimValue,
    #[serde(default = "default_anim_zero")]
    pub to: AnimValue,
    #[serde(default)]
    pub start_sec: f32,
    #[serde(default)]
    pub end_sec: f32,
    #[serde(default)]
    pub keyframes: Vec<Keyframe>,
    #[serde(default)]
    pub easing: Easing,
    #[serde(default)]
    pub bezier: Option<[f32; 4]>,
    #[serde(default)]
    pub spring: Option<SpringConfig>,
    #[serde(default)]
    pub repeat: Option<RepeatSpec>,
    #[serde(default)]
    pub yoyo: bool,
}

/// Canonical snake_case names for every [`AnimProperty`] variant, using the
/// same spelling the serializer emits. Used to keep `schema.json` in sync.
#[allow(dead_code)]
pub const ANIM_PROPERTY_NAMES: &[&str] = &[
    "volume",
    "x",
    "y",
    "width",
    "height",
    "opacity",
    "scale",
    "scale_x",
    "scale_y",
    "focal_x",
    "focal_y",
    "rotation",
    "skew_x",
    "skew_y",
    "rotate_x",
    "rotate_y",
    "rotate_z",
    "perspective",
    "z_index",
    "cx",
    "cy",
    "r",
    "radius",
    "color",
    "border_color",
    "path_progress",
    "progress",
    "stroke_dashoffset",
];

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnimProperty {
    Volume,
    X,
    Y,
    Width,
    Height,
    Opacity,
    Scale,
    ScaleX,
    ScaleY,
    FocalX,
    FocalY,
    Rotation,
    SkewX,
    SkewY,
    RotateX,
    RotateY,
    RotateZ,
    Perspective,
    #[serde(alias = "z")]
    ZIndex,
    Cx,
    Cy,
    R,
    Radius,
    #[serde(alias = "background")]
    Color,
    BorderColor,
    PathProgress,
    Progress,
    StrokeDashoffset,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    EaseInExpo,
    EaseOutExpo,
    EaseInOutExpo,
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,
    EaseOutBounce,
    CubicBezier,
    Spring,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextDirectionSpec {
    #[default]
    Auto,
    Ltr,
    Rtl,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignSpec {
    #[default]
    Left,
    Center,
    Right,
    Start,
    End,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontWeightSpec {
    Thin,
    ExtraLight,
    Light,
    #[default]
    Normal,
    Medium,
    SemiBold,
    Bold,
    ExtraBold,
    Black,
}

impl FontWeightSpec {
    pub fn to_u16(self) -> u16 {
        match self {
            FontWeightSpec::Thin => 100,
            FontWeightSpec::ExtraLight => 200,
            FontWeightSpec::Light => 300,
            FontWeightSpec::Normal => 400,
            FontWeightSpec::Medium => 500,
            FontWeightSpec::SemiBold => 600,
            FontWeightSpec::Bold => 700,
            FontWeightSpec::ExtraBold => 800,
            FontWeightSpec::Black => 900,
        }
    }

    pub fn from_u16(n: u16) -> Option<Self> {
        match n {
            100 => Some(FontWeightSpec::Thin),
            200 => Some(FontWeightSpec::ExtraLight),
            300 => Some(FontWeightSpec::Light),
            400 => Some(FontWeightSpec::Normal),
            500 => Some(FontWeightSpec::Medium),
            600 => Some(FontWeightSpec::SemiBold),
            700 => Some(FontWeightSpec::Bold),
            800 => Some(FontWeightSpec::ExtraBold),
            900 => Some(FontWeightSpec::Black),
            _ => None,
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "thin" | "100" => Some(FontWeightSpec::Thin),
            "extralight" | "extra_light" | "extra-light" | "200" => {
                Some(FontWeightSpec::ExtraLight)
            }
            "light" | "300" => Some(FontWeightSpec::Light),
            "normal" | "regular" | "400" => Some(FontWeightSpec::Normal),
            "medium" | "500" => Some(FontWeightSpec::Medium),
            "semibold" | "semi_bold" | "semi-bold" | "600" => Some(FontWeightSpec::SemiBold),
            "bold" | "700" => Some(FontWeightSpec::Bold),
            "extrabold" | "extra_bold" | "extra-bold" | "800" => Some(FontWeightSpec::ExtraBold),
            "black" | "heavy" | "900" => Some(FontWeightSpec::Black),
            _ => None,
        }
    }
}

impl<'de> serde::Deserialize<'de> for FontWeightSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct FontWeightVisitor;
        impl<'de> serde::de::Visitor<'de> for FontWeightVisitor {
            type Value = FontWeightSpec;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(
                    f,
                    "a font weight name (thin..black) or numeric weight (100..900)"
                )
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                FontWeightSpec::from_str_loose(v)
                    .ok_or_else(|| E::custom(format!("invalid font_weight '{v}'")))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                u16::try_from(v)
                    .ok()
                    .and_then(FontWeightSpec::from_u16)
                    .ok_or_else(|| E::custom(format!("invalid font_weight {v}")))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                u16::try_from(v)
                    .ok()
                    .and_then(FontWeightSpec::from_u16)
                    .ok_or_else(|| E::custom(format!("invalid font_weight {v}")))
            }
        }
        deserializer.deserialize_any(FontWeightVisitor)
    }
}
