use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::Receiver,
    },
    thread,
};

use eframe::egui;
use tokio::sync::watch;

use crate::{app_log::AppLogger, market::ChartSnapshot};

use super::{ChartIndicatorData, ChartKind, IndicatorSnapshot};

pub struct IndicatorWorkerConfig {
    pub feed_id: u64,
    pub timeframe: String,
    pub snapshot_rx: watch::Receiver<Arc<ChartSnapshot>>,
    pub chart_plan_rx: watch::Receiver<Arc<[ChartKind]>>,
    pub update_rx: Receiver<()>,
    pub indicator_tx: watch::Sender<Arc<IndicatorSnapshot>>,
    pub egui_ctx: egui::Context,
    pub active_feed_id: Arc<AtomicU64>,
    pub feed: String,
    pub logger: AppLogger,
}

pub fn spawn_indicator_worker(config: IndicatorWorkerConfig) -> thread::JoinHandle<()> {
    let IndicatorWorkerConfig {
        feed_id,
        timeframe,
        snapshot_rx,
        chart_plan_rx,
        update_rx,
        indicator_tx,
        egui_ctx,
        active_feed_id,
        feed,
        logger,
    } = config;
    thread::Builder::new()
        .name(format!("chart-indicators-{feed_id}"))
        .spawn(move || {
            let mut momentum_error_active = false;
            while update_rx.recv().is_ok() {
                // Skip intermediate work; both watch receivers retain the
                // latest market snapshot and chart plan.
                while matches!(update_rx.try_recv(), Ok(())) {}

                let snapshot = {
                    let guard = snapshot_rx.borrow();
                    Arc::clone(&guard)
                };
                let chart_plan = {
                    let guard = chart_plan_rx.borrow();
                    Arc::clone(&guard)
                };

                let charts = chart_plan
                    .iter()
                    .copied()
                    .map(|kind| (kind, kind.calculate_indicators(&snapshot, &timeframe)))
                    .collect::<std::collections::HashMap<_, _>>();
                if let Some(ChartIndicatorData::Momentum(data)) = charts.get(&ChartKind::Momentum) {
                    if let Some(error) = &data.error {
                        if !momentum_error_active {
                            logger.warning(
                                Some(feed.clone()),
                                format!("No se pudo calcular Momentum: {error}"),
                            );
                            momentum_error_active = true;
                        }
                    } else if momentum_error_active {
                        logger.info(Some(feed.clone()), "El cálculo de Momentum se recuperó");
                        momentum_error_active = false;
                    }
                }
                indicator_tx.send_replace(Arc::new(IndicatorSnapshot {
                    source_generation: snapshot.generation,
                    charts,
                }));

                if active_feed_id.load(Ordering::Relaxed) == feed_id {
                    egui_ctx.request_repaint();
                }
            }
        })
        .expect("failed to start chart indicator worker")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicU64, mpsc};

    use eframe::egui;
    use tokio::sync::watch;

    use crate::{
        charts::{ChartIndicatorData, ChartKind, IndicatorSnapshot},
        market::ChartSnapshot,
    };

    use super::{IndicatorWorkerConfig, spawn_indicator_worker};

    #[test]
    fn worker_publishes_results_for_the_tab_chart_plan() {
        let (snapshot_tx, snapshot_rx) = watch::channel(Arc::new(ChartSnapshot::default()));
        let (plan_tx, plan_rx) = watch::channel(Arc::<[ChartKind]>::from([
            ChartKind::Main,
            ChartKind::Momentum,
            ChartKind::BidAskFlow,
        ]));
        let (signal_tx, signal_rx) = mpsc::sync_channel(1);
        let (indicator_tx, indicator_rx) = watch::channel(Arc::new(IndicatorSnapshot::default()));
        let worker = spawn_indicator_worker(IndicatorWorkerConfig {
            feed_id: 7,
            timeframe: "1m".to_owned(),
            snapshot_rx,
            chart_plan_rx: plan_rx,
            update_rx: signal_rx,
            indicator_tx,
            egui_ctx: egui::Context::default(),
            active_feed_id: Arc::new(AtomicU64::new(0)),
            feed: "binance/BTCUSDT/1m".to_owned(),
            logger: crate::app_log::AppLogger::new(egui::Context::default()),
        });

        signal_tx.send(()).unwrap();
        drop(signal_tx);
        drop(snapshot_tx);
        drop(plan_tx);
        worker.join().unwrap();

        let result = indicator_rx.borrow();
        assert_eq!(result.source_generation, 0);
        assert!(matches!(
            result.charts.get(&ChartKind::Main),
            Some(ChartIndicatorData::Main(_))
        ));
        assert!(matches!(
            result.charts.get(&ChartKind::Momentum),
            Some(ChartIndicatorData::Momentum(_))
        ));
        assert!(matches!(
            result.charts.get(&ChartKind::BidAskFlow),
            Some(ChartIndicatorData::BidAskFlow(_))
        ));
    }
}
