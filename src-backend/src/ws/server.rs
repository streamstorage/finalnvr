use crate::db::models::*;
use crate::ws::protocol as p;
use actix::prelude::*;
use anyhow::{bail, Context as _, Result};
use diesel::prelude::*;
use gst::prelude::*;
use std::collections::{HashMap, HashSet};
use tracing::{error, info};

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

/// AddCamera
#[derive(Message)]
#[rtype(result = "()")]
pub struct AddCamera {
    pub camera: Camera,
}

/// EditCamera
#[derive(Message)]
#[rtype(result = "()")]
pub struct EditCamera {
    pub camera: Camera,
}

/// RemoveCamera
#[derive(Message)]
#[rtype(result = "()")]
pub struct RemoveCamera {
    pub camera: Camera,
}

/// List of cameras
pub struct ListCameras;

impl actix::Message for ListCameras {
    type Result = Vec<Camera>;
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
            bail!("Peer {id} is not part of session {}", self.id)
        }
    }
}

#[derive(Debug)]
pub struct Server {
    port: u16,
    db: String,
    peers: HashMap<PeerId, (Recipient<Message>, p::PeerStatus)>,
    pipelines: HashMap<String, Pipeline>,

    sessions: HashMap<String, Session>,
    consumer_sessions: HashMap<String, HashSet<String>>,
    producer_sessions: HashMap<String, HashSet<String>>,
}

