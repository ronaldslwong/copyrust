// use crate::pumpfun::{Config, load_config};
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    system_program,
};
use solana_sdk::pubkey::Pubkey;
use crate::build_tx::utils::get_account;
use crate::constants::pump_fun::{GLOBAL_ACCOUNT, FEE_RECIPIENT, MINT_AUTHORITY, PUMP_FUN_PROGRAM_ID_PUBKEY};
use crate::init::wallet_loader::get_wallet_keypair;
use solana_sdk::signature::Signer;
use crate::build_tx::pump_swap::SwapDirection;
use std::str::FromStr;

/// Represents the state of a bonding curve on pump.fun.
/// This struct is based on the implementation from the provided GitHub link.
#[derive(Clone, Copy, BorshSerialize, BorshDeserialize, Debug)]
pub struct BondingCurve {
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
    pub creator: Pubkey,
}

impl Default for BondingCurve {
    fn default() -> Self {
        BondingCurve {
            virtual_token_reserves: 0,
            virtual_sol_reserves: 0,
            real_token_reserves: 0,
            real_sol_reserves: 0,
            token_total_supply: 0,
            complete: false,
            creator: Pubkey::default(),
        }
    }
}

/// Helper function to create default BondingCurve
pub fn default_bonding_curve() -> BondingCurve {
    BondingCurve::default()
}

/// Struct to hold all required accounts for PumpFun operations
#[derive(Clone, Debug)]
pub struct PumpFunAccounts {
    pub global_account: Pubkey,
    pub fee_recipient: Pubkey,
    pub mint: Pubkey,
    pub bonding_curve_pda: Pubkey,
    pub bonding_curve_ata: Pubkey,
    pub user_ata: Pubkey,
    pub user: Pubkey,
    pub system_program: Pubkey,
    pub spl_token_program: Pubkey,
    pub creator_fee_vault: Pubkey,
    pub mint_authority: Pubkey,
    pub pump_fun_program: Pubkey,
    pub global_volume_accumulator: Pubkey,
    pub user_volume_accumulator: Pubkey,
}

impl Default for PumpFunAccounts {
    fn default() -> Self {
        PumpFunAccounts {
            global_account: GLOBAL_ACCOUNT,
            fee_recipient: FEE_RECIPIENT,
            mint: Pubkey::default(),
            bonding_curve_pda: Pubkey::default(),
            bonding_curve_ata: Pubkey::default(),
            user_ata: Pubkey::default(),
            user: Pubkey::default(),
            system_program: system_program::ID,
            spl_token_program: spl_token::ID,
            creator_fee_vault: Pubkey::default(),
            mint_authority: MINT_AUTHORITY,
            pump_fun_program: PUMP_FUN_PROGRAM_ID_PUBKEY,
            global_volume_accumulator: global_volume_accumulator_pda(),
            user_volume_accumulator: user_volume_accumulator_pda(&Pubkey::default()),
        }
    }
}

/// Helper function to create default PumpFun accounts
pub fn default_pump_fun_accounts() -> PumpFunAccounts {
    PumpFunAccounts::default()
}

fn get_discriminator(direction: SwapDirection) -> [u8; 8] {
    match direction {
        SwapDirection::Buy => [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea],
        SwapDirection::Sell => [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad],
    }
}


