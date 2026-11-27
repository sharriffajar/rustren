use anyhow::{Context, Result, anyhow};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::schema::AudioTrack;

pub struct FfmpegPipe {
    child: Child,
    stdin: ChildStdin,
}

/// Partition host vCPUs between Rayon frame rendering and FFmpeg x264 encoding
/// so both stages run in parallel without thread-oversubscription thrashing:
/// - 4 vCPU -> (3 rayon, 1 ffmpeg)
/// - 8 vCPU -> (6 rayon, 2 ffmpeg)
pub fn compute_vcpu_partition(total_cpus: usize) -> (usize, usize) {
    match total_cpus {
        0..=2 => (1, 1),
        3 => (2, 1),
        4 => (3, 1),
        5..=8 => (total_cpus - 2, 2),
        n => {
            let ffmpeg = (n / 4).clamp(2, 4);
            (n - ffmpeg, ffmpeg)
        }
    }
}

/// Probe whether an FFmpeg hardware video encoder actually works on this host
/// by encoding a single tiny black frame to null.
fn probe_hw_encoder(encoder: &str) -> bool {
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:d=0.04",
            "-frames:v",
            "1",
            "-c:v",
            encoder,
            "-f",
            "null",
            "-",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Detect a working H.264 hardware encoder by default (`auto`), falling back
/// cleanly to `libx264` when no GPU encoder is usable.
/// Set `RUSTREN_HW_ENCODER=off` (or `0`/`cpu`) to force `libx264`, or an explicit
/// encoder name (`h264_nvenc`, `h264_videotoolbox`, `h264_qsv`).
fn detect_video_encoder() -> &'static str {
    static CACHED: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    CACHED.get_or_init(|| {
        let mode = std::env::var("RUSTREN_HW_ENCODER").unwrap_or_default();
        let mode_trim = mode.trim();
        if mode_trim == "0"
            || mode_trim.eq_ignore_ascii_case("off")
            || mode_trim.eq_ignore_ascii_case("cpu")
            || mode_trim.eq_ignore_ascii_case("libx264")
        {
            return "libx264";
        }
        if mode_trim.eq_ignore_ascii_case("h264_nvenc") {
            return "h264_nvenc";
        }
        if mode_trim.eq_ignore_ascii_case("h264_videotoolbox") {
            return "h264_videotoolbox";
        }
        if mode_trim.eq_ignore_ascii_case("h264_qsv") {
            return "h264_qsv";
        }
        if let Ok(out) = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            for candidate in ["h264_nvenc", "h264_videotoolbox", "h264_qsv"] {
                if text.contains(candidate) && probe_hw_encoder(candidate) {
                    return candidate;
                }
            }
        }
        "libx264"
    })
}

const VALID_X264_PRESETS: &[&str] = &[
    "ultrafast",
    "superfast",
    "veryfast",
    "faster",
    "fast",
    "medium",
    "slow",
    "slower",
    "veryslow",
    "placebo",
];

