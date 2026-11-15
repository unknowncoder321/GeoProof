//! The timeline: time ruler, click-marker lane, and a sample-accurate waveform
//! with a red line at every detected click. Zoom goes down to individual samples.

use crate::analysis::ClickEvent;
use crate::media::SAMPLE_RATE;
use crate::theme::{BG, BORDER, CLICK, CLICK_DIM, FAINT, MUTED, PANEL, PLAYHEAD, TEXT, WAVE};
use eframe::egui::{self, pos2, vec2, Align2, Color32, FontId, PointerButton, Pos2, Rect, Sense, Shape, Stroke, Ui};
use std::sync::Arc;

const BASE: usize = 64;
const FACTOR: usize = 8;
const LEVELS: usize = 4;

/// Multi-resolution min/max of the audio so any zoom level draws fast and exact.
pub struct WavePyramid {
    audio: Arc<Vec<f32>>,
    levels: Vec<(usize, Vec<[f32; 2]>)>,
    pub peak: f32,
}

impl WavePyramid {
    pub fn new(audio: Arc<Vec<f32>>) -> Self {
        let mut peak = 1e-6f32;
        let mut base: Vec<[f32; 2]> = Vec::with_capacity(audio.len() / BASE + 1);
        for chunk in audio.chunks(BASE) {
            let mut mn = f32::MAX;
            let mut mx = f32::MIN;
            for &v in chunk {
                if v < mn {
                    mn = v;
                }
                if v > mx {
                    mx = v;
                }
            }
            peak = peak.max(mn.abs()).max(mx.abs());
            base.push([mn, mx]);
        }
        let mut levels: Vec<(usize, Vec<[f32; 2]>)> = vec![(BASE, base)];
        for _ in 1..LEVELS {
            let (nb, next) = {
                let (b, prev) = levels.last().unwrap();
                let next: Vec<[f32; 2]> = prev
                    .chunks(FACTOR)
                    .map(|c| {
                        let mut mn = f32::MAX;
                        let mut mx = f32::MIN;
                        for m in c {
                            mn = mn.min(m[0]);
                            mx = mx.max(m[1]);
                        }
                        [mn, mx]
                    })
                    .collect();
                (*b * FACTOR, next)
            };
            levels.push((nb, next));
        }
        WavePyramid { audio, levels, peak }
    }

    pub fn len(&self) -> usize {
        self.audio.len()
    }

    pub fn sample(&self, i: usize) -> f32 {
        self.audio.get(i).copied().unwrap_or(0.0)
    }

    /// Min and max over samples [s0, s1).
    pub fn range(&self, s0: usize, s1: usize) -> Option<(f32, f32)> {
        let len = self.audio.len();
        if s0 >= len {
            return None;
        }
        let s1 = s1.min(len).max(s0 + 1);
        let n = s1 - s0;
        for (b, v) in self.levels.iter().rev() {
            let b = *b;
            if b <= n {
                let i0 = s0 / b;
                let i1 = ((s1 + b - 1) / b).min(v.len());
                if i0 >= i1 {
                    continue;
                }
                let mut mn = f32::MAX;
                let mut mx = f32::MIN;
                for m in &v[i0..i1] {
                    mn = mn.min(m[0]);
                    mx = mx.max(m[1]);
                }
                return Some((mn, mx));
            }
        }
        let mut mn = f32::MAX;
        let mut mx = f32::MIN;
        for &x in &self.audio[s0..s1] {
            mn = mn.min(x);
            mx = mx.max(x);
        }
        Some((mn, mx))
    }
}

pub struct TimelineState {
    pub start: f64,
    pub span: f64,
    pub gain: f32,
}

impl Default for TimelineState {
    fn default() -> Self {
        TimelineState { start: 0.0, span: 10.0, gain: 1.0 }
    }
}

impl TimelineState {
    pub fn fit(&mut self, duration: f64) {
        self.start = 0.0;
        self.span = duration.max(0.05);
    }