/// Standalone function to calculate swap amounts for pump.fun bonding curve
/// This can be used independently of the BondingCurve struct
pub fn calculate_pump_fun_swap_amount(
    direction: crate::build_tx::pump_swap::SwapDirection,
    bonding_curve_state: BondingCurve,
    swap_amount: u64,
    fee_basis_points: u64,
) -> (u64, Pubkey) {

    match direction {
        crate::build_tx::pump_swap::SwapDirection::Buy => {
            // Calculate the product of virtual reserves using u128 to avoid overflow
            let n: u128 = (bonding_curve_state.virtual_sol_reserves as u128) * (bonding_curve_state.virtual_token_reserves as u128);
            
            // Calculate the new virtual sol reserves after the purchase
            let i: u128 = (bonding_curve_state.virtual_sol_reserves as u128) + (swap_amount as u128);
            
            // Calculate the new virtual token reserves after the purchase
            let r: u128 = n / i + 1;
            
            // Calculate the amount of tokens to be purchased
            let s: u128 = (bonding_curve_state.virtual_token_reserves as u128) - r;
            
            // Convert back to u64 and return the minimum of calculated tokens and real reserves
            let s_u64 = s as u64;
            if s_u64 < bonding_curve_state.real_token_reserves {
                (s_u64, bonding_curve_state.creator)
            } else {
                (bonding_curve_state.real_token_reserves, bonding_curve_state.creator)
            }
        },
        crate::build_tx::pump_swap::SwapDirection::Sell => {
            // Calculate the proportional amount of virtual sol reserves to be received using u128
            let n: u128 = ((swap_amount as u128) * (bonding_curve_state.virtual_sol_reserves as u128)) / 
                         ((bonding_curve_state.virtual_token_reserves as u128) + (swap_amount as u128));
            
            // Calculate the fee amount in the same units
            let a: u128 = (n * (fee_basis_points as u128)) / 10000;
            
            // Return the net amount after deducting the fee, converting back to u64
            ((n - a) as u64, bonding_curve_state.creator)
        },
    }
}



pub fn build_pump_fun_instruction(
    accounts: &PumpFunAccounts,
    direction: crate::build_tx::pump_swap::SwapDirection,
    amount: u64,
    limit_quote_amount: u64,
) -> Instruction {
    let discriminator = get_discriminator(direction);

    let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&limit_quote_amount.to_le_bytes());
        data.extend_from_slice(&amount.to_le_bytes());
    let full_data = [discriminator.as_ref(), data.as_slice()].concat();

    let mut metas = vec![];
    if direction == SwapDirection::Buy {
        metas = vec![
            AccountMeta::new_readonly(accounts.global_account, false),
            AccountMeta::new(accounts.fee_recipient, false),
            AccountMeta::new_readonly(accounts.mint, false),
            AccountMeta::new(accounts.bonding_curve_pda, false),
            AccountMeta::new(accounts.bonding_curve_ata, false),
            AccountMeta::new(accounts.user_ata, false),
            AccountMeta::new(accounts.user, true),
            AccountMeta::new_readonly(accounts.system_program, false),
            AccountMeta::new_readonly(accounts.spl_token_program, false),
            AccountMeta::new(accounts.creator_fee_vault, false),
            AccountMeta::new_readonly(accounts.mint_authority, false),
            AccountMeta::new_readonly(accounts.pump_fun_program, false),
            AccountMeta::new(accounts.global_volume_accumulator, false),
            AccountMeta::new(accounts.user_volume_accumulator, false),
        ];
    } else {
        metas = vec![
            AccountMeta::new_readonly(accounts.global_account, false),
            AccountMeta::new(accounts.fee_recipient, false),
            AccountMeta::new_readonly(accounts.mint, false),
            AccountMeta::new(accounts.bonding_curve_pda, false),
            AccountMeta::new(accounts.bonding_curve_ata, false),
            AccountMeta::new(accounts.user_ata, false),
            AccountMeta::new(accounts.user, true),
            AccountMeta::new_readonly(accounts.system_program, false),
            AccountMeta::new(accounts.creator_fee_vault, false),
            AccountMeta::new_readonly(accounts.spl_token_program, false),
            AccountMeta::new_readonly(accounts.mint_authority, false),
            AccountMeta::new_readonly(accounts.pump_fun_program, false),
            AccountMeta::new(accounts.global_volume_accumulator, false),
            AccountMeta::new(accounts.user_volume_accumulator, false),
        ];
    }
    Instruction {
        program_id: PUMP_FUN_PROGRAM_ID_PUBKEY,
        accounts: metas,
        data: full_data,
    }

}


/// Calculate the creator fee vault PDA for a given mint address.
pub fn get_creator_fee_vault(creator_vault: &Pubkey) -> Pubkey {
    let program_id = PUMP_FUN_PROGRAM_ID_PUBKEY;
    let (pda, _bump) =
        Pubkey::find_program_address(&[b"creator-vault", creator_vault.as_ref()], &program_id);
    pda
}

