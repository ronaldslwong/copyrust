use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use crate::init::wallet_loader::get_wallet_keypair;
use crate::build_tx::tx_builder::{build_and_sign_transaction, create_instruction};
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use crate::send_tx::rpc::send_tx_via_send_rpcs;
use chrono::Utc;

pub async fn send_rpc(cu_limit: u32, cu_price: u64, mint: Pubkey, instructions: Vec<Instruction>) -> Result<String, Box<dyn std::error::Error>> {
    let rpc: &solana_client::rpc_client::RpcClient = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    
    let compute_budget_instruction = create_instruction(
        cu_limit,
        cu_price,
        mint,
        instructions,
    );
    let tx = build_and_sign_transaction(
        rpc,
        &compute_budget_instruction,
        get_wallet_keypair(),
    )
    .ok();
    // println!("Signed tx, elapsed: {:.2?}", start_time.elapsed());
    let sig = send_tx_via_send_rpcs(&tx.unwrap()).await.unwrap();
    let now = Utc::now();
    println!(
        "[{}] - sell tx sent with sig: {}",
        now.format("%Y-%m-%d %H:%M:%S%.3f"),
        sig
    );
    Ok(sig)
}

