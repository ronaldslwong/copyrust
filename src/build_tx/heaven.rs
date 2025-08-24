// pump_swap.rs
// Build buy and sell instructions for PumpSwap AMM
// Inspired by pump.go and pumpSwap.go (Go code)

use solana_program::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use std::vec::Vec;
use crate::init::wallet_loader::get_wallet_keypair;
use solana_sdk::signature::Signer;
use std::str::FromStr;
use spl_associated_token_account;
use crate::constants::heaven::{HEAVEN_15, HEAVEN_16, HEAVEN_PROGRAM_ID_PUBKEY};
use spl_token_2022::id as token_2022_program_id;
/// Enum for swap direction
#[derive(PartialEq, Copy, Clone)]
pub enum SwapDirection {
    Buy,
    Sell,
}

/// Struct to hold all required accounts for a Heaven swap
#[derive(PartialEq, Copy, Clone, Debug)]
pub struct HeavenAccounts {
    pub token_a_program: Pubkey,
    pub token_b_program: Pubkey,
    pub associated_token_program: Pubkey,
    pub system_program: Pubkey,
    pub liquidity_pool_state: Pubkey,
    pub user: Pubkey,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub user_token_a_vault: Pubkey,
    pub user_token_b_vault: Pubkey,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub protocol_config: Pubkey,
    pub instruction_sysvar_account_info: Pubkey,
    pub custom_1: Pubkey,
    pub custom_2: Pubkey,
}

impl Default for HeavenAccounts {
    fn default() -> Self {
        HeavenAccounts {
            token_a_program: Pubkey::default(),
            token_b_program: Pubkey::default(),
            associated_token_program: Pubkey::default(),
            system_program: Pubkey::default(),
            liquidity_pool_state: Pubkey::default(),
            user: Pubkey::default(),
            token_a_mint: Pubkey::default(),
            token_b_mint: Pubkey::default(),
            user_token_a_vault: Pubkey::default(),
            user_token_b_vault: Pubkey::default(),
            token_a_vault: Pubkey::default(),
            token_b_vault: Pubkey::default(),
            protocol_config: Pubkey::default(),
            instruction_sysvar_account_info: Pubkey::default(),
            custom_1: Pubkey::default(),
            custom_2: Pubkey::default(),
        }
    }
}

pub fn default_heaven_accounts() -> HeavenAccounts {
    HeavenAccounts::default()
}

/// Helper to get the discriminator for buy/sell
fn get_discriminator(direction: SwapDirection) -> [u8; 8] {
    match direction {
        SwapDirection::Buy => [102, 6, 61, 18, 1, 218, 235, 234],
        SwapDirection::Sell => [51, 230, 133, 164, 1, 127, 131, 173],
    }
}

/// Build a Heaven swap instruction (buy or sell)
pub fn build_heaven_swap_instruction(
    accounts: &HeavenAccounts,
    direction: SwapDirection,
    amount: u64,
    limit_quote_amount: u64,
) -> Instruction {
    let discriminator = get_discriminator(direction);

    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&limit_quote_amount.to_le_bytes());
    let flags: u32 = 0; // or whatever you need
    data.extend_from_slice(&flags.to_le_bytes());              // u32
    
    let full_data = [discriminator.as_ref(), data.as_slice()].concat();

    let metas = vec![
        AccountMeta::new_readonly(accounts.token_a_program, false),
        AccountMeta::new_readonly(accounts.token_b_program, false),
        AccountMeta::new_readonly(accounts.associated_token_program, false),
        AccountMeta::new_readonly(accounts.system_program, false),
        AccountMeta::new(accounts.liquidity_pool_state, false),
        AccountMeta::new(accounts.user, true),
        AccountMeta::new_readonly(accounts.token_a_mint, false),
        AccountMeta::new_readonly(accounts.token_b_mint, false),
        AccountMeta::new(accounts.user_token_a_vault, false),
        AccountMeta::new(accounts.user_token_b_vault, false),
        AccountMeta::new(accounts.token_a_vault, false),
        AccountMeta::new(accounts.token_b_vault, false),
        AccountMeta::new(accounts.protocol_config, false),
        AccountMeta::new_readonly(accounts.instruction_sysvar_account_info, false),
        AccountMeta::new_readonly(accounts.custom_1, false),
        AccountMeta::new_readonly(accounts.custom_2, false),
    ];
    
    Instruction {
        program_id: HEAVEN_PROGRAM_ID_PUBKEY,
        accounts: metas,
        data: full_data,
    }
}


pub fn get_instruction_accounts(
    instruction_accounts: &[AccountMeta]
) -> HeavenAccounts {
    let mint = instruction_accounts[6].pubkey;
    let base_ata = spl_associated_token_account::get_associated_token_address_with_program_id(&get_wallet_keypair().pubkey(), &mint, &token_2022_program_id(),);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap());
 
    HeavenAccounts {
        token_a_program: instruction_accounts[0].pubkey,
        token_b_program: instruction_accounts[1].pubkey,
        associated_token_program: instruction_accounts[2].pubkey,
        system_program: instruction_accounts[3].pubkey,
        liquidity_pool_state: instruction_accounts[4].pubkey,
        user: get_wallet_keypair().pubkey(),
        token_a_mint: instruction_accounts[6].pubkey,
        token_b_mint: instruction_accounts[7].pubkey,
        user_token_a_vault: base_ata,
        user_token_b_vault: quote_ata,
        token_a_vault: instruction_accounts[10].pubkey,
        token_b_vault: instruction_accounts[11].pubkey,
        protocol_config: instruction_accounts[12].pubkey, // Placeholder
        instruction_sysvar_account_info: instruction_accounts[13].pubkey,
        custom_1: HEAVEN_15,
        custom_2: HEAVEN_16,
    }
}


pub fn build_heaven_sell_instruction(
    amount: u64,
    heaven_accounts: &HeavenAccounts,
) -> Instruction {

    return build_heaven_swap_instruction(&heaven_accounts, SwapDirection::Sell, amount,  0);
}