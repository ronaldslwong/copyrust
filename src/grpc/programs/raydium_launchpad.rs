use crate::build_tx::ray_launch::build_ray_launch_buy_instruction_no_quote;
use crate::build_tx::ray_launch::get_instruction_accounts;
use crate::grpc::utils::parse_tx;
use crate::utils::logger::{log_event, EventType};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Instant;
use solana_program::instruction::Instruction;
use crate::build_tx::ray_launch::RayLaunchAccounts;

pub fn raydium_launchpad_build_buy_tx(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
    sig_bytes_input: Option<Arc<Vec<u8>>>,
    detection_time: Instant,
    data: &[u8],
    amount: u64,
    slippage_basis_points: u64,
) -> (Instruction, Pubkey, u64, RayLaunchAccounts) {
    let (mint, u1, u2) = parse_tx(&account_keys, &accounts, 9, 16, 8, data);

    if let Some(ref sig_bytes) = sig_bytes_input {
        log_event(
            EventType::ArpcDetectionProcessing,
            sig_bytes.as_slice(),
            detection_time,
            None,
        );
    };

    // println!("mint: {:?}, u1: {:?}, u2: {:?}", mint, u1, u2);
    let ray_launch_accounts = get_instruction_accounts(&account_keys, &accounts);

    let target_token_buy = (amount as f64 / u2 as f64 * u1 as f64) as u64;

    // println!("target_token_buy: {:?}", target_token_buy);

    let buy_instruction = build_ray_launch_buy_instruction_no_quote(
        amount,
        target_token_buy,
        slippage_basis_points,
        &ray_launch_accounts,
    );
    (buy_instruction, mint, target_token_buy, ray_launch_accounts)
}
