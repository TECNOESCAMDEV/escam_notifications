use gloo_console::console_dbg;
//use gloo_console::console_dbg;
use pulldown_cmark::{html, Parser};
use regex::Regex;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlTextAreaElement, InputEvent};
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

fn is_cursor_on_img_tag(text: &str, cursor_pos_utf16: usize) -> bool {
    // Convert UTF-16 cursor position to byte index
    let cursor_pos_byte = text.encode_utf16()
        .take(cursor_pos_utf16)
        .map(|c| char::from_u32(c as u32).unwrap().len_utf8())
        .sum::<usize>();

    let re = Regex::new(r"\[img:[^]]+]").unwrap();
    for mat in re.find_iter(text) {
        let start = mat.start();
        let end = mat.end();
        if cursor_pos_byte >= start && cursor_pos_byte <= end {
            return true;
        }
    }
    false
}

// Message enum for component state changes
pub enum Msg {
    SetTab(String),         // Switches between editor and preview tabs
    UpdateText(String),     // Updates the text in the editor
    Undo,                   // Undo last change
    Redo,                   // Redo change
    ApplyStyle(String, ()), // Applies markdown style to selected text
    AutoResize,             // Automatically resizes the textarea
    CursorOnImgTag(bool),   // Checks if cursor is on an image tag
    OpenFileDialog,      // Opens the file selection dialog
    FileSelected(web_sys::File), // Handles selected file
}

// Component state struct
pub struct StaticTextComponent {
    text: String,          // Current text in the editor
    history: Vec<String>,  // Undo/redo history
    history_index: usize,  // Current position in history
    active_tab: String,    // Selected tab ("editor" or "preview")
    textarea_ref: NodeRef, // Reference to the textarea element
    file_input_ref: NodeRef, // Reference to the file input element
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
    type Properties = ();

    // Initializes the component state
    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            text: String::new(),
            history: vec![String::new()],
            history_index: 0,
            active_tab: "editor".to_string(),
            textarea_ref: Default::default(),
            file_input_ref: Default::default(),
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
            // Automatically resizes the textarea based on content
            Msg::AutoResize => {
                self.resize_textarea();
                false
            }
            Msg::CursorOnImgTag(value) => {
                console_dbg!("Cursor on img tag:", value);
                false
            }
            Msg::OpenFileDialog => {
                if let Some(input) = self.file_input_ref.cast::<web_sys::HtmlInputElement>() {
                    input.click();
                }
                false
            }
            Msg::FileSelected(file) => {
                let url = web_sys::Url::create_object_url_with_blob(&file).unwrap_or_default();
                if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                    if let Some(textarea) = document
                        .get_element_by_id("static-textarea")
                        .and_then(|e| e.dyn_into::<HtmlTextAreaElement>().ok())
                    {
                        let start_utf16 = textarea.selection_start().unwrap_or(Some(0)).unwrap_or(0) as usize;
                        let end_utf16 = textarea.selection_end().unwrap_or(Some(0)).unwrap_or(0) as usize;
                        let start = self.text.encode_utf16().take(start_utf16).map(|c| char::from_u32(c as u32).unwrap().len_utf8()).sum();
                        let end = self.text.encode_utf16().take(end_utf16).map(|c| char::from_u32(c as u32).unwrap().len_utf8()).sum();
                        let styled = format!("[img:{}]", url);
                        self.text = format!("{}{}{}", &self.text[..start], styled, &self.text[end..]);
                        textarea.set_value(&self.text);
                    }
                }
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

            AttrValue::from(final_html.into_owned())
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
                                        if is_cursor_on_img_tag(&text, cursor_pos) && !arrow_keys.contains(&e.key().as_str()) {
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
                                              let is_on_img_tag = e.target()
                                                  .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
                                                  .map_or(false, |textarea| {
                                                      let text = textarea.value();
                                                      let cursor_pos = textarea.selection_start().unwrap_or(Some(0)).unwrap_or(0) as usize;
                                                      is_cursor_on_img_tag(&text, cursor_pos)
                                                  });
                                              Msg::CursorOnImgTag(is_on_img_tag)
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
                                                return Msg::FileSelected(file);
                                            }
                                        }
                                        Msg::AutoResize
                                    })}
                                />
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
}

fn byte_to_utf16_idx(s: &str, byte_idx: usize) -> u32 {
    s[..byte_idx].encode_utf16().count() as u32
}
