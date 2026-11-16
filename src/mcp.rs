//! MCP (Model Context Protocol) stdio JSON-RPC 2.0 Server (Fase 5).
//!
//! Memungkinkan AI Agent (Claude Desktop, Cursor, Antigravity, dll)
//! menginspeksi dan memodifikasi scene video `rustren` secara langsung.

use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;

use crate::actions::{execute_action, ActionRequest};
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
        "tools/list" => Ok(json!({
            "tools": [
                {
                    "name": "get_scene_summary",
                    "description": "Get canvas dimensions, fps, duration, and list of layers in the scene.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "add_layer",
                    "description": "Add a new layer (rect, circle, text, image, video, etc.) to the scene.",
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
                    "description": "Remove a layer by index or name.",
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
                    "description": "Add an animation/keyframe to a layer by index or name.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "index": { "type": "integer" },
                            "name": { "type": "string" },
                            "animation": { "type": "object" }
                        },
                        "required": ["animation"]
                    }
                }
            ]
        })),
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or(json!({}));
            let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let mut args = params.get("arguments").cloned().unwrap_or(json!({}));
            if let Some(obj) = args.as_object_mut() {
                obj.insert("action".to_string(), Value::String(tool_name.to_string()));
            }
            call_scene_action(scene_path, args)
        }
        other => Err(anyhow::anyhow!("Method not found: {}", other)),
    };

    Some(match result {
        Ok(res) => json!({ "jsonrpc": "2.0", "id": id, "result": res }),
        Err(e) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("{:#}", e) }
        }),
    })
}

fn call_scene_action(scene_path: &Path, action_val: Value) -> Result<Value> {
    let raw = std::fs::read_to_string(scene_path)?;
    let mut scene: Scene = serde_json::from_str(&raw)?;
    let req: ActionRequest = serde_json::from_value(action_val)?;
    let resp = execute_action(&mut scene, req)?;
    let text = serde_json::to_string_pretty(&resp)?;
    Ok(json!({
        "content": [{ "type": "text", "text": text }]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_initialize_and_tools_list() {
        let dummy = Path::new("nonexistent.json");
        let init_req = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" });
        let init_resp = handle_mcp_request(&init_req, dummy).unwrap();
        assert_eq!(init_resp["result"]["serverInfo"]["name"], "rustren-mcp");

        let list_req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
        let list_resp = handle_mcp_request(&list_req, dummy).unwrap();
        let tools = list_resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4);
    }
}
