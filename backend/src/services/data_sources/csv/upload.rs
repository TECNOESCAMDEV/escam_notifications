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
use std::io::{BufRead, BufReader, Write};

/// Validate each CSV header cell.
/// - `header_str` is the raw header line (without trailing CR/LF).
/// - `header_re` is the precompiled regex used to validate each cell.
fn validate_header_cells_and_extract(
    header_str: &str,
    header_re: &Regex,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut titles = Vec::new();
    for cell in header_str.split(';') {
        let mut f = cell.trim();
        // remove surrounding quotes if any
        if f.starts_with('"') && f.ends_with('"') && f.len() >= 2 {
            f = &f[1..f.len() - 1];
        }
        if f.is_empty() {
            return Err("CSV header cells must not be empty".into());
        }
        if !header_re.is_match(f) {
            return Err(
                "CSV header cells must contain only text (letras, espacios, '-', '_')".into(),
            );
        }
        titles.push(f.to_string());
    }

    // Uniqueness check
    let mut seen = HashSet::new();
    for t in &titles {
        if !seen.insert(t.clone()) {
            return Err("CSV header titles must be unique".into());
        }
    }

    Ok(titles)
}

/// HTTP handler wrapper that converts internal result to an `HttpResponse`.
///
/// - On success: returns `200 OK` with the JSON string of placeholders as body.
/// - On failure: returns `400 Bad Request` with the error message.
pub async fn process(payload: Multipart) -> impl Responder {
    match upload_data_source(payload).await {
        Ok(result_json) => HttpResponse::Ok().body(result_json),
        Err(e) => HttpResponse::BadRequest().body(format!("Error: {}", e)),
    }
}

/// Uploads a CSV file associated with a `DataSource` JSON part and returns a JSON string
/// representing a `Vec<PlaceHolder>` on success. MD5 calculation and DB update are preserved.
pub async fn upload_data_source(
    mut payload: Multipart,
) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::BufWriter;

    let mut data_source: Option<DataSource> = None;
    let mut db_md5: Option<String> = None;
    let mut md5_hasher = Context::new();
    let mut file_written = false;
    let mut header_titles: Option<Vec<String>> = None;

    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_type = field
            .content_disposition()
            .and_then(|cd| cd.get_name().map(|n| n.to_string()));

        match content_type.as_deref() {
            Some("file") => {
                let filename = field
                    .content_disposition()
                    .and_then(|cd| cd.get_filename().map(|f| f.to_string()))
                    .unwrap_or_default();

                if !filename.ends_with(".csv") {
                    return Err("The file must end with .csv".into());
                }

                if let Some(ref ds) = data_source {
                    let mut header_buf: Vec<u8> = Vec::new();
                    let mut header_validated = false;
                    let mut writer_opt: Option<BufWriter<File>> = None;

                    // Regex to validate header cells: letters, marks, spaces, hyphen, underscore.
                    let header_re = Regex::new(r"^[\p{L}\p{M}\s\-_]+$")
                        .map_err(|e| format!("Regex error: {}", e))?;

                    while let Some(chunk) = field.next().await {
                        let chunk = chunk?;
                        // Update md5 always (we hash the uploaded bytes).
                        md5_hasher.consume(&chunk);

                        if !header_validated {
                            header_buf.extend_from_slice(&chunk);
                            if let Some(pos) = header_buf.iter().position(|&b| b == b'\n') {
                                let mut header_line = header_buf[..pos].to_vec();
                                if header_line.ends_with(&[b'\r']) {
                                    header_line.pop();
                                }
                                let header_str = String::from_utf8(header_line.clone())
                                    .map_err(|_| "Header is not valid UTF-8")?;

                                // Validate and extract titles
                                let titles =
                                    validate_header_cells_and_extract(&header_str, &header_re)?;
                                header_titles = Some(titles);

                                // Header validated -> create file and write buffered bytes (including remaining of chunk).
                                let file = File::create(format!("{}.csv", ds.template_id))?;
                                let mut writer = BufWriter::new(file);
                                writer.write_all(&header_buf)?;
                                header_buf.clear();
                                header_validated = true;
                                writer_opt = Some(writer);
                                file_written = true;
                            } else {
                                continue;
                            }
                        } else {
                            if let Some(w) = writer_opt.as_mut() {
                                w.write_all(&chunk)?;
                            }
                        }
                    }

                    if !header_validated {
                        let mut header_line = header_buf.clone();
                        if header_line.ends_with(&[b'\r']) {
                            header_line.pop();
                        }
                        let header_str = String::from_utf8(header_line)
                            .map_err(|_| "Header is not valid UTF-8")?;

                        // Reuse helper here as well
                        let titles = validate_header_cells_and_extract(&header_str, &header_re)?;
                        header_titles = Some(titles);

                        let file = File::create(format!("{}.csv", ds.template_id))?;
                        let mut writer = BufWriter::new(file);
                        writer.write_all(&header_buf)?;
                        file_written = true;
                    }
                } else {
                    return Err("DataSource JSON must be sent before the file".into());
                }
            }

            Some("json") => {
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

            _ => {}
        }
    }

    let ds = data_source.ok_or("Missing DataSource")?;
    if !file_written {
        return Err("Missing file".into());
    }

    // Infer placeholder types by sampling the written file
    let titles = header_titles.ok_or("Header titles missing")?;
    let sample_path = format!("{}.csv", ds.template_id);
    let file = File::open(&sample_path)?;
    let reader = BufReader::new(file);

    // Skip header line, then sample up to N lines
    const MAX_SAMPLE_LINES: usize = 1000;
    let mut sampled = 0usize;

    // per-column accumulators
    let cols_count = titles.len();
    let mut is_number = vec![true; cols_count];
    let mut is_currency = vec![false; cols_count];
    let mut is_email = vec![false; cols_count];

    let currency_symbols = ['$', '€', '£', '¥'];

    for (i, line_res) in reader.lines().enumerate() {
        let line = line_res?;
        if i == 0 {
            // header
            continue;
        }
        if sampled >= MAX_SAMPLE_LINES {
            break;
        }
        sampled += 1;

        // simple split by comma, trimming quotes/spaces (note: doesn't handle all CSV edge cases)
        let cells: Vec<String> = line
            .split(',')
            .map(|c| {
                let mut s = c.trim();
                if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                    s = &s[1..s.len() - 1];
                }
                s.to_string()
            })
            .collect();

        for col_idx in 0..cols_count {
            if col_idx >= cells.len() {
                // missing cell: treat as non-number for safety
                is_number[col_idx] = false;
                continue;
            }
            let val = cells[col_idx].trim();
            if val.is_empty() {
                // empty cell does not disqualify number, skip
                continue;
            }
            // email check
            if val.contains('@') && val.contains('.') {
                is_email[col_idx] = true;
            }
            // currency check
            if val.chars().any(|ch| currency_symbols.contains(&ch)) {
                is_currency[col_idx] = true;
            }

            // number check
            if is_number[col_idx] {
                if val.parse::<f64>().is_err() {
                    // Try to strip common thousand separators or currency symbols and retry
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

    // Decide final placeholder types
    let mut placeholders: Vec<PlaceHolder> = Vec::with_capacity(cols_count);
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

    let computed_md5 = format!("{:x}", md5_hasher.finalize());

    // Update DB if md5 changed
    match db_md5.as_ref() {
        Some(existing) if existing == &computed_md5 => {
            // return serialized placeholders
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
