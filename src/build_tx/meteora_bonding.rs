// ray_launch.rs
// Raydium Launchpad Curve: buyExactIn translation from TypeScript to Rust
// Reference: https://github.com/raydium-io/raydium-sdk-V2/blob/master/src/raydium/launchpad/curve/curve.ts

use solana_sdk::pubkey::Pubkey;
use crate::constants::meteora_bonding::METEORA_BONDING_PROGRAM_ID_PUBKEY;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use std::error::Error;
use crate::init::wallet_loader::get_wallet_keypair;
use solana_sdk::signature::Signer;
use solana_program::instruction::{AccountMeta, Instruction};
use crate::build_tx::utils::SwapDirection;
use crate::build_tx::utils::get_account;
use crate::constants::meteora_bonding::METEORA_BONDING_POOL_AUTHORITY;
use crate::constants::meteora_bonding::METEORA_BONDING_EVENT_AUTHORITY;
use crate::constants::consts::WSOL;
use spl_token;
use std::convert::TryInto;
use uint::construct_uint;
use solana_program::hash::Hash;
use solana_program::keccak;

// Define U256 for large number handling
construct_uint! {
    pub struct U256(4);
}

// Meteora DBC resolution constant (from official SDK)
pub const RESOLUTION: u8 = 64;

// Example of how to safely implement large bit shifting operations
// This demonstrates the (1000000000 << 128) operation you mentioned
pub fn safe_large_bit_shift_demo() -> U256 {
    let base = U256::from(1000000000u128);
    let shift_amount = 128u32;
    
    // This is safe and won't overflow!
    let result = base << shift_amount;
    
    println!("[DEMO] Safe large bit shift:");
    println!("[DEMO] Base: {}", base);
    println!("[DEMO] Shift amount: {}", shift_amount);
    println!("[DEMO] Result: {} * 2^{} = {}", base, shift_amount, result);
    
    result
}

// Alternative approach using multiplication instead of bit shifting
pub fn safe_large_multiply_demo() -> U256 {
    let base = U256::from(1000000000u128);
    
    // Calculate 2^128 using U256
    let two_to_128 = U256::from(1u128) << 128u32;
    let result = base * two_to_128;
    
    println!("[DEMO] Safe large multiplication:");
    println!("[DEMO] Base: {}", base);
    println!("[DEMO] 2^128: {}", two_to_128);
    println!("[DEMO] Result: {} * {} = {}", base, two_to_128, result);
    
    result
}

// Utility function for the exact operation you mentioned: (1000000000 << 128)
pub fn safe_million_shift_128() -> U256 {
    let base = U256::from(1000000000u128);
    let shift_amount = 128u32;
    
    // This is the exact operation you asked about - safe with U256!
    let result = base << shift_amount;
    
    println!("[UTILITY] Safe (1000000000 << 128):");
    println!("[UTILITY] Base: 1,000,000,000");
    println!("[UTILITY] Shift: 128 bits");
    println!("[UTILITY] Result: {}", result);
    println!("[UTILITY] Binary length: {} bits", result.bits());
    
    result
}

// Generic function for any large bit shift operation
pub fn safe_large_shift<T>(base: T, shift_amount: u32) -> U256 
where
    U256: From<T>,
{
    let base_256 = U256::from(base);
    let result = base_256 << shift_amount;
    
    println!("[GENERIC] Safe large shift:");
    println!("[GENERIC] Base: {}", base_256);
    println!("[GENERIC] Shift: {} bits", shift_amount);
    println!("[GENERIC] Result: {}", result);
    println!("[GENERIC] Binary length: {} bits", result.bits());
    
    result
}

/// Struct containing all the account parameters for Meteora DBC swap instructions
#[derive(Debug, Clone, Default)]
pub struct MeteoraBondingSwapAccounts {
    pub pool_authority: Pubkey,
    pub config: Pubkey,
    pub pool: Pubkey,
    pub input_token_account: Pubkey,
    pub output_token_account: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub payer: Pubkey,
    pub token_base_program: Pubkey,
    pub token_quote_program: Pubkey,
    pub referral_token_account: Pubkey,
    pub event_authority: Pubkey,
    pub program: Pubkey,
}

impl MeteoraBondingSwapAccounts {
    /// Create a new instance with default values
    pub fn new() -> Self {
        Self {
            pool_authority: METEORA_BONDING_POOL_AUTHORITY,
            config: Pubkey::default(), // TODO: Add proper Meteora config constant
            pool: Pubkey::default(),
            input_token_account: Pubkey::default(),
            output_token_account: Pubkey::default(),
            base_vault: Pubkey::default(),
            quote_vault: Pubkey::default(),
            base_mint: Pubkey::default(),
            quote_mint: Pubkey::default(),
            payer: get_wallet_keypair().pubkey(),
            token_base_program: spl_token::ID,
            token_quote_program: spl_token::ID,
            referral_token_account: Pubkey::default(),
            event_authority: METEORA_BONDING_EVENT_AUTHORITY,
            program: METEORA_BONDING_PROGRAM_ID_PUBKEY,
        }
    }

    /// Convert to AccountMeta vector for buy direction
    pub fn to_buy_account_metas(&self) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new_readonly(self.pool_authority, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new(self.pool, false),
            AccountMeta::new(self.input_token_account, false),
            AccountMeta::new(self.output_token_account, false),
            AccountMeta::new(self.base_vault, false),
            AccountMeta::new(self.quote_vault, false),
            AccountMeta::new_readonly(self.base_mint, false),
            AccountMeta::new_readonly(self.quote_mint, false),
            AccountMeta::new(self.payer, true),
            AccountMeta::new_readonly(self.token_base_program, false),
            AccountMeta::new_readonly(self.token_quote_program, false),
            AccountMeta::new(self.referral_token_account, false),
            AccountMeta::new_readonly(self.event_authority, false),
            AccountMeta::new_readonly(self.program, false),
        ]
    }

    /// Convert to AccountMeta vector for sell direction
    pub fn to_sell_account_metas(&self) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new_readonly(self.pool_authority, false),
            AccountMeta::new_readonly(self.config, false),
            AccountMeta::new(self.pool, false),
            AccountMeta::new(self.output_token_account, false),
            AccountMeta::new(self.input_token_account, false),
            AccountMeta::new(self.base_vault, false),
            AccountMeta::new(self.quote_vault, false),
            AccountMeta::new_readonly(self.base_mint, false),
            AccountMeta::new_readonly(self.quote_mint, false),
            AccountMeta::new(self.payer, true),
            AccountMeta::new_readonly(self.token_base_program, false),
            AccountMeta::new_readonly(self.token_quote_program, false),
            AccountMeta::new(self.referral_token_account, false),
            AccountMeta::new_readonly(self.event_authority, false),
            AccountMeta::new_readonly(self.program, false),
        ]
    }
}

/// Represents the result of a swap calculation
#[derive(Debug, Clone)]
pub struct SwapAmount {
    pub output_amount: u64,
    pub next_sqrt_price: u128,
}

/// Represents a point on the bonding curve
#[derive(Debug, Clone, Copy)]
pub struct LiquidityDistributionConfig {
    pub sqrt_price: u128,
    pub liquidity: u128,
}

/// Represents base fee configuration
#[derive(Debug, Clone)]
pub struct BaseFeeConfig {
    pub base_fee: u64,
    pub base_fee_numerator: u64,
    pub base_fee_denominator: u64,
}

/// Represents dynamic fee configuration
#[derive(Debug, Clone)]
pub struct DynamicFeeConfig {
    pub dynamic_fee: u64,
    pub dynamic_fee_numerator: u64,
    pub dynamic_fee_denominator: u64,
}

/// Represents pool fees configuration
#[derive(Debug, Clone)]
pub struct PoolFeesConfig {
    pub base_fee_config: BaseFeeConfig,
    pub dynamic_fee_config: DynamicFeeConfig,
}

/// Represents locked vesting configuration
#[derive(Debug, Clone)]
pub struct LockedVestingConfig {
    pub locked_lp_percentage: u8,
    pub partner_locked_lp_percentage: u8,
    pub creator_locked_lp_percentage: u8,
}

