use crate::tops_sheet::yw_material_top_sheet::{
    close_top_sheet, open_top_sheet, YwMaterialTopSheet,
};
use base64::{engine::general_purpose, Engine as _};
use common::model::image::Image;
use common::model::template::Template;
use gloo_console::console_dbg;
use gloo_file::{futures::read_as_bytes, Blob};
use gloo_net::http::Request;
use pulldown_cmark::{html, Parser};
use regex::Regex;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlTextAreaElement, InputEvent};
use yew::platform::spawn_local;
use yew::prelude::*;

// CSS styles for the component UI elements
const BUTTON_STYLE: &str = "
                             .static-text-root { width: 100%; }
                             .icon-toolbar { display: flex; gap: 16px; margin-bottom: 8px; justify-content: flex-start; }
                             .icon-btn { background: #f5f5f5; border: none; border-radius: 4px; padding: 4px 10px 0 10px; cursor: pointer; font-size: 20px; transition: background 0.2s; display: flex; flex-direction: column; align-items: center; width: 56px; height: 48px; box-sizing: border-box; }
                             .icon-btn.wide { width: 90px; }
                             .icon-btn:hover { background: #e0e0e0; }
                             .icon-label { font-size: 10px; color: #555; margin-top: 2px; text-align: center; letter-spacing: 0.5px; white-space: nowrap; }
                             .material-icons { vertical-align: middle; font-size: 20px; color: #333; }
                             .tab-bar { display: flex; gap: 2px; margin-bottom: 12px; border-bottom: 1px solid #ddd; }
                             .tab-btn { background: #f5f5f5; border: none; border-radius: 4px 4px 0 0; padding: 6px 18px; cursor: pointer; font-size: 14px; color: #555; margin-bottom: -1px; border-bottom: 2px solid transparent; transition: background 0.2s, border-bottom 0.2s; }
                             .tab-btn.active { background: #fff; color: #222; border-bottom: 2px solid #1976d2; font-weight: bold; }
                             .tab-btn:hover { background: #e0e0e0; }
                             #static-textarea { font-size: 11px; font-family: Arial, sans-serif; }
                             .markdown-preview { font-size: 11px; font-family: Arial, sans-serif; }
                         ";

/// Maximum allowed file size in bytes for image uploads
const MAX_FILE_SIZE: u32 = 4_000_000;

// Renders a <style> tag with the component styles
fn style_tag() -> Html {
    html! { <style>{BUTTON_STYLE}</style> }
}

// Renders an icon button with a label and click callback
fn icon_button(icon_name: &str, label: &str, on_click: Callback<MouseEvent>, wide: bool) -> Html {
    let class = if wide { "icon-btn wide" } else { "icon-btn" };
    html! {
        <button class={class} onclick={on_click.clone()}>
            <i class="material-icons">{icon_name}</i>
            <span class="icon-label">{label}</span>
        </button>
    }
}

fn get_img_tag_id_at_cursor(text: &str, cursor_pos_utf16: usize) -> Option<String> {
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

// Message enum for component state changes
pub enum Msg {
    SetTab(String),              // Switches between editor and preview tabs
    UpdateText(String),          // Updates the text in the editor
    Undo,                        // Undo last change
    Redo,                        // Redo change
    ApplyStyle(String, ()),      // Applies markdown style to selected text
    AutoResize,                  // Automatically resizes the textarea
    OpenFileDialog,              // Opens the file selection dialog
    FileSelected(web_sys::File), // Handles selected file
    AddImageToTemplate { id: String, base64: String }, // Adds image to template
    OpenImageDialogWithId(String), // Opens image dialog for a specific image ID
    DeleteImage(String),         // Deletes an image from the template
    Save,                        // Saves the current template
    SetTemplate(Option<Template>),
}

// Properties struct
#[derive(Properties, PartialEq, Clone)]
pub struct StaticTextProps {
    #[prop_or_default]
    pub template_id: Option<String>, // Optional template ID to load
}

// Component state struct
pub struct StaticTextComponent {
    text: String,                      // Current text in the editor
    history: Vec<String>,              // Undo/redo history
    history_index: usize,              // Current position in history
    active_tab: String,                // Selected tab ("editor" or "preview")
    textarea_ref: NodeRef,             // Reference to the textarea element
    file_input_ref: NodeRef,           // Reference to the file input element
    image_dialog_ref: NodeRef,         // Reference to the image viewer dialog
    template: Option<Template>,        // Optional template to sync text with
    selected_image_id: Option<String>, // Currently selected image ID
    loaded: bool,                      // Flag to indicate if the template has been loaded
}

impl StaticTextComponent {
    /// Resizes the textarea to fit its content automatically.
    fn resize_textarea(&self) {
        if let Some(textarea) = self.textarea_ref.cast::<HtmlTextAreaElement>() {
            if let Ok(html_elem) = textarea.clone().dyn_into::<HtmlElement>() {
                let style = html_elem.style();
                let _ = style.set_property("height", "auto");
                let scroll_height = textarea.scroll_height();
                let _ = style.set_property("height", &format!("{}px", scroll_height));
            }
        }
    }
}

impl Component for StaticTextComponent {
    type Message = Msg;
    type Properties = StaticTextProps;

    // Initializes the component state
    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            text: String::new(),
            history: vec![String::new()],
            history_index: 0,
            active_tab: "editor".to_string(),
            textarea_ref: Default::default(),
            file_input_ref: Default::default(),
            image_dialog_ref: Default::default(),
            template: None,
            selected_image_id: None,
            loaded: false,
        }
    }

    // Handles state updates based on messages
    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            // Updates text and history
            Msg::UpdateText(new_text) => {
                if self.text != new_text {
                    self.text = new_text.clone();
                    self.history.truncate(self.history_index + 1);
                    self.history.push(new_text);
                    self.history_index = self.history.len() - 1;
                }
                true
            }
            // Undo last change
            Msg::Undo => {
                if self.history_index > 0 {
                    self.history_index -= 1;
                    self.text = self.history[self.history_index].clone();
                }
                true
            }
            // Redo change
            Msg::Redo => {
                if self.history_index + 1 < self.history.len() {
                    self.history_index += 1;
                    self.text = self.history[self.history_index].clone();
                }
                true
            }
            // Switches between editor and preview tabs
            Msg::SetTab(tab) => {
                self.active_tab = tab;
                if self.active_tab == "editor" {
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
            // Applies markdown style to selected text
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

                        let start = self
                            .text
                            .encode_utf16()
                            .take(start_utf16)
                            .map(|c| char::from_u32(c as u32).unwrap().len_utf8())
                            .sum();
                        let end = self
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

                        self.text =
                            format!("{}{}{}", &self.text[..start], styled, &self.text[end..]);
                        textarea.set_value(&self.text);

                        // Find position of inserted "texto"
                        let text_pos = self.text[start..].find("texto").unwrap_or(0) + start;
                        let select_start = byte_to_utf16_idx(&self.text, text_pos);
                        let select_end = byte_to_utf16_idx(&self.text, text_pos + 5);

                        textarea.set_selection_start(Some(select_start)).ok();
                        textarea.set_selection_end(Some(select_end)).ok();
                        textarea.focus().ok();
                    }
                }
                true
            }
            // Automatically resizes the textarea based on content and syncs text with template
            Msg::AutoResize => {
                self.resize_textarea();
                if let Some(template) = &mut self.template {
                    template.text = self.text.clone();
                    // Filter images to keep only those referenced in the text
                    if let Some(images) = &mut template.images {
                        images.retain(|img| self.text.contains(&format!("[img:{}]", img.id)));
                    }
                } else {
                    self.template = Some(Template {
                        id: String::new(),
                        text: self.text.clone(),
                        images: None,
                    });
                }
                console_dbg!(
                    "Template json after AutoResize:",
                    serde_json::to_string(&self.template).unwrap_or_default()
                );
                false
            }
            Msg::OpenFileDialog => {
                if let Some(input) = self.file_input_ref.cast::<web_sys::HtmlInputElement>() {
                    input.click();
                }
                false
            }
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
                        let start = self
                            .text
                            .encode_utf16()
                            .take(start_utf16)
                            .map(|c| char::from_u32(c as u32).unwrap().len_utf8())
                            .sum();
                        let end = self
                            .text
                            .encode_utf16()
                            .take(end_utf16)
                            .map(|c| char::from_u32(c as u32).unwrap().len_utf8())
                            .sum();
                        let styled = format!("[img:{}]", uuid);
                        self.text =
                            format!("{}{}{}", &self.text[..start], styled, &self.text[end..]);
                        textarea.set_value(&self.text);

                        // Read the file as bytes and convert to base64
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
                    }
                }
                true
            }
            Msg::AddImageToTemplate { id, base64 } => {
                let image = Image { id, base64 };
                if let Some(template) = &mut self.template {
                    match &mut template.images {
                        Some(images) => images.push(image),
                        None => template.images = Some(vec![image]),
                    }
                } else {
                    self.template = Some(Template {
                        id: String::new(),
                        text: self.text.clone(),
                        images: Some(vec![image]),
                    });
                }
                false
            }
            Msg::OpenImageDialogWithId(id) => {
                self.selected_image_id = Some(id);
                open_top_sheet(self.image_dialog_ref.clone());
                true
            }
            Msg::DeleteImage(id) => {
                if let Some(template) = &mut self.template {
                    if let Some(images) = &mut template.images {
                        images.retain(|img| img.id != id);
                    }
                    self.text = self.text.replace(&format!("[img:{}]", id), "");
                    template.text = self.text.clone(); // Sync text with template
                }
                self.selected_image_id = None;
                close_top_sheet(self.image_dialog_ref.clone());
                true
            }
            Msg::Save => {
                // Ensure the template exists and has an ID
                let template = self.template.get_or_insert_with(|| Template {
                    id: String::new(),
                    text: self.text.clone(),
                    images: None,
                });

                if template.id.is_empty() {
                    template.id = uuid::Uuid::new_v4().to_string();
                }

                // Clone the template and send a POST request
                let template_clone = template.clone();
                spawn_local(async move {
                    match Request::post("/api/templates/save")
                        .json(&template_clone)
                        .unwrap()
                        .send()
                        .await
                    {
                        Ok(response) if response.status() == 200 => {
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
            Msg::SetTemplate(template_opt) => {
                self.template = template_opt;
                true
            }
        }
    }

    // Renders the component UI
    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        // Converts Markdown text to HTML for preview.
        let preview_html = {
            // Step 0: Replace every '\n' with ' \n' to ensure a space before each newline
            let text_with_spaces = self.text.replace("\n", " \n");

            // Step 1: Mark multiple newlines with a special marker
            let re = Regex::new(r"(\n\s*){2,}").unwrap();
            let marked_text = re.replace_all(&text_with_spaces, |caps: &regex::Captures| {
                let count = caps[0].matches('\n').count();
                format!("br{}", count)
            });

            // Step 2: Parse the marked text as Markdown
            let parser = Parser::new(&marked_text);
            let mut html_output = String::new();
            html::push_html(&mut html_output, parser);

            // Step 3: Replace the special markers with actual <br> tags
            let re_br = Regex::new(r"br(\d+)").unwrap();
            let final_html = re_br.replace_all(&html_output, |caps: &regex::Captures| {
                let n = caps[1].parse::<usize>().unwrap_or(1);
                "<br>".repeat(n)
            });

            // Step 4: Replace image tags with actual <img> elements
            let mut html_with_images = final_html.into_owned();
            if let Some(template) = &self.template {
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
        };

        // Helper to create style button callbacks
        let make_style_callback =
            |style: &'static str| link.callback(move |_| Msg::ApplyStyle(style.to_string(), ()));

        html! {
            <div class="static-text-root">
                // Injects the component styles
                { style_tag() }
                // Toolbar with style buttons
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
                // Tab bar for switching between editor and preview
                <div class="tab-bar">
                    <button
                        class={classes!("tab-btn", if self.active_tab == "editor" { "active" } else { "" })}
                        onclick={link.callback(|_| Msg::SetTab("editor".to_string()))}
                    >{"Editor"}</button>
                    <button
                        class={classes!("tab-btn", if self.active_tab == "preview" { "active" } else { "" })}
                        onclick={link.callback(|_| Msg::SetTab("preview".to_string()))}
                    >{"Previsualización"}</button>
                </div>
                {
                    // Shows the editor textarea if "editor" tab is active
                    if self.active_tab == "editor" {
                        html! {
                            <>
                                <textarea
                                    id="static-textarea"
                                    ref={self.textarea_ref.clone()}
                                    value={self.text.clone()}
                                    spellcheck="false"
                                    oninput={link.batch_callback(|e: InputEvent| {
                                        let value = e.target_unchecked_into::<HtmlTextAreaElement>().value();
                                        vec![
                                            Msg::UpdateText(value),
                                            Msg::AutoResize,
                                        ]
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
                                <input
                                    type="file"
                                    accept="image/*"
                                    ref={self.file_input_ref.clone()}
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

                                <YwMaterialTopSheet node_ref={self.image_dialog_ref.clone()}>
                                    <div style="position:fixed;top:0;left:0;width:100vw;height:100vh;background:rgba(0,0,0,0.85);z-index:9999;display:flex;flex-direction:column;align-items:center;justify-content:center;">
                                        <button
                                            onclick={{
                                                let dialog_ref = self.image_dialog_ref.clone();
                                                Callback::from(move |_| close_top_sheet(dialog_ref.clone()))
                                            }}
                                            style="position:absolute;top:24px;right:32px;z-index:10000;padding:0.5rem 1rem;font-size:1.5rem;background:#fff;border:none;border-radius:4px;cursor:pointer;"
                                        >
                                            { "✕" }
                                        </button>
                                        {
                                            if let Some(id) = &self.selected_image_id {
                                                let id_cloned = id.clone();
                                                if let Some(template) = &self.template {
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
                                </YwMaterialTopSheet>
                            </>

                        }
                    } else {
                        // Shows the markdown preview if "preview" tab is active
                        html! { <div class="markdown-preview">{ Html::from_html_unchecked(preview_html.clone()) }</div> }
                    }
                }
            </div>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render && !self.loaded {
            self.loaded = true;

            if let Some(template_id) = &ctx.props().template_id {
                let link = ctx.link().clone();
                let template_id = template_id.clone();
                spawn_local(async move {
                    let response = Request::get(&format!("/api/templates/{}", template_id))
                        .send()
                        .await;

                    match response {
                        Ok(resp) if resp.status() == 200 => {
                            if let Ok(template) = resp.json::<Template>().await {
                                link.send_message_batch(vec![
                                    Msg::UpdateText(template.text.clone()),
                                    Msg::SetTemplate(Some(template)),
                                    Msg::SetTab("editor".to_string()),
                                ]);
                                show_toast("Plantilla cargada correctamente.");
                            } else {
                                create_new_template(link);
                            }
                        }
                        _ => create_new_template(link),
                    }
                });
            } else {
                self.template = Some(create_empty_template());
                show_toast("No se proporcionó ID de plantilla. Se creó una nueva.");
            }
        }
    }
}

fn create_new_template(link: yew::html::Scope<StaticTextComponent>) {
    link.send_message_batch(vec![
        Msg::SetTemplate(Some(create_empty_template())),
        Msg::UpdateText(String::new()),
        Msg::SetTab("editor".to_string()),
    ]);
    show_toast("Error cargando plantilla. Se creó una nueva.");
}

fn create_empty_template() -> Template {
    Template {
        id: uuid::Uuid::new_v4().to_string(),
        text: String::new(),
        images: None,
    }
}

fn show_toast(message: &str) {
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
fn byte_to_utf16_idx(s: &str, byte_idx: usize) -> u32 {
    s[..byte_idx].encode_utf16().count() as u32
}
