use crate::ws::protocol as p;
use actix::prelude::*;
use anyhow::{anyhow, bail, Context as _, Result};
use gst::prelude::*;
use std::collections::{HashMap, HashSet};
use tracing::{info, error};

type PeerId = String;

/// Chat server sends this messages to connection
#[derive(Message)]
#[rtype(result = "()")]
pub struct Message(pub p::OutgoingMessage);

/// New ws connection is created
#[derive(Message)]
#[rtype(PeerId)]
pub struct Connect {
    pub addr: Recipient<Message>,
}

/// Disconnect
#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect {
    pub connection_id: PeerId,
}

/// Set peer status
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetPeerStatus {
    pub connection_id: PeerId,
    pub peer_status: p::PeerStatus,
}

/// Preview
#[derive(Message)]
#[rtype(result = "()")]
pub struct Preview {
    pub camera_id: String,
    pub camera_url: String,
    pub connection_id: PeerId,
}

/// Stop preview
#[derive(Message)]
#[rtype(result = "()")]
pub struct StopPreview {
    pub camera_id: String,
    pub connection_id: PeerId,
}

/// Start session
#[derive(Message)]
#[rtype(result = "()")]
pub struct StartSession {
    pub consumer_id: PeerId,
    pub producer_id: PeerId,
}

/// End session
#[derive(Message)]
#[rtype(result = "()")]
pub struct EndSession {
    pub peer_id: PeerId,
    pub session_id: String,
}

/// Peer
#[derive(Message)]
#[rtype(result = "()")]
pub struct Peer {
    pub peer_id: PeerId,
    pub peer_msg: p::PeerMessage,
}

#[derive(Clone, Debug)]
struct Pipeline {
    pipeline: gst::Element,
    clients: HashSet<String>,
}

#[derive(Clone, Debug)]
struct Session {
    id: String,
    consumer: PeerId,
    producer: PeerId,
}

impl Session {
    fn other_peer_id(&self, id: &str) -> Result<&str> {
        if self.producer == id {
            Ok(&self.consumer)
        } else if self.consumer == id {
            Ok(&self.producer)
        } else {
            bail!("Peer {id} is not part of {}", self.id)
        }
    }
}

#[derive(Debug)]
pub struct Server {
    port: u16,
    peers: HashMap<PeerId, (Recipient<Message>, p::PeerStatus)>,
    pipelines: HashMap<String, Pipeline>,

    sessions: HashMap<String, Session>,
    consumer_sessions: HashMap<String, HashSet<String>>,
    producer_sessions: HashMap<String, HashSet<String>>,
}

impl Server {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            peers: Default::default(),
            pipelines: Default::default(),

            sessions: Default::default(),
            consumer_sessions: Default::default(),
            producer_sessions: Default::default(),
        }
    }

    fn stop_producer(&mut self, peer_id: &str) {
        if let Some(session_ids) = self.producer_sessions.remove(peer_id) {
            for session_id in session_ids {
                if let Err(e) = self.end_session(peer_id, &session_id) {
                    error!("Could not end session {session_id}: {e:?}");
                }
            }
        }
    }

    fn stop_consumer(&mut self, peer_id: &str) {
        if let Some(session_ids) = self.consumer_sessions.remove(peer_id) {
            for session_id in session_ids {
                if let Err(e) = self.end_session(peer_id, &session_id) {
                    error!("Could not end session {session_id}: {e:?}");
                }
            }
        }
    }

    /// End a session between two peers
    /// 1. last listener stops the pipeline which make producer send end_session event
    /// 2. listener stops ws connection which end the session
    fn end_session(&mut self, peer_id: &str, session_id: &str) -> Result<()> {
        let session = self
            .sessions
            .remove(session_id)
            .with_context(|| format!("Session {session_id} doesn't exist"))?;

        self.consumer_sessions
            .entry(session.consumer.clone())
            .and_modify(|sessions| {
                sessions.remove(session_id);
            });

        self.producer_sessions
            .entry(session.producer.clone())
            .and_modify(|sessions| {
                sessions.remove(session_id);
            });

        let other_peer_id = session.other_peer_id(peer_id)?;
        let (peer_addr, _) = self.peers.get(other_peer_id).with_context(|| format!("Peer {other_peer_id} doesn't exist"))?;
        peer_addr.do_send(Message(p::OutgoingMessage::EndSession(p::EndSessionMessage {
            session_id: session_id.to_string(),
        })));

        Ok(())
    }
}

