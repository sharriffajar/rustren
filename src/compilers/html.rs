//! HTML -> CompiledScene compiler.
//!
//! Mengonversi file HTML deklaratif menjadi `CompiledScene::Static(Scene)`.
//! Mendukung tag:
//! - `<scene>`: atribut `width`, `height`, `fps`, `duration`, `background`, `schema-version`
//! - `<profiles>`: tag anak `<profile name="...">`
//! - `<fonts>`: tag anak `<font src="...">`
//! - `<audio-tracks>`: tag anak `<audio src="..." start="0" volume="1" ...>`
//! - `<rect>`: layer rect (x, y, width, height, radius, color, border-color, border-width, opacity, scale, rotation)
//! - `<circle>`: layer circle (cx, cy, r, color, border-color, border-width, opacity, scale)
//! - `<text>`: layer text (x, y, content atau text body, font-size, line-height, letter-spacing, font-family, font-weight, text-align, direction, color, opacity, scale)
//! - `<image>`: layer image (x, y, width, height, src, fit, opacity, rotation, scale)
//! - `<svg>`: layer svg (x, y, width, height, src, path-progress, stroke-dashoffset, opacity, rotation)
//! - `<video>`: layer video (x, y, width, height, src, fit, trim-start, trim-end, playback-rate, volume, muted, opacity, scale)
//! - `<waveform>`: layer waveform (x, y, width, height, bars, bar-gap, color, seed, speed, opacity, scale)
//! - `<lottie>`: layer lottie (x, y, width, height, src, loop, speed, opacity, scale)
//! - `<container>`: layer container (x, y, width, height, layout direction, justify-content, align-items, gap, padding, background, timeline, children)
//! - `<animate>` / `<animation>`: child tags di dalam layer untuk menganimasikan properti

use anyhow::{Context, Result};
use scraper::{ElementRef, Html, Selector};
use std::collections::HashMap;
use std::path::Path;

use super::Compiler;
use crate::ir::CompiledScene;
use crate::schema::*;

pub struct HtmlCompiler;

const MAX_INCLUDE_DEPTH: usize = 8;

/// Substitutes `{{key}}` and `{{key|default}}` placeholders in a template string.
pub fn substitute_variables(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("}}") {
            let expr = after_open[..end].trim();
            if let Some((key, default_val)) = expr.split_once('|') {
                let key = key.trim();
                let val = vars.get(key).map(|s| s.as_str()).unwrap_or(default_val.trim());
                result.push_str(val);
            } else if let Some(val) = vars.get(expr) {
                result.push_str(val);
            } else {
                // Keep unresolved placeholder as-is
                result.push_str("{{");
                result.push_str(&after_open[..end]);
                result.push_str("}}");
            }
            rest = &after_open[end + 2..];
        } else {
            result.push_str("{{");
            rest = after_open;
        }
    }
    result.push_str(rest);
    result
}

impl Compiler for HtmlCompiler {
    fn compile(&self, path: &Path) -> Result<CompiledScene> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Gagal membaca file HTML: {:?}", path))?;

        let scene = parse_html_scene_with_base(&content, path.parent())?;
        Ok(CompiledScene::Static(scene))
    }

    fn format_name(&self) -> &'static str {
        "html"
    }
}

pub fn parse_html_scene(html_content: &str) -> Result<Scene> {
    parse_html_scene_with_base(html_content, None)
}

pub fn parse_html_scene_with_base(html_content: &str, base_dir: Option<&Path>) -> Result<Scene> {
    parse_html_scene_internal(html_content, base_dir, 0, &HashMap::new())
}

