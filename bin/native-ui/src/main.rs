use eframe::egui::{self, Color32, RichText, Stroke};
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints, Polygon};

const BACKGROUND: Color32 = Color32::BLACK;
const PANEL: Color32 = Color32::BLACK;
const LIME: Color32 = Color32::from_rgb(0, 255, 0);
const LIME_GREEN: Color32 = Color32::from_rgb(50, 205, 50);
const GRID: Color32 = Color32::from_rgb(119, 136, 153);
const TEXT: Color32 = Color32::WHITE;
const MUTED: Color32 = GRID;

#[derive(Clone, Copy)]
struct Candle {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

struct ChartTab {
    symbol: String,
    timeframe: String,
    candles: Vec<Candle>,
}

impl Candle {
    fn is_bullish(self) -> bool {
        self.close >= self.open
    }
}

struct ChartApp {
    tabs: Vec<ChartTab>,
    active_tab: usize,
}

impl ChartApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        set_green_dark_theme(&cc.egui_ctx);

        Self {
            tabs: vec![make_tab("BTCUSDT", "5m"), make_tab("ADAUSDT", "1m")],
            active_tab: 0,
        }
    }

    fn show_tabs(&mut self, ui: &mut egui::Ui) {
        let mut activate_tab = None;
        let mut close_tab = None;
        let mut add_tab = None;

        egui::Frame::new()
            .fill(PANEL)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.menu_button(RichText::new("+").strong().color(LIME), |ui| {
                        ui.set_min_width(150.0);
                        for symbol in ["BTCUSDT", "ETHUSDT", "ADAUSDT", "SOLUSDT"] {
                            ui.menu_button(symbol, |ui| {
                                for timeframe in ["1m", "5m", "15m", "1h", "4h"] {
                                    if ui.button(timeframe).clicked() {
                                        add_tab = Some((symbol.to_string(), timeframe.to_string()));
                                    }
                                }
                            });
                        }
                    })
                    .response
                    .on_hover_text("Añadir símbolo y temporalidad");

                    egui::ScrollArea::horizontal()
                        .id_salt("chart-tabs-scroll")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (index, tab) in self.tabs.iter().enumerate() {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    if ui
                                        .selectable_label(
                                            self.active_tab == index,
                                            RichText::new(format!(
                                                "{}  {}",
                                                tab.symbol, tab.timeframe
                                            ))
                                            .small()
                                            .strong(),
                                        )
                                        .clicked()
                                    {
                                        activate_tab = Some(index);
                                    }

                                    if self.tabs.len() > 1
                                        && ui
                                            .small_button("×")
                                            .on_hover_text("Cerrar pestaña")
                                            .clicked()
                                    {
                                        close_tab = Some(index);
                                    }
                                }
                            });
                        });
                });
            });

        if let Some(index) = close_tab {
            self.tabs.remove(index);
            if self.active_tab > index {
                self.active_tab -= 1;
            } else if self.active_tab == index {
                self.active_tab = self.active_tab.min(self.tabs.len() - 1);
            }
        } else if let Some((symbol, timeframe)) = add_tab {
            if let Some(index) = self
                .tabs
                .iter()
                .position(|tab| tab.symbol == symbol && tab.timeframe == timeframe)
            {
                self.active_tab = index;
            } else {
                self.tabs.push(make_tab(&symbol, &timeframe));
                self.active_tab = self.tabs.len() - 1;
            }
        } else if let Some(index) = activate_tab {
            self.active_tab = index;
        }
    }

    fn show_price_chart(&self, ui: &mut egui::Ui, tab: &ChartTab) {
        let min_price = tab
            .candles
            .iter()
            .map(|candle| candle.low)
            .fold(f64::INFINITY, f64::min);
        let max_price = tab
            .candles
            .iter()
            .map(|candle| candle.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let price_padding = (max_price - min_price) * 0.08;
        let x_max = tab.candles.len() as f64;
        let x_link = egui::Id::NULL.with(("ohlcv-time-axis", &tab.symbol, &tab.timeframe));

        Plot::new(("ohlcv-price", &tab.symbol, &tab.timeframe))
            .height(410.0)
            .default_x_bounds(-1.0, x_max)
            .default_y_bounds(min_price - price_padding, max_price + price_padding)
            .link_axis(x_link, egui::Vec2b::new(true, false))
            .grid_color(GRID)
            .show_axes(egui::Vec2b::new(false, true))
            .show_x(false)
            .allow_boxed_zoom(false)
            .show(ui, |plot_ui| {
                for (index, candle) in tab.candles.iter().copied().enumerate() {
                    let x = index as f64;
                    let body_color = if candle.is_bullish() {
                        Color32::BLACK
                    } else {
                        Color32::WHITE
                    };
                    let body_low = candle.open.min(candle.close);
                    let body_high = candle.open.max(candle.close);
                    let body_height = (body_high - body_low).max(candle.open.abs() * 0.000002);
                    let body_low = (body_low + body_high - body_height) * 0.5;
                    let body_high = body_low + body_height;
                    let half_width = 0.32;

                    plot_ui.line(
                        Line::new(
                            format!("wick-{index}"),
                            PlotPoints::from(vec![[x, candle.low], [x, candle.high]]),
                        )
                        .color(LIME)
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
                        .fill_color(body_color)
                        .stroke(Stroke::new(1.0, LIME))
                        .allow_hover(false),
                    );
                }
            });
    }

    fn show_volume_chart(&self, ui: &mut egui::Ui, tab: &ChartTab) {
        let x_max = tab.candles.len() as f64;
        let max_volume = tab
            .candles
            .iter()
            .map(|candle| candle.volume)
            .fold(0.0, f64::max);
        let bars = tab
            .candles
            .iter()
            .copied()
            .enumerate()
            .map(|(index, candle)| {
                Bar::new(index as f64, candle.volume)
                    .width(0.64)
                    .fill(LIME_GREEN)
                    .stroke(Stroke::new(1.0, LIME_GREEN))
            })
            .collect();

        Plot::new(("ohlcv-volume", &tab.symbol, &tab.timeframe))
            .height(135.0)
            .default_x_bounds(-1.0, x_max)
            .default_y_bounds(0.0, max_volume * 1.25)
            .link_axis(
                egui::Id::NULL.with(("ohlcv-time-axis", &tab.symbol, &tab.timeframe)),
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
                self.show_tabs(ui);
                ui.add_space(8.0);

                let tab = &self.tabs[self.active_tab];
                let last = tab.candles.last().copied().unwrap_or(Candle {
                    open: 0.0,
                    high: 0.0,
                    low: 0.0,
                    close: 0.0,
                    volume: 0.0,
                });
                let change_color = if last.is_bullish() { LIME } else { TEXT };
                let precision = if tab.symbol == "ADAUSDT" { 4 } else { 2 };

                ui.horizontal(|ui| {
                    ui.heading(RichText::new(&tab.symbol).strong().color(TEXT));
                    ui.label(
                        RichText::new(&tab.timeframe)
                            .strong()
                            .color(LIME)
                            .background_color(Color32::from_rgb(22, 48, 34)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("● MERCADO ABIERTO").color(LIME).small());
                        ui.label(RichText::new("CRYPTO · Demo").color(MUTED).small());
                    });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("O").color(MUTED).small());
                    ui.label(
                        RichText::new(format!("{:.*}", precision, last.open))
                            .color(TEXT)
                            .monospace(),
                    );
                    ui.label(RichText::new("H").color(MUTED).small());
                    ui.label(
                        RichText::new(format!("{:.*}", precision, last.high))
                            .color(TEXT)
                            .monospace(),
                    );
                    ui.label(RichText::new("L").color(MUTED).small());
                    ui.label(
                        RichText::new(format!("{:.*}", precision, last.low))
                            .color(TEXT)
                            .monospace(),
                    );
                    ui.label(RichText::new("C").color(MUTED).small());
                    ui.label(
                        RichText::new(format!("{:.*}", precision, last.close))
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
                        self.show_price_chart(ui, tab);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("VOLUMEN").color(MUTED).small().strong());
                            ui.label(RichText::new("·  ticks").color(MUTED).small());
                        });
                        self.show_volume_chart(ui, tab);
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").color(LIME));
                    ui.label(RichText::new("Alcista").color(MUTED).small());
                    ui.label(RichText::new("●").color(TEXT));
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
    visuals.extreme_bg_color = BACKGROUND;
    visuals.faint_bg_color = PANEL;
    visuals.hyperlink_color = LIME;
    visuals.selection.bg_fill = Color32::from_rgb(30, 92, 57);
    visuals.selection.stroke = Stroke::new(1.0, LIME);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(28, 56, 39);
    visuals.widgets.active.bg_fill = Color32::from_rgb(35, 82, 52);
    ctx.set_visuals(visuals);
}

