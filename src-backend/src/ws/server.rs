use crate::db::models::*;
use crate::ws::protocol as p;
use actix::prelude::*;
use anyhow::{anyhow, bail, Context as _, Result};
use diesel::prelude::*;
use gst::prelude::*;
use nix::unistd::{fork, ForkResult};
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::thread::{self, JoinHandle};
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
#[derive(Message)]
#[rtype(result = "()")]
pub struct ListCameras {
    pub addr: Recipient<Message>,
}

/// StartRecorder
#[derive(Message)]
#[rtype(result = "()")]
pub struct StartRecorder {
    pub camera: Camera,
}

/// StopRecorder
#[derive(Message)]
#[rtype(result = "()")]
pub struct StopRecorder {
    pub camera: Camera,
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
    recorder_path: String,
    peers: HashMap<PeerId, (Recipient<Message>, p::PeerStatus)>,
    pipelines: HashMap<String, Pipeline>,

    sessions: HashMap<String, Session>,
    consumer_sessions: HashMap<PeerId, HashSet<PeerId>>,
    producer_sessions: HashMap<PeerId, HashSet<PeerId>>,

    recorders: HashMap<String, PeerId>,
}

impl Server {
    pub fn new(port: u16, db: String, recorder_path: String) -> Self {
        Self {
            port,
            db,
            recorder_path,
            peers: Default::default(),
            pipelines: Default::default(),

            sessions: Default::default(),
            consumer_sessions: Default::default(),
            producer_sessions: Default::default(),

            recorders: Default::default(),
        }
    }

    fn disconnect(&mut self, connection_id: &str) -> Result<()> {
        self.pipelines.retain(|_, pipeline| {
            pipeline.clients.remove(connection_id);
            if pipeline.clients.is_empty() {
                if let Err(err) = pipeline.pipeline.set_state(gst::State::Null) {
                    error!("Failed to stop the pipeline: {}", err);
                }
                return false;
            }
            true
        });
        self.recorders.retain(|_, peer_id| {
            peer_id != connection_id
        });

        info!(connection_id = %connection_id, "removing peer");
        let peer = self.peers.remove(connection_id);
        
        if let Some((_, peer_status)) = peer {
            if peer_status.recording() {
                self.peers.iter().for_each(|(_, (addr, status))| {
                    if status.listening() {
                        addr.do_send(Message(p::OutgoingMessage::PeerStatusChanged(p::PeerStatus {
                            peer_id: None,
                            roles: peer_status.roles.clone(),
                            meta: peer_status.meta.clone(),
                        })));
                    }
                })
            }
        }

        self.stop_producer(connection_id);
        self.stop_consumer(connection_id);

        Ok(())
    }

    fn set_peer_status(&mut self, connection_id: &str, peer_status: p::PeerStatus) -> Result<()> {
        let old_status = self
            .peers
            .get_mut(connection_id)
            .with_context(|| format!("Peer '{}' hasn't been welcomed", connection_id))?;

        if peer_status == old_status.1 {
            info!("Status for '{}' hasn't changed", connection_id);
            return Ok(());
        }

        // if old_status.producing() && !status.producing() {
        //     self.stop_producer(peer_id);
        // }

        let mut status = peer_status.clone();
        status.peer_id = Some(connection_id.to_owned());
        old_status.1 = status;
        info!(connection_id=%connection_id, "set peer status");

        if peer_status.producing() {
            if let Some(meta) = &peer_status.meta {
                if let Ok(meta) = serde_json::from_value::<p::CameraMeta>(meta.clone()) {
                    if let Some((addr, _)) = self.peers.get(&meta.init) {
                        addr.do_send(Message(p::OutgoingMessage::PeerStatusChanged(p::PeerStatus {
                            peer_id: Some(connection_id.to_owned()),
                            roles: peer_status.roles,
                            meta: peer_status.meta,
                        })));
                    }
                }
            }
        } else if peer_status.recording() {
            if let Some(meta) = peer_status.meta.clone() {
                if let Some(id) = meta.get("id").and_then(|value| value.as_str()) {
                    self.recorders.insert(id.to_owned(), connection_id.to_string());
                    self.peers.iter().for_each(|(_, (addr, status))| {
                        if status.listening() {
                            addr.do_send(Message(p::OutgoingMessage::PeerStatusChanged(p::PeerStatus {
                                peer_id: Some(connection_id.to_owned()),
                                roles: peer_status.roles.clone(),
                                meta: peer_status.meta.clone(),
                            })));
                        }
                    })
                }
            }
        }

        Ok(())
    }