fn parse_html_scene_internal(
    html_content: &str,
    base_dir: Option<&Path>,
    depth: usize,
    vars: &HashMap<String, String>,
) -> Result<Scene> {
    let resolved_content = if vars.is_empty() {
        html_content.to_string()
    } else {
        substitute_variables(html_content, vars)
    };
    let document = Html::parse_document(&resolved_content);

    // Cari tag <scene> utama, jika tidak ada, coba cari di <component>, <body> atau root
    let scene_sel = Selector::parse("scene").unwrap();
    let comp_sel = Selector::parse("component").unwrap();
    let body_sel = Selector::parse("body").unwrap();

    let root_elem = document
        .select(&scene_sel)
        .next()
        .or_else(|| document.select(&comp_sel).next())
        .or_else(|| document.select(&body_sel).next())
        .ok_or_else(|| anyhow::anyhow!("Tag <scene>, <component>, atau <body> tidak ditemukan di dokumen HTML"))?;

    // 1. Canvas
    let width = get_attr_f32(&root_elem, "width").unwrap_or(1280.0) as u32;
    let height = get_attr_f32(&root_elem, "height").unwrap_or(720.0) as u32;
    let fps = get_attr_f32(&root_elem, "fps").unwrap_or(30.0) as u32;
    let duration_secs = get_attr_f32(&root_elem, "duration")
        .or_else(|| get_attr_f32(&root_elem, "duration-secs"))
        .or_else(|| get_attr_f32(&root_elem, "duration_secs"))
        .unwrap_or(5.0);
    let background = get_attr_str(&root_elem, "background")
        .or_else(|| get_attr_str(&root_elem, "bg"))
        .unwrap_or_else(|| "#000000".to_string());

    let canvas = Canvas {
        width,
        height,
        fps,
        duration_secs,
        background,
    };

    let schema_version = get_attr_str(&root_elem, "schema-version")
        .or_else(|| get_attr_str(&root_elem, "version"))
        .unwrap_or_else(|| "1.1".to_string());

    // 2. Profiles
    let mut profiles = HashMap::new();
    let profile_sel = Selector::parse("profiles > profile, profile").unwrap();
    for prof_elem in root_elem.select(&profile_sel) {
        if let Some(name) = get_attr_str(&prof_elem, "name").or_else(|| get_attr_str(&prof_elem, "id")) {
            let p_width = get_attr_f32(&prof_elem, "width").map(|v| v as u32);
            let p_height = get_attr_f32(&prof_elem, "height").map(|v| v as u32);
            let p_fps = get_attr_f32(&prof_elem, "fps").map(|v| v as u32);
            let p_dur = get_attr_f32(&prof_elem, "duration").or_else(|| get_attr_f32(&prof_elem, "duration-secs"));
            let p_bg = get_attr_str(&prof_elem, "background");
            profiles.insert(
                name,
                CompositionProfile {
                    width: p_width,
                    height: p_height,
                    fps: p_fps,
                    duration_secs: p_dur,
                    background: p_bg,
                },
            );
        }
    }

    // 3. Fonts
    let mut fonts = Vec::new();
    let font_sel = Selector::parse("fonts > font, font").unwrap();
    for font_elem in root_elem.select(&font_sel) {
        if let Some(src) = get_attr_str(&font_elem, "src") {
            fonts.push(src);
        }
    }

    // 4. Audio
    let mut audio = Vec::new();
    let audio_sel = Selector::parse("audio-tracks > audio, audio").unwrap();
    for a_elem in root_elem.select(&audio_sel) {
        if let Some(src) = get_attr_str(&a_elem, "src") {
            let start_sec = get_attr_f32(&a_elem, "start").or_else(|| get_attr_f32(&a_elem, "start-sec")).unwrap_or(0.0);
            let volume = get_attr_f32(&a_elem, "volume").unwrap_or(1.0);
            let trim_start_sec = get_attr_f32(&a_elem, "trim-start").or_else(|| get_attr_f32(&a_elem, "trim-start-sec"));
            let trim_end_sec = get_attr_f32(&a_elem, "trim-end").or_else(|| get_attr_f32(&a_elem, "trim-end-sec"));
            let playback_rate = get_attr_f32(&a_elem, "playback-rate").or_else(|| get_attr_f32(&a_elem, "speed")).unwrap_or(1.0);
            let fade_in_secs = get_attr_f32(&a_elem, "fade-in").or_else(|| get_attr_f32(&a_elem, "fade-in-secs")).unwrap_or(0.0);
            let fade_out_secs = get_attr_f32(&a_elem, "fade-out").or_else(|| get_attr_f32(&a_elem, "fade-out-secs")).unwrap_or(0.0);

            let animations = parse_animations(&a_elem)?;

            audio.push(AudioTrack {
                src,
                start_sec,
                volume,
                trim_start_sec,
                trim_end_sec,
                playback_rate,
                fade_in_secs,
                fade_out_secs,
                animations,
            });
        }
    }

    // 5. Layers (with <include> support)
    let layers = parse_children_layers(&root_elem, base_dir, depth, vars)?;

    Ok(Scene {
        schema_version,
        canvas,
        profiles,
        fonts,
        audio,
        layers,
    })
}