/// Make actor from `Server`
impl Actor for Server {
    /// We are going to use simple Context, we just need ability to communicate
    /// with other actors.
    type Context = Context<Self>;
}

/// Handler for Connect message.
impl Handler<Connect> for Server {
    type Result = PeerId;

    fn handle(&mut self, msg: Connect, _: &mut Context<Self>) -> PeerId {
        let id = uuid::Uuid::new_v4().to_string();
        self.peers.insert(id.clone(), (msg.addr, Default::default()));
        id
    }
}

/// Handler for Disconnect message.
impl Handler<Disconnect> for Server {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
        self.pipelines.retain(|_, pipeline| {
            if pipeline.clients.contains(&msg.connection_id) {
                pipeline.clients.remove(&msg.connection_id);
                if pipeline.clients.len() == 0 {
                    pipeline.pipeline.set_state(gst::State::Null).expect("stop the pipeline");
                    return false;
                }
            }
            true
        });

        info!(connection_id = %msg.connection_id, "removing peer");
        self.peers.remove(&msg.connection_id);

        self.stop_producer(&msg.connection_id);
        self.stop_consumer(&msg.connection_id);
    }
}

/// Handler for SetPeerStatus message. 
impl Handler<SetPeerStatus> for Server {
    type Result = ();

    fn handle(&mut self, msg: SetPeerStatus, _: &mut Context<Self>) {
        let old_status = self
            .peers
            .get_mut(&msg.connection_id)
            .expect(format!("Peer '{}' hasn't been welcomed", msg.connection_id).as_str());

        if &msg.peer_status == &old_status.1 {
            info!("Status for '{}' hasn't changed", msg.connection_id);
            return;
        }

        // if old_status.producing() && !status.producing() {
        //     self.stop_producer(peer_id);
        // }

        let mut status = msg.peer_status.clone();
        status.peer_id = Some(msg.connection_id.clone());
        old_status.1 = status;
        info!(connection_id=%msg.connection_id, "set peer status");

        if msg.peer_status.producing() {
            if let Some(meta) = &msg.peer_status.meta {
                if let Ok(meta) = serde_json::from_value::<p::CameraMeta>(meta.clone()) {
                    if let Some((addr, _)) = self.peers.get_mut(&meta.init) {
                        let msg = p::OutgoingMessage::List {
                            producers: vec![p::Peer {
                                id: msg.connection_id,
                                meta: msg.peer_status.meta,
                            }],
                        };
                        addr.do_send(Message(msg));
                    }
                }
            }
        }
    }
}

/// Handler for Preview message.
impl Handler<Preview> for Server {
    type Result = ();

    fn handle(&mut self, msg: Preview, _: &mut Context<Self>) {
        info!(camera_id = %msg.camera_id, url = %msg.camera_url, viewer = %msg.connection_id, "preview");
        let pipeline = self.pipelines.get_mut(&msg.camera_id);
        if let Some(pipeline) = pipeline {
            pipeline.clients.insert(msg.connection_id.clone());

            for (id, peer) in &self.peers {
                if peer.1.producing() {
                    if let Some(meta) = &peer.1.meta {
                        if meta.get("id").filter(|v| **v == serde_json::json!(msg.camera_id)).is_some() {
                            if let Some((addr, _)) = self.peers.get(&msg.connection_id) {
                                let msg = p::OutgoingMessage::List {
                                    producers: vec![p::Peer {
                                        id: id.clone(),
                                        meta: peer.1.meta.clone(),
                                    }],
                                };
                                addr.do_send(Message(msg));
                            }
                            return;
                        }
                    }
                }
            }
        } else {
            gst::init().unwrap();
            let pipeline_str = format!(
                "webrtcsink name=ws meta=\"meta,id={},init={}\" signaller::address=\"ws://127.0.0.1:{}/ws\" \
                    rtspsrc location={} drop-on-latency=true latency=50 ! rtph264depay ! h264parse ! video/x-h264,alignment=au ! avdec_h264 ! ws.",
                    //audiotestsrc ! ws.",
                msg.camera_id, msg.connection_id, self.port, msg.camera_url
            );
            let pipeline = gst::parse_launch(&pipeline_str).expect("parse launch");
            pipeline.set_state(gst::State::Playing).expect("start the pipeline");
            let mut clients = HashSet::new();
            clients.insert(msg.connection_id);
            self.pipelines.insert(msg.camera_id, Pipeline {
                pipeline,
                clients,
            });
        }
    }
}

