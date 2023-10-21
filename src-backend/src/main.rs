mod db;
mod ws;
use crate::ws::server::Server;
use crate::ws::connection::Connection;
use actix::Actor;
use actix_web::{get, web, App, Error, HttpRequest, HttpResponse, HttpServer};
use anyhow::{Context, Result};
use serde::Serialize;
use tracing::info;

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
    src_backend::initialize_logging()?;
    let args = src_backend::get_args();
    let server = Server::new(args.port, args.db).start();
    info!("Listening on: {}:{}", args.host, args.port);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(server.clone()))
            .service(index)
            .route("/ws", web::get().to(ws_route))
    })
    .bind((args.host.clone(), args.port))
    .with_context(|| format!("Failed to bind {}:{}", args.host, args.port))?
    .run()
    .await
    .with_context(|| "Fail to run http server")
}
