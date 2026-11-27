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
//! - `<video>`: layer video (x, y, width, height, src, fit, trim-start, trim-end, playback-rate, loop, focal-x, focal-y, luma-key, volume, muted, opacity, scale)
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

fn expand_self_closing_tags(input: &str) -> String {
    let bytes = input.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(n + 128);
    let mut i = 0;

    while i < n {
        if bytes[i] == b'<' && i + 1 < n && bytes[i + 1] != b'/' && bytes[i + 1] != b'!' {
            let tag_start = i + 1;
            let mut j = tag_start;
            while j < n
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-' || bytes[j] == b'_')
            {
                j += 1;
            }
            if j > tag_start {
                let tag_name = &input[tag_start..j];
                let is_custom_tag = matches!(
                    tag_name,
                    "rect"
                        | "circle"
                        | "text"
                        | "image"
                        | "svg"
                        | "mesh3d"
                        | "waveform"
                        | "lottie"
                        | "video"
                        | "particles"
                        | "container"
                        | "div"
                        | "section"
                        | "p"
                        | "span"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "img"
                        | "animate"
                        | "animation"
                        | "include"
                        | "matte"
                        | "profile"
                        | "font"
                        | "audio"
                        | "path"
                        | "line"
                        | "polygon"
                        | "polyline"
                        | "ellipse"
                        | "g"
                );

                if is_custom_tag {
                    let mut in_quote: Option<u8> = None;
                    let mut k = j;
                    while k < n {
                        let b = bytes[k];
                        if let Some(q) = in_quote {
                            if b == q {
                                in_quote = None;
                            }
                        } else if b == b'"' || b == b'\'' {
                            in_quote = Some(b);
                        } else if b == b'>' {
                            break;
                        }
                        k += 1;
                    }

                    if k < n && bytes[k] == b'>' {
                        let mut prev = k.saturating_sub(1);
                        while prev >= j && bytes[prev].is_ascii_whitespace() {
                            prev = prev.saturating_sub(1);
                        }
                        if prev >= j && bytes[prev] == b'/' {
                            out.push('<');
                            out.push_str(&input[tag_start..prev]);
                            out.push_str("></");
                            out.push_str(tag_name);
                            out.push('>');
                            i = k + 1;
                            continue;
                        }
                    }
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn parse_gradient_from_str(s: &str) -> Option<LinearGradientSpec> {
    let s = s.trim();
    let (kind, inner) = if s.starts_with("linear-gradient(") && s.ends_with(')') {
        (
            GradientTypeSpec::Linear,
            s["linear-gradient(".len()..s.len() - 1].trim(),
        )
    } else if s.starts_with("radial-gradient(") && s.ends_with(')') {
        (
            GradientTypeSpec::Radial,
            s["radial-gradient(".len()..s.len() - 1].trim(),
        )
    } else {
        return None;
    };
    let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
    if parts.is_empty() {
        return None;
    }

    let mut angle = 0.0f32;
    let mut cx: Option<f32> = None;
    let mut cy: Option<f32> = None;
    let mut color_idx = 0;

    let first = parts[0];
    if kind == GradientTypeSpec::Linear {
        if first.ends_with("deg") {
            if let Ok(a) = first.trim_end_matches("deg").trim().parse::<f32>() {
                angle = a;
                color_idx = 1;
            }
        } else if first == "to right" {
            angle = 0.0;
            color_idx = 1;
        } else if first == "to bottom" {
            angle = 90.0;
            color_idx = 1;
        } else if first == "to left" {
            angle = 180.0;
            color_idx = 1;
        } else if first == "to top" {
            angle = 270.0;
            color_idx = 1;
        } else if first == "to bottom right" || first == "to right bottom" {
            angle = 45.0;
            color_idx = 1;
        }
    } else {
        // Optional radial-gradient shape/position prefix, e.g. "circle", "circle at 30% 30%", "at 40% 60%"
        let first_lower = first.to_ascii_lowercase();
        if first_lower.starts_with("circle")
            || first_lower.starts_with("ellipse")
            || first_lower.starts_with("at ")
        {
            if let Some(at_pos) = first_lower.find("at ") {
                let coords: Vec<&str> = first_lower[at_pos + 3..].split_whitespace().collect();
                if let Some(x_tok) = coords.first()
                    && let Some(pct) = x_tok.strip_suffix('%').and_then(|v| v.parse::<f32>().ok())
                {
                    cx = Some(pct / 100.0);
                }
                if let Some(y_tok) = coords.get(1)
                    && let Some(pct) = y_tok.strip_suffix('%').and_then(|v| v.parse::<f32>().ok())
                {
                    cy = Some(pct / 100.0);
                }
            }
            color_idx = 1;
        }
    }

    let color_parts = &parts[color_idx..];
    if color_parts.is_empty() {
        return None;
    }

    let count = color_parts.len();
    let mut stops = Vec::with_capacity(count);
    for (i, cp) in color_parts.iter().enumerate() {
        let tokens: Vec<&str> = cp.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        let color = tokens[0].to_string();
        let offset = if tokens.len() > 1 && tokens[1].ends_with('%') {
            tokens[1]
                .trim_end_matches('%')
                .parse::<f32>()
                .unwrap_or(0.0)
                / 100.0
        } else if count == 1 {
            0.0
        } else {
            i as f32 / (count - 1) as f32
        };
        stops.push(GradientStopSpec { offset, color });
    }

    let from = stops.first().map(|s| s.color.clone());
    let to = stops.last().map(|s| s.color.clone());

    Some(LinearGradientSpec {
        kind,
        from,
        to,
        stops,
        angle,
        cx,
        cy,
        fx: cx,
        fy: cy,
        r: None,
    })
}

fn parse_px_f32(s: &str) -> Option<f32> {
    s.trim().trim_end_matches("px").trim().parse::<f32>().ok()
}

/// Parse CSS box shorthand (`"10"`, `"4 10"`, `"4 10 8"`, `"4 10 8 12"`) into `(top, right, bottom, left)`.
fn parse_box_shorthand(val: Option<&str>) -> Option<(f32, f32, f32, f32)> {
    let vals: Vec<f32> = val?.split_whitespace().filter_map(parse_px_f32).collect();
    match vals.len() {
        1 => Some((vals[0], vals[0], vals[0], vals[0])),
        2 => Some((vals[0], vals[1], vals[0], vals[1])),
        3 => Some((vals[0], vals[1], vals[2], vals[1])),
        4 => Some((vals[0], vals[1], vals[2], vals[3])),
        _ => None,
    }
}

/// Resolve per-side `(top, right, bottom, left)` box spacing from shorthand + axis + side attributes.
fn resolve_box_sides(
    elem: &ElementRef,
    prefix: &str,
) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
    let shorthand = parse_box_shorthand(get_attr_str(elem, prefix).as_deref());
    let (mut t, mut r, mut b, mut l) = match shorthand {
        Some((st, sr, sb, sl)) => (Some(st), Some(sr), Some(sb), Some(sl)),
        None => (None, None, None, None),
    };
    if let Some(vx) = get_attr_f32(elem, &format!("{prefix}-x")) {
        l = Some(vx);
        r = Some(vx);
    }
    if let Some(vy) = get_attr_f32(elem, &format!("{prefix}-y")) {
        t = Some(vy);
        b = Some(vy);
    }
    if let Some(v) = get_attr_f32(elem, &format!("{prefix}-top")) {
        t = Some(v);
    }
    if let Some(v) = get_attr_f32(elem, &format!("{prefix}-right")) {
        r = Some(v);
    }
    if let Some(v) = get_attr_f32(elem, &format!("{prefix}-bottom")) {
        b = Some(v);
    }
    if let Some(v) = get_attr_f32(elem, &format!("{prefix}-left")) {
        l = Some(v);
    }
    (t, r, b, l)
}

fn parse_border_shorthand(s: &str) -> (Option<f32>, Option<String>) {
    let trimmed = s.trim();
    let color_idx = trimmed
        .find('#')
        .or_else(|| trimmed.find("rgba("))
        .or_else(|| trimmed.find("rgb("));
    let (prefix, color) = match color_idx {
        Some(idx) => (&trimmed[..idx], Some(trimmed[idx..].trim().to_string())),
        None => (trimmed, None),
    };
    let width = prefix.split_whitespace().find_map(parse_px_f32);
    (width, color)
}

fn resolve_border_style(elem: &ElementRef, allow_stroke_alias: bool) -> (Option<String>, f32) {
    let (sh_w, sh_c) = get_attr_str(elem, "border")
        .map(|s| parse_border_shorthand(&s))
        .unwrap_or((None, None));
    let color = get_attr_str(elem, "border-color")
        .or_else(|| {
            if allow_stroke_alias {
                get_attr_str(elem, "stroke")
            } else {
                None
            }
        })
        .or(sh_c);
    let width = get_attr_f32(elem, "border-width")
        .or_else(|| {
            if allow_stroke_alias {
                get_attr_f32(elem, "stroke-width")
            } else {
                None
            }
        })
        .or(sh_w)
        .unwrap_or(0.0);
    (color, width)
}

fn parse_shadow_shorthand(s: &str) -> Option<ShadowSpec> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed == "none" {
        return None;
    }
    let color_idx = trimmed
        .find('#')
        .or_else(|| trimmed.find("rgba("))
        .or_else(|| trimmed.find("rgb("));
    let (nums_part, color) = match color_idx {
        Some(idx) => (trimmed[..idx].trim(), trimmed[idx..].trim().to_string()),
        None => (trimmed, "#00000080".to_string()),
    };
    let nums: Vec<f32> = nums_part
        .split_whitespace()
        .filter_map(parse_px_f32)
        .collect();
    if nums.len() < 2 {
        return None;
    }
    Some(ShadowSpec {
        color,
        offset_x: nums[0],
        offset_y: nums[1],
        blur: nums.get(2).copied().unwrap_or(4.0),
    })
}

fn parse_dash_array(val: Option<&str>) -> Option<Vec<f32>> {
    let nums: Vec<f32> = val?
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f32>().ok())
        .collect();
    if nums.is_empty() { None } else { Some(nums) }
}

fn parse_font_weight(val: Option<&str>) -> FontWeightSpec {
    val.and_then(FontWeightSpec::from_str_loose)
        .unwrap_or(FontWeightSpec::Normal)
}

fn parse_easing_attr(val: Option<&str>) -> Easing {
    match val {
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
    }
}

fn clean_inline_text_node(raw: &str) -> String {
    if !raw.contains('\n') && !raw.contains('\r') {
        return raw.to_string();
    }
    let trimmed_start = raw.trim_start_matches(['\n', '\r', ' ', '\t']);
    let had_leading_inline_space =
        !raw.starts_with('\n') && !raw.starts_with('\r') && raw.starts_with(char::is_whitespace);
    let trimmed = trimmed_start.trim_end_matches(['\n', '\r', ' ', '\t']);
    if trimmed.is_empty() {
        return String::new();
    }
    let had_trailing_inline_space =
        !raw.ends_with('\n') && !raw.ends_with('\r') && raw.ends_with(char::is_whitespace);
    let middle = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    if had_leading_inline_space {
        out.push(' ');
    }
    out.push_str(&middle);
    if had_trailing_inline_space {
        out.push(' ');
    }
    out
}

fn parse_inline_text_spans(elem: &ElementRef) -> (String, Vec<TextSpan>) {
    let has_inline_elem = elem.children().any(|c| {
        ElementRef::wrap(c).is_some_and(|el| matches!(el.value().name(), "span" | "b" | "strong"))
    });
    if !has_inline_elem {
        let mut content = get_attr_str(elem, "content")
            .or_else(|| get_attr_str(elem, "text"))
            .unwrap_or_default();
        if content.is_empty() {
            let inner: String = elem.text().collect();
            content = inner.trim().to_string();
        }
        return (content, Vec::new());
    }

    let mut spans = Vec::new();
    for node in elem.children() {
        match node.value() {
            scraper::Node::Text(t) => {
                let cleaned = clean_inline_text_node(t.text.as_ref());
                if !cleaned.is_empty() {
                    spans.push(TextSpan {
                        text: cleaned,
                        color: None,
                        font_weight: None,
                        font_family: None,
                        font_size: None,
                    });
                }
            }
            scraper::Node::Element(_) => {
                if let Some(el) = ElementRef::wrap(node) {
                    let tag = el.value().name();
                    if matches!(tag, "span" | "b" | "strong") {
                        let text: String = el.text().collect();
                        if text.is_empty() {
                            continue;
                        }
                        let color =
                            get_attr_str(&el, "color").or_else(|| get_attr_str(&el, "fill"));
                        let font_weight = if tag == "b" || tag == "strong" {
                            Some(FontWeightSpec::Bold)
                        } else {
                            get_attr_str(&el, "font-weight").map(|w| parse_font_weight(Some(&w)))
                        };
                        let font_family =
                            get_attr_str(&el, "font-family").or_else(|| get_attr_str(&el, "font"));
                        let font_size =
                            get_attr_f32(&el, "font-size").or_else(|| get_attr_f32(&el, "size"));
                        spans.push(TextSpan {
                            text,
                            color,
                            font_weight,
                            font_family,
                            font_size,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    (String::new(), spans)
}

fn svg_attr_canonical_name(name: &str) -> &str {
    match name {
        "viewbox" => "viewBox",
        "preserveaspectratio" => "preserveAspectRatio",
        "gradientunits" => "gradientUnits",
        "gradienttransform" => "gradientTransform",
        "spreadmethod" => "spreadMethod",
        "patternunits" => "patternUnits",
        "clippathunits" => "clipPathUnits",
        _ => name,
    }
}

fn escape_attr_val(val: &str) -> String {
    val.replace('&', "&amp;").replace('"', "&quot;")
}

fn build_svg_open_tag(elem: &ElementRef) -> String {
    let mut out = String::from("<svg");
    let mut has_xmlns = false;
    for (k, v) in elem.value().attrs() {
        if matches!(
            k,
            "x" | "y" | "opacity" | "path-progress" | "stroke-dashoffset" | "rotation"
        ) {
            continue;
        }
        if k == "xmlns" {
            has_xmlns = true;
        }
        let cname = svg_attr_canonical_name(k);
        out.push(' ');
        out.push_str(cname);
        out.push_str("=\"");
        out.push_str(&escape_attr_val(v));
        out.push('"');
    }
    if !has_xmlns {
        out.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
    }
    out.push('>');
    out
}

fn serialize_svg_element_without_animate(elem: &ElementRef, strip_anim_props: bool) -> String {
    let tag = elem.value().name();
    let mut out = format!("<{tag}");
    for (k, v) in elem.value().attrs() {
        if strip_anim_props && matches!(k, "opacity" | "path-progress" | "stroke-dashoffset" | "id")
        {
            continue;
        }
        let cname = svg_attr_canonical_name(k);
        out.push(' ');
        out.push_str(cname);
        out.push_str("=\"");
        out.push_str(&escape_attr_val(v));
        out.push('"');
    }
    out.push('>');
    for child in elem.children() {
        match child.value() {
            scraper::Node::Text(t) => out.push_str(t.text.as_ref()),
            scraper::Node::Element(_) => {
                if let Some(c_el) = ElementRef::wrap(child)
                    && !matches!(c_el.value().name(), "animate" | "animation")
                {
                    out.push_str(&serialize_svg_element_without_animate(&c_el, false));
                }
            }
            _ => {}
        }
    }
    out.push_str(&format!("</{tag}>"));
    out
}

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
                let val = vars
                    .get(key)
                    .map(|s| s.as_str())
                    .unwrap_or(default_val.trim());
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
            .with_context(|| format!("Failed to read HTML file: {:?}", path))?;

        let scene = parse_html_scene_with_base(&content, path.parent())?;
        Ok(CompiledScene::Static(scene))
    }

    fn format_name(&self) -> &'static str {
        "html"
    }
}

/// Parse an HTML scene that has no on-disk base directory. Used by tests and
/// the example snippets; CLI/compiler paths always know their base dir and call
/// [`parse_html_scene_with_base`] instead.
#[allow(dead_code)]
pub fn parse_html_scene(html_content: &str) -> Result<Scene> {
    parse_html_scene_with_base(html_content, None)
}

pub fn parse_html_scene_with_base(html_content: &str, base_dir: Option<&Path>) -> Result<Scene> {
    // `data-composition-variables` declares defaults for `{{name}}` placeholders.
    // Studio applies user overrides before parsing; the plain CLI compiles with
    // the declared defaults so the same file validates and renders everywhere.
    let with_defaults = apply_composition_defaults(html_content);
    let content = with_defaults.as_deref().unwrap_or(html_content);
    parse_html_scene_internal(content, base_dir, 0, &HashMap::new())
}

/// Substitute `data-composition-variables` defaults into the source. Returns
/// `None` when the document declares no variables, so the input is untouched.
fn apply_composition_defaults(html_content: &str) -> Option<String> {
    let document = Html::parse_document(html_content);
    let sel = Selector::parse("html[data-composition-variables]").ok()?;
    let attr = document
        .select(&sel)
        .next()
        .and_then(|el| el.value().attr("data-composition-variables"))?;
    let vars_json: Vec<serde_json::Value> = serde_json::from_str(attr).ok()?;
    let mut vars = HashMap::new();
    for dv in &vars_json {
        let Some(id) = dv.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(default) = dv.get("default") else {
            continue;
        };
        let value = match default {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            other => other.to_string(),
        };
        vars.insert(id.to_string(), value);
    }
    if vars.is_empty() {
        return None;
    }
    Some(substitute_variables(html_content, &vars))
}

fn parse_html_scene_internal(
    html_content: &str,
    base_dir: Option<&Path>,
    depth: usize,
    vars: &HashMap<String, String>,
) -> Result<Scene> {
    // Fail on malformed `<include>` tags before building the DOM, so the error
    // can point at the offending tag in the original source.
    if let Some(loc) = first_include_without_src(html_content) {
        anyhow::bail!(
            "HTML: {loc}: <include> is missing a `src` attribute (write <include src=\"file.html\"/>)"
        );
    }
    let resolved_content = if vars.is_empty() {
        html_content.to_string()
    } else {
        substitute_variables(html_content, vars)
    };
    let expanded_content = expand_self_closing_tags(&resolved_content);
    let document = Html::parse_document(&expanded_content);

    // Cari tag <scene> utama, jika tidak ada, coba cari di <component>, <body> atau root
    let scene_sel = Selector::parse("scene").unwrap();
    let comp_sel = Selector::parse("component").unwrap();
    let body_sel = Selector::parse("body").unwrap();

    let root_elem = document
        .select(&scene_sel)
        .next()
        .or_else(|| document.select(&comp_sel).next())
        .or_else(|| document.select(&body_sel).next())
        .ok_or_else(|| {
            let hint = missing_root_hint(&resolved_content);
            anyhow::anyhow!(
                "HTML: missing <scene>, <component>, or <body> root tag{}",
                hint
            )
        })?;

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
        if let Some(name) =
            get_attr_str(&prof_elem, "name").or_else(|| get_attr_str(&prof_elem, "id"))
        {
            let p_width = get_attr_f32(&prof_elem, "width").map(|v| v as u32);
            let p_height = get_attr_f32(&prof_elem, "height").map(|v| v as u32);
            let p_fps = get_attr_f32(&prof_elem, "fps").map(|v| v as u32);
            let p_dur = get_attr_f32(&prof_elem, "duration")
                .or_else(|| get_attr_f32(&prof_elem, "duration-secs"));
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
            let start_sec = get_attr_f32(&a_elem, "start")
                .or_else(|| get_attr_f32(&a_elem, "start-sec"))
                .unwrap_or(0.0);
            let volume = get_attr_f32(&a_elem, "volume").unwrap_or(1.0);
            let trim_start_sec = get_attr_f32(&a_elem, "trim-start")
                .or_else(|| get_attr_f32(&a_elem, "trim-start-sec"));
            let trim_end_sec =
                get_attr_f32(&a_elem, "trim-end").or_else(|| get_attr_f32(&a_elem, "trim-end-sec"));
            let playback_rate = get_attr_f32(&a_elem, "playback-rate")
                .or_else(|| get_attr_f32(&a_elem, "speed"))
                .unwrap_or(1.0);
            let fade_in_secs = get_attr_f32(&a_elem, "fade-in")
                .or_else(|| get_attr_f32(&a_elem, "fade-in-secs"))
                .unwrap_or(0.0);
            let fade_out_secs = get_attr_f32(&a_elem, "fade-out")
                .or_else(|| get_attr_f32(&a_elem, "fade-out-secs"))
                .unwrap_or(0.0);

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

    // 5. Layers (with <include> support) & scene-level targeted <animate target="id">
    let mut layers = parse_children_layers(&root_elem, base_dir, depth, vars, false)?;
    for child in root_elem.children() {
        let Some(a_elem) = ElementRef::wrap(child) else {
            continue;
        };
        if !matches!(a_elem.value().name(), "animate" | "animation") {
            continue;
        }
        if let Some(target) = get_attr_str(&a_elem, "target") {
            let anim = parse_animation_elem(&a_elem)?;
            attach_layer_animation(&mut layers, &target, &anim);
        }
    }

    Ok(Scene {
        schema_version,
        canvas,
        world: None,
        profiles,
        fonts,
        audio,
        layers,
    })
}

/// Build a short diagnostic suffix when no root tag is found: point at the
/// first meaningful tag (a likely layer) or at the first non-blank line.
fn missing_root_hint(src: &str) -> String {
    let tags = crate::source_map::scan_tags(src);
    let map = crate::source_map::SourceMap::new(src);

    if let Some(first_tag) = tags.iter().find(|t| !t.closing && !t.name.is_empty()) {
        return format!(
            " (first tag <{}> at {})",
            first_tag.name,
            map.loc(first_tag.start)
        );
    }

    match src.find(|c: char| !c.is_whitespace()) {
        Some(offset) => format!(" (content starts at {})", map.loc(offset)),
        None => " (the file is empty)".to_string(),
    }
}

/// Find the `line L, column C` of the first `<include>` tag that has no `src`.
///
/// This is checked on the raw source because `scraper` has already normalised
/// the tag by the time the layer walker sees it, losing the position.
fn first_include_without_src(src: &str) -> Option<String> {
    let tags = crate::source_map::scan_tags(src);
    let map = crate::source_map::SourceMap::new(src);
    tags.iter()
        .find(|t| !t.closing && t.name == "include" && t.attr("src").is_none())
        .map(|t| map.loc(t.start))
}

fn parse_children_layers(
    parent: &ElementRef,
    base_dir: Option<&Path>,
    depth: usize,
    parent_vars: &HashMap<String, String>,
    is_inside_container: bool,
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
                    anyhow::anyhow!("HTML: <include> is missing a `src` attribute")
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
                    format!("Failed to read <include src=\"{}\"> ({:?})", src, inc_path)
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
                )
                .with_context(|| format!("in <include src=\"{}\">", src))?;
                let inc_pos = get_attr_str(&el, "position");
                let inc_has_xy =
                    get_attr_str(&el, "x").is_some() || get_attr_str(&el, "y").is_some();
                for mut layer in inc_scene.layers {
                    if is_inside_container {
                        match inc_pos.as_deref() {
                            Some("absolute") => layer.position = PositionSpec::Absolute,
                            Some("relative") => layer.position = PositionSpec::Relative,
                            _ if inc_has_xy => layer.position = PositionSpec::Absolute,
                            _ => {}
                        }
                    }
                    layers.push(layer);
                }

                continue;
            }
            if tag == "svg"
                && let Some(svg_layers) = try_parse_animated_svg_layers(
                    &el,
                    base_dir,
                    depth,
                    parent_vars,
                    is_inside_container,
                )?
            {
                layers.extend(svg_layers);
                continue;
            }
            if let Some(layer) =
                parse_layer_element(&el, base_dir, depth, parent_vars, is_inside_container)?
            {
                layers.push(layer);
            }
        }
    }
    Ok(layers)
}

#[derive(Clone)]
struct CommonLayerMeta {
    name: Option<String>,
    start_sec: f32,
    end_sec: Option<f32>,
    duration_secs: Option<f32>,
    overlap_secs: f32,
    position: PositionSpec,
    flex_grow: Option<f32>,
    margin: Option<f32>,
    margin_top: Option<f32>,
    margin_right: Option<f32>,
    margin_bottom: Option<f32>,
    margin_left: Option<f32>,
    blend_mode: BlendModeSpec,
    blur: f32,
    shadow: Option<ShadowSpec>,
    anchor: AnchorSpec,
    scale_x: f32,
    scale_y: f32,
    pivot_x: f32,
    pivot_y: f32,
    skew_x: f32,
    skew_y: f32,
    rotate_x: f32,
    rotate_y: f32,
    perspective: f32,
    backface_visible: bool,
    z_index: f32,
    grain: f32,
    vignette: f32,
    halftone: f32,
    wave_warp: Option<WaveWarpSpec>,
    track_matte: TrackMatteMode,
    matte_layer: Option<Box<Layer>>,
    echo: Option<EchoSpec>,
}

impl CommonLayerMeta {
    fn into_layer(self, kind: LayerKind, animations: Vec<Animation>) -> Layer {
        Layer {
            name: self.name,
            space: crate::schema::LayerSpace::Screen,
            depth_slot: None,
            depth_z: None,
            uv_warp: None,
            relight: None,
            start_sec: self.start_sec,
            end_sec: self.end_sec,
            duration_secs: self.duration_secs,
            overlap_secs: self.overlap_secs,
            position: self.position,
            flex_grow: self.flex_grow,
            margin: self.margin,
            margin_top: self.margin_top,
            margin_right: self.margin_right,
            margin_bottom: self.margin_bottom,
            margin_left: self.margin_left,
            blend_mode: self.blend_mode,
            blur: self.blur,
            shadow: self.shadow,
            anchor: self.anchor,
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            pivot_x: self.pivot_x,
            pivot_y: self.pivot_y,
            skew_x: self.skew_x,
            skew_y: self.skew_y,
            rotate_x: self.rotate_x,
            rotate_y: self.rotate_y,
            perspective: self.perspective,
            backface_visible: self.backface_visible,
            z_index: self.z_index,
            grain: self.grain,
            grain_mode: crate::schema::GrainMode::Fine,
            vignette: self.vignette,
            halftone: self.halftone,
            wave_warp: self.wave_warp,
            track_matte: self.track_matte,
            matte_layer: self.matte_layer,
            echo: self.echo,
            kind,
            animations,
        }
    }
}

fn parse_common_layer_meta(
    elem: &ElementRef,
    base_dir: Option<&Path>,
    depth: usize,
    vars: &HashMap<String, String>,
    is_inside_container: bool,
) -> Result<CommonLayerMeta> {
    let name = get_attr_str(elem, "name").or_else(|| get_attr_str(elem, "id"));
    let start_sec = get_attr_f32(elem, "start")
        .or_else(|| get_attr_f32(elem, "start-sec"))
        .unwrap_or(0.0);
    let end_sec = get_attr_f32(elem, "end").or_else(|| get_attr_f32(elem, "end-sec"));
    let duration_secs =
        get_attr_f32(elem, "duration").or_else(|| get_attr_f32(elem, "duration-secs"));
    let overlap_secs = get_attr_f32(elem, "overlap")
        .or_else(|| get_attr_f32(elem, "overlap-secs"))
        .unwrap_or(0.0);

    let position = match get_attr_str(elem, "position").as_deref() {
        Some("relative") => PositionSpec::Relative,
        Some("absolute") => PositionSpec::Absolute,
        _ => {
            if is_inside_container {
                PositionSpec::Relative
            } else {
                PositionSpec::Absolute
            }
        }
    };
    let flex_grow = get_attr_f32(elem, "flex-grow");
    let margin = get_attr_f32(elem, "margin");
    let (margin_top, margin_right, margin_bottom, margin_left) = resolve_box_sides(elem, "margin");

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
        get_attr_str(elem, "shadow")
            .or_else(|| get_attr_str(elem, "box-shadow"))
            .and_then(|s| parse_shadow_shorthand(&s))
    };

    let anchor = parse_anchor(get_attr_str(elem, "anchor").as_deref());
    let scale_x = get_attr_f32(elem, "scale-x")
        .or_else(|| get_attr_f32(elem, "scale_x"))
        .unwrap_or(1.0);
    let scale_y = get_attr_f32(elem, "scale-y")
        .or_else(|| get_attr_f32(elem, "scale_y"))
        .unwrap_or(1.0);
    let pivot_x = get_attr_f32(elem, "pivot-x")
        .or_else(|| get_attr_f32(elem, "pivot_x"))
        .unwrap_or(0.5);
    let pivot_y = get_attr_f32(elem, "pivot-y")
        .or_else(|| get_attr_f32(elem, "pivot_y"))
        .unwrap_or(0.5);
    let skew_x = get_attr_f32(elem, "skew-x")
        .or_else(|| get_attr_f32(elem, "skew_x"))
        .unwrap_or(0.0);
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
        if let Some(c_el) = ElementRef::wrap(c)
            && c_el.value().name() == "matte"
        {
            let matte_children = parse_children_layers(&c_el, base_dir, depth, vars, false)?;
            if let Some(first) = matte_children.into_iter().next() {
                matte_layer = Some(Box::new(first));
            }
            break;
        }
    }
    let skew_y = get_attr_f32(elem, "skew-y")
        .or_else(|| get_attr_f32(elem, "skew_y"))
        .unwrap_or(0.0);
    let rotate_x = get_attr_f32(elem, "rotate-x")
        .or_else(|| get_attr_f32(elem, "rotate_x"))
        .unwrap_or(0.0);
    let rotate_y = get_attr_f32(elem, "rotate-y")
        .or_else(|| get_attr_f32(elem, "rotate_y"))
        .unwrap_or(0.0);
    let perspective = get_attr_f32(elem, "perspective").unwrap_or(0.0);
    let backface_visible = !matches!(
        get_attr_str(elem, "backface-visible")
            .or_else(|| get_attr_str(elem, "backface_visible"))
            .as_deref(),
        Some("false" | "0" | "hidden")
    );
    let z_index = get_attr_f32(elem, "z-index")
        .or_else(|| get_attr_f32(elem, "z_index"))
        .or_else(|| get_attr_f32(elem, "z"))
        .unwrap_or(0.0);
    let grain = get_attr_f32(elem, "grain").unwrap_or(0.0);
    let vignette = get_attr_f32(elem, "vignette").unwrap_or(0.0);
    let halftone = get_attr_f32(elem, "halftone").unwrap_or(0.0);

    Ok(CommonLayerMeta {
        name,
        start_sec,
        end_sec,
        duration_secs,
        overlap_secs,
        position,
        flex_grow,
        margin,
        margin_top,
        margin_right,
        margin_bottom,
        margin_left,
        blend_mode,
        blur,
        shadow,
        anchor,
        scale_x,
        scale_y,
        pivot_x,
        pivot_y,
        skew_x,
        skew_y,
        rotate_x,
        rotate_y,
        perspective,
        backface_visible,
        z_index,
        grain,
        vignette,
        halftone,
        wave_warp: parse_wave_warp(elem),
        track_matte,
        matte_layer,
        echo,
    })
}

fn parse_wave_warp(elem: &ElementRef) -> Option<WaveWarpSpec> {
    let amplitude = get_attr_f32(elem, "wave-warp-amplitude")?;
    let wavelength = get_attr_f32(elem, "wave-warp-wavelength").unwrap_or(80.0);
    if wavelength <= 0.0 {
        return None;
    }
    let axis = match get_attr_str(elem, "wave-warp-axis").as_deref() {
        Some("y") => WaveAxisSpec::Y,
        Some("stretch_x") | Some("stretch-x") => WaveAxisSpec::StretchX,
        Some("stretch_y") | Some("stretch-y") => WaveAxisSpec::StretchY,
        _ => WaveAxisSpec::X,
    };
    Some(WaveWarpSpec {
        amplitude,
        wavelength,
        speed: get_attr_f32(elem, "wave-warp-speed").unwrap_or(0.0),
        phase: get_attr_f32(elem, "wave-warp-phase").unwrap_or(0.0),
        axis,
    })
}

fn parse_text_animator(elem: &ElementRef) -> Option<TextAnimatorSpec> {
    let mode = match get_attr_str(elem, "text-animator").as_deref() {
        Some("word") => TextAnimatorModeSpec::Word,
        Some(_) => TextAnimatorModeSpec::Char,
        None => return None,
    };
    Some(TextAnimatorSpec {
        mode,
        start_sec: get_attr_f32(elem, "text-animator-start").unwrap_or(0.0),
        stagger_secs: get_attr_f32(elem, "text-animator-stagger").unwrap_or(0.05),
        duration_secs: get_attr_f32(elem, "text-animator-duration").unwrap_or(0.4),
        y_offset: get_attr_f32(elem, "text-animator-y-offset").unwrap_or(0.0),
        x_offset: get_attr_f32(elem, "text-animator-x-offset").unwrap_or(0.0),
        opacity_from: get_attr_f32(elem, "text-animator-opacity-from").unwrap_or(1.0),
        scale_from: get_attr_f32(elem, "text-animator-scale-from").unwrap_or(1.0),
        easing: parse_easing_attr(get_attr_str(elem, "text-animator-easing").as_deref()),
    })
}

fn try_parse_animated_svg_layers(
    el: &ElementRef,
    base_dir: Option<&Path>,
    depth: usize,
    vars: &HashMap<String, String>,
    is_inside_container: bool,
) -> Result<Option<Vec<Layer>>> {
    if get_attr_str(el, "src").is_some() {
        return Ok(None);
    }
    let mut static_html = String::new();
    let mut animated_children = Vec::new();
    for child in el.children() {
        let Some(c_el) = ElementRef::wrap(child) else {
            continue;
        };
        if matches!(c_el.value().name(), "animate" | "animation") {
            continue;
        }
        let has_anim = c_el.children().any(|grand| {
            ElementRef::wrap(grand)
                .is_some_and(|g| matches!(g.value().name(), "animate" | "animation"))
        });
        if has_anim {
            animated_children.push(c_el);
        } else {
            static_html.push_str(&c_el.html());
        }
    }
    if animated_children.is_empty() {
        return Ok(None);
    }

    let meta = parse_common_layer_meta(el, base_dir, depth, vars, is_inside_container)?;
    let x = get_attr_f32(el, "x").unwrap_or(0.0);
    let y = get_attr_f32(el, "y").unwrap_or(0.0);
    let width = get_attr_f32(el, "width");
    let height = get_attr_f32(el, "height");
    let base_prog = get_attr_f32(el, "path-progress").unwrap_or(1.0);
    let base_offset = get_attr_f32(el, "stroke-dashoffset");
    let base_dash = parse_dash_array(get_attr_str(el, "stroke-dasharray").as_deref());
    let base_opacity = get_attr_f32(el, "opacity").unwrap_or(1.0);
    let rotation = get_attr_f32(el, "rotation").unwrap_or(0.0);
    let svg_open = build_svg_open_tag(el);

    let mut out = Vec::new();
    let wrap_in_container = is_inside_container && meta.position == PositionSpec::Relative;
    let layer_x = if wrap_in_container { 0.0 } else { x };
    let layer_y = if wrap_in_container { 0.0 } else { y };

    if !static_html.trim().is_empty() {
        let base_anims = parse_animations(el)?;
        let mut base_meta = meta.clone();
        if wrap_in_container {
            base_meta.position = PositionSpec::Absolute;
        }
        out.push(base_meta.into_layer(
            LayerKind::Svg {
                x: layer_x,
                y: layer_y,
                width,
                height,
                src: format!("{svg_open}{static_html}</svg>"),
                path_progress: base_prog,
                stroke_dashoffset: base_offset,
                stroke_dasharray: base_dash.clone(),
                opacity: base_opacity,
                rotation,
            },
            base_anims,
        ));
    }

    for anim_child in animated_children {
        let child_anims = parse_animations(&anim_child)?;
        let child_markup = serialize_svg_element_without_animate(&anim_child, true);
        let child_op = get_attr_f32(&anim_child, "opacity").unwrap_or(base_opacity);
        let child_prog = get_attr_f32(&anim_child, "path-progress").unwrap_or(base_prog);
        let child_offset = get_attr_f32(&anim_child, "stroke-dashoffset").or(base_offset);
        let child_dash = parse_dash_array(get_attr_str(&anim_child, "stroke-dasharray").as_deref())
            .or_else(|| base_dash.clone());
        let mut child_meta = meta.clone();
        if let Some(cid) =
            get_attr_str(&anim_child, "name").or_else(|| get_attr_str(&anim_child, "id"))
        {
            child_meta.name = Some(cid);
        }
        if wrap_in_container {
            child_meta.position = PositionSpec::Absolute;
        }
        out.push(child_meta.into_layer(
            LayerKind::Svg {
                x: layer_x,
                y: layer_y,
                width,
                height,
                src: format!("{svg_open}{child_markup}</svg>"),
                path_progress: child_prog,
                stroke_dashoffset: child_offset,
                stroke_dasharray: child_dash,
                opacity: child_op,
                rotation,
            },
            child_anims,
        ));
    }

    if wrap_in_container && out.len() > 1 {
        let mut wrapper_meta = meta;
        wrapper_meta.margin = None;
        let wrapper = wrapper_meta.into_layer(
            LayerKind::Container {
                x,
                y,
                width,
                height,
                layout: LayoutSpec::default(),
                background: None,
                gradient: None,
                border_color: None,
                border_width: 0.0,
                radius: 0.0,
                opacity: 1.0,
                rotation: 0.0,
                scale: 1.0,
                clip: false,
                clip_feather: 0.0,
                cascade_opacity: false,
                timeline: TimelineMode::Global,
                stagger_secs: 0.0,
                children: out,
            },
            Vec::new(),
        );
        return Ok(Some(vec![wrapper]));
    }

    Ok(Some(out))
}

fn parse_layer_element(
    elem: &ElementRef,
    base_dir: Option<&Path>,
    depth: usize,
    vars: &HashMap<String, String>,
    is_inside_container: bool,
) -> Result<Option<Layer>> {
    let tag = elem.value().name();
    let meta = parse_common_layer_meta(elem, base_dir, depth, vars, is_inside_container)?;

    let animations = parse_animations(elem)?;

    let kind = match tag {
        "rect" | "div"
            if get_attr_str(elem, "type").as_deref() == Some("rect") || tag == "rect" =>
        {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width")
                .or_else(|| get_attr_f32(elem, "w"))
                .unwrap_or(100.0);
            let height = get_attr_f32(elem, "height")
                .or_else(|| get_attr_f32(elem, "h"))
                .unwrap_or(100.0);
            let radius = get_attr_f32(elem, "radius")
                .or_else(|| get_attr_f32(elem, "border-radius"))
                .unwrap_or(0.0);
            let raw_color = get_attr_str(elem, "color")
                .or_else(|| get_attr_str(elem, "background"))
                .or_else(|| get_attr_str(elem, "fill"));
            let mut gradient =
                get_attr_str(elem, "gradient").and_then(|g| parse_gradient_from_str(&g));
            let color = match raw_color {
                Some(c)
                    if c.trim().starts_with("linear-gradient(")
                        || c.trim().starts_with("radial-gradient(") =>
                {
                    gradient = parse_gradient_from_str(&c);
                    "#000000".to_string()
                }
                Some(c) => c,
                None => "#FFFFFF".to_string(),
            };
            let (border_color, border_width) = resolve_border_style(elem, false);
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
                gradient,
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
            let cx = get_attr_f32(elem, "cx")
                .or_else(|| get_attr_f32(elem, "x"))
                .unwrap_or(0.0);
            let cy = get_attr_f32(elem, "cy")
                .or_else(|| get_attr_f32(elem, "y"))
                .unwrap_or(0.0);
            let r = get_attr_f32(elem, "r")
                .or_else(|| get_attr_f32(elem, "radius"))
                .unwrap_or(50.0);
            let raw_color = get_attr_str(elem, "color")
                .or_else(|| get_attr_str(elem, "fill"))
                .or_else(|| get_attr_str(elem, "background"));
            let mut gradient =
                get_attr_str(elem, "gradient").and_then(|g| parse_gradient_from_str(&g));
            let color = match raw_color {
                Some(c)
                    if c.trim().starts_with("linear-gradient(")
                        || c.trim().starts_with("radial-gradient(") =>
                {
                    gradient = parse_gradient_from_str(&c);
                    "#FFFFFF".to_string()
                }
                Some(c) => c,
                None => "#FFFFFF".to_string(),
            };
            let (border_color, border_width) = resolve_border_style(elem, true);
            let path_progress = get_attr_f32(elem, "path-progress").unwrap_or(1.0);
            let stroke_dashoffset = get_attr_f32(elem, "stroke-dashoffset");
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);

            LayerKind::Circle {
                cx,
                cy,
                r,
                color,
                gradient,
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
            let (content, spans) = parse_inline_text_spans(elem);
            let font_size = get_attr_f32(elem, "font-size")
                .or_else(|| get_attr_f32(elem, "size"))
                .unwrap_or(48.0);
            let line_height = get_attr_f32(elem, "line-height");
            let letter_spacing = get_attr_f32(elem, "letter-spacing").unwrap_or(0.0);
            let font_family =
                get_attr_str(elem, "font-family").or_else(|| get_attr_str(elem, "font"));
            let font_weight = parse_font_weight(get_attr_str(elem, "font-weight").as_deref());
            let text_align = match get_attr_str(elem, "text-align")
                .or_else(|| get_attr_str(elem, "align"))
                .as_deref()
            {
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
            let color = get_attr_str(elem, "color")
                .or_else(|| get_attr_str(elem, "fill"))
                .unwrap_or_else(|| "#FFFFFF".to_string());
            let gradient = get_attr_str(elem, "gradient").and_then(|g| parse_gradient_from_str(&g));
            let outline = if get_attr_str(elem, "outline-color").is_some()
                || get_attr_f32(elem, "outline-width").is_some()
            {
                Some(TextOutlineSpec {
                    color: get_attr_str(elem, "outline-color")
                        .unwrap_or_else(|| "#000000".to_string()),
                    width: get_attr_f32(elem, "outline-width").unwrap_or(2.0),
                })
            } else {
                None
            };
            let dynamic = match get_attr_str(elem, "dynamic").as_deref() {
                Some("typewriter") => Some(DynamicTextSpec::Typewriter {
                    text: content.clone(),
                    start_sec: get_attr_f32(elem, "dynamic-start"),
                    end_sec: get_attr_f32(elem, "dynamic-end"),
                    cursor: get_attr_str(elem, "cursor"),
                }),
                Some("counter") => Some(DynamicTextSpec::Counter {
                    from: get_attr_f32(elem, "from").unwrap_or(0.0),
                    to: get_attr_f32(elem, "to").unwrap_or(100.0),
                    decimals: get_attr_f32(elem, "decimals").unwrap_or(0.0) as usize,
                    prefix: get_attr_str(elem, "prefix").unwrap_or_default(),
                    suffix: get_attr_str(elem, "suffix").unwrap_or_default(),
                    start_sec: get_attr_f32(elem, "dynamic-start"),
                    end_sec: get_attr_f32(elem, "dynamic-end"),
                    easing: parse_easing_attr(get_attr_str(elem, "easing").as_deref()),
                }),
                Some("timecode") => Some(DynamicTextSpec::Timecode {
                    format: get_attr_str(elem, "format").unwrap_or_else(|| "mm:ss".to_string()),
                    offset_sec: get_attr_f32(elem, "offset").unwrap_or(0.0),
                }),
                _ => None,
            };
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let max_width = get_attr_f32(elem, "max-width").or_else(|| get_attr_f32(elem, "width"));
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);

            LayerKind::Text {
                x,
                y,
                content,
                dynamic,
                spans,
                font_size,
                line_height,
                letter_spacing,
                font_family,
                font_weight,
                text_align,
                direction,
                color,
                gradient,
                outline,
                opacity,
                max_width,
                scale,
                text_animator: parse_text_animator(elem),
            }
        }
        "image" | "img" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width");
            let height = get_attr_f32(elem, "height");
            let mut src = get_attr_str(elem, "src").unwrap_or_default();
            if src.is_empty() {
                let mut html = elem.html();
                if !html.contains("xmlns=") {
                    html = html.replacen("<svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"", 1);
                }
                src = html;
            }
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
            let mut src = get_attr_str(elem, "src").unwrap_or_default();
            if src.is_empty() {
                let mut html = elem.html();
                if !html.contains("xmlns=") {
                    html = html.replacen("<svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"", 1);
                }
                src = html;
            }
            let path_progress = get_attr_f32(elem, "path-progress").unwrap_or(1.0);
            let stroke_dashoffset = get_attr_f32(elem, "stroke-dashoffset");
            let stroke_dasharray =
                parse_dash_array(get_attr_str(elem, "stroke-dasharray").as_deref());
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
                stroke_dasharray,
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
            let loop_video = get_attr_str(elem, "loop")
                .or_else(|| get_attr_str(elem, "loop-video"))
                .is_some();
            let focal_x = get_attr_f32(elem, "focal-x")
                .or_else(|| get_attr_f32(elem, "focal_x"))
                .unwrap_or(0.5);
            let focal_y = get_attr_f32(elem, "focal-y")
                .or_else(|| get_attr_f32(elem, "focal_y"))
                .unwrap_or(0.5);
            let luma_key = get_attr_f32(elem, "luma-key")
                .or_else(|| get_attr_f32(elem, "luma_key"));
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
                loop_video,
                focal_x,
                focal_y,
                luma_key,
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
            let mut src = get_attr_str(elem, "src").unwrap_or_default();
            if src.is_empty() {
                let mut html = elem.html();
                if !html.contains("xmlns=") {
                    html = html.replacen("<svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"", 1);
                }
                src = html;
            }
            let loop_animation = get_attr_str(elem, "loop").is_some();
            let speed = get_attr_f32(elem, "speed").unwrap_or(1.0);
            let progress = get_attr_f32(elem, "progress");
            let color_overrides = get_attr_str(elem, "color-overrides")
                .or_else(|| get_attr_str(elem, "color_overrides"))
                .and_then(|s| serde_json::from_str::<HashMap<String, String>>(&s).ok());
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
                progress,
                color_overrides,
                opacity,
                rotation,
                scale,
            }
        }
        "particles" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width")
                .or_else(|| get_attr_f32(elem, "w"))
                .unwrap_or(400.0);
            let height = get_attr_f32(elem, "height")
                .or_else(|| get_attr_f32(elem, "h"))
                .unwrap_or(400.0);
            let count = get_attr_f32(elem, "count").unwrap_or(40.0).max(0.0) as usize;
            let seed = get_attr_f32(elem, "seed").unwrap_or(42.0).max(0.0) as u64;
            let speed = get_attr_f32(elem, "speed").unwrap_or(1.0);
            let gravity = get_attr_f32(elem, "gravity").unwrap_or(120.0);
            let spread = get_attr_f32(elem, "spread").unwrap_or(360.0);
            let angle = get_attr_f32(elem, "angle").unwrap_or(-90.0);
            let size = get_attr_f32(elem, "size").unwrap_or(8.0);
            let color = get_attr_str(elem, "color").unwrap_or_else(|| "#FFFFFF".to_string());
            let colors: Vec<String> = get_attr_str(elem, "colors")
                .map(|s| {
                    s.split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|p| !p.is_empty())
                        .map(|p| p.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let shape = match get_attr_str(elem, "shape").as_deref() {
                Some("rect") => ParticleShapeSpec::Rect,
                Some("confetti") => ParticleShapeSpec::Confetti,
                _ => ParticleShapeSpec::Circle,
            };
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let rotation = get_attr_f32(elem, "rotation").unwrap_or(0.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);

            LayerKind::Particles {
                x,
                y,
                width,
                height,
                count,
                seed,
                speed,
                gravity,
                spread,
                angle,
                size,
                color,
                colors,
                shape,
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
            let raw_bg = get_attr_str(elem, "background")
                .or_else(|| get_attr_str(elem, "bg"))
                .or_else(|| get_attr_str(elem, "color"))
                .or_else(|| get_attr_str(elem, "fill"));
            let mut gradient =
                get_attr_str(elem, "gradient").and_then(|g| parse_gradient_from_str(&g));
            let background = match raw_bg {
                Some(bg)
                    if bg.trim().starts_with("linear-gradient(")
                        || bg.trim().starts_with("radial-gradient(") =>
                {
                    gradient = parse_gradient_from_str(&bg);
                    None
                }
                other => other,
            };
            let (border_color, border_width) = resolve_border_style(elem, false);
            let radius = get_attr_f32(elem, "radius")
                .or_else(|| get_attr_f32(elem, "border-radius"))
                .unwrap_or(0.0);
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let rotation = get_attr_f32(elem, "rotation").unwrap_or(0.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);
            let clip = get_attr_str(elem, "clip").is_some()
                || get_attr_str(elem, "overflow").as_deref() == Some("hidden");
            let cascade_opacity = get_attr_str(elem, "cascade-opacity").is_some()
                || get_attr_str(elem, "cascade_opacity").is_some();
            let stagger_secs = get_attr_f32(elem, "stagger")
                .or_else(|| get_attr_f32(elem, "stagger-secs"))
                .unwrap_or(0.0);

            let timeline = match get_attr_str(elem, "timeline").as_deref() {
                Some("local") | Some("parallel") => TimelineMode::Local,
                Some("series") => TimelineMode::Series,
                _ => TimelineMode::Global,
            };

            let layout_dir = match get_attr_str(elem, "layout")
                .or_else(|| get_attr_str(elem, "flex-direction"))
                .or_else(|| get_attr_str(elem, "direction"))
                .as_deref()
            {
                Some("column") => FlexDirectionSpec::Column,
                Some("row") => FlexDirectionSpec::Row,
                _ => FlexDirectionSpec::Column,
            };
            let flex_wrap = get_attr_str(elem, "flex-wrap").as_deref() == Some("wrap")
                || get_attr_str(elem, "wrap")
                    .is_some_and(|w| w == "true" || w == "wrap" || w == "1");

            let justify = parse_align(
                get_attr_str(elem, "justify-content")
                    .or_else(|| get_attr_str(elem, "justify"))
                    .as_deref(),
            );
            let align = parse_align(
                get_attr_str(elem, "align-items")
                    .or_else(|| get_attr_str(elem, "align"))
                    .as_deref(),
            );
            let gap_vals: Vec<f32> = get_attr_str(elem, "gap")
                .map(|g| g.split_whitespace().filter_map(parse_px_f32).collect())
                .unwrap_or_default();
            let gap = gap_vals.first().copied().unwrap_or(0.0);
            let row_gap = get_attr_f32(elem, "row-gap").or_else(|| {
                if gap_vals.len() >= 2 {
                    Some(gap_vals[0])
                } else {
                    None
                }
            });
            let column_gap = get_attr_f32(elem, "column-gap").or_else(|| {
                if gap_vals.len() >= 2 {
                    Some(gap_vals[1])
                } else {
                    None
                }
            });
            let padding = get_attr_f32(elem, "padding").unwrap_or(0.0);
            let (padding_top, padding_right, padding_bottom, padding_left) =
                resolve_box_sides(elem, "padding");

            let layout = LayoutSpec {
                direction: layout_dir,
                flex_wrap,
                justify_content: justify,
                align_items: align,
                gap,
                row_gap,
                column_gap,
                padding,
                padding_top,
                padding_right,
                padding_bottom,
                padding_left,
            };

            // Parse children (supports <include> inside containers too)
            let mut children = parse_children_layers(elem, base_dir, depth, vars, true)?;

            // Declarative inter-scene transition sugar for timeline="series" containers.
            // Lowers directly to overlap_secs + standard animations / track_matte with zero renderer changes.
            if timeline == TimelineMode::Series
                && let Some(trans) = get_attr_str(elem, "transition")
            {
                let trans_name = trans.trim().to_ascii_lowercase();
                let dur = get_attr_f32(elem, "transition-duration")
                    .or_else(|| get_attr_f32(elem, "transition_duration"))
                    .unwrap_or(0.6)
                    .max(0.01);
                let ref_w = width.unwrap_or(1920.0);
                let ref_h = height.unwrap_or(1080.0);
                let make_anim = |property: AnimProperty,
                                 from: f32,
                                 to: f32,
                                 easing: Easing|
                 -> Animation {
                    Animation {
                        property,
                        from: AnimValue::Number(from),
                        to: AnimValue::Number(to),
                        start_sec: 0.0,
                        end_sec: dur,
                        easing,
                        bezier: None,
                        spring: None,
                        keyframes: Vec::new(),
                        repeat: None,
                        yoyo: false,
                        expr: None,
                    }
                };
                for child in children.iter_mut().skip(1) {
                    if child.overlap_secs <= 0.0 {
                        child.overlap_secs = dur;
                    }
                    match trans_name.as_str() {
                        "fade" | "crossfade" => {
                            if let LayerKind::Container {
                                cascade_opacity, ..
                            } = &mut child.kind
                            {
                                *cascade_opacity = true;
                            }
                            child.animations.push(make_anim(
                                AnimProperty::Opacity,
                                0.0,
                                1.0,
                                Easing::EaseOut,
                            ));
                        }
                        "pan-up" | "slide-up" => {
                            child.animations.push(make_anim(
                                AnimProperty::Y,
                                ref_h,
                                0.0,
                                Easing::EaseInOut,
                            ));
                        }
                        "pan-down" | "slide-down" => {
                            child.animations.push(make_anim(
                                AnimProperty::Y,
                                -ref_h,
                                0.0,
                                Easing::EaseInOut,
                            ));
                        }
                        "pan-left" | "slide-left" => {
                            child.animations.push(make_anim(
                                AnimProperty::X,
                                ref_w,
                                0.0,
                                Easing::EaseInOut,
                            ));
                        }
                        "pan-right" | "slide-right" => {
                            child.animations.push(make_anim(
                                AnimProperty::X,
                                -ref_w,
                                0.0,
                                Easing::EaseInOut,
                            ));
                        }
                        "iris" | "iris-wipe" | "circle-wipe" => {
                            let max_r = (ref_w * 0.5).hypot(ref_h * 0.5) + 4.0;
                            let matte_circle = Layer {
                                name: None,
                                start_sec: 0.0,
                                end_sec: None,
                                duration_secs: None,
                                overlap_secs: 0.0,
                                z_index: 0.0,
                                position: PositionSpec::Absolute,
                                margin: None,
                                margin_top: None,
                                margin_right: None,
                                margin_bottom: None,
                                margin_left: None,
                                flex_grow: None,
                                anchor: AnchorSpec::TopLeft,
                                pivot_x: 0.5,
                                pivot_y: 0.5,
                                scale_x: 1.0,
                                scale_y: 1.0,
                                skew_x: 0.0,
                                skew_y: 0.0,
                                rotate_x: 0.0,
                                rotate_y: 0.0,
                                perspective: 1000.0,
                                backface_visible: true,
                                blend_mode: BlendModeSpec::Normal,
                                blur: 0.0,
                                shadow: None,
                                wave_warp: None,
                                echo: None,
                                grain: 0.0,
                                grain_mode: crate::schema::GrainMode::Fine,
                                vignette: 0.0,
                                halftone: 0.0,
                                track_matte: TrackMatteMode::None,
                                matte_layer: None,
                                space: crate::schema::LayerSpace::Screen,
                                depth_slot: None,
                                depth_z: None,
                                uv_warp: None,
                                relight: None,
                                kind: LayerKind::Circle {
                                    cx: ref_w * 0.5,
                                    cy: ref_h * 0.5,
                                    r: max_r,
                                    color: "#FFFFFF".to_string(),
                                    gradient: None,
                                    border_color: None,
                                    border_width: 0.0,
                                    path_progress: 1.0,
                                    stroke_dashoffset: None,
                                    opacity: 1.0,
                                    scale: 1.0,
                                },
                                animations: vec![make_anim(
                                    AnimProperty::R,
                                    0.0,
                                    max_r,
                                    Easing::EaseInOut,
                                )],
                            };
                            child.track_matte = TrackMatteMode::Alpha;
                            child.matte_layer = Some(Box::new(matte_circle));
                        }
                        _ => {}
                    }
                }
            }

            LayerKind::Container {
                x,
                y,
                width,
                height,
                layout,
                background,
                gradient,
                border_color,
                border_width,
                radius,
                opacity,
                rotation,
                scale,
                clip,
                clip_feather: get_attr_f32(elem, "clip-feather").unwrap_or(0.0),
                cascade_opacity,
                timeline,
                stagger_secs,
                children,
            }
        }
        #[cfg(feature = "exp-3d")]
        "mesh3d" => {
            let x = get_attr_f32(elem, "x").unwrap_or(0.0);
            let y = get_attr_f32(elem, "y").unwrap_or(0.0);
            let width = get_attr_f32(elem, "width").unwrap_or(200.0);
            let height = get_attr_f32(elem, "height").unwrap_or(200.0);
            let primitive = match get_attr_str(elem, "primitive").as_deref() {
                Some("sphere") => crate::schema::Mesh3dPrimitive::Sphere,
                Some("pyramid") => crate::schema::Mesh3dPrimitive::Pyramid,
                Some("torus") => crate::schema::Mesh3dPrimitive::Torus,
                _ => crate::schema::Mesh3dPrimitive::Cube,
            };
            let src = get_attr_str(elem, "src");
            let texture = get_attr_str(elem, "texture");
            let opacity = get_attr_f32(elem, "opacity").unwrap_or(1.0);
            let scale = get_attr_f32(elem, "scale").unwrap_or(1.0);
            let rotate_x = get_attr_f32(elem, "rotate-x").unwrap_or(0.0);
            let rotate_y = get_attr_f32(elem, "rotate-y").unwrap_or(0.0);
            let rotate_z = get_attr_f32(elem, "rotate-z").unwrap_or(0.0);
            let fov = get_attr_f32(elem, "fov").unwrap_or(45.0);
            let color = get_attr_str(elem, "color").unwrap_or_else(|| "#38BDF8".to_string());
            let light_dir = get_attr_str(elem, "light-dir")
                .or_else(|| get_attr_str(elem, "light_dir"))
                .and_then(|s| {
                    let parts: Vec<f32> = s
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .filter(|p| !p.is_empty())
                        .filter_map(|p| p.parse::<f32>().ok())
                        .collect();
                    if parts.len() == 3 {
                        Some([parts[0], parts[1], parts[2]])
                    } else {
                        None
                    }
                });
            let wireframe = get_attr_str(elem, "wireframe")
                .map(|s| s.trim().to_lowercase() == "true")
                .unwrap_or(false);

            LayerKind::Mesh3d {
                x,
                y,
                width,
                height,
                primitive,
                src,
                texture,
                opacity,
                scale,
                rotate_x,
                rotate_y,
                rotate_z,
                fov,
                color,
                light_dir,
                wireframe,
            }
        }
        _ => return Ok(None),
    };

    Ok(Some(meta.into_layer(kind, animations)))
}

fn parse_animations(elem: &ElementRef) -> Result<Vec<Animation>> {
    let mut animations = Vec::new();
    for child in elem.children() {
        let Some(a_elem) = ElementRef::wrap(child) else {
            continue;
        };
        if !matches!(a_elem.value().name(), "animate" | "animation") {
            continue;
        }
        if get_attr_str(&a_elem, "target").is_some() {
            continue;
        }
        animations.push(parse_animation_elem(&a_elem)?);
    }
    Ok(animations)
}

fn attach_layer_animation(layers: &mut [Layer], target: &str, anim: &Animation) {
    for layer in layers {
        if layer.name.as_deref() == Some(target) {
            layer.animations.push(anim.clone());
        }
        if let LayerKind::Container { children, .. } = &mut layer.kind {
            attach_layer_animation(children, target, anim);
        }
    }
}

fn parse_animation_elem(a_elem: &ElementRef) -> Result<Animation> {
    let prop_str = get_attr_str(a_elem, "property")
        .or_else(|| get_attr_str(a_elem, "prop"))
        .unwrap_or_else(|| "opacity".to_string());

    let property = match prop_str.as_str() {
        "x" => AnimProperty::X,
        "y" | "translate_y" | "translate-y" => AnimProperty::Y,
        "width" | "w" => AnimProperty::Width,
        "height" | "h" => AnimProperty::Height,
        "opacity" => AnimProperty::Opacity,
        "scale" => AnimProperty::Scale,
        "scale_x" | "scale-x" => AnimProperty::ScaleX,
        "scale_y" | "scale-y" => AnimProperty::ScaleY,
        "focal_x" | "focal-x" => AnimProperty::FocalX,
        "focal_y" | "focal-y" => AnimProperty::FocalY,
        "rotation" => AnimProperty::Rotation,
        "skew_x" | "skew-x" => AnimProperty::SkewX,
        "skew_y" | "skew-y" => AnimProperty::SkewY,
        "rotate_x" | "rotate-x" => AnimProperty::RotateX,
        "rotate_y" | "rotate-y" => AnimProperty::RotateY,
        "rotate_z" | "rotate-z" => AnimProperty::RotateZ,
        "perspective" => AnimProperty::Perspective,
        "z" | "z_index" | "z-index" => AnimProperty::ZIndex,
        "cx" => AnimProperty::Cx,
        "cy" => AnimProperty::Cy,
        "r" | "radius" => AnimProperty::Radius,
        "color" | "background" => AnimProperty::Color,
        "border_color" | "border-color" => AnimProperty::BorderColor,
        "path_progress" | "path-progress" => AnimProperty::PathProgress,
        "progress" => AnimProperty::Progress,
        "stroke_dashoffset" | "stroke-dashoffset" => AnimProperty::StrokeDashoffset,
        "volume" => AnimProperty::Volume,
        _ => AnimProperty::Opacity,
    };

    let expr = get_attr_str(a_elem, "expr");
    let from = parse_anim_value(get_attr_str(a_elem, "from").as_deref());
    let to = parse_anim_value(get_attr_str(a_elem, "to").as_deref());

    let delay = get_attr_f32(a_elem, "delay").unwrap_or(0.0);
    let start_sec = get_attr_f32(a_elem, "start")
        .or_else(|| get_attr_f32(a_elem, "start-sec"))
        .unwrap_or(delay);
    let duration = get_attr_f32(a_elem, "duration")
        .or_else(|| get_attr_f32(a_elem, "duration-sec"))
        .or_else(|| get_attr_f32(a_elem, "dur"));
    let end_sec = get_attr_f32(a_elem, "end")
        .or_else(|| get_attr_f32(a_elem, "end-sec"))
        .unwrap_or({
            if let Some(d) = duration {
                start_sec + d
            } else {
                1.0
            }
        });

    let easing = parse_easing_attr(get_attr_str(a_elem, "easing").as_deref());

    let yoyo = get_attr_str(a_elem, "yoyo").is_some();
    let repeat = match get_attr_str(a_elem, "repeat").as_deref() {
        Some("infinite") => Some(RepeatSpec::Mode(RepeatMode::Infinite)),
        Some(n) => n.parse::<u32>().ok().map(RepeatSpec::Count),
        None => None,
    };

    Ok(Animation {
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
    })
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
            LayerKind::Rect {
                width,
                color,
                radius,
                ..
            } => {
                assert_eq!(*width, 400.0);
                assert_eq!(color, "#FF0000");
                assert_eq!(*radius, 16.0);
            }
            _ => panic!("Expected Rect layer"),
        }

        assert_eq!(scene.layers[0].animations.len(), 1);
        assert_eq!(
            scene.layers[0].animations[0].property,
            AnimProperty::Opacity
        );

        match &scene.layers[1].kind {
            LayerKind::Text {
                content,
                font_size,
                color,
                ..
            } => {
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

    #[test]
    fn test_self_closing_container_and_scoped_animations() {
        let html = r##"
        <scene width="800" height="600" fps="30" duration="3.0" background="#000000">
            <container id="stage" x="10" y="10" width="500" height="200">
                <container id="dot" width="24" height="24" background="#FF0000" />
                <container id="inner" width="100" height="40">
                    <animate property="width" from="0" to="90" start="0.0" end="1.0" />
                    <text font-size="20" color="#FFFFFF">Badge</text>
                </container>
            </container>
            <animate target="dot" property="opacity" from="1" to="0.2" start="0.5" end="1.0" />
        </scene>
        "##;

        let scene = parse_html_scene(html).unwrap();
        assert_eq!(scene.layers.len(), 1);
        let LayerKind::Container { children, .. } = &scene.layers[0].kind else {
            panic!("Expected stage container");
        };
        assert!(
            scene.layers[0].animations.is_empty(),
            "Parent stage container must not inherit nested child <animate> tags"
        );
        assert_eq!(
            children.len(),
            2,
            "Self-closing <container /> must not swallow siblings"
        );
        assert_eq!(
            children[0].animations.len(),
            1,
            "Targeted root <animate> should attach to dot"
        );
        assert_eq!(children[1].animations.len(), 1);
    }

    #[test]
    fn test_2d_box_spacing_border_shadow_and_flex_wrap() {
        let html = r##"
        <scene width="800" height="600" fps="30" duration="2.0" background="#000000">
            <container x="40" y="40" width="400" direction="row" flex-wrap="wrap" padding="4 10" gap="12 8" border="2px solid #ff0000" shadow="0 4 12 rgba(0, 0, 0, 0.5)">
                <rect width="120" height="40" margin-top="10" margin-left="6" color="#ffffff" />
            </container>
        </scene>
        "##;

        let scene = parse_html_scene(html).unwrap();
        let container = &scene.layers[0];
        let sh = container.shadow.as_ref().expect("shadow parsed");
        assert_eq!(sh.offset_x, 0.0);
        assert_eq!(sh.offset_y, 4.0);
        assert_eq!(sh.blur, 12.0);
        assert_eq!(sh.color, "rgba(0, 0, 0, 0.5)");

        let LayerKind::Container {
            layout,
            border_color,
            border_width,
            children,
            ..
        } = &container.kind
        else {
            panic!("Expected container");
        };
        assert_eq!(border_color.as_deref(), Some("#ff0000"));
        assert_eq!(*border_width, 2.0);
        assert!(layout.flex_wrap);
        assert_eq!(layout.padding_trbl(), (4.0, 10.0, 4.0, 10.0));
        assert_eq!(layout.gaps(), (8.0, 12.0));
        assert_eq!(children[0].margin_trbl(), (10.0, 0.0, 0.0, 6.0));
    }

    #[test]
    fn test_inline_rich_text_spans_and_svg_path_split() {
        let html = r##"
        <scene width="800" height="600" fps="30" duration="3.0" background="#000000">
            <text x="20" y="20" font-size="24" color="#ffffff">Hello <span color="#00ff00" font-weight="700">World</span>!</text>
            <svg x="0" y="0" width="800" height="600" viewBox="0 0 800 600" fill="none">
                <path d="M 10 10 L 100 100" stroke="#ffffff" stroke-width="2" />
                <path id="conn1" d="M 100 100 L 300 100" stroke="#ffb020" stroke-width="5" opacity="0.0">
                    <animate property="opacity" from="0.0" to="1.0" start="0.5" end="1.2" />
                </path>
            </svg>
        </scene>
        "##;

        let scene = parse_html_scene(html).unwrap();
        assert_eq!(
            scene.layers.len(),
            3,
            "Text + static SVG + animated SVG path"
        );

        let LayerKind::Text { spans, .. } = &scene.layers[0].kind else {
            panic!("Expected Text");
        };
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "Hello ");
        assert_eq!(spans[1].text, "World");
        assert_eq!(spans[1].color.as_deref(), Some("#00ff00"));
        assert_eq!(spans[1].font_weight, Some(FontWeightSpec::Bold));
        assert_eq!(spans[2].text, "!");

        let LayerKind::Svg {
            src: static_src, ..
        } = &scene.layers[1].kind
        else {
            panic!("Expected static Svg");
        };
        assert!(static_src.contains("M 10 10 L 100 100"));

        let anim_layer = &scene.layers[2];
        assert_eq!(anim_layer.name.as_deref(), Some("conn1"));
        assert_eq!(anim_layer.animations.len(), 1);
        let LayerKind::Svg {
            src: anim_src,
            opacity,
            ..
        } = &anim_layer.kind
        else {
            panic!("Expected animated Svg");
        };
        assert_eq!(*opacity, 0.0);
        assert!(anim_src.contains("M 100 100 L 300 100"));
        assert!(!anim_src.contains("opacity=\"0.0\""));
    }

    #[test]
    fn test_html_lottie_progress_color_overrides_and_monolithic_svg_lint() {
        let html = r##"
            <scene width="1280" height="720" fps="30" duration="3">
              <lottie src="hero.json" progress="0.25" color-overrides='{"accent":"#38BDF8","#f00":"#00FF00"}'>
                <animate prop="progress" from="0.0" to="1.0" start="0.0" end="2.0" />
              </lottie>
              <svg width="1280" height="720">
                <rect x="0" y="0" width="1280" height="2" fill="#334155" />
                <rect x="0" y="718" width="1280" height="2" fill="#334155" />
              </svg>
            </scene>
        "##;
        let scene = parse_html_scene(html).unwrap();
        let LayerKind::Lottie {
            progress,
            color_overrides,
            ..
        } = &scene.layers[0].kind
        else {
            panic!("Expected Lottie layer");
        };
        assert_eq!(*progress, Some(0.25));
        let map = color_overrides.as_ref().expect("color_overrides parsed");
        assert_eq!(map.get("accent").map(String::as_str), Some("#38BDF8"));
        assert_eq!(map.get("#f00").map(String::as_str), Some("#00FF00"));
        assert_eq!(scene.layers[0].animations[0].property, AnimProperty::Progress);

        // Simple 2-rect 1280x720 background SVG must NOT trigger static_monolithic_svg
        let warnings = crate::lint::collect_lint_warnings_structured(&scene);
        assert!(!warnings.iter().any(|w| w.code == "static_monolithic_svg"));

        // Complex 12-shape monolithic static SVG MUST trigger static_monolithic_svg with fix_for
        let many_paths: String = (0..12)
            .map(|i| format!("<path d=\"M {i} 0 L {i} 10\" />"))
            .collect();
        let mono_html = format!(
            "<scene width=\"1280\" height=\"720\" fps=\"30\" duration=\"3\"><svg width=\"400\" height=\"400\">{many_paths}</svg></scene>"
        );
        let mono_scene = parse_html_scene(&mono_html).unwrap();
        let warnings2 = crate::lint::collect_lint_warnings_structured(&mono_scene);
        let mono_w = warnings2
            .iter()
            .find(|w| w.code == "static_monolithic_svg")
            .expect("Expected static_monolithic_svg warning");
        assert!(mono_w.fix.is_some());

        // <rect color="radial-gradient(...)"> must parse gradient just like <circle>
        let rect_grad_html = r##"
            <scene width="640" height="360" fps="30" duration="1">
              <rect x="10" y="20" width="100" height="100" color="radial-gradient(circle at 30% 30%, #FFFFFF, #38BDF8)" />
            </scene>
        "##;
        let rg_scene = parse_html_scene(rect_grad_html).unwrap();
        let LayerKind::Rect { gradient, .. } = &rg_scene.layers[0].kind else {
            panic!("Expected Rect layer");
        };
        let g = gradient.as_ref().expect("Expected parsed radial gradient on rect");
        assert_eq!(g.kind, GradientTypeSpec::Radial);
        assert_eq!(g.cx, Some(0.3));
        assert_eq!(g.cy, Some(0.3));
    }
}
