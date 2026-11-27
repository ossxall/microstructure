use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};

use eframe::egui::{self, Color32, RichText, Stroke};
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints, Polygon};
use tokio::sync::watch;

use crate::{
    app_log::{AppLogger, LogLevel, format_elapsed},
    market::{
        CalculationWorkerConfig, ChartDataRequest, ChartKind, ChartSnapshot, DEFAULT_HISTORY_LIMIT,
        FeedKey, FeedStatus, spawn_calculation_worker,
    },
    network::{NetworkCommand, NetworkRuntime},
};

const BACKGROUND: Color32 = Color32::BLACK;
const PANEL: Color32 = Color32::BLACK;
const LIME: Color32 = Color32::from_rgb(0, 255, 0);
const LIME_GREEN: Color32 = Color32::from_rgb(50, 205, 50);
const GRID: Color32 = Color32::from_rgb(119, 136, 153);
const TEXT: Color32 = Color32::WHITE;
const MUTED: Color32 = GRID;

struct Chart {
    id: u64,
    kind: ChartKind,
    data_request: ChartDataRequest,
}

struct ChartTab {
    feed_id: u64,
    key: FeedKey,
    charts: Vec<Chart>,
    requested_history: u64,
    _calculation_tx: mpsc::Sender<crate::market::CalculationInput>,
    snapshot_rx: watch::Receiver<Arc<ChartSnapshot>>,
}

pub struct ChartApp {
    tabs: Vec<ChartTab>,
    active_tab: usize,
    next_feed_id: u64,
    next_chart_id: u64,
    active_feed_id: Arc<AtomicU64>,
    network: NetworkRuntime,
    egui_ctx: egui::Context,
    logger: AppLogger,
    logs_open: bool,
}

#[derive(Clone, Copy)]
enum TabAction {
    AddChart(ChartKind),
    RemoveChart(u64),
    LoadMoreHistory,
}

