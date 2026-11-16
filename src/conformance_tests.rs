use crate::assets::AssetStore;
use crate::capabilities::{validate_scene_capabilities, CapabilityRegistry};
use crate::renderer::render_frame;
use crate::schema::Scene;

fn render_json_frame(json: &str, frame_idx: u32) -> Vec<u8> {
    let scene: Scene = serde_json::from_str(json).expect("valid scene json");
    validate_scene_capabilities(&scene).expect("compatible schema_version");
    let assets = AssetStore::load_for_scene(&scene, std::path::Path::new(".rustren_cache"))
        .expect("load_for_scene ok");
    render_frame(&scene, &assets, frame_idx).expect("render_frame ok")
}

fn pixel_at(buf: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * width + x) * 4) as usize;
    [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]]
}

#[test]
fn test_schema_and_capabilities_alignment() {
    let schema_raw = include_str!("../schema.json");
    let schema_val: serde_json::Value =
        serde_json::from_str(schema_raw).expect("schema.json must be valid JSON");
    assert_eq!(schema_val["title"], "RustrenScene");

    let reg = CapabilityRegistry::current();
    assert_eq!(reg.renderer_version, "1.1");
    assert_eq!(reg.schema_version, "1.1");
    assert!(reg.capabilities.elements.container);
    assert!(reg.capabilities.elements.rect);
    assert!(reg.capabilities.elements.circle);
    assert!(reg.capabilities.elements.text);
    assert!(reg.capabilities.elements.svg);
    assert!(reg.capabilities.elements.image);
    assert!(reg.capabilities.elements.video);
    assert!(reg.capabilities.media.video);
    assert!(reg.capabilities.animation.color_interpolation);
    assert!(reg.capabilities.animation.path_animation);
    assert!(reg.capabilities.animation.stroke_animation);
    assert!(reg.capabilities.animation.cubic_bezier);
    assert!(reg.capabilities.styles.clip);
    assert!(reg.capabilities.animation.custom_spring);
    assert!(reg.capabilities.animation.repeat);
    assert!(reg.capabilities.animation.yoyo);
    assert!(reg.capabilities.effects.blur);
    assert!(reg.capabilities.effects.box_shadow);
    assert!(reg.capabilities.effects.blend_mode);
    assert!(reg.capabilities.typography.rich_text);
    assert!(reg.capabilities.typography.letter_spacing);
    assert!(reg.capabilities.timeline.local_timeline);
    assert!(reg.capabilities.timeline.series_sequencing);
    assert!(reg.capabilities.timeline.transition_overlap);
}

#[test]
fn test_version_compatibility_gate() {
    let v10 = r#"{"schema_version":"1.0","canvas":{"width":64,"height":64,"fps":30,"duration_secs":1.0},"layers":[]}"#;
    let v11 = r#"{"schema_version":"1.1","canvas":{"width":64,"height":64,"fps":30,"duration_secs":1.0},"layers":[]}"#;
    let v20 = r#"{"schema_version":"2.0","canvas":{"width":64,"height":64,"fps":30,"duration_secs":1.0},"layers":[]}"#;

    let s10: Scene = serde_json::from_str(v10).unwrap();
    let s11: Scene = serde_json::from_str(v11).unwrap();
    let s20: Scene = serde_json::from_str(v20).unwrap();

    assert!(validate_scene_capabilities(&s10).is_ok());
    assert!(validate_scene_capabilities(&s11).is_ok());
    assert!(validate_scene_capabilities(&s20).is_err());
}

#[test]
fn test_conformance_container_auto_size_renders_pixels() {
    let json = r##"{
        "schema_version": "1.1",
        "canvas": { "width": 100, "height": 100, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "container",
                "position": "absolute",
                "x": 10,
                "y": 10,
                "background": "#00FF00",
                "layout": { "direction": "row", "padding": 5 },
                "children": [
                    { "type": "rect", "width": 20, "height": 20, "color": "#FF0000" }
                ]
            }
        ]
    }"##;
    let buf = render_json_frame(json, 0);
    // Padding area should be green, inner rect should be red
    assert_eq!(pixel_at(&buf, 100, 12, 12), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&buf, 100, 20, 20), [255, 0, 0, 255]);
}

