//! Handles the persistence of template data, including text content and associated images.
//!
//! This module provides the `POST /api/templates/save` endpoint, which is the primary
//! mechanism for creating new templates or updating existing ones. It receives a complete
//! `Template` object and synchronizes its state with the database.
//!
//! ---
//!
//! ## Workflow
//!
//! 1.  **Receive Payload**: The `process` handler receives a JSON payload that conforms to the
//!     `common::model::template::Template` struct. This includes the template's `id`, `text`,
//!     and an optional list of `Image` objects.
//!
//! 2.  **Database Upsert**: The `save_template` function performs an "upsert" operation on the
//!     `templates` table. It inserts a new row if the `id` doesn't exist or updates the `text`
//!     if it does. Note that this operation only modifies the `text` field, leaving other
//!     template-related columns (like `datasource_md5` or `verified`) untouched, as those
//!     are managed by other services (e.g., `data_sources::csv`).
//!
//! 3.  **Image Synchronization**: The function intelligently synchronizes the images associated
//!     with the template:
//!     - If the payload contains an `images` array, it compares the incoming image IDs with
//!       those already in the database for the given `template_id`.
//!     - Images present in the database but not in the payload are deleted (orphan removal).
//!     - Images in the payload are inserted or updated using `INSERT OR REPLACE`.
//!     - If the payload's `images` field is `null` or omitted, all existing images for that
//!       template are deleted.
//!
//! This ensures that the database state for a template's images perfectly mirrors the
//! state sent by the client on each save operation.

use actix_web::{web, Responder};
use common::model::template::Template;
use rusqlite::{params, Connection};

/// Handles the HTTP POST request to save a template.
///
/// This function serves as the Actix web endpoint. It deserializes the JSON payload
/// into a `Template` object and passes it to `save_template` for processing.
/// It returns an appropriate HTTP response indicating success or failure.
///
/// # Arguments
/// * `payload` - A `web::Json<Template>` containing the template data sent by the client.
///
/// # Returns
/// - `200 OK` with a success message if the template is saved correctly.
/// - `503 Service Unavailable` with an error message if any database operation fails.
pub async fn process(payload: web::Json<Template>) -> impl Responder {
    match save_template(&payload).await {
        Ok(_) => actix_web::HttpResponse::Ok().body("Template saved successfully"),
        Err(e) => actix_web::HttpResponse::ServiceUnavailable()
            .body(format!("Error saving template: {}", e)),
    }
}

/// Saves or updates a template and its associated images in the database.
///
/// This function contains the core logic for persisting template data. It performs
/// a transaction-like sequence of operations:
/// 1. Validates that the template ID is not empty.
/// 2. Inserts or updates the template's main text content.
/// 3. Synchronizes the associated images by deleting orphans and upserting new/updated ones.
///
/// # Arguments
/// * `payload` - A reference to the `Template` object to be saved.
///
/// # Returns
/// - `Ok(())` on successful completion of all database operations.
/// - `Err(String)` if the template ID is invalid or if any database query fails.
pub async fn save_template(payload: &Template) -> Result<(), String> {
    if payload.id.trim().is_empty() {
        return Err("Template id cannot be empty".to_string());
    }

    let conn = Connection::open("templify.sqlite").map_err(|e| e.to_string())?;

    // Insert or update the template's text.
    // This uses `ON CONFLICT` to perform an "upsert". It only touches the `text` column,
    // preserving other data like data source info which is managed by other services.
    conn.execute(
        "INSERT INTO templates (id, text) VALUES (?1, ?2)
         ON CONFLICT(id) DO UPDATE SET text = excluded.text",
        params![&payload.id, &payload.text],
    )
        .map_err(|e| e.to_string())?;

    match &payload.images {
        Some(images) => {
            // If images are provided, sync them.
            // First, get all existing image IDs for this template.
            let existing_ids: Vec<String> = conn
                .prepare("SELECT id FROM images WHERE template_id = ?1")
                .map_err(|e| e.to_string())?
                .query_map(params![&payload.id], |row| row.get(0))
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .collect();

            // Delete any images that are no longer in the payload (orphans).
            for old_id in &existing_ids {
                if !images.iter().any(|img| &img.id == old_id) {
                    conn.execute(
                        "DELETE FROM images WHERE id = ?1 AND template_id = ?2",
                        params![old_id, &payload.id],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }

            // Insert or replace all images from the payload.
            for image in images {
                conn.execute(
                    "INSERT OR REPLACE INTO images (id, template_id, base64) VALUES (?1, ?2, ?3)",
                    params![&image.id, &payload.id, &image.base64],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        None => {
            // If no images are provided in the payload, delete all associated images.
            conn.execute(
                "DELETE FROM images WHERE template_id = ?1",
                params![&payload.id],
            )
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}
