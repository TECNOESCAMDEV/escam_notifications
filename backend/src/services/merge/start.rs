//! # Merge Job Start Service
//!
//! This module provides the `POST /api/merge/start` endpoint, which initiates a
//! background job to perform a "mail merge" operation. It combines a verified
//! data source (CSV) with a template to generate multiple PDF documents, one for
//! each row in the data file.
//!
//! ## Workflow:
//!
//! 1.  **HTTP Request**: The `process` handler receives a `StartMergeRequest` containing
//!     a `template_id`.
//!
//! 2.  **Job Scheduling**: It calls `schedule_merge_job`, which:
//!     - Creates a unique `job_id` for the merge operation.
//!     - Sets the initial job status to `Pending` in the shared `JobsState`.
//!     - Immediately returns the `job_id` to the client, allowing for asynchronous status polling.
//!     - Spawns a new Tokio task to manage the job's lifecycle.
//!
//! 3.  **Background Processing**: The spawned task uses `tokio::task::spawn_blocking` to
//!     run the `merge_blocking` function on a dedicated thread pool. This prevents the
//!     CPU-intensive file I/O and PDF generation from blocking the server's async runtime.
//!
//! 4.  **Data Validation**: `merge_blocking` first connects to the database to ensure the
//!     template's associated data source has been successfully verified (`verified = 1`).
//!     It retrieves the `datasource_md5` to locate the correct CSV file.
//!
//! 5.  **CSV Processing & Parallelization**: It reads the CSV file header and then uses the `rayon`
//!     crate to process each data row in parallel. This significantly speeds up the generation
//!     of a large number of PDFs.
//!
//! 6.  **Iterative PDF Generation**: For each row, a task is spawned in a `rayon` thread pool.
//!     - It calls `generate_pdf_for_task`, which fetches the template text, substitutes placeholders
//!       (e.g., `{{column_name}}`) with the corresponding values from the current row, and
//!       leverages the `services::templates::pdf` module to render a complete PDF.
//!     - Each generated PDF is saved with a unique name (e.g., `{job_id}_{row_index}.pdf`).
//!
//! 7.  **Progress Reporting**: A two-tiered channel system reports progress without blocking:
//!     - The synchronous `rayon` threads send `MergeUpdate` messages to an async listener task.
//!     - This listener translates these updates into `JobUpdate` messages (with calculated percentages)
//!       and forwards them to the central `job_controller` to update the global job state.
//!
//! 8.  **Error Handling**: The parallel processing uses an `Arc<Mutex>` to capture the *first*
//!     error that occurs in any thread. Once an error is captured, all other threads stop
//!     processing new rows, and the job is marked as `Failed`.

use crate::job_controller::state::{JobUpdate, JobsState};
use crate::services::data_sources::csv::verify::{
    detect_delimiter, read_header_and_second_line, validate_and_normalize_titles,
};
use crate::services::templates::pdf;
use actix_web::{web, HttpResponse, Responder};
use common::jobs::JobStatus;
use common::requests::StartMergeRequest;
use regex::Regex;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::{fs, io};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Represents a status update for a merge job or one of its sub-tasks.
///
/// This enum is used for internal communication, sending progress from the synchronous
/// worker thread (`merge_blocking`) back to the asynchronous Tokio task that manages
/// the job. This separation prevents the blocking worker from needing to `await` anything.
#[derive(Debug)]
pub enum MergeUpdate {
    /// Updates the overall status of the entire merge job (e.g., to Failed).
    Job(JobStatus),
    /// Reports the progress of an individual merge task (i.e., one PDF generation).
    /// This is used to calculate the percentage completion of the overall job.
    Task { row_index: usize, total_rows: usize },
}

/// The Actix web handler for `POST /api/merge/start`.
///
/// Receives a `StartMergeRequest`, schedules the background merge job,
/// and immediately returns a `job_id` to the client. The client can use this
/// ID to poll the job's status.
///
/// # Arguments
/// * `state` - The shared `JobsState`, injected by Actix, for managing job statuses.
/// * `payload` - The JSON payload containing the `template_id` for the merge.
///
/// # Returns
/// An `HttpResponse` with the `job_id` on success, or an `InternalServerError` on failure.
pub(crate) async fn process(
    state: web::Data<JobsState>,
    payload: web::Json<StartMergeRequest>,
) -> impl Responder {
    match schedule_merge_job(state, payload.into_inner()).await {
        Ok(job_id) => HttpResponse::Ok().json(serde_json::json!({ "job_id": job_id })),
        Err(err) => HttpResponse::InternalServerError().body(err),
    }
}

