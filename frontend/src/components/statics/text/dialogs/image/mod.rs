use crate::components::statics::text::{Msg, StaticTextComponent};
use crate::tops_sheet::yw_material_top_sheet::close_top_sheet;
use yew::html::Scope;
use yew::prelude::*;

pub fn image_dialog(component: &StaticTextComponent, link: &Scope<StaticTextComponent>) -> Html {
    html! {
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
    }
}