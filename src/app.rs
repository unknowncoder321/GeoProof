use crate::analysis::{map_to_frames, ClickEvent};
use crate::detect::{self, Params};
use crate::media::{self, MediaInfo, SAMPLE_RATE};
use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column, TableBuilder};
use egui_plot::{Line, Plot, PlotPoints, Points, VLine};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};

const INK: Color32 = Color32::from_rgb(16, 22, 33);
const PANEL: Color32 = Color32::from_rgb(22, 30, 44);
const RAISED: Color32 = Color32::from_rgb(31, 42, 60);
const TEXT: Color32 = Color32::from_rgb(232, 228, 218);
const MUTED: Color32 = Color32::from_rgb(138, 150, 170);
const WAVE: Color32 = Color32::from_rgb(88, 160, 168);
const CLICK: Color32 = Color32::from_rgb(242, 184, 75);
const WARN: Color32 = Color32::from_rgb(226, 120, 96);

pub fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = INK;
    v.faint_bg_color = RAISED;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = Color32::from_rgb(120, 92, 38);
    v.widgets.inactive.bg_fill = RAISED;
    v.widgets.inactive.weak_bg_fill = RAISED;
    ctx.set_visuals(v);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    ctx.set_style(style);
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

struct Loaded {
    path: PathBuf,
    info: MediaInfo,
    audio: Arc<Vec<f32>>,
    wave_hi: Vec<[f64; 2]>,
    wave_lo: Vec<[f64; 2]>,
    events: Vec<ClickEvent>,
}

enum Msg {
    Loaded(Box<Loaded>),
    Redetected(Vec<ClickEvent>),
    Failed(String),
}

pub struct GeoProofApp {
    rx: Option<mpsc::Receiver<Msg>>,
    busy: bool,
    status: String,
    error: Option<String>,
    loaded: Option<Loaded>,
    sens: Sens,
    params: Params,
}

impl Default for GeoProofApp {
    fn default() -> Self {
        GeoProofApp {
            rx: None,
            busy: false,
            status: String::new(),
            error: None,
            loaded: None,
            sens: Sens::Balanced,
            params: Params::default(),
        }
    }
}

/// Min/max envelope for drawing; at most ~6000 buckets however long the recording is.
fn envelope(audio: &[f32], offset: f64) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
    let bucket = (audio.len() / 6000).max(1);
    let mut hi = Vec::new();
    let mut lo = Vec::new();
    for (n, chunk) in audio.chunks(bucket).enumerate() {
        let mut mn = f32::MAX;
        let mut mx = f32::MIN;
        for &v in chunk {
            mn = mn.min(v);
            mx = mx.max(v);
        }
        let t = (n * bucket + chunk.len() / 2) as f64 / SAMPLE_RATE as f64 + offset;
        hi.push([t, mx as f64]);
        lo.push([t, mn as f64]);
    }
    (hi, lo)
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
    clicks: &'a [ClickEvent],
}

