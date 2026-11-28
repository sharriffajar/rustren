//! Source diagnostics for HTML scenes.
//!
//! HTML parsing goes through `scraper`, which does not expose source spans. To
//! still report `line`/`column`, this module keeps a lightweight scan of the
//! original text: tags, attributes and their byte offsets, mapped to
//! line/column numbers.
//!
//! It also powers [`lint_html_source`], which flags attribute values the
//! compiler would silently ignore (for example `x="100px"`, which `f32::parse`
//! cannot read, so the layer falls back to `0`).

/// An attribute found inside a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrRef {
    /// Attribute name, lowercased.
    pub name: String,
    /// Raw value as written (quotes removed). `None` for bare boolean attrs.
    pub value: Option<String>,
    /// Byte offset where the value starts (after the opening quote).
    pub value_offset: usize,
}

/// A tag occurrence found by [`scan_tags`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRef {
    /// Lowercased tag name (`scene`, `rect`, ...). Empty for a bare `<>`.
    pub name: String,
    /// Byte offset of the opening `<`.
    pub start: usize,
    /// `true` when this is a closing tag (`</rect>`).
    pub closing: bool,
    pub attrs: Vec<AttrRef>,
}

impl TagRef {
    pub fn attr(&self, name: &str) -> Option<&AttrRef> {
        self.attrs.iter().find(|a| a.name == name)
    }
}

/// Maps byte offsets to 1-based line and column numbers.
#[derive(Debug, Clone)]
pub struct SourceMap {
    line_starts: Vec<usize>,
    len: usize,
}

impl SourceMap {
    pub fn new(src: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (idx, b) in src.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self {
            line_starts,
            len: src.len(),
        }
    }

    /// 1-based `(line, column)` for a byte offset. Offsets past the end clamp
    /// to the end of the source.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.len);
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        (line_idx + 1, offset - line_start + 1)
    }

    /// Format `offset` as `line L, column C`.
    pub fn loc(&self, offset: usize) -> String {
        let (line, column) = self.line_col(offset);
        format!("line {line}, column {column}")
    }
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.')
}

/// Scan `src` for tags, skipping comments, doctypes and processing instructions.
pub fn scan_tags(src: &str) -> Vec<TagRef> {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut tags = Vec::new();
    let mut i = 0;

    while i < n {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        if src[i..].starts_with("<!--") {
            i = src[i..].find("-->").map(|p| i + p + 3).unwrap_or(n);
            continue;
        }
        if bytes.get(i + 1) == Some(&b'!') || bytes.get(i + 1) == Some(&b'?') {
            i = src[i..].find('>').map(|p| i + p + 1).unwrap_or(n);
            continue;
        }

        let closing = src[i..].starts_with("</");
        let start = i;
        let mut j = i + if closing { 2 } else { 1 };
        let name_start = j;
        while j < n && is_name_byte(bytes[j]) {
            j += 1;
        }
        let name = src[name_start..j].to_ascii_lowercase();
        let mut attrs = Vec::new();
        let mut in_quote: Option<u8> = None;

        while j < n {
            let b = bytes[j];
            if let Some(q) = in_quote {
                j += 1;
                if b == q {
                    in_quote = None;
                }
                continue;
            }
            if b == b'"' || b == b'\'' {
                in_quote = Some(b);
                j += 1;
                continue;
            }
            if b == b'>' {
                j += 1;
                break;
            }
            if b.is_ascii_whitespace() || b == b'/' {
                j += 1;
                continue;
            }

            // Attribute name.
            let aname_start = j;
            while j < n
                && !bytes[j].is_ascii_whitespace()
                && !matches!(bytes[j], b'=' | b'>' | b'/')
            {
                j += 1;
            }
            let aname = src[aname_start..j].to_ascii_lowercase();
            if aname.is_empty() {
                j += 1;
                continue;
            }

            // Optional `= value`.
            let mut k = j;
            while k < n && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k >= n || bytes[k] != b'=' {
                attrs.push(AttrRef {
                    name: aname,
                    value: None,
                    value_offset: j,
                });
                continue;
            }
            k += 1;
            while k < n && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k >= n {
                break;
            }

            match bytes[k] {
                b'"' | b'\'' => {
                    let q = bytes[k];
                    let vstart = k + 1;
                    let mut vend = vstart;
                    while vend < n && bytes[vend] != q {
                        vend += 1;
                    }
                    attrs.push(AttrRef {
                        name: aname,
                        value: Some(src[vstart..vend].to_string()),
                        value_offset: vstart,
                    });
                    j = (vend + 1).min(n);
                }
                _ => {
                    let vstart = k;
                    let mut vend = vstart;
                    while vend < n
                        && !bytes[vend].is_ascii_whitespace()
                        && !matches!(bytes[vend], b'>')
                    {
                        vend += 1;
                    }
                    attrs.push(AttrRef {
                        name: aname,
                        value: Some(src[vstart..vend].to_string()),
                        value_offset: vstart,
                    });
                    j = vend;
                }
            }
        }

        tags.push(TagRef {
            name,
            start,
            closing,
            attrs,
        });
        i = j;
    }

    tags
}

