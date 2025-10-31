use gloo_console::console_dbg;
use pulldown_cmark::{html, Parser};
use yew::prelude::*;

const BUTTON_STYLE: &str = "
                        .static-text-root {
                            width: 100%;
                        }
                        .icon-toolbar {
                            display: flex;
                            gap: 16px;
                            margin-bottom: 8px;
                            justify-content: flex-start;
                        }
                        .icon-btn {
                            background: #f5f5f5;
                            border: none;
                            border-radius: 4px;
                            padding: 4px 10px 0 10px;
                            cursor: pointer;
                            font-size: 20px;
                            transition: background 0.2s;
                            display: flex;
                            flex-direction: column;
                            align-items: center;
                            width: 56px;
                            height: 48px;
                            box-sizing: border-box;
                        }
                        .icon-btn.wide {
                            width: 90px;
                        }
                        .icon-btn:hover {
                            background: #e0e0e0;
                        }
                        .icon-label {
                            font-size: 10px;
                            color: #555;
                            margin-top: 2px;
                            text-align: center;
                            letter-spacing: 0.5px;
                            white-space: nowrap;
                        }
                        .material-icons {
                            vertical-align: middle;
                            font-size: 20px;
                            color: #333;
                        }
                        .tab-bar {
                            display: flex;
                            gap: 2px;
                            margin-bottom: 12px;
                            border-bottom: 1px solid #ddd;
                        }
                        .tab-btn {
                            background: #f5f5f5;
                            border: none;
                            border-radius: 4px 4px 0 0;
                            padding: 6px 18px;
                            cursor: pointer;
                            font-size: 14px;
                            color: #555;
                            margin-bottom: -1px;
                            border-bottom: 2px solid transparent;
                            transition: background 0.2s, border-bottom 0.2s;
                        }
                        .tab-btn.active {
                            background: #fff;
                            color: #222;
                            border-bottom: 2px solid #1976d2;
                            font-weight: bold;
                        }
                        .tab-btn:hover {
                            background: #e0e0e0;
                        }
                    ";

fn style_tag() -> Html {
    html! { <style>{BUTTON_STYLE}</style> }
}

fn icon_button(icon_name: &str, label: &str, onclick: Callback<yew::MouseEvent>, wide: bool) -> Html {
    let class = if wide { "icon-btn wide" } else { "icon-btn" };
    html! {
                            <button class={class} onclick={onclick}>
                                <i class="material-icons">{icon_name}</i>
                                <span class="icon-label">{label}</span>
                            </button>
                        }
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
                                <div class="static-text-root">
                                    { style_tag() }
                                    <div class="icon-toolbar">
                                        {icon_button("text_fields", "Normal", link.callback(|_| Msg::ApplyStyle("normal".to_string())), false)}
                                        {icon_button("format_bold", "Negrita", link.callback(|_| Msg::ApplyStyle("bold".to_string())), false)}
                                        {icon_button("format_italic", "Cursiva", link.callback(|_| Msg::ApplyStyle("italic".to_string())), false)}
                                        {icon_button("format_bold", "Negrita+Cursiva", link.callback(|_| Msg::ApplyStyle("bolditalic".to_string())), true)}
                                    </div>
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
                                        if self.active_tab == "editor" {
                                            html! {
                                                <textarea
                                                    value={self.text.clone()}
                                                    oninput={link.callback(|e: web_sys::InputEvent| {
                                                        let input: web_sys::HtmlTextAreaElement = e.target_unchecked_into();
                                                        Msg::UpdateText(input.value())
                                                    })}
                                                    rows={10}
                                                    cols={200}
                                                    style="width: 100%;"
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