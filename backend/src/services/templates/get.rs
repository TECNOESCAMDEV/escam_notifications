use actix_web::web;
use common::model::image::Image;
use common::model::template::Template;
use rusqlite::{params, Connection};

pub async fn process(template_id: web::Path<String>) -> impl actix_web::Responder {
    match get_template(&template_id).await {
        Ok(template) => actix_web::HttpResponse::Ok().json(template),
        Err(e) => actix_web::HttpResponse::ServiceUnavailable()
            .body(format!("Error retrieving template: {}", e)),
    }
}
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
    let images: Vec<Image> = image_iter
        .filter_map(Result::ok)
        .collect();

    if !images.is_empty() {
        template.images = Some(images);
    }

    Ok(template)
}