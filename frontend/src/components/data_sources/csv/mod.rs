//! # CSV Data Source Management Component
//!
//! This module defines the `CsvDataSourceComponent`, a Yew component responsible for managing
//! CSV data sources associated with a template. It provides a complete UI and logic flow for
//! uploading CSV files, triggering asynchronous server-side verification, polling for status
//! updates, and displaying the results.
//!
//! ## Core Features:
//!
//! - **File Upload**: Implements a modal UI for selecting and uploading a `.csv` file. It uses
//!   `XmlHttpRequest` to handle the `multipart/form-data` upload, which includes both the
//!   file binary and the associated `template_id`.
//!
//! - **Asynchronous Verification**: After an upload or when the component first loads with a
//!   `template_id`, it sends a request to the backend to start a background verification job
//!   for the CSV data.
//!
//! - **Status Polling**: Upon receiving a `job_ticket` from the verification endpoint, the
//!   component periodically polls a status endpoint (`/api/data_sources/csv/status/{job_id}`)
//!   to fetch real-time progress updates.
//!
//! - **Interactive Modal UI**: A modal dialog serves as the central interface. It displays:
//!   - The current verification status (e.g., "Verifying...", "Verified", "Error").
//!   - An upload button to trigger the file selection flow.
//!   - A confirmation dialog to warn users before replacing an existing CSV.
//!   - A list of detected columns from a successfully verified CSV.
//!
//! - **Column Selection**: Users can double-click a column name from the verified list. This
//!   action emits a `on_column_selected` callback, allowing parent components (like the main
//!   editor) to insert the column placeholder (e.g., `{{column_name}}`) into the template text.
//!
//! ## Workflow:
//!
//! 1.  The component is initialized with a `template_id` property.
//! 2.  On `rendered`, it automatically triggers `start_verification` for the template's
//!     currently associated CSV.
//! 3.  The user can click the main component button to open the management modal.
//! 4.  Inside the modal, the user can initiate a new file upload. A warning is displayed
//!     to confirm the action, as it may affect existing placeholders in the template.
//! 5.  Once a file is selected, `start_upload` sends it to the backend.
//! 6.  Upon successful upload, `start_verification` is called again for the new file.
//! 7.  The `poll_status` task begins, updating the component's state (`job_status`) as the
//!     backend processes the file.
//! 8.  If the job completes successfully, the `column_checks` are populated, and the list of
//!     columns is rendered in the modal, ready for selection.

use common::jobs::JobStatus;
use common::model::csv::ColumnCheck;
use gloo_timers::future::sleep;
use num_format::{Locale, ToFormattedString};
use serde_json::Value;
use std::time::Duration;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Event, File, HtmlInputElement};
use yew::{html, Callback, Component, Context, Html, MouseEvent, NodeRef, Properties};

/// Manages the state and UI for CSV data source operations, including verification,
/// status polling, and file uploads.
pub struct CsvDataSourceComponent {
    // --- State for the verification job ---
    /// True if a verification job is currently active.
    is_verifying: bool,
    /// The final result of a verification job (either success or an error message).
    verify_result: Option<Result<bool, String>>,
    /// The unique ticket ID for the current background job.
    job_ticket: Option<String>,
    /// The current status of the background job, updated via polling.
    job_status: Option<JobStatus>,
    /// A list of detected columns and their properties, received upon successful verification.
    column_checks: Option<Vec<ColumnCheck>>,
    /// The template ID for which the last verification was started, to prevent redundant checks.
    started_for_template: Option<String>,

    // --- UI State ---
    /// Controls the visibility of the main management modal.
    show_modal: bool,
    /// A reference to the hidden file input element.
    file_input_ref: NodeRef,
    /// True if a file upload is in progress.
    uploading: bool,
    /// An error message related to the file upload process.
    upload_error: Option<String>,
    /// The index of the currently selected column in the UI list.
    selected_column: Option<usize>,
    /// Controls the visibility of the confirmation dialog shown before uploading.
    show_confirm_upload: bool,
}

