use actix_web::{web, Responder};
use common::model::template::Template;
use rusqlite::{params, Connection};

/// Handles the HTTP POST request to save a template.
/// Returns an appropriate HTTP response based on the result.
pub async fn process(payload: web::Json<Template>) -> impl Responder {
    match save_template(&payload).await {
        Ok(_) => actix_web::HttpResponse::Ok().body("Template saved successfully"),
        Err(e) => actix_web::HttpResponse::ServiceUnavailable()
            .body(format!("Error saving template: {}", e)),
    }
}

/// Saves or updates a template and its associated images in the database.
/// - Validates that the template ID is not empty.
/// - Inserts or updates the template.
/// - If images are provided:
///     - Deletes images not present in the payload.
///     - Inserts or updates the provided images.
/// - If no images are provided:
///     - Deletes all images associated with the template.
pub async fn save_template(payload: &Template) -> Result<(), String> {
    if payload.id.trim().is_empty() {
        return Err("Template id cannot be empty".to_string());
    }

    let conn = Connection::open("templify.sqlite").map_err(|e| e.to_string())?;

    // Insert or update the template
    conn.execute(
        "INSERT INTO templates (id, text) VALUES (?1, ?2)
         ON CONFLICT(id) DO UPDATE SET text = excluded.text",
        params![&payload.id, &payload.text],
    )
        .map_err(|e| e.to_string())?;

    match &payload.images {
        Some(images) => {
            let existing_ids: Vec<String> = conn
                .prepare("SELECT id FROM images WHERE template_id = ?1")
                .map_err(|e| e.to_string())?
                .query_map(params![&payload.id], |row| row.get(0))
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .collect();

            for old_id in &existing_ids {
                if !images.iter().any(|img| &img.id == old_id) {
                    conn.execute(
                        "DELETE FROM images WHERE id = ?1 AND template_id = ?2",
                        params![old_id, &payload.id],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }

            for image in images {
                conn.execute(
                    "INSERT OR REPLACE INTO images (id, template_id, base64) VALUES (?1, ?2, ?3)",
                    params![&image.id, &payload.id, &image.base64],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        None => {
            conn.execute(
                "DELETE FROM images WHERE template_id = ?1",
                params![&payload.id],
            )
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}