fn parse_children_layers(
    parent: &ElementRef,
    base_dir: Option<&Path>,
    depth: usize,
    parent_vars: &HashMap<String, String>,
) -> Result<Vec<Layer>> {
    let mut layers = Vec::new();
    for child in parent.children() {
        if let Some(el) = ElementRef::wrap(child) {
            let tag = el.value().name();
            if tag == "profile"
                || tag == "profiles"
                || tag == "font"
                || tag == "fonts"
                || tag == "audio"
                || tag == "audio-tracks"
                || tag == "script"
                || tag == "style"
                || tag == "animate"
                || tag == "animation"
            {
                continue;
            }
            if tag == "include" {
                let src = get_attr_str(&el, "src").ok_or_else(|| {
                    anyhow::anyhow!("Tag <include> wajib memiliki atribut 'src'")
                })?;
                if depth >= MAX_INCLUDE_DEPTH {
                    anyhow::bail!(
                        "Max include depth ({}) exceeded while including '{}'",
                        MAX_INCLUDE_DEPTH,
                        src
                    );
                }
                let inc_path = match base_dir {
                    Some(dir) => dir.join(&src),
                    None => Path::new(&src).to_path_buf(),
                };
                let inc_content = std::fs::read_to_string(&inc_path).with_context(|| {
                    format!("Gagal membaca file <include src=\"{}\"> ({:?})", src, inc_path)
                })?;

                let mut child_vars = parent_vars.clone();
                for (k, v) in el.value().attrs() {
                    if k != "src" {
                        child_vars.insert(k.to_string(), v.to_string());
                    }
                }

                let inc_scene = parse_html_scene_internal(
                    &inc_content,
                    inc_path.parent(),
                    depth + 1,
                    &child_vars,
                )?;
                layers.extend(inc_scene.layers);
                // In HTML5, custom tags like <include ... /> are not void elements,
                // so following siblings may be nested inside `el`. Parse them too.
                let nested_siblings = parse_children_layers(&el, base_dir, depth, parent_vars)?;
                layers.extend(nested_siblings);
                continue;
            }
            if let Some(layer) = parse_layer_element(&el, base_dir, depth, parent_vars)? {
                layers.push(layer);
            }
        }
    }
    Ok(layers)
}

