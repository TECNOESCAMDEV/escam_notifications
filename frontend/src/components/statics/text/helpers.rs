//! Utility functions for the static text editor component.
//!
//! This module provides a collection of helper functions that support the main
//! component logic found in `update.rs` and `view.rs`. Responsibilities include:
//!
//! - **Index Conversion**: Translating between Rust's native UTF-8 byte indices
//!   and the UTF-16 code unit indices used by browser textarea APIs (`selectionStart`,
//!   `selectionEnd`). This is crucial for accurate text manipulation.
//! - **Tag Detection**: Identifying special tags like `[img:<id>]` at the cursor's
//!   position to trigger contextual UI, such as opening an image dialog.
//! - **User Feedback**: Displaying temporary "toast" notifications to inform the
//!   user about the status of operations like saving or loading.
//! - **Model Instantiation**: Creating empty `Template` objects for new documents.
//! - **Security & Hashing**: Escaping HTML to prevent XSS in previews and computing
//!   MD5 hashes for dirty-checking unsaved changes.

use regex::Regex;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

/// Finds the ID of an `[img:<id>]` tag if the cursor is inside it.
///
/// This function is called from the `onselect` event handler in `view.rs`. When the
/// user's selection moves inside an image tag, this function extracts the image ID.
/// The view then dispatches a `Msg::OpenImageDialogWithId` message, which is handled
/// in `update.rs` to open the image management dialog.
///
/// # Arguments
/// * `text` - The full string content of the textarea.
/// * `cursor_pos_utf16` - The cursor position in UTF-16 code units, as provided by
///   `textarea.selectionStart()`.
///
/// # Returns
/// `Some(String)` containing the image ID if the cursor is within an `[img:...]` tag,
/// otherwise `None`.
pub fn get_img_tag_id_at_cursor(text: &str, cursor_pos_utf16: usize) -> Option<String> {
    // Convert UTF-16 cursor position to a UTF-8 byte index for Rust string slicing.
    let cursor_pos_byte = utf16_to_byte_idx(text, cursor_pos_utf16);

    let re = Regex::new(r"\[img:([^]]+)]").unwrap();
    for mat in re.captures_iter(text) {
        if let Some(m) = mat.get(0) {
            let (start, end) = (m.start(), m.end());
            // The cursor is "inside" if it's after the opening '[' and before the closing ']'.
            if cursor_pos_byte > start && cursor_pos_byte < end {
                return mat.get(1).map(|id| id.as_str().to_string());
            }
        }
    }
    None
}

/// Converts a UTF-8 byte index to its corresponding UTF-16 code unit index.
///
/// This is the inverse of `utf16_to_byte_idx`. It's used when a text position is
/// calculated in Rust (e.g., after inserting a placeholder) and needs to be
/// converted back to a UTF-16 index to programmatically set the cursor position
/// in the browser's textarea using `set_selection_range`.
///
/// # Arguments
/// * `s` - The string slice being measured.
/// * `byte_idx` - The UTF-8 byte index.
///
/// # Returns
/// The equivalent position in UTF-16 code units.
pub fn byte_to_utf16_idx(s: &str, byte_idx: usize) -> u32 {
    s[..byte_idx].encode_utf16().count() as u32
}

/// Converts a UTF-16 code unit index to its corresponding UTF-8 byte index.
///
/// Browser textarea APIs (`selectionStart`, `selectionEnd`) work with UTF-16
/// indices. Rust's string manipulation is based on UTF-8 bytes. This function
/// is essential for converting a browser-reported cursor position into a valid
/// byte index that can be used for slicing and manipulation within `update.rs`.
///
/// # Arguments
/// * `s` - The full string context.
/// * `utf16_idx` - The index in UTF-16 code units.
///
/// # Returns
/// The equivalent position as a UTF-8 byte index.
pub fn utf16_to_byte_idx(s: &str, utf16_idx: usize) -> usize {
    s.char_indices()
        .map(|(byte_idx, _)| byte_idx)
        .nth(s.encode_utf16().take(utf16_idx).count())
        .unwrap_or(s.len())
}

/// Displays a temporary notification message at the bottom of the screen.
///
/// This function creates and injects a styled `div` into the DOM to provide
/// non-blocking feedback to the user. It is used throughout `update.rs` and
/// `mod.rs` to confirm actions (e.g., "Plantilla guardada") or report errors
/// (e.g., "Error al guardar"). The toast automatically removes itself after a
/// few seconds.
///
/// # Arguments
/// * `message` - The text content to display in the toast.
pub fn show_toast(message: &str) {
    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document() {
            if let (Ok(toast), Some(body)) = (document.create_element("div"), document.body()) {
                toast.set_inner_html(message);
                let html_toast: HtmlElement = toast.unchecked_into();
                let style = html_toast.style();
                style.set_property("position", "fixed").ok();
                style.set_property("bottom", "20px").ok();
                style.set_property("left", "50%").ok();
                style.set_property("transform", "translateX(-50%)").ok();
                style.set_property("background", "rgba(0, 0, 0, 0.8)").ok();
                style.set_property("color", "#fff").ok();
                style.set_property("padding", "10px 20px").ok();
                style.set_property("border-radius", "4px").ok();
                style.set_property("z-index", "10000").ok();
                style.set_property("font-family", "Arial, sans-serif").ok();

                if body.append_child(&html_toast).is_ok() {
                    wasm_bindgen_futures::spawn_local(async move {
                        gloo_timers::future::TimeoutFuture::new(3000).await;
                        if let Some(parent) = html_toast.parent_node() {
                            parent.remove_child(&html_toast).ok();
                        }
                    });
                }
            }
        }
    }
}

/// Creates a new, empty `Template` instance with a unique ID.
///
/// This factory function is called from `mod.rs` during component initialization
/// if no `template_id` prop is provided, or if loading an existing template fails.
/// It ensures the editor always has a valid `Template` object to work with,
/// preventing panics and allowing the user to start creating content immediately.
pub fn create_empty_template() -> common::model::template::Template {
    common::model::template::Template {
        id: uuid::Uuid::new_v4().to_string(),
        text: String::new(),
        images: None,
    }
}

/// Escapes special HTML characters in a string.
///
/// This is a security and rendering utility used in the preview generation
/// pipeline (`view.rs`). When placeholder tags like `[ph:...]` are processed,
/// their decoded content is escaped using this function before being embedded
/// in the final HTML. This prevents content from being misinterpreted as HTML
/// tags, mitigating XSS risks.
///
/// # Arguments
/// * `input` - The raw string to escape.
///
/// # Returns
/// A new string with `&`, `<`, `>`, `"`, and `'` replaced by their respective
/// HTML entities.
pub fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Computes the MD5 hash of a string and returns it as a hex digest.
///
/// This function is central to the editor's "dirty checking" mechanism. In
/// `update.rs`, the hash of the text is stored in `original_md5` upon load or
/// save (`Msg::SetTemplate`, `Msg::SaveSucceeded`). In `view.rs`, the hash of the
/// current text is compared against `original_md5` to determine if there are
/// unsaved changes, showing or hiding a "dirty" indicator in the UI.
///
/// # Arguments
/// * `input` - The string to hash.
///
/// # Returns
/// A `String` containing the 32-character hexadecimal representation of the MD5 hash.
pub fn compute_md5(input: &str) -> String {
    format!("{:x}", md5::compute(input))
}
