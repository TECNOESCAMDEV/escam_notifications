//! Static text editor module: helpers for caret/index conversions, inline image tag detection,
//! toast notifications, and utility constructors used by the component.
//!
//! This module groups small, focused functions that support the editor UX:
//! - Map between byte indices and UTF-16 indices as reported by browser textareas.
//! - Detect an inline image tag like `[img:<id>]` at the current caret position.
//! - Show short-lived toast messages (Spanish, the app's user language) in the browser.
//! - Provide a ready-to-use empty `Template` instance for new documents.
//!
//! Notes
//! - The editor relies on browser selection indices, which are measured in UTF-16 code units.
//!   Rust strings are UTF-8 in memory; the helpers below bridge that gap.
//! - Inline images are represented in text as `[img:<id>]` and later resolved to real `<img>`
//!   nodes during preview rendering.

use regex::Regex;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

/// Returns the `[img:<id>]` tag identifier if the caret (UTF-16 units) is currently
/// positioned inside such a tag in `text`.
///
/// Parameters
/// - `text`: The full editor content.
/// - `cursor_pos_utf16`: Caret position measured in UTF-16 code units (as returned by
///   `HTMLTextAreaElement.selectionStart/selectionEnd`).
///
/// Returns
/// - `Some(String)` with the `id` captured from `[img:<id>]` when the caret lies within the tag
///   bounds; otherwise `None`.
///
/// Implementation detail
/// - The caret position is converted to a UTF-8 byte index to match Rust's string slicing.
/// - Detection uses the regex `\[img:([^]]+)]` and checks whether the caret byte index
///   falls within a match's bounds.
///
/// Example
/// ```ignore
/// let text = "Hello [img:123] world";
/// // If the caret is anywhere from the opening '[' to the closing ']',
/// // this function returns Some("123".to_string()).
/// ```
// language: rust
pub fn get_img_tag_id_at_cursor(text: &str, cursor_pos_utf16: usize) -> Option<String> {
    // Safely convert UTF-16 position to UTF-8 byte index.
    let mut utf16_count = 0usize;
    let mut cursor_pos_byte = text.len();
    for (byte_idx, ch) in text.char_indices() {
        let ch_utf16_len = ch.len_utf16();
        if utf16_count + ch_utf16_len > cursor_pos_utf16 {
            cursor_pos_byte = byte_idx;
            break;
        }
        utf16_count += ch_utf16_len;
    }

    let cursor_pos_byte = cursor_pos_byte.min(text.len());

    let re = Regex::new(r"\[img:([^]]+)]").unwrap();
    for mat in re.captures_iter(text) {
        let m = mat.get(0).unwrap();
        let start = m.start();
        let end = m.end();

        // Make `start` and `end` exclusive: the cursor immediately before '['
        // or immediately after ']' is no longer considered "inside".
        if cursor_pos_byte > start && cursor_pos_byte < end {
            return mat.get(1).map(|id| id.as_str().to_string());
        }
    }
    None
}

/// Converts a UTF-8 byte index within `s` to the corresponding UTF-16 code unit index.
///
/// This is the inverse of the caret conversion used by browsers and is handy when
/// you need to set `selectionStart/selectionEnd` after computing positions in bytes.
///
/// Parameters
/// - `s`: The original string.
/// - `byte_idx`: A valid byte boundary into `s`.
///
/// Returns
/// - The count of UTF-16 code units that represent `s[..byte_idx]`.
pub fn byte_to_utf16_idx(s: &str, byte_idx: usize) -> u32 {
    s[..byte_idx].encode_utf16().count() as u32
}

/// Shows a temporary toast at the bottom center of the page with a dark background.
///
/// Characteristics
/// - Visual only; no ARIA roles are added.
/// - Disappears automatically after ~3 seconds.
/// - Message content is expected to be in Spanish (app audience), but this helper does not
///   enforce any language constraints.
pub fn show_toast(message: &str) {
    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document() {
            let toast = document.create_element("div").unwrap();
            toast.set_inner_html(message);
            let html_toast = toast.dyn_into::<HtmlElement>().unwrap();
            let style = html_toast.style();
            let _ = style.set_property("position", "fixed");
            let _ = style.set_property("bottom", "20px");
            let _ = style.set_property("left", "50%");
            let _ = style.set_property("transform", "translateX(-50%)");
            let _ = style.set_property("background", "rgba(0, 0, 0, 0.8)");
            let _ = style.set_property("color", "#fff");
            let _ = style.set_property("padding", "10px 20px");
            let _ = style.set_property("border-radius", "4px");
            let _ = style.set_property("z-index", "10000");
            let _ = style.set_property("font-family", "Arial, sans-serif");
            document.body().unwrap().append_child(&html_toast).unwrap();

            let toast_clone = html_toast.clone();
            wasm_bindgen_futures::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(3000).await;
                if let Some(parent) = toast_clone.parent_node() {
                    parent.remove_child(&toast_clone).ok();
                }
            });
        }
    }
}

/// Creates a brand-new `Template` with a random id, empty text, and no images.
///
/// Used when the editor starts without a template id or when loading fails, so users can
/// immediately begin typing and save later.
pub fn create_empty_template() -> common::model::template::Template {
    common::model::template::Template {
        id: uuid::Uuid::new_v4().to_string(),
        text: String::new(),
        images: None,
    }
}


/// Helper to escape HTML special characters to avoid XSS when injecting decoded content.
/// This is a simple replacer for the common characters.
pub fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}