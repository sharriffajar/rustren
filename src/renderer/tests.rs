use super::*;
use crate::schema::Scene;
use std::collections::HashMap;

#[test]
fn test_parse_hex_color() {
    let c = parse_hex_color("#FF0000", 0.5).unwrap();
    assert!((c.red() - 1.0).abs() < 1e-5);
    assert!((c.alpha() - 0.5).abs() < 1e-5);
    assert!(parse_hex_color("#123", 1.0).is_err());
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
        audio_paths: vec![],
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
        audio_paths: vec![],
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
        audio_paths: vec![],
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
            .any(|d| d.kind == LayoutDiagnosticKind::UnexpectedTextWrap && d.path == "layers[3].children[0]"),
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
        audio_paths: vec![],
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
    assert_eq!(culled_frame[c_idx], 0, "Backface should be culled when backface-visible=false");

    let visible_html = r##"
    <scene width="100" height="100" fps="30" duration="1.0" background="#000000">
        <rect x="20" y="20" width="60" height="60" color="#FF0000" rotate-y="180" backface-visible="true" />
    </scene>
    "##;
    let visible_scene = crate::compilers::html::parse_html_scene(visible_html).unwrap();
    let visible_frame = render_frame_at_time(&visible_scene, &empty_assets, 0.0).unwrap();
    assert_eq!(visible_frame[c_idx], 255, "Backface should render when backface-visible=true");
}

#[test]
fn test_z_index_depth_sorting_root_and_container() {
    let empty_assets = AssetStore {
        fonts: vec![],
        images: HashMap::new(),
        videos: HashMap::new(),
        svgs: HashMap::new(),
        audio_paths: vec![],
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
        audio_paths: vec![],
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
    assert_eq!(f0_a, f0_b, "Surface effects + particles must be byte-for-byte deterministic");

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
        audio_paths: vec![],
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
    use crate::renderer::mesh3d::{Mesh, render_mesh3d_to_pixmap};
    use crate::compilers::html::parse_html_scene;
    use crate::assets::AssetStore;
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
            50.0, 50.0,
            100.0, 100.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            0.0, 0.0, 0.0,
        );

        // Check that some pixels were drawn (non-transparent)
        let data = pixmap.data();
        let mut non_zero = 0;
        for chunk in data.chunks_exact(4) {
            if chunk[3] > 0 {
                non_zero += 1;
            }
        }
        assert!(non_zero > 100, "Expected many non-transparent pixels, got {}", non_zero);
    }

    #[test]
    fn test_mesh3d_deterministic_render() {
        let mesh = Mesh::cube();

        let mut pixmap1 = Pixmap::new(200, 200).unwrap();
        render_mesh3d_to_pixmap(
            &mut pixmap1.as_mut(),
            &mesh,
            50.0, 50.0,
            100.0, 100.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            45.0, 30.0, 15.0,
        );

        let mut pixmap2 = Pixmap::new(200, 200).unwrap();
        render_mesh3d_to_pixmap(
            &mut pixmap2.as_mut(),
            &mesh,
            50.0, 50.0,
            100.0, 100.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            45.0, 30.0, 15.0,
        );

        assert_eq!(pixmap1.data(), pixmap2.data(), "Two renders with same params must be byte-for-byte identical");
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
            audio_paths: vec![],
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
            20.0, 20.0,
            160.0, 160.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            20.0, 30.0, 0.0,
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
            20.0, 20.0,
            160.0, 160.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            false,
            1.0,
            1.0,
            20.0, 30.0, 0.0,
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
            50.0, 50.0,
            100.0, 100.0,
            45.0,
            "#38BDF8",
            [0.5, -1.0, 0.5],
            true, // wireframe
            1.0,
            1.0,
            0.0, 0.0, 0.0,
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
            let html = format!(r##"
            <scene width="200" height="200" fps="30" duration="1.0" background="#000000">
                <mesh3d x="50" y="50" width="100" height="100" primitive="{}" color="#38BDF8" />
            </scene>
            "##, prim_name);

            let scene = parse_html_scene(&html).unwrap();
            let assets = AssetStore {
                fonts: vec![],
                images: HashMap::new(),
                videos: HashMap::new(),
                svgs: HashMap::new(),
                audio_paths: vec![],
            };

            let frame = crate::renderer::render_frame_at_time(&scene, &assets, 0.0).unwrap();

            let data = &frame;
            let mut non_zero = 0;
            for chunk in data.chunks_exact(4) {
                if chunk[3] > 0 {
                    non_zero += 1;
                }
            }
            assert!(non_zero > 0, "Primitive '{}' should render pixels", prim_name);
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
        assert_eq!(scene.layers.len(), 3, "all three sibling mesh3d layers must survive parsing");
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
                audio_paths: vec![],
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