    fn preview(&mut self, camera_id: &str, camera_url: &str, connection_id: &str) -> Result<()> {
        info!(camera_id = %camera_id, url = %camera_url, listener = %connection_id, "preview");
        let pipeline = self.pipelines.get_mut(camera_id);
        if let Some(pipeline) = pipeline {
            pipeline.clients.insert(connection_id.to_owned());

            for (id, (_, peer_status)) in &self.peers {
                if peer_status.producing() {
                    if let Some(meta) = &peer_status.meta {
                        if meta.get("id").filter(|v| **v == serde_json::json!(camera_id)).is_some() {
                            if let Some((addr, _)) = self.peers.get(connection_id) {
                                addr.do_send(Message(p::OutgoingMessage::PeerStatusChanged(p::PeerStatus {
                                    peer_id: Some(id.to_owned()),
                                    roles: peer_status.roles.clone(),
                                    meta: peer_status.meta.clone(),
                                })));
                                break;
                            }
                        }
                    }
                }
            }
        } else {
            gst::init()?;
            let pipeline_str = format!(
                "webrtcsink name=ws meta=\"meta,id={},init={}\" signaller::address=\"ws://127.0.0.1:{}/ws\" \
                    rtspsrc location={} drop-on-latency=true latency=50 ! rtph264depay ! h264parse ! video/x-h264,alignment=au ! avdec_h264 ! ws.",
                    //audiotestsrc ! ws.",
                camera_id, connection_id, self.port, camera_url
            );
            let pipeline = gst::parse_launch(&pipeline_str)?;
            
            pipeline.set_state(gst::State::Playing)?;

            let mut clients = HashSet::new();
            clients.insert(connection_id.to_owned());
            self.pipelines.insert(camera_id.to_owned(), Pipeline {
                pipeline,
                clients,
            });
        }

        Ok(())
    }

    fn stop_preview(&mut self, camera_id: &str, connection_id: &str) -> Result<()> {
        info!(camera_id = %camera_id, connection_id = %connection_id, "stop preview");
        if let Some(pipeline) = self.pipelines.get_mut(camera_id) {
            pipeline.clients.remove(connection_id);
            if pipeline.clients.is_empty() {
                if let Err(err) = pipeline.pipeline.set_state(gst::State::Null) {
                    error!("Failed to stop the pipeline: {}", err);
                }
                self.pipelines.remove(camera_id);
            }    
        }

        Ok(())
    }

    fn start_session(&mut self, producer_id: &str, consumer_id: &str) -> Result<()> {
        let producer_addr = self.peers.get(producer_id).map_or_else(
            || bail!("No producer with ID: '{}'", producer_id),
            |(addr, peer)| {
                match peer.producing() {
                    true => Ok(addr),
                    false => bail!("Peer with id {} is not registered as a producer", producer_id)
                }
            }
        )?;
        
        let (consumer_addr, _) = &self.peers.get(consumer_id).ok_or_else(
           || anyhow!("No consumer with ID: '{}'", consumer_id)
        )?;

        let session_id = uuid::Uuid::new_v4().to_string();
        self.sessions.insert(
            session_id.clone(),
            Session {
                id: session_id.clone(),
                consumer: consumer_id.to_owned(),
                producer: producer_id.to_owned(),
            },
        );
        self.consumer_sessions
            .entry(consumer_id.to_owned())
            .or_insert_with(HashSet::new)
            .insert(session_id.clone());
        self.producer_sessions
            .entry(producer_id.to_owned())
            .or_insert_with(HashSet::new)
            .insert(session_id.clone());
        
        consumer_addr.do_send(Message(p::OutgoingMessage::SessionStarted {
            peer_id: producer_id.to_owned(),
            session_id: session_id.clone(),
        }));

        producer_addr.do_send(Message(p::OutgoingMessage::StartSession {
            peer_id: consumer_id.to_owned(),
            session_id: session_id.clone(),
        }));

        info!(id = %session_id, producer_id = %producer_id, consumer_id = %consumer_id, "started a session");

        Ok(())
    }

