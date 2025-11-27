//! This module orchestrates the asynchronous verification of CSV data sources.
//!
//! It provides the API endpoint to initiate a verification job and contains the core logic
//! for validating the CSV file in a background task. This decouples the potentially
//! long-running verification process from the HTTP request/response cycle, allowing the
//! client to receive an immediate job ID and poll for status updates.
//!
//! ## Workflow
//!
//! 1.  **Initiation**: A client sends a `POST` request to `/api/data_sources/csv/verify`
//!     (handled by `process`) with a template ID.
//!
//! 2.  **Job Scheduling**: The `schedule_verify_job` function is called. It generates a unique
//!     `job_id`, sets the initial job status to `Pending` in the shared `JobsState`, and
//!     returns the `job_id` to the client immediately.
//!
//! 3.  **Background Execution**: A non-blocking Tokio task is spawned. This task, in turn,
//!     spawns a blocking thread using `tokio::task::spawn_blocking` to execute the CPU-intensive
//!     `verify_csv_data_blocking` function. This prevents the verification work from stalling
//!     the Tokio runtime.
//!
//! 4.  **Verification Logic**: `verify_csv_data_blocking` performs the validation:
//!     - It fetches the template's metadata from the database, including the current
//!       `datasource_md5` and `last_verified_md5`.
//!     - It implements a "fast-path" optimization: if the current CSV is already marked as
//!       verified (`verified == 1` and `datasource_md5 == last_verified_md5`), it simply
//!       infers column types from the first data row and completes the job successfully
//!       without a full scan.
//!     - It reads the CSV file chunk by chunk, validating headers and data rows in parallel
//!       using Rayon for efficiency.
//!     - It sends `JobStatus::InProgress` updates via the `mpsc::Sender` in `JobsState`
//!       as it processes chunks.
//!
//! 5.  **Outcome & State Update**:
//!     - **On Success**: The `templates` table in the database is updated to set `verified = 1`.
//!       A `JobStatus::Completed` message, containing the inferred column schema as a JSON
//!       string, is sent to the job controller.
//!     - **On Failure**: If any validation error occurs (e.g., bad header, invalid data),
//!       the database is rolled back by restoring the `datasource_md5` from `last_verified_md5`
//!       (if available). A `JobStatus::Failed` message with a descriptive error is sent.
//!
//! 6.  **Status Polling**: The client uses the `job_id` to poll the
//!     `GET /api/data_sources/csv/status/{job_id}` endpoint (defined in `get_status.rs`),
//!     which reads the job's current status from the shared `JobsState`.

use crate::job_controller::state::{JobUpdate, JobsState};
use actix_web::{web, HttpResponse, Responder};
use common::jobs::JobStatus;
use common::model::csv::ColumnCheck;
use common::model::place_holder::PlaceholderType;
use common::requests::VerifyCsvRequest;
use rayon::prelude::*;
use rusqlite::{params, Connection};
use serde_json;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    time::Instant,
};
use tokio::sync::mpsc;

/// Validates a single cell value against a `PlaceholderType`.
///
/// # Arguments
/// * `var_type` - The expected data type for the cell.
/// * `value` - The string content of the cell to validate.
///
/// # Returns
/// `true` if the `value` conforms to the `var_type` heuristic, `false` otherwise.
fn validate_value(var_type: &PlaceholderType, value: &str) -> bool {
    match var_type {
        PlaceholderType::Text => true,
        PlaceholderType::Number | PlaceholderType::Currency => value.parse::<f64>().is_ok(),
        PlaceholderType::Email => value.contains('@') && value.contains('.'),
    }
}