impl CsvDataSourceComponent {
    /// Parses the payload from a `JobStatus::Completed` message and updates the component's state.
    ///
    /// # Arguments
    /// * `payload` - A JSON string expected to contain a `Vec<ColumnCheck>`.
    fn apply_completed(&mut self, payload: String) {
        match serde_json::from_str::<Vec<ColumnCheck>>(&payload) {
            Ok(cols) => {
                self.column_checks = Some(cols);
                self.verify_result = Some(Ok(true));
            }
            Err(e) => {
                self.column_checks = None;
                self.verify_result = Some(Err(format!("Deserialize ColumnCheck: {}", e)));
            }
        }
    }

    /// Initiates a file upload using `XmlHttpRequest` to send a `multipart/form-data` request.
    ///
    /// This approach is used to construct a request that matches the backend's expectations,
    /// similar to a `curl` command with multiple form parts (`json` and `file`).
    ///
    /// # Arguments
    /// * `link` - The component's scope, used to send messages back (e.g., `UploadResult`).
    /// * `template_id` - The ID of the template to associate the CSV with.
    /// * `file` - The `File` object to be uploaded.
    fn start_upload(link: html::Scope<Self>, template_id: Option<String>, file: File) {
        let filename = file.name();
        let tpl = template_id.unwrap_or_default();
        spawn_local(async move {
            let url = "/api/data_sources/csv/upload";

            // Build the FormData payload.
            let form = web_sys::FormData::new().expect("FormData should be available");
            let json = format!("{{\"template_id\":\"{}\"}}", tpl);
            form.append_with_str("json", &json).ok();
            form.append_with_blob_and_filename("file", &file, &filename)
                .ok();

            // Create and configure the XmlHttpRequest.
            let xhr = web_sys::XmlHttpRequest::new().expect("XHR should be available");
            xhr.open_with_async("POST", &url, true)
                .expect("Failed to open XHR");

            // Define the `onload` handler for a successful request.
            let xhr_clone = xhr.clone();
            let link_clone = link.clone();
            let onload = Closure::wrap(Box::new(move || {
                let status = xhr_clone.status().unwrap_or_default();
                if (200..300).contains(&status) {
                    link_clone.send_message(CsvDataSourceMsg::UploadResult(Ok(())));
                } else {
                    let text = xhr_clone.response_text().ok().flatten().unwrap_or_default();
                    link_clone.send_message(CsvDataSourceMsg::UploadResult(Err(format!(
                        "HTTP {}: {}",
                        status, text
                    ))));
                }
            }) as Box<dyn FnMut()>);
            xhr.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget(); // The closure needs to live as long as the request.

            // Define the `onerror` handler for network errors.
            let xhr_err = xhr.clone();
            let link_err = link.clone();
            let onerror = Closure::wrap(Box::new(move || {
                let status = xhr_err.status().unwrap_or_default();
                link_err.send_message(CsvDataSourceMsg::UploadResult(Err(format!(
                    "Network error, status {}",
                    status
                ))));
            }) as Box<dyn FnMut()>);
            xhr.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            onerror.forget();

            // Send the request.
            xhr.send_with_opt_form_data(Some(&form)).ok();
        });
    }
}

/// Properties for the `CsvDataSourceComponent`.
#[derive(Properties, PartialEq)]
pub struct CsvDataSourceProps {
    /// The unique ID of the template this data source is associated with.
    #[prop_or_default]
    pub template_id: Option<String>,
    /// A callback that is emitted when a user double-clicks a column name in the modal.
    /// The parent component can use this to insert the column into the editor.
    #[prop_or_default]
    pub on_column_selected: Option<Callback<ColumnCheck>>,
    /// A callback that is emitted when a CSV has been successfully verified, providing the
    /// new set of column checks to the parent.
    #[prop_or_default]
    pub on_csv_changed: Option<Callback<Vec<ColumnCheck>>>,
}

/// Messages that drive state changes within the `CsvDataSourceComponent`.
pub enum CsvDataSourceMsg {
    // --- Job Lifecycle Messages ---
    /// Sent when the initial verification request completes (or fails immediately).
    VerifyCompleted(Result<bool, String>),
    /// Sent when the backend returns a job ticket after starting verification.
    TicketReceived(String),
    /// Sent by the polling task with the latest job status.
    StatusUpdated(JobStatus),
    /// Sent when an error occurs during the polling process.
    VerifyError(String),

