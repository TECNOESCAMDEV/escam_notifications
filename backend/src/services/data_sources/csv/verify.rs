use crate::job_controller::state::{JobStatus, JobUpdate, JobsState};
use actix_web::{web, HttpResponse, Responder};
use common::model::pleaceholder::PlaceholderType;
use rayon::prelude::*;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    time::Instant,
};
use tokio::sync::mpsc;

#[derive(Deserialize, Serialize, Clone)]
pub struct ColumnCheck {
    pub title: String,
    pub placeholder_type: PlaceholderType,
}

#[derive(Deserialize)]
pub struct VerifyCsvRequest {
    pub uuid: String,
}

/// Validate a single cell value against a placeholder type.
fn validate_value(var_type: &PlaceholderType, value: &str) -> bool {
    match var_type {
        PlaceholderType::Text => true,
        PlaceholderType::Number | PlaceholderType::Currency => value.parse::<f64>().is_ok(),
        PlaceholderType::Email => value.contains('@') && value.contains('.'),
    }
}

/// Find the first invalid row inside a chunk using parallel iteration.
/// Returns Some((row_index, column_title)) when invalid found.
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
                    return Some((idx + 2, col.title.clone())); // +2: header + second line offset
                }
            } else {
                return Some((idx + 2, col.title.clone())); // title missing
            }
        }
        None
    })
}

/// Trim and normalize a CSV cell (remove outer quotes and NBSP).
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

/// Infer placeholder types from a sample data line and titles.
/// Uses heuristics: email if contains '@' and '.', currency if contains currency symbols,
/// number if parseable as f64, otherwise text.
fn infer_column_checks(titles: &[String], second_line: &str, delimiter: char) -> Vec<ColumnCheck> {
    let cells: Vec<String> = second_line
        .split(delimiter)
        .map(|c| normalize_cell(c))
        .collect();

    let currency_symbols = ['$', '€', '£', '¥'];
    let mut columns = Vec::with_capacity(titles.len());

    for (idx, title) in titles.iter().enumerate() {
        let placeholder_type = if idx < cells.len() {
            let val = cells[idx].trim();
            if val.contains('@') && val.contains('.') {
                PlaceholderType::Email
            } else if val.chars().any(|ch| currency_symbols.contains(&ch)) {
                PlaceholderType::Currency
            } else if val.parse::<f64>().is_ok() {
                PlaceholderType::Number
            } else {
                PlaceholderType::Text
            }
        } else {
            PlaceholderType::Text
        };

        columns.push(ColumnCheck {
            title: title.clone(),
            placeholder_type,
        });
    }

    columns
}

/// Update templates table after verification attempt.
/// If success is true: set verified=1 and last_verified_md5 = datasource_md5.
/// If success is false: set verified=1 and overwrite datasource_md5 with last_verified_md5.
fn update_template_verification(
    conn: &Connection,
    id: &str,
    datasource_md5: &str,
    last_verified_md5: &str,
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

/// Send a failure job update with the first invalid row details.
fn handle_first_invalid_sync(
    tx: &mpsc::Sender<JobUpdate>,
    job_id: &str,
    row: usize,
    title: &str,
    start: Instant,
) -> Result<(), String> {
    let _ = tx.blocking_send(JobUpdate {
        job_id: job_id.to_string(),
        status: JobStatus::Failed(format!(
            "First invalid row at: row {}, column '{}'",
            row, title
        )),
    });
    println!("verify_csv_data finished in: {:.2?}", start.elapsed());
    Ok(())
}

/// Process a single chunk synchronously; returns Ok(true) if an invalid was found and handled.
fn process_chunk_sync(
    tx: &mpsc::Sender<JobUpdate>,
    job_id: &str,
    chunk: &[(usize, String)],
    columns: &[ColumnCheck],
    title_to_index: &HashMap<String, usize>,
    delimiter: char,
    start: Instant,
) -> Result<bool, String> {
    if let Some((row, title)) = find_first_invalid(chunk, columns, title_to_index, delimiter) {
        handle_first_invalid_sync(tx, job_id, row, &title, start)?;
        return Ok(true);
    }
    Ok(false)
}

/// Read header and second line from reader; return trimmed lines.
fn read_header_and_second_line(reader: &mut BufReader<File>) -> Result<(String, String), String> {
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
        return Err("CSV file has no data rows".to_string());
    }
    let second_line = second_line.trim_end_matches(&['\n', '\r'][..]).to_string();

    Ok((header_line, second_line))
}

