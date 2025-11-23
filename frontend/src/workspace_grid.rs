use yew::{html, Children, Component, Context, Html, Properties};

#[derive(Properties, PartialEq)]
pub struct WorkspaceGridProps {
    pub columns: usize,
    pub rows: usize,
    pub children: Children,
}

pub struct WorkspaceGrid;

impl Component for WorkspaceGrid {
    type Message = ();
    type Properties = WorkspaceGridProps;

    fn create(_ctx: &Context<Self>) -> Self {
        WorkspaceGrid
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let style = format!(
            "display: grid;
             grid-template-columns: repeat({}, 1fr);
             grid-template-rows: repeat({}, 1fr);
             width: 19.59cm;
             height: 27.94cm;
             margin: auto;
             padding: 10mm;
             background: white;
             box-shadow: 0 0 8px #ccc;",
            props.columns, props.rows
        );

        html! {
            <div style={style}>
                { for props.children.iter() }
            </div>
        }
    }
}