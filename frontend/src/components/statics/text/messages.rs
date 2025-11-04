#[derive(Clone)]
pub enum Msg {
    SetTab(String),
    UpdateText(String),
    Undo,
    Redo,
    ApplyStyle(String, ()),
    AutoResize,
    OpenFileDialog,
    FileSelected(web_sys::File),
    AddImageToTemplate { id: String, base64: String },
    OpenImageDialogWithId(String),
    DeleteImage(String),
    Save,
    SetTemplate(Option<common::model::template::Template>),
}