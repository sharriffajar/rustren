//! MCP (Model Context Protocol) stdio JSON-RPC 2.0 Server.
//!
//! Lets AI agents inspect, edit *and verify* a `rustren` scene end to end
//! without leaving the protocol:
//!
//! - scene actions (`add_layer`, `remove_layer`, `add_animation`) persist to
//!   disk atomically, so an edit is never silently lost;
//! - sensor tools (`validate_scene`, `snapshot_scene`, `inspect_layout`,
//!   `diff_scenes`) close the author -> measure -> repair loop;
//! - `render_video` turns the final scene into an MP4.
//!
//! It is stdio-only (no tokio/axum), so it is available in every build.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::actions::{ActionRequest, execute_action};
use crate::schema::Scene;

pub fn run_mcp_server(scene_path: &Path) -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("Parse error: {}", e) }
                });
                writeln!(out, "{}", err_resp)?;
                out.flush()?;
                continue;
            }
        };

        if let Some(resp) = handle_mcp_request(&req, scene_path) {
            writeln!(out, "{}", resp)?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Tool catalogue advertised by `tools/list`.
fn tool_definitions() -> Value {
    json!([
        {
            "name": "get_scene_summary",
            "description": "Get canvas dimensions, fps, duration, and list of layers in the scene.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "add_layer",
            "description": "Add a new layer (rect, circle, text, image, video, etc.) to the scene. The edit is written back to disk.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "layer": { "type": "object" },
                    "index": { "type": "integer" }
                },
                "required": ["layer"]
            }
        },
        {
            "name": "remove_layer",
            "description": "Remove a layer by index or name. The edit is written back to disk.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "index": { "type": "integer" },
                    "name": { "type": "string" }
                }
            }
        },
        {
            "name": "add_animation",
            "description": "Add an animation/keyframe to a layer by index or name. The edit is written back to disk.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "index": { "type": "integer" },
                    "name": { "type": "string" },
                    "animation": { "type": "object" }
                },
                "required": ["animation"]
            }
        },
        {
            "name": "validate_scene",
            "description": "Compile + lint the scene, check that referenced local assets exist, and return JSON diagnostics without rendering.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "strict_determinism": { "type": "boolean" },
                    "composition": { "type": "string" }
                }
            }
        },
        {
            "name": "snapshot_scene",
            "description": "Render one or more timestamp frames to PNG (or a contact sheet) without encoding a video.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "t": { "type": "number" },
                    "times": { "type": "array", "items": { "type": "number" } },
                    "output": { "type": "string" },
                    "sheet": { "type": "boolean" },
                    "cols": { "type": "integer" },
                    "composition": { "type": "string" }
                }
            }
        },
        {
            "name": "inspect_layout",
            "description": "Return computed bounding boxes and measured text sizes at a timestamp, optionally with overlap/clipping diagnostics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "t": { "type": "number" },
                    "diagnose": { "type": "boolean" },
                    "composition": { "type": "string" }
                }
            }
        },
        {
            "name": "diff_scenes",
            "description": "Compare this scene against another scene at a timestamp and return MAE/RMSE/changed-pixel metrics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "other_path": { "type": "string" },
                    "t": { "type": "number" },
                    "threshold": { "type": "number" }
                },
                "required": ["other_path"]
            }
        },
        {
            "name": "render_video",
            "description": "Render the scene to an MP4 file with optional crf, preset and composition profile.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "output": { "type": "string" },
                    "crf": { "type": "integer" },
                    "preset": { "type": "string" },
                    "composition": { "type": "string" }
                },
                "required": ["output"]
            }
        },
        {
            "name": "detect_beats",
            "description": "Detect audio beat timestamps (in seconds) from an audio or video file for beat-sync automation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "audio_path": { "type": "string" },
                    "sensitivity": { "type": "number" }
                },
                "required": ["audio_path"]
            }
        },
        {
            "name": "qa_video",
            "description": "Audit a rendered MP4/WebM video for visual & audio defects (black frames, frozen frames, EBU R128 loudness & true-peak clipping).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "video_path": { "type": "string" }
                },
                "required": ["video_path"]
            }
        }
    ])
}

