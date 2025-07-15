// ray_launch.rs
// Raydium Launchpad Curve: buyExactIn translation from TypeScript to Rust
// Reference: https://github.com/raydium-io/raydium-sdk-V2/blob/master/src/raydium/launchpad/curve/curve.ts

use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use std::error::Error;
use crate::init::wallet_loader::get_wallet_keypair;
use solana_sdk::signature::Signer;
use solana_program::instruction::{AccountMeta, Instruction};
use num_bigint::BigUint;
use crate::build_tx::utils::get_pool_accounts;
use borsh::{BorshDeserialize, BorshSerialize};
use crate::build_tx::utils::get_constant_product_swap_amount;
use crate::build_tx::utils::get_pool_vault_amount;
use crate::build_tx::utils::SwapDirection;

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
pub mod ray_cpmm_constants {
    use solana_program::pubkey;
    use solana_program::pubkey::Pubkey;

    pub const RAY_CPMM_PROGRAM_ID: Pubkey = pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");
    pub const RAY_CPMM_AUTHORITY: Pubkey = pubkey!("GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL");
    pub const WSOL: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
}

#[derive(PartialEq, Copy, Clone, Debug,BorshDeserialize, BorshSerialize)]

pub struct RaydiumCpmmPoolState {
    pub amm_config: Pubkey,
    pub pool_creator: Pubkey,
    pub token_0_vault: Pubkey,
    pub token_1_vault: Pubkey,
    pub lp_mint: Pubkey,
    pub token_0_mint: Pubkey,
    pub token_1_mint: Pubkey,
    pub token_0_program: Pubkey,
    pub token_1_program: Pubkey,
    pub observation_key: Pubkey,
    pub auth_bump: u8,
    pub status: u8,
    pub lp_mint_decimals: u8,
    pub mint_0_decimals: u8,
    pub mint_1_decimals: u8,
    pub lp_supply: u64,
    pub protocol_fees_token_0: u64,
    pub protocol_fees_token_1: u64,
    pub fund_fees_token_0: u64,
    pub fund_fees_token_1: u64,
    pub open_time: u64,
    pub recent_epoch: u64,
    pub padding: [u64; 31],
}


/// Helper to get the discriminator for buy/sell
fn get_discriminator(direction: SwapDirection) -> [u8; 8] {
    match direction {
        SwapDirection::Buy => [55, 217, 98, 86, 163, 74, 180, 173],
        SwapDirection::Sell => [143, 190, 90, 218, 196, 30, 51, 222],
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

pub fn build_ray_cpmm_buy_instruction(
    // base_vault: Pubkey,
    // quote_vault: Pubkey,
    pool_state: Pubkey,
    amount: u64,
    slippage_basis_points: u64,
    mint: Pubkey,
) -> (Instruction, u64) {
    let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

    let slippage_factor = 1.0+slippage_basis_points as f64 /10000.0;
    let instruction: Instruction = Instruction{
        program_id: Pubkey::new_unique(),
        accounts: vec![],
        data: vec![],
    };
    // let pool_state = match get_pool_accounts(mint, &rpc_client, [200u64], ray_cpmm_constants::RAY_CPMM_PROGRAM_ID) {
    //     Some(pk) => pk,
    //     None => {
    //         eprintln!("Failed to find pool account for mint {}", mint);
    //         return (instruction, 0);
    //     }
    // };
    let pool_state_data = match rpc_client.get_account_data(&pool_state) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to fetch account data: {}", e);
            return (instruction, 0);
        }
    };
    if pool_state_data.iter().all(|&b| b == 0) {
        println!("pool_state_data is all zeros!");
        return (instruction, 0); // or return an error, or handle as needed
    }

    let pool_ac_detail = RaydiumCpmmPoolState::deserialize(&mut &pool_state_data[8..]).unwrap();

    let base_vault = pool_ac_detail.token_1_vault;
    let quote_vault = pool_ac_detail.token_0_vault;
    let (base_amount, quote_amount) = get_pool_vault_amount(base_vault, quote_vault).unwrap();

    println!("base_amount: {}, quote_amount: {}", base_amount, quote_amount);
    let token_quote_amount = get_constant_product_swap_amount(
        SwapDirection::Buy,
        base_amount,
        quote_amount,
        amount,
        0,
        0,
    ).expect("Failed to calculate buy limit_quote_amount");
    // println!("amount: {}, limit_quote_amount: {}, limit_quote_amount_slippage: {}", amount, limit_quote_amount, (limit_quote_amount as f64*slippage_factor) as u64);
    let instruction = build_ray_cpmm_swap_instruction(pool_state, pool_ac_detail, SwapDirection::Buy, (amount as f64*slippage_factor) as u64, token_quote_amount);
    println!("token_quote_amount: {}", token_quote_amount);
    (instruction, token_quote_amount)
}

