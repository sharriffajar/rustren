//! Safe, conservative auto-fixes for common AI-authoring mistakes in scenes.
//!
//! Each fix is only applied when the intent is unambiguous:
//! - numeric fields authored as strings with a `px`/`s` suffix (`"48px"` -> `48`)
//! - named CSS colors (`"white"`) in color fields -> hex (`"#FFFFFF"`)
//! - an invalid `position` value (`"center"`) moves the intent into `anchor`
//!
//! `fix_json_str` returns the rewritten JSON (pretty-printed) plus a list of
//! applied fixes. `fix_html_str` does the same at the attribute level for HTML
//! scenes. Neither invents fields nor drops unknown data.

use serde_json::Value;

/// A single applied fix, in machine-readable form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedFix {
    pub path: String,
    pub field: String,
    pub before: String,
    pub after: String,
}

/// Fields whose values are numbers (px / unitless) rather than length strings.
const NUMERIC_FIELDS: &[&str] = &[
    "x",
    "y",
    "width",
    "height",
    "radius",
    "cx",
    "cy",
    "r",
    "opacity",
    "scale",
    "scale_x",
    "scale_y",
    "pivot_x",
    "pivot_y",
    "focal_x",
    "focal_y",
    "luma_key",
    "rotation",
    "border_width",
    "font_size",
    "line_height",
    "letter_spacing",
    "blur",
    "skew_x",
    "skew_y",
    "rotate_x",
    "rotate_y",
    "perspective",
    "z_index",
    "z",
    "grain",
    "vignette",
    "halftone",
    "path_progress",
    "stroke_dashoffset",
    "overlap_secs",
    "flex_grow",
    "margin",
    "margin_top",
    "margin_right",
    "margin_bottom",
    "margin_left",
    "start_sec",
    "end_sec",
    "duration_secs",
    "offset_x",
    "offset_y",
    "stagger_secs",
    "speed",
    "gravity",
    "spread",
    "angle",
    "size",
    "bar_gap",
    "volume",
    "playback_rate",
    "trim_start_sec",
    "trim_end_sec",
    "fade_in_secs",
    "fade_out_secs",
    "padding",
    "padding_top",
    "padding_right",
    "padding_bottom",
    "padding_left",
    "gap",
    "row_gap",
    "column_gap",
    "from_sec",
    "to_sec",
];

/// Color-valued fields.
const COLOR_FIELDS: &[&str] = &[
    "color",
    "background",
    "border_color",
    "fill",
    "stroke",
    "shadow_color",
];

/// Minimal named-color map for the names most often produced by LLMs.
pub(crate) fn named_color_to_hex(name: &str) -> Option<&'static str> {
    let n = name.trim().to_ascii_lowercase();
    Some(match n.as_str() {
        "white" => "#FFFFFF",
        "black" => "#000000",
        "red" => "#FF0000",
        "green" => "#008000",
        "blue" => "#0000FF",
        "yellow" => "#FFFF00",
        "orange" => "#FFA500",
        "purple" => "#800080",
        "gray" | "grey" => "#808080",
        "transparent" => "#00000000",
        "cyan" | "aqua" => "#00FFFF",
        "magenta" | "fuchsia" => "#FF00FF",
        "silver" => "#C0C0C0",
        "navy" => "#000080",
        "teal" => "#008080",
        "lime" => "#00FF00",
        "maroon" => "#800000",
        "olive" => "#808000",
        "pink" => "#FFC0CB",
        _ => return None,
    })
}

fn strip_numeric_units(s: &str) -> Option<f64> {
    let t = s.trim();
    let t = t
        .strip_suffix("px")
        .or_else(|| t.strip_suffix("deg"))
        .or_else(|| t.strip_suffix('s'))
        .or_else(|| t.strip_suffix('%'))
        .unwrap_or(t)
        .trim();
    t.parse::<f64>().ok()
}

