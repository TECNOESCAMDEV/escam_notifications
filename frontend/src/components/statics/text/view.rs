//! View rendering for the static text editor component.
//!
//! The UI is split across two tabs: "Editor" (a growing `<textarea>`) and
//! "Preview" (a markdown preview). A simple icon toolbar provides formatting
//! actions and image insertion. Inline images are represented by `[img:<id>]`
//! tags in the text and are resolved to `<img>` elements in the preview.
//!
//! Notes
//! - All user-facing messages remain in Spanish by design.
//! - The preview pipeline performs a whitespace-preserving trick: runs of
//!   multiple newlines are temporarily replaced by markers, parsed by
//!   `pulldown_cmark`, then expanded into repeated `<br>` tags to emulate a
//!   loose paragraph style while preserving single newlines.

use super::helpers::{compute_md5, escape_html, get_img_tag_id_at_cursor};
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

/// Main view function for the static text editor component.
/// Renders the toolbar, tab bar, and the active pane (editor or preview).
pub fn view(component: &StaticTextComponent, ctx: &Context<StaticTextComponent>) -> Html {
    let link = ctx.link();
    let preview_html = compute_preview_html(component);

    html! {
        <div class="static-text-root">
            { build_toolbar(component, link) }
            { build_tab_bar(component, link) }

            {
                if component.active_tab == "editor" {
                    build_editor_tab(component, link)
                } else {
                    build_preview_tab(preview_html)
                }
            }
        </div>
    }
}

/// Builds the left toolbar with formatting buttons, image and CSV helper.
/// Uses `icon_button`, `CsvDataSourceComponent` and forwards events via `link`.
fn build_toolbar(component: &StaticTextComponent, link: &Scope<StaticTextComponent>) -> Html {
    html! {
        <div class="icon-toolbar">
            { icon_button("undo", "Deshacer", link.callback(|_| Msg::Undo), false) }
            { icon_button("redo", "Rehacer", link.callback(|_| Msg::Redo), false) }
            { icon_button("text_fields", "Normal", make_style_callback(link, "normal"), false) }
            { icon_button("format_bold", "Negrita", make_style_callback(link, "bold"), false) }
            { icon_button("format_italic", "Cursiva", make_style_callback(link, "italic"), false) }
            { icon_button("format_bold", "Negrita+Cursiva", make_style_callback(link, "bolditalic"), true) }
            { icon_button("format_list_bulleted", "Items", make_style_callback(link, "bulleted_list"), false) }
            { icon_button("image", "Imagen", link.callback(|_| Msg::OpenFileDialog), false) }
            { icon_button("picture_as_pdf", "PDF", link.callback(|_| Msg::OpenPdf), false) }
            { icon_button("save", "Guardar", link.callback(|_| Msg::Save), false) }
            <div>
                <CsvDataSourceComponent
                    template_id={component.template.as_ref().and_then(|t| Some(t.id.clone()))}
                    on_column_selected={link.callback(|col_check| Msg::InsertCsvColumnPlaceholder(col_check))}
                    on_csv_changed={link.callback(|cols: Vec<ColumnCheck>| Msg::CsvColumnsUpdated(cols))}
                />
            </div>
        </div>
    }
}

/// Creates a style application callback for the toolbar.
/// `link` is the component `Scope` and `style` is the style name to apply.
fn make_style_callback(
    link: &Scope<StaticTextComponent>,
    style: &'static str,
) -> Callback<MouseEvent> {
    let s = style.to_string();
    link.callback(move |_| Msg::ApplyStyle(s.clone(), ()))
}

/// Builds the tab bar for switching between Editor and Preview.
/// Shows a red dot if there are unsaved changes.
fn build_tab_bar(component: &StaticTextComponent, link: &Scope<StaticTextComponent>) -> Html {
    let dirty = component
        .original_md5
        .as_ref()
        .map_or(false, |orig| orig != &compute_md5(&component.text));

    html! {
        <div class="tab-bar">
            <button
                class={classes!("tab-btn", if component.active_tab == "editor" { "active" } else { "" })}
                onclick={link.callback(|_| Msg::SetTab("editor".to_string()))}
                style="position: relative;"
            >
                {"Editor"}
                {
                    if dirty {
                        html! {
                            <span
                                title="Cambios sin guardar"
                                style="
                                        position: absolute;
                                        top: 4px;
                                        right: 6px;
                                        width: 8px;
                                        height: 8px;
                                        background: #e53935;
                                        border-radius: 50%;
                                        display: inline-block;
                                        vertical-align: middle;
                                    "
                            />
                        }
                    } else {
                        html! {}
                    }
                }
            </button>
            <button
                class={classes!("tab-btn", if component.active_tab == "preview" { "active" } else { "" })}
                onclick={link.callback(|_| Msg::SetTab("preview".to_string()))}
            >
                {"Previsualización"}
            </button>
        </div>
    }
}

