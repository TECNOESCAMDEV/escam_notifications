use actix_web::{web, Responder};
use common::model::template::Template;
use rusqlite::{params, Connection};

pub async fn process(payload: web::Json<Template>) -> impl Responder {
    match save_template(&payload).await {
        Ok(_) => actix_web::HttpResponse::Ok().body("Template guardado correctamente"),
        Err(e) => actix_web::HttpResponse::ServiceUnavailable()
            .body(format!("Error al guardar template: {}", e)),
    }
}

pub async fn save_template(payload: &Template) -> Result<(), String> {
    if payload.id.trim().is_empty() {
        return Err("El id del template no puede estar vacío".to_string());
    }

    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;

    // Insert or update template
    conn.execute(
        "INSERT OR REPLACE INTO templates (id, text) VALUES (?1, ?2)",
        params![&payload.id, &payload.text],
    )
        .map_err(|e| e.to_string())?;

    if let Some(images) = &payload.images {
        // Get existing image IDs for the template
        let existing_ids: Vec<String> = conn
            .prepare("SELECT id FROM images WHERE template_id = ?1")
            .map_err(|e| e.to_string())?
            .query_map(params![&payload.id], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();

        // Delete images that are no longer present
        for old_id in &existing_ids {
            if !images.iter().any(|img| &img.id == old_id) {
                conn.execute(
                    "DELETE FROM images WHERE id = ?1 AND template_id = ?2",
                    params![old_id, &payload.id],
                )
                    .map_err(|e| e.to_string())?;
            }
        }

        // Insert or update images
        for image in images {
            conn.execute(
                "INSERT OR REPLACE INTO images (id, template_id, base64) VALUES (?1, ?2, ?3)",
                params![&image.id, &payload.id, &image.base64],
            )
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}
