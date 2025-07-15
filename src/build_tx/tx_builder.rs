use solana_client::rpc_client::RpcClient;
use solana_sdk::instruction::Instruction;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use solana_sdk::compute_budget;
use solana_sdk::pubkey::Pubkey;
use crate::init::wallet_loader::get_wallet_keypair;
use crate::utils::ata::create_ata;

/// Build and sign a Solana transaction from an array of instructions, fetching the recent blockhash from the network.
///
/// # Arguments
/// * `rpc_client` - Reference to an RpcClient for fetching the recent blockhash
/// * `instructions` - A slice of Solana instructions to include in the transaction
/// * `signer` - The keypair to sign the transaction
///
/// # Returns
/// A Result containing the signed Transaction object, or an error
pub fn build_and_sign_transaction(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    // Fetch the recent blockhash from the network
    let recent_blockhash = rpc_client.get_latest_blockhash()?;

    // Build and sign the transaction
    let tx = Transaction::new_signed_with_payer(
        instructions,
        Some(&signer.pubkey()),
        &[signer],
        recent_blockhash,
    );
    Ok(tx)
}


pub fn create_instruction(
    cu_limit: u32,
    cu_price: u64,
    mint: Pubkey,
    instructions: Vec<Instruction>,
) -> Vec<Instruction> {
    let keypair: &'static Keypair = get_wallet_keypair();

    let limit_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
    let price_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(cu_price);

    let ata_ix = create_ata(&keypair, &keypair.pubkey(), &mint);

    let mut result = vec![limit_ix, price_ix, ata_ix];
    result.extend(instructions);
    result
}