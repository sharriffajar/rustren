use super::*;
use crate::schema::Scene;
use std::collections::HashMap;

#[test]
fn test_parse_hex_color() {
    let c = parse_hex_color("#FF0000", 0.5).unwrap();
    assert!((c.red() - 1.0).abs() < 1e-5);
    assert!((c.alpha() - 0.5).abs() < 1e-5);
}

#[test]
fn test_parse_hex_color_expands_shorthand() {
    // schema.json allows `#RGB`, so the renderer must accept it too.
    let short = parse_hex_color("#FFF", 1.0).unwrap();
    assert!((short.red() - 1.0).abs() < 1e-5);
    assert!((short.green() - 1.0).abs() < 1e-5);
    assert!((short.blue() - 1.0).abs() < 1e-5);
    let short_alpha = parse_hex_color("#F00F", 1.0).unwrap();
    assert!((short_alpha.red() - 1.0).abs() < 1e-5);
    assert_eq!(short_alpha.alpha(), 1.0);
    let half = parse_hex_color("#0008", 1.0).unwrap();
    assert!((half.alpha() - 136.0 / 255.0).abs() < 1e-5);
}

#[test]
fn test_parse_hex_color_accepts_named_and_rgba() {
    let white = parse_hex_color("white", 1.0).unwrap();
    assert!((white.red() - 1.0).abs() < 1e-5);
    assert!((white.green() - 1.0).abs() < 1e-5);
    let clear = parse_hex_color("transparent", 0.5).unwrap();
    assert_eq!(clear.alpha(), 0.0);
    let rgba = parse_hex_color("rgba(0, 120, 212, 0.14)", 1.0).unwrap();
    assert!((rgba.blue() - 212.0 / 255.0).abs() < 1e-3);
    assert!((rgba.alpha() - 0.14).abs() < 1e-3);
}

#[test]
fn test_render_frame_with_container_and_absolute() {
    let json = r##"{
        "canvas": { "width": 320, "height": 240, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "container",
                "x": 10, "y": 10, "width": 300, "height": 220,
                "gradient": { "from": "#111111", "to": "#222222", "angle": 45 },
                "border_color": "#da5c2c", "border_width": 2,
                "children": [
                    { "type": "rect", "width": 50, "height": 50, "color": "#38BDF8" },
                    { "type": "rect", "position": "absolute", "x": 200, "y": 20, "width": 40, "height": 40, "color": "#22C55E" }
                ]
            }
        ]
    }"##;
    let scene: Scene = serde_json::from_str(json).unwrap();
    let assets = AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        lotties: HashMap::new(),
        audio_paths: vec![],
        #[cfg(feature = "exp-3d")]
        mesh3d: HashMap::new(),
    };
    let frame = render_frame(&scene, &assets, 0).unwrap();
    assert_eq!(frame.len(), 320 * 240 * 4);
}

#[test]
fn test_track_matte_alpha_and_inverted() {
    let json = r##"{
        "canvas": { "width": 100, "height": 100, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "rect",
                "x": 0, "y": 0, "width": 100, "height": 100,
                "color": "#FF0000",
                "track_matte": "alpha",
                "matte_layer": {
                    "type": "rect",
                    "x": 20, "y": 20, "width": 40, "height": 40,
                    "color": "#FFFFFF"
                }
            }
        ]
    }"##;
    let scene: Scene = serde_json::from_str(json).unwrap();
    let assets = AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        lotties: HashMap::new(),
        audio_paths: vec![],
        #[cfg(feature = "exp-3d")]
        mesh3d: HashMap::new(),
    };
    let frame = render_frame(&scene, &assets, 0).unwrap();
    // Pixel at (10, 10) is outside matte -> should be black (0, 0, 0)
    let idx_out = (10 * 100 + 10) * 4;
    assert_eq!(frame[idx_out], 0);
    // Pixel at (30, 30) is inside matte -> should be red (255, 0, 0)
    let idx_in = (30 * 100 + 30) * 4;
    assert_eq!(frame[idx_in], 255);
    assert_eq!(frame[idx_in + 1], 0);
}

#[test]
fn test_track_matte_inside_25d_container() {
    let json = r##"{
        "canvas": { "width": 200, "height": 200, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "container",
                "x": 50, "y": 50, "width": 100, "height": 100,
                "rotate_y": 15,
                "perspective": 800,
                "children": [
                    {
                        "type": "rect",
                        "x": 0, "y": 0, "width": 100, "height": 100,
                        "color": "#00FF00",
                        "track_matte": "alpha",
                        "matte_layer": {
                            "type": "rect",
                            "x": 0, "y": 0, "width": 100, "height": 100,
                            "color": "#FFFFFF"
                        }
                    }
                ]
            }
        ]
    }"##;
    let scene: Scene = serde_json::from_str(json).unwrap();
    let assets = AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        lotties: HashMap::new(),
        audio_paths: vec![],
        #[cfg(feature = "exp-3d")]
        mesh3d: HashMap::new(),
    };
    let frame = render_frame(&scene, &assets, 0).unwrap();
    let green_pixels = frame
        .chunks_exact(4)
        .filter(|px| px[1] > 180 && px[0] < 50 && px[2] < 50)
        .count();
    assert!(
        green_pixels > 1000,
        "track_matte inside 2.5D container must render visible pixels, got {green_pixels}"
    );
}

#[test]
fn test_waveform_color_animation() {
    let json = r##"{
        "canvas": { "width": 120, "height": 80, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "waveform",
                "x": 10, "y": 10, "width": 100, "height": 60,
                "bars": 8,
                "color": "#FF0000",
                "animations": [
                    {
                        "property": "color",
                        "from": "#FF0000",
                        "to": "#0000FF",
                        "start_sec": 0.0,
                        "end_sec": 1.0
                    }
                ]
            }
        ]
    }"##;
    let scene: Scene = serde_json::from_str(json).unwrap();
    let assets = AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        lotties: HashMap::new(),
        audio_paths: vec![],
        #[cfg(feature = "exp-3d")]
        mesh3d: HashMap::new(),
    };
    let f0 = crate::renderer::render_frame_at_time(&scene, &assets, 0.0).unwrap();
    let f1 = crate::renderer::render_frame_at_time(&scene, &assets, 1.0).unwrap();
    let red_t0 = f0.chunks_exact(4).filter(|p| p[0] > 200 && p[2] < 50).count();
    let blue_t1 = f1.chunks_exact(4).filter(|p| p[2] > 200 && p[0] < 50).count();
    assert!(red_t0 > 100, "waveform at t=0 should be red, got {red_t0}");
    assert!(blue_t1 > 100, "waveform at t=1 should be blue, got {blue_t1}");
}

#[test]
fn test_echo_smear_renders_trail() {
    let json = r##"{
        "canvas": { "width": 200, "height": 100, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "rect",
                "x": 0, "y": 25, "width": 20, "height": 50,
                "color": "#FFFFFF",
                "echo": { "count": 4, "step_sec": 0.1, "decay": 0.5 },
                "animations": [
                    { "property": "x", "from": 0, "to": 150, "start_sec": 0.0, "end_sec": 1.0, "easing": "linear" }
                ]
            }
        ]
    }"##;
    let scene: Scene = serde_json::from_str(json).unwrap();
    let assets = AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        lotties: HashMap::new(),
        audio_paths: vec![],
        #[cfg(feature = "exp-3d")]
        mesh3d: HashMap::new(),
    };
    // At frame 15 (t = 0.5s), main rect is at x = 75..95, echo 1 (t=0.4s) is at x = 60..80
    let frame = render_frame(&scene, &assets, 15).unwrap();
    let idx_trail = (50 * 200 + 65) * 4;
    assert!(frame[idx_trail] > 0, "Expected echo trail pixels at x=65");
}