/// Rewrite the scene file atomically (temp file in the same directory, then
/// rename) so a crash between writes cannot leave a half-written scene.
fn write_scene_atomically(scene_path: &Path, pretty: &str) -> Result<()> {
    let parent = scene_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = scene_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("scene.json");
    let tmp = parent.join(format!(".{file_name}.rustren-tmp"));
    // Keep the usual "text file ends with a newline" convention so the
    // rewritten scene does not show up as a no-newline-at-eof diff.
    let pretty = if pretty.ends_with('\n') {
        pretty.to_string()
    } else {
        format!("{pretty}\n")
    };
    std::fs::write(&tmp, pretty.as_bytes())
        .with_context(|| format!("Failed to write temp scene: {}", tmp.display()))?;
    std::fs::rename(&tmp, scene_path)
        .with_context(|| format!("Failed to replace scene: {}", scene_path.display()))?;
    Ok(())
}

/// Load the scene, apply an action, persist it when it changed, and return the
/// action response plus the on-disk path.
fn call_scene_action(scene_path: &Path, action_val: Value) -> Result<Value> {
    let raw = std::fs::read_to_string(scene_path)?;
    let mut root: Value = serde_json::from_str(&raw)?;
    let mut scene: Scene = serde_json::from_value(root.clone())?;
    // Keep the author's original JSON for inserted items so the file gains only
    // the fields they actually wrote, not every serialized default.
    let raw_layer = action_val.get("layer").cloned();
    let raw_animation = action_val.get("animation").cloned();
    let req: ActionRequest = serde_json::from_value(action_val)?;
    let resp = execute_action(&mut scene, req.clone())?;

    // Persist by editing the original document in place instead of re-serializing
    // the whole scene. That keeps the authored field order and any extra keys on
    // the layers the action did not touch.
    let layers = root
        .get_mut("layers")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("scene has no `layers` array"))?;
    match req {
        ActionRequest::AddLayer { index, .. } => {
            let at = index.unwrap_or(layers.len()).min(layers.len());
            let value = raw_layer.ok_or_else(|| {
                anyhow::anyhow!("add_layer: `layer` object is required to persist the edit")
            })?;
            layers.insert(at, value);
        }
        ActionRequest::RemoveLayer { .. } => {
            if let Some(idx) = resp
                .data
                .as_ref()
                .and_then(|d| d.get("removed_index"))
                .and_then(|v| v.as_u64())
            {
                layers.remove(idx as usize);
            }
        }
        ActionRequest::AddAnimation { .. } => {
            let value = raw_animation.ok_or_else(|| {
                anyhow::anyhow!("add_animation: `animation` object is required to persist the edit")
            })?;
            if let Some(idx) = resp
                .data
                .as_ref()
                .and_then(|d| d.get("layer_index"))
                .and_then(|v| v.as_u64())
                && let Some(layer) = layers.get_mut(idx as usize)
            {
                let animations = layer
                    .as_object_mut()
                    .ok_or_else(|| anyhow::anyhow!("layer is not an object"))?
                    .entry("animations")
                    .or_insert_with(|| Value::Array(Vec::new()));
                if !animations.is_array() {
                    *animations = Value::Array(Vec::new());
                }
                animations
                    .as_array_mut()
                    .expect("checked array above")
                    .push(value);
            }
        }
        ActionRequest::GetSceneSummary => {
            // Read-only: nothing to persist.
            return Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&resp)? }]
            }));
        }
    }

    let pretty = serde_json::to_string_pretty(&root)?;
    write_scene_atomically(scene_path, &pretty)?;
    let mut text = serde_json::to_string_pretty(&resp)?;
    text.push_str(&format!("\npersisted to {}", scene_path.display()));
    Ok(json!({
        "content": [{ "type": "text", "text": text }]
    }))
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn arg_f32(args: &Value, key: &str) -> Option<f32> {
    args.get(key).and_then(|v| v.as_f64()).map(|v| v as f32)
}

fn arg_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

/// Resolve `output` against the process working directory.
fn output_path(args: &Value, default: &str) -> PathBuf {
    PathBuf::from(arg_str(args, "output").unwrap_or(default))
}

/// Compile a scene and apply an optional composition profile.
fn load_scene(scene_path: &Path, composition: Option<&str>) -> Result<Scene> {
    let compiler = crate::compilers::detect(scene_path)?;
    let mut compiled = compiler.compile(scene_path)?;
    let scene = compiled.as_static_mut();
    if let Some(name) = composition {
        scene.apply_profile(name)?;
    }
    crate::capabilities::validate_scene_capabilities(scene)?;
    Ok(scene.clone())
}