    // --- UI Interaction Messages ---
    /// Toggles the visibility of the main management modal.
    ToggleModal,
    /// Initiates the file selection process by showing the confirmation dialog.
    TriggerFilePicker,
    /// Sent when a file is selected from the file input.
    FilePicked(File),
    /// Sent from the `start_upload` task with the result of the upload attempt.
    UploadResult(Result<(), String>),
    /// Sent when a user single-clicks a column in the list.
    SelectColumn(usize),
    /// Sent when a user double-clicks a column, triggering the `on_column_selected` callback.
    DoubleClickColumn(usize),

    // --- Confirmation Dialog Messages ---
    /// User confirmed the warning and wants to proceed with file selection.
    AcceptUploadWarning,
    /// User cancelled the upload process from the confirmation dialog.
    RejectUploadWarning,
}

impl Component for CsvDataSourceComponent {
    type Message = CsvDataSourceMsg;
    type Properties = CsvDataSourceProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            is_verifying: false,
            verify_result: None,
            job_ticket: None,
            job_status: None,
            column_checks: None,
            started_for_template: None,
            show_modal: false,
            file_input_ref: NodeRef::default(),
            uploading: false,
            upload_error: None,
            selected_column: None,
            show_confirm_upload: false,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            // --- Job Lifecycle Updates ---
            CsvDataSourceMsg::VerifyCompleted(res) => {
                self.is_verifying = false;
                self.verify_result = Some(res);
                true
            }
            CsvDataSourceMsg::TicketReceived(ticket) => {
                self.job_ticket = Some(ticket);
                self.job_status = Some(JobStatus::Pending);
                true
            }
            CsvDataSourceMsg::StatusUpdated(status) => {
                self.job_status = Some(status.clone());
                match status {
                    JobStatus::Pending | JobStatus::InProgress(_) => {
                        self.is_verifying = true;
                    }
                    JobStatus::Completed(payload) => {
                        self.is_verifying = false;
                        self.apply_completed(payload);

                        // Notify the parent component about the new set of columns.
                        if let Some(cb) = &ctx.props().on_csv_changed {
                            if let Some(cols) = &self.column_checks {
                                cb.emit(cols.clone());
                            }
                        }
                    }
                    JobStatus::Failed(err_msg) => {
                        self.is_verifying = false;
                        self.verify_result = Some(Err(err_msg));
                    }
                }
                true
            }
            CsvDataSourceMsg::VerifyError(e) => {
                self.is_verifying = false;
                self.verify_result = Some(Err(e));
                true
            }