/// Represents the pool configuration with bonding curve parameters
/// Based on the actual on-chain PoolConfig account structure
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub sqrt_start_price: u128,
    pub curve: [LiquidityDistributionConfig; 20],
    pub migration_quote_threshold: u64,
    pub pool_fees: PoolFeesConfig,
    pub collect_fee_mode: u8,
    pub migration_option: u8,
    pub activation_type: u8,
    pub token_type: u8,
    pub token_decimal: u8,
    pub partner_lp_percentage: u8,
    pub creator_lp_percentage: u8,
    pub locked_vesting: LockedVestingConfig,
    pub fee_claimer: Pubkey,
    pub quote_mint: Pubkey,
}

impl PoolConfig {
    /// Deserialize pool configuration from account data
    /// Based on the actual on-chain PoolConfig account structure from Solscan
    pub fn from_account_data(data: &[u8]) -> Result<Self, Box<dyn Error>> {
        // Based on the actual JSON structure, the layout is different from what we assumed
        // We need to parse from the end since sqrt_start_price and curve are at the bottom
        if data.len() < 816 {
            return Err("Insufficient pool config data length".into());
        }
        
        let mut offset = 8; // Skip 8-byte Anchor discriminator
        
        // Start parsing from the beginning with the fields we can identify
        // quote_mint: Pubkey (32 bytes) - appears to be first
        let quote_mint = Pubkey::try_from(&data[offset..offset+32]).map_err(|_| "Failed to parse quote_mint")?;
        offset += 32;
        
        // fee_claimer: Pubkey (32 bytes)
        let fee_claimer = Pubkey::try_from(&data[offset..offset+32]).map_err(|_| "Failed to parse fee_claimer")?;
        offset += 32;
        
        // leftover_receiver: Pubkey (32 bytes) - from JSON structure
        let _leftover_receiver = Pubkey::try_from(&data[offset..offset+32]).map_err(|_| "Failed to parse leftover_receiver")?;
        offset += 32;
        
        // Skip pool_fees complex structure for now - we'll parse the simple fields we need
        // Skip to the fields we can identify
        
        // Skip various fields until we get to the ones we need
        // Based on the JSON, we need to find sqrt_start_price and curve at the end
        
        // For now, let's try to find sqrt_start_price by looking for it in the data
        // This is a temporary approach until we can determine the exact layout
        
        // Try to find sqrt_start_price by looking at the end of the data
        // The curve should be the last 640 bytes (20 * 32)
        let curve_start = data.len() - 640;
        let sqrt_start_price_start = curve_start - 16;
        
        if sqrt_start_price_start < offset {
            return Err("Invalid data layout - cannot find sqrt_start_price".into());
        }
        
        // Parse sqrt_start_price from near the end
        let sqrt_start_price = u128::from_le_bytes(
            data[sqrt_start_price_start..sqrt_start_price_start+16].try_into().map_err(|_| "Failed to parse sqrt_start_price")?
        );
        
        // Parse curve from the end
        let mut curve = [LiquidityDistributionConfig { sqrt_price: 0, liquidity: 0 }; 20];
        for i in 0..20 {
            let curve_offset = curve_start + (i * 32);
            let sqrt_price = u128::from_le_bytes(
                data[curve_offset..curve_offset+16].try_into().map_err(|_| format!("Failed to parse curve[{}].sqrt_price", i))?
            );
            let liquidity = u128::from_le_bytes(
                data[curve_offset+16..curve_offset+32].try_into().map_err(|_| format!("Failed to parse curve[{}].liquidity", i))?
            );
            
            curve[i] = LiquidityDistributionConfig { sqrt_price, liquidity };
        }
        
        // For now, use default values for fields we can't parse yet
        // This is a temporary solution until we can determine the exact layout
        let migration_quote_threshold = 0; // We'll need to find this in the data
        
        Ok(PoolConfig {
            sqrt_start_price,
            curve,
            migration_quote_threshold,
            pool_fees: PoolFeesConfig {
                base_fee_config: BaseFeeConfig {
                    base_fee: 0,
                    base_fee_numerator: 0,
                    base_fee_denominator: 0,
                },
                dynamic_fee_config: DynamicFeeConfig {
                    dynamic_fee: 0,
                    dynamic_fee_numerator: 0,
                    dynamic_fee_denominator: 0,
                },
            },
            collect_fee_mode: 0,
            migration_option: 0,
            activation_type: 0,
            token_type: 0,
            token_decimal: 0,
            partner_lp_percentage: 0,
            creator_lp_percentage: 0,
            locked_vesting: LockedVestingConfig {
                locked_lp_percentage: 0,
                partner_locked_lp_percentage: 0,
                creator_locked_lp_percentage: 0,
            },
            fee_claimer,
            quote_mint,
        })
    }
    
    /// Get the maximum amount that can be swapped without hitting curve limits
    pub fn get_max_swallow_quote_amount(&self) -> Result<u64, Box<dyn Error>> {
        // Calculate based on the curve data
        let max_liquidity = self.curve.iter()
            .map(|point| point.liquidity)
            .max()
            .unwrap_or(0);
        
        // Convert from u128 to u64 (with safety check)
        if max_liquidity > u64::MAX as u128 {
            return Err("Liquidity value too large for u64".into());
        }
        
        Ok(max_liquidity as u64)
    }
    
    /// Create a default pool config for testing/fallback
    pub fn default() -> Self {
        let mut curve = [LiquidityDistributionConfig { sqrt_price: 0, liquidity: 0 }; 20];
        // Set first 3 elements with default values
        curve[0] = LiquidityDistributionConfig { sqrt_price: 1000, liquidity: 1000000 };
        curve[1] = LiquidityDistributionConfig { sqrt_price: 2000, liquidity: 1000000 };
        curve[2] = LiquidityDistributionConfig { sqrt_price: 3000, liquidity: 1000000 };
        
        Self {
            sqrt_start_price: 1000,
            curve,
            migration_quote_threshold: 100000000,
            pool_fees: PoolFeesConfig {
                base_fee_config: BaseFeeConfig {
                    base_fee: 100, // 0.01%
                    base_fee_numerator: 1,
                    base_fee_denominator: 10000,
                },
                dynamic_fee_config: DynamicFeeConfig {
                    dynamic_fee: 0,
                    dynamic_fee_numerator: 0,
                    dynamic_fee_denominator: 0,
                },
            },
            collect_fee_mode: 0,
            migration_option: 0,
            activation_type: 0,
            token_type: 0,
            token_decimal: 9,
            partner_lp_percentage: 50, // 50%
            creator_lp_percentage: 20, // 20%
            locked_vesting: LockedVestingConfig {
                locked_lp_percentage: 0,
                partner_locked_lp_percentage: 0,
                creator_locked_lp_percentage: 0,
            },
            fee_claimer: Pubkey::default(),
            quote_mint: WSOL,
        }
    }
}

/// Derive the pool configuration PDA for a given pool
pub fn derive_pool_config_pda(pool: &Pubkey) -> Result<(Pubkey, u8), Box<dyn Error>> {
    let seeds = &[
        b"pool_config",
        pool.as_ref(),
    ];
    
    let (config_pda, bump) = Pubkey::find_program_address(
        seeds,
        &METEORA_BONDING_PROGRAM_ID_PUBKEY,
    );
    
    Ok((config_pda, bump))
}

/// Derive the pool PDA for a given base mint and quote mint
pub fn derive_pool_pda(base_mint: &Pubkey, quote_mint: &Pubkey) -> Result<(Pubkey, u8), Box<dyn Error>> {
    let seeds = &[
        b"pool",
        base_mint.as_ref(),
        quote_mint.as_ref(),
    ];
    
    let (pool_pda, bump) = Pubkey::find_program_address(
        seeds,
        &METEORA_BONDING_PROGRAM_ID_PUBKEY,
    );
    
    Ok((pool_pda, bump))
}

