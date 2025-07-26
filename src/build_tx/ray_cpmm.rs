// ray_launch.rs
// Raydium Launchpad Curve: buyExactIn translation from TypeScript to Rust
// Reference: https://github.com/raydium-io/raydium-sdk-V2/blob/master/src/raydium/launchpad/curve/curve.ts

use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use crate::constants::raydium_cpmm::RAYDIUM_CPMM_PROGRAM_ID_PUBKEY;
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
use crate::build_tx::utils::get_account;
use crate::constants::raydium_cpmm::RAYDIUM_CPMM_AUTHORITY;
use crate::constants::raydium_cpmm::RAYDIUM_CPMM_AMM_CONFIG;
use crate::constants::consts::WSOL;

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

#[derive(PartialEq, Copy, Clone, Debug, BorshDeserialize, BorshSerialize)]
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

impl Default for RaydiumCpmmPoolState {
    fn default() -> Self {
        Self {
            // initialize all fields with their default values
            // e.g., field1: Default::default(), field2: 0, ...
            amm_config: Pubkey::default(),
            pool_creator: Pubkey::default(),
            token_0_vault: Pubkey::default(),
            token_1_vault: Pubkey::default(),
            lp_mint: Pubkey::default(),
            token_0_mint: Pubkey::default(),
            token_1_mint: Pubkey::default(),
            token_0_program: Pubkey::default(),
            token_1_program: Pubkey::default(),
            observation_key: Pubkey::default(),
            auth_bump: 0,
            status: 0,
            lp_mint_decimals: 0,
            mint_0_decimals: 0,
            mint_1_decimals: 0,
            lp_supply: 0,
            protocol_fees_token_0: 0,
            protocol_fees_token_1: 0,
            fund_fees_token_0: 0,
            fund_fees_token_1: 0,
            open_time: 0,
            recent_epoch: 0,
            padding: [0; 31],
        }
    }
}

/// Struct containing all the account parameters for Ray CPMM swap instructions
#[derive(Debug, Clone, Default)]
pub struct RayCpmmSwapAccounts {
    pub wallet: Pubkey,
    pub authority: Pubkey,
    pub amm_config: Pubkey,
    pub pool_state: Pubkey,
    pub quote_ata: Pubkey,
    pub base_ata: Pubkey,
    pub token_0_vault: Pubkey,
    pub token_1_vault: Pubkey,
    pub token_0_program: Pubkey,
    pub token_1_program: Pubkey,
    pub token_0_mint: Pubkey,
    pub token_1_mint: Pubkey,
    pub observation_key: Pubkey,
}

impl RayCpmmSwapAccounts {
    /// Create a new instance with default values
    pub fn new() -> Self {
        Self {
            wallet: get_wallet_keypair().pubkey(),
            authority: RAYDIUM_CPMM_AUTHORITY,
            amm_config: Pubkey::default(),
            pool_state: Pubkey::default(),
            quote_ata: Pubkey::default(),
            base_ata: Pubkey::default(),
            token_0_vault: Pubkey::default(),
            token_1_vault: Pubkey::default(),
            token_0_program: Pubkey::default(),
            token_1_program: Pubkey::default(),
            token_0_mint: Pubkey::default(),
            token_1_mint: Pubkey::default(),
            observation_key: Pubkey::default(),
        }
    }

    /// Create from a RaydiumCpmmPoolState and additional parameters
    pub fn from_pool_state(
        pool_state: Pubkey,
        pool_detail: &RaydiumCpmmPoolState,
        wallet: Pubkey,
        base_ata: Pubkey,
        quote_ata: Pubkey,
    ) -> Self {
        Self {
            wallet,
            authority: RAYDIUM_CPMM_AUTHORITY,
            amm_config: pool_detail.amm_config,
            pool_state,
            quote_ata,
            base_ata,
            token_0_vault: pool_detail.token_0_vault,
            token_1_vault: pool_detail.token_1_vault,
            token_0_program: pool_detail.token_0_program,
            token_1_program: pool_detail.token_1_program,
            token_0_mint: pool_detail.token_0_mint,
            token_1_mint: pool_detail.token_1_mint,
            observation_key: pool_detail.observation_key,
        }
    }