/// Searches a chunk of lines for the first invalid row using parallel iteration.
///
/// This function leverages Rayon's `par_iter` to efficiently scan multiple rows at once.
///
/// # Arguments
/// * `chunk` - A slice of tuples, where each tuple contains a line's index and its string content.
/// * `columns` - A slice of `ColumnCheck` structs defining the expected type for each column.
/// * `title_to_index` - A map from column titles to their zero-based index.
/// * `delimiter` - The character used to separate columns in the CSV.
///
/// # Returns
/// `Some((row_index, column_title, reason))` if an invalid cell is found. The `row_index`
/// is the 1-based CSV row number. Returns `None` if the entire chunk is valid.
fn find_first_invalid(
    chunk: &[(usize, String)],
    columns: &[ColumnCheck],
    title_to_index: &HashMap<String, usize>,
    delimiter: char,
) -> Option<(usize, String, String)> {
    chunk.par_iter().find_map_any(|(idx, line)| {
        let record: Vec<_> = line.split(delimiter).collect();
        for col in columns {
            if let Some(&col_idx) = title_to_index.get(&col.title) {
                if col_idx >= record.len() {
                    return Some((
                        idx + 2,
                        col.title.clone(),
                        "column missing in row".to_string(),
                    ));
                }
                let cell = normalize_cell(record[col_idx]);
                if !validate_value(&col.placeholder_type, &cell) {
                    let tipo = match col.placeholder_type {
                        PlaceholderType::Text => "text",
                        PlaceholderType::Number => "number",
                        PlaceholderType::Currency => "currency",
                        PlaceholderType::Email => "email",
                    };
                    return Some((
                        idx + 2,
                        col.title.clone(),
                        format!("value '{}' does not match expected type: {}", cell, tipo),
                    ));
                }
            } else {
                return Some((
                    idx + 2,
                    col.title.clone(),
                    "header title not found".to_string(),
                ));
            }
        }
        None
    })
}

/// Trims and normalizes a CSV cell's content.
///
/// This function performs the following operations:
/// 1. Removes surrounding whitespace.
/// 2. Strips outer single or double quotes.
/// 3. Replaces non-breaking spaces (`\u{00A0}`) with regular spaces.
/// 4. Trims whitespace again.
///
/// # Arguments
/// * `cell` - The raw string content of the cell.
///
/// # Returns
/// A normalized `String`.
fn normalize_cell(cell: &str) -> String {
    let s = cell.trim();
    let s = s
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .map(|s| s.to_string())
        .unwrap_or_else(|| s.to_string());
    s.replace('\u{00A0}', " ").trim().to_string()
}

/// Validates the header line of the CSV and normalizes the titles.
///
/// This function ensures that:
/// - The header is not empty.
/// - No title is empty after normalization.
/// - No title is purely numeric.
/// - All normalized titles are unique.
///
/// Normalization involves collapsing runs of whitespace into a single underscore (`_`).
///
/// # Arguments
/// * `header_line` - The raw string of the CSV header row.
/// * `delimiter` - The column delimiter character.
///
/// # Returns
/// A `Result` containing a `Vec<String>` of normalized titles on success, or an error `String` on failure.
pub(crate) fn validate_and_normalize_titles(
    header_line: &str,
    delimiter: char,
) -> Result<Vec<String>, String> {
    let raw_titles: Vec<String> = header_line
        .split(delimiter)
        .map(|s| normalize_cell(s))
        .collect();

    if raw_titles.is_empty() {
        return Err("Header line contains no titles".to_string());
    }

    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(raw_titles.len());

    for t in raw_titles {
        let t_trim = t.trim();
        if t_trim.is_empty() {
            return Err("Header contains an empty title".to_string());
        }

        // Reject purely numeric titles
        if t_trim.parse::<f64>().is_ok() {
            return Err(format!(
                "Header titles must be textual, but found numeric title: '{}'",
                t_trim
            ));
        }

        // Normalize spaces: collapse runs of whitespace into a single underscore
        let norm = t_trim.split_whitespace().collect::<Vec<_>>().join("_");

        if seen.contains(&norm) {
            return Err(format!(
                "Duplicate title in header after normalization: '{}'",
                norm
            ));
        }
        seen.insert(norm.clone());
        normalized.push(norm);
    }

    Ok(normalized)
}

