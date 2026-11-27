use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};

use eframe::egui::{self, Color32, RichText};
use tokio::sync::watch;

use crate::{
    app_log::{AppLogger, LogLevel, format_elapsed},
    charts::{self, Chart, ChartKind},
    market::{
        CalculationWorkerConfig, ChartDataRequest, ChartSnapshot, DEFAULT_HISTORY_LIMIT, FeedKey,
        FeedStatus, spawn_calculation_worker,
    },
    network::{NetworkCommand, NetworkRuntime},
    theme::{self, BACKGROUND, LIME, MUTED, PANEL, TEXT},
};

struct ChartTab {
    feed_id: u64,
    key: FeedKey,
    /// This tab's chart instances and their display order.
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
    AddChart {
        feed_id: u64,
        kind: ChartKind,
    },
    RemoveChart {
        feed_id: u64,
        chart_id: u64,
    },
    MoveChart {
        feed_id: u64,
        dragged_chart_id: u64,
        target_chart_id: u64,
        after: bool,
    },
    LoadMoreHistory,
}

impl ChartApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::set_green_dark_theme(&cc.egui_ctx);
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
        let chart_layout = ChartKind::ALL
            .into_iter()
            .map(|kind| self.new_chart(kind))
            .collect::<Vec<_>>();
        let requests = Self::chart_requests(&chart_layout, DEFAULT_HISTORY_LIMIT);
        let requested_history = requests
            .iter()
            .map(|request| request.history_limit)
            .max()
            .unwrap_or(DEFAULT_HISTORY_LIMIT);
        let subscribe_live = requests.iter().any(|request| request.live);

        let (calculation_tx, calculation_rx) = mpsc::channel();
        let (snapshot_tx, snapshot_rx) = watch::channel(Arc::new(ChartSnapshot::default()));

        let _ = self.network.sender.send(NetworkCommand::OpenFeed {
            feed_id,
            key: key.clone(),
            history_limit: requested_history,
            subscribe_live,
            calculation_tx: calculation_tx.clone(),
        });

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
            charts: chart_layout,
            requested_history,
            _calculation_tx: calculation_tx,
            snapshot_rx,
        });
        self.activate_tab(self.tabs.len() - 1);
    }

    fn new_chart(&mut self, kind: ChartKind) -> Chart {
        let id = self.next_chart_id;
        self.next_chart_id += 1;
        Chart::new(id, kind)
    }

    fn chart_requests(charts: &[Chart], history_limit: u64) -> Vec<ChartDataRequest> {
        charts
            .iter()
            .map(|chart| chart.data_request(history_limit))
            .collect()
    }

    fn update_chart_requests(&self, feed_id: u64) {
        if let Some(tab) = self.tabs.iter().find(|tab| tab.feed_id == feed_id) {
            let requests = Self::chart_requests(&tab.charts, tab.requested_history);
            let _ = tab
                ._calculation_tx
                .send(crate::market::CalculationInput::ChartRequests(requests));
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

    fn add_chart(&mut self, feed_id: u64, kind: ChartKind) {
        let Some(tab_index) = self.tabs.iter().position(|tab| tab.feed_id == feed_id) else {
            return;
        };
        let chart = self.new_chart(kind);
        let feed = {
            let tab = &mut self.tabs[tab_index];
            add_chart_to_layout(&mut tab.charts, chart);
            feed_label(&tab.key)
        };
        self.update_chart_requests(feed_id);
        self.logger.info(
            Some(feed),
            format!("Chart añadido a esta pestaña: {kind:?}"),
        );
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

    fn remove_chart(&mut self, feed_id: u64, chart_id: u64) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.feed_id == feed_id) else {
            return;
        };
        if !remove_chart_from_layout(&mut tab.charts, chart_id) {
            return;
        }
        let feed = feed_label(&tab.key);
        self.update_chart_requests(feed_id);
        self.logger.info(
            Some(feed),
            "Chart cerrado en esta pestaña; cálculo actualizado",
        );
    }

    fn move_chart(
        &mut self,
        feed_id: u64,
        dragged_chart_id: u64,
        target_chart_id: u64,
        after: bool,
    ) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.feed_id == feed_id) else {
            return;
        };

        if move_chart_in_layout(&mut tab.charts, dragged_chart_id, target_chart_id, after) {
            self.logger.info(
                Some(feed_label(&tab.key)),
                "Orden de Charts actualizado en esta pestaña",
            );
        }
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

    fn show_tab_content(
        ui: &mut egui::Ui,
        tab: &ChartTab,
        chart_layout: &[Chart],
        action: &mut Option<TabAction>,
    ) {
        let snapshot = tab.snapshot_rx.borrow().clone();
        let last = snapshot.full_bars.last().map(|bar| bar.candle);
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
                    RichText::new(format!("{} Bars completas", snapshot.full_bars.len()))
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
                for kind in ChartKind::ALL {
                    if ui.button(kind.menu_label()).clicked() {
                        *action = Some(TabAction::AddChart {
                            feed_id: tab.feed_id,
                            kind,
                        });
                    }
                }
            });
            if ui.button("Cargar 50 Bars más").clicked() {
                *action = Some(TabAction::LoadMoreHistory);
            }
        });

        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for chart in chart_layout.iter().copied() {
                let response = charts::show(
                    ui,
                    chart,
                    tab.feed_id,
                    &tab.key.timeframe,
                    &snapshot,
                    chart_layout.len() > 1,
                );
                if response.close {
                    *action = Some(TabAction::RemoveChart {
                        feed_id: tab.feed_id,
                        chart_id: chart.id,
                    });
                } else if let Some(drop) = response.drop {
                    *action = Some(TabAction::MoveChart {
                        feed_id: tab.feed_id,
                        dragged_chart_id: drop.dragged_chart_id,
                        target_chart_id: drop.target_chart_id,
                        after: drop.after,
                    });
                }
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
                    Self::show_tab_content(ui, tab, &tab.charts, &mut action);
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
            Some(TabAction::AddChart { feed_id, kind }) => self.add_chart(feed_id, kind),
            Some(TabAction::RemoveChart { feed_id, chart_id }) => {
                self.remove_chart(feed_id, chart_id)
            }
            Some(TabAction::MoveChart {
                feed_id,
                dragged_chart_id,
                target_chart_id,
                after,
            }) => self.move_chart(feed_id, dragged_chart_id, target_chart_id, after),
            Some(TabAction::LoadMoreHistory) => self.load_more_history(),
            None => {}
        }
    }
}