/// Builds the editor tab UI: line numbers, textarea, and image dialog.
/// Handles textarea events and protections for placeholders and image tags.
fn build_editor_tab(component: &StaticTextComponent, link: &Scope<StaticTextComponent>) -> Html {
    let line_count = component.text.lines().count().max(1);
    let line_numbers = (1..=line_count)
        .map(|n| html! { <div class="line-number">{n}</div> })
        .collect::<Html>();

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
                    onscroll={link.callback(|_: Event| Msg::AutoResize)}
                    onkeydown={link.batch_callback(|e: KeyboardEvent| {
                        let textarea = e.target_unchecked_into::<HtmlTextAreaElement>();
                        let text = textarea.value();
                        let cursor_pos = textarea.selection_start().unwrap_or(Some(0)).unwrap_or(0) as usize;
                        let arrow_keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];

                        // Protect [ph:...] placeholders
                        if let Some((start, end)) = get_ph_bounds_at_cursor(&text, cursor_pos) {
                            if e.key() == "Delete" {
                                e.prevent_default();
                                let mut new_text = String::with_capacity(text.len());
                                new_text.push_str(&text[..start]);
                                new_text.push_str(&text[end..]);
                                return vec![ Msg::UpdateText(new_text), Msg::AutoResize ];
                            } else if !arrow_keys.contains(&e.key().as_str()) {
                                e.prevent_default();
                                return vec![];
                            }
                        }

                        // Protect inline image tags
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
            { pdf_dialog(component, link) }
        </>
    }
}

/// Builds the preview tab HTML wrapper.
/// Receives precomputed `preview_html` and returns the preview container.
fn build_preview_tab(preview_html: AttrValue) -> Html {
    html! {
        <div class="markdown-preview">{ Html::from_html_unchecked(preview_html) }</div>
    }
}

/// Renders a toolbar button with a Material icon and a label.
fn icon_button(icon_name: &str, label: &str, on_click: Callback<MouseEvent>, wide: bool) -> Html {
    let class = if wide { "icon-btn wide" } else { "icon-btn" };
    html! {
        <button class={class} onclick={on_click.clone()}>
            <i class="material-icons">{icon_name}</i>
            <span class="icon-label">{label}</span>
        </button>
    }
}

/// Returns `(start, end)` byte indexes of a `[ph:...:BASE64]` placeholder containing `cursor_pos`, or `None` if cursor is outside.
/// The end index is exclusive.
fn get_ph_bounds_at_cursor(text: &str, cursor_pos: usize) -> Option<(usize, usize)> {
    let pos = cursor_pos.min(text.len());
    if let Some(start) = text[..pos].rfind("[ph:") {
        if let Some(rel_end) = text[start..].find(']') {
            let end = start + rel_end + 1;
            if pos >= start && pos < end {
                return Some((start, end));
            }
        }
    }
    None
}

use crate::components::statics::text::dialogs::pdf::pdf_dialog;
use uuid::Uuid;
use yew::html::Scope;
use yew::virtual_dom::AttrValue;

/// Normalizes line endings and trims invisible characters at the start.
/// Converts CRLF and CR to LF. Removes BOM / zero-width spaces at the beginning.
fn normalize_text(input: &str) -> String {
    input
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_start_matches(|c: char| c == '\u{feff}' || c == '\u{200b}')
        .to_string()
}

/// Preserves single-newline spacing before markdown parsing.
/// Replaces each `\n` with ` \n` to encourage pulldown_cmark to keep single newlines.
fn preserve_single_newline_trick(input: &str) -> String {
    input.replace("\n", " \n")
}

/// Finds `[ph:TITLE:BASE64]` placeholders and replaces them with unique tokens.
/// Returns the transformed text and a vector of `(token, html_snippet)` replacements.
/// The HTML snippets are escaped and safe for reinsertion.
fn replace_ph_placeholders(input: &str) -> (String, Vec<(String, String)>) {
    let ph_re = Regex::new(r"\[ph:([^:\]]+):([A-Za-z0-9+/=]+)]").unwrap();
    let mut replacements: Vec<(String, String)> = Vec::new();

    let text_with_tokens = ph_re
        .replace_all(input, |caps: &regex::Captures| {
            let title = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let b64 = caps.get(2).map(|m| m.as_str()).unwrap_or("");

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

            let uuid = Uuid::new_v4().simple().to_string();
            let token = format!("PH{}", uuid);
            replacements.push((token.clone(), replacement_html));
            token
        })
        .into_owned();

    (text_with_tokens, replacements)
}

/// Parses markdown text into HTML using pulldown_cmark.
fn parse_markdown_to_html(input: &str) -> String {
    let parser = Parser::new(input);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Expands `BR_MARKER{N}` placeholders into repeated `<br>` tags.
fn expand_br_markers(input: &str) -> String {
    let re_br = Regex::new(r"BR_MARKER(\d+)").unwrap();
    re_br
        .replace_all(input, |caps: &regex::Captures| {
            let n = caps[1].parse::<usize>().unwrap_or(1);
            "<br>".repeat(n)
        })
        .into_owned()
}

/// Replaces previously generated PH... tokens with their safe HTML snippets.
fn replace_tokens_with_html(mut html: String, replacements: &[(String, String)]) -> String {
    for (token, snippet) in replacements {
        html = html.replace(token, snippet);
    }
    html
}

/// Resolves inline template images of the form `[img:<id>]` into data URLs.
/// Uses `component.template.images` to find matches and substitute with `<img ... />` elements.
fn resolve_inline_images(mut html: String, component: &StaticTextComponent) -> String {
    if let Some(template) = &component.template {
        if let Some(images) = &template.images {
            for image in images {
                let img_tag = format!("[img:{}]", image.id);
                let img_html = format!(
                    r#"<img src="data:image/*;base64,{}" style="max-width:200px;max-height:200px;vertical-align:middle;" />"#,
                    image.base64
                );
                html = html.replace(&img_tag, &img_html);
            }
        }
    }
    html
}

/// Compresses multiple newlines outside the last bullet list block.
/// Only compresses newlines outside the bullet block so that content after the last bullet is rendered outside the `<ul>`.
fn compress_newlines_outside_bullets(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut result = String::new();
    let mut last_bullet_idx = None;

    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("- ") {
            last_bullet_idx = Some(i);
        }
    }

    let mut i = 0;
    while i < lines.len() {
        if let Some(last_idx) = last_bullet_idx {
            if i <= last_idx {
                result.push_str(lines[i]);
                result.push('\n');
                i += 1;
                continue;
            }
        }
        if lines[i].trim().is_empty() {
            let mut count = 1;
            while i + count < lines.len() && lines[i + count].trim().is_empty() {
                count += 1;
            }
            if count > 1 {
                result.push_str(&format!("\nBR_MARKER{}\n", count));
                i += count;
            } else {
                result.push('\n');
                i += 1;
            }
        } else {
            result.push_str(lines[i]);
            result.push('\n');
            i += 1;
        }
    }
    result
}

fn compress_newlines_after_styles(input: &str) -> String {
    let style_re = Regex::new(r"(\*\*.*\*\*|_.*_|`.*`)$").unwrap();
    let lines: Vec<&str> = input.lines().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < lines.len() {
        result.push_str(lines[i]);
        result.push('\n');
        if style_re.is_match(lines[i]) {
            let mut count = 0;
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().is_empty() {
                count += 1;
                j += 1;
            }
            if count > 1 {
                result.push_str(&format!("BR_MARKER{}\n", count));
                i += count;
                continue;
            }
        }
        i += 1;
    }
    result
}

/// Main orchestrator for the preview HTML pipeline.
/// Runs normalization, single-newline preservation, placeholder replacement, newline compression,
/// markdown parsing, marker expansion, placeholder reinsertion, and image resolution.
/// Returns an `AttrValue` for Yew.
pub fn compute_preview_html(component: &StaticTextComponent) -> AttrValue {
    let text = normalize_text(&component.text);
    let text = compress_newlines_outside_bullets(&text);
    let text = compress_newlines_after_styles(&text);
    let text = preserve_single_newline_trick(&text);
    let (text, replacements) = replace_ph_placeholders(&text);

    let parsed_html = parse_markdown_to_html(&text);
    let expanded_html = expand_br_markers(&parsed_html);
    let replaced_html = replace_tokens_with_html(expanded_html, &replacements);
    let final_html = resolve_inline_images(replaced_html, component);

    AttrValue::from(final_html)
}
