// use crate::pumpfun::{Config, load_config};
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    system_program,
};
use solana_sdk::pubkey::Pubkey;
/// Constants related to the pump.fun program.
pub mod pump_fun_constants {
    use solana_program::pubkey;
    use solana_program::pubkey::Pubkey;

    /// The program ID for the pump.fun protocol.
    pub const PUMP_FUN_PROGRAM_ID: Pubkey = pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
    /// The global account for the pump.fun protocol.
    pub const GLOBAL_ACCOUNT: Pubkey = pubkey!("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf");
    /// The fee recipient account for the pump.fun protocol.
    pub const FEE_RECIPIENT: Pubkey = pubkey!("FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz");
    /// The SPL Token program ID.
    pub const MINT_AUTHORITY: Pubkey = pubkey!("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");
}

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

impl BondingCurve {
    /// Calculates the amount of tokens a user will receive for a given amount of SOL.
    /// This implementation is derived from the reference you provided.
    pub fn get_buy_price(&self, sol_in: u64) -> u64 {
        // let virtual_sol_reserves_after = self.virtual_sol_reserves.checked_add(sol_in).unwrap();
        // let virtual_token_reserves_after = (self.virtual_sol_reserves as u128)
        //     .checked_mul(self.virtual_token_reserves as u128)
        //     .unwrap()
        //     .checked_div(virtual_sol_reserves_after as u128)
        //     .unwrap() as u64;

        // self.virtual_token_reserves
        //     .checked_sub(virtual_token_reserves_after)
        //     .unwrap()



                    // Calculate the product of virtual reserves using u128 to avoid overflow
        let n: u128 = (self.virtual_sol_reserves as u128) * (self.virtual_token_reserves as u128);

        // Calculate the new virtual sol reserves after the purchase
        let i: u128 = (self.virtual_sol_reserves as u128) + (sol_in as u128);

        // Calculate the new virtual token reserves after the purchase
        let r: u128 = n / i + 1;

        // Calculate the amount of tokens to be purchased
        let s: u128 = (self.virtual_token_reserves as u128) - r;

        // Convert back to u64 and return the minimum of calculated tokens and real reserves
        let s_u64 = s as u64;
        if s_u64 < self.real_token_reserves {
            s_u64
        } else {
            self.real_token_reserves
        }
    }

    pub fn get_sell_price(&self, amount: u64, fee_basis_points: u64) -> u64 {
        // if self.complete {
        //     return Err("Curve is complete");
        // }

        // if amount == 0 {
        //     return Ok(0);
        // }

        // Calculate the proportional amount of virtual sol reserves to be received using u128
        let n: u128 = ((amount as u128) * (self.virtual_sol_reserves as u128))
            / ((self.virtual_token_reserves as u128) + (amount as u128));

        // Calculate the fee amount in the same units
        let a: u128 = (n * (fee_basis_points as u128)) / 10000;

        // Return the net amount after deducting the fee, converting back to u64
        (n - a) as u64
    }
}

