use crate::app::App;

mod app;
mod tops_sheet;
mod components;
mod workspace_grid;

fn main() {
    yew::Renderer::<App>::new().render();
}

