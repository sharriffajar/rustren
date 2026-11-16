use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Scene {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    pub canvas: Canvas,
    #[serde(default)]
    pub fonts: Vec<String>,
    #[serde(default)]
    pub audio: Vec<AudioTrack>,
    pub layers: Vec<Layer>,
}

fn default_schema_version() -> String {
    "1.0".to_string()
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
    #[serde(flatten)]
    pub kind: LayerKind,
    #[serde(default)]
    pub animations: Vec<Animation>,
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
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LayoutSpec {
    #[serde(default)]
    pub direction: FlexDirectionSpec,
    #[serde(default)]
    pub justify_content: Option<AlignSpec>,
    #[serde(default)]
    pub align_items: Option<AlignSpec>,
    #[serde(default)]
    pub gap: f32,
    #[serde(default)]
    pub padding: f32,
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

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignSpec {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FontWeightSpec {
    #[default]
    Normal,
    Bold,
}
