use crate::components::statics::text::StaticTextComponent;
use crate::tops_sheet::yw_material_top_sheet::{close_top_sheet, YwMaterialTopSheet};
use yew::html::Scope;
use yew::prelude::*;

pub fn pdf_dialog(component: &StaticTextComponent, _link: &Scope<StaticTextComponent>) -> Html {
    let dialog_ref = component.pdf_viewer_dialog_ref.clone();
    let on_close = {
        let dr = dialog_ref.clone();
        Callback::from(move |_| {
            close_top_sheet(dr.clone());
        })
    };

    html! {
        <YwMaterialTopSheet node_ref={dialog_ref}>
            <div style="position:fixed;top:0;left:0;width:100vw;height:100vh;background:rgba(0,0,0,0.85);z-index:9999;display:flex;flex-direction:column;align-items:center;justify-content:center;">
                <button
                    onclick={on_close}
                    style="position:absolute;top:24px;right:32px;z-index:10000;padding:0.5rem 1rem;font-size:1.5rem;background:#fff;border:none;border-radius:4px;cursor:pointer;"
                >
                    { "✕" }
                </button>

                {
                    if let Some(url) = &component.pdf_url {
                        html! {
                            <iframe
                                src={url.clone()}
                                style="width:80vw;height:80vh;border:none;background:#fff;border-radius:4px;"
                            />
                        }
                    } else {
                        html! { <div style="color:#fff;">{"No hay PDF disponible"}</div> }
                    }
                }
            </div>
        </YwMaterialTopSheet>
    }
}
