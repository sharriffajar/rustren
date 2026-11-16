mod animation;
mod assets;
mod capabilities;
#[cfg(test)]
mod conformance_tests;
mod encoder;
mod lint;
mod renderer;
mod schema;
mod security;
mod server;

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
#[command(name = "rustren", version, about = "Browserless 2D Video Render Engine in Rust")]
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
    },
    /// Validate and lint a scene JSON file without rendering
    Validate {
        /// Path to the scene JSON file
        input: PathBuf,
        /// Also flag non-deterministic patterns (remote URLs, missing bundled fonts, repeat=infinite)
        #[arg(long, default_value_t = false)]
        strict_determinism: bool,
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
    },
    /// Print supported renderer and schema capabilities as JSON
    Capabilities,
    /// Start the HTTP render service and job queue (Phase 3 VPS API)
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
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Capabilities => {
            let reg = capabilities::CapabilityRegistry::current();
            println!("{}", serde_json::to_string_pretty(&reg)?);
        }
        Commands::Validate {
            input,
            strict_determinism,
        } => {
            let raw = fs::read_to_string(&input)
                .with_context(|| format!("Failed to read scene file: {}", input.display()))?;
            lint::validate_no_unknown_fields(&raw)?;
            let scene: Scene = serde_json::from_str(&raw).context("Failed to parse scene JSON")?;
            capabilities::validate_scene_capabilities(&scene)?;
            let mut warnings = collect_lint_warnings(&scene);
            if strict_determinism {
                warnings.extend(lint::collect_determinism_warnings(&scene));
            }
            for w in &warnings {
                eprintln!("warning: {w}");
            }
            println!(
                "rustren: valid JSON (schema v{}, {}x{} @ {}fps, {:.2}s, {} root layers, {} warning(s))",
                scene.schema_version,
                scene.canvas.width,
                scene.canvas.height,
                scene.canvas.fps,
                scene.canvas.duration_secs,
                scene.layers.len(),
                warnings.len()
            );
        }
        Commands::Render {
            input,
            output,
            cache_dir,
            crf,
            preset,
            profile,
        } => {
            let start_time = Instant::now();
            let raw = fs::read_to_string(&input)
                .with_context(|| format!("Failed to read scene file: {}", input.display()))?;
            let scene: Scene =
                serde_json::from_str(&raw).context("Failed to parse scene JSON")?;
            capabilities::validate_scene_capabilities(&scene)?;

            let total_frames = (scene.canvas.duration_secs * scene.canvas.fps as f32).ceil() as u32;
            println!(
                "rustren: rendering {}x{} @ {}fps ({} frames, crf={}, preset={}) -> {}",
                scene.canvas.width,
                scene.canvas.height,
                scene.canvas.fps,
                total_frames,
                crf,
                preset,
                output.display()
            );

            render_scene_to_file_with_profile(
                &scene,
                &output,
                Path::new(&cache_dir),
                crf,
                &preset,
                profile,
            )?;
            println!(
                "rustren: done in {:.2}s -> {}",
                start_time.elapsed().as_secs_f32(),
                output.display()
            );
        }
        Commands::Snapshot {
            input,
            t,
            output,
            sheet,
            cols,
            cache_dir,
        } => {
            let start_time = Instant::now();
            let raw = fs::read_to_string(&input)
                .with_context(|| format!("Failed to read scene file: {}", input.display()))?;
            let scene: Scene =
                serde_json::from_str(&raw).context("Failed to parse scene JSON")?;
            capabilities::validate_scene_capabilities(&scene)?;
            let assets = AssetStore::load_for_scene(&scene, Path::new(&cache_dir))?;
            render_snapshots(&scene, &assets, &t, &output, sheet, cols)?;
            println!(
                "rustren: snapshot(s) rendered in {:.2}s -> {}",
                start_time.elapsed().as_secs_f32(),
                output.display()
            );
        }
        Commands::Layout {
            input,
            t,
            cache_dir,
        } => {
            let raw = fs::read_to_string(&input)
                .with_context(|| format!("Failed to read scene file: {}", input.display()))?;
            let scene: Scene =
                serde_json::from_str(&raw).context("Failed to parse scene JSON")?;
            capabilities::validate_scene_capabilities(&scene)?;
            let assets = AssetStore::load_for_scene(&scene, Path::new(&cache_dir))?;
            let layout = renderer::inspect_layout_at_time(&scene, &assets, t)?;
            println!("{}", serde_json::to_string_pretty(&layout)?);
        }
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
        } => {
            run_watch(&input, t, &output, &cache_dir)?;
        }
        Commands::Diff {
            baseline,
            current,
            t,
            threshold,
            out,
            cache_dir,
        } => {
            run_diff(&baseline, &current, t, threshold, out.as_deref(), &cache_dir)?;
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
            schema::TimelineMode::Series => (series_cursor + layer.start_sec - layer.overlap_secs).max(0.0),
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
                if let Some(va) = assets.videos.get(src) {
                    if va.has_audio {
                        let effective_trim_end = trim_end_sec.or_else(|| {
                            Some(trim_start_sec + layer_dur * playback_rate.max(0.1))
                        });
                        out.push((
                            schema::AudioTrack {
                                src: src.clone(),
                                start_sec: global_start,
                                volume: *volume,
                                trim_start_sec: Some(*trim_start_sec),
                                trim_end_sec: effective_trim_end,
                                playback_rate: *playback_rate,
                            },
                            va.path.clone(),
                        ));
                    }
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
                collect_video_audio_tracks(children, *child_tl, child_offset, layer_dur, assets, out);
            }
            _ => {}
        }
    }
}

