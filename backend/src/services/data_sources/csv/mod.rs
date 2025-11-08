use actix_web::Scope;

mod get_status;
mod verify;

const API_PATH: &str = "/api/data_sources/csv";

pub fn configure_routes() -> Scope {
    actix_web::web::scope(API_PATH)
        .route("/verify", actix_web::web::post().to(verify::process))
        .route(
            "/status/{job_id}",
            actix_web::web::get().to(get_status::process),
        )
}
