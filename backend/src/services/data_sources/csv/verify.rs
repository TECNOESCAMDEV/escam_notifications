use crate::job_controller::state::{JobStatus, JobUpdate, JobsState};
use actix_web::web::Data;
use actix_web::Responder;
use tokio::sync::mpsc;

pub(crate) async fn process(jobs_state: Data<JobsState>) -> impl Responder {
    match schedule_verify_job(jobs_state).await {
        Ok(job_id) => actix_web::HttpResponse::Ok().body(job_id),
        Err(err) => actix_web::HttpResponse::InternalServerError().body(err),
    }
}

async fn schedule_verify_job(jobs_state: Data<JobsState>) -> Result<String, String> {
    let job_id = uuid::Uuid::new_v4().to_string();

    {
        let mut jobs = jobs_state.jobs.write().await;
        jobs.insert(job_id.clone(), JobStatus::Pending);
    }

    let tx = jobs_state.tx.clone();

    // Spawn a new task to handle the verification process
    let value = job_id.clone();
    tokio::spawn(async move {
        if let Err(e) = verify_csv_data(tx, value.clone()).await {
            let mut jobs = jobs_state.jobs.write().await;
            jobs.insert(value.clone(), JobStatus::Failed(e));
        }
    });

    Ok(job_id)
}

async fn verify_csv_data(tx: mpsc::Sender<JobUpdate>, job_id: String) -> Result<(), String> {
    // Simulate verification process
    for progress in 1..=100 {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        tx.send(JobUpdate {
            job_id: job_id.clone(),
            status: JobStatus::InProgress(progress),
        })
            .await
            .map_err(|e| e.to_string())?;
    }

    // After verification is complete
    tx.send(JobUpdate {
        job_id: job_id.clone(),
        status: JobStatus::Completed("Verification successful".to_string()),
    })
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}
