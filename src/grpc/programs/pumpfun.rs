use crate::build_tx::pump_fun::get_instruction_accounts as get_pump_fun_instruction_accounts;
use crate::build_tx::pump_fun::PumpFunAccounts;
use crate::build_tx::pump_fun::build_pump_fun_instruction;
use crate::build_tx::pump_swap::SwapDirection;
use crate::utils::logger::{log_event, EventType};
use solana_program::instruction::{Instruction, AccountMeta};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Instant;
use crate::build_tx::utils::get_buy_swap_amount;

pub fn pump_fun_build_buy_tx(
    instruction_accounts: &[AccountMeta],
    sig_bytes_input: Option<Arc<Vec<u8>>>,
    detection_time: Instant,
    amount: u64,
    grpc_sol: u64,
    grpc_token: u64,
    slippage_basis_points: u64,
) -> (Instruction, Pubkey, u64, PumpFunAccounts) {
    if let Some(ref sig_bytes) = sig_bytes_input {
        log_event(
            EventType::ArpcDetectionProcessing,
            sig_bytes.as_slice(),
            detection_time,
            None,
        );
    };

    let slippage_factor = 1.0 + slippage_basis_points as f64 / 10000.0;
    let pump_fun_accounts = get_pump_fun_instruction_accounts(&instruction_accounts);
    // println!("pump_fun_accounts: {:?}", pump_fun_accounts);

    println!("buy amount: {:?}", amount);
    let expected_token_amount = get_buy_swap_amount(   
        grpc_sol,
        grpc_token,
        amount,
    ).unwrap_or(0);  // Extract the value once

    let buy_instruction = build_pump_fun_instruction(
        &pump_fun_accounts,
        SwapDirection::Buy,
        (amount as f64 * slippage_factor) as u64,  // SOL amount being spent
        expected_token_amount,  // Minimum token amount expected to receive
    );

    println!("[PUMPFUN] DEBUG - SOL amount: {}, Expected token amount: {}", amount, expected_token_amount);

    // Get mint from instruction account 2 (based on the debug output we saw)
    let mint = instruction_accounts[2].pubkey;

    (
        buy_instruction,
        mint,
        expected_token_amount,  // Store the expected token amount, not SOL limit
        pump_fun_accounts,
    )
}