#[test]
fn test_conformance_container_clip_prevents_overflow() {
    let json = r##"{
        "schema_version": "1.1",
        "canvas": { "width": 120, "height": 60, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "container",
                "position": "absolute",
                "x": 20,
                "y": 20,
                "width": 40,
                "height": 20,
                "clip": true,
                "children": [
                    { "type": "rect", "position": "absolute", "x": 0, "y": 0, "width": 90, "height": 20, "color": "#FF0000" }
                ]
            }
        ]
    }"##;
    let buf = render_json_frame(json, 0);
    // Inside clip [20..60) is red; outside clip at x=70 is black background
    assert_eq!(pixel_at(&buf, 120, 30, 30), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&buf, 120, 70, 30), [0, 0, 0, 255]);
}

#[test]
fn test_conformance_cascade_opacity() {
    let make_scene = |cascade: bool| {
        format!(
            r##"{{
                "schema_version": "1.1",
                "canvas": {{ "width": 60, "height": 60, "fps": 30, "duration_secs": 1.0, "background": "#000000" }},
                "layers": [
                    {{
                        "type": "container",
                        "position": "absolute",
                        "x": 0, "y": 0, "width": 60, "height": 60,
                        "opacity": 0.5,
                        "cascade_opacity": {cascade},
                        "children": [
                            {{ "type": "rect", "position": "absolute", "x": 10, "y": 10, "width": 20, "height": 20, "color": "#FFFFFF" }}
                        ]
                    }}
                ]
            }}"##
        )
    };
    let buf_no = render_json_frame(&make_scene(false), 0);
    let buf_yes = render_json_frame(&make_scene(true), 0);
    assert_eq!(pixel_at(&buf_no, 60, 15, 15)[0], 255);
    assert!(pixel_at(&buf_yes, 60, 15, 15)[0] < 140);
}

#[test]
fn test_conformance_determinism_complex_scene() {
    let json = r##"{
        "schema_version": "1.1",
        "canvas": { "width": 160, "height": 160, "fps": 30, "duration_secs": 2.0, "background": "#050811" },
        "layers": [
            {
                "type": "container",
                "position": "absolute",
                "x": 20, "y": 20, "width": 120, "height": 120,
                "background": "#0F172A",
                "radius": 16,
                "clip": true,
                "cascade_opacity": true,
                "rotation": 15.0,
                "scale": 0.9,
                "layout": { "direction": "column", "gap": 8, "padding": 12 },
                "animations": [
                    { "property": "scale", "from": 0.8, "to": 1.1, "start_sec": 0.0, "end_sec": 0.5, "easing": "spring", "spring": { "stiffness": 150, "damping": 12, "mass": 1.0 } }
                ],
                "children": [
                    { "type": "circle", "r": 10, "color": "#22D3EE", "animations": [
                        { "property": "scale", "from": 0.7, "to": 1.3, "start_sec": 0.0, "end_sec": 0.5, "easing": "ease_in_out", "repeat": "infinite", "yoyo": true }
                    ]},
                    { "type": "text", "content": "OK", "font_size": 28, "color": "#F8FAFC", "max_width": 96 }
                ]
            }
        ]
    }"##;
    let frame_a = render_json_frame(json, 15);
    let frame_b = render_json_frame(json, 15);
    assert_eq!(frame_a, frame_b, "render_frame must be byte-for-byte deterministic");
}

#[test]
fn test_conformance_blur_and_shadow() {
    let json = r##"{
        "schema_version": "1.1",
        "canvas": { "width": 100, "height": 100, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "rect",
                "position": "absolute",
                "x": 20, "y": 20, "width": 30, "height": 30,
                "color": "#FFFFFF",
                "blur": 6.0,
                "shadow": { "color": "#FF0000FF", "offset_x": 15, "offset_y": 15, "blur": 4.0 }
            }
        ]
    }"##;
    let buf = render_json_frame(json, 0);
    // Shadow offset (x=55, y=55) should have red channel > 0
    let sh_px = pixel_at(&buf, 100, 55, 55);
    assert!(sh_px[0] > 0, "expected shadow red pixel, got {sh_px:?}");
    // Blurred edge just outside rect (x=18, y=35) should be > 0 due to blur
    let blur_px = pixel_at(&buf, 100, 18, 35);
    assert!(blur_px[0] > 0, "expected blurred edge pixel, got {blur_px:?}");
}

