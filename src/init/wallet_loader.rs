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
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use solana_sdk::hash::Hash;
use solana_client::rpc_client::RpcClient;


static GLOBAL_KEYPAIR: OnceCell<Keypair> = OnceCell::new();
static GLOBAL_WALLET_PUBKEY: OnceCell<Pubkey> = OnceCell::new();
static GLOBAL_NONCE_ACCOUNTS: OnceCell<Vec<Keypair>> = OnceCell::new();
static GLOBAL_NONCE_PUBKEYS: OnceCell<Vec<Pubkey>> = OnceCell::new();
static GLOBAL_NONCE_INDEX: OnceCell<std::sync::atomic::AtomicUsize> = OnceCell::new();

// Global nonce blockhash cache - maps nonce pubkey to (blockhash, last_updated)
static GLOBAL_NONCE_BLOCKHASH_CACHE: OnceCell<Arc<RwLock<HashMap<Pubkey, (Hash, std::time::Instant)>>>> = OnceCell::new();

// Background task handle for nonce blockhash prefetching
static NONCE_PREFETCH_HANDLE: OnceCell<tokio::task::JoinHandle<()>> = OnceCell::new();

// Static mapping from nonce index to nonce address (built at startup)
static NONCE_INDEX_TO_ADDRESS: OnceCell<Vec<Pubkey>> = OnceCell::new();

// Blockhash cache for fast lookups by index (continuously updated)
static NONCE_BLOCKHASH_CACHE: OnceCell<Arc<RwLock<Vec<Hash>>>> = OnceCell::new();

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
        .set(nonce_pubkeys.clone())
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Nonce pubkeys already initialized")))?;
    
    GLOBAL_NONCE_INDEX
        .set(std::sync::atomic::AtomicUsize::new(0))
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Nonce index already initialized")))?;
    
    // NEW: Initialize nonce blockhash infrastructure
    initialize_nonce_blockhash_cache(&nonce_pubkeys)?;
    
    let count = GLOBAL_NONCE_ACCOUNTS.get().unwrap().len();
    println!("[WALLET_LOADER] Loaded {} nonce accounts", count);
    
    Ok(())
}

/// Initialize the nonce blockhash cache infrastructure
fn initialize_nonce_blockhash_cache(nonce_pubkeys: &[Pubkey]) -> Result<(), Box<dyn Error>> {
    // Set up the static index-to-address mapping
    NONCE_INDEX_TO_ADDRESS
        .set(nonce_pubkeys.to_vec())
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Nonce index mapping already initialized")))?;
    
    // Initialize the blockhash cache with real blockhashes from RPC (synchronous prefetch)
    println!("[WALLET_LOADER] Fetching initial blockhashes for {} nonce accounts...", nonce_pubkeys.len());
    
    let rpc_client = crate::init::initialize::GLOBAL_RPC_CLIENT
        .get()
        .ok_or("RPC client not initialized")?;
    
    let mut initial_blockhashes = Vec::new();
    
    for (index, nonce_pubkey) in nonce_pubkeys.iter().enumerate() {
        match fetch_nonce_blockhash_sync(rpc_client, nonce_pubkey) {
            Ok(blockhash) => {
                initial_blockhashes.push(blockhash);
                println!("[WALLET_LOADER] Fetched nonce {} blockhash: {}", index, blockhash);
            }
            Err(e) => {
                eprintln!("[WALLET_LOADER] Failed to fetch nonce {} blockhash: {}. Using default hash.", index, e);
                initial_blockhashes.push(Hash::default());
            }
        }
    }
    
    NONCE_BLOCKHASH_CACHE
        .set(Arc::new(RwLock::new(initial_blockhashes.clone())))
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Nonce blockhash cache already initialized")))?;
    
    // Initialize the global cache
    let global_cache: HashMap<Pubkey, (Hash, std::time::Instant)> = HashMap::new();
    GLOBAL_NONCE_BLOCKHASH_CACHE
        .set(Arc::new(RwLock::new(global_cache)))
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Global nonce blockhash cache already initialized")))?;
    
    println!("[WALLET_LOADER] Nonce blockhash cache infrastructure initialized with {} real blockhashes", initial_blockhashes.len());
    Ok(())
}

/// Get the nonce account index for a given pubkey
pub fn get_nonce_account_index(nonce_pubkey: &Pubkey) -> Option<usize> {
    NONCE_INDEX_TO_ADDRESS
        .get()
        .and_then(|addresses| {
            addresses.iter().position(|addr| addr == nonce_pubkey)
        })
}

/// Get a cached nonce blockhash by index (fast lookup)
pub fn get_cached_nonce_blockhash(nonce_index: usize) -> Option<Hash> {
    NONCE_BLOCKHASH_CACHE
        .get()
        .and_then(|cache| {
            cache.try_read().ok().map(|guard| guard[nonce_index])
        })
}

/// Get a cached nonce blockhash by pubkey (fast lookup)
pub fn get_cached_nonce_blockhash_by_pubkey(nonce_pubkey: &Pubkey) -> Option<Hash> {
    get_nonce_account_index(nonce_pubkey)
        .and_then(|index| get_cached_nonce_blockhash(index))
}