/// Infers the `PlaceholderType` for each column based on the first data row.
///
/// It uses simple heuristics to guess the data type (Email, Currency, Number, or Text)
/// and captures the value from the first data row for each column.
///
/// # Arguments
/// * `titles` - A slice of normalized header titles.
/// * `second_line` - The string content of the first data row (the second line of the file).
/// * `delimiter` - The column delimiter character.
///
/// # Returns
/// A `Vec<ColumnCheck>` where each element corresponds to a column, containing its title,
/// inferred type, and the value from the first data row.
fn infer_column_checks(titles: &[String], second_line: &str, delimiter: char) -> Vec<ColumnCheck> {
    let cells: Vec<String> = second_line
        .split(delimiter)
        .map(|c| normalize_cell(c))
        .collect();

    let currency_symbols = ['$', '€', '£', '¥'];
    let mut columns = Vec::with_capacity(titles.len());

    for (idx, title) in titles.iter().enumerate() {
        let (placeholder_type, first_row) = if idx < cells.len() {
            let val = cells[idx].trim();
            let placeholder_type = if val.contains('@') && val.contains('.') {
                PlaceholderType::Email
            } else if val.chars().any(|ch| currency_symbols.contains(&ch)) {
                PlaceholderType::Currency
            } else if val.parse::<f64>().is_ok() {
                PlaceholderType::Number
            } else {
                PlaceholderType::Text
            };
            (placeholder_type, Some(cells[idx].clone()))
        } else {
            (PlaceholderType::Text, None)
        };

        columns.push(ColumnCheck {
            title: title.clone(),
            placeholder_type,
            first_row,
        });
    }

    columns
}