    /// Convert to AccountMeta vector for buy direction
    pub fn to_buy_account_metas(&self) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(self.wallet, true),
            AccountMeta::new(self.authority, false),
            AccountMeta::new_readonly(self.amm_config, false),
            AccountMeta::new(self.pool_state, false),
            AccountMeta::new(self.quote_ata, false),
            AccountMeta::new(self.base_ata, false),
            AccountMeta::new(self.token_0_vault, false),
            AccountMeta::new(self.token_1_vault, false),
            AccountMeta::new_readonly(self.token_0_program, false),
            AccountMeta::new_readonly(self.token_1_program, false),
            AccountMeta::new_readonly(self.token_0_mint, false),
            AccountMeta::new_readonly(self.token_1_mint, false),
            AccountMeta::new(self.observation_key, false),
        ]
    }

    /// Convert to AccountMeta vector for sell direction
    pub fn to_sell_account_metas(&self) -> Vec<AccountMeta> {
        vec![
            AccountMeta::new(self.wallet, true),
            AccountMeta::new(self.authority, false),
            AccountMeta::new_readonly(self.amm_config, false),
            AccountMeta::new(self.pool_state, false),
            AccountMeta::new(self.base_ata, false),
            AccountMeta::new(self.quote_ata, false),
            AccountMeta::new(self.token_1_vault, false),
            AccountMeta::new(self.token_0_vault, false),
            AccountMeta::new_readonly(self.token_1_program, false),
            AccountMeta::new_readonly(self.token_0_program, false),
            AccountMeta::new_readonly(self.token_1_mint, false),
            AccountMeta::new_readonly(self.token_0_mint, false),
            AccountMeta::new(self.observation_key, false),
        ]
    }
}

