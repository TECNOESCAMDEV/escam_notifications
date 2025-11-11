use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub enum JobStatus {
    Pending,
    InProgress(u32),
    Completed(String),
    Failed(String),
}