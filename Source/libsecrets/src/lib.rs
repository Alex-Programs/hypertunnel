use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng, Key},
    ChaCha20Poly1305, Nonce
};

pub use chacha20poly1305::Error;
pub type EncryptionError = Error;

use sha2::{Sha256, Digest};

pub type EncryptionKey = Key<ChaCha20Poly1305>;

pub fn form_key(data: &[u8]) -> EncryptionKey {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize()
}

fn random_nonce() -> Nonce {
    ChaCha20Poly1305::generate_nonce(&mut OsRng)
}

pub fn encrypt(data: &[u8], key: &EncryptionKey) -> Result<Vec<u8>, EncryptionError> {
    let nonce = random_nonce();
    let cipher = ChaCha20Poly1305::new(key);
    let mut encrypted = cipher.encrypt(&nonce, data.as_ref())?;
    encrypted.extend_from_slice(nonce.as_ref()); // Append 12-byte nonce
    Ok(encrypted)
}

pub fn decrypt(data: &[u8], key: &EncryptionKey) -> Result<Vec<u8>, EncryptionError> {
    let nonce = Nonce::from_slice(&data[data.len() - 12..]); // Extract 12-byte nonce
    let data = &data[..data.len() - 12]; // Remove 12-byte nonce

    let cipher = ChaCha20Poly1305::new(key);
    let decrypted = cipher.decrypt(&nonce, data.as_ref())?;
    Ok(decrypted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let key = form_key(b"01234567890123456789012345678901");
        let data = b"Hello world!";
        let encrypted = encrypt(data, &key).unwrap();
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(data, decrypted.as_slice());
    }

    #[test]
    fn test_decryption_wrong_key() {
        let key = form_key(b"01234567890123456789012345678901");
        let data = b"Hello world!";
        let encrypted = encrypt(data, &key).unwrap();
        let key = form_key(b"Hello World!");
        let decrypted = decrypt(&encrypted, &key);
        assert!(decrypted.is_err());
    }

    #[test]
    fn test_form_key() {
        let key = form_key(b"Hello, world!");
        assert_eq!(key.len(), 32);
    }
}