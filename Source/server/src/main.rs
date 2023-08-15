use tokio::runtime;
mod config;
mod control_server;
mod shared;

fn main() {
    // Start a single-threaded tokio runtime
    let rt = runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let configuration = config::load_config();

    // Start the control server
    rt.block_on(control_server::start_control_server(configuration));
}
