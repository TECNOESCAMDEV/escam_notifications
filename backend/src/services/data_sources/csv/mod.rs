//! Manages CSV data source interactions, including uploading, verification, and status tracking.
//!
//! This module provides the HTTP API endpoints for handling CSV files that serve as data sources
//! for templates. It integrates with the asynchronous job system (`job_controller`) to perform
//! long-running validation tasks without blocking the server.
//!
//! The provided routes are:
//! - `POST /api/data_sources/csv/upload`: Handles multipart/form-data uploads. It expects a `json`
//!   field containing template metadata and a `file` field with the CSV data. The file is saved
//!   to disk with a name derived from its MD5 hash, and the corresponding template record in the
//!   database is updated to link to this new file and mark it as unverified.
//!
//! - `POST /api/data_sources/csv/verify`: Initiates an asynchronous background job to validate a
//!   CSV file associated with a template. It immediately returns a unique `job_id`. The client
//!   can use this ID to poll for the verification status. The verification process checks for
//!   header integrity, data type consistency, and structural correctness.
//!
//! - `GET /api/data_sources/csv/status/{job_id}`: Allows clients to poll for the status of a
//!   background job (e.g., the verification job started by `/verify`). It takes a `job_id` as a
//!   path parameter and returns the current `JobStatus` (`Pending`, `InProgress`, `Completed`, or
//!   `Failed`) from the shared `JobsState`.

use actix_web::web::{get, post, scope};
use actix_web::Scope;

mod get_status;
mod upload;
mod verify;

const API_PATH: &str = "/api/data_sources/csv";

/// Configures and returns the Actix scope for CSV data source routes.
pub fn configure_routes() -> Scope {
    scope(API_PATH)
        // Route to start a new CSV verification job.
        .route("/verify", post().to(verify::process))
        // Route to get the status of an ongoing verification job.
        .route("/status/{job_id}", get().to(get_status::process))
        // Route to upload a new CSV file.
        .route("/upload", post().to(upload::process))
}