            // --- UI and Upload Flow ---
            CsvDataSourceMsg::ToggleModal => {
                self.show_modal = !self.show_modal;
                self.upload_error = None; // Clear any previous errors when opening/closing.
                true
            }
            CsvDataSourceMsg::TriggerFilePicker => {
                // Instead of directly opening the file picker, show a confirmation dialog first.
                self.show_confirm_upload = true;
                true
            }
            CsvDataSourceMsg::AcceptUploadWarning => {
                self.show_confirm_upload = false;
                // Programmatically click the hidden file input to open the file dialog.
                if let Some(input) = self.file_input_ref.cast::<HtmlInputElement>() {
                    input.set_value(""); // Clear previous selection.
                    input.click();
                }
                false // No re-render needed, the browser handles the file dialog.
            }
            CsvDataSourceMsg::RejectUploadWarning => {
                self.show_confirm_upload = false;
                true
            }
            CsvDataSourceMsg::FilePicked(file) => {
                self.uploading = true;
                self.upload_error = None;
                let link = ctx.link().clone();
                let tpl = ctx.props().template_id.clone();
                Self::start_upload(link, tpl, file);
                true
            }
            CsvDataSourceMsg::UploadResult(res) => {
                self.uploading = false;
                match res {
                    Ok(()) => {
                        self.upload_error = None;
                        self.show_modal = false; // Close modal on successful upload.
                        // Always trigger a new verification after a successful upload.
                        if let Some(id) = ctx.props().template_id.clone() {
                            self.is_verifying = true;
                            self.column_checks = None; // Clear old results.
                            self.started_for_template = Some(id.clone());
                            start_verification(ctx.link().clone(), id);
                        }
                    }
                    Err(e) => {
                        self.upload_error = Some(e);
                    }
                }
                true
            }
            CsvDataSourceMsg::SelectColumn(idx) => {
                self.selected_column = Some(idx);
                true
            }
            CsvDataSourceMsg::DoubleClickColumn(idx) => {
                self.selected_column = Some(idx);
                // Emit the selected column data to the parent component.
                if let Some(cb) = &ctx.props().on_column_selected {
                    if let Some(cols) = &self.column_checks {
                        if let Some(col) = cols.get(idx) {
                            cb.emit(col.clone());
                        }
                    }
                }
                self.show_modal = false; // Close modal after selection.
                true
            }
        }
    }

    /// Detects changes in properties, primarily to trigger verification when the `template_id` changes.
    fn changed(&mut self, ctx: &Context<Self>, old_props: &Self::Properties) -> bool {
        if old_props.template_id != ctx.props().template_id {
            if let Some(id) = ctx.props().template_id.clone() {
                // Avoid re-triggering if verification for this ID has already been started.
                if self.started_for_template.as_deref() != Some(&id) {
                    self.is_verifying = true;
                    self.started_for_template = Some(id.clone());
                    start_verification(ctx.link().clone(), id);
                    return true;
                }
            }
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        // Determine the status text to display on the main button.
        let status_text = if let Some(job_status) = &self.job_status {
            match job_status {
                JobStatus::Pending => "Verificando CSV...".to_string(),
                JobStatus::InProgress(n) => {
                    format!("Líneas verificadas: {}", n.to_formatted_string(&Locale::es))
                }
                JobStatus::Completed(_) => "CSV Verificado".to_string(),
                JobStatus::Failed(msg) => format!("Error: {}", msg),
            }
        } else if self.is_verifying {
            "Verificando CSV...".to_string()
        } else if let Some(Ok(true)) = &self.verify_result {
            "CSV Verificado".to_string()
        } else if let Some(Err(e)) = &self.verify_result {
            format!("Error: {}", e)
        } else {
            "CSV".to_string()
        };

        // Determine if the component is in an error state to apply specific styling.
        let is_error = matches!(
            (&self.job_status, &self.verify_result),
            (Some(JobStatus::Failed(_)), _) | (_, Some(Err(_)))
        );

        // Compute CSS classes for the main button.
        let mut btn_classes = if status_text.len() > 30 {
            "icon-btn limited".to_string()
        } else {
            "icon-btn".to_string()
        };
        if is_error {
            btn_classes.push_str(" error");
        }
        let title_attr = status_text.clone();

        // Render the list of detected columns if available.
        let column_options = if let Some(cols) = &self.column_checks {
            html! {
                <div class="modal-section">
                    <h3>{"Columnas detectadas"}</h3>
                    <div class="column-list">
                        { for cols.iter().enumerate().map(|(i, c)| {
                            let idx = i;
                            let label = c.title.clone();
                            let tooltip = format!("Haz doble click en '{}' para insertarla en la plantilla", label.clone());
                            let onclick = ctx.link().callback(move |_| CsvDataSourceMsg::SelectColumn(idx));
                            let ondblclick = ctx.link().callback(move |_| CsvDataSourceMsg::DoubleClickColumn(idx));
                            html! {
                                <button
                                    class="col-option"
                                    {onclick}
                                    {ondblclick}
                                    title={tooltip}
                                    aria-label={format!("Insertar columna {}", label.clone())}>
                                    { label }
                                </button>
                            }
                        })}
                    </div>
                </div>
            }
        } else {
            html! {}
        };

        // Configure the upload button's state (disabled while uploading).
        let upload_disabled = self.uploading;
        let upload_onclick = if upload_disabled {
            Callback::noop()
        } else {
            ctx.link().callback(|_| CsvDataSourceMsg::TriggerFilePicker)
        };

        html! {
            <>
            // Main button to open the modal.
            <button
                class={btn_classes}
                title={title_attr}
                onclick={ctx.link().callback(|_| CsvDataSourceMsg::ToggleModal)}>
                <i class="material-icons">{"table_chart"}</i>
                <span class="icon-label">{status_text}</span>
            </button>

            // The main management modal.
            { if self.show_modal {
                html! {
                    <div class="modal-overlay" onclick={ctx.link().callback(|_| CsvDataSourceMsg::ToggleModal)}>
                        <div class="modal-card" onclick={|e: MouseEvent| e.stop_propagation()}>
                            <header class="modal-header">
                                <div class="modal-header-left">
                                    <i class="material-icons header-icon">{"table_chart"}</i>
                                    <h2 class="modal-title">{"CSV - Manager"}</h2>
                                </div>
                                <button class="close-btn" onclick={ctx.link().callback(|_| CsvDataSourceMsg::ToggleModal)}>{"✕"}</button>
                            </header>
                            <div class="modal-body">
                                <section class="modal-section upload-section">
                                    <h3>{"Subir CSV"}</h3>
                                    <p class="muted">{"Selecciona un archivo .csv como fuente de datos para tu plantilla."}</p>
                                    <p class="muted" style="color: #a00;">
                                        {"Advertencia: al subir un nuevo CSV, las etiquetas en el documento que no estén presentes en el CSV procesado pueden ser purgadas."}
                                    </p>
                                    <div class="upload-actions">
                                        <button
                                            class="primary upload-btn"
                                            disabled={upload_disabled}
                                            onclick={upload_onclick}
                                            aria-busy={self.uploading.to_string()}
                                            title={ if upload_disabled { "Subiendo..." } else { "Subir archivo" } }>
                                            <i class="material-icons">{"file_upload"}</i>
                                            { if self.uploading { " Subiendo..." } else { " Subir archivo" } }
                                        </button>
                                        // Hidden file input, triggered programmatically.
                                        <input ref={self.file_input_ref.clone()}
                                            type="file"
                                            accept=".csv"
                                            style="display:none"
                                            onchange={ctx.link().callback(|event: Event| {
                                                let input: HtmlInputElement = event.target().unwrap().dyn_into().unwrap();
                                                if let Some(list) = input.files() {
                                                    if let Some(file) = list.get(0) {
                                                        return CsvDataSourceMsg::FilePicked(file);
                                                    }
                                                }
                                                CsvDataSourceMsg::UploadResult(Err("No file selected".into()))
                                            })}
                                        />
                                        { if let Some(err) = &self.upload_error {
                                            html! { <p class="error">{ err }</p> }
                                        } else { html!{} } }
                                    </div>
                                </section>

                                { column_options }
                            </div>

                            <footer class="modal-footer">
                                <button class="secondary close-btn" onclick={ctx.link().callback(|_| CsvDataSourceMsg::ToggleModal)}>{"Cerrar"}</button>
                            </footer>
                        </div>
                    </div>
                }
            } else {
                html! {}
            } }

            // Confirmation dialog shown before triggering the file picker.
            { if self.show_confirm_upload {
                    html! {
                        <div class="modal-overlay" onclick={ctx.link().callback(|_| CsvDataSourceMsg::RejectUploadWarning)}>
                            <div class="modal-card" onclick={|e: MouseEvent| e.stop_propagation()}>
                                <header class="modal-header">
                                    <h2 class="modal-title">{"Advertencia: reemplazo de etiquetas"}</h2>
                                </header>
                                <div class="modal-body">
                                    <p>
                                        {"Estás a punto de subir un nuevo CSV. Las etiquetas (placeholders) que estén actualmente en el documento y que no aparezcan en el CSV procesado pueden ser eliminadas (purgadas). ¿Deseas continuar?"}
                                    </p>
                                </div>
                                <footer class="modal-footer">
                                    <button class="secondary" onclick={ctx.link().callback(|_| CsvDataSourceMsg::RejectUploadWarning)}>{"Cancelar"}</button>
                                    <button class="primary" onclick={ctx.link().callback(|_| CsvDataSourceMsg::AcceptUploadWarning)}>{"Continuar y seleccionar archivo"}</button>
                                </footer>
                            </div>
                        </div>
                    }
                } else {
                    html! {}
                }
            }
            </>
        }
    }

    /// Called after the component has been rendered.
    /// On the first render, it triggers the initial verification check.
    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            if let Some(id) = ctx.props().template_id.clone() {
                if self.started_for_template.as_deref() != Some(&id) {
                    self.is_verifying = true;
                    self.started_for_template = Some(id.clone());
                    start_verification(ctx.link().clone(), id);
                }
            }
        }
    }
}

