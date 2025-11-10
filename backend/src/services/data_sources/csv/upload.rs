// Rust
use actix_multipart::Multipart;
use actix_web::{HttpResponse, Responder};
use common::model::datasource::DataSource;
use futures_util::StreamExt;
use md5::Context;
use regex::Regex;
use rusqlite::{params, Connection};
use serde_json::from_slice;
use std::fs::File;
use std::io::Write;

/// Validate each CSV header cell.
/// - `header_str` is the raw header line (without trailing CR/LF).
/// - `header_re` is the precompiled regex used to validate each cell.
fn validate_header_cells(
    header_str: &str,
    header_re: &Regex,
) -> Result<(), Box<dyn std::error::Error>> {
    // Iterate cells split by comma and apply the same trimming/quote logic as before.
    for cell in header_str.split(',') {
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
    }
    Ok(())
}

/// HTTP handler wrapper that converts internal result to an `HttpResponse`.
///
/// - On success: returns `200 OK` with the boolean result as body.
/// - On failure: returns `400 Bad Request` with the error message.
pub async fn process(payload: Multipart) -> impl Responder {
    match upload_data_source(payload).await {
        Ok(result) => HttpResponse::Ok().body(result.to_string()),
        Err(e) => HttpResponse::BadRequest().body(format!("Error: {}", e)),
    }
}

/// Uploads a CSV file associated with a `DataSource` JSON part and synchronizes the `datasource_md5`
/// field in the local `templify.sqlite` database.
pub async fn upload_data_source(
    mut payload: Multipart,
) -> Result<bool, Box<dyn std::error::Error>> {
    use std::io::BufWriter;

    let mut data_source: Option<DataSource> = None;
    let mut db_md5: Option<String> = None;
    let mut md5_hasher = Context::new();
    let mut file_written = false;

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

                                // Use extracted helper for validation
                                validate_header_cells(&header_str, &header_re)?;

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
                        validate_header_cells(&header_str, &header_re)?;

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

    let computed_md5 = format!("{:x}", md5_hasher.finalize());

    match db_md5.as_ref() {
        Some(existing) if existing == &computed_md5 => Ok(true),
        _ => {
            let conn = Connection::open("templify.sqlite")?;
            conn.execute(
                "UPDATE templates SET datasource_md5 = ?1 WHERE id = ?2",
                params![computed_md5, ds.template_id],
            )?;
            Ok(false)
        }
    }
}