#[test]
fn test_diagnose_layout_detects_sibling_text_overlap_and_clipping() {
    let json = r##"{
        "canvas": { "width": 400, "height": 200, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "text",
                "name": "heading",
                "x": 20, "y": 20, "font_size": 32,
                "color": "#FFFFFF",
                "content": "Overlapping Headline"
            },
            {
                "type": "text",
                "name": "subtitle",
                "x": 20, "y": 28, "font_size": 18,
                "color": "#CCCCCC",
                "content": "Overlapping Subtitle"
            },
            {
                "type": "text",
                "name": "clipped_edge",
                "x": 360, "y": 140, "font_size": 24,
                "color": "#FFAA00",
                "content": "Clipped Edge Label"
            },
            {
                "type": "container",
                "name": "narrow_card",
                "x": 20, "y": 90, "width": 80, "height": 60,
                "children": [
                    {
                        "type": "text",
                        "name": "wrapped_inside",
                        "font_size": 20,
                        "color": "#FFFFFF",
                        "content": "VeryLongTitle WordWrap"
                    }
                ]
            }
        ]
    }"##;
    let scene: Scene = serde_json::from_str(json).unwrap();
    let assets = AssetStore::load_for_scene(&scene, &std::env::temp_dir()).unwrap();

    let layout = inspect_layout_at_time(&scene, &assets, 0.5).unwrap();
    let card_text = &layout[3].children[0];
    let tm = card_text.text_measured.as_ref().expect("text_measured");
    assert!(
        tm.line_count >= 2,
        "Expected narrow_card text to wrap into >=2 lines, got {}",
        tm.line_count
    );

    let diags = diagnose_layout_at_time(&scene, &assets, 0.5).unwrap();
    assert!(
        diags
            .iter()
            .any(|d| d.kind == LayoutDiagnosticKind::SiblingOverlap),
        "Expected SiblingOverlap diagnostic, got: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|d| d.kind == LayoutDiagnosticKind::CanvasEdgeClipped && d.path == "layers[2]"),
        "Expected CanvasEdgeClipped on layers[2], got: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .any(|d| d.kind == LayoutDiagnosticKind::UnexpectedTextWrap
                && d.path == "layers[3].children[0]"),
        "Expected UnexpectedTextWrap on layers[3].children[0], got: {diags:?}"
    );
}

#[test]
fn test_25d_perspective_rotation_and_backface_culling() {
    let empty_assets = AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        lotties: HashMap::new(),
        audio_paths: vec![],
        #[cfg(feature = "exp-3d")]
        mesh3d: HashMap::new(),
    };

    // Tilted 3D card should exhibit perspective foreshortening: left edge taller than right edge for ry > 0
    // Wait: for ry = +40 deg, u > 0 (right side) has z3 = -u*sin(ry) < 0 (closer to camera at -d -> denom = d + z3 smaller -> LARGER!),
    // while left side u < 0 has z3 > 0 (farther -> SMALLER!). Either way left vs right top y must differ visibly.
    let tilted_html = r##"
    <scene width="200" height="200" fps="30" duration="1.0" background="#000000">
        <rect x="40" y="50" width="120" height="100" color="#FFFFFF" rotate-y="40" perspective="400" />
    </scene>
    "##;
    let tilted_scene = crate::compilers::html::parse_html_scene(tilted_html).unwrap();
    let tilted_frame = render_frame_at_time(&tilted_scene, &empty_assets, 0.0).unwrap();

    let count_white_col = |x: u32| -> usize {
        (0..200)
            .filter(|&y| tilted_frame[((y * 200 + x) * 4) as usize] > 128)
            .count()
    };
    let left_h = count_white_col(65);
    let right_h = count_white_col(135);
    assert!(
        left_h > 0 && right_h > left_h,
        "Expected perspective keystone foreshortening (left_h={left_h} < right_h={right_h})"
    );

    // 180-deg flip with backface-visible="false" must cull completely; true must render
    let culled_html = r##"
    <scene width="100" height="100" fps="30" duration="1.0" background="#000000">
        <rect x="20" y="20" width="60" height="60" color="#FF0000" rotate-y="180" backface-visible="false" />
    </scene>
    "##;
    let culled_scene = crate::compilers::html::parse_html_scene(culled_html).unwrap();
    let culled_frame = render_frame_at_time(&culled_scene, &empty_assets, 0.0).unwrap();
    let c_idx = (50 * 100 + 50) * 4;
    assert_eq!(
        culled_frame[c_idx], 0,
        "Backface should be culled when backface-visible=false"
    );

    let visible_html = r##"
    <scene width="100" height="100" fps="30" duration="1.0" background="#000000">
        <rect x="20" y="20" width="60" height="60" color="#FF0000" rotate-y="180" backface-visible="true" />
    </scene>
    "##;
    let visible_scene = crate::compilers::html::parse_html_scene(visible_html).unwrap();
    let visible_frame = render_frame_at_time(&visible_scene, &empty_assets, 0.0).unwrap();
    assert_eq!(
        visible_frame[c_idx], 255,
        "Backface should render when backface-visible=true"
    );
}

#[test]
fn test_z_index_depth_sorting_root_and_container() {
    let empty_assets = AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        lotties: HashMap::new(),
        audio_paths: vec![],
        #[cfg(feature = "exp-3d")]
        mesh3d: HashMap::new(),
    };

    let html = r##"
    <scene width="200" height="100" fps="30" duration="1.0" background="#000000">
        <!-- Container children depth sort: red (z=10) rendered after green (z=1) even though green is second in DOM -->
        <container x="10" y="10" width="80" height="80">
            <rect position="absolute" x="10" y="10" width="40" height="40" color="#FF0000" z-index="10" />
            <rect position="absolute" x="10" y="10" width="40" height="40" color="#00FF00" z-index="1" />
        </container>
        <!-- Root layers depth sort: blue (z=5) on top of yellow (z=-2) -->
        <rect x="120" y="20" width="40" height="40" color="#0000FF" z="5" />
        <rect x="120" y="20" width="40" height="40" color="#FFFF00" z="-2" />
    </scene>
    "##;
    let scene = crate::compilers::html::parse_html_scene(html).unwrap();
    let frame = render_frame_at_time(&scene, &empty_assets, 0.0).unwrap();

    // Container overlap pixel at (30, 30) -> Red (255, 0, 0)
    let c_idx = (30 * 200 + 30) * 4;
    assert_eq!(&frame[c_idx..c_idx + 4], &[255, 0, 0, 255]);

    // Root overlap pixel at (130, 30) -> Blue (0, 0, 255)
    let r_idx = (30 * 200 + 130) * 4;
    assert_eq!(&frame[r_idx..r_idx + 4], &[0, 0, 255, 255]);
}

