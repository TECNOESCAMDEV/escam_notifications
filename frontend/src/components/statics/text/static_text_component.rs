use pulldown_cmark::{html, Parser};
use yew::prelude::*;

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
            html_output
        };

        html! {
            <div>
                <div>
                    <button onclick={link.callback(|_| Msg::ApplyStyle("normal".to_string()))}>{"Normal"}</button>
                    <button onclick={link.callback(|_| Msg::ApplyStyle("bold".to_string()))}>{"Negrilla"}</button>
                    <button onclick={link.callback(|_| Msg::ApplyStyle("italic".to_string()))}>{"Itálica"}</button>
                    <button onclick={link.callback(|_| Msg::ApplyStyle("bolditalic".to_string()))}>{"Negrilla Itálica"}</button>
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
                        html! {
                            <div>
                                <div dangerously_set_inner_html={preview_html} />
                            </div>
                        }
                    }
                }
            </div>
        }
    }
}