/// A problem found by scanning HTML source directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIssue {
    /// Stable warning code, e.g. `unparsed_attribute`.
    pub code: &'static str,
    /// Offending tag, e.g. `rect[1] x`.
    pub path: String,
    /// Human-readable message, prefixed with `line L, column C`.
    pub message: String,
    pub fix: Option<String>,
}

/// HTML attribute names the compiler reads as numbers (`get_attr_f32`).
const NUMERIC_ATTRIBUTES: &[&str] = &[
    "angle",
    "bar-gap",
    "bars",
    "blur",
    "border-radius",
    "border-width",
    "clip-feather",
    "column-gap",
    "count",
    "cx",
    "cy",
    "decimals",
    "delay",
    "dur",
    "duration",
    "duration-sec",
    "duration-secs",
    "duration_secs",
    "dynamic-end",
    "dynamic-start",
    "echo-count",
    "echo-decay",
    "echo-step",
    "echo_count",
    "echo_decay",
    "echo_step",
    "end",
    "end-sec",
    "fade-in",
    "fade-in-secs",
    "fade-out",
    "fade-out-secs",
    "flex-grow",
    "font-size",
    "fov",
    "fps",
    "from",
    "grain",
    "gravity",
    "h",
    "halftone",
    "height",
    "letter-spacing",
    "line-height",
    "margin",
    "max-width",
    "offset",
    "opacity",
    "outline-width",
    "overlap",
    "overlap-secs",
    "padding",
    "path-progress",
    "perspective",
    "playback-rate",
    "r",
    "radius",
    "rotate-x",
    "rotate-y",
    "rotate-z",
    "rotate_x",
    "rotate_y",
    "rotation",
    "row-gap",
    "scale",
    "scale-x",
    "scale-y",
    "scale_x",
    "scale_y",
    "seed",
    "shadow-blur",
    "shadow-x",
    "shadow-y",
    "size",
    "skew-x",
    "skew-y",
    "skew_x",
    "skew_y",
    "speed",
    "spread",
    "stagger",
    "stagger-secs",
    "start",
    "start-sec",
    "stroke-dashoffset",
    "stroke-width",
    "to",
    "trim-end",
    "trim-end-sec",
    "trim-start",
    "trim-start-sec",
    "vignette",
    "volume",
    "w",
    "width",
    "x",
    "y",
    "z",
    "z-index",
    "z_index",
    "wave-warp-amplitude",
    "wave-warp-wavelength",
    "wave-warp-speed",
    "wave-warp-phase",
    "text-animator-start",
    "text-animator-stagger",
    "text-animator-duration",
    "text-animator-y-offset",
    "text-animator-x-offset",
    "text-animator-opacity-from",
    "text-animator-scale-from",
];

/// Attributes that may hold a CSS box shorthand (`"4 10"`, `"1 2 3 4"`).
const BOX_SHORTHAND_ATTRIBUTES: &[&str] = &["padding", "margin", "gap"];