impl ChartApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        set_green_dark_theme(&cc.egui_ctx);
        let logger = AppLogger::new(cc.egui_ctx.clone());
        logger.info(None, "Aplicación iniciada");
        let network = NetworkRuntime::new(logger.clone());

        Self {
            tabs: Vec::new(),
            active_tab: 0,
            next_feed_id: 1,
            next_chart_id: 1,
            active_feed_id: Arc::new(AtomicU64::new(0)),
            network,
            egui_ctx: cc.egui_ctx.clone(),
            logger,
            logs_open: false,
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
                                for timeframe in ["1m", "5m", "15m", "30m", "1h", "4h", "1d"] {
                                    if ui.button(timeframe).clicked() {
                                        add_tab = Some((symbol.to_string(), timeframe.to_string()));
                                    }
                                }
                            });
                        }
                    })
                    .response
                    .on_hover_text("Añadir símbolo y temporalidad");
                    if ui.button("Logs").on_hover_text("Estado y logs").clicked() {
                        self.logs_open = true;
                        self.logger.set_dialog_open(true);
                    }

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
                                                tab.key.symbol, tab.key.timeframe
                                            ))
                                            .small()
                                            .strong(),
                                        )
                                        .clicked()
                                    {
                                        activate_tab = Some(index);
                                    }

                                    if ui
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
            self.close_tab(index);
        } else if let Some((symbol, timeframe)) = add_tab {
            if let Some(index) = self
                .tabs
                .iter()
                .position(|tab| tab.key.symbol == symbol && tab.key.timeframe == timeframe)
            {
                self.activate_tab(index);
            } else {
                self.open_tab(symbol, timeframe);
            }
        } else if let Some(index) = activate_tab {
            self.activate_tab(index);
        }
    }

    fn open_tab(&mut self, symbol: String, timeframe: String) {
        let feed_id = self.next_feed_id;
        self.next_feed_id += 1;
        let key = FeedKey {
            source: "binance".to_string(),
            symbol,
            timeframe,
        };
        let charts = vec![
            self.new_chart(ChartKind::Price),
            self.new_chart(ChartKind::Volume),
        ];
        let requested_history = charts
            .iter()
            .map(|chart| chart.data_request.history_limit)
            .max()
            .unwrap_or(DEFAULT_HISTORY_LIMIT);
        let subscribe_live = charts.iter().any(|chart| chart.data_request.live);

        let (calculation_tx, calculation_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = watch::channel(Arc::new(ChartSnapshot::default()));

        let _ = self.network.sender.send(NetworkCommand::OpenFeed {
            feed_id,
            key: key.clone(),
            history_limit: requested_history,
            subscribe_live,
            calculation_tx: calculation_tx.clone(),
        });

        let requests = charts
            .iter()
            .map(|chart| chart.data_request.clone())
            .collect();
        spawn_calculation_worker(CalculationWorkerConfig {
            feed_id,
            requests,
            receiver: calculation_rx,
            snapshot_tx,
            network_tx: self.network.sender.clone(),
            egui_ctx: self.egui_ctx.clone(),
            active_feed_id: self.active_feed_id.clone(),
            feed_key: key.clone(),
            logger: self.logger.clone(),
        });

        self.logger.info(
            Some(feed_label(&key)),
            "Tab abierta; solicitando history y suscripción live",
        );
        self.tabs.push(ChartTab {
            feed_id,
            key,
            charts,
            requested_history,
            _calculation_tx: calculation_tx,
            snapshot_rx,
        });
        self.activate_tab(self.tabs.len() - 1);
    }

    fn new_chart(&mut self, kind: ChartKind) -> Chart {
        let id = self.next_chart_id;
        self.next_chart_id += 1;
        Chart {
            id,
            kind,
            data_request: ChartDataRequest::for_chart(kind),
        }
    }

    fn activate_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
            self.active_feed_id
                .store(self.tabs[index].feed_id, Ordering::Relaxed);
        }
    }

    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }

        let tab = self.tabs.remove(index);
        self.logger.info(
            Some(feed_label(&tab.key)),
            "Tab cerrada; cancelando feed y cálculo",
        );
        let _ = self.network.sender.send(NetworkCommand::CloseFeed {
            feed_id: tab.feed_id,
        });
        drop(tab);

        if self.tabs.is_empty() {
            self.active_tab = 0;
            self.active_feed_id.store(0, Ordering::Relaxed);
        } else {
            if self.active_tab > index {
                self.active_tab -= 1;
            } else if self.active_tab == index {
                self.active_tab = self.active_tab.min(self.tabs.len() - 1);
            }
            self.active_feed_id
                .store(self.tabs[self.active_tab].feed_id, Ordering::Relaxed);
        }
    }

    fn add_chart(&mut self, kind: ChartKind) {
        let mut chart = self.new_chart(kind);
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        chart.data_request.history_limit = tab.requested_history;
        tab.charts.push(chart);
        let chart_kind = tab.charts.last().map(|chart| chart.kind);
        self.logger.info(
            Some(feed_label(&tab.key)),
            format!("Chart añadido: {chart_kind:?}"),
        );
        let requests = tab
            .charts
            .iter()
            .map(|chart| chart.data_request.clone())
            .collect();
        let _ = tab
            ._calculation_tx
            .send(crate::market::CalculationInput::ChartRequests(requests));
    }

    fn load_more_history(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        tab.requested_history = tab.requested_history.saturating_add(DEFAULT_HISTORY_LIMIT);
        self.logger.info(
            Some(feed_label(&tab.key)),
            format!(
                "Solicitando hasta {} Bars de history",
                tab.requested_history
            ),
        );
        let _ = self.network.sender.send(NetworkCommand::FetchHistory {
            feed_id: tab.feed_id,
            history_limit: tab.requested_history,
        });
    }

    fn remove_chart(&mut self, chart_id: u64) {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        if tab.charts.len() <= 1 {
            return;
        }
        tab.charts.retain(|chart| chart.id != chart_id);
        self.logger.info(
            Some(feed_label(&tab.key)),
            "Chart cerrado; actualizando necesidades de cálculo",
        );
        let requests = tab
            .charts
            .iter()
            .map(|chart| chart.data_request.clone())
            .collect();
        let _ = tab
            ._calculation_tx
            .send(crate::market::CalculationInput::ChartRequests(requests));
    }

    fn show_log_window(&mut self, ctx: &egui::Context) {
        if !self.logs_open {
            self.logger.set_dialog_open(false);
            return;
        }

        let logger = self.logger.clone();
        let entries = logger.snapshot();
        let mut clear = false;
        egui::Window::new("Estado y logs")
            .id(egui::Id::unique("native-ui-app-logs"))
            .open(&mut self.logs_open)
            .default_size([820.0, 420.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{} entradas · límite {}", entries.len(), 500));
                    if ui.button("Limpiar").clicked() {
                        clear = true;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for entry in &entries {
                            ui.horizontal_wrapped(|ui| {
                                ui.monospace(format_elapsed(entry.elapsed_ms));
                                let (label, color) = match entry.level {
                                    LogLevel::Info => ("INFO", Color32::LIGHT_BLUE),
                                    LogLevel::Warning => ("WARN", Color32::YELLOW),
                                    LogLevel::Error => ("ERROR", Color32::LIGHT_RED),
                                };
                                ui.colored_label(color, label);
                                if let Some(feed) = &entry.feed {
                                    ui.monospace(feed);
                                }
                                ui.label(&entry.message);
                                if entry.repeat_count > 1 {
                                    ui.label(
                                        RichText::new(format!("×{}", entry.repeat_count))
                                            .color(MUTED)
                                            .small(),
                                    );
                                }
                            });
                        }
                    });
            });

        if clear {
            logger.clear();
        }
        logger.set_dialog_open(self.logs_open);
    }

    fn show_tab_content(ui: &mut egui::Ui, tab: &ChartTab, action: &mut Option<TabAction>) {
        let snapshot = tab.snapshot_rx.borrow().clone();
        let last = snapshot.candles.last().copied();
        let precision = if tab.key.symbol == "ADAUSDT" { 4 } else { 2 };
        let status_text = match &snapshot.status {
            FeedStatus::Loading => "Cargando".to_string(),
            FeedStatus::Connecting => "Conectando".to_string(),
            FeedStatus::Live => "En vivo".to_string(),
            FeedStatus::Disconnected => "Reconectando".to_string(),
            FeedStatus::Error(error) => format!("Error: {error}"),
        };

        ui.horizontal(|ui| {
            ui.heading(RichText::new(&tab.key.symbol).strong().color(TEXT));
            ui.label(
                RichText::new(&tab.key.timeframe)
                    .strong()
                    .color(LIME)
                    .background_color(Color32::from_rgb(22, 48, 34)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let status_color = if matches!(&snapshot.status, FeedStatus::Live) {
                    LIME
                } else {
                    MUTED
                };
                ui.label(
                    RichText::new(format!("● {status_text}"))
                        .color(status_color)
                        .small(),
                );
                ui.label(
                    RichText::new(format!("{} Bars completas", snapshot.full_bar_count))
                        .color(MUTED)
                        .small(),
                );
            });
        });

        if let Some(error) = &snapshot.history_error {
            ui.colored_label(Color32::LIGHT_RED, format!("History: {error}"));
        }

        ui.add_space(4.0);
        if let Some(last) = last {
            let change_color = if last.is_bullish() { LIME } else { TEXT };
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
        }

        ui.horizontal(|ui| {
            ui.menu_button("Añadir Chart", |ui| {
                if ui.button("Precio").clicked() {
                    *action = Some(TabAction::AddChart(ChartKind::Price));
                }
                if ui.button("Volumen").clicked() {
                    *action = Some(TabAction::AddChart(ChartKind::Volume));
                }
            });
            if ui.button("Cargar 50 Bars más").clicked() {
                *action = Some(TabAction::LoadMoreHistory);
            }
        });

        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for chart in &tab.charts {
                egui::Frame::new()
                    .fill(PANEL)
                    .corner_radius(egui::CornerRadius::same(5))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let title = match chart.kind {
                                ChartKind::Price => "PRECIO · OHLC",
                                ChartKind::Volume => "VOLUMEN · total_qty",
                            };
                            ui.label(RichText::new(title).color(MUTED).small().strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(
                                            tab.charts.len() > 1,
                                            egui::Button::new("×").small(),
                                        )
                                        .on_hover_text("Cerrar Chart")
                                        .clicked()
                                    {
                                        *action = Some(TabAction::RemoveChart(chart.id));
                                    }
                                },
                            );
                        });
                        match chart.kind {
                            ChartKind::Price => {
                                Self::show_price_chart(ui, tab, chart, &snapshot.candles);
                            }
                            ChartKind::Volume => {
                                Self::show_volume_chart(ui, tab, chart, &snapshot.candles);
                            }
                        }
                    });
                ui.add_space(8.0);
            }
        });

        ui.horizontal(|ui| {
            ui.label(RichText::new("●").color(LIME));
            ui.label(RichText::new("Alcista").color(MUTED).small());
            ui.label(RichText::new("●").color(TEXT));
            ui.label(RichText::new("Bajista").color(MUTED).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new("OHLCV calculado desde las Bars completas")
                        .color(MUTED)
                        .small(),
                );
            });
        });
    }

    fn show_price_chart(
        ui: &mut egui::Ui,
        tab: &ChartTab,
        chart: &Chart,
        candles: &[crate::market::Candle],
    ) {
        if candles.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Esperando history/live…").color(MUTED));
            });
            return;
        }

        let min_price = candles
            .iter()
            .map(|candle| candle.low)
            .fold(f64::INFINITY, f64::min);
        let max_price = candles
            .iter()
            .map(|candle| candle.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let price_padding = ((max_price - min_price) * 0.08).max(max_price.abs() * 0.00001);
        let interval = timeframe_seconds(&tab.key.timeframe);
        let x_min = candles[0].timestamp_ms as f64 / 1_000.0 - interval * 0.75;
        let x_max = candles[candles.len() - 1].timestamp_ms as f64 / 1_000.0 + interval * 0.75;
        let x_link = egui::Id::NULL.with(("ohlcv-time-axis", tab.feed_id));

        Plot::new(("ohlcv-price", tab.feed_id, chart.id))
            .height(410.0)
            .default_x_bounds(x_min, x_max.max(x_min + interval))
            .default_y_bounds(min_price - price_padding, max_price + price_padding)
            .link_axis(x_link, egui::Vec2b::new(true, false))
            .grid_color(GRID)
            .show_axes(egui::Vec2b::new(false, true))
            .show_x(false)
            .allow_boxed_zoom(false)
            .show(ui, |plot_ui| {
                for (index, candle) in candles.iter().copied().enumerate() {
                    let x = candle.timestamp_ms as f64 / 1_000.0;
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
                    let half_width = interval * 0.32;

                    plot_ui.line(
                        Line::new(
                            format!("wick-{}-{index}", chart.id),
                            PlotPoints::from(vec![[x, candle.low], [x, candle.high]]),
                        )
                        .color(LIME)
                        .width(1.2)
                        .allow_hover(false),
                    );

                    plot_ui.polygon(
                        Polygon::new(
                            format!("candle-{}-{index}", chart.id),
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

    fn show_volume_chart(
        ui: &mut egui::Ui,
        tab: &ChartTab,
        chart: &Chart,
        candles: &[crate::market::Candle],
    ) {
        if candles.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Esperando history/live…").color(MUTED));
            });
            return;
        }

        let max_volume = candles
            .iter()
            .map(|candle| candle.volume)
            .fold(0.0, f64::max);
        let interval = timeframe_seconds(&tab.key.timeframe);
        let x_min = candles[0].timestamp_ms as f64 / 1_000.0 - interval * 0.75;
        let x_max = candles[candles.len() - 1].timestamp_ms as f64 / 1_000.0 + interval * 0.75;
        let bars = candles
            .iter()
            .map(|candle| {
                Bar::new(candle.timestamp_ms as f64 / 1_000.0, candle.volume)
                    .width(interval * 0.64)
                    .fill(LIME_GREEN)
                    .stroke(Stroke::new(1.0, LIME_GREEN))
            })
            .collect();

        Plot::new(("ohlcv-volume", tab.feed_id, chart.id))
            .height(170.0)
            .default_x_bounds(x_min, x_max.max(x_min + interval))
            .default_y_bounds(0.0, (max_volume * 1.25).max(1.0))
            .link_axis(
                egui::Id::NULL.with(("ohlcv-time-axis", tab.feed_id)),
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
        let mut action = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(14.0))
            .show(ui, |ui| {
                self.show_tabs(ui);
                ui.separator();
                ui.add_space(8.0);

                if let Some(tab) = self.tabs.get(self.active_tab) {
                    Self::show_tab_content(ui, tab, &mut action);
                } else {
                    ui.add_space(48.0);
                    ui.vertical_centered(|ui| {
                        ui.heading(RichText::new("Sin gráficos abiertos").color(TEXT));
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Usa «+» para abrir un símbolo y una temporalidad.")
                                .color(MUTED),
                        );
                    });
                }
            });

        self.show_log_window(ui.ctx());

        match action {
            Some(TabAction::AddChart(kind)) => self.add_chart(kind),
            Some(TabAction::RemoveChart(chart_id)) => self.remove_chart(chart_id),
            Some(TabAction::LoadMoreHistory) => self.load_more_history(),
            None => {}
        }
    }
}

fn feed_label(key: &FeedKey) -> String {
    format!("{}/{}/{}", key.source, key.symbol, key.timeframe)
}

fn timeframe_seconds(timeframe: &str) -> f64 {
    match timeframe {
        "1m" => 60.0,
        "5m" => 300.0,
        "15m" => 900.0,
        "30m" => 1_800.0,
        "1h" => 3_600.0,
        "4h" => 14_400.0,
        "1d" => 86_400.0,
        _ => 60.0,
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