pub fn build_ray_cpmm_sell_instruction(
    // base_vault: Pubkey,
    // quote_vault: Pubkey,
    amount: u64,
    slippage_basis_points: u64,
    mint: Pubkey,
) -> (Instruction) {
    let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

    let slippage_factor = 1.0-slippage_basis_points as f64 /10000.0;
    let instruction: Instruction = Instruction{
        program_id: Pubkey::new_unique(),
        accounts: vec![],
        data: vec![],
    };

    let pool_state = get_pool_accounts(mint, &rpc_client, [43u64], ray_cpmm_constants::RAY_CPMM_PROGRAM_ID);
    let pool_state_data = rpc_client.get_account_data(&pool_state.unwrap()).unwrap();
    if pool_state_data.iter().all(|&b| b == 0) {
        println!("pool_state_data is all zeros!");
        return instruction; // or return an error, or handle as needed
    }

    let pool_ac_detail = RaydiumCpmmPoolState::deserialize(&mut &pool_state_data[8..]).unwrap();

    let base_vault = pool_ac_detail.token_1_vault;
    let quote_vault = pool_ac_detail.token_0_vault;
    let (base_amount, quote_amount) = get_pool_vault_amount(base_vault, quote_vault).unwrap();

    let limit_quote_amount = get_constant_product_swap_amount(
        SwapDirection::Sell,
        base_amount,
        quote_amount,
        amount,
        0,
        0,
    ).expect("Failed to calculate buy limit_quote_amount");
    println!("amount: {}, limit_quote_amount: {}, limit_quote_amount_slippage: {}", amount, limit_quote_amount, (limit_quote_amount as f64*slippage_factor) as u64);
    let instruction = build_ray_cpmm_swap_instruction(pool_state.unwrap(), pool_ac_detail, SwapDirection::Sell, amount, (limit_quote_amount as f64*slippage_factor) as u64);
    (instruction)
}

pub fn build_ray_cpmm_sell_instruction_with_pool_state(
    // base_vault: Pubkey,
    // quote_vault: Pubkey,
    pool_state: Pubkey,
    amount: u64,
    slippage_basis_points: u64,
    mint: Pubkey,
) -> (Instruction) {
    let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

    let slippage_factor = 1.0-slippage_basis_points as f64 /10000.0;
    let instruction: Instruction = Instruction{
        program_id: Pubkey::new_unique(),
        accounts: vec![],
        data: vec![],
    };

    let pool_state_data = rpc_client.get_account_data(&pool_state).unwrap();
    if pool_state_data.iter().all(|&b| b == 0) {
        println!("pool_state_data is all zeros!");
        return instruction; // or return an error, or handle as needed
    }

    let pool_ac_detail = RaydiumCpmmPoolState::deserialize(&mut &pool_state_data[8..]).unwrap();

    let base_vault = pool_ac_detail.token_1_vault;
    let quote_vault = pool_ac_detail.token_0_vault;
    let (base_amount, quote_amount) = get_pool_vault_amount(base_vault, quote_vault).unwrap();

    let limit_quote_amount = get_constant_product_swap_amount(
        SwapDirection::Sell,
        base_amount,
        quote_amount,
        amount,
        0,
        0,
    ).expect("Failed to calculate buy limit_quote_amount");
    println!("amount: {}, limit_quote_amount: {}, limit_quote_amount_slippage: {}", amount, limit_quote_amount, (limit_quote_amount as f64*slippage_factor) as u64);
    let instruction = build_ray_cpmm_swap_instruction(pool_state, pool_ac_detail, SwapDirection::Sell, amount, (limit_quote_amount as f64*slippage_factor) as u64);
    (instruction)
}


/// Build a PumpSwap instruction (buy or sell)
pub fn build_ray_cpmm_swap_instruction(
    pool_state: Pubkey,
    accounts: RaydiumCpmmPoolState,
    direction: SwapDirection,
    limit_quote_amount: u64,
    amount: u64,
) -> Instruction {
    let discriminator = get_discriminator(direction);
    let wallet = get_wallet_keypair();

    let base_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &accounts.token_1_mint);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &ray_cpmm_constants::WSOL);

    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&limit_quote_amount.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    let mut full_data = [discriminator.as_ref(), data.as_slice()].concat();
    full_data.extend_from_slice(&[0u8; 16]);

    let mut metas = vec![];
    if direction == SwapDirection::Buy {
        metas = vec![
            AccountMeta::new(wallet.pubkey(), true),
            AccountMeta::new(ray_cpmm_constants::RAY_CPMM_AUTHORITY, false),
            AccountMeta::new_readonly(accounts.amm_config, false),
            AccountMeta::new(pool_state, false),
            AccountMeta::new(quote_ata, false),
            AccountMeta::new(base_ata, false),
            AccountMeta::new(accounts.token_0_vault, false),
            AccountMeta::new(accounts.token_1_vault, false),
            AccountMeta::new_readonly(accounts.token_0_program, false),
            AccountMeta::new_readonly(accounts.token_1_program, false),
            AccountMeta::new_readonly(accounts.token_0_mint, false),
            AccountMeta::new_readonly(accounts.token_1_mint, false),
            AccountMeta::new(accounts.observation_key, false),
    ];
    } else {
        metas = vec![
            AccountMeta::new(wallet.pubkey(), true),
            AccountMeta::new(ray_cpmm_constants::RAY_CPMM_AUTHORITY, false),
            AccountMeta::new_readonly(accounts.amm_config, false),
            AccountMeta::new(pool_state, false),
            AccountMeta::new(base_ata, false),
            AccountMeta::new(quote_ata, false),
            AccountMeta::new(accounts.token_1_vault, false),
            AccountMeta::new(accounts.token_0_vault, false),
            AccountMeta::new_readonly(accounts.token_1_program, false),
            AccountMeta::new_readonly(accounts.token_0_program, false),
            AccountMeta::new_readonly(accounts.token_1_mint, false),
            AccountMeta::new_readonly(accounts.token_0_mint, false),
            AccountMeta::new(accounts.observation_key, false),
        ];
    }

    Instruction {
        program_id: ray_cpmm_constants::RAY_CPMM_PROGRAM_ID,
        accounts: metas,
        data: full_data,
    }
}

