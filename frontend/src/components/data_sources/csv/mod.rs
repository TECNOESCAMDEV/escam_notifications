use common::jobs::JobStatus;
use common::model::csv::ColumnCheck;
use gloo_net::http::Request;
use gloo_timers::future::sleep;
use num_format::{Locale, ToFormattedString};
use serde_json::Value;
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;
use yew::{html, Component, Context, Html, Properties};

/// Component that triggers a CSV verification job and polls for status.
pub struct CsvDataSourceComponent {
    /// Flag indicating an ongoing verification.
    is_verifying: bool,
    /// Result of the verification once finished.
    verify_result: Option<Result<bool, String>>,
    /// Ticket UUID returned by the verify POST.
    job_ticket: Option<String>,
    /// Latest polled job status.
    job_status: Option<JobStatus>,
    /// Parsed column checks when job completes successfully.
    column_checks: Option<Vec<ColumnCheck>>,
    /// Template id for which verification has already been started (prevent duplicates).
    started_for_template: Option<String>,
}

#[derive(Properties, PartialEq)]
pub struct CsvDataSourceProps {
    #[prop_or_default]
    pub template_id: Option<String>,
}

pub enum CsvDataSourceMsg {
    VerifyCompleted(Result<bool, String>),
    TicketReceived(String),
    StatusUpdated(JobStatus),
    VerifyError(String),
}

impl Component for CsvDataSourceComponent {
    type Message = CsvDataSourceMsg;
    type Properties = CsvDataSourceProps;

    fn create(_ctx: &Context<Self>) -> Self {
        // Do not start network activity in `create`. Start after first render or on prop change.
        CsvDataSourceComponent {
            is_verifying: false,
            verify_result: None,
            job_ticket: None,
            job_status: None,
            column_checks: None,
            started_for_template: None,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
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
                    JobStatus::Pending => {
                        self.is_verifying = true;
                    }
                    JobStatus::InProgress(_) => {
                        self.is_verifying = true;
                    }
                    JobStatus::Completed(payload) => {
                        self.is_verifying = false;
                        // Payload contains a JSON string representing Vec<ColumnCheck>
                        match serde_json::from_str::<Vec<ColumnCheck>>(&payload) {
                            Ok(cols) => {
                                self.column_checks = Some(cols);
                                self.verify_result = Some(Ok(true));
                            }
                            Err(e) => {
                                self.verify_result =
                                    Some(Err(format!("Deserialize ColumnCheck: {}", e)));
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
        }
    }

    /// Called when properties change; if `template_id` transitions to Some, start the verification.
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

    fn view(&self, _ctx: &Context<Self>) -> Html {
        // Build a user-facing status label in Spanish per JobStatus:
        // Pending -> "Verificando"
        // InProgress(n) -> "Verificando: n"
        // Completed(_) -> "Verified"
        // Failed(msg) -> "Failed: msg"
        let status_text = if let Some(job_status) = &self.job_status {
            match job_status {
                JobStatus::Pending => "Verificando".to_string(),
                JobStatus::InProgress(n) => format!("Líneas verificadas: {}", n.to_formatted_string(&Locale::es)),
                JobStatus::Completed(_) => "CSV Verificado".to_string(),
                JobStatus::Failed(msg) => format!("Failed: {}", msg),
            }
        } else if self.is_verifying {
            "Verificando".to_string()
        } else if let Some(Ok(true)) = &self.verify_result {
            "Verified".to_string()
        } else if let Some(Err(e)) = &self.verify_result {
            format!("Failed: {}", e)
        } else {
            "CSV".to_string()
        };

        let btn_classes = if status_text.len() > 30 {
            "icon-btn limited"
        } else {
            "icon-btn"
        };

        html! {
            <button class={btn_classes} title="CSV data source">
                <i class="material-icons">{"table_chart"}</i>
                <span class="icon-label">{status_text}</span>
            </button>
        }
    }

    /// Called after render; kick off verification if it's the first render and `template_id` is Some.
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

/// Start the POST request that returns a ticket and begin polling the job status.
/// This function runs asynchronously and communicates results back to the component via `link`.
fn start_verification(link: html::Scope<CsvDataSourceComponent>, template_id: String) {
    spawn_local(async move {
        let url = "/api/data_sources/csv/verify";
        let body = serde_json::json!({ "uuid": template_id }).to_string();
        match Request::post(url)
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
                    // Since the API returns plain text with the ticket UUID, prefer the simple extractor:
                    let ticket = match extract_ticket_from_text(&text) {
                        Some(t) => t,
                        None => {
                            // Notify component of error and stop
                            link.send_message(CsvDataSourceMsg::VerifyCompleted(Err(
                                "Empty ticket returned from verify endpoint".to_string(),
                            )));
                            return;
                        }
                    };
                    link.send_message(CsvDataSourceMsg::TicketReceived(ticket.clone()));

                    // Start polling job status periodically
                    let poll_link = link.clone();
                    spawn_local(async move {
                        let mut finished = false;
                        while !finished {
                            sleep(Duration::from_secs(1)).await;
                            let status_url = format!("/api/data_sources/csv/status/{}", ticket);
                            match Request::get(&status_url).send().await {
                                Ok(resp) => {
                                    if let Ok(body_text) = resp.text().await {
                                        if let Some(json_val) =
                                            serde_json::from_str::<Value>(&body_text).ok()
                                        {
                                            if let Some(job_status) = parse_job_status(&json_val) {
                                                // Notify status to component
                                                poll_link.send_message(
                                                    CsvDataSourceMsg::StatusUpdated(
                                                        job_status.clone(),
                                                    ),
                                                );
                                                match job_status {
                                                    JobStatus::Completed(_)
                                                    | JobStatus::Failed(_) => {
                                                        finished = true;
                                                    }
                                                    _ => {}
                                                }
                                            } else {
                                                // Could not parse job status: send error and stop polling
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

/// Extract ticket UUID from plain text response (trimmed). Returns `None` if empty.
fn extract_ticket_from_text(text: &str) -> Option<String> {
    let s = text.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Parse job status via direct deserialization into JobStatus.
fn parse_job_status(v: &Value) -> Option<JobStatus> {
    serde_json::from_value(v.clone()).ok()
}
