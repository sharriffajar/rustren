use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::schema::{Layer, LayerKind, Scene};

/// Maximum number of decoded frames kept per video. A windowed cache keeps a
/// 1–3 minute clip from being decoded into RAM in full.
const VIDEO_CACHE_MAX_FRAMES: usize = 96;
/// Byte budget for a single video's frame window (anti-OOM guard for 4K clips).
const VIDEO_CACHE_MAX_BYTES: usize = 512 * 1024 * 1024;

#[allow(dead_code)]
enum VideoBackend {
    /// Short clips and unit tests: every frame is already in memory.
    Preloaded(Vec<Arc<Vec<u8>>>),
    /// Real files: decode a window on demand and keep it in `cache`.
    File,
}

#[derive(Default)]
struct FrameCache {
    frames: HashMap<usize, Arc<Vec<u8>>>,
    order: VecDeque<usize>,
    decodes: u64,
}

pub struct VideoAsset {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub has_audio: bool,
    pub path: PathBuf,
    pub frame_count: usize,
    /// Number of frames a windowed decode keeps resident.
    pub cache_capacity: usize,
    backend: VideoBackend,
    cache: Mutex<FrameCache>,
}

impl VideoAsset {
    /// Build an in-memory asset (tests and callers that already decoded a clip).
    #[allow(dead_code)]
    pub fn from_frames(
        width: u32,
        height: u32,
        fps: f32,
        has_audio: bool,
        frames: Vec<Vec<u8>>,
        path: PathBuf,
    ) -> Self {
        let frames: Vec<Arc<Vec<u8>>> = frames.into_iter().map(Arc::new).collect();
        let frame_count = frames.len();
        Self {
            width,
            height,
            fps,
            has_audio,
            path,
            frame_count,
            cache_capacity: VIDEO_CACHE_MAX_FRAMES.min(frame_count.max(1)),
            backend: VideoBackend::Preloaded(frames),
            cache: Mutex::new(FrameCache::default()),
        }
    }

    /// Fetch one decoded frame. File-backed assets decode a deterministic
    /// window around `idx` with ffmpeg and retain it in an LRU window.
    pub fn frame(&self, idx: usize) -> Result<Arc<Vec<u8>>> {
        if self.frame_count == 0 {
            return Err(anyhow!("Video has no frames: {}", self.path.display()));
        }
        let idx = idx.min(self.frame_count - 1);
        match &self.backend {
            VideoBackend::Preloaded(frames) => Ok(frames[idx].clone()),
            VideoBackend::File => self.frame_from_file(idx),
        }
    }

    fn frame_from_file(&self, idx: usize) -> Result<Arc<Vec<u8>>> {
        {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(frame) = cache.frames.get(&idx).cloned() {
                if let Some(pos) = cache.order.iter().position(|&i| i == idx) {
                    cache.order.remove(pos);
                }
                cache.order.push_back(idx);
                return Ok(frame);
            }
        }

        let window = self.cache_capacity.max(1);
        let start = idx - idx % window;
        let count = (self.frame_count - start).min(window);
        let decoded = self.decode_window(start, count)?;

        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.decodes += 1;
        for (offset, data) in decoded.into_iter().enumerate() {
            let frame_idx = start + offset;
            if frame_idx >= self.frame_count {
                break;
            }
            if cache.frames.insert(frame_idx, Arc::new(data)).is_none() {
                cache.order.push_back(frame_idx);
            }
        }
        while cache.order.len() > self.cache_capacity {
            if let Some(old) = cache.order.pop_front() {
                cache.frames.remove(&old);
            }
        }
        cache
            .frames
            .get(&idx)
            .cloned()
            .ok_or_else(|| anyhow!("Failed to decode frame {} of {}", idx, self.path.display()))
    }

