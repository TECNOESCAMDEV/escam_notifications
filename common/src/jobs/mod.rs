use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    InProgress(u32),
    Completed(String),
    Failed(String),
}
