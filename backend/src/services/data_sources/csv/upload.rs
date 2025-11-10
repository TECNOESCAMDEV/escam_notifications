use actix_multipart::Multipart;
use actix_web::{HttpResponse, Responder};
use common::model::datasource::DataSource;
use futures_util::StreamExt;
use md5::Context;
use serde_json::from_slice;
use std::fs::File;
use std::io::Write;

pub async fn process(payload: Multipart) -> impl Responder {
    match upload_data_source(payload).await {
        Ok(_) => HttpResponse::Ok().body("File uploaded successfully"),
        Err(e) => HttpResponse::BadRequest().body(format!("Error: {}", e)),
    }
}

pub async fn upload_data_source(mut payload: Multipart) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::BufWriter;

    let mut data_source: Option<DataSource> = None;
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
                    let file = File::create(format!("{}.csv", ds.id))?;
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
                data_source = Some(ds);
            }
            _ => {}
        }
    }

    let ds = data_source.ok_or("Missing DataSource")?;
    if !file_written {
        return Err("Missing file".into());
    }

    let computed_md5 = format!("{:x}", md5_hasher.finalize());
    if ds.csv_md5 != computed_md5 {
        return Err(format!(
            "MD5 mismatch: expected {}, got {}",
            ds.csv_md5, computed_md5
        )
            .into());
    }

    Ok(())
}
