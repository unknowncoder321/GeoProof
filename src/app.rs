use crate::analysis::{apply_filter, map_to_frames, ClickEvent};
use crate::detect::{self, mean_signature, Params, Signature};
use crate::media::{self, MediaInfo, SAMPLE_RATE};
use crate::theme::{card, kv, section_label, BG, BORDER, CLICK, FAINT, MUTED, PANEL, PLAYHEAD, SELECT, TEXT, WARN, WAVE};
use crate::timeline::{self, chip, fmt_timecode, TimelineInput, TimelineState, WavePyramid};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, ColorImage, FontId, Key, Layout, Margin, Rect, RichText, Sense, Stroke,
    TextureHandle, TextureOptions,
};
use egui_extras::{Column, TableBuilder};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};

#[derive(Default)]
struct Progress {
    stage: Mutex<String>,
    permille: AtomicU32,
}

impl Progress {
    fn set(&self, stage: &str, frac: f32) {
        if let Ok(mut s) = self.stage.lock() {
            *s = stage.to_string();
        }
        self.permille.store((frac.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
    }
    fn frac(&self) -> f32 {
        self.permille.load(Ordering::Relaxed) as f32 / 1000.0
    }
    fn stage(&self) -> String {
        self.stage.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Sens {
    Sensitive,
    Balanced,
    Strict,
}

impl Sens {
    fn label(self) -> &'static str {
        match self {
            Sens::Sensitive => "Sensitive",
            Sens::Balanced => "Balanced",
            Sens::Strict => "Strict",
        }
    }
    fn k(self) -> f32 {
        match self {
            Sens::Sensitive => 6.0,
            Sens::Balanced => 10.0,
            Sens::Strict => 16.0,
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Sens::Sensitive => "Catches quiet clicks, but also more false hits from music.",
            Sens::Balanced => "Good starting point for clear click audio.",
            Sens::Strict => "Only loud, sharp clicks. Will miss soft ones.",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum RightTab {
    Inspect,
    Clicks,
}

#[derive(Default)]
struct Actions {
    open: bool,
    redetect: bool,
    export: bool,
    learn: bool,
    forget: bool,
    refilter: bool,
    fit: bool,
    seek: Option<f64>,
    /// (candidate id, zoom the timeline in on it)
    select: Option<(usize, bool)>,
    step: i64,
    jump: i32,
    zoom: Option<f64>,
}

struct Loaded {
    path: PathBuf,
    info: MediaInfo,
    audio: Arc<Vec<f32>>,
    pyramid: WavePyramid,
    candidates: Vec<ClickEvent>,
}

enum Msg {
    Loaded(Box<Loaded>),
    Redetected(Vec<ClickEvent>),
    Frame { t: f64, image: Result<ColorImage, String> },
    Failed(String),
}

pub struct GeoProofApp {
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    progress: Arc<Progress>,
    busy: bool,
    status: String,
    error: Option<String>,
    loaded: Option<Loaded>,
    /// Accepted clicks (all candidates unless a click sound is learned).
    active: Vec<ClickEvent>,
    samples: Vec<Signature>,
    template: Option<Signature>,
    min_sim: f32,
    playhead_s: f64,
    selected: Option<usize>,
    frame_tex: Option<TextureHandle>,
    frame_t: f64,
    frame_busy: bool,
    frame_want: Option<f64>,
    frame_err: Option<String>,
    tl: TimelineState,
    tab: RightTab,
    sens: Sens,
    params: Params,
}

impl Default for GeoProofApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        GeoProofApp {
            tx,
            rx,
            progress: Arc::new(Progress::default()),
            busy: false,
            status: String::new(),
            error: None,
            loaded: None,
            active: Vec::new(),
            samples: Vec::new(),
            template: None,
            min_sim: 0.93,
            playhead_s: -1.0,
            selected: None,
            frame_tex: None,
            frame_t: 0.0,
            frame_busy: false,
            frame_want: None,
            frame_err: None,
            tl: TimelineState::default(),
            tab: RightTab::Inspect,
            sens: Sens::Balanced,
            params: Params::default(),
        }
    }
}

fn median_of(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

#[derive(Serialize)]
struct Export<'a> {
    tool: &'static str,
    note: &'static str,
    file: String,
    fps: f64,
    duration_s: f64,
    audio_offset_s: f64,
    band_hz: [f32; 2],
    threshold_k: f32,
    click_sound_samples: usize,
    min_similarity: Option<f32>,
    clicks: &'a [ClickEvent],
}

fn position_bar(ui: &mut egui::Ui, pos: f64) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, BG);
    for q in [0.25f32, 0.5, 0.75] {
        let x = rect.left() + rect.width() * q;
        p.line_segment([pos2(x, rect.top() + 4.0), pos2(x, rect.bottom() - 4.0)], Stroke::new(1.0, BORDER));
    }
    let x = rect.left() + rect.width() * pos as f32;
    p.line_segment([pos2(x, rect.top() + 2.0), pos2(x, rect.bottom() - 2.0)], Stroke::new(2.5, CLICK));
}

fn subframe_histogram(ui: &mut egui::Ui, events: &[ClickEvent]) {
    let bins = 10usize;
    let mut counts = vec![0usize; bins];
    for e in events {
        let b = ((e.subframe * bins as f64) as usize).min(bins - 1);
        counts[b] += 1;
    }
    let max = counts.iter().copied().max().unwrap_or(1).max(1);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 88.0), Sense::hover());
    let p = ui.painter();
    let base = rect.bottom() - 16.0;
    let gap = 3.0f32;
    let bw = (rect.width() - gap * (bins as f32 - 1.0)) / bins as f32;
    for (i, c) in counts.iter().enumerate() {
        let h = (rect.height() - 20.0) * (*c as f32 / max as f32);
        let x = rect.left() + i as f32 * (bw + gap);
        let bar = Rect::from_min_max(pos2(x, base - h.max(1.0)), pos2(x + bw, base));
        p.rect_filled(bar, 2.0, WAVE);
    }
    p.line_segment([pos2(rect.left(), base), pos2(rect.right(), base)], Stroke::new(1.0, BORDER));
    p.text(pos2(rect.left(), rect.bottom()), Align2::LEFT_BOTTOM, "start of frame", FontId::proportional(10.0), FAINT);
    p.text(pos2(rect.right(), rect.bottom()), Align2::RIGHT_BOTTOM, "end", FontId::proportional(10.0), FAINT);
}

