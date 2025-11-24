use crate::components::statics::text::Msg::{ClosePdfDialog, PdfLoaded};
use crate::components::statics::text::StaticTextComponent;
use crate::tops_sheet::yw_material_top_sheet::{close_top_sheet, YwMaterialTopSheet};
use yew::html::Scope;
use yew::prelude::*;

pub fn pdf_dialog(component: &StaticTextComponent, link: &Scope<StaticTextComponent>) -> Html {
    let dialog_ref = component.pdf_viewer_dialog_ref.clone();
    let on_close = {
        let dr = dialog_ref.clone();
        let cb_link = link.clone();
        Callback::from(move |_| {
            close_top_sheet(dr.clone());
            cb_link.send_message(ClosePdfDialog);
        })
    };

    // Callback for when the iframe finishes loading -> send Msg::PdfLoaded
    let on_iframe_load = {
        let cb_link = link.clone();
        Callback::from(move |_: Event| {
            cb_link.send_message(PdfLoaded);
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
                        // Hide iframe while loading to prevent showing previous content,
                        // and show a full white overlay as placeholder.
                        let iframe_style = if component.pdf_loading {
                            "width:100%;height:100%;border:none;background:#fff;border-radius:4px;visibility:hidden;"
                        } else {
                            "width:100%;height:100%;border:none;background:#fff;border-radius:4px;visibility:visible;"
                        };

                        html! {
                            <div style="position:relative;width:80vw;height:80vh;">
                                // iframe occupies the whole container but stays hidden while generating
                                <iframe
                                    src={url.clone()}
                                    style={iframe_style}
                                    onload={on_iframe_load}
                                />

                                {
                                    if component.pdf_loading {
                                        // Full-size white overlay covering the iframe (prevents seeing previous PDF)
                                        html! {
                                            <div style="position:absolute;top:0;left:0;width:100%;height:100%;display:flex;align-items:center;justify-content:center;background:#fff;z-index:10001;">
                                                <div style="background:transparent;padding:24px;border-radius:8px;display:flex;flex-direction:column;align-items:center;">
                                                    <div class="spin" style="width:48px;height:48px;border:6px solid #ccc;border-top-color:#1976d2;border-radius:50%;animation:spin 1s linear infinite;"></div>
                                                    <div style="margin-top:12px;color:#000;">{"Generando PDF..."}</div>
                                                </div>
                                                <style>{r#"
                                                        @keyframes spin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
                                                    "#}</style>
                                            </div>
                                        }
                                    } else {
                                        html! { <></> }
                                    }
                                }
                            </div>
                        }
                    } else {
                        html! { <div style="color:#fff;">{"No hay PDF disponible"}</div> }
                    }
                }
            </div>
        </YwMaterialTopSheet>
    }
}