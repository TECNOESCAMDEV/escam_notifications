use gloo_console::console_dbg;
use pulldown_cmark::{html, Parser};
use yew::prelude::*;

const BUTTON_STYLE: &str = "
    .icon-toolbar {
        display: flex;
        gap: 8px;
        margin-bottom: 8px;
    }
    .icon-btn {
        background: #f5f5f5;
        border: none;
        border-radius: 4px;
        padding: 4px;
        cursor: pointer;
        font-size: 20px;
        transition: background 0.2s;
    }
    .icon-btn:hover {
        background: #e0e0e0;
    }
    .material-icons {
        vertical-align: middle;
        font-size: 20px;
        color: #333;
    }
";

fn style_tag() -> Html {
    html! { <style>{BUTTON_STYLE}</style> }
}

fn icon(name: &str) -> Html {
    html! { <i class="material-icons">{name}</i> }
}


pub enum Msg {
    SetTab(String),
    UpdateText(String),
    ApplyStyle(String),
}

pub struct StaticTextComponent {
    text: String,
    active_tab: String,
}

impl Component for StaticTextComponent {
    type Message = Msg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            text: String::new(),
            active_tab: "editor".to_string(),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::SetTab(tab) => {
                self.active_tab = tab;
                true
            }
            Msg::UpdateText(new_text) => {
                self.text = new_text;
                true
            }
            Msg::ApplyStyle(style) => {
                let styled = match style.as_str() {
                    "bold" => "**texto**",
                    "italic" => "*texto*",
                    "bolditalic" => "***texto***",
                    "normal" => "texto",
                    _ => "",
                };
                self.text.push_str(styled);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        let preview_html = {
            let parser = Parser::new(&self.text);
            let mut html_output = String::new();
            html::push_html(&mut html_output, parser);
            AttrValue::from(html_output)
        };
        console_dbg!("Preview HTML:", &preview_html);

        html! {
            <div>
                { style_tag() }
                <div class="icon-toolbar">
                    <button class="icon-btn" onclick={link.callback(|_| Msg::ApplyStyle("normal".to_string()))}>{icon("text_fields")}</button>
                    <button class="icon-btn" onclick={link.callback(|_| Msg::ApplyStyle("bold".to_string()))}>{icon("format_bold")}</button>
                    <button class="icon-btn" onclick={link.callback(|_| Msg::ApplyStyle("italic".to_string()))}>{icon("format_italic")}</button>
                    <button class="icon-btn" onclick={link.callback(|_| Msg::ApplyStyle("bolditalic".to_string()))}>{icon("format_bold")}{icon("format_italic")}</button>
                </div>
                <div>
                    <button onclick={link.callback(|_| Msg::SetTab("editor".to_string()))}>{"Editor"}</button>
                    <button onclick={link.callback(|_| Msg::SetTab("preview".to_string()))}>{"Previsualización"}</button>
                </div>
                {
                    if self.active_tab == "editor" {
                        html! {
                            <textarea
                                value={self.text.clone()}
                                oninput={link.callback(|e: InputEvent| {
                                    let input: web_sys::HtmlTextAreaElement = e.target_unchecked_into();
                                    Msg::UpdateText(input.value())
                                })}
                                rows={10}
                                cols={50}
                            />
                        }
                    } else {
                        html! { <>{ Html::from_html_unchecked(preview_html.clone()) }</> }
                    }
                }
            </div>
        }
    }
}