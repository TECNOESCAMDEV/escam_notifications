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
use pulldown_cmark::{html, Parser};
use regex::Regex;
use wasm_bindgen::JsCast;
use web_sys::{HtmlTextAreaElement, InputEvent};
use yew::prelude::*;

use super::helpers::{get_img_tag_id_at_cursor, show_toast};
use super::messages::Msg;
use super::state::StaticTextComponent;
use super::styles::style_tag;

use crate::tops_sheet::yw_material_top_sheet::close_top_sheet;

const MAX_FILE_SIZE: u32 = 4_000_000;

/// Top-level view function called by the component's `view()` method.
pub fn view(component: &StaticTextComponent, ctx: &Context<StaticTextComponent>) -> Html {
    let link = ctx.link();
    let preview_html = compute_preview_html(component);

    let make_style_callback =
        |style: &'static str| link.callback(move |_| Msg::ApplyStyle(style.to_string(), ()));

    html! {
        <div class="static-text-root">
            { style_tag() }
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
                            <textarea
                                id="static-textarea"
                                ref={component.textarea_ref.clone()}
                                value={component.text.clone()}
                                spellcheck="false"
                                oninput={link.batch_callback(|e: InputEvent| {
                                    let value = e.target_unchecked_into::<HtmlTextAreaElement>().value();
                                    vec![ Msg::UpdateText(value), Msg::AutoResize ]
                                })}
                                onkeydown={link.batch_callback(|e: KeyboardEvent| {
                                    let textarea = e.target_unchecked_into::<HtmlTextAreaElement>();
                                    let text = textarea.value();
                                    let cursor_pos = textarea.selection_start().unwrap_or(Some(0)).unwrap_or(0) as usize;
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
                                            super::helpers::get_img_tag_id_at_cursor(&text, cursor_pos)
                                        });
                                    match id_opt {
                                        Some(id) => Msg::OpenImageDialogWithId(id),
                                        None => Msg::AutoResize,
                                    }
                                })}
                                rows={1}
                                style="width: 100%; min-height: 40px; resize: none; overflow: hidden;"
                            />
                            <input
                                type="file"
                                accept="image/*"
                                ref={component.file_input_ref.clone()}
                                style="display: none;"
                                onchange={link.callback(|e: Event| {
                                    let input = e.target_unchecked_into::<web_sys::HtmlInputElement>();
                                    if let Some(files) = input.files() {
                                        if let Some(file) = files.get(0) {
                                            if file.size() > MAX_FILE_SIZE.into() {
                                                show_toast("El archivo es demasiado grande (máx. {} MB).".replace("{}", &(MAX_FILE_SIZE / 1_000_000).to_string()).as_str());
                                                return Msg::AutoResize;
                                            }
                                            return Msg::FileSelected(file);
                                        }
                                    }
                                    Msg::AutoResize
                                })}
                            />

                            <crate::tops_sheet::yw_material_top_sheet::YwMaterialTopSheet node_ref={component.image_dialog_ref.clone()}>
                                <div style="position:fixed;top:0;left:0;width:100vw;height:100vh;background:rgba(0,0,0,0.85);z-index:9999;display:flex;flex-direction:column;align-items:center;justify-content:center;">
                                    <button
                                        onclick={{
                                            let dialog_ref = component.image_dialog_ref.clone();
                                            Callback::from(move |_| close_top_sheet(dialog_ref.clone()))
                                        }}
                                        style="position:absolute;top:24px;right:32px;z-index:10000;padding:0.5rem 1rem;font-size:1.5rem;background:#fff;border:none;border-radius:4px;cursor:pointer;"
                                    >
                                        { "✕" }
                                    </button>
                                    {
                                        if let Some(id) = &component.selected_image_id {
                                            let id_cloned = id.clone();
                                            if let Some(template) = &component.template {
                                                if let Some(images) = &template.images {
                                                    if let Some(image) = images.iter().find(|img| &img.id == id) {
                                                        html! {
                                                            <>
                                                                <img src={format!("data:image/*;base64,{}", image.base64)} style="max-width:400px;max-height:400px;margin-bottom:24px;" />
                                                                <button
                                                                    style="padding:0.5rem 1rem;font-size:1rem;background:#d32f2f;color:#fff;border:none;border-radius:4px;cursor:pointer;"
                                                                    onclick={link.callback(move |_| Msg::DeleteImage(id_cloned.clone()))}
                                                                >
                                                                    { "Borrar" }
                                                                </button>
                                                            </>
                                                        }
                                                    } else { html! { <span style="color:#fff;">{"Imagen no encontrada"}</span> } }
                                                } else { html! { <span style="color:#fff;">{"Sin imágenes"}</span> } }
                                            } else { html! { <span style="color:#fff;">{"Sin template"}</span> } }
                                        } else { html! { <span style="color:#fff;">{"No hay imagen seleccionada"}</span> } }
                                    }
                                </div>
                            </crate::tops_sheet::yw_material_top_sheet::YwMaterialTopSheet>
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

/// Produces the HTML used by the preview tab.
///
/// Steps
/// 1. Replace raw newlines with " \n" to help the markdown parser preserve spacing.
/// 2. Compress 2+ newline sequences into a temporary marker (e.g., "br3").
/// 3. Parse the marked text with `pulldown_cmark` to HTML.
/// 4. Expand markers back into repeated `<br>` tags.
/// 5. Replace `[img:<id>]` placeholders with `<img src="data:...">` for template images.
fn compute_preview_html(component: &StaticTextComponent) -> AttrValue {
    let text_with_spaces = component.text.replace("\n", " \n");

    let re = Regex::new(r"(\n\s*){2,}").unwrap();
    let marked_text = re.replace_all(&text_with_spaces, |caps: &regex::Captures| {
        let count = caps[0].matches('\n').count();
        format!("br{}", count)
    });

    let parser = Parser::new(&marked_text);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    let re_br = Regex::new(r"br(\d+)").unwrap();
    let final_html = re_br.replace_all(&html_output, |caps: &regex::Captures| {
        let n = caps[1].parse::<usize>().unwrap_or(1);
        "<br>".repeat(n)
    });

    let mut html_with_images = final_html.into_owned();
    if let Some(template) = &component.template {
        if let Some(images) = &template.images {
            for image in images {
                let img_tag = format!("[img:{}]", image.id);
                let img_html = format!(
                    r#"<img src="data:image/*;base64,{}" style="max-width:200px;max-height:200px;vertical-align:middle;" />"#,
                    image.base64
                );
                html_with_images = html_with_images.replace(&img_tag, &img_html);
            }
        }
    }
    AttrValue::from(html_with_images)
}
