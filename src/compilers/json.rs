//! JSON -> CompiledScene compiler.
//!
//! Mendukung:
//! - Standard `Scene` JSON
//! - Reusable component & nested scene via `{"type": "include", "src": "card.json", "vars": {...}}`
//! - Template variable substitution `{{var}}` dan `{{var|default}}`
//! - Recursion depth guard (`MAX_INCLUDE_DEPTH = 8`)

use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use super::html::substitute_variables;
use super::Compiler;
use crate::ir::CompiledScene;
use crate::schema::Scene;

const MAX_INCLUDE_DEPTH: usize = 8;

pub struct JsonCompiler;

impl Compiler for JsonCompiler {
    fn compile(&self, path: &Path) -> Result<CompiledScene> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read JSON file: {:?}", path))?;

        let scene = parse_json_scene_with_base(&content, path.parent(), path)?;
        Ok(CompiledScene::Static(scene))
    }

    fn format_name(&self) -> &'static str {
        "json"
    }
}

pub fn parse_json_scene_with_base(
    content: &str,
    base_dir: Option<&Path>,
    source_path: &Path,
) -> Result<Scene> {
    let mut root: Value = serde_json::from_str(content).map_err(|e| {
        anyhow::anyhow!(
            "JSON syntax error in {:?} (line {}, column {}): {}",
            source_path,
            e.line(),
            e.column(),
            e
        )
    })?;

    if let Some(layers) = root.get_mut("layers").and_then(|v| v.as_array_mut()) {
        let expanded = expand_json_layers(layers, base_dir, 0, &HashMap::new())?;
        *layers = expanded;
    }

    let scene: Scene = {
        let mut track = serde_path_to_error::Track::new();
        let de = serde_path_to_error::Deserializer::new(root.clone(), &mut track);
        use serde::Deserialize;
        Scene::deserialize(de).map_err(|e| {
            let path = track.path().to_string();
            // Best-effort line/column: locate the offending path in the source
            // text by re-parsing the raw JSON with a position-tracking pass.
            let loc = json_value_position(content, &path)
                .map(|(line, col)| format!(" (line {line}, column {col})"))
                .unwrap_or_default();
            anyhow::anyhow!(
                "JSON schema error in {:?} at `{}`{}: {}",
                source_path,
                path,
                loc,
                e
            )
        })?
    };
    Ok(scene)
}