/// Schedules the CSV merge job to run in the background.
///
/// This function creates a new job ID, sets its status to `Pending` in the shared `JobsState`,
/// and spawns a Tokio task to perform the actual work. The heavy lifting is delegated to
/// `merge_blocking` within a `spawn_blocking` call to avoid blocking the async runtime.
///
/// # Arguments
/// * `state` - The application's shared `JobsState`.
/// * `req` - The `StartMergeRequest` containing the template ID.
///
/// # Returns
/// A `Result` containing the new `job_id` on success, or an error `String` on failure.
async fn schedule_merge_job(
    state: web::Data<JobsState>,
    req: StartMergeRequest,
) -> Result<String, String> {
    let job_id = Uuid::new_v4().to_string();
    // Immediately register the job as Pending.
    state
        .jobs
        .write()
        .await
        .insert(job_id.clone(), JobStatus::Pending);

    let tx = state.tx.clone(); // Channel to the central job updater.
    let job_id_clone = job_id.clone();
    let template_id = req.template_id;

    tokio::spawn(async move {
        // Create a dedicated channel for this specific job's updates.
        let (merge_tx, mut merge_rx) = mpsc::channel::<MergeUpdate>(100);

        // Spawn a listener task. It receives `MergeUpdate`s from the blocking worker
        // and translates them into `JobUpdate`s for the central job controller.
        let job_updater_tx = tx.clone();
        let job_id_for_updater = job_id_clone.clone();
        tokio::spawn(async move {
            while let Some(update) = merge_rx.recv().await {
                let status = match update {
                    MergeUpdate::Job(job_status) => job_status,
                    MergeUpdate::Task {
                        row_index,
                        total_rows,
                    } => {
                        // Calculate progress percentage based on the number of processed rows.
                        let progress = if total_rows > 0 {
                            ((row_index + 1) as f32 / total_rows as f32 * 100.0) as u32
                        } else {
                            0
                        };
                        JobStatus::InProgress(progress)
                    }
                };

                // Send the standardized update to the central job controller.
                let _ = job_updater_tx
                    .send(JobUpdate {
                        job_id: job_id_for_updater.clone(),
                        status,
                    })
                    .await;
            }
        });

        // Execute the synchronous, blocking part of the job in a dedicated thread.
        let job_id_for_blocking = job_id_clone.clone();
        let template_id_for_blocking = template_id.clone();
        let handle = tokio::task::spawn_blocking(move || {
            merge_blocking(merge_tx, &job_id_for_blocking, &template_id_for_blocking)
        });

        // Handle the result of the blocking task.
        match handle.await {
            Ok(Ok(_)) => {
                // On success, report completion.
                let _ = tx
                    .send(JobUpdate {
                        job_id: job_id_clone,
                        status: JobStatus::Completed("Merge completed successfully".to_string()),
                    })
                    .await;
            }
            Ok(Err(e)) => {
                // If the blocking task returned a specific error, report it as Failed.
                let _ = tx
                    .send(JobUpdate {
                        job_id: job_id_clone,
                        status: JobStatus::Failed(e),
                    })
                    .await;
            }
            Err(e) => {
                // If the task panicked or was cancelled, report a join error.
                let _ = tx
                    .send(JobUpdate {
                        job_id: job_id_clone,
                        status: JobStatus::Failed(format!("Task join error: {}", e)),
                    })
                    .await;
            }
        }
    });

    Ok(job_id)
}

