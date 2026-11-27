//! Public Bar model and append-only mmap storage used by the aggregator.

pub mod bar_store;
pub mod model;

#[cfg(test)]
mod persistence {
    use crate::model::{Bar, decimal_string};
    use serde::Serialize;

    #[derive(Serialize)]
    pub struct BarEvent<'a> {
        pub event_id: &'a str,
        pub event_type: &'a str,
        #[serde(serialize_with = "decimal_string::serialize")]
        pub timestamp: u64,
        pub source: &'a str,
        pub symbol: &'a str,
        pub timeframe: &'a str,
        #[serde(serialize_with = "decimal_string::serialize")]
        pub revision: u64,
        pub is_final: bool,
        pub data: &'a Bar,
    }
}
