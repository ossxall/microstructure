use eframe::egui::{self, Color32, RichText, Stroke};
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints, Polygon};

const BACKGROUND: Color32 = Color32::from_rgb(8, 12, 10);
const PANEL: Color32 = Color32::from_rgb(14, 21, 17);
const GREEN: Color32 = Color32::from_rgb(48, 190, 120);
const RED: Color32 = Color32::from_rgb(210, 82, 82);
const GRID: Color32 = Color32::from_rgb(38, 68, 51);
const TEXT: Color32 = Color32::from_rgb(205, 222, 211);
const MUTED: Color32 = Color32::from_rgb(119, 145, 128);

#[derive(Clone, Copy)]
struct Candle {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

impl Candle {
    fn is_bullish(self) -> bool {
        self.close >= self.open
    }
}

struct ChartApp {
    candles: Vec<Candle>,
}

impl ChartApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        set_green_dark_theme(&cc.egui_ctx);

        Self {
            candles: sample_candles(),
        }
    }

    fn show_price_chart(&self, ui: &mut egui::Ui) {
        let min_price = self
            .candles
            .iter()
            .map(|candle| candle.low)
            .fold(f64::INFINITY, f64::min);
        let max_price = self
            .candles
            .iter()
            .map(|candle| candle.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let price_padding = (max_price - min_price) * 0.08;
        let x_max = self.candles.len() as f64;
        let x_link = egui::Id::unique("ohlcv-time-axis");

        Plot::new("ohlcv-price")
            .height(410.0)
            .default_x_bounds(-1.0, x_max)
            .default_y_bounds(min_price - price_padding, max_price + price_padding)
            .link_axis(x_link, egui::Vec2b::new(true, false))
            .grid_color(GRID)
            .show_axes(egui::Vec2b::new(false, true))
            .show_x(false)
            .allow_boxed_zoom(false)
            .show(ui, |plot_ui| {
                for (index, candle) in self.candles.iter().copied().enumerate() {
                    let x = index as f64;
                    let color = if candle.is_bullish() { GREEN } else { RED };
                    let body_low = candle.open.min(candle.close);
                    let body_high = candle.open.max(candle.close);
                    let body_height = (body_high - body_low).max(0.000015);
                    let body_low = (body_low + body_high - body_height) * 0.5;
                    let body_high = body_low + body_height;
                    let half_width = 0.32;

                    plot_ui.line(
                        Line::new(
                            format!("wick-{index}"),
                            PlotPoints::from(vec![[x, candle.low], [x, candle.high]]),
                        )
                        .color(color)
                        .width(1.2)
                        .allow_hover(false),
                    );

                    plot_ui.polygon(
                        Polygon::new(
                            format!("candle-{index}"),
                            vec![
                                [x - half_width, body_low],
                                [x + half_width, body_low],
                                [x + half_width, body_high],
                                [x - half_width, body_high],
                            ],
                        )
                        .fill_color(color)
                        .stroke(Stroke::new(1.0, color))
                        .allow_hover(false),
                    );
                }
            });
    }

    fn show_volume_chart(&self, ui: &mut egui::Ui) {
        let x_max = self.candles.len() as f64;
        let max_volume = self
            .candles
            .iter()
            .map(|candle| candle.volume)
            .fold(0.0, f64::max);
        let bars = self
            .candles
            .iter()
            .copied()
            .enumerate()
            .map(|(index, candle)| {
                let color = if candle.is_bullish() { GREEN } else { RED };
                Bar::new(index as f64, candle.volume)
                    .width(0.64)
                    .fill(color.linear_multiply(0.62))
                    .stroke(Stroke::new(1.0, color))
            })
            .collect();

        Plot::new("ohlcv-volume")
            .height(135.0)
            .default_x_bounds(-1.0, x_max)
            .default_y_bounds(0.0, max_volume * 1.25)
            .link_axis(
                egui::Id::unique("ohlcv-time-axis"),
                egui::Vec2b::new(true, false),
            )
            .grid_color(GRID)
            .show_axes(egui::Vec2b::new(true, false))
            .show_y(false)
            .allow_boxed_zoom(false)
            .show(ui, |plot_ui| {
                plot_ui.bar_chart(BarChart::new("Volumen", bars).allow_hover(false));
            });
    }
}

