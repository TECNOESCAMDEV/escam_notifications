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

/// Component that triggers a CSV verification job, polls status and provides upload + modal UI.
pub struct CsvDataSourceComponent {
    is_verifying: bool,
    verify_result: Option<Result<bool, String>>,
    job_ticket: Option<String>,
    job_status: Option<JobStatus>,
    column_checks: Option<Vec<ColumnCheck>>,
    started_for_template: Option<String>,

    // UI state
    show_modal: bool,
    file_input_ref: NodeRef,
    uploading: bool,
    upload_error: Option<String>,
    selected_column: Option<usize>,
}

impl CsvDataSourceComponent {
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

    /// Start upload using XHR + FormData to emulate the curl multipart form.
    fn start_upload(link: html::Scope<Self>, template_id: Option<String>, file: File) {
        // clone file and template string for closure
        let filename = file.name();
        let tpl = template_id.unwrap_or_default();
        spawn_local(async move {
            let url = "/api/data_sources/csv/upload";

            // Build FormData
            let form = web_sys::FormData::new().expect("FormData available");
            let json = format!("{{\"template_id\":\"{}\"}}", tpl);
            form.append_with_str("json", &json).ok();
            form.append_with_blob_and_filename("file", &file, &filename)
                .ok();

            // Create XHR
            let xhr = web_sys::XmlHttpRequest::new().expect("xhr");
            xhr.open_with_async("POST", &url, true).expect("open");

            // Handlers
            let xhr_clone = xhr.clone();
            let link_clone = link.clone();
            let onload = Closure::wrap(Box::new(move || {
                let status = xhr_clone.status().unwrap_or_default();
                if status >= 200 && status < 300 {
                    link_clone.send_message(CsvDataSourceMsg::UploadResult(Ok(())));
                } else {
                    let text = xhr_clone
                        .response_text()
                        .ok()
                        .and_then(|r| r)
                        .unwrap_or_default();
                    link_clone.send_message(CsvDataSourceMsg::UploadResult(Err(format!(
                        "HTTP {}: {}",
                        status, text
                    ))));
                }
            }) as Box<dyn FnMut()>);
            xhr.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();

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

            // Send
            xhr.send_with_opt_form_data(Some(&form)).ok();
        });
    }
}

#[derive(Properties, PartialEq)]
pub struct CsvDataSourceProps {
    #[prop_or_default]
    pub template_id: Option<String>,
    #[prop_or_default]
    pub on_column_selected: Option<Callback<ColumnCheck>>,
}

pub enum CsvDataSourceMsg {
    VerifyCompleted(Result<bool, String>),
    TicketReceived(String),
    StatusUpdated(JobStatus),
    VerifyError(String),

    // UI messages
    ToggleModal,
    TriggerFilePicker,
    FilePicked(File),
    UploadResult(Result<(), String>),
    SelectColumn(usize),
    DoubleClickColumn(usize),
}

impl Component for CsvDataSourceComponent {
    type Message = CsvDataSourceMsg;
    type Properties = CsvDataSourceProps;

