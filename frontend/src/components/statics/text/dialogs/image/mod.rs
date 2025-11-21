use crate::components::statics::text::helpers::show_toast;
use crate::components::statics::text::{Msg, StaticTextComponent};
use crate::tops_sheet::yw_material_top_sheet::{close_top_sheet, YwMaterialTopSheet};
use web_sys::Event;
use yew::html::Scope;
use yew::prelude::*;

const MAX_FILE_SIZE: u32 = 4_000_000;

pub fn image_dialog(component: &StaticTextComponent, link: &Scope<StaticTextComponent>) -> Html {
    // Close callback
    let on_close = {
        let dialog_ref = component.image_dialog_ref.clone();
        Callback::from(move |_| close_top_sheet(dialog_ref.clone()))
    };

    // File input onchange handler
    let onchange = link.callback(|e: Event| {
        let input = e.target_unchecked_into::<web_sys::HtmlInputElement>();
        if let Some(files) = input.files() {
            if let Some(file) = files.get(0) {
                if file.size() > MAX_FILE_SIZE.into() {
                    show_toast(
                        &format!("El archivo es demasiado grande (máx. {} MB).", MAX_FILE_SIZE / 1_000_000)
                    );
                    return Msg::AutoResize;
                }
                return Msg::FileSelected(file);
            }
        }
        Msg::AutoResize
    });

    // Compute preview / controls content in one place
    let content: Html = match (&component.selected_image_id, &component.template) {
        (Some(id), Some(template)) => {
            if let Some(images) = &template.images {
                if let Some(image) = images.iter().find(|img| &img.id == id) {
                    let id_cloned = id.clone();
                    html! {
                        <>
                            <img
                                src={format!("data:image/*;base64,{}", image.base64)}
                                style="max-width:400px;max-height:400px;margin-bottom:24px;"
                            />
                            <button
                                style="padding:0.5rem 1rem;font-size:1rem;background:#d32f2f;color:#fff;border:none;border-radius:4px;cursor:pointer;"
                                onclick={link.callback(move |_| Msg::DeleteImage(id_cloned.clone()))}
                            >
                                { "Borrar" }
                            </button>
                        </>
                    }
                } else {
                    html! { <span style="color:#fff;">{"Imagen no encontrada"}</span> }
                }
            } else {
                html! { <span style="color:#fff;">{"Sin imágenes"}</span> }
            }
        }
        _ => html! { <span style="color:#fff;">{"No hay imagen seleccionada"}</span> },
    };

    html! {
        <YwMaterialTopSheet node_ref={component.image_dialog_ref.clone()}>
            <div style="position:fixed;top:0;left:0;width:100vw;height:100vh;background:rgba(0,0,0,0.85);z-index:9999;display:flex;flex-direction:column;align-items:center;justify-content:center;">
                <button
                    onclick={on_close}
                    style="position:absolute;top:24px;right:32px;z-index:10000;padding:0.5rem 1rem;font-size:1.5rem;background:#fff;border:none;border-radius:4px;cursor:pointer;"
                >
                    { "✕" }
                </button>

                <input
                    type="file"
                    accept="image/*"
                    ref={component.file_input_ref.clone()}
                    style="display: none;"
                    onchange={onchange}
                />

                { content }
            </div>
        </YwMaterialTopSheet>
    }
}