    fn decode_window(&self, start: usize, count: usize) -> Result<Vec<Vec<u8>>> {
        let fps = if self.fps > 1e-6 { self.fps } else { 30.0 };
        // Seek half a frame before the window start so the first emitted frame
        // is deterministically `start` (input seeking discards earlier frames).
        let seek = ((start as f64 - 0.5).max(0.0)) / fps as f64;
        let output = Command::new("ffmpeg")
            .args(["-v", "error", "-ss", &format!("{seek:.6}"), "-i"])
            .arg(&self.path)
            .args([
                "-frames:v",
                &count.to_string(),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-",
            ])
            .output()
            .with_context(|| {
                format!(
                    "Failed to decode video window [{start}..{}] from {}",
                    start + count,
                    self.path.display()
                )
            })?;
        if !output.status.success() {
            return Err(anyhow!(
                "ffmpeg video window decode failed on {}: {}",
                self.path.display(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let frame_bytes = (self.width as usize) * (self.height as usize) * 4;
        let frames: Vec<Vec<u8>> = output
            .stdout
            .chunks_exact(frame_bytes)
            .map(|c| c.to_vec())
            .collect();
        if frames.is_empty() {
            return Err(anyhow!(
                "Decoded 0 video frames from {} starting at frame {}",
                self.path.display(),
                start
            ));
        }
        Ok(frames)
    }

    /// Number of frames currently resident in the window cache.
    #[allow(dead_code)]
    pub fn cached_frame_count(&self) -> usize {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .frames
            .len()
    }

    /// Number of ffmpeg window decodes performed so far (test/metrics hook).
    #[allow(dead_code)]
    pub fn decode_count(&self) -> u64 {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .decodes
    }
}

pub struct AssetStore {
    pub fonts: Vec<Vec<u8>>,
    pub images: HashMap<String, image::RgbaImage>,
    pub svgs: HashMap<String, Vec<u8>>,
    pub videos: HashMap<String, VideoAsset>,
    pub audio_paths: Vec<PathBuf>,
    /// Uploaded `.glb`/`.gltf` models keyed by the layer's `src` string, parsed
    /// once at load time instead of per frame.
    #[cfg(feature = "exp-3d")]
    pub mesh3d: HashMap<String, std::sync::Arc<crate::renderer::mesh3d::Mesh>>,
}

/// Convert straight (non-premultiplied) RGBA8 pixels to premultiplied RGBA8 in
/// place.
///
/// Everything the compositor touches is premultiplied: `tiny_skia::Pixmap`
/// documents its data as premultiplied RGBA, and the SIMD image resize runs with
/// `use_alpha(false)` so it filters all four channels linearly (which is only
/// correct for premultiplied data). PNG encoders commonly store a fully
/// transparent pixel as `A=0, RGB=255`; passed through untouched it composites
/// as opaque white and bleeds a halo around logos. Premultiplying maps those
/// pixels to `(0,0,0,0)` — the only valid premultiplied value for `A=0`.
pub fn premultiply_rgba(pixels: &mut [u8]) {
    for px in pixels.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a == 255 {
            continue; // fully opaque: premultiplied == straight
        }
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
            continue;
        }
        let scale = a as f32 / 255.0;
        px[0] = (px[0] as f32 * scale).round() as u8;
        px[1] = (px[1] as f32 * scale).round() as u8;
        px[2] = (px[2] as f32 * scale).round() as u8;
    }
}

/// Resolve a local asset path exactly the way the loader does, for linters.
///
/// Relative paths are tried against the scene file's directory first and then
/// against the process working directory. The first existing candidate wins; if
/// neither exists, the scene-relative path is returned so the warning names the
/// path the author most likely meant.
pub fn resolve_local_asset(src: &str, base_dir: Option<&Path>) -> PathBuf {
    let p = PathBuf::from(src);
    if p.is_absolute() {
        return p;
    }
    if let Some(dir) = base_dir {
        let candidate = dir.join(&p);
        if candidate.exists() {
            return candidate;
        }
    }
    if p.exists() {
        return p;
    }
    match base_dir {
        Some(dir) => dir.join(p),
        None => p,
    }
}

impl AssetStore {
    /// Load every asset referenced by `scene`. Relative asset paths resolve
    /// against `base_dir` (the scene file's directory) first, then the working
    /// directory, so `validate --check-assets`, `snapshot`, `layout`, `diff`
    /// and `render` all agree.
    pub fn load_for_scene_with_base(
        scene: &Scene,
        cache_dir: &Path,
        base_dir: Option<&Path>,
    ) -> Result<Self> {
        fs::create_dir_all(cache_dir).context("Failed to create asset cache directory")?;

        let mut fonts = Vec::with_capacity(scene.fonts.len());
        for font_src in &scene.fonts {
            let path = resolve_to_path_with_base(font_src, cache_dir, base_dir)?;
            let bytes = fs::read(&path)
                .with_context(|| format!("Failed to read font file: {}", path.display()))?;
            fonts.push(bytes);
        }

        let mut images = HashMap::new();
        let mut svgs = HashMap::new();
        let mut videos = HashMap::new();
        collect_layer_assets(
            &scene.layers,
            cache_dir,
            base_dir,
            &mut images,
            &mut svgs,
            &mut videos,
        )?;

        let mut audio_paths = Vec::with_capacity(scene.audio.len());
        for track in &scene.audio {
            let path = resolve_to_path_with_base(&track.src, cache_dir, base_dir)?;
            audio_paths.push(path);
        }

        #[cfg(feature = "exp-3d")]
        let mesh3d = collect_mesh3d_models(&scene.layers, cache_dir, base_dir)?;

        Ok(Self {
            fonts,
            images,
            svgs,
            videos,
            audio_paths,
            #[cfg(feature = "exp-3d")]
            mesh3d,
        })
    }

    /// Load assets relative to the working directory, for callers that render
    /// an already-compiled scene without knowing its source path.
    #[cfg_attr(not(any(feature = "pro", test)), allow(dead_code))]
    pub fn load_for_scene(scene: &Scene, cache_dir: &Path) -> Result<Self> {
        Self::load_for_scene_with_base(scene, cache_dir, None)
    }
}

/// Parse every `Mesh3d.src` model once, resolving relative paths the same way
/// images/videos do. Errors surface at load time, so a missing or corrupt model
/// fails loudly instead of rendering an empty layer.
#[cfg(feature = "exp-3d")]
fn collect_mesh3d_models(
    layers: &[Layer],
    cache_dir: &Path,
    base_dir: Option<&Path>,
) -> Result<HashMap<String, std::sync::Arc<crate::renderer::mesh3d::Mesh>>> {
    let mut out = HashMap::new();
    collect_mesh3d_models_into(layers, cache_dir, base_dir, &mut out)?;
    Ok(out)
}

#[cfg(feature = "exp-3d")]
fn collect_mesh3d_models_into(
    layers: &[Layer],
    cache_dir: &Path,
    base_dir: Option<&Path>,
    out: &mut HashMap<String, std::sync::Arc<crate::renderer::mesh3d::Mesh>>,
) -> Result<()> {
    for layer in layers {
        match &layer.kind {
            LayerKind::Mesh3d { src: Some(src), .. }
                if !src.trim().is_empty() && !out.contains_key(src) =>
            {
                let path = resolve_to_path_with_base(src, cache_dir, base_dir)?;
                let bytes = fs::read(&path)
                    .with_context(|| format!("Failed to read 3D model: {}", path.display()))?;
                let base = path.parent();
                let mesh =
                    crate::renderer::mesh3d::Mesh::from_gltf_bytes_with_base(&bytes, base)
                        .with_context(|| format!("Failed to parse 3D model: {}", path.display()))?;
                out.insert(src.clone(), std::sync::Arc::new(mesh));
            }
            LayerKind::Mesh3d { .. } => {}
            LayerKind::Container { children, .. } => {
                collect_mesh3d_models_into(children, cache_dir, base_dir, out)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_layer_assets(
    layers: &[Layer],
    cache_dir: &Path,
    base_dir: Option<&Path>,
    images: &mut HashMap<String, image::RgbaImage>,
    svgs: &mut HashMap<String, Vec<u8>>,
    videos: &mut HashMap<String, VideoAsset>,
) -> Result<()> {
    for layer in layers {
        match &layer.kind {
            LayerKind::Image { src, .. } => {
                if !images.contains_key(src) {
                    let path = resolve_to_path_with_base(src, cache_dir, base_dir)?;
                    let mut img = image::open(&path)
                        .with_context(|| format!("Failed to decode image: {}", path.display()))?
                        .to_rgba8();
                    // Premultiply once at load time so resize + compositing see
                    // premultiplied pixels (see `premultiply_rgba`).
                    premultiply_rgba(img.as_flat_samples_mut().as_mut_slice());
                    images.insert(src.clone(), img);
                }
            }
            LayerKind::Svg { src, .. } => {
                if !svgs.contains_key(src) {
                    let bytes = if src.trim_start().starts_with('<') {
                        src.as_bytes().to_vec()
                    } else {
                        let path = resolve_to_path_with_base(src, cache_dir, base_dir)?;
                        fs::read(&path)
                            .with_context(|| format!("Failed to read SVG: {}", path.display()))?
                    };
                    svgs.insert(src.clone(), bytes);
                }
            }
            LayerKind::Video { src, .. } => {
                if !videos.contains_key(src) {
                    let path = resolve_to_path_with_base(src, cache_dir, base_dir)?;
                    let asset = load_video_asset(&path)?;
                    videos.insert(src.clone(), asset);
                }
            }
            LayerKind::Container { children, .. } => {
                collect_layer_assets(children, cache_dir, base_dir, images, svgs, videos)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_fps_rational(s: &str) -> f32 {
    if let Some((num, den)) = s.split_once('/') {
        let n: f32 = num.parse().unwrap_or(30.0);
        let d: f32 = den.parse().unwrap_or(1.0);
        if d > 0.0 { n / d } else { 30.0 }
    } else {
        s.parse().unwrap_or(30.0)
    }
}

fn load_video_asset(path: &Path) -> Result<VideoAsset> {
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,width,height,r_frame_rate,avg_frame_rate,nb_frames,duration",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("Failed to run ffprobe on {}", path.display()))?;
    if !probe.status.success() {
        return Err(anyhow!(
            "ffprobe failed on {}: {}",
            path.display(),
            String::from_utf8_lossy(&probe.stderr)
        ));
    }

    let info: serde_json::Value =
        serde_json::from_slice(&probe.stdout).context("Failed to parse ffprobe JSON output")?;
    let streams = info["streams"]
        .as_array()
        .ok_or_else(|| anyhow!("No streams found in {}", path.display()))?;

    let mut width = 0u32;
    let mut height = 0u32;
    let mut fps = 30.0f32;
    let mut has_audio = false;
    let mut has_video = false;
    let mut nb_frames: Option<usize> = None;
    let mut duration: Option<f64> = None;

    for st in streams {
        match st["codec_type"].as_str() {
            Some("video") if !has_video => {
                width = st["width"].as_u64().unwrap_or(0) as u32;
                height = st["height"].as_u64().unwrap_or(0) as u32;
                fps = st["r_frame_rate"]
                    .as_str()
                    .or_else(|| st["avg_frame_rate"].as_str())
                    .map(parse_fps_rational)
                    .filter(|f| f.is_finite() && *f > 0.0)
                    .unwrap_or(30.0);
                nb_frames = st["nb_frames"]
                    .as_str()
                    .and_then(|s| s.parse::<usize>().ok())
                    .or_else(|| st["nb_frames"].as_u64().map(|n| n as usize))
                    .filter(|n| *n > 0);
                duration = st["duration"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok());
                has_video = true;
            }
            Some("audio") => has_audio = true,
            _ => {}
        }
    }
    if duration.is_none() {
        duration = info["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok());
    }

    if !has_video || width == 0 || height == 0 {
        return Err(anyhow!("No valid video stream in {}", path.display()));
    }

    let frame_count = nb_frames
        .or_else(|| {
            duration
                .filter(|d| d.is_finite() && *d > 0.0)
                .map(|d| (d * fps as f64).round().max(1.0) as usize)
        })
        .or_else(|| probe_frame_count(path).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| anyhow!("Could not determine frame count for {}", path.display()))?;

    let frame_bytes = (width as usize) * (height as usize) * 4;
    let byte_capacity = (VIDEO_CACHE_MAX_BYTES / frame_bytes.max(1)).max(1);
    let cache_capacity = VIDEO_CACHE_MAX_FRAMES
        .min(byte_capacity)
        .min(frame_count.max(1));

    Ok(VideoAsset {
        width,
        height,
        fps,
        has_audio,
        path: path.to_path_buf(),
        frame_count,
        cache_capacity,
        backend: VideoBackend::File,
        cache: Mutex::new(FrameCache::default()),
    })
}

/// Last-resort frame count for containers that omit `nb_frames` and duration.
fn probe_frame_count(path: &Path) -> Result<usize> {
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("Failed to run ffprobe -count_frames on {}", path.display()))?;
    if !probe.status.success() {
        return Err(anyhow!(
            "ffprobe -count_frames failed on {}: {}",
            path.display(),
            String::from_utf8_lossy(&probe.stderr)
        ));
    }
    let info: serde_json::Value = serde_json::from_slice(&probe.stdout)
        .context("Failed to parse ffprobe -count_frames JSON output")?;
    let count = info["streams"]
        .as_array()
        .and_then(|s| s.first())
        .and_then(|st| st["nb_read_frames"].as_str())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    if count == 0 {
        Err(anyhow!("No frames counted in {}", path.display()))
    } else {
        Ok(count)
    }
}

/// Resolve a remote URL into the cache, or a local path against the scene
/// file's directory first (see [`resolve_local_asset`]).
pub fn resolve_to_path_with_base(
    src: &str,
    cache_dir: &Path,
    base_dir: Option<&Path>,
) -> Result<PathBuf> {
    if src.starts_with("http://") || src.starts_with("https://") {
        crate::security::validate_remote_url(src)?;
        let mut hasher = Sha256::new();
        hasher.update(src.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        let ext = src
            .split('?')
            .next()
            .and_then(|s| Path::new(s).extension())
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty() && e.len() <= 8 && e.bytes().all(|b| b.is_ascii_alphanumeric()))
            .unwrap_or("bin");
        let cached_path = cache_dir.join(format!("{}.{}", hash, ext));

        if cached_path.exists() {
            return Ok(cached_path);
        }

        let resp = ureq::get(src)
            .call()
            .with_context(|| format!("Failed to download asset: {}", src))?;
        let mut bytes = Vec::new();
        resp.into_reader()
            .take(50 * 1024 * 1024) // 50 MB limit
            .read_to_end(&mut bytes)
            .with_context(|| format!("Failed to read response body: {}", src))?;

        fs::write(&cached_path, &bytes)
            .with_context(|| format!("Failed to write cache file: {}", cached_path.display()))?;
        Ok(cached_path)
    } else {
        Ok(resolve_local_asset(src, base_dir))
    }
}
