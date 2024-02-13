use clientcore::{begin_core_client, ClientArguments};

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(name = "Hypertunnel Client")]
#[command(version, about, author)]
struct Args {
    #[clap(long, default_value = "127.0.0.1")]
    listen_host: String,

    #[clap(long, default_value = "1080")]
    listen_port: u16,

    #[clap(long, default_value = "http://127.0.1:8000")]
    target_host: String,

    #[clap(long, default_value = "12345")]
    password: String,
}

#[tokio::main]
async fn main() {
    let arguments = Args::parse();

    let client_args = ClientArguments {
        listen_address: arguments.listen_host,
        listen_port: arguments.listen_port,
        target_host: arguments.target_host,
        password: arguments.password,
    };

    begin_core_client(client_args).await;
}