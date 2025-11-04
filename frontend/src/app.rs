use crate::components::statics::text::StaticTextComponent;
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
                    <StaticTextComponent template_id="329a252d-5241-4df0-91b0-4a3e831a2b35" />
                </WorkspaceGrid>
        }
    }
}
