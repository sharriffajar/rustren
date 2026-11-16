//! Unified Action Registry (Fase 5 — MCP & AI Copilot Core).
//!
//! Menyediakan antarmuka perintah terstruktur (`ActionRequest`) sehingga
//! operasi yang sama dapat dipicu oleh Studio UI, CLI, maupun AI Agent (MCP).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::schema::{Animation, Layer, Scene};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ActionRequest {
    /// Ringkasan struktur scene saat ini (canvas + daftar layer)
    GetSceneSummary,
    /// Tambah layer baru ke scene (opsional: posisi index)
    AddLayer {
        layer: Layer,
        #[serde(default)]
        index: Option<usize>,
    },
    /// Hapus layer berdasarkan index atau nama
    RemoveLayer {
        #[serde(default)]
        index: Option<usize>,
        #[serde(default)]
        name: Option<String>,
    },
    /// Tambahkan animasi / keyframe ke layer tertentu
    AddAnimation {
        #[serde(default)]
        index: Option<usize>,
        #[serde(default)]
        name: Option<String>,
        animation: Animation,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionResponse {
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

fn resolve_layer_index(scene: &Scene, index: Option<usize>, name: Option<&str>) -> Result<usize> {
    if let Some(idx) = index {
        if idx < scene.layers.len() {
            return Ok(idx);
        }
        anyhow::bail!("Layer index {} out of bounds (total: {})", idx, scene.layers.len());
    }
    if let Some(target_name) = name {
        if let Some(idx) = scene.layers.iter().position(|l| l.name.as_deref() == Some(target_name)) {
            return Ok(idx);
        }
        anyhow::bail!("Layer with name '{}' not found", target_name);
    }
    anyhow::bail!("Either 'index' or 'name' must be provided")
}

pub fn execute_action(scene: &mut Scene, req: ActionRequest) -> Result<ActionResponse> {
    match req {
        ActionRequest::GetSceneSummary => {
            let layers: Vec<Value> = scene
                .layers
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    serde_json::json!({
                        "index": i,
                        "name": l.name,
                        "start_sec": l.start_sec,
                        "duration_secs": l.duration_secs,
                        "animations_count": l.animations.len(),
                    })
                })
                .collect();
            Ok(ActionResponse {
                ok: true,
                message: format!("Scene has {} layers", scene.layers.len()),
                data: Some(serde_json::json!({
                    "width": scene.canvas.width,
                    "height": scene.canvas.height,
                    "fps": scene.canvas.fps,
                    "duration_secs": scene.canvas.duration_secs,
                    "layers": layers,
                })),
            })
        }
        ActionRequest::AddLayer { layer, index } => {
            let insert_idx = index.unwrap_or(scene.layers.len()).min(scene.layers.len());
            scene.layers.insert(insert_idx, layer);
            Ok(ActionResponse {
                ok: true,
                message: format!("Added layer at index {}", insert_idx),
                data: Some(serde_json::json!({ "index": insert_idx, "total_layers": scene.layers.len() })),
            })
        }
        ActionRequest::RemoveLayer { index, name } => {
            let idx = resolve_layer_index(scene, index, name.as_deref())?;
            scene.layers.remove(idx);
            Ok(ActionResponse {
                ok: true,
                message: format!("Removed layer at index {}", idx),
                data: Some(serde_json::json!({ "removed_index": idx, "total_layers": scene.layers.len() })),
            })
        }
        ActionRequest::AddAnimation {
            index,
            name,
            animation,
        } => {
            let idx = resolve_layer_index(scene, index, name.as_deref())?;
            scene.layers[idx].animations.push(animation);
            Ok(ActionResponse {
                ok: true,
                message: format!("Added animation to layer {}", idx),
                data: Some(serde_json::json!({ "layer_index": idx, "animations_count": scene.layers[idx].animations.len() })),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{AnimProperty, AnimValue, Easing};

    #[test]
    fn test_unified_action_registry_crud() {
        let json = r##"{
            "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 3.0 },
            "layers": []
        }"##;
        let mut scene: Scene = serde_json::from_str(json).unwrap();

        // 1. AddLayer
        let add_req: ActionRequest = serde_json::from_str(
            r##"{
                "action": "add_layer",
                "layer": { "name": "hero_box", "type": "rect", "x": 100, "y": 100, "width": 200, "height": 200, "color": "#38BDF8" }
            }"##,
        )
        .unwrap();
        let res = execute_action(&mut scene, add_req).unwrap();
        assert!(res.ok);
        assert_eq!(scene.layers.len(), 1);

        // 2. AddAnimation by layer name
        let anim_req = ActionRequest::AddAnimation {
            index: None,
            name: Some("hero_box".to_string()),
            animation: Animation {
                property: AnimProperty::Opacity,
                from: AnimValue::Number(0.0),
                to: AnimValue::Number(1.0),
                keyframes: vec![],
                start_sec: 0.0,
                end_sec: 1.0,
                easing: Easing::EaseOut,
                bezier: None,
                spring: None,
                repeat: None,
                yoyo: false,
                expr: None,
            },
        };
        let res = execute_action(&mut scene, anim_req).unwrap();
        assert!(res.ok);
        assert_eq!(scene.layers[0].animations.len(), 1);

        // 3. GetSceneSummary
        let sum = execute_action(&mut scene, ActionRequest::GetSceneSummary).unwrap();
        assert!(sum.ok);

        // 4. RemoveLayer by name
        let rm_req = ActionRequest::RemoveLayer {
            index: None,
            name: Some("hero_box".to_string()),
        };
        let res = execute_action(&mut scene, rm_req).unwrap();
        assert!(res.ok);
        assert_eq!(scene.layers.len(), 0);
    }
}
