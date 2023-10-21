use anyhow::{Context as _, Result, bail, anyhow};
use async_tungstenite::tungstenite::Message;
use futures::channel::mpsc;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use src_backend::ws::protocol as p;
use tokio::runtime::Handle;
use tokio::time::{sleep, timeout, Duration};
use tracing::{debug, error, info};

#[tokio::main]
async fn main() -> Result<()> {
    src_backend::initialize_logging()?;

    let handle = Handle::current();

    // start gstreamer pipeline
    // if the pipeline fails, terminate the application
    let meta = Some(serde_json::json!({ "id": "9989dfdc-5aa4-4a23-864a-37d0d259aff3" }));
    while let Err(err) = connect(&handle, &meta).await {
        error!("Connect failed due to: {}", err);
        sleep(Duration::from_secs(1)).await;
    }

    Ok(())
}

async fn connect(handle: &Handle, meta: &Option<serde_json::Value>) -> Result<()> {
    let arg = src_backend::get_args();
    let uri = format!("ws://{}:{}/ws", arg.host, arg.port);

    let (ws, _) = timeout(
        Duration::from_secs(5),
        async_tungstenite::tokio::connect_async(uri),
    )
    .await??;

    info!("Server connected");
    let (mut ws_sink, mut ws_stream) = ws.split();
  
    // 1000 is completely arbitrary, we simply don't want infinite piling
    // up of messages as with unbounded
    let (mut websocket_sender, mut websocket_receiver) = mpsc::channel::<p::IncomingMessage>(1000);
    
    let send_task_handle = handle.spawn(async move {
            while let Some(msg) = websocket_receiver.next().await {
                ws_sink
                    .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                    .await.with_context(|| "Error sending websocket msg")?;
            }
            ws_sink.close().await.with_context(|| "Error cloing ws sink: {}")?;
            Ok(())
        });

    let mut websocket_sender_clone = websocket_sender.clone();
    let meta_clone = meta.to_owned();
    let receive_task_handle = handle.spawn(async move {
            while let Some(msg) = tokio_stream::StreamExt::next(&mut ws_stream).await {
                debug!("Received message {:?}", msg);
                match msg {
                    Ok(Message::Text(msg)) => {
                        if let Ok(msg) = serde_json::from_str::<p::OutgoingMessage>(&msg) {
                            match msg {
                                p::OutgoingMessage::Welcome { peer_id } => {
                                    websocket_sender_clone.send(p::IncomingMessage::SetPeerStatus(
                                         p::PeerStatus {
                                            peer_id: Some(peer_id),
                                            meta: meta_clone.clone(),
                                            roles: vec![p::PeerRole::Recorder],
                                        }
                                    )).await.with_context(|| "Error sending mpsc msg")?;
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(Message::Close(reason)) => {
                        bail!("Websocket closed due to: {:?}", reason);
                    }
                    Ok(_) => (),
                    Err(err) => {
                        bail!("Websocket caught an error: {}", err);
                    }
                }
            }
            Ok(())
        });
    let mut tasks = FuturesUnordered::new();
    tasks.push(send_task_handle);
    tasks.push(receive_task_handle);

    let mut result: Result<()> = Ok(());
    while let Some(finished_task) = tasks.next().await {
        websocket_sender.close().await.unwrap_or_else(|err| {
            error!("Error closing mpsc channel: {}", err);
        });
        match finished_task {
            Err(e) => {
                error!("Caught join error: {}", e);
                tasks.iter().for_each(|v| {
                    v.abort();
                });
                result = Err(anyhow!("Some task unexpectly terminated"));
            }
            Ok(Err(e)) => {
                error!("Task failed with: {}", e);
                tasks.iter().for_each(|v| {
                    v.abort();
                });
                result = Err(anyhow!("Some task unexpectly terminated"));
            } 
            _ => {},
        }
    }

    result
}