impl GeoProofApp {
    fn fps(&self) -> f64 {
        self.loaded.as_ref().map(|l| l.info.fps).unwrap_or(60.0)
    }

    fn duration(&self) -> f64 {
        self.loaded.as_ref().map(|l| l.info.duration_s).unwrap_or(0.0)
    }

    fn refilter(&mut self) {
        let Some(l) = &mut self.loaded else { return };
        self.active = apply_filter(&mut l.candidates, self.template.as_ref(), self.min_sim, l.info.fps);
    }

    fn start_load(&mut self, ctx: &egui::Context, path: PathBuf) {
        let params = self.params.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let prog = self.progress.clone();
        self.busy = true;
        self.error = None;
        self.status.clear();
        prog.set("Reading file", 0.0);
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<Loaded> {
                prog.set("Reading file info", 0.02);
                ctx.request_repaint();
                let info = media::probe(&path)?;
                prog.set("Decoding audio", 0.06);
                ctx.request_repaint();
                let audio = media::decode_audio(&path)?;
                prog.set("Listening for clicks", 0.12);
                ctx.request_repaint();
                let onsets = detect::detect_with_progress(&audio, SAMPLE_RATE as f64, &params, &mut |f: f32| {
                    prog.set("Listening for clicks", 0.12 + 0.76 * f);
                    ctx.request_repaint();
                });
                prog.set("Placing clicks on frames", 0.92);
                let candidates = map_to_frames(&onsets, &info);
                prog.set("Building timeline", 0.96);
                ctx.request_repaint();
                let audio = Arc::new(audio);
                let pyramid = WavePyramid::new(audio.clone());
                Ok(Loaded { path, info, audio, pyramid, candidates })
            })();
            let _ = match res {
                Ok(l) => tx.send(Msg::Loaded(Box::new(l))),
                Err(e) => tx.send(Msg::Failed(format!("{e:#}"))),
            };
            ctx.request_repaint();
        });
    }

    fn redetect(&mut self, ctx: &egui::Context) {
        let (audio, info) = match &self.loaded {
            Some(l) => (l.audio.clone(), l.info.clone()),
            None => return,
        };
        let params = self.params.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let prog = self.progress.clone();
        self.busy = true;
        prog.set("Listening for clicks", 0.0);
        std::thread::spawn(move || {
            let onsets = detect::detect_with_progress(&audio, SAMPLE_RATE as f64, &params, &mut |f: f32| {
                prog.set("Listening for clicks", f);
                ctx.request_repaint();
            });
            let _ = tx.send(Msg::Redetected(map_to_frames(&onsets, &info)));
            ctx.request_repaint();
        });
    }

    fn request_frame(&mut self, ctx: &egui::Context) {
        if self.frame_busy {
            self.frame_want = Some(self.playhead_s);
            return;
        }
        let Some(l) = &self.loaded else { return };
        let path = l.path.clone();
        let info = l.info.clone();
        let t = self.playhead_s;
        self.frame_busy = true;
        self.frame_t = t;
        self.frame_want = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let centre = t + 0.5 / info.fps;
            let image = media::extract_frame(&path, centre, &info, 960)
                .map(|(w, h, rgba)| ColorImage::from_rgba_unmultiplied([w, h], &rgba))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Frame { t, image });
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Loaded(l) => {
                    self.busy = false;
                    let l = *l;
                    let dur = l.info.duration_s;
                    self.loaded = Some(l);
                    self.samples.clear();
                    self.template = None;
                    self.selected = None;
                    self.tl.fit(dur);
                    self.playhead_s = -1.0;
                    self.frame_tex = None;
                    self.refilter();
                    self.status = format!("{} clicks found", self.active.len());
                    let first = self.active.first().map(|c| c.t_video_s).unwrap_or(0.0);
                    self.set_playhead(ctx, first);
                }
                Msg::Redetected(ev) => {
                    self.busy = false;
                    if let Some(l) = &mut self.loaded {
                        l.candidates = ev;
                    }
                    self.selected = None;
                    self.refilter();
                    self.status = format!("{} clicks found", self.active.len());
                }
                Msg::Frame { t, image } => {
                    self.frame_busy = false;
                    let _ = t;
                    match image {
                        Ok(img) => {
                            self.frame_tex = Some(ctx.load_texture("frame", img, TextureOptions::LINEAR));
                            self.frame_err = None;
                        }
                        Err(e) => self.frame_err = Some(e),
                    }
                    if let Some(w) = self.frame_want.take() {
                        if (w - self.frame_t).abs() > 1e-9 {
                            self.request_frame(ctx);
                        }
                    }
                }
                Msg::Failed(e) => {
                    self.busy = false;
                    self.error = Some(e);
                    self.status.clear();
                }
            }
        }
    }

    fn set_playhead(&mut self, ctx: &egui::Context, t: f64) {
        let Some(l) = &self.loaded else { return };
        let fps = l.info.fps;
        let dur = l.info.duration_s.max(0.0);
        let frame = (t.clamp(0.0, dur) * fps + 1e-6).floor();
        let snapped = frame / fps;
        if (snapped - self.playhead_s).abs() < 1e-9 && self.frame_tex.is_some() {
            return;
        }
        self.playhead_s = snapped;
        self.request_frame(ctx);
    }

    fn select_click(&mut self, ctx: &egui::Context, id: usize, focus: bool) {
        let t = self.loaded.as_ref().and_then(|l| l.candidates.get(id)).map(|c| c.t_video_s);
        let Some(t) = t else { return };
        self.selected = Some(id);
        self.set_playhead(ctx, t);
        let d = self.duration();
        if focus && self.tl.span > 3.0 {
            self.tl.span = 1.5;
            self.tl.start = self.playhead_s - 0.75;
            self.tl.clamp(d);
        } else {
            self.tl.ensure_visible(self.playhead_s, d);
        }
    }

    fn step_frames(&mut self, ctx: &egui::Context, n: i64) {
        let fps = self.fps();
        let t = self.playhead_s + n as f64 / fps;
        self.set_playhead(ctx, t);
        let d = self.duration();
        self.tl.ensure_visible(self.playhead_s, d);
    }

    fn jump_click(&mut self, ctx: &egui::Context, dir: i32) {
        if self.active.is_empty() {
            return;
        }
        let cur = self.selected.and_then(|id| self.active.iter().position(|c| c.cand_id == id));
        let fps = self.fps();
        let ph_frame = (self.playhead_s.max(0.0) * fps + 1e-6).floor() as i64;
        let picked: Option<usize> = if dir > 0 {
            match cur {
                Some(p) => self.active.get(p + 1).map(|c| c.cand_id),
                None => self.active.iter().find(|c| c.frame as i64 > ph_frame).map(|c| c.cand_id),
            }
        } else {
            match cur {
                Some(p) => p.checked_sub(1).and_then(|q| self.active.get(q)).map(|c| c.cand_id),
                None => self.active.iter().rev().find(|c| (c.frame as i64) < ph_frame).map(|c| c.cand_id),
            }
        };
        if let Some(id) = picked {
            self.select_click(ctx, id, true);
        }
    }

    fn learn_selected(&mut self) {
        let Some(id) = self.selected else { return };
        let sig = self.loaded.as_ref().and_then(|l| l.candidates.get(id)).and_then(|c| c.sig);
        if let Some(s) = sig {
            self.samples.push(s);
            self.template = mean_signature(&self.samples);
            self.refilter();
        }
    }

    fn export_json(&mut self) {
        let Some(l) = &self.loaded else { return };
        let out = Export {
            tool: "GeoProof 0.2",
            note: "Click measurements only. Timing patterns alone are not proof of cheating or legitimacy.",
            file: l.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            fps: l.info.fps,
            duration_s: l.info.duration_s,
            audio_offset_s: l.info.audio_offset(),
            band_hz: [self.params.f_lo, self.params.f_hi],
            threshold_k: self.params.k,
            click_sound_samples: self.samples.len(),
            min_similarity: if self.template.is_some() { Some(self.min_sim) } else { None },
            clicks: &self.active,
        };
        let target = l.path.with_extension("geoproof.json");
        match serde_json::to_string_pretty(&out) {
            Ok(json) => match std::fs::write(&target, json) {
                Ok(()) => self.status = format!("Saved {}", target.display()),
                Err(e) => self.error = Some(format!("Could not save report: {e}")),
            },
            Err(e) => self.error = Some(format!("Could not build report: {e}")),
        }
    }

    // ---------------------------------------------------------------- panels

    fn top_bar(&mut self, ui: &mut egui::Ui, act: &mut Actions) {
        ui.horizontal_centered(|ui| {
            ui.label(RichText::new("GeoProof").size(19.0).strong());
            ui.add_space(16.0);
            if ui.button("Open recording").clicked() {
                act.open = true;
            }
            if let Some(l) = &self.loaded {
                ui.add_space(12.0);
                let name = l.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ui.label(RichText::new(name).strong());
                ui.label(
                    RichText::new(format!(
                        "{:.2} fps   {}x{}   {:.1} s   audio offset {:+.1} ms",
                        l.info.fps,
                        l.info.width,
                        l.info.height,
                        l.info.duration_s,
                        l.info.audio_offset() * 1000.0
                    ))
                    .monospace()
                    .color(MUTED),
                );
                if l.info.is_variable_frame_rate() {
                    ui.label(RichText::new("variable frame rate: frame numbers are approximate").small().color(WARN));
                }
                if l.info.fps < 29.5 {
                    ui.label(RichText::new("under 30 fps: timing is coarse").small().color(WARN));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.loaded.is_some() && ui.button("Save report").clicked() {
                    act.export = true;
                }
                if self.busy {
                    ui.add(egui::ProgressBar::new(self.progress.frac()).desired_width(150.0));
                    ui.label(RichText::new(self.progress.stage()).color(MUTED));
                    ui.spinner();
                } else if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).color(MUTED));
                }
            });
        });
    }

    fn inspector(&mut self, ui: &mut egui::Ui, act: &mut Actions) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, RightTab::Inspect, "Inspect");
            ui.selectable_value(&mut self.tab, RightTab::Clicks, "Clicks");
        });
        ui.add_space(6.0);
        match self.tab {
            RightTab::Inspect => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    self.inspect_tab(ui, act);
                });
            }
            RightTab::Clicks => self.clicks_tab(ui, act),
        }
    }

    fn inspect_tab(&mut self, ui: &mut egui::Ui, act: &mut Actions) {
        section_label(ui, "Selected click");
        let sel = self.selected.and_then(|id| self.loaded.as_ref().and_then(|l| l.candidates.get(id)).cloned());
        let acc = sel.as_ref().and_then(|c| self.active.iter().find(|a| a.cand_id == c.cand_id).cloned());
        card(ui, |ui| match &sel {
            Some(c) => {
                kv(ui, "Time", fmt_timecode(c.t_video_s));
                kv(ui, "Frame", c.frame.to_string());
                kv(ui, "Position in frame", format!("{:.2}", c.subframe));
                position_bar(ui, c.subframe);
                ui.add_space(2.0);
                match &acc {
                    Some(a) => match (a.gap_ms, a.gap_frames) {
                        (Some(ms), Some(fr)) => kv(ui, "Gap before", format!("{ms:.1} ms  ({fr:.2} f)")),
                        _ => kv(ui, "Gap before", "-"),
                    },
                    None => {
                        ui.label(RichText::new("Rejected: does not match your click sound").small().color(WARN));
                    }
                }
                if let Some(s) = c.similarity {
                    kv(ui, "Match", format!("{s:.3}"));
                }
                kv(ui, "Strength", format!("{:.0}", c.strength));
                if c.sig.is_some() {
                    ui.add_space(4.0);
                    if ui.button("Add to click sound").clicked() {
                        act.learn = true;
                    }
                }
            }
            None => {
                ui.label(RichText::new("Click a red marker on the timeline, or use Ctrl+arrow keys.").color(MUTED));
            }
        });

        section_label(ui, "Click sound");
        card(ui, |ui| {
            ui.label(
                RichText::new(
                    "Select clean clicks and add them. GeoProof learns what your click sounds like and drops anything that does not match. Works best when clicks are louder than the music.",
                )
                .small()
                .color(MUTED),
            );
            ui.add_space(6.0);
            kv(ui, "Learned from", format!("{} click(s)", self.samples.len()));
            if self.template.is_some() {
                ui.add_space(4.0);
                if ui.add(egui::Slider::new(&mut self.min_sim, 0.80..=0.99).text("match").fixed_decimals(2)).changed() {
                    act.refilter = true;
                }
                if ui.button("Forget click sound").clicked() {
                    act.forget = true;
                }
            }
        });

        section_label(ui, "Recording");
        card(ui, |ui| match &self.loaded {
            Some(l) => {
                let n = self.active.len();
                let rejected = l.candidates.len().saturating_sub(n);
                kv(ui, "Clicks", n.to_string());
                if rejected > 0 {
                    kv(ui, "Rejected", rejected.to_string());
                }
                if l.info.duration_s > 0.0 {
                    kv(ui, "Clicks per second", format!("{:.2}", n as f64 / l.info.duration_s));
                }
                let gaps: Vec<f64> = self.active.iter().filter_map(|e| e.gap_frames).collect();
                if let Some(m) = median_of(gaps) {
                    kv(ui, "Median gap", format!("{m:.2} frames"));
                }
                kv(ui, "Frame rate", format!("{:.2} fps", l.info.fps));
            }
            None => {
                ui.label(RichText::new("No recording loaded.").color(MUTED));
            }
        });

        section_label(ui, "Position inside frame");
        card(ui, |ui| {
            subframe_histogram(ui, &self.active);
            ui.label(RichText::new("Where each click lands within its frame. Descriptive only.").small().color(MUTED));
        });

        section_label(ui, "Detection");
        card(ui, |ui| {
            ui.horizontal(|ui| {
                for s in [Sens::Sensitive, Sens::Balanced, Sens::Strict] {
                    if ui.selectable_value(&mut self.sens, s, s.label()).clicked() {
                        self.params.k = s.k();
                    }
                }
            });
            ui.label(RichText::new(self.sens.hint()).small().color(MUTED));
            ui.add_space(4.0);
            ui.add(egui::Slider::new(&mut self.params.f_lo, 500.0..=4000.0).suffix(" Hz").text("low"));
            ui.add(egui::Slider::new(&mut self.params.f_hi, 3000.0..=16000.0).suffix(" Hz").text("high"));
            ui.add_space(4.0);
            ui.add_enabled_ui(self.loaded.is_some() && !self.busy, |ui| {
                if ui.button("Detect again").clicked() {
                    act.redetect = true;
                }
            });
        });
        ui.add_space(10.0);
        ui.label(
            RichText::new(
                "GeoProof reports what it can measure in a recording. Timing patterns alone are not proof of cheating or of legitimacy.",
            )
            .small()
            .color(FAINT),
        );
    }

    fn clicks_tab(&mut self, ui: &mut egui::Ui, act: &mut Actions) {
        let rows = self.active.len();
        let selected = self.selected;
        let mut clicked: Option<usize> = None;
        TableBuilder::new(ui)
            .striped(true)
            .vscroll(true)
            .column(Column::exact(44.0))
            .column(Column::exact(86.0))
            .column(Column::exact(56.0))
            .column(Column::exact(46.0))
            .column(Column::remainder())
            .header(22.0, |mut h| {
                for t in ["#", "Time", "Frame", "Pos", "Gap (f)"] {
                    h.col(|ui| {
                        ui.label(RichText::new(t).strong().color(MUTED));
                    });
                }
            })
            .body(|body| {
                body.rows(20.0, rows, |mut row| {
                    let e = &self.active[row.index()];
                    let is_sel = selected == Some(e.cand_id);
                    row.col(|ui| {
                        if ui.selectable_label(is_sel, (e.index + 1).to_string()).clicked() {
                            clicked = Some(e.cand_id);
                        }
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(fmt_timecode(e.t_video_s)).monospace());
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(e.frame.to_string()).monospace());
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(format!("{:.2}", e.subframe)).monospace());
                    });
                    row.col(|ui| {
                        let g = e.gap_frames.map(|g| format!("{g:.2}")).unwrap_or_else(|| "-".into());
                        ui.label(RichText::new(g).monospace());
                    });
                });
            });
        if let Some(id) = clicked {
            act.select = Some((id, true));
        }
    }

    fn center(&mut self, ui: &mut egui::Ui, act: &mut Actions) {
        let avail = ui.available_rect_before_wrap();
        ui.allocate_rect(avail, Sense::hover());
        if self.loaded.is_none() {
            self.empty_state(ui, avail, act);
            return;
        }
        let transport_h = 52.0;
        let video_rect = Rect::from_min_max(avail.min, pos2(avail.max.x, avail.max.y - transport_h - 8.0));
        let transport_rect = Rect::from_min_max(pos2(avail.min.x, avail.max.y - transport_h), avail.max);

        let p = ui.painter().clone();
        p.rect_filled(video_rect, 8.0, Color32::BLACK);
        if let Some(tex) = &self.frame_tex {
            let sz = tex.size_vec2();
            let scale = (video_rect.width() / sz.x).min(video_rect.height() / sz.y);
            let r = Rect::from_center_size(video_rect.center(), sz * scale);
            p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            p.text(video_rect.center(), Align2::CENTER_CENTER, "Loading frame", FontId::proportional(14.0), MUTED);
        }
        let fps = self.fps();
        let frame = (self.playhead_s.max(0.0) * fps + 1e-6).floor() as u64;
        chip(
            &p,
            video_rect.left_top() + vec2(12.0, 12.0),
            &format!("FRAME {frame}   {}", fmt_timecode(self.playhead_s)),
            TEXT,
            Color32::from_rgba_unmultiplied(0, 0, 0, 175),
            video_rect,
        );
        if let Some(e) = &self.frame_err {
            chip(
                &p,
                video_rect.left_bottom() + vec2(12.0, -36.0),
                e,
                WARN,
                Color32::from_rgba_unmultiplied(0, 0, 0, 175),
                video_rect,
            );
        }

        ui.allocate_ui_at_rect(transport_rect, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(((ui.available_width() - 640.0) / 2.0).max(0.0));
                if ui.button("|<  Prev click").clicked() {
                    act.jump = -1;
                }
                if ui.button("<  Frame").clicked() {
                    act.step = -1;
                }
                ui.add_space(10.0);
                ui.label(RichText::new(fmt_timecode(self.playhead_s)).monospace().size(22.0).color(PLAYHEAD));
                ui.label(RichText::new(format!("F {frame}")).monospace().color(MUTED));
                ui.add_space(10.0);
                if ui.button("Frame  >").clicked() {
                    act.step = 1;
                }
                if ui.button("Next click  >|").clicked() {
                    act.jump = 1;
                }
            });
        });
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, avail: Rect, act: &mut Actions) {
        let r = Rect::from_center_size(avail.center(), vec2(560.0, 280.0));
        ui.allocate_ui_at_rect(r, |ui| {
            card(ui, |ui| {
                if self.busy {
                    ui.label(RichText::new("Analysing recording").size(20.0).strong());
                    ui.add_space(6.0);
                    ui.label(RichText::new(self.progress.stage()).color(MUTED));
                    ui.add_space(8.0);
                    let w = ui.available_width();
                    ui.add(egui::ProgressBar::new(self.progress.frac()).desired_width(w).animate(true));
                } else {
                    ui.label(RichText::new("Drop a recording to begin").size(22.0).strong());
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "GeoProof finds every click in the audio, places it on the exact video frame, and shows it on a timeline you can scrub.",
                        )
                        .color(MUTED),
                    );
                    ui.add_space(8.0);
                    ui.label(RichText::new("Raw video with the clicks audible. MP4, MKV, AVI, MOV or WebM.").small().color(FAINT));
                    ui.label(RichText::new("Everything is analysed on this computer.").small().color(FAINT));
                    ui.add_space(12.0);
                    if ui.add(egui::Button::new(RichText::new("Open recording").strong()).fill(SELECT)).clicked() {
                        act.open = true;
                    }
                }
            });
        });
    }

    fn timeline_panel(&mut self, ui: &mut egui::Ui, act: &mut Actions) {
        let Some(l) = &self.loaded else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("The timeline appears here once a recording is loaded.").color(MUTED));
            });
            return;
        };
        egui::Frame::none().inner_margin(Margin::symmetric(14.0, 8.0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("TIMELINE").size(10.5).strong().color(MUTED));
                ui.add_space(10.0);
                if ui.small_button("-").clicked() {
                    act.zoom = Some(1.6);
                }
                if ui.small_button("+").clicked() {
                    act.zoom = Some(1.0 / 1.6);
                }
                if ui.small_button("Fit").clicked() {
                    act.fit = true;
                }
                ui.add_space(10.0);
                ui.add(egui::Slider::new(&mut self.tl.gain, 1.0..=12.0).logarithmic(true).text("wave gain"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new("Scroll: zoom    Shift+scroll or right-drag: pan    Arrows: step frame    Ctrl+arrows: next click")
                            .small()
                            .color(FAINT),
                    );
                });
            });
        });
        let input = TimelineInput {
            pyramid: &l.pyramid,
            audio_offset_s: l.info.audio_offset(),
            duration_s: l.info.duration_s,
            fps: l.info.fps,
            candidates: &l.candidates,
            playhead_s: self.playhead_s,
            selected: self.selected,
        };
        let out = timeline::show(ui, &input, &mut self.tl);
        if let Some(t) = out.seek {
            act.seek = Some(t);
        }
        if let Some(id) = out.select {
            act.select = Some((id, false));
        }
    }
}