#[test]
fn test_conformance_blend_mode_difference() {
    let json = r##"{
        "schema_version": "1.1",
        "canvas": { "width": 60, "height": 60, "fps": 30, "duration_secs": 1.0, "background": "#FFFFFF" },
        "layers": [
            {
                "type": "rect",
                "position": "absolute",
                "x": 10, "y": 10, "width": 40, "height": 40,
                "color": "#FFFFFF",
                "blend_mode": "difference"
            }
        ]
    }"##;
    let buf = render_json_frame(json, 0);
    // White difference White == Black
    assert_eq!(pixel_at(&buf, 60, 20, 20), [0, 0, 0, 255]);
}

#[test]
fn test_conformance_rich_text_and_letter_spacing() {
    let json = r##"{
        "schema_version": "1.1",
        "canvas": { "width": 300, "height": 100, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "text",
                "position": "absolute",
                "x": 10, "y": 20,
                "font_size": 36,
                "letter_spacing": 6.0,
                "max_width": 280,
                "spans": [
                    { "text": "HI ", "color": "#FF0000", "font_weight": "bold" },
                    { "text": "WORLD", "color": "#00FF00" }
                ]
            }
        ]
    }"##;
    let buf = render_json_frame(json, 0);
    let has_red = buf.chunks_exact(4).any(|p| p[0] > 180 && p[1] < 50);
    let has_green = buf.chunks_exact(4).any(|p| p[1] > 180 && p[0] < 50);
    assert!(has_red && has_green, "expected both red and green rich text spans");
}

#[test]
fn test_conformance_timeline_local_and_series() {
    let json = r##"{
        "schema_version": "1.1",
        "canvas": { "width": 80, "height": 80, "fps": 10, "duration_secs": 3.0, "background": "#000000" },
        "layers": [
            {
                "type": "container",
                "position": "absolute",
                "x": 0, "y": 0, "width": 80, "height": 80,
                "start_sec": 0.5,
                "duration_secs": 2.5,
                "timeline": "series",
                "children": [
                    {
                        "type": "rect",
                        "position": "absolute",
                        "x": 10, "y": 10, "width": 20, "height": 20,
                        "color": "#FF0000",
                        "duration_secs": 1.0
                    },
                    {
                        "type": "rect",
                        "position": "absolute",
                        "x": 40, "y": 10, "width": 20, "height": 20,
                        "color": "#00FF00",
                        "duration_secs": 1.0,
                        "overlap_secs": 0.2
                    }
                ]
            }
        ]
    }"##;
    // Frame 0 (t=0.0s): before container start_sec=0.5 -> both black
    let f0 = render_json_frame(json, 0);
    assert_eq!(pixel_at(&f0, 80, 15, 15), [0, 0, 0, 255]);
    assert_eq!(pixel_at(&f0, 80, 45, 15), [0, 0, 0, 255]);

    // Frame 8 (t=0.8s, local_t=0.3s): only first child (red) is active
    let f8 = render_json_frame(json, 8);
    assert_eq!(pixel_at(&f8, 80, 15, 15), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&f8, 80, 45, 15), [0, 0, 0, 255]);

    // Frame 14 (t=1.4s, local_t=0.9s): overlap window [0.8..1.0], both red and green active!
    let f14 = render_json_frame(json, 14);
    assert_eq!(pixel_at(&f14, 80, 15, 15), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&f14, 80, 45, 15), [0, 255, 0, 255]);

    // Frame 18 (t=1.8s, local_t=1.3s): only second child (green) is active
    let f18 = render_json_frame(json, 18);
    assert_eq!(pixel_at(&f18, 80, 15, 15), [0, 0, 0, 255]);
    assert_eq!(pixel_at(&f18, 80, 45, 15), [0, 255, 0, 255]);
}