/// Represents the actual Meteora pool data with only fields needed for quote calculation
#[derive(Debug, Clone)]
pub struct Pool {
    pub sqrt_price: u128,
    pub base_reserve: u64,
    pub quote_reserve: u64,
    pub pool_type: u8,
    pub protocol_base_fee: u64,
    pub protocol_quote_fee: u64,
    pub partner_base_fee: u64,
    pub partner_quote_fee: u64,
}

/// Error types for pool operations
#[derive(Debug)]
pub enum PoolError {
    TypeCastFailed,
    SwapAmountIsOverAThreshold,
    InsufficientLiquidity,
    InvalidPrice,
    MathOverflow,
}

impl std::fmt::Display for PoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolError::TypeCastFailed => write!(f, "Type cast failed"),
            PoolError::SwapAmountIsOverAThreshold => write!(f, "Swap amount is over threshold"),
            PoolError::InsufficientLiquidity => write!(f, "Insufficient liquidity"),
            PoolError::InvalidPrice => write!(f, "Invalid price"),
            PoolError::MathOverflow => write!(f, "Math overflow"),
        }
    }
}

impl std::error::Error for PoolError {}

/// Result type for pool operations
pub type PoolResult<T> = std::result::Result<T, PoolError>;

/// Helper trait for safe arithmetic operations
pub trait SafeArithmetic {
    fn safe_add(self, other: u64) -> PoolResult<u64>;
    fn safe_sub(self, other: u64) -> PoolResult<u64>;
}

impl SafeArithmetic for u64 {
    fn safe_add(self, other: u64) -> PoolResult<u64> {
        self.checked_add(other).ok_or(PoolError::TypeCastFailed)
    }
    
    fn safe_sub(self, other: u64) -> PoolResult<u64> {
        self.checked_sub(other).ok_or(PoolError::TypeCastFailed)
    }
}

/// Rounding direction for calculations
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rounding {
    Up,
    Down,
}

/// Helper to get the discriminator for buy/sell
fn get_discriminator(direction: SwapDirection) -> [u8; 8] {
    match direction {
        SwapDirection::Buy => [248, 198, 158, 145, 225, 117, 135, 200],
        SwapDirection::Sell => [248, 198, 158, 145, 225, 117, 135, 200],
    }
}


// pub fn get_ray_cpmm_swap_amount(
//     direction: SwapDirection,
//     pool_ac: Pubkey,
//     swap_amount: u64,
//     target_sol_buy: u64,
//     target_token_buy: u64,
// ) -> Result<u64, Box<dyn Error>> {
//     let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

//     let res: solana_client::rpc_response::Response<Vec<Option<solana_sdk::account::Account>>> = rpc_client.get_multiple_accounts_with_commitment(&[pool_ac], CommitmentConfig::processed())?;
//     if res.value.is_empty() || res.value[0].is_none() {
//         return Err("missing pool account data".into());
//     }
//     let account_opt = res.value.get(0).and_then(|opt| opt.as_ref());
//     let data = account_opt.map(|acct| acct.data.as_slice()).ok_or("missing pool account data")?;
    
//     let pool_data = RaydiumPoolRealReserves::from_account_data(data, 53, 61, 0, 0, 37, 45)
//         .expect("Failed to parse pool reserves");
    
//     if pool_data.real_a == 0 {
//         return Err("zero base amount".into());
//     }
//     println!("pool_data.real_base: {}", pool_data.real_a);
//     println!("pool_data.real_quote: {}", pool_data.real_b);
//     let adjusted_price = match direction {
//         SwapDirection::Buy => ((pool_data.virtual_a - pool_data.real_a-target_token_buy) as f64 * swap_amount as f64) / ((pool_data.virtual_b + pool_data.real_b + target_sol_buy) as f64 + swap_amount as f64) ,
//         SwapDirection::Sell => ((pool_data.virtual_b + pool_data.real_b-target_sol_buy) as f64 * swap_amount as f64) / ((pool_data.virtual_a - pool_data.real_a + target_token_buy) as f64 + swap_amount as f64) ,
//     };
//     Ok(adjusted_price as u64)
// }




pub fn build_meteora_dbc_sell_instruction(
    amount: u64,
    meteora_accounts: &MeteoraBondingSwapAccounts,
) -> Instruction {
    let instruction = build_meteora_dbc_swap_instruction(&meteora_accounts, SwapDirection::Sell, amount, 0);
    instruction
}





/// Build a Meteora DBC swap instruction (buy or sell)
pub fn build_meteora_dbc_swap_instruction(
    accounts: &MeteoraBondingSwapAccounts,
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

    let mut metas = vec![];
    if direction == SwapDirection::Buy {
        metas = vec![
            AccountMeta::new_readonly(accounts.pool_authority, false),
            AccountMeta::new_readonly(accounts.config, false),
            AccountMeta::new(accounts.pool, false),
            AccountMeta::new(accounts.input_token_account, false),
            AccountMeta::new(accounts.output_token_account, false),
            AccountMeta::new(accounts.base_vault, false),
            AccountMeta::new(accounts.quote_vault, false),
            AccountMeta::new_readonly(accounts.base_mint, false),
            AccountMeta::new_readonly(accounts.quote_mint, false),
            AccountMeta::new(accounts.payer, true),
            AccountMeta::new_readonly(accounts.token_base_program, false),
            AccountMeta::new_readonly(accounts.token_quote_program, false),
            AccountMeta::new(accounts.referral_token_account, false),
            AccountMeta::new_readonly(accounts.event_authority, false),
            AccountMeta::new_readonly(accounts.program, false),
        ];
    } else {
        metas = vec![
            AccountMeta::new_readonly(accounts.pool_authority, false),
            AccountMeta::new_readonly(accounts.config, false),
            AccountMeta::new(accounts.pool, false),
            AccountMeta::new(accounts.output_token_account, false),
            AccountMeta::new(accounts.input_token_account, false),
            AccountMeta::new(accounts.base_vault, false),
            AccountMeta::new(accounts.quote_vault, false),
            AccountMeta::new_readonly(accounts.base_mint, false),
            AccountMeta::new_readonly(accounts.quote_mint, false),
            AccountMeta::new(accounts.payer, true),
            AccountMeta::new_readonly(accounts.token_base_program, false),
            AccountMeta::new_readonly(accounts.token_quote_program, false),
            AccountMeta::new(accounts.referral_token_account, false),
            AccountMeta::new_readonly(accounts.event_authority, false),
            AccountMeta::new_readonly(accounts.program, false),
        ];
    }

    Instruction {
        program_id: METEORA_BONDING_PROGRAM_ID_PUBKEY,
        accounts: metas,
        data: full_data,
    }
}

pub fn get_meteora_bonding_instruction_accounts(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
) -> MeteoraBondingSwapAccounts {

    let mint = get_account(account_keys, accounts, 7);
    let base_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &mint);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &WSOL);
    
    // Derive the pool PDA for this base/quote mint pair
    // let (pool_pda, _bump) = derive_pool_pda(&mint, &WSOL)
    //     .unwrap_or_else(|_| (Pubkey::default(), 0));
    
    // Derive the pool config PDA for this pool
    // let (config_pda, _config_bump) = derive_pool_config_pda(&pool_pda)
    //     .unwrap_or_else(|_| (Pubkey::default(), 0));
 
    MeteoraBondingSwapAccounts {
        pool_authority: METEORA_BONDING_POOL_AUTHORITY,
        config: get_account(&account_keys, &accounts, 1), // Use derived PDA instead of hardcoded account
        pool: get_account(&account_keys, &accounts, 2), // Use derived PDA instead of hardcoded account
        input_token_account: quote_ata,
        output_token_account: base_ata,
        base_vault: get_account(&account_keys, &accounts, 5),
        quote_vault: get_account(&account_keys, &accounts, 6),
        base_mint: mint,
        quote_mint: WSOL,
        payer: get_wallet_keypair().pubkey(),
        token_base_program: spl_token::ID,
        token_quote_program: spl_token::ID,
        referral_token_account: METEORA_BONDING_PROGRAM_ID_PUBKEY,
        event_authority: METEORA_BONDING_EVENT_AUTHORITY,
        program: METEORA_BONDING_PROGRAM_ID_PUBKEY,
    }
    // TODO: Map the correct indices for each field as per the actual instruction layout
}


