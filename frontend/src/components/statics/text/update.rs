//! Update function for the static text editor component.
//!
//! This module contains a single `update` function following an Elm-style architecture:
//! it receives the current `StaticTextComponent` state, the `Context`, and a `Msg`,
//! mutates the state accordingly, and returns a `bool` indicating whether the view should
//! re-render.
//!
//! Key behaviors
//! - Text editing with undo/redo history.
//! - Applying style snippets (markdown-like) at the current selection.
//! - Auto-resizing the textarea and syncing the backing `Template` model.
//! - Handling image insertion: upload -> base64 -> `[img:<uuid>]` tag -> template images list.
//! - Deleting images, which removes both the asset and its inline tag.
//! - Persisting the template via a backend POST, with user-facing toast messages (Spanish).
//! - Generating and displaying a PDF preview of the template.

use base64::{engine::general_purpose, Engine as _};
use gloo_file::{futures::read_as_bytes, Blob};
use gloo_net::http::Request;
use js_sys::Date;
use js_sys::Reflect;
use regex::Regex;
use std::collections::HashSet;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::HtmlTextAreaElement;

use yew::platform::spawn_local;
use yew::prelude::*;

use common::model::image::Image;
use common::model::template::Template;

use crate::tops_sheet::yw_material_top_sheet::{close_top_sheet, open_top_sheet};

use super::helpers::{byte_to_utf16_idx, compute_md5, show_toast};
use super::messages::Msg;
use super::state::StaticTextComponent;