/// Tags whose attributes are raw SVG/HTML, not scene authoring.
fn is_opaque_tag(name: &str) -> bool {
    matches!(
        name,
        "html" | "head" | "body" | "meta" | "link" | "script" | "style"
    )
}

/// Known JSON/schema field names that the HTML compiler expects in kebab-case.
///
/// Authors (especially agents trained on JSON scenes) often write `font_size=`,
/// which the HTML compiler silently drops and falls back to a default. This
/// list maps each silently-dropped snake_case name to the kebab-case name that
/// actually works, so `validate` can warn and `--fix` can rewrite it.
pub const SNAKE_CASE_ATTRIBUTES: &[(&str, &str)] = &[
    ("font_size", "font-size"),
    ("font_weight", "font-weight"),
    ("font_family", "font-family"),
    ("letter_spacing", "letter-spacing"),
    ("line_height", "line-height"),
    ("text_align", "text-align"),
    ("max_width", "max-width"),
    ("border_radius", "border-radius"),
    ("border_width", "border-width"),
    ("blend_mode", "blend-mode"),
    ("box_shadow", "box-shadow"),
    ("flex_direction", "flex-direction"),
    ("flex_wrap", "flex-wrap"),
    ("flex_grow", "flex-grow"),
    ("justify_content", "justify-content"),
    ("align_items", "align-items"),
    ("row_gap", "row-gap"),
    ("column_gap", "column-gap"),
    ("shadow_color", "shadow-color"),
    ("shadow_blur", "shadow-blur"),
    ("shadow_x", "shadow-x"),
    ("shadow_y", "shadow-y"),
    ("outline_color", "outline-color"),
    ("outline_width", "outline-width"),
    ("stroke_width", "stroke-width"),
    ("stroke_dasharray", "stroke-dasharray"),
    ("trim_start", "trim-start"),
    ("trim_end", "trim-end"),
    ("fade_in", "fade-in"),
    ("fade_out", "fade-out"),
    ("playback_rate", "playback-rate"),
    ("bar_gap", "bar-gap"),
    ("start_sec", "start-sec"),
    ("end_sec", "end-sec"),
    ("stagger_secs", "stagger-secs"),
    ("dynamic_start", "dynamic-start"),
    ("dynamic_end", "dynamic-end"),
    ("trim_start_sec", "trim-start-sec"),
    ("trim_end_sec", "trim-end-sec"),
];

/// Look up the kebab-case spelling for a silently-dropped snake_case attribute.
pub fn kebab_case_for(name: &str) -> Option<&'static str> {
    SNAKE_CASE_ATTRIBUTES
        .iter()
        .find(|(snake, _)| *snake == name)
        .map(|(_, kebab)| *kebab)
}

