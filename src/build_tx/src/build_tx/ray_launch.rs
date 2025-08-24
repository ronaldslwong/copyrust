// ray_launch.rs
// Raydium Launchpad Curve: buyExactIn translation from TypeScript to Rust
// Reference: https://github.com/raydium-io/raydium-sdk-V2/blob/master/src/raydium/launchpad/curve/curve.ts

use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use std::error::Error;
use crate::build_tx::utils::SwapDirection;
use crate::init::wallet_loader::get_wallet_keypair;
use crate::build_tx::utils::get_account;
use solana_sdk::signature::Signer;
use solana_program::instruction::{AccountMeta, Instruction};
use num_bigint::BigUint;
use crate::constants::consts;
use crate::constants::raydium_launchpad;
use borsh::{BorshDeserialize, BorshSerialize};

#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub struct RaydiumPoolState {
    pub epoch: u64,
    pub auth_bump: u8,
    pub status: u8,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub migrate_type: u8,
    pub supply: u64,
    // pub padding: [u8; 7],
    pub total_base_sell: u64,
    pub virtual_base: u64,
    pub virtual_quote: u64,
    pub real_base: u64,
    pub real_quote: u64,
    pub total_quote_fund_raising: u64,
    pub quote_protocol_fee: u64,
    pub platform_fee: u64,
    pub migrate_fee: u64,
}

impl Default for RaydiumPoolState {
    fn default() -> Self {
        Self {
            epoch: 0,
            auth_bump: 0,
            status: 0,
            base_decimals: 0,
            quote_decimals: 0,
            migrate_type: 0,
            supply: 0,
            // padding: [0; 7],
            total_base_sell: 0,
            virtual_base: 0,
            virtual_quote: 0,
            real_base: 0,
            real_quote: 0,
            total_quote_fund_raising: 0,
            quote_protocol_fee: 0,
            platform_fee: 0,
            migrate_fee: 0,
        }
    }
}

impl RaydiumPoolState {
    /// Deserialize from account data (skipping the 8-byte discriminator)
    pub fn from_account_data(data: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if data.len() < 8 {
            return Err("Account data too short".into());
        }
        
        // Skip the 8-byte discriminator and deserialize the rest
        let pool_state = RaydiumPoolState::try_from_slice(&data[8..])?;
        Ok(pool_state)
    }
    
    /// Get pool reserves in the format expected by existing code
    pub fn get_reserves(&self) -> RaydiumPoolRealReserves {
        RaydiumPoolRealReserves {
            total_sell_a: self.total_base_sell.into(),
            total_fund_raising_b: self.total_quote_fund_raising.into(),
            real_a: self.real_base,
            real_b: self.real_quote,
            virtual_a: self.virtual_base,
            virtual_b: self.virtual_quote,
        }
    }
    
    /// Check if pool is complete (status > 0)
    pub fn is_complete(&self) -> bool {
        self.status > 0
    }
    
    /// Check if pool has migrated (migrate_type == 1)
    pub fn has_migrated(&self) -> bool {
        self.migrate_type == 1
    }
}

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

impl Default for RayLaunchAccounts {
    fn default() -> Self {
        RayLaunchAccounts {
            payer: Pubkey::default(),
            authority: Pubkey::default(),
            global_config: Pubkey::default(),
            platform_config: Pubkey::default(),
            pool_state: Pubkey::default(),
            user_base_token: Pubkey::default(),
            user_quote_token: Pubkey::default(),
            base_vault: Pubkey::default(),
            quote_vault: Pubkey::default(),
            base_token_mint: Pubkey::default(),
            quote_token_mint: Pubkey::default(),
            base_token_program: Pubkey::default(),
            quote_token_program: Pubkey::default(),
            event_authority: Pubkey::default(),
            program: Pubkey::default(),
        }
    }
}

pub fn default_ray_launch_accounts() -> RayLaunchAccounts {
    RayLaunchAccounts::default()
}


pub struct PoolBaseAmount {
    pub total_sell_a: BigUint,
    pub total_fund_raising_b: BigUint,
    pub real_a: BigUint,
    pub real_b: BigUint,
}

impl Default for PoolBaseAmount {
    fn default() -> Self {
        PoolBaseAmount {
            total_sell_a: BigUint::default(),
            total_fund_raising_b: BigUint::default(),
            real_a: BigUint::default(),
            real_b: BigUint::default(),
        }
    }
}

pub fn default_pool_base_amount() -> PoolBaseAmount {
    PoolBaseAmount::default()
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
    pool_state: &RaydiumPoolState,
    swap_amount: u64,
    target_sol_buy: u64,
    target_token_buy: u64,
) -> Result<u64, Box<dyn Error>> {

    let adjusted_price = match direction {
        SwapDirection::Buy => ((pool_state.virtual_base - pool_state.real_base-target_token_buy) as f64 * swap_amount as f64) / ((pool_state.virtual_quote + pool_state.real_quote + target_sol_buy) as f64 + swap_amount as f64) ,
        SwapDirection::Sell => ((pool_state.virtual_quote + pool_state.real_quote-target_sol_buy) as f64 * swap_amount as f64) / ((pool_state.virtual_base - pool_state.real_base + target_token_buy) as f64 + swap_amount as f64) ,
    };
    Ok(adjusted_price as u64)
}

