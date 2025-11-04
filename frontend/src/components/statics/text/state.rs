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
    pub template: Option<Template>,
    pub selected_image_id: Option<String>,
    pub loaded: bool,
}

impl StaticTextComponent {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            history: vec![String::new()],
            history_index: 0,
            active_tab: "editor".to_string(),
            textarea_ref: Default::default(),
            file_input_ref: Default::default(),
            image_dialog_ref: Default::default(),
            template: None,
            selected_image_id: None,
            loaded: false,
        }
    }

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