//! Turns detected audio onsets into per-click frame positions and gaps, and
//! filters them against the user's learned click sound.
//!
//! A click's `frame` is the video frame whose presentation interval contains
//! the click sound, and `subframe` is how far into that interval it lands
//! (0.0 = start, 1.0 = end). These are measurements of the recording, not
//! verdicts; capture jitter and dropped frames limit what they can prove.

use crate::detect::{similarity, Onset, Signature};
use crate::media::MediaInfo;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ClickEvent {
    /// Ordinal among accepted clicks.
    pub index: usize,
    /// Stable id: position in the full candidate list.
    #[serde(skip)]
    pub cand_id: usize,
    /// Seconds on the video timeline (audio time plus stream start offset).
    pub t_video_s: f64,
    pub frame: u64,
    /// Position inside the frame, 0.0..1.0.
    pub subframe: f64,
    /// Time since the previous accepted click.
    pub gap_ms: Option<f64>,
    /// Same gap expressed in video frames (fractional).
    pub gap_frames: Option<f64>,
    pub strength: f32,
    /// Match to the learned click sound, when one exists.
    pub similarity: Option<f32>,
    #[serde(skip)]
    pub accepted: bool,
    #[serde(skip)]
    pub sig: Option<Signature>,
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
        let id = events.len();
        events.push(ClickEvent {
            index: id,
            cand_id: id,
            t_video_s: t,
            frame: frame as u64,
            subframe: pos - frame,
            gap_ms,
            gap_frames,
            strength: o.strength,
            similarity: None,
            accepted: true,
            sig: o.sig,
        });
    }
    events
}

/// Scores every candidate against `template` and returns the accepted clicks,
/// renumbered, with gaps recomputed between accepted clicks only. With no
/// template every candidate is accepted.
pub fn apply_filter(
    cands: &mut [ClickEvent],
    template: Option<&Signature>,
    min_sim: f32,
    fps: f64,
) -> Vec<ClickEvent> {
    let mut out: Vec<ClickEvent> = Vec::with_capacity(cands.len());
    for c in cands.iter_mut() {
        c.similarity = match (template, &c.sig) {
            (Some(t), Some(s)) => Some(similarity(t, s)),
            _ => None,
        };
        c.accepted = match (template, c.similarity) {
            (Some(_), Some(sim)) => sim >= min_sim,
            (Some(_), None) => false,
            (None, _) => true,
        };
        if c.accepted {
            let mut e = c.clone();
            e.index = out.len();
            match out.last() {
                Some(prev) => {
                    let dt = e.t_video_s - prev.t_video_s;
                    e.gap_ms = Some(dt * 1000.0);
                    e.gap_frames = Some(dt * fps);
                }
                None => {
                    e.gap_ms = None;
                    e.gap_frames = None;
                }
            }
            out.push(e);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::SIG_BANDS;

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
        Onset { t, flux: 1.0, thr: 0.5, strength: 10.0, sig: None }
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

    #[test]
    fn filter_keeps_only_matching_clicks() {
        let mut a = [0.0f32; SIG_BANDS];
        a[0] = 1.0;
        let mut b = [0.0f32; SIG_BANDS];
        b[1] = 1.0;
        let mut on = vec![onset(1.0), onset(1.1), onset(1.2)];
        on[0].sig = Some(a);
        on[1].sig = Some(b);
        on[2].sig = Some(a);
        let mut cands = map_to_frames(&on, &info(60.0, 0.0));
        let kept = apply_filter(&mut cands, Some(&a), 0.9, 60.0);
        assert_eq!(kept.len(), 2);
        assert!((kept[1].gap_ms.unwrap() - 200.0).abs() < 1e-6);
        assert!(!cands[1].accepted);
        let all = apply_filter(&mut cands, None, 0.9, 60.0);
        assert_eq!(all.len(), 3);
    }
}