/// Detect delimiter by choosing the character with the most occurrences in the header.
fn detect_delimiter(header_line: &str) -> char {
    [',', ';', '\t', '|']
        .iter()
        .max_by_key(|&&d| header_line.matches(d).count())
        .copied()
        .unwrap_or(',')
}

/// Main blocking verification function executed inside spawn_blocking.
/// Returns Ok(json_columns) on success where json_columns is a JSON array of ColumnCheck.
fn verify_csv_data_blocking(
    tx: mpsc::Sender<JobUpdate>,
    job_id: String,
    template_id: String,
) -> Result<String, String> {
    let start = Instant::now();

    // Open DB and fetch template row
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
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i32>(3)?,
            ))
        })
        .map_err(|_| "Template not found".to_string())?;

    let (id, datasource_md5, last_verified_md5, verified) = template;
    if verified != 0 {
        return Err("Template already verified".to_string());
    }

    // Build file path and open file
    let file_path = format!("./{}_{}.csv", id, datasource_md5);
    if !Path::new(&file_path).exists() {
        return Err("CSV file not found".to_string());
    }
    let file = File::open(&file_path).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);

    // Read header and second line, detect delimiter
    let (header_line, second_line) = read_header_and_second_line(&mut reader)?;
    let delimiter = detect_delimiter(&header_line);

    // Build titles and index map
    let titles: Vec<String> = header_line
        .split(delimiter)
        .map(|s| s.trim().to_string())
        .collect();
    let mut title_to_index = HashMap::new();
    for (i, t) in titles.iter().enumerate() {
        title_to_index.insert(t.clone(), i);
    }

    // Infer column checks from second line
    let columns = infer_column_checks(&titles, &second_line, delimiter);

    // Process file in chunks
    let chunk_size = 250_000;
    let mut chunk = Vec::with_capacity(chunk_size);
    let mut lines_processed = 0usize;

    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        chunk.push((i, line));
        if chunk.len() == chunk_size {
            if process_chunk_sync(
                &tx,
                &job_id,
                &chunk,
                &columns,
                &title_to_index,
                delimiter,
                start,
            )? {
                // Failure: revert datasource_md5 and mark verified
                update_template_verification(
                    &conn,
                    &id,
                    &datasource_md5,
                    &last_verified_md5,
                    false,
                )?;
                return Err("Verification failed: invalid row found".to_string());
            }
            lines_processed += chunk.len();
            chunk.clear();
            let _ = tx.blocking_send(JobUpdate {
                job_id: job_id.clone(),
                status: JobStatus::InProgress(lines_processed as u32),
            });
        }
    }

    if !chunk.is_empty() {
        if process_chunk_sync(
            &tx,
            &job_id,
            &chunk,
            &columns,
            &title_to_index,
            delimiter,
            start,
        )? {
            update_template_verification(&conn, &id, &datasource_md5, &last_verified_md5, false)?;
            return Err("Verification failed: invalid row found".to_string());
        }
    }

    // If we reach here: verification successful
    // Update DB: set verified and last_verified_md5
    update_template_verification(&conn, &id, &datasource_md5, &last_verified_md5, true)?;

    // Serialize inferred columns to JSON to return and to send in JobUpdate
    let json_columns = serde_json::to_string(&columns).map_err(|e| e.to_string())?;

    let _ = tx.blocking_send(JobUpdate {
        job_id: job_id.clone(),
        status: JobStatus::Completed(json_columns.clone()),
    });

    println!("verify_csv_data finished in: {:.2?}", start.elapsed());
    Ok(json_columns)
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
    let js = jobs_state.clone();
    let uuid = req.uuid;

    tokio::spawn(async move {
        // Clone values to move into the blocking task without consuming `value`
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
                    JobStatus::Failed(format!("join error: {}", join_err)),
                );
            }
        }
    });

    Ok(job_id)
}
