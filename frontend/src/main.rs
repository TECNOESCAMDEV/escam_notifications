use crate::app::App;

mod app;
mod tops_sheet;
mod components;

fn main() {
    yew::Renderer::<App>::new().render();
}

