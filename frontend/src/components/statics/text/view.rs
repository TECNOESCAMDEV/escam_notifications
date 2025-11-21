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

/// Top-level view function orchestrating the smaller helpers.
///
/// Calls `compute_preview_html` and composes the toolbar, tab bar and the
/// currently active pane (editor or preview).
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

/// Build the left toolbar with formatting buttons, image and CSV helper.
///
/// Uses `icon_button`, `CsvDataSourceComponent` and forwards events via `link`.
fn build_toolbar(component: &StaticTextComponent, link: &Scope<StaticTextComponent>) -> Html {
    // Determine if the text is dirty compared to original MD5
    let dirty = component
        .original_md5
        .as_ref()
        .map_or(false, |orig| orig != &compute_md5(&component.text));

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
            { icon_button("save", "Guardar", link.callback(|_| Msg::Save), false) }
            // Dirty indicator
            {
                if dirty {
                    html! {
                        <span
                            title="Cambios sin guardar"
                            style="display:inline-block;width:10px;height:10px;background:#e53935;border-radius:50%;margin-left:8px;vertical-align:middle;"
                        />
                    }
                } else {
                    html! {}
                }
            }
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

/// Create a style application callback for the toolbar.
///
/// `link` is the component `Scope` and `style` is the style name to apply.
fn make_style_callback(
    link: &Scope<StaticTextComponent>,
    style: &'static str,
) -> Callback<MouseEvent> {
    let s = style.to_string();
    link.callback(move |_| Msg::ApplyStyle(s.clone(), ()))
}

/// Build the tab bar switching between Editor and Preview.
///
/// Uses `component.active_tab` and `link` to dispatch `Msg::SetTab`.
fn build_tab_bar(component: &StaticTextComponent, link: &Scope<StaticTextComponent>) -> Html {
    html! {
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
    }
}

/// Build the editor tab UI: line numbers, textarea with protections and image dialog.
///
/// Preserves the original textarea callbacks and behaviour but scoped inside this helper.
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
        </>
    }
}

/// Build the preview tab HTML wrapper.
///
/// Receives precomputed `preview_html` and returns the preview container.
fn build_preview_tab(preview_html: AttrValue) -> Html {
    html! {
        <div class="markdown-preview">{ Html::from_html_unchecked(preview_html) }</div>
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
///
/// The end index is exclusive.
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
/// Pipeline:
/// 1. Normalize line endings and trim invisible characters at the start.
/// 2. Apply a single-newline preservation trick to encourage `pulldown_cmark` to keep single line breaks.
/// 3. Replace `ph` placeholders with temporary tokens and store safe HTML snippets.
/// 4. Compress multiple-newline runs into markers so they survive markdown parsing.
/// 5. Parse with `pulldown_cmark`.
/// 6. Expand newline markers into repeated `<br>` tags.
/// 7. Reinstate safe placeholder HTML snippets.
/// 8. Resolve inline template images (`[img:<id>]`) into `data:` URLs.
use uuid::Uuid;
use yew::html::Scope;
use yew::virtual_dom::AttrValue;


/// Normalize line endings and trim invisible characters at the start.
///
/// - Converts CRLF and CR to LF.
/// - Removes BOM / zero-width spaces at the beginning.
fn normalize_text(input: &str) -> String {
    input
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_start_matches(|c: char| c == '\u{feff}' || c == '\u{200b}')
        .to_string()
}

/// Apply the "preserve single-newline spacing" trick used before parsing.
///
/// Replaces each `\n` with ` \n` to encourage pulldown_cmark to keep single newlines.
fn preserve_single_newline_trick(input: &str) -> String {
    input.replace("\n", " \n")
}

/// Find `[ph:TITLE:BASE64]` placeholders and replace them with temporary unique tokens.
///
/// Returns the transformed text and a vector of `(token, html_snippet)` replacements.
/// The HTML snippets are already escaped and considered safe for reinsertion.
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

/// Compress sequences of 2+ newlines into markers of the form `\nBR_MARKER{N}\n`.
///
/// The count N equals the number of newlines compressed so it can later expand to N `<br>` tags.
fn compress_multiple_newlines(input: &str) -> String {
    let re = Regex::new(r"(\n\s*){2,}").unwrap();
    re.replace_all(input, |caps: &regex::Captures| {
        let count = caps[0].matches('\n').count();
        format!("\nBR_MARKER{}\n", count)
    })
        .into_owned()
}

/// Parse markdown text into HTML using pulldown_cmark.
fn parse_markdown_to_html(input: &str) -> String {
    let parser = Parser::new(input);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Expand `BR_MARKER{N}` placeholders into repeated `<br>` tags.
fn expand_br_markers(input: &str) -> String {
    let re_br = Regex::new(r"BR_MARKER(\d+)").unwrap();
    re_br
        .replace_all(input, |caps: &regex::Captures| {
            let n = caps[1].parse::<usize>().unwrap_or(1);
            "<br>".repeat(n)
        })
        .into_owned()
}

/// Replace previously generated PH... tokens with their safe HTML snippets.
fn replace_tokens_with_html(mut html: String, replacements: &[(String, String)]) -> String {
    for (token, snippet) in replacements {
        html = html.replace(token, snippet);
    }
    html
}

/// Resolve inline template images of the form `[img:<id>]` into data URLs.
///
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

/// Top-level orchestrator: reconstructs the original `compute_preview_html` behavior by
/// composing the smaller helpers.
///
/// Returns an `AttrValue` suitable to pass to Yew.
pub fn compute_preview_html(component: &StaticTextComponent) -> AttrValue {
    // 1. Normalize and trim
    let text = normalize_text(&component.text);

    // 2. Preserve single-newline trick
    let text = preserve_single_newline_trick(&text);

    // 3. Replace placeholders with tokens
    let (text, replacements) = replace_ph_placeholders(&text);

    // 4. Compress multiple newlines into markers
    let marked_text = compress_multiple_newlines(&text);

    // 5. Parse markdown to HTML
    let parsed_html = parse_markdown_to_html(&marked_text);

    // 6. Expand BR markers into <br>
    let expanded_html = expand_br_markers(&parsed_html);

    // 7. Replace placeholder tokens with safe HTML
    let replaced_html = replace_tokens_with_html(expanded_html, &replacements);

    // 8. Resolve inline images
    let final_html = resolve_inline_images(replaced_html, component);

    AttrValue::from(final_html)
}
