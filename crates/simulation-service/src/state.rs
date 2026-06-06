//! Shared application state: a configured store root, an in-memory run index, and
//! a job queue drained by a bounded worker pool.
//!
//! `POST /backtests` only *enqueues* a job and returns immediately; a fixed pool
//! of workers pulls ids off the queue and runs the simulation (the heavy compute
//! goes to `spawn_blocking`, off the async HTTP threads). Clients poll status and
//! fetch the result when it's done. This is in-process for now; the same
//! submit→poll→fetch contract survives a future move to an external queue/worker.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_channel::{Receiver, Sender, TrySendError};
use serde_json::Value;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};

/// The inputs a worker needs to run a queued job.
#[derive(Clone)]
pub struct StoredRequest {
    pub graph: Graph,
    pub config: BacktestConfig,
    pub policy: SimulationPolicy,
    pub market_data: Option<MarketDataBundle>,
}

#[derive(Clone)]
pub struct Job {
    pub id: String,
    pub status: String, // queued | running | succeeded | failed
    pub error: Option<String>,
    pub graph_hash: String,
    pub policy_profile: String,
    pub start: String,
    pub end: String,
    pub interval: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub request: StoredRequest,
    pub summary: Value,
    pub result: Option<Value>,
    pub trace: Option<Value>,
    pub data_coverage: Vec<Value>,
    pub warnings: Vec<String>,
}

impl Job {
    pub fn queued(id: String, request: StoredRequest) -> Self {
        let graph_hash = crate::support::graph_hash(&request.graph);
        Job {
            id,
            status: "queued".into(),
            error: None,
            graph_hash,
            policy_profile: request.policy.profile.clone(),
            start: request.config.start.clone(),
            end: request.config.end.clone(),
            interval: request.config.interval.clone(),
            created_at: now(),
            started_at: None,
            finished_at: None,
            request,
            summary: Value::Null,
            result: None,
            trace: None,
            data_coverage: vec![],
            warnings: vec![],
        }
    }
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Debug)]
pub enum SubmitError {
    QueueFull,
}

struct Inner {
    store_root: Option<String>,
    runs: Mutex<HashMap<String, Job>>,
    counter: AtomicU64,
    tx: Sender<String>,
    rx: Receiver<String>,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState::new(None, 1024)
    }
}

impl AppState {
    pub fn new(store_root: Option<String>, queue_capacity: usize) -> Self {
        let (tx, rx) = async_channel::bounded(queue_capacity.max(1));
        AppState {
            inner: Arc::new(Inner {
                store_root,
                runs: Mutex::new(HashMap::new()),
                counter: AtomicU64::new(0),
                tx,
                rx,
            }),
        }
    }

    pub fn store_root(&self) -> Option<String> {
        self.inner.store_root.clone()
    }

    pub fn next_run_id(&self) -> String {
        let n = self.inner.counter.fetch_add(1, Ordering::Relaxed);
        format!("run_{}_{n}", chrono::Utc::now().timestamp_micros())
    }

    /// Record a queued job and enqueue it for a worker. Returns `QueueFull` if the
    /// queue is at capacity (the job is not recorded in that case).
    pub fn submit(&self, job: Job) -> Result<(), SubmitError> {
        let id = job.id.clone();
        self.inner.runs.lock().unwrap().insert(id.clone(), job);
        match self.inner.tx.try_send(id.clone()) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => {
                self.inner.runs.lock().unwrap().remove(&id);
                Err(SubmitError::QueueFull)
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.inner.runs.lock().unwrap().get(id).cloned()
    }

    pub fn list(&self, graph_hash: Option<&str>) -> Vec<Job> {
        let mut out: Vec<Job> = self
            .inner
            .runs
            .lock()
            .unwrap()
            .values()
            .filter(|j| graph_hash.map(|h| j.graph_hash == h).unwrap_or(true))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        out
    }

    /// Apply a mutation to a stored job (used by workers to advance status).
    pub fn update(&self, id: &str, f: impl FnOnce(&mut Job)) {
        if let Some(job) = self.inner.runs.lock().unwrap().get_mut(id) {
            f(job);
        }
    }

    /// Spawn `n` worker tasks draining the queue. Must be called within a Tokio runtime.
    pub fn start_workers(&self, n: usize) {
        for _ in 0..n.max(1) {
            let state = self.clone();
            let rx = self.inner.rx.clone();
            tokio::spawn(async move {
                while let Ok(id) = rx.recv().await {
                    crate::worker::run_job(&state, &id).await;
                }
            });
        }
    }
}