/// `true` when the value carries a unit suffix (`px`, `deg`, `s`, `%`) and
/// therefore must be rewritten for strict parsers. Plain numeric strings are
/// valid HTML attribute values, so they are left alone to avoid diff churn.
fn has_unit_suffix(s: &str) -> bool {
    let t = s.trim();
    ["px", "deg", "s", "%"].iter().any(|u| {
        t.strip_suffix(u)
            .is_some_and(|v| v.trim().parse::<f64>().is_ok())
    })
}

fn is_hex_color(s: &str) -> bool {
    let t = s.trim();
    let body = t.strip_prefix('#').unwrap_or(t);
    matches!(body.len(), 3 | 4 | 6 | 8) && body.chars().all(|c| c.is_ascii_hexdigit())
}

/// Expand CSS shorthand hex (`#RGB`/`#RGBA`) to its long form. Returns `None`
/// when the input is not a shorthand color, so callers can leave it untouched.
fn expand_short_hex(s: &str) -> Option<String> {
    let t = s.trim();
    let body = t.strip_prefix('#')?;
    if !matches!(body.len(), 3 | 4) || !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let expanded: String = body.chars().flat_map(|c| [c, c]).collect();
    Some(format!("#{}", expanded.to_ascii_uppercase()))
}

/// Walk a JSON value and apply safe fixes, collecting them into `applied`.
pub fn fix_json_value(root: &mut Value) -> Vec<AppliedFix> {
    let mut applied = Vec::new();
    walk(root, "root", &mut applied);
    applied
}