/// Handler for StopPreview message.
impl Handler<StopPreview> for Server {
    type Result = ();

    fn handle(&mut self, msg: StopPreview, _: &mut Context<Self>) {
        info!(camera_id = %msg.camera_id, connection_id = %msg.connection_id, "stop preview");
        if let Some(pipeline) = self.pipelines.get_mut(&msg.camera_id) {
            if pipeline.clients.contains(&msg.connection_id) {
                pipeline.clients.remove(&msg.connection_id);
                if pipeline.clients.len() == 0 {
                    pipeline.pipeline.set_state(gst::State::Null).expect("stop the pipeline");
                    self.pipelines.remove(&msg.camera_id);
                }
            }
        }
    }
}

/// Handler for StartSession message.
impl Handler<StartSession> for Server {
    type Result = ();

    fn handle(&mut self, msg: StartSession, _: &mut Context<Self>) {
        let producer_addr = self.peers.get(&msg.producer_id).map_or_else(
            || Err(anyhow!("No producer with ID: '{}'", &msg.producer_id)),
            |(addr, peer): &(Recipient<Message>, p::PeerStatus)| {
                if !peer.producing() {
                    Err(anyhow!(
                        "Peer with id {} is not registered as a producer",
                        &msg.producer_id
                    ))
                } else {
                    Ok(addr)
                }
            },
        ).expect("producer exists");

        let consumer_addr = self.peers
            .get(&msg.consumer_id)
            .map_or_else(|| Err(anyhow!("No consumer with ID: '{}'", &msg.consumer_id)), 
            |(addr, _)| Ok(addr)
        ).expect("consumer exists");

        let session_id = uuid::Uuid::new_v4().to_string();
        self.sessions.insert(
            session_id.clone(),
            Session {
                id: session_id.clone(),
                consumer: msg.consumer_id.clone(),
                producer: msg.producer_id.clone(),
            },
        );
        self.consumer_sessions
            .entry(msg.consumer_id.clone())
            .or_insert_with(HashSet::new)
            .insert(session_id.clone());
        self.producer_sessions
            .entry(msg.producer_id.clone())
            .or_insert_with(HashSet::new)
            .insert(session_id.clone());
        
        consumer_addr.do_send(Message(p::OutgoingMessage::SessionStarted {
            peer_id: msg.producer_id.clone(),
            session_id: session_id.clone(),
        }));

        producer_addr.do_send(Message(p::OutgoingMessage::StartSession {
            peer_id: msg.consumer_id.clone(),
            session_id: session_id.clone(),
        }));

        info!(id = %session_id, producer_id = %msg.producer_id, consumer_id = %msg.consumer_id, "started a session");
    }
}

/// Handler for EndSession message.
impl Handler<EndSession> for Server {
    type Result = ();

    fn handle(&mut self, msg: EndSession, _: &mut Context<Self>) {
        self.end_session(&msg.peer_id, &msg.peer_id).expect("end session");
    }
}

/// Handler for Peer message.
impl Handler<Peer> for Server {
    type Result = ();

    fn handle(&mut self, msg: Peer, _: &mut Context<Self>) {
        let session_id = &msg.peer_msg.session_id;
        let session = self
            .sessions
            .get(session_id)
            .expect(format!("Session {} doesn't exist", session_id).as_str());

        if matches!(
            msg.peer_msg.peer_message,
            p::PeerMessageInner::Sdp(p::SdpMessage::Offer { .. })
        ) && msg.peer_id == session.consumer
        {
            error!(
                r#"cannot forward offer from "{}" to "{}" as "{}" is not the producer"#,
                msg.peer_id, session.producer, msg.peer_id
            );
            return;
        }

        let peer_id = session.other_peer_id(&msg.peer_id).expect("other peer id");
        let (peer_addr, _) = self.peers.get(peer_id).expect("other peer addr");
        peer_addr.do_send(Message(p::OutgoingMessage::Peer(msg.peer_msg)));
    }
}
