use eframe::egui::{self, Color32, Stroke};

pub const BACKGROUND: Color32 = Color32::BLACK;
pub const PANEL: Color32 = Color32::BLACK;
pub const LIME: Color32 = Color32::from_rgb(0, 255, 0);
pub const VOLUME: Color32 = Color32::from_rgb(50, 205, 50);
pub const EMA_55: Color32 = Color32::from_rgb(0, 255, 255);
pub const EMA_200: Color32 = Color32::from_rgb(30, 144, 255);
pub const PROFILE_BULLISH: Color32 = Color32::from_rgb(30, 144, 255);
pub const PROFILE_BEARISH: Color32 = Color32::from_rgb(255, 0, 0);
pub const PROFILE_NEUTRAL: Color32 = Color32::from_rgb(119, 136, 153);
pub const GRID: Color32 = Color32::from_rgb(119, 136, 153);
pub const TEXT: Color32 = Color32::WHITE;
pub const MUTED: Color32 = GRID;

pub fn set_green_dark_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(MUTED);
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = BACKGROUND;
    visuals.extreme_bg_color = BACKGROUND;
    visuals.faint_bg_color = PANEL;
    visuals.hyperlink_color = LIME;
    visuals.selection.bg_fill = Color32::from_rgb(30, 92, 57);
    visuals.selection.stroke = Stroke::new(1.0, LIME);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(28, 56, 39);
    visuals.widgets.active.bg_fill = Color32::from_rgb(35, 82, 52);
    ctx.set_visuals(visuals);
}
