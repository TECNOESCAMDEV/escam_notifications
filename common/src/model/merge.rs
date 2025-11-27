use serde::{Deserialize, Serialize};

/// Represents the overall state of a PDF merge job.
///
/// A merge job processes a template with its associated CSV data source,
/// generating individual PDFs for each row of data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeJobState {
    /// The job is queued but hasn't started processing yet.
    Pending,
    /// The job is actively processing rows. Contains the number of rows processed so far.
    InProgress(u32),
    /// The job completed successfully. Contains the total number of PDFs generated.
    Completed(u32),
    /// The job failed with an error. Contains the error message.
    Failed(String),
}

/// Represents the state of processing a single row (task) within a merge job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeTaskState {
    /// The task is waiting to be processed.
    Pending,
    /// The task is currently being processed.
    Processing,
    /// The task completed successfully.
    Completed,
    /// The task failed. Contains the error message.
    Failed(String),
}

/// Represents a complete merge job, tracking the processing of a template
/// with its CSV data source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeJob {
    /// Unique identifier for this merge job (UUID).
    pub job_id: String,
    /// The ID of the template being used for the merge.
    pub template_id: String,
    /// Current state of the merge job.
    pub state: MergeJobState,
    /// List of individual row processing tasks within this job.
    pub tasks: Vec<MergeTask>,
}

/// Represents the processing of a single CSV row within a merge job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeTask {
    /// The 0-based index of the row in the CSV file (excluding the header).
    pub row_index: usize,
    /// Current state of processing this particular row.
    pub state: MergeTaskState,
    /// Optional error message if the task failed.
    pub error: Option<String>,
}