/// Helper to get the discriminator for buy/sell
fn get_discriminator(direction: SwapDirection) -> [u8; 8] {
    match direction {
        SwapDirection::Buy => [55, 217, 98, 86, 163, 74, 180, 173],
        SwapDirection::Sell => [143, 190, 90, 218, 196, 30, 51, 222],
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

// pub fn build_ray_cpmm_buy_instruction(
//     // base_vault: Pubkey,
//     // quote_vault: Pubkey,
//     pool_state: Pubkey,
//     amount: u64,
//     slippage_basis_points: u64,
//     mint: Pubkey,
// ) -> (Instruction, u64, RaydiumCpmmPoolState) {
//     let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

//     let slippage_factor = 1.0+slippage_basis_points as f64 /10000.0;
//     let instruction: Instruction = Instruction{
//         program_id: Pubkey::new_unique(),
//         accounts: vec![],
//         data: vec![],
//     };
//     // let pool_state = match get_pool_accounts(mint, &rpc_client, [200u64], ray_cpmm_constants::RAY_CPMM_PROGRAM_ID) {
//     //     Some(pk) => pk,
//     //     None => {
//     //         eprintln!("Failed to find pool account for mint {}", mint);
//     //         return (instruction, 0);
//     //     }
//     // };
//     let pool_state_data = match rpc_client.get_account_data(&pool_state) {
//         Ok(data) => data,
//         Err(e) => {
//             eprintln!("Failed to fetch account data: {}", e);
//             return (instruction, 0, RaydiumCpmmPoolState::default());
//         }
//     };
//     if pool_state_data.iter().all(|&b| b == 0) {
//         println!("pool_state_data is all zeros!");
//         return (instruction, 0, RaydiumCpmmPoolState::default()); // or return an error, or handle as needed
//     }

//     let pool_ac_detail = RaydiumCpmmPoolState::deserialize(&mut &pool_state_data[8..]).unwrap();

//     let base_vault = pool_ac_detail.token_1_vault;
//     let quote_vault = pool_ac_detail.token_0_vault;
//     let (base_amount, quote_amount) = get_pool_vault_amount(base_vault, quote_vault).unwrap();

//     println!("base_amount: {}, quote_amount: {}", base_amount, quote_amount);
//     let token_quote_amount = get_constant_product_swap_amount(
//         SwapDirection::Buy,
//         base_amount,
//         quote_amount,
//         amount,
//         0,
//         0,
//     ).expect("Failed to calculate buy limit_quote_amount");
//     // println!("amount: {}, limit_quote_amount: {}, limit_quote_amount_slippage: {}", amount, limit_quote_amount, (limit_quote_amount as f64*slippage_factor) as u64);
//     let instruction = build_ray_cpmm_swap_instruction(pool_state, pool_ac_detail, SwapDirection::Buy, (amount as f64*slippage_factor) as u64, token_quote_amount);
//     println!("token_quote_amount: {}", token_quote_amount);
//     (instruction, token_quote_amount, pool_ac_detail)
// }

pub fn build_ray_cpmm_sell_instruction(
    // base_vault: Pubkey,
    // quote_vault: Pubkey,
    amount: u64,
    // slippage_basis_points: u64,
    ray_cpmm_accounts: &RayCpmmSwapAccounts,
) -> (Instruction) {
    // let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

    // let slippage_factor = 1.0-slippage_basis_points as f64 /10000.0;
    
    let instruction = build_ray_cpmm_swap_instruction(&ray_cpmm_accounts, SwapDirection::Sell, amount, 0);
    (instruction)
}

// pub fn build_ray_cpmm_sell_instruction_with_pool_state(
//     // base_vault: Pubkey,
//     // quote_vault: Pubkey,
//     pool_state: Pubkey,
//     amount: u64,
//     slippage_basis_points: u64,
//     mint: Pubkey,
// ) -> (Instruction) {
//     let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

//     let slippage_factor = 1.0-slippage_basis_points as f64 /10000.0;
//     let instruction: Instruction = Instruction{
//         program_id: Pubkey::new_unique(),
//         accounts: vec![],
//         data: vec![],
//     };

//     let pool_state_data = rpc_client.get_account_data(&pool_state).unwrap();
//     if pool_state_data.iter().all(|&b| b == 0) {
//         println!("pool_state_data is all zeros!");
//         return instruction; // or return an error, or handle as needed
//     }

//     let pool_ac_detail = RaydiumCpmmPoolState::deserialize(&mut &pool_state_data[8..]).unwrap();

//     let base_vault = pool_ac_detail.token_1_vault;
//     let quote_vault = pool_ac_detail.token_0_vault;
//     let (base_amount, quote_amount) = get_pool_vault_amount(base_vault, quote_vault).unwrap();

//     let limit_quote_amount = get_constant_product_swap_amount(
//         SwapDirection::Sell,
//         base_amount,
//         quote_amount,
//         amount,
//         0,
//         0,
//     ).expect("Failed to calculate buy limit_quote_amount");
//     println!("amount: {}, limit_quote_amount: {}, limit_quote_amount_slippage: {}", amount, limit_quote_amount, (limit_quote_amount as f64*slippage_factor) as u64);
//     let instruction = build_ray_cpmm_swap_instruction(pool_state, pool_ac_detail, SwapDirection::Sell, amount, (limit_quote_amount as f64*slippage_factor) as u64);
//     (instruction)
// }

// pub fn build_ray_cpmm_sell_instruction_no_quote(
//     // base_vault: Pubkey,
//     // quote_vault: Pubkey,
//     pool_state: Pubkey,
//     pool_ac_detail: RaydiumCpmmPoolState,
//     amount: u64,
// ) -> (Instruction) {

//     let instruction = build_ray_cpmm_swap_instruction(pool_state, pool_ac_detail, SwapDirection::Sell, amount, 0);
//     (instruction)
// }



/// Build a PumpSwap instruction (buy or sell)
pub fn build_ray_cpmm_swap_instruction(
    accounts: &RayCpmmSwapAccounts,
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
            AccountMeta::new(accounts.wallet, true),
            AccountMeta::new_readonly(accounts.authority, false),
            AccountMeta::new_readonly(accounts.amm_config, false),
            AccountMeta::new(accounts.pool_state, false),
            AccountMeta::new(accounts.quote_ata, false),
            AccountMeta::new(accounts.base_ata, false),
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
            AccountMeta::new(accounts.wallet, true),
            AccountMeta::new_readonly(accounts.authority, false),
            AccountMeta::new_readonly(accounts.amm_config, false),
            AccountMeta::new(accounts.pool_state, false),
            AccountMeta::new(accounts.base_ata, false),
            AccountMeta::new(accounts.quote_ata, false),
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
        program_id: RAYDIUM_CPMM_PROGRAM_ID_PUBKEY,
        accounts: metas,
        data: full_data,
    }
}

pub fn get_instruction_accounts(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
) -> RayCpmmSwapAccounts {

    let mint = get_account(account_keys, accounts, 11);
    let base_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &mint);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &WSOL);
 
    RayCpmmSwapAccounts {
        wallet: get_wallet_keypair().pubkey(),
        authority: RAYDIUM_CPMM_AUTHORITY,
        amm_config: RAYDIUM_CPMM_AMM_CONFIG,
        pool_state: get_account(&account_keys, &accounts, 3),
        quote_ata: quote_ata,
        base_ata: base_ata,
        token_0_vault: get_account(&account_keys, &accounts, 6),
        token_1_vault: get_account(&account_keys, &accounts, 7),
        token_0_program: get_account(&account_keys, &accounts, 8),
        token_1_program: get_account(&account_keys, &accounts, 9),
        token_0_mint: WSOL,
        token_1_mint: mint,
        observation_key: get_account(&account_keys, &accounts, 12),
    }
    // TODO: Map the correct indices for each field as per the actual instruction layout
}

