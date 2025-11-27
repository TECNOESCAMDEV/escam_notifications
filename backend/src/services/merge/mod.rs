mod start;

use crate::services::merge::start::process;
use actix_web::web;

const API_PATH: &str = "/api/merge";

/// Configures and returns the Actix `Scope` for all merge-related routes.
pub fn configure_routes() -> actix_web::Scope {
    web::scope(API_PATH).route("/start", web::post().to(process))
}