fn walk(node: &mut Value, path: &str, applied: &mut Vec<AppliedFix>) {
    match node {
        Value::Object(map) => {
            // Numeric fields authored as strings with units.
            for key in NUMERIC_FIELDS {
                if let Some(Value::String(s)) = map.get(*key)
                    && let Some(n) = strip_numeric_units(s)
                {
                    let before = s.clone();
                    let after_value = if n.fract() == 0.0 && n.abs() < 9.0e15 {
                        Value::from(n as i64)
                    } else {
                        Value::from(n)
                    };
                    let after = after_value.to_string();
                    map.insert((*key).to_string(), after_value);
                    applied.push(AppliedFix {
                        path: path.to_string(),
                        field: (*key).to_string(),
                        before,
                        after,
                    });
                }
            }

            // Named colors in color fields, plus CSS shorthand hex normalization.
            for key in COLOR_FIELDS {
                if let Some(Value::String(s)) = map.get(*key) {
                    let replacement = if !is_hex_color(s) {
                        named_color_to_hex(s).map(|hex| hex.to_string())
                    } else {
                        expand_short_hex(s)
                    };
                    let Some(after) = replacement else { continue };
                    let before = s.clone();
                    map.insert((*key).to_string(), Value::String(after.clone()));
                    applied.push(AppliedFix {
                        path: path.to_string(),
                        field: (*key).to_string(),
                        before,
                        after,
                    });
                }
            }

            // position: "center"/"middle" -> anchor: "center".
            if let Some(Value::String(pos)) = map.get("position") {
                let lower = pos.trim().to_ascii_lowercase();
                if lower != "relative" && lower != "absolute" {
                    let anchor = match lower.as_str() {
                        "center" | "middle" | "centre" => Some("center"),
                        "top" => Some("top_center"),
                        "bottom" => Some("bottom_center"),
                        "left" => Some("center_left"),
                        "right" => Some("center_right"),
                        _ => None,
                    };
                    if let Some(anchor) = anchor {
                        let before = pos.clone();
                        map.remove("position");
                        map.entry("anchor".to_string())
                            .or_insert_with(|| Value::String(anchor.to_string()));
                        applied.push(AppliedFix {
                            path: path.to_string(),
                            field: "position".to_string(),
                            before,
                            after: format!("anchor: \"{anchor}\""),
                        });
                    }
                }
            }

            for (k, v) in map.iter_mut() {
                walk(v, &format!("{path}.{k}"), applied);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter_mut().enumerate() {
                walk(v, &format!("{path}[{i}]"), applied);
            }
        }
        _ => {}
    }
}

/// Apply fixes to a raw JSON string. Returns the pretty-printed result and the
/// list of fixes. Returns `None` when the input is not valid JSON (the caller
/// should report the syntax error instead).
///
/// When nothing needs fixing the original text is returned byte-for-byte, so
/// `--fix` does not churn formatting or float literals on already-valid scenes.
pub fn fix_json_str(raw: &str) -> Option<(String, Vec<AppliedFix>)> {
    let mut root: Value = serde_json::from_str(raw).ok()?;
    let applied = fix_json_value(&mut root);
    if applied.is_empty() {
        return Some((raw.to_string(), applied));
    }
    let rendered = serde_json::to_string_pretty(&root).ok()?;
    Some((rendered, applied))
}

/// Numeric HTML attribute names (kebab-case) that must be bare numbers.
const HTML_NUMERIC_ATTRS: &[&str] = &[
    "x",
    "y",
    "width",
    "height",
    "radius",
    "border-radius",
    "cx",
    "cy",
    "r",
    "opacity",
    "scale",
    "scale-x",
    "scale-y",
    "pivot-x",
    "pivot-y",
    "focal-x",
    "focal-y",
    "luma-key",
    "rotation",
    "border-width",
    "stroke-width",
    "font-size",
    "line-height",
    "letter-spacing",
    "blur",
    "shadow-blur",
    "shadow-x",
    "shadow-y",
    "skew-x",
    "skew-y",
    "rotate-x",
    "rotate-y",
    "perspective",
    "z-index",
    "z",
    "grain",
    "vignette",
    "halftone",
    "path-progress",
    "stroke-dashoffset",
    "overlap",
    "overlap-secs",
    "flex-grow",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "start",
    "start-sec",
    "end",
    "end-sec",
    "duration",
    "duration-secs",
    "stagger",
    "speed",
    "gravity",
    "spread",
    "angle",
    "size",
    "bar-gap",
    "volume",
    "playback-rate",
    "trim-start",
    "trim-end",
    "fade-in",
    "fade-out",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "gap",
    "row-gap",
    "column-gap",
    "max-width",
    "opacity-threshold",
];

/// Color-valued HTML attribute names.
const HTML_COLOR_ATTRS: &[&str] = &[
    "color",
    "background",
    "bg",
    "border-color",
    "fill",
    "stroke",
    "shadow-color",
    "outline-color",
];

fn html_anchor_from_position(v: &str) -> Option<&'static str> {
    match v.trim().to_ascii_lowercase().as_str() {
        "center" | "middle" | "centre" => Some("center"),
        "top" => Some("top_center"),
        "bottom" => Some("bottom_center"),
        "left" => Some("center_left"),
        "right" => Some("center_right"),
        _ => None,
    }
}

/// Parsed attribute token inside a tag body: leading whitespace is kept so the
/// original spacing survives, and `quote` records whether the value was quoted.
struct HtmlAttr<'a> {
    leading_ws: &'a str,
    name: &'a str,
    value: Option<&'a str>,
    quote: Option<char>,
}

