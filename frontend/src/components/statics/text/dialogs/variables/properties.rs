use common::model::variable::Variable;
use yew::{Callback, Properties};

#[derive(Properties, PartialEq)]
pub struct VariableCreatorProperties {
    pub on_created: Callback<Variable>,
}