pub fn get_pool_state(ray_cpmm_accounts: &RayCpmmSwapAccounts) -> RaydiumCpmmPoolState {
    let client = match GLOBAL_RPC_CLIENT.get() {
        Some(client) => client,
        None => {
            eprintln!("!!!!!!RPC ERROR: RPC client not initialized");
            return RaydiumCpmmPoolState::default();
        }
    };
    
    let account_data = match client.get_account_data(&ray_cpmm_accounts.pool_state) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("!!!!!!RPC ERROR: Failed to get account data for pool state: {:?}", e);
            eprintln!("!!!!!!Pool state account: {:?}", ray_cpmm_accounts.pool_state);
            return RaydiumCpmmPoolState::default();
        }
    };
    
    let pool_state = match RaydiumCpmmPoolState::deserialize(&mut &account_data[8..]) {
        Ok(state) => state,
        Err(e) => {
            eprintln!("!!!!!!RPC ERROR: Failed to deserialize pool state: {:?}", e);
            eprintln!("!!!!!!Account data length: {}", account_data.len());
            return RaydiumCpmmPoolState::default();
        }
    };
    
    pool_state
}

/// Dummy function to create RayCpmmSwapAccounts for migrate instruction
/// Based on get_instruction_accounts from ray_cpmm.rs
pub fn get_instruction_accounts_migrate(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
) -> RayCpmmSwapAccounts {
    let mint = get_account(account_keys, accounts, 1);
    let base_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &mint);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &WSOL);
 
    RayCpmmSwapAccounts {
        wallet: get_wallet_keypair().pubkey(),
        authority: RAYDIUM_CPMM_AUTHORITY,
        amm_config: RAYDIUM_CPMM_AMM_CONFIG,
        pool_state: get_account(&account_keys, &accounts, 5),
        quote_ata: quote_ata,
        base_ata: base_ata,
        token_0_vault: get_account(&account_keys, &accounts, 9),
        token_1_vault: get_account(&account_keys, &accounts, 8),
        token_0_program: spl_token::ID,
        token_1_program: spl_token::ID,
        token_0_mint: WSOL,
        token_1_mint: mint,
        observation_key: get_account(&account_keys, &accounts, 12),
    }
}