/// Kicks off the CSV verification process by calling the backend endpoint.
/// If successful, it starts the polling task.
fn start_verification(link: html::Scope<CsvDataSourceComponent>, template_id: String) {
    spawn_local(async move {
        let url = "/api/data_sources/csv/verify";
        let body = serde_json::json!({ "uuid": template_id }).to_string();
        match gloo_net::http::Request::post(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .unwrap()
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                if status == 200 {
                    // Extract the job ticket from the response.
                    let ticket = match extract_ticket_from_text(&text) {
                        Some(t) => t,
                        None => {
                            link.send_message(CsvDataSourceMsg::VerifyCompleted(Err(
                                "Empty ticket returned from verify endpoint".to_string(),
                            )));
                            return;
                        }
                    };
                    link.send_message(CsvDataSourceMsg::TicketReceived(ticket.clone()));

                    // Spawn a new task to poll for the job status.
                    spawn_local(poll_status(link.clone(), ticket));
                } else {
                    link.send_message(CsvDataSourceMsg::VerifyCompleted(Err(format!(
                        "HTTP {}: {}",
                        status, text
                    ))));
                }
            }
            Err(err) => {
                link.send_message(CsvDataSourceMsg::VerifyCompleted(Err(err.to_string())));
            }
        }
    });
}

