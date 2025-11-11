use yew::{html, Component, Context, Html, Properties};

pub struct CsvDataSourceComponent;

#[derive(Properties, PartialEq)]
pub struct CsvDataSourceProps {
    #[prop_or_default]
    pub template_id: Option<String>,
}

impl Component for CsvDataSourceComponent {
    type Message = ();
    type Properties = CsvDataSourceProps;

    fn create(_ctx: &Context<Self>) -> Self {
        CsvDataSourceComponent
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        html! {
            <button class="icon-btn" title="Fuente de datos CSV">
                <i class="material-icons">{"table_chart"}</i>
                <span class="icon-label">{"CSV"}</span>
            </button>
        }
    }
}