/// Updates the template's verification status in the database after a verification attempt.
///
/// - On success, it sets `verified = 1` and updates `last_verified_md5` to the current `datasource_md5`.
/// - On failure, it performs a rollback by setting `verified = 1` but restoring `datasource_md5`
///   from the `last_verified_md5` field. This effectively reverts to the last known-good version.
///
/// # Arguments
/// * `conn` - A reference to the database connection.
/// * `id` - The ID of the template to update.
/// * `datasource_md5` - The MD5 hash of the file that was just verified.
/// * `last_verified_md5` - The MD5 hash of the previously verified file, used for rollback.
/// * `success` - A boolean indicating whether the verification was successful.
///
/// # Returns
/// `Ok(())` on success, or an error `String` if the database operation fails.
fn update_template_verification(
    conn: &Connection,
    id: &str,
    datasource_md5: Option<&str>,
    last_verified_md5: Option<&str>,
    success: bool,
) -> Result<(), String> {
    if success {
        conn.execute(
            "UPDATE templates SET verified = 1, last_verified_md5 = ?1 WHERE id = ?2",
            params![datasource_md5, id],
        )
            .map_err(|e| e.to_string())?;
    } else {
        conn.execute(
            "UPDATE templates SET verified = 1, datasource_md5 = ?1 WHERE id = ?2",
            params![last_verified_md5, id],
        )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Sends a `JobStatus::Failed` update via the MPSC channel.
///
/// This is a helper to format a failure message and send it using a blocking send,
/// suitable for use within a synchronous context like `spawn_blocking`.
///
/// # Arguments
/// * `tx` - The MPSC sender for `JobUpdate` messages.
/// * `job_id` - The ID of the failing job.
/// * `row` - The row number where the error occurred.
/// * `title` - The column title where the error occurred.
/// * `reason` - A string describing the validation failure.
/// * `start` - The `Instant` when the job started, used for logging total duration.
///
/// # Returns
/// `Ok(())` on success. The underlying send can fail if the receiver is dropped.
fn handle_first_invalid_sync(
    tx: &mpsc::Sender<JobUpdate>,
    job_id: &str,
    row: usize,
    title: &str,
    reason: &str,
    start: Instant,
) -> Result<(), String> {
    let _ = tx.blocking_send(JobUpdate {
        job_id: job_id.to_string(),
        status: JobStatus::Failed(format!(
            "First invalid row at: row {}, column '{}': {}",
            row, title, reason
        )),
    });
    println!("verify_csv_data finished in: {:.2?}", start.elapsed());
    Ok(())
}

/// Processes a single chunk of CSV lines synchronously.
///
/// It calls `find_first_invalid` to check for validation errors within the chunk.
///
/// # Arguments
/// * `chunk` - The slice of lines to process.
/// * `columns` - The column schema to validate against.
/// * `title_to_index` - A map from column titles to their index.
/// * `delimiter` - The CSV delimiter character.
///
/// # Returns
/// `Ok(Some(details))` if an invalid row is found, where `details` is a tuple containing
/// the row, title, and reason. `Ok(None)` if the chunk is valid. `Err` on internal failure.
fn process_chunk_sync(
    chunk: &[(usize, String)],
    columns: &[ColumnCheck],
    title_to_index: &HashMap<String, usize>,
    delimiter: char,
) -> Result<Option<(usize, String, String)>, String> {
    Ok(find_first_invalid(
        chunk,
        columns,
        title_to_index,
        delimiter,
    ))
}

/// Reads the header line and the first data line from a CSV file.
///
/// # Arguments
/// * `reader` - A mutable reference to a `BufReader` for the CSV file.
///
/// # Returns
/// A `Result` containing a tuple `(header_line, second_line)` on success, or an
/// error `String` if the file is empty, contains no data rows, or a read error occurs.
pub(crate) fn read_header_and_second_line(
    reader: &mut BufReader<File>,
) -> Result<(String, String), String> {
    let mut header_line = String::new();
    reader
        .read_line(&mut header_line)
        .map_err(|e| e.to_string())?;
    let header_line = header_line.trim_end_matches(&['\n', '\r'][..]).to_string();

    let mut second_line = String::new();
    if reader
        .read_line(&mut second_line)
        .map_err(|e| e.to_string())?
        == 0
    {
        return Err("CSV file does not contain any data rows".to_string());
    }
    let second_line = second_line.trim_end_matches(&['\n', '\r'][..]).to_string();

    Ok((header_line, second_line))
}

/// Detects the CSV delimiter by analyzing the header line.
///
/// It counts occurrences of candidate delimiters (`,`, `;`, `\t`, `|`) and selects the
/// one with the highest count. Defaults to comma (`,`) if no candidates are found.
///
/// # Arguments
/// * `header_line` - The header string to analyze.
///
/// # Returns
/// The detected delimiter character.
pub(crate) fn detect_delimiter(header_line: &str) -> char {
    [',', ';', '\t', '|']
        .iter()
        .max_by_key(|&&d| header_line.matches(d).count())
        .copied()
        .unwrap_or(',')
}

/// The main blocking verification function, designed to be run in `spawn_blocking`.
///
/// This function contains the complete, synchronous logic for CSV verification, including
/// database interaction, file I/O, and data validation. It sends status updates back to the
/// main async context via the provided MPSC sender.
///
/// # Arguments
/// * `tx` - The MPSC sender to communicate job status updates.
/// * `job_id` - The unique ID for this verification job.
/// * `template_id` - The ID of the template associated with the CSV file.
///
/// # Returns
/// A `Result` containing a JSON `String` of the inferred `ColumnCheck` schema on success,
/// or an error `String` on failure.
fn verify_csv_data_blocking(
    tx: mpsc::Sender<JobUpdate>,
    job_id: String,
    template_id: String,
) -> Result<String, String> {
    let start = Instant::now();

    // Open DB and fetch template row (allow NULLs)
    let conn = Connection::open("templify.sqlite").map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, datasource_md5, last_verified_md5, verified FROM templates WHERE id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let template = stmt
        .query_row(params![template_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i32>(3)?,
            ))
        })
        .map_err(|e| "Failed to get template from database: ".to_string() + &e.to_string())?;

    let (id, datasource_md5, last_verified_md5, verified) = template;

    // Fast-path: If the file is already verified and unchanged, skip the full scan.
    if let (Some(ds_md5), Some(last_md5)) =
        (datasource_md5.as_deref(), last_verified_md5.as_deref())
    {
        if ds_md5 == last_md5 && verified == 1 {
            let file_path = format!("./{}_{}.csv", id, ds_md5);
            if !Path::new(&file_path).exists() {
                return Err("CSV file not found".to_string());
            }
            let file = File::open(&file_path).map_err(|e| e.to_string())?;
            let mut reader = BufReader::new(file);

            let (header_line, second_line) = read_header_and_second_line(&mut reader)?;
            let delimiter = detect_delimiter(&header_line);

            let titles = validate_and_normalize_titles(&header_line, delimiter)
                .map_err(|e| format!("Header validation failed: {}", e))?;

            let columns = infer_column_checks(&titles, &second_line, delimiter);
            let json_columns = serde_json::to_string(&columns).map_err(|e| e.to_string())?;

            let _ = tx.blocking_send(JobUpdate {
                job_id: job_id.clone(),
                status: JobStatus::Completed(json_columns.clone()),
            });

            println!(
                "verify_csv_data finished (fast-path) in: {:.2?}",
                start.elapsed()
            );
            return Ok(json_columns);
        }
    }

    // If template has a stale verified flag, reset it and proceed with verification.
    if verified != 0 {
        conn.execute(
            "UPDATE templates SET verified = 0 WHERE id = ?1",
            params![id],
        )
            .map_err(|e| format!("Failed to reset verified flag: {}", e))?;
        println!(
            "Template '{}' had verified != 0; resetting to 0 and continuing verification.",
            id
        );
    }

    // From here, proceed with full verification.
    let ds_md5 = match datasource_md5.as_deref() {
        Some(s) => s,
        None => {
            // No datasource to verify, attempt rollback and fail.
            update_template_verification(
                &conn,
                &id,
                datasource_md5.as_deref(),
                last_verified_md5.as_deref(),
                false,
            )
                .map_err(|db_err| format!("Datasource MD5 missing; rollback failed: {}", db_err))?;
            return Err("No associated data file to verify".to_string());
        }
    };

    let file_path = format!("./{}_{}.csv", id, ds_md5);
    if !Path::new(&file_path).exists() {
        return Err("CSV file not found".to_string());
    }
    let file = File::open(&file_path).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);

    let (header_line, second_line) = read_header_and_second_line(&mut reader)?;
    let delimiter = detect_delimiter(&header_line);

    // Validate headers. If it fails, roll back and exit.
    let titles = match validate_and_normalize_titles(&header_line, delimiter) {
        Ok(t) => t,
        Err(e) => {
            update_template_verification(
                &conn,
                &id,
                datasource_md5.as_deref(),
                last_verified_md5.as_deref(),
                false,
            )
                .map_err(|db_err| {
                    format!(
                        "Header validation failed: {}; rollback failed: {}",
                        e, db_err
                    )
                })?;
            return Err(format!("Header validation failed: {}", e));
        }
    };

    let mut title_to_index = HashMap::new();
    for (i, t) in titles.iter().enumerate() {
        title_to_index.insert(t.clone(), i);
    }

    let columns = infer_column_checks(&titles, &second_line, delimiter);

    // Process file in chunks, sending progress updates.
    let chunk_size = 250_000;
    let mut chunk = Vec::with_capacity(chunk_size);
    let mut lines_processed = 0usize;

    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        chunk.push((i, line));
        if chunk.len() == chunk_size {
            process_and_handle_chunk(
                &tx,
                &job_id,
                &chunk,
                &columns,
                &title_to_index,
                delimiter,
                start,
                &conn,
                &id,
                datasource_md5.as_deref(),
                last_verified_md5.as_deref(),
            )?;
            lines_processed += chunk.len();
            chunk.clear();
            let _ = tx.blocking_send(JobUpdate {
                job_id: job_id.clone(),
                status: JobStatus::InProgress(lines_processed as u32),
            });
        }
    }

    // Process the final partial chunk.
    if !chunk.is_empty() {
        process_and_handle_chunk(
            &tx,
            &job_id,
            &chunk,
            &columns,
            &title_to_index,
            delimiter,
            start,
            &conn,
            &id,
            datasource_md5.as_deref(),
            last_verified_md5.as_deref(),
        )?;
    }

    // If we reach here, verification was successful.
    update_template_verification(
        &conn,
        &id,
        datasource_md5.as_deref(),
        last_verified_md5.as_deref(),
        true,
    )?;

    let json_columns = serde_json::to_string(&columns).map_err(|e| e.to_string())?;

    let _ = tx.blocking_send(JobUpdate {
        job_id: job_id.clone(),
        status: JobStatus::Completed(json_columns.clone()),
    });

    println!("verify_csv_data finished in: {:.2?}", start.elapsed());
    Ok(json_columns)
}

