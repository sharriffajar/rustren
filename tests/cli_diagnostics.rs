//! End-to-end CLI diagnostics tests.
//!
//! These lock the *shape* of agent-facing output: exit codes, stream split
//! (warnings on stderr, payloads on stdout) and the `line`/`column` hints.
//! Diagnostics wording is user-visible contract here, so a change should
//! either update this test or be recognised as a breakage.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_rustren")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run rustren {args:?}: {e}"))
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn exit_code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

#[test]
fn warnings_go_to_stderr_and_exit_zero() {
    let out = run(&["validate", fixture("html_bad_attr.html").to_str().unwrap()]);
    assert_eq!(exit_code(&out), 0, "warnings must not fail validate");
    let err = stderr(&out);
    assert!(
        err.contains("warning: line 2, column 12: numeric attribute x=\"100px\""),
        "{err}"
    );
    // The success summary belongs on stdout so stdout stays parseable.
    assert!(
        stdout(&out).contains("rustren: valid HTML"),
        "{}",
        stdout(&out)
    );
    assert!(!stdout(&out).contains("warning:"), "{}", stdout(&out));
}

#[test]
fn unknown_field_error_exits_two() {
    let out = run(&[
        "validate",
        fixture("json_unknown_field.json").to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&out), 2);
    assert!(
        stderr(&out).contains("Unknown field 'layers[0].bogus' on type 'rect'"),
        "{}",
        stderr(&out)
    );
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
}

#[test]
fn json_type_error_reports_field_path_and_position() {
    let out = run(&["validate", fixture("json_bad_type.json").to_str().unwrap()]);
    assert_eq!(exit_code(&out), 2);
    let err = stderr(&out);
    assert!(err.contains("`canvas.width`"), "{err}");
    assert!(err.contains("(line 2, column 24)"), "{err}");
    assert!(
        err.contains("invalid type: string \"640\", expected u32"),
        "{err}"
    );
}

#[test]
fn json_syntax_error_reports_single_position() {
    let out = run(&[
        "validate",
        fixture("json_bad_syntax.json").to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&out), 2);
    let err = stderr(&out);
    assert!(err.contains("JSON syntax error"), "{err}");
    assert!(err.contains("(line 4, column 1)"), "{err}");
    // The position must appear once, not repeated by serde's Display.
    assert_eq!(err.matches("line 4, column 1").count(), 1, "{err}");
}

#[test]
fn html_error_points_at_include_tag() {
    let out = run(&[
        "validate",
        fixture("html_bad_include.html").to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&out), 2);
    let err = stderr(&out);
    assert!(err.contains("line 2, column 3"), "{err}");
    assert!(
        err.contains("<include> is missing a `src` attribute"),
        "{err}"
    );
}

#[test]
fn json_error_format_emits_payload_without_stderr() {
    let out = run(&[
        "validate",
        fixture("json_unknown_field.json").to_str().unwrap(),
        "--error-format",
        "json",
    ]);
    assert_eq!(exit_code(&out), 2);
    assert!(stderr(&out).is_empty(), "{}", stderr(&out));
    let payload: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("stdout must be JSON");
    assert_eq!(payload["valid"], false);
    assert_eq!(payload["error"]["code"], "invalid_scene");
}

#[test]
fn json_warning_payload_exposes_fields_agents_use() {
    let out = run(&[
        "validate",
        fixture("html_bad_attr.html").to_str().unwrap(),
        "--error-format",
        "json",
    ]);
    assert_eq!(exit_code(&out), 0);
    let payload: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("stdout must be JSON");
    assert_eq!(payload["valid"], true);
    assert_eq!(payload["warning_count"], 1);
    let warning = &payload["warnings"][0];
    assert_eq!(warning["code"], "unparsed_attribute");
    assert_eq!(warning["path"], "rect[0].x");
    assert!(
        warning["message"]
            .as_str()
            .unwrap()
            .starts_with("line 2, column 12:")
    );
    assert!(warning["fix"].as_str().unwrap().contains("x=\"100\""));
    assert!(warning["affected_paths"].is_array());
}

