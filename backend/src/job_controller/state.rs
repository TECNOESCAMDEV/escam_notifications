//! Manages the state of long-running, asynchronous background jobs.
//!
//! This module provides the core components for tracking the progress of tasks
//! that are executed outside the immediate request/response cycle, such as the
//! CSV verification process found in `backend/src/services/data_sources/csv/verify.rs`.
//!
//! The main components are:
//! - `JobsState`: A clonable, thread-safe struct that holds the shared state of all jobs.
//!   It is injected into the Actix application state in `main.rs`.
//! - `JobUpdate`: A message struct used to communicate status changes from a background
//!   job back to the central state manager.
//! - `start_job_updater`: A long-running task that listens for `JobUpdate` messages
//!   on an MPSC channel and updates the shared `JobsState` accordingly.

use common::jobs::JobStatus;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, RwLock};

/// A thread-safe, shareable container for the state of all background jobs.
///
/// This struct is created in `main.rs` and shared across the Actix application
/// as `web::Data`. It allows different parts of the application to interact with
/// the job system in a coordinated way.
#[derive(Clone)]
pub struct JobsState {
    /// A map from a unique job ID (String) to its current `JobStatus`.
    ///
    /// This map is the single source of truth for the status of all jobs.
    /// It is protected by an `Arc<RwLock>` to allow concurrent reads (e.g., by the
    /// `/api/data_sources/csv/status/{job_id}` endpoint) and exclusive writes
    /// (by the `start_job_updater` task).
    pub jobs: Arc<RwLock<HashMap<String, JobStatus>>>,

    /// A multi-producer, single-consumer (MPSC) channel sender.
    ///
    /// Background tasks (like the one spawned in `schedule_verify_job`) use this
    /// sender to push `JobUpdate` messages into a channel. This decouples the job
    /// execution logic from the state update logic, allowing tasks to report
    /// progress without needing direct write access to the `jobs` map.
    pub tx: mpsc::Sender<JobUpdate>,
}

/// Represents a status update for a specific background job.
///
/// These messages are sent by background workers via the `JobsState.tx` sender
/// and are processed by the `start_job_updater` task.
#[derive(Debug)]
pub struct JobUpdate {
    /// The unique identifier of the job being updated.
    pub(crate) job_id: String,
    /// The new status of the job.
    pub(crate) status: JobStatus,
}

/// Starts the central job state updater task.
///
/// This function should be spawned as a long-running background task (as seen in `main.rs`).
/// It continuously listens for `JobUpdate` messages on the provided `rx` receiver.
///
/// Upon receiving an update, it acquires a write lock on the `jobs` map in the
/// shared `JobsState` and updates the status for the corresponding `job_id`.
pub async fn start_job_updater(state: JobsState, mut rx: mpsc::Receiver<JobUpdate>) {
    while let Some(update) = rx.recv().await {
        let mut jobs = state.jobs.write().await;
        jobs.insert(update.job_id.clone(), update.status);
    }
}
