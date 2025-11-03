mod save;

use actix_web::{web, Scope};

const API_PATH: &str = "/api/templates";

pub fn configure_routes() -> Scope {
    web::scope(API_PATH).route("/save", web::post().to(save::process))
}