fn parse_html_attrs(body: &str) -> Vec<HtmlAttr<'_>> {
    let bytes = body.as_bytes();
    let n = bytes.len();
    let mut attrs = Vec::new();
    let mut i = 0;

    while i < n {
        let ws_start = i;
        while i < n && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let leading_ws = &body[ws_start..i];

        if i >= n {
            // Trailing whitespace before `/>` or `>`: keep it verbatim.
            if !leading_ws.is_empty() {
                attrs.push(HtmlAttr {
                    leading_ws,
                    name: "",
                    value: None,
                    quote: None,
                });
            }
            break;
        }

        let name_start = i;
        while i < n && !matches!(bytes[i], b'=' | b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>') {
            i += 1;
        }
        let name = &body[name_start..i];

        if name.is_empty() {
            // Stray `>` or `/` inside the body (e.g. an unclosed tag): emit the
            // character verbatim and advance, so the loop always makes progress.
            attrs.push(HtmlAttr {
                leading_ws,
                name: &body[i..i + 1],
                value: None,
                quote: None,
            });
            i += 1;
            continue;
        }

        if i >= n || bytes[i] != b'=' {
            attrs.push(HtmlAttr {
                leading_ws,
                name,
                value: None,
                quote: None,
            });
            continue;
        }
        i += 1; // '='

        let (value, quote) = match bytes.get(i) {
            Some(b'"') | Some(b'\'') => {
                let q = bytes[i];
                let vstart = i + 1;
                i += 1;
                while i < n && bytes[i] != q {
                    i += 1;
                }
                let v = &body[vstart..i];
                i = (i + 1).min(n);
                (Some(v), Some(char::from(q)))
            }
            _ => {
                let vstart = i;
                while i < n && !matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b'>') {
                    i += 1;
                }
                (Some(&body[vstart..i]), None)
            }
        };

        attrs.push(HtmlAttr {
            leading_ws,
            name,
            value,
            quote,
        });
    }

    attrs
}

fn render_attr(name: &str, value: &str, quote: Option<char>) -> String {
    match quote {
        Some(q) => format!("{name}={q}{value}{q}"),
        None => format!("{name}={value}"),
    }
}

/// Rewrite attributes in a single tag body (`x="1" y="2"`), pushing any
/// applied fixes. Returns the rewritten attributes.
fn fix_html_attrs(tag: &str, path: &str, applied: &mut Vec<AppliedFix>) -> String {
    let mut out = String::with_capacity(tag.len() + 32);

    for attr in parse_html_attrs(tag) {
        out.push_str(attr.leading_ws);

        let Some(value) = attr.value else {
            out.push_str(attr.name);
            continue;
        };

        let lower = attr.name.to_ascii_lowercase();

        // The HTML compiler only reads kebab-case names; a snake_case attribute
        // is silently dropped, so rename it to the spelling that works.
        if let Some(kebab) = crate::source_map::kebab_case_for(&lower) {
            applied.push(AppliedFix {
                path: path.to_string(),
                field: attr.name.to_string(),
                before: attr.name.to_string(),
                after: kebab.to_string(),
            });
            out.push_str(&render_attr(kebab, value, attr.quote));
            continue;
        }

        if HTML_NUMERIC_ATTRS.contains(&lower.as_str())
            && has_unit_suffix(value)
            && let Some(num) = strip_numeric_units(value)
        {
            let after = if num.fract() == 0.0 && num.abs() < 9.0e15 {
                format!("{}", num as i64)
            } else {
                format!("{num}")
            };
            if after != value {
                applied.push(AppliedFix {
                    path: path.to_string(),
                    field: attr.name.to_string(),
                    before: value.to_string(),
                    after: after.clone(),
                });
                out.push_str(&render_attr(attr.name, &after, attr.quote));
                continue;
            }
        } else if HTML_COLOR_ATTRS.contains(&lower.as_str())
            && !is_hex_color(value)
            && let Some(hex) = named_color_to_hex(value)
        {
            applied.push(AppliedFix {
                path: path.to_string(),
                field: attr.name.to_string(),
                before: value.to_string(),
                after: hex.to_string(),
            });
            out.push_str(&render_attr(attr.name, hex, attr.quote));
            continue;
        } else if HTML_COLOR_ATTRS.contains(&lower.as_str())
            && let Some(hex) = expand_short_hex(value)
        {
            applied.push(AppliedFix {
                path: path.to_string(),
                field: attr.name.to_string(),
                before: value.to_string(),
                after: hex.clone(),
            });
            out.push_str(&render_attr(attr.name, &hex, attr.quote));
            continue;
        } else if lower == "position"
            && let Some(anchor) = html_anchor_from_position(value)
        {
            applied.push(AppliedFix {
                path: path.to_string(),
                field: "position".to_string(),
                before: value.to_string(),
                after: format!("anchor=\"{anchor}\""),
            });
            // `position` is a layout keyword in scenes; the author meant anchor.
            out.push_str(&render_attr("anchor", anchor, attr.quote.or(Some('"'))));
            continue;
        }

        // Preserve everything else verbatim.
        out.push_str(&render_attr(attr.name, value, attr.quote));
    }

    out
}

