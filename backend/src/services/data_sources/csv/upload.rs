// Rust
use actix_multipart::Multipart;
use actix_web::{HttpResponse, Responder};
use common::model::datasource::DataSource;
use common::model::pleaceholder::{PlaceHolder, PlaceholderType};
use futures_util::StreamExt;
use md5::Context;
use regex::Regex;
use rusqlite::{params, Connection};
use serde_json::from_slice;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::str;

type DynError = Box<dyn std::error::Error>;

/// Decide delimiter from header line. Prefer ';' if present, otherwise ','.
fn detect_delimiter(header_line: &str) -> char {
    if header_line.contains(';') {
        ';'
    } else {
        ','
    }
}

/// Trim surrounding quotes and spaces from a CSV cell.
fn normalize_cell(raw: &str) -> String {
    let mut s = raw.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s = &s[1..s.len() - 1];
    }
    s.to_string()
}

/// Validate header cells and ensure uniqueness. Uses given delimiter.
fn validate_header_cells_and_extract(
    header_str: &str,
    header_re: &Regex,
    delimiter: char,
) -> Result<Vec<String>, DynError> {
    let mut titles = Vec::new();
    for cell in header_str.split(delimiter) {
        let field = normalize_cell(cell);
        if field.is_empty() {
            return Err("CSV header cells must not be empty".into());
        }
        if !header_re.is_match(&field) {
            return Err(
                "CSV header cells must contain only text (letters, marks, spaces, '-', '_')".into(),
            );
        }
        titles.push(field);
    }

    // uniqueness check
    let mut seen = HashSet::new();
    for t in &titles {
        if !seen.insert(t.clone()) {
            return Err("CSV header titles must be unique".into());
        }
    }

    Ok(titles)
}

/// Write uploaded file streaming from multipart field, validate header (first line),
/// detect delimiter and return the detected delimiter plus the header titles.
/// Also consumes bytes into md5_hasher.
async fn write_file_with_header_validation(
    mut field: actix_multipart::Field,
    template_id: &str,
    md5_hasher: &mut Context,
) -> Result<(char, Vec<String>), DynError> {
    // Buffer until we have the full header line
    let mut header_buf: Vec<u8> = Vec::new();
    let mut header_validated = false;
    let mut writer_opt: Option<BufWriter<File>> = None;
    let header_re =
        Regex::new(r"^[\p{L}\p{M}\s\-_]+$").map_err(|e| format!("Regex error: {}", e))?;

    while let Some(chunk_res) = field.next().await {
        let chunk = chunk_res?;
        md5_hasher.consume(&chunk);

        if !header_validated {
            header_buf.extend_from_slice(&chunk);
            if let Some(pos) = header_buf.iter().position(|&b| b == b'\n') {
                // extract header line (up to pos)
                let mut header_line = header_buf[..pos].to_vec();
                if header_line.ends_with(&[b'\r']) {
                    header_line.pop();
                }
                let header_str =
                    String::from_utf8(header_line).map_err(|_| "Header is not valid UTF-8")?;
                let delimiter = detect_delimiter(&header_str);
                let _ = validate_header_cells_and_extract(&header_str, &header_re, delimiter)?;

                // create file and write buffered bytes (including remainder of current chunk(s))
                let file = File::create(format!("{}.csv", template_id))?;
                let mut writer = BufWriter::new(file);
                writer.write_all(&header_buf)?;
                writer.flush()?;
                header_buf.clear();
                header_validated = true;
                writer_opt = Some(writer);

                // continue: further iterations will write remaining chunks
            } else {
                // still waiting for header line
                continue;
            }
        } else {
            if let Some(w) = writer_opt.as_mut() {
                w.write_all(&chunk)?;
            }
        }
    }

    // Edge: if we finished stream without encountering newline in header (single-line file)
    if !header_validated {
        // header_buf contains entire file; treat its first line as header
        let mut header_line = header_buf.clone();
        if header_line.ends_with(&[b'\r']) {
            header_line.pop();
        }
        let header_str =
            String::from_utf8(header_line.clone()).map_err(|_| "Header is not valid UTF-8")?;
        let delimiter = detect_delimiter(&header_str);
        let titles = validate_header_cells_and_extract(&header_str, &header_re, delimiter)?;

        // write whole buffer as file
        let file = File::create(format!("{}.csv", template_id))?;
        let mut writer = BufWriter::new(file);
        writer.write_all(&header_buf)?;
        writer.flush()?;
        return Ok((delimiter, titles));
    }

    // flush writer if present
    if let Some(mut w) = writer_opt {
        w.flush()?;
    } else {
        return Err("Internal error: writer missing after header validation".into());
    }

    // Re-open file to read header for returning delimiter and titles
    let sample_path_str = format!("{}.csv", template_id);
    let mut file = File::open(&sample_path_str)?;
    let mut first_line = String::new();
    let mut br = BufReader::new(&mut file);
    br.read_line(&mut first_line)?;
    while first_line.ends_with('\n') || first_line.ends_with('\r') {
        first_line.pop();
    }
    let delimiter = detect_delimiter(&first_line);
    let titles = validate_header_cells_and_extract(&first_line, &header_re, delimiter)?;
    Ok((delimiter, titles))
}

