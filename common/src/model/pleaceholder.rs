use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceHolder {
    pub title: String,
    pub placeholder_type: PlaceholderType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlaceholderType {
    Text,
    Number,
    Currency,
    Email,
}