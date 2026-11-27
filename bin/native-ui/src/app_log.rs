use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use eframe::egui;

const MAX_LOG_ENTRIES: usize = 500;
const DUPLICATE_WINDOW: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub elapsed_ms: u64,
    pub level: LogLevel,
    pub feed: Option<String>,
    pub message: String,
    pub repeat_count: u32,
}

struct LogState {
    started_at: Instant,
    entries: VecDeque<LogEntry>,
}

#[derive(Clone)]
pub struct AppLogger {
    state: Arc<Mutex<LogState>>,
    dialog_open: Arc<AtomicBool>,
    egui_ctx: egui::Context,
}

impl AppLogger {
    pub fn new(egui_ctx: egui::Context) -> Self {
        Self {
            state: Arc::new(Mutex::new(LogState {
                started_at: Instant::now(),
                entries: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            })),
            dialog_open: Arc::new(AtomicBool::new(false)),
            egui_ctx,
        }
    }

    pub fn info(&self, feed: Option<String>, message: impl Into<String>) {
        self.push(LogLevel::Info, feed, message.into());
    }

    pub fn warning(&self, feed: Option<String>, message: impl Into<String>) {
        self.push(LogLevel::Warning, feed, message.into());
    }

    pub fn error(&self, feed: Option<String>, message: impl Into<String>) {
        self.push(LogLevel::Error, feed, message.into());
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.entries.iter().cloned().collect()
    }

    pub fn clear(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.entries.clear();
        drop(state);
        self.repaint_if_open();
    }

    pub fn set_dialog_open(&self, open: bool) {
        self.dialog_open.store(open, Ordering::Relaxed);
    }

    fn push(&self, level: LogLevel, feed: Option<String>, message: String) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let elapsed_ms = u64::try_from(state.started_at.elapsed().as_millis()).unwrap_or(u64::MAX);

        if let Some(last) = state.entries.back_mut()
            && last.level == level
            && last.feed == feed
            && last.message == message
            && elapsed_ms.saturating_sub(last.elapsed_ms)
                < u64::try_from(DUPLICATE_WINDOW.as_millis()).unwrap_or(u64::MAX)
        {
            last.elapsed_ms = elapsed_ms;
            last.repeat_count = last.repeat_count.saturating_add(1);
        } else {
            if state.entries.len() == MAX_LOG_ENTRIES {
                state.entries.pop_front();
            }
            state.entries.push_back(LogEntry {
                elapsed_ms,
                level,
                feed,
                message,
                repeat_count: 1,
            });
        }
        drop(state);
        self.repaint_if_open();
    }

    fn repaint_if_open(&self) {
        if self.dialog_open.load(Ordering::Relaxed) {
            self.egui_ctx.request_repaint();
        }
    }
}

pub fn format_elapsed(elapsed_ms: u64) -> String {
    let seconds = elapsed_ms / 1_000;
    let hours = seconds / 3_600;
    let minutes = seconds / 60 % 60;
    let seconds = seconds % 60;
    let milliseconds = elapsed_ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{milliseconds:03}")
}

#[cfg(test)]
mod tests {
    use super::{AppLogger, LogLevel, format_elapsed};
    use eframe::egui;

    #[test]
    fn elapsed_time_format_is_stable() {
        assert_eq!(format_elapsed(3_723_045), "01:02:03.045");
    }

    #[test]
    fn repeated_status_entries_are_coalesced() {
        let logger = AppLogger::new(egui::Context::default());
        logger.info(Some("binance/BTCUSDT/1m".into()), "Conectando");
        logger.info(Some("binance/BTCUSDT/1m".into()), "Conectando");

        let entries = logger.snapshot();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].level, LogLevel::Info);
        assert_eq!(entries[0].repeat_count, 2);
    }

    #[test]
    fn log_buffer_is_bounded_and_can_be_cleared() {
        let logger = AppLogger::new(egui::Context::default());
        for index in 0..501 {
            logger.info(None, format!("evento-{index}"));
        }

        let entries = logger.snapshot();
        assert_eq!(entries.len(), 500);
        assert_eq!(entries.first().unwrap().message, "evento-1");
        assert_eq!(entries.last().unwrap().message, "evento-500");

        logger.clear();
        assert!(logger.snapshot().is_empty());
    }
}
