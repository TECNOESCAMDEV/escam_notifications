//! # Data Source Models
//!
//! This module defines the data structures that represent the connection between a template
//! and an external source of data, such as a CSV file. These models are part of the `common`
//! crate to ensure a consistent representation between the backend and frontend.
//!
//! The core concept is the separation of a template's static content (defined in `common::model::template`)
//! from the dynamic data that populates it. This module provides the structures for managing
//! that dynamic data aspect.

use serde::{Deserialize, Serialize};

/// Represents the conceptual link between a template and its external data source.
///
/// This struct is a high-level model that establishes the relationship between a
/// `Template` and the data that will be merged into it. While the backend services
/// often operate directly with a `template_id` to manage data source-related
/// attributes (like verification status or file hashes stored in the `templates` table),
/// this struct provides a clear, conceptual model for what a data source is.
///
/// ## Key Responsibilities & Context:
/// - **Linking**: The primary role of this model is to associate a data source with a
///   specific template via the `template_id`.
/// - **Separation of Concerns**: It reinforces the architectural principle of separating
///   the template's design/layout (`Template`) from its variable data (`DataSource`).
///   The template defines *what the document looks like*, while the data source provides
///   *what it is filled with*.
/// - **Backend Services**: In the backend, services like `data_sources::csv` use the
///   `template_id` to perform operations such as:
///     - Verifying the structure and content of an associated CSV file.
///     - Storing metadata like the file's MD5 hash (`datasource_md5`) and verification
///       status (`verified`) in the `templates` database table.
///     - Inferring a schema of `ColumnCheck` objects that the frontend can display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSource {
    /// The unique identifier (UUID) of the template to which this data source is linked.
    /// This acts as the foreign key connecting the data source information to its
    /// corresponding template in the database and API operations.
    pub template_id: String,
}
