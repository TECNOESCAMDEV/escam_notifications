//! Defines the properties for the `StaticTextComponent`.
//!
//! This module contains the `StaticTextProps` struct, which specifies the data that can be
//! passed from a parent component to the static text editor. These properties are used to
//! configure the initial state of the editor upon mounting.

use yew::prelude::*;

/// Properties for the `StaticTextComponent`.
///
/// This struct is used by Yew to pass configuration data to the editor. It allows parent
/// components to control how the editor is initialized.
#[derive(Properties, PartialEq, Clone)]
pub struct StaticTextProps {
    /// The optional ID of a template to load from the server when the component is first rendered.
    ///
    /// - If `Some(id)` is provided, the component will make an API request to fetch the
    ///   template with the specified `id`. If successful, the editor will be populated with
    ///   the template's text and images. If the request fails, a new, empty template is
    ///   created as a fallback.
    ///
    /// - If `None` (the default), the component will start with a new, empty template,
    ///   allowing the user to create content from scratch.
    ///
    /// This property is checked only once during the `rendered` lifecycle hook on the first render.
    #[prop_or_default]
    pub template_id: Option<String>,
}
