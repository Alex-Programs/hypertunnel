use actix_web::web::Bytes;
use actix_web::{get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use hex;
use libsecrets::{self, EncryptionKey};
use rand::Rng;
use libtransit::SerialMessage;

use debug_print::{
    debug_eprint as deprint, debug_eprintln as deprintln, debug_print as dprint,
    debug_println as dprintln,
};

use dashmap::DashMap;

mod config;
mod new_transit;
use new_transit::SessionActorsStorage;
mod meta;

use libtransit::{
    ClientMessageUpstream, DeclarationToken, ServerMessageDownstream,
    ServerMetaDownstream, SocketID, SocksSocketUpstream, SocksSocketDownstream, UnifiedPacketInfo,
    ServerMetaDownstreamServerStats
};

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use std::sync::atomic::Ordering;

// State passed to all request handlers
struct AppState {
    config: config::Config,                                     // Configuration
    users: Vec<User>, // Users from the configuration with the passwords preprocessed into keys
    //                   for faster initial handshake when there are many users,
    actor_lookup: DashMap<DeclarationToken, SessionActorsStorage>, // Lookup table for actors
}

// Simple user definition
struct User {
    name: String,
    password: String,
    key: EncryptionKey,
}

async fn form_meta_response(session_actor_storage: &SessionActorsStorage, seq_num: u32) -> ServerMetaDownstream {
    let millis_time = SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").as_millis();

    let traffic_stats = (*session_actor_storage.traffic_stats).into_server_meta_downstream_traffic_stats();

    ServerMetaDownstream {
        packet_info: UnifiedPacketInfo { unix_ms: millis_time as u64, seq_num: seq_num },
        server_stats: ServerMetaDownstreamServerStats { cpu_usage: 0.0, memory_usage_kb: 0 },
        logs: Vec::new(),
        traffic_stats,
    }
}

fn parse_token(token_hex: String) -> Option<DeclarationToken> {
    dprintln!("Hex token: {}", token_hex);

    // Convert to bytes
    let token_bytes = hex::decode(token_hex).unwrap();

    // Check it's the correct length
    if token_bytes.len() != 16 {
        // Return 404
        dprintln!("Token is not 16 bytes!");
        return None;
    }

    // Convert to array
    let token: DeclarationToken = token_bytes[..16].try_into().unwrap();

    dprintln!("Token correct!: {:?}", token);

    // Implicit return
    Some(token)
}

// Temporary - used to check the server works
#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().body("Hello world!")
}

#[get("/download")]
async fn downstream_data(
    app_state: web::Data<AppState>,
    req: HttpRequest,
    body_bytes: Bytes,
) -> impl Responder {
    dprintln!("Received request to download");

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

    // Get actor
    let actor = app_state.actor_lookup.get(&token);
    if actor.is_none() {
        // Return 404 - resist active probing by not telling the client what went wrong
        dprintln!("No session found for token!");
        return HttpResponse::NotFound().body("No page exists");
    }
    let actor = actor.unwrap();

    // Get body
    let body = body_bytes;

    // Decrypt body
    let key = actor.key;

    let mut decrypted = match libsecrets::decrypt(&body, &key) {
        Ok(decrypted) => decrypted, // If it succeeds, pull out content from Result<>
        Err(_) => {
            // Return 404 - resist active probing by not telling the client what went wrong
            dprintln!("Failed to decrypt body!");
            return HttpResponse::NotFound().body("No page exists");
        }
    };

    // Parse into ClientMessageUpstream
    let upstream: ClientMessageUpstream =
        match libtransit::ClientMessageUpstream::decode_from_bytes(&mut decrypted) {
            Ok(upstream) => upstream, // If it succeeds, pull out content from Result<>
            Err(_) => {
                // Return 404 - resist active probing by not telling the client what went wrong
                dprintln!("Failed to parse decrypted body into ClientMessageUpstream");
                return HttpResponse::NotFound().body("No page exists");
            }
        };

    // Don't send on - just get
    let mut downstream_socks_sockets = {
        let return_messages = actor.from_bundler_stream.recv_async().await;

        match return_messages {
            Ok(return_messages) => return_messages,
            Err(_) => {
                // Return 404 - resist active probing by not telling the client what went wrong
                dprintln!("Failed to receive messages from actor!");
                return HttpResponse::NotFound().body("No page exists");
            }
        }
    };

    // Modify traffic stats
    let return_msgs_bytes = downstream_socks_sockets
        .iter()
        .map(|msg| msg.payload.len())
        .sum::<usize>();

    actor.traffic_stats.coordinator_down_to_http_message_passer_bytes.fetch_sub(return_msgs_bytes as u32, Ordering::Relaxed);

    dprintln!("Returning data: {:?}", downstream_socks_sockets);

    // Get meta information
    let meta = form_meta_response(&actor, actor.seq_num_down.fetch_add(1, Ordering::SeqCst)).await;

    // Form response
    let response = ServerMessageDownstream {
        metadata: meta,
        socks_sockets: downstream_socks_sockets,
        payload_size: 0,
    };

    // Encode response
    let response_bytes = response.encoded().unwrap(); // TODO handle error

    // Encrypt response
    let encrypted = libsecrets::encrypt(&response_bytes, &key).unwrap(); // TODO handle error

    // Return response
    HttpResponse::Ok().body(encrypted)
}