#[test]
fn test_conformance_video_layer_decode_and_trim() {
    let tmp_video = std::env::temp_dir().join("rustren_conformance_test_video.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=32x32:d=0.5:r=10",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=32x32:d=0.5:r=10",
            "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1:a=0",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            tmp_video.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("ffmpeg must be available for video conformance test");
    assert!(status.success(), "ffmpeg failed to generate test video");

    let video_path = tmp_video.to_string_lossy().replace('\\', "/");
    let json_untreated = format!(
        r#"{{
        "canvas": {{ "width": 32, "height": 32, "fps": 10, "duration_secs": 1.0 }},
        "layers": [
            {{ "type": "video", "src": "{video_path}", "x": 0, "y": 0, "width": 32, "height": 32 }}
        ]
    }}"#
    );
    let json_trimmed = format!(
        r#"{{
        "canvas": {{ "width": 32, "height": 32, "fps": 10, "duration_secs": 1.0 }},
        "layers": [
            {{ "type": "video", "src": "{video_path}", "x": 0, "y": 0, "width": 32, "height": 32, "trim_start_sec": 0.6, "playback_rate": 1.0, "muted": true }}
        ]
    }}"#
    );

    // Frame 0 without trim -> red-dominant
    let f0_raw = render_json_frame(&json_untreated, 0);
    let p_raw = pixel_at(&f0_raw, 32, 16, 16);
    assert!(p_raw[0] > 200 && p_raw[2] < 50, "expected red pixel, got {:?}", p_raw);

    // Frame 0 with trim_start_sec=0.6 -> blue-dominant
    let f0_trim_a = render_json_frame(&json_trimmed, 0);
    let f0_trim_b = render_json_frame(&json_trimmed, 0);
    assert_eq!(f0_trim_a, f0_trim_b, "video frame rendering must be deterministic");
    let p_trim = pixel_at(&f0_trim_a, 32, 16, 16);
    assert!(p_trim[2] > 200 && p_trim[0] < 50, "expected blue pixel after trim, got {:?}", p_trim);

    let _ = std::fs::remove_file(&tmp_video);
}

#[test]
fn test_conformance_color_interpolation_and_cubic_bezier() {
    let json = r##"{
        "canvas": { "width": 20, "height": 20, "fps": 10, "duration_secs": 1.0 },
        "layers": [
            {
                "type": "rect",
                "x": 0, "y": 0, "width": 20, "height": 20,
                "color": "#ff0000",
                "animations": [
                    {
                        "property": "color",
                        "from": "#ff0000",
                        "to": "#0000ff",
                        "start_sec": 0.0,
                        "end_sec": 1.0,
                        "easing": "cubic_bezier",
                        "bezier": [0.0, 0.0, 1.0, 1.0]
                    }
                ]
            }
        ]
    }"##;

    let f0 = render_json_frame(json, 0);
    assert_eq!(pixel_at(&f0, 20, 10, 10), [255, 0, 0, 255]);

    let f5_a = render_json_frame(json, 5);
    let f5_b = render_json_frame(json, 5);
    assert_eq!(f5_a, f5_b, "color interpolation must be deterministic");
    let p5 = pixel_at(&f5_a, 20, 10, 10);
    assert!(
        (p5[0] as i32 - 128).abs() <= 2 && p5[1] == 0 && (p5[2] as i32 - 128).abs() <= 2,
        "expected purple midpoint ~[128,0,128,255], got {:?}",
        p5
    );

    let f10 = render_json_frame(json, 10);
    assert_eq!(pixel_at(&f10, 20, 10, 10), [0, 0, 255, 255]);
}

