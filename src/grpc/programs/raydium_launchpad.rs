use crate::build_tx::ray_launch::build_ray_launch_buy_instruction_no_quote;
use crate::build_tx::ray_launch::get_instruction_accounts;
use crate::grpc::utils::parse_tx;
use crate::utils::logger::{log_event, EventType};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Instant;
use solana_program::instruction::Instruction;
use crate::build_tx::ray_launch::RayLaunchAccounts;
use crate::build_tx::utils::SwapDirection;
use crate::build_tx::ray_launch::get_ray_launch_swap_amount;
use crate::build_tx::ray_launch::get_pool_state;

pub fn raydium_launchpad_build_buy_tx(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
    sig_bytes_input: Option<Arc<Vec<u8>>>,
    detection_time: Instant,
    data: &[u8],
    amount: u64,
    slippage_basis_points: u64,
) -> (Instruction, Pubkey, u64, RayLaunchAccounts) {
    let overall_start = Instant::now();
    
    // Step 1: Parse transaction data
    let parse_start = Instant::now();
    let (mint, _, u2) = parse_tx(&account_keys, &accounts, 9, 16, 8, data);
    let parse_duration = parse_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILING] Parse transaction data: {:?}", parse_duration);

    // Step 2: Log event (if sig_bytes provided)
    let log_start = Instant::now();
    if let Some(ref sig_bytes) = sig_bytes_input {
        log_event(
            EventType::ArpcDetectionProcessing,
            sig_bytes.as_slice(),
            detection_time,
            None,
        );
    };
    let log_duration = log_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILING] Log event: {:?}", log_duration);

    #[cfg(feature = "verbose_logging")]
    println!("mint: {:?}, u2: {:?}", mint, u2);
    
    // Step 3: Get instruction accounts
    let accounts_start = Instant::now();
    let ray_launch_accounts = get_instruction_accounts(&account_keys, &accounts);
    let accounts_duration = accounts_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILING] Get instruction accounts: {:?}", accounts_duration);
    
    // Step 4: Get pool state
    let pool_state_start = Instant::now();
    let pool_state = get_pool_state(&ray_launch_accounts);
    let pool_state_duration = pool_state_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILING] Get pool state: {:?}", pool_state_duration);

    // Step 5: Calculate first swap amount
    let swap1_start = Instant::now();
    let u1 = get_ray_launch_swap_amount(
        SwapDirection::Buy,
        &pool_state,
        amount,
        0,
        0,
    ).expect("Failed to calculate buy limit_quote_amount");
    let swap1_duration = swap1_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILING] Calculate first swap amount (u1): {:?}", swap1_duration);

    // Step 6: Calculate target token buy amount
    let swap2_start = Instant::now();
    let target_token_buy = get_ray_launch_swap_amount(
        SwapDirection::Buy,
        &pool_state,
        amount,
        u2,
        u1,
    ).expect("Failed to calculate buy limit_quote_amount");
    let swap2_duration = swap2_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILING] Calculate target token buy amount: {:?}", swap2_duration);

    // let target_token_buy = (amount as f64 / u2 as f64 * u1 as f64) as u64;

    // println!("target_token_buy: {:?}", target_token_buy);

    // Step 7: Build buy instruction
    let build_instruction_start = Instant::now();
    let buy_instruction = build_ray_launch_buy_instruction_no_quote(
        amount,
        target_token_buy,
        slippage_basis_points,
        &ray_launch_accounts,
    );
    let build_instruction_duration = build_instruction_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILING] Build buy instruction: {:?}", build_instruction_duration);
    
    // Overall timing
    let overall_duration = overall_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    {
        println!("[PROFILING] Total function execution time: {:?}", overall_duration);
        println!("[PROFILING] Breakdown:");
        println!("  - Parse transaction: {:.2}%", (parse_duration.as_nanos() as f64 / overall_duration.as_nanos() as f64) * 100.0);
        println!("  - Log event: {:.2}%", (log_duration.as_nanos() as f64 / overall_duration.as_nanos() as f64) * 100.0);
        println!("  - Get accounts: {:.2}%", (accounts_duration.as_nanos() as f64 / overall_duration.as_nanos() as f64) * 100.0);
        println!("  - Get pool state: {:.2}%", (pool_state_duration.as_nanos() as f64 / overall_duration.as_nanos() as f64) * 100.0);
        println!("  - First swap calc: {:.2}%", (swap1_duration.as_nanos() as f64 / overall_duration.as_nanos() as f64) * 100.0);
        println!("  - Second swap calc: {:.2}%", (swap2_duration.as_nanos() as f64 / overall_duration.as_nanos() as f64) * 100.0);
        println!("  - Build instruction: {:.2}%", (build_instruction_duration.as_nanos() as f64 / overall_duration.as_nanos() as f64) * 100.0);
    }
    
    (buy_instruction, mint, target_token_buy, ray_launch_accounts)
}
