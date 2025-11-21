//! Static text editor: root module wiring the Yew `Component` implementation
//! with submodules for state, update logic, view rendering, and helpers.
//!
//! Responsibilities
//! - Re-export selected types (`Msg`, `StaticTextProps`, `StaticTextComponent`).
//! - Provide the `Component` implementation that delegates to `update::update` and `view::view`.
//! - On first render, load an existing template (if `template_id` is provided) or
//!   create a fresh one and notify users via toast messages (in Spanish).

use gloo_net::http::Request;
use js_sys::Reflect;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::BeforeUnloadEvent;
use yew::platform::spawn_local;
use yew::prelude::*;

mod helpers;
mod messages;
mod props;
mod state;
mod update;
mod view;
mod dialogs;

use helpers::{create_empty_template, show_toast};
pub use messages::Msg;
pub use props::StaticTextProps;
pub use state::StaticTextComponent;

impl Component for StaticTextComponent {
    type Message = Msg;
    type Properties = StaticTextProps;

    fn create(_ctx: &Context<Self>) -> Self {
        StaticTextComponent::new()
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        update::update(self, ctx, msg)
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        view::view(self, ctx)
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render && !self.loaded {
            self.loaded = true;

            // Initialize the global dirty flag
            if let Some(window) = web_sys::window() {
                let _ = Reflect::set(
                    &window,
                    &JsValue::from_str("app_dirty"),
                    &JsValue::from_bool(false),
                );
            }

            // Register beforeunload event to warn about unsaved changes
            if let Some(window) = web_sys::window() {
                let closure = Closure::wrap(Box::new(move |evt: Event| {
                    let window = web_sys::window().unwrap();
                    let dirty = Reflect::get(&window, &JsValue::from_str("app_dirty"))
                        .ok()
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if dirty {
                        // Try to get a custom message, fallback to default
                        let message = Reflect::get(&window, &JsValue::from_str("app_dirty_message"))
                            .ok()
                            .and_then(|v| v.as_string())
                            .unwrap_or_else(|| "Hay cambios sin guardar.".to_string());

                        // prevents to close the tab
                        if let Some(bu) = evt.dyn_ref::<BeforeUnloadEvent>() {
                            bu.prevent_default();
                        }
                        let _ = Reflect::set(
                            evt.as_ref(),
                            &JsValue::from_str("returnValue"),
                            &JsValue::from_str(&message),
                        );
                    }
                }) as Box<dyn FnMut(_)>);

                window
                    .add_event_listener_with_callback("beforeunload", closure.as_ref().unchecked_ref())
                    .ok();

                // Avoid dropping the closure
                closure.forget();
            }

            if let Some(template_id) = &ctx.props().template_id {
                let link = ctx.link().clone();
                let template_id = template_id.clone();
                spawn_local(async move {
                    let response = Request::get(&format!("/api/templates/{}", template_id))
                        .send()
                        .await;

                    match response {
                        Ok(resp) if resp.status() == 200 => {
                            if let Ok(template) =
                                resp.json::<common::model::template::Template>().await
                            {
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

fn create_new_template(link: html::Scope<StaticTextComponent>) {
    link.send_message_batch(vec![
        Msg::SetTemplate(Some(create_empty_template())),
        Msg::UpdateText(String::new()),
        Msg::SetTab("editor".to_string()),
    ]);
    show_toast("Error cargando plantilla. Se creó una nueva.");
}
