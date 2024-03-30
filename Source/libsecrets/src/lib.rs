use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng, Key},
    ChaCha20Poly1305, Nonce
};

#[derive(Debug)]
pub enum EncryptionError {
    CiphertextTooShort { actual: usize, minimum: usize },
    CipherFailure(chacha20poly1305::Error),
}

impl From<chacha20poly1305::Error> for EncryptionError {
    fn from(error: chacha20poly1305::Error) -> Self {
        Self::CipherFailure(error)
    }
}

impl std::fmt::Display for EncryptionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CiphertextTooShort { actual, minimum } => write!(
                formatter,
                "ciphertext is {actual} bytes; at least {minimum} bytes are required"
            ),
            Self::CipherFailure(_) => write!(formatter, "cipher operation failed"),
        }
    }
}

impl std::error::Error for EncryptionError {}

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
    const NONCE_LENGTH: usize = 12;

    if data.len() < NONCE_LENGTH {
        return Err(EncryptionError::CiphertextTooShort {
            actual: data.len(),
            minimum: NONCE_LENGTH,
        });
    }

    let nonce = Nonce::from_slice(&data[data.len() - NONCE_LENGTH..]);
    let data = &data[..data.len() - NONCE_LENGTH];

    let cipher = ChaCha20Poly1305::new(key);
    let decrypted = cipher.decrypt(nonce, data.as_ref())?;
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
    fn test_reject_short_ciphertext() {
        let key = form_key(b"Hello, world!");
        let result = decrypt(&[0; 11], &key);

        assert!(matches!(
            result,
            Err(EncryptionError::CiphertextTooShort {
                actual: 11,
                minimum: 12
            })
        ));
    }

    #[test]
    fn test_form_key() {
        let key = form_key(b"Hello, world!");
        assert_eq!(key.len(), 32);
    }
}
