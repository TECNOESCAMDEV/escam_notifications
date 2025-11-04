use regex::Regex;
use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

pub fn get_img_tag_id_at_cursor(text: &str, cursor_pos_utf16: usize) -> Option<String> {
    let cursor_pos_byte = text
        .encode_utf16()
        .take(cursor_pos_utf16)
        .map(|c| char::from_u32(c as u32).unwrap().len_utf8())
        .sum::<usize>();

    let re = Regex::new(r"\[img:([^]]+)]").unwrap();
    for mat in re.captures_iter(text) {
        let m = mat.get(0).unwrap();
        let start = m.start();
        let end = m.end();
        if cursor_pos_byte >= start && cursor_pos_byte <= end {
            return mat.get(1).map(|id| id.as_str().to_string());
        }
    }
    None
}

pub fn byte_to_utf16_idx(s: &str, byte_idx: usize) -> u32 {
    s[..byte_idx].encode_utf16().count() as u32
}

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

pub fn create_empty_template() -> common::model::template::Template {
    common::model::template::Template {
        id: uuid::Uuid::new_v4().to_string(),
        text: String::new(),
        images: None,
    }
}