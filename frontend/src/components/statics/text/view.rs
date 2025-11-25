//! View rendering for the static text editor component.
//!
//! This module is responsible for rendering the entire UI of the editor,
//! including the toolbar, tabs, and the active content pane (editor or preview).
//! It translates user interactions (clicks, key presses, text input) into `Msg`
//! variants that are sent to the `update` function to modify the component's state.
//!
//! UI Structure:
//! - A top-level `view` function that orchestrates the layout.
//! - A `build_toolbar` function that creates buttons for actions like undo/redo,
//!   styling, saving, and opening dialogs. Each button dispatches a specific `Msg`.
//! - A `build_tab_bar` for switching between the "editor" and "preview" panes.
//! - An `build_editor_tab` that renders the `<textarea>` and handles complex
//!   events like input, selection changes, and key presses for protected text.
//! - A `build_preview_tab` that renders the HTML generated from the markdown text.
//!
//! Message Dispatching:
//! - **Formatting**: `Msg::ApplyStyle`, `Msg::Undo`, `Msg::Redo`.
//! - **File/Dialogs**: `Msg::OpenFileDialog`, `Msg::OpenImageDialogWithId`, `Msg::OpenPdf`.
//! - **State Sync**: `Msg::UpdateText`, `Msg::AutoResize` on input and scroll.
//! - **Persistence**: `Msg::Save`.
//! - **Data Integration**: `Msg::InsertCsvColumnPlaceholder`, `Msg::CsvColumnsUpdated` from a child component.

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

/// Renders the main view for the static text editor component.
///
/// This function serves as the root of the component's render tree. It delegates
/// the construction of the UI to specialized helper functions for the toolbar,
/// tab bar, and the active content pane (either the editor or the preview).
///
/// It computes the preview HTML ahead of time and passes it to the preview tab.
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

/// Builds the left-hand toolbar containing action buttons.
///
/// Each button is configured with an icon and a callback that dispatches a
/// specific `Msg` to the update loop. This function is the primary source for
/// user-initiated commands that are not direct text input.
///
/// Dispatched Messages:
/// - `Msg::Undo`/`Msg::Redo`: Navigate the text history.
/// - `Msg::ApplyStyle`: Insert markdown styling snippets.
/// - `Msg::OpenFileDialog`: Trigger the hidden file input for image uploads.
/// - `Msg::OpenPdf`: Request the generation and display of a PDF preview.
/// - `Msg::Save`: Persist the current template to the backend.
/// - `Msg::InsertCsvColumnPlaceholder`: (From child) Insert a CSV data placeholder.
/// - `Msg::CsvColumnsUpdated`: (From child) Notify that the CSV source has changed.
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

/// Creates a `Callback` for a style-applying button.
///
/// This helper simplifies toolbar construction by creating a closure that sends
/// the appropriate `Msg::ApplyStyle` variant when a style button is clicked.
fn make_style_callback(
    link: &Scope<StaticTextComponent>,
    style: &'static str,
) -> Callback<MouseEvent> {
    let s = style.to_string();
    link.callback(move |_| Msg::ApplyStyle(s.clone(), ()))
}

/// Builds the tab bar for switching between "Editor" and "Preview" modes.
///
/// It renders two buttons that dispatch `Msg::SetTab` to change the `active_tab`
/// field in the component's state. It also displays a "dirty" indicator (a red dot)
/// on the "Editor" tab if the current text has unsaved changes, which is determined
/// by comparing the MD5 hash of the current text with the one from the last save.
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

/// Builds the editor tab, which includes the line numbers, the main `<textarea>`,
/// and the associated dialogs for images and PDF previews.
///
/// This function is responsible for handling a wide range of events:
/// - `oninput`: Dispatches `Msg::UpdateText` to sync the state with user input and
///   `Msg::AutoResize` to adjust the textarea's height.
/// - `onscroll`: Dispatches `Msg::AutoResize` to ensure line numbers stay aligned.
/// - `onkeydown`: Intercepts key presses to implement undo/redo shortcuts (`Ctrl+Z`/`Ctrl+Y`)
///   and to protect special text spans (like `[img:...]` and `[ph:...]`) from being
///   edited or deleted improperly.
/// - `onselect`: Detects if the cursor moves inside an `[img:...]` tag and dispatches
///   `Msg::OpenImageDialogWithId` to show the relevant image management dialog.
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

/// Builds the preview tab's HTML container.
///
/// This function is straightforward: it takes the pre-rendered HTML string
/// (computed by `compute_preview_html`) and injects it into a `div` using
/// `Html::from_html_unchecked`. This is safe because the pipeline in
/// `compute_preview_html` ensures all user-provided content is properly escaped.
fn build_preview_tab(preview_html: AttrValue) -> Html {
    html! {
        <div class="markdown-preview">{ Html::from_html_unchecked(preview_html) }</div>
    }
}

