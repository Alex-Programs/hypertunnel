use actix_web::{get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder, http::header::ContentType};

use debug_print::{
    debug_eprint as deprint, debug_eprintln as deprintln, debug_print as dprint,
    debug_println as dprintln,
};

use crate::shared::parse_token;
use libsecrets::{self, EncryptionKey};

use rand;
use rand::Rng;