/// Find the `line`/`column` of the value at a serde path such as
/// `layers[0].x` or `canvas.width` by scanning the raw JSON text.
///
/// Returns `None` when the path cannot be resolved (for example when an
/// `<include>` expansion changed the shape of the document).
fn json_value_position(content: &str, path: &str) -> Option<(usize, usize)> {
    // Walk the raw text once, tracking the byte offset of every key/value the
    // same way serde_path_to_error names them.
    let bytes = content.as_bytes();
    let mut offsets: Vec<(String, usize)> = Vec::new();

    fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    }

    // Recursive descent that records `path -> value offset` for scalars.
    fn walk(
        bytes: &[u8],
        i: &mut usize,
        path: &mut String,
        offsets: &mut Vec<(String, usize)>,
    ) -> Option<()> {
        *i = skip_ws(bytes, *i);
        offsets.push((path.clone(), *i));
        match *bytes.get(*i)? {
            b'{' => {
                *i += 1;
                loop {
                    *i = skip_ws(bytes, *i);
                    match *bytes.get(*i)? {
                        b'}' => {
                            *i += 1;
                            return Some(());
                        }
                        b',' => {
                            *i += 1;
                            continue;
                        }
                        b'"' => {
                            let key = scan_string(bytes, i)?;
                            *i = skip_ws(bytes, *i);
                            // ':'
                            *i += 1;
                            let saved = path.len();
                            if path.is_empty() {
                                path.push_str(&key);
                            } else {
                                path.push('.');
                                path.push_str(&key);
                            }
                            walk(bytes, i, path, offsets)?;
                            path.truncate(saved);
                        }
                        _ => return Some(()),
                    }
                    *i = skip_ws(bytes, *i);
                }
            }
            b'[' => {
                *i += 1;
                let mut index = 0usize;
                loop {
                    *i = skip_ws(bytes, *i);
                    match *bytes.get(*i)? {
                        b']' => {
                            *i += 1;
                            return Some(());
                        }
                        b',' => {
                            *i += 1;
                            continue;
                        }
                        _ => {
                            let saved = path.len();
                            path.push('[');
                            path.push_str(&index.to_string());
                            path.push(']');
                            walk(bytes, i, path, offsets)?;
                            path.truncate(saved);
                            index += 1;
                        }
                    }
                    *i = skip_ws(bytes, *i);
                }
            }
            _ => {
                skip_scalar(bytes, i)?;
                Some(())
            }
        }
    }

    fn scan_string(bytes: &[u8], i: &mut usize) -> Option<String> {
        let q = *bytes.get(*i)?;
        if q != b'"' {
            return None;
        }
        *i += 1;
        let mut out = String::new();
        while let Some(&b) = bytes.get(*i) {
            match b {
                b'"' => {
                    *i += 1;
                    return Some(out);
                }
                b'\\' => {
                    out.push('\\');
                    *i += 1;
                    if let Some(&esc) = bytes.get(*i) {
                        out.push(esc as char);
                        *i += 1;
                    }
                }
                _ => {
                    out.push(b as char);
                    *i += 1;
                }
            }
        }
        None
    }

    fn skip_scalar(bytes: &[u8], i: &mut usize) -> Option<()> {
        match *bytes.get(*i)? {
            b'"' => {
                scan_string(bytes, i)?;
                Some(())
            }
            _ => {
                while let Some(&b) = bytes.get(*i) {
                    if b == b',' || b == b'}' || b == b']' || b.is_ascii_whitespace() {
                        break;
                    }
                    *i += 1;
                }
                Some(())
            }
        }
    }

    let mut i = 0usize;
    let mut p = String::new();
    walk(bytes, &mut i, &mut p, &mut offsets)?;

    let target = offsets
        .iter()
        .find(|(k, _)| k == path)
        .or_else(|| offsets.iter().find(|(k, _)| k.starts_with(path)))?;
    let offset = target.1;

    let mut line = 1usize;
    let mut col = 1usize;
    for b in &bytes[..offset.min(bytes.len())] {
        if *b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    Some((line, col))
}

