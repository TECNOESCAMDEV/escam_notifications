//! # Placeholder Schema and Type Definitions
//!
//! This module defines the core data structures for representing placeholders, which are
//! dynamic fields derived from a data source (like a CSV file). These structures are
//! fundamental to the `common` crate, ensuring a consistent understanding of data types
//! across the backend and frontend.

use serde::{Deserialize, Serialize};

/// Represents the schema for a single placeholder available for use in a template.
///
/// While not directly used in the current backend-to-frontend communication flow, this
/// struct establishes the conceptual model for a placeholder: a named field with a
/// specific data type. The actual data transfer for column schemas is handled by
/// `common::model::csv::ColumnCheck`, which includes additional context like an example
/// value from the first data row. `PlaceHolder` can be seen as the simplified, abstract
/// representation of a data source column once it has been verified and is ready to be
/// used as a placeholder (e.g., `{{title}}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceHolder {
    /// The name of the placeholder, derived from a data source column header
    /// (e.g., "first_name", "order_total").
    pub title: String,
    /// The data type associated with the placeholder, which dictates potential
    /// formatting or validation rules.
    pub placeholder_type: PlaceholderType,
}

/// An enumeration of the possible data types that can be inferred for a data source column.
///
/// This enum is a critical component of the data verification process. The backend service
/// `services::data_sources::csv::mod.rs` uses heuristics to assign a `PlaceholderType` to
/// each column of an uploaded CSV file. For example, it checks for '@' to infer `Email`,
/// currency symbols for `Currency`, and attempts to parse a value as a float for `Number`.
///
/// This type information is then packaged within the `ColumnCheck` struct and sent to the
/// frontend upon successful verification. The frontend UI can then use this type to:
/// - Display a relevant icon next to each column name (e.g., a number sign for `Number`).
/// - Provide context to the user about the kind of data in each column.
/// - Potentially enable type-specific formatting options in the future.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlaceholderType {
    /// Generic text data. This is the default or fallback type.
    Text,
    /// A numeric value, which could be an integer or a floating-point number.
    Number,
    /// A monetary value, identified by the presence of common currency symbols ($, €, £, ¥).
    Currency,
    /// An email address, identified by the presence of '@' and '.' characters.
    Email,
}