/// Builds a sell instruction for the pump.fun protocol.
///
/// # Arguments
///
/// * `user` - The public key of the seller (the signer).
/// * `mint` - The public key of the token's mint address.
/// * `bonding_curve_pda` - The PDA of the bonding curve state.
/// * `token_amount` - The amount of tokens to sell.
/// * `slippage_basis_points` - The slippage tolerance in basis points (e.g., 50 for 0.5%).
///
/// # Returns
///
/// A Solana `Instruction` for selling tokens on pump.fun.
pub fn build_sell_instruction(
    sell_token_amount: u64,
    slippage_basis_points: u64,
    pump_fun_accounts: &PumpFunAccounts,
    bonding_curve_state: BondingCurve,
) -> Instruction {

    let slippage_factor = 1.0 - slippage_basis_points as f64 / 10000.0;

    let (receive_sol_amount, _) = calculate_pump_fun_swap_amount(
        SwapDirection::Sell,
        bonding_curve_state,
        sell_token_amount,
        0,
    );

    let sell_instruction = build_pump_fun_instruction(
        &pump_fun_accounts,
        SwapDirection::Sell,
        (receive_sol_amount as f64 * slippage_factor) as u64,
        sell_token_amount,
    );

    sell_instruction
}


/// Fast way to build PumpFunAccounts by leveraging the default struct
/// Only sets the dynamic fields, uses defaults for static constants
pub fn get_instruction_accounts(
    account_keys: &[Vec<u8>],
    accounts: &[u8],
) -> PumpFunAccounts {
    // Start with default (which has all the static constants pre-filled)
    let mut pump_fun_accounts = PumpFunAccounts::default();
    
    // Only set the dynamic fields that need to be computed
    let mint = get_account(&account_keys, &accounts,   2);
    let bonding_curve_pda = get_account(&account_keys, &accounts, 3); // Adjust index as needed
    let user = get_wallet_keypair().pubkey();
    
    // Compute derived addresses
    let user_ata = spl_associated_token_account::get_associated_token_address(&user, &mint);
    let bonding_curve_ata = spl_associated_token_account::get_associated_token_address(&bonding_curve_pda, &mint);
    
    // Get creator from bonding curve data (you might need to implement this based on your needs)
    // let creator = get_account(&account_keys, &accounts, 5); // Adjust index as needed
    // let creator_fee_vault = get_creator_fee_vault(&creator);
    
    // Update only the dynamic fields
    pump_fun_accounts.mint = mint;
    pump_fun_accounts.bonding_curve_pda = bonding_curve_pda;
    pump_fun_accounts.bonding_curve_ata = bonding_curve_ata;
    pump_fun_accounts.user_ata = user_ata;
    pump_fun_accounts.user = user;
    pump_fun_accounts.global_volume_accumulator = global_volume_accumulator_pda();
    pump_fun_accounts.user_volume_accumulator = user_volume_accumulator_pda(&user);
    // pump_fun_accounts.creator_fee_vault = creator_fee_vault;
    
    pump_fun_accounts
}

pub fn get_bonding_curve_state(pump_fun_accounts: &PumpFunAccounts) -> BondingCurve {
    let client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    let account_data = client.get_account_data(&pump_fun_accounts.bonding_curve_pda).expect("Failed to get account data");
    
    let bonding_curve_state = BondingCurve::deserialize(&mut &account_data[8..]).expect("Failed to deserialize bonding curve state");
    
    bonding_curve_state
}

// Pump program
pub fn global_volume_accumulator_pda() -> Pubkey {
    let (global_volume_accumulator, _bump) = Pubkey::find_program_address(
        &[b"global_volume_accumulator"],
        &Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P").unwrap(),
    );
    global_volume_accumulator
}

pub fn user_volume_accumulator_pda(user: &Pubkey) -> Pubkey {
    let (user_volume_accumulator, _bump) = Pubkey::find_program_address(
        &[b"user_volume_accumulator", user.as_ref()],
        &Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P").unwrap(),
    );
    user_volume_accumulator
}