#[test]
fn test_conformance_svg_path_and_rect_stroke_animation() {
    let tmp_svg = std::env::temp_dir().join("rustren_conformance_path.svg");
    std::fs::write(
        &tmp_svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20">
            <path d="M 0 10 L 100 10" stroke="#00ff00" stroke-width="6" fill="none"/>
        </svg>"##,
    )
    .unwrap();
    let svg_path = tmp_svg.to_string_lossy().replace('\\', "/");

    let json_svg = format!(
        r##"{{
        "canvas": {{ "width": 100, "height": 20, "fps": 10, "duration_secs": 1.0 }},
        "layers": [
            {{
                "type": "svg",
                "src": "{svg_path}",
                "x": 0, "y": 0, "width": 100, "height": 20,
                "path_progress": 0.0,
                "animations": [
                    {{ "property": "path_progress", "from": 0.0, "to": 1.0, "start_sec": 0.0, "end_sec": 1.0, "easing": "linear" }}
                ]
            }}
        ]
    }}"##
    );

    let f5 = render_json_frame(&json_svg, 5);
    assert_eq!(pixel_at(&f5, 100, 25, 10), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&f5, 100, 75, 10), [0, 0, 0, 255]);

    let f10 = render_json_frame(&json_svg, 10);
    assert_eq!(pixel_at(&f10, 100, 25, 10), [0, 255, 0, 255]);
    assert_eq!(pixel_at(&f10, 100, 75, 10), [0, 255, 0, 255]);

    let _ = std::fs::remove_file(&tmp_svg);

    // Rect border stroke animation (path_progress = 0.25 draws top edge, not bottom edge)
    let json_rect = r##"{
        "canvas": { "width": 40, "height": 40, "fps": 10, "duration_secs": 1.0 },
        "layers": [
            {
                "type": "rect",
                "x": 5, "y": 5, "width": 30, "height": 30,
                "color": "#00000000",
                "border_color": "#ff0000",
                "border_width": 4.0,
                "path_progress": 0.25
            }
        ]
    }"##;
    let fr = render_json_frame(json_rect, 0);
    assert_eq!(pixel_at(&fr, 40, 20, 5), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&fr, 40, 20, 35), [0, 0, 0, 255]);
}

#[test]
fn test_conformance_circle_absolute_cx_inside_container_and_svg_gradient_progress() {
    // 1. Circle absolute inside container at x=50, y=50 with cx animated 10 -> 30
    let json_circle = r##"{
        "canvas": { "width": 100, "height": 100, "fps": 10, "duration_secs": 1.0 },
        "layers": [
            {
                "type": "container",
                "x": 50, "y": 50, "width": 50, "height": 50,
                "children": [
                    {
                        "type": "circle",
                        "position": "absolute",
                        "cx": 10, "cy": 25, "r": 5,
                        "color": "#ff0000",
                        "animations": [
                            { "property": "cx", "from": 10.0, "to": 30.0, "start_sec": 0.0, "end_sec": 1.0, "easing": "linear" }
                        ]
                    }
                ]
            }
        ]
    }"##;
    let f10 = render_json_frame(json_circle, 10);
    // At t=1.0s, circle center should be at (50+30=80, 50+25=75), NOT (30, 75)
    assert_eq!(pixel_at(&f10, 100, 80, 75), [255, 0, 0, 255]);
    assert_eq!(pixel_at(&f10, 100, 30, 75), [0, 0, 0, 255]);

    // 2. SVG with linearGradient fill + path_progress=0.5 still renders gradient fill
    let tmp_svg = std::env::temp_dir().join("rustren_conformance_grad_svg.svg");
    std::fs::write(
        &tmp_svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40">
            <defs>
                <linearGradient id="g" x1="0" y1="0" x2="1" y2="0">
                    <stop offset="0%" stop-color="#00ff00"/>
                    <stop offset="100%" stop-color="#0000ff"/>
                </linearGradient>
            </defs>
            <path d="M 0 0 L 40 0 L 40 40 L 0 40 Z" fill="url(#g)" stroke="#ffffff" stroke-width="2"/>
        </svg>"##,
    )
    .unwrap();
    let svg_path = tmp_svg.to_string_lossy().replace('\\', "/");
    let json_svg = format!(
        r##"{{
        "canvas": {{ "width": 40, "height": 40, "fps": 10, "duration_secs": 1.0 }},
        "layers": [
            {{ "type": "svg", "src": "{svg_path}", "x": 0, "y": 0, "width": 40, "height": 40, "path_progress": 0.5 }}
        ]
    }}"##
    );
    let f_svg = render_json_frame(&json_svg, 0);
    let p_mid = pixel_at(&f_svg, 40, 20, 20);
    assert!(p_mid[1] > 100 && p_mid[2] > 100, "expected green-blue gradient fill, got {:?}", p_mid);
    let _ = std::fs::remove_file(&tmp_svg);
}