/// An async task that periodically polls the job status endpoint until the job
/// is completed or has failed.
async fn poll_status(poll_link: html::Scope<CsvDataSourceComponent>, ticket: String) {
    let mut finished = false;
    while !finished {
        sleep(Duration::from_secs(1)).await;
        let status_url = format!("/api/data_sources/csv/status/{}", ticket);
        match gloo_net::http::Request::get(&status_url).send().await {
            Ok(resp) => {
                if let Ok(body_text) = resp.text().await {
                    match serde_json::from_str::<Value>(&body_text) {
                        Ok(json_val) => {
                            if let Some(job_status) = parse_job_status(&json_val) {
                                // Send the updated status back to the component.
                                poll_link.send_message(CsvDataSourceMsg::StatusUpdated(
                                    job_status.clone(),
                                ));
                                // Stop polling if the job is in a terminal state.
                                if matches!(
                                    job_status,
                                    JobStatus::Completed(_) | JobStatus::Failed(_)
                                ) {
                                    finished = true;
                                }
                            } else {
                                poll_link.send_message(CsvDataSourceMsg::VerifyError(
                                    "Could not parse job status".into(),
                                ));
                                finished = true;
                            }
                        }
                        Err(_) => {
                            poll_link.send_message(CsvDataSourceMsg::VerifyError(
                                "Response is not valid JSON".into(),
                            ));
                            finished = true;
                        }
                    }
                } else {
                    poll_link.send_message(CsvDataSourceMsg::VerifyError(
                        "Could not read response body".into(),
                    ));
                    finished = true;
                }
            }
            Err(e) => {
                poll_link.send_message(CsvDataSourceMsg::VerifyError(e.to_string()));
                finished = true;
            }
        }
    }
}

/// Extracts the job ticket string from the raw text response.
fn extract_ticket_from_text(text: &str) -> Option<String> {
    let s = text.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Parses a `JobStatus` from a generic `serde_json::Value`.
fn parse_job_status(v: &Value) -> Option<JobStatus> {
    serde_json::from_value(v.clone()).ok()
}
