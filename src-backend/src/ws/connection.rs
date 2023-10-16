use crate::ws::protocol as p;
use crate::ws::server;
use actix::prelude::*;
use actix_web_actors::ws;
use std::time::{Duration, Instant};
use tracing::{info, warn, debug};

/// How often heartbeat pings are sent
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Connection {
    // /// unique connection id
    pub id: String,

    /// Client must send ping at least once per CLIENT_TIMEOUT seconds,
    /// otherwise drop connection.
    pub hb: Instant,

    // /// server
    pub addr: Addr<server::Server>,
}

impl Connection {
    pub fn new(addr: Addr<server::Server>) -> Self {
        Self {
            id: "".to_owned(),
            hb: Instant::now(),
            addr,
        }
    }
    /// helper method that sends ping to client every 5 seconds (HEARTBEAT_INTERVAL).
    ///
    /// also this method checks heartbeats from client
    fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            // check client heartbeats
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                // heartbeat timed out
                info!("Connection {} heartbeat failed, disconnecting", act.id);

                // notify chat server
                act.addr.do_send(server::Disconnect { connection_id: act.id.clone() });

                // stop actor
                ctx.stop();

                // don't try to send a ping
                return;
            }

            ctx.ping(b"");
        });
    }
}

impl Actor for Connection {
    type Context = ws::WebsocketContext<Self>;

    /// Method is called on actor start.
    /// We register ws connection with Server
    fn started(&mut self, ctx: &mut Self::Context) {
        // we'll start heartbeat process on connection start.
        self.hb(ctx);

        let addr = ctx.address();
        self.addr
            .send(server::Connect {
                addr: addr.recipient(),
            })
            .into_actor(self)
            .then(|res, act, ctx| {
                match res {
                    Ok(id) =>{
                        act.id = id.clone();
                        ctx.text(p::OutgoingMessage::Welcome {
                            peer_id: id,
                        }.to_string());
                    } ,
                    // something is wrong with server
                    _ => ctx.stop(),
                }
                fut::ready(())
            })
            .wait(ctx);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        // notify chat server
        self.addr.do_send(server::Disconnect {
            connection_id: self.id.clone(),
        });
        Running::Stop
    }
}

/// Handle messages from server, simply send it to peer websocket
impl Handler<server::Message> for Connection {
    type Result = ();

    fn handle(&mut self, msg: server::Message, ctx: &mut Self::Context) {
        ctx.text(msg.0.to_string());
    }
}

/// WebSocket message handler
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for Connection {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        let msg = match msg {
            Err(_) => {
                ctx.stop();
                return;
            }
            Ok(msg) => msg,
        };
        debug!("MESSAGE: {msg:?}");

        match msg {
            ws::Message::Ping(msg) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            ws::Message::Pong(_) => {
                self.hb = Instant::now();
            }
            ws::Message::Binary(_) => warn!("Unexpected binary"),
            ws::Message::Close(reason) => {
                ctx.close(reason);
                ctx.stop();
            }
            ws::Message::Continuation(_) => {
                ctx.stop();
            }
            ws::Message::Nop => (),
            ws::Message::Text(text) => {
                match serde_json::from_slice::<p::IncomingMessage>(text.as_bytes()) {
                    Ok(msg) => {
                        match msg {
                            p::IncomingMessage::SetPeerStatus(peer_status) => {
                                self.addr.do_send(server::SetPeerStatus {
                                    connection_id: self.id.clone(),
                                    peer_status,
                                });
                            }
                            p::IncomingMessage::StartSession(msg) => {
                                self.addr.do_send(server::StartSession {
                                    consumer_id: self.id.clone(),
                                    producer_id: msg.peer_id,
                                });
                            }
                            p::IncomingMessage::Peer(msg) => {
                                self.addr.do_send(server::Peer {
                                    peer_id: self.id.clone(),
                                    peer_msg: msg,
                                });
                            }
                            p::IncomingMessage::EndSession(msg) => {
                                self.addr.do_send(server::EndSession {
                                    peer_id: self.id.clone(),
                                    session_id: msg.session_id,
                                });
                            }
                            p::IncomingMessage::NewPeer | p::IncomingMessage::List => {
                                info!("NewPeer or List");
                            }
                            p::IncomingMessage::Preview(camera) => {
                                self.addr.do_send(server::Preview {
                                    camera_id: camera.id,
                                    camera_url: camera.url,
                                    connection_id: self.id.clone(),
                                });
                            }
                            p::IncomingMessage::StopPreview(camera) => {
                                self.addr.do_send(server::StopPreview {
                                    camera_id: camera.id,
                                    connection_id: self.id.clone(),
                                });
                            }
                            p::IncomingMessage::AddCamera(camera) => {
                                self.addr.do_send(server::AddCamera {
                                    camera,
                                });
                            }
                            p::IncomingMessage::EditCamera(camera) => {
                                self.addr.do_send(server::EditCamera {
                                    camera,
                                });
                            }
                            p::IncomingMessage::ListCameras => {
                                self.addr
                                    .send(server::ListCameras {})
                                    .into_actor(self)
                                    .then(|res, _act, ctx| {
                                        match res {
                                            Ok(cameras) =>{
                                                ctx.text(p::OutgoingMessage::ListCameras {
                                                    cameras,
                                                }.to_string());
                                            } ,
                                            // something is wrong with server
                                            _ => ctx.stop(),
                                        }
                                        fut::ready(())
                                    })
                                    .wait(ctx);
                            }
                        }
                    }
                    Err(err) => {
                        ctx.text(p::OutgoingMessage::Error {
                            details: err.to_string(),
                        }.to_string());
                    }
                } 

            }
        }
    }
}