#[test]
fn test_schema_md_examples_parse_and_validate() {
    let schema_json: serde_json::Value =
        serde_json::from_str(include_str!("../schema.json")).expect("schema.json must be valid JSON");
    assert!(schema_json.get("properties").is_some(), "schema.json missing properties");

    let md = include_str!("../SCHEMA.md");
    let mut rest = md;
    let mut checked = 0usize;
    while let Some(start) = rest.find("```json\n") {
        let after = &rest[start + "```json\n".len()..];
        let Some(end) = after.find("```") else {
            break;
        };
        let block = after[..end].trim();
        rest = &after[end + 3..];
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(block) {
            if val.get("canvas").is_some() && val.get("layers").is_some() {
                let scene: crate::schema::Scene = serde_json::from_value(val)
                    .unwrap_or_else(|e| panic!("SCHEMA.md example #{checked} failed to parse: {e}\n{block}"));
                crate::capabilities::validate_scene_capabilities(&scene)
                    .unwrap_or_else(|e| panic!("SCHEMA.md example #{checked} failed capability validation: {e}"));
                checked += 1;
            }
        }
    }
    assert!(checked >= 1, "Expected at least 1 complete scene JSON example in SCHEMA.md");
}

#[test]
fn test_smart_lint_grouping_contrast_and_bounds() {
    // Landscape scene: 16px should NOT warn, but 10px x5 should group into 1 summary line,
    // plus low-contrast text on blue rect and off-canvas rect should warn.
    let json = r##"{
        "canvas": { "width": 1280, "height": 720, "fps": 30, "duration_secs": 3.0, "background": "#2D4AF2" },
        "layers": [
            { "type": "rect", "x": 2000, "y": 100, "width": 100, "height": 100, "color": "#FFFFFF" },
            { "type": "text", "x": 100, "y": 100, "font_size": 16, "color": "#FFFFFF", "content": "HUD OK" },
            { "type": "text", "x": 100, "y": 140, "font_size": 16, "color": "#3344EE", "content": "Low contrast" },
            { "type": "text", "x": 100, "y": 200, "font_size": 10, "color": "#FFFFFF", "content": "tiny1" },
            { "type": "text", "x": 100, "y": 220, "font_size": 10, "color": "#FFFFFF", "content": "tiny2" },
            { "type": "text", "x": 100, "y": 240, "font_size": 10, "color": "#FFFFFF", "content": "tiny3" },
            { "type": "text", "x": 100, "y": 260, "font_size": 10, "color": "#FFFFFF", "content": "tiny4" }
        ]
    }"##;
    let scene: crate::schema::Scene = serde_json::from_str(json).unwrap();
    let warnings = crate::lint::collect_lint_warnings(&scene);
    assert!(
        warnings.iter().any(|w| w.contains("4 layer memiliki font_size < 14px")),
        "expected grouped font_size warning, got {warnings:?}"
    );
    assert!(
        warnings.iter().any(|w| w.contains("kontras rendah")),
        "expected low contrast warning, got {warnings:?}"
    );
    assert!(
        warnings.iter().any(|w| w.contains("di luar kanvas")),
        "expected out of canvas warning, got {warnings:?}"
    );
    assert!(
        !warnings.iter().any(|w| w.contains("layers[1]")),
        "16px HUD in landscape should not warn: {warnings:?}"
    );
}

