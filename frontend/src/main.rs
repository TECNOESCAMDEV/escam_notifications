use crate::app::App;

mod app;
mod tops_sheet;

fn main() {
    yew::Renderer::<App>::new().render();
}

