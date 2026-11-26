use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct TimeSpan {
    pub start_sec: f64,
    pub end_sec: f64,
    pub duration_sec: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct QaIssue {
    pub severity: &'static str, // "fail" | "warn"
    pub category: &'static str, // "eyes" | "ears" | "container"
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QaReport {
    pub file: String,
    pub verdict: &'static str, // "PASS" | "WARN" | "FAIL"
    pub width: u32,
    pub height: u32,
    pub duration_sec: f64,
    pub has_audio: bool,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub integrated_lufs: Option<f64>,
    pub true_peak_dbtp: Option<f64>,
    pub black_segments: Vec<TimeSpan>,
    pub freeze_segments: Vec<TimeSpan>,
    pub issues: Vec<QaIssue>,
}

pub fn run_qa(video_path: &Path, json_output: bool) -> Result<QaReport> {
    if !video_path.exists() {
        bail!("Video file not found: {}", video_path.display());
    }

    // 1. Probe container & stream metadata via ffprobe
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(video_path)
        .output()
        .context("Failed to execute ffprobe (is ffmpeg/ffprobe installed?)")?;
    if !probe.status.success() {
        bail!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&probe.stderr).trim()
        );
    }
    let meta: serde_json::Value = serde_json::from_slice(&probe.stdout)
        .context("Failed to parse ffprobe JSON output")?;

    let mut width = 0u32;
    let mut height = 0u32;
    let mut has_audio = false;
    let mut video_codec = None;
    let mut audio_codec = None;
    if let Some(streams) = meta.get("streams").and_then(|s| s.as_array()) {
        for s in streams {
            match s.get("codec_type").and_then(|v| v.as_str()) {
                Some("video") if video_codec.is_none() => {
                    width = s.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    height = s.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    video_codec = s
                        .get("codec_name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
                Some("audio") if audio_codec.is_none() => {
                    has_audio = true;
                    audio_codec = s
                        .get("codec_name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
                _ => {}
            }
        }
    }
    let duration_sec = meta
        .pointer("/format/duration")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    // 2. Eyes pass: blackdetect + freezedetect via ffmpeg
    let eyes_out = Command::new("ffmpeg")
        .args(["-hide_banner", "-i"])
        .arg(video_path)
        .args([
            "-vf",
            "blackdetect=d=0.1:pix_th=0.10,freezedetect=n=-60dB:d=2.0",
            "-an",
            "-f",
            "null",
            "-",
        ])
        .output()
        .context("Failed to execute ffmpeg eyes pass")?;
    let eyes_stderr = String::from_utf8_lossy(&eyes_out.stderr);
    let black_segments = parse_blackdetect(&eyes_stderr);
    let freeze_segments = parse_freezedetect(&eyes_stderr);

    // 3. Ears pass: EBU R128 loudness & true-peak clipping
    let mut integrated_lufs = None;
    let mut true_peak_dbtp = None;
    if has_audio {
        let ears_out = Command::new("ffmpeg")
            .args(["-hide_banner", "-i"])
            .arg(video_path)
            .args(["-vn", "-af", "ebur128=peak=true", "-f", "null", "-"])
            .output()
            .context("Failed to execute ffmpeg ears pass")?;
        let ears_stderr = String::from_utf8_lossy(&ears_out.stderr);
        let (lufs, peak) = parse_ebur128_summary(&ears_stderr);
        integrated_lufs = lufs;
        true_peak_dbtp = peak;
    }

    // 4. Evaluate issues & verdict
    let mut issues = Vec::new();
    if width == 0 || height == 0 || duration_sec <= 0.0 {
        issues.push(QaIssue {
            severity: "fail",
            category: "container",
            code: "invalid_stream",
            message: format!(
                "Invalid video stream ({}x{}, {:.2}s).",
                width, height, duration_sec
            ),
        });
    }
    for seg in &black_segments {
        if seg.start_sec <= 0.05 {
            issues.push(QaIssue {
                severity: "warn",
                category: "eyes",
                code: "black_frame_zero",
                message: format!(
                    "Video starts with black frames (0.00s..{:.2}s, {:.2}s).",
                    seg.end_sec, seg.duration_sec
                ),
            });
        } else {
            issues.push(QaIssue {
                severity: "warn",
                category: "eyes",
                code: "black_segment",
                message: format!(
                    "Black segment detected ({:.2}s..{:.2}s, {:.2}s).",
                    seg.start_sec, seg.end_sec, seg.duration_sec
                ),
            });
        }
    }
    for seg in &freeze_segments {
        issues.push(QaIssue {
            severity: "warn",
            category: "eyes",
            code: "frozen_frames",
            message: format!(
                "Frozen video segment ({:.2}s..{:.2}s, {:.2}s).",
                seg.start_sec, seg.end_sec, seg.duration_sec
            ),
        });
    }
    if !has_audio {
        issues.push(QaIssue {
            severity: "warn",
            category: "ears",
            code: "silent_video",
            message: "No audio stream detected (silent video).".to_string(),
        });
    } else {
        if let Some(peak) = true_peak_dbtp {
            if peak > -1.0 {
                issues.push(QaIssue {
                    severity: "warn",
                    category: "ears",
                    code: "audio_clipping",
                    message: format!(
                        "True peak {:.1} dBTP exceeds -1.0 dBTP (clipping risk).",
                        peak
                    ),
                });
            }
        }
        if let Some(lufs) = integrated_lufs {
            if lufs < -28.0 {
                issues.push(QaIssue {
                    severity: "warn",
                    category: "ears",
                    code: "audio_too_quiet",
                    message: format!(
                        "Integrated loudness {:.1} LUFS is very quiet (target -14 to -18 LUFS).",
                        lufs
                    ),
                });
            }
        }
    }

    let verdict = if issues.iter().any(|i| i.severity == "fail") {
        "FAIL"
    } else if !issues.is_empty() {
        "WARN"
    } else {
        "PASS"
    };

    let report = QaReport {
        file: video_path.display().to_string(),
        verdict,
        width,
        height,
        duration_sec: (duration_sec * 100.0).round() / 100.0,
        has_audio,
        video_codec,
        audio_codec,
        integrated_lufs,
        true_peak_dbtp,
        black_segments,
        freeze_segments,
        issues,
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "rustren qa: {} [{}]",
            report.file, report.verdict
        );
        println!(
            "  video: {}x{} · {:.2}s · codec={}",
            report.width,
            report.height,
            report.duration_sec,
            report.video_codec.as_deref().unwrap_or("none")
        );
        if report.has_audio {
            println!(
                "  audio: codec={} · loudness={} · true_peak={}",
                report.audio_codec.as_deref().unwrap_or("unknown"),
                report
                    .integrated_lufs
                    .map(|v| format!("{v:.1} LUFS"))
                    .unwrap_or_else(|| "n/a".into()),
                report
                    .true_peak_dbtp
                    .map(|v| format!("{v:.1} dBTP"))
                    .unwrap_or_else(|| "n/a".into())
            );
        } else {
            println!("  audio: none");
        }
        if report.issues.is_empty() {
            println!("  checks: all eyes/ears QA checks passed.");
        } else {
            for issue in &report.issues {
                println!(
                    "  [{}] ({}/{}) {}",
                    issue.severity.to_uppercase(),
                    issue.category,
                    issue.code,
                    issue.message
                );
            }
        }
    }

    Ok(report)
}

fn parse_kv_f64(line: &str, key: &str) -> Option<f64> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let token = rest.split_whitespace().next()?;
    token.parse::<f64>().ok()
}

pub fn parse_blackdetect(stderr: &str) -> Vec<TimeSpan> {
    let mut out = Vec::new();
    for line in stderr.lines() {
        if line.contains("black_start:") {
            if let (Some(s), Some(e), Some(d)) = (
                parse_kv_f64(line, "black_start:"),
                parse_kv_f64(line, "black_end:"),
                parse_kv_f64(line, "black_duration:"),
            ) {
                out.push(TimeSpan {
                    start_sec: s,
                    end_sec: e,
                    duration_sec: d,
                });
            }
        }
    }
    out
}

pub fn parse_freezedetect(stderr: &str) -> Vec<TimeSpan> {
    let mut out = Vec::new();
    let mut cur_start: Option<f64> = None;
    for line in stderr.lines() {
        if let Some(s) = parse_kv_f64(line, "freeze_start: ") {
            cur_start = Some(s);
        } else if let Some(d) = parse_kv_f64(line, "freeze_duration: ") {
            if let Some(s) = cur_start.take() {
                out.push(TimeSpan {
                    start_sec: s,
                    end_sec: s + d,
                    duration_sec: d,
                });
            }
        }
    }
    out
}

pub fn parse_ebur128_summary(stderr: &str) -> (Option<f64>, Option<f64>) {
    let mut lufs = None;
    let mut peak = None;
    let mut in_summary = false;
    for line in stderr.lines() {
        let trimmed = line.trim();
        if trimmed == "Summary:" {
            in_summary = true;
            continue;
        }
        if !in_summary {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("I:") {
            if let Some(val) = rest.split_whitespace().next().and_then(|s| s.parse::<f64>().ok()) {
                lufs = Some(val);
            }
        } else if let Some(rest) = trimmed.strip_prefix("Peak:") {
            if let Some(val) = rest.split_whitespace().next().and_then(|s| s.parse::<f64>().ok()) {
                peak = Some(val);
            }
        }
    }
    (lufs, peak)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_blackdetect_and_freezedetect() {
        let sample = "[blackdetect @ 0x1] black_start:0 black_end:0.35 black_duration:0.35\n\
                      [freezedetect @ 0x2] lavfi.freezedetect.freeze_start: 1.5\n\
                      [freezedetect @ 0x2] lavfi.freezedetect.freeze_duration: 2.25\n";
        let blacks = parse_blackdetect(sample);
        assert_eq!(blacks.len(), 1);
        assert!((blacks[0].duration_sec - 0.35).abs() < 1e-6);

        let freezes = parse_freezedetect(sample);
        assert_eq!(freezes.len(), 1);
        assert!((freezes[0].start_sec - 1.5).abs() < 1e-6);
        assert!((freezes[0].duration_sec - 2.25).abs() < 1e-6);
    }

    #[test]
    fn test_parse_ebur128_summary() {
        let sample = "Summary:\n  Integrated loudness:\n    I:         -14.2 LUFS\n  True peak:\n    Peak:       -0.8 dBFS\n";
        let (lufs, peak) = parse_ebur128_summary(sample);
        assert_eq!(lufs, Some(-14.2));
        assert_eq!(peak, Some(-0.8));
    }
}
