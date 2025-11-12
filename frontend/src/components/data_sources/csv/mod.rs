use gloo_net::http::Request;
use serde_json::json;
use wasm_bindgen_futures::spawn_local;
use yew::{html, Component, Context, Html, Properties};

pub struct CsvDataSourceComponent {
    verifying: bool,
    verified: Option<Result<bool, String>>,
}

#[derive(Properties, PartialEq)]
pub struct CsvDataSourceProps {
    #[prop_or_default]
    pub template_id: Option<String>,
}

pub enum CsvDataSourceMsg {
    VerifyCompleted(Result<bool, String>),
}

impl Component for CsvDataSourceComponent {
    type Message = CsvDataSourceMsg;
    type Properties = CsvDataSourceProps;

    fn create(ctx: &Context<Self>) -> Self {
        let mut instance = CsvDataSourceComponent {
            verifying: false,
            verified: None,
        };

        if let Some(id) = ctx.props().template_id.clone() {
            instance.verifying = true;
            let link = ctx.link().clone();
            spawn_local(async move {
                let url = "/api/data_sources/csv/verify";
                let body = json!({ "uuid": id }).to_string();
                let result = match Request::post(url)
                    .header("Content-Type", "application/json")
                    .body(body)
                    .unwrap()
                    .send()
                    .await
                {
                    Ok(response) => {
                        let status = response.status();
                        if status == 200 {
                            Ok(true)
                        } else {
                            let text = response.text().await.unwrap_or_default();
                            Err(format!("HTTP {}: {}", status, text))
                        }
                    }
                    Err(err) => Err(err.to_string()),
                };
                link.send_message(CsvDataSourceMsg::VerifyCompleted(result));
            });
        }

        instance
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            CsvDataSourceMsg::VerifyCompleted(res) => {
                self.verifying = false;
                self.verified = Some(res);
                true
            }
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        let status_text = if self.verifying {
            "Verificando..."
        } else if let Some(Ok(true)) = &self.verified {
            "Verificado"
        } else if let Some(Err(e)) = &self.verified {
            &format!("Error: {}", e)
        } else {
            "CSV"
        };

        html! {
            <button class="icon-btn" title="Fuente de datos CSV">
                <i class="material-icons">{"table_chart"}</i>
                <span class="icon-label">{status_text}</span>
            </button>
        }
    }
}
