//! Handles the multipart/form-data upload of CSV files for data sources.
//!
//! This module provides the `POST /api/data_sources/csv/upload` endpoint, which is the
//! primary mechanism for introducing new CSV data into the system. It works in concert
//! with the verification module (`verify.rs`) by preparing the data and database state
//! for a subsequent, asynchronous validation process.
//!
//! ---
//!
//! ## Workflow
//!
//! 1.  **Receive Multipart Data**: The handler expects a `multipart/form-data` request
//!     containing two parts:
//!     - `json`: A JSON string representing the `DataSource` model, which includes the
//!       `template_id` to associate the CSV with.
//!     - `file`: The raw binary data of the CSV file.
//!
//! 2.  **Stream and Hash**: The file is streamed to a temporary location on disk.
//!     Simultaneously, an MD5 checksum of the file's contents is computed. This avoids
//!     loading the entire file into memory and ensures data integrity.
//!
//! 3.  **Preserve Previous State for Rollback**: Before updating the template with the new
//!     data source, it checks if the existing data source was `verified`. If it was,
//!     the current `datasource_md5` is copied to the `last_verified_md5` column in the
//!     `templates` table. This is a critical step that enables the verification service
//!     (`verify.rs`) to roll back to the last known-good version if the new file fails
//!     validation.
//!
//! 4.  **Persist File**: The temporary file is renamed to its final destination, following
//!     the convention `{template_id}_{computed_md5}.csv`. This naming scheme ensures
//!     that each unique file version has a unique path.
//!
//! 5.  **Update Database**: The `templates` table is updated for the given `template_id`.
//!     The `datasource_md5` is set to the newly computed hash, and the `verified` flag
//!     is set to `0` (false), indicating that the new file requires validation.

use actix_multipart::Multipart;
use actix_web::{HttpResponse, Responder};
use common::model::datasource::DataSource;
use futures_util::StreamExt;
use md5::Context;
use rusqlite::{params, Connection};
use serde_json::from_slice;
use std::fs::{rename, File};
use std::io::{BufWriter, Write};

type DynError = Box<dyn std::error::Error>;

/// HTTP handler for the CSV upload endpoint (`POST /api/data_sources/csv/upload`).
///
/// Accepts a `multipart/form-data` payload and delegates processing to
/// `upload_data_source`.
///
/// # Returns
/// - `200 OK` on success.
/// - `400 Bad Request` with an error message if the upload fails due to invalid
///   data, missing parts, or internal processing errors.
pub async fn process(payload: Multipart) -> impl Responder {
    match upload_data_source(payload).await {
        Ok(_) => HttpResponse::Ok().finish(),
        Err(e) => HttpResponse::BadRequest().body(format!("Error: {}", e)),
    }
}

/// Parses a multipart upload, persists the uploaded CSV, and updates template metadata.
///
/// This function orchestrates the entire upload process, from parsing the request
/// to updating the database.
///
/// # Behavior
/// - Expects two multipart fields: `json` (a serialized `DataSource`) and `file` (the CSV).
/// - Streams the file to a temporary location while computing its MD5 checksum.
/// - If the template was previously verified (`verified == 1`), it updates
///   `last_verified_md5` with the current `datasource_md5` to enable rollbacks.
/// - Renames the temp file to its final name: `{template_id}_{md5}.csv`.
/// - Updates the `templates` table, setting `datasource_md5` to the new hash and
///   resetting `verified` to `0`.
///
/// # Arguments
/// * `payload` - The incoming `Multipart` stream from the Actix request.
///
/// # Errors
/// Returns an error if the `json` or `file` part is missing, or if any
/// filesystem or database operation fails.
pub async fn upload_data_source(mut payload: Multipart) -> Result<(), DynError> {
    let mut data_source: Option<DataSource> = None;
    let mut file_received = false;
    let temp_file_path = "upload_temp_file.csv";
    let mut md5_hasher = Context::new();

    // Prepare a buffered writer for the temporary file.
    let mut temp_file = BufWriter::new(File::create(temp_file_path)?);

    // Process each part of the multipart form data.
    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_name = field
            .content_disposition()
            .and_then(|cd| cd.get_name().map(|n| n.to_string()));

        match content_name.as_deref() {
            Some("json") => {
                let mut bytes = Vec::new();
                while let Some(chunk) = field.next().await {
                    bytes.extend_from_slice(&chunk?);
                }
                let ds: DataSource = from_slice(&bytes)?;
                data_source = Some(ds);
            }
            Some("file") => {
                file_received = true;
                while let Some(chunk) = field.next().await {
                    let data = chunk?;
                    md5_hasher.consume(&data); // Update hash.
                    temp_file.write_all(&data)?; // Write to temp file.
                }
            }
            _ => {} // Ignore other fields.
        }
    }
    temp_file.flush()?; // Ensure all buffered data is written to disk.

    let ds = data_source.ok_or("Missing 'json' part in multipart form")?;
    if !file_received {
        return Err("Missing 'file' part in multipart form".into());
    }

    let conn = Connection::open("templify.sqlite")?;

    // Fetch the current verification status and datasource MD5 for the template.
    let row = conn.query_row(
        "SELECT verified, datasource_md5 FROM templates WHERE id = ?1",
        params![ds.template_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)),
    );

    let (verified, datasource_md5) = match row {
        Ok((v, md5)) => (v, md5),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err("Template not found".into());
        }
        Err(e) => return Err(Box::new(e)),
    };

    // If the existing data source was verified, save its MD5 for potential rollback.
    if verified == 1 {
        conn.execute(
            "UPDATE templates SET last_verified_md5 = ?1 WHERE id = ?2",
            params![datasource_md5, ds.template_id],
        )?;
    }

    // Finalize the MD5 hash and format it as a hex string.
    let computed_md5 = format!("{:x}", md5_hasher.finalize());

    // Rename the temporary file to its permanent name.
    let final_file_name = format!("{}_{}.csv", ds.template_id, computed_md5);
    rename(temp_file_path, &final_file_name)?;

    // Update the template record with the new data source MD5 and reset verification status.
    conn.execute(
        "UPDATE templates SET datasource_md5 = ?1, verified = 0 WHERE id = ?2",
        params![computed_md5, ds.template_id],
    )?;

    Ok(())
}
