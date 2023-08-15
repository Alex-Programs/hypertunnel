use libtransit::DeclarationToken;
use hex;

use debug_print::{
    debug_eprint as deprint, debug_eprintln as deprintln, debug_print as dprint,
    debug_println as dprintln,
};

pub fn parse_token(token_hex: String) -> Option<DeclarationToken> {
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