use anyhow::{bail, Result};
use serde::Serialize;

use crate::schema::Scene;

pub const RENDERER_VERSION: &str = "1.1";
pub const SCHEMA_VERSION: &str = "1.1";
pub const MIN_SCHEMA_VERSION: &str = "1.0";

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityRegistry {
    pub renderer_version: &'static str,
    pub schema_version: &'static str,
    pub min_schema_version: &'static str,
    pub capabilities: Capabilities,
    pub http_api: HttpApiCapabilities,
}

#[derive(Debug, Clone, Serialize)]
pub struct HttpApiCapabilities {
    pub snapshot: &'static str,
    pub layout: &'static str,
    pub validate: &'static str,
    pub jobs: &'static str,
    pub discovery: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub elements: ElementCapabilities,
    pub styles: StyleCapabilities,
    pub layout: LayoutCapabilities,
    pub animation: AnimationCapabilities,
    pub timeline: TimelineCapabilities,
    pub effects: EffectCapabilities,
    pub typography: TypographyCapabilities,
    pub media: MediaCapabilities,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineCapabilities {
    pub global_absolute: bool,
    pub duration_secs: bool,
    pub local_timeline: bool,
    pub series_sequencing: bool,
    pub transition_overlap: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ElementCapabilities {
    pub container: bool,
    pub rect: bool,
    pub circle: bool,
    pub text: bool,
    pub svg: bool,
    pub image: bool,
    pub video: bool,
    pub waveform: bool,
    pub lottie: bool,
    pub particles: bool,
    #[cfg(feature = "exp-3d")]
    pub mesh3d: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StyleCapabilities {
    pub solid_color: bool,
    pub linear_gradient: bool,
    pub border: bool,
    pub border_radius: bool,
    pub opacity: bool,
    pub cascade_opacity: bool,
    pub clip: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayoutCapabilities {
    pub flexbox: bool,
    pub auto_size_container: bool,
    pub absolute_positioning: bool,
    pub flex_grow: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnimationCapabilities {
    pub properties: &'static [&'static str],
    pub easings: &'static [&'static str],
    pub custom_spring: bool,
    pub repeat: bool,
    pub yoyo: bool,
    pub color_interpolation: bool,
    pub path_animation: bool,
    pub stroke_animation: bool,
    pub cubic_bezier: bool,
    pub expressions: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectCapabilities {
    pub blur: bool,
    pub box_shadow: bool,
    pub blend_mode: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TypographyCapabilities {
    pub custom_fonts: bool,
    pub multiline_wrap: bool,
    pub rich_text: bool,
    pub letter_spacing: bool,
    pub rtl_direction: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MediaCapabilities {
    pub audio: bool,
    pub video: bool,
}

impl CapabilityRegistry {
    pub fn current() -> Self {
        Self {
            renderer_version: RENDERER_VERSION,
            schema_version: SCHEMA_VERSION,
            min_schema_version: MIN_SCHEMA_VERSION,
            capabilities: Capabilities {
                elements: ElementCapabilities {
                    container: true,
                    rect: true,
                    circle: true,
                    text: true,
                    svg: true,
                    image: true,
                    video: true,
                    waveform: true,
                    lottie: true,
                    particles: true,
                    #[cfg(feature = "exp-3d")]
                    mesh3d: true,
                },
                styles: StyleCapabilities {
                    solid_color: true,
                    linear_gradient: true,
                    border: true,
                    border_radius: true,
                    opacity: true,
                    cascade_opacity: true,
                    clip: true,
                },
                layout: LayoutCapabilities {
                    flexbox: true,
                    auto_size_container: true,
                    absolute_positioning: true,
                    flex_grow: true,
                },
                animation: AnimationCapabilities {
                    properties: &[
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
                        "volume",
                        "cx",
                        "cy",
                        "r",
                        "radius",
                        "color",
                        "border_color",
                        "path_progress",
                        "stroke_dashoffset",
                    ],
                    easings: &[
                        "linear",
                        "ease_in",
                        "ease_out",
                        "ease_in_out",
                        "ease_in_quad",
                        "ease_out_quad",
                        "ease_in_out_quad",
                        "ease_in_expo",
                        "ease_out_expo",
                        "ease_in_out_expo",
                        "ease_in_back",
                        "ease_out_back",
                        "ease_in_out_back",
                        "ease_out_bounce",
                        "cubic_bezier",
                        "spring",
                    ],
                    custom_spring: true,
                    repeat: true,
                    yoyo: true,
                    color_interpolation: true,
                    path_animation: true,
                    stroke_animation: true,
                    cubic_bezier: true,
                    expressions: true,
                },
                timeline: TimelineCapabilities {
                    global_absolute: true,
                    duration_secs: true,
                    local_timeline: true,
                    series_sequencing: true,
                    transition_overlap: true,
                },
                effects: EffectCapabilities {
                    blur: true,
                    box_shadow: true,
                    blend_mode: true,
                },
                typography: TypographyCapabilities {
                    custom_fonts: true,
                    multiline_wrap: true,
                    rich_text: true,
                    letter_spacing: true,
                    rtl_direction: true,
                },
                media: MediaCapabilities {
                    audio: true,
                    video: true,
                },
            },
            http_api: HttpApiCapabilities {
                snapshot: "POST /snapshot — JSON {\"scene\", \"time\"?, \"times\"?, \"sheet\"?, \"cols\"?} -> image/png (max 16 timestamps)",
                layout: "POST /layout — JSON {\"scene\", \"time\"?} -> JSON {\"time\", \"layers\", \"diagnostics\"}",
                validate: "POST /validate — Scene JSON -> JSON {\"valid\", \"warnings\"}",
                jobs: "POST /jobs -> 202 {\"job_id\"}, GET /jobs/:id -> status, GET /jobs/:id/video -> video/mp4",
                discovery: "GET / and GET /llms.txt -> Markdown API guide",
            },
        }
    }
}

fn parse_major_minor(v: &str) -> Result<(u32, u32)> {
    let mut parts = v.trim().split('.');
    let major: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Invalid schema_version format '{v}' (expected 'major.minor', e.g. '1.1')"))?;
    let minor: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("Invalid schema_version format '{v}' (expected 'major.minor', e.g. '1.1')"))?;
    if parts.next().is_some() {
        bail!("Invalid schema_version format '{v}' (expected 'major.minor', e.g. '1.1')");
    }
    Ok((major, minor))
}

pub fn check_schema_version(version: &str) -> Result<()> {
    let req = parse_major_minor(version)?;
    let min = parse_major_minor(MIN_SCHEMA_VERSION)?;
    let max = parse_major_minor(SCHEMA_VERSION)?;
    if req < min || req > max {
        bail!(
            "Unsupported schema_version '{version}'. This renderer (v{RENDERER_VERSION}) supports schema_version {MIN_SCHEMA_VERSION} through {SCHEMA_VERSION}."
        );
    }
    Ok(())
}

pub fn validate_scene_capabilities(scene: &Scene) -> Result<()> {
    check_schema_version(&scene.schema_version)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_version_compatibility() {
        assert!(check_schema_version("1.0").is_ok());
        assert!(check_schema_version("1.1").is_ok());
        assert!(check_schema_version("1.2").is_err());
        assert!(check_schema_version("2.0").is_err());
        assert!(check_schema_version("0.9").is_err());
        assert!(check_schema_version("invalid").is_err());
    }
}
