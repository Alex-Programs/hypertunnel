use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use std::sync::RwLock;
use libtransit::Message;
use once_cell::sync::Lazy;

static C_TO_S_MESSAGES: Lazy<RwLock<Vec<Box<Message>>>> = Lazy::new(|| RwLock::new(Vec::new()));
static S_TO_C_MESSAGES: Lazy<RwLock<Vec<Box<Message>>>> = Lazy::new(|| RwLock::new(Vec::new()));

#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().body("Hello world!")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| App::new().service(index))
        .bind(("0.0.0.0", 8080))?
        .run()
        .await
}