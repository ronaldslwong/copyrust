use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64;
use once_cell::sync::OnceCell;
use serde_json;
use sha2::{Digest, Sha256};
use solana_sdk::signature::{Keypair};
use std::error::Error;
use std::fs;

static GLOBAL_KEYPAIR: OnceCell<Keypair> = OnceCell::new();

/// Load and decrypt the keypair, storing it in a global static.
pub fn load_wallet_keypair_global(path: &str, passphrase: &str) -> Result<(), Box<dyn Error>> {
    let keypair = decrypt_and_load_keypair(path, passphrase)?;
    GLOBAL_KEYPAIR
        .set(keypair)
        .map_err(|_| "Keypair already initialized".into())
}

/// Get a reference to the global keypair (after calling load_wallet_keypair_global)
pub fn get_wallet_keypair() -> &'static Keypair {
    GLOBAL_KEYPAIR.get().expect("Keypair not initialized")
}

/// Decrypts the encrypted keypair file and returns a Keypair
fn decrypt_and_load_keypair(path: &str, passphrase: &str) -> Result<Keypair, Box<dyn Error>> {
    // Read the base64-encoded ciphertext from file
    let encoded = fs::read_to_string(path)?;
    let ciphertext = base64::decode(encoded.trim())?;

    if ciphertext.len() < 12 {
        return Err("Ciphertext too short".into());
    }

    // Derive AES-256 key from passphrase
    let key = Sha256::digest(passphrase.as_bytes());
    let key = Key::<Aes256Gcm>::from_slice(&key);

    // Split nonce and actual ciphertext
    let (nonce_bytes, ciphertext) = ciphertext.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    // Decrypt
    let cipher = Aes256Gcm::new(key);
    let decrypted = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("AES decrypt error: {:?}", e))?;

    // The decrypted data is a JSON array of bytes (like [1,2,3,...])
    let secret_bytes: Vec<u8> = serde_json::from_slice(&decrypted)?;
    let keypair = Keypair::from_bytes(&secret_bytes)?;

    Ok(keypair)
}
