use actix_multipart::Multipart;
use actix_web::{HttpResponse, Responder};
use common::model::datasource::DataSource;
use futures_util::StreamExt;
use md5::Context;
use rusqlite::{params, Connection};
use serde_json::from_slice;
use std::fs::File;
use std::io::Write;

pub async fn process(payload: Multipart) -> impl Responder {
    match upload_data_source(payload).await {
        Ok(result) => HttpResponse::Ok().body(result.to_string()),
        Err(e) => HttpResponse::BadRequest().body(format!("Error: {}", e)),
    }
}

pub async fn upload_data_source(
    mut payload: Multipart,
) -> Result<bool, Box<dyn std::error::Error>> {
    use std::io::BufWriter;

    let mut data_source: Option<DataSource> = None;
    // db_md5 will hold the value from DB; None means the column is NULL
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
                    // Save the file and compute MD5 simultaneously
                    let file = File::create(format!("{}.csv", ds.template_id))?;
                    let mut writer = BufWriter::new(file);
                    while let Some(chunk) = field.next().await {
                        let chunk = chunk?;
                        md5_hasher.consume(&chunk);
                        writer.write_all(&chunk)?;
                    }
                    file_written = true;
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
                // Validate template existence immediately and read datasource_md5 (maybe NULL)
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

    // Compute MD5 hex string
    let computed_md5 = format!("{:x}", md5_hasher.finalize());

    // Compare taking into account that db_md5 may be NULL
    match db_md5.as_ref() {
        Some(existing) if existing == &computed_md5 => {
            // Match -> return true (no update needed)
            Ok(true)
        }
        _ => {
            // Mismatch or NULL -> update DB with new md5 and return false
            let conn = Connection::open("templify.sqlite")?;
            conn.execute(
                "UPDATE templates SET datasource_md5 = ?1 WHERE id = ?2",
                params![computed_md5, ds.template_id],
            )?;
            Ok(false)
        }
    }
}
