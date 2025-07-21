// pump_swap.rs
// Build buy and sell instructions for PumpSwap AMM
// Inspired by pump.go and pumpSwap.go (Go code)

use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::system_program;
use solana_sdk::pubkey::Pubkey;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use std::error::Error;
use std::vec::Vec;
use crate::init::wallet_loader::get_wallet_keypair;
use solana_sdk::signature::Signer;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_account_decoder::UiAccountEncoding;
use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use solana_client::rpc_config::RpcProgramAccountsConfig;
use solana_client::rpc_config::RpcAccountInfoConfig;
use solana_client::rpc_client::RpcClient;
use borsh::{BorshDeserialize, BorshSerialize};
use crate::build_tx::utils::get_account;

/// Enum for swap direction
#[derive(PartialEq, Copy, Clone)]
pub enum SwapDirection {
    Buy,
    Sell,
}

#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub struct PoolAccountInfo {
    pub pool_bump: u8,
    pub index: u16,
    pub creator: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub lp_mint: Pubkey,
    pub pool_base_token_account: Pubkey,
    pub pool_quote_token_account: Pubkey,
    pub lp_supply: u64,
    pub coin_creator: Pubkey,
}

/// Constants for PumpSwap program
pub mod pump_swap_constants {
    use solana_program::pubkey;
    use solana_program::pubkey::Pubkey;

    pub const PUMP_SWAP_PROGRAM_ID: Pubkey = pubkey!("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA");
    // Add other relevant constants as needed
    pub const PUMP_SWAP_GLOBAL_CONFIG: Pubkey = pubkey!("ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw");
    pub const PUMP_SWAP_PROTOCOL_FEE_RECIPIENT: Pubkey = pubkey!("G5UZAVbAf46s7cKWoyKu8kYTip9DGTpbLZ2qa9Aq69dP");
    pub const PUMP_SWAP_PROTOCOL_FEE_TOKEN_ACCOUNT: Pubkey = pubkey!("BWXT6RUhit9FfJQM3pBmqeFLPYmuxgmyhMGC5sGr8RbA");
    pub const PUMP_SWAP_EVENT_AUTHORITY: Pubkey = pubkey!("GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR");
    pub const PUMP_SWAP_ASSOCIATED_TOKEN_PROGRAM: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
    pub const WSOL: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
}

/// Struct to hold all required accounts for a swap
#[derive(PartialEq, Copy, Clone, Debug)]
pub struct PumpAmmAccounts {
    pub pool: Pubkey,
    pub user: Pubkey,
    pub global_config: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub user_base_token_account: Pubkey,
    pub user_quote_token_account: Pubkey,
    pub pool_base_token_account: Pubkey,
    pub pool_quote_token_account: Pubkey,
    pub protocol_fee_recipient: Pubkey,
    pub protocol_fee_token_account: Pubkey,
    pub base_token_program: Pubkey,
    pub quote_token_program: Pubkey,
    pub system_program: Pubkey,
    pub associated_token_program: Pubkey,
    pub event_authority: Pubkey,
    pub pump_program: Pubkey,
    pub coin_creator_vault_ata: Pubkey,
    pub coin_creator_vault_authority: Pubkey,
}

impl Default for PumpAmmAccounts {
    fn default() -> Self {
        PumpAmmAccounts {
            pool: Pubkey::default(),
            user: Pubkey::default(),
            global_config: Pubkey::default(),
            base_mint: Pubkey::default(),
            quote_mint: Pubkey::default(),
            user_base_token_account: Pubkey::default(),
            user_quote_token_account: Pubkey::default(),
            pool_base_token_account: Pubkey::default(),
            pool_quote_token_account: Pubkey::default(),
            protocol_fee_recipient: Pubkey::default(),
            protocol_fee_token_account: Pubkey::default(),
            base_token_program: Pubkey::default(),
            quote_token_program: Pubkey::default(),
            system_program: Pubkey::default(),
            associated_token_program: Pubkey::default(),
            event_authority: Pubkey::default(),
            pump_program: Pubkey::default(),
            coin_creator_vault_ata: Pubkey::default(),
            coin_creator_vault_authority: Pubkey::default(),
        }
    }
}

pub fn default_pump_amm_accounts() -> PumpAmmAccounts {
    PumpAmmAccounts::default()
}

