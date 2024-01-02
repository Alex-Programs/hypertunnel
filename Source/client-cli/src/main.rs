use clientcore::{begin_core_client, ClientArguments};

#[tokio::main]
async fn main() {
    let args = ClientArguments {
        listen_address: "127.0.0.1".to_string(),
        listen_port: 1080,
        target_host: "http://127.0.0.1:8000".to_string(),
        password: "12345".to_string()
    };

    begin_core_client(args).await;
}