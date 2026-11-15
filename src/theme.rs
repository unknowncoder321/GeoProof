//! Visual identity: a neutral graphite editor look. Chrome stays quiet; the
//! only saturated colours are data: teal audio, red clicks, amber playhead.

use eframe::egui::{
    self, Align, Color32, FontFamily, FontId, Frame, Layout, Margin, RichText, Rounding, Stroke, TextStyle, Ui,
};

pub const BG: Color32 = Color32::from_rgb(10, 11, 14);
pub const PANEL: Color32 = Color32::from_rgb(19, 21, 26);
pub const RAISED: Color32 = Color32::from_rgb(28, 31, 38);
pub const HOVER: Color32 = Color32::from_rgb(38, 42, 52);
pub const ACTIVE: Color32 = Color32::from_rgb(48, 54, 68);
pub const BORDER: Color32 = Color32::from_rgb(40, 44, 54);
pub const TEXT: Color32 = Color32::from_rgb(230, 233, 238);
pub const MUTED: Color32 = Color32::from_rgb(134, 142, 158);
pub const FAINT: Color32 = Color32::from_rgb(86, 93, 108);
pub const SELECT: Color32 = Color32::from_rgb(52, 66, 96);

pub const WAVE: Color32 = Color32::from_rgb(64, 170, 186);
pub const CLICK: Color32 = Color32::from_rgb(255, 64, 82);
pub const CLICK_DIM: Color32 = Color32::from_rgb(104, 46, 54);
pub const PLAYHEAD: Color32 = Color32::from_rgb(255, 196, 64);
pub const WARN: Color32 = Color32::from_rgb(232, 126, 98);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.faint_bg_color = Color32::from_rgb(23, 26, 32);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_rounding = Rounding::same(10.0);
    v.selection.bg_fill = SELECT;
    v.selection.stroke = Stroke::new(1.0, TEXT);

    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);

    v.widgets.inactive.bg_fill = RAISED;
    v.widgets.inactive.weak_bg_fill = RAISED;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);

    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(74, 82, 100));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);

    v.widgets.active.bg_fill = ACTIVE;
    v.widgets.active.weak_bg_fill = ACTIVE;
    v.widgets.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(96, 106, 130));
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.rounding = Rounding::same(6.0);
    }
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 28.0;
    style.text_styles.insert(TextStyle::Heading, FontId::new(16.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Button, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace));
    ctx.set_style(style);
}

/// Raised rounded container used for inspector sections.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::none()
        .fill(RAISED)
        .stroke(Stroke::new(1.0, BORDER))
        .rounding(8.0)
        .inner_margin(Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn section_label(ui: &mut Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title.to_uppercase()).size(10.5).strong().color(MUTED));
}

/// Key on the left, monospace value on the right.
pub fn kv(ui: &mut Ui, key: &str, value: impl Into<String>) {
    let value: String = value.into();
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).color(MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).monospace());
        });
    });
}
