mod ws;
use crate::ws::server::Server;
use crate::ws::connection::Connection;
use actix::Actor;
use actix_web::{get, web, App, Error, HttpRequest, HttpResponse, HttpServer};
use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;
use tracing::info;
use tracing_subscriber::prelude::*;

#[derive(Parser, Debug)]
#[clap(about, version, author)]
/// Program arguments
struct Args {
    /// Address to listen on
    #[clap(long, default_value = "0.0.0.0")]
    host: String,
    /// Port to listen on
    #[clap(short, long, default_value_t = 8080)]
    port: u16,
}

fn initialize_logging() -> Result<()> {
    tracing_log::LogTracer::init()?;
    let env_filter = tracing_subscriber::EnvFilter::from_default_env();
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_thread_ids(true)
        .with_target(true)
        .with_span_events(
            tracing_subscriber::fmt::format::FmtSpan::NEW
                | tracing_subscriber::fmt::format::FmtSpan::CLOSE,
        );
    let subscriber = tracing_subscriber::Registry::default()
        .with(env_filter)
        .with(fmt_layer);
    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}


#[derive(Debug, Serialize)]
struct SimpleJson {
    code: i32,
    msg: String,
}

#[get("/api")]
async fn index() -> HttpResponse {
    HttpResponse::Ok().json(SimpleJson{
        code: 200,
        msg: "Ok".to_string()
    })
}

async fn ws_route(
    req: HttpRequest,
    stream: web::Payload,
    srv: web::Data<actix::Addr<Server>>,
) -> Result<HttpResponse, Error> {
    actix_web_actors::ws::start(
        Connection::new(srv.get_ref().clone()),
        &req,
        stream,
    )
}

#[actix_web::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    initialize_logging()?;

    let server = Server::new(args.port).start();
    info!("Listening on: {}:{}", args.host, args.port);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(server.clone()))
            .service(index)
            .route("/ws", web::get().to(ws_route))
    })
    .bind((args.host.clone(), args.port.clone()))
    .with_context(|| format!("Failed to bind {}:{}", args.host, args.port))?
    .run()
    .await
    .with_context(|| "Fail to run http server")
}
