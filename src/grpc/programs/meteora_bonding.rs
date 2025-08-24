use crate::build_tx::meteora_bonding::{
    get_meteora_bonding_instruction_accounts,
    build_meteora_dbc_swap_instruction,
    get_pool_state,
    MeteoraBondingSwapAccounts,
    get_swap_amount_from_quote_to_base,
    create_basic_pool_config,
    get_meteora_pool_config,
};
use crate::grpc::utils::parse_tx;
use crate::utils::logger::{log_event, EventType};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use std::time::Instant;
use solana_program::instruction::Instruction;
use crate::build_tx::utils::SwapDirection;
use std::error::Error;

// Custom error type for meteora bonding operations
#[derive(Debug)]
pub enum MeteoraBondingError {
    PoolStateNotFound(String),
    PoolConfigNotFound(String),
    SwapCalculationFailed(String),
    ParseError(String),
    Other(Box<dyn Error>),
}

impl std::fmt::Display for MeteoraBondingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeteoraBondingError::PoolStateNotFound(msg) => write!(f, "Pool state not found: {}", msg),
            MeteoraBondingError::PoolConfigNotFound(msg) => write!(f, "Pool config not found: {}", msg),
            MeteoraBondingError::SwapCalculationFailed(msg) => write!(f, "Swap calculation failed: {}", msg),
            MeteoraBondingError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            MeteoraBondingError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for MeteoraBondingError {}

impl From<Box<dyn Error>> for MeteoraBondingError {
    fn from(err: Box<dyn Error>) -> Self {
        MeteoraBondingError::Other(err)
    }
}

pub fn meteora_bonding_build_buy_tx(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
    sig_bytes_input: Option<Arc<Vec<u8>>>,
    detection_time: Instant,
    data: &[u8],
    amount: u64,
    slippage_basis_points: u64,
) -> Result<(Instruction, Pubkey, u64, MeteoraBondingSwapAccounts), MeteoraBondingError> {
    let overall_start = Instant::now();
    
    // Step 1: Parse transaction data
    let parse_start = Instant::now();
    let (mint, _, u2) = parse_tx(&account_keys, &accounts, 7, 16, 8, data);
    let parse_duration = parse_start.elapsed();
    let slippage_factor = 1.0 - slippage_basis_points as f64 / 10000.0;
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
    let meteora_bonding_accounts = get_meteora_bonding_instruction_accounts(&account_keys, &accounts);
    let accounts_duration = accounts_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILING] Get instruction accounts: {:?}", accounts_duration);
    
    // Step 4: Get pool state with proper error handling
    let pool_state_start = Instant::now();
    println!("meteora_bonding_accounts: {:?}", meteora_bonding_accounts.pool);
    
    let pool_state = match get_pool_state(&meteora_bonding_accounts) {
        Ok(state) => state,
        Err(e) => {
            let error_msg = format!("Failed to get pool state for pool {}: {}", meteora_bonding_accounts.pool, e);
            eprintln!("[ERROR] {}", error_msg);
            return Err(MeteoraBondingError::PoolStateNotFound(error_msg));
        }
    };
    
    println!("pool accounts: {:?}, base_reserve: {:?}, quote_reserve: {:?}", 
        meteora_bonding_accounts.pool, pool_state.base_reserve, pool_state.quote_reserve);
    let pool_state_duration = pool_state_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILING] Get pool state: {:?}", pool_state_duration);

    // Step 5: Calculate first swap amount using Meteora DBC pricing
    let swap1_start = Instant::now();
    
    // Get the actual pool config from the config account instead of using backup function
    let pool_config = match get_meteora_pool_config(&meteora_bonding_accounts) {
        Ok(config) => {
            println!("[DEBUG] Successfully fetched pool config from config account {:?}", meteora_bonding_accounts.config);
            println!("[DEBUG] - sqrt_start_price: {}", config.sqrt_start_price);
            println!("[DEBUG] - curve points count: {}", config.curve.len());
            println!("[DEBUG] - migration_quote_threshold: {}", config.migration_quote_threshold);
            
            // Print first few curve points for debugging
            for (i, point) in config.curve.iter().take(5).enumerate() {
                if point.sqrt_price > 0 || point.liquidity > 0 {
                    println!("[DEBUG] - Curve point {}: sqrt_price={}, liquidity={}", 
                        i, point.sqrt_price, point.liquidity);
                }
            }
            config
        },
        Err(e) => {
            eprintln!("[ERROR] Failed to fetch pool config from config account: {:?}", e);
            eprintln!("[ERROR] Falling back to backup pool config...");
            create_basic_pool_config(&pool_state)
        }
    };
    
    let swap_result = match get_swap_amount_from_quote_to_base(
        &pool_state,
        &pool_config,
        amount,
    ) {
        Ok(result) => result,
        Err(e) => {
            let error_msg = format!("Failed to calculate swap amount: {:?}. Current price: {}, Amount: {}, Base reserve: {}, Quote reserve: {}", 
                e, pool_state.sqrt_price, amount, pool_state.base_reserve, pool_state.quote_reserve);
            eprintln!("[ERROR] {}", error_msg);
            return Err(MeteoraBondingError::SwapCalculationFailed(error_msg));
        }
    };
    
    let target_token_buy = swap_result.output_amount;
    let swap1_duration = swap1_start.elapsed();
    #[cfg(feature = "verbose_logging")]
    println!("[PROFILING] Calculate first swap amount (u1): {:?}", swap1_duration);

    // Step 6: Build buy instruction using Meteora DBC
    let build_instruction_start = Instant::now();
    println!("slippage_factor: {:?}, target_token_buy: {:?}", slippage_factor, (target_token_buy as f64 * slippage_factor) as u64);
    let buy_instruction = build_meteora_dbc_swap_instruction(
        &meteora_bonding_accounts,
        SwapDirection::Buy,
        amount,
        (target_token_buy as f64 * slippage_factor) as u64,
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
        println!("  - Build instruction: {:.2}%", (build_instruction_duration.as_nanos() as f64 / overall_duration.as_nanos() as f64) * 100.0);
    }
    
    Ok((buy_instruction, mint, target_token_buy, meteora_bonding_accounts))
}

pub fn meteora_bonding_build_sell_tx(
    token_amount: u64,
    meteora_bonding_accounts: &MeteoraBondingSwapAccounts,
) -> Result<Instruction, MeteoraBondingError> {
    let sell_instruction = build_meteora_dbc_swap_instruction(
        meteora_bonding_accounts,
        SwapDirection::Sell,
        token_amount,
        0,
    );
    Ok(sell_instruction)
}