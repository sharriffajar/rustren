use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::assets::AssetStore;
use crate::schema::Scene;
use crate::{capabilities, compilers, lint, premul_to_rgba_image, render_scene_to_file, renderer, schema};

/// Launch the browser-based Rustren live studio web server and WebSocket handler.
pub async fn run_studio(input_path: Option<&Path>, host: &str, port: u16, cache_dir: &Path) -> Result<()> {
    use axum::{
        extract::{ws::WebSocketUpgrade, State},
        response::Html,
        routing::get,
        Router,
    };

    #[derive(Clone)]
    struct StudioState {
        cache_dir: PathBuf,
        input_path: Option<PathBuf>,
    }

    let state = StudioState {
        cache_dir: cache_dir.to_path_buf(),
        input_path: input_path.map(|p| p.to_path_buf()),
    };

    let app = Router::new()
        .route("/", get(|| async { Html(include_str!("studio.html")) }))
        .route(
            "/logo.png",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "image/png")],
                    include_bytes!("../assets/logo.png").as_slice(),
                )
            }),
        )
        .route(
            "/ws",
            get(
                |ws: WebSocketUpgrade, State(state): State<StudioState>| async move {
                    ws.on_upgrade(move |socket| handle_studio_ws(socket, state.cache_dir, state.input_path))
                },
            ),
        )
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    println!("🦀 rustren studio / preview listening at http://{}", addr);
    if let Some(p) = input_path {
        println!("   Loading scene: {}", p.display());
    }
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let url = format!("http://{}", addr);
    open_browser(&url);
    axum::serve(listener, app).await?;
    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