#[test]
fn test_surface_effects_and_particles_determinism() {
    let empty_assets = AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        lotties: HashMap::new(),
        audio_paths: vec![],
        #[cfg(feature = "exp-3d")]
        mesh3d: HashMap::new(),
    };

    let html = r##"
    <scene width="240" height="160" fps="30" duration="2.0" background="#000000">
        <rect x="10" y="10" width="100" height="140" color="#FFFFFF" vignette="0.9" grain="0.35" />
        <rect x="120" y="10" width="100" height="140" color="#DDDDDD" halftone="6" />
        <particles x="10" y="10" width="220" height="140" count="32" seed="777" shape="confetti" speed="1.2" gravity="160" />
    </scene>
    "##;
    let scene = crate::compilers::html::parse_html_scene(html).unwrap();
    let f0_a = render_frame_at_time(&scene, &empty_assets, 0.6).unwrap();
    let f0_b = render_frame_at_time(&scene, &empty_assets, 0.6).unwrap();
    assert_eq!(
        f0_a, f0_b,
        "Surface effects + particles must be byte-for-byte deterministic"
    );

    // Vignette should darken near corner (14, 14) compared to center (60, 80)
    let corner_luma = f0_a[(14 * 240 + 14) * 4];
    let center_luma = f0_a[(80 * 240 + 60) * 4];
    assert!(
        corner_luma < center_luma,
        "Expected vignette corner {corner_luma} < center {center_luma}"
    );

    // At t=0.0 vs t=1.2 particles move significantly (frames differ)
    let f_early = render_frame_at_time(&scene, &empty_assets, 0.0).unwrap();
    assert_ne!(f0_a, f_early);
}

#[test]
fn test_shadow_container_fx_cache_hits() {
    let empty_assets = AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        lotties: HashMap::new(),
        audio_paths: vec![],
        #[cfg(feature = "exp-3d")]
        mesh3d: HashMap::new(),
    };

    let html = r##"
    <scene width="400" height="300" fps="30" duration="2.0" background="#0b0c10">
        <container id="card" x="40" y="40" width="200" height="120" background="#16181d" radius="12" shadow="0 10 24 rgba(0,0,0,0.5)">
            <rect width="60" height="60" color="#f59e0b" />
        </container>
        <animate target="card" property="opacity" from="0" to="1" start="0.0" end="0.5" />
    </scene>
    "##;
    let scene = crate::compilers::html::parse_html_scene(html).unwrap();

    // Record hits before and verify global cross-thread FX cache populates & hits across frames
    let (_, _, hits_before, _) = get_cache_stats();

    let _ = render_frame_at_time(&scene, &empty_assets, 1.0).unwrap();
    let _ = render_frame_at_time(&scene, &empty_assets, 1.1).unwrap();
    let _ = render_frame_at_time(&scene, &empty_assets, 1.2).unwrap();
    let _ = render_frame_at_time(&scene, &empty_assets, 1.3).unwrap();

    let (_, _, hits_after, _) = get_cache_stats();
    let has_entries = !GLOBAL_BLUR_CACHE.read().unwrap().is_empty();
    assert!(has_entries, "Expected populated global FX cache entries");
    assert!(
        hits_after >= hits_before + 3,
        "Expected at least 3 FX cache hits on subsequent frames, delta={}",
        hits_after - hits_before
    );
}

#[cfg(feature = "exp-3d")]
mod exp_3d_tests {
    use crate::assets::AssetStore;
    use crate::compilers::html::parse_html_scene;
    use crate::renderer::mesh3d::{Mesh, render_mesh3d_to_pixmap};
    use std::collections::HashMap;
    use tiny_skia::Pixmap;

    #[test]
    fn test_mesh3d_primitives_generate_geometry() {
        let cube = Mesh::cube();
        assert_eq!(cube.vertices.len(), 24);
        assert_eq!(cube.indices.len(), 36);
        assert_eq!(cube.indices.len() % 3, 0);

        let sphere = Mesh::sphere(12, 24);
        assert!(sphere.vertices.len() > 0);
        assert!(sphere.indices.len() > 0);
        assert_eq!(sphere.indices.len() % 3, 0);

        let pyramid = Mesh::pyramid();
        assert_eq!(pyramid.vertices.len(), 18);
        assert_eq!(pyramid.indices.len(), 18);

        let torus = Mesh::torus(0.5, 0.2, 16, 12);
        assert!(torus.vertices.len() > 0);
        assert!(torus.indices.len() > 0);
        assert_eq!(torus.indices.len() % 3, 0);
    }

    #[test]
    fn test_mesh3d_renders_non_empty_pixels() {
        let mut pixmap = Pixmap::new(200, 200).unwrap();
        let mesh = Mesh::cube();

        render_mesh3d_to_pixmap(
            &mut pixmap.as_mut(),
            &mesh,
            50.0,
            50.0,
            100.0,
            100.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
            None,
        );

        // Check that some pixels were drawn (non-transparent)
        let data = pixmap.data();
        let mut non_zero = 0;
        for chunk in data.chunks_exact(4) {
            if chunk[3] > 0 {
                non_zero += 1;
            }
        }
        assert!(
            non_zero > 100,
            "Expected many non-transparent pixels, got {}",
            non_zero
        );
    }

    #[test]
    fn test_mesh3d_deterministic_render() {
        let mesh = Mesh::cube();

        let mut pixmap1 = Pixmap::new(200, 200).unwrap();
        render_mesh3d_to_pixmap(
            &mut pixmap1.as_mut(),
            &mesh,
            50.0,
            50.0,
            100.0,
            100.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            45.0,
            30.0,
            15.0,
            None,
        );

        let mut pixmap2 = Pixmap::new(200, 200).unwrap();
        render_mesh3d_to_pixmap(
            &mut pixmap2.as_mut(),
            &mesh,
            50.0,
            50.0,
            100.0,
            100.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            45.0,
            30.0,
            15.0,
            None,
        );

        assert_eq!(
            pixmap1.data(),
            pixmap2.data(),
            "Two renders with same params must be byte-for-byte identical"
        );
    }

    #[test]
    fn test_mesh3d_deterministic_full_pipeline() {
        let html = r##"
        <scene width="320" height="240" fps="30" duration="2.0" background="#000000">
            <mesh3d x="80" y="40" width="160" height="160" primitive="cube" color="#38BDF8" fov="50" wireframe="false" light-dir="0.5, -1.0, 0.5">
                <animate property="rotate_x" from="0" to="720" start="0" end="2" easing="linear" repeat="infinite" />
                <animate property="rotate_y" from="0" to="360" start="0" end="2" easing="linear" repeat="infinite" />
            </mesh3d>
        </scene>
        "##;

        let scene = parse_html_scene(html).unwrap();
        let assets = AssetStore {
            fonts: vec![],
            images: HashMap::new(),
            videos: HashMap::new(),
            svgs: HashMap::new(),
        lotties: HashMap::new(),
            audio_paths: vec![],
            #[cfg(feature = "exp-3d")]
            mesh3d: HashMap::new(),
        };

        // Render same frame twice
        let frame1 = crate::renderer::render_frame_at_time(&scene, &assets, 1.0).unwrap();
        let frame2 = crate::renderer::render_frame_at_time(&scene, &assets, 1.0).unwrap();

        assert_eq!(frame1, frame2, "Full pipeline render must be deterministic");
    }