impl FfmpegPipe {
    pub fn spawn(
        width: u32,
        height: u32,
        fps: u32,
        audio_tracks: &[(AudioTrack, PathBuf)],
        output: &Path,
        crf: u8,
        preset: &str,
    ) -> Result<Self> {
        if crf > 51 {
            return Err(anyhow!("Invalid CRF {}: must be between 0 and 51", crf));
        }
        if !VALID_X264_PRESETS.contains(&preset) {
            return Err(anyhow!(
                "Invalid x264 preset '{}': expected one of {:?}",
                preset,
                VALID_X264_PRESETS
            ));
        }
        let size_arg = format!("{}x{}", width, height);
        let fps_arg = fps.to_string();
        let input_pix_fmt =
            if cfg!(feature = "pro") && width.is_multiple_of(2) && height.is_multiple_of(2) {
                "yuv420p"
            } else {
                "rgba"
            };

        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            input_pix_fmt,
            "-s",
            &size_arg,
            "-r",
            &fps_arg,
            "-i",
            "pipe:0",
        ]);

        for (track, path) in audio_tracks {
            // Fail before spawning ffmpeg: a missing audio file otherwise
            // surfaces as a broken pipe several frames later, which points
            // agents at the encoder instead of the asset.
            if !path.exists() {
                return Err(anyhow!(
                    "audio file not found: {} (src: {:?}); create the file or fix audio[].src",
                    path.display(),
                    track.src
                ));
            }
            if let Some(ss) = track.trim_start_sec {
                cmd.args(["-ss", &format!("{:.3}", ss)]);
            }
            if let Some(to) = track.trim_end_sec {
                cmd.args(["-to", &format!("{:.3}", to)]);
            }
            cmd.arg("-i").arg(path);
        }

        if !audio_tracks.is_empty() {
            let mut filters = Vec::new();
            let mut mix_inputs = Vec::new();
            for (i, (track, _)) in audio_tracks.iter().enumerate() {
                let input_idx = i + 1;
                let delay_ms = (track.start_sec.max(0.0) * 1000.0).round() as u64;
                let label = format!("[a{}]", i);
                let mut tempo_filters = Vec::new();
                let mut rate = track.playback_rate.max(0.1);
                if (rate - 1.0).abs() > 1e-3 {
                    while rate > 2.0 {
                        tempo_filters.push("atempo=2.0".to_string());
                        rate /= 2.0;
                    }
                    while rate < 0.5 {
                        tempo_filters.push("atempo=0.5".to_string());
                        rate *= 2.0;
                    }
                    tempo_filters.push(format!("atempo={:.4}", rate));
                }
                let tempo_prefix = if tempo_filters.is_empty() {
                    String::new()
                } else {
                    format!("{},", tempo_filters.join(","))
                };
                let mut extra_filters = Vec::new();
                if track.fade_in_secs > 0.0 {
                    extra_filters.push(format!(
                        "afade=t=in:st={:.3}:d={:.3}",
                        track.start_sec.max(0.0),
                        track.fade_in_secs
                    ));
                }
                if track.fade_out_secs > 0.0 {
                    let end_t = track
                        .trim_end_sec
                        .unwrap_or(track.start_sec + track.fade_out_secs);
                    let fade_start = (end_t - track.fade_out_secs).max(0.0);
                    extra_filters.push(format!(
                        "afade=t=out:st={:.3}:d={:.3}",
                        fade_start, track.fade_out_secs
                    ));
                }
                let vol_anims: Vec<_> = track
                    .animations
                    .iter()
                    .filter(|a| a.property == crate::schema::AnimProperty::Volume)
                    .collect();
                if vol_anims.is_empty() {
                    extra_filters.push(format!("volume={:.3}", track.volume));
                } else {
                    let mut expr = format!("{:.3}", track.volume);
                    for anim in vol_anims.iter().rev() {
                        let s = anim.start_sec;
                        let e = anim.end_sec.max(s + 0.001);
                        let from = match anim.from {
                            crate::schema::AnimValue::Number(v) => v,
                            _ => track.volume,
                        };
                        let to = match anim.to {
                            crate::schema::AnimValue::Number(v) => v,
                            _ => track.volume,
                        };
                        expr = format!(
                            "if(between(t\\,{s:.3}\\,{e:.3})\\,{from:.3}+({to:.3}-{from:.3})*(t-{s:.3})/({e:.3}-{s:.3})\\,{expr})"
                        );
                    }
                    extra_filters.push(format!("volume='{}':eval=frame", expr));
                }

                filters.push(format!(
                    "[{}:a]{}adelay={}:all=1,{}{}",
                    input_idx,
                    tempo_prefix,
                    delay_ms,
                    extra_filters.join(","),
                    label
                ));
                mix_inputs.push(label);
            }

            let filter_complex = if audio_tracks.len() == 1 {
                filters[0].replace("[a0]", "[aout]")
            } else {
                format!(
                    "{};{}amix=inputs={}:duration=longest:normalize=0[aout]",
                    filters.join(";"),
                    mix_inputs.join(""),
                    audio_tracks.len()
                )
            };

            cmd.args([
                "-filter_complex",
                &filter_complex,
                "-map",
                "0:v",
                "-map",
                "[aout]",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
            ]);
        }

        let crf_arg = crf.to_string();
        let vcodec = detect_video_encoder();
        cmd.args([
            "-sws_flags",
            "fast_bilinear",
            "-c:v",
            vcodec,
            "-pix_fmt",
            "yuv420p",
        ]);
        let (_, ffmpeg_threads) = compute_vcpu_partition(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
        );
        let ffmpeg_threads_arg = ffmpeg_threads.to_string();
        match vcodec {
            "h264_nvenc" => {
                cmd.args(["-preset", "p4", "-cq", &crf_arg]);
            }
            "h264_qsv" => {
                cmd.args(["-preset", "faster", "-global_quality", &crf_arg]);
            }
            "h264_videotoolbox" => {
                cmd.args(["-q:v", "65"]);
            }
            _ => {
                cmd.args([
                    "-threads",
                    &ffmpeg_threads_arg,
                    "-preset",
                    preset,
                    "-crf",
                    &crf_arg,
                ]);
            }
        }
        cmd.arg(output)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .context("Failed to spawn ffmpeg. Is ffmpeg installed and in PATH?")?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to open ffmpeg stdin"))?;

        Ok(Self { child, stdin })
    }

    pub fn write_frame(&mut self, rgba: &[u8]) -> Result<()> {
        self.stdin
            .write_all(rgba)
            .context("Failed to write RGBA frame to ffmpeg stdin")?;
        Ok(())
    }

    pub fn finish(self) -> Result<()> {
        drop(self.stdin);
        let output = self
            .child
            .wait_with_output()
            .context("Failed to wait for ffmpeg process")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("ffmpeg exited with error:\n{}", stderr));
        }
        Ok(())
    }
}

