// backend/src/services/data_sources/csv/upload.rs
// Rust
//! Module responsible for handling multipart CSV uploads paired with a JSON `DataSource` payload.
//!
//! High-level behavior:
//! - Receives a multipart payload that must include a `json` part (the DataSource) and a `file` part (the CSV).
//! - Validates that the referenced template exists in the `templify.sqlite` database immediately after
//!   parsing the JSON part. If the template does not exist, processing stops and an error is returned.
//! - Reads the existing `datasource_md5` value (may be `NULL`) from the `templates` table.
//! - Streams the uploaded file to disk while computing an MD5 hash incrementally.
//! - Compares the computed MD5 with the DB value: if equal, returns `Ok(true)` (no update).
//!   If different or NULL, updates the DB with the new MD5 and returns `Ok(false)`.
//!
//! Important notes for maintainers:
//! - The database path `templify.sqlite` is used directly here; consider injecting/configuring the DB path
//!   or connection for better testability and portability.
//! - The function returns `Result<bool, Box<dyn std::error::Error>>` where the `bool` encodes whether the
//!   MD5 matched the stored value (`true`) or required an update (`false`).
//! - Error messages returned to clients are simple strings; for an API, consider using structured error types.

use actix_multipart::Multipart;
use actix_web::{HttpResponse, Responder};
use common::model::datasource::DataSource;
use futures_util::StreamExt;
use md5::Context;
use rusqlite::{params, Connection};
use serde_json::from_slice;
use std::fs::File;
use std::io::Write;

/// HTTP handler wrapper that converts internal result to an `HttpResponse`.
///
/// - On success: returns `200 OK` with the boolean result as body.
/// - On failure: returns `400 Bad Request` with the error message.
pub async fn process(payload: Multipart) -> impl Responder {
    match upload_data_source(payload).await {
        Ok(result) => HttpResponse::Ok().body(result.to_string()),
        Err(e) => HttpResponse::BadRequest().body(format!("Error: {}", e)),
    }
}

/// Uploads a CSV file associated with a `DataSource` JSON part and synchronizes the `datasource_md5`
/// field in the local `templify.sqlite` database.
///
/// Behavior details:
/// - Expects the multipart payload to contain:
///   * a `json` part containing the serialized `DataSource` (must include `template_id`),
///   * a `file` part containing the `.csv` file to store and hash.
/// - Immediately validates that the template with `template_id` exists. If not found, returns an error
///   and does not process the file.
/// - Reads the `datasource_md5` column which may be `NULL` (mapped to `Option<String>`).
/// - Streams the file to disk as `{template_id}.csv` while computing the MD5 incrementally.
/// - After receiving the whole file, compares the computed MD5 with the DB value:
///   * If equal -> returns `Ok(true)` (no DB update).
///   * If different or NULL -> updates DB with the new MD5 and returns `Ok(false)`.
///
/// Returns:
/// - `Ok(true)`  -> MD5 matched, no update.
/// - `Ok(false)` -> MD5 updated (or was NULL).
/// - `Err(...)`  -> Any validation, IO, JSON, or DB error encountered.
pub async fn upload_data_source(
    mut payload: Multipart,
) -> Result<bool, Box<dyn std::error::Error>> {
    use std::io::BufWriter;

    // Parsed DataSource from the `json` part; must be present before processing the file.
    let mut data_source: Option<DataSource> = None;

    // `db_md5` stores the `datasource_md5` read from the DB. `None` means the column was NULL.
    let mut db_md5: Option<String> = None;

    // MD5 hasher updated while streaming the file chunks.
    let mut md5_hasher = Context::new();

    // Track whether we actually wrote a file part.
    let mut file_written = false;

    // Iterate multipart fields in arrival order.
    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_type = field
            .content_disposition()
            .and_then(|cd| cd.get_name().map(|n| n.to_string()));

        match content_type.as_deref() {
            // File part: stream to disk and feed bytes into MD5 hasher.
            Some("file") => {
                let filename = field
                    .content_disposition()
                    .and_then(|cd| cd.get_filename().map(|f| f.to_string()))
                    .unwrap_or_default();

                // Basic validation: require .csv extension.
                if !filename.ends_with(".csv") {
                    return Err("The file must end with .csv".into());
                }

                // Ensure we already received and validated the DataSource JSON before processing the file.
                if let Some(ref ds) = data_source {
                    // Write streamed chunks to `{template_id}.csv` while updating the MD5 context.
                    let file = File::create(format!("{}.csv", ds.template_id))?;
                    let mut writer = BufWriter::new(file);

                    while let Some(chunk) = field.next().await {
                        let chunk = chunk?;
                        // `consume` appends bytes into the MD5 context incrementally.
                        md5_hasher.consume(&chunk);
                        writer.write_all(&chunk)?;
                    }

                    file_written = true;
                } else {
                    // If JSON part not present before file, fail fast.
                    return Err("DataSource JSON must be sent before the file".into());
                }
            }

            // JSON part: parse Payload into `DataSource`, then validate template existence and read DB MD5.
            Some("json") => {
                let mut bytes = Vec::new();
                while let Some(chunk) = field.next().await {
                    bytes.extend_from_slice(&chunk?);
                }

                let ds: DataSource = from_slice(&bytes)?;

                // Immediately open DB and validate template existence, reading `datasource_md5` (may be NULL).
                let conn = Connection::open("templify.sqlite")?;
                let db_md5_result: Result<Option<String>, rusqlite::Error> = conn.query_row(
                    "SELECT datasource_md5 FROM templates WHERE id = ?1",
                    params![ds.template_id],
                    |row| row.get::<_, Option<String>>(0),
                );

                match db_md5_result {
                    // Found row: store the optional md5 and accept the DataSource.
                    Ok(opt_val) => {
                        db_md5 = opt_val;
                        data_source = Some(ds);
                    }
                    // No row -> template not found; stop processing immediately.
                    Err(rusqlite::Error::QueryReturnedNoRows) => {
                        return Err("Template not found".into());
                    }
                    // Any other DB error -> bubble up.
                    Err(e) => return Err(Box::new(e)),
                }
            }

            // Ignore other multipart parts.
            _ => {}
        }
    }

    // Validate that both JSON and file were present and processed.
    let ds = data_source.ok_or("Missing DataSource")?;
    if !file_written {
        return Err("Missing file".into());
    }

    // Finalize the MD5 hex string from the incremental context.
    let computed_md5 = format!("{:x}", md5_hasher.finalize());

    // Compare with DB value (which may be NULL). If equal, return true; otherwise update DB and return false.
    match db_md5.as_ref() {
        Some(existing) if existing == &computed_md5 => {
            // Match -> return true (no update needed).
            Ok(true)
        }
        _ => {
            // Mismatch or NULL -> update DB with new md5 and return false.
            let conn = Connection::open("templify.sqlite")?;
            conn.execute(
                "UPDATE templates SET datasource_md5 = ?1 WHERE id = ?2",
                params![computed_md5, ds.template_id],
            )?;
            Ok(false)
        }
    }
}
