use clap::Parser;
use std::env;
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::error;
use tracing::info;
use tracing::warn;

mod db_client;
mod settings;
mod utils;
mod web_client;

use db_client::DBClient;
use settings::Config;
use web_client::WebClient;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    settings: String,
}

fn start_logging() {
    let subscriber = tracing_subscriber::fmt()
        // Display source code file paths
        .with_file(true)
        // Display source code line numbers
        .with_line_number(true)
        // Display the thread ID an event was recorded on
        .with_thread_ids(true)
        // Don't display the event's target (module path)
        .with_target(false)
        // Assign a log-level
        .with_max_level(tracing::Level::DEBUG)
        // Use a more compact, abbreviated log format
        .compact()
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
}

fn graceful_shutdown(is_graceful_shutdown: &mut bool, shutdown_signal: &CancellationToken) {
    *is_graceful_shutdown = true;
    info!("Graceful shutdown initiated");
    shutdown_signal.cancel();
}

async fn startup_db() -> DBClient {
    let config =
        env::var("OPTIONS_CFG").expect("Failed to get the cfg file from the environment variable.");
    let settings = Config::read_config_file(&config).expect("Failed to parse config file");
    match DBClient::new(&settings).await {
        Err(val) => {
            info!("Settings file error: {val}");
            std::process::exit(1);
        }
        Ok(val) => val,
    }
}

const BASE_URL_UAT: &str = "api.tastyworks.com";
const WS_URL_UAT: &str = "streamer.tastyworks.com";

#[tokio::main]
async fn main() {
    start_logging();
    info!("___/********Options Trader********\\___");
    let cmdline_args = Args::parse();
    let settings = match Config::read_config_file(cmdline_args.settings.as_str()) {
        Err(val) => {
            info!("Settings file error: {val}");
            std::process::exit(1);
        }
        Ok(val) => val,
    };
    let cancel_token = CancellationToken::new();
    let mut web_client = match WebClient::new(BASE_URL_UAT, cancel_token.clone()).await {
        Ok(val) => val,
        Err(err) => {
            error!("{}", err);
            std::process::exit(1);
        }
    };
    let db = startup_db().await;
    let mut is_graceful_shutdown = false;
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate()).unwrap();
    let mut receiver = web_client.subscribe_to_events();
    if let Err(err) = web_client.startup(WS_URL_UAT, settings, &db).await {
        error!("Failed to startup web_client, error: {}, exiting app", err);
        std::process::exit(1);
    }
    loop {
        tokio::select! {
            event = receiver.recv() => {
                info!("Receieved message from websocket {}", event.unwrap());
            }
            _ = cancel_token.cancelled() => {
                if is_graceful_shutdown {
                    std::process::exit(0);
                }
                else {
                    warn!("exiting early");
                    std::process::exit(1)
                }
            }
            _ = sigterm.recv() => {
                graceful_shutdown(&mut is_graceful_shutdown, &cancel_token);
            }
            _ = signal::ctrl_c() => {
                graceful_shutdown(&mut is_graceful_shutdown, &cancel_token);
            }
        }
    }
}