/// Infer placeholder types sampling up to MAX_SAMPLE_LINES using the provided delimiter.
fn infer_placeholders_from_file(
    path: &Path,
    titles: &[String],
    delimiter: char,
) -> Result<Vec<PlaceHolder>, DynError> {
    const MAX_SAMPLE_LINES: usize = 1000;
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let cols_count = titles.len();
    let mut is_number = vec![true; cols_count];
    let mut is_currency = vec![false; cols_count];
    let mut is_email = vec![false; cols_count];
    let currency_symbols = ['$', '€', '£', '¥'];

    let mut sampled = 0usize;
    for (i, line_res) in reader.lines().enumerate() {
        let line = line_res?;
        if i == 0 {
            // skip header
            continue;
        }
        if sampled >= MAX_SAMPLE_LINES {
            break;
        }
        sampled += 1;

        let cells: Vec<String> = line.split(delimiter).map(|c| normalize_cell(c)).collect();

        for col_idx in 0..cols_count {
            if col_idx >= cells.len() {
                is_number[col_idx] = false;
                continue;
            }
            let val = cells[col_idx].trim();
            if val.is_empty() {
                continue;
            }
            if val.contains('@') && val.contains('.') {
                is_email[col_idx] = true;
            }
            if val.chars().any(|ch| currency_symbols.contains(&ch)) {
                is_currency[col_idx] = true;
            }
            if is_number[col_idx] {
                if val.parse::<f64>().is_err() {
                    let cleaned: String = val
                        .chars()
                        .filter(|c| c.is_digit(10) || *c == '.' || *c == '-' || *c == ',')
                        .collect();
                    let cleaned = cleaned.replace(',', "");
                    if cleaned.is_empty() || cleaned.parse::<f64>().is_err() {
                        is_number[col_idx] = false;
                    }
                }
            }
        }
    }

    let mut placeholders = Vec::with_capacity(cols_count);
    for idx in 0..cols_count {
        let ptype = if is_email[idx] {
            PlaceholderType::Email
        } else if is_currency[idx] {
            PlaceholderType::Currency
        } else if is_number[idx] {
            PlaceholderType::Number
        } else {
            PlaceholderType::Text
        };
        placeholders.push(PlaceHolder {
            title: titles[idx].clone(),
            placeholder_type: ptype,
        });
    }
    Ok(placeholders)
}

/// HTTP wrapper that returns 200 with the placeholders JSON string on success.
pub async fn process(payload: Multipart) -> impl Responder {
    match upload_data_source(payload).await {
        Ok(result_json) => HttpResponse::Ok().body(result_json),
        Err(e) => HttpResponse::BadRequest().body(format!("Error: {}", e)),
    }
}

/// Main upload handler. Reads multipart, expects `json` part first (DataSource) and `file` part.
/// Returns JSON string of `Vec<PlaceHolder>` on success.
pub async fn upload_data_source(mut payload: Multipart) -> Result<String, DynError> {
    let mut data_source: Option<DataSource> = None;
    let mut db_md5: Option<String> = None;
    let mut md5_hasher = Context::new();
    let mut file_processed = false;
    let mut detected_delimiter: Option<char> = None;
    let mut header_titles: Option<Vec<String>> = None;

    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_name = field
            .content_disposition()
            .and_then(|cd| cd.get_name().map(|n| n.to_string()));

        match content_name.as_deref() {
            Some("json") => {
                // read json part fully
                let mut bytes = Vec::new();
                while let Some(chunk) = field.next().await {
                    bytes.extend_from_slice(&chunk?);
                }
                let ds: DataSource = from_slice(&bytes)?;
                let conn = Connection::open("templify.sqlite")?;
                let db_md5_result: Result<Option<String>, rusqlite::Error> = conn.query_row(
                    "SELECT datasource_md5 FROM templates WHERE id = ?1",
                    params![ds.template_id],
                    |row| row.get::<_, Option<String>>(0),
                );

                match db_md5_result {
                    Ok(opt_val) => {
                        db_md5 = opt_val;
                        data_source = Some(ds);
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => {
                        return Err("Template not found".into());
                    }
                    Err(e) => return Err(Box::new(e)),
                }
            }

            Some("file") => {
                let filename = field
                    .content_disposition()
                    .and_then(|cd| cd.get_filename().map(|f| f.to_string()))
                    .unwrap_or_default();

                if !filename.ends_with(".csv") {
                    return Err("The file must end with .csv".into());
                }

                let ds = data_source
                    .as_ref()
                    .ok_or("DataSource JSON must be sent before the file")?;
                // process streaming upload, validate header and write file; md5 is updated inside
                let (delimiter, titles) =
                    write_file_with_header_validation(field, &ds.template_id, &mut md5_hasher)
                        .await?;
                detected_delimiter = Some(delimiter);
                header_titles = Some(titles);
                file_processed = true;
            }

            _ => {
                // ignore other parts
            }
        }
    }

    let ds = data_source.ok_or("Missing DataSource")?;
    if !file_processed {
        return Err("Missing file".into());
    }

    let titles = header_titles.ok_or("Header titles missing")?;
    let delimiter = detected_delimiter.ok_or("Delimiter detection failed")?;
    let sample_path_buf = PathBuf::from(format!("{}.csv", ds.template_id));
    if !sample_path_buf.exists() {
        return Err("Written CSV file not found".into());
    }

    // Infer placeholders using the detected delimiter
    let placeholders = infer_placeholders_from_file(&sample_path_buf, &titles, delimiter)?;

    let computed_md5 = format!("{:x}", md5_hasher.finalize());

    // Update DB if md5 changed
    match db_md5.as_ref() {
        Some(existing) if existing == &computed_md5 => {
            let json = serde_json::to_string(&placeholders)?;
            Ok(json)
        }
        _ => {
            let conn = Connection::open("templify.sqlite")?;
            conn.execute(
                "UPDATE templates SET datasource_md5 = ?1 WHERE id = ?2",
                params![computed_md5, ds.template_id],
            )?;
            let json = serde_json::to_string(&placeholders)?;
            Ok(json)
        }
    }
}
