use actix_web::{get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder, http::header::ContentType};

use debug_print::{
    debug_eprint as deprint, debug_eprintln as deprintln, debug_print as dprint,
    debug_println as dprintln,
};

use crate::shared::parse_token;
use libsecrets::{self, EncryptionKey};
use std::sync::RwLock;

use rand;
use rand::Rng;

use once_cell::sync::Lazy;

static APP_STATE: Lazy<RwLock<ControlServerState>> = Lazy::new(|| RwLock::new(ControlServerState {
    config: None,
    users: vec![],
    available_ports: vec![],
}));

#[post("/submit")]
async fn client_hello(req: HttpRequest, body_bytes: web::Bytes) -> impl Responder {
    dprintln!("Received request to /submit");

    // Get token
    let token = req.cookie("token");

    let token = match token {
        Some(token) => {
            match parse_token(token.value().to_string()) {
                Some(token) => token,
                None => {
                    // Return 404 - resist active probing by not telling the client what went wrong
                    dprintln!("Token is invalid!");
                    return HttpResponse::NotFound().body("No page exists");
                }
            }
        }
        None => {
            // Return 404 - resist active probing by not telling the client what went wrong
            dprintln!("No token (client identifier) supplied!");
            return HttpResponse::NotFound().body("No page exists");
        }
    };

    // Get body
    let body = body_bytes.to_vec();

    // Iterate through users trying their keys. TODO consider Rayon here for performance
    let mut key: Option<EncryptionKey> = None;
    for user in APP_STATE.read().unwrap().users.iter() {
        // Attempt to decrypt body with user's key
        let decrypted = match libsecrets::decrypt(&body, &user.key) {
            Ok(decrypted) => decrypted, // If it succeeds, pull out content from Result<>
            Err(_) => continue,                  // If it fails, try the next user
        };

        // Convert to string. It should always be valid ASCII, and UTF-8 is a superset of ASCII
        let decrypted = match String::from_utf8(decrypted) {
            Ok(decrypted) => decrypted,
            Err(_) => continue,
        };

        // Check if decrypted data contains "Hello!"
        if decrypted.contains(&"Hello. Protocol version:".to_string()) {
            // Set key
            key = Some(user.key.clone()); // We've found it!
            dprintln!("Key found! User: {}", user.name);
            dprintln!("Decrypted data: {}", decrypted);
            break;
        } else {
            dprintln!("Data decrypted, but does not contain client hello!");
        }
    }

    // Check if key was found. If so, extract contents of Option<>
    let key = match key {
        Some(key) => key,
        None => {
            // Return 404
            dprintln!("No key found!");
            return HttpResponse::NotFound().body("No page exists");
        }
    };

    // Generate response. Encrypted with their key to show we're legitimate
    let response_text = "CONNECTION ACCEPTED";

    // Pad with 0-100 spaces to avoid fingerprinting based on response length
    let mut rng = rand::thread_rng();
    let amount = rng.gen_range(0..100);
    let response_text = response_text.to_string() + &" ".repeat(amount);

    // Encrypt response
    let encrypted = match libsecrets::encrypt(response_text.as_bytes(), &key) {
        Ok(encrypted) => encrypted,
        Err(_) => {
            // This should never happen, but if it does, return 404
            // Return 404
            return HttpResponse::NotFound().body("No page exists");
        }
    };

    dprintln!("Formed response: {:?}", encrypted);

    // Return encrypted response
    HttpResponse::Ok().body(encrypted)
}

struct KeyGeneratedUser {
    name: String,
    key: EncryptionKey,
}

struct ControlServerState {
    config: Option<crate::config::Config>,
    users: Vec<KeyGeneratedUser>,
    available_ports: Vec<u16>,
}

pub async fn start_control_server(config: crate::config::Config) {
    {
        let available_ports = (config.start_port..config.max_port).collect::<Vec<u16>>();

        let mut users = vec![];
        for user in config.users.iter() {
            let key = libsecrets::form_key(user.password.as_bytes());
            users.push(KeyGeneratedUser {
                name: user.name.clone(),
                key,
            });
        }

        let new_state = ControlServerState {
            config: Some(config.clone()),
            users,
            available_ports,
        };

        let mut state_mut = APP_STATE.write().unwrap();
        *state_mut = new_state;
    }

    HttpServer::new(|| {
        App::new()
            .service(client_hello)
    })
    .workers(1)
    .bind((config.host, config.control_port))
    .unwrap()
    .run()
    .await
    .unwrap();
}
