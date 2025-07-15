use solana_sdk::signature::Signer;
use solana_sdk::{pubkey::Pubkey, signature::Keypair};
use spl_associated_token_account::instruction::create_associated_token_account_idempotent;
use spl_token::solana_program::instruction::Instruction;

pub fn create_ata(payer: &Keypair, wallet_address: &Pubkey, mint: &Pubkey) -> Instruction {
    let program_id_str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    let program_id = program_id_str.parse::<Pubkey>().expect("Invalid pubkey");
    let associated_token_account_ix = create_associated_token_account_idempotent(
        &payer.pubkey(),
        &wallet_address,
        &mint,
        &program_id,
    );
    associated_token_account_ix
}