fn expand_json_layers(
    layers: &[Value],
    base_dir: Option<&Path>,
    depth: usize,
    parent_vars: &HashMap<String, String>,
) -> Result<Vec<Value>> {
    let mut out = Vec::with_capacity(layers.len());
    for item in layers {
        let is_include = item
            .get("type")
            .and_then(|t| t.as_str())
            .map(|t| t == "include")
            .unwrap_or(false);

        if is_include {
            let src = item
                .get("src")
                .and_then(|s| s.as_str())
                .ok_or_else(|| anyhow::anyhow!("JSON layer 'include' wajib memiliki field 'src'"))?;

            if depth >= MAX_INCLUDE_DEPTH {
                anyhow::bail!(
                    "Max include depth ({}) exceeded while including '{}'",
                    MAX_INCLUDE_DEPTH,
                    src
                );
            }

            let inc_path = match base_dir {
                Some(dir) => dir.join(src),
                None => Path::new(src).to_path_buf(),
            };
            let inc_raw = std::fs::read_to_string(&inc_path).with_context(|| {
                format!("Failed to read included JSON file '{}' ({:?})", src, inc_path)
            })?;

            let mut child_vars = parent_vars.clone();
            if let Some(vars_obj) = item
                .get("vars")
                .or_else(|| item.get("variables"))
                .and_then(|v| v.as_object())
            {
                for (k, v) in vars_obj {
                    let s = match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        other => other.to_string(),
                    };
                    child_vars.insert(k.clone(), s);
                }
            }

            let resolved = if child_vars.is_empty() {
                inc_raw
            } else {
                substitute_variables(&inc_raw, &child_vars)
            };

            let inc_val: Value = serde_json::from_str(&resolved).map_err(|e| {
                anyhow::anyhow!(
                    "JSON syntax error in included file {:?} (line {}, column {}): {}",
                    inc_path,
                    e.line(),
                    e.column(),
                    e
                )
            })?;

            let raw_inc_layers = if let Some(arr) = inc_val.as_array() {
                arr.clone()
            } else if let Some(arr) = inc_val.get("layers").and_then(|v| v.as_array()) {
                arr.clone()
            } else {
                anyhow::bail!(
                    "Included JSON file {:?} must be an array of layers or an object with a 'layers' field",
                    inc_path
                );
            };

            let expanded_inc =
                expand_json_layers(&raw_inc_layers, inc_path.parent(), depth + 1, &child_vars)?;
            out.extend(expanded_inc);
            continue;
        }

        // Recursively expand children inside containers
        let mut cloned = item.clone();
        if let Some(children) = cloned.get_mut("children").and_then(|c| c.as_array_mut()) {
            let expanded_children =
                expand_json_layers(children, base_dir, depth, parent_vars)?;
            *children = expanded_children;
        }
        out.push(cloned);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::LayerKind;

    #[test]
    fn test_json_include_component_with_variables() {
        let dir = std::env::temp_dir().join("rustren_test_json_include");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let card_json = r##"{
            "layers": [
                { "type": "rect", "x": 0, "y": 0, "width": 200, "height": 100, "color": "{{bg|#111111}}" },
                { "type": "text", "x": 10, "y": 20, "content": "{{label|Default}}", "font_size": 24, "color": "#FFFFFF" }
            ]
        }"##;
        std::fs::write(dir.join("card.json"), card_json).unwrap();

        let main_json = r##"{
            "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 2.0 },
            "layers": [
                { "type": "include", "src": "card.json", "vars": { "bg": "#238636", "label": "Card 1" } },
                { "type": "include", "src": "card.json", "vars": { "label": "Card 2" } }
            ]
        }"##;
        let main_path = dir.join("main.json");
        std::fs::write(&main_path, main_json).unwrap();

        let compiler = JsonCompiler;
        let compiled = compiler.compile(&main_path).unwrap();
        let scene = compiled.as_static();
        assert_eq!(scene.layers.len(), 4);

        match &scene.layers[0].kind {
            LayerKind::Rect { color, .. } => assert_eq!(color, "#238636"),
            _ => panic!("Expected Rect"),
        }
        match &scene.layers[1].kind {
            LayerKind::Text { content, .. } => assert_eq!(content, "Card 1"),
            _ => panic!("Expected Text"),
        }
        match &scene.layers[2].kind {
            LayerKind::Rect { color, .. } => assert_eq!(color, "#111111"),
            _ => panic!("Expected Rect"),
        }
        match &scene.layers[3].kind {
            LayerKind::Text { content, .. } => assert_eq!(content, "Card 2"),
            _ => panic!("Expected Text"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_value_position_reports_line_and_column() {
        let src = "{\"canvas\": {\"width\": 640},\n \"layers\": [{\"x\": 10}]}";
        assert_eq!(json_value_position(src, "canvas.width"), Some((1, 22)));
        assert_eq!(json_value_position(src, "layers[0].x"), Some((2, 19)));
        assert_eq!(json_value_position(src, "canvas"), Some((1, 12)));
        assert_eq!(json_value_position(src, "missing.path"), None);
    }

    #[test]
    fn schema_error_for_typed_field_carries_path_and_location() {
        let source = std::path::Path::new("scene.json");
        let content = "{\"canvas\":{\"width\":\"640\",\"height\":360,\"fps\":30,\"duration_secs\":1},\"layers\":[]}";
        let err = parse_json_scene_with_base(content, None, source).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("`canvas.width`"), "{msg}");
        assert!(msg.contains("line 1, column 20"), "{msg}");
    }
}
