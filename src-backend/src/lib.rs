pub mod db;
pub mod ws;
use anyhow::Result;
use clap::Parser;
use tracing_subscriber::prelude::*;

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
    /// db
    #[clap(short, long, default_value = "dev.db")]
    pub db: String,
}

pub fn initialize_logging() -> Result<()> {
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

pub fn get_args() -> Args {
    Args::parse()
}