/// Helper to get the discriminator for buy/sell
fn get_discriminator(direction: SwapDirection) -> [u8; 8] {
    match direction {
        SwapDirection::Buy => [102, 6, 61, 18, 1, 218, 235, 234],
        SwapDirection::Sell => [51, 230, 133, 164, 1, 127, 131, 173],
    }
}

/// Build a PumpSwap instruction (buy or sell)
pub fn build_pump_swap_instruction(
    accounts: &PumpAmmAccounts,
    direction: SwapDirection,
    amount: u64,
    limit_quote_amount: u64,
) -> Instruction {
    let discriminator = get_discriminator(direction);


    let mut data = Vec::with_capacity(16);
        data.extend_from_slice(&limit_quote_amount.to_le_bytes());
        data.extend_from_slice(&amount.to_le_bytes());
    let full_data = [discriminator.as_ref(), data.as_slice()].concat();

    let metas = vec![
        AccountMeta::new(accounts.pool, false),
        AccountMeta::new(accounts.user, true),
        AccountMeta::new_readonly(accounts.global_config, false),
        AccountMeta::new_readonly(accounts.base_mint, false),
        AccountMeta::new_readonly(accounts.quote_mint, false),
        AccountMeta::new(accounts.user_base_token_account, false),
        AccountMeta::new(accounts.user_quote_token_account, false),
        AccountMeta::new(accounts.pool_base_token_account, false),
        AccountMeta::new(accounts.pool_quote_token_account, false),
        AccountMeta::new_readonly(accounts.protocol_fee_recipient, false),
        AccountMeta::new(accounts.protocol_fee_token_account, false),
        AccountMeta::new_readonly(accounts.base_token_program, false),
        AccountMeta::new_readonly(accounts.quote_token_program, false),
        AccountMeta::new_readonly(accounts.system_program, false),
        AccountMeta::new_readonly(accounts.associated_token_program, false),
        AccountMeta::new_readonly(accounts.event_authority, false),
        AccountMeta::new_readonly(accounts.pump_program, false),
        AccountMeta::new(accounts.coin_creator_vault_ata, false),
        AccountMeta::new_readonly(accounts.coin_creator_vault_authority, false),
    ];
    Instruction {
        program_id: pump_swap_constants::PUMP_SWAP_PROGRAM_ID,
        accounts: metas,
        data: full_data,
    }
}

// /// Convenience wrappers for buy/sell
// pub fn build_pump_buy_instruction(
//     // base_vault: Pubkey,
//     // quote_vault: Pubkey,
//     amount: u64,
//     slippage_basis_points: u64,
//     account_keys: Vec<Vec<u8>>,
//     accounts: Vec<u8>,
//     target_sol_buy: u64,
//     target_token_buy: u64,
// ) -> (Instruction, u64) {


//     let slippage_factor = 1.0+slippage_basis_points as f64 /10000.0;
//     let base_vault = get_account(account_keys.clone(), accounts.clone(), 7);
//     let quote_vault = get_account(account_keys.clone(), accounts.clone(), 8);

//     // println!("base_vault: {}", base_vault);
//     // println!("quote_vault: {}", quote_vault);
//     let limit_quote_amount = get_pump_swap_amount(
//         SwapDirection::Buy,
//         base_vault,
//         quote_vault,
//         amount,
//         target_sol_buy,
//         target_token_buy,
//     ).expect("Failed to calculate buy limit_quote_amount");

//     let accounts = get_instruction_accounts(account_keys.clone(), accounts.clone());

//     let instruction = build_pump_swap_instruction(accounts, SwapDirection::Buy, (amount as f64*slippage_factor) as u64, limit_quote_amount);
//     (instruction, limit_quote_amount)
// }

pub fn build_pump_sell_instruction(
    amount: u64,
    slippage_basis_points: u64,
    pump_swap_accounts: &PumpAmmAccounts,
) -> Instruction {

    let slippage_factor = 1.0-slippage_basis_points as f64 /10000.0;
    
    let limit_quote_amount = get_pump_swap_amount(
        SwapDirection::Sell,
        pump_swap_accounts.pool_base_token_account,
        pump_swap_accounts.pool_quote_token_account,
        amount,
        0,
        0,
    ).expect("Failed to calculate sell limit_quote_amount");

    return build_pump_swap_instruction(&pump_swap_accounts, SwapDirection::Sell,  (limit_quote_amount as f64*slippage_factor) as u64, amount);
}

