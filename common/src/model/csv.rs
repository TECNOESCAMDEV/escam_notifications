use crate::model::place_holder::PlaceholderType;
use serde::{Deserialize, Serialize};

/// Represents the inferred schema of a single CSV column, generated during the
/// verification process on the backend.
///
/// When a user uploads a CSV file, the backend service (`data_sources::csv::verify`)
/// analyzes the header and the first data row to guess the structure and data types
/// of the columns. A vector of `ColumnCheck` structs, `Vec<ColumnCheck>`, is created
/// to represent this inferred schema.
///
/// This vector is then serialized into a JSON string and sent to the frontend as the
/// payload of a `JobStatus::Completed` message. The frontend deserializes this JSON
/// to display the column details to the user, allowing them to review and confirm
/// the detected schema before linking the data source to a template.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ColumnCheck {
    /// The normalized column header title from the CSV file.
    /// Spaces are typically replaced with underscores for consistency.
    pub title: String,
    /// The data type (`Text`, `Number`, `Currency`, `Email`) inferred from the
    /// content of the first data row for this column.
    pub placeholder_type: PlaceholderType,
    /// The actual value from the first data row for this column.
    /// This is used on the frontend to provide the user with a concrete example
    /// of the data in the column, helping them validate the inferred type.
    pub first_row: Option<String>,
}
