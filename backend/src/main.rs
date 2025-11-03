mod config;
mod services;

use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use env_logger::Env;
use include_dir::{include_dir, Dir};
use log::info;
use mime_guess::from_path;
use std::thread;
use std::time::Duration;

static STATIC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/static/dist");

async fn serve_embedded(req: HttpRequest) -> HttpResponse {
    let path = req.path().trim_start_matches('/');
    let file_path = if path.is_empty() { "index.html" } else { path };

    match STATIC_DIR.get_file(file_path) {
        Some(file) => {
            let mime = from_path(file_path).first_or_octet_stream();
            HttpResponse::Ok()
                .content_type(mime.as_ref())
                .body(file.contents().to_vec())
        }
        None => match STATIC_DIR.get_file("index.html") {
            Some(index) => HttpResponse::Ok()
                .content_type("text/html; charset=utf-8")
                .body(index.contents().to_vec()),
            None => HttpResponse::NotFound().body("Not Found"),
        },
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(Env::default().default_filter_or("info"));
    let host = "127.0.0.1";
    let port = 8080;
    let url = format!("http://{}:{}", host, port);

    {
        let _url_clone = url.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            let _ = webbrowser::open(&_url_clone);
        });
    }

    info!("Server running at {}", url);

    HttpServer::new(move || {
        App::new()
            .app_data(web::JsonConfig::default().limit(10 * 1024 * 1024)) // 10 MB
            .service(services::templates::configure_routes())
            .default_service(web::route().to(serve_embedded))
    })
        .bind((host, port))?
        .run()
        .await
}