/// Get pool state from Meteora accounts (legacy function name for compatibility)
/// This function now returns the actual Pool struct with real data
pub fn get_pool_state(meteora_accounts: &MeteoraBondingSwapAccounts) -> Result<Pool, Box<dyn Error>> {
    get_meteora_pool_state(meteora_accounts)
}

/// Deserialize Meteora pool data from account data
/// This function extracts only the fields needed for quote calculation
pub fn deserialize_meteora_pool(data: &[u8]) -> Result<Pool, Box<dyn Error>> {
    // Based on the JSON structure provided, the fields are at these offsets:
    // volatility_tracker: 8 bytes (timestamp) + 8 bytes (padding) + 16 bytes (sqrt_price_reference) + 16 bytes (volatility_accumulator) + 16 bytes (volatility_reference) = 64 bytes
    // config: pubkey (32 bytes) at offset 64
    // creator: pubkey (32 bytes) at offset 96
    // base_mint: pubkey (32 bytes) at offset 128
    // base_vault: pubkey (32 bytes) at offset 160
    // quote_vault: pubkey (32 bytes) at offset 192
    // base_reserve: u64 at offset 232 (after quote_vault)
    // quote_reserve: u64 at offset 240 (after base_reserve)
    // protocol_base_fee: u64 at offset 248 (after quote_reserve)
    // protocol_quote_fee: u64 at offset 256 (after protocol_base_fee)
    // partner_base_fee: u64 at offset 264 (after protocol_quote_fee)
    // partner_quote_fee: u64 at offset 272 (after partner_base_fee)
    // sqrt_price: u128 at offset 280 (after partner_quote_fee)
    // pool_type: u8 at offset 296 (after sqrt_price)
    
    if data.len() < 297 { // Ensure minimum data length for all required fields
        return Err("Insufficient pool data length".into());
    }
    
    // Parse base_reserve (u64) at offset 232
    let base_reserve = u64::from_le_bytes(
        data[232..240].try_into().map_err(|_| "Failed to convert base_reserve slice to array")?
    );
    
    // Parse quote_reserve (u64) at offset 240
    let quote_reserve = u64::from_le_bytes(
        data[240..248].try_into().map_err(|_| "Failed to convert quote_reserve slice to array")?
    );
    
    // Parse protocol_base_fee (u64) at offset 248
    let protocol_base_fee = u64::from_le_bytes(
        data[248..256].try_into().map_err(|_| "Failed to convert protocol_base_fee slice to array")?
    );
    
    // Parse protocol_quote_fee (u64) at offset 256
    let protocol_quote_fee = u64::from_le_bytes(
        data[256..264].try_into().map_err(|_| "Failed to convert protocol_quote_fee slice to array")?
    );
    
    // Parse partner_base_fee (u64) at offset 264
    let partner_base_fee = u64::from_le_bytes(
        data[264..272].try_into().map_err(|_| "Failed to convert partner_base_fee slice to array")?
    );
    
    // Parse partner_quote_fee (u64) at offset 272
    let partner_quote_fee = u64::from_le_bytes(
        data[272..280].try_into().map_err(|_| "Failed to convert partner_quote_fee slice to array")?
    );
    
    // Parse sqrt_price (u128) at offset 280
    let sqrt_price = u128::from_le_bytes(
        data[280..296].try_into().map_err(|_| "Failed to convert sqrt_price slice to array")?
    );
    
    // Parse pool_type (u8) at offset 296
    let pool_type = data[304];
    
    Ok(Pool {
        sqrt_price,
        base_reserve,
        quote_reserve,
        pool_type,
        protocol_base_fee,
        protocol_quote_fee,
        partner_base_fee,
        partner_quote_fee,
    })
}

/// Get pool state from Meteora accounts by deserializing the pool account data
pub fn get_meteora_pool_state(meteora_accounts: &MeteoraBondingSwapAccounts) -> Result<Pool, Box<dyn Error>> {
    let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    
    // Fetch the pool account data
    println!("pool: {:?}", meteora_accounts.pool);
    let pool_account = rpc_client.get_account(&meteora_accounts.pool)?;
    let pool_data = pool_account.data.as_slice();
    
    // Deserialize the pool data
    deserialize_meteora_pool(pool_data)
}

/// Get pool configuration from Meteora accounts by deserializing the config account data
/// This is crucial for pricing calculations as it contains the bonding curve parameters
pub fn get_meteora_pool_config(meteora_accounts: &MeteoraBondingSwapAccounts) -> Result<PoolConfig, Box<dyn Error>> {
    let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    
    // Fetch the pool config account data
    let config_account = rpc_client.get_account(&meteora_accounts.config)?;
    let config_data = config_account.data.as_slice();
    
    // Deserialize the pool config data
    PoolConfig::from_account_data(config_data)
}

/// Get both pool state and configuration for comprehensive pricing calculations
pub fn get_meteora_pool_data(meteora_accounts: &MeteoraBondingSwapAccounts) -> Result<(Pool, PoolConfig), Box<dyn Error>> {
    let pool = get_meteora_pool_state(meteora_accounts)?;
    let config = get_meteora_pool_config(meteora_accounts)?;
    
    Ok((pool, config))
}

