use crate::model::place_holder::PlaceholderType;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
/// Represents a column validation rule with a normalized title and inferred placeholder type.
pub struct ColumnCheck {
    pub title: String,
    pub placeholder_type: PlaceholderType,
    pub first_row: Option<String>,
}