pub fn render_scene_to_file(scene: &Scene, output: &Path, cache_dir: &Path) -> Result<()> {
    render_scene_to_file_with_options(scene, output, cache_dir, 18, "medium")
}

pub fn render_scene_to_file_with_options(
    scene: &Scene,
    output: &Path,
    cache_dir: &Path,
    crf: u8,
    preset: &str,
) -> Result<()> {
    render_scene_to_file_with_profile(scene, output, cache_dir, crf, preset, false)
}

pub fn render_scene_to_file_with_profile(
    scene: &Scene,
    output: &Path,
    cache_dir: &Path,
    crf: u8,
    preset: &str,
    profile: bool,
) -> Result<()> {
    if profile {
        renderer::reset_cache_stats();
    }
    let assets = AssetStore::load_for_scene(scene, cache_dir)?;
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

    // Render in small parallel batches to keep RAM usage low
    let batch_size = rayon::current_num_threads().max(4) as u32;
    for batch_start in (0..total_frames).step_by(batch_size as usize) {
        let batch_end = (batch_start + batch_size).min(total_frames);
        let frames: Vec<(Vec<u8>, Vec<u64>)> = (batch_start..batch_end)
            .into_par_iter()
            .map(|idx| {
                if profile {
                    renderer::render_frame_with_base_profiled(
                        scene,
                        &assets,
                        idx,
                        static_prefix_len,
                        static_base.as_ref(),
                    )
                } else {
                    renderer::render_frame_with_base(
                        scene,
                        &assets,
                        idx,
                        static_prefix_len,
                        static_base.as_ref(),
                    )
                    .map(|b| (b, Vec::new()))
                }
            })
            .collect::<Result<Vec<_>>>()?;

        for (frame, lns) in frames {
            if profile {
                for (i, ns) in lns.into_iter().enumerate() {
                    total_layer_ns[i] += ns;
                }
            }
            pipe.write_frame(&frame)?;
        }
    }

    pipe.finish()?;
    if profile {
        let (svg_hits, svg_misses, blur_hits, blur_misses) = renderer::get_cache_stats();
        println!("--- rustren profile ---");
        println!("static_prefix_skipped: {static_prefix_len}/{} root layers", scene.layers.len());
        println!(
            "svg_raster_cache:  hits={svg_hits}, misses={svg_misses} ({:.1}% hit)",
            if svg_hits + svg_misses > 0 {
                svg_hits as f64 * 100.0 / (svg_hits + svg_misses) as f64
            } else {
                0.0
            }
        );
        println!(
            "blur_cache:        hits={blur_hits}, misses={blur_misses} ({:.1}% hit)",
            if blur_hits + blur_misses > 0 {
                blur_hits as f64 * 100.0 / (blur_hits + blur_misses) as f64
            } else {
                0.0
            }
        );
        let mut ranked: Vec<(usize, u64)> = total_layer_ns.into_iter().enumerate().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        println!("top root layers (cumulative CPU time across {total_frames} frames):");
        for (idx, ns) in ranked.into_iter().filter(|(_, ns)| *ns > 0).take(10) {
            let name = scene.layers[idx]
                .name
                .as_deref()
                .unwrap_or("(unnamed)");
            println!("  layers[{idx}] ({name}): {:.2}ms", ns as f64 / 1_000_000.0);
        }
    }
    Ok(())
}

