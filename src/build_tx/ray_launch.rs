// ray_launch.rs
// Raydium Launchpad Curve: buyExactIn translation from TypeScript to Rust
// Reference: https://github.com/raydium-io/raydium-sdk-V2/blob/master/src/raydium/launchpad/curve/curve.ts

use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use std::error::Error;
use crate::build_tx::pump_swap::SwapDirection;
use crate::init::wallet_loader::get_wallet_keypair;
use crate::build_tx::pump_swap::get_account;
use solana_sdk::signature::Signer;
use solana_program::instruction::{AccountMeta, Instruction};
use num_bigint::BigUint;
use num_traits::cast::ToPrimitive;
use num_traits::Zero;

#[derive(Debug, Clone)]
pub struct RaydiumPoolRealReserves {
    pub total_sell_a: BigUint,
    pub total_fund_raising_b: BigUint,
    pub real_a: u64,
    pub real_b: u64,
    pub virtual_a: u64,
    pub virtual_b: u64,
}

impl RaydiumPoolRealReserves {
    /// Deserialize from a byte slice, given the offsets for real_base and real_quote.
    pub fn from_account_data(data: &[u8], real_base_offset: usize, real_quote_offset: usize, fundraise_b_offset: usize, total_sell_b_offset: usize, virtual_base_offset: usize, virtual_quote_offset: usize) -> Option<Self> {
        if data.len() < real_quote_offset + 8 {
            return None;
        }
        let real_a = u64::from_le_bytes(data[real_base_offset..real_base_offset+8].try_into().ok()?);
        let real_b = u64::from_le_bytes(data[real_quote_offset..real_quote_offset+8].try_into().ok()?);
        let total_fund_raising_b = u64::from_le_bytes(data[fundraise_b_offset..fundraise_b_offset+8].try_into().ok()?);
        let total_sell_a = u64::from_le_bytes(data[total_sell_b_offset..total_sell_b_offset+8].try_into().ok()?);
        let virtual_a = u64::from_le_bytes(data[virtual_base_offset..virtual_base_offset+8].try_into().ok()?);
        let virtual_b = u64::from_le_bytes(data[virtual_quote_offset..virtual_quote_offset+8].try_into().ok()?);

        Some(Self {
            total_sell_a: total_sell_a.into(),
            total_fund_raising_b: total_fund_raising_b.into(),
            real_a: real_a,
            real_b: real_b,
            virtual_a: virtual_a,
            virtual_b: virtual_b,
        })
    }
}
pub mod ray_launch_constants {
    use solana_program::pubkey;
    use solana_program::pubkey::Pubkey;

    pub const RAY_LAUNCH_PROGRAM_ID: Pubkey = pubkey!("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj");
    pub const RAY_LAUNCH_AUTHORITY: Pubkey = pubkey!("WLHv2UAZm6z4KyaaELi5pjdbJh6RESMva1Rnn8pJVVh");
    // Add other relevant constants as needed
    pub const RAY_LAUNCH_GLOBAL_CONFIG: Pubkey = pubkey!("6s1xP3hpbAfFoNtUNF8mfHsjr2Bd97JxFJRWLbL6aHuX");
    pub const RAY_LAUNCH_PROGRAM_CONFIG: Pubkey = pubkey!("FfYek5vEz23cMkWsdJwG2oa6EphsvXSHrGpdALN4g6W1");
    
    pub const RAY_LAUNCH_EVENT_AUTHORITY: Pubkey = pubkey!("2DPAtwB8L12vrMRExbLuyGnC7n2J5LNoZQSejeQGpwkr");
    pub const WSOL: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
}

#[derive(PartialEq, Copy, Clone, Debug)]

