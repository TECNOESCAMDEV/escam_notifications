use crate::job_controller::state::{JobStatus, JobUpdate, JobsState};
use actix_web::{web, HttpResponse, Responder};
use common::model::variable::VariableType;
use rayon::prelude::*;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;
use tokio::sync::mpsc;

/// Structure to specify which columns and types to check
#[derive(Deserialize, Clone)]
pub struct ColumnCheck {
    pub column_index: usize,
    pub var_type: VariableType,
}

/// Input structure for CSV verification (multiple columns)
#[derive(Deserialize)]
pub struct VerifyCsvRequest {
    pub uuid: String,
    pub columns: Vec<ColumnCheck>,
}

/// POST endpoint to start CSV verification
pub(crate) async fn process(
    jobs_state: web::Data<JobsState>,
    req: web::Json<VerifyCsvRequest>,
) -> impl Responder {
    match schedule_verify_job(jobs_state, req.into_inner()).await {
        Ok(job_id) => HttpResponse::Ok().body(job_id),
        Err(err) => HttpResponse::InternalServerError().body(err),
    }
}

/// Schedules a verification job
async fn schedule_verify_job(
    jobs_state: web::Data<JobsState>,
    req: VerifyCsvRequest,
) -> Result<String, String> {
    let job_id = uuid::Uuid::new_v4().to_string();

    {
        let mut jobs = jobs_state.jobs.write().await;
        jobs.insert(job_id.clone(), JobStatus::Pending);
    }

    let tx = jobs_state.tx.clone();
    let value = job_id.clone();

    tokio::spawn(async move {
        if let Err(e) = verify_csv_data(tx, value.clone(), req).await {
            let mut jobs = jobs_state.jobs.write().await;
            jobs.insert(value.clone(), JobStatus::Failed(e));
        }
    });

    Ok(job_id)
}

/// Validates all specified columns in a row
fn validate_row(columns: &[ColumnCheck], record: &csv::StringRecord) -> Option<usize> {
    for col in columns {
        if col.column_index >= record.len()
            || !validate_value(&col.var_type, &record[col.column_index])
        {
            return Some(col.column_index);
        }
    }
    None
}

/// Processes a chunk and returns the first invalid row and column index, if any
fn find_first_invalid_in_chunk(
    chunk: &[(usize, String)],
    columns: &[ColumnCheck],
) -> Option<(usize, usize)> {
    chunk.par_iter().find_map_any(|(idx, line)| {
        let record = csv::StringRecord::from(line.split(';').collect::<Vec<_>>());
        if let Some(invalid_col) = validate_row(columns, &record) {
            Some((idx + 1, invalid_col))
        } else {
            None
        }
    })
}

/// Verifies the CSV file data and fails at the first invalid line and column.
/// Measures and prints the execution time.
async fn verify_csv_data(
    tx: mpsc::Sender<JobUpdate>,
    job_id: String,
    req: VerifyCsvRequest,
) -> Result<(), String> {
    let start = Instant::now();

    let file_path = format!("./{}.csv", req.uuid);
    if !Path::new(&file_path).exists() {
        return Err("CSV file not found".to_string());
    }

    let file = File::open(&file_path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);

    let chunk_size = 250_000;
    let mut chunk = Vec::with_capacity(chunk_size);
    let mut lines_processed = 0usize;

    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        chunk.push((i, line));
        if chunk.len() == chunk_size {
            if process_chunk_and_handle_invalid(&chunk, &req.columns, &tx, &job_id, start).await? {
                return Ok(());
            }
            lines_processed += chunk.len();
            chunk.clear();

            tx.send(JobUpdate {
                job_id: job_id.clone(),
                status: JobStatus::InProgress(lines_processed as u32),
            })
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    if !chunk.is_empty() {
        if process_chunk_and_handle_invalid(&chunk, &req.columns, &tx, &job_id, start).await? {
            return Ok(());
        }
    }

    tx.send(JobUpdate {
        job_id: job_id.clone(),
        status: JobStatus::Completed("Verification successful".to_string()),
    })
        .await
        .map_err(|e| e.to_string())?;

    let duration = start.elapsed();
    println!("verify_csv_data finished in: {:.2?}", duration);

    Ok(())
}

/// Validates a value according to the VariableType
fn validate_value(var_type: &VariableType, value: &str) -> bool {
    match var_type {
        VariableType::Text => true,
        VariableType::Number => value.parse::<f64>().is_ok(),
        VariableType::Currency => value.parse::<f64>().is_ok(),
        VariableType::Email => value.contains('@') && value.contains('.'),
    }
}

/// Reports the first invalid row and column
async fn handle_invalid_row(
    tx: &mpsc::Sender<JobUpdate>,
    job_id: &str,
    invalid_row_number: usize,
    invalid_col: usize,
    start: Instant,
) -> Result<(), String> {
    let msg = format!(
        "First invalid row at: row {}, column {}",
        invalid_row_number, invalid_col
    );
    tx.send(JobUpdate {
        job_id: job_id.to_string(),
        status: JobStatus::Failed(msg),
    })
        .await
        .map_err(|e| e.to_string())?;
    let duration = start.elapsed();
    println!("verify_csv_data finished in: {:.2?}", duration);
    Ok(())
}

/// Processes a chunk and handles the first invalid row if found
async fn process_chunk_and_handle_invalid(
    chunk: &[(usize, String)],
    columns: &[ColumnCheck],
    tx: &mpsc::Sender<JobUpdate>,
    job_id: &str,
    start: Instant,
) -> Result<bool, String> {
    if let Some((invalid_row_number, invalid_col)) = find_first_invalid_in_chunk(chunk, columns) {
        handle_invalid_row(tx, job_id, invalid_row_number, invalid_col, start).await?;
        return Ok(true);
    }
    Ok(false)
}