/// Builds a buy instruction for the pump.fun protocol.
///
/// # Arguments
///
/// * `user` - The public key of the buyer (the signer).
/// * `mint` - The public key of the token's mint address.
/// * `sol_amount` - The amount of SOL to spend.
/// * `slippage_basis_points` - The slippage tolerance in basis points (e.g., 50 for 0.5%).
///
/// # Returns
///
/// A Solana `Instruction` for buying tokens on pump.fun.
pub fn build_buy_instruction(
    user: Pubkey,
    mint: Pubkey,
    bonding_curve_pda: Pubkey,
    sol_amount: u64,
    slippage_basis_points: u64,
    buy_sol: u64, 
    token_amount: u64,
) -> Result<(Instruction, u64), Box<dyn std::error::Error>> {
    // 2. Fetch the account data
    let client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    println!("bonding_curve_pda: {:?}", bonding_curve_pda);
    let account_data = client.get_account_data(&bonding_curve_pda)?;

    // 3. Deserialize the BondingCurve (skip 8 bytes if there's a discriminator)
    let mut bonding_curve_state = BondingCurve::deserialize(&mut &account_data[8..])?;
    bonding_curve_state.virtual_sol_reserves = bonding_curve_state.virtual_sol_reserves + buy_sol;
    bonding_curve_state.virtual_token_reserves = bonding_curve_state.virtual_token_reserves - token_amount;
    // 4. Calculate the expected amount of tokens to receive
    let token_amount = bonding_curve_state.get_buy_price(sol_amount); // Placeholder for buy_sol and token_amount

    // 5. Calculate the maximum SOL cost including slippage
    let max_sol_cost = sol_amount
        .checked_add(
            (sol_amount as u128)
                .checked_mul(slippage_basis_points as u128)
                .unwrap()
                .checked_div(10000)
                .unwrap() as u64,
        )
        .unwrap();

    // 6. Derive PDAs and ATAs as before
    // let (event_authority, _) =
    //     Pubkey::find_program_address(&[b"__events"], &pump_fun_constants::PUMP_FUN_PROGRAM_ID);
    let user_ata = spl_associated_token_account::get_associated_token_address(&user, &mint);
    let bonding_curve_ata =
        spl_associated_token_account::get_associated_token_address(&bonding_curve_pda, &mint);

    // 7. Build the instruction data
    let instruction_data = {
        let buy_discriminator: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(&buy_discriminator);
        data.extend_from_slice(&token_amount.to_le_bytes());
        data.extend_from_slice(&max_sol_cost.to_le_bytes());
        data
    };

    // 8. Build the accounts list
    let accounts = vec![
        AccountMeta::new_readonly(pump_fun_constants::GLOBAL_ACCOUNT, false),
        AccountMeta::new(pump_fun_constants::FEE_RECIPIENT, false),
        AccountMeta::new_readonly(mint, false),
        AccountMeta::new(bonding_curve_pda, false),
        AccountMeta::new(bonding_curve_ata, false),
        AccountMeta::new(user_ata, false),
        AccountMeta::new(user, true),
        AccountMeta::new_readonly(system_program::ID, false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new(get_creator_fee_vault(&bonding_curve_state.creator), false),
        AccountMeta::new_readonly(pump_fun_constants::MINT_AUTHORITY, false),
        AccountMeta::new_readonly(pump_fun_constants::PUMP_FUN_PROGRAM_ID, false),
    ];

    Ok((
        Instruction {
            program_id: pump_fun_constants::PUMP_FUN_PROGRAM_ID,
            accounts,
            data: instruction_data,
        },
        token_amount,
    ))
}

/// Calculate the creator fee vault PDA for a given mint address.
pub fn get_creator_fee_vault(creator_vault: &Pubkey) -> Pubkey {
    let program_id = pump_fun_constants::PUMP_FUN_PROGRAM_ID;
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
    user: Pubkey,
    mint: Pubkey,
    bonding_curve_pda: Pubkey,
    token_amount: u64,
    slippage_basis_points: u64,
) -> Result<Instruction, Box<dyn std::error::Error>> {
    let client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    let account_data = client.get_account_data(&bonding_curve_pda)?;
    let bonding_curve_state = BondingCurve::deserialize(&mut &account_data[8..])?;

    // Calculate the expected amount of SOL to receive for selling these tokens
    // (You may want to implement a get_sell_price method on BondingCurve for more accuracy)
    // let sol_amount = token_amount; // Placeholder: replace with actual calculation if available
    let sol_amount = bonding_curve_state.get_sell_price(token_amount, 95);
    // Calculate the minimum SOL to receive after slippage
    let min_sol_receive = sol_amount
        .checked_sub(
            (sol_amount as u128)
                .checked_mul(slippage_basis_points as u128)
                .unwrap()
                .checked_div(10000)
                .unwrap() as u64,
        )
        .unwrap();

    let user_ata = spl_associated_token_account::get_associated_token_address(&user, &mint);
    let bonding_curve_ata =
        spl_associated_token_account::get_associated_token_address(&bonding_curve_pda, &mint);

    // Discriminator for 'sell' instruction: 33e685a4017f83ad
    let sell_discriminator: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
    let mut instruction_data = Vec::with_capacity(24);
    instruction_data.extend_from_slice(&sell_discriminator);
    instruction_data.extend_from_slice(&token_amount.to_le_bytes());
    instruction_data.extend_from_slice(&min_sol_receive.to_le_bytes());

    let accounts = vec![
        AccountMeta::new_readonly(pump_fun_constants::GLOBAL_ACCOUNT, false),
        AccountMeta::new(pump_fun_constants::FEE_RECIPIENT, false),
        AccountMeta::new_readonly(mint, false),
        AccountMeta::new(bonding_curve_pda, false),
        AccountMeta::new(bonding_curve_ata, false),
        AccountMeta::new(user_ata, false),
        AccountMeta::new(user, true),
        AccountMeta::new_readonly(system_program::ID, false),
        AccountMeta::new(get_creator_fee_vault(&bonding_curve_state.creator), false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(pump_fun_constants::MINT_AUTHORITY, false),
        AccountMeta::new_readonly(pump_fun_constants::PUMP_FUN_PROGRAM_ID, false),
    ];

    Ok(Instruction {
        program_id: pump_fun_constants::PUMP_FUN_PROGRAM_ID,
        accounts,
        data: instruction_data,
    })
}
