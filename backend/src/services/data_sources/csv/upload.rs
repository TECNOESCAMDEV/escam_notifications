use actix_multipart::Multipart;
use actix_web::{HttpResponse, Responder};
use common::model::datasource::DataSource;
use futures_util::StreamExt;
use md5::Context;
use rusqlite::{params, Connection};
use serde_json::from_slice;
use std::fs::{rename, File};
use std::io::{BufWriter, Write};

type DynError = Box<dyn std::error::Error>;

/// HTTP handler for the CSV upload endpoint.
///
/// Accepts a multipart/form-data payload and delegates processing to
/// `upload_data_source`. Returns:
/// - `HttpResponse::Ok` on success.
/// - `HttpResponse::BadRequest` with an error message on failure.
pub async fn process(payload: Multipart) -> impl Responder {
    match upload_data_source(payload).await {
        Ok(_) => HttpResponse::Ok().finish(),
        Err(e) => HttpResponse::BadRequest().body(format!("Error: {}", e)),
    }
}

/// Parse a multipart upload, persist the uploaded CSV and update template metadata.
///
/// Behavior:
/// - Expects two multipart fields:
///   - `json`: JSON-serialized `DataSource` containing `template_id`.
///   - `file`: the CSV file bytes to be stored.
/// - Streams file bytes to a temporary file while computing an MD5 checksum.
/// - If the template was previously verified (`verified == 1`), updates
///   `last_verified_md5` with the current `datasource_md5` before overwriting.
/// - Moves the temp file to `template_id\_{md5}.csv` (final filename).
/// - Updates the `templates` table setting `datasource_md5 = computed_md5` and `verified = 0`.
///
/// Errors:
/// - Returns an error if `json` or `file` part is missing.
/// - Propagates filesystem and DB errors.
///
/// Returns:
/// - `Ok(())` on success.
/// - `Err(DynError)` on failure.
pub async fn upload_data_source(mut payload: Multipart) -> Result<(), DynError> {
    let mut data_source: Option<DataSource> = None;
    let mut file_received = false;
    let temp_file_path = "upload_temp_file.csv";
    let mut md5_hasher = Context::new();

    // Prepare temp file writer
    let mut temp_file = BufWriter::new(File::create(temp_file_path)?);

    // Process multipart form data
    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_name = field
            .content_disposition()
            .and_then(|cd| cd.get_name().map(|n| n.to_string()));

        match content_name.as_deref() {
            Some("json") => {
                let mut bytes = Vec::new();
                while let Some(chunk) = field.next().await {
                    bytes.extend_from_slice(&chunk?);
                }
                let ds: DataSource = from_slice(&bytes)?;
                data_source = Some(ds);
            }
            Some("file") => {
                file_received = true;
                while let Some(chunk) = field.next().await {
                    let data = chunk?;
                    md5_hasher.consume(&data);
                    temp_file.write_all(&data)?;
                }
            }
            _ => {}
        }
    }
    temp_file.flush()?;

    let ds = data_source.ok_or("Missing DataSource")?;
    if !file_received {
        return Err("Missing file".into());
    }

    let conn = Connection::open("templify.sqlite")?;

    let row = conn.query_row(
        "SELECT verified, datasource_md5 FROM templates WHERE id = ?1",
        params![ds.template_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)),
    );

    let (verified, datasource_md5) = match row {
        Ok((v, md5)) => (v, md5),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err("Template not found".into());
        }
        Err(e) => return Err(Box::new(e)),
    };

    // If verified, update last_verified_md5
    if verified == 1 {
        conn.execute(
            "UPDATE templates SET last_verified_md5 = ?1 WHERE id = ?2",
            params![datasource_md5, ds.template_id],
        )?;
    }

    // Compute MD5 hash of the file
    let computed_md5 = format!("{:x}", md5_hasher.finalize());

    // Rename temp file to final name templateID-md5.csv
    let final_file_name = format!("{}_{}.csv", ds.template_id, computed_md5);
    rename(temp_file_path, &final_file_name)?;

    // Update datasource_md5 and set verified to 0
    conn.execute(
        "UPDATE templates SET datasource_md5 = ?1, verified = 0 WHERE id = ?2",
        params![computed_md5, ds.template_id],
    )?;

    Ok(())
}
