use ed25519_dalek::{Keypair as DalekKeypair, Signer as DalekSigner, Signature as DalekSignature};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use solana_sdk::compute_budget;
use solana_sdk::pubkey::Pubkey;
use crate::init::wallet_loader::get_wallet_keypair;
use crate::utils::ata::create_ata;
use solana_program::instruction::Instruction;
use solana_sdk::signature::Signature;
use crate::send_tx::rpc::get_cached_blockhash;
/// Returns a default Instruction (empty program_id, empty accounts, empty data)
pub fn default_instruction() -> Instruction {
    Instruction {
        program_id: Pubkey::default(),
        accounts: vec![],
        data: vec![],
    }
}

/// Build and sign a Solana transaction from an array of instructions, fetching the recent blockhash from the network.
/// Uses ed25519-dalek for signing.
// pub fn build_and_sign_transaction(
//     rpc_client: &solana_client::rpc_client::RpcClient,
//     instructions: &[Instruction],
//     signer: &Keypair,
// ) -> Result<Transaction, Box<dyn std::error::Error>> {
//     // Fetch the recent blockhash from the network
//     let recent_blockhash = rpc_client.get_latest_blockhash()?;

//     // Build the message
//     let message = solana_sdk::message::Message::new(instructions, Some(&signer.pubkey()));
//     let message_bytes = message.serialize();

//     // Convert solana_sdk::Keypair to ed25519_dalek::Keypair
//     let dalek_keypair = DalekKeypair::from_bytes(&signer.to_bytes()).expect("Keypair conversion failed");

//     // Sign the message with dalek
//     let dalek_signature: DalekSignature = dalek_keypair.sign(&message_bytes);
//     let solana_signature = solana_sdk::signature::Signature::from(dalek_signature.to_bytes());

//     // Build the transaction manually
//     let mut tx = Transaction::new_unsigned(message);
//     tx.signatures = vec![solana_signature];
//     Ok(tx)
// }

pub fn build_and_sign_transaction(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let recent_blockhash = rt.block_on(get_cached_blockhash());
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