pub fn build_pump_sell_instruction_raw(
    amount: u64,
    slippage_basis_points: u64,
    mint: Pubkey,
) -> Instruction {
    let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

    let slippage_factor = 1.0-slippage_basis_points as f64 /10000.0;
    let instruction: Instruction = Instruction{
        program_id: Pubkey::new_unique(),
        accounts: vec![],
        data: vec![],
    };


    let pool_ac = get_pool_accounts(mint, rpc_client);

    let account_data = match rpc_client.get_account_data(&pool_ac.unwrap()) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("!!!!!!RPC ERROR: Failed to get account data for pool: {:?}", e);
            eprintln!("!!!!!!Pool account: {:?}", pool_ac.unwrap());
            return instruction; // Return default instruction on error
        }
    };
    if account_data.iter().all(|&b| b == 0) {
        println!("account_data is all zeros!");
        return instruction; // or return an error, or handle as needed
    }
    let pool_ac_detail = match PoolAccountInfo::deserialize(&mut &account_data[8..]) {
        Ok(detail) => detail,
        Err(e) => {
            eprintln!("!!!!!!RPC ERROR: Failed to deserialize pool account info: {:?}", e);
            eprintln!("!!!!!!Account data length: {}", account_data.len());
            return instruction; // Return default instruction on error
        }
    };
    println!("pool_ac_detail: {:?}", pool_ac_detail);


    let limit_quote_amount = get_pump_swap_amount(
        SwapDirection::Sell,
        pool_ac_detail.pool_base_token_account,
        pool_ac_detail.pool_quote_token_account,
        amount,
        0,
        0,
    ).expect("Failed to calculate sell limit_quote_amount");

    let accounts = get_instruction_accounts_rpc(mint, pool_ac.unwrap(), pool_ac_detail.pool_base_token_account, pool_ac_detail.pool_quote_token_account, pool_ac_detail.coin_creator);

    return build_pump_swap_instruction(&accounts, SwapDirection::Sell,  (limit_quote_amount as f64*slippage_factor) as u64, amount);
}

/// Calculates the expected output amount for a buy or sell swap.
///
/// # Arguments
/// * `rpc_client` - Reference to an RpcClient for fetching vault balances
/// * `direction` - SwapDirection (Buy or Sell)
/// * `base_vault` - Base token vault Pubkey
/// * `quote_vault` - Quote token vault Pubkey
/// * `swap_amount` - Amount of input token (in base units)
/// * `base_decimals` - Decimals for base token
/// * `quote_decimals` - Decimals for quote token
///
/// # Returns
/// * `Ok(u64)` - The expected output amount
/// * `Err` - If fetching or calculation fails
pub fn get_pump_swap_amount(
    direction: SwapDirection,
    base_vault: Pubkey,
    quote_vault: Pubkey,
    swap_amount: u64,
    target_sol_buy: u64,
    target_token_buy: u64,
) -> Result<u64, Box<dyn Error>> {
    let keys = vec![base_vault, quote_vault];
    let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

    let res = match rpc_client.get_multiple_accounts_with_commitment(&keys, CommitmentConfig::processed()) {
        Ok(response) => response,
        Err(e) => {
            eprintln!("!!!!!!RPC ERROR: Failed to get multiple accounts in get_pump_swap_amount: {:?}", e);
            eprintln!("!!!!!!Keys being requested: {:?}", keys);
            return Err(format!("RPC call failed: {:?}", e).into());
        }
    };
    if res.value.len() != 2 || res.value[0].is_none() || res.value[1].is_none() {
        return Err("missing vault data".into());
    }
    let base_data = res.value[0].as_ref().unwrap().data.as_slice();
    let quote_data = res.value[1].as_ref().unwrap().data.as_slice();
    if base_data.len() < 72 || quote_data.len() < 72 {
        return Err("vault account data too short".into());
    }
    let base_amount = u64::from_le_bytes(base_data[64..72].try_into().unwrap());
    let quote_amount = u64::from_le_bytes(quote_data[64..72].try_into().unwrap());
    if base_amount == 0 {
        return Err("zero base amount".into());
    }
    let adjusted_price = match direction {
        SwapDirection::Buy => ((base_amount-target_token_buy) as f64 * swap_amount as f64) / ((quote_amount + target_sol_buy) as f64 + swap_amount as f64) ,
        SwapDirection::Sell => ((quote_amount-target_sol_buy) as f64 * swap_amount as f64) / ((base_amount + target_token_buy) as f64 + swap_amount as f64) ,
    };
    Ok(adjusted_price as u64)
} 