/// Detect audio beat timestamps (in seconds) from an audio/video file using FFmpeg mono PCM + energy peak detection.
#[allow(dead_code)]
pub fn detect_audio_beats(audio_path: &Path, sensitivity: f32) -> Result<Vec<f32>> {
    let sample_rate = 22050usize;
    let output = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-i",
            &audio_path.to_string_lossy(),
            "-ac",
            "1",
            "-ar",
            &sample_rate.to_string(),
            "-f",
            "f32le",
            "-",
        ])
        .output()
        .context("Failed to run ffmpeg for audio beat detection")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg beat detection failed: {}", stderr);
    }

    let mut samples = Vec::with_capacity(output.stdout.len() / 4);
    for chunk in output.stdout.as_chunks::<4>().0 {
        samples.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }

    Ok(detect_beats_from_pcm(&samples, sample_rate, sensitivity))
}

/// Pure-Rust RMS energy peak detector over mono f32 PCM samples.
pub fn detect_beats_from_pcm(samples: &[f32], sample_rate: usize, sensitivity: f32) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 {
        return Vec::new();
    }
    let window_size = (sample_rate / 20).max(1); // 50ms windows
    let energies: Vec<f32> = samples
        .chunks(window_size)
        .map(|w| {
            let sum_sq: f32 = w.iter().map(|&s| s * s).sum();
            (sum_sq / w.len() as f32).sqrt()
        })
        .collect();

    if energies.is_empty() {
        return Vec::new();
    }

    let sens = sensitivity.max(1.05);
    let mut beats = Vec::new();
    let cooldown_windows = 4; // 200ms minimum between beats
    let mut last_beat_idx: Option<usize> = None;

    for i in 0..energies.len() {
        let start = i.saturating_sub(10);
        let end = (i + 11).min(energies.len());
        let local_avg: f32 = energies[start..end].iter().sum::<f32>() / (end - start) as f32;

        if energies[i] > local_avg * sens && energies[i] > 0.02 {
            let ok_cooldown = match last_beat_idx {
                Some(prev) => i - prev >= cooldown_windows,
                None => true,
            };
            if ok_cooldown {
                beats.push((i * window_size) as f32 / sample_rate as f32);
                last_beat_idx = Some(i);
            }
        }
    }
    beats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_beats_from_pcm_finds_peaks() {
        let sr = 22050usize;
        let mut samples = vec![0.01f32; sr * 2]; // 2 seconds of quiet background
        // Inject strong beat bursts at t=0.5s and t=1.2s
        for idx in (sr / 2)..(sr / 2 + 1000) {
            samples[idx] = 0.9;
        }
        for idx in (sr * 12 / 10)..(sr * 12 / 10 + 1000) {
            samples[idx] = 0.9;
        }
        let beats = detect_beats_from_pcm(&samples, sr, 1.5);
        assert_eq!(beats.len(), 2);
        assert!((beats[0] - 0.5).abs() < 0.06);
        assert!((beats[1] - 1.2).abs() < 0.06);
    }
}