    pub fn clamp(&mut self, duration: f64) {
        let dur = duration.max(0.05);
        self.span = self.span.clamp(0.002, dur);
        let max_start = (dur - self.span).max(0.0);
        self.start = self.start.clamp(0.0, max_start);
    }

    pub fn zoom_at(&mut self, factor: f64, t: f64, duration: f64) {
        let frac = ((t - self.start) / self.span).clamp(0.0, 1.0);
        let new_span = (self.span * factor).clamp(0.002, duration.max(0.05));
        self.start = t - frac * new_span;
        self.span = new_span;
        self.clamp(duration);
    }

    pub fn pan(&mut self, dt: f64, duration: f64) {
        self.start += dt;
        self.clamp(duration);
    }

    pub fn ensure_visible(&mut self, t: f64, duration: f64) {
        let margin = self.span * 0.06;
        if t < self.start + margin || t > self.start + self.span - margin {
            self.start = t - self.span * 0.5;
            self.clamp(duration);
        }
    }
}

pub struct TimelineInput<'a> {
    pub pyramid: &'a WavePyramid,
    pub audio_offset_s: f64,
    pub duration_s: f64,
    pub fps: f64,
    pub candidates: &'a [ClickEvent],
    pub playhead_s: f64,
    pub selected: Option<usize>,
}

#[derive(Default)]
pub struct TimelineOutput {
    pub seek: Option<f64>,
    pub select: Option<usize>,
}

pub fn fmt_timecode(t: f64) -> String {
    let t = t.max(0.0);
    let m = (t / 60.0).floor() as u64;
    let s = t - m as f64 * 60.0;
    format!("{}:{:06.3}", m, s)
}

fn fmt_tick(t: f64, step: f64) -> String {
    let t = t.max(0.0);
    let m = (t / 60.0).floor() as u64;
    let s = t - m as f64 * 60.0;
    if step >= 1.0 {
        format!("{}:{:02}", m, s.round() as u64)
    } else if step >= 0.01 {
        format!("{}:{:05.2}", m, s)
    } else {
        format!("{}:{:06.3}", m, s)
    }
}

/// Small readout label that stays inside `bounds`.
pub fn chip(p: &egui::Painter, pos: Pos2, text: &str, fg: Color32, bg: Color32, bounds: Rect) {
    let galley = p.layout_no_wrap(text.to_string(), FontId::monospace(11.5), fg);
    let size = galley.size() + vec2(14.0, 8.0);
    let mut pos = pos;
    if pos.x + size.x > bounds.right() - 4.0 {
        pos.x = bounds.right() - 4.0 - size.x;
    }
    if pos.x < bounds.left() + 4.0 {
        pos.x = bounds.left() + 4.0;
    }
    let r = Rect::from_min_size(pos, size);
    p.rect_filled(r, 5.0, bg);
    p.galley(r.min + vec2(7.0, 4.0), galley, fg);
}

