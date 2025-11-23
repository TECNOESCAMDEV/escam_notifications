mod get;
mod save;
mod pdf;

use actix_web::web::{get, post, scope};
use actix_web::Scope;

const API_PATH: &str = "/api/templates";

pub fn configure_routes() -> Scope {
    scope(API_PATH)
        .route("/save", post().to(save::process))
        .route("/{template_id}", get().to(get::process))
        .route("/pdf/{template_id}", get().to(pdf::process))
}
