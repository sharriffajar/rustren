use anyhow::{anyhow, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::schema::AudioTrack;

pub struct FfmpegPipe {
    child: Child,
    stdin: ChildStdin,
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

        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            &size_arg,
            "-r",
            &fps_arg,
            "-i",
            "pipe:0",
        ]);

        for (track, path) in audio_tracks {
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
                filters.push(format!(
                    "[{}:a]{}adelay={}:all=1,volume={:.3}{}",
                    input_idx, tempo_prefix, delay_ms, track.volume, label
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
        cmd.args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            preset,
            "-crf",
            &crf_arg,
        ])
        .arg(output)
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
