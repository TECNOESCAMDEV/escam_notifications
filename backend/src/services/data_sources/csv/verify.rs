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

/// Validate a single cell value against a `PlaceholderType`.
///
/// Returns `true` if the `value` conforms to the provided `var_type` heuristic, `false` otherwise.
fn validate_value(var_type: &PlaceholderType, value: &str) -> bool {
    match var_type {
        PlaceholderType::Text => true,
        PlaceholderType::Number | PlaceholderType::Currency => value.parse::<f64>().is_ok(),
        PlaceholderType::Email => value.contains('@') && value.contains('.'),
    }
}

/// Search a chunk of lines for the first invalid row using parallel iteration.
///
/// Returns `Some((row_index, column_title))` if an invalid cell is found.
/// `row_index` is the 1-based CSV row index including header (+2 offset used elsewhere).
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
                        "columna ausente en la fila".to_string(),
                    ));
                }
                let cell = normalize_cell(record[col_idx]);
                if !validate_value(&col.placeholder_type, &cell) {
                    let tipo = match col.placeholder_type {
                        PlaceholderType::Text => "texto",
                        PlaceholderType::Number => "número",
                        PlaceholderType::Currency => "moneda",
                        PlaceholderType::Email => "email",
                    };
                    return Some((
                        idx + 2,
                        col.title.clone(),
                        format!("valor '{}' no cumple el tipo esperado: {}", cell, tipo),
                    ));
                }
            } else {
                return Some((
                    idx + 2,
                    col.title.clone(),
                    "título de cabecera no encontrado".to_string(),
                ));
            }
        }
        None
    })
}

/// Trim and normalize a CSV cell.
///
/// Removes outer single or double quotes if present, replaces NBSP with a space,
/// and trims surrounding whitespace. Returns the normalized string.
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

/// Validate header titles and normalize them.
///
/// Behavior:
/// - Splits `header_line` by `delimiter` and normalizes each title via `normalize_cell`.
/// - Ensures no empty titles and that titles are not purely numeric.
/// - Collapses any runs of whitespace into a single underscore (`_`) to produce normalized titles.
/// - Ensures normalized titles are unique.
///
/// Returns a `Vec<String>` with normalized titles on success, or `Err(String)` with an error message.
fn validate_and_normalize_titles(
    header_line: &str,
    delimiter: char,
) -> Result<Vec<String>, String> {
    let raw_titles: Vec<String> = header_line
        .split(delimiter)
        .map(|s| normalize_cell(s))
        .collect();

    if raw_titles.is_empty() {
        return Err("La línea de cabecera no contiene títulos".to_string());
    }

    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(raw_titles.len());

    for t in raw_titles {
        let t_trim = t.trim();
        if t_trim.is_empty() {
            return Err("La cabecera contiene un título vacío".to_string());
        }

        // Reject purely numeric titles
        if t_trim.parse::<f64>().is_ok() {
            return Err(format!(
                "Los títulos de la cabecera deben ser textuales, se encontró el título numérico: '{}'",
                t_trim
            ));
        }

        // Normalize spaces: collapse runs of whitespace into a single underscore
        let norm = t_trim.split_whitespace().collect::<Vec<_>>().join("_");

        if seen.contains(&norm) {
            return Err(format!(
                "Título duplicado en la cabecera tras normalizar: '{}'",
                norm
            ));
        }
        seen.insert(norm.clone());
        normalized.push(norm);
    }

    Ok(normalized)
}

/// Infer placeholder types for each column using the sample `second_line`.
///
/// Heuristics:
/// - Email if the value contains `@` and `.`
/// - Currency if it contains common currency symbols
/// - Number if parsable as `f64`
/// - Otherwise `Text`
///
/// Returns a vector of `ColumnCheck` aligned with `titles`.
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

/// Update the `templates` table after a verification attempt.
///
/// Behavior:
/// - If `success` is `true`: set `verified = 1` and `last_verified_md5 = datasource_md5`.
/// - If `success` is `false`: set `verified = 1` and restore `datasource_md5 = last_verified_md5`.
///
/// Returns `Ok(())` on success or `Err(String)` with the DB error message.
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

