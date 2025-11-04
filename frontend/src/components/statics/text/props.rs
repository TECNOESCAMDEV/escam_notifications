//! Component properties for the static text editor.
//!
//! Currently only supports an optional `template_id` used to preload a server-side
//! template on first render. If omitted, a new empty template is created.
use yew::prelude::*;

#[derive(Properties, PartialEq, Clone)]
pub struct StaticTextProps {
    /// Optional id of the template to fetch on mount. If `None`, the editor
    /// starts with a brand-new empty template.
    #[prop_or_default]
    pub template_id: Option<String>,
}