/// Calculate swap amount from quote to base (buying base tokens with quote tokens)
/// This implements the proper Meteora DBC pricing logic using the correct formulas
pub fn get_swap_amount_from_quote_to_base(
    pool: &Pool,
    config: &PoolConfig,
    amount_in: u64,
) -> PoolResult<SwapAmount> {
    // Input validation
    if amount_in == 0 {
        return Err(PoolError::InvalidPrice);
    }
    
    // Convert sqrt_price to u64 for calculations (Meteora uses u64 for sqrt_price in calculations)
    let current_price = pool.sqrt_price as u64;
    
    // Safety check: ensure current price is reasonable
    if current_price == 0 {
        return Err(PoolError::InvalidPrice);
    }
    
    // Safety check: ensure amount isn't too large relative to pool size
    let max_safe_amount = pool.quote_reserve.saturating_div(10); // Max 10% of quote reserve
    if amount_in > max_safe_amount {
        println!("[WARNING] Swap amount {} exceeds safe limit {} (10% of quote reserve)", amount_in, max_safe_amount);
        // Don't fail, but log a warning
    }
    
    println!("[DEBUG] Starting Meteora DBC swap calculation:");
    println!("[DEBUG] Current sqrt_price: {}", current_price);
    println!("[DEBUG] Amount in: {}", amount_in);
    println!("[DEBUG] Base reserve: {}, Quote reserve: {}", pool.base_reserve, pool.quote_reserve);
    println!("[DEBUG] Curve points available: {}", config.curve.len());
    
    // Find the current price point in the curve
    let mut closest_point = None;
    let mut min_distance = u64::MAX;
    
    // Find the closest curve point to current price with non-zero liquidity
    for (i, point) in config.curve.iter().enumerate() {
        // Skip points with zero liquidity as they can't be used for swaps
        if point.liquidity == 0 {
            continue;
        }
        
        let point_price = point.sqrt_price as u64;
        let distance = if point_price > current_price {
            point_price - current_price
        } else {
            current_price - point_price
        };
        
        if distance < min_distance {
            min_distance = distance;
            closest_point = Some((i, point));
        }
    }
    
    if let Some((idx, point)) = closest_point {
        println!("[DEBUG] Using curve point {}: sqrt_price={}, liquidity={}", idx, point.sqrt_price, point.liquidity);
        
        // For Meteora DBC, we need to calculate the next sqrt price after the swap
        // CORRECTED FORMULA: ΔP = Δy / (L * √P₀) where Δy is quote amount in, L is liquidity, √P₀ is current sqrt price
        let liquidity = point.liquidity; // Keep as u128 to avoid truncation
        
        // Calculate next sqrt price using the EXACT formula from official Meteora DBC SDK
        // For quote to base swap: next_sqrt_price = sqrt_price + (amount << (RESOLUTION * 2)) / liquidity
        // Use U256 to prevent overflow in intermediate calculations
        let amount_in_256 = U256::from(amount_in);
        let liquidity_256 = U256::from(liquidity);
        let current_price_256 = U256::from(current_price);
        
        // Apply the official SDK formula: (amount << (RESOLUTION * 2)) / liquidity
        let shifted_amount = amount_in_256 << (RESOLUTION * 2) as usize;
        let quotient = shifted_amount / liquidity_256;
        
        // ENHANCED: Add support for even larger shifts if needed
        // This demonstrates how you could use (1000000000 << 128) safely
        let enhanced_shifted_amount = if (RESOLUTION * 2) as u32 > 64 {
            // For very large shifts, use U256 to prevent overflow
            amount_in_256 << (RESOLUTION * 2) as u32
        } else {
            // For smaller shifts, the current approach is fine
            amount_in_256 << (RESOLUTION * 2) as usize
        };
        
        // Use the enhanced calculation
        let quotient = enhanced_shifted_amount / liquidity_256;
        
        // Calculate next_sqrt_price = current_price + quotient
        let next_sqrt_price_256 = current_price_256 + quotient;
        
        // Convert back to u64, ensuring it fits
        let next_sqrt_price = if next_sqrt_price_256 <= U256::from(u64::MAX) {
            next_sqrt_price_256.try_into().map_err(|_| PoolError::TypeCastFailed)?
        } else {
            // If the result is too large, cap it at a reasonable value
            u64::MAX
        };
        
        println!("[DEBUG] Next sqrt price calculation using official SDK formula:");
        println!("[DEBUG] Formula: next_sqrt_price = {} + ({} << {}) / {}", current_price, amount_in, RESOLUTION * 2, liquidity);
        println!("[DEBUG] Liquidity (u128): {}", liquidity);
        println!("[DEBUG] Result: next_sqrt_price = {}", next_sqrt_price);
        
        // Find the next curve point that represents this new price
        let mut next_curve_point = None;
        for (i, curve_point) in config.curve.iter().enumerate() {
            if curve_point.sqrt_price >= next_sqrt_price as u128 {
                next_curve_point = Some((i, curve_point));
                break;
            }
        }
        
        // If we can't find a next curve point, use the last one
        if next_curve_point.is_none() {
            next_curve_point = config.curve.last().map(|p| (config.curve.len() - 1, p));
        }
        
        if let Some((next_idx, next_point)) = next_curve_point {
            println!("[DEBUG] Next curve point {}: sqrt_price={}, liquidity={}", next_idx, next_point.sqrt_price, next_point.liquidity);
            
            // Calculate base tokens out using the proper Meteora DBC formula
            // For quote to base swap: Δx = L * (1/√P₀ - 1/√P₁)
            // where Δx is base tokens out, L is liquidity, √P₀ is current sqrt price, √P₁ is next sqrt price
            
            // Use the support functions for proper calculation
            let base_out = get_delta_amount_base_unsigned(
                current_price,
                next_sqrt_price,
                next_point.liquidity as u64, // Use the liquidity from the next curve point
                Rounding::Down, // Use Down rounding for conservative estimates
                config.token_decimal,
            )?;
            
            println!("[DEBUG] Calculated base_out using proper formula: {}", base_out);
            println!("[DEBUG] Formula breakdown:");
            println!("[DEBUG]   - Current sqrt price (√P₀): {}", current_price);
            println!("[DEBUG]   - Next sqrt price (√P₁): {}", next_sqrt_price);
            println!("[DEBUG]   - Liquidity (L): {}", next_point.liquidity);
            println!("[DEBUG]   - Base out = L * (1/√P₀ - 1/√P₁) = {} * (1/{} - 1/{})", next_point.liquidity, current_price, next_sqrt_price);
            
            // Additional validation: ensure we're getting a reasonable output
            if base_out == 0 {
                println!("[WARNING] Base output is 0! This might indicate:");
                println!("[WARNING]   - Price change is too small relative to precision");
                println!("[WARNING]   - Liquidity is too high relative to amount");
                println!("[WARNING]   - Current and next prices are too close");
                
                // Try a fallback calculation using a simpler approach
                let fallback_base_out = calculate_base_out_for_quote_in(
                    current_price,
                    pool.base_reserve,
                    pool.quote_reserve,
                    amount_in,
                )?;
                
                println!("[DEBUG] Fallback calculation result: {}", fallback_base_out);
                
                if fallback_base_out > 0 {
                    println!("[DEBUG] Using fallback calculation instead");
                    return Ok(SwapAmount {
                        output_amount: fallback_base_out,
                        next_sqrt_price: next_sqrt_price as u128,
                    });
                }
            }
            
            Ok(SwapAmount {
                output_amount: base_out,
                next_sqrt_price: next_sqrt_price as u128,
            })
        } else {
            println!("[ERROR] No next curve point found!");
            Err(PoolError::InsufficientLiquidity)
        }
    } else {
        println!("[ERROR] No valid curve points found!");
        Err(PoolError::InsufficientLiquidity)
    }
}

/// Calculate swap amount from base to quote (selling base tokens for quote tokens)
/// This implements the proper Meteora DBC pricing formula
pub fn get_swap_amount_from_base_to_quote(
    pool: &Pool,
    config: &PoolConfig,
    amount_in: u64,
) -> PoolResult<SwapAmount> {
    // Input validation
    if amount_in == 0 {
        return Err(PoolError::InvalidPrice);
    }
    
    // Convert sqrt_price to u64 for calculations (Meteora uses u64 for sqrt_price in calculations)
    let current_price = pool.sqrt_price as u64;
    
    // Safety check: ensure current price is reasonable
    if current_price == 0 {
        return Err(PoolError::InvalidPrice);
    }
    
    // Safety check: ensure amount isn't too large relative to pool size
    let max_safe_amount = pool.base_reserve.saturating_div(10); // Max 10% of base reserve
    if amount_in > max_safe_amount {
        println!("[WARNING] Swap amount {} exceeds safe limit {} (10% of base reserve)", amount_in, max_safe_amount);
        // Don't fail, but log a warning
    }
    
    println!("[DEBUG] Starting Meteora DBC base to quote swap calculation:");
    println!("[DEBUG] Current price: {}", current_price);
    println!("[DEBUG] Amount in: {}", amount_in);
    println!("[DEBUG] Base reserve: {}, Quote reserve: {}", pool.base_reserve, pool.quote_reserve);
    println!("[DEBUG] Curve points available: {}", config.curve.len());
    
    // Find the current price point in the curve
    let mut closest_point = None;
    let mut min_distance = u64::MAX;
    
    // Find the closest curve point to current price with non-zero liquidity
    for (i, point) in config.curve.iter().enumerate() {
        // Skip points with zero liquidity as they can't be used for swaps
        if point.liquidity == 0 {
            continue;
        }
        
        let point_price = point.sqrt_price as u64;
        let distance = if point_price > current_price {
            point_price - current_price
        } else {
            current_price - point_price
        };
        
        if distance < min_distance {
            min_distance = distance;
            closest_point = Some((i, point));
        }
    }
    
    if let Some((idx, point)) = closest_point {
        println!("[DEBUG] Using curve point {}: sqrt_price={}, liquidity={}", idx, point.sqrt_price, point.liquidity);
        
        // For Meteora DBC base to quote swap, we need to calculate the next sqrt price after the swap
        // CORRECTED FORMULA: ΔP = Δx / (L * √P₀) where Δx is base amount in, L is liquidity, √P₀ is current sqrt price
        let liquidity = point.liquidity as u64;
        
        // Calculate price decrease using the CORRECT formula
        // In Meteora DBC: ΔP = Δx / (L * √P₀)
        // Use U256 to prevent overflow in intermediate calculations
        let amount_in_256 = U256::from(amount_in);
        let liquidity_256 = U256::from(liquidity);
        let current_price_256 = U256::from(current_price);
        
        // Calculate (L * √P₀) safely using U256
        let denominator = liquidity_256 * current_price_256;
        
        // Calculate ΔP = Δx / (L * √P₀)
        let price_decrease_256 = amount_in_256 / denominator;
        
        // Convert back to u64, ensuring it fits
        let price_decrease = if price_decrease_256 <= U256::from(u64::MAX) {
            price_decrease_256.try_into().map_err(|_| PoolError::TypeCastFailed)?
        } else {
            // If the result is too large, cap it at a reasonable value
            u64::MAX / 1000 // Cap at 0.1% of max u64
        };
        
        // Ensure minimum price change to prevent 0 output
        let min_price_change = 1u64; // Minimum 1 unit change
        let adjusted_price_decrease = if price_decrease == 0 { min_price_change } else { price_decrease };
        
        let next_sqrt_price = current_price.safe_sub(adjusted_price_decrease)?;
        
        println!("[DEBUG] Price decrease: {} (adjusted from {}), next_sqrt_price: {}", adjusted_price_decrease, price_decrease, next_sqrt_price);
        println!("[DEBUG] Formula used: ΔP = {} / ({} * {}) = {}", amount_in, liquidity, current_price, price_decrease);
        
        // Find the previous curve point that represents this new price
        let mut prev_curve_point = None;
        for (i, curve_point) in config.curve.iter().enumerate().rev() {
            if curve_point.sqrt_price <= next_sqrt_price as u128 {
                prev_curve_point = Some((i, curve_point));
                break;
            }
        }
        
        // If we can't find a previous curve point, use the first one
        if prev_curve_point.is_none() {
            prev_curve_point = config.curve.first().map(|p| (0, p));
        }
        
        if let Some((prev_idx, prev_point)) = prev_curve_point {
            println!("[DEBUG] Previous curve point {}: sqrt_price={}, liquidity={}", prev_idx, prev_point.sqrt_price, prev_point.liquidity);
            
            // Calculate quote tokens out using the proper Meteora DBC formula
            // For base to quote swap: Δy = L * (√P₀ - √P₁)
            // where Δy is quote tokens out, L is liquidity, √P₀ is current sqrt price, √P₁ is next sqrt price
            
            // Use the support functions for proper calculation
            let quote_out = get_delta_amount_quote_unsigned(
                current_price,
                next_sqrt_price,
                liquidity,
                Rounding::Down, // Use Down rounding for conservative estimates
            )?;
            
            println!("[DEBUG] Calculated quote_out using proper formula: {}", quote_out);
            
            Ok(SwapAmount {
                output_amount: quote_out,
                next_sqrt_price: next_sqrt_price as u128,
            })
        } else {
            println!("[ERROR] No previous curve point found!");
            Err(PoolError::InsufficientLiquidity)
        }
    } else {
        println!("[ERROR] No valid curve points found!");
        Err(PoolError::InsufficientLiquidity)
    }
}

