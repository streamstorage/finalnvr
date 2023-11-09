mod db;
mod http;
mod ws;
use crate::ws::server::Server;
use crate::ws::connection::Connection;
use actix::Actor;
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer, middleware};
use anyhow::{Context, Result};
use clap::Parser;
use tracing::info;


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

#[derive(Parser, Debug)]
#[clap(about, version, author)]
/// Program arguments
pub struct Args {
    /// Address to listen on
    #[clap(long, default_value = "0.0.0.0")]
    pub host: String,
    /// Port to listen on
    #[clap(short, long, default_value_t = 8080)]
    pub port: u16,
    /// DB
    #[clap(short, long, default_value = "dev.db")]
    pub db: String,
    /// Recorder path
    #[clap(short, long, default_value = "./target/debug/rtsp_camera_to_pravega")]
    pub recorder_path: String,
}

#[actix_web::main]
async fn main() -> Result<()> {
    src_backend::initialize_logging()?;
    let args = Args::parse();
    let server = Server::new(args.port, args.db, args.recorder_path).start();
    info!("Listening on: {}:{}", args.host, args.port);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(server.clone()))
            // enable logger
            // https://docs.rs/actix-web/latest/actix_web/middleware/struct.Logger.html
            .wrap(middleware::Logger::default())
            .configure(http::config::config_routing)
            .route("/ws", web::get().to(ws_route))
    })
    .bind((args.host.clone(), args.port))
    .with_context(|| format!("Failed to bind {}:{}", args.host, args.port))?
    .run()
    .await
    .with_context(|| "Fail to run http server")
}