/// Update a nonce blockhash in the cache
pub async fn update_nonce_blockhash(nonce_pubkey: &Pubkey, new_blockhash: Hash) -> Result<(), Box<dyn Error>> {
    if let Some(index) = get_nonce_account_index(nonce_pubkey) {
        // Update the index-based cache
        if let Some(cache) = NONCE_BLOCKHASH_CACHE.get() {
            let mut guard = cache.write().await;
            guard[index] = new_blockhash;
        }
        
        // Update the global cache
        if let Some(global_cache) = GLOBAL_NONCE_BLOCKHASH_CACHE.get() {
            let mut guard = global_cache.write().await;
            guard.insert(*nonce_pubkey, (new_blockhash, std::time::Instant::now()));
        }
        
        println!("[WALLET_LOADER] Updated nonce blockhash for index {}: {}", index, new_blockhash);
    }
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

/// Start the background nonce blockhash prefetching task
pub fn start_nonce_blockhash_prefetching() -> Result<(), Box<dyn Error>> {
    let rpc_client = crate::init::initialize::GLOBAL_RPC_CLIENT
        .get()
        .ok_or("RPC client not initialized")?;
    
    let handle = tokio::spawn(async move {
        prefetch_all_nonce_blockhashes(rpc_client).await;
        keep_nonce_blockhashes_fresh(rpc_client).await;
    });
    
    NONCE_PREFETCH_HANDLE
        .set(handle)
        .map_err(|_| Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Nonce prefetch handle already set")))?;
    
    println!("[WALLET_LOADER] Started nonce blockhash prefetching task");
    Ok(())
}

/// Prefetch all nonce blockhashes on startup
async fn prefetch_all_nonce_blockhashes(rpc_client: &RpcClient) {
    if let Some(nonce_pubkeys) = GLOBAL_NONCE_PUBKEYS.get() {
        println!("[WALLET_LOADER] Background prefetching blockhashes for {} nonce accounts...", nonce_pubkeys.len());
        
        for (index, nonce_pubkey) in nonce_pubkeys.iter().enumerate() {
            match fetch_nonce_blockhash_sync(rpc_client, nonce_pubkey) {
                Ok(blockhash) => {
                    if let Err(e) = update_nonce_blockhash(nonce_pubkey, blockhash).await {
                        eprintln!("[WALLET_LOADER] Failed to update nonce blockhash for index {}: {}", index, e);
                    } else {
                        println!("[WALLET_LOADER] Background prefetched nonce {} blockhash: {}", index, blockhash);
                    }
                }
                Err(e) => {
                    eprintln!("[WALLET_LOADER] Failed to background prefetch nonce {} blockhash: {}", index, e);
                }
            }
        }
        
        println!("[WALLET_LOADER] Background nonce blockhash prefetching complete");
    }
}

/// Keep nonce blockhashes fresh in the background
async fn keep_nonce_blockhashes_fresh(rpc_client: &RpcClient) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30)); // Refresh every 30 seconds
    
    loop {
        interval.tick().await;
        
        if let Some(nonce_pubkeys) = GLOBAL_NONCE_PUBKEYS.get() {
            for nonce_pubkey in nonce_pubkeys.iter() {
                if let Ok(blockhash) = fetch_nonce_blockhash_sync(rpc_client, nonce_pubkey) {
                    if let Err(e) = update_nonce_blockhash(nonce_pubkey, blockhash).await {
                        eprintln!("[WALLET_LOADER] Failed to refresh nonce blockhash: {}", e);
                    }
                }
            }
        }
    }
}

/// Fetch a nonce blockhash from RPC (async version)
async fn fetch_nonce_blockhash(rpc_client: &RpcClient, nonce_account: &Pubkey) -> Result<Hash, Box<dyn Error + Send + Sync>> {
    // Get the nonce account data
    let account_data = rpc_client.get_account_data(nonce_account)?;
    
    // Parse the nonce account to get the current nonce
    let nonce_state: solana_sdk::nonce::state::Versions = bincode::deserialize(&account_data)?;
    
    // Get the nonce blockhash
    let nonce_blockhash = match nonce_state {
        solana_sdk::nonce::state::Versions::Current(boxed_state) => {
            match *boxed_state {
                solana_sdk::nonce::state::State::Initialized(ref data) => {
                    data.blockhash()
                }
                _ => {
                    return Err("Nonce account not initialized".into());
                }
            }
        }
        _ => {
            return Err("Unsupported nonce version".into());
        }
    };
    
    Ok(nonce_blockhash)
}

/// Fetch a nonce blockhash from RPC (synchronous version for startup)
pub fn fetch_nonce_blockhash_sync(rpc_client: &RpcClient, nonce_account: &Pubkey) -> Result<Hash, Box<dyn std::error::Error + Send + Sync>> {
    // Get the nonce account data
    let account_data = rpc_client.get_account_data(nonce_account)?;
    
    // Parse the nonce account to get the current nonce
    let nonce_state: solana_sdk::nonce::state::Versions = bincode::deserialize(&account_data)?;
    
    // Get the nonce blockhash
    let nonce_blockhash = match nonce_state {
        solana_sdk::nonce::state::Versions::Current(boxed_state) => {
            match *boxed_state {
                solana_sdk::nonce::state::State::Initialized(ref data) => {
                    data.blockhash()
                }
                _ => {
                    return Err("Nonce account not initialized".into());
                }
            }
        }
        _ => {
            return Err("Unsupported nonce version".into());
        }
    };
    
    Ok(nonce_blockhash)
}