async fn handle_studio_ws(
    mut socket: axum::extract::ws::WebSocket,
    cache_dir: PathBuf,
    input_path: Option<PathBuf>,
) {
    use axum::extract::ws::Message;
    use std::io::Cursor;

    #[derive(serde::Deserialize)]
    struct StudioReq {
        #[serde(default)]
        action: Option<String>,
        #[serde(default)]
        scene: Option<String>,
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        t: Option<f32>,
        #[serde(default)]
        variables: Option<serde_json::Map<String, serde_json::Value>>,
        #[serde(default)]
        layer_index: Option<usize>,
        #[serde(default)]
        dx: Option<f32>,
        #[serde(default)]
        dy: Option<f32>,
        #[serde(default)]
        ds: Option<f32>,
        #[serde(default)]
        drot: Option<f32>,
    }

    let mut dropped_file: Option<(String, String)> = None;
    let mut layer_offsets: std::collections::HashMap<usize, (f32, f32)> = std::collections::HashMap::new();
    let mut layer_scales: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
    let mut layer_rots: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();

    let parse_scene_from_str = |raw_content: &str, name_or_path: &str, vars: Option<&serde_json::Map<String, serde_json::Value>>| -> Result<(Scene, String, Vec<serde_json::Value>, serde_json::Value)> {
        let mut content = raw_content.to_string();
        let mut detected_vars = Vec::new();
        if let Ok(doc) = scraper::Html::parse_document(&content)
            .select(&scraper::Selector::parse("html[data-composition-variables]").unwrap())
            .next()
            .and_then(|el| el.value().attr("data-composition-variables"))
            .map(serde_json::from_str::<Vec<serde_json::Value>>)
            .transpose()
            && let Some(dv) = doc {
                detected_vars = dv;
            }

        for dv in &detected_vars {
            if let (Some(id), Some(def)) = (dv.get("id").and_then(|v| v.as_str()), dv.get("default")) {
                let placeholder = format!("{{{{{}}}}}", id);
                let override_val = vars.and_then(|m| m.get(id)).unwrap_or(def);
                let val_str = match override_val {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    other => other.to_string(),
                };
                content = content.replace(&placeholder, &val_str);
            }
        }
        if let Some(v_map) = vars {
            for (k, val) in v_map {
                let placeholder = format!("{{{{{}}}}}", k);
                let val_str = match val {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    other => other.to_string(),
                };
                content = content.replace(&placeholder, &val_str);
            }
        }

        let is_html = name_or_path.ends_with(".html") || name_or_path.ends_with(".htm");
        let scene = if is_html {
            compilers::html::parse_html_scene(&content)?
        } else {
            serde_json::from_str::<Scene>(&content)?
        };
        let scene_val = serde_json::json!({
            "canvas": {
                "width": scene.canvas.width,
                "height": scene.canvas.height,
                "fps": scene.canvas.fps,
                "duration_secs": scene.canvas.duration_secs,
            },
            "layers": scene.layers.iter().map(|l| {
                serde_json::json!({
                    "name": l.name,
                    "type": match &l.kind {
                        schema::LayerKind::Text { .. } => "text",
                        schema::LayerKind::Image { .. } => "image",
                        schema::LayerKind::Rect { .. } => "rect",
                        schema::LayerKind::Circle { .. } => "circle",
                        schema::LayerKind::Video { .. } => "video",
                        schema::LayerKind::Waveform { .. } => "waveform",
                        schema::LayerKind::Lottie { .. } => "lottie",
                        schema::LayerKind::Particles { .. } => "particles",
                        schema::LayerKind::Svg { .. } => "svg",
                        schema::LayerKind::Container { .. } => "container",
                    },
                    "start_sec": l.start_sec,
                    "duration_secs": l.duration_secs,
                    "animations": l.animations.iter().map(|a| {
                        serde_json::json!({
                            "start_sec": a.start_sec,
                            "end_sec": a.end_sec,
                        })
                    }).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>()
        });
        Ok((scene, name_or_path.to_string(), detected_vars, scene_val))
    };

    let load_current_scene = |dropped: Option<&(String, String)>, vars: Option<&serde_json::Map<String, serde_json::Value>>| -> Result<(Scene, String, Vec<serde_json::Value>, serde_json::Value)> {
        if let Some((fname, fcontent)) = dropped {
            parse_scene_from_str(fcontent, fname, vars)
        } else if let Some(ref path) = input_path {
            let content = std::fs::read_to_string(path)?;
            parse_scene_from_str(&content, &path.display().to_string(), vars)
        } else {
            let default_json = "{\n  \"canvas\": { \"width\": 1280, \"height\": 720, \"fps\": 30, \"duration_secs\": 4.0, \"background\": { \"color\": \"#0b0d13\" } },\n  \"layers\": [\n    { \"type\": \"rect\", \"x\": 340, \"y\": 210, \"width\": 600, \"height\": 300, \"color\": \"#1e293b\" },\n    { \"type\": \"text\", \"x\": 640, \"y\": 360, \"anchor\": \"center\", \"content\": \"RUSTREN STUDIO\", \"font_size\": 48, \"color\": \"#60a5fa\" }\n  ]\n}";
            let scene: Scene = serde_json::from_str(default_json)?;
            let scene_val = serde_json::json!({
                "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 4.0 },
                "layers": [
                    { "id": "background", "type": "rect", "start_sec": 0.0, "duration_secs": 4.0 },
                    { "id": "title", "type": "text", "start_sec": 0.0, "duration_secs": 4.0 }
                ]
            });
            Ok((scene, "default_scene.json".to_string(), vec![], scene_val))
        }
    };

    while let Some(Ok(msg)) = socket.recv().await {
        let Message::Text(txt) = msg else { continue };
        let req: StudioReq = match serde_json::from_str(&txt) {
            Ok(r) => r,
            Err(e) => {
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({ "error": format!("Invalid request: {}", e) })
                            .to_string(),
                    ))
                    .await;
                continue;
            }
        };

        if req.action.as_deref() == Some("transform_layer") {
            let idx = req.layer_index.unwrap_or(0);
            if let Some(ds) = req.ds {
                let s = layer_scales.entry(idx).or_insert(1.0);
                *s = (*s + ds).clamp(0.05, 10.0);
            }
            if let Some(drot) = req.drot {
                let r = layer_rots.entry(idx).or_insert(0.0);
                *r += drot;
            }
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({ "type": "layer_transformed", "layer_index": idx }).to_string(),
                ))
                .await;
            continue;
        }

        if req.action.as_deref() == Some("move_layer") {
            let idx = req.layer_index.unwrap_or(0);
            let dx = req.dx.unwrap_or(0.0);
            let dy = req.dy.unwrap_or(0.0);
            let entry = layer_offsets.entry(idx).or_insert((0.0, 0.0));
            entry.0 += dx;
            entry.1 += dy;
            let _ = socket
                .send(Message::Text(
                    serde_json::json!({ "type": "layer_moved", "layer_index": idx }).to_string(),
                ))
                .await;
            continue;
        }

        if req.action.as_deref() == Some("load_file") {
            layer_offsets.clear();
            if let (Some(fname), Some(fcontent)) = (req.filename, req.content) {
                match parse_scene_from_str(&fcontent, &fname, None) {
                    Ok((_scene, path_str, vars, scene_val)) => {
                        dropped_file = Some((fname, fcontent));
                        let _ = socket.send(Message::Text(
                            serde_json::json!({
                                "type": "init",
                                "scene": scene_val,
                                "path": path_str,
                                "variables": vars
                            }).to_string()
                        )).await;
                    }
                    Err(e) => {
                        let _ = socket.send(Message::Text(
                            serde_json::json!({ "error": format!("Failed to parse dropped file: {:#}", e) }).to_string()
                        )).await;
                    }
                }
            }
            continue;
        }

        if req.action.as_deref() == Some("export_mp4") {
            match load_current_scene(dropped_file.as_ref(), req.variables.as_ref()) {
                Ok((scene, _, _, _)) => {
                    let out_path = PathBuf::from("rustren_studio_export.mp4");
                    let cache_clone = cache_dir.clone();
                    let out_clone = out_path.clone();
                    let res = tokio::task::spawn_blocking(move || {
                        render_scene_to_file(&scene, &out_clone, &cache_clone)
                    })
                    .await;
                    match res {
                        Ok(Ok(_)) => {
                            let _ = socket
                                .send(Message::Text(
                                    serde_json::json!({
                                        "type": "export_done",
                                        "path": out_path.display().to_string()
                                    })
                                    .to_string(),
                                ))
                                .await;
                        }
                        Ok(Err(e)) => {
                            let _ = socket
                                .send(Message::Text(
                                    serde_json::json!({ "error": format!("Export failed: {:#}", e) })
                                        .to_string(),
                                ))
                                .await;
                        }
                        Err(e) => {
                            let _ = socket
                                .send(Message::Text(
                                    serde_json::json!({ "error": format!("Export task panicked: {}", e) })
                                        .to_string(),
                                ))
                                .await;
                        }
                    }
                }
                Err(e) => {
                    let _ = socket
                        .send(Message::Text(
                            serde_json::json!({ "error": format!("Export failed: {:#}", e) })
                                .to_string(),
                        ))
                        .await;
                }
            }
            continue;
        }

        if req.action.as_deref() == Some("get_scene") {
            match load_current_scene(dropped_file.as_ref(), None) {
                Ok((_scene, path_str, vars, scene_val)) => {
                    let _ = socket.send(Message::Text(
                        serde_json::json!({
                            "type": "init",
                            "scene": scene_val,
                            "path": path_str,
                            "variables": vars
                        }).to_string()
                    )).await;
                }
                Err(e) => {
                    let _ = socket.send(Message::Text(
                        serde_json::json!({ "error": format!("Failed to load scene: {:#}", e) }).to_string()
                    )).await;
                }
            }
            continue;
        }

        let t = req.t.unwrap_or(0.0);
        let start = std::time::Instant::now();
        let render_res: Result<(Vec<u8>, u32, u32)> = (|| {
            let mut scene = if let Some(ref s_str) = req.scene {
                lint::validate_no_unknown_fields(s_str)?;
                serde_json::from_str::<Scene>(s_str)?
            } else {
                let (s, _, _, _) = load_current_scene(dropped_file.as_ref(), req.variables.as_ref())?;
                s
            };
            for (&idx, &(ox, oy)) in &layer_offsets {
                if let Some(layer) = scene.layers.get_mut(idx) {
                    match &mut layer.kind {
                        schema::LayerKind::Rect { x, y, .. }
                        | schema::LayerKind::Text { x, y, .. }
                        | schema::LayerKind::Image { x, y, .. }
                        | schema::LayerKind::Svg { x, y, .. }
                        | schema::LayerKind::Video { x, y, .. }
                        | schema::LayerKind::Waveform { x, y, .. }
                        | schema::LayerKind::Lottie { x, y, .. }
                        | schema::LayerKind::Particles { x, y, .. }
                        | schema::LayerKind::Container { x, y, .. } => {
                            *x += ox;
                            *y += oy;
                        }
                        schema::LayerKind::Circle { cx, cy, .. } => {
                            *cx += ox;
                            *cy += oy;
                        }
                    }
                }
            }

            for (&idx, &smul) in &layer_scales {
                if let Some(layer) = scene.layers.get_mut(idx) {
                    match &mut layer.kind {
                        schema::LayerKind::Rect { scale, .. }
                        | schema::LayerKind::Circle { scale, .. }
                        | schema::LayerKind::Text { scale, .. }
                        | schema::LayerKind::Image { scale, .. }
                        | schema::LayerKind::Video { scale, .. }
                        | schema::LayerKind::Waveform { scale, .. }
                        | schema::LayerKind::Lottie { scale, .. }
                        | schema::LayerKind::Particles { scale, .. }
                        | schema::LayerKind::Container { scale, .. } => {
                            *scale *= smul;
                        }
                        _ => {}
                    }
                }
            }
            for (&idx, &drot) in &layer_rots {
                if let Some(layer) = scene.layers.get_mut(idx) {
                    match &mut layer.kind {
                        schema::LayerKind::Rect { rotation, .. }
                        | schema::LayerKind::Image { rotation, .. }
                        | schema::LayerKind::Svg { rotation, .. }
                        | schema::LayerKind::Video { rotation, .. }
                        | schema::LayerKind::Particles { rotation, .. }
                        | schema::LayerKind::Container { rotation, .. } => {
                            *rotation += drot;
                        }
                        _ => {}
                    }
                }
            }

            capabilities::validate_scene_capabilities(&scene)?;
            let assets = AssetStore::load_for_scene(&scene, &cache_dir)?;
            let rgba = renderer::render_frame_at_time(&scene, &assets, t)?;
            let img = premul_to_rgba_image(scene.canvas.width, scene.canvas.height, rgba)?;
            let mut png_bytes = Vec::new();
            img.write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)?;
            Ok((png_bytes, scene.canvas.width, scene.canvas.height))
        })();

        match render_res {
            Ok((png_bytes, width, height)) => {
                let ms = start.elapsed().as_secs_f64() * 1000.0;
                let _ = socket.send(Message::Binary(png_bytes)).await;
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({ "type": "stats", "width": width, "height": height, "ms": ms })
                            .to_string(),
                    ))
                    .await;
            }
            Err(e) => {
                let _ = socket
                    .send(Message::Text(
                        serde_json::json!({ "error": format!("{:#}", e) }).to_string(),
                    ))
                    .await;
            }
        }
    }
}
