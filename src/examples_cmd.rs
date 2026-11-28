//! Curated, self-contained scenes for `rustren examples`.
//!
//! Agents do worse when they have to guess a working scene shape from prose,
//! so this command keeps a handful of minimal, copy-ready snippets embedded in
//! the binary. Every snippet is compiled and validated by a conformance test,
//! so they cannot drift away from the real schema.

/// One copy-ready scene example.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Example {
    /// Stable identifier used by `rustren examples <name>`.
    pub name: &'static str,
    /// One-line description of what the snippet shows.
    pub description: &'static str,
    /// `html` or `json`.
    pub format: &'static str,
    /// Search tags, e.g. `["text", "animation"]`.
    pub tags: &'static [&'static str],
    /// The full scene source, ready to save to a file.
    pub source: &'static str,
}

pub const EXAMPLES: &[Example] = &[
    Example {
        name: "minimal-html",
        description: "Smallest valid HTML scene: one rect, one text layer, nothing else.",
        format: "html",
        tags: &["basics", "text"],
        source: r##"<scene width="1280" height="720" fps="30" duration="3.0" background="#0F172A">
  <rect x="0" y="0" width="1280" height="720" color="#0F172A" />
  <text x="120" y="300" text="Hello from rustren" font-size="64" font-weight="bold" color="#FFFFFF" />
</scene>
"##,
    },
    Example {
        name: "container-flex",
        description: "Container with column flex layout, padding, gap and wrapping text.",
        format: "html",
        tags: &["layout", "container", "text"],
        source: r##"<scene width="1280" height="720" fps="30" duration="4.0" background="#0B1220">
  <container x="200" y="240" width="880" height="240" layout="column"
             padding="24" gap="12" radius="16" background="#1E293B">
    <text font-size="40" font-weight="bold" color="#FFFFFF" max-width="832">Container layout</text>
    <text font-size="20" color="#94A3B8" max-width="832">Children stack with gap and padding. Text needs max-width to wrap correctly.</text>
  </container>
</scene>
"##,
    },
    Example {
        name: "animate-fade",
        description: "Layer-scoped animation: fade and slide in, then hold.",
        format: "html",
        tags: &["animation", "timeline"],
        source: r##"<scene width="1280" height="720" fps="30" duration="3.0" background="#0F172A">
  <rect x="0" y="0" width="1280" height="720" color="#0F172A" />
  <text x="120" y="320" text="Animated title" font-size="72" font-weight="bold" color="#FFFFFF">
    <animate property="opacity" from="0" to="1" start="0.2" end="1.0" easing="ease_out" />
    <animate property="y" from="360" to="320" start="0.2" end="1.0" easing="ease_out" />
  </text>
</scene>
"##,
    },
    Example {
        name: "effects-particles",
        description: "Deterministic particle burst plus grain, vignette and halftone on a card.",
        format: "html",
        tags: &["effects", "particles", "grain"],
        source: r##"<scene width="1280" height="720" fps="30" duration="4.0" background="#070A12">
  <rect x="0" y="0" width="1280" height="720" gradient="linear-gradient(180deg, #070a12 0%, #0f172a 60%, #070a12 100%)" vignette="0.4" />
  <rect x="340" y="220" width="600" height="280" radius="18" color="#1E293B" grain="0.12" halftone="5" />
  <particles x="260" y="180" width="760" height="360" count="60" seed="2026" shape="confetti"
             speed="2.1" gravity="160" spread="150" angle="-90" size="13" start="0.6" />
</scene>
"##,
    },
    Example {
        name: "vertical-captions",
        description: "9:16 safe layout with a caption pill and a lower third.",
        format: "html",
        tags: &["9:16", "captions", "text"],
        source: r##"<scene width="1080" height="1920" fps="30" duration="5.0" background="#0B0F19">
  <rect x="0" y="0" width="1080" height="1920" color="#0B0F19" />
  <container x="90" y="1360" width="900" layout="column" gap="10"
             padding="28" radius="20" background="#111827" opacity="0.92">
    <text font-size="34" font-weight="bold" color="#38BDF8" max-width="844">Speaker name</text>
    <text font-size="46" color="#FFFFFF" max-width="844">Caption line that stays inside the safe area.</text>
  </container>
</scene>
"##,
    },
    Example {
        name: "minimal-json",
        description: "The same idea in JSON-schema form: canvas plus rect and text layers.",
        format: "json",
        tags: &["basics", "json"],
        source: r##"{
  "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 3.0, "background": "#0F172A" },
  "layers": [
    { "type": "rect", "x": 0, "y": 0, "width": 1280, "height": 720, "color": "#0F172A" },
    { "type": "text", "x": 120, "y": 300, "content": "Hello from rustren",
      "font_size": 64, "font_weight": "bold", "color": "#FFFFFF", "max_width": 900 }
  ]
}
"##,
    },
];

/// Look an example up by name.
pub fn find(name: &str) -> Option<&'static Example> {
    EXAMPLES.iter().find(|e| e.name == name)
}

/// Examples filtered by an optional tag, in declaration order.
pub fn by_tag(tag: Option<&str>) -> Vec<&'static Example> {
    match tag {
        Some(tag) => EXAMPLES.iter().filter(|e| e.tags.contains(&tag)).collect(),
        None => EXAMPLES.iter().collect(),
    }
}

/// Names of every distinct tag, for `structured` listings and error hints.
pub fn all_tags() -> Vec<&'static str> {
    let mut tags: Vec<&'static str> = EXAMPLES
        .iter()
        .flat_map(|e| e.tags.iter().copied())
        .collect();
    tags.sort_unstable();
    tags.dedup();
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_non_empty() {
        let mut names: Vec<_> = EXAMPLES.iter().map(|e| e.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), EXAMPLES.len());
        assert!(
            EXAMPLES
                .iter()
                .all(|e| !e.name.is_empty() && !e.source.is_empty())
        );
    }

    #[test]
    fn every_example_compiles_and_validates() {
        for example in EXAMPLES {
            let scene = match example.format {
                "html" => crate::compilers::html::parse_html_scene(example.source)
                    .unwrap_or_else(|e| panic!("{} does not compile: {e:#}", example.name)),
                "json" => crate::compilers::json::parse_json_scene_with_base(
                    example.source,
                    None,
                    std::path::Path::new("example.json"),
                )
                .unwrap_or_else(|e| panic!("{} does not compile: {e:#}", example.name)),
                other => panic!("{} has unknown format {other}", example.name),
            };
            crate::capabilities::validate_scene_capabilities(&scene)
                .unwrap_or_else(|e| panic!("{} fails capabilities: {e:#}", example.name));
            assert!(!scene.layers.is_empty(), "{} has no layers", example.name);
        }
    }

    #[test]
    fn json_example_has_no_unknown_fields() {
        for example in EXAMPLES.iter().filter(|e| e.format == "json") {
            crate::lint::validate_no_unknown_fields(example.source)
                .unwrap_or_else(|e| panic!("{} has unknown fields: {e:#}", example.name));
        }
    }
}
