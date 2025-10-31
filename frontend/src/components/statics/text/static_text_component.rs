use pulldown_cmark::{html, Parser};
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

// Message enum for component state changes
pub enum Msg {
    SetTab(String),
    UpdateText(String),
    Undo,
    Redo,
    ApplyStyle(String, ()),
    AutoResize(InputEvent),
}

// Component state struct
pub struct StaticTextComponent {
    text: String,                // Current text in the editor
    history: Vec<String>,        // Undo/redo history
    history_index: usize,        // Current position in history
    active_tab: String,          // Selected tab ("editor" or "preview")
    textarea_ref: NodeRef,       // Reference to the textarea element
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
        }
    }

    // Handles state updates based on messages
    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
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
                true
            }
            // Applies markdown style to selected text
            Msg::ApplyStyle(style, _) => {
                if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                    if let Some(textarea) = document.get_element_by_id("static-textarea")
                        .and_then(|e| e.dyn_into::<HtmlTextAreaElement>().ok()) {
                        let start = textarea.selection_start().unwrap_or(Some(0)).unwrap_or(0) as usize;
                        let end = textarea.selection_end().unwrap_or(Some(0)).unwrap_or(0) as usize;
                        let styled = match style.as_str() {
                            "bold" => "**texto**",
                            "italic" => "*texto*",
                            "bolditalic" => "***texto***",
                            "normal" => "texto",
                            _ => "",
                        };
                        self.text = format!(
                            "{}{}{}",
                            &self.text[..start],
                            styled,
                            &self.text[end..]
                        );
                        textarea.set_value(&self.text);
                        let select_start = (start + styled.find("texto").unwrap_or(0)) as u32;
                        let select_end = select_start + 5;
                        textarea.set_selection_start(Some(select_start)).ok();
                        textarea.set_selection_end(Some(select_end)).ok();
                        textarea.focus().ok();
                    }
                }
                true
            }
            // Automatically resizes the textarea based on content
            Msg::AutoResize(e) => {
                if let Some(textarea) = e.target_dyn_into::<HtmlTextAreaElement>() {
                    if let Ok(html_elem) = textarea.clone().dyn_into::<HtmlElement>() {
                        let style = html_elem.style();
                        let _ = style.set_property("height", "auto");
                        let scroll_height = textarea.scroll_height();
                        let _ = style.set_property("height", &format!("{}px", scroll_height));
                    }
                }
                false
            }
        }
    }

    // Renders the component UI
    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();

        // Converts markdown text to HTML for preview
        let preview_html = {
            let parser = Parser::new(&self.text);
            let mut html_output = String::new();
            html::push_html(&mut html_output, parser);
            AttrValue::from(html_output)
        };

        // Helper to create style button callbacks
        let make_style_callback = |style: &'static str| {
            link.callback(move |_| Msg::ApplyStyle(style.to_string(), ()))
        };

        html! {
            <div class="static-text-root">
                // Injects the component styles
                { style_tag() }
                // Toolbar with style buttons
                <div class="icon-toolbar">
                    {icon_button("text_fields", "Normal", make_style_callback("normal"), false)}
                    {icon_button("format_bold", "Negrita", make_style_callback("bold"), false)}
                    {icon_button("format_italic", "Cursiva", make_style_callback("italic"), false)}
                    {icon_button("format_bold", "Negrita+Cursiva", make_style_callback("bolditalic"), true)}
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
                            <textarea
                                id="static-textarea"
                                ref={self.textarea_ref.clone()}
                                value={self.text.clone()}
                                // Handles text input and auto-resize
                                oninput={link.batch_callback(|e: InputEvent| {
                                    let value = e.target_unchecked_into::<HtmlTextAreaElement>().value();
                                    vec![
                                        Msg::UpdateText(value),
                                        Msg::AutoResize(e),
                                    ]
                                })}
                                // Handles undo/redo keyboard shortcuts
                                onkeydown={link.batch_callback(|e: KeyboardEvent| {
                                    if e.ctrl_key() && e.key() == "z" {
                                        vec![Msg::Undo]
                                    } else if e.ctrl_key() && e.key() == "y" {
                                        vec![Msg::Redo]
                                    } else {
                                        vec![]
                                    }
                                })}
                                rows={1}
                                style="width: 100%; min-height: 40px; resize: none; overflow: hidden;"
                            />
                        }
                    } else {
                        // Shows the markdown preview if "preview" tab is active
                        html! { <>{ Html::from_html_unchecked(preview_html.clone()) }</> }
                    }
                }
            </div>
        }
    }
}