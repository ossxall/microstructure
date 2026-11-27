mod app;
mod app_log;
mod charts;
mod market;
mod network;
mod theme;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([760.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Native UI · OHLCV",
        options,
        Box::new(|cc| Ok(Box::new(app::ChartApp::new(cc)))),
    )
}