    #[test]
    fn test_mesh3d_anti_aliasing_produces_partial_coverage() {
        // Anti-aliasing (supersample + box downsample) must produce partially
        // covered pixels along the silhouette instead of hard 1-bit edges.
        let mut pixmap = Pixmap::new(200, 200).unwrap();
        let mesh = Mesh::sphere(16, 32);

        render_mesh3d_to_pixmap(
            &mut pixmap.as_mut(),
            &mesh,
            20.0,
            20.0,
            160.0,
            160.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            20.0,
            30.0,
            0.0,
            None,
        );

        let data = pixmap.data();
        let mut opaque = 0;
        let mut partial = 0;
        for chunk in data.chunks_exact(4) {
            match chunk[3] {
                255 => opaque += 1,
                1..=254 => partial += 1,
                _ => {}
            }
        }

        assert!(opaque > 500, "solid interior expected, got {}", opaque);
        // A 2x supersample can only ever yield alpha multiples of 64, and a
        // 160px sphere has ~500 edge pixels, so a healthy number is expected.
        assert!(
            partial > 100,
            "silhouette should be anti-aliased, got {} partial pixels",
            partial
        );
    }

    #[test]
    fn test_mesh3d_no_cracks_inside_silhouette() {
        // Regression: at 2x supersampling, shared triangle edges used to leave
        // 1px background seams inside the body (visible as speckles on the sphere).
        let mut pixmap = Pixmap::new(200, 200).unwrap();
        let mesh = Mesh::sphere(16, 32);

        render_mesh3d_to_pixmap(
            &mut pixmap.as_mut(),
            &mesh,
            20.0,
            20.0,
            160.0,
            160.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            20.0,
            30.0,
            0.0,
            None,
        );

        // Interior box is comfortably inside the projected silhouette, so every
        // pixel there must be covered.
        let data = pixmap.data();
        let w = pixmap.width() as usize;
        let mut holes = Vec::new();
        for y in 80..120 {
            for x in 80..120 {
                let idx = (y * w + x) * 4;
                if data[idx + 3] == 0 {
                    holes.push((x, y));
                }
            }
        }
        assert!(
            holes.is_empty(),
            "background holes inside the sphere body: {:?}",
            &holes[..holes.len().min(8)]
        );
    }

    #[test]
    fn test_mesh3d_wireframe_renders() {
        let mut pixmap = Pixmap::new(200, 200).unwrap();
        let mesh = Mesh::cube();

        render_mesh3d_to_pixmap(
            &mut pixmap.as_mut(),
            &mesh,
            50.0,
            50.0,
            100.0,
            100.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            true, // wireframe
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
            None,
        );

        let data = pixmap.data();
        let mut non_zero = 0;
        for chunk in data.chunks_exact(4) {
            if chunk[3] > 0 {
                non_zero += 1;
            }
        }
        assert!(non_zero > 10, "Wireframe should render some pixels");
    }

    #[test]
    fn test_mesh3d_different_primitives_render() {
        for prim_name in ["cube", "sphere", "pyramid", "torus"] {
            let html = format!(
                r##"
            <scene width="200" height="200" fps="30" duration="1.0" background="#000000">
                <mesh3d x="50" y="50" width="100" height="100" primitive="{}" color="#38BDF8" />
            </scene>
            "##,
                prim_name
            );

            let scene = parse_html_scene(&html).unwrap();
            let assets = AssetStore {
                fonts: vec![],
                images: HashMap::new(),
                videos: HashMap::new(),
                svgs: HashMap::new(),
        lotties: HashMap::new(),
                audio_paths: vec![],
                #[cfg(feature = "exp-3d")]
                mesh3d: HashMap::new(),
            };

            let frame = crate::renderer::render_frame_at_time(&scene, &assets, 0.0).unwrap();

            let data = &frame;
            let mut non_zero = 0;
            for chunk in data.chunks_exact(4) {
                if chunk[3] > 0 {
                    non_zero += 1;
                }
            }
            assert!(
                non_zero > 0,
                "Primitive '{}' should render pixels",
                prim_name
            );
        }
    }

    #[test]
    fn test_mesh3d_self_closing_siblings_all_render() {
        // Regression: `<mesh3d ... />` must behave like any other self-closing tag,
        // so following sibling layers are not swallowed as its children.
        let html = r##"
        <scene width="640" height="240" fps="30" duration="1.0" background="#000000">
            <mesh3d x="20" y="20" width="200" height="200" primitive="cube" color="#FF0000" />
            <mesh3d x="220" y="20" width="200" height="200" primitive="sphere" color="#00FF00" />
            <mesh3d x="420" y="20" width="200" height="200" primitive="torus" color="#0000FF" />
        </scene>
        "##;
        let scene = parse_html_scene(html).unwrap();
        assert_eq!(
            scene.layers.len(),
            3,
            "all three sibling mesh3d layers must survive parsing"
        );
    }

    #[test]
    fn test_mesh3d_rotated_geometry_is_not_dropped() {
        // Regression: rotations at/after 90 degrees used to trip the generic
        // backface check for image-like layers and skip the whole mesh.
        for angle in [0.0, 90.0, 180.0, 270.0] {
            let html = format!(
                r##"
                <scene width="200" height="200" fps="30" duration="1.0" background="#000000">
                    <mesh3d x="20" y="20" width="160" height="160" primitive="cube" color="#38BDF8" rotate-y="{}" />
                </scene>
                "##,
                angle
            );
            let scene = parse_html_scene(&html).unwrap();
            let assets = AssetStore {
                fonts: vec![],
                images: HashMap::new(),
                videos: HashMap::new(),
                svgs: HashMap::new(),
        lotties: HashMap::new(),
                audio_paths: vec![],
                #[cfg(feature = "exp-3d")]
                mesh3d: HashMap::new(),
            };
            let frame = crate::renderer::render_frame_at_time(&scene, &assets, 0.0).unwrap();
            let non_zero = frame.chunks_exact(4).filter(|c| c[3] > 0).count();
            assert!(
                non_zero > 0,
                "mesh3d rotated by {} degrees must still render pixels, got {}",
                angle,
                non_zero
            );
        }
    }
}