fn parse_layer_element(
    elem: &ElementRef,
    base_dir: Option<&Path>,
    depth: usize,
    vars: &HashMap<String, String>,
) -> Result<Option<Layer>> {
    let tag = elem.value().name();

    let name = get_attr_str(elem, "name").or_else(|| get_attr_str(elem, "id"));
    let start_sec = get_attr_f32(elem, "start").or_else(|| get_attr_f32(elem, "start-sec")).unwrap_or(0.0);
    let end_sec = get_attr_f32(elem, "end").or_else(|| get_attr_f32(elem, "end-sec"));
    let duration_secs = get_attr_f32(elem, "duration").or_else(|| get_attr_f32(elem, "duration-secs"));
    let overlap_secs = get_attr_f32(elem, "overlap").or_else(|| get_attr_f32(elem, "overlap-secs")).unwrap_or(0.0);

    let position = match get_attr_str(elem, "position").as_deref() {
        Some("relative") => PositionSpec::Relative,
        _ => PositionSpec::Absolute,
    };
    let flex_grow = get_attr_f32(elem, "flex-grow");
    let margin = get_attr_f32(elem, "margin");

    let blend_mode = match get_attr_str(elem, "blend-mode").as_deref() {
        Some("multiply") => BlendModeSpec::Multiply,
        Some("screen") => BlendModeSpec::Screen,
        Some("overlay") => BlendModeSpec::Overlay,
        Some("darken") => BlendModeSpec::Darken,
        Some("lighten") => BlendModeSpec::Lighten,
        Some("color_dodge") | Some("color-dodge") => BlendModeSpec::ColorDodge,
        Some("color_burn") | Some("color-burn") => BlendModeSpec::ColorBurn,
        Some("hard_light") | Some("hard-light") => BlendModeSpec::HardLight,
        Some("soft_light") | Some("soft-light") => BlendModeSpec::SoftLight,
        Some("difference") => BlendModeSpec::Difference,
        Some("exclusion") => BlendModeSpec::Exclusion,
        _ => BlendModeSpec::Normal,
    };

    let blur = get_attr_f32(elem, "blur").unwrap_or(0.0);

    let shadow = if let Some(s_color) = get_attr_str(elem, "shadow-color") {
        Some(ShadowSpec {
            color: s_color,
            blur: get_attr_f32(elem, "shadow-blur").unwrap_or(4.0),
            offset_x: get_attr_f32(elem, "shadow-x").unwrap_or(0.0),
            offset_y: get_attr_f32(elem, "shadow-y").unwrap_or(0.0),
        })
    } else {
        None
    };

    let anchor = parse_anchor(get_attr_str(elem, "anchor").as_deref());
    let skew_x = get_attr_f32(elem, "skew-x").or_else(|| get_attr_f32(elem, "skew_x")).unwrap_or(0.0);
    let echo = get_attr_f32(elem, "echo-count")
        .or_else(|| get_attr_f32(elem, "echo_count"))
        .map(|cnt| EchoSpec {
            count: cnt.max(0.0) as u32,
            step_sec: get_attr_f32(elem, "echo-step")
                .or_else(|| get_attr_f32(elem, "echo_step"))
                .unwrap_or(0.03),
            decay: get_attr_f32(elem, "echo-decay")
                .or_else(|| get_attr_f32(elem, "echo_decay"))
                .unwrap_or(0.65),
        });
    let track_matte = match get_attr_str(elem, "track-matte")
        .or_else(|| get_attr_str(elem, "track_matte"))
        .as_deref()
    {
        Some("alpha") => TrackMatteMode::Alpha,
        Some("alpha_inverted") | Some("alpha-inverted") => TrackMatteMode::AlphaInverted,
        Some("luma") => TrackMatteMode::Luma,
        Some("luma_inverted") | Some("luma-inverted") => TrackMatteMode::LumaInverted,
        _ => TrackMatteMode::None,
    };
    let mut matte_layer = None;
    for c in elem.children() {
        if let Some(c_el) = ElementRef::wrap(c) {
            if c_el.value().name() == "matte" {
                let matte_children = parse_children_layers(&c_el, base_dir, depth, vars)?;
                if let Some(first) = matte_children.into_iter().next() {
                    matte_layer = Some(Box::new(first));
                }
                break;
            }
        }
    }
    let skew_y = get_attr_f32(elem, "skew-y").or_else(|| get_attr_f32(elem, "skew_y")).unwrap_or(0.0);

    let animations = parse_animations(elem)?;

    let kind = match tag {
        "rect" | "div" if get_attr_str(elem, "type").as_deref() == Some("rect") || tag == "rect" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width").or_else(|| get_attr_f32(elem, "w")).unwrap_or(100.0);
            let height = get_attr_f32(elem, "height").or_else(|| get_attr_f32(elem, "h")).unwrap_or(100.0);
            let radius = get_attr_f32(elem, "radius").or_else(|| get_attr_f32(elem, "border-radius")).unwrap_or(0.0);
            let color = get_attr_str(elem, "color").or_else(|| get_attr_str(elem, "background")).unwrap_or_else(|| "#FFFFFF".to_string());
            let border_color = get_attr_str(elem, "border-color");
            let border_width = get_attr_f32(elem, "border-width").unwrap_or(0.0);
            let path_progress = get_attr_f32(elem, "path-progress").unwrap_or(1.0);
            let stroke_dashoffset = get_attr_f32(elem, "stroke-dashoffset");
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);
            let rotation = get_attr_f32(elem, "rotation").unwrap_or(0.0);

            LayerKind::Rect {
                x,
                y,
                width,
                height,
                radius,
                color,
                gradient: None,
                border_color,
                border_width,
                path_progress,
                stroke_dashoffset,
                opacity,
                scale,
                rotation,
            }
        }
        "circle" => {
            let cx = get_attr_f32(elem, "cx").or_else(|| get_attr_f32(elem, "x")).unwrap_or(0.0);
            let cy = get_attr_f32(elem, "cy").or_else(|| get_attr_f32(elem, "y")).unwrap_or(0.0);
            let r = get_attr_f32(elem, "r").or_else(|| get_attr_f32(elem, "radius")).unwrap_or(50.0);
            let color = get_attr_str(elem, "color").or_else(|| get_attr_str(elem, "background")).unwrap_or_else(|| "#FFFFFF".to_string());
            let border_color = get_attr_str(elem, "border-color");
            let border_width = get_attr_f32(elem, "border-width").unwrap_or(0.0);
            let path_progress = get_attr_f32(elem, "path-progress").unwrap_or(1.0);
            let stroke_dashoffset = get_attr_f32(elem, "stroke-dashoffset");
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);

            LayerKind::Circle {
                cx,
                cy,
                r,
                color,
                border_color,
                border_width,
                path_progress,
                stroke_dashoffset,
                opacity,
                scale,
            }
        }
        "text" | "p" | "span" | "h1" | "h2" | "h3" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let mut content = get_attr_str(elem, "content").unwrap_or_default();
            if content.is_empty() {
                // Ambil text langsung dari child nodes
                let inner: String = elem.text().collect();
                content = inner.trim().to_string();
            }
            let font_size = get_attr_f32(elem, "font-size").or_else(|| get_attr_f32(elem, "size")).unwrap_or(48.0);
            let line_height = get_attr_f32(elem, "line-height");
            let letter_spacing = get_attr_f32(elem, "letter-spacing").unwrap_or(0.0);
            let font_family = get_attr_str(elem, "font-family").or_else(|| get_attr_str(elem, "font"));
            let font_weight = match get_attr_str(elem, "font-weight").as_deref() {
                Some("bold") => FontWeightSpec::Bold,
                _ => FontWeightSpec::Normal,
            };
            let text_align = match get_attr_str(elem, "text-align").or_else(|| get_attr_str(elem, "align")).as_deref() {
                Some("center") => TextAlignSpec::Center,
                Some("right") => TextAlignSpec::Right,
                Some("start") => TextAlignSpec::Start,
                Some("end") => TextAlignSpec::End,
                _ => TextAlignSpec::Left,
            };
            let direction = match get_attr_str(elem, "direction").as_deref() {
                Some("ltr") => TextDirectionSpec::Ltr,
                Some("rtl") => TextDirectionSpec::Rtl,
                _ => TextDirectionSpec::Auto,
            };
            let color = get_attr_str(elem, "color").unwrap_or_else(|| "#FFFFFF".to_string());
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let max_width = get_attr_f32(elem, "max-width").or_else(|| get_attr_f32(elem, "width"));
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);

            LayerKind::Text {
                x,
                y,
                content,
                dynamic: None,
                spans: Vec::new(),
                font_size,
                line_height,
                letter_spacing,
                font_family,
                font_weight,
                text_align,
                direction,
                color,
                gradient: None,
                outline: None,
                opacity,
                max_width,
                scale,
            }
        }
        "image" | "img" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width");
            let height = get_attr_f32(elem, "height");
            let src = get_attr_str(elem, "src").unwrap_or_default();
            let fit = parse_fit(get_attr_str(elem, "fit").as_deref());
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let rotation = get_attr_f32(elem, "rotation").unwrap_or(0.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);

            LayerKind::Image {
                x,
                y,
                width,
                height,
                src,
                fit,
                opacity,
                rotation,
                scale,
            }
        }
        "svg" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width");
            let height = get_attr_f32(elem, "height");
            let src = get_attr_str(elem, "src").unwrap_or_default();
            let path_progress = get_attr_f32(elem, "path-progress").unwrap_or(1.0);
            let stroke_dashoffset = get_attr_f32(elem, "stroke-dashoffset");
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let rotation = get_attr_f32(elem, "rotation").unwrap_or(0.0);

            LayerKind::Svg {
                x,
                y,
                width,
                height,
                src,
                path_progress,
                stroke_dashoffset,
                stroke_dasharray: None,
                opacity,
                rotation,
            }
        }
        "video" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width");
            let height = get_attr_f32(elem, "height");
            let src = get_attr_str(elem, "src").unwrap_or_default();
            let fit = parse_fit(get_attr_str(elem, "fit").as_deref());
            let trim_start_sec = get_attr_f32(elem, "trim-start").unwrap_or(0.0);
            let trim_end_sec = get_attr_f32(elem, "trim-end");
            let playback_rate = get_attr_f32(elem, "playback-rate").unwrap_or(1.0);
            let volume = get_attr_f32(elem, "volume").unwrap_or(1.0);
            let muted = get_attr_str(elem, "muted").is_some();
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let rotation = get_attr_f32(elem, "rotation").unwrap_or(0.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);

            LayerKind::Video {
                x,
                y,
                width,
                height,
                src,
                fit,
                trim_start_sec,
                trim_end_sec,
                playback_rate,
                volume,
                muted,
                opacity,
                rotation,
                scale,
            }
        }
        "waveform" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width").unwrap_or(200.0);
            let height = get_attr_f32(elem, "height").unwrap_or(60.0);
            let bars = get_attr_f32(elem, "bars").unwrap_or(24.0) as usize;
            let bar_gap = get_attr_f32(elem, "bar-gap").unwrap_or(4.0);
            let color = get_attr_str(elem, "color").unwrap_or_else(|| "#FFFFFF".to_string());
            let seed = get_attr_f32(elem, "seed").unwrap_or(42.0) as u64;
            let speed = get_attr_f32(elem, "speed").unwrap_or(1.0);
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let rotation = get_attr_f32(elem, "rotation").unwrap_or(0.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);

            LayerKind::Waveform {
                x,
                y,
                width,
                height,
                bars,
                bar_gap,
                color,
                seed,
                speed,
                opacity,
                rotation,
                scale,
            }
        }
        "lottie" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width");
            let height = get_attr_f32(elem, "height");
            let src = get_attr_str(elem, "src").unwrap_or_default();
            let loop_animation = get_attr_str(elem, "loop").is_some();
            let speed = get_attr_f32(elem, "speed").unwrap_or(1.0);
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let rotation = get_attr_f32(elem, "rotation").unwrap_or(0.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);

            LayerKind::Lottie {
                x,
                y,
                width,
                height,
                src,
                loop_animation,
                speed,
                opacity,
                rotation,
                scale,
            }
        }
        "container" | "div" | "section" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width");
            let height = get_attr_f32(elem, "height");
            let background = get_attr_str(elem, "background").or_else(|| get_attr_str(elem, "color"));
            let border_color = get_attr_str(elem, "border-color");
            let border_width = get_attr_f32(elem, "border-width").unwrap_or(0.0);
            let radius = get_attr_f32(elem, "radius").or_else(|| get_attr_f32(elem, "border-radius")).unwrap_or(0.0);
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let rotation = get_attr_f32(elem, "rotation").unwrap_or(0.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);
            let clip = get_attr_str(elem, "clip").is_some() || get_attr_str(elem, "overflow").as_deref() == Some("hidden");
            let cascade_opacity = get_attr_str(elem, "cascade-opacity").is_some() || get_attr_str(elem, "cascade_opacity").is_some();
            let stagger_secs = get_attr_f32(elem, "stagger").or_else(|| get_attr_f32(elem, "stagger-secs")).unwrap_or(0.0);

            let timeline = match get_attr_str(elem, "timeline").as_deref() {
                Some("local") | Some("parallel") => TimelineMode::Local,
                Some("series") => TimelineMode::Series,
                _ => TimelineMode::Global,
            };

            let layout_dir = match get_attr_str(elem, "layout").or_else(|| get_attr_str(elem, "flex-direction")).as_deref() {
                Some("column") => FlexDirectionSpec::Column,
                _ => FlexDirectionSpec::Row,
            };

            let justify = parse_align(get_attr_str(elem, "justify-content").or_else(|| get_attr_str(elem, "justify")).as_deref());
            let align = parse_align(get_attr_str(elem, "align-items").or_else(|| get_attr_str(elem, "align")).as_deref());
            let gap = get_attr_f32(elem, "gap").unwrap_or(0.0);
            let padding = get_attr_f32(elem, "padding").unwrap_or(0.0);

            let layout = LayoutSpec {
                direction: layout_dir,
                justify_content: justify,
                align_items: align,
                gap,
                padding,
            };

            // Parse children (supports <include> inside containers too)
            let children = parse_children_layers(elem, base_dir, depth, vars)?;

            LayerKind::Container {
                x,
                y,
                width,
                height,
                layout,
                background,
                gradient: None,
                border_color,
                border_width,
                radius,
                opacity,
                rotation,
                scale,
                clip,
                cascade_opacity,
                timeline,
                stagger_secs,
                children,
            }
        }
        _ => return Ok(None),
    };

    Ok(Some(Layer {
        name,
        start_sec,
        end_sec,
        duration_secs,
        overlap_secs,
        position,
        flex_grow,
        margin,
        blend_mode,
        blur,
        shadow,
        anchor,
        skew_x,
        skew_y,
        track_matte,
        matte_layer,
        echo,
        kind,
        animations,
    }))
}

