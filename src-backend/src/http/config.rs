use actix_web::{get, HttpResponse, ResponseError, Result, web};
use pravega_client::client_factory::ClientFactoryAsync;
use pravega_client_config::ClientConfigBuilder;
use pravega_client_shared::{CToken, Scope};
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use tracing::{error, info};

#[derive(Debug, Serialize)]
struct SimpleJson {
    msg: String,
}

#[derive(Debug)]
struct InternalError(String);

impl std::fmt::Display for InternalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ResponseError for InternalError {
    fn error_response(&self) -> HttpResponse {
        error!("{}", self.0);
        HttpResponse::InternalServerError().finish()
    }
}

#[derive(Debug, Deserialize)]
pub struct VideosRequest {
    #[serde(default)]
    ctoken: String,
}

#[get("/videos")]
async fn index(req: web::Query<VideosRequest>) -> Result<HttpResponse> {
    let config = ClientConfigBuilder::default()
        .controller_uri("tcp://127.0.0.1:9090")
        .build().map_err(|err| InternalError(err))?;

    let handle = Handle::try_current().map_err(|err| InternalError(format!("Failed to get tokio handle: {}", err.to_string())))?;
    let client_factory = ClientFactoryAsync::new(config, handle);
    let controller_client = client_factory.controller_client();
    let scope = Scope {
        name: "examples".to_string(),
    };
    let tag = "video";
    let ctoken = CToken::new(req.ctoken.clone());
    let streams = controller_client.list_streams_for_tag(&scope, tag, &ctoken).await.map_err(|err|
        InternalError(format!("Failed to list streams: {}", err.to_string()))
    )?;

    Ok(HttpResponse::Ok().json(streams))
}

pub fn config_routing(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/v1").service(index)
    );
}