    fn peer(&mut self, peer_id: &str, peer_msg: p::PeerMessage) -> Result<()> {
        let session_id = &peer_msg.session_id;
        let session = self
            .sessions
            .get(session_id).ok_or_else(||anyhow!("Session {} doesn't exist", session_id))?;

        if matches!(
            &peer_msg.peer_message,
            p::PeerMessageInner::Sdp(p::SdpMessage::Offer { .. })
        ) && peer_id == session.consumer {
            bail!(
                r#"cannot forward offer from "{}" to "{}" as "{}" is not the producer"#,
                peer_id, session.producer, peer_id
            );
        }

        let peer_id = session.other_peer_id(peer_id)?;
        let (peer_addr, _) = self.peers.get(peer_id).ok_or_else( 
            || anyhow!("Peer with id {} dosen't exist", peer_id))?;
        peer_addr.do_send(Message(p::OutgoingMessage::Peer(peer_msg)));

        Ok(())
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

        self.list_cameras_all_listener(connection)?;

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
        
        self.list_cameras_all_listener(connection)?;

        // Close the existing preview pipeline in case of caching camera url
        if let Some(pipeline) = self.pipelines.remove(&camera.id) {
            if let Err(err) = pipeline.pipeline.set_state(gst::State::Null) {
                error!("Failed to stop the pipeline: {}", err);
            }
        }

        Ok(())
    }

    fn remove_camera(&mut self, camera_id: &String) -> Result<()> {
        use crate::db::schema::cameras::dsl::*;

        let connection = &mut self.establish_db_connection()?;

        diesel::delete(cameras.find(camera_id))
            .execute(connection)
            .map(|_| ())
            .with_context(|| "Error executing delete query")?;
        
        self.list_cameras_all_listener(connection)?;

        if let Some(pipeline) = self.pipelines.remove(camera_id) {
            if let Err(err) = pipeline.pipeline.set_state(gst::State::Null) {
                error!("Failed to stop the pipeline: {}", err);
            }
        }

        if let Some((addr, peer_status)) = self.recorders.get(camera_id).and_then(|peer_id| self.peers.get(peer_id)) {
            if peer_status.recording() {
                addr.do_send(Message(p::OutgoingMessage::EndSession(p::EndSessionMessage {
                    session_id: camera_id.to_owned(),
                })));
            } else {
                bail!("Peer with id {:?} is not registered as a recorder", peer_status.peer_id);
            }
        }

        Ok(())
    }

    fn list_cameras(&mut self, addr: Recipient<Message>) -> Result<()> {
        use crate::db::schema::cameras::dsl::*;
        let connection = &mut self.establish_db_connection()?;

        let cams = cameras
            .select(Camera::as_select())
            .load(connection)
            .with_context(|| "Error loading cameras")?;

        addr.do_send(Message(p::OutgoingMessage::ListCameras{
            cameras: cams.clone()
        }));

        cams.iter().for_each(|camera| {
            if let Some(peer_id) = self.recorders.get(&camera.id) {
                if let Some((_, peer_status)) = self.peers.get(peer_id) {
                    addr.do_send(Message(p::OutgoingMessage::PeerStatusChanged(p::PeerStatus {
                        peer_id: Some(peer_id.to_owned()),
                        roles: peer_status.roles.clone(),
                        meta: peer_status.meta.clone(),
                    })));
                }
            }
        });

        Ok(())
    }

    fn list_cameras_all_listener(&mut self, connection: &mut SqliteConnection) -> Result<()> {
        use crate::db::schema::cameras::dsl::cameras;

        let results = cameras
            .select(Camera::as_select())
            .load(connection)
            .with_context(|| "Error loading cameras")?;
        
        for (_, (addr, status)) in self.peers.iter() {
            if status.listening() {
                addr.do_send(Message(p::OutgoingMessage::ListCameras {
                    cameras: results.clone()
                }));
            }
        }

        Ok(())
    }

