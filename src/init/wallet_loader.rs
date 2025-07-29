use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64;
use once_cell::sync::OnceCell;
use serde_json;
use sha2::{Digest, Sha256};
use solana_sdk::signature::{Keypair};
use solana_sdk::pubkey::Pubkey;
use std::error::Error;
use std::fs;
use solana_sdk::signer::Signer;


static GLOBAL_KEYPAIR: OnceCell<Keypair> = OnceCell::new();
static GLOBAL_WALLET_PUBKEY: OnceCell<Pubkey> = OnceCell::new();
static GLOBAL_NONCE_ACCOUNTS: OnceCell<Vec<Keypair>> = OnceCell::new();
static GLOBAL_NONCE_PUBKEYS: OnceCell<Vec<Pubkey>> = OnceCell::new();
static GLOBAL_NONCE_INDEX: OnceCell<std::sync::atomic::AtomicUsize> = OnceCell::new();

/// Load and decrypt the keypair, storing it in a global static.
pub fn load_wallet_keypair_global(path: &str, passphrase: &str) -> Result<(), Box<dyn Error>> {
    let keypair = decrypt_and_load_keypair(path, passphrase)?;
    let pubkey = keypair.pubkey();
    
    GLOBAL_KEYPAIR
        .set(keypair)
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Keypair already initialized")))?;
    
    GLOBAL_WALLET_PUBKEY
        .set(pubkey)
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Wallet pubkey already initialized")))?;
    
    Ok(())
}

/// Get a reference to the global keypair (after calling load_wallet_keypair_global)
pub fn get_wallet_keypair() -> &'static Keypair {
    GLOBAL_KEYPAIR.get().expect("Keypair not initialized")
}

/// Load multiple nonce account keypairs, storing them in a global static.
pub fn load_nonce_account_global(path: &str) -> Result<(), Box<dyn Error>> {
    let nonce_keypairs = load_nonce_account_keypairs(path)?;
    let nonce_pubkeys: Vec<Pubkey> = nonce_keypairs.iter().map(|kp| kp.pubkey()).collect();
    
    GLOBAL_NONCE_ACCOUNTS
        .set(nonce_keypairs)
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Nonce accounts already initialized")))?;
    
    GLOBAL_NONCE_PUBKEYS
        .set(nonce_pubkeys)
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Nonce pubkeys already initialized")))?;
    
    GLOBAL_NONCE_INDEX
        .set(std::sync::atomic::AtomicUsize::new(0))
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Nonce index already initialized")))?;
    
    let count = GLOBAL_NONCE_ACCOUNTS.get().unwrap().len();
    println!("[WALLET_LOADER] Loaded {} nonce accounts", count);
    
    Ok(())
}

/// Get the next nonce account keypair in rotation (after calling load_nonce_account_global)
pub fn get_next_nonce_account_keypair() -> &'static Keypair {
    match GLOBAL_NONCE_ACCOUNTS.get() {
        Some(accounts) => {
            match GLOBAL_NONCE_INDEX.get() {
                Some(index) => {
                    let current_index = index.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let actual_index = current_index % accounts.len();
                    
                    println!("[WALLET_LOADER] Using nonce account {} of {}", actual_index + 1, accounts.len());
                    &accounts[actual_index]
                }
                None => {
                    println!("[WALLET_LOADER] Nonce index not initialized, using main wallet keypair");
                    get_wallet_keypair()
                }
            }
        }
        None => {
            println!("[WALLET_LOADER] No nonce accounts initialized, using main wallet keypair");
            get_wallet_keypair()
        }
    }
}

/// Get the next nonce account pubkey (after calling load_nonce_account_global)
pub fn get_nonce_account() -> &'static Pubkey {
    match GLOBAL_NONCE_PUBKEYS.get() {
        Some(pubkeys) => {
            match GLOBAL_NONCE_INDEX.get() {
                Some(index) => {
                    let current_index = index.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let actual_index = current_index % pubkeys.len();
                    
                    println!("[WALLET_LOADER] Using nonce account {} of {}", actual_index + 1, pubkeys.len());
                    &pubkeys[actual_index]
                }
                None => {
                    println!("[WALLET_LOADER] Nonce index not initialized, using main wallet pubkey");
                    GLOBAL_WALLET_PUBKEY.get().expect("Wallet pubkey not initialized")
                }
            }
        }
        None => {
            println!("[WALLET_LOADER] No nonce accounts initialized, using main wallet pubkey");
            GLOBAL_WALLET_PUBKEY.get().expect("Wallet pubkey not initialized")
        }
    }
}

/// Get the next nonce account keypair and pubkey atomically (prevents race conditions)
pub fn get_next_nonce_account_atomic() -> (&'static Keypair, &'static Pubkey) {
    match GLOBAL_NONCE_ACCOUNTS.get() {
        Some(accounts) => {
            match GLOBAL_NONCE_INDEX.get() {
                Some(index) => {
                    let current_index = index.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let actual_index = current_index % accounts.len();
                    
                    println!("[WALLET_LOADER] Using nonce account {} of {} (atomic)", actual_index + 1, accounts.len());
                    (&accounts[actual_index], &GLOBAL_NONCE_PUBKEYS.get().unwrap()[actual_index])
                }
                None => {
                    println!("[WALLET_LOADER] Nonce index not initialized, using main wallet (atomic)");
                    let wallet = get_wallet_keypair();
                    let pubkey = GLOBAL_WALLET_PUBKEY.get().expect("Wallet pubkey not initialized");
                    (wallet, pubkey)
                }
            }
        }
        None => {
            println!("[WALLET_LOADER] No nonce accounts initialized, using main wallet (atomic)");
            let wallet = get_wallet_keypair();
            let pubkey = GLOBAL_WALLET_PUBKEY.get().expect("Wallet pubkey not initialized");
            (wallet, pubkey)
        }
    }
}

/// Get a reference to the global nonce account keypair (legacy function for backward compatibility)
pub fn get_nonce_account_keypair() -> &'static Keypair {
    get_next_nonce_account_keypair()
}

/// Loads multiple nonce account keypairs from a JSON file containing an array of keypairs
fn load_nonce_account_keypairs(path: &str) -> Result<Vec<Keypair>, Box<dyn Error>> {
    // Read the JSON array of keypairs from file
    let content = fs::read_to_string(path)?;
    println!("[WALLET_LOADER] File content length: {}", content.len());
    println!("[WALLET_LOADER] File content preview: {}", &content[..content.len().min(100)]);
    
    // Try to parse as array of arrays first (multiple keypairs)
    let keypairs_data: Result<Vec<Vec<u8>>, _> = serde_json::from_str(&content);
    
    let keypairs_data = match keypairs_data {
        Ok(data) => {
            println!("[WALLET_LOADER] Parsed {} keypairs (array of arrays format)", data.len());
            data
        }
        Err(_) => {
            // Try to parse as single array (one keypair)
            let single_keypair: Vec<u8> = serde_json::from_str(&content)?;
            println!("[WALLET_LOADER] Parsed 1 keypair (single array format)");
            vec![single_keypair]
        }
    };
    
    // Create keypairs from the secret bytes
    let mut keypairs = Vec::new();
    for (i, secret_bytes) in keypairs_data.iter().enumerate() {
        let keypair = Keypair::from_bytes(secret_bytes)?;
        println!("[WALLET_LOADER] Loaded nonce account {}: {}", i + 1, keypair.pubkey());
        keypairs.push(keypair);
    }
    
    // Return the keypairs
    Ok(keypairs)
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
