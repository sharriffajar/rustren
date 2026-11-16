use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::schema::{Layer, LayerKind, Scene};

pub struct VideoAsset {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub has_audio: bool,
    pub frames: Vec<Vec<u8>>,
    pub path: PathBuf,
}

pub struct AssetStore {
    pub fonts: Vec<Vec<u8>>,
    pub images: HashMap<String, image::RgbaImage>,
    pub svgs: HashMap<String, Vec<u8>>,
    pub videos: HashMap<String, VideoAsset>,
    pub audio_paths: Vec<PathBuf>,
}

impl AssetStore {
    pub fn load_for_scene(scene: &Scene, cache_dir: &Path) -> Result<Self> {
        fs::create_dir_all(cache_dir).context("Failed to create asset cache directory")?;

        let mut fonts = Vec::with_capacity(scene.fonts.len());
        for font_src in &scene.fonts {
            let path = resolve_to_path(font_src, cache_dir)?;
            let bytes = fs::read(&path)
                .with_context(|| format!("Failed to read font file: {}", path.display()))?;
            fonts.push(bytes);
        }

        let mut images = HashMap::new();
        let mut svgs = HashMap::new();
        let mut videos = HashMap::new();
        collect_layer_assets(&scene.layers, cache_dir, &mut images, &mut svgs, &mut videos)?;

        let mut audio_paths = Vec::with_capacity(scene.audio.len());
        for track in &scene.audio {
            let path = resolve_to_path(&track.src, cache_dir)?;
            audio_paths.push(path);
        }

        Ok(Self {
            fonts,
            images,
            svgs,
            videos,
            audio_paths,
        })
    }
}

fn collect_layer_assets(
    layers: &[Layer],
    cache_dir: &Path,
    images: &mut HashMap<String, image::RgbaImage>,
    svgs: &mut HashMap<String, Vec<u8>>,
    videos: &mut HashMap<String, VideoAsset>,
) -> Result<()> {
    for layer in layers {
        match &layer.kind {
            LayerKind::Image { src, .. } => {
                if !images.contains_key(src) {
                    let path = resolve_to_path(src, cache_dir)?;
                    let img = image::open(&path)
                        .with_context(|| format!("Failed to decode image: {}", path.display()))?
                        .to_rgba8();
                    images.insert(src.clone(), img);
                }
            }
            LayerKind::Svg { src, .. } => {
                if !svgs.contains_key(src) {
                    let bytes = if src.trim_start().starts_with('<') {
                        src.as_bytes().to_vec()
                    } else {
                        let path = resolve_to_path(src, cache_dir)?;
                        fs::read(&path)
                            .with_context(|| format!("Failed to read SVG: {}", path.display()))?
                    };
                    svgs.insert(src.clone(), bytes);
                }
            }
            LayerKind::Video { src, .. } => {
                if !videos.contains_key(src) {
                    let path = resolve_to_path(src, cache_dir)?;
                    let asset = load_video_asset(&path)?;
                    videos.insert(src.clone(), asset);
                }
            }
            LayerKind::Container { children, .. } => {
                collect_layer_assets(children, cache_dir, images, svgs, videos)?;
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
            "stream=codec_type,width,height,r_frame_rate",
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

    let info: serde_json::Value = serde_json::from_slice(&probe.stdout)
        .context("Failed to parse ffprobe JSON output")?;
    let streams = info["streams"]
        .as_array()
        .ok_or_else(|| anyhow!("No streams found in {}", path.display()))?;

    let mut width = 0u32;
    let mut height = 0u32;
    let mut fps = 30.0f32;
    let mut has_audio = false;
    let mut has_video = false;

    for st in streams {
        match st["codec_type"].as_str() {
            Some("video") if !has_video => {
                width = st["width"].as_u64().unwrap_or(0) as u32;
                height = st["height"].as_u64().unwrap_or(0) as u32;
                fps = st["r_frame_rate"]
                    .as_str()
                    .map(parse_fps_rational)
                    .unwrap_or(30.0);
                has_video = true;
            }
            Some("audio") => has_audio = true,
            _ => {}
        }
    }

    if !has_video || width == 0 || height == 0 {
        return Err(anyhow!("No valid video stream in {}", path.display()));
    }

    let decode = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .output()
        .with_context(|| format!("Failed to decode video frames from {}", path.display()))?;
    if !decode.status.success() {
        return Err(anyhow!(
            "ffmpeg video decode failed on {}: {}",
            path.display(),
            String::from_utf8_lossy(&decode.stderr)
        ));
    }

    let frame_bytes = (width as usize) * (height as usize) * 4;
    let frames: Vec<Vec<u8>> = decode
        .stdout
        .chunks_exact(frame_bytes)
        .map(|c| c.to_vec())
        .collect();
    if frames.is_empty() {
        return Err(anyhow!("Decoded 0 video frames from {}", path.display()));
    }

    Ok(VideoAsset {
        width,
        height,
        fps,
        has_audio,
        frames,
        path: path.to_path_buf(),
    })
}

pub fn resolve_to_path(src: &str, cache_dir: &Path) -> Result<PathBuf> {
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
        Ok(PathBuf::from(src))
    }
}
