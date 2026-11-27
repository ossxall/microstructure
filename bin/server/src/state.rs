use aggregator::bar_store::BarStore;
use pulsar::{Pulsar, TokioExecutor};
use questdb::QuestDb;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
pub struct AppState {
    pub questdb: Arc<QuestDb>,
    pub pulsar: Arc<Pulsar<TokioExecutor>>,
    pub pulsar_namespace: String,
    pub bar_stores: Arc<BTreeMap<String, Arc<BarStore>>>,
}

impl AppState {
    pub fn new(
        questdb: Arc<QuestDb>,
        pulsar: Arc<Pulsar<TokioExecutor>>,
        pulsar_namespace: String,
        bar_stores: BTreeMap<String, Arc<BarStore>>,
    ) -> Self {
        Self {
            questdb,
            pulsar,
            pulsar_namespace,
            bar_stores: Arc::new(bar_stores),
        }
    }
}