fn validate_scene_tool(scene_path: &Path, args: &Value) -> Result<Value> {
    let strict = arg_bool(args, "strict_determinism").unwrap_or(false);
    let composition = arg_str(args, "composition");
    // JSON scenes get the same unknown-field gate as the CLI, so an agent
    // cannot get a green validate for a field the renderer would ignore.
    if scene_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        crate::lint::validate_no_unknown_fields(&std::fs::read_to_string(scene_path)?)?;
    }
    let compiler = crate::compilers::detect(scene_path)?;
    let mut compiled = compiler.compile(scene_path)?;
    let scene = compiled.as_static_mut();
    if let Some(name) = composition {
        scene.apply_profile(name)?;
    }
    crate::capabilities::validate_scene_capabilities(scene)?;

    let mut warnings = crate::lint::collect_lint_warnings_structured(scene);
    if strict {
        warnings.extend(crate::lint::collect_determinism_warnings_structured(scene));
    }
    warnings.extend(crate::lint::check_local_assets(scene, scene_path.parent()));
    if compiler.format_name() == "html" {
        let raw = std::fs::read_to_string(scene_path)?;
        for issue in crate::source_map::lint_html_source(&raw) {
            warnings.push(crate::lint::LintWarning {
                code: issue.code,
                path: issue.path,
                message: issue.message,
                fix: issue.fix,
                affected_paths: Vec::new(),
            });
        }
    }

    let payload = json!({
        "valid": true,
        "format": compiler.format_name(),
        "schema_version": scene.schema_version,
        "canvas": {
            "width": scene.canvas.width,
            "height": scene.canvas.height,
            "fps": scene.canvas.fps,
            "duration_secs": scene.canvas.duration_secs,
        },
        "root_layers": scene.layers.len(),
        "warning_count": warnings.len(),
        "warnings": warnings.iter().map(|w| json!({
            "code": w.code,
            "path": w.path,
            "message": w.message,
            "fix": w.fix,
            "affected_paths": w.affected_paths,
        })).collect::<Vec<_>>(),
    });
    Ok(json!({ "content": [{ "type": "text", "text": payload.to_string() }] }))
}

fn snapshot_scene_tool(scene_path: &Path, args: &Value) -> Result<Value> {
    let scene = load_scene(scene_path, arg_str(args, "composition"))?;
    let times: Vec<f32> = if let Some(times) = args.get("times").and_then(|v| v.as_array()) {
        times
            .iter()
            .filter_map(|v| v.as_f64())
            .map(|v| v as f32)
            .collect()
    } else {
        vec![arg_f32(args, "t").unwrap_or(0.0)]
    };
    if times.is_empty() {
        anyhow::bail!("snapshot_scene: provide `t` or a non-empty `times` array");
    }
    let output = output_path(args, "snapshot.png");
    let sheet = arg_bool(args, "sheet").unwrap_or(false);
    let cols = args.get("cols").and_then(|v| v.as_u64()).map(|v| v as u32);
    let assets = crate::assets::AssetStore::load_for_scene_with_base(
        &scene,
        Path::new(".rustren_cache"),
        scene_path.parent(),
    )?;
    crate::render_snapshots(&scene, &assets, &times, &output, sheet, cols)?;
    Ok(json!({
        "content": [{ "type": "text", "text": json!({
            "ok": true,
            "output": output,
            "times": times,
            "count": times.len(),
            "sheet": sheet,
        }).to_string() }]
    }))
}

fn inspect_layout_tool(scene_path: &Path, args: &Value) -> Result<Value> {
    let t = arg_f32(args, "t").unwrap_or(0.0);
    let diagnose = arg_bool(args, "diagnose").unwrap_or(false);
    let scene = load_scene(scene_path, arg_str(args, "composition"))?;
    let assets = crate::assets::AssetStore::load_for_scene_with_base(
        &scene,
        Path::new(".rustren_cache"),
        scene_path.parent(),
    )?;
    let layout = crate::renderer::inspect_layout_at_time(&scene, &assets, t)?;
    let payload = if diagnose {
        let diagnostics = crate::renderer::diagnose_layout_at_time(&scene, &assets, t)?;
        json!({ "layout": layout, "diagnostics": diagnostics })
    } else {
        json!({ "layout": layout })
    };
    Ok(json!({ "content": [{ "type": "text", "text": payload.to_string() }] }))
}