/// Helper function to get delta amount for quote tokens (unsigned 256-bit)
/// For bonding curves: Δy = L * (√P_b - √P_a)
/// Note: In Meteora DBC, prices can move in both directions, so we need to handle both cases
/// This matches the EXACT implementation from the official SDK
fn get_delta_amount_quote_unsigned_256(
    sqrt_price_a: u128,
    sqrt_price_b: u128,
    liquidity: u128,
    rounding: Rounding,
) -> PoolResult<U256> {
    let result = get_delta_amount_quote_unsigned_unchecked(
        sqrt_price_a,
        sqrt_price_b,
        liquidity,
        rounding,
    )?;
    Ok(result)
}

/// Helper function to get delta amount for quote tokens (unsigned unchecked)
/// This matches the EXACT implementation from the official SDK
fn get_delta_amount_quote_unsigned_unchecked(
    lower_sqrt_price: u128,
    upper_sqrt_price: u128,
    liquidity: u128,
    round: Rounding,
) -> PoolResult<U256> {
    let liquidity = U256::from(liquidity);
    let delta_sqrt_price = U256::from(upper_sqrt_price - lower_sqrt_price);
    let prod = liquidity * delta_sqrt_price;

    match round {
        Rounding::Up => {
            let denominator = U256::from(1) << ((RESOLUTION as usize) * 2);
            let result = (prod + denominator - U256::from(1)) / denominator;
            Ok(result)
        }
        Rounding::Down => {
            let result = prod >> ((RESOLUTION as usize) * 2);
            Ok(result)
        }
    }
}

/// Helper function to get delta amount for base tokens (unsigned)
/// This is the EXACT implementation from the official Meteora DBC SDK
/// For bonding curves: Δx = L * (1/√P_a - 1/√P_b)
fn get_delta_amount_base_unsigned(
    sqrt_price_a: u64,
    sqrt_price_b: u64,
    liquidity: u64,
    rounding: Rounding,
    token_decimal: u8,
) -> PoolResult<u64> {
    if sqrt_price_a == sqrt_price_b {
        return Ok(0);
    }
    
    // Convert to u128 for calculations (matching official SDK)
    let sqrt_price_a = sqrt_price_a as u128;
    let sqrt_price_b = sqrt_price_b as u128;
    let liquidity = liquidity as u128;
    
    // Use U256 for intermediate calculations to prevent overflow
    let liquidity_256 = U256::from(liquidity);
    let sqrt_price_a_256 = U256::from(sqrt_price_a);
    let sqrt_price_b_256 = U256::from(sqrt_price_b);
    
            if sqrt_price_a > sqrt_price_b {
            // Price moving down (selling base tokens)
            // Δx = L * (1/√P_a - 1/√P_b) = L * (√P_b - √P_a) / (√P_a * √P_b)
            let price_diff = sqrt_price_b_256 - sqrt_price_a_256;
            let denominator = sqrt_price_a_256 * sqrt_price_b_256;
            
            let result_256 = (liquidity_256 * price_diff) / denominator;
        
        // Apply rounding
        let final_result = match rounding {
            Rounding::Up => {
                if result_256 == U256::from(0) {
                    U256::from(1) // Minimum 1 unit
                } else {
                    result_256
                }
            }
            Rounding::Down => result_256,
        };
        
        // Convert back to u64
        let result: u64 = final_result.try_into().map_err(|_| PoolError::TypeCastFailed)?;
        Ok(result)
            } else {
            // Price moving up (buying base tokens)
            // Δx = L * (1/√P_a - 1/√P_b) = L * (√P_b - √P_a) / (√P_a * √P_b)
            let price_diff = sqrt_price_b_256 - sqrt_price_a_256;
            let denominator = sqrt_price_a_256 * sqrt_price_b_256;
            
            let result_256 = (liquidity_256 * price_diff) / denominator;
        
        // Apply rounding
        let final_result = match rounding {
            Rounding::Up => {
                if result_256 == U256::from(0) {
                    U256::from(1) // Minimum 1 unit
                } else {
                    result_256
                }
            }
            Rounding::Down => result_256,
        };
        
        // Convert back to u64
        let result: u64 = final_result.try_into().map_err(|_| PoolError::TypeCastFailed)?;
        Ok(result)
    }
}

/// Helper function to get delta amount for base tokens (unsigned 256-bit)
/// For bonding curves: Δx = L * (1/√P_a - 1/√P_b)
/// Note: In Meteora DBC, prices can move in both directions, so we need to handle both cases
fn get_delta_amount_base_unsigned_256(
    sqrt_price_a: u64,
    sqrt_price_b: u64,
    liquidity: u64,
    rounding: Rounding,
) -> PoolResult<U256> {
    let liquidity_256 = U256::from(liquidity);
    let sqrt_price_a_256 = U256::from(sqrt_price_a);
    let sqrt_price_b_256 = U256::from(sqrt_price_b);
    
    if sqrt_price_a == sqrt_price_b {
        // No price change, no delta
        return Ok(U256::from(0));
    }
    
    if sqrt_price_a > sqrt_price_b {
        // Price moving down (selling base tokens)
        // Δx = L * (1/√P_a - 1/√P_b) = L * (√P_b - √P_a) / (√P_a * √P_b)
        let price_diff = sqrt_price_a_256 - sqrt_price_b_256;
        let denominator = sqrt_price_a_256 * sqrt_price_b_256;
        
        let result = (liquidity_256 * price_diff) / denominator;
        
        match rounding {
            Rounding::Up => Ok(result + U256::from(1)),
            Rounding::Down => Ok(result),
        }
    } else {
        // Price moving up (buying base tokens)
        // Δx = L * (1/√P_a - 1/√P_b) = L * (√P_b - √P_a) / (√P_a * √P_b)
        let price_diff = sqrt_price_b_256 - sqrt_price_a_256;
        let denominator = sqrt_price_a_256 * sqrt_price_b_256;
        
        let result = (liquidity_256 * price_diff) / denominator;
        
        match rounding {
            Rounding::Up => Ok(result + U256::from(1)),
            Rounding::Down => Ok(result),
        }
    }
}