pub fn show(ui: &mut Ui, input: &TimelineInput<'_>, st: &mut TimelineState) -> TimelineOutput {
    let mut out = TimelineOutput::default();
    let size = ui.available_size();
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = resp.rect;

    let gutter = 72.0f32;
    let ruler_h = 30.0f32;
    let lane_h = 22.0f32;
    let x0 = rect.left() + gutter;
    let w = (rect.width() - gutter).max(20.0);
    let dur = input.duration_s.max(0.05);
    let fps = input.fps;
    st.clamp(dur);

    // Wheel: zoom around the cursor. Shift+wheel or sideways scroll: pan.
    if resp.hovered() {
        let (scroll, zoom_d, shift) = ui.input(|i| (i.raw_scroll_delta, i.zoom_delta(), i.modifiers.shift));
        if let Some(hp) = resp.hover_pos() {
            let ht = st.start + ((hp.x - x0) / w) as f64 * st.span;
            if shift || scroll.x != 0.0 {
                let px = if scroll.x != 0.0 { scroll.x } else { scroll.y };
                st.pan(-(px as f64) * st.span / w as f64, dur);
            } else if zoom_d != 1.0 {
                st.zoom_at(1.0 / zoom_d as f64, ht, dur);
            } else if scroll.y != 0.0 {
                st.zoom_at((-(scroll.y as f64) * 0.0025).exp(), ht, dur);
            }
        }
    }
    if resp.dragged_by(PointerButton::Middle) || resp.dragged_by(PointerButton::Secondary) {
        st.pan(-(resp.drag_delta().x as f64) * st.span / w as f64, dur);
    }

    let (vs, vspan) = (st.start, st.span);
    let x_of = |t: f64| -> f32 { x0 + ((t - vs) / vspan * w as f64) as f32 };
    let t_of = |x: f32| -> f64 { (vs + ((x - x0) / w) as f64 * vspan).clamp(0.0, dur) };
    let margin_t = 12.0 * vspan / w as f64;
    let visible = |c: &[ClickEvent]| -> (usize, usize) {
        let lo = c.partition_point(|e| e.t_video_s < vs - margin_t);
        let hi = c.partition_point(|e| e.t_video_s <= vs + vspan + margin_t);
        (lo, hi)
    };
    let marker_at = |x: f32| -> Option<usize> {
        let (lo, hi) = visible(input.candidates);
        let mut best: Option<(f32, usize)> = None;
        for e in &input.candidates[lo..hi] {
            let d = (x_of(e.t_video_s) - x).abs();
            if d <= 8.0 && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, e.cand_id));
            }
        }
        best.map(|(_, id)| id)
    };

    if resp.dragged_by(PointerButton::Primary) {
        if let Some(p) = resp.interact_pointer_pos() {
            out.seek = Some(t_of(p.x));
        }
    }
    if resp.clicked_by(PointerButton::Primary) {
        if let Some(p) = resp.interact_pointer_pos() {
            match marker_at(p.x) {
                Some(id) => out.select = Some(id),
                None => out.seek = Some(t_of(p.x)),
            }
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }

    // ---- drawing ----
    let p = painter.with_clip_rect(rect);
    let ruler = Rect::from_min_size(pos2(x0, rect.top()), vec2(w, ruler_h));
    let lane = Rect::from_min_size(pos2(x0, rect.top() + ruler_h), vec2(w, lane_h));
    let track = Rect::from_min_max(pos2(x0, rect.top() + ruler_h + lane_h), rect.max);
    let pt = painter.with_clip_rect(track);

    p.rect_filled(rect, 0.0, BG);
    p.rect_filled(ruler, 0.0, PANEL);
    p.rect_filled(lane, 0.0, Color32::from_rgb(14, 16, 20));
    p.rect_filled(track, 0.0, Color32::from_rgb(12, 14, 18));
    p.rect_filled(Rect::from_min_max(rect.min, pos2(x0, rect.bottom())), 0.0, PANEL);
    p.line_segment([pos2(x0, rect.top()), pos2(x0, rect.bottom())], Stroke::new(1.0, BORDER));
    p.line_segment([pos2(x0, lane.top()), pos2(rect.right(), lane.top())], Stroke::new(1.0, BORDER));
    p.line_segment([pos2(x0, track.top()), pos2(rect.right(), track.top())], Stroke::new(1.0, BORDER));
    let label_font = FontId::proportional(10.0);
    p.text(pos2(rect.left() + 14.0, ruler.center().y), Align2::LEFT_CENTER, "TIME", label_font.clone(), FAINT);
    p.text(pos2(rect.left() + 14.0, lane.center().y), Align2::LEFT_CENTER, "CLICKS", label_font.clone(), CLICK);
    p.text(pos2(rect.left() + 14.0, track.center().y), Align2::LEFT_CENTER, "AUDIO", label_font, WAVE);

    // Ruler ticks
    let px_per_s = w as f64 / vspan;
    let steps = [
        0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0,
        600.0,
    ];
    let major = steps.iter().copied().find(|s| s * px_per_s >= 90.0).unwrap_or(600.0);
    let minor = major / 5.0;
    let tick_col = Color32::from_rgb(74, 82, 100);
    let mut k = (vs / minor).floor() as i64;
    loop {
        let t = k as f64 * minor;
        if t > vs + vspan + minor {
            break;
        }
        let x = x_of(t);
        if x >= x0 && x <= rect.right() {
            if k.rem_euclid(5) == 0 {
                p.line_segment([pos2(x, ruler.bottom() - 12.0), pos2(x, ruler.bottom())], Stroke::new(1.0, tick_col));
                p.text(
                    pos2(x + 4.0, ruler.top() + 9.0),
                    Align2::LEFT_CENTER,
                    fmt_tick(t, major),
                    FontId::monospace(10.5),
                    MUTED,
                );
            } else {
                p.line_segment([pos2(x, ruler.bottom() - 6.0), pos2(x, ruler.bottom())], Stroke::new(1.0, tick_col));
            }
        }
        k += 1;
    }

    // Frame grid, once frames are wide enough to tell apart
    let ppf = px_per_s / fps;
    if ppf >= 6.0 {
        let f0 = (vs * fps).floor().max(0.0) as i64;
        let f1 = ((vs + vspan) * fps).ceil() as i64;
        for f in f0..=f1 {
            let x = x_of(f as f64 / fps);
            if x < x0 || x > rect.right() {
                continue;
            }
            p.line_segment([pos2(x, ruler.bottom() - 5.0), pos2(x, ruler.bottom())], Stroke::new(1.0, tick_col));
            p.line_segment(
                [pos2(x, lane.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 12)),
            );
            if ppf >= 30.0 {
                p.text(
                    pos2(x + 3.0, ruler.bottom() - 8.0),
                    Align2::LEFT_CENTER,
                    f.to_string(),
                    FontId::monospace(9.5),
                    FAINT,
                );
            }
        }
    }

    // Selected frame band
    let ph_frame = (input.playhead_s * fps + 1e-6).floor();
    let fx0 = x_of(ph_frame / fps);
    let fx1 = x_of((ph_frame + 1.0) / fps);
    let band = Rect::from_min_max(pos2(fx0, lane.top()), pos2(fx1.max(fx0 + 1.5), rect.bottom()));
    p.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(255, 196, 64, 26));

    // Waveform
    let sr = SAMPLE_RATE as f64;
    let spp = vspan * sr / w as f64;
    let mid = track.center().y;
    pt.line_segment([pos2(x0, mid), pos2(rect.right(), mid)], Stroke::new(1.0, Color32::from_rgb(30, 34, 42)));
    let amp = (track.height() * 0.5 - 8.0).max(4.0) * st.gain / input.pyramid.peak;
    if spp <= 1.5 {
        let s_a = ((vs - input.audio_offset_s) * sr).floor().max(0.0) as usize;
        let s_b = ((((vs + vspan - input.audio_offset_s) * sr).ceil()).max(0.0) as usize + 1).min(input.pyramid.len());
        if s_b > s_a + 1 {
            let mut pts: Vec<Pos2> = Vec::with_capacity(s_b - s_a);
            for i in s_a..s_b {
                let t = input.audio_offset_s + i as f64 / sr;
                pts.push(pos2(x_of(t), mid - input.pyramid.sample(i) * amp));
            }
            if spp < 0.3 {
                for q in &pts {
                    pt.circle_filled(*q, 2.2, WAVE);
                }
            }
            pt.add(Shape::line(pts, Stroke::new(1.4, WAVE)));
        }
    } else {
        let n_cols = w as usize;
        let len = input.pyramid.len();
        let mut shapes: Vec<Shape> = Vec::with_capacity(n_cols);
        for xi in 0..n_cols {
            let t0 = vs + xi as f64 * vspan / w as f64 - input.audio_offset_s;
            let t1 = t0 + vspan / w as f64;
            if t1 <= 0.0 {
                continue;
            }
            let s0 = (t0 * sr).floor().max(0.0) as usize;
            let s1 = (t1 * sr).ceil().max(0.0) as usize;
            if s0 >= len {
                break;
            }
            if let Some((mn, mx)) = input.pyramid.range(s0, s1) {
                let x = x0 + xi as f32 + 0.5;
                let y1 = mid - mx * amp;
                let mut y2 = mid - mn * amp;
                if y2 - y1 < 1.0 {
                    y2 = y1 + 1.0;
                }
                shapes.push(Shape::line_segment([pos2(x, y1), pos2(x, y2)], Stroke::new(1.0, WAVE)));
            }
        }
        pt.extend(shapes);
    }

    // Click markers: red line through the audio, flag in the lane
    let (lo, hi) = visible(input.candidates);
    let hovered_marker = resp.hover_pos().and_then(|hp| if hp.y > lane.top() { marker_at(hp.x) } else { None });
    for c in &input.candidates[lo..hi] {
        let x = x_of(c.t_video_s);
        let selected = input.selected == Some(c.cand_id);
        let hot = hovered_marker == Some(c.cand_id);
        let (col, lw) = if c.accepted { (CLICK, if hot { 2.4 } else { 1.5 }) } else { (CLICK_DIM, 1.0) };
        if selected {
            pt.line_segment(
                [pos2(x, track.top()), pos2(x, track.bottom())],
                Stroke::new(7.0, Color32::from_rgba_unmultiplied(255, 64, 82, 55)),
            );
        }
        pt.line_segment([pos2(x, track.top()), pos2(x, track.bottom())], Stroke::new(lw, col));
        let tri = vec![pos2(x - 5.5, lane.top() + 4.0), pos2(x + 5.5, lane.top() + 4.0), pos2(x, lane.bottom() - 3.0)];
        p.add(Shape::convex_polygon(tri.clone(), col, Stroke::NONE));
        if selected {
            p.add(Shape::closed_line(tri, Stroke::new(1.5, Color32::WHITE)));
        }
    }

    // Playhead
    let px = x_of(input.playhead_s);
    if px >= x0 - 1.0 && px <= rect.right() + 1.0 {
        p.line_segment([pos2(px, rect.top() + 14.0), pos2(px, rect.bottom())], Stroke::new(1.5, PLAYHEAD));
        let head = vec![
            pos2(px - 7.0, rect.top() + 1.0),
            pos2(px + 7.0, rect.top() + 1.0),
            pos2(px + 7.0, rect.top() + 10.0),
            pos2(px, rect.top() + 17.0),
            pos2(px - 7.0, rect.top() + 10.0),
        ];
        p.add(Shape::convex_polygon(head, PLAYHEAD, Stroke::NONE));
    }

    // Hover readout
    if let Some(hp) = resp.hover_pos() {
        if hp.x >= x0 {
            let chip_bg = Color32::from_rgba_unmultiplied(8, 9, 12, 225);
            let at = pos2(hp.x + 12.0, track.top() + 8.0);
            match hovered_marker.and_then(|id| input.candidates.get(id)) {
                Some(c) => {
                    let mut label = format!("frame {}   position {:.2}", c.frame, c.subframe);
                    if let Some(s) = c.similarity {
                        label.push_str(&format!("   match {:.2}", s));
                    }
                    if !c.accepted {
                        label.push_str("   rejected");
                    }
                    chip(&p, at, &label, TEXT, chip_bg, rect);
                }
                None => {
                    let t = t_of(hp.x);
                    let label = format!("{}   frame {}", fmt_timecode(t), (t * fps + 1e-6).floor() as u64);
                    chip(&p, at, &label, MUTED, chip_bg, rect);
                }
            }
        }
    }

    out
}