/// The main synchronous merge function, designed to be run via `spawn_blocking`.
///
/// This function contains the complete, synchronous logic for the CSV merge, including
/// database interaction, file I/O, and parallel PDF generation using `rayon`. It sends status updates
/// back to the main async context via the provided MPSC sender.
///
/// # Arguments
/// * `tx` - The MPSC sender to communicate `MergeUpdate`s back to the async listener.
/// * `job_id` - The unique ID for this merge job.
/// * `template_id` - The ID of the template associated with the CSV file.
///
/// # Returns
/// An empty `Result` on success, or an error `String` on failure.
fn merge_blocking(
    tx: mpsc::Sender<MergeUpdate>,
    job_id: &str,
    template_id: &str,
) -> Result<(), String> {
    // Send initial progress update.
    let _ = tx.blocking_send(MergeUpdate::Job(JobStatus::InProgress(0)));

    let conn = Connection::open("templify.sqlite").map_err(|e| e.to_string())?;

    // Fetch template metadata to find the correct CSV file and check verification status.
    let (_id, datasource_md5, verified) =
        get_template_metadata(&conn, template_id).map_err(|e| e.to_string())?;

    // The data source must be verified before a merge can proceed.
    if verified != 1 {
        let err_msg = "Template data source has not been verified.".to_string();
        let _ = tx.blocking_send(MergeUpdate::Job(JobStatus::Failed(err_msg.clone())));
        return Err(err_msg);
    }

    let ds_md5 = datasource_md5.ok_or("Datasource MD5 not found for verified template.")?;
    let file_path = format!("./{}_{}.csv", template_id, ds_md5);
    let file = File::open(&file_path).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);

    // Read header and determine CSV delimiter.
    let (header_line, _) = read_header_and_second_line(&mut reader)?;
    let delimiter = detect_delimiter(&header_line);
    let titles = Arc::new(validate_and_normalize_titles(&header_line, delimiter)?);

    // Count total data rows for progress tracking.
    let total_rows = count_lines_raw(&file_path).map_err(|e| e.to_string())?;

    // --- Parallel processing with Rayon ---
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(10) // Limit concurrency to 10 threads.
        .build()
        .map_err(|e| e.to_string())?;

    // Use a shared, thread-safe container to capture the first error that occurs.
    let first_error = Arc::new(Mutex::new(None::<String>));

    let value = first_error.clone();
    pool.scope(move |s| {
        // Iterate over the remaining lines of the file (data rows).
        for (i, line_result) in reader.lines().enumerate() {
            // If an error has already occurred in another thread, stop processing new lines.
            if value.lock().unwrap().is_some() {
                break;
            }

            let line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    // If there's an error reading a line, capture it and break.
                    let mut error_guard = value.lock().unwrap();
                    *error_guard = Some(format!("Error reading line {}: {}", i, e));
                    break;
                }
            };

            // Clone necessary variables to move into the spawned thread.
            let tx_clone = tx.clone();
            let titles_clone = Arc::clone(&titles);
            let job_id_clone = job_id.to_string();
            let template_id_clone = template_id.to_string();
            let first_error_clone = Arc::clone(&value);

            s.spawn(move |_| {
                // Before doing any work, check if another thread has already failed.
                if first_error_clone.lock().unwrap().is_some() {
                    return;
                }

                // Parse the current CSV line into a map of placeholders.
                let mut placeholders = HashMap::new();
                let values: Vec<&str> = line.split(delimiter).collect();
                for (j, title) in titles_clone.iter().enumerate() {
                    if let Some(value) = values.get(j) {
                        placeholders.insert(title.clone(), value.to_string());
                    }
                }

                let output_filename = format!("{}_{}.pdf", job_id_clone, i);
                let output_path = Path::new("./pdfs").join(&output_filename);

                // Generate the PDF for the current row.
                if let Err(e) =
                    generate_pdf_for_task(&template_id_clone, &placeholders, &output_path)
                {
                    let err_msg = format!("Failed to generate PDF for row {}: {}", i + 1, e);
                    let mut error_guard = first_error_clone.lock().unwrap();
                    // Only store the very first error to occur.
                    if error_guard.is_none() {
                        *error_guard = Some(err_msg.clone());
                        // Immediately report the failure.
                        let _ =
                            tx_clone.blocking_send(MergeUpdate::Job(JobStatus::Failed(err_msg)));
                    }
                    return; // Stop this thread's execution.
                }

                // Report successful processing of this row.
                let _ = tx_clone.blocking_send(MergeUpdate::Task {
                    row_index: i,
                    total_rows,
                });
            });
        }
    });

    // After the pool finishes, check if any error was captured.
    if let Some(err_msg) = first_error.lock().unwrap().take() {
        return Err(err_msg);
    }

    Ok(())
}