/// Apply conservative fixes to a raw HTML scene string. Attribute-level only:
/// the tag structure, whitespace, and comments are preserved. When nothing
/// needs fixing the input is returned byte-for-byte.
pub fn fix_html_str(raw: &str) -> (String, Vec<AppliedFix>) {
    let mut applied = Vec::new();
    let bytes = raw.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(n + 64);
    let mut i = 0;
    // Indices are per tag name, matching the paths used by `validate` warnings
    // (`rect[0].x`) rather than a document-wide counter.
    let mut tag_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    // Raw SVG content uses percentage lengths (cx="18%", r="55%"), so the
    // inside of an `<svg>` element is left byte-for-byte untouched.
    let mut svg_depth = 0usize;

    while i < n {
        if bytes[i] != b'<' {
            let start = i;
            i += 1;
            while i < n && bytes[i] != b'<' {
                i += 1;
            }
            out.push_str(&raw[start..i]);
            continue;
        }
        let start = i;
        i += 1;
        let mut in_quote: Option<u8> = None;
        while i < n {
            let b = bytes[i];
            if let Some(q) = in_quote {
                if b == q {
                    in_quote = None;
                }
            } else if b == b'"' || b == b'\'' {
                in_quote = Some(b);
            } else if b == b'>' {
                i += 1;
                break;
            }
            i += 1;
        }
        let tag = &raw[start..i];

        // Comments, doctypes and processing instructions are not attribute lists.
        let is_closing = tag.starts_with("</");
        // Closing tags (`</svg>`) must still yield the element name, otherwise
        // `svg_depth` never returns to 0 and every later tag is skipped.
        let name_from = if is_closing { 2 } else { 1 };
        let name_start = tag[name_from.min(tag.len())..]
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .map(|p| p + name_from)
            .unwrap_or(tag.len());
        let tag_name = tag[name_from.min(tag.len())..name_start].trim();
        if tag.starts_with("<!") || tag.starts_with("<?") || tag_name.is_empty() {
            out.push_str(tag);
            continue;
        }

        let is_self_closing = tag.trim_end().ends_with("/>");
        let is_svg = tag_name.eq_ignore_ascii_case("svg");

        if is_svg {
            if is_closing {
                svg_depth = svg_depth.saturating_sub(1);
            } else if !is_self_closing {
                svg_depth += 1;
            }
        }

        if is_svg || svg_depth > 0 {
            out.push_str(tag);
            continue;
        }

        let index = tag_index.entry(tag_name.to_string()).or_insert(0);
        let path = format!("{tag_name}[{index}]");
        *index += 1;

        // Keep the leading `<name` and trailing `>`/`/>` intact, rewrite the middle.
        let body_start = name_start;
        let body_end = tag
            .rfind('>')
            .map(|p| if tag[..p].ends_with('/') { p - 1 } else { p })
            .unwrap_or(tag.len());
        if body_end <= body_start {
            out.push_str(tag);
            continue;
        }
        out.push_str(&tag[..body_start]);
        out.push_str(&fix_html_attrs(
            &tag[body_start..body_end],
            &path,
            &mut applied,
        ));
        out.push_str(&tag[body_end..]);
    }

    (out, applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_paths_are_indexed_per_tag_name() {
        // Two rects plus a scene: `--fix` paths must match validate warning
        // paths (`rect[0]`, `rect[1]`), not a document-wide counter.
        let raw =
            r##"<scene width="640"><rect x="10px" y="20px"/><rect x="30px" color="red"/></scene>"##;
        let (_, applied) = fix_html_str(raw);
        assert_eq!(applied[0].path, "rect[0]");
        assert_eq!(applied[1].path, "rect[0]");
        assert_eq!(applied[2].path, "rect[1]");
    }

    #[test]
    fn fixes_numeric_units_and_named_colors() {
        let raw = r##"{"canvas":{"width":"1280px","height":"720px","fps":30,"duration_secs":"2.0s"},"layers":[{"type":"rect","x":"0px","y":"0px","width":"100px","height":"100px","color":"white"}]}"##;
        let (fixed, applied) = fix_json_str(raw).unwrap();
        assert!(
            applied
                .iter()
                .any(|f| f.path == "root.canvas" && f.field == "width" && f.after == "1280")
        );
        assert!(
            applied
                .iter()
                .any(|f| f.path == "root.layers[0]" && f.field == "color" && f.after == "#FFFFFF")
        );
        let v: Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(v["canvas"]["width"], 1280);
        assert_eq!(v["layers"][0]["color"], "#FFFFFF");
    }

    #[test]
    fn expands_shorthand_hex_colors() {
        let raw = r##"{"canvas":{"background":"#FFF"},"layers":[{"type":"rect","color":"#F00","border_color":"#0f08"}]}"##;
        let (fixed, applied) = fix_json_str(raw).unwrap();
        let v: Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(v["canvas"]["background"], "#FFFFFF");
        assert_eq!(v["layers"][0]["color"], "#FF0000");
        assert_eq!(v["layers"][0]["border_color"], "#00FF0088");
        assert_eq!(applied.len(), 3, "{applied:?}");
    }

    #[test]
    fn expands_shorthand_hex_in_html_attributes() {
        let raw = r##"<scene width="640"><rect x="0" y="0" width="20" height="20" color="#F00"/></scene>"##;
        let (fixed, applied) = fix_html_str(raw);
        assert!(fixed.contains(r##"color="#FF0000""##), "{fixed}");
        assert_eq!(applied.len(), 1, "{applied:?}");
        assert_eq!(applied[0].before, "#F00");
    }

    #[test]
    fn moves_invalid_position_into_anchor() {
        let raw = r#"{"layers":[{"type":"text","content":"hi","position":"center"}]}"#;
        let (fixed, applied) = fix_json_str(raw).unwrap();
        let v: Value = serde_json::from_str(&fixed).unwrap();
        let layer = &v["layers"][0];
        assert!(layer.get("position").is_none());
        assert_eq!(layer["anchor"], "center");
        assert!(applied.iter().any(|f| f.field == "position"));
    }

    #[test]
    fn leaves_valid_data_untouched() {
        let raw = r##"{"canvas":{"width":1280},"layers":[{"type":"rect","x":0,"color":"#FFFFFF","position":"absolute"}]}"##;
        let (fixed, applied) = fix_json_str(raw).unwrap();
        assert!(applied.is_empty());
        let v: Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(v["layers"][0]["color"], "#FFFFFF");
        assert_eq!(v["layers"][0]["position"], "absolute");
    }

    #[test]
    fn fixes_html_numeric_units_and_named_colors() {
        let raw = r#"<scene width="640" height="360" fps="30" duration="2.0">
  <rect x="10px" y="20px" width="100px" height="50px" color="white" border-color="red" />
</scene>"#;
        let (fixed, applied) = fix_html_str(raw);
        assert!(fixed.contains(r##"width="640""##)); // valid value untouched
        assert!(fixed.contains(r##"x="10""##), "{fixed}");
        assert!(fixed.contains(r##"color="#FFFFFF""##), "{fixed}");
        assert!(fixed.contains(r##"border-color="#FF0000""##), "{fixed}");
        assert!(applied.iter().any(|f| f.field == "x" && f.after == "10"));
        assert!(
            applied
                .iter()
                .any(|f| f.field == "color" && f.after == "#FFFFFF")
        );
        // Structure and whitespace are preserved.
        assert!(
            fixed.starts_with(
                "<scene width=\"640\" height=\"360\" fps=\"30\" duration=\"2.0\">\n  <rect"
            ),
            "{fixed}"
        );
        assert!(fixed.ends_with("</scene>"), "{fixed}");
    }

    #[test]
    fn fixes_html_position_into_anchor() {
        let raw = r#"<scene><text x="0" y="0" position="center">hi</text></scene>"#;
        let (fixed, applied) = fix_html_str(raw);
        assert!(!fixed.contains("position="), "{fixed}");
        assert!(fixed.contains(r##"anchor="center""##), "{fixed}");
        assert!(
            applied
                .iter()
                .any(|f| f.field == "position" && f.after.contains("center"))
        );
    }

    #[test]
    fn renames_snake_case_html_attributes() {
        let raw = "<scene width=\"640\">\n  <text font_size=\"32\" max_width=\"200\">Hi</text>\n</scene>";
        let (fixed, applied) = fix_html_str(raw);
        assert!(fixed.contains("font-size=\"32\""), "{fixed}");
        assert!(fixed.contains("max-width=\"200\""), "{fixed}");
        assert!(!fixed.contains("font_size"), "{fixed}");
        assert_eq!(applied.len(), 2);
        assert_eq!(applied[0].path, "text[0]");
        assert_eq!(applied[0].after, "font-size");
    }

    #[test]
    fn fixes_attributes_after_a_closing_svg_tag() {
        // A closing `</svg>` must reset `svg_depth`; otherwise every later tag
        // is treated as raw SVG and its fixes are skipped.
        let raw = "<container>\n  <svg width=\"32\" height=\"32\"><path d=\"M0 0\" /></svg>\n  <text font_size=\"22\">Hi</text>\n</container>";
        let (fixed, applied) = fix_html_str(raw);
        assert!(fixed.contains("font-size=\"22\""), "{fixed}");
        assert_eq!(applied.len(), 1);
    }

    #[test]
    fn leaves_valid_html_untouched() {
        let raw = r##"<scene width="640" height="360"><rect x="10" y="20" color="#FFFFFF" position="absolute" /></scene>"##;
        let (fixed, applied) = fix_html_str(raw);
        assert!(applied.is_empty(), "{applied:?}");
        assert_eq!(fixed, raw);
    }

    #[test]
    fn keeps_html_comments_and_doctype_untouched() {
        let raw = "<!DOCTYPE html><!-- x=\"1px\" --><scene><rect x=\"2px\" /></scene>";
        let (fixed, applied) = fix_html_str(raw);
        assert!(
            fixed.starts_with("<!DOCTYPE html><!-- x=\"1px\" -->"),
            "{fixed}"
        );
        assert!(fixed.contains(r##"x="2""##), "{fixed}");
        assert_eq!(applied.len(), 1);
    }

    #[test]
    fn leaves_svg_content_untouched() {
        let raw = r##"<scene><svg x="10" width="200" height="120" src="<svg><radialGradient id="g" cx="18%" cy="22%" r="55%"/></svg>"/><rect x="4px" /></scene>"##;
        let (fixed, applied) = fix_html_str(raw);
        assert!(fixed.contains("cx=\"18%\""), "{fixed}");
        assert!(fixed.contains("x=\"4\""), "{fixed}");
        assert_eq!(applied.len(), 1, "{applied:?}");
    }

    #[test]
    fn does_not_hang_on_unclosed_or_odd_tags() {
        for raw in [
            r#"<scene><text x="1">a<b</text></scene>"#,
            "<scene><text x=\"1\">a<b</text><rect x=\"2px\"/></scene>",
            "<scene></scene>>",
            "<scene></>",
        ] {
            let (fixed, applied) = fix_html_str(raw);
            assert!(!fixed.is_empty());
            let _ = applied;
        }
    }
}
