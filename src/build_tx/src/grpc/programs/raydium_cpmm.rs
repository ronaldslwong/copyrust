use crate::build_tx::ray_cpmm::build_ray_cpmm_swap_instruction;
use crate::build_tx::ray_cpmm::get_instruction_accounts;
use crate::build_tx::ray_cpmm::get_pool_state;
use crate::build_tx::ray_cpmm::RayCpmmSwapAccounts;
use crate::build_tx::ray_launch::build_ray_launch_buy_instruction_no_quote;
use crate::build_tx::utils::get_constant_product_swap_amount;
use crate::build_tx::utils::get_pool_vault_amount;
use crate::build_tx::utils::SwapDirection;
use crate::constants::consts::WSOL;
use crate::grpc::utils::parse_tx;
use crate::utils::logger::{log_event, EventType};
use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Instant;
use crate::build_tx::utils::get_account;

pub fn raydium_cpmm_build_buy_tx(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
    sig_bytes_input: Option<Arc<Vec<u8>>>,
    detection_time: Instant,
    amount: u64,
    slippage_basis_points: u64,
) -> (Instruction, Pubkey, u64, RayCpmmSwapAccounts) {
    // let (mint, u1, u2) = parse_tx(&account_keys, &accounts, 9, 16, 8, data);

    if let Some(ref sig_bytes) = sig_bytes_input {
        log_event(
            EventType::ArpcDetectionProcessing,
            sig_bytes.as_slice(),
            detection_time,
            None,
        );
    };
    let slippage_factor = 1.0 + slippage_basis_points as f64 / 10000.0;

    // println!("mint: {:?}, u1: {:?}, u2: {:?}", mint, u1, u2);
    let tx_mint = get_account(&account_keys, &accounts, 11);
    println!("!!!tx_mint: {:?}" , tx_mint );
    if tx_mint == WSOL {
        return (
            Instruction {
                program_id: Pubkey::new_unique(),
                accounts: vec![],
                data: vec![],
            },
            Pubkey::default(),
            0,
            RayCpmmSwapAccounts::new(),
        );
    } else {
        let ray_cpmm_accounts = get_instruction_accounts(&account_keys, &accounts);
        let pool_state = get_pool_state(&ray_cpmm_accounts);
        let (base_amount, quote_amount) =
            get_pool_vault_amount(pool_state.token_1_vault, pool_state.token_0_vault).unwrap();

        let target_token_buy = get_constant_product_swap_amount(
            SwapDirection::Buy,
            base_amount,
            quote_amount,
            amount,
            0,
            0,
        )
        .expect("Failed to calculate buy limit_quote_amount");

        let buy_instruction = build_ray_cpmm_swap_instruction(
            &ray_cpmm_accounts,
            SwapDirection::Buy,
            (amount as f64 * slippage_factor) as u64,
            target_token_buy,
        );
        (
            buy_instruction,
            ray_cpmm_accounts.token_1_mint,
            target_token_buy,
            ray_cpmm_accounts,
        )
    }
}
