use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use std::sync::RwLock;
use libtransit::Message;
use once_cell::sync::Lazy;

mod config;

static C_TO_S_MESSAGES: Lazy<RwLock<Vec<Box<Message>>>> = Lazy::new(|| RwLock::new(Vec::new()));
static S_TO_C_MESSAGES: Lazy<RwLock<Vec<Box<Message>>>> = Lazy::new(|| RwLock::new(Vec::new()));

struct AppState {
    config: config::Config,
}

#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().body("Hello world!")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Loading configuration...");
    let configuration = config::load_config();

    let appstate = web::Data::new(AppState {
        config: configuration.clone(),
    });

    println!("Starting server...");
    HttpServer::new(move || {
            App::new()
                .app_data(appstate.clone())
                .service(index)
        })
        .bind((configuration.host, configuration.port))?
        .run()
        .await
}