fn parse_animations(elem: &ElementRef) -> Result<Vec<Animation>> {
    let mut animations = Vec::new();
    let anim_sel = Selector::parse("animate, animation").unwrap();

    for a_elem in elem.select(&anim_sel) {
        let prop_str = get_attr_str(&a_elem, "property")
            .or_else(|| get_attr_str(&a_elem, "prop"))
            .unwrap_or_else(|| "opacity".to_string());

        let property = match prop_str.as_str() {
            "x" => AnimProperty::X,
            "y" => AnimProperty::Y,
            "width" | "w" => AnimProperty::Width,
            "height" | "h" => AnimProperty::Height,
            "opacity" => AnimProperty::Opacity,
            "scale" => AnimProperty::Scale,
            "rotation" => AnimProperty::Rotation,
            "skew_x" | "skew-x" => AnimProperty::SkewX,
            "skew_y" | "skew-y" => AnimProperty::SkewY,
            "cx" => AnimProperty::Cx,
            "cy" => AnimProperty::Cy,
            "r" | "radius" => AnimProperty::Radius,
            "color" | "background" => AnimProperty::Color,
            "border_color" | "border-color" => AnimProperty::BorderColor,
            "path_progress" | "path-progress" => AnimProperty::PathProgress,
            "stroke_dashoffset" | "stroke-dashoffset" => AnimProperty::StrokeDashoffset,
            "volume" => AnimProperty::Volume,
            _ => AnimProperty::Opacity,
        };

        let expr = get_attr_str(&a_elem, "expr");
        let from = parse_anim_value(get_attr_str(&a_elem, "from").as_deref());
        let to = parse_anim_value(get_attr_str(&a_elem, "to").as_deref());

        let start_sec = get_attr_f32(&a_elem, "start").or_else(|| get_attr_f32(&a_elem, "start-sec")).unwrap_or(0.0);
        let end_sec = get_attr_f32(&a_elem, "end").or_else(|| get_attr_f32(&a_elem, "end-sec")).unwrap_or(1.0);

        let easing = match get_attr_str(&a_elem, "easing").as_deref() {
            Some("ease_in") | Some("ease-in") => Easing::EaseIn,
            Some("ease_out") | Some("ease-out") => Easing::EaseOut,
            Some("ease_in_out") | Some("ease-in-out") => Easing::EaseInOut,
            Some("ease_in_quad") | Some("ease-in-quad") => Easing::EaseInQuad,
            Some("ease_out_quad") | Some("ease-out-quad") => Easing::EaseOutQuad,
            Some("ease_in_out_quad") | Some("ease-in-out-quad") => Easing::EaseInOutQuad,
            Some("ease_in_expo") | Some("ease-in-expo") => Easing::EaseInExpo,
            Some("ease_out_expo") | Some("ease-out-expo") => Easing::EaseOutExpo,
            Some("ease_in_out_expo") | Some("ease-in-out-expo") => Easing::EaseInOutExpo,
            Some("ease_in_back") | Some("ease-in-back") => Easing::EaseInBack,
            Some("ease_out_back") | Some("ease-out-back") => Easing::EaseOutBack,
            Some("ease_in_out_back") | Some("ease-in-out-back") => Easing::EaseInOutBack,
            Some("ease_out_bounce") | Some("ease-out-bounce") => Easing::EaseOutBounce,
            Some("cubic_bezier") | Some("cubic-bezier") => Easing::CubicBezier,
            Some("spring") => Easing::Spring,
            _ => Easing::Linear,
        };

        let yoyo = get_attr_str(&a_elem, "yoyo").is_some();
        let repeat = match get_attr_str(&a_elem, "repeat").as_deref() {
            Some("infinite") => Some(RepeatSpec::Mode(RepeatMode::Infinite)),
            Some(n) => n.parse::<u32>().ok().map(RepeatSpec::Count),
            None => None,
        };

        animations.push(Animation {
            property,
            expr,
            from,
            to,
            start_sec,
            end_sec,
            keyframes: Vec::new(),
            easing,
            bezier: None,
            spring: None,
            repeat,
            yoyo,
        });
    }

    Ok(animations)
}