impl eframe::App for GeoProofApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);

        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(path) = dropped.first().and_then(|f| f.path.clone()) {
            self.start_load(ctx, path);
        }

        let mut act = Actions::default();

        if self.loaded.is_some() && !ctx.wants_keyboard_input() {
            ctx.input(|i| {
                let big: i64 = if i.modifiers.shift { 10 } else { 1 };
                let ctrl = i.modifiers.command;
                if i.key_pressed(Key::ArrowLeft) {
                    if ctrl {
                        act.jump = -1;
                    } else {
                        act.step = -big;
                    }
                }
                if i.key_pressed(Key::ArrowRight) {
                    if ctrl {
                        act.jump = 1;
                    } else {
                        act.step = big;
                    }
                }
            });
        }

        egui::TopBottomPanel::top("bar")
            .exact_height(54.0)
            .frame(egui::Frame::none().fill(PANEL).inner_margin(Margin::symmetric(16.0, 8.0)))
            .show(ctx, |ui| {
                self.top_bar(ui, &mut act);
            });

        egui::TopBottomPanel::bottom("timeline")
            .resizable(true)
            .default_height(320.0)
            .min_height(230.0)
            .frame(egui::Frame::none().fill(BG))
            .show(ctx, |ui| {
                self.timeline_panel(ui, &mut act);
            });

        egui::SidePanel::right("inspector")
            .exact_width(330.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(Margin::same(14.0)),
            )
            .show(ctx, |ui| {
                self.inspector(ui, &mut act);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(Margin::same(14.0)))
            .show(ctx, |ui| {
                self.center(ui, &mut act);
            });

        if act.open {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Video", &["mp4", "mkv", "avi", "mov", "webm"])
                .pick_file()
            {
                self.start_load(ctx, path);
            }
        }
        if act.redetect {
            self.redetect(ctx);
        }
        if act.export {
            self.export_json();
        }
        if act.learn {
            self.learn_selected();
        }
        if act.forget {
            self.samples.clear();
            self.template = None;
            self.refilter();
        }
        if act.refilter {
            self.refilter();
        }
        if let Some((id, focus)) = act.select {
            self.select_click(ctx, id, focus);
        }
        if let Some(t) = act.seek {
            self.set_playhead(ctx, t);
        }
        if act.step != 0 {
            self.step_frames(ctx, act.step);
        }
        if act.jump != 0 {
            self.jump_click(ctx, act.jump);
        }
        if let Some(f) = act.zoom {
            let d = self.duration();
            let c = self.playhead_s.max(0.0);
            self.tl.zoom_at(f, c, d);
        }
        if act.fit {
            let d = self.duration();
            self.tl.fit(d);
        }
    }
}