fn diff_scenes_tool(scene_path: &Path, args: &Value) -> Result<Value> {
    let other = arg_str(args, "other_path")
        .ok_or_else(|| anyhow::anyhow!("diff_scenes: missing `other_path`"))?;
    let t = arg_f32(args, "t").unwrap_or(0.0);
    let threshold = arg_f32(args, "threshold").unwrap_or(0.01);
    let report = crate::run_diff(
        scene_path,
        Path::new(other),
        t,
        threshold,
        None,
        Path::new(".rustren_cache"),
    )?;
    let regressed = report.mae as f32 > threshold;
    let payload = json!({
        "ok": !regressed,
        "t": t,
        "other_path": other,
        "mae": report.mae,
        "rmse": report.rmse,
        "changed_pixels_pct": report.changed_pct,
        "threshold": threshold,
    });
    Ok(json!({ "content": [{ "type": "text", "text": payload.to_string() }] }))
}

fn render_video_tool(scene_path: &Path, args: &Value) -> Result<Value> {
    let output = output_path(args, "output.mp4");
    let crf = args.get("crf").and_then(|v| v.as_u64()).unwrap_or(18) as u8;
    let preset = arg_str(args, "preset").unwrap_or("medium");
    let scene = load_scene(scene_path, arg_str(args, "composition"))?;
    crate::render_scene_to_file_with_base(
        &scene,
        &output,
        Path::new(".rustren_cache"),
        scene_path.parent(),
        crf,
        preset,
        false,
    )?;
    Ok(json!({
        "content": [{ "type": "text", "text": json!({
            "ok": true,
            "output": output,
            "crf": crf,
            "preset": preset,
        }).to_string() }]
    }))
}

