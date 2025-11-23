//! Message types for the static text editor `Component`.
//!
//! This enum drives the component's update cycle (Elm-style) and groups all
//! user intents and async results that can affect the UI state.
//!
//! Variants
//! - `SetTab(String)`: Switch between tabs ("editor" or "preview").
//! - `UpdateText(String)`: Replace the editor content and push into history.
//! - `Undo` / `Redo`: Navigate the undo/redo stack.
//! - `ApplyStyle(String, ())`: Insert a style snippet at the selection (e.g., bold).
//! - `AutoResize`: Recompute textarea height and keep template in sync.
//! - `OpenFileDialog`: Programmatically click the hidden file input.
//! - `FileSelected(File)`: A file was chosen; insert an `[img:<uuid>]` tag and read bytes.
//! - `AddImageToTemplate { id, base64 }`: Add the image to the current template.
//! - `OpenImageDialogWithId(String)`: Open the modal/top sheet showing the selected image.
//! - `DeleteImage(String)`: Remove image from template and text.
//! - `Save`: Persist the current template to the backend.
//! - `SetTemplate(Option<Template>)`: Replace the in-memory template (load or reset).

use common::model::csv::ColumnCheck;

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
    SaveSucceeded,
    SetTemplate(Option<common::model::template::Template>),
    InsertCsvColumnPlaceholder(ColumnCheck),
    CsvColumnsUpdated(Vec<ColumnCheck>),
    OpenPdf,
    PdfLoaded,
}
