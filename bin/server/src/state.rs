use pulsar::{Pulsar, TokioExecutor};
use questdb::QuestDb;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub questdb: Arc<QuestDb>,
    pub pulsar: Arc<Pulsar<TokioExecutor>>,
    pub pulsar_namespace: String,
}

impl AppState {
    pub fn new(
        questdb: Arc<QuestDb>,
        pulsar: Arc<Pulsar<TokioExecutor>>,
        pulsar_namespace: String,
    ) -> Self {
        Self {
            questdb,
            pulsar,
            pulsar_namespace,
        }
    }
}