pub fn handle_mcp_request(req: &Value, scene_path: &Path) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

    // Notifications (no id) do not require a response
    let id = id?;

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "rustren-mcp", "version": env!("CARGO_PKG_VERSION") }
        })),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or(json!({}));
            let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let mut args = params.get("arguments").cloned().unwrap_or(json!({}));
            if args.is_null() {
                args = json!({});
            }
            match tool_name {
                "validate_scene" => validate_scene_tool(scene_path, &args),
                "snapshot_scene" => snapshot_scene_tool(scene_path, &args),
                "inspect_layout" => inspect_layout_tool(scene_path, &args),
                "diff_scenes" => diff_scenes_tool(scene_path, &args),
                "render_video" => render_video_tool(scene_path, &args),
                "detect_beats" => {
                    let audio_path = arg_str(&args, "audio_path").unwrap_or("");
                    let sensitivity = arg_f32(&args, "sensitivity").unwrap_or(1.5);
                    crate::encoder::detect_audio_beats(Path::new(audio_path), sensitivity).map(
                        |beats| {
                            json!({
                                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&beats).unwrap_or_default() }]
                            })
                        },
                    )
                }
                "qa_video" => {
                    let video_path = arg_str(&args, "video_path").unwrap_or("");
                    crate::qa::run_qa(Path::new(video_path), false).map(|report| {
                        json!({
                            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&report).unwrap_or_default() }]
                        })
                    })
                }
                other => {
                    if let Some(obj) = args.as_object_mut() {
                        obj.insert("action".to_string(), Value::String(other.to_string()));
                    }
                    call_scene_action(scene_path, args)
                }
            }
        }
        other => Err(anyhow::anyhow!("Method not found: {}", other)),
    };

    Some(match result {
        Ok(res) => json!({ "jsonrpc": "2.0", "id": id, "result": res }),
        Err(e) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32603, "message": format!("{:#}", e) }
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_scene_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustren_mcp_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    const MINIMAL_SCENE: &str = r##"{
  "canvas": { "width": 640, "height": 360, "fps": 30, "duration_secs": 2.0, "background": "#000000" },
  "layers": []
}"##;

    fn call(scene_path: &Path, tool: &str, args: Value) -> Value {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": tool, "arguments": args }
        });
        handle_mcp_request(&req, scene_path).unwrap()
    }

    #[test]
    fn test_mcp_initialize_and_tools_list() {
        let dummy = Path::new("nonexistent.json");
        let init_req = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" });
        let init_resp = handle_mcp_request(&init_req, dummy).unwrap();
        assert_eq!(init_resp["result"]["serverInfo"]["name"], "rustren-mcp");

        let list_req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
        let list_resp = handle_mcp_request(&list_req, dummy).unwrap();
        let tools = list_resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 11);
        for name in [
            "validate_scene",
            "snapshot_scene",
            "inspect_layout",
            "diff_scenes",
            "render_video",
        ] {
            assert!(
                tools.iter().any(|t| t["name"] == name),
                "missing tool {name}"
            );
        }
    }

    #[test]
    fn test_mcp_add_layer_persists_to_disk() {
        let path = temp_scene_path("persist.json");
        std::fs::write(&path, MINIMAL_SCENE).unwrap();

        let layer = json!({
            "type": "rect",
            "x": 10.0,
            "y": 10.0,
            "width": 100.0,
            "height": 50.0,
            "color": "#FF0000"
        });
        let resp = call(&path, "add_layer", json!({ "layer": layer }));
        assert!(resp.get("error").is_none(), "{resp}");
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("persisted to"), "{text}");

        // The mutation must be on disk, not only in memory.
        let on_disk: Scene =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk.layers.len(), 1);
        assert!(matches!(
            on_disk.layers[0].kind,
            crate::schema::LayerKind::Rect { .. }
        ));

        // A second mutation must read the persisted state, not the original.
        let resp = call(&path, "remove_layer", json!({ "index": 0 }));
        assert!(resp.get("error").is_none(), "{resp}");
        let on_disk: Scene =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(on_disk.layers.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_mcp_validate_and_layout_return_json() {
        let path = temp_scene_path("validate.json");
        std::fs::write(&path, MINIMAL_SCENE).unwrap();

        let resp = call(&path, "validate_scene", json!({}));
        assert!(resp.get("error").is_none(), "{resp}");
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        assert_eq!(payload["valid"], true);
        assert_eq!(payload["root_layers"], 0);

        let resp = call(&path, "inspect_layout", json!({ "t": 0.0 }));
        assert!(resp.get("error").is_none(), "{resp}");
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        assert!(payload["layout"].is_array(), "{payload}");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_mcp_writes_pretty_json_and_keeps_authored_shape() {
        let path = temp_scene_path("pretty.json");
        std::fs::write(&path, MINIMAL_SCENE).unwrap();
        let layer = json!({
            "type": "circle", "cx": 100.0, "cy": 100.0, "r": 40.0, "color": "#38BDF8"
        });
        let resp = call(&path, "add_layer", json!({ "layer": layer }));
        assert!(resp.get("error").is_none(), "{resp}");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.starts_with("{\n"), "scene is pretty-printed: {raw}");
        // The persisted file keeps the layer exactly as the agent wrote it:
        // no serialized defaults, no injected `null` fields.
        assert!(!raw.contains("null"), "{raw}");
        assert!(!raw.contains("backface_visible"), "{raw}");
        let on_disk: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(on_disk["layers"][0]["color"], "#38BDF8");
        assert_eq!(
            on_disk["canvas"]["background"], "#000000",
            "canvas survives untouched"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_mcp_add_animation_persists() {
        let path = temp_scene_path("anim.json");
        std::fs::write(
            &path,
            r##"{
  "canvas": { "width": 640, "height": 360, "fps": 30, "duration_secs": 2.0, "background": "#000000" },
  "layers": [{ "type": "rect", "x": 0, "y": 0, "width": 10, "height": 10, "color": "#FFFFFF" }]
}"##,
        )
        .unwrap();
        let animation = json!({
            "property": "opacity", "from": 0.0, "to": 1.0, "start_sec": 0.0, "end_sec": 1.0
        });
        let resp = call(
            &path,
            "add_animation",
            json!({ "index": 0, "animation": animation }),
        );
        assert!(resp.get("error").is_none(), "{resp}");
        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            on_disk["layers"][0]["animations"].as_array().unwrap().len(),
            1
        );
        let _ = std::fs::remove_file(&path);
    }
}