/// Minimal in-memory GLB: a unit quad with positions, UVs and indices, and
/// deliberately no NORMAL attribute so the loader has to synthesize normals.
#[cfg(feature = "exp-3d")]
fn build_glb_quad() -> Vec<u8> {
    // Buffer layout: 4 positions (48 B), 4 UVs (32 B), 6 u16 indices (12 B).
    let mut bin: Vec<u8> = Vec::with_capacity(92);
    for p in [
        [-1.0f32, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ] {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    for uv in [[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]] {
        for c in uv {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    for idx in [0u16, 1, 2, 2, 3, 0] {
        bin.extend_from_slice(&idx.to_le_bytes());
    }

    let json = serde_json::json!({
        "asset": { "version": "2.0", "generator": "rustren-test" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0 }],
        "meshes": [{
            "primitives": [{
                "attributes": { "POSITION": 0, "TEXCOORD_0": 1 },
                "indices": 2,
                "mode": 4
            }]
        }],
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": 48, "target": 34962 },
            { "buffer": 0, "byteOffset": 48, "byteLength": 32, "target": 34962 },
            { "buffer": 0, "byteOffset": 80, "byteLength": 12, "target": 34963 }
        ],
        "accessors": [
            {
                "bufferView": 0, "componentType": 5126, "count": 4,
                "type": "VEC3", "min": [-1.0, -1.0, 0.0], "max": [1.0, 1.0, 0.0]
            },
            {
                "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC2"
            },
            {
                "bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR"
            }
        ]
    });
    let json_bytes = serde_json::to_vec(&json).unwrap();

    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());

    let total_len = 12 + 8 + json_bytes.len() + 8 + bin.len();
    out.extend_from_slice(&(total_len as u32).to_le_bytes());

    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // "JSON"
    out.extend_from_slice(&json_bytes);

    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // "BIN\0"
    out.extend_from_slice(&bin);
    out
}

#[cfg(feature = "exp-3d")]
fn opaque_count(data: &[u8]) -> usize {
    data.as_chunks::<4>().0.iter().filter(|c| c[3] > 0).count()
}

#[cfg(feature = "exp-3d")]
mod exp_3d_phase2_tests {
    use super::*;
    use crate::compilers::html::parse_html_scene;
    use crate::renderer::mesh3d::{Mesh, MeshTexture, render_mesh3d_to_pixmap};
    use tiny_skia::Pixmap;

    #[test]
    fn test_gltf_loader_parses_in_memory_glb() {
        let glb = build_glb_quad();
        let mesh = Mesh::from_gltf_bytes(&glb).expect("in-memory GLB must parse");
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.indices.len(), 6);
        // Normals are synthesized because the file has no NORMAL attribute.
        for v in &mesh.vertices {
            assert!(
                (v.normal.length() - 1.0).abs() < 1e-4,
                "synthesized normal must be unit length, got {:?}",
                v.normal
            );
        }
        // UVs come straight from TEXCOORD_0.
        assert!((mesh.vertices[1].uv[0] - 1.0).abs() < 1e-5);
        // The loader normalizes into the unit cube.
        for v in &mesh.vertices {
            for axis in 0..3 {
                assert!(
                    v.position[axis].abs() <= 0.5 + 1e-5,
                    "normalized position out of unit cube: {:?}",
                    v.position
                );
            }
        }
    }

    #[test]
    fn test_gltf_loader_renders_non_empty_pixels() {
        let mesh = Mesh::from_gltf_bytes(&build_glb_quad()).expect("GLB must parse");
        let mut pixmap = Pixmap::new(200, 200).unwrap();
        render_mesh3d_to_pixmap(
            &mut pixmap.as_mut(),
            &mesh,
            20.0,
            20.0,
            160.0,
            160.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
            None,
        );
        let drawn = opaque_count(pixmap.data());
        assert!(
            drawn > 1000,
            "imported GLB mesh must fill pixels, got {}",
            drawn
        );
    }

    #[test]
    fn test_gltf_loader_rejects_garbage() {
        assert!(Mesh::from_gltf_bytes(b"not a gltf file").is_err());
        assert!(Mesh::from_gltf_bytes(&[]).is_err());
    }

    /// 4x4 texture split into red and blue halves, so UV sampling produces two
    /// clearly distinct albedos across the cube face.
    fn two_tone_texture() -> MeshTexture {
        let mut pixels = Vec::with_capacity(4 * 4 * 4);
        for y in 0..4 {
            for x in 0..4 {
                if x < 2 {
                    pixels.extend_from_slice(&[220, 30, 30, 255]);
                } else {
                    pixels.extend_from_slice(&[30, 60, 220, 255]);
                }
                let _ = y;
            }
        }
        MeshTexture::from_raw(4, 4, pixels).unwrap()
    }

    #[test]
    fn test_uv_texture_sampling_differs_from_flat_color() {
        let mesh = Mesh::cube();
        let texture = two_tone_texture();

        let mut flat = Pixmap::new(200, 200).unwrap();
        render_mesh3d_to_pixmap(
            &mut flat.as_mut(),
            &mesh,
            20.0,
            20.0,
            160.0,
            160.0,
            45.0,
            "#FFFFFF",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            20.0,
            30.0,
            0.0,
            None,
        );

        let mut textured = Pixmap::new(200, 200).unwrap();
        render_mesh3d_to_pixmap(
            &mut textured.as_mut(),
            &mesh,
            20.0,
            20.0,
            160.0,
            160.0,
            45.0,
            "#FFFFFF",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            20.0,
            30.0,
            0.0,
            Some(&texture),
        );

        assert_ne!(
            flat.data(),
            textured.data(),
            "textured render must differ from the flat-colored render"
        );
        // Both texture halves must show up, otherwise UV mapping is degenerate.
        let mut reddish = 0;
        let mut bluish = 0;
        for chunk in textured.data().as_chunks::<4>().0 {
            if chunk[3] == 0 {
                continue;
            }
            if chunk[0] > chunk[2] + 30 {
                reddish += 1;
            }
            if chunk[2] > chunk[0] + 30 {
                bluish += 1;
            }
        }
        assert!(
            reddish > 100 && bluish > 100,
            "both texture halves must be sampled, got red={reddish} blue={bluish}"
        );
    }

    #[test]
    fn test_textured_render_is_deterministic() {
        let mesh = Mesh::cube();
        let texture = two_tone_texture();

        let render = |pixmap: &mut Pixmap| {
            render_mesh3d_to_pixmap(
                &mut pixmap.as_mut(),
                &mesh,
                20.0,
                20.0,
                160.0,
                160.0,
                45.0,
                "#FFFFFF",
                [0.5, -1.0, 0.5],
                false,
                1.0,
                1.0,
                30.0,
                25.0,
                10.0,
                Some(&texture),
            );
        };

        let mut a = Pixmap::new(200, 200).unwrap();
        let mut b = Pixmap::new(200, 200).unwrap();
        render(&mut a);
        render(&mut b);
        assert_eq!(
            a.data(),
            b.data(),
            "textured render must be byte-for-byte deterministic"
        );
    }

    #[test]
    fn test_specular_and_rim_do_not_darken_the_base_color() {
        // Regression guard for the shading upgrade: the brighter Blinn-Phong
        // pipeline must still keep a lit face at least as bright as the old
        // ambient floor, and never produce pure black on a lit silhouette.
        let mesh = Mesh::sphere(16, 32);
        let mut pixmap = Pixmap::new(200, 200).unwrap();
        render_mesh3d_to_pixmap(
            &mut pixmap.as_mut(),
            &mesh,
            20.0,
            20.0,
            160.0,
            160.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
            None,
        );
        let data = pixmap.data();
        let mut max_blue = 0u8;
        let mut black = 0usize;
        for chunk in data.as_chunks::<4>().0 {
            if chunk[3] == 0 {
                continue;
            }
            max_blue = max_blue.max(chunk[2]);
            if chunk[0] == 0 && chunk[1] == 0 && chunk[2] == 0 {
                black += 1;
            }
        }
        assert!(
            max_blue > 200,
            "lit pixel must reach near base blue, got {max_blue}"
        );
        assert_eq!(black, 0, "lit shading must not paint pure black pixels");
    }

    #[test]
    fn test_mesh3d_src_and_texture_through_asset_store() {
        // End-to-end wiring: `src`/`texture` on a real Mesh3d layer must load
        // through AssetStore and reach the pixmap, and repeat identically.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let model = root.join("assets/mesh_cube.glb");
        let texture = root.join("assets/mesh_checker.png");
        assert!(
            model.exists(),
            "showcase GLB must exist: {}",
            model.display()
        );
        assert!(
            texture.exists(),
            "showcase texture must exist: {}",
            texture.display()
        );

        // Absolute paths keep the test independent of the process cwd, which
        // other parallel tests also read.
        let model_src = model.display().to_string().replace('\\', "/");
        let texture_src = texture.display().to_string().replace('\\', "/");
        let html = format!(
            r##"<scene width="320" height="320" fps="30" duration="1.0" background="#070B14">
                <mesh3d x="40" y="40" width="240" height="240" src="{model_src}"
                        texture="{texture_src}" color="#FFFFFF" rotate-x="20" rotate-y="30" />
            </scene>"##
        );
        let scene = parse_html_scene(&html).unwrap();

        let assets = AssetStore::load_for_scene(&scene, &root.join(".rustren_cache"))
            .expect("AssetStore must preload the GLB model");
        assert!(
            assets.mesh3d.contains_key(&model_src),
            "mesh3d cache must be populated by load_for_scene"
        );

        let frame_a = crate::renderer::render_frame_at_time(&scene, &assets, 0.0).unwrap();
        let frame_b = crate::renderer::render_frame_at_time(&scene, &assets, 0.0).unwrap();
        assert_eq!(
            frame_a, frame_b,
            "GLB pipeline render must be deterministic"
        );

        // Both checker colors must be present, proving UV texture mapping ran.
        let mut warm = 0usize;
        let mut cool = 0usize;
        for chunk in frame_a.as_chunks::<4>().0 {
            if chunk[3] == 0 {
                continue;
            }
            if chunk[0] > chunk[2] + 25 {
                warm += 1;
            }
            if chunk[2] > chunk[0] + 25 {
                cool += 1;
            }
        }
        assert!(
            warm > 500 && cool > 500,
            "checker texture must show both hues, got warm={warm} cool={cool}"
        );
    }
}