#[post("/upload")]
async fn upstream_data(
    app_state: web::Data<AppState>,
    req: HttpRequest,
    body_bytes: Bytes,
) -> impl Responder {
    dprintln!("Received request to upload");

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

    // Get actor
    let actor = app_state.actor_lookup.get(&token);
    if actor.is_none() {
        // Return 404 - resist active probing by not telling the client what went wrong
        dprintln!("No session found for token!");
        return HttpResponse::NotFound().body("No page exists");
    }

    let actor = actor.unwrap();

    dprintln!("Got actor");

    // Get body
    let body = body_bytes;

    dprintln!("Got body");

    // Decrypt body
    let key = actor.key;

    dprintln!("Got key");

    let mut decrypted = match libsecrets::decrypt(&body, &key) {
        Ok(decrypted) => decrypted, // If it succeeds, pull out content from Result<>
        Err(_) => {
            // Return 404 - resist active probing by not telling the client what went wrong
            dprintln!("Failed to decrypt body!");
            return HttpResponse::NotFound().body("No page exists");
        }
    };

    dprintln!("Decrypted data");

    // Parse into ClientMessageUpstream
    let upstream: ClientMessageUpstream =
        match libtransit::ClientMessageUpstream::decode_from_bytes(&mut decrypted) {
            Ok(upstream) => upstream, // If it succeeds, pull out content from Result<>
            Err(_) => {
                // Return 404 - resist active probing by not telling the client what went wrong
                dprintln!("Failed to parse decrypted body into ClientMessageUpstream");
                return HttpResponse::NotFound().body("No page exists");
            }
        };

    // Get sequence number
    let seq_num = upstream.metadata.packet_info.seq_num;

    // Get yellow sockets to close
    let yellow_sockets = upstream.metadata.yellow_to_stop_reading_from;

    while actor.next_seq_num_up.load(Ordering::SeqCst) != seq_num {
        // Wait until that's the case
        // Sleep 1ms
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    // Send on to actor
    dprintln!("Sending on messages to actor...");
    {
        let to_bundler = &actor.to_bundler_stream;

        for socket in upstream.socks_sockets {
            let payload_length = socket.payload.len() as u32;

            to_bundler.send(socket).unwrap();

            actor.traffic_stats.http_up_to_coordinator_bytes.fetch_add(payload_length, Ordering::Relaxed);
        }

        actor.next_seq_num_up.store(seq_num + 1, Ordering::SeqCst);

        dprintln!("Sent on messages to actor");

        dprintln!("Did not send on close socket msgs");
    }

    // Get meta information
    let meta = form_meta_response(&actor, 0).await;

    // Form response
    let response = ServerMessageDownstream {
        socks_sockets: Vec::with_capacity(0),
        metadata: meta,
        payload_size: 0,
    };

    // Encode response
    let response_bytes = response.encoded().unwrap(); // TODO handle error

    // Encrypt response
    let encrypted = libsecrets::encrypt(&response_bytes, &key).unwrap(); // TODO handle error

    // Return response
    HttpResponse::Ok().body(encrypted)
}

// First request, which associates a client identifier with a key and proves the server is legitimate
#[post("/submit")]
async fn client_greeting(
    app_state: web::Data<AppState>,
    req: HttpRequest,
    body_bytes: Bytes,
) -> impl Responder {
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
    for user in &app_state.users {
        // Attempt to decrypt body with user's key
        let decrypted = match libsecrets::decrypt(&body, &user.key) {
            Ok(decrypted) => decrypted, // If it succeeds, pull out content from Result<>
            Err(_) => continue,         // If it fails, try the next user
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

    // Create actor
    let actor_storage = new_transit::create_actor(&key, token).await;
    // Add to sessions
    app_state.actor_lookup.insert(token, actor_storage);

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
        // Check for malformed config
        let mut duplicate_user_before = false;
        let mut duplicate_password_before = false;

        for other_user in &configuration.users {
            if user.name == other_user.name {
                if duplicate_user_before {
                    panic!("Duplicate user '{}'!", user.name);
                }
                duplicate_user_before = true;
            }
            if user.password == other_user.password && user.name != other_user.name {
                if duplicate_password_before {
                    panic!("Duplicate password '{}'!", user.password);
                }
                duplicate_password_before = true;
            }
        }

        let key = libsecrets::form_key(user.password.as_bytes());

        users.push(User {
            name: user.name.clone(),
            password: user.password.clone(),
            key,
        });

        println!("Derived key for user '{}'", user.name);
    }

    let appstate = web::Data::new(AppState {
        config: configuration.clone(),
        users,
        actor_lookup: DashMap::new()
    });

    println!(
        "Starting server at {}:{} with {} workers",
        configuration.host, configuration.port, configuration.workers
    );
    HttpServer::new(move || {
        // Closure - inline function. Move keyword moves ownership of configuration into the closure
        App::new()
            .app_data(appstate.clone()) // Insert appdata
            .service(index) // Insert index route
            .service(client_greeting) // Insert client_greeting route
            .service(upstream_data)
            .service(downstream_data)
    })
    .workers(configuration.workers) // Set number of workers
    .bind((configuration.host, configuration.port))? // Bind to host and port
    .run() // Execute
    .await // Wait for completion with the asynchronous runtime
}
