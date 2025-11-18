//! View rendering for the static text editor component.
//!
//! The UI is split across two tabs: "Editor" (a growing `<textarea>`) and
//! "Previsualización" (a markdown preview). A simple icon toolbar provides
//! formatting actions and image insertion. Inline images are represented by
//! `[img:<id>]` tags in the text, resolved to `<img>` elements in the preview.
//!
//! Notes
//! - All user-facing messages remain in Spanish by design.
//! - The preview pipeline performs a whitespace-preserving trick: runs of multiple
//!   newlines are temporarily replaced, parsed by `pulldown_cmark`, then expanded
//!   into repeated `<br>` tags to emulate a loose paragraph style.

use super::helpers::{escape_html, get_img_tag_id_at_cursor};
use super::messages::Msg;
use super::state::StaticTextComponent;
use crate::components::data_sources::csv::CsvDataSourceComponent;
use crate::components::statics::text::dialogs::image::image_dialog;
use base64::engine::general_purpose;
use base64::Engine;
use common::model::csv::ColumnCheck;
use pulldown_cmark::{html, Parser};
use regex::Regex;
use wasm_bindgen::JsCast;
use web_sys::{HtmlTextAreaElement, InputEvent};
use yew::prelude::*;

/// Top-level view function called by the component's `view()` method.
pub fn view(component: &StaticTextComponent, ctx: &Context<StaticTextComponent>) -> Html {
    let link = ctx.link();
    let preview_html = compute_preview_html(component);

    let make_style_callback =
        |style: &'static str| link.callback(move |_| Msg::ApplyStyle(style.to_string(), ()));

    let line_count = component.text.lines().count().max(1);
    let line_numbers = (1..=line_count)
        .map(|n| html! { <div class="line-number">{n}</div> })
        .collect::<Html>();

    html! {
        <div class="static-text-root">
            <div class="icon-toolbar">
                {icon_button("undo", "Deshacer", link.callback(|_| Msg::Undo), false)}
                {icon_button("redo", "Rehacer", link.callback(|_| Msg::Redo), false)}
                {icon_button("text_fields", "Normal", make_style_callback("normal"), false)}
                {icon_button("format_bold", "Negrita", make_style_callback("bold"), false)}
                {icon_button("format_italic", "Cursiva", make_style_callback("italic"), false)}
                {icon_button("format_bold", "Negrita+Cursiva", make_style_callback("bolditalic"), true)}
                {icon_button("format_list_bulleted", "Items", make_style_callback("bulleted_list"), false)}
                {icon_button("image", "Imagen", link.callback(|_| Msg::OpenFileDialog), false)}
                {icon_button("save", "Guardar", link.callback(|_| Msg::Save), false)}
                <div>
                    <CsvDataSourceComponent
                        template_id={component.template.as_ref().and_then(|t| Some(t.id.clone()))}
                        on_column_selected={link.callback(|col_check| Msg::InsertCsvColumnPlaceholder(col_check))}
                        on_csv_changed={link.callback(|cols: Vec<ColumnCheck>| Msg::CsvColumnsUpdated(cols))}
                    />
                </div>
            </div>

            <div class="tab-bar">
                <button
                    class={classes!("tab-btn", if component.active_tab == "editor" { "active" } else { "" })}
                    onclick={link.callback(|_| Msg::SetTab("editor".to_string()))}
                >{"Editor"}</button>
                <button
                    class={classes!("tab-btn", if component.active_tab == "preview" { "active" } else { "" })}
                    onclick={link.callback(|_| Msg::SetTab("preview".to_string()))}
                >{"Previsualización"}</button>
            </div>

            {
                if component.active_tab == "editor" {
                    html! {
                        <>
                            <div style="display: flex; align-items: flex-start;">
                                <div
                                    class="line-numbers"
                                    style="user-select:none; text-align:right; padding:8px 4px 8px 0; color:#aaa; background:#fafafa; font-size:11px; font-family:monospace; min-width:32px;"
                                >
                                    { line_numbers }
                                </div>
                                <textarea
                                    id="static-textarea"
                                    ref={component.textarea_ref.clone()}
                                    value={component.text.clone()}
                                    spellcheck="false"
                                    oninput={link.batch_callback(|e: InputEvent| {
                                        let value = e.target_unchecked_into::<HtmlTextAreaElement>().value();
                                        vec![ Msg::UpdateText(value), Msg::AutoResize ]
                                    })}
                                    onscroll={link.callback(|_: Event| {
                                        Msg::AutoResize
                                    })}
                                    onkeydown={link.batch_callback(|e: KeyboardEvent| {
                                        let textarea = e.target_unchecked_into::<HtmlTextAreaElement>();
                                        let text = textarea.value();
                                        let cursor_pos = textarea.selection_start().unwrap_or(Some(0)).unwrap_or(0) as usize;
                                        let arrow_keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];

                                        // Check if cursor is inside a [ph:...] placeholder
                                        if let Some((start, end)) = get_ph_bounds_at_cursor(&text, cursor_pos) {
                                            // If it's the Delete key, remove the whole placeholder
                                            if e.key() == "Delete" {
                                                e.prevent_default();
                                                let mut new_text = String::with_capacity(text.len());
                                                new_text.push_str(&text[..start]);
                                                new_text.push_str(&text[end..]);
                                                return vec![ Msg::UpdateText(new_text), Msg::AutoResize ];
                                            } else if !arrow_keys.contains(&e.key().as_str()) {
                                                // Block any other key (including typing, backspace, etc.)
                                                e.prevent_default();
                                                return vec![];
                                            }
                                        }

                                        // Existing image-tag protection logic
                                        let arrow_keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];
                                        if get_img_tag_id_at_cursor(&text, cursor_pos).is_some() && !arrow_keys.contains(&e.key().as_str()) {
                                            e.prevent_default();
                                            vec![]
                                        } else if e.ctrl_key() && e.key() == "z" {
                                            vec![Msg::Undo]
                                        } else if e.ctrl_key() && e.key() == "y" {
                                            vec![Msg::Redo]
                                        } else {
                                            vec![]
                                        }
                                    })}
                                    onselect={link.callback(|e: Event| {
                                        let id_opt = e.target()
                                            .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
                                            .and_then(|textarea| {
                                                let text = textarea.value();
                                                let cursor_pos = textarea.selection_start().unwrap_or(Some(0)).unwrap_or(0) as usize;
                                                get_img_tag_id_at_cursor(&text, cursor_pos)
                                            });
                                        match id_opt {
                                            Some(id) => Msg::OpenImageDialogWithId(id),
                                            None => Msg::AutoResize,
                                        }
                                    })}
                                    rows={1}
                                    style="width: 100%; min-height: 40px; resize: none; overflow: hidden;"
                                />
                            </div>
                            { image_dialog(component, link) }
                        </>
                    }
                } else {
                    html! { <div class="markdown-preview">{ Html::from_html_unchecked(preview_html.clone()) }</div> }
                }
            }
        </div>
    }
}