#[test]
fn test_transparent_png_does_not_bleed_white_onto_background() {
    // PNG encoders routinely leave fully transparent pixels as RGB=255.
    // tiny-skia pixmaps are premultiplied, so a `A=0, RGB=255` pixel composites
    // as "white on top of everything" and paints a halo around the logo. The
    // loader must premultiply those pixels before they are resized/composited.
    let dir = std::env::temp_dir().join("rustren_premultiplied_png");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("transparent_white.png");

    // 16x16 fully transparent white with an opaque white 8x8 core.
    let mut img = image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 255, 255, 0]));
    for y in 4..12 {
        for x in 4..12 {
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    img.save(&path).unwrap();
    let img_path = path.to_string_lossy().replace('\\', "/");

    let json = format!(
        r##"{{
        "canvas": {{ "width": 16, "height": 16, "fps": 30, "duration_secs": 1.0, "background": "#000000" }},
        "layers": [
            {{ "type": "image", "x": 4, "y": 4, "width": 8, "height": 8, "src": "{img_path}" }}
        ]
    }}"##
    );
    let scene: Scene = serde_json::from_str(&json).unwrap();
    let assets = AssetStore::load_for_scene_with_base(&scene, &dir.join("cache"), None).unwrap();

    let px = |frame: &[u8], x: u32, y: u32| -> [u8; 4] {
        let i = ((y * scene.canvas.width + x) * 4) as usize;
        [frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]
    };

    // The loader must have zeroed the `A=0, RGB=255` source pixels.
    let loaded = assets.images.values().next().expect("image asset loaded");
    assert_eq!(
        loaded.get_pixel(0, 0).0,
        [0, 0, 0, 0],
        "transparent-white pixel was not premultiplied at load"
    );
    assert_eq!(loaded.get_pixel(8, 8).0, [255, 255, 255, 255]);

    let frame = render_frame(&scene, &assets, 0).unwrap();
    // Outside the layer: untouched background.
    assert_eq!(px(&frame, 0, 0), [0, 0, 0, 255], "corner must stay black");
    assert_eq!(px(&frame, 15, 15), [0, 0, 0, 255], "corner must stay black");
    // Inside the layer, but on a fully transparent source pixel (resized from
    // the image's transparent-white border). No white bleed allowed.
    assert_eq!(
        px(&frame, 4, 4),
        [0, 0, 0, 255],
        "transparent source pixel painted white: {:?}",
        px(&frame, 4, 4)
    );
    assert_eq!(
        px(&frame, 11, 11),
        [0, 0, 0, 255],
        "transparent source pixel painted white: {:?}",
        px(&frame, 11, 11)
    );
    // The opaque white core still renders white.
    assert_eq!(
        px(&frame, 8, 8),
        [255, 255, 255, 255],
        "opaque core must stay white: {:?}",
        px(&frame, 8, 8)
    );

    // Same asset at native size (no resize) must behave identically.
    let json_native = format!(
        r##"{{
        "canvas": {{ "width": 16, "height": 16, "fps": 30, "duration_secs": 1.0, "background": "#000000" }},
        "layers": [
            {{ "type": "image", "x": 0, "y": 0, "width": 16, "height": 16, "src": "{img_path}" }}
        ]
    }}"##
    );
    let scene_native: Scene = serde_json::from_str(&json_native).unwrap();
    let frame_native = render_frame(&scene_native, &assets, 0).unwrap();
    assert_eq!(
        px(&frame_native, 0, 0),
        [0, 0, 0, 255],
        "native-size transparent pixel painted white: {:?}",
        px(&frame_native, 0, 0)
    );
    assert_eq!(
        px(&frame_native, 7, 7),
        [255, 255, 255, 255],
        "opaque core must stay white"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

fn test_assets() -> AssetStore {
    AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        lotties: HashMap::new(),
        audio_paths: vec![],
        #[cfg(feature = "exp-3d")]
        mesh3d: HashMap::new(),
    }
}

fn test_pixel(frame: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * width + x) * 4) as usize;
    [frame[idx], frame[idx + 1], frame[idx + 2], frame[idx + 3]]
}

#[test]
fn test_layer_scale_x_and_scale_y_render_independently() {
    let json = r##"{
        "canvas": { "width": 100, "height": 100, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "rect",
                "x": 40, "y": 40, "width": 20, "height": 20,
                "color": "#FF0000",
                "scale_x": 2.0,
                "scale_y": 0.5
            }
        ]
    }"##;
    let scene: Scene = serde_json::from_str(json).unwrap();
    let frame = render_frame(&scene, &test_assets(), 0).unwrap();

    // Scaling happens about the default center (50, 50): 40x10 box (30..70, 45..55).
    assert_eq!(
        test_pixel(&frame, 100, 35, 50),
        [255, 0, 0, 255],
        "horizontal scale must widen the rect"
    );
    assert_eq!(
        test_pixel(&frame, 100, 50, 47),
        [255, 0, 0, 255],
        "vertical scale must keep the squashed rect visible near the center"
    );
    assert_eq!(
        test_pixel(&frame, 100, 35, 44),
        [0, 0, 0, 255],
        "scale_y must not be applied to the horizontal axis"
    );
    assert_eq!(
        test_pixel(&frame, 100, 71, 50),
        [0, 0, 0, 255],
        "scaled bounds must stop at x = 70"
    );
}

