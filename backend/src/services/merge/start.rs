use crate::job_controller::state::{JobUpdate, JobsState};
use crate::services::data_sources::csv::verify::{
    detect_delimiter, read_header_and_second_line, validate_and_normalize_titles,
};
use crate::services::templates::pdf;
use actix_web::{web, HttpResponse, Responder};
use common::jobs::JobStatus;
use common::requests::StartMergeRequest;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Represents a status update for a merge job or one of its tasks.
///
/// This enum is used to communicate progress from the synchronous worker thread
/// back to the asynchronous `job_controller`.
#[derive(Debug)]
pub enum MergeUpdate {
    /// Updates the overall status of the merge job.
    Job(JobStatus),
    /// Reports the progress of an individual merge task (generating a single PDF).
    Task { row_index: usize, total_rows: usize },
}

/// The Actix web handler for `POST /api/merge/start`.
///
/// Receives a `StartMergeRequest`, schedules the background merge job,
/// and immediately returns a `job_id` to the client.
///
/// # Arguments
/// * `state` - The shared `JobsState` injected by Actix.
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
    state
        .jobs
        .write()
        .await
        .insert(job_id.clone(), JobStatus::Pending);

    let tx = state.tx.clone();
    let job_id_clone = job_id.clone();
    let template_id = req.template_id;

    tokio::spawn(async move {
        // Create a specific channel for merge updates.
        let (merge_tx, mut merge_rx) = mpsc::channel::<MergeUpdate>(100);

        // Thread to listen for merge updates and translate them into JobUpdates.
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
                        // Calculate the progress percentage.
                        let progress = if total_rows > 0 {
                            ((row_index + 1) as f32 / total_rows as f32 * 100.0) as u32
                        } else {
                            0
                        };
                        JobStatus::InProgress(progress)
                    }
                };

                let _ = job_updater_tx
                    .send(JobUpdate {
                        job_id: job_id_for_updater.clone(),
                        status,
                    })
                    .await;
            }
        });

        // Run the blocking job.
        let job_id_for_blocking = job_id_clone.clone();
        let template_id_for_blocking = template_id.clone();
        let handle = tokio::task::spawn_blocking(move || {
            merge_blocking(merge_tx, &job_id_for_blocking, &template_id_for_blocking)
        });

        match handle.await {
            Ok(Ok(_)) => {
                let _ = tx
                    .send(JobUpdate {
                        job_id: job_id_clone,
                        status: JobStatus::Completed("Merge completed successfully".to_string()),
                    })
                    .await;
            }
            Ok(Err(e)) => {
                let _ = tx
                    .send(JobUpdate {
                        job_id: job_id_clone,
                        status: JobStatus::Failed(e),
                    })
                    .await;
            }
            Err(e) => {
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

/// The main synchronous merge function, designed to be run in `spawn_blocking`.
///
/// This function contains the complete, synchronous logic for the CSV merge, including
/// database interaction, file I/O, and PDF generation. It sends status updates
/// back to the main async context via the provided MPSC sender.
///
/// # Arguments
/// * `tx` - The MPSC sender to communicate job status updates.
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
    let _ = tx.blocking_send(MergeUpdate::Job(JobStatus::InProgress(0)));

    let conn = Connection::open("templify.sqlite").map_err(|e| e.to_string())?;

    let (_id, datasource_md5, verified) =
        get_template_metadata(&conn, template_id).map_err(|e| e.to_string())?;

    if verified != 1 {
        let err_msg = "Template is not verified.".to_string();
        let _ = tx.blocking_send(MergeUpdate::Job(JobStatus::Failed(err_msg.clone())));
        return Err(err_msg);
    }

    let ds_md5 = datasource_md5.ok_or("Datasource MD5 not found for verified template.")?;
    let file_path = format!("./{}_{}.csv", template_id, ds_md5);
    let file = fs::File::open(&file_path).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);

    let (header_line, _) = read_header_and_second_line(&mut reader)?;
    let delimiter = detect_delimiter(&header_line);
    let titles = validate_and_normalize_titles(&header_line, delimiter)?;

    let lines: Vec<String> = reader
        .lines()
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let total_rows = lines.len();

    for (i, line) in lines.iter().enumerate() {
        let mut placeholders = HashMap::new();
        let values: Vec<&str> = line.split(delimiter).collect();
        for (j, title) in titles.iter().enumerate() {
            if let Some(value) = values.get(j) {
                placeholders.insert(title.clone(), value.to_string());
            }
        }

        let output_filename = format!("{}_{}.pdf", job_id, i);
        let output_path = Path::new("./pdfs").join(&output_filename);

        if let Err(e) = generate_pdf_for_task(template_id, &placeholders, &output_path) {
            // If a task fails, we can decide whether to fail the whole job or continue.
            // Here, we fail the entire job.
            let err_msg = format!("Failed to generate PDF for row {}: {}", i + 1, e);
            let _ = tx.blocking_send(MergeUpdate::Job(JobStatus::Failed(err_msg.clone())));
            return Err(err_msg);
        }

        // Send task progress update.
        let _ = tx.blocking_send(MergeUpdate::Task {
            row_index: i,
            total_rows,
        });
    }

    Ok(())
}

/// Generates a single PDF for a merge task.
fn generate_pdf_for_task(
    template_id: &str,
    placeholders: &HashMap<String, String>,
    output_path: &Path,
) -> Result<(), String> {
    let conn = Connection::open("templify.sqlite").map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT text FROM templates WHERE id = ?1")
        .map_err(|e| e.to_string())?;
    let mut template_text: String = stmt
        .query_row([template_id], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    // Substitute placeholders
    for (key, value) in placeholders {
        let ph = format!("{{{{{}}}}}", key);
        template_text = template_text.replace(&ph, value);
    }

    let images_map = pdf::load_images(&conn, template_id).map_err(|e| e.to_string())?;
    let mut doc = pdf::configure_document().map_err(|e| e.to_string())?;
    let mut temp_files = Vec::new();

    for line in template_text.lines() {
        if line.starts_with("[img:") && line.ends_with(']') {
            pdf::handle_image_line(line, &images_map, &mut temp_files, &mut doc)
                .map_err(|e| e.to_string())?;
        } else if line.starts_with("- ") {
            pdf::handle_list_item(&mut doc, &line[2..]);
        } else {
            pdf::handle_normal_line(line, &mut doc);
        }
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut out_file = fs::File::create(output_path).map_err(|e| e.to_string())?;
    doc.render(&mut out_file).map_err(|e| e.to_string())?;

    Ok(())
}

/// Retrieves template metadata from the database.
///
/// # Arguments
/// * `conn` - Database connection
/// * `template_id` - ID of the template
///
/// # Returns
/// A tuple containing (id, datasource_md5, verified status)
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
            row.get::<_, i32>(2)?,
        ))
    })
}
