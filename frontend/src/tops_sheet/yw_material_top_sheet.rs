use uuid::Uuid;
use web_sys::js_sys;
use yew::{html, Component, Context, Html, NodeRef, Properties};

pub struct YwMaterialTopSheet {
    pub id: String,

}

#[derive(Properties, PartialEq)]
pub struct Props {
    #[prop_or_default]
    pub children: Html,
    pub node_ref: NodeRef,
}

impl Component for YwMaterialTopSheet {
    type Message = ();
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            id: format!("id-{}", Uuid::new_v4()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <>
                <div class="top-sheet" id={self.id.clone()} ref={ctx.props().node_ref.clone()}>
                        { ctx.props().children.clone() }
                </div>
            </>
        }
    }
}

pub fn open_top_sheet(top_sheet_ref: NodeRef) {
    if let Some(top_sheet) = top_sheet_ref.cast::<web_sys::HtmlElement>() {
        let class_name = "show";
        let func = js_sys::Function::new_no_args(&format!(
            "document.querySelector('#{}').classList.add('{}')",
            top_sheet.id(),
            class_name
        ));
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&func, 50).unwrap();
    }
}

pub fn close_top_sheet(top_sheet_ref: NodeRef) {
    if let Some(top_sheet) = top_sheet_ref.cast::<web_sys::HtmlElement>() {
        let class_name = "show";
        let func = js_sys::Function::new_no_args(&format!(
            "document.querySelector('#{}').classList.remove('{}')",
            top_sheet.id(),
            class_name
        ));
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&func, 50).unwrap();
    }
}