/// The Actix web handler for `POST /api/data_sources/csv/verify`.
///
/// It receives a `VerifyCsvRequest`, schedules the background verification job,
/// and immediately returns a job ID to the client.
///
/// # Arguments
/// * `jobs_state` - The shared `JobsState` injected by Actix.
/// * `req` - The JSON payload containing the `template_id` to verify.
///
/// # Returns
/// An `HttpResponse` with the `job_id` on success, or an `InternalServerError` on failure.
pub(crate) async fn process(
    jobs_state: web::Data<JobsState>,
    req: web::Json<VerifyCsvRequest>,
) -> impl Responder {
    match schedule_verify_job(jobs_state, req.into_inner()).await {
        Ok(job_id) => HttpResponse::Ok().body(job_id),
        Err(err) => HttpResponse::InternalServerError().body(err),
    }
}

/// Schedules the CSV verification job to run in the background.
///
/// This function creates a new job ID, sets its status to `Pending` in the shared `JobsState`,
/// and spawns a Tokio task to perform the actual work. The heavy lifting is delegated to
/// `verify_csv_data_blocking` inside a `spawn_blocking` call to avoid blocking the async runtime.
///
/// # Arguments
/// * `jobs_state` - The application's shared `JobsState`.
/// * `req` - The `VerifyCsvRequest` containing the template ID.
///
/// # Returns
/// A `Result` containing the new `job_id` on success, or an error `String` on failure.
async fn schedule_verify_job(
    jobs_state: web::Data<JobsState>,
    req: VerifyCsvRequest,
) -> Result<String, String> {
    let job_id = uuid::Uuid::new_v4().to_string();
    jobs_state
        .jobs
        .write()
        .await
        .insert(job_id.clone(), JobStatus::Pending);
    let tx = jobs_state.tx.clone();
    let value = job_id.clone();
    let js = jobs_state.clone();
    let uuid = req.uuid;

    tokio::spawn(async move {
        let tx_block = tx.clone();
        let value_for_blocking = value.clone();
        let uuid_for_blocking = uuid.clone();

        let handle = tokio::task::spawn_blocking(move || {
            verify_csv_data_blocking(tx_block, value_for_blocking, uuid_for_blocking)
        });

        match handle.await {
            Ok(Ok(json_columns)) => {
                js.jobs
                    .write()
                    .await
                    .insert(value, JobStatus::Completed(json_columns));
            }
            Ok(Err(e)) => {
                js.jobs.write().await.insert(value, JobStatus::Failed(e));
            }
            Err(join_err) => {
                js.jobs.write().await.insert(
                    value,
                    JobStatus::Failed(format!("task join error: {}", join_err)),
                );
            }
        }
    });

    Ok(job_id)
}

