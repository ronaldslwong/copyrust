// pump_swap.rs
// Build buy and sell instructions for PumpSwap AMM
// Inspired by pump.go and pumpSwap.go (Go code)

use solana_sdk::pubkey::Pubkey;
use std::vec::Vec;
use solana_account_decoder::UiAccountEncoding;
use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use solana_client::rpc_config::RpcProgramAccountsConfig;
use solana_client::rpc_config::RpcAccountInfoConfig;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use std::error::Error;
use std::convert::TryInto;

#[derive(PartialEq, Copy, Clone)]
pub enum SwapDirection {
    Buy,
    Sell,
}


pub fn get_account(account_keys: &[Vec<u8>], accounts: &[u8], index: usize) -> Pubkey {
    if accounts.len() > index {
        let idx = accounts[index] as usize;
        if account_keys.len() > idx {
            Pubkey::try_from(account_keys[idx].as_slice()).unwrap_or_default()
        } else {
            Pubkey::default()
        }
    } else {
        Pubkey::default()
    }
}

pub fn get_pool_accounts(
    mint: Pubkey,
    rpc_client: &RpcClient,
    offset: [u64; 1],
    program_id: Pubkey,
) -> Option<Pubkey> {
    let mint_offsets: [u64; 1] = offset; // or whatever offsets you want

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

    //     let pool_account = Pubkey::from_str("7r96KS2R4WJQs632AoJvdLDeiC77i6qE19Tuju7A2PS").unwrap();

    // // Mint you're searching for
    // let mint_to_find = Pubkey::from_str("5PbctXDry7VFXjMHoGAN72DLv4ouveNoCYUSw6abonk").unwrap();
    // let mint_bytes = mint_to_find.to_bytes();

    // // Fetch account data
    // let account = rpc_client.get_account(&pool_account).unwrap();
    // let data = account.data;

    // println!("Searching {} bytes for mint {}...", data.len(), mint_to_find);

    // let mut matches = 0;
    // for i in 0..=(data.len().saturating_sub(32)) {
    //     let slice = &data[i..i + 32];
    //     if slice == mint_bytes {
    //         println!("✅ Mint matched at offset {}", i);
    //         matches += 1;
    //     }
    // }

    // if matches == 0 {
    //     println!("No match found.");
    // }

    // let b64_data = "9+3j9dfD3kazIT+6i/nIf6keR4GWKMOD4AvqfpjHoD4DuhBpz8P28w4cMKqYHfQFXquWGompdGOEqagNNJHpN29eNNqync0hdS8E8Nj+zzlKXB/UOAoAIV0gBG1oGzcp+SaJnXomsvvuCuG3TCE6I4+bvx41U7m30zkX1dUmQOl0qv9S9jbF7X0QZVNM9nvASW4pR4CB26IMXvl2ck7feLLexsfQv/xNAR/fpIzUc7MQ98o3mopG+SKC1PFNnIDCiHsFPPmXYFUGm4hX/quBhPtof2NGGMA12sQ53BrrO1WYoPAAAAAAAQbd9uHudY/eGEJdvORszdq2GvxNg7kNJ/69+SjYoYv8Bt324ddloZPZy+FGzut5rBy0he1fWzeROoz1hX7/AKleFF03OXdBO+ev3PQOsfwWERI1LQVC1zyhagntMlHKK/0ACQkJaiOEb3prAABCDIgtO1oAAKTRaAgAAAAARMnq5tYGAACcFocAAAAAANvZbWgAAAAALwMAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

    // let data = decode(b64_data).expect("failed to decode base64");
    // println!("Account data length: {}", data.len());

    // // Step 2: Your target mint
    // let mint = Pubkey::from_str("5PbctXDry7VFXjMHoGAN72DLv4ouveNoCYUSw6abonk").unwrap();
    // let mint_bytes = mint.to_bytes();

    // // Step 3: Scan for matching offset
    // for i in 0..=(data.len() - 32) {
    //     if &data[i..i + 32] == mint_bytes {
    //         println!("✅ Mint matched at offset: {}", i);
    //     }
    // }

        match rpc_client.get_program_accounts_with_config(&program_id, config) {
            Ok(accounts) => {
                if accounts.is_empty() {
                    println!("No matches found at offset {}", offset);
                } else {
                    println!("Found {} market(s) at offset {}:", accounts.len(), offset);
                    for (pubkey, _) in accounts {
                        println!(" - Market account: {}", pubkey);
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

pub fn get_constant_product_swap_amount(
    direction: SwapDirection,
    base_reserve: u64,
    quote_reserve: u64,
    swap_amount: u64,
    target_sol_buy: u64,
    target_token_buy: u64,
) -> Result<u64, Box<dyn Error>> {

    let adjusted_price = match direction {
        SwapDirection::Buy => ((base_reserve-target_token_buy) as f64 * swap_amount as f64) / ((quote_reserve + target_sol_buy) as f64 + swap_amount as f64) ,
        SwapDirection::Sell => ((quote_reserve-target_sol_buy) as f64 * swap_amount as f64) / ((base_reserve + target_token_buy) as f64 + swap_amount as f64) ,
    };
    Ok(adjusted_price as u64)
}


pub fn get_pool_vault_amount(
    base_vault: Pubkey,
    quote_vault: Pubkey,
) -> Result<(u64, u64), Box<dyn Error>> {
    let keys = vec![base_vault, quote_vault];
    let rpc_client = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");

    let res = rpc_client.get_multiple_accounts_with_commitment(&keys, CommitmentConfig::processed())?;
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

    Ok((base_amount, quote_amount))
} 
