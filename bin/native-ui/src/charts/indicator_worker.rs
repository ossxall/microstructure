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

use crate::market::ChartSnapshot;

use super::{ChartKind, IndicatorSnapshot};

pub fn spawn_indicator_worker(
    feed_id: u64,
    snapshot_rx: watch::Receiver<Arc<ChartSnapshot>>,
    chart_plan_rx: watch::Receiver<Arc<[ChartKind]>>,
    update_rx: Receiver<()>,
    indicator_tx: watch::Sender<Arc<IndicatorSnapshot>>,
    egui_ctx: egui::Context,
    active_feed_id: Arc<AtomicU64>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name(format!("chart-indicators-{feed_id}"))
        .spawn(move || {
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
                    .map(|kind| (kind, kind.calculate_indicators(&snapshot)))
                    .collect();
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

    use super::spawn_indicator_worker;

    #[test]
    fn worker_publishes_results_for_the_tab_chart_plan() {
        let (snapshot_tx, snapshot_rx) = watch::channel(Arc::new(ChartSnapshot::default()));
        let (plan_tx, plan_rx) = watch::channel(Arc::<[ChartKind]>::from([ChartKind::Main]));
        let (signal_tx, signal_rx) = mpsc::sync_channel(1);
        let (indicator_tx, indicator_rx) = watch::channel(Arc::new(IndicatorSnapshot::default()));
        let worker = spawn_indicator_worker(
            7,
            snapshot_rx,
            plan_rx,
            signal_rx,
            indicator_tx,
            egui::Context::default(),
            Arc::new(AtomicU64::new(0)),
        );

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
    }
}