fn parse_anim_value(val: Option<&str>) -> AnimValue {
    match val {
        Some(s) => {
            if let Ok(num) = s.parse::<f32>() {
                AnimValue::Number(num)
            } else {
                AnimValue::Color(s.to_string())
            }
        }
        None => AnimValue::Number(0.0),
    }
}

fn parse_fit(val: Option<&str>) -> FitSpec {
    match val {
        Some("contain") => FitSpec::Contain,
        Some("fill") => FitSpec::Fill,
        _ => FitSpec::Cover,
    }
}

fn parse_align(val: Option<&str>) -> Option<AlignSpec> {
    match val {
        Some("start") | Some("flex_start") | Some("flex-start") => Some(AlignSpec::Start),
        Some("center") => Some(AlignSpec::Center),
        Some("end") | Some("flex_end") | Some("flex-end") => Some(AlignSpec::End),
        Some("space_between") | Some("space-between") => Some(AlignSpec::SpaceBetween),
        Some("space_around") | Some("space-around") => Some(AlignSpec::SpaceAround),
        Some("stretch") => Some(AlignSpec::Stretch),
        _ => None,
    }
}

fn parse_anchor(val: Option<&str>) -> AnchorSpec {
    match val {
        Some("top_center") | Some("top-center") => AnchorSpec::TopCenter,
        Some("top_right") | Some("top-right") => AnchorSpec::TopRight,
        Some("center_left") | Some("center-left") => AnchorSpec::CenterLeft,
        Some("center") => AnchorSpec::Center,
        Some("center_right") | Some("center-right") => AnchorSpec::CenterRight,
        Some("bottom_left") | Some("bottom-left") => AnchorSpec::BottomLeft,
        Some("bottom_center") | Some("bottom-center") => AnchorSpec::BottomCenter,
        Some("bottom_right") | Some("bottom-right") => AnchorSpec::BottomRight,
        _ => AnchorSpec::TopLeft,
    }
}

