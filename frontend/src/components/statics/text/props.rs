use yew::prelude::*;

#[derive(Properties, PartialEq, Clone)]
pub struct StaticTextProps {
    #[prop_or_default]
    pub template_id: Option<String>,
}