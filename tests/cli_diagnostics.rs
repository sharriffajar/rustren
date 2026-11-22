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
    let out = run(&[
        "validate",
        fixture("html_bad_attr.html").to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&out), 0, "warnings must not fail validate");
    let err = stderr(&out);
    assert!(
        err.contains("warning: line 2, column 12: numeric attribute x=\"100px\"")
    , "{err}");
    // The success summary belongs on stdout so stdout stays parseable.
    assert!(stdout(&out).contains("rustren: valid HTML"), "{}", stdout(&out));
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
    let out = run(&[
        "validate",
        fixture("json_bad_type.json").to_str().unwrap(),
    ]);
    assert_eq!(exit_code(&out), 2);
    let err = stderr(&out);
    assert!(err.contains("`canvas.width`"), "{err}");
    assert!(err.contains("(line 2, column 24)"), "{err}");
    assert!(err.contains("invalid type: string \"640\", expected u32"), "{err}");
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
    assert!(err.contains("<include> is missing a `src` attribute"), "{err}");
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
    assert!(warning["message"]
        .as_str()
        .unwrap()
        .starts_with("line 2, column 12:"));
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
    assert!(stderr(&snapshot).contains("nope.png"), "{}", stderr(&snapshot));
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