fn get_attr_str(elem: &ElementRef, name: &str) -> Option<String> {
    elem.value().attr(name).map(|s| s.to_string())
}

fn get_attr_f32(elem: &ElementRef, name: &str) -> Option<f32> {
    elem.value().attr(name).and_then(|s| s.parse::<f32>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_html() {
        let html = r##"
        <scene width="1920" height="1080" fps="60" duration="10.0" background="#101010">
            <rect x="100" y="200" width="400" height="300" color="#FF0000" radius="16">
                <animate property="opacity" from="0" to="1" start="0" end="1.5" easing="ease_out" />
            </rect>
            <text x="150" y="250" font-size="32" color="#FFFFFF">Hello HTML Video</text>
        </scene>
        "##;

        let scene = parse_html_scene(html).unwrap();
        assert_eq!(scene.canvas.width, 1920);
        assert_eq!(scene.canvas.height, 1080);
        assert_eq!(scene.canvas.fps, 60);
        assert_eq!(scene.canvas.duration_secs, 10.0);
        assert_eq!(scene.canvas.background, "#101010");
        assert_eq!(scene.layers.len(), 2);

        match &scene.layers[0].kind {
            LayerKind::Rect { width, color, radius, .. } => {
                assert_eq!(*width, 400.0);
                assert_eq!(color, "#FF0000");
                assert_eq!(*radius, 16.0);
            }
            _ => panic!("Expected Rect layer"),
        }

        assert_eq!(scene.layers[0].animations.len(), 1);
        assert_eq!(scene.layers[0].animations[0].property, AnimProperty::Opacity);

        match &scene.layers[1].kind {
            LayerKind::Text { content, font_size, color, .. } => {
                assert_eq!(content, "Hello HTML Video");
                assert_eq!(*font_size, 32.0);
                assert_eq!(color, "#FFFFFF");
            }
            _ => panic!("Expected Text layer"),
        }
    }

    #[test]
    fn test_substitute_variables_and_defaults() {
        let mut vars = HashMap::new();
        vars.insert("title".to_string(), "Rustren".to_string());
        let tpl = "Hello {{title}}! Color: {{color|#FF0000}}, Missing: {{other}}";
        let out = substitute_variables(tpl, &vars);
        assert_eq!(out, "Hello Rustren! Color: #FF0000, Missing: {{other}}");
    }

    #[test]
    fn test_html_include_component_with_variables() {
        let dir = std::env::temp_dir().join("rustren_test_include");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let card_html = r##"
        <component>
            <rect x="0" y="0" width="300" height="200" color="{{bg|#111111}}"></rect>
            <text x="20" y="40" font-size="24" color="#FFFFFF">{{label|Default}}</text>
        </component>
        "##;
        std::fs::write(dir.join("card.html"), card_html).unwrap();

        let main_html = r##"
        <scene width="1280" height="720" fps="30" duration="3.0">
            <include src="card.html" bg="#238636" label="Card A" />
            <include src="card.html" label="Card B" />
        </scene>
        "##;
        let main_path = dir.join("main.html");
        std::fs::write(&main_path, main_html).unwrap();

        let compiler = HtmlCompiler;
        let compiled = compiler.compile(&main_path).unwrap();
        let scene = compiled.as_static();
        assert_eq!(scene.layers.len(), 4);

        match &scene.layers[0].kind {
            LayerKind::Rect { color, .. } => assert_eq!(color, "#238636"),
            _ => panic!("Expected Rect"),
        }
        match &scene.layers[1].kind {
            LayerKind::Text { content, .. } => assert_eq!(content, "Card A"),
            _ => panic!("Expected Text"),
        }
        match &scene.layers[2].kind {
            LayerKind::Rect { color, .. } => assert_eq!(color, "#111111"), // default fallback
            _ => panic!("Expected Rect"),
        }
        match &scene.layers[3].kind {
            LayerKind::Text { content, .. } => assert_eq!(content, "Card B"),
            _ => panic!("Expected Text"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
