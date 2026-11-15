//! Turns detected audio onsets into per-click frame positions and gaps.
//!
//! A click's `frame` is the video frame whose presentation interval contains
//! the click sound, and `subframe` is how far into that interval it lands
//! (0.0 = start, 1.0 = end). These are measurements of the recording, not
//! verdicts; capture jitter and dropped frames limit what they can prove.

use crate::detect::Onset;
use crate::media::MediaInfo;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ClickEvent {
    pub index: usize,
    /// Seconds on the video timeline (audio time plus stream start offset).
    pub t_video_s: f64,
    pub frame: u64,
    /// Position inside the frame, 0.0..1.0.
    pub subframe: f64,
    /// Time since the previous click.
    pub gap_ms: Option<f64>,
    /// Same gap expressed in video frames (fractional).
    pub gap_frames: Option<f64>,
    pub strength: f32,
}

pub fn map_to_frames(onsets: &[Onset], info: &MediaInfo) -> Vec<ClickEvent> {
    let offset = info.audio_offset();
    let mut events: Vec<ClickEvent> = Vec::with_capacity(onsets.len());
    for o in onsets {
        let t = o.t + offset;
        if t < 0.0 {
            continue;
        }
        let pos = t * info.fps;
        let frame = pos.floor();
        let (gap_ms, gap_frames) = match events.last() {
            Some(prev) => {
                let dt = t - prev.t_video_s;
                (Some(dt * 1000.0), Some(dt * info.fps))
            }
            None => (None, None),
        };
        events.push(ClickEvent {
            index: events.len(),
            t_video_s: t,
            frame: frame as u64,
            subframe: pos - frame,
            gap_ms,
            gap_frames,
            strength: o.strength,
        });
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(fps: f64, audio_start: f64) -> MediaInfo {
        MediaInfo {
            fps,
            avg_fps: fps,
            duration_s: 10.0,
            nb_frames: None,
            width: 1920,
            height: 1080,
            video_start_s: 0.0,
            audio_start_s: audio_start,
        }
    }

    fn onset(t: f64) -> Onset {
        Onset { t, flux: 1.0, thr: 0.5, strength: 10.0 }
    }

    #[test]
    fn maps_time_to_frame_and_subframe() {
        // 60 fps: frame length 1/60 s. 1.0 s is exactly frame 60, 1.0 + 0.5/60 is halfway in.
        let ev = map_to_frames(&[onset(1.0), onset(1.0 + 0.5 / 60.0 + 3.0 / 60.0)], &info(60.0, 0.0));
        assert_eq!(ev[0].frame, 60);
        assert!(ev[0].subframe < 1e-6);
        assert_eq!(ev[1].frame, 63);
        assert!((ev[1].subframe - 0.5).abs() < 1e-6);
        assert!((ev[1].gap_frames.unwrap() - 3.5).abs() < 1e-6);
    }

    #[test]
    fn applies_audio_stream_offset() {
        let ev = map_to_frames(&[onset(1.0)], &info(60.0, 0.5));
        assert!((ev[0].t_video_s - 1.5).abs() < 1e-9);
        assert_eq!(ev[0].frame, 90);
    }
}
