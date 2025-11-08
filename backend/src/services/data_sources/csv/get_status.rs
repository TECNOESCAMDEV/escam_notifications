use crate::job_controller::state::JobsState;
use actix_web::{web, Responder};

pub(crate) async fn process(job_id: web::Path<String>, state: web::Data<JobsState>) -> impl Responder {
    get_csv_job_status(job_id, state).await
}
async fn get_csv_job_status(
    job_id: web::Path<String>,
    state: web::Data<JobsState>,
) -> impl Responder {
    let jobs = state.jobs.read().await;
    if let Some(status) = jobs.get(&job_id.into_inner()) {
        actix_web::HttpResponse::Ok().json(status)
    } else {
        actix_web::HttpResponse::NotFound().body("Job ID not found")
    }
}