impl Server {
    pub fn new(port: u16, db: String) -> Self {
        Self {
            port,
            db,
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

    fn establish_db_connection(&self) -> Result<SqliteConnection> {
        SqliteConnection::establish(&self.db).with_context(|| format!("Cannot connect to {}", self.db))
    }

    fn add_camera(&mut self, camera: Camera) -> Result<()> {
        use crate::db::schema::cameras;
        let connection = &mut self.establish_db_connection()?;

        let camera_id = uuid::Uuid::new_v4().to_string();
        let new_camera = NewCamera { 
            id: camera_id.as_str(),
            name: camera.name.as_str(),
            location: camera.location.as_str(),
            url: camera.url.as_str(),
        };

        diesel::insert_into(cameras::table)
            .values(&new_camera)
            .execute(connection)
            .with_context(|| "Error executing insert query")?;

        self.list_cameras_all(connection)?;

        Ok(())
    }

    fn edit_camera(&mut self, camera: Camera) -> Result<()> {
        use crate::db::schema::cameras::dsl::*;

        let connection = &mut self.establish_db_connection()?;

        diesel::update(cameras.find(&camera.id))
            .set((name.eq(&camera.name), location.eq(&camera.location), url.eq(&camera.url)))
            .execute(connection)
            .map(|_| ())
            .with_context(|| "Error executing update query")?;
        
        self.list_cameras_all(connection)?;

        if let Some(pipeline) = self.pipelines.remove(&camera.id) {
            if let Err(err) = pipeline.pipeline.set_state(gst::State::Null) {
                error!("Failed to stop the pipeline: {}", err);
            }
        }

        Ok(())
    }

    fn remove_camera(&mut self, camera: Camera) -> Result<()> {
        use crate::db::schema::cameras::dsl::*;

        let connection = &mut self.establish_db_connection()?;

        diesel::delete(cameras.find(&camera.id))
            .execute(connection)
            .map(|_| ())
            .with_context(|| "Error executing delete query")?;
        
        self.list_cameras_all(connection)?;

        if let Some(pipeline) = self.pipelines.remove(&camera.id) {
            if let Err(err) = pipeline.pipeline.set_state(gst::State::Null) {
                error!("Failed to stop the pipeline: {}", err);
            }
        }

        Ok(())
    }

    fn list_cameras(&mut self) -> Result<Vec<Camera>> {
        use crate::db::schema::cameras::dsl::*;
        let connection = &mut self.establish_db_connection()?;

        cameras
            .select(Camera::as_select())
            .load(connection)
            .with_context(|| "Error loading cameras")
    }

    fn list_cameras_all(&mut self, connection: &mut SqliteConnection) -> Result<()> {
        use crate::db::schema::cameras::dsl::cameras;

        let results = cameras
            .select(Camera::as_select())
            .load(connection)
            .with_context(|| "Error loading cameras")?;
        
        for (_, (addr, status)) in self.peers.iter() {
            if status.listening() {
                addr.do_send(Message(p::OutgoingMessage::ListCameras{
                    cameras: results.clone()
                }));
            }
        }

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
                if pipeline.clients.is_empty() {
                    if let Err(err) = pipeline.pipeline.set_state(gst::State::Null) {
                        error!("Failed to stop the pipeline: {}", err);
                    }
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
        let old_status = match self
            .peers
            .get_mut(&msg.connection_id) {
                Some(status) => status,
                None => {
                    error!("Peer '{}' hasn't been welcomed", msg.connection_id);
                    return;
                }
            };

        if msg.peer_status == old_status.1 {
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
                        addr.do_send(Message(p::OutgoingMessage::PeerStatusChanged(p::PeerStatus {
                            peer_id: Some(msg.connection_id),
                            roles: msg.peer_status.roles,
                            meta: msg.peer_status.meta,
                        })));
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

            for (id, (_, peer_status)) in &self.peers {
                if peer_status.producing() {
                    if let Some(meta) = &peer_status.meta {
                        if meta.get("id").filter(|v| **v == serde_json::json!(msg.camera_id)).is_some() {
                            if let Some((addr, _)) = self.peers.get(&msg.connection_id) {
                                addr.do_send(Message(p::OutgoingMessage::PeerStatusChanged(p::PeerStatus {
                                    peer_id: Some(id.to_owned()),
                                    roles: peer_status.roles.clone(),
                                    meta: peer_status.meta.clone(),
                                })));
                            }
                            return;
                        }
                    }
                }
            }
        } else {
            if let Err(err) = gst::init() {
                error!("Fail to init gst, err: {}", err);
                return;
            }
            let pipeline_str = format!(
                "webrtcsink name=ws meta=\"meta,id={},init={}\" signaller::address=\"ws://127.0.0.1:{}/ws\" \
                    rtspsrc location={} drop-on-latency=true latency=50 ! rtph264depay ! h264parse ! video/x-h264,alignment=au ! avdec_h264 ! ws.",
                    //audiotestsrc ! ws.",
                msg.camera_id, msg.connection_id, self.port, msg.camera_url
            );
            let pipeline = match gst::parse_launch(&pipeline_str) {
                Ok(p) => p,
                Err(err) => {
                    error!("Failed to parse the pipeline: {}", err);
                    return;
                }
            };
            if let Err(err) = pipeline.set_state(gst::State::Playing) {
                error!("Failed to play the pipeline: {}", err);
                return;
            }
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
                if pipeline.clients.is_empty() {
                    if let Err(err) = pipeline.pipeline.set_state(gst::State::Null) {
                        error!("Failed to stop the pipeline: {}", err);
                    }
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
        let producer_addr = match self.peers.get(&msg.producer_id) {
            Some((addr, peer)) => {
                if peer.producing() {
                    addr
                } else {
                    error!("Peer with id {} is not registered as a producer", &msg.producer_id);
                    return;
                }
            }
            None => {
                error!("No producer with ID: '{}'", &msg.producer_id);
                return;
            }
        };

        let consumer_addr = match self.peers.get(&msg.consumer_id) {
            Some((addr, _)) => addr,
            None => {
                error!("No consumer with ID: '{}'", &msg.consumer_id);
                return;
            }
        };

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
        self.end_session(&msg.peer_id, &msg.peer_id).unwrap_or_else(|err| {
            error!("Session {} failed to end: {}", msg.session_id ,err);
        })
    }
}

/// Handler for Peer message.
impl Handler<Peer> for Server {
    type Result = ();

    fn handle(&mut self, msg: Peer, _: &mut Context<Self>) {
        let session_id = &msg.peer_msg.session_id;
        let session = match self
            .sessions
            .get(session_id) {
            Some(s) => s,
            None => {
                error!("Session {} doesn't exist", session_id);
                return;
            }
        };

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

        let peer_id = match session.other_peer_id(&msg.peer_id) {
            Ok(id) => id,
            Err(err) => {
                error!("No peer found: {}", err);
                return;
            }
        };
        let peer_addr = match self.peers.get(peer_id) {
            Some((addr, _)) => addr,
            None => {
                error!("Peer with id {} dosen't exist", peer_id);
                return;      
            }
        };
        peer_addr.do_send(Message(p::OutgoingMessage::Peer(msg.peer_msg)));
    }
}

/// Handler for AddCamera message.
impl Handler<AddCamera> for Server {
    type Result = ();

    fn handle(&mut self, msg: AddCamera, _: &mut Context<Self>) {
        self.add_camera(msg.camera).unwrap_or_else(|err| {
            error!("Failed to add camera: {}", err);
        })
    }
}

/// Handler for EditCamera message.
impl Handler<EditCamera> for Server {
    type Result = ();

    fn handle(&mut self, msg: EditCamera, _: &mut Context<Self>) {
        self.edit_camera(msg.camera).unwrap_or_else(|err| {
            error!("Failed to edit camera: {}", err);
        })
    }
}

/// Handler for RemoveCamera message.
impl Handler<RemoveCamera> for Server {
    type Result = ();

    fn handle(&mut self, msg: RemoveCamera, _: &mut Context<Self>) {
        self.remove_camera(msg.camera).unwrap_or_else(|err| {
            error!("Failed to remove camera: {}", err);
        })
    }
}

/// Handler for ListCameras message.
impl Handler<ListCameras> for Server {
    type Result = MessageResult<ListCameras>;

    fn handle(&mut self, _: ListCameras, _: &mut Context<Self>) -> Self::Result {
        self.list_cameras().map_or_else(|err|{
            error!("Unable to list cameras: {}", err);
            MessageResult(vec![])
        }, |v|{
            MessageResult(v)
        })
    }
}