/// Scan HTML source for values the compiler would silently ignore or that the
/// renderer would reject, with precise `line`/`column` locations.
///
/// This is intentionally conservative: it only reports cases where the author
/// clearly meant a different format (`"48px"`, `"white"`, `"18%"` in a non-SVG
/// numeric attribute).
pub fn lint_html_source(src: &str) -> Vec<SourceIssue> {
    let map = SourceMap::new(src);
    let tags = scan_tags(src);
    let mut issues = Vec::new();
    let mut counters: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    // Inside `<svg>` raw markup, percentage and CSS lengths are normal.
    let mut svg_depth = 0usize;
    // `src="<svg ...>"` attributes hold a whole SVG document.
    let mut raw_svg_attr = false;

    // `scraper` always synthesizes <body>, so a document without <scene> or
    // <component> silently falls back to 1280x720 @ 5s defaults. Point at it.
    let has_root = tags
        .iter()
        .any(|t| !t.closing && matches!(t.name.as_str(), "scene" | "component" | "body" | "html"));
    if !has_root {
        let loc = match tags.iter().find(|t| !t.closing && !t.name.is_empty()) {
            Some(t) => map.loc(t.start),
            None => match src.find(|c: char| !c.is_whitespace()) {
                Some(off) => map.loc(off),
                None => "line 1, column 1".to_string(),
            },
        };
        let first = tags
            .iter()
            .find(|t| !t.closing && !t.name.is_empty())
            .map(|t| format!("<{}>", t.name))
            .unwrap_or_else(|| "no tags".to_string());
        issues.push(SourceIssue {
            code: "missing_root_tag",
            path: "document".to_string(),
            message: format!(
                "{loc}: no <scene> or <component> root tag found (first tag: {first}); canvas falls back to 1280x720 @ 30fps, 5s"
            ),
            fix: Some("wrap the layers in <scene width= height= fps= duration=>".to_string()),
        });
    }

    for tag in &tags {
        if tag.closing {
            if tag.name == "svg" {
                svg_depth = svg_depth.saturating_sub(1);
            }
            continue;
        }

        if tag.name == "svg" {
            let has_src = tag
                .attr("src")
                .and_then(|a| a.value.as_deref())
                .is_some_and(|v| v.trim_start().starts_with('<'));
            if !has_src {
                svg_depth += 1;
                continue;
            }
            raw_svg_attr = true;
        } else if svg_depth > 0 {
            continue;
        }

        if is_opaque_tag(&tag.name) {
            continue;
        }

        let index = counters.entry(tag.name.as_str()).or_insert(0);
        let path_prefix = format!("{}[{}]", tag.name, *index);
        *index += 1;

        if raw_svg_attr {
            raw_svg_attr = false;
            continue; // the `src` payload is scanned as its own document
        }

        for attr in &tag.attrs {
            let Some(value) = attr.value.as_deref() else {
                continue;
            };
            let name = attr.name.as_str();

            // `font_size=` is valid in JSON but silently ignored in HTML, where
            // the compiler only reads `font-size`. Warn instead of falling back
            // to a default without telling the author.
            if let Some(kebab) = kebab_case_for(name) {
                issues.push(SourceIssue {
                    code: "snake_case_attribute",
                    path: format!("{path_prefix}.{name}"),
                    message: format!(
                        "{}: attribute {name}=\"{value}\" is ignored by the HTML compiler; use the kebab-case form {kebab}=\"{value}\"",
                        map.loc(attr.value_offset)
                    ),
                    fix: Some(format!("rename {name} to {kebab}")),
                });
                continue;
            }

            if BOX_SHORTHAND_ATTRIBUTES.contains(&name) {
                if value.split_whitespace().all(|p| p.parse::<f32>().is_ok()) {
                    continue;
                }
                if name == "gap" {
                    continue; // `gap` also accepts row/column pairs elsewhere
                }
            } else if NUMERIC_ATTRIBUTES.contains(&name) && name != "gap" {
                if value.parse::<f32>().is_ok() {
                    continue;
                }
                let trimmed = value.trim();
                let unit = trimmed
                    .strip_suffix("px")
                    .or_else(|| trimmed.strip_suffix("deg"))
                    .or_else(|| trimmed.strip_suffix('%'))
                    .or_else(|| trimmed.strip_suffix('s'));
                let Some(raw) = unit.filter(|u| u.trim().parse::<f32>().is_ok()) else {
                    continue; // not a number with a unit: leave it to other checks
                };
                issues.push(SourceIssue {
                    code: "unparsed_attribute",
                    path: format!("{path_prefix}.{name}"),
                    message: format!(
                        "{}: numeric attribute {name}=\"{value}\" is not a number, the compiler ignores it (falls back to the default)",
                        map.loc(attr.value_offset)
                    ),
                    fix: Some(format!("write {name}=\"{}\" (no unit)", raw.trim())),
                });
                continue;
            }
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_offsets_to_line_and_column() {
        let src = "<scene>\n  <rect/>\n</scene>";
        let map = SourceMap::new(src);
        assert_eq!(map.line_col(0), (1, 1));
        let rect = src.find("<rect").unwrap();
        assert_eq!(map.line_col(rect), (2, 3));
        assert_eq!(map.loc(rect), "line 2, column 3");
    }

    #[test]
    fn skips_comments_and_doctypes() {
        let src = "<!DOCTYPE html><!-- <rect/> -->\n<scene><text/></scene>";
        let tags = scan_tags(src);
        let names: Vec<_> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["scene", "text", "scene"]);
        assert!(!tags.iter().any(|t| t.name == "rect"));
    }

    #[test]
    fn tracks_closing_tags_and_quoted_gt() {
        let src = r#"<scene><svg src="<svg><rect/></svg>"/></scene>"#;
        let tags = scan_tags(src);
        assert_eq!(tags.len(), 3, "{tags:?}");
        assert_eq!(tags[0].name, "scene");
        assert_eq!(tags[1].name, "svg");
        assert!(tags[2].closing);
        assert_eq!(
            tags[1].attr("src").unwrap().value.as_deref(),
            Some("<svg><rect/></svg>")
        );
    }

    #[test]
    fn captures_attribute_names_values_and_offsets() {
        let src = r#"<scene width="640" duration=2><rect x='10px' muted/></scene>"#;
        let tags = scan_tags(src);
        let scene = &tags[0];
        assert_eq!(scene.attr("width").unwrap().value.as_deref(), Some("640"));
        assert_eq!(scene.attr("duration").unwrap().value.as_deref(), Some("2"));
        let rect = &tags[1];
        assert_eq!(rect.attr("x").unwrap().value.as_deref(), Some("10px"));
        assert_eq!(rect.attr("muted").unwrap().value, None);
        let x_off = rect.attr("x").unwrap().value_offset;
        assert_eq!(&src[x_off..x_off + 4], "10px");
    }

    #[test]
    fn flags_unparsed_units() {
        let src = "<scene width=\"640\">\n  <rect x=\"40px\" y=\"12\" />\n</scene>";
        let issues = lint_html_source(src);
        let codes: Vec<_> = issues.iter().map(|i| i.code).collect();
        assert_eq!(codes, ["unparsed_attribute"], "{issues:?}");
        assert!(
            issues[0].message.starts_with("line 2, column 12"),
            "{}",
            issues[0].message
        );
        assert_eq!(issues[0].path, "rect[0].x");
    }

    #[test]
    fn accepts_named_colors_and_rgba() {
        // `parse_hex_color` resolves named colors, so they are not warnings.
        let src = "<scene><rect color=\"white\" background=\"rgba(0, 0, 0, 0.5)\"/></scene>";
        let issues = lint_html_source(src);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn accepts_valid_values_and_box_shorthands() {
        let src = "<scene><container padding=\"4 10\" x=\"40\" color=\"#FFF\"/></scene>";
        let issues = lint_html_source(src);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn flags_snake_case_attributes_with_kebab_fix() {
        let src = "<scene width=\"640\">\n  <text font_size=\"32\" max_width=\"200\"/></scene>";
        let issues = lint_html_source(src);
        let codes: Vec<_> = issues.iter().map(|i| i.code).collect();
        assert_eq!(codes, ["snake_case_attribute", "snake_case_attribute"]);
        assert!(
            issues[0].message.contains("line 2, column 20"),
            "{}",
            issues[0].message
        );
        assert!(
            issues[0].message.contains("font-size=\"32\""),
            "{}",
            issues[0].message
        );
        assert_eq!(issues[0].path, "text[0].font_size");
        assert_eq!(
            issues[0].fix.as_deref(),
            Some("rename font_size to font-size")
        );
        assert_eq!(issues[1].path, "text[0].max_width");
    }

    #[test]
    fn accepted_snake_case_aliases_are_not_flagged() {
        // The HTML compiler genuinely reads these snake_case spellings, and
        // animation properties accept both spellings too.
        let src = "<scene><container z_index=\"2\" rotate_x=\"5\" track_matte=\"alpha\"/></scene>";
        let issues = lint_html_source(src);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn ignores_raw_svg_content() {
        let src = r#"<scene><svg src="<svg><radialGradient cx='18%' r='55%'/></svg>"/></scene>"#;
        let issues = lint_html_source(src);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
