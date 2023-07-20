use actix_web::{get, post, web, App, HttpResponse, HttpServer, HttpRequest, Responder};
use actix_web::web::Bytes;
use libsecrets::{EncryptionKey, self};
use rand::Rng;

use debug_print::{
    debug_print as dprint,
    debug_println as dprintln,
    debug_eprint as deprint,
    debug_eprintln as deprintln,
};

use dashmap::DashMap;

mod config;
mod transit_socket;
use transit_socket::{TransitSocket, ClientStatistics};

// State passed to all request handlers
struct AppState {
    config: config::Config, // Configuration
    sessions: DashMap<[u8; 16], TransitSocket>, // Currently-in-use sessions with the
    //                                             client identifier as the key
    users: Vec<User>, // Users from the configuration with the passwords preprocessed into keys
    //                   for faster initial handshake when there are many users
}

// Simple user definition
struct User {
    name: String,
    password: String,
    key: EncryptionKey,
}

// Temporary - used to check the server works
#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().body("Hello world!")
}

// First request, which associates a client identifier with a key and proves the server is legitimate
#[post("/submit")]
async fn client_greeting(app_state: web::Data<AppState>,req: HttpRequest, body_bytes: Bytes) -> impl Responder {
    dprintln!("Received request to /submit");
    // Get cookie for client identifier
    let token = req.cookie("token");

    // Check if token exists and if so, parse it
    let token = match token {
        Some(token) => {
            // Check it's 16 bytes of u8s
            let token = token.value().to_string();
            if token.len() != 16 {
                // Return 404
                return HttpResponse::NotFound().body("No page exists");
            }

            // Convert to bytes
            let token_bytes = token.as_bytes();

            // Convert to array
            let mut token = [0u8; 16];
            for i in 0..16 {
                token[i] = token_bytes[i];
            }

            dprintln!("Token correct!: {:?}", token);

            // Implicit return
            token
        }
        None => {
            // Return 404 - resist active probing by not telling the client what went wrong
            dprintln!("No token supplied!");
            return HttpResponse::NotFound().body("No page exists");
        }
    };

    // Get body
    let body = body_bytes.to_vec();
    
    // Iterate through users trying their keys. TODO consider Rayon here for performance
    let mut key: Option<EncryptionKey> = None;
    for user in &app_state.users {
        // Attempt to decrypt body with user's key
        let decrypted = match libsecrets::decrypt(&body, &user.key) {
            Ok(decrypted) => decrypted, // If it succeeds, pull out content from Result<>
            Err(_) => continue, // If it fails, try the next user
        };

        // Convert to string. It should always be valid ASCII, and UTF-8 is a superset of ASCII
        let decrypted = match String::from_utf8(decrypted) {
            Ok(decrypted) => decrypted,
            Err(_) => continue,
        };

        // Check if decrypted data contains "Hello!"
        if decrypted.contains(&"Hello!".to_string()) {
            // Set key
            key = Some(user.key.clone()); // We've found it!
            dprintln!("Key found! User: {}", user.name);
            break;
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
        Err(_) => { // This should never happen, but if it does, return 404
            // Return 404
            return HttpResponse::NotFound().body("No page exists");
        }
    };

    dprintln!("Formed response: {:?}", encrypted);

    // Add it to the sessions
    let socket = TransitSocket {
        key,
        client_statistics: ClientStatistics {
            to_send: 0,
            to_reply: 0,
        },
    };

    app_state.sessions.insert(token, socket);

    dprintln!("Added session to sessions; returning response");

    // Return encrypted response
    HttpResponse::Ok().body(encrypted)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Loading configuration...");
    let configuration = config::load_config();

    // Create users from configuration
    // Check if there are any users
    if configuration.users.len() == 0 {
        panic!("No users defined in configuration file!");
    }

    println!("Hashing user keys...");
    let mut users = Vec::new();
    for user in &configuration.users {
        let key = libsecrets::form_key(user.password.as_bytes());

        users.push(User {
            name: user.name.clone(),
            password: user.password.clone(),
            key,
        });

        println!("Hashed key for user '{}'", user.name);
    }

    // Create appstate that will be provided to requests
    let appstate = web::Data::new(AppState {
        config: configuration.clone(),
        sessions: DashMap::new(),
        users,
    });

    println!("Starting server at {}:{}", configuration.host, configuration.port);
    HttpServer::new(move || { // Closure - inline function. Move keyword moves ownership of configuration into the closure
            App::new()
                .app_data(appstate.clone()) // Insert appdata
                .service(index) // Insert index route
                .service(client_greeting) // Insert client_greeting route
        })
        .bind((configuration.host, configuration.port))? // Bind to host and port
        .run() // Execute
        .await // Wait for completion with the asynchronous runtime
}