impl GeoProofApp {
    fn start_load(&mut self, ctx: &egui::Context, path: PathBuf) {
        let params = self.params.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        self.error = None;
        self.status = "Reading audio and finding clicks".into();
        std::thread::spawn(move || {
            let res = (|| -> anyhow::Result<Loaded> {
                let info = media::probe(&path)?;
                let audio = media::decode_audio(&path)?;
                let onsets = detect::detect(&audio, SAMPLE_RATE as f64, &params);
                let events = map_to_frames(&onsets, &info);
                let (wave_hi, wave_lo) = envelope(&audio, info.audio_offset());
                Ok(Loaded { path, info, audio: Arc::new(audio), wave_hi, wave_lo, events })
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
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        self.status = "Finding clicks".into();
        std::thread::spawn(move || {
            let onsets = detect::detect(&audio, SAMPLE_RATE as f64, &params);
            let _ = tx.send(Msg::Redetected(map_to_frames(&onsets, &info)));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        let msg = self.rx.as_ref().and_then(|rx| rx.try_recv().ok());
        let Some(msg) = msg else { return };
        self.busy = false;
        match msg {
            Msg::Loaded(l) => {
                self.status = format!("{} clicks found", l.events.len());
                self.loaded = Some(*l);
            }
            Msg::Redetected(ev) => {
                self.status = format!("{} clicks found", ev.len());
                if let Some(l) = &mut self.loaded {
                    l.events = ev;
                }
            }
            Msg::Failed(e) => {
                self.error = Some(e);
                self.status.clear();
            }
        }
    }

    fn export_json(&mut self) {
        let Some(l) = &self.loaded else { return };
        let out = Export {
            tool: "GeoProof 0.1",
            note: "Click measurements only. Timing patterns alone are not proof of cheating or legitimacy.",
            file: l.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            fps: l.info.fps,
            duration_s: l.info.duration_s,
            audio_offset_s: l.info.audio_offset(),
            band_hz: [self.params.f_lo, self.params.f_hi],
            threshold_k: self.params.k,
            clicks: &l.events,
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
}

impl eframe::App for GeoProofApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();

        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(path) = dropped.first().and_then(|f| f.path.clone()) {
            self.start_load(ctx, path);
        }

        let mut open_clicked = false;
        let mut redetect = false;
        let mut export = false;

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("GeoProof").strong().size(18.0));
                ui.add_space(12.0);
                if ui.button("Open recording").clicked() {
                    open_clicked = true;
                }
                if self.busy {
                    ui.spinner();
                }
                ui.label(RichText::new(&self.status).color(MUTED));
            });
            if let Some(l) = &self.loaded {
                ui.horizontal_wrapped(|ui| {
                    let name = l.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.label(RichText::new(name).strong());
                    ui.label(RichText::new(format!(
                        "{:.2} fps   {}x{}   {:.1} s   audio offset {:+.1} ms",
                        l.info.fps,
                        l.info.width,
                        l.info.height,
                        l.info.duration_s,
                        l.info.audio_offset() * 1000.0
                    )).color(MUTED));
                });
                if l.info.is_variable_frame_rate() {
                    ui.label(RichText::new(
                        "Variable frame rate detected. Frame numbers are approximate; ask for a constant-frame-rate recording.",
                    ).color(WARN));
                }
                if l.info.fps < 29.5 {
                    ui.label(RichText::new("Under 30 fps: click timing is coarse.").color(WARN));
                }
            }
            ui.add_space(6.0);
        });

        egui::SidePanel::left("side").exact_width(270.0).show(ctx, |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("Click detection").strong().size(15.0));
            ui.add_space(4.0);
            for s in [Sens::Sensitive, Sens::Balanced, Sens::Strict] {
                if ui.radio_value(&mut self.sens, s, s.label()).changed() {
                    self.params.k = s.k();
                }
            }
            ui.label(RichText::new(self.sens.hint()).small().color(MUTED));
            ui.add_space(6.0);
            ui.collapsing("Frequency band", |ui| {
                ui.add(egui::Slider::new(&mut self.params.f_lo, 500.0..=4000.0).suffix(" Hz").text("low"));
                ui.add(egui::Slider::new(&mut self.params.f_hi, 3000.0..=16000.0).suffix(" Hz").text("high"));
                ui.label(RichText::new("Narrow this to where your click sound lives to ignore cymbals and bass.").small().color(MUTED));
            });
            ui.add_space(6.0);
            ui.add_enabled_ui(self.loaded.is_some() && !self.busy, |ui| {
                if ui.button("Detect again").clicked() {
                    redetect = true;
                }
            });

            if let Some(l) = &self.loaded {
                ui.add_space(14.0);
                ui.separator();
                ui.label(RichText::new("This recording").strong().size(15.0));
                let n = l.events.len();
                ui.label(format!("{n} clicks"));
                if l.info.duration_s > 0.0 {
                    ui.label(format!("{:.1} clicks per second on average", n as f64 / l.info.duration_s));
                }
                let gaps: Vec<f64> = l.events.iter().filter_map(|e| e.gap_ms).collect();
                if let Some(m) = median_of(gaps) {
                    ui.label(format!("Median gap {:.0} ms ({:.1} frames)", m, m / 1000.0 * l.info.fps));
                }
                ui.add_space(8.0);
                if ui.button("Save report (JSON)").clicked() {
                    export = true;
                }
            }

            ui.add_space(14.0);
            ui.separator();
            ui.label(
                RichText::new(
                    "GeoProof reports what it can measure in a recording. Timing patterns alone are not proof of cheating or of legitimacy.",
                )
                .small()
                .color(MUTED),
            );
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = &self.error {
                ui.label(RichText::new(err).color(WARN));
                ui.add_space(8.0);
            }
            let Some(l) = &self.loaded else {
                ui.add_space(60.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Drop a recording here").size(22.0).strong());
                    ui.add_space(6.0);
                    ui.label(RichText::new("Use the raw video with the click audio audible. MP4, MKV, AVI, MOV and WebM work.").color(MUTED));
                });
                return;
            };

            let dur = l.info.duration_s.max(1.0);
            let avail = ui.available_height();

            ui.label(RichText::new("Audio and detected clicks (drag to pan, Ctrl+scroll to zoom)").color(MUTED));
            Plot::new("wave").height(avail * 0.33).include_x(0.0).include_x(dur).show(ui, |p| {
                p.line(Line::new(PlotPoints::from(l.wave_hi.clone())).color(WAVE));
                p.line(Line::new(PlotPoints::from(l.wave_lo.clone())).color(WAVE));
                for e in &l.events {
                    p.vline(VLine::new(e.t_video_s).color(CLICK).width(1.0));
                }
            });

            ui.add_space(6.0);
            ui.label(RichText::new("Gap between clicks, in video frames (gaps over 40 frames hidden)").color(MUTED));
            let pts: Vec<[f64; 2]> = l
                .events
                .iter()
                .filter_map(|e| e.gap_frames.filter(|g| *g <= 40.0).map(|g| [e.t_video_s, g]))
                .collect();
            Plot::new("gaps").height(avail * 0.21).include_x(0.0).include_x(dur).include_y(0.0).show(ui, |p| {
                p.points(Points::new(PlotPoints::from(pts)).radius(2.5).color(CLICK));
            });

            ui.add_space(6.0);
            TableBuilder::new(ui)
                .striped(true)
                .vscroll(true)
                .column(Column::exact(50.0))
                .column(Column::exact(100.0))
                .column(Column::exact(80.0))
                .column(Column::exact(110.0))
                .column(Column::exact(90.0))
                .column(Column::exact(100.0))
                .column(Column::remainder())
                .header(22.0, |mut h| {
                    for t in ["#", "Time (s)", "Frame", "Position in frame", "Gap (ms)", "Gap (frames)", "Strength"] {
                        h.col(|ui| {
                            ui.strong(t);
                        });
                    }
                })
                .body(|body| {
                    body.rows(18.0, l.events.len(), |mut row| {
                        let e = &l.events[row.index()];
                        row.col(|ui| {
                            ui.label(format!("{}", e.index + 1));
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.4}", e.t_video_s));
                        });
                        row.col(|ui| {
                            ui.label(format!("{}", e.frame));
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.2}", e.subframe));
                        });
                        row.col(|ui| {
                            ui.label(e.gap_ms.map(|g| format!("{g:.1}")).unwrap_or_else(|| "-".into()));
                        });
                        row.col(|ui| {
                            ui.label(e.gap_frames.map(|g| format!("{g:.2}")).unwrap_or_else(|| "-".into()));
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.0}", e.strength));
                        });
                    });
                });
        });

        if open_clicked {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Video", &["mp4", "mkv", "avi", "mov", "webm"])
                .pick_file()
            {
                self.start_load(ctx, path);
            }
        }
        if redetect {
            self.redetect(ctx);
        }
        if export {
            self.export_json();
        }
    }
}
