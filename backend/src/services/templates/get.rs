//! # Template Retrieval Service
//!
//! This module is responsible for fetching the complete data for a single template,
//! including its text content and all associated images. It provides the backend logic
//! for the `GET /api/templates/{template_id}` endpoint.
//!
//! ## Workflow
//!
//! 1.  **HTTP Request**: The `process` function serves as the Actix web handler. It receives
//!     an HTTP GET request containing a `template_id` in the URL path.
//!
//! 2.  **Data Fetching**: It delegates the core logic to the `get_template` function.
//!
//! 3.  **Database Query**: `get_template` connects to the `templify.sqlite` database and performs
//!     two main queries:
//!     - It first retrieves the template's `id` and `text` from the `templates` table.
//!     - It then fetches all associated images (their `id` and `base64` content) from the
//!       `images` table using the `template_id`.
//!
//! 4.  **Model Assembly**: The results are assembled into a `common::model::template::Template`
//!     struct. This struct contains the template's text and an `Option<Vec<Image>>` for its images.
//!
//! 5.  **HTTP Response**: The `process` function serializes the resulting `Template` object into
//!     a JSON payload and returns it in a `200 OK` response. If the template is not found or a
//!     database error occurs, it returns an appropriate error response (e.g., `503 Service Unavailable`).
//!
//! This module exclusively handles the retrieval of template content and does not interact with
//! data source-related fields like `datasource_md5` or `verified`, which are managed by other services.

use actix_web::web;
use common::model::image::Image;
use common::model::template::Template;
use rusqlite::{params, Connection};

/// Actix web handler for the `GET /api/templates/{template_id}` endpoint.
///
/// This function receives a template ID from the URL path, calls `get_template`
/// to fetch the data, and returns the result as an HTTP response.
///
/// # Arguments
/// * `template_id` - The unique identifier of the template, extracted from the URL path.
///
/// # Returns
/// - `200 OK` with the `Template` object as a JSON payload on success.
/// - `503 Service Unavailable` with an error message if the template cannot be retrieved.
pub async fn process(template_id: web::Path<String>) -> impl actix_web::Responder {
    match get_template(&template_id).await {
        Ok(template) => actix_web::HttpResponse::Ok().json(template),
        Err(e) => actix_web::HttpResponse::ServiceUnavailable()
            .body(format!("Error retrieving template: {}", e)),
    }
}

/// Fetches a template and its associated images from the database.
///
/// Connects to the SQLite database, queries for the template text and all related
/// images, and constructs a `Template` model.
///
/// # Arguments
/// * `template_id` - The ID of the template to fetch.
///
/// # Returns
/// - `Ok(Template)` containing the complete template data if found.
/// - `Err(String)` if the template is not found or a database error occurs.
pub async fn get_template(template_id: &str) -> Result<Template, String> {
    // Open a SQLite connection to the file templify.sqlite
    let conn = Connection::open("templify.sqlite").map_err(|e| e.to_string())?;

    // Query the template by ID
    let mut stmt = conn
        .prepare("SELECT id, text FROM templates WHERE id = ?1")
        .map_err(|e| e.to_string())?;
    let template_iter = stmt
        .query_map(params![template_id], |row| {
            Ok(Template {
                id: row.get(0)?,
                text: row.get(1)?,
                images: None,
            })
        })
        .map_err(|e| e.to_string())?;

    // Get the template (there should be only one)
    let mut template: Template = match template_iter.into_iter().next() {
        Some(Ok(t)) => t,
        Some(Err(e)) => return Err(e.to_string()),
        None => return Err("Template not found".to_string()),
    };

    // Query associated images
    let mut img_stmt = conn
        .prepare("SELECT id, base64 FROM images WHERE template_id = ?1")
        .map_err(|e| e.to_string())?;
    let image_iter = img_stmt
        .query_map(params![template_id], |row| {
            Ok(Image {
                id: row.get(0)?,
                base64: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;

    // Collect images into a vector
    let images: Vec<Image> = image_iter.filter_map(Result::ok).collect();

    if !images.is_empty() {
        template.images = Some(images);
    }

    Ok(template)
}
