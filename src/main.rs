mod actions;
mod animation;
mod assets;
mod capabilities;
mod compilers;
#[cfg(test)]
mod conformance_tests;
mod encoder;
mod examples_cmd;
mod fix;
mod ir;
mod lint;
mod mcp;
mod qa;
mod renderer;
mod schema;
mod security;
#[cfg(feature = "pro")]
mod server;
mod source_map;
#[cfg(feature = "pro")]
mod studio;

pub use lint::collect_lint_warnings;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use assets::AssetStore;
use encoder::FfmpegPipe;
use schema::Scene;

#[derive(Parser)]
#[command(
    name = "rustren",
    version,
    about = "Browserless 2D Video Render Engine in Rust"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Render a declarative JSON scene into an MP4 video
    Render {
        /// Path to the scene JSON file
        input: PathBuf,
        /// Output MP4 file path
        #[arg(short, long, default_value = "output.mp4")]
        output: PathBuf,
        /// Directory for caching downloaded assets
        #[arg(long, default_value = ".rustren_cache")]
        cache_dir: PathBuf,
        /// Constant Rate Factor for libx264 (0-51, lower is higher quality)
        #[arg(long, default_value_t = 18, value_parser = clap::value_parser!(u8).range(0..=51))]
        crf: u8,
        /// libx264 encoding preset (ultrafast..veryslow)
        #[arg(long, default_value = "medium")]
        preset: String,
        /// Print per-layer timing and cache hit/miss stats after render
        #[arg(long, default_value_t = false)]
        profile: bool,
        /// Name of composition profile to apply (e.g. "portrait", "square", "4k")
        #[arg(long)]
        composition: Option<String>,
        /// Emit a machine-readable JSON summary (progress moves to stderr)
        #[arg(long, value_enum, default_value_t = ErrorFormat::Text)]
        error_format: ErrorFormat,
    },
    /// Validate and lint a scene JSON file without rendering
    Validate {
        /// Path to the scene JSON file
        input: PathBuf,
        /// Also flag non-deterministic patterns (remote URLs, missing bundled fonts, repeat=infinite)
        #[arg(long, default_value_t = false)]
        strict_determinism: bool,
        /// Name of composition profile to apply (e.g. "portrait", "square", "4k")
        #[arg(long)]
        composition: Option<String>,
        /// Emit machine-readable JSON instead of plain text
        #[arg(long, value_enum, default_value_t = ErrorFormat::Text)]
        error_format: ErrorFormat,
        /// Do not fail on unknown fields/JSON-schema errors (warn only)
        #[arg(long, default_value_t = false)]
        ignore_unknown: bool,
        /// Also verify that local asset paths referenced by the scene exist
        #[arg(long, default_value_t = true)]
        check_assets: bool,
        /// Print a fixed version of the scene to stdout instead of validating it
        #[arg(long, default_value_t = false)]
        fix: bool,
        /// With --fix, rewrite the input file in place instead of printing to stdout
        #[arg(short = 'i', long, default_value_t = false)]
        in_place: bool,
    },
    /// Render one or more timestamp frames to PNG (or a contact sheet)
    Snapshot {
        /// Path to the scene JSON file
        input: PathBuf,
        /// Comma-separated timestamps in seconds (e.g. "1.5" or "1.5,5.6,14.5")
        #[arg(short, long, value_delimiter = ',', required = true)]
        t: Vec<f32>,
        /// Output PNG file path (or directory/prefix when multiple -t without --sheet)
        #[arg(short, long, default_value = "snapshot.png")]
        output: PathBuf,
        /// Combine multiple timestamps into a single contact sheet PNG
        #[arg(long, default_value_t = false)]
        sheet: bool,
        /// Number of columns for contact sheet (default: auto up to 4)
        #[arg(long)]
        cols: Option<u32>,
        /// Directory for caching downloaded assets
        #[arg(long, default_value = ".rustren_cache")]
        cache_dir: PathBuf,
        /// Name of composition profile to apply (e.g. "portrait", "square", "4k")
        #[arg(long)]
        composition: Option<String>,
        /// Emit a machine-readable JSON summary (progress moves to stderr)
        #[arg(long, value_enum, default_value_t = ErrorFormat::Text)]
        error_format: ErrorFormat,
    },
    /// Inspect computed bounding boxes and measured text sizes at a given timestamp
    Layout {
        /// Path to the scene JSON file
        input: PathBuf,
        /// Timestamp in seconds to evaluate layout and animations
        #[arg(short, long, default_value_t = 0.0)]
        t: f32,
        /// Directory for caching downloaded assets
        #[arg(long, default_value = ".rustren_cache")]
        cache_dir: PathBuf,
        /// Name of composition profile to apply (e.g. "portrait", "square", "4k")
        #[arg(long)]
        composition: Option<String>,
        /// Run automated layout troubleshooting diagnostics (overlaps, clipping, overflow)
        #[arg(long, default_value_t = false)]
        diagnose: bool,
        /// Emit a machine-readable JSON error payload instead of plain text
        #[arg(long, value_enum, default_value_t = ErrorFormat::Text)]
        error_format: ErrorFormat,
    },
    /// Print supported renderer and schema capabilities as JSON
    Capabilities,
    /// List built-in copy-ready scene examples, or print one to stdout
    Examples {
        /// Example name (omit to list every example)
        name: Option<String>,
        /// Only list examples carrying this tag
        #[arg(long)]
        tag: Option<String>,
        /// Emit a machine-readable JSON listing instead of plain text
        #[arg(long, value_enum, default_value_t = ErrorFormat::Text)]
        error_format: ErrorFormat,
    },
    /// Start the HTTP render service and job queue (Phase 3 VPS API)
    #[cfg(feature = "pro")]
    Serve {
        /// Bind address
        #[arg(long, default_value = "0.0.0.0")]
        host: String,
        /// Listen port
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
        /// Maximum concurrent render workers
        #[arg(short, long, default_value_t = 2)]
        workers: usize,
        /// Directory to store rendered MP4 outputs
        #[arg(long, default_value = ".rustren_jobs")]
        output_dir: PathBuf,
        /// Directory for caching downloaded assets
        #[arg(long, default_value = ".rustren_cache")]
        cache_dir: PathBuf,
        /// Output file TTL in seconds before auto-cleanup
        #[arg(long, default_value_t = 3600)]
        ttl_secs: u64,
        /// Max job submissions per IP per minute
        #[arg(long, default_value_t = 30)]
        rate_limit: usize,
        /// Optional API key (or set RUSTREN_API_KEY env var)
        #[arg(long)]
        api_key: Option<String>,
        /// Allow local filesystem paths in asset src (unsafe for public VPS)
        #[arg(long, default_value_t = false)]
        allow_local_paths: bool,
        /// Maximum allowed video duration in seconds
        #[arg(long, default_value_t = 60.0)]
        max_duration_secs: f32,
        /// Maximum allowed canvas width in px
        #[arg(long, default_value_t = 1920)]
        max_width: u32,
        /// Maximum allowed canvas height in px
        #[arg(long, default_value_t = 1920)]
        max_height: u32,
        /// Maximum allowed pixel count (width * height, default 1080*1920)
        #[arg(long, default_value_t = 1080 * 1920)]
        max_pixels: u64,
        /// Maximum allowed FPS
        #[arg(long, default_value_t = 60)]
        max_fps: u32,
    },
    /// Watch a scene JSON file and re-render the active frame to PNG on every save
    Watch {
        /// Input scene JSON file
        input: PathBuf,
        /// Timestamp in seconds to render on each save
        #[arg(short, long, default_value_t = 0.0)]
        t: f32,
        /// Output preview PNG path
        #[arg(short, long, default_value = "preview.png")]
        output: PathBuf,
        /// Directory for caching downloaded assets
        #[arg(long, default_value = ".rustren_cache")]
        cache_dir: PathBuf,
        /// Name of composition profile to apply (e.g. "portrait", "square", "4k")
        #[arg(long)]
        composition: Option<String>,
    },
    /// Compare two scene JSON files visually at timestamp t (visual regression test)
    Diff {
        /// Baseline scene JSON file
        baseline: PathBuf,
        /// Current scene JSON file
        current: PathBuf,
        /// Timestamp in seconds to compare
        #[arg(short, long, default_value_t = 0.0)]
        t: f32,
        /// Maximum allowed normalized pixel MAE (0.0 - 1.0)
        #[arg(long, default_value_t = 0.01)]
        threshold: f32,
        /// Optional output path for highlighted diff PNG
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Directory for caching downloaded assets
        #[arg(long, default_value = ".rustren_cache")]
        cache_dir: PathBuf,
        /// Emit a machine-readable JSON summary (progress moves to stderr)
        #[arg(long, value_enum, default_value_t = ErrorFormat::Text)]
        error_format: ErrorFormat,
    },
    /// Audit a rendered MP4 video for visual & audio defects (black frames, frozen frames, EBU R128 loudness)
    Qa {
        /// Path to the rendered MP4/WebM video file
        video: PathBuf,
        /// Emit machine-readable JSON report
        #[arg(long)]
        json: bool,
    },
    /// Launch browser-based live studio UI with interactive timeline, scrubber and preview
    #[cfg(feature = "pro")]
    Studio {
        /// Optional path to scene file (JSON or HTML) to open in studio
        input: Option<PathBuf>,
        /// Listen host
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Listen port
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
        /// Directory for caching downloaded assets
        #[arg(long, default_value = ".rustren_cache")]
        cache_dir: PathBuf,
    },
    /// Alias for Studio: preview a scene file in browser with live scrubber
    #[cfg(feature = "pro")]
    Preview {
        /// Path to scene file (JSON or HTML) to preview
        input: PathBuf,
        /// Listen host
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Listen port
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
        /// Directory for caching downloaded assets
        #[arg(long, default_value = ".rustren_cache")]
        cache_dir: PathBuf,
    },
    /// Start MCP (Model Context Protocol) stdio JSON-RPC server for AI agents
    Mcp {
        /// Path to the scene JSON file to inspect/edit
        input: PathBuf,
    },
    /// Detect audio beat timestamps (in seconds) from an audio or video file
    Beats {
        /// Path to the audio/video file
        input: PathBuf,
        /// Peak sensitivity multiplier (default: 1.5, higher = fewer beats)
        #[arg(long, default_value_t = 1.5)]
        sensitivity: f32,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ErrorFormat {
    #[default]
    Text,
    Json,
}

fn emit_validate_error(format: &ErrorFormat, message: &str) -> Result<()> {
    emit_error(format, "invalid_scene", message)
}

fn emit_error(format: &ErrorFormat, code: &str, message: &str) -> Result<()> {
    match format {
        ErrorFormat::Json => {
            // Shared envelope for every command: `ok` for render/snapshot/diff
            // style consumers, `valid` for the validate contract.
            let payload = serde_json::json!({
                "ok": false,
                "valid": false,
                "error": { "code": code, "message": message },
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        ErrorFormat::Text => eprintln!("error: {message}"),
    }
    Ok(())
}

/// Report a command failure and terminate with the requested exit code.
/// Errors that describe the scene use exit 2 (same as `validate`); I/O and
/// render-runtime failures use exit 1 so agents can tell them apart.
fn fail(error_format: &ErrorFormat, code: &str, message: &str, exit: i32) -> ! {
    let _ = emit_error(error_format, code, message);
    std::process::exit(exit);
}

fn is_json_input(input: &Path) -> bool {
    input
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
}

/// Fail early, in the requested format, when the input file does not exist.
/// I/O problems are exit 1 (runtime failure), not exit 2 (invalid scene).
fn require_input(input: &Path, error_format: &ErrorFormat) {
    if !input.exists() {
        fail(
            error_format,
            "input_error",
            &format!("input file not found: {}", input.display()),
            1,
        );
    }
}

/// JSON-only unknown-field gate, shared by `validate`, `render`, `snapshot`,
/// `layout` and `diff`. Unknown fields are a hard error so a misspelled field
/// cannot be silently ignored by a render that already passed compilation.
fn check_json_unknown_fields(input: &Path, error_format: &ErrorFormat) {
    if !is_json_input(input) {
        return;
    }
    require_input(input, error_format);
    let raw = match fs::read_to_string(input) {
        Ok(raw) => raw,
        Err(e) => fail(
            error_format,
            "input_error",
            &format!("Failed to read input: {}: {}", input.display(), e),
            1,
        ),
    };
    if let Err(e) = lint::validate_no_unknown_fields(&raw) {
        let message = gate_error_message(input, &e);
        fail(error_format, "invalid_scene", &message, 2);
    }
}

/// Turn a gate failure into a single-line message. JSON syntax errors get a
/// usable line/column; serde reports column 0 at EOF, so clamp it to 1.
fn gate_error_message(input: &Path, e: &anyhow::Error) -> String {
    match e.downcast_ref::<serde_json::Error>() {
        Some(syntax) => {
            // serde's Display repeats the position; keep the message once.
            let detail = syntax
                .to_string()
                .split(" at line ")
                .next()
                .unwrap_or("")
                .to_string();
            format!(
                "JSON syntax error in {:?} (line {}, column {}): {}",
                input,
                syntax.line(),
                syntax.column().max(1),
                detail
            )
        }
        None => e.to_string(),
    }
}

fn main() -> Result<()> {
    if std::env::var_os("RAYON_NUM_THREADS").is_none() {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let (rayon_threads, _) = encoder::compute_vcpu_partition(cpus);
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(rayon_threads)
            .build_global();
    }
    let cli = Cli::parse();

    match cli.command {
        Commands::Capabilities => {
            let reg = capabilities::CapabilityRegistry::current();
            println!("{}", serde_json::to_string_pretty(&reg)?);
        }
        Commands::Examples {
            name,
            tag,
            error_format,
        } => {
            run_examples(name.as_deref(), tag.as_deref(), &error_format)?;
        }
        Commands::Validate {
            input,
            strict_determinism,
            composition,
            error_format,
            ignore_unknown,
            check_assets,
            fix,
            in_place,
        } => {
            let base_dir = input.parent();
            require_input(&input, &error_format);
            let compiler = match compilers::detect(&input) {
                Ok(c) => c,
                Err(e) => fail(&error_format, "unsupported_format", &format!("{e:#}"), 2),
            };

            // --fix: rewrite (or print) safe corrections before validating the rest.
            if fix {
                let raw = fs::read_to_string(&input)
                    .with_context(|| format!("Failed to read input: {}", input.display()))?;
                let (fixed, applied) = if compiler.format_name() == "html" {
                    fix::fix_html_str(&raw)
                } else {
                    let Some((fixed, applied)) = fix::fix_json_str(&raw) else {
                        emit_validate_error(
                            &error_format,
                            "cannot --fix: input is not valid JSON",
                        )?;
                        std::process::exit(2);
                    };
                    (fixed, applied)
                };
                if in_place {
                    fs::write(&input, &fixed).with_context(|| {
                        format!("Failed to write fixed scene: {}", input.display())
                    })?;
                } else {
                    // Files that were already clean are echoed byte-for-byte; only
                    // add a trailing newline when the output has none (pretty JSON).
                    print!("{fixed}");
                    if !fixed.ends_with('\n') {
                        println!();
                    }
                }
                match error_format {
                    ErrorFormat::Json => {
                        let payload = serde_json::json!({
                            "fixed": applied.len(),
                            "fixes": applied.iter().map(|f| serde_json::json!({
                                "path": f.path,
                                "field": f.field,
                                "before": f.before,
                                "after": f.after,
                            })).collect::<Vec<_>>(),
                        });
                        eprintln!("{}", serde_json::to_string_pretty(&payload)?);
                    }
                    ErrorFormat::Text => {
                        for f in &applied {
                            eprintln!("fix: {} {} {} -> {}", f.path, f.field, f.before, f.after);
                        }
                    }
                }
                if in_place {
                    eprintln!(
                        "rustren: rewrote {} ({} fix(es))",
                        input.display(),
                        applied.len()
                    );
                }
                return Ok(());
            }

            // Unknown-field validation needs the raw source. HTML scenes are
            // checked structurally by the compiler, so only JSON is pre-scanned.
            if !ignore_unknown {
                check_json_unknown_fields(&input, &error_format);
            }

            let mut compiled = match compiler.compile(&input) {
                Ok(c) => c,
                Err(e) => fail(&error_format, "invalid_scene", &format!("{e:#}"), 2),
            };
            let scene = compiled.as_static_mut();
            if let Some(comp_name) = composition
                && let Err(e) = scene.apply_profile(&comp_name)
            {
                fail(&error_format, "invalid_scene", &format!("{e:#}"), 2);
            }
            if let Err(e) = capabilities::validate_scene_capabilities(scene) {
                fail(&error_format, "invalid_scene", &format!("{e:#}"), 2);
            }

            let mut structured = lint::collect_lint_warnings_structured(scene);
            if strict_determinism {
                structured.extend(lint::collect_determinism_warnings_structured(scene));
            }
            if check_assets {
                structured.extend(lint::check_local_assets(scene, base_dir));
            }
            if compiler.format_name() == "html" {
                let raw = fs::read_to_string(&input)
                    .with_context(|| format!("Failed to read input: {}", input.display()))?;
                for issue in source_map::lint_html_source(&raw) {
                    structured.push(lint::LintWarning {
                        code: issue.code,
                        path: issue.path,
                        message: issue.message,
                        fix: issue.fix,
                        affected_paths: Vec::new(),
                    });
                }
            }

            match error_format {
                ErrorFormat::Json => {
                    let payload = serde_json::json!({
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
                        "warning_count": structured.len(),
                        "warnings": structured.iter().map(|w| serde_json::json!({
                            "code": w.code,
                            "path": w.path,
                            "message": w.message,
                            "fix": w.fix,
                            "affected_paths": w.affected_paths,
                        })).collect::<Vec<_>>(),
                    });
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                }
                ErrorFormat::Text => {
                    for w in &structured {
                        eprintln!("warning: {}", w.to_text());
                    }
                    println!(
                        "rustren: valid {} (schema v{}, {}x{} @ {}fps, {:.2}s, {} root layers, {} warning(s))",
                        compiler.format_name().to_uppercase(),
                        scene.schema_version,
                        scene.canvas.width,
                        scene.canvas.height,
                        scene.canvas.fps,
                        scene.canvas.duration_secs,
                        scene.layers.len(),
                        structured.len()
                    );
                }
            }
        }
        Commands::Render {
            input,
            output,
            cache_dir,
            crf,
            preset,
            profile,
            composition,
            error_format,
        } => {
            require_input(&input, &error_format);
            check_json_unknown_fields(&input, &error_format);
            let start_time = Instant::now();
            let compiler = match compilers::detect(&input) {
                Ok(c) => c,
                Err(e) => fail(&error_format, "unsupported_format", &format!("{e:#}"), 2),
            };
            let mut compiled = match compiler.compile(&input) {
                Ok(c) => c,
                Err(e) => fail(&error_format, "invalid_scene", &format!("{e:#}"), 2),
            };
            let scene = compiled.as_static_mut();
            if let Some(comp_name) = composition
                && let Err(e) = scene.apply_profile(&comp_name)
            {
                fail(&error_format, "invalid_scene", &format!("{e:#}"), 2);
            }
            if let Err(e) = capabilities::validate_scene_capabilities(scene) {
                fail(&error_format, "invalid_scene", &format!("{e:#}"), 2);
            }

            // Catch missing local assets before the (expensive) render starts,
            // instead of after ffmpeg has been spawned. Paths resolve exactly
            // the way the asset loader resolves them: relative to the scene
            // file first, then the working directory.
            if let Some(warning) = lint::check_local_assets(scene, input.parent()).first() {
                fail(&error_format, "missing_asset", &warning.message, 1);
            }

            let total_frames = (scene.canvas.duration_secs * scene.canvas.fps as f32).ceil() as u32;
            match error_format {
                ErrorFormat::Text => println!(
                    "rustren: rendering {}x{} @ {}fps ({} frames, crf={}, preset={}) -> {}",
                    scene.canvas.width,
                    scene.canvas.height,
                    scene.canvas.fps,
                    total_frames,
                    crf,
                    preset,
                    output.display()
                ),
                ErrorFormat::Json => eprintln!(
                    "rustren: rendering {}x{} @ {}fps ({} frames) -> {}",
                    scene.canvas.width,
                    scene.canvas.height,
                    scene.canvas.fps,
                    total_frames,
                    output.display()
                ),
            }

            if let Err(e) = render_scene_to_file_with_base(
                scene,
                &output,
                Path::new(&cache_dir),
                input.parent(),
                crf,
                &preset,
                profile,
            ) {
                fail(&error_format, "render_failed", &format!("{e:#}"), 1);
            }

            let elapsed = start_time.elapsed().as_secs_f32();
            match error_format {
                ErrorFormat::Text => {
                    println!("rustren: done in {:.2}s -> {}", elapsed, output.display())
                }
                ErrorFormat::Json => {
                    let payload = serde_json::json!({
                        "ok": true,
                        "output": output,
                        "canvas": {
                            "width": scene.canvas.width,
                            "height": scene.canvas.height,
                            "fps": scene.canvas.fps,
                            "duration_secs": scene.canvas.duration_secs,
                        },
                        "frames": total_frames,
                        "elapsed_secs": elapsed,
                    });
                    println!("{}", serde_json::to_string_pretty(&payload).unwrap());
                }
            }
        }
        Commands::Snapshot {
            input,
            t,
            output,
            sheet,
            cols,
            cache_dir,
            composition,
            error_format,
        } => {
            require_input(&input, &error_format);
            check_json_unknown_fields(&input, &error_format);
            let start_time = Instant::now();
            let compiler = match compilers::detect(&input) {
                Ok(c) => c,
                Err(e) => fail(&error_format, "unsupported_format", &format!("{e:#}"), 2),
            };
            let mut compiled = match compiler.compile(&input) {
                Ok(c) => c,
                Err(e) => fail(&error_format, "invalid_scene", &format!("{e:#}"), 2),
            };
            let scene = compiled.as_static_mut();
            if let Some(comp_name) = composition
                && let Err(e) = scene.apply_profile(&comp_name)
            {
                fail(&error_format, "invalid_scene", &format!("{e:#}"), 2);
            }
            if let Err(e) = capabilities::validate_scene_capabilities(scene) {
                fail(&error_format, "invalid_scene", &format!("{e:#}"), 2);
            }
            let base_dir = input.parent();
            let assets =
                match AssetStore::load_for_scene_with_base(scene, Path::new(&cache_dir), base_dir) {
                    Ok(a) => a,
                    Err(e) => fail(&error_format, "missing_asset", &format!("{e:#}"), 1),
                };
            if let Err(e) = render_snapshots(scene, &assets, &t, &output, sheet, cols) {
                fail(&error_format, "snapshot_failed", &format!("{e:#}"), 1);
            }
            let elapsed = start_time.elapsed().as_secs_f32();
            match error_format {
                ErrorFormat::Text => println!(
                    "rustren: snapshot(s) rendered in {:.2}s -> {}",
                    elapsed,
                    output.display()
                ),
                ErrorFormat::Json => {
                    let payload = serde_json::json!({
                        "ok": true,
                        "output": output,
                        "times": t,
                        "count": t.len(),
                        "elapsed_secs": elapsed,
                    });
                    println!("{}", serde_json::to_string_pretty(&payload).unwrap());
                }
            }
        }
        Commands::Layout {
            input,
            t,
            cache_dir,
            composition,
            diagnose,
            error_format,
        } => {
            require_input(&input, &error_format);
            check_json_unknown_fields(&input, &error_format);
            let compiler = match compilers::detect(&input) {
                Ok(c) => c,
                Err(e) => fail(&error_format, "unsupported_format", &format!("{e:#}"), 2),
            };
            let mut compiled = match compiler.compile(&input) {
                Ok(c) => c,
                Err(e) => fail(&error_format, "invalid_scene", &format!("{e:#}"), 2),
            };
            let scene = compiled.as_static_mut();
            if let Some(comp_name) = composition
                && let Err(e) = scene.apply_profile(&comp_name)
            {
                fail(&error_format, "invalid_scene", &format!("{e:#}"), 2);
            }
            if let Err(e) = capabilities::validate_scene_capabilities(scene) {
                fail(&error_format, "invalid_scene", &format!("{e:#}"), 2);
            }
            let base_dir = input.parent();
            let assets =
                match AssetStore::load_for_scene_with_base(scene, Path::new(&cache_dir), base_dir) {
                    Ok(a) => a,
                    Err(e) => fail(&error_format, "missing_asset", &format!("{e:#}"), 1),
                };
            let layout = match renderer::inspect_layout_at_time(scene, &assets, t) {
                Ok(l) => l,
                Err(e) => fail(&error_format, "layout_failed", &format!("{e:#}"), 1),
            };
            if diagnose {
                let diagnostics = match renderer::diagnose_layout_at_time(scene, &assets, t) {
                    Ok(d) => d,
                    Err(e) => fail(&error_format, "layout_failed", &format!("{e:#}"), 1),
                };
                eprintln!(
                    "rustren: layout diagnostics: {} issue(s) found",
                    diagnostics.len()
                );
                let out = serde_json::json!({
                    "layout": layout,
                    "diagnostics": diagnostics,
                });
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            } else {
                println!("{}", serde_json::to_string_pretty(&layout).unwrap());
            }
        }
        #[cfg(feature = "pro")]
        Commands::Serve {
            host,
            port,
            workers,
            output_dir,
            cache_dir,
            ttl_secs,
            rate_limit,
            api_key,
            allow_local_paths,
            max_duration_secs,
            max_width,
            max_height,
            max_pixels,
            max_fps,
        } => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(server::run_server(server::ServeConfig {
                host,
                port,
                workers,
                output_dir,
                cache_dir,
                ttl_secs,
                rate_limit_per_min: rate_limit,
                api_key: api_key.or_else(|| std::env::var("RUSTREN_API_KEY").ok()),
                limits: security::ServiceLimits {
                    allow_local_paths,
                    max_width,
                    max_height,
                    max_pixels,
                    max_fps,
                    max_duration_secs,
                },
            }))?;
        }
        Commands::Watch {
            input,
            t,
            output,
            cache_dir,
            composition,
        } => {
            run_watch(&input, t, &output, &cache_dir, composition.as_deref())?;
        }
        Commands::Qa { video, json } => {
            let report = qa::run_qa(&video, json)?;
            if report.verdict == "FAIL" {
                std::process::exit(1);
            }
        }
        Commands::Diff {
            baseline,
            current,
            t,
            threshold,
            out,
            cache_dir,
            error_format,
        } => {
            check_json_unknown_fields(&baseline, &error_format);
            check_json_unknown_fields(&current, &error_format);
            require_input(&baseline, &error_format);
            require_input(&current, &error_format);
            match run_diff(
                &baseline,
                &current,
                t,
                threshold,
                out.as_deref(),
                &cache_dir,
            ) {
                Ok(report) => {
                    let regressed = report.mae as f32 > threshold;
                    match error_format {
                        ErrorFormat::Text => {
                            println!(
                                "Diff at t={:.2}s: MAE={:.6} RMSE={:.6} changed_pixels={:.2}% (threshold={:.6})",
                                t, report.mae, report.rmse, report.changed_pct, threshold
                            );
                            if regressed {
                                fail(
                                    &error_format,
                                    "visual_regression",
                                    &format!(
                                        "Visual regression detected: MAE {:.6} > threshold {:.6}",
                                        report.mae, threshold
                                    ),
                                    1,
                                );
                            }
                        }
                        ErrorFormat::Json => {
                            // One JSON document: `ok: false` already carries the verdict,
                            // so a regression does not need a second error payload.
                            // Round f32s so 0.01 does not leak as 0.009999999776482582.
                            let round = |v: f32| ((v as f64) * 1e6).round() / 1e6;
                            let payload = serde_json::json!({
                                "ok": !regressed,
                                "t": round(t),
                                "mae": report.mae,
                                "rmse": report.rmse,
                                "changed_pixels_pct": report.changed_pct,
                                "threshold": round(threshold),
                                "out": out,
                            });
                            println!("{}", serde_json::to_string_pretty(&payload).unwrap());
                            if regressed {
                                std::process::exit(1);
                            }
                        }
                    }
                }
                Err(e) => fail(&error_format, "diff_failed", &format!("{e:#}"), 1),
            }
        }
        #[cfg(feature = "pro")]
        Commands::Studio {
            input,
            host,
            port,
            cache_dir,
        } => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(studio::run_studio(
                input.as_deref(),
                &host,
                port,
                &cache_dir,
            ))?;
        }
        #[cfg(feature = "pro")]
        Commands::Preview {
            input,
            host,
            port,
            cache_dir,
        } => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(studio::run_studio(Some(&input), &host, port, &cache_dir))?;
        }
        Commands::Mcp { input } => {
            mcp::run_mcp_server(&input)?;
        }
        Commands::Beats { input, sensitivity } => {
            let beats = encoder::detect_audio_beats(&input, sensitivity)?;
            println!("{}", serde_json::to_string_pretty(&beats)?);
        }
    }

    Ok(())
}

/// Print either the example listing or a single example's source.
fn run_examples(name: Option<&str>, tag: Option<&str>, format: &ErrorFormat) -> Result<()> {
    if let Some(name) = name {
        let Some(example) = examples_cmd::find(name) else {
            let message = format!(
                "unknown example '{}'; run `rustren examples` to list {} examples",
                name,
                examples_cmd::EXAMPLES.len()
            );
            emit_error(format, "unknown_example", &message)?;
            std::process::exit(2);
        };
        match format {
            // Source goes to stdout unchanged so it can be piped straight to a file.
            ErrorFormat::Text => print!("{}", example.source),
            ErrorFormat::Json => {
                let payload = serde_json::json!({
                    "name": example.name,
                    "description": example.description,
                    "format": example.format,
                    "tags": example.tags,
                    "source": example.source,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
        }
        return Ok(());
    }

    let listed = examples_cmd::by_tag(tag);

    // Unknown tags are a typo, not an empty result; fail so agents notice.
    if tag.is_some() && listed.is_empty() {
        let message = format!(
            "no example has tag '{}'; available tags: {}",
            tag.unwrap_or_default(),
            examples_cmd::all_tags().join(", ")
        );
        emit_error(format, "unknown_example_tag", &message)?;
        std::process::exit(2);
    }

    match format {
        ErrorFormat::Json => {
            let payload = serde_json::json!({
                "count": listed.len(),
                "tags": examples_cmd::all_tags(),
                "examples": listed.iter().map(|e| serde_json::json!({
                    "name": e.name,
                    "description": e.description,
                    "format": e.format,
                    "tags": e.tags,
                })).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        ErrorFormat::Text => {
            for example in &listed {
                println!(
                    "{} ({}) - {}",
                    example.name, example.format, example.description
                );
            }
            println!(
                "\n{} example(s). Print one with `rustren examples <name>`; tags: {}",
                listed.len(),
                examples_cmd::all_tags().join(", ")
            );
        }
    }
    Ok(())
}

fn collect_video_audio_tracks(
    layers: &[schema::Layer],
    timeline: schema::TimelineMode,
    time_offset: f32,
    parent_duration: f32,
    assets: &AssetStore,
    out: &mut Vec<(schema::AudioTrack, PathBuf)>,
) {
    let mut series_cursor = 0.0f32;
    for layer in layers {
        let local_start = match timeline {
            schema::TimelineMode::Series => {
                (series_cursor + layer.start_sec - layer.overlap_secs).max(0.0)
            }
            _ => layer.start_sec,
        };
        let layer_dur = layer
            .duration_secs
            .or_else(|| layer.end_sec.map(|e| (e - layer.start_sec).max(0.0)))
            .unwrap_or((parent_duration - local_start).max(0.0));
        if timeline == schema::TimelineMode::Series {
            series_cursor = local_start + layer_dur;
        }
        let global_start = time_offset + local_start;
        match &layer.kind {
            schema::LayerKind::Video {
                src,
                trim_start_sec,
                trim_end_sec,
                playback_rate,
                volume,
                muted,
                ..
            } if !*muted && *volume > 0.0 => {
                if let Some(va) = assets.videos.get(src)
                    && va.has_audio
                {
                    let effective_trim_end = trim_end_sec
                        .or_else(|| Some(trim_start_sec + layer_dur * playback_rate.max(0.1)));
                    out.push((
                        schema::AudioTrack {
                            src: src.clone(),
                            start_sec: global_start,
                            volume: *volume,
                            trim_start_sec: Some(*trim_start_sec),
                            trim_end_sec: effective_trim_end,
                            playback_rate: *playback_rate,
                            fade_in_secs: 0.0,
                            fade_out_secs: 0.0,
                            animations: Vec::new(),
                        },
                        va.path.clone(),
                    ));
                }
            }
            schema::LayerKind::Container {
                timeline: child_tl,
                children,
                ..
            } => {
                let child_offset = match child_tl {
                    schema::TimelineMode::Global => time_offset,
                    schema::TimelineMode::Local | schema::TimelineMode::Series => global_start,
                };
                collect_video_audio_tracks(
                    children,
                    *child_tl,
                    child_offset,
                    layer_dur,
                    assets,
                    out,
                );
            }
            _ => {}
        }
    }
}

pub fn render_scene_to_file(scene: &Scene, output: &Path, cache_dir: &Path) -> Result<()> {
    render_scene_to_file_with_base(scene, output, cache_dir, None, 18, "medium", false)
}

pub fn render_scene_to_file_with_options(
    scene: &Scene,
    output: &Path,
    cache_dir: &Path,
    crf: u8,
    preset: &str,
) -> Result<()> {
    render_scene_to_file_with_base(scene, output, cache_dir, None, crf, preset, false)
}

pub fn render_scene_to_file_with_profile(
    scene: &Scene,
    output: &Path,
    cache_dir: &Path,
    crf: u8,
    preset: &str,
    profile: bool,
) -> Result<()> {
    render_scene_to_file_with_base(scene, output, cache_dir, None, crf, preset, profile)
}

/// Render a scene whose assets resolve against `base_dir` (the scene file's
/// directory) first, then the working directory.
#[allow(clippy::too_many_arguments)]
pub fn render_scene_to_file_with_base(
    scene: &Scene,
    output: &Path,
    cache_dir: &Path,
    base_dir: Option<&Path>,
    crf: u8,
    preset: &str,
    profile: bool,
) -> Result<()> {
    if profile {
        renderer::reset_cache_stats();
    } else {
        renderer::clear_render_caches();
    }
    let mut assets = AssetStore::load_for_scene_with_base(scene, cache_dir, base_dir)?;
    let mut audio_tracks: Vec<_> = scene
        .audio
        .iter()
        .cloned()
        .zip(assets.audio_paths.iter().cloned())
        .collect();
    collect_video_audio_tracks(
        &scene.layers,
        schema::TimelineMode::Global,
        0.0,
        scene.canvas.duration_secs,
        &assets,
        &mut audio_tracks,
    );

    let (static_prefix_len, static_base) = renderer::build_static_prefix_pixmap(scene, &assets)?;

    let total_frames = (scene.canvas.duration_secs * scene.canvas.fps as f32).ceil() as u32;
    let mut pipe = FfmpegPipe::spawn(
        scene.canvas.width,
        scene.canvas.height,
        scene.canvas.fps,
        &audio_tracks,
        output,
        crf,
        preset,
    )?;

    let mut total_layer_ns = if profile {
        vec![0u64; scene.layers.len()]
    } else {
        Vec::new()
    };

    // Render batches in parallel with Rayon while streaming encoded frames over a bounded channel.
    let batch_size = (rayon::current_num_threads() * 8).clamp(16, 64) as u32;
    let (tx, rx) =
        std::sync::mpsc::sync_channel::<std::sync::Arc<Vec<u8>>>((batch_size as usize) * 2);
    let writer_handle = std::thread::spawn(move || -> Result<()> {
        for frame in rx {
            pipe.write_frame(&frame)?;
            if let Ok(buf) = std::sync::Arc::try_unwrap(frame) {
                renderer::recycle_frame_buffer(buf);
            }
        }
        pipe.finish()
    });

    let mut render_err: Option<anyhow::Error> = None;
    'batches: for batch_start in (0..total_frames).step_by(batch_size as usize) {
        if assets.images.len() > 8 {
            let batch_start_sec = batch_start as f32 / scene.canvas.fps as f32;
            assets.evict_images_before(scene, batch_start_sec - 0.5);
        }
        let batch_end = (batch_start + batch_size).min(total_frames);
        let mut batch_sigs: Vec<(u32, Option<u64>)> =
            Vec::with_capacity((batch_end - batch_start) as usize);
        let mut unique_to_render: Vec<(u32, Option<u64>)> =
            Vec::with_capacity((batch_end - batch_start) as usize);
        let mut seen_sigs: Vec<u64> = Vec::with_capacity((batch_end - batch_start) as usize);

        for idx in batch_start..batch_end {
            let t = idx as f32 / scene.canvas.fps as f32;
            let sig = renderer::compute_frame_temporal_signature(scene, t, static_prefix_len);
            if let Some(s) = sig {
                if seen_sigs.contains(&s) {
                    batch_sigs.push((idx, Some(s)));
                    continue;
                }
                seen_sigs.push(s);
            }
            batch_sigs.push((idx, sig));
            unique_to_render.push((idx, sig));
        }

        let skipped_in_batch = batch_sigs.len() - unique_to_render.len();
        if skipped_in_batch > 0 {
            renderer::record_frame_dedup_hits(skipped_in_batch as u64);
        }

        let frames_res: Result<Vec<(u32, Option<u64>, std::sync::Arc<Vec<u8>>, Vec<u64>)>> =
            unique_to_render
                .into_par_iter()
                .map(|(idx, sig)| {
                    renderer::render_frame_arc_with_base_profiled(
                        scene,
                        &assets,
                        idx,
                        static_prefix_len,
                        static_base.as_ref(),
                        profile,
                    )
                    .map(|(arc, lns)| (idx, sig, arc, lns))
                })
                .collect();

        match frames_res {
            Ok(rendered) => {
                let mut by_idx: Vec<(u32, std::sync::Arc<Vec<u8>>)> =
                    Vec::with_capacity(rendered.len());
                let mut by_sig: Vec<(u64, std::sync::Arc<Vec<u8>>)> =
                    Vec::with_capacity(rendered.len());
                for (idx, sig, arc, lns) in rendered {
                    if profile {
                        for (i, ns) in lns.into_iter().enumerate() {
                            total_layer_ns[i] += ns;
                        }
                    }
                    if let Some(s) = sig {
                        by_sig.push((s, arc.clone()));
                    }
                    by_idx.push((idx, arc));
                }
                for (idx, sig) in batch_sigs {
                    let frame = if let Some(s) = sig {
                        by_sig
                            .iter()
                            .find(|(ks, _)| *ks == s)
                            .map(|(_, a)| a.clone())
                    } else {
                        None
                    }
                    .or_else(|| {
                        by_idx
                            .iter()
                            .find(|(ki, _)| *ki == idx)
                            .map(|(_, a)| a.clone())
                    })
                    .expect("rendered frame must exist for batch index");
                    if tx.send(frame).is_err() {
                        break 'batches;
                    }
                }
            }
            Err(e) => {
                render_err = Some(e);
                break 'batches;
            }
        }
    }

    drop(tx);
    let writer_res = writer_handle
        .join()
        .map_err(|_| anyhow::anyhow!("FFmpeg writer thread panicked"))?;

    if let Some(e) = render_err {
        renderer::clear_render_caches();
        return Err(e);
    }
    writer_res?;
    renderer::clear_render_caches();

    if profile {
        let (svg_hits, svg_misses, blur_hits, blur_misses) = renderer::get_cache_stats();
        let (text_hits, text_misses) = renderer::get_text_cache_stats();
        let (dedup_hits, dedup_misses) = renderer::get_frame_dedup_stats();
        let pct = |h: u64, m: u64| {
            if h + m > 0 {
                h as f64 * 100.0 / (h + m) as f64
            } else {
                0.0
            }
        };
        println!("--- rustren profile ---");
        println!(
            "static_prefix_skipped: {static_prefix_len}/{} root layers",
            scene.layers.len()
        );
        println!(
            "svg_raster_cache:  hits={svg_hits}, misses={svg_misses} ({:.1}% hit)",
            pct(svg_hits, svg_misses)
        );
        println!(
            "blur_cache:        hits={blur_hits}, misses={blur_misses} ({:.1}% hit)",
            pct(blur_hits, blur_misses)
        );
        println!(
            "text_raster_cache: hits={text_hits}, misses={text_misses} ({:.1}% hit)",
            pct(text_hits, text_misses)
        );
        println!(
            "frame_dedup_cache: hits={dedup_hits}, misses={dedup_misses} ({:.1}% hit)",
            pct(dedup_hits, dedup_misses)
        );
        let mut ranked: Vec<(usize, u64)> = total_layer_ns.into_iter().enumerate().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        println!("top root layers (cumulative CPU time across {total_frames} frames):");
        for (idx, ns) in ranked.into_iter().filter(|(_, ns)| *ns > 0).take(10) {
            let name = scene.layers[idx].name.as_deref().unwrap_or("(unnamed)");
            println!("  layers[{idx}] ({name}): {:.2}ms", ns as f64 / 1_000_000.0);
        }
    }
    Ok(())
}

pub(crate) fn premul_to_rgba_image(w: u32, h: u32, mut data: Vec<u8>) -> Result<image::RgbaImage> {
    for px in data.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a > 0 && a < 255 {
            let af = 255.0 / a as f32;
            px[0] = ((px[0] as f32) * af).round().min(255.0) as u8;
            px[1] = ((px[1] as f32) * af).round().min(255.0) as u8;
            px[2] = ((px[2] as f32) * af).round().min(255.0) as u8;
        }
    }
    image::RgbaImage::from_raw(w, h, data)
        .ok_or_else(|| anyhow::anyhow!("Failed to construct RGBA image {w}x{h}"))
}

pub(crate) fn render_snapshots_to_png_bytes(
    scene: &Scene,
    assets: &AssetStore,
    times: &[f32],
    sheet: bool,
    cols_opt: Option<u32>,
) -> Result<Vec<u8>> {
    let w = scene.canvas.width;
    let h = scene.canvas.height;
    let frames: Vec<(f32, image::RgbaImage)> = times
        .par_iter()
        .map(|&ts| {
            let raw = renderer::render_frame_at_time(scene, assets, ts)?;
            Ok((ts, premul_to_rgba_image(w, h, raw)?))
        })
        .collect::<Result<Vec<_>>>()?;

    let img = if !sheet && frames.len() == 1 {
        frames.into_iter().next().unwrap().1
    } else {
        let count = frames.len() as u32;
        let cols = cols_opt.unwrap_or_else(|| count.min(4)).max(1);
        let rows = count.div_ceil(cols);
        let max_thumb_w = 640u32;
        let (tw, th) = if w > max_thumb_w {
            let scale = max_thumb_w as f32 / w as f32;
            (max_thumb_w, ((h as f32) * scale).round().max(1.0) as u32)
        } else {
            (w, h)
        };
        let pad = 16u32;
        let sheet_w = cols * tw + (cols + 1) * pad;
        let sheet_h = rows * th + (rows + 1) * pad;
        let mut canvas =
            image::RgbaImage::from_pixel(sheet_w, sheet_h, image::Rgba([24, 24, 24, 255]));
        for (idx, (_, frame_img)) in frames.into_iter().enumerate() {
            let col = (idx as u32) % cols;
            let row = (idx as u32) / cols;
            let thumb = if tw != w || th != h {
                image::imageops::resize(&frame_img, tw, th, image::imageops::FilterType::Triangle)
            } else {
                frame_img
            };
            let ox = pad + col * (tw + pad);
            let oy = pad + row * (th + pad);
            image::imageops::overlay(&mut canvas, &thumb, ox as i64, oy as i64);
        }
        canvas
    };

    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

pub(crate) fn render_snapshots(
    scene: &Scene,
    assets: &AssetStore,
    times: &[f32],
    output: &Path,
    sheet: bool,
    cols_opt: Option<u32>,
) -> Result<()> {
    let w = scene.canvas.width;
    let h = scene.canvas.height;
    let frames: Vec<(f32, image::RgbaImage)> = times
        .par_iter()
        .map(|&ts| {
            let raw = renderer::render_frame_at_time(scene, assets, ts)?;
            Ok((ts, premul_to_rgba_image(w, h, raw)?))
        })
        .collect::<Result<Vec<_>>>()?;

    if sheet
        || frames.len() > 1 && output.extension().and_then(|e| e.to_str()) == Some("png") && sheet
    {
        // handled below
    }

    if !sheet && frames.len() == 1 {
        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        frames[0].1.save(output)?;
        return Ok(());
    }

    if !sheet {
        let is_dir = output.is_dir() || output.extension().is_none();
        if is_dir {
            fs::create_dir_all(output)?;
            for (ts, img) in frames {
                let p = output.join(format!("frame_{ts:.2}s.png"));
                img.save(p)?;
            }
        } else {
            let stem = output
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("snapshot");
            let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("png");
            let parent = output.parent().unwrap_or_else(|| Path::new("."));
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
            for (ts, img) in frames {
                let p = parent.join(format!("{stem}_{ts:.2}s.{ext}"));
                img.save(p)?;
            }
        }
        return Ok(());
    }

    let count = frames.len() as u32;
    let cols = cols_opt.unwrap_or_else(|| count.min(4)).max(1);
    let rows = count.div_ceil(cols);
    let max_thumb_w = 640u32;
    let (tw, th) = if w > max_thumb_w {
        let scale = max_thumb_w as f32 / w as f32;
        (max_thumb_w, ((h as f32) * scale).round().max(1.0) as u32)
    } else {
        (w, h)
    };
    let pad = 16u32;
    let sheet_w = cols * tw + (cols + 1) * pad;
    let sheet_h = rows * th + (rows + 1) * pad;
    let mut canvas = image::RgbaImage::from_pixel(sheet_w, sheet_h, image::Rgba([24, 24, 24, 255]));
    for (idx, (_, img)) in frames.into_iter().enumerate() {
        let col = (idx as u32) % cols;
        let row = (idx as u32) / cols;
        let thumb = if tw != w || th != h {
            image::imageops::resize(&img, tw, th, image::imageops::FilterType::Triangle)
        } else {
            img
        };
        let ox = pad + col * (tw + pad);
        let oy = pad + row * (th + pad);
        image::imageops::overlay(&mut canvas, &thumb, ox as i64, oy as i64);
    }
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    canvas.save(output)?;
    Ok(())
}

fn render_preview_once(
    input: &Path,
    t: f32,
    output: &Path,
    cache_dir: &Path,
    composition: Option<&str>,
) -> Result<()> {
    let start = std::time::Instant::now();
    let compiler = compilers::detect(input)?;
    let mut compiled = compiler.compile(input)?;
    let scene = compiled.as_static_mut();
    if let Some(comp_name) = composition {
        scene.apply_profile(comp_name)?;
    }
    capabilities::validate_scene_capabilities(scene)?;
    let assets = AssetStore::load_for_scene_with_base(scene, cache_dir, input.parent())?;
    let rgba = renderer::render_frame_at_time(scene, &assets, t)?;
    let img = premul_to_rgba_image(scene.canvas.width, scene.canvas.height, rgba)?;
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    img.save(output)?;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "Rendered t={:.2}s -> {} ({}x{}) in {:.1}ms",
        t,
        output.display(),
        scene.canvas.width,
        scene.canvas.height,
        elapsed_ms
    );
    Ok(())
}

fn run_watch(
    input: &Path,
    t: f32,
    output: &Path,
    cache_dir: &Path,
    composition: Option<&str>,
) -> Result<()> {
    use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    let abs_input = fs::canonicalize(input)
        .with_context(|| format!("Failed to resolve path: {}", input.display()))?;
    let watch_dir = abs_input
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    println!(
        "Watching {} (t={:.2}s -> {}). Press Ctrl+C to stop.",
        abs_input.display(),
        t,
        output.display()
    );
    if let Err(e) = render_preview_once(&abs_input, t, output, cache_dir, composition) {
        eprintln!("Initial render error: {:#}", e);
    }

    let (tx, rx) = channel();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())
        .context("Failed to initialize file watcher")?;
    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("Failed to watch directory: {}", watch_dir.display()))?;

    let mut last_render = Instant::now() - Duration::from_secs(1);
    for res in rx {
        match res {
            Ok(event) => {
                let touches_target = event.paths.iter().any(|p| p == &abs_input);
                if touches_target && last_render.elapsed() >= Duration::from_millis(80) {
                    last_render = Instant::now();
                    if let Err(e) =
                        render_preview_once(&abs_input, t, output, cache_dir, composition)
                    {
                        eprintln!("Render error: {:#}", e);
                    }
                }
            }
            Err(e) => eprintln!("Watch error: {:?}", e),
        }
    }
    Ok(())
}

#[derive(Debug)]
struct DiffReport {
    mae: f64,
    rmse: f64,
    changed_pct: f64,
}

pub(crate) fn run_diff(
    baseline: &Path,
    current: &Path,
    t: f32,
    _threshold: f32,
    out: Option<&Path>,
    cache_dir: &Path,
) -> Result<DiffReport> {
    let base_compiler = compilers::detect(baseline)?;
    let base_compiled = base_compiler.compile(baseline)?;
    let scene_base = base_compiled.as_static();
    capabilities::validate_scene_capabilities(scene_base)?;
    let assets_base =
        AssetStore::load_for_scene_with_base(scene_base, cache_dir, baseline.parent())?;
    let rgba_base = renderer::render_frame_at_time(scene_base, &assets_base, t)?;

    let curr_compiler = compilers::detect(current)?;
    let curr_compiled = curr_compiler.compile(current)?;
    let scene_curr = curr_compiled.as_static();
    capabilities::validate_scene_capabilities(scene_curr)?;
    let assets_curr =
        AssetStore::load_for_scene_with_base(scene_curr, cache_dir, current.parent())?;
    let rgba_curr = renderer::render_frame_at_time(scene_curr, &assets_curr, t)?;

    if scene_base.canvas.width != scene_curr.canvas.width
        || scene_base.canvas.height != scene_curr.canvas.height
    {
        anyhow::bail!(
            "Canvas dimension mismatch: baseline {}x{} vs current {}x{}",
            scene_base.canvas.width,
            scene_base.canvas.height,
            scene_curr.canvas.width,
            scene_curr.canvas.height
        );
    }

    let img_base =
        premul_to_rgba_image(scene_base.canvas.width, scene_base.canvas.height, rgba_base)?;
    let img_curr =
        premul_to_rgba_image(scene_curr.canvas.width, scene_curr.canvas.height, rgba_curr)?;

    let total_channels = (img_base.width() as u64) * (img_base.height() as u64) * 4;
    let mut abs_sum: u64 = 0;
    let mut sq_sum: u128 = 0;
    let mut diff_pixels: u64 = 0;

    let mut diff_img = image::RgbaImage::new(img_base.width(), img_base.height());
    for (x, y, p_out) in diff_img.enumerate_pixels_mut() {
        let a = img_base.get_pixel(x, y);
        let b = img_curr.get_pixel(x, y);
        let mut px_diff: u32 = 0;
        for c in 0..4 {
            let d = (a[c] as i32 - b[c] as i32).unsigned_abs();
            abs_sum += d as u64;
            sq_sum += (d as u128) * (d as u128);
            px_diff = px_diff.max(d);
        }
        if px_diff > 0 {
            diff_pixels += 1;
            let boost = (px_diff.saturating_mul(4)).min(255) as u8;
            *p_out = image::Rgba([255, 255 - boost, 255 - boost, 255]);
        } else {
            // Dimmed grayscale background for unchanged pixels
            let luma = ((b[0] as u16 + b[1] as u16 + b[2] as u16) / 6) as u8;
            *p_out = image::Rgba([luma, luma, luma, 255]);
        }
    }

    let mae = (abs_sum as f64) / (total_channels as f64 * 255.0);
    let rmse = ((sq_sum as f64) / (total_channels as f64)).sqrt() / 255.0;
    let changed_pct =
        (diff_pixels as f64) * 100.0 / ((img_base.width() as f64) * (img_base.height() as f64));

    if let Some(out_path) = out {
        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        diff_img.save(out_path)?;
    }

    Ok(DiffReport {
        mae,
        rmse,
        changed_pct,
    })
}
