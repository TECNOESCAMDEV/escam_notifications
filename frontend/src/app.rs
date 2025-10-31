use crate::components::statics::text::static_text_component::StaticTextComponent;
use crate::workspace_grid::WorkspaceGrid;
use yew::{html, Component, Context, Html};

pub struct App;

impl Component for App {
    type Message = ();
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        html! {
                <WorkspaceGrid columns={1} rows={3}>
                    <StaticTextComponent />
                </WorkspaceGrid>
            }
    }
}