fn premul_to_rgba_image(w: u32, h: u32, mut data: Vec<u8>) -> Result<image::RgbaImage> {
    for px in data.chunks_exact_mut(4) {
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

fn render_snapshots(
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

    if sheet || frames.len() > 1 && output.extension().and_then(|e| e.to_str()) == Some("png") && sheet {
        // handled below
    }

    if !sheet && frames.len() == 1 {
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
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
            let ext = output
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png");
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
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    canvas.save(output)?;
    Ok(())
}

fn render_preview_once(input: &Path, t: f32, output: &Path, cache_dir: &Path) -> Result<()> {
    let start = std::time::Instant::now();
    let raw = fs::read_to_string(input)
        .with_context(|| format!("Failed to read scene file: {}", input.display()))?;
    lint::validate_no_unknown_fields(&raw)?;
    let scene: Scene = serde_json::from_str(&raw).context("Failed to parse scene JSON")?;
    capabilities::validate_scene_capabilities(&scene)?;
    let assets = AssetStore::load_for_scene(&scene, cache_dir)?;
    let rgba = renderer::render_frame_at_time(&scene, &assets, t)?;
    let img = premul_to_rgba_image(scene.canvas.width, scene.canvas.height, rgba)?;
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
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

fn run_watch(input: &Path, t: f32, output: &Path, cache_dir: &Path) -> Result<()> {
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
    if let Err(e) = render_preview_once(&abs_input, t, output, cache_dir) {
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
                    if let Err(e) = render_preview_once(&abs_input, t, output, cache_dir) {
                        eprintln!("Render error: {:#}", e);
                    }
                }
            }
            Err(e) => eprintln!("Watch error: {:?}", e),
        }
    }
    Ok(())
}

fn run_diff(
    baseline: &Path,
    current: &Path,
    t: f32,
    threshold: f32,
    out: Option<&Path>,
    cache_dir: &Path,
) -> Result<()> {
    let raw_base = fs::read_to_string(baseline)
        .with_context(|| format!("Failed to read baseline: {}", baseline.display()))?;
    let scene_base: Scene =
        serde_json::from_str(&raw_base).context("Failed to parse baseline JSON")?;
    capabilities::validate_scene_capabilities(&scene_base)?;
    let assets_base = AssetStore::load_for_scene(&scene_base, cache_dir)?;
    let rgba_base = renderer::render_frame_at_time(&scene_base, &assets_base, t)?;

    let raw_curr = fs::read_to_string(current)
        .with_context(|| format!("Failed to read current: {}", current.display()))?;
    let scene_curr: Scene =
        serde_json::from_str(&raw_curr).context("Failed to parse current JSON")?;
    capabilities::validate_scene_capabilities(&scene_curr)?;
    let assets_curr = AssetStore::load_for_scene(&scene_curr, cache_dir)?;
    let rgba_curr = renderer::render_frame_at_time(&scene_curr, &assets_curr, t)?;

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

    let img_base = premul_to_rgba_image(scene_base.canvas.width, scene_base.canvas.height, rgba_base)?;
    let img_curr = premul_to_rgba_image(scene_curr.canvas.width, scene_curr.canvas.height, rgba_curr)?;

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
    let changed_pct = (diff_pixels as f64) * 100.0 / ((img_base.width() as f64) * (img_base.height() as f64));

    if let Some(out_path) = out {
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        diff_img.save(out_path)?;
    }

    println!(
        "Diff at t={:.2}s: MAE={:.6} RMSE={:.6} changed_pixels={:.2}% (threshold={:.6})",
        t, mae, rmse, changed_pct, threshold
    );

    if mae as f32 > threshold {
        anyhow::bail!(
            "Visual regression detected: MAE {:.6} > threshold {:.6}",
            mae,
            threshold
        );
    }

    Ok(())
}