/// Send a `JobUpdate::Failed` with the first invalid row details.
///
/// This helper formats an informative failure message and sends it over `tx`
/// using the blocking send API. It also logs timing information.
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
            "Primera fila inválida en: fila {}, columna '{}': {}",
            row, title, reason
        )),
    });
    println!("verify_csv_data finished in: {:.2?}", start.elapsed());
    Ok(())
}

/// Process a single chunk synchronously.
///
/// Returns `Ok(true)` if an invalid row was found and handled (a `JobUpdate::Failed` has been sent),
/// or `Ok(false)` if the chunk passed validation. Errors are returned as `Err(String)`.
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

/// Read the header line and the first data line from a CSV reader.
///
/// Returns a tuple `(header_line, second_line)` with trailing newlines removed,
/// or `Err(String)` if reading fails or no data rows are present.
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
        return Err("El archivo CSV no contiene filas de datos".to_string());
    }
    let second_line = second_line.trim_end_matches(&['\n', '\r'][..]).to_string();

    Ok((header_line, second_line))
}

/// Detect the CSV delimiter by selecting the character with the most occurrences in the header.
///
/// Candidate delimiters: comma, semicolon, tab, pipe. Defaults to comma if none found.
fn detect_delimiter(header_line: &str) -> char {
    [',', ';', '\t', '|']
        .iter()
        .max_by_key(|&&d| header_line.matches(d).count())
        .copied()
        .unwrap_or(',')
}

/// Main blocking verification function executed inside `spawn_blocking`.
///
/// Workflow summary:
/// - Load template record from DB and ensure it is not already verified.
/// - If the template is already verified **and** `datasource_md5 == last_verified_md5`,
///   short-circuit and return inferred `ColumnCheck` from the first data line.
/// - Otherwise proceed with the full verification flow (read file, validate headers, scan lines).
///
/// Returns `Ok(String)` with JSON-serialized `ColumnCheck` vector on success, or `Err(String)` on failure.
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
        .map_err(|e| {
            "Fallo al obtener la plantilla de la base de datos: ".to_string() + &e.to_string()
        })?;

    let (id, datasource_md5, last_verified_md5, verified) = template;

    // Fast-path: require both MD5 presentes y coincidentes, y verified == 1
    if let (Some(ds_md5), Some(last_md5)) =
        (datasource_md5.as_deref(), last_verified_md5.as_deref())
    {
        if ds_md5 == last_md5 && verified == 1 {
            // Build file path and open file
            let file_path = format!("./{}_{}.csv", id, ds_md5);
            if !Path::new(&file_path).exists() {
                return Err("Archivo CSV no encontrado".to_string());
            }
            let file = File::open(&file_path).map_err(|e| e.to_string())?;
            let mut reader = BufReader::new(file);

            // Read header and second line, detect delimiter
            let (header_line, second_line) = read_header_and_second_line(&mut reader)?;
            let delimiter = detect_delimiter(&header_line);

            // Validate and normalize titles from header
            let titles = validate_and_normalize_titles(&header_line, delimiter)
                .map_err(|e| format!("Validación de cabecera fallida: {}", e))?;

            // Infer column checks and return JSON without scanning the whole file or updating DB
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

    // If template has verified flag set (possible bug / partial verification), reset it to 0 and continue.
    if verified != 0 {
        conn.execute(
            "UPDATE templates SET verified = 0 WHERE id = ?1",
            params![id],
        )
            .map_err(|e| format!("Fallo al restablecer el flag verified: {}", e))?;
        println!(
            "La plantilla '{}' tenía verified != 0; restableciendo verified = 0 y continuando verificación",
            id
        );
    }

    // Build file path and open file: datasource_md5 must be present to locate the file
    let ds_md5 = match datasource_md5.as_deref() {
        Some(s) => s,
        None => {
            // No datasource_md5: nothing que verificar -> revert to last (if any) then error
            update_template_verification(
                &conn,
                &id,
                datasource_md5.as_deref(),
                last_verified_md5.as_deref(),
                false,
            )
                .map_err(|db_err| format!("Datasource MD5 ausente; fallo al revertir: {}", db_err))?;
            return Err("Sin archivos de datos asociados para verificar".to_string());
        }
    };

    let file_path = format!("./{}_{}.csv", id, ds_md5);
    if !Path::new(&file_path).exists() {
        return Err("Archivo CSV no encontrado".to_string());
    }
    let file = File::open(&file_path).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);

    // Read header and second line, detect delimiter
    let (header_line, second_line) = read_header_and_second_line(&mut reader)?;
    let delimiter = detect_delimiter(&header_line);

    // Validate and normalize titles from header (ensures uniqueness and textual titles)
    // If validation fails, revert datasource_md5 via update_template_verification(..., false)
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
                        "Validación de cabecera fallida: {}; fallo al revertir: {}",
                        e, db_err
                    )
                })?;
            return Err(format!("Validación de cabecera fallida: {}", e));
        }
    };

    let mut title_to_index = HashMap::new();
    for (i, t) in titles.iter().enumerate() {
        title_to_index.insert(t.clone(), i);
    }

    // Infer column checks from second line using normalized header titles
    let columns = infer_column_checks(&titles, &second_line, delimiter);

    // Process file in chunks
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

    // If we reach here: verification successful
    // Update DB: set verified and last_verified_md5 (allow NULL)
    update_template_verification(
        &conn,
        &id,
        datasource_md5.as_deref(),
        last_verified_md5.as_deref(),
        true,
    )?;

    // Serialize inferred columns to JSON to return and to send in JobUpdate
    let json_columns = serde_json::to_string(&columns).map_err(|e| e.to_string())?;

    let _ = tx.blocking_send(JobUpdate {
        job_id: job_id.clone(),
        status: JobStatus::Completed(json_columns.clone()),
    });

    println!("verify_csv_data finished in: {:.2?}", start.elapsed());
    Ok(json_columns)
}

