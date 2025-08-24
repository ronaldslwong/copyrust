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
use crate::constants::boop_fun::{BOOP_FUN_PROGRAM_ID, BOOP_FUN_PROGRAM_ID_PUBKEY};
use spl_token_2022::id as token_2022_program_id;
/// Enum for swap direction
#[derive(PartialEq, Copy, Clone)]
pub enum SwapDirection {
    Buy,
    Sell,
}

/// Struct to hold all required accounts for a BoopFun swap
#[derive(PartialEq, Copy, Clone, Debug)]
pub struct BoopFunAccounts {
    pub mint: Pubkey,                                    // #1 - Mint
    pub bonding_curve: Pubkey,                           // #2 - Bonding Curve
    pub trading_fees_vault: Pubkey,                      // #3 - Trading Fees Vault
    pub bonding_curve_vault: Pubkey,                     // #4 - Bonding Curve Vault
    pub bonding_curve_sol_vault: Pubkey,                 // #5 - Bonding Curve Sol Vault
    pub recipient_token_account: Pubkey,                  // #6 - Recipient Token Account
    pub buyer: Pubkey,                                   // #7 - Buyer
    pub config: Pubkey,                                  // #8 - Config
    pub vault_authority: Pubkey,                         // #9 - Vault Authority
    pub wsol: Pubkey,                                    // #10 - Wsol
    pub system_program: Pubkey,                          // #11 - System Program
    pub token_program: Pubkey,                           // #12 - Token Program
    pub associated_token_program: Pubkey,                 // #13 - Associated Token Program
}

impl Default for BoopFunAccounts {
    fn default() -> Self {
        BoopFunAccounts {
            mint: Pubkey::default(),
            bonding_curve: Pubkey::default(),
            trading_fees_vault: Pubkey::default(),
            bonding_curve_vault: Pubkey::default(),
            bonding_curve_sol_vault: Pubkey::default(),
            recipient_token_account: Pubkey::default(),
            buyer: Pubkey::default(),
            config: Pubkey::default(),
            vault_authority: Pubkey::default(),
            wsol: Pubkey::default(),
            system_program: Pubkey::default(),
            token_program: Pubkey::default(),
            associated_token_program: Pubkey::default(),
        }
    }
}

pub fn default_boop_fun_accounts() -> BoopFunAccounts {
    BoopFunAccounts::default()
}

/// Helper to get the discriminator for buy/sell
fn get_discriminator(direction: SwapDirection) -> [u8; 8] {
    match direction {
        SwapDirection::Buy => [8, 167, 240, 229, 178, 101, 119, 54],
        SwapDirection::Sell => [109, 61, 40, 187, 230, 176, 135, 174],
    }
}

/// Build a BoopFun swap instruction (buy or sell)
pub fn build_boop_fun_swap_instruction(
    accounts: &BoopFunAccounts,
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
        AccountMeta::new_readonly(accounts.mint, false),                    // #1 - Mint
        AccountMeta::new(accounts.bonding_curve, false),                    // #2 - Bonding Curve (WRITABLE)
        AccountMeta::new(accounts.trading_fees_vault, false),               // #3 - Trading Fees Vault (WRITABLE)
        AccountMeta::new(accounts.bonding_curve_vault, false),              // #4 - Bonding Curve Vault (WRITABLE)
        AccountMeta::new(accounts.bonding_curve_sol_vault, false),          // #5 - Bonding Curve Sol Vault (WRITABLE)
        AccountMeta::new(accounts.recipient_token_account, false),          // #6 - Recipient Token Account (WRITABLE)
        AccountMeta::new(accounts.buyer, true),                             // #7 - Buyer (WRITABLE, SIGNER, FEE PAYER)
        AccountMeta::new_readonly(accounts.config, false),                  // #8 - Config
        AccountMeta::new_readonly(accounts.vault_authority, false),          // #9 - Vault Authority
        AccountMeta::new_readonly(accounts.wsol, false),                    // #10 - Wsol
        AccountMeta::new_readonly(accounts.system_program, false),           // #11 - System Program (PROGRAM)
        AccountMeta::new_readonly(accounts.token_program, false),            // #12 - Token Program (PROGRAM)
        AccountMeta::new_readonly(accounts.associated_token_program, false), // #13 - Associated Token Program (PROGRAM)
    ];
    
    Instruction {
        program_id: BOOP_FUN_PROGRAM_ID_PUBKEY,
        accounts: metas,
        data: full_data,
    }
}


pub fn get_instruction_accounts(
    instruction_accounts: &[AccountMeta]
) -> BoopFunAccounts {
    let mint = instruction_accounts[0].pubkey;
    let base_ata = spl_associated_token_account::get_associated_token_address_with_program_id(&get_wallet_keypair().pubkey(), &mint, &token_2022_program_id(),);
    let quote_ata = spl_associated_token_account::get_associated_token_address(&get_wallet_keypair().pubkey(), &Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap());
 
    BoopFunAccounts {
        mint: instruction_accounts[0].pubkey,
        bonding_curve: instruction_accounts[1].pubkey,
        trading_fees_vault: instruction_accounts[2].pubkey,
        bonding_curve_vault: instruction_accounts[3].pubkey,
        bonding_curve_sol_vault: instruction_accounts[4].pubkey,
        recipient_token_account: base_ata,
        buyer: get_wallet_keypair().pubkey(),
        config: instruction_accounts[7].pubkey,
        vault_authority: instruction_accounts[8].pubkey,
        wsol: instruction_accounts[9].pubkey,
        system_program: instruction_accounts[10].pubkey,
        token_program: instruction_accounts[11].pubkey,
        associated_token_program: instruction_accounts[12].pubkey,
    }
}


pub fn build_boop_fun_sell_instruction(
    amount: u64,
    boop_fun_accounts: &BoopFunAccounts,
) -> Instruction {

    return build_boop_fun_swap_instruction(&boop_fun_accounts, SwapDirection::Sell, amount,  0);
}