fn feed_label(key: &FeedKey) -> String {
    format!("{}/{}/{}", key.source, key.symbol, key.timeframe)
}

fn add_chart_to_layout(chart_layout: &mut Vec<Chart>, chart: Chart) {
    chart_layout.push(chart);
}

fn remove_chart_from_layout(chart_layout: &mut Vec<Chart>, chart_id: u64) -> bool {
    if chart_layout.len() <= 1 {
        return false;
    }
    let Some(chart_index) = chart_layout.iter().position(|chart| chart.id == chart_id) else {
        return false;
    };
    chart_layout.remove(chart_index);
    true
}

fn move_chart_in_layout(
    chart_layout: &mut Vec<Chart>,
    dragged_chart_id: u64,
    target_chart_id: u64,
    after: bool,
) -> bool {
    if dragged_chart_id == target_chart_id {
        return false;
    }

    let Some(dragged_index) = chart_layout
        .iter()
        .position(|chart| chart.id == dragged_chart_id)
    else {
        return false;
    };
    if !chart_layout.iter().any(|chart| chart.id == target_chart_id) {
        return false;
    }

    let original_order = chart_layout
        .iter()
        .map(|chart| chart.id)
        .collect::<Vec<_>>();
    let dragged_chart = chart_layout.remove(dragged_index);
    let Some(target_index) = chart_layout
        .iter()
        .position(|chart| chart.id == target_chart_id)
    else {
        return false;
    };
    let insertion_index = target_index + usize::from(after);
    chart_layout.insert(insertion_index, dragged_chart);

    chart_layout.iter().map(|chart| chart.id).ne(original_order)
}

#[cfg(test)]
mod tests {
    use crate::charts::{Chart, ChartKind};

    use super::{add_chart_to_layout, move_chart_in_layout, remove_chart_from_layout};

    fn layout(ids: &[u64]) -> Vec<Chart> {
        ids.iter()
            .map(|id| Chart::new(*id, ChartKind::Price))
            .collect()
    }

    fn ids(charts: &[Chart]) -> Vec<u64> {
        charts.iter().map(|chart| chart.id).collect()
    }

    #[test]
    fn moving_chart_before_another_changes_only_the_requested_order() {
        let mut charts = layout(&[1, 2, 3]);

        assert!(move_chart_in_layout(&mut charts, 3, 1, false));
        assert_eq!(ids(&charts), [3, 1, 2]);
    }

    #[test]
    fn moving_chart_after_another_changes_only_the_requested_order() {
        let mut charts = layout(&[1, 2, 3]);

        assert!(move_chart_in_layout(&mut charts, 1, 2, true));
        assert_eq!(ids(&charts), [2, 1, 3]);
    }

    #[test]
    fn dropping_in_the_existing_position_is_a_no_op() {
        let mut charts = layout(&[1, 2, 3]);

        assert!(!move_chart_in_layout(&mut charts, 2, 3, false));
        assert_eq!(ids(&charts), [1, 2, 3]);
    }

    #[test]
    fn reordering_one_tab_does_not_change_another_tabs_layout() {
        let mut first_tab_charts = layout(&[1, 2, 3]);
        let second_tab_charts = layout(&[4, 5, 6]);

        assert!(move_chart_in_layout(&mut first_tab_charts, 3, 1, false));
        assert_eq!(ids(&first_tab_charts), [3, 1, 2]);
        assert_eq!(ids(&second_tab_charts), [4, 5, 6]);
    }

    #[test]
    fn adding_and_removing_charts_only_changes_the_requested_tab() {
        let mut first_tab_charts = layout(&[1, 2]);
        let second_tab_charts = layout(&[3, 4]);

        add_chart_to_layout(&mut first_tab_charts, Chart::new(5, ChartKind::Volume));
        assert!(remove_chart_from_layout(&mut first_tab_charts, 1));

        assert_eq!(ids(&first_tab_charts), [2, 5]);
        assert_eq!(ids(&second_tab_charts), [3, 4]);
    }

    #[test]
    fn a_tab_cannot_remove_its_last_chart() {
        let mut charts = layout(&[1]);

        assert!(!remove_chart_from_layout(&mut charts, 1));
        assert_eq!(ids(&charts), [1]);
    }
}