/// Small helper to render a toolbar button with a Material icon and a label.
fn icon_button(icon_name: &str, label: &str, on_click: Callback<MouseEvent>, wide: bool) -> Html {
    let class = if wide { "icon-btn wide" } else { "icon-btn" };
    html! {
        <button class={class} onclick={on_click.clone()}>
            <i class="material-icons">{icon_name}</i>
            <span class="icon-label">{label}</span>
        </button>
    }
}

/// Return `(start, end)` byte indexes of a `[ph:...:BASE64]` placeholder that
/// contains `cursor_pos`, or `None` if cursor is outside.
fn get_ph_bounds_at_cursor(text: &str, cursor_pos: usize) -> Option<(usize, usize)> {
    let pos = cursor_pos.min(text.len());
    // Search backwards for the last "[ph:" before or at cursor
    if let Some(start) = text[..pos].rfind("[ph:") {
        // Find the next closing bracket after start
        if let Some(rel_end) = text[start..].find(']') {
            let end = start + rel_end + 1; // end is exclusive
            // Ensure cursor is actually inside the found span (end is exclusive)
            if pos >= start && pos < end {
                return Some((start, end));
            }
        }
    }
    None
}

/// Produces the HTML used by the preview tab.
///
/// Steps
/// 1. Replace raw newlines with " \n" to help the Markdown parser preserve spacing.
/// 2. Compress 2+ newline sequences into a temporary marker (e.g., "br3").
/// 3. Parse the marked text with `pulldown_cmark` to HTML.
/// 4. Expand markers back into repeated `<br>` tags.
/// 5. Replace `[img:<id>]` placeholders with `<img src="data:...">` for template images.
/// 6. Replace `[ph:TITLE:BASE64]` placeholders by decoding BASE64 and inserting an escaped span.
fn compute_preview_html(component: &StaticTextComponent) -> AttrValue {
    // Preserve single-newline spacing trick
    let text_with_spaces = component.text.replace("\n", " \n");

    // Use a stricter regex and generate alphanumeric-only tokens using UUIDs
    let ph_re = Regex::new(r"\[ph:([^:\]]+):([A-Za-z0-9+/=]+)]").unwrap();
    let mut replacements: Vec<(String, String)> = Vec::new();

    let text_with_tokens = ph_re
        .replace_all(&text_with_spaces, |caps: &regex::Captures| {
            let title = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let b64 = caps.get(2).map(|m| m.as_str()).unwrap_or("");

            // Build the safe span HTML (escaped content)
            let replacement_html = match general_purpose::STANDARD.decode(b64) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(decoded) => {
                        let unquoted = match serde_json::from_str::<serde_json::Value>(&decoded) {
                            Ok(serde_json::Value::String(s)) => s,
                            _ => decoded,
                        };
                        let title_esc = escape_html(title);
                        let decoded_esc = escape_html(&unquoted);
                        format!(r#"<span title="{}">{}</span>"#, title_esc, decoded_esc)
                    }
                    Err(_) => r#"<span>[invalid utf8]</span>"#.to_string(),
                },
                Err(_) => r#"<span>[invalid base64]</span>"#.to_string(),
            };

            // Generate a safe alphanumeric token using a UUID (no dashes)
            let uuid = uuid::Uuid::new_v4().simple().to_string();
            let token = format!("PH{}", uuid); // e.g. PHa1b2c3...
            replacements.push((token.clone(), replacement_html));
            token
        })
        .into_owned();

    // Compress multiple newlines into temporary markers
    let re = Regex::new(r"(\n\s*){2,}").unwrap();
    let marked_text = re.replace_all(&text_with_tokens, |caps: &regex::Captures| {
        let count = caps[0].matches('\n').count();
        format!("br{}", count)
    });

    // Parse markdown normally
    let parser = Parser::new(&marked_text);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    // Expand temporary br markers back into repeated <br> tags
    let re_br = Regex::new(r"br(\d+)").unwrap();
    let mut final_html = re_br
        .replace_all(&html_output, |caps: &regex::Captures| {
            let n = caps[1].parse::<usize>().unwrap_or(1);
            "<br>".repeat(n)
        })
        .into_owned();

    // Replace tokens with their corresponding safe span HTML
    for (token, html_snippet) in &replacements {
        final_html = final_html.replace(token, html_snippet);
    }

    // Resolve inline images `[img:<id>]` into real <img> tags
    if let Some(template) = &component.template {
        if let Some(images) = &template.images {
            for image in images {
                let img_tag = format!("[img:{}]", image.id);
                let img_html = format!(
                    r#"<img src="data:image/*;base64,{}" style="max-width:200px;max-height:200px;vertical-align:middle;" />"#,
                    image.base64
                );
                final_html = final_html.replace(&img_tag, &img_html);
            }
        }
    }

    AttrValue::from(final_html)
}