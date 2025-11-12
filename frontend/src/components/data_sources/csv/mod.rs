use common::jobs::JobStatus;
use common::model::csv::ColumnCheck;
use gloo_net::http::Request;
use gloo_timers::future::sleep;
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

    fn create(ctx: &Context<Self>) -> Self {
        let mut instance = CsvDataSourceComponent {
            is_verifying: false,
            verify_result: None,
            job_ticket: None,
            job_status: None,
            column_checks: None,
        };

        if let Some(id) = ctx.props().template_id.clone() {
            instance.is_verifying = true;
            let link = ctx.link().clone();

            // Initial POST request that returns a ticket (uuid)
            spawn_local(async move {
                let url = "/api/data_sources/csv/verify";
                let body = serde_json::json!({ "uuid": id }).to_string();
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
                            // Try to extract a ticket from JSON or plain text
                            let ticket =
                                extract_ticket_from_text(&text).unwrap_or_else(|| text.clone());
                            link.send_message(CsvDataSourceMsg::TicketReceived(ticket.clone()));

                            // Start polling job status periodically
                            let poll_link = link.clone();
                            spawn_local(async move {
                                let mut finished = false;
                                while !finished {
                                    sleep(Duration::from_secs(1)).await;
                                    let status_url =
                                        format!("/api/data_sources/csv/status/{}", ticket);
                                    match Request::get(&status_url).send().await {
                                        Ok(resp) => {
                                            if let Ok(body_text) = resp.text().await {
                                                if let Some(json_val) =
                                                    serde_json::from_str::<Value>(&body_text).ok()
                                                {
                                                    if let Some(job_status) =
                                                        parse_job_status(&json_val)
                                                    {
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
                                                    poll_link.send_message(
                                                        CsvDataSourceMsg::VerifyError(
                                                            "Response is not valid JSON".into(),
                                                        ),
                                                    );
                                                    finished = true;
                                                }
                                            } else {
                                                poll_link.send_message(
                                                    CsvDataSourceMsg::VerifyError(
                                                        "Could not read response body".into(),
                                                    ),
                                                );
                                                finished = true;
                                            }
                                        }
                                        Err(e) => {
                                            poll_link.send_message(CsvDataSourceMsg::VerifyError(
                                                e.to_string(),
                                            ));
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

        instance
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

    fn view(&self, _ctx: &Context<Self>) -> Html {
        // Build a user-facing status label in English
        let status_text = if self.is_verifying {
            "Verifying..."
        } else if let Some(Ok(true)) = &self.verify_result {
            "Verified"
        } else if let Some(Err(e)) = &self.verify_result {
            &format!("Error: {}", e)
        } else if let Some(JobStatus::InProgress(n)) = &self.job_status {
            &format!("Processing {} lines...", n)
        } else if let Some(JobStatus::Pending) = &self.job_status {
            "Pending..."
        } else {
            "CSV"
        };

        html! {
            <button class="icon-btn" title="CSV data source">
                <i class="material-icons">{"table_chart"}</i>
                <span class="icon-label">{status_text}</span>
            </button>
        }
    }
}

/// Try to extract a ticket UUID from a text that may be JSON with `ticket` or `uuid` fields,
/// or a raw JSON string literal.
fn extract_ticket_from_text(text: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        if let Some(t) = v.get("ticket").and_then(|t| t.as_str()) {
            return Some(t.to_string());
        }
        if let Some(t) = v.get("uuid").and_then(|t| t.as_str()) {
            return Some(t.to_string());
        }
        if v.is_string() {
            return v.as_str().map(|s| s.to_string());
        }
    }
    None
}

/// Parse job status flexibly from JSON `Value`.
/// Supports common Rust enum serializations:
/// - single-key object like `{ "Pending": {} }`, `{ "InProgress": 123 }`, `{ "Completed": "..." }`
/// - or object with `{ "status": "...", "data": ... }`
/// - or a simple string like `"Pending"`.
fn parse_job_status(v: &Value) -> Option<JobStatus> {
    match v {
        Value::String(s) => match s.as_str() {
            "Pending" => Some(JobStatus::Pending),
            other => {
                // If string contains serialized payload, treat as Completed
                if other.starts_with('{') || other.starts_with('[') {
                    Some(JobStatus::Completed(other.to_string()))
                } else {
                    None
                }
            }
        },
        Value::Object(map) => {
            if map.len() == 1 {
                let (k, val) = map.iter().next().unwrap();
                match k.as_str() {
                    "Pending" => Some(JobStatus::Pending),
                    "InProgress" => val.as_u64().map(|n| JobStatus::InProgress(n as u32)),
                    "Completed" => {
                        if val.is_string() {
                            val.as_str().map(|s| JobStatus::Completed(s.to_string()))
                        } else {
                            serde_json::to_string(val).ok().map(JobStatus::Completed)
                        }
                    }
                    "Failed" => {
                        if val.is_string() {
                            val.as_str().map(|s| JobStatus::Failed(s.to_string()))
                        } else {
                            serde_json::to_string(val).ok().map(JobStatus::Failed)
                        }
                    }
                    _ => None,
                }
            } else {
                // Alternative shape: { "status": "Pending", "data": ... }
                if let Some(status) = map.get("status").and_then(|s| s.as_str()) {
                    match status {
                        "Pending" => Some(JobStatus::Pending),
                        "InProgress" => map
                            .get("data")
                            .and_then(|d| d.as_u64())
                            .map(|n| JobStatus::InProgress(n as u32)),
                        "Completed" => {
                            if let Some(d) = map.get("data") {
                                serde_json::to_string(d).ok().map(JobStatus::Completed)
                            } else {
                                None
                            }
                        }
                        "Failed" => map
                            .get("data")
                            .and_then(|d| d.as_str())
                            .map(|s| JobStatus::Failed(s.to_string())),
                        _ => None,
                    }
                } else {
                    None
                }
            }
        }
        _ => None,
    }
}
