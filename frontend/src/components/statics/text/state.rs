//! Component state and utilities for the static text editor.
//!
//! Fields overview
//! - `text`: Current textarea content.
//! - `history` / `history_index`: Simple linear undo/redo stack.
//! - `active_tab`: Either "editor" or "preview".
//! - `textarea_ref`, `file_input_ref`, `image_dialog_ref`: Node references into the DOM.
//! - `template`: The bound `Template` model kept in sync with `text`.
//! - `selected_image_id`: The id of the image whose dialog is open (if any).
//! - `loaded`: Guards the one-time initialization in `rendered`.
//!
//! Methods
//! - `new()`: Default constructor with initial values.
//! - `resize_textarea()`: Auto-grow the textarea to fit its scroll height.
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlTextAreaElement};
use yew::prelude::*;

use common::model::template::Template;

pub struct StaticTextComponent {
    pub text: String,
    pub history: Vec<String>,
    pub history_index: usize,
    pub active_tab: String,
    pub textarea_ref: NodeRef,
    pub file_input_ref: NodeRef,
    pub image_dialog_ref: NodeRef,
    pub pdf_viewer_dialog_ref: NodeRef,
    pub template: Option<Template>,
    pub selected_image_id: Option<String>,
    pub pdf_url: Option<String>,
    pub loaded: bool,
    pub original_md5: Option<String>,
}

impl StaticTextComponent {
    /// Creates a component with sane defaults: empty text, initial history entry,
    /// "editor" tab active, and no template loaded.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            history: vec![String::new()],
            history_index: 0,
            active_tab: "editor".to_string(),
            textarea_ref: Default::default(),
            file_input_ref: Default::default(),
            image_dialog_ref: Default::default(),
            pdf_viewer_dialog_ref: Default::default(),
            template: None,
            selected_image_id: None,
            pdf_url: None,
            loaded: false,
            original_md5: None,
        }
    }

    /// Adjusts the textarea CSS height to its scroll height, yielding an
    /// auto-growing input that avoids internal scrollbars.
    pub fn resize_textarea(&self) {
        if let Some(textarea) = self.textarea_ref.cast::<HtmlTextAreaElement>() {
            if let Ok(html_elem) = textarea.clone().dyn_into::<HtmlElement>() {
                let style = html_elem.style();
                let _ = style.set_property("height", "auto");
                let scroll_height = textarea.scroll_height();
                let _ = style.set_property("height", &format!("{}px", scroll_height));
            }
        }
    }
}