    fn start_recorder(&mut self, camera_id: &String) -> Result<()> {
        let id = camera_id.to_owned();
        let port = self.port.to_string();
        let recorder_path = self.recorder_path.clone();
        // Thanks to https://stackoverflow.com/questions/62978157/rust-how-to-spawn-child-process-that-continues-to-live-after-parent-receives-si
        // To spwan the detached recorder process
        let joinhandle: JoinHandle<Result<()>> = thread::Builder::new().spawn( move || {
            unsafe {
                let result = fork().with_context(||"Fork failed")?;
                if let ForkResult::Child = result {
                    let mut cmd = Command::new(recorder_path);
                    let command = cmd.arg("--port").arg(port).arg("--id").arg(id);
                    command.spawn().with_context(||"Fail to spawn child process")?;
                }
                Ok(())
            }
        }).with_context(||"Fail to create new thread")?;

        match joinhandle.join() {
            Ok(result) => {
                if let Err(err) = result {
                    bail!("Error when running the thread: {}", err);
                }
            }
            _ => {
                bail!("Thread failed to join");
            }
        }

        Ok(())
    }

    fn stop_recorder(&mut self, camera_id: &String) -> Result<()> {
        if let Some((addr, peer_status)) = self.recorders.get(camera_id).and_then(|peer_id| self.peers.get(peer_id)) {
            if peer_status.recording() {
                addr.do_send(Message(p::OutgoingMessage::EndSession(p::EndSessionMessage {
                    session_id: camera_id.to_owned(),
                })));
            } else {
                bail!("Peer with id {:?} is not registered as a recorder", peer_status.peer_id);
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
        self.disconnect(&msg.connection_id).unwrap_or_else(|err| {
            error!("Failed to excute disconnect handler: {}", err);
        })
    }
}

/// Handler for SetPeerStatus message. 
impl Handler<SetPeerStatus> for Server {
    type Result = ();

    fn handle(&mut self, msg: SetPeerStatus, _: &mut Context<Self>) {
        self.set_peer_status(&msg.connection_id, msg.peer_status).unwrap_or_else(|err| {
            error!("Failed to set peer status: {}", err);
        })
    }
}

/// Handler for Preview message.
impl Handler<Preview> for Server {
    type Result = ();

    fn handle(&mut self, msg: Preview, _: &mut Context<Self>) {
        self.preview(&msg.camera_id, &msg.camera_url, &msg.connection_id).unwrap_or_else(|err| {
            error!("Failed to preview: {}", err);
        })
    }
}

/// Handler for StopPreview message.
impl Handler<StopPreview> for Server {
    type Result = ();

    fn handle(&mut self, msg: StopPreview, _: &mut Context<Self>) {
        self.stop_preview(&msg.camera_id, &msg.connection_id).unwrap_or_else(|err| {
            error!("Failed to preview: {}", err);
        })
    }
}

/// Handler for StartSession message.
impl Handler<StartSession> for Server {
    type Result = ();

    fn handle(&mut self, msg: StartSession, _: &mut Context<Self>) {
        self.start_session(&msg.producer_id, &msg.consumer_id).unwrap_or_else(|err| {
            error!("Failed to start session: {}", err);
        })
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
        self.peer(&msg.peer_id, msg.peer_msg).unwrap_or_else(|err| {
            error!("Fail to peer: {}",err);
        })
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
        self.remove_camera(&msg.camera.id).unwrap_or_else(|err| {
            error!("Failed to remove camera: {}", err);
        })
    }
}

/// Handler for ListCameras message.
impl Handler<ListCameras> for Server {
    type Result = ();

    fn handle(&mut self, msg: ListCameras, _: &mut Context<Self>) {
        self.list_cameras(msg.addr).unwrap_or_else(|err|{
            error!("Unable to list cameras: {}", err);
        })
    }
}

/// Handler for StartRecorder message.
impl Handler<StartRecorder> for Server {
    type Result = ();

    fn handle(&mut self, msg: StartRecorder, _: &mut Context<Self>) {
        self.start_recorder(&msg.camera.id).unwrap_or_else(|err| {
            error!("Failed to start recorder: {}", err);
        })
    }
}

/// Handler for StopRecorder message.
impl Handler<StopRecorder> for Server {
    type Result = ();

    fn handle(&mut self, msg: StopRecorder, _: &mut Context<Self>) {
        self.stop_recorder(&msg.camera.id).unwrap_or_else(|err| {
            error!("Failed to stop recorder: {}", err);
        })
    }
}
