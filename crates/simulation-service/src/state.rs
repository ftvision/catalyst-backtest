//! Shared application state: a configured store root + an in-memory run index.
//!
//! The run index holds each run's status, trace, summarized result, and metadata
//! so the lifecycle endpoints can serve them. In-memory for now; a durable
//! artifact store is a later refinement.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct RunRecord {
    pub id: String,
    pub graph_hash: String,
    pub status: String, // "succeeded" | "failed"
    pub error: Option<String>,
    pub policy_profile: String,
    pub start: String,
    pub end: String,
    pub interval: String,
    pub created_at: String,
    pub summary: Value,
    pub result: Option<Value>,
    pub trace: Option<Value>,
    pub data_coverage: Vec<Value>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Default)]
pub struct AppState {
    /// Parquet store root for by-store runs/coverage (e.g. `data/market-data` or `s3://...`).
    pub store_root: Option<String>,
    runs: Arc<Mutex<HashMap<String, RunRecord>>>,
    counter: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(store_root: Option<String>) -> Self {
        AppState { store_root, ..Default::default() }
    }

    /// A process-unique, time-ordered run id.
    pub fn next_run_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let ts = chrono::Utc::now().timestamp_micros();
        format!("run_{ts}_{n}")
    }

    pub fn insert(&self, record: RunRecord) {
        self.runs.lock().unwrap().insert(record.id.clone(), record);
    }

    pub fn get(&self, id: &str) -> Option<RunRecord> {
        self.runs.lock().unwrap().get(id).cloned()
    }

    /// All runs, optionally filtered by graph hash, sorted by creation time.
    pub fn list(&self, graph_hash: Option<&str>) -> Vec<RunRecord> {
        let mut out: Vec<RunRecord> = self
            .runs
            .lock()
            .unwrap()
            .values()
            .filter(|r| graph_hash.map(|h| r.graph_hash == h).unwrap_or(true))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        out
    }
}
