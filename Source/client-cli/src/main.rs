use clientcore::{begin_core_client, ClientArguments};
use simple_logger;
use log::{info, Level};
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

    #[clap(long, default_value = "8")]
    push_client_count: usize,

    #[clap(long, default_value = "8")]
    pull_client_count: usize,

    #[clap(long, default_value = "128")]
    timeout_time_s: usize,

    #[clap(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() {
    let arguments = Args::parse();

    let level: Level = match arguments.log_level.to_lowercase().as_str() {
        "trace" => Level::Trace,
        "debug" => Level::Debug,
        "info" => Level::Info,
        "warn" => Level::Warn,
        "error" => Level::Error,
        _ => Level::Info,
    };

    simple_logger::set_up_color_terminal();
    simple_logger::init_with_level(level).unwrap();

    info!("Received arguments: {:?}", arguments);

    let client_args = ClientArguments {
        listen_address: arguments.listen_host,
        listen_port: arguments.listen_port,
        target_host: arguments.target_host,
        password: arguments.password,
        push_client_count: arguments.push_client_count,
        pull_client_count: arguments.pull_client_count,
        timeout_time_s: arguments.timeout_time_s,
        client_name: "Client CLI via Client Core".to_string(),
    };

    begin_core_client(client_args).await;
}