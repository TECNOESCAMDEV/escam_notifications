//! # API Request Payloads
//!
//! This module defines the data structures used for deserializing the bodies of incoming
//! HTTP requests. These structs serve as the primary Data Transfer Objects (DTOs) for
//! client-to-server communication, ensuring that request payloads are strongly typed
//! and easily validated.
//!
//! Each struct in this module corresponds to a specific API endpoint and encapsulates
//! the parameters required for that operation. By centralizing these definitions in the
//! `common` crate, we maintain consistency between the expectations of the backend
//! services and the data sent by the frontend client.

use serde::Deserialize;

/// Represents the JSON payload for a request to the `POST /api/data_sources/csv/verify` endpoint.
///
/// This request is sent by the frontend to initiate a background job that validates the
/// integrity and structure of a CSV file associated with a specific template. The backend
/// service (`services::data_sources::csv::process`) receives this request, creates a new
/// job, and immediately returns a `job_id` to the client. The client can then use this
/// `job_id` to poll for the status of the verification process.
///
/// ## Workflow Context:
/// 1. A user associates a CSV file with a template in the frontend UI.
/// 2. The frontend sends a request containing this payload to the backend.
/// 3. The backend's `csv::process` handler uses the `uuid` (as `template_id`) to find the
///    template and its associated data source information in the database.
/// 4. It then schedules a blocking task (`verify_csv_data_blocking`) to perform the
///    heavy lifting of reading and validating the CSV file without blocking the server's
///    async runtime.
#[derive(Deserialize)]
pub struct VerifyCsvRequest {
    /// The unique identifier (UUID) of the `Template` for which the associated CSV data
    /// source should be verified. This ID acts as the key to link the verification
    /// request to the correct template and its corresponding data file on the server.
    pub uuid: String,
}

/// Represents the payload for a request to `POST /api/merge/start`.
/// Starts a background job to generate PDFs from a template and its associated CSV data source.
#[derive(Deserialize)]
pub struct StartMergeRequest {
    /// The unique identifier (UUID) of the template to use for the merge.
    pub template_id: String,
}