pub fn get_instruction_accounts(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
) -> PumpAmmAccounts {

    let mint = get_account(account_keys, accounts, 3);
    let base_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &mint);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &pump_swap_constants::WSOL);
 
    PumpAmmAccounts {
        pool: get_account(account_keys, accounts, 0),
        user: get_wallet_keypair().pubkey(),
        global_config: pump_swap_constants::PUMP_SWAP_GLOBAL_CONFIG,
        base_mint: mint,
        quote_mint: pump_swap_constants::WSOL,
        user_base_token_account: base_ata,
        user_quote_token_account: quote_ata,
        pool_base_token_account: get_account(account_keys, accounts, 7),
        pool_quote_token_account: get_account(account_keys, accounts, 8),
        protocol_fee_recipient: pump_swap_constants::PUMP_SWAP_PROTOCOL_FEE_RECIPIENT,
        protocol_fee_token_account: pump_swap_constants::PUMP_SWAP_PROTOCOL_FEE_TOKEN_ACCOUNT,
        base_token_program: spl_token::ID,
        quote_token_program: spl_token::ID,
        system_program: system_program::ID,
        associated_token_program: pump_swap_constants::PUMP_SWAP_ASSOCIATED_TOKEN_PROGRAM,
        event_authority: pump_swap_constants::PUMP_SWAP_EVENT_AUTHORITY,
        pump_program: pump_swap_constants::PUMP_SWAP_PROGRAM_ID,
        coin_creator_vault_ata: get_account(account_keys, accounts, 17),
        coin_creator_vault_authority: get_account(account_keys, accounts, 18),
    }
    // TODO: Map the correct indices for each field as per the actual instruction layout
}

pub fn get_instruction_accounts_rpc(
    mint: Pubkey,
    pool_ac: Pubkey,
    pool_base_token_account: Pubkey,
    pool_quote_token_account: Pubkey,
    coin_creator: Pubkey,
) -> PumpAmmAccounts {

    let base_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &mint);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &pump_swap_constants::WSOL);
    // println!("base_ata: {}", base_ata);
    // println!("quote_ata: {}", quote_ata);

    let (fee_va, _) = derive_creator_vault_authority(&coin_creator);
    
    PumpAmmAccounts {
        pool: pool_ac,
        user: get_wallet_keypair().pubkey(),
        global_config: pump_swap_constants::PUMP_SWAP_GLOBAL_CONFIG,
        base_mint: mint,
        quote_mint: pump_swap_constants::WSOL,
        user_base_token_account: base_ata,
        user_quote_token_account: quote_ata,
        pool_base_token_account: pool_base_token_account,
        pool_quote_token_account: pool_quote_token_account,
        protocol_fee_recipient: pump_swap_constants::PUMP_SWAP_PROTOCOL_FEE_RECIPIENT,
        protocol_fee_token_account: pump_swap_constants::PUMP_SWAP_PROTOCOL_FEE_TOKEN_ACCOUNT,
        base_token_program: spl_token::ID,
        quote_token_program: spl_token::ID,
        system_program: system_program::ID,
        associated_token_program: pump_swap_constants::PUMP_SWAP_ASSOCIATED_TOKEN_PROGRAM,
        event_authority: pump_swap_constants::PUMP_SWAP_EVENT_AUTHORITY,
        pump_program: pump_swap_constants::PUMP_SWAP_PROGRAM_ID,
        coin_creator_vault_ata: spl_associated_token_account::get_associated_token_address(&fee_va, &pump_swap_constants::WSOL),
        coin_creator_vault_authority: fee_va,
    }
    // TODO: Map the correct indices for each field as per the actual instruction layout
}

// pub fn get_account(
//     account_keys: Vec<Vec<u8>>,
//     accounts: Vec<u8>,
//     index: u8,
// ) -> Pubkey {
//     let idx = *accounts.get(index as usize).unwrap_or(&0) as usize;
//     Pubkey::try_from(account_keys.get(idx as usize).unwrap().as_slice()).unwrap()
// }