pub struct RayLaunchAccounts {
    /// #1 - Payer (WRITABLE, SIGNER, FEE PAYER)
    pub payer: Pubkey,
    /// #2 - Authority (Raydium Launchpad Authority)
    pub authority: Pubkey,
    /// #3 - Global Config
    pub global_config: Pubkey,
    /// #4 - Platform Config
    pub platform_config: Pubkey,
    /// #5 - Pool State (WRITABLE)
    pub pool_state: Pubkey,
    /// #6 - User Base Token (WRITABLE)
    pub user_base_token: Pubkey,
    /// #7 - User Quote Token (WRITABLE)
    pub user_quote_token: Pubkey,
    /// #8 - Base Vault (WRITABLE)
    pub base_vault: Pubkey,
    /// #9 - Quote Vault (WRITABLE)
    pub quote_vault: Pubkey,
    /// #10 - Base Token Mint
    pub base_token_mint: Pubkey,
    /// #11 - Quote Token Mint
    pub quote_token_mint: Pubkey,
    /// #12 - Base Token Program (PROGRAM)
    pub base_token_program: Pubkey,
    /// #13 - Quote Token Program (PROGRAM)
    pub quote_token_program: Pubkey,
    /// #14 - Event Authority
    pub event_authority: Pubkey,
    /// #15 - Program (Raydium Launchpad PROGRAM)
    pub program: Pubkey,
}


pub struct PoolBaseAmount {
    pub total_sell_a: BigUint,
    pub total_fund_raising_b: BigUint,
    pub real_a: BigUint,
    pub real_b: BigUint,
}


/// Helper to get the discriminator for buy/sell
fn get_discriminator(direction: SwapDirection) -> [u8; 8] {
    match direction {
        SwapDirection::Buy => [24, 211, 116, 40, 105, 3, 153, 56],
        SwapDirection::Sell => [149, 39, 222, 155, 211, 124, 152, 26],
    }
}

pub fn get_ray_launch_swap_amount(
    direction: SwapDirection,
    pool_ac: Pubkey,
    swap_amount: u64,
    target_sol_buy: u64,
    target_token_buy: u64,
) -> Result<u64, Box<dyn Error>> {
    let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

    let res: solana_client::rpc_response::Response<Vec<Option<solana_sdk::account::Account>>> = rpc_client.get_multiple_accounts_with_commitment(&[pool_ac], CommitmentConfig::processed())?;
    if res.value.is_empty() || res.value[0].is_none() {
        return Err("missing pool account data".into());
    }
    let account_opt = res.value.get(0).and_then(|opt| opt.as_ref());
    let data = account_opt.map(|acct| acct.data.as_slice()).ok_or("missing pool account data")?;
    
    let pool_data = RaydiumPoolRealReserves::from_account_data(data, 53, 61, 0, 0, 37, 45)
        .expect("Failed to parse pool reserves");
    
    if pool_data.real_a == 0 {
        return Err("zero base amount".into());
    }
    println!("pool_data.real_base: {}", pool_data.real_a);
    println!("pool_data.real_quote: {}", pool_data.real_b);
    let adjusted_price = match direction {
        SwapDirection::Buy => ((pool_data.virtual_a - pool_data.real_a-target_token_buy) as f64 * swap_amount as f64) / ((pool_data.virtual_b + pool_data.real_b + target_sol_buy) as f64 + swap_amount as f64) ,
        SwapDirection::Sell => ((pool_data.virtual_b + pool_data.real_b-target_sol_buy) as f64 * swap_amount as f64) / ((pool_data.virtual_a - pool_data.real_a + target_token_buy) as f64 + swap_amount as f64) ,
    };
    Ok(adjusted_price as u64)
}

pub fn build_ray_launch_buy_instruction(
    // base_vault: Pubkey,
    // quote_vault: Pubkey,
    amount: u64,
    slippage_basis_points: u64,
    accounts: RayLaunchAccounts,
    target_sol_buy: u64,
    target_token_buy: u64,
) -> (Instruction, u64) {


    let slippage_factor = 1.0+slippage_basis_points as f64 /10000.0;

    // println!("base_vault: {}", base_vault);
    // println!("quote_vault: {}", quote_vault);
    println!("pool_state: {}", accounts.pool_state.to_string());
    let limit_quote_amount = match get_ray_launch_swap_amount(
        SwapDirection::Buy,
        accounts.pool_state,
        amount,
        target_sol_buy,
        target_token_buy,
    ) {
        Ok(val) => val,
        Err(e) => {
            eprintln!("Could not calculate buy limit_quote_amount: {}", e);
            // handle gracefully, e.g. return, skip, or set a default value
            return (Instruction::new_with_bincode(ray_launch_constants::RAY_LAUNCH_PROGRAM_ID, &[0u8; 32], vec![]), 0);
        }
    };
    println!("amount: {}, limit_quote_amount: {}, limit_quote_amount_slippage: {}", amount, limit_quote_amount, (limit_quote_amount as f64*slippage_factor) as u64);

    let instruction = build_ray_launch_swap_instruction(accounts, SwapDirection::Buy, limit_quote_amount, (amount as f64*slippage_factor) as u64);
    (instruction, limit_quote_amount)
}

