use actix_web::{web, Responder};
use common::model::template::Template;
use rusqlite::Connection;

pub async fn process(payload: web::Json<Template>) -> impl Responder {
    match save_template(&payload).await {
        Ok(_) => actix_web::HttpResponse::Ok().body("Template saved successfully"),
        Err(e) => actix_web::HttpResponse::ServiceUnavailable()
            .body(format!("Template saving failed: {}", e)),
    }
}

pub async fn save_template(payload: &Template) -> Result<(), String> {
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    // Insert template
    conn.execute(
        "INSERT INTO templates (id, text) VALUES (?1, ?2)",
        (&payload.id, &payload.text),
    )
        .map_err(|e| e.to_string())?;
    // Insert images if any
    if let Some(images) = &payload.images {
        for image in images {
            conn.execute(
                "INSERT INTO images (id, template_id, base64) VALUES (?1, ?2, ?3)",
                (&image.id, &payload.id, &image.base64),
            )
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