#[test]
fn test_layout_inspection_and_snapshot_at_time() {
    let json = r##"{
        "canvas": { "width": 320, "height": 180, "fps": 30, "duration_secs": 2.0, "background": "#101010" },
        "layers": [
            {
                "type": "container",
                "name": "root",
                "x": 0,
                "y": 0,
                "width": 320,
                "height": 180,
                "children": [
                    {
                        "type": "text",
                        "name": "title",
                        "x": 40,
                        "y": 60,
                        "font_size": 32,
                        "color": "#FFFFFF",
                        "content": "rustren",
                        "position": "absolute"
                    }
                ]
            }
        ]
    }"##;
    let scene: crate::schema::Scene = serde_json::from_str(json).unwrap();
    let assets = crate::assets::AssetStore::load_for_scene(&scene, &std::env::temp_dir()).unwrap();
    let layout = crate::renderer::inspect_layout_at_time(&scene, &assets, 0.5).unwrap();
    assert_eq!(layout.len(), 1);
    assert_eq!(layout[0].children.len(), 1);
    let title = &layout[0].children[0];
    assert_eq!(title.x, 40.0);
    assert_eq!(title.y, 60.0);
    let measured = title.text_measured.as_ref().expect("text_measured must be present");
    assert!(measured.width > 20.0 && measured.height > 20.0);

    let frame = crate::renderer::render_frame_at_time(&scene, &assets, 0.5).unwrap();
    assert_eq!(frame.len(), 320 * 180 * 4);
}

#[test]
fn test_tier2_anchor_skew_stagger_dynamic_and_golden_hash() {
    use sha2::{Digest, Sha256};

    let json = r##"{
        "canvas": { "width": 320, "height": 180, "fps": 30, "duration_secs": 2.0, "background": "#E9E6D3" },
        "layers": [
            {
                "type": "rect",
                "x": 160,
                "y": 90,
                "width": 80,
                "height": 60,
                "radius": 14,
                "color": "#2D4AF2",
                "anchor": "center",
                "skew_x": -12
            },
            {
                "type": "container",
                "x": 20,
                "y": 20,
                "width": 200,
                "height": 40,
                "timeline": "local",
                "stagger_secs": 0.5,
                "children": [
                    { "type": "rect", "x": 0, "y": 0, "width": 16, "height": 16, "color": "#161310", "position": "absolute" },
                    { "type": "rect", "x": 24, "y": 0, "width": 16, "height": 16, "color": "#161310", "position": "absolute" }
                ]
            },
            {
                "type": "text",
                "x": 160,
                "y": 150,
                "font_size": 20,
                "color": "#161310",
                "anchor": "center",
                "dynamic": { "mode": "typewriter", "text": "rustren", "start_sec": 0.0, "end_sec": 1.0 }
            },
            {
                "type": "text",
                "x": 280,
                "y": 20,
                "font_size": 16,
                "color": "#161310",
                "dynamic": { "mode": "timecode", "format": "mm:ss" }
            }
        ]
    }"##;

    let scene: crate::schema::Scene = serde_json::from_str(json).unwrap();
    let assets = crate::assets::AssetStore::load_for_scene(&scene, &std::env::temp_dir()).unwrap();

    // At t=0.25s, stagger child[0] is active (0.25 >= 0.0), child[1] is NOT yet active (0.25 < 0.5)
    let layout_early = crate::renderer::inspect_layout_at_time(&scene, &assets, 0.25).unwrap();
    assert_eq!(layout_early[1].children.len(), 1, "stagger should hide child[1] at t=0.25s");

    // At t=0.75s, both stagger children are active, and centered rect covers (160, 90)
    let f1 = crate::renderer::render_frame_at_time(&scene, &assets, 0.75).unwrap();
    let f2 = crate::renderer::render_frame_at_time(&scene, &assets, 0.75).unwrap();
    let h1 = format!("{:x}", Sha256::digest(&f1));
    let h2 = format!("{:x}", Sha256::digest(&f2));
    assert_eq!(h1, h2, "golden-frame determinism hash must match across renders");

    let center_px = pixel_at(&f1, 320, 160, 90);
    assert!(center_px[2] > 200 && center_px[0] < 80, "centered blue rect expected at (160,90), got {:?}", center_px);
}
