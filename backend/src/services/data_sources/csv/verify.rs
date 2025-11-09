use crate::job_controller::state::{JobStatus, JobUpdate, JobsState};
use actix_web::{web, HttpResponse, Responder};
use common::model::variable::VariableType;
use rayon::prelude::*;
use serde::Deserialize;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    time::Instant,
};
use tokio::sync::mpsc;

#[derive(Deserialize, Clone)]
pub struct ColumnCheck {
    pub column_index: usize,
    pub var_type: VariableType,
}

#[derive(Deserialize)]
pub struct VerifyCsvRequest {
    pub uuid: String,
    pub columns: Vec<ColumnCheck>,
}

pub(crate) async fn process(
    jobs_state: web::Data<JobsState>,
    req: web::Json<VerifyCsvRequest>,
) -> impl Responder {
    match schedule_verify_job(jobs_state, req.into_inner()).await {
        Ok(job_id) => HttpResponse::Ok().body(job_id),
        Err(err) => HttpResponse::InternalServerError().body(err),
    }
}

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
    tokio::spawn(async move {
        if let Err(e) = verify_csv_data(tx, value.clone(), req).await {
            jobs_state
                .jobs
                .write()
                .await
                .insert(value, JobStatus::Failed(e));
        }
    });
    Ok(job_id)
}

fn validate_value(var_type: &VariableType, value: &str) -> bool {
    match var_type {
        VariableType::Text => true,
        VariableType::Number | VariableType::Currency => value.parse::<f64>().is_ok(),
        VariableType::Email => value.contains('@') && value.contains('.'),
    }
}

fn find_first_invalid(
    chunk: &[(usize, String)],
    columns: &[ColumnCheck],
) -> Option<(usize, usize)> {
    chunk.par_iter().find_map_any(|(idx, line)| {
        let record: Vec<_> = line.split(';').collect();
        for col in columns {
            if col.column_index >= record.len()
                || !validate_value(&col.var_type, record[col.column_index])
            {
                return Some((idx + 1, col.column_index));
            }
        }
        None
    })
}

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
            if process_chunk(&tx, &job_id, &chunk, &req.columns, start).await? {
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
        if process_chunk(&tx, &job_id, &chunk, &req.columns, start).await? {
            return Ok(());
        }
    }
    tx.send(JobUpdate {
        job_id: job_id.clone(),
        status: JobStatus::Completed("Verification successful".to_string()),
    })
        .await
        .map_err(|e| e.to_string())?;
    println!("verify_csv_data finished in: {:.2?}", start.elapsed());
    Ok(())
}

async fn process_chunk(
    tx: &mpsc::Sender<JobUpdate>,
    job_id: &str,
    chunk: &[(usize, String)],
    columns: &[ColumnCheck],
    start: Instant,
) -> Result<bool, String> {
    if let Some((row, col)) = find_first_invalid(chunk, columns) {
        handle_first_invalid(tx, job_id, row, col, start).await?;
        return Ok(true);
    }
    Ok(false)
}

async fn handle_first_invalid(
    tx: &mpsc::Sender<JobUpdate>,
    job_id: &str,
    row: usize,
    col: usize,
    start: Instant,
) -> Result<(), String> {
    tx.send(JobUpdate {
        job_id: job_id.to_string(),
        status: JobStatus::Failed(format!("First invalid row at: row {}, column {}", row, col)),
    })
        .await
        .map_err(|e| e.to_string())?;
    println!("verify_csv_data finished in: {:.2?}", start.elapsed());
    Ok(())
}
