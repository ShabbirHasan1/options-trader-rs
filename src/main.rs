use clap::Parser;
use tracing::info;

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
        .with_max_level(tracing::Level::INFO)
        // Use a more compact, abbreviated log format
        .compact()
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
}

#[tokio::main]
async fn main() {
    start_logging();
    let cmdline_args = Args::parse();
    let settings = match Config::read_config_file(cmdline_args.settings.as_str()) {
        Err(val) => {
            info!("Settings file error: {val}");
            std::process::exit(1);
        }
        Ok(val) => val,
    };
    let db = DBClient::new(&settings).await.unwrap();
    let web_client = WebClient::new(
        "api.cert.tastyworks.com",
        "streamer.cert.tastyworks.com",
        db,
    )
    .await;
}