#[test]
fn test_layer_custom_pivot_anchors_scaling() {
    let json = r##"{
        "canvas": { "width": 100, "height": 100, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "rect",
                "x": 40, "y": 40, "width": 20, "height": 20,
                "color": "#00FF00",
                "scale": 2.0,
                "pivot_x": 0.0,
                "pivot_y": 0.0
            }
        ]
    }"##;
    let scene: Scene = serde_json::from_str(json).unwrap();
    let frame = render_frame(&scene, &test_assets(), 0).unwrap();

    // Pivot (0, 0) keeps the top-left at (40, 40), so the rect covers 40..80.
    assert_eq!(test_pixel(&frame, 100, 75, 75), [0, 255, 0, 255]);
    assert_eq!(
        test_pixel(&frame, 100, 35, 35),
        [0, 0, 0, 255],
        "with a top-left pivot the rect must not grow up or left"
    );
}

#[test]
fn test_center_anchor_and_text_align_without_max_width() {
    let json = r##"{
        "canvas": { "width": 400, "height": 120, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
        "layers": [
            {
                "type": "text",
                "x": 200, "y": 60,
                "content": "MMMM",
                "font_size": 40,
                "anchor": "center",
                "text_align": "center",
                "color": "#FFFFFF"
            }
        ]
    }"##;
    let scene: Scene = serde_json::from_str(json).unwrap();
    let frame = render_frame(&scene, &test_assets(), 0).unwrap();

    let mut min_x = u32::MAX;
    let mut max_x = 0u32;
    for y in 0..120u32 {
        for x in 0..400u32 {
            let px = test_pixel(&frame, 400, x, y);
            if px[0] > 200 && px[1] > 200 && px[2] > 200 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    assert!(max_x > min_x, "text must render visible glyphs");
    let center = (min_x + max_x) as f32 * 0.5;
    assert!(
        (center - 200.0).abs() <= 4.0,
        "centered text must be centered on x=200 without max_width, got center={center} (bounds {min_x}..{max_x})"
    );
}

#[test]
fn test_multiline_text_line_height_scales_with_newlines() {
    let render_height = |line_height: f32| -> u32 {
        let json = format!(
            r##"{{
            "canvas": {{ "width": 200, "height": 300, "fps": 30, "duration_secs": 1.0, "background": "#000000" }},
            "layers": [
                {{
                    "type": "text",
                    "x": 20, "y": 20,
                    "content": "HH\nHH",
                    "font_size": 10,
                    "line_height": {line_height},
                    "color": "#FFFFFF"
                }}
            ]
        }}"##
        );
        let scene: Scene = serde_json::from_str(&json).unwrap();
        let frame = render_frame(&scene, &test_assets(), 0).unwrap();
        let mut min_y = u32::MAX;
        let mut max_y = 0u32;
        for y in 0..300u32 {
            for x in 0..200u32 {
                let px = test_pixel(&frame, 200, x, y);
                if px[0] > 200 && px[1] > 200 && px[2] > 200 {
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        assert!(max_y > min_y, "multiline text must render glyphs");
        max_y - min_y
    };

    let tight = render_height(20.0);
    let loose = render_height(80.0);
    assert!(
        loose as f32 >= tight as f32 * 1.8,
        "line_height must space explicit \\n lines: tight={tight}px loose={loose}px"
    );
}

fn video_scene(duration: f32, rate: f32, luma_key: Option<f32>) -> Scene {
    let luma = luma_key
        .map(|k| format!(r#", "luma_key": {k}"#))
        .unwrap_or_default();
    let json = format!(
        r##"{{
        "canvas": {{ "width": 16, "height": 16, "fps": 30, "duration_secs": {duration}, "background": "#00FF00" }},
        "layers": [
            {{
                "type": "video",
                "x": 0, "y": 0, "width": 16, "height": 16,
                "src": "test.mp4",
                "fit": "fill",
                "playback_rate": {rate}{luma}
            }}
        ]
    }}"##
    );
    serde_json::from_str(&json).unwrap()
}

fn solid_frame(r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity(16 * 16 * 4);
    for _ in 0..16 * 16 {
        data.extend_from_slice(&[r, g, b, 255]);
    }
    data
}

fn video_assets(frames: Vec<Vec<u8>>) -> AssetStore {
    let mut assets = test_assets();
    assets.videos.insert(
        "test.mp4".to_string(),
        crate::assets::VideoAsset::from_frames(
            16,
            16,
            1.0,
            false,
            frames,
            std::path::PathBuf::from("test.mp4"),
        ),
    );
    assets
}

#[test]
fn test_video_reverse_playback_rate_steps_backwards() {
    let assets = video_assets(vec![
        solid_frame(255, 0, 0),
        solid_frame(0, 255, 0),
        solid_frame(0, 0, 255),
    ]);

    // fps=1, 3 frames => 3s clip. Reverse starts at the last frame.
    let backward = video_scene(4.0, -1.0, None);
    let start = render_frame_at_time(&backward, &assets, 0.0).unwrap();
    assert_eq!(
        test_pixel(&start, 16, 8, 8),
        [0, 0, 255, 255],
        "negative playback_rate must start from the end of the clip"
    );
    let mid = render_frame_at_time(&backward, &assets, 2.0).unwrap();
    assert_eq!(
        test_pixel(&mid, 16, 8, 8),
        [0, 255, 0, 255],
        "reverse playback must step toward earlier frames"
    );

    let forward = video_scene(4.0, 1.0, None);
    let fwd = render_frame_at_time(&forward, &assets, 2.0).unwrap();
    assert_eq!(
        test_pixel(&fwd, 16, 8, 8),
        [0, 0, 255, 255],
        "positive playback_rate must play forward"
    );
}

#[test]
fn test_video_luma_key_makes_black_pixels_transparent() {
    let mut frame = Vec::with_capacity(16 * 16 * 4);
    for _y in 0..16 {
        for x in 0..16 {
            let c = if x < 8 { [0, 0, 0, 255] } else { [255, 255, 255, 255] };
            frame.extend_from_slice(&c);
        }
    }
    let assets = video_assets(vec![frame]);

    let keyed = video_scene(1.0, 1.0, Some(0.1));
    let out = render_frame_at_time(&keyed, &assets, 0.0).unwrap();
    assert_eq!(
        test_pixel(&out, 16, 4, 8),
        [0, 255, 0, 255],
        "black video pixels must be luma-keyed to transparent (background shows through)"
    );
    assert_eq!(
        test_pixel(&out, 16, 12, 8),
        [255, 255, 255, 255],
        "bright video pixels must stay opaque"
    );
}

#[test]
fn test_text_animator_staggers_chars_and_words() {
    let render = |animator: &str, time: f32| -> usize {
        let json = format!(
            r##"{{
            "canvas": {{ "width": 360, "height": 120, "fps": 30, "duration_secs": 2.5, "background": "#000000" }},
            "layers": [
                {{
                    "type": "text",
                    "x": 20, "y": 20,
                    "content": "AB CD",
                    "font_size": 48,
                    "color": "#FFFFFF"{animator}
                }}
            ]
        }}"##
        );
        let scene: Scene = serde_json::from_str(&json).unwrap();
        let frame = render_frame_at_time(&scene, &test_assets(), time).unwrap();
        let mut lit = 0usize;
        for y in 0..120u32 {
            for x in 0..360u32 {
                let p = test_pixel(&frame, 360, x, y);
                if p[0] > 180 && p[1] > 180 && p[2] > 180 {
                    lit += 1;
                }
            }
        }
        lit
    };

    let base = render("", 0.4);
    assert!(base > 0, "text must render visible glyphs");

    let word = r#", "text_animator": { "mode": "word", "start_sec": 0.0, "stagger_secs": 0.5, "duration_secs": 0.4, "opacity_from": 0.0, "easing": "linear" }"#;
    let partial = render(word, 0.4);
    assert!(partial > 0, "first word must be visible once its stagger completes");
    assert!(
        partial < base,
        "with word stagger the second word must still be hidden (partial={partial} full={base})"
    );
    let settled = render(word, 2.0);
    assert_eq!(
        settled, base,
        "after the stagger settles the text must match the unanimated render"
    );

    let ch = r#", "text_animator": { "mode": "char", "start_sec": 0.0, "stagger_secs": 0.1, "duration_secs": 0.1, "opacity_from": 0.0, "easing": "linear" }"#;
    let few = render(ch, 0.15);
    assert!(few > 0, "earliest characters must be visible");
    assert!(
        few < base,
        "char stagger must reveal glyphs progressively (few={few} full={base})"
    );
}

#[test]
fn test_container_clip_feather_softens_mask_edge() {
    let render = |feather: f32| -> Vec<u8> {
        let json = format!(
            r##"{{
            "canvas": {{ "width": 100, "height": 100, "fps": 30, "duration_secs": 1.0, "background": "#000000" }},
            "layers": [
                {{
                    "type": "container",
                    "x": 40, "y": 40, "width": 40, "height": 40,
                    "clip": true,
                    "clip_feather": {feather},
                    "children": [
                        {{ "type": "rect", "x": 0, "y": 0, "width": 40, "height": 40, "color": "#FF0000" }}
                    ]
                }}
            ]
        }}"##
        );
        let scene: Scene = serde_json::from_str(&json).unwrap();
        render_frame(&scene, &test_assets(), 0).unwrap()
    };

    let hard = render(0.0);
    let soft = render(8.0);
    assert_eq!(
        test_pixel(&hard, 100, 60, 60),
        [255, 0, 0, 255],
        "hard clip center must stay opaque"
    );
    assert_eq!(
        test_pixel(&soft, 100, 60, 60),
        [255, 0, 0, 255],
        "feather must not dim the container center"
    );
    let hard_edge = test_pixel(&hard, 100, 40, 60)[0];
    let soft_edge = test_pixel(&soft, 100, 40, 60)[0];
    assert_eq!(hard_edge, 255, "hard clip edge must stay fully opaque");
    assert!(
        soft_edge < 220,
        "feather must ramp the mask at the container edge (hard={hard_edge} soft={soft_edge})"
    );
    assert_eq!(
        test_pixel(&soft, 100, 35, 60),
        [0, 0, 0, 255],
        "feather must not spill outside the container"
    );
}

