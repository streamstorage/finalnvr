use anyhow::{Context as _, Result, bail, anyhow};
use async_tungstenite::tungstenite::Message;
use clap::Parser;
use futures::channel::mpsc;
use futures::prelude::*;
use futures::stream::FuturesUnordered;
use glib::MainLoop;
use gst::prelude::*;
use src_backend::ws::protocol as p;
use tokio::runtime::Handle;
use tokio::time::{sleep, timeout, Duration};
use tracing::{debug, error, info, warn};

#[derive(Parser, Debug)]
#[clap(about, version, author)]
/// Program arguments
pub struct Args {
    /// Port to listen on
    #[clap(short, long, default_value_t = 8080)]
    pub port: u16,
    /// id
    #[clap(short, long, default_value = "9989dfdc-5aa4-4a23-864a-37d0d259aff3")]
    pub id: String,
    #[clap(short, long)]
    pub camera_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    src_backend::initialize_logging()?;
    // fork
    nix::unistd::setsid()?;

    let args = Args::parse();
    let url = format!("ws://127.0.0.1:{}/ws", args.port);

    let handle = Handle::current();

    let controller = "tcp://127.0.0.1:9090";
    let stream = format!("{}/{}", "examples", &args.id);
    let buffer_size = 100 * 1024 *1024;

    // start gstreamer pipeline
    // if the pipeline fails, terminate the application
    gst::init()?;
    let main_loop = MainLoop::new(None, false);

    let pipeline_str = format!(
        "rtspsrc name=rtspsrc location={} drop-on-latency=true latency=50 \
            ! rtph264depay ! h264parse ! video/x-h264,alignment=au \
            ! timestampcvt input-timestamp-mode=start-at-current-time \
            ! queue max-size-buffers=0 max-size-time=0 max-size-bytes={} \
            ! pravegasink allow-create-scope=true controller={} stream={} sync=false buffer-size={} timestamp-mode=tai",
        args.camera_url, buffer_size, controller, stream, buffer_size
    );
    let pipeline = gst::parse_launch(&pipeline_str)?;
    let bus = pipeline.bus().with_context(||"Fail to get pipeline bus")?;
    info!("pipeline: {}", pipeline_str);

    let main_loop_clone = main_loop.clone();
    let _bus_watch = bus
        .add_watch(move |_, msg| {
            use gst::MessageView;

            let main_loop: &MainLoop = &main_loop_clone;
            match msg.view() {
                MessageView::Eos(..) => main_loop.quit(),
                MessageView::Error(err) => {
                    println!(
                        "Error from {:?}: {} ({:?})",
                        err.src().map(|s| s.path_string()),
                        err.error(),
                        err.debug()
                    );
                    main_loop.quit();
                }
                _ => (),
            };

            glib::ControlFlow::Continue
        })?;

    let handle_clone = handle.clone();
    let main_loop_clone = main_loop.clone();
    let live_check_task = handle.spawn(async move {
        while let Err(err) = connect(&handle_clone, &url, &args.id, main_loop_clone.clone()).await {
            error!("Connect failed due to: {}", err);
            sleep(Duration::from_secs(1)).await;
        }
    });

    info!("live check task spawned");
    let main_loop_clone = main_loop.clone();
    handle.spawn_blocking(move || {
        pipeline.set_state(gst::State::Playing).unwrap();
        info!("pipeline is playing");
        main_loop_clone.run();
        live_check_task.abort();
        pipeline.set_state(gst::State::Null).unwrap();
    }).await?;

    Ok(())
}

async fn connect(handle: &Handle, url: &String, id: &String, main_loop: MainLoop) -> Result<()> {
    let (ws, _) = timeout(
        Duration::from_secs(5),
        async_tungstenite::tokio::connect_async(url),
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
    let camera_id = id.to_owned();
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
                                            meta: Some(serde_json::json!({ "id": camera_id.clone() })),
                                            roles: vec![p::PeerRole::Recorder],
                                        }
                                    )).await.with_context(|| "Error sending mpsc msg")?;
                                }
                                p::OutgoingMessage::EndSession(p::EndSessionMessage { session_id}) => {
                                    info!("Stopping recorder now...");
                                    if session_id == camera_id {
                                        websocket_sender_clone.close().await.unwrap_or_else(|err| {
                                            error!("Error closing mpsc channel: {}", err);
                                        });
                                        main_loop.quit();
                                        break;
                                    }
                                }
                                _ => {
                                    warn!("Got unknown msg: {:?}", msg);
                                }
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