pub fn get_pool_accounts(
    mint: Pubkey,
    rpc_client: &RpcClient,
) -> Option<Pubkey> {
    let mint_offsets = [43u64, 75u64]; // or whatever offsets you want

    for offset in mint_offsets {
        println!("\nChecking offset {} for mint {}...", offset, mint);

        let filters = vec![
            RpcFilterType::Memcmp(Memcmp::new(
                offset.try_into().unwrap(),
                MemcmpEncodedBytes::Base58(mint.to_string()),
            )),
        ];

        let config = RpcProgramAccountsConfig {
            filters: Some(filters),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                ..Default::default()
            },
            ..Default::default()
        };

        match rpc_client.get_program_accounts_with_config(&pump_swap_constants::PUMP_SWAP_PROGRAM_ID, config) {
            Ok(accounts) => {
                if accounts.is_empty() {
                    println!("No matches found at offset {}", offset);
                } else {
                    println!("Found {} market(s) at offset {}:", accounts.len(), offset);
                    for (pubkey, _) in accounts {
                        println!("- Market account: {}", pubkey);
                        return Some(pubkey);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error querying offset {}: {:?}", offset, e);
            }
        }
    }
    None
}

pub fn derive_creator_vault_authority(creator: &Pubkey) -> (Pubkey, u8) {
    let program_id = pump_swap_constants::PUMP_SWAP_PROGRAM_ID;
    let seeds = &[b"creator_vault", creator.as_ref()];
    Pubkey::find_program_address(seeds, &program_id)
}


pub fn get_instruction_accounts_migrate_pump(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
) -> PumpAmmAccounts {

    let mint = get_account(account_keys, accounts, 2);
    let base_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &mint);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &pump_swap_constants::WSOL);
 
    let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    
    // Add 1-second delay before RPC call to prevent rate limiting
    println!("[PUMP_SWAP] Waiting 1 second before RPC call to prevent rate limiting...");
    std::thread::sleep(std::time::Duration::from_secs(1));
    
    let account_data = match rpc_client.get_account_data(&get_account(account_keys, accounts, 9)) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("!!!!!!RPC ERROR: Failed to get account data in get_instruction_accounts_migrate_pump: {:?}", e);
            eprintln!("!!!!!!Account being requested: {:?}", get_account(account_keys, accounts, 9));
            // Return default accounts on error
            return PumpAmmAccounts::default();
        }
    };
    if account_data.iter().all(|&b| b == 0) {
        println!("account_data is all zeros!");
    }
    let pool_ac_detail = match PoolAccountInfo::deserialize(&mut &account_data[8..]) {
        Ok(detail) => detail,
        Err(e) => {
            eprintln!("!!!!!!RPC ERROR: Failed to deserialize pool account info in migrate: {:?}", e);
            eprintln!("!!!!!!Account data length: {}", account_data.len());
            return PumpAmmAccounts::default(); // Return default accounts on error
        }
    };
    let (creator_vault_authority, _) = derive_creator_vault_authority(&pool_ac_detail.coin_creator);
    let creator_vault_ata = spl_associated_token_account::get_associated_token_address(&creator_vault_authority, &pump_swap_constants::WSOL);
    
    PumpAmmAccounts {
        pool: get_account(account_keys, accounts, 9),
        user: get_wallet_keypair().pubkey(),
        global_config: pump_swap_constants::PUMP_SWAP_GLOBAL_CONFIG,
        base_mint: mint,
        quote_mint: pump_swap_constants::WSOL,
        user_base_token_account: base_ata,
        user_quote_token_account: quote_ata,
        pool_base_token_account: get_account(account_keys, accounts, 17),
        pool_quote_token_account: get_account(account_keys, accounts, 18),
        protocol_fee_recipient: pump_swap_constants::PUMP_SWAP_PROTOCOL_FEE_RECIPIENT,
        protocol_fee_token_account: pump_swap_constants::PUMP_SWAP_PROTOCOL_FEE_TOKEN_ACCOUNT,
        base_token_program: get_account(account_keys, accounts, 19),
        quote_token_program: spl_token::ID,
        system_program: system_program::ID,
        associated_token_program: pump_swap_constants::PUMP_SWAP_ASSOCIATED_TOKEN_PROGRAM,
        event_authority: pump_swap_constants::PUMP_SWAP_EVENT_AUTHORITY,
        pump_program: pump_swap_constants::PUMP_SWAP_PROGRAM_ID,
        coin_creator_vault_ata: creator_vault_ata,
        coin_creator_vault_authority: creator_vault_authority,
    }
    // TODO: Map the correct indices for each field as per the actual instruction layout
}