/// HTTP handler that enqueues a verification job.
///
/// Receives a JSON `VerifyCsvRequest`, schedules a background task and returns the generated job id.
pub(crate) async fn process(
    jobs_state: web::Data<JobsState>,
    req: web::Json<VerifyCsvRequest>,
) -> impl Responder {
    match schedule_verify_job(jobs_state, req.into_inner()).await {
        Ok(job_id) => HttpResponse::Ok().body(job_id),
        Err(err) => HttpResponse::InternalServerError().body(err),
    }
}

/// Schedule a CSV verification job in the background.
///
/// Inserts a `Pending` job into `jobs_state`, spawns a task that runs
/// `verify_csv_data_blocking` in a blocking thread and updates `jobs_state` on completion.
///
/// Returns the generated job id on success.
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
                    JobStatus::Failed(format!("error al esperar la tarea: {}", join_err)),
                );
            }
        }
    });

    Ok(job_id)
}

/// Processes a chunk of CSV lines and performs DB rollback if a validation failure occurs.
///
/// Behavior:
/// - Calls `process_chunk_sync` to search for the first invalid row inside `chunk`.
/// - If an invalid row is found:
///   - `process_chunk_sync` will send a `JobUpdate::Failed` via `tx` with row and column details.
///   - This function will call `update_template_verification(conn, id, datasource_md5, last_verified_md5, false)`
///     to revert `datasource_md5` and mark the template as verified (rollback).
///   - Returns `Err(String)` describing the failure (either validation or DB rollback error).
/// - If no invalid rows are found, returns `Ok(())`.
///
/// Parameters:
/// - `tx`: sender for job updates (`JobUpdate`).
/// - `job_id`: identifier of the running job (for messages).
/// - `chunk`: slice of `(line_index, line_text)` to validate.
/// - `columns`: inferred column specifications (`ColumnCheck`).
/// - `title_to_index`: map from normalized title to column index.
/// - `delimiter`: CSV delimiter character.
/// - `start`: Instant used for logging in called helpers.
/// - `conn`: database connection used to perform the rollback when needed.
/// - `id`: template id in `templates` table.
/// - `datasource_md5`: current datasource MD5 (to be reverted).
/// - `last_verified_md5`: previously verified MD5 (restored on rollback).
///
/// Returns:
/// - `Ok(())` if the chunk processed without errors.
/// - `Err(String)` if an invalid row was detected or if the rollback operation failed.
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
        // Notify to the job controller about the first invalid row
        handle_first_invalid_sync(tx, job_id, row, &title, &reason, start)?;
        // Rollback the template verification state in the database
        update_template_verification(conn, id, datasource_md5, last_verified_md5, false)?;
        return Err(format!(
            "Verificación fallida: fila {}, columna '{}': {}",
            row, title, reason
        ));
    }
    Ok(())
}