    fn create(_ctx: &Context<Self>) -> Self {
        CsvDataSourceComponent {
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
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
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
                    JobStatus::Pending => self.is_verifying = true,
                    JobStatus::InProgress(_) => self.is_verifying = true,
                    JobStatus::Completed(payload) => {
                        self.is_verifying = false;
                        self.apply_completed(payload);
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

            // UI
            CsvDataSourceMsg::ToggleModal => {
                self.show_modal = !self.show_modal;
                self.upload_error = None;
                true
            }
            CsvDataSourceMsg::TriggerFilePicker => {
                // trigger the hidden file input
                if let Some(input) = self.file_input_ref.cast::<HtmlInputElement>() {
                    input.set_value(""); // clear previous file
                    input.click();
                }
                false
            }
            CsvDataSourceMsg::FilePicked(file) => {
                self.uploading = true;
                self.upload_error = None;
                // Kick off upload using current prop template id
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
                        self.show_modal = false;
                        // Always re-trigger verification after upload
                        if let Some(id) = ctx.props().template_id.clone() {
                            self.is_verifying = true;
                            // Clear previous results
                            self.column_checks = None;
                            // Update started_for_template to avoid double starts
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
                if let Some(cb) = &ctx.props().on_column_selected {
                    if let Some(cols) = &self.column_checks {
                        if let Some(col) = cols.get(idx) {
                            cb.emit(col.clone());
                        }
                    }
                }
                // Close modal after double-click selection
                self.show_modal = false;
                true
            }
        }
    }

    fn changed(&mut self, ctx: &Context<Self>, old_props: &Self::Properties) -> bool {
        if old_props.template_id != ctx.props().template_id {
            if let Some(id) = ctx.props().template_id.clone() {
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
        // Status text (same logic as before)
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

        // Determine if error state
        let is_error = match (&self.job_status, &self.verify_result) {
            (Some(JobStatus::Failed(_)), _) => true,
            (_, Some(Err(_))) => true,
            _ => false,
        };

        // Compute button classes
        let mut btn_classes = if status_text.len() > 30 {
            "icon-btn limited".to_string()
        } else {
            "icon-btn".to_string()
        };
        if is_error {
            btn_classes.push_str(" error");
        }
        let title_attr = status_text.clone();

        // column options from column_checks
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
                                    ondblclick={ondblclick}
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

        // Upload button state
        let upload_disabled = self.uploading;
        let upload_onclick = if upload_disabled {
            Callback::<MouseEvent>::noop()
        } else {
            ctx.link().callback(|_| CsvDataSourceMsg::TriggerFilePicker)
        };

        html! {
            <>
            <button
                class={btn_classes}
                title={title_attr}
                onclick={ctx.link().callback(|_| CsvDataSourceMsg::ToggleModal)}>
                <i class="material-icons">{"table_chart"}</i>
                <span class="icon-label">{status_text}</span>
            </button>

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
            </>
        }
    }

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

                    let poll_link = link.clone();
                    spawn_local(async move {
                        let mut finished = false;
                        while !finished {
                            sleep(Duration::from_secs(1)).await;
                            let status_url = format!("/api/data_sources/csv/status/{}", ticket);
                            match gloo_net::http::Request::get(&status_url).send().await {
                                Ok(resp) => {
                                    if let Ok(body_text) = resp.text().await {
                                        if let Some(json_val) =
                                            serde_json::from_str::<Value>(&body_text).ok()
                                        {
                                            if let Some(job_status) = parse_job_status(&json_val) {
                                                poll_link.send_message(
                                                    CsvDataSourceMsg::StatusUpdated(
                                                        job_status.clone(),
                                                    ),
                                                );
                                                match job_status {
                                                    JobStatus::Completed(_)
                                                    | JobStatus::Failed(_) => finished = true,
                                                    _ => {}
                                                }
                                            } else {
                                                poll_link.send_message(
                                                    CsvDataSourceMsg::VerifyError(
                                                        "Could not parse job status".into(),
                                                    ),
                                                );
                                                finished = true;
                                            }
                                        } else {
                                            poll_link.send_message(CsvDataSourceMsg::VerifyError(
                                                "Response is not valid JSON".into(),
                                            ));
                                            finished = true;
                                        }
                                    } else {
                                        poll_link.send_message(CsvDataSourceMsg::VerifyError(
                                            "Could not read response body".into(),
                                        ));
                                        finished = true;
                                    }
                                }
                                Err(e) => {
                                    poll_link
                                        .send_message(CsvDataSourceMsg::VerifyError(e.to_string()));
                                    finished = true;
                                }
                            }
                        }
                    });
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

fn extract_ticket_from_text(text: &str) -> Option<String> {
    let s = text.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn parse_job_status(v: &Value) -> Option<JobStatus> {
    serde_json::from_value(v.clone()).ok()
}
