//! Provides the API endpoint for querying the status of asynchronous background jobs.
//!
//! This module is a key part of the long-polling mechanism for tracking tasks like CSV
//! verification. After a client initiates a job via the `POST /api/data_sources/csv/verify`
//! endpoint (handled by `verify.rs`), it receives a `job_id`. The client can then
//! repeatedly call the endpoint defined here, `GET /api/data_sources/csv/status/{job_id}`,
//! to get the current status of that job.
//!
//! The handler reads from the shared, thread-safe `JobsState` (defined in `job_controller/state.rs`),
//! which acts as the single source of truth for the status of all ongoing and completed jobs.

use crate::job_controller::state::JobsState;
use actix_web::{web, Responder};

/// The main Actix web handler for the `GET /api/data_sources/csv/status/{job_id}` route.
///
/// It extracts the `job_id` from the URL path and the shared `JobsState` from the
/// application data, then delegates to `get_csv_job_status` to perform the lookup.
///
/// # Arguments
/// * `job_id` - The unique identifier of the job, provided as a path parameter.
/// * `state` - A `web::Data` wrapper around the application's shared `JobsState`.
///
/// # Returns
/// An `impl Responder` that resolves to one of the following HTTP responses:
/// - `200 OK` with a JSON body containing the `JobStatus` if the job ID is found.
/// - `404 Not Found` with a plain text body if the job ID does not exist in the state.
pub(crate) async fn process(
    job_id: web::Path<String>,
    state: web::Data<JobsState>,
) -> impl Responder {
    get_csv_job_status(job_id, state).await
}

/// Core logic to retrieve the status of a specific background job.
///
/// This function acquires a read lock on the `jobs` map within the shared `JobsState`.
/// It then looks up the status associated with the given `job_id`. This provides a
/// non-blocking, thread-safe way for HTTP requests to inspect the state of a job
/// that is being executed in a separate background task.
///
/// # Arguments
/// * `job_id` - The unique identifier for the job to look up.
/// * `state` - The shared `JobsState` containing the master record of all jobs.
///
/// # Returns
/// An `HttpResponse` containing either the job's status or a "not found" error.
async fn get_csv_job_status(
    job_id: web::Path<String>,
    state: web::Data<JobsState>,
) -> impl Responder {
    let jobs = state.jobs.read().await;
    if let Some(status) = jobs.get(&job_id.into_inner()) {
        actix_web::HttpResponse::Ok().json(status)
    } else {
        actix_web::HttpResponse::NotFound().body("Job ID not found")
    }
}
