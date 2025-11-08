use actix_web::web::{get, post, scope};
use actix_web::Scope;

mod get_status;
mod verify;

const API_PATH: &str = "/api/data_sources/csv";

pub fn configure_routes() -> Scope {
    scope(API_PATH)
        .route("/verify", post().to(verify::process))
        .route(
            "/status/{job_id}",
            get().to(get_status::process),
        )
}