fn make_tab(symbol: &str, timeframe: &str) -> ChartTab {
    let start_price = match symbol {
        "BTCUSDT" => 68_000.0,
        "ETHUSDT" => 3_500.0,
        "ADAUSDT" => 0.75,
        "SOLUSDT" => 150.0,
        _ => 1.0,
    };
    let volatility = match timeframe {
        "1m" => 0.6,
        "5m" => 1.0,
        "15m" => 1.6,
        "1h" => 2.5,
        "4h" => 4.0,
        _ => 1.0,
    };

    ChartTab {
        symbol: symbol.to_string(),
        timeframe: timeframe.to_string(),
        candles: sample_candles(start_price, volatility),
    }
}

fn sample_candles(start_price: f64, volatility: f64) -> Vec<Candle> {
    let mut candles = Vec::with_capacity(72);
    let mut previous_close = start_price;

    for index in 0..72 {
        let wave = start_price
            * ((index as f64 * 0.37).sin() * 0.00022 + ((index * 17 % 11) as f64 - 5.0) * 0.000025)
            * volatility;
        let open = previous_close;
        let close = open + wave;
        let wick = start_price * ((index * 7 % 9) as f64 + 2.0) * 0.000022 * volatility;

        candles.push(Candle {
            open,
            high: open.max(close) + wick,
            low: open.min(close) - wick * 0.8,
            close,
            volume: 65.0 + (index * 29 + 13) as f64 % 90.0 + wave.abs() / start_price * 100_000.0,
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