#[test]
fn missing_asset_warns_in_validate_but_fails_render() {
    let validate = run(&[
        "validate",
        fixture("html_missing_asset.html").to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&validate), 0);
    assert!(
        stderr(&validate).contains("warning: image not found"),
        "{}",
        stderr(&validate)
    );

    let dir = std::env::temp_dir().join("rustren_cli_missing_asset");
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("out.png");
    let snapshot = run(&[
        "snapshot",
        fixture("html_missing_asset.html").to_str().unwrap(),
        "-t",
        "1",
        "-o",
        png.to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&snapshot), 1, "{}", stderr(&snapshot));
    assert!(
        stderr(&snapshot).contains("nope.png"),
        "{}",
        stderr(&snapshot)
    );

    // render now catches the missing asset before spawning ffmpeg.
    let render = run(&[
        "render",
        fixture("html_missing_asset.html").to_str().unwrap(),
        "-o",
        "/tmp/rustren_cli_missing_asset.mp4",
    ]);
    assert_eq!(exit_code(&render), 1, "{}", stderr(&render));
    assert!(stderr(&render).contains("nope.png"), "{}", stderr(&render));
    assert!(!stderr(&render).contains("ffmpeg"), "{}", stderr(&render));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fix_prints_diagnostics_to_stderr_and_scene_to_stdout() {
    let out = run(&[
        "validate",
        fixture("html_bad_attr.html").to_str().unwrap(),
        "--fix",
    ]);
    assert_eq!(exit_code(&out), 0);
    let err = stderr(&out);
    assert!(err.contains("x 100px -> 100"), "{err}");
    assert!(err.contains("color red -> #FF0000"), "{err}");
    let fixed = stdout(&out);
    assert!(fixed.contains("x=\"100\""), "{fixed}");
    assert!(fixed.contains("color=\"#FF0000\""), "{fixed}");
    // Structure and indentation survive the rewrite.
    assert!(fixed.lines().count() >= 3, "{fixed}");
}

#[test]
fn render_gates_unknown_fields_like_validate() {
    // `render` used to ignore unknown fields that `validate` rejected, so an
    // agent could get a green validate and a silently wrong video.
    let out = run(&[
        "render",
        fixture("json_unknown_field.json").to_str().unwrap(),
        "-o",
        "/tmp/rustren_cli_gate.mp4",
        "--error-format",
        "json",
    ]);
    assert_eq!(exit_code(&out), 2, "{}", stderr(&out));
    assert!(stderr(&out).is_empty(), "{}", stderr(&out));
    let payload: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("stdout must be JSON");
    assert_eq!(payload["valid"], false);
    assert_eq!(payload["error"]["code"], "invalid_scene");
    assert!(
        payload["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown field 'layers[0].bogus'")
    );
}

#[test]
fn snapshot_and_layout_gate_unknown_fields() {
    let snapshot = run(&[
        "snapshot",
        fixture("json_unknown_field.json").to_str().unwrap(),
        "-t",
        "0",
        "-o",
        "/tmp/rustren_cli_gate.png",
    ]);
    assert_eq!(exit_code(&snapshot), 2, "{}", stderr(&snapshot));
    assert!(
        stderr(&snapshot).contains("Unknown field 'layers[0].bogus'"),
        "{}",
        stderr(&snapshot)
    );

    // `layout` prints JSON on stdout and has no -o.
    let layout = run(&[
        "layout",
        fixture("json_unknown_field.json").to_str().unwrap(),
        "-t",
        "0",
    ]);
    assert_eq!(exit_code(&layout), 2, "{}", stderr(&layout));
    assert!(
        stderr(&layout).contains("Unknown field 'layers[0].bogus'"),
        "{}",
        stderr(&layout)
    );
}

#[test]
fn missing_audio_names_the_track_instead_of_broken_pipe() {
    // A missing audio file used to surface as "Failed to write RGBA frame to
    // ffmpeg stdin / Broken pipe", pointing agents at the encoder.
    let out = run(&[
        "render",
        fixture("json_missing_audio.json").to_str().unwrap(),
        "-o",
        "/tmp/rustren_cli_audio.mp4",
    ]);
    assert_eq!(exit_code(&out), 1, "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("audio file not found"), "{err}");
    assert!(err.contains("nope.wav"), "{err}");
    assert!(!err.contains("Broken pipe"), "{err}");
}

#[test]
fn shorthand_hex_renders_and_fixes() {
    let scene = std::env::temp_dir().join("rustren_cli_shorthand_hex.json");
    std::fs::write(
        &scene,
        r##"{"canvas":{"width":32,"height":32,"fps":30,"duration_secs":1,"background":"#FFF"},
        "layers":[{"type":"rect","x":0,"y":0,"width":16,"height":16,"color":"#F00"}]}"##,
    )
    .unwrap();

    // schema.json allows #RGB, so rendering it must no longer fail.
    let render = run(&[
        "render",
        scene.to_str().unwrap(),
        "-o",
        "/tmp/rustren_cli_shorthand.mp4",
    ]);
    assert_eq!(exit_code(&render), 0, "{}", stderr(&render));

    let fix = run(&["validate", scene.to_str().unwrap(), "--fix"]);
    assert_eq!(exit_code(&fix), 0);
    assert!(stdout(&fix).contains("#FFFFFF"), "{}", stdout(&fix));
    assert!(stdout(&fix).contains("#FF0000"), "{}", stdout(&fix));
    let _ = std::fs::remove_file(&scene);
}

#[test]
fn json_commands_report_missing_input_as_json() {
    for (command, extra) in [
        ("validate", vec!["/tmp/rustren_cli_nope_scene.json"]),
        (
            "render",
            vec!["/tmp/rustren_cli_nope_scene.json", "-o", "/tmp/x.mp4"],
        ),
        (
            "snapshot",
            vec!["/tmp/rustren_cli_nope_scene.json", "-t", "0"],
        ),
        (
            "layout",
            vec!["/tmp/rustren_cli_nope_scene.json", "-t", "0"],
        ),
    ] {
        let mut args = vec![command];
        args.extend(extra);
        args.extend(["--error-format", "json"]);
        let out = run(&args);
        assert_eq!(exit_code(&out), 1, "{command}: {}", stderr(&out));
        assert!(stderr(&out).is_empty(), "{command}: {}", stderr(&out));
        let payload: serde_json::Value =
            serde_json::from_str(&stdout(&out)).expect("stdout must be JSON");
        assert_eq!(payload["error"]["code"], "input_error", "{command}");
    }
}

#[test]
fn diff_json_reports_verdict_without_stderr() {
    let dir = std::env::temp_dir().join("rustren_cli_diff_json");
    std::fs::create_dir_all(&dir).unwrap();
    let red = dir.join("red.json");
    let green = dir.join("green.json");
    std::fs::write(
        &red,
        r##"{"canvas":{"width":32,"height":32,"fps":30,"duration_secs":1},"layers":[{"type":"rect","x":0,"y":0,"width":32,"height":32,"color":"#FF0000"}]}"##,
    )
    .unwrap();
    std::fs::write(
        &green,
        r##"{"canvas":{"width":32,"height":32,"fps":30,"duration_secs":1},"layers":[{"type":"rect","x":0,"y":0,"width":32,"height":32,"color":"#00FF00"}]}"##,
    )
    .unwrap();

    let same = run(&[
        "diff",
        red.to_str().unwrap(),
        red.to_str().unwrap(),
        "--error-format",
        "json",
    ]);
    assert_eq!(exit_code(&same), 0, "{}", stderr(&same));
    let payload: serde_json::Value =
        serde_json::from_str(&stdout(&same)).expect("stdout must be JSON");
    assert_eq!(payload["ok"], true);

    let different = run(&[
        "diff",
        red.to_str().unwrap(),
        green.to_str().unwrap(),
        "--error-format",
        "json",
    ]);
    // One JSON verdict document on stdout, no second error payload.
    assert_eq!(exit_code(&different), 1, "{}", stderr(&different));
    assert!(stderr(&different).is_empty(), "{}", stderr(&different));
    let payload: serde_json::Value =
        serde_json::from_str(&stdout(&different)).expect("stdout must be JSON");
    assert_eq!(payload["ok"], false);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn snake_case_html_attribute_warns_and_fixes() {
    let dir = std::env::temp_dir().join("rustren_cli_snake_case");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("snake.html");
    std::fs::write(
        &path,
        "<scene width=\"640\" height=\"360\" duration=\"2\">\n  <text x=\"10\" y=\"10\" font_size=\"32\" color=\"#FFFFFF\">Hi</text>\n</scene>\n",
    )
    .unwrap();

    let validate = run(&["validate", path.to_str().unwrap()]);
    assert_eq!(exit_code(&validate), 0, "{}", stderr(&validate));
    let err = stderr(&validate);
    assert!(
        err.contains("attribute font_size=\"32\" is ignored"),
        "{err}"
    );
    assert!(err.contains("font-size=\"32\""), "{err}");

    let fixed = run(&["validate", "--fix", path.to_str().unwrap()]);
    assert_eq!(exit_code(&fixed), 0);
    let out = stdout(&fixed);
    assert!(out.contains("font-size=\"32\""), "{out}");
    assert!(!out.contains("font_size="), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn composition_variables_resolve_at_compile_time() {
    // `data-composition-variables` used to be applied only by Studio, so the CLI
    // rendered `#{{glowColor}}` and failed with an invalid hex color.
    let validate = run(&["validate", "examples/cuboid_carousel.html"]);
    assert_eq!(exit_code(&validate), 0, "{}", stderr(&validate));
    assert!(
        !stderr(&validate).contains("glowColor"),
        "composition defaults were not substituted: {}",
        stderr(&validate)
    );

    let out = run(&[
        "snapshot",
        "examples/cuboid_carousel.html",
        "-t",
        "1.0",
        "-o",
        "/tmp/rustren_cli_carousel.png",
    ]);
    assert_eq!(exit_code(&out), 0, "{}", stderr(&out));
    assert!(!stderr(&out).contains("glowColor"), "{}", stderr(&out));
    let _ = std::fs::remove_file("/tmp/rustren_cli_carousel.png");
}

#[test]
fn validate_assets_match_render_resolution() {
    // `showreel.json` lives in examples/ but references `assets/logo.png`
    // relative to the repo root. validate --check-assets and render must agree
    // that the asset exists.
    let out = run(&["validate", "--check-assets", "examples/showreel.json"]);
    assert_eq!(exit_code(&out), 0, "{}", stderr(&out));
    assert!(
        !stderr(&out).contains("logo.png"),
        "false missing-asset warning: {}",
        stderr(&out)
    );
}

#[test]
fn mcp_tools_persist_and_sense_end_to_end() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let dir = std::env::temp_dir().join("rustren_cli_mcp");
    std::fs::create_dir_all(&dir).unwrap();
    let scene = dir.join("scene.json");
    std::fs::write(
        &scene,
        r##"{
  "canvas": { "width": 320, "height": 180, "fps": 30, "duration_secs": 1.0, "background": "#000000" },
  "layers": []
}"##,
    )
    .unwrap();

    let mut child = Command::new(binary())
        .args(["mcp", scene.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp server");

    let requests = [
        r#"{"jsonrpc":"2.0","id":0,"method":"initialize"}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#.to_string(),
        r##"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_layer","arguments":{"layer":{"type":"rect","x":10.0,"y":10.0,"width":50.0,"height":20.0,"color":"#22D3EE"}}}}"##.to_string(),
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"validate_scene","arguments":{}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"inspect_layout","arguments":{"t":0.0,"diagnose":true}}}"#.to_string(),
    ];
    {
        let stdin = child.stdin.as_mut().unwrap();
        for req in &requests {
            writeln!(stdin, "{req}").unwrap();
        }
    }
    let _ = child.stdin.take();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut responses = Vec::new();
    for line in BufReader::new(std::io::Cursor::new(output.stdout)).lines() {
        responses.push(serde_json::from_str::<serde_json::Value>(&line.unwrap()).unwrap());
    }
    assert_eq!(responses.len(), requests.len(), "{responses:#?}");

    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 11);
    assert!(responses[2].get("error").is_none(), "{:?}", responses[2]);

    // The edit must be on disk.
    let on_disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&scene).unwrap()).unwrap();
    assert_eq!(on_disk["layers"].as_array().unwrap().len(), 1);

    let validate_payload: serde_json::Value = serde_json::from_str(
        responses[3]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(validate_payload["valid"], true);
    assert_eq!(validate_payload["root_layers"], 1);

    let layout_payload: serde_json::Value = serde_json::from_str(
        responses[4]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(layout_payload["layout"].is_array(), "{layout_payload}");

    let _ = std::fs::remove_dir_all(&dir);
}
