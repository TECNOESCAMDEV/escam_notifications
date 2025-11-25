//! # Template Service Module
//!
//! This module aggregates all API endpoints related to the management of templates.
//! It acts as a router, directing incoming HTTP requests under the `/api/templates`
//! path to the appropriate handler logic defined in its sub-modules.
//!
//! ## Sub-modules:
//! - `get`: Handles the retrieval of a specific template's data from the database.
//! - `save`: Manages the creation and updating of templates and their associated images.
//! - `pdf`: Responsible for generating and serving a PDF document from a given template.

mod get;
mod pdf;
mod save;

use actix_web::web::{get, post, scope};
use actix_web::Scope;

/// The base path for all template-related API endpoints.
const API_PATH: &str = "/api/templates";

/// Configures and returns the Actix `Scope` for all template-related routes.
///
/// This function groups the template endpoints under the common `/api/templates` path.
///
/// # Registered Routes:
///
/// *   **`POST /save`**:
///     - **Handler**: `save::process`
///     - **Description**: Creates a new template or updates an existing one. It expects a
///       JSON payload representing a `Template` object, which includes the template's
///       unique ID, its text content, and an optional list of associated images (ID and Base64 data).
///       The handler persists this information in the database.
///
/// *   **`GET /{template_id}`**:
///     - **Handler**: `get::process`
///     - **Description**: Retrieves the complete data for a single template, identified by its
///       `template_id` in the URL path. It returns a JSON object containing the template's
///       text and all its associated images.
///
/// *   **`GET /pdf/{template_id}`**:
///     - **Handler**: `pdf::process`
///     - **Description**: Generates a PDF document from the specified template and serves it
///       to the client. The handler fetches the template's text and images, renders them
///       into a PDF file, and returns the file for inline display in the browser.
pub fn configure_routes() -> Scope {
    scope(API_PATH)
        .route("/save", post().to(save::process))
        .route("/{template_id}", get().to(get::process))
        .route("/pdf/{template_id}", get().to(pdf::process))
}