/// Generates a single PDF for one row of data.
///
/// This function fetches the template content, substitutes placeholders with the provided
/// data, and then uses the `pdf` service's helpers to render the final document.
///
/// # Arguments
/// * `template_id` - The ID of the template to use.
/// * `placeholders` - A map of column titles to their values for the current row.
/// * `output_path` - The path where the generated PDF will be saved.
///
/// # Returns
/// An empty `Result` on success, or an error `String` on failure.
fn generate_pdf_for_task(
    template_id: &str,
    placeholders: &HashMap<String, String>,
    output_path: &Path,
) -> Result<(), String> {
    let conn = Connection::open("templify.sqlite").map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT text FROM templates WHERE id = ?1")
        .map_err(|e| e.to_string())?;
    let template_text: String = stmt
        .query_row([template_id], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    // --- Placeholder Substitution ---
    // This regex is designed to find placeholders created by the frontend, which have the format
    // `[ph:COLUMN_NAME:BASE64_ENCODED_DEFAULT_VALUE]`. We only care about the COLUMN_NAME.
    let re = Regex::new(r"\[ph:([^:]+):[^]]+]").map_err(|e| e.to_string())?;

    // Replace each found placeholder using a closure.
    let substituted_text = re.replace_all(&template_text, |caps: &regex::Captures| {
        // Get the column title captured by the regex (e.g., "name" or "email").
        let column_title = &caps[1];
        // Look up the corresponding value in the current row's data.
        // If not found, replace with an empty string.
        placeholders.get(column_title).cloned().unwrap_or_default()
    });

    // --- PDF Generation using helpers from the `pdf` service ---
    let images_map = pdf::load_images(&conn, template_id).map_err(|e| e.to_string())?;
    let mut doc = pdf::configure_document().map_err(|e| e.to_string())?;
    let mut temp_files = Vec::new(); // To manage the lifetime of temporary image files.

    // Process the substituted template text line by line, using the same logic as the PDF preview.
    for line in substituted_text.lines() {
        if line.starts_with("[img:") && line.ends_with(']') {
            pdf::handle_image_line(line, &images_map, &mut temp_files, &mut doc)
                .map_err(|e| e.to_string())?;
        } else if line.starts_with("- ") {
            pdf::handle_list_item(&mut doc, &line[2..]);
        } else {
            // The line is treated as normal text.
            // `[ph:...]` placeholders have already been replaced with their actual values.
            // The `handle_normal_line` function will parse any markdown styling (e.g., **bold**).
            pdf::handle_normal_line(line, &mut doc);
        }
    }

    // Ensure the output directory exists and render the PDF.
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut out_file = File::create(output_path).map_err(|e| e.to_string())?;
    doc.render(&mut out_file).map_err(|e| e.to_string())?;

    Ok(())
}

/// Retrieves essential metadata for a template from the database.
///
/// # Arguments
/// * `conn` - An open database connection.
/// * `template_id` - The ID of the template to query.
///
/// # Returns
/// A `Result` containing a tuple of (`id`, `datasource_md5`, `verified` status) on success.
fn get_template_metadata(
    conn: &Connection,
    template_id: &str,
) -> Result<(String, Option<String>, i32), rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT id, datasource_md5, verified FROM templates WHERE id = ?1")?;

    stmt.query_row(params![template_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, i32>(2)?, // `verified` is stored as an INTEGER (0 or 1).
        ))
    })
}

/// Efficiently counts the number of lines in a file, excluding the header.
///
/// This function reads the file in large chunks to minimize I/O operations and quickly
/// counts newline characters (`\n`). It's designed to provide a total count for
/// progress reporting without loading the entire file into memory.
///
/// # Arguments
/// * `file_path` - The path to the file to be read.
///
/// # Returns
/// An `io::Result` containing the number of lines minus one (for the header), or 0 if empty.
fn count_lines_raw(file_path: &str) -> io::Result<usize> {
    const BUFFER_SIZE: usize = 1024 * 64; // 64KB buffer.
    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = [0; BUFFER_SIZE];
    let mut count = 0;
    let mut last_byte_was_newline = true;

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        count += buffer[..bytes_read].iter().filter(|&&b| b == b'\n').count();
        last_byte_was_newline = buffer[bytes_read - 1] == b'\n';
    }

    // If the file doesn't end with a newline, the last line wasn't counted.
    if !last_byte_was_newline && count > 0 {
        count += 1;
    }

    // Subtract 1 for the header row.
    if count > 0 {
        Ok(count - 1)
    } else {
        Ok(0)
    }
}
