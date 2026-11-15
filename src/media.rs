//! Video probing, audio extraction and single-frame extraction via ffmpeg/ffprobe.
//!
//! ffmpeg/ffprobe are looked up next to the executable first (bundled build),
//! then on PATH.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const SAMPLE_RATE: u32 = 48_000;

#[derive(Debug, Clone)]
pub struct MediaInfo {
    /// Nominal frame rate (r_frame_rate).
    pub fps: f64,
    /// Average frame rate; differs from `fps` for variable-frame-rate captures.
    pub avg_fps: f64,
    pub duration_s: f64,
    pub nb_frames: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub video_start_s: f64,
    pub audio_start_s: f64,
}

impl MediaInfo {
    /// Seconds to add to an audio-stream timestamp to get a video-timeline timestamp.
    pub fn audio_offset(&self) -> f64 {
        self.audio_start_s - self.video_start_s
    }

    pub fn is_variable_frame_rate(&self) -> bool {
        self.avg_fps > 0.0 && (self.fps - self.avg_fps).abs() / self.fps > 0.01
    }
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<Stream>,
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    nb_frames: Option<String>,
    start_time: Option<String>,
    duration: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

fn tool_path(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let file = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.to_string()
            };
            let local = dir.join(file);
            if local.exists() {
                return local;
            }
        }
    }
    PathBuf::from(name)
}

fn command(name: &str) -> Command {
    let mut c = Command::new(tool_path(name));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

fn parse_rate(s: &str) -> f64 {
    match s.split_once('/') {
        Some((n, d)) => {
            let n: f64 = n.parse().unwrap_or(0.0);
            let d: f64 = d.parse().unwrap_or(1.0);
            if d == 0.0 {
                0.0
            } else {
                n / d
            }
        }
        None => s.parse().unwrap_or(0.0),
    }
}

fn parse_f64(s: &Option<String>) -> Option<f64> {
    s.as_ref().and_then(|v| v.parse::<f64>().ok())
}

pub fn probe(path: &Path) -> Result<MediaInfo> {
    let out = command("ffprobe")
        .args(["-v", "error", "-print_format", "json", "-show_streams", "-show_format"])
        .arg(path)
        .output()
        .context("could not run ffprobe; put ffprobe.exe next to geoproof.exe or install ffmpeg on PATH")?;
    if !out.status.success() {
        bail!("ffprobe failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let probe: Probe = serde_json::from_slice(&out.stdout).context("unreadable ffprobe output")?;

    let video = probe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| anyhow!("no video stream found in this file"))?;
    let audio = probe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"))
        .ok_or_else(|| anyhow!("no audio stream found; GeoProof needs the click audio"))?;

    let fps = video.r_frame_rate.as_deref().map(parse_rate).unwrap_or(0.0);
    let avg_fps = video.avg_frame_rate.as_deref().map(parse_rate).unwrap_or(fps);
    if fps <= 0.0 {
        bail!("could not read the video frame rate");
    }
    let duration_s = parse_f64(&video.duration)
        .or_else(|| probe.format.as_ref().and_then(|f| parse_f64(&f.duration)))
        .unwrap_or(0.0);

    Ok(MediaInfo {
        fps,
        avg_fps,
        duration_s,
        nb_frames: video.nb_frames.as_ref().and_then(|v| v.parse().ok()),
        width: video.width.unwrap_or(0),
        height: video.height.unwrap_or(0),
        video_start_s: parse_f64(&video.start_time).unwrap_or(0.0),
        audio_start_s: parse_f64(&audio.start_time).unwrap_or(0.0),
    })
}

/// Decode the first audio stream to mono f32 at `SAMPLE_RATE`.
pub fn decode_audio(path: &Path) -> Result<Vec<f32>> {
    let out = command("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar"])
        .arg(SAMPLE_RATE.to_string())
        .args(["-f", "f32le", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("could not run ffmpeg; put ffmpeg.exe next to geoproof.exe or install it on PATH")?;
    if !out.status.success() {
        bail!("ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    if out.stdout.len() < 4 {
        bail!("the audio stream is empty");
    }
    Ok(out
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

/// Decode the frame at `time_s` (video timeline) to RGBA, scaled to `width` pixels wide.
/// Returns (width, height, rgba bytes).
pub fn extract_frame(path: &Path, time_s: f64, info: &MediaInfo, width: usize) -> Result<(usize, usize, Vec<u8>)> {
    if info.width == 0 || info.height == 0 {
        bail!("unknown video size");
    }
    let w = width.max(2) & !1;
    let h = (((w as f64) * info.height as f64 / info.width as f64 / 2.0).round() as usize * 2).max(2);
    let out = command("ffmpeg")
        .args(["-v", "error", "-ss"])
        .arg(format!("{:.4}", time_s.max(0.0)))
        .arg("-i")
        .arg(path)
        .args(["-frames:v", "1", "-an", "-vf"])
        .arg(format!("scale={w}:{h}:flags=bicubic"))
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("could not run ffmpeg")?;
    if !out.status.success() {
        bail!("ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    if out.stdout.len() != w * h * 4 {
        bail!("frame decode returned {} bytes, expected {}", out.stdout.len(), w * h * 4);
    }
    Ok((w, h, out.stdout))
}