impl eframe::App for ChartApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(14.0))
            .show(ui, |ui| {
                let last = self.candles.last().copied().unwrap_or(Candle {
                    open: 0.0,
                    high: 0.0,
                    low: 0.0,
                    close: 0.0,
                    volume: 0.0,
                });
                let change_color = if last.is_bullish() { GREEN } else { RED };

                ui.horizontal(|ui| {
                    ui.heading(RichText::new("EURUSD").strong().color(TEXT));
                    ui.label(
                        RichText::new("M5")
                            .strong()
                            .color(GREEN)
                            .background_color(Color32::from_rgb(22, 48, 34)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("● MERCADO ABIERTO").color(GREEN).small());
                        ui.label(RichText::new("FX · Demo").color(MUTED).small());
                    });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("O").color(MUTED).small());
                    ui.label(
                        RichText::new(format!("{:.5}", last.open))
                            .color(TEXT)
                            .monospace(),
                    );
                    ui.label(RichText::new("H").color(MUTED).small());
                    ui.label(
                        RichText::new(format!("{:.5}", last.high))
                            .color(TEXT)
                            .monospace(),
                    );
                    ui.label(RichText::new("L").color(MUTED).small());
                    ui.label(
                        RichText::new(format!("{:.5}", last.low))
                            .color(TEXT)
                            .monospace(),
                    );
                    ui.label(RichText::new("C").color(MUTED).small());
                    ui.label(
                        RichText::new(format!("{:.5}", last.close))
                            .color(change_color)
                            .monospace()
                            .strong(),
                    );
                    ui.label(RichText::new("VOL").color(MUTED).small());
                    ui.label(
                        RichText::new(format!("{:.0}", last.volume))
                            .color(TEXT)
                            .monospace(),
                    );
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                egui::Frame::new()
                    .fill(PANEL)
                    .corner_radius(egui::CornerRadius::same(5))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        self.show_price_chart(ui);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("VOLUMEN").color(MUTED).small().strong());
                            ui.label(RichText::new("·  ticks").color(MUTED).small());
                        });
                        self.show_volume_chart(ui);
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").color(GREEN));
                    ui.label(RichText::new("Alcista").color(MUTED).small());
                    ui.label(RichText::new("●").color(RED));
                    ui.label(RichText::new("Bajista").color(MUTED).small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(
                                "Datos de ejemplo · desplaza para zoom · arrastra para mover",
                            )
                            .color(MUTED)
                            .small(),
                        );
                    });
                });
            });
    }
}

fn set_green_dark_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(MUTED);
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = BACKGROUND;
    visuals.extreme_bg_color = Color32::from_rgb(5, 8, 6);
    visuals.faint_bg_color = PANEL;
    visuals.hyperlink_color = GREEN;
    visuals.selection.bg_fill = Color32::from_rgb(30, 92, 57);
    visuals.selection.stroke = Stroke::new(1.0, GREEN);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(28, 56, 39);
    visuals.widgets.active.bg_fill = Color32::from_rgb(35, 82, 52);
    ctx.set_visuals(visuals);
}

fn sample_candles() -> Vec<Candle> {
    let mut candles = Vec::with_capacity(72);
    let mut previous_close = 1.08420;

    for index in 0..72 {
        let wave =
            (index as f64 * 0.37).sin() * 0.00022 + ((index * 17 % 11) as f64 - 5.0) * 0.000025;
        let open = previous_close;
        let close = open + wave;
        let wick = ((index * 7 % 9) as f64 + 2.0) * 0.000022;

        candles.push(Candle {
            open,
            high: open.max(close) + wick,
            low: open.min(close) - wick * 0.8,
            close,
            volume: 65.0 + (index * 29 + 13) as f64 % 90.0 + wave.abs() * 100_000.0,
        });

        previous_close = close;
    }

    candles
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([760.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Native UI · OHLCV",
        options,
        Box::new(|cc| Ok(Box::new(ChartApp::new(cc)))),
    )
}