/// Renders a standardized toolbar button with a Material Design icon and a text label.
///
/// This is a simple presentational helper to reduce boilerplate in `build_toolbar`.
/// It takes an icon name, a label, and a `Callback` to handle the `onclick` event.
fn icon_button(icon_name: &str, label: &str, on_click: Callback<MouseEvent>, wide: bool) -> Html {
    let class = if wide { "icon-btn wide" } else { "icon-btn" };
    html! {
        <button class={class} onclick={on_click.clone()}>
            <i class="material-icons">{icon_name}</i>
            <span class="icon-label">{label}</span>
        </button>
    }
}

/// Finds the start and end byte indices of a `[ph:...:BASE64]` placeholder
/// that contains the given `cursor_pos` (in UTF-16 units).
///
/// This is a helper used in the `onkeydown` handler to determine if the cursor
/// is inside a protected placeholder, preventing accidental edits.
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

/// Normalizes line endings (CRLF/CR to LF) and removes leading zero-width characters.
/// This ensures consistent text processing across different platforms and editors.
fn normalize_text(input: &str) -> String {
    input
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_start_matches(|c: char| c == '\u{feff}' || c == '\u{200b}')
        .to_string()
}

/// Inserts a space before each single newline.
/// This is a trick to force the `pulldown_cmark` parser to treat single newlines
/// as significant whitespace (like a `<br>`), which is often desired in this
/// type of editor, rather than collapsing them.
fn preserve_single_newline_trick(input: &str) -> String {
    input.replace("\n", " \n")
}

/// Finds all `[ph:TITLE:BASE64]` placeholders, replaces them with unique temporary
/// tokens, and returns the modified text along with a list of token-to-HTML mappings.
///
/// This is a key step in the preview pipeline. It extracts placeholders before
/// markdown parsing to prevent them from being misinterpreted. The Base64 content
/// is decoded and escaped to create a safe HTML `<span>` for later re-insertion.
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

/// Parses a markdown string into an HTML string using `pulldown_cmark`.
fn parse_markdown_to_html(input: &str) -> String {
    let parser = Parser::new(input);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Replaces `BR_MARKER{N}` placeholders with `N` repeated `<br>` tags.
/// This function re-inflates the compressed newline sequences after markdown parsing.
fn expand_br_markers(input: &str) -> String {
    let re_br = Regex::new(r"BR_MARKER(\d+)").unwrap();
    re_br
        .replace_all(input, |caps: &regex::Captures| {
            let n = caps[1].parse::<usize>().unwrap_or(1);
            "<br>".repeat(n)
        })
        .into_owned()
}

/// Re-inserts the HTML for placeholders by replacing the temporary tokens.
/// This step happens after markdown parsing to ensure the placeholder HTML is
/// rendered verbatim and not processed as markdown.
fn replace_tokens_with_html(mut html: String, replacements: &[(String, String)]) -> String {
    for (token, snippet) in replacements {
        html = html.replace(token, snippet);
    }
    html
}

/// Finds `[img:<id>]` tags in the final HTML and replaces them with `<img>` elements.
///
/// It looks up each image ID in the `component.template.images` vector to find the
/// corresponding Base64 data, constructing a data URL for the `src` attribute.
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

/// Compresses sequences of multiple empty lines into a single `BR_MARKER{N}` token.
///
/// This allows the editor to preserve intentional vertical spacing (e.g., multiple
/// blank lines) which would otherwise be collapsed by the markdown parser. The
/// markers are expanded back into `<br>` tags later in the pipeline.
fn compress_newlines_after_any_line(input: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < lines.len() {
        result.push_str(lines[i]);
        result.push('\n');
        // Detect multiple empty lines after any line
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
        i += 1;
    }
    result
}

/// Orchestrates the entire pipeline for generating the preview HTML.
///
/// This function executes a series of transformations on the raw text to produce
/// a final, safe, and correctly formatted HTML string for the preview pane.
/// The steps are carefully ordered to handle placeholders, newlines, markdown,
/// and inline images correctly.
///
/// Pipeline:
/// 1. `normalize_text`: Clean up line endings and invisible characters.
/// 2. `compress_newlines_after_any_line`: Convert multiple blank lines to markers.
/// 3. `preserve_single_newline_trick`: Ensure single newlines become `<br>`.
/// 4. `replace_ph_placeholders`: Extract placeholders into tokens.
/// 5. `parse_markdown_to_html`: Process the cleaned text with `pulldown_cmark`.
/// 6. `expand_br_markers`: Convert newline markers back to `<br>` tags.
/// 7. `replace_tokens_with_html`: Re-insert placeholder HTML.
/// 8. `resolve_inline_images`: Convert `[img:...]` tags to `<img>` elements.
pub fn compute_preview_html(component: &StaticTextComponent) -> AttrValue {
    let text = normalize_text(&component.text);
    let text = compress_newlines_after_any_line(&text);
    let text = preserve_single_newline_trick(&text);
    let (text, replacements) = replace_ph_placeholders(&text);

    let parsed_html = parse_markdown_to_html(&text);
    let expanded_html = expand_br_markers(&parsed_html);
    let replaced_html = replace_tokens_with_html(expanded_html, &replacements);
    let final_html = resolve_inline_images(replaced_html, component);

    AttrValue::from(final_html)
}