/// Central update function for the component.
///
/// Contract
/// - Mutates `component` based on `msg`.
/// - May dispatch further messages via `ctx.link()` (e.g., async callbacks).
/// - Returns `true` to re-render the view, `false` to short-circuit when only side effects occur.
pub fn update(
    component: &mut StaticTextComponent,
    ctx: &Context<StaticTextComponent>,
    msg: Msg,
) -> bool {
    match msg {
        // **`UpdateText(new_text)`**: Handles user input from the textarea.
        // It updates the component's `text` state, manages the undo/redo history by
        // pushing the new text onto the history stack, and sets a global 'dirty' flag
        // to indicate unsaved changes. Returns `true` to re-render.
        Msg::UpdateText(new_text) => {
            if component.text != new_text {
                component.text = new_text.clone();
                component.history.truncate(component.history_index + 1);
                component.history.push(new_text);
                component.history_index = component.history.len() - 1;

                // Update dirty flag
                set_window_dirty_flag(component);
            }
            true
        }
        // **`Undo`**: Navigates backward in the history stack.
        // It decrements the `history_index`, updates the `text` state to the previous
        // version, and updates the dirty flag. Returns `true` to re-render.
        Msg::Undo => {
            if component.history_index > 0 {
                component.history_index -= 1;
                component.text = component.history[component.history_index].clone();
                // Update dirty flag
                set_window_dirty_flag(component);
            }
            true
        }
        // **`Redo`**: Navigates forward in the history stack.
        // It increments the `history_index`, updates the `text` state to the next
        // version, and updates the dirty flag. Returns `true` to re-render.
        Msg::Redo => {
            if component.history_index + 1 < component.history.len() {
                component.history_index += 1;
                component.text = component.history[component.history_index].clone();
                // Update dirty flag
                set_window_dirty_flag(component);
            }
            true
        }
        // **`SetTab(tab)`**: Switches the active view between "editor" and "preview".
        // If switching to the editor, it schedules an `AutoResize` message to ensure
        // the textarea height is correct. Returns `true` to re-render the new tab.
        Msg::SetTab(tab) => {
            component.active_tab = tab;
            if component.active_tab == "editor" {
                ctx.link().send_message(Msg::AutoResize);
                let link = ctx.link().clone();
                wasm_bindgen_futures::spawn_local(async move {
                    gloo_timers::future::TimeoutFuture::new(200).await;
                    link.send_message(Msg::AutoResize);
                });
                return true;
            }
            true
        }
        // **`ApplyStyle(style, _)`**: Inserts a markdown-style snippet at the cursor.
        // It wraps the selected text (or inserts a placeholder) with style markers
        // like `**` for bold or `*` for italic, then programmatically selects the
        // placeholder text for immediate editing. Updates the dirty flag. Returns `true`.
        Msg::ApplyStyle(style, _) => {
            if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                if let Some(textarea) = document
                    .get_element_by_id("static-textarea")
                    .and_then(|e| e.dyn_into::<HtmlTextAreaElement>().ok())
                {
                    let start_utf16 =
                        textarea.selection_start().unwrap_or(Some(0)).unwrap_or(0) as usize;
                    let end_utf16 =
                        textarea.selection_end().unwrap_or(Some(0)).unwrap_or(0) as usize;

                    let start = component
                        .text
                        .encode_utf16()
                        .take(start_utf16)
                        .map(|c| char::from_u32(c as u32).unwrap().len_utf8())
                        .sum();
                    let end = component
                        .text
                        .encode_utf16()
                        .take(end_utf16)
                        .map(|c| char::from_u32(c as u32).unwrap().len_utf8())
                        .sum();

                    let styled = match style.as_str() {
                        "bold" => "**texto**",
                        "italic" => "*texto*",
                        "bolditalic" => "***texto***",
                        "normal" => "texto",
                        "bulleted_list" => "- texto",
                        "image" => "[img:url]",
                        _ => "",
                    };

                    component.text = format!(
                        "{}{}{}",
                        &component.text[..start],
                        styled,
                        &component.text[end..]
                    );
                    textarea.set_value(&component.text);

                    let text_pos = component.text[start..].find("texto").unwrap_or(0) + start;
                    let select_start = byte_to_utf16_idx(&component.text, text_pos);
                    let select_end = byte_to_utf16_idx(&component.text, text_pos + 5);

                    textarea.set_selection_start(Some(select_start)).ok();
                    textarea.set_selection_end(Some(select_end)).ok();
                    textarea.focus().ok();

                    // Update dirty flag
                    set_window_dirty_flag(component);
                }
            }
            true
        }
        // **`AutoResize`**: Synchronizes component state with the textarea content.
        // It adjusts the textarea's height to fit its content, updates the `template.text`
        // model, and removes any image data from `template.images` if its corresponding
        // `[img:...]` tag is no longer in the text. Returns `false` as it's a background task.
        Msg::AutoResize => {
            component.resize_textarea();
            if let Some(template) = &mut component.template {
                template.text = component.text.clone();
                if let Some(images) = &mut template.images {
                    images.retain(|img| component.text.contains(&format!("[img:{}]", img.id)));
                }
            } else {
                component.template = Some(Template {
                    id: String::new(),
                    text: component.text.clone(),
                    images: None,
                });
            }

            false
        }
        // **`OpenFileDialog`**: Programmatically triggers the hidden file input.
        // This allows using a styled button to open the browser's file selection dialog
        // for image uploads. Returns `false` as it's a side effect.
        Msg::OpenFileDialog => {
            if let Some(input) = component.file_input_ref.cast::<web_sys::HtmlInputElement>() {
                input.click();
            }
            false
        }
        // **`FileSelected(file)`**: Handles the result of a file dialog selection.
        // It generates a unique ID for the new image, inserts an `[img:<id>]` tag at the
        // cursor, and spawns an async task to read the file as bytes and send an
        // `AddImageToTemplate` message with the Base64 data. Returns `true`.
        Msg::FileSelected(file) => {
            use uuid::Uuid;
            let uuid = Uuid::new_v4().to_string();

            if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                if let Some(textarea) = document
                    .get_element_by_id("static-textarea")
                    .and_then(|e| e.dyn_into::<HtmlTextAreaElement>().ok())
                {
                    let start_utf16 =
                        textarea.selection_start().unwrap_or(Some(0)).unwrap_or(0) as usize;
                    let end_utf16 =
                        textarea.selection_end().unwrap_or(Some(0)).unwrap_or(0) as usize;
                    let start = component
                        .text
                        .encode_utf16()
                        .take(start_utf16)
                        .map(|c| char::from_u32(c as u32).unwrap().len_utf8())
                        .sum();
                    let end = component
                        .text
                        .encode_utf16()
                        .take(end_utf16)
                        .map(|c| char::from_u32(c as u32).unwrap().len_utf8())
                        .sum();
                    let styled = format!("[img:{}]", uuid);
                    component.text = format!(
                        "{}{}{}",
                        &component.text[..start],
                        styled,
                        &component.text[end..]
                    );
                    textarea.set_value(&component.text);

                    let file_clone = file.clone();
                    let link = ctx.link().clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let blob = Blob::from(file_clone);
                        if let Ok(bytes) = read_as_bytes(&blob).await {
                            let base64 = general_purpose::STANDARD.encode(&bytes);
                            link.send_message_batch(vec![
                                Msg::AutoResize,
                                Msg::AddImageToTemplate { id: uuid, base64 },
                            ]);
                        }
                    });
                    // Update dirty flag
                    set_window_dirty_flag(component);
                }
            }
            true
        }
        // **`AddImageToTemplate { id, base64 }`**: Adds image data to the in-memory template.
        // This is the callback from `FileSelected`. It creates an `Image` struct and adds
        // it to the `template.images` vector. Returns `false`.
        Msg::AddImageToTemplate { id, base64 } => {
            let image = Image { id, base64 };
            if let Some(template) = &mut component.template {
                match &mut template.images {
                    Some(images) => images.push(image),
                    None => template.images = Some(vec![image]),
                }
            } else {
                component.template = Some(Template {
                    id: String::new(),
                    text: component.text.clone(),
                    images: Some(vec![image]),
                });
            }
            false
        }
        // **`OpenImageDialogWithId(id)`**: Opens the image management dialog.
        // Triggered when the user's selection enters an `[img:...]` tag. It sets the
        // `selected_image_id` state and opens the top sheet dialog. Returns `true`.
        Msg::OpenImageDialogWithId(id) => {
            component.selected_image_id = Some(id);
            open_top_sheet(component.image_dialog_ref.clone());
            true
        }
        // **`DeleteImage(id)`**: Removes an image from the template.
        // It removes the image data from `template.images` and strips all occurrences
        // of its `[img:...]` tag from the editor text. Updates the dirty flag. Returns `true`.
        Msg::DeleteImage(id) => {
            if let Some(template) = &mut component.template {
                if let Some(images) = &mut template.images {
                    images.retain(|img| img.id != id);
                }
                component.text = component.text.replace(&format!("[img:{}]", id), "");
                template.text = component.text.clone();
            }
            component.selected_image_id = None;
            close_top_sheet(component.image_dialog_ref.clone());

            // Update dirty flag
            set_window_dirty_flag(component);
            true
        }
        // **`Save`**: Persists the current template to the backend.
        // It sends the entire `template` object (ID, text, and images) to the
        // `/api/templates/save` endpoint. On success, it dispatches `SaveSucceeded`.
        // Shows toast notifications for success or failure. Returns `false`.
        Msg::Save => {
            let template = component.template.get_or_insert_with(|| Template {
                id: String::new(),
                text: component.text.clone(),
                images: None,
            });

            if template.id.is_empty() {
                template.id = uuid::Uuid::new_v4().to_string();
            }

            let template_clone = template.clone();
            let link = ctx.link().clone();
            spawn_local(async move {
                match Request::post("/api/templates/save")
                    .json(&template_clone)
                    .unwrap()
                    .send()
                    .await
                {
                    Ok(response) if response.status() == 200 => {
                        link.send_message(Msg::SaveSucceeded);
                        show_toast("Plantilla guardada correctamente.");
                    }
                    Ok(response) => {
                        show_toast(&format!(
                            "Error al guardar la plantilla: {}",
                            response.text().await.unwrap_or_default()
                        ));
                    }
                    Err(err) => {
                        show_toast(&format!("Error al guardar la plantilla: {}", err));
                    }
                }
            });

            false
        }
        // **`SetTemplate(template_opt)`**: Replaces the component's entire template.
        // Typically used on initial load. It sets the `template` state and calculates
        // the `original_md5` hash of the text, which is used to track unsaved changes.
        // Returns `true`.
        Msg::SetTemplate(template_opt) => {
            component.template = template_opt;
            component.original_md5 = component.template.as_ref().map(|t| compute_md5(&t.text));

            // Update dirty flag
            set_window_dirty_flag(component);
            true
        }
        // **`InsertCsvColumnPlaceholder(col_check)`**: Inserts a CSV data placeholder.
        // It creates a `[ph:TITLE:BASE64]` tag using data from the selected CSV column
        // and inserts it at the current cursor position in the textarea. Returns `true`.
        Msg::InsertCsvColumnPlaceholder(col_check) => {
            if let Some(textarea) = component.textarea_ref.cast::<HtmlTextAreaElement>() {
                let utf16_pos = textarea.selection_start().unwrap_or(Some(0)).unwrap_or(0) as usize;
                let byte_pos = super::helpers::utf16_to_byte_idx(&component.text, utf16_pos);

                let mut text = component.text.clone();
                let value = col_check.first_row.clone().unwrap_or_default();
                let base64 = general_purpose::STANDARD.encode(value);
                let placeholder = format!("[ph:{}:{}]", col_check.title, base64);
                text.insert_str(byte_pos, &placeholder);
                component.text = text;

                let new_utf16_pos = byte_to_utf16_idx(
                    &component.text,
                    byte_pos + placeholder.len(),
                );

                let textarea_ref = component.textarea_ref.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    gloo_timers::future::TimeoutFuture::new(10).await;
                    if let Some(textarea) = textarea_ref.cast::<HtmlTextAreaElement>() {
                        textarea
                            .set_selection_range(new_utf16_pos, new_utf16_pos)
                            .ok();
                    }
                });
                true
            } else {
                false
            }
        }
        // **`CsvColumnsUpdated(cols)`**: Prunes placeholders for removed CSV columns.
        // When the associated CSV file changes, this message is sent with the new set of
        // valid columns. It scans the text and removes any `[ph:...]` placeholders whose
        // title no longer exists in the new column list. Returns `true` if text changed.
        Msg::CsvColumnsUpdated(cols) => {
            // Build a set of allowed titles
            let allowed: HashSet<String> = cols.into_iter().map(|c| c.title).collect();

            // Regex for placeholders: [ph:TITLE:BASE64]
            let ph_re = Regex::new(r"\[ph:([^:\]]+):([A-Za-z0-9+/=]+)]").unwrap();

            // Replace placeholders whose TITLE is not in `allowed` with an empty string
            let new_text = ph_re
                .replace_all(&component.text, |caps: &regex::Captures| {
                    let title = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                    if allowed.contains(title) {
                        caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string()
                    } else {
                        String::new()
                    }
                })
                .into_owned();

            if new_text != component.text {
                component.text = new_text.clone();
                // Keep the template synchronized if present
                if let Some(template) = &mut component.template {
                    template.text = new_text.clone();
                }
                // Update the textarea DOM if present
                if let Some(textarea) = component.textarea_ref.cast::<HtmlTextAreaElement>() {
                    textarea.set_value(&new_text);
                }
                // Recalculate size and refresh images if applicable
                ctx.link().send_message(Msg::AutoResize);

                // Update dirty flag
                set_window_dirty_flag(component);
                return true;
            }
            false
        }
        // **`SaveSucceeded`**: Updates the dirty-checking baseline after a successful save.
        // It recalculates `original_md5` with the current text content, effectively marking
        // the current state as "saved". Resets the global dirty flag. Returns `true`.
        Msg::SaveSucceeded => {
            component.original_md5 = Some(compute_md5(&component.text));

            // Update dirty flag
            set_window_dirty_flag(component);
            true
        }
        // **`OpenPdf`**: Prepares and opens the PDF preview dialog.
        // It checks for unsaved changes, then sets the `pdf_url` to the backend endpoint
        // `/api/templates/pdf/{id}`, including a cache-busting timestamp. It also sets
        // `pdf_loading` to `true` and opens the dialog. Returns `true`.
        Msg::OpenPdf => {
            if let Some(template) = &component.template {
                if template.id.is_empty() {
                    show_toast("Guarda la plantilla antes de generar el PDF.");
                    return true;
                }

                // Only proceed if the text hasn't changed since last save
                let current_md5 = compute_md5(&component.text);
                if let Some(orig) = &component.original_md5 {
                    if orig != &current_md5 {
                        show_toast("Guarda la plantilla antes de generar el PDF.");
                        return true;
                    }
                } else {
                    // If no original md5, must save first
                    show_toast("Guarda la plantilla antes de generar el PDF.");
                    return true;
                }

                // Force a cache-busting timestamp
                let ts = Date::now() as u64;
                component.pdf_url = Some(format!("/api/templates/pdf/{}?t={}", template.id, ts));

                // Mostrar modal de progreso hasta que el iframe cargue
                component.pdf_loading = true;

                open_top_sheet(component.pdf_viewer_dialog_ref.clone());
            } else {
                show_toast("No hay plantilla cargada.");
            }
            true
        }
        // **`PdfLoaded`**: Acknowledges that the PDF iframe has finished loading.
        // This message is sent from the `pdf_dialog`'s `onload` event. It sets the
        // `pdf_loading` flag to `false`, hiding any loading indicators. Returns `true`.
        Msg::PdfLoaded => {
            // El iframe ha terminado de cargar
            component.pdf_loading = false;
            true
        }
        // **`ClosePdfDialog`**: Resets state related to the PDF viewer.
        // It clears the `pdf_url` and `pdf_loading` flags, effectively closing the
        // PDF preview dialog and cleaning up its state. Returns `true`.
        Msg::ClosePdfDialog => {
            component.pdf_url = None;
            component.pdf_loading = false;
            true
        }
    }
}

/// Sets the global `app_dirty` flag based on whether the current text
/// differs from the last saved state (`original_md5`).
fn set_window_dirty_flag(component: &StaticTextComponent) {
    if let Some(window) = web_sys::window() {
        let dirty = component
            .original_md5
            .as_ref()
            .map_or(!component.text.is_empty(), |orig| {
                orig != &compute_md5(&component.text)
            });
        let _ = Reflect::set(
            &window,
            &JsValue::from_str("app_dirty"),
            &JsValue::from_bool(dirty),
        );
    }
}