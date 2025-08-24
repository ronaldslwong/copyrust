use crate::build_tx::pump_swap::SwapDirection;
use crate::build_tx::pump_swap::build_pump_swap_instruction;
use crate::build_tx::pump_swap::PumpAmmAccounts;
use crate::utils::logger::{log_event, EventType};
use solana_program::instruction::{Instruction, AccountMeta};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Instant;
use crate::build_tx::utils::get_buy_swap_amount;
use crate::build_tx::pump_swap::get_instruction_accounts;

pub fn pump_swap_build_buy_tx(
    instruction_accounts: &[AccountMeta],
    sig_bytes_input: Option<Arc<Vec<u8>>>,
    detection_time: Instant,
    amount: u64,
    grpc_sol: u64,
    grpc_token: u64,
    slippage_basis_points: u64,
) -> (Instruction, Pubkey, u64, PumpAmmAccounts) {
    if let Some(ref sig_bytes) = sig_bytes_input {
        log_event(
            EventType::ArpcDetectionProcessing,
            sig_bytes.as_slice(),
            detection_time,
            None,
        );
    };

    let slippage_factor = 1.0 + slippage_basis_points as f64 / 10000.0;
    let pump_swap_accounts: crate::build_tx::pump_swap::PumpAmmAccounts = get_instruction_accounts(&instruction_accounts);
    // println!("pump_fun_accounts: {:?}", pump_swap_accounts);

    println!("buy amount: {:?}", amount);
    let expected_token_amount = get_buy_swap_amount(   
        grpc_sol,
        grpc_token,
        amount,
    ).unwrap_or(0);  // Extract the value once

    let buy_instruction = build_pump_swap_instruction(
        &pump_swap_accounts,
        SwapDirection::Buy,
        (amount as f64 * slippage_factor) as u64,  // SOL amount being spent
        expected_token_amount,  // Minimum token amount expected to receive
    );

    println!("[PUMPFUN] DEBUG - SOL amount: {}, Expected token amount: {}", amount, expected_token_amount);

    // Get mint from instruction account 2 (based on the debug output we saw)
    let mint = instruction_accounts[3].pubkey;

    (
        buy_instruction,
        mint,
        expected_token_amount,  // Store the expected token amount, not SOL limit
        pump_swap_accounts,
    )
}