fn red_right_edge(frame: &[u8], width: u32, y: u32, total_w: u32) -> u32 {
    let mut last = 0u32;
    for x in 0..total_w {
        let p = test_pixel(frame, width, x, y);
        if p[0] > 180 && p[2] < 80 {
            last = x;
        }
    }
    last
}

#[test]
fn test_wave_warp_displaces_pixels_deterministically() {
    let render = |wave: bool| -> Vec<u8> {
        let wave = if wave {
            r#", "wave_warp": { "amplitude": 10.0, "wavelength": 20.0, "speed": 0.0, "phase": 0.0, "axis": "x" }"#
        } else {
            ""
        };
        let json = format!(
            r##"{{
            "canvas": {{ "width": 100, "height": 100, "fps": 30, "duration_secs": 1.0, "background": "#0000FF" }},
            "layers": [
                {{ "type": "rect", "x": 0, "y": 0, "width": 50, "height": 100, "color": "#FF0000"{wave} }}
            ]
        }}"##
        );
        let scene: Scene = serde_json::from_str(&json).unwrap();
        render_frame(&scene, &test_assets(), 0).unwrap()
    };

    let plain = render(false);
    let warped = render(true);
    let plain_again = render(false);
    let warped_again = render(true);
    assert_eq!(plain, plain_again, "plain render must be deterministic");
    assert_eq!(warped, warped_again, "wave warp must be deterministic");

    // axis x drives displacement from sin(y): right at y=5, left at y=15.
    let base_edge = red_right_edge(&plain, 100, 50, 100);
    let right_edge = red_right_edge(&warped, 100, 5, 100);
    let left_edge = red_right_edge(&warped, 100, 15, 100);
    let warped_mid = red_right_edge(&warped, 100, 50, 100);
    assert_eq!(base_edge, 49, "unwarped half-rect right edge must be at x=49");
    assert!(
        right_edge > base_edge + 4,
        "positive wave phase must stretch the boundary right (base={base_edge} warped={right_edge})"
    );
    assert!(
        left_edge + 4 < warped_mid,
        "negative wave phase must pull the boundary left (warped={left_edge} mid={warped_mid})"
    );
}

#[test]
fn test_video_windowed_decode_is_bounded_and_deterministic() {
    let dir = std::env::temp_dir().join(format!("rustren_windowed_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let video_path = dir.join("windowed.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=32x32:d=7.5:r=10",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=32x32:d=7.5:r=10",
            "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1:a=0",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&video_path)
        .status()
        .expect("ffmpeg must be available for the windowed decode test");
    assert!(status.success(), "ffmpeg failed to generate the windowed test clip");

    let json = format!(
        r##"{{
        "canvas": {{ "width": 32, "height": 32, "fps": 10, "duration_secs": 15.0, "background": "#000000" }},
        "layers": [
            {{ "type": "video", "x": 0, "y": 0, "width": 32, "height": 32, "src": "{}" }}
        ]
    }}"##,
        video_path.display()
    );
    let scene: Scene = serde_json::from_str(&json).unwrap();
    let assets = AssetStore::load_for_scene(&scene, &dir).unwrap();
    let asset = assets
        .videos
        .get(&video_path.display().to_string())
        .expect("video asset must be loaded");

    assert_eq!(asset.frame_count, 150, "clip must report 150 decoded frames");
    assert!(
        asset.cache_capacity <= 96,
        "window must stay bounded (capacity={})",
        asset.cache_capacity
    );
    assert_eq!(
        asset.cached_frame_count(),
        0,
        "loading must not decode the whole clip into RAM"
    );

    let center = |idx: usize| -> [u8; 4] {
        let f = asset.frame(idx).unwrap();
        let off = (16 * 32 + 16) * 4;
        [f[off], f[off + 1], f[off + 2], f[off + 3]]
    };

    let first = center(0);
    assert!(first[0] > 200 && first[2] < 60, "frame 0 must be red: {first:?}");
    let last = center(149);
    assert!(last[2] > 200 && last[0] < 60, "frame 149 must be blue: {last:?}");

    for idx in 0..150usize {
        let _ = asset.frame(idx).unwrap();
        assert!(
            asset.cached_frame_count() <= asset.cache_capacity,
            "cache exceeded its window at frame {idx}: {} > {}",
            asset.cached_frame_count(),
            asset.cache_capacity
        );
        assert!(
            asset.cached_frame_count() <= 96,
            "no more than 96 frames may stay resident"
        );
    }
    assert!(
        asset.decode_count() > 1,
        "a 150-frame clip must be decoded in more than one window (got {})",
        asset.decode_count()
    );

    let stored_149 = asset.frame(149).unwrap();
    let _ = center(0);
    let again_149 = asset.frame(149).unwrap();
    assert_eq!(
        stored_149.as_slice(),
        again_149.as_slice(),
        "windowed decoding must return identical bytes after the cache window moved"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