/// Processes a chunk of CSV lines and handles validation failures by initiating a DB rollback.
///
/// This function serves as a bridge between the synchronous chunk processing and the
/// database rollback logic. If `process_chunk_sync` finds an invalid row, this function
/// ensures the failure is reported and the database state is reverted.
///
/// # Arguments
/// * `tx` - Sender for `JobUpdate` messages.
/// * `job_id` - The ID of the current job.
/// * `chunk` - The slice of lines to validate.
/// * `columns` - The inferred column schema.
/// * `title_to_index` - Map from title to column index.
/// * `delimiter` - The CSV delimiter character.
/// * `start` - The job start time, for logging.
/// * `conn` - The database connection for performing a rollback.
/// * `id` - The template ID.
/// * `datasource_md5` - The MD5 of the current file.
/// * `last_verified_md5` - The MD5 of the last good file, for rollback.
///
/// # Returns
/// `Ok(())` if the chunk is valid. `Err(String)` if an invalid row was found, which
/// signals to the caller to stop processing.
fn process_and_handle_chunk(
    tx: &mpsc::Sender<JobUpdate>,
    job_id: &str,
    chunk: &[(usize, String)],
    columns: &[ColumnCheck],
    title_to_index: &HashMap<String, usize>,
    delimiter: char,
    start: Instant,
    conn: &Connection,
    id: &str,
    datasource_md5: Option<&str>,
    last_verified_md5: Option<&str>,
) -> Result<(), String> {
    if let Some((row, title, reason)) =
        process_chunk_sync(chunk, columns, title_to_index, delimiter)?
    {
        // Report the first invalid row found.
        handle_first_invalid_sync(tx, job_id, row, &title, &reason, start)?;
        // Roll back the template verification state in the database.
        update_template_verification(conn, id, datasource_md5, last_verified_md5, false)?;
        return Err(format!(
            "Verification failed: row {}, column '{}': {}",
            row, title, reason
        ));
    }
    Ok(())
}
