use once_cell::sync::OnceCell;
use solana_client::rpc_client::RpcClient;
use solana_sdk::transaction::Transaction;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time;
use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::compute_budget;
use crate::config_load::{Config};
use crate::init::wallet_loader::{get_wallet_keypair, get_nonce_account};
use crate::utils::ata::create_ata;
use solana_sdk::signature::Signer;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_sdk::hash::Hash;
use solana_program::system_instruction;


// Global slice of RPC clients
pub static GLOBAL_SEND_RPC_CLIENTS: OnceCell<Arc<RwLock<Vec<Arc<RpcClient>>>>> = OnceCell::new();

/// Initialize the global RPC clients from config.send_rpc
pub fn initialize_send_rpc_clients(config: &Config) {
    let clients: Vec<Arc<RpcClient>> = config
        .send_rpc
        .iter()
        .map(|url| Arc::new(RpcClient::new(url.clone())))
        .collect();
    let _ = GLOBAL_SEND_RPC_CLIENTS.set(Arc::new(RwLock::new(clients)));
}

pub static GLOBAL_LATEST_BLOCKHASH: once_cell::sync::OnceCell<RwLock<Hash>> = once_cell::sync::OnceCell::new();

// Initialize the global blockhash at file load time
#[allow(dead_code)]
fn _init_blockhash_once() {
    let _ = GLOBAL_LATEST_BLOCKHASH.set(RwLock::new(Hash::default()));
}

/// Periodically fetch and cache the latest blockhash from the first send RPC client
pub async fn keep_blockhash_fresh() {
    let clients = GLOBAL_SEND_RPC_CLIENTS
        .get()
        .expect("Send RPC clients not initialized")
        .clone();
    let mut interval = time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        let clients_guard = clients.read().await;
        if let Some(client) = clients_guard.get(0) {
            match client.get_latest_blockhash() {
                Ok(blockhash) => {
                    if let Some(lock) = GLOBAL_LATEST_BLOCKHASH.get() {
                        let mut hash_guard = lock.write().await;
                        *hash_guard = blockhash;
                    }
                }
                Err(e) => {
                    eprintln!("[Blockhash] Failed to fetch latest blockhash: {}", e);
                }
            }
        }
    }
}

/// Get the current cached blockhash (clone)
pub async fn get_cached_blockhash() -> Hash {
    GLOBAL_LATEST_BLOCKHASH
        .get()
        .expect("Blockhash not initialized")
        .read()
        .await
        .clone()
}

/// Send a transaction by looping through the list of send RPCs and sending via RPC call
pub async fn send_tx_via_send_rpcs(tx: &Transaction) -> Result<String, String> {
    let clients = GLOBAL_SEND_RPC_CLIENTS
        .get()
        .expect("Send RPC clients not initialized")
        .clone();
    let clients_guard = clients.read().await;
    for (i, client) in clients_guard.iter().enumerate() {
        match client.send_transaction_with_config(
            tx,
            RpcSendTransactionConfig {
                skip_preflight: true,
                ..RpcSendTransactionConfig::default()
            },
        ) {
            Ok(sig) => {
                println!("[SendRPC {}] Sent transaction: {}", i, sig);
                return Ok(sig.to_string());
            }
            Err(e) => {
                eprintln!("[SendRPC {}] Failed to send transaction: {}", i, e);
            }
        }
    }
    Err("All send RPCs failed to send transaction".to_string())
} 

pub fn create_instruction_rpc(
    cu_limit: u32,
    cu_price: u64,
    mint: Pubkey,
    instructions: Vec<Instruction>,
    tip_amount: u64,
    nonce_account: &Pubkey,
) -> Vec<Instruction> {
    // let limit_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
    let cu_price_tip = ((tip_amount / cu_limit as u64) as f64 * 1_000_000.0) as u64;
    let price_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(cu_price_tip);

    let keypair = get_wallet_keypair();

    let ata_ix = create_ata(&keypair, &keypair.pubkey(), &mint);

    let advance_nonce_ix = system_instruction::advance_nonce_account(
        nonce_account,
        &keypair.pubkey(),
    );
    let mut result = vec![advance_nonce_ix, price_ix];
    result.extend(instructions);
    result
}
