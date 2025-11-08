use crate::components::statics::text::dialogs::variables::messages::Msg;
use crate::components::statics::text::dialogs::variables::properties::VariableCreatorProperties;
use crate::components::statics::text::dialogs::variables::state::VariableCreatorComponent;
use yew::{Component, Context, Html};

impl Component for VariableCreatorComponent {
    type Message = Msg;
    type Properties = VariableCreatorProperties;

    fn create(_ctx: &Context<Self>) -> Self {
        Self
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        todo!()
    }
}