pub fn build_ray_launch_sell_instruction(
    // base_vault: Pubkey,
    // quote_vault: Pubkey,
    amount: u64,
    slippage_basis_points: u64,
    accounts: RayLaunchAccounts,
) -> (Instruction) {


    let slippage_factor = 1.0-slippage_basis_points as f64 /10000.0;

    // println!("base_vault: {}", base_vault);
    // println!("quote_vault: {}", quote_vault);
    println!("pool_state: {}", accounts.pool_state.to_string());
    let limit_quote_amount = get_ray_launch_swap_amount(
        SwapDirection::Sell,
        accounts.pool_state,
        amount,
        0,
        0,
    ).expect("Failed to calculate buy limit_quote_amount");
    println!("amount: {}, limit_quote_amount: {}, limit_quote_amount_slippage: {}", amount, limit_quote_amount, (limit_quote_amount as f64*slippage_factor) as u64);
    let instruction = build_ray_launch_swap_instruction(accounts, SwapDirection::Sell, amount, (limit_quote_amount as f64*slippage_factor) as u64);
    (instruction)
}

/// Build a PumpSwap instruction (buy or sell)
pub fn build_ray_launch_swap_instruction(
    accounts: RayLaunchAccounts,
    direction: SwapDirection,
    limit_quote_amount: u64,
    amount: u64,
) -> Instruction {
    let discriminator = get_discriminator(direction);


    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&limit_quote_amount.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    let mut full_data = [discriminator.as_ref(), data.as_slice()].concat();
    full_data.extend_from_slice(&[0u8; 16]);

    let metas = vec![
        AccountMeta::new(accounts.payer, true),
        AccountMeta::new_readonly(accounts.authority, false),
        AccountMeta::new_readonly(accounts.global_config, false),
        AccountMeta::new_readonly(accounts.platform_config, false),
        AccountMeta::new(accounts.pool_state, false),
        AccountMeta::new(accounts.user_base_token, false),
        AccountMeta::new(accounts.user_quote_token, false),
        AccountMeta::new(accounts.base_vault, false),
        AccountMeta::new(accounts.quote_vault, false),
        AccountMeta::new_readonly(accounts.base_token_mint, false),
        AccountMeta::new_readonly(accounts.quote_token_mint, false),
        AccountMeta::new_readonly(accounts.base_token_program, false),
        AccountMeta::new_readonly(accounts.quote_token_program, false),
        AccountMeta::new_readonly(accounts.event_authority, false),
        AccountMeta::new_readonly(accounts.program, false),
    ];
    Instruction {
        program_id: ray_launch_constants::RAY_LAUNCH_PROGRAM_ID,
        accounts: metas,
        data: full_data,
    }
}


pub fn get_instruction_accounts(
    account_keys: Vec<Vec<u8>>,
    accounts: Vec<u8>,
) -> RayLaunchAccounts {
    let mint = get_account(account_keys.clone(), accounts.clone(), 9);
    let base_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &mint);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &ray_launch_constants::WSOL);

    RayLaunchAccounts {
        payer: get_wallet_keypair().pubkey(),
        authority: ray_launch_constants::RAY_LAUNCH_AUTHORITY,
        global_config: ray_launch_constants::RAY_LAUNCH_GLOBAL_CONFIG,
        platform_config: ray_launch_constants::RAY_LAUNCH_PROGRAM_CONFIG,
        pool_state: get_account(account_keys.clone(), accounts.clone(), 4),
        user_base_token: base_ata,
        user_quote_token: quote_ata,
        base_vault: get_account(account_keys.clone(), accounts.clone(), 7),
        quote_vault: get_account(account_keys.clone(), accounts.clone(), 8),
        base_token_mint: mint,
        quote_token_mint: ray_launch_constants::WSOL,
        base_token_program: spl_token::ID,
        quote_token_program: spl_token::ID,
        event_authority: ray_launch_constants::RAY_LAUNCH_EVENT_AUTHORITY,
        program: ray_launch_constants::RAY_LAUNCH_PROGRAM_ID,
    }
}