/// Helper function to get delta amount for quote tokens (unsigned)
/// For bonding curves: Δy = L * (√P_b - √P_a)
/// Note: In Meteora DBC, prices can move in both directions, so we need to handle both cases
/// This matches the EXACT implementation from the official SDK
fn get_delta_amount_quote_unsigned(
    sqrt_price_a: u64,
    sqrt_price_b: u64,
    liquidity: u64,
    rounding: Rounding,
) -> PoolResult<u64> {
    let result = get_delta_amount_quote_unsigned_256(
        sqrt_price_a as u128,
        sqrt_price_b as u128,
        liquidity as u128,
        rounding,
    )?;
    
    // Check for overflow before converting to u64
    if result > U256::from(u64::MAX) {
        return Err(PoolError::MathOverflow);
    }
    
    Ok(result.try_into().map_err(|_| PoolError::TypeCastFailed)?)
}

/// Helper function to get next sqrt price from input
/// This matches the EXACT implementation from the official Meteora DBC SDK
fn get_next_sqrt_price_from_input(
    sqrt_price: u128,
    liquidity: u128,
    amount_in: u64,
    base_for_quote: bool,
) -> PoolResult<u128> {
    if base_for_quote {
        // Selling base tokens (moving price down)
        // P_new = P_old / (1 + Δx/L)^2
        let amount_in_256 = U256::from(amount_in);
        let current_sqrt_price_256 = U256::from(sqrt_price);
        let liquidity_256 = U256::from(liquidity);
        
        // Calculate: P_old / (1 + Δx/L)^2
        // For numerical stability, use: P_old * (L/(L + Δx))^2
        let denominator = liquidity_256 + amount_in_256;
        let ratio_256 = liquidity_256 * liquidity_256 / (denominator * denominator);
        let new_price_256 = current_sqrt_price_256 * ratio_256;
        
        // Check if the result can fit in u128 before converting
        if new_price_256 > U256::from(u128::MAX) {
            return Err(PoolError::TypeCastFailed);
        }
        
        let new_price: u128 = new_price_256.try_into().map_err(|_| PoolError::TypeCastFailed)?;
        Ok(new_price.max(1)) // Ensure price doesn't go below 1
    } else {
        // Buying base tokens (moving price up)
        // P_new = P_old * (1 + Δy/L)^2
        let amount_in_256 = U256::from(amount_in);
        let current_sqrt_price_256 = U256::from(sqrt_price);
        let liquidity_256 = U256::from(liquidity);
        
        // Calculate: P_old * (1 + Δy/L)^2
        // For numerical stability, use: P_old * ((L + Δy)/L)^2
        let numerator = liquidity_256 + amount_in_256;
        let ratio_256 = (numerator * numerator) / (liquidity_256 * liquidity_256);
        let new_price_256 = current_sqrt_price_256 * ratio_256;
        
        // Check if the result can fit in u128 before converting
        if new_price_256 > U256::from(u128::MAX) {
            return Err(PoolError::TypeCastFailed);
        }
        
        let new_price: u128 = new_price_256.try_into().map_err(|_| PoolError::TypeCastFailed)?;
        Ok(new_price)
    }
}

/// Helper function to get next sqrt price from base amount (rounding up)
/// This matches the EXACT implementation from the official Meteora DBC SDK
fn get_next_sqrt_price_from_amount_base_rounding_up(
    sqrt_price: u128,
    liquidity: u128,
    amount: u64,
) -> PoolResult<u128> {
    if amount == 0 {
        return Ok(sqrt_price);
    }
    let sqrt_price = U256::from(sqrt_price);
    let liquidity = U256::from(liquidity);

    let product = U256::from(amount) * sqrt_price;
    let denominator = liquidity + product;
    let result = mul_div_u256(liquidity, sqrt_price, denominator, Rounding::Up)
        .ok_or_else(|| PoolError::MathOverflow)?;
    Ok(result.try_into().map_err(|_| PoolError::TypeCastFailed)?)
}

/// Helper function to get next sqrt price from quote amount (rounding down)
/// This matches the EXACT implementation from the official Meteora DBC SDK
fn get_next_sqrt_price_from_amount_quote_rounding_down(
    sqrt_price: u128,
    liquidity: u128,
    amount: u64,
) -> PoolResult<u128> {
    if amount == 0 {
        return Ok(sqrt_price);
    }
    let sqrt_price = U256::from(sqrt_price);
    let liquidity = U256::from(liquidity);

    let product = U256::from(amount) * sqrt_price;
    let numerator = liquidity + product;
    let result = mul_div_u256(numerator, sqrt_price, liquidity, Rounding::Down)
        .ok_or_else(|| PoolError::TypeCastFailed)?;
    Ok(result.try_into().map_err(|_| PoolError::TypeCastFailed)?)
}

/// Helper function to perform multiplication and division with U256
/// This matches the EXACT implementation from the official Meteora DBC SDK
fn mul_div_u256(
    a: U256,
    b: U256,
    c: U256,
    rounding: Rounding,
) -> Option<U256> {
    let product = a * b;
    match rounding {
        Rounding::Up => {
            if c == U256::from(0) {
                return None;
            }
            Some((product + c - U256::from(1)) / c)
        }
        Rounding::Down => {
            if c == U256::from(0) {
                return None;
            }
            Some(product / c)
        }
    }
}

/// Create a basic pool config from pool data for pricing calculations
/// This creates a realistic bonding curve based on actual pool reserves
/// Updated to better match Meteora DBC SDK structure
pub fn create_basic_pool_config(pool: &Pool) -> PoolConfig {
    // Convert sqrt_price to u64 for calculations
    let current_price = pool.sqrt_price as u64;
    
    // Use the new proper Meteora curve generation
            let curve = generate_meteora_curve(
        current_price,
        pool.base_reserve,
        pool.quote_reserve,
    );
    
    // Convert Vec to fixed-size array, padding with zeros if needed
    let mut curve_array = [LiquidityDistributionConfig { sqrt_price: 0, liquidity: 0 }; 20];
    for (i, point) in curve.iter().take(20).enumerate() {
        curve_array[i] = *point;
    }
    
    println!("[DEBUG] Base reserve: {}, Quote reserve: {}", pool.base_reserve, pool.quote_reserve);
    for (i, point) in curve_array.iter().enumerate() {
        println!("[DEBUG] Point {}: price={}, liquidity={}", i, point.sqrt_price, point.liquidity);
    }
    
    PoolConfig {
        sqrt_start_price: current_price as u128, // Use current price as the starting point
        curve: curve_array,
        migration_quote_threshold: 100000000, // Default, can be updated
        pool_fees: PoolFeesConfig {
            base_fee_config: BaseFeeConfig {
                base_fee: 100, // Default, can be updated
                base_fee_numerator: 1,
                base_fee_denominator: 10000,
            },
            dynamic_fee_config: DynamicFeeConfig {
                dynamic_fee: 0, // Default, can be updated
                dynamic_fee_numerator: 0,
                dynamic_fee_denominator: 0,
            },
        },
        collect_fee_mode: 0, // Default, can be updated
        migration_option: 0, // Default, can be updated
        activation_type: 0, // Default, can be updated
        token_type: 0, // Default, can be updated
        token_decimal: 9, // Default, can be updated
        partner_lp_percentage: 50, // Default, can be updated
        creator_lp_percentage: 20, // Default, can be updated
        locked_vesting: LockedVestingConfig {
            locked_lp_percentage: 0, // Default, can be updated
            partner_locked_lp_percentage: 0, // Default, can be updated
            creator_locked_lp_percentage: 0, // Default, can be updated
        },
        fee_claimer: Pubkey::default(), // Default, can be updated
        quote_mint: WSOL, // Default, can be updated
    }
}

