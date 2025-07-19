use crate::build_tx::pump_fun::get_instruction_accounts as get_pump_fun_instruction_accounts;
use crate::build_tx::pump_fun::PumpFunAccounts;
use crate::build_tx::pump_fun::calculate_pump_fun_swap_amount;
use crate::build_tx::pump_fun::build_pump_fun_instruction;
use crate::build_tx::pump_fun::get_creator_fee_vault;
use crate::build_tx::pump_fun::get_bonding_curve_state;
use crate::build_tx::pump_swap::build_pump_swap_instruction;
use crate::build_tx::pump_swap::get_instruction_accounts as get_pump_swap_instruction_accounts;
use crate::build_tx::pump_swap::get_pump_swap_amount;
use crate::build_tx::pump_swap::PumpAmmAccounts;
use crate::build_tx::pump_swap::SwapDirection;
use crate::build_tx::utils::get_account;
use crate::utils::logger::{log_event, EventType};
use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Instant;

pub fn axiom_pump_swap_build_buy_tx(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
    sig_bytes_input: Option<Arc<Vec<u8>>>,
    detection_time: Instant,
    amount: u64,
    slippage_basis_points: u64,
) -> (Instruction, Pubkey, u64, PumpAmmAccounts) {
    // let (mint, _, _) = parse_tx(&account_keys, &accounts, 3, 9, 1, data);

    if let Some(ref sig_bytes) = sig_bytes_input {
        log_event(
            EventType::ArpcDetectionProcessing,
            sig_bytes.as_slice(),
            detection_time,
            None,
        );
    };

    // println!("mint: {:?}, u1: {:?}, u2: {:?}", mint, u1, u2);
    let slippage_factor = 1.0 + slippage_basis_points as f64 / 10000.0;
    let pump_swap_accounts = get_pump_swap_instruction_accounts(&account_keys, &accounts);

    // println!("base_vault: {}", base_vault);
    // println!("quote_vault: {}", quote_vault);
    let limit_quote_amount = get_pump_swap_amount(
        SwapDirection::Buy,
        pump_swap_accounts.pool_base_token_account,
        pump_swap_accounts.pool_quote_token_account,
        amount,
        0,
        0,
    )
    .expect("Failed to calculate buy limit_quote_amount");

    // println!("target_token_buy: {:?}", target_token_buy);

    let buy_instruction = build_pump_swap_instruction(
        &pump_swap_accounts,
        SwapDirection::Buy,
        (amount as f64 * slippage_factor) as u64,
        limit_quote_amount,
    );

    (
        buy_instruction,
        get_account(account_keys, accounts, 3),
        limit_quote_amount,
        pump_swap_accounts,
    )
}

pub fn axiom_pump_fun_build_buy_tx(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
    sig_bytes_input: Option<Arc<Vec<u8>>>,
    detection_time: Instant,
    amount: u64,
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
    let mut pump_fun_accounts = get_pump_fun_instruction_accounts(&account_keys, &accounts);

    let bonding_curve_state = get_bonding_curve_state(&pump_fun_accounts);

    let (limit_quote_amount, creator) = calculate_pump_fun_swap_amount(
        SwapDirection::Buy,
        bonding_curve_state,
        amount,
        0,
    );

    pump_fun_accounts.creator_fee_vault = get_creator_fee_vault(&creator);

    let buy_instruction = build_pump_fun_instruction(
        &pump_fun_accounts,
        SwapDirection::Buy,
        (amount as f64 * slippage_factor) as u64,
        limit_quote_amount,
    );

    (
        buy_instruction,
        pump_fun_accounts.mint,
        limit_quote_amount,
        pump_fun_accounts,
    )
}
