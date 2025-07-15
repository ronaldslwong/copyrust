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
use crate::init::wallet_loader::get_wallet_keypair;
use crate::utils::ata::create_ata;
use solana_sdk::signature::Signer;
use solana_client::rpc_config::RpcSendTransactionConfig;


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

/// Periodically call get_health on all send RPC clients every 30 seconds to keep connections warm
pub async fn keep_send_rpc_connections_warm() {
    let clients = GLOBAL_SEND_RPC_CLIENTS
        .get()
        .expect("Send RPC clients not initialized")
        .clone();
    let mut interval = time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        let clients_guard = clients.read().await;
        for (i, client) in clients_guard.iter().enumerate() {
            let client = client.clone();
            tokio::spawn(async move {
                match client.get_health() {
                    Ok(health) => {
                        // if health != "ok" {
                        //     eprintln!("[SendRPC {}] Health not ok: {}", i, health);
                        // }
                    }
                    Err(e) => {
                        // eprintln!("[SendRPC {}] Health check failed: {}", i, e);
                    }
                }
            });
        }
    }
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
) -> Vec<Instruction> {
    let limit_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
    let price_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(cu_price);

    let keypair = get_wallet_keypair();

    let ata_ix = create_ata(&keypair, &keypair.pubkey(), &mint);

    let mut result = vec![limit_ix, price_ix, ata_ix];
    result.extend(instructions);
    result
}