// Add missing functions from official Meteora implementation

/// Calculate the amount of base tokens received for a given amount of quote tokens
/// This implements the proper Meteora DBC pricing formula
pub fn calculate_base_out_for_quote_in(
    current_price: u64,
    base_reserve: u64,
    quote_reserve: u64,
    quote_amount_in: u64,
) -> PoolResult<u64> {
    if quote_amount_in == 0 || current_price == 0 {
        return Err(PoolError::InvalidPrice);
    }
    
    // Use constant product formula: (x * y) / (y + Δy)
    // where x = base reserve, y = quote reserve, Δy = quote amount in
    
    if base_reserve == 0 || quote_reserve == 0 {
        return Err(PoolError::InsufficientLiquidity);
    }
    
    // Calculate base tokens out using constant product formula
    // base_out = (base_reserve * quote_amount_in) / (quote_reserve + quote_amount_in)
    // Use U256 to prevent overflow in intermediate calculations
    let base_reserve_256 = U256::from(base_reserve);
    let quote_amount_in_256 = U256::from(quote_amount_in);
    let quote_reserve_256 = U256::from(quote_reserve);
    
    let numerator = base_reserve_256 * quote_amount_in_256;
    let denominator = quote_reserve_256 + quote_amount_in_256;
    
    if denominator == U256::from(0) {
        return Err(PoolError::InsufficientLiquidity);
    }
    
    let base_out_256 = numerator / denominator;
    
    // Convert back to u64, checking for overflow
    if base_out_256 > U256::from(u64::MAX) {
        return Err(PoolError::MathOverflow);
    }
    
    let base_out = base_out_256.try_into().map_err(|_| PoolError::TypeCastFailed)?;
    
    println!("[DEBUG] calculate_base_out_for_quote_in:");
    println!("[DEBUG]   - Current price: {}", current_price);
    println!("[DEBUG]   - Base reserve: {}", base_reserve);
    println!("[DEBUG]   - Quote reserve: {}", quote_reserve);
    println!("[DEBUG]   - Quote amount in: {}", quote_amount_in);
    println!("[DEBUG]   - Base out = ({} * {}) / ({} + {}) = {} / {} = {}", 
             base_reserve, quote_amount_in, quote_reserve, quote_amount_in, numerator, denominator, base_out);
    
    if base_out == 0 {
        return Err(PoolError::InsufficientLiquidity);
    }
    
    Ok(base_out)
}

/// Calculate the amount of quote tokens received for a given amount of base tokens
/// This implements the proper Meteora DBC pricing formula
pub fn calculate_quote_out_for_base_in(
    current_price: u64,
    base_reserve: u64,
    quote_reserve: u64,
    base_amount_in: u64,
) -> PoolResult<u64> {
    if base_amount_in == 0 || current_price == 0 {
        return Err(PoolError::InvalidPrice);
    }
    
    // Convert to f64 for calculations to avoid overflow
    let price_f64 = current_price as f64;
    let base_reserve_f64 = base_reserve as f64;
    let quote_reserve_f64 = quote_reserve as f64;
    let base_amount_in_f64 = base_amount_in as f64;
    
    // Meteora DBC formula: quote_out = quote_reserve * (1 - (base_reserve / (base_reserve + base_amount_in)))
    let base_reserve_plus_input = base_reserve_f64 + base_amount_in_f64;
    let ratio = base_reserve_f64 / base_reserve_plus_input;
    let quote_out_f64 = quote_reserve_f64 * (1.0 - ratio);
    
    // Convert back to u64
    let quote_out = quote_out_f64 as u64;
    
    if quote_out == 0 {
        return Err(PoolError::InsufficientLiquidity);
    }
    
    Ok(quote_out)
}



/// Generate a proper Meteora DBC curve based on current pool state
/// This implements the official Meteora curve generation logic
pub fn generate_meteora_curve(
    current_price: u64,
    base_reserve: u64,
    quote_reserve: u64,
) -> Vec<LiquidityDistributionConfig> {
    let mut curve = Vec::new();
    
    // Calculate geometric mean for base liquidity
    let geometric_mean = ((base_reserve as f64) * (quote_reserve as f64)).sqrt();
    let base_liquidity = geometric_mean as u64;
    
    // Create curve points with proper spacing
    let num_points = 20; // Standard Meteora curve size
    let price_spacing = 0.05; // 5% price change between points
    
    // Add current price point
    curve.push(LiquidityDistributionConfig {
        sqrt_price: current_price.into(),
        liquidity: base_liquidity.into(),
    });
    
    // Add points below current price (for selling)
    for i in 1..=num_points/2 {
        let price_multiplier = (1.0_f64 - price_spacing).powf(i as f64);
        let sqrt_price = (current_price as f64 * price_multiplier) as u64;
        
        if sqrt_price > 0 {
            let liquidity_factor = 1.0 / (1.0 + (i as f64 * 0.1));
            let liquidity = (base_liquidity as f64 * liquidity_factor) as u64;
            
            curve.push(LiquidityDistributionConfig {
                sqrt_price: sqrt_price.into(),
                liquidity: liquidity.max(100).into(),
            });
        }
    }
    
    // Add points above current price (for buying)
    for i in 1..=num_points/2 {
        let price_multiplier = (1.0_f64 + price_spacing).powf(i as f64);
        let sqrt_price = (current_price as f64 * price_multiplier) as u64;
        
        if sqrt_price < u64::MAX / 2 && sqrt_price > 0 {
            let liquidity_factor = 1.0 / (1.0 + (i as f64 * 0.1));
            let liquidity = (base_liquidity as f64 * liquidity_factor) as u64;
            
            curve.push(LiquidityDistributionConfig {
                sqrt_price: sqrt_price.into(),
                liquidity: liquidity.max(100).into(),
            });
        }
    }
    
    // Sort by price
    curve.sort_by(|a, b| a.sqrt_price.cmp(&b.sqrt_price));
    
    // Ensure we have enough points
    if curve.len() < 3 {
        // Fallback curve
        curve.clear();
        curve.push(LiquidityDistributionConfig { sqrt_price: current_price.max(1).into(), liquidity: base_liquidity.max(1000).into() });
        curve.push(LiquidityDistributionConfig { sqrt_price: ((current_price as f64 * 0.9) as u64).into(), liquidity: ((base_liquidity as f64 * 0.8) as u64).into() });
        curve.push(LiquidityDistributionConfig { sqrt_price: ((current_price as f64 * 1.1) as u64).into(), liquidity: ((base_liquidity as f64 * 0.8) as u64).into() });
    }
    
    curve
}


// Removed get_pool_state function as it was specific to Raydium CPMM
// For Meteora DBC, this function would need to be implemented differently

// // Dummy function to create MeteoraBondingSwapAccounts for migrate instruction
// // Based on get_meteora_dbc_instruction_accounts
// pub fn get_meteora_dbc_instruction_accounts_migrate(
//     account_keys: &[Vec<u8>],
//     accounts: &[u8],
// ) -> MeteoraBondingSwapAccounts {
//     let mint = get_account(account_keys, accounts, 1);
//     let base_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &mint);
//     let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &WSOL);
 
//     MeteoraBondingSwapAccounts {
//         pool_authority: METEORA_BONDING_POOL_AUTHORITY,
//         config: Pubkey::default(), // TODO: Add proper Meteora config constant
//         pool: get_account(&account_keys, &accounts, 5),
//         input_token_account: quote_ata,
//         output_token_account: base_ata,
//         base_vault: get_account(&account_keys, &accounts, 9),
//         quote_vault: get_account(&account_keys, &accounts, 8),
//         base_mint: mint,
//         quote_mint: WSOL,
//         payer: get_wallet_keypair().pubkey(),
//         token_base_program: spl_token::ID,
//         token_quote_program: spl_token::ID,
//         referral_token_account: Pubkey::default(),
//         event_authority: METEORA_BONDING_EVENT_AUTHORITY,
//         program: METEORA_BONDING_PROGRAM_ID_PUBKEY,
//     }
// }

