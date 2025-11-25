//! Component state and utilities for the static text editor.
//!
//! This module defines the state struct that holds the editor's runtime data
//! (text, undo/redo history, DOM refs, template model, and PDF viewer state),
//! along with small helper methods used by the view and update logic.
//!
//! The inline documentation describes each public field and the contract of
//! the `resize_textarea` helper.

use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlTextAreaElement};
use yew::prelude::*;

use common::model::template::Template;

/// Main state container for the `StaticTextComponent`.
///
/// Holds the current textarea content, undo/redo history, active UI tab,
/// references to DOM nodes, the bound `Template` model, selection metadata,
/// PDF viewer state, and a flag to guard one-time initialization.
///
/// Fields are `pub` because they are accessed by `view` and `update` modules.
pub struct StaticTextComponent {
    /// Current content of the textarea (UTF-8 `String`).
    pub text: String,

    /// Linear history stack for undo/redo. Each entry is a full snapshot of `text`.
    pub history: Vec<String>,

    /// Current index into `history` pointing to the active version.
    pub history_index: usize,

    /// Active tab: either `"editor"` or `"preview"`.
    pub active_tab: String,

    /// Reference to the `<textarea>` DOM node.
    pub textarea_ref: NodeRef,

    /// Reference to the hidden file input used for image selection.
    pub file_input_ref: NodeRef,

    /// Reference to the image dialog/top-sheet container node.
    pub image_dialog_ref: NodeRef,

    /// Reference to the PDF viewer dialog/top-sheet container node.
    pub pdf_viewer_dialog_ref: NodeRef,

    /// The `Template` model partially synchronized with `text` (text + images).
    /// May be `None` until a template is created or loaded.
    pub template: Option<Template>,

    /// ID of the image currently selected in the image dialog (if any).
    pub selected_image_id: Option<String>,

    /// Temporary URL used by the PDF iframe when showing the generated PDF.
    pub pdf_url: Option<String>,

    /// Flag indicating whether the PDF is being generated/loaded (shows overlay).
    pub pdf_loading: bool,

    /// Guard to avoid running first-render initialization more than once.
    pub loaded: bool,

    /// MD5 checksum of the text at last successful save. Used for dirty tracking.
    pub original_md5: Option<String>,
}

impl StaticTextComponent {
    /// Constructs a new instance with sensible defaults:
    /// - empty `text`
    /// - `history` initialized with one empty entry
    /// - `active_tab` set to `"editor"`
    /// - empty `NodeRef`s
    /// - no `template` loaded
    /// - PDF-related fields cleared
    /// - `loaded` false and `original_md5` none
    ///
    /// Guarantees a consistent initial state for the UI and undo/redo logic.
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
            pdf_loading: false,
            loaded: false,
            original_md5: None,
        }
    }

    /// Adjusts the textarea CSS height to match its `scrollHeight`, producing
    /// an auto-growing textarea that avoids internal scrollbars.
    ///
    /// Behavior:
    /// - Obtains the element from `textarea_ref`.
    /// - Resets `height` to `"auto"` to recalculate `scrollHeight`.
    /// - Sets the style `height` to the computed `scrollHeight` in pixels.
    ///
    /// This method is safe: it checks presence and types before manipulating styles.
    pub fn resize_textarea(&self) {
        if let Some(textarea) = self.textarea_ref.cast::<HtmlTextAreaElement>() {
            if let Ok(html_elem) = textarea.clone().dyn_into::<HtmlElement>() {
                // Force height recalculation
                let style = html_elem.style();
                let _ = style.set_property("height", "auto");
                let scroll_height = textarea.scroll_height();
                let _ = style.set_property("height", &format!("{}px", scroll_height));
            }
        }
    }
}
