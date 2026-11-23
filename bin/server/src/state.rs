use pulsar::{Pulsar, TokioExecutor};
use questdb::QuestDb;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub questdb: Arc<QuestDb>,
    pub pulsar: Arc<Pulsar<TokioExecutor>>,
}

impl AppState {
    pub fn new(questdb: Arc<QuestDb>, pulsar: Arc<Pulsar<TokioExecutor>>) -> Self {
        Self { questdb, pulsar }
    }
}
