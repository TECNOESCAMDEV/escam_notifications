pub struct Variable {
    pub id: String,
    pub var_type: VariableType,
}

pub enum VariableType {
    Text,
    Number,
    Currency,
    Email,
}