use crate::job_controller::state::{JobStatus, JobUpdate, JobsState};
use actix_web::{web, HttpResponse, Responder};
use common::model::variable::VariableType;
use csv::ReaderBuilder;
use serde::Deserialize;
use std::fs::File;
use std::path::Path;
use tokio::sync::mpsc;

/// Input structure for CSV verification
#[derive(Deserialize)]
pub struct VerifyCsvRequest {
    pub uuid: String,
    pub var_type: VariableType,
    pub column_index: usize,
}

/// POST endpoint to start CSV verification
pub(crate) async fn process(
    jobs_state: web::Data<JobsState>,
    req: web::Json<VerifyCsvRequest>,
) -> impl Responder {
    match schedule_verify_job(jobs_state, req.into_inner()).await {
        Ok(job_id) => HttpResponse::Ok().body(job_id),
        Err(err) => HttpResponse::InternalServerError().body(err),
    }
}

/// Schedules a verification job
async fn schedule_verify_job(
    jobs_state: web::Data<JobsState>,
    req: VerifyCsvRequest,
) -> Result<String, String> {
    let job_id = uuid::Uuid::new_v4().to_string();

    {
        let mut jobs = jobs_state.jobs.write().await;
        jobs.insert(job_id.clone(), JobStatus::Pending);
    }

    let tx = jobs_state.tx.clone();

    let value = job_id.clone();
    tokio::spawn(async move {
        if let Err(e) = verify_csv_data(tx, value.clone(), req).await {
            let mut jobs = jobs_state.jobs.write().await;
            jobs.insert(value.clone(), JobStatus::Failed(e));
        }
    });

    Ok(job_id)
}

/// Verifies the CSV file data according to the variable type and column index
async fn verify_csv_data(
    tx: mpsc::Sender<JobUpdate>,
    job_id: String,
    req: VerifyCsvRequest,
) -> Result<(), String> {
    // Build file path
    let file_path = format!("./{}.csv", req.uuid);
    if !Path::new(&file_path).exists() {
        return Err("CSV file not found".to_string());
    }

    // Open CSV file
    let file = File::open(&file_path).map_err(|e| e.to_string())?;
    let mut rdr = ReaderBuilder::new().has_headers(true).from_reader(file);

    let mut invalid_rows = Vec::new();
    let mut total_rows = 0usize;

    // Iterate efficiently over CSV records
    for (i, result) in rdr.records().enumerate() {
        let record = result.map_err(|e| e.to_string())?;
        total_rows += 1;

        // Check if column exists
        if req.column_index >= record.len() {
            return Err(format!(
                "Column index {} out of bounds at row {}",
                req.column_index,
                i + 1
            ));
        }

        let value = &record[req.column_index];
        if !validate_value(&req.var_type, value) {
            invalid_rows.push(i + 1);
        }

        // Report progress every 100 rows
        if (i + 1) % 100 == 0 {
            let progress: u32 = (((i + 1) * 100 / total_rows.max(1)) as u32)
                .try_into()
                .map_err(|_| "Failed to convert progress value to u32")?;

            tx.send(JobUpdate {
                job_id: job_id.clone(),
                status: JobStatus::InProgress(progress),
            })
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    // Final status
    if invalid_rows.is_empty() {
        tx.send(JobUpdate {
            job_id: job_id.clone(),
            status: JobStatus::Completed("Verification successful".to_string()),
        })
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        let msg = format!("Invalid rows at: {:?}", invalid_rows);
        tx.send(JobUpdate {
            job_id: job_id.clone(),
            status: JobStatus::Completed(msg),
        })
            .await
            .map_err(|e| e.to_string())?;
        Err("Some rows failed validation".to_string())
    }
}

/// Validates a value according to the VariableType
fn validate_value(var_type: &VariableType, value: &str) -> bool {
    match var_type {
        VariableType::Text => true,
        VariableType::Number => value.parse::<f64>().is_ok(),
        VariableType::Currency => value.parse::<f64>().is_ok(),
        VariableType::Email => {
            // Simple email validation
            value.contains('@') && value.contains('.')
        }
    }
}
