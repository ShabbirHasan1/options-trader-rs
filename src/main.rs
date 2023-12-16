mod web_client;

use web_client::WebClient;

#[tokio::main]
async fn main() {
    web_client = WebClient::new();
}
