use crate::job_controller::state::{JobStatus, JobUpdate, JobsState};
use actix_web::{web, HttpResponse, Responder};
use common::model::pleaceholder::PlaceholderType;
use rayon::prelude::*;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    time::Instant,
};
use tokio::sync::mpsc;

#[derive(Deserialize, Clone)]
pub struct ColumnCheck {
    pub title: String,
    pub placeholder_type: PlaceholderType,
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

fn validate_value(var_type: &PlaceholderType, value: &str) -> bool {
    match var_type {
        PlaceholderType::Text => true,
        PlaceholderType::Number | PlaceholderType::Currency => value.parse::<f64>().is_ok(),
        PlaceholderType::Email => value.contains('@') && value.contains('.'),
    }
}

fn find_first_invalid(
    chunk: &[(usize, String)],
    columns: &[ColumnCheck],
    title_to_index: &HashMap<String, usize>,
    delimiter: char,
) -> Option<(usize, String)> {
    chunk.par_iter().find_map_any(|(idx, line)| {
        let record: Vec<_> = line.split(delimiter).collect();
        for col in columns {
            if let Some(&col_idx) = title_to_index.get(&col.title) {
                if col_idx >= record.len()
                    || !validate_value(&col.placeholder_type, record[col_idx])
                {
                    return Some((idx + 2, col.title.clone())); // +2 porque idx empieza en 0 y saltamos header
                }
            } else {
                return Some((idx + 2, col.title.clone())); // título no encontrado
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
    let mut reader = BufReader::new(file);

    // Detect delimiter from header
    let mut header_line = String::new();
    reader
        .read_line(&mut header_line)
        .map_err(|e| e.to_string())?;
    let header_line = header_line.trim_end_matches(&['\n', '\r'][..]);
    let delimiter = [',', ';', '\t', '|']
        .iter()
        .max_by_key(|&&d| header_line.matches(d).count())
        .copied()
        .unwrap_or(',');

    let titles: Vec<&str> = header_line.split(delimiter).collect();
    let mut title_to_index = HashMap::new();
    for (i, t) in titles.iter().enumerate() {
        title_to_index.insert(t.trim().to_string(), i);
    }

    let chunk_size = 250_000;
    let mut chunk = Vec::with_capacity(chunk_size);
    let mut lines_processed = 0usize;

    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        chunk.push((i, line));
        if chunk.len() == chunk_size {
            if process_chunk(
                &tx,
                &job_id,
                &chunk,
                &req.columns,
                &title_to_index,
                delimiter,
                start,
            )
                .await?
            {
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
        if process_chunk(
            &tx,
            &job_id,
            &chunk,
            &req.columns,
            &title_to_index,
            delimiter,
            start,
        )
            .await?
        {
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
    title_to_index: &HashMap<String, usize>,
    delimiter: char,
    start: Instant,
) -> Result<bool, String> {
    if let Some((row, title)) = find_first_invalid(chunk, columns, title_to_index, delimiter) {
        handle_first_invalid(tx, job_id, row, &title, start).await?;
        return Ok(true);
    }
    Ok(false)
}

async fn handle_first_invalid(
    tx: &mpsc::Sender<JobUpdate>,
    job_id: &str,
    row: usize,
    title: &str,
    start: Instant,
) -> Result<(), String> {
    tx.send(JobUpdate {
        job_id: job_id.to_string(),
        status: JobStatus::Failed(format!(
            "First invalid row at: row {}, column '{}'",
            row, title
        )),
    })
        .await
        .map_err(|e| e.to_string())?;
    println!("verify_csv_data finished in: {:.2?}", start.elapsed());
    Ok(())
}
