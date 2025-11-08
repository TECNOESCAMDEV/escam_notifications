use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub id: String,
    pub var_type: VariableType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VariableType {
    Text,
    Number,
    Currency,
    Email,
}