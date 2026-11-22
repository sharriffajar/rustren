use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Scene {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    pub canvas: Canvas,
    #[serde(default)]
    pub profiles: std::collections::HashMap<String, CompositionProfile>,
    #[serde(default)]
    pub fonts: Vec<String>,
    #[serde(default)]
    pub audio: Vec<AudioTrack>,
    pub layers: Vec<Layer>,
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
    pub shadow: Option<ShadowSpec>,
    #[serde(default)]
    pub anchor: AnchorSpec,
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
    pub vignette: f32,
    #[serde(default)]
    pub halftone: f32,
    #[serde(default)]
    pub track_matte: TrackMatteMode,
    #[serde(default)]
    pub matte_layer: Option<Box<Layer>>,
    #[serde(default)]
    pub echo: Option<EchoSpec>,
    #[serde(flatten)]
    pub kind: LayerKind,
    #[serde(default)]
    pub animations: Vec<Animation>,
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

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct EchoSpec {
    #[serde(default = "default_echo_count")]
    pub count: u32,
    #[serde(default = "default_echo_step")]
    pub step_sec: f32,
    #[serde(default = "default_echo_decay")]
    pub decay: f32,
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

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FontWeightSpec {
    #[default]
    Normal,
    Bold,
}
