use serde::Serialize;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, RwLock};

#[derive(Clone)]
pub struct JobsState {
    pub jobs: Arc<RwLock<HashMap<String, JobStatus>>>,
    pub tx: mpsc::Sender<JobUpdate>,
}

#[derive(Clone, Debug, Serialize)]
pub enum JobStatus {
    Pending,
    InProgress(u8),
    Completed(String),
    Failed(String),
}

#[derive(Debug)]
pub struct JobUpdate {
    pub(crate) job_id: String,
    pub(crate) status: JobStatus,
}

pub async fn start_job_updater(state: JobsState, mut rx: mpsc::Receiver<JobUpdate>) {
    while let Some(update) = rx.recv().await {
        let mut jobs = state.jobs.write().await;
        jobs.insert(update.job_id.clone(), update.status);
    }
}