// pub fn build_ray_launch_buy_instruction(
//     // base_vault: Pubkey,
//     // quote_vault: Pubkey,
//     sol_amount: u64,
//     slippage_basis_points: u64,
//     accounts: RayLaunchAccounts,
//     target_sol_buy: u64,
//     target_token_buy: u64,
// ) -> (Instruction, u64) {


//     let slippage_factor = 1.0+slippage_basis_points as f64 /10000.0;

//     let limit_quote_amount = match get_ray_launch_swap_amount(
//         SwapDirection::Buy,
//         accounts.pool_state,
//         amount,
//         target_sol_buy,
//         target_token_buy,
//     ) {
//         Ok(val) => val,
//         Err(e) => {
//             eprintln!("Could not calculate buy limit_quote_amount: {}", e);
//             // handle gracefully, e.g. return, skip, or set a default value
//             return (Instruction::new_with_bincode(raydium_launchpad::RAY_LAUNCH_PROGRAM_ID, &[0u8; 32], vec![]), 0);
//         }
//     };
//     // println!("amount: {}, limit_quote_amount: {}, limit_quote_amount_slippage: {}", amount, limit_quote_amount, (limit_quote_amount as f64*slippage_factor) as u64);

//     let instruction = build_ray_launch_swap_instruction(accounts, SwapDirection::Buy, limit_quote_amount, (amount as f64*slippage_factor) as u64);
//     (instruction, limit_quote_amount)
// }

pub fn build_ray_launch_buy_instruction_no_quote(
    // base_vault: Pubkey,
    // quote_vault: Pubkey,
    sol_amount: u64,
    token_amount: u64,
    slippage_basis_points: u64,
    accounts: &RayLaunchAccounts,
) -> (Instruction) {

    let slippage_factor = 1.0+slippage_basis_points as f64 /10000.0;

    // println!("amount: {}, limit_quote_amount: {}, limit_quote_amount_slippage: {}", amount, limit_quote_amount, (limit_quote_amount as f64*slippage_factor) as u64);

    let instruction = build_ray_launch_swap_instruction(accounts, SwapDirection::Buy, token_amount, (sol_amount as f64*slippage_factor) as u64);
    (instruction)
}

pub fn build_ray_launch_sell_instruction(
    // base_vault: Pubkey,
    // quote_vault: Pubkey,
    amount: u64,
    slippage_basis_points: u64,
    accounts: &RayLaunchAccounts,
) -> (Instruction) {


    let slippage_factor = 1.0-slippage_basis_points as f64 /10000.0;

    let pool_state = get_pool_state(&accounts);

    let limit_quote_amount = get_ray_launch_swap_amount(
        SwapDirection::Sell,
        &pool_state,
        amount,
        0,
        0,
    ).expect("Failed to calculate buy limit_quote_amount");
    // println!("amount: {}, limit_quote_amount: {}, limit_quote_amount_slippage: {}", amount, limit_quote_amount, (limit_quote_amount as f64*slippage_factor) as u64);
    let instruction = build_ray_launch_swap_instruction(&accounts, SwapDirection::Sell, amount, (limit_quote_amount as f64*slippage_factor) as u64);
    (instruction)
}

/// Build a PumpSwap instruction (buy or sell)
pub fn build_ray_launch_swap_instruction(
    accounts: &RayLaunchAccounts,
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
        program_id: raydium_launchpad::RAY_LAUNCH_PROGRAM_ID,
        accounts: metas,
        data: full_data,
    }
}


pub fn get_instruction_accounts(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
) -> RayLaunchAccounts {
    let mint = get_account(account_keys, accounts, 9);
    let base_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &mint);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &consts::WSOL);

    RayLaunchAccounts {
        payer: get_wallet_keypair().pubkey(),
        authority: raydium_launchpad::RAY_LAUNCH_AUTHORITY,
        global_config: raydium_launchpad::RAY_LAUNCH_GLOBAL_CONFIG,
        platform_config: get_account(account_keys, accounts, 3),
        pool_state: get_account(account_keys, accounts, 4),
        user_base_token: base_ata,
        user_quote_token: quote_ata,
        base_vault: get_account(account_keys, accounts, 7),
        quote_vault: get_account(account_keys, accounts, 8),
        base_token_mint: mint,
        quote_token_mint: consts::WSOL,
        base_token_program: spl_token::ID,
        quote_token_program: spl_token::ID,
        event_authority: raydium_launchpad::RAY_LAUNCH_EVENT_AUTHORITY,
        program: raydium_launchpad::RAY_LAUNCH_PROGRAM_ID,
    }
}
pub fn get_pool_state(ray_launch_accounts: &RayLaunchAccounts) -> RaydiumPoolState {
    let client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    let account_data = client.get_account_data(&ray_launch_accounts.pool_state).expect("Failed to get account data");
    println!("pool_state: {:?}", ray_launch_accounts.pool_state);
    let pool_state = RaydiumPoolState::deserialize(&mut &account_data[8..]).expect("Failed to deserialize bonding curve state");
    
    pool_state
}


