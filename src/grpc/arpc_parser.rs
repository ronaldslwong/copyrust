use crate::arpc::SubscribeResponse;
use crate::build_tx::pump_fun::{build_buy_instruction};
use crate::build_tx::pump_swap::{build_pump_buy_instruction};
use solana_program::instruction::{Instruction};

use crate::build_tx::tx_builder::build_and_sign_transaction;
use crate::config_load::Config;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use crate::init::wallet_loader::get_wallet_keypair;
use crate::send_tx::nextblock::create_instruction_nextblock;
use bs58;
use chrono::{TimeZone, Utc};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::transaction::Transaction;
use std::str::FromStr;
use std::time::Instant;
use crate::build_tx::tx_builder::create_instruction;
use crate::build_tx::ray_launch::{build_ray_launch_buy_instruction, get_instruction_accounts};
use crate::build_tx::pump_swap::get_account;
use crate::build_tx::ray_launch::RayLaunchAccounts;
use crate::build_tx::ray_cpmm::build_ray_cpmm_buy_instruction;
use crate::send_tx::zero_slot::create_instruction_zeroslot;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;

// use chrono::Local;

// Global map: signature (String) -> Transaction
pub static GLOBAL_TX_MAP: Lazy<DashMap<String, TxWithPubkey>> = Lazy::new(DashMap::new);

// Global atomic counter for ARPC messages
static ARPC_MESSAGE_COUNT: AtomicUsize = AtomicUsize::new(0);

// Function to get the current count (for testing/logging)
pub fn get_arpc_message_count() -> usize {
    ARPC_MESSAGE_COUNT.load(Ordering::Relaxed)
}

// Call this in your ARPC message handler:
// ARPC_MESSAGE_COUNT.fetch_add(1, Ordering::Relaxed);

#[derive(Debug, Clone)]
pub struct ParsedTrade {
    pub sig: String,
    pub program_id: String,
    pub mint: String,
    pub sol_size: f64,
    pub direction: String,
    pub slot: u64,
}#
[derive(Debug, Clone)]

pub struct TxWithPubkey {
    pub tx: Transaction,
    pub bonding_curve: Pubkey,
    pub mint: Pubkey,
    pub token_amount: u64,
    pub tx_type: String,
    pub ray_launch_accounts: RayLaunchAccounts,
    pub ray_cpmm_pool_state: Pubkey,
    pub send_sig: String,
    pub send_time: Instant,
    pub send_slot: u64,
}
pub async fn process_grpc_msg(resp: &SubscribeResponse, config: &Config) -> Option<ParsedTrade> {
    let tx = resp.transaction.as_ref()?;
    let account_keys = &tx.account_keys;
    let slot = tx.slot;
    let sig = tx
        .signatures
        .get(0)
        .map(|s| bs58::encode(s).into_string())
        .unwrap_or_default();
    // println!(
    //     "[{}] - Trade detected - sig: {}",
    //     Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
    //     sig
    // );
    let _detection_delay = if let Some(created_at) = resp.created_at.as_ref() {
        let tx_time = Utc
            .timestamp_opt(created_at.seconds, created_at.nanos as u32)
            .single();
        if let Some(tx_time) = tx_time {
            let now = Utc::now();
            (now - tx_time).num_microseconds().unwrap_or(0) as f64 / 1000.0
        } else {
            0.0
        }
    } else {
        0.0
    };
    let start_time = Instant::now();
    let lamports = (config.buy_sol * 1_000_000_000.0) as u64;

    for instr in &tx.instructions {
        let program_id_index = instr.program_id_index as usize;
        let mut send_tx = false;
        let mut buy_instruction: Instruction = Instruction{
            program_id: Pubkey::new_unique(),
            accounts: vec![],
            data: vec![],
        };
        let mut token_amount = 0;
        let mut tx_type = "unknown".to_string();

        if let Some(account_inst_bytes) = account_keys.get(program_id_index) {
            let account_inst = Pubkey::try_from(account_inst_bytes.as_slice())
                .map(|pk| pk.to_string())
                .unwrap_or_default();
            // println!("account_inst: {:?}", account_inst);
            // for mkt in &config.accounts_monitor {
            //     if &account_inst == mkt {
            let data = &instr.data;
            let account_list = &instr.accounts;
            let mut sol_size = 0.0;
            let mut mint = Pubkey::default();
            let mut direction = "unknown".to_string();
            let mut bonding_curve_state = Pubkey::default();
            let mut ray_cpmm_pool_state = Pubkey::default();
            let mut ray_launch_accounts = RayLaunchAccounts{
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
            };

            if account_inst == "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"
                && account_keys.len() > 10
                && data.len() > 16
            {
                direction = if account_inst == "So11111111111111111111111111111111111111112" {
                    "SELL".to_string()
                } else {
                    "BUY".to_string()
                };
                let (m, s, _, _) = parse_tx(
                    &account_inst,
                    data,
                    account_keys,
                    account_list,
                    3,
                    4,
                    16,
                    8,
                    &direction,
                );
                mint = m;
                sol_size = s;
            }
            if account_inst == "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo"
                && account_keys.len() > 10
                && data.len() > 16
            {
                if data.len() > 8 && &data[0..8] == [65, 75, 63, 76, 235, 91, 91, 136] {
                    direction = if data.len() > 2 && &data[data.len() - 2..] == [1, 0] {
                        "BUY".to_string()
                    } else {
                        "SELL".to_string()
                    };
                    let (m, s, u1, _) = parse_tx(
                        &account_inst,
                        data,
                        account_keys,
                        account_list,
                        6,
                        7,
                        16,
                        8,
                        &direction,
                    );
                    if u1 == 0 {
                        sol_size = 0.0;
                    } else {
                        sol_size = s;
                    }
                    mint = m;
                }
            }
            if account_inst == "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj"
            {
                if data.len() > 8 && &data[0..8] == [250, 234, 13, 123, 213, 156, 19, 236] {
                    let (m, s, u1, u2) = parse_tx(
                        &account_inst,
                        data,
                        account_keys,
                        account_list,
                        9,
                        10,
                        16,
                        8,
                        &direction,
                    );
                    println!("sig: {}, mint: {}, sol_size: {}, u1: {}, u2: {}", sig, m, s, u1, u2);
                    println!(
                        "[{}] - [arpc] Raydium Launchpad buy detected | elapsed: {:.2?} | sig: {}",
                        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                        start_time.elapsed(),
                        sig.clone()
                    );
                    if u1 == 0 {
                        sol_size = 0.0;
                    } else {
                        sol_size = s;
                    }
                    mint = m;
                    bonding_curve_state = get_account(account_keys.clone(), instr.accounts.clone(), 4);
                    // println!("bonding_curve_state: {}", bonding_curve_state.to_string());
                    ray_launch_accounts = get_instruction_accounts(account_keys.clone(), instr.accounts.clone());
                    (buy_instruction, token_amount) = build_ray_launch_buy_instruction(
                        lamports,
                        config.buy_slippage_bps,
                        ray_launch_accounts.clone(),
                        u2,u1
                        // u1,
                        // u2,
                    );
                    //define fields for tx_with_pubkey
                    // println!("sig: {}, token_amount: {}", sig, token_amount);
                    send_tx = true;
                    tx_type = "raylaunch".to_string();


                    // ARPC_MESSAGE_COUNT.fetch_add(1, Ordering::Relaxed);
                    // let arpc_message_count = get_arpc_message_count();
                    // println!("arpc_message_count: {:?}", arpc_message_count);

                }
            }
            if account_inst == "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C" //raydium cpmm
            {
                if data.len() > 8 && (&data[0..8] == [143, 190, 90, 218, 196, 30, 51, 222] || &data[0..8] == [55, 217, 98, 86, 163, 74, 180, 173]) {
                    let (m, s, u1, u2) = parse_tx(
                        &account_inst,
                        data,
                        account_keys,
                        account_list,
                        11,
                        10,
                        16,
                        8,
                        &direction,
                    );
                    mint = m;
                    if u1 > u2 {
                        println!("sig: {}, mint: {}, sol_size: {}, u1: {}, u2: {}", sig, m, s, u1, u2);
                        println!(
                            "[{}] - [arpc] Raydium CPMM buy detected | elapsed: {:.2?} | sig: {}",
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                            start_time.elapsed(),
                            sig.clone()
                        );
                        ray_cpmm_pool_state = get_account(account_keys.clone(), instr.accounts.clone(), 3);
                        (buy_instruction, token_amount) = build_ray_cpmm_buy_instruction(
                            ray_cpmm_pool_state,
                            lamports,
                            config.buy_slippage_bps,
                            m,
                        );
                        //define fields for tx_with_pubkey
                        // println!("sig: {}, token_amount: {}", sig, token_amount)
                        if token_amount > 0 {
                            println!("sig: {}, token_amount: {}", sig, token_amount);
                            send_tx = true;
                            tx_type = "ray_cpmm".to_string();

                            // ARPC_MESSAGE_COUNT.fetch_add(1, Ordering::Relaxed);
                            // let arpc_message_count = get_arpc_message_count();
                            // println!("arpc_message_count: {:?}", arpc_message_count);

                        }
                    }




                }
            }
            if account_inst == "BSfD6SHZigAfDWSjzD5Q41jw8LmKwtmjskPH9XW1mrRW"
                && account_keys.len() > 10
                && data.len() > 16
            {
                let (m, s, u1, u2) = parse_tx(
                    &account_inst,
                    data,
                    account_keys,
                    account_list,
                    3,
                    4,
                    16,
                    8,
                    &direction,
                );
                if u1 == 0 {
                    sol_size = 0.0;
                } else {
                    sol_size = s;
                }
                mint = m;
                // println!("tx: {:#?}", instr);
                let mut first_three_invalid = true;
                for (i, &x) in instr.accounts.iter().enumerate() {
                    let empty_key = Vec::new();
                    let key_bytes = account_keys.get(x as usize).unwrap_or(&empty_key);
                    let pubkey_str = Pubkey::try_from(key_bytes.as_slice())
                        .map(|p| p.to_string())
                        .unwrap_or_else(|_| "[Invalid Pubkey]".to_string());
                    // println!("account: {}", pubkey_str);
                    // Check if the first three are all "[Invalid Pubkey]"
                    // let mut bonding_curve_state: Option<BondingCurve> = None;

                    if i < 3 && pubkey_str != "[Invalid Pubkey]" {
                        first_three_invalid = false;
                        // println!("BondingCurve: {:?}", bonding_curve_state);
                    }
                }

                // After the loop, you can check:
                if instr.accounts.len() >= 3 && first_three_invalid {

                    // check if buy or not
                    let expected_buy: [u8; 8] = [0x52, 0xE1, 0x77, 0xE7, 0x4E, 0x1D, 0x2D, 0x46];
                    direction = if data.len() >= 8 && &data[..8] == expected_buy {
                        "BUY".to_string()
                    } else {
                        "SELL".to_string()
                    };
                    if direction == "BUY" {
                        println!(
                            "[{}] - [arpc] PumpFun buy detected | elapsed: {:.2?} | sig: {}",
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                            start_time.elapsed(),
                            sig.clone()
                        );
                        bonding_curve_state = Pubkey::try_from(
                            account_keys
                                .get(*instr.accounts.get(4).unwrap_or(&0) as usize)
                                .unwrap()
                                .as_slice(),
                        )
                        .unwrap();
                        (buy_instruction, token_amount) = match build_buy_instruction(
                            get_wallet_keypair().pubkey(),
                            mint,
                            bonding_curve_state,
                            lamports,
                            config.buy_slippage_bps,
                            u1,
                            u2,
                        ) {
                            Ok(result) => result,
                            Err(e) => {
                                eprintln!(
                                    "[{}] - Failed to build buy instruction for mint {}: {}",
                                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                                    mint,
                                    e
                                );
                                return None; // or handle as appropriate for your function
                            }
                        };
                        send_tx = true;
                        tx_type = "pumpfun".to_string();

                    }
                }
                if data[0..8] == [0x2c, 0x77, 0xaf, 0xda, 0xc7, 0x4d, 0xc4, 0xeb] {
                    //pumpswap
                    let (mint, s, u1, u2) = parse_tx(
                        &account_inst,
                        data,
                        account_keys,
                        account_list,
                        3,
                        4,
                        16,
                        8,
                        &direction,
                    );

                    if u1 > u2 {
                        println!(
                            "[{}] - [arpc] PumpSwap buy detected | elapsed: {:.2?} | sig: {}",
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                            start_time.elapsed(),
                            sig.clone()
                        );
                        //build buy instruction
                        (buy_instruction, token_amount) = build_pump_buy_instruction(
                            lamports,
                            config.buy_slippage_bps,
                            account_keys.clone(),
                            instr.accounts.clone(),
                            u2,
                            u1,
                        );
                        //define fields for tx_with_pubkey
                        send_tx = true;
                        tx_type = "pumpswap".to_string();

                        // {
                        //     Ok(result) => result,
                        //     Err(e) => {
                        //         eprintln!(
                        //             "[{}] - Failed to build buy instruction for mint {}: {}",
                        //             Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                        //             mint,
                        //             e
                        //         );
                        //         return None; // or handle as appropriate for your function
                        //     }
                        // };


                        // //sell

                        // let (sell_instruction) = build_pump_sell_instruction(
                        //     token_amount,
                        //     config.sell_slippage_bps,
                        //     account_keys.clone(),
                        //     instr.accounts.clone(),
                        // );

                        // // {
                        // //     Ok(result) => result,
                        // //     Err(e) => {
                        // //         eprintln!(
                        // //             "[{}] - Failed to build buy instruction for mint {}: {}",
                        // //             Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                        // //             mint,
                        // //             e
                        // //         );
                        // //         return None; // or handle as appropriate for your function
                        // //     }
                        // // };

                        // let rpc = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
                        // println!(
                        //     "[{}] - mint: {}, elapsed: {:.2?}",
                        //     Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                        //     mint.to_string(),
                        //     start_time.elapsed()
                        // );
                        // let compute_budget_instruction = create_instruction_nextblock(
                        //     config.cu_limit,
                        //     config.nextblock_cu_price,
                        //     mint,
                        //     vec![sell_instruction],
                        //     (config.nextblock_buy_tip * 1_000_000_000.0) as u64,
                        // );
                        // let tx = build_and_sign_transaction(
                        //     rpc,
                        //     &compute_budget_instruction,
                        //     get_wallet_keypair(),
                        // )
                        // .ok()?;
                        // println!(
                        //     "[{}] - Signed tx, elapsed: {:.2?}",
                        //     Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                        //     start_time.elapsed()
                        // );

                        // let sig = send_tx_nextblock(&tx, &config.nextblock_api).await.unwrap();
                        // // let elapsed = start_time.elapsed();
                        // // println!("Sent tx with sig: {} (elapsed: {:.2?})", sig, elapsed);
                        // let now = Utc::now();
                        // println!(
                        //     "[{}] - Sent tx with sig: {}",
                        //     now.format("%Y-%m-%d %H:%M:%S%.3f"),
                        //     sig
                        // );

                        println!("")
                    }
                }
            }
            // if send_tx && get_arpc_message_count() == 1 {
            if send_tx {

                let build_time = Instant::now();
                //build buy instruction
                let rpc = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
                // let compute_budget_instruction = create_instruction_nextblock(
                //     config.cu_limit,
                //     config.nextblock_cu_price,
                //     mint,
                //     vec![buy_instruction.clone()],
                //     (config.nextblock_buy_tip * 1_000_000_000.0) as u64,
                // );
                let compute_budget_instruction = create_instruction(
                    config.cu_limit,
                    config.cu_price0_slot,
                    mint,
                    vec![buy_instruction.clone()],
                );
                // let final_instruction = create_instruction_nextblock(compute_budget_instruction,  (config.nextblock_buy_tip * 1_000_000_000.0) as u64);
                let final_instruction = create_instruction_zeroslot(compute_budget_instruction,  (config.zeroslot_buy_tip * 1_000_000_000.0) as u64);

                let tx = build_and_sign_transaction(
                    rpc,
                    &final_instruction,
                    get_wallet_keypair(),
                )
                .ok()?;
                println!(
                    "[{}] - Signed tx, elapsed: {:.2?}",
                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    start_time.elapsed()
                );

                // let sig = send_tx_nextblock(&tx, &config.nextblock_api).await.unwrap();
                // let elapsed = start_time.elapsed();
                // println!("Sent tx with sig: {} (elapsed: {:.2?})", sig, elapsed);
                // let now = Utc::now();
                // println!(
                //     "[{}] - Sent tx with sig: {}",
                //     now.format("%Y-%m-%d %H:%M:%S%.3f"),
                //     sig
                // );

                //load tx data to GLOBAL_TX_MAP for send on grpc detection
                let tx_with_pubkey = TxWithPubkey {
                    tx: tx,
                    bonding_curve: bonding_curve_state,
                    mint: mint,
                    token_amount: token_amount,
                    tx_type: tx_type,
                    ray_launch_accounts: ray_launch_accounts,
                    send_sig: "".to_string(),
                    send_time: Instant::now(),
                    send_slot: slot,
                    ray_cpmm_pool_state: ray_cpmm_pool_state,
                };
                // println!("tx_with_pubkey 0: {:?}", tx_with_pubkey);
                // println!("sig: {}, tx_with_pubkey 0: {:?}", sig, tx_with_pubkey);
                GLOBAL_TX_MAP.insert(sig.clone(), tx_with_pubkey);

                let now = Utc::now();
                println!("[{}] - TX built | detected for slot {} | time to build tx: {:.2?} | time to parse: {:.2?}, waiting for grpc confirmation", now.format("%Y-%m-%d %H:%M:%S%.3f"), slot, start_time.elapsed(), build_time.elapsed());
                println!("arpc_message_count: {:?}", get_arpc_message_count());

            }

        }
    }
    None
}

fn parse_tx(
    program: &str,
    data: &[u8],
    account_key_list: &[Vec<u8>],
    account_list: &[u8],
    ac_x_pos: usize,
    ac_y_pos: usize,
    mint_x_byte_pos: usize,
    mint_y_byte_pos: usize,
    buy_sell: &str,
) -> (Pubkey, f64, u64, u64) {
    let mintx = if account_list.len() > ac_x_pos {
        let idx = account_list[ac_x_pos] as usize;
        
        if account_key_list.len() > idx {
            Pubkey::try_from(account_key_list[idx].as_slice()).unwrap_or_default()
        } else {
            eprintln!(
                "[{}] - [parse_tx] x account_key_list too short: idx={}, len={}",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                idx,
                account_key_list.len()
            );
            Pubkey::default()
        }
    } else {
        eprintln!(
            "[{}] - [parse_tx] account_list too short for ac_x_pos: {}",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            ac_x_pos
        );
        Pubkey::default()
    };
    let minty = if account_list.len() > ac_y_pos {
        let idx = account_list[ac_y_pos] as usize;
        if account_key_list.len() > idx {
            Pubkey::try_from(account_key_list[idx].as_slice()).unwrap_or_default()
        } else {
            // eprintln!("[parse_tx] y account_key_list too short: idx={}, len={}", idx, account_key_list.len());
            Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap()
        }
    } else {
        eprintln!(
            "[{}] - [parse_tx] account_list too short for ac_y_pos: {}",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            ac_y_pos
        );
        Pubkey::default()
    };
    let u1 = if data.len() >= mint_x_byte_pos + 8 {
        let slice = &data[mint_x_byte_pos..mint_x_byte_pos + 8];
        u64::from_le_bytes(slice.try_into().unwrap())
    } else {
        eprintln!(
            "[{}] - [parse_tx] data too short for mint_x_byte_pos: {}",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            mint_x_byte_pos
        );
        0
    };
    let u2 = if data.len() >= mint_y_byte_pos + 8 {
        let slice = &data[mint_y_byte_pos..mint_y_byte_pos + 8];
        u64::from_le_bytes(slice.try_into().unwrap())
    } else {
        eprintln!(
            "[{}] - [parse_tx] data too short for mint_y_byte_pos: {}",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            mint_y_byte_pos
        );
        0
    };
    let mut sol_size;
    let mint;
    if mintx.to_string() == "So11111111111111111111111111111111111111112" {
        sol_size = u2 as f64 / 1_000_000_000.0;
        mint = minty;
    } else {
        sol_size = u1 as f64 / 1_000_000_000.0;
        mint = mintx;
    }
    if program == "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo" {
        if buy_sell == "BUY" {
            sol_size = u2 as f64 / 1_000_000_000.0;
        } else {
            sol_size = u1 as f64 / 1_000_000_000.0;
        }
    }
    (mint, sol_size, u1, u2)
}

/// Parses a slice of bytes using a sliding window, printing any valid 32-byte chunk
/// that can be interpreted as a Solana public key. This is useful for finding keys
/// that may not be aligned on a 32-byte boundary.
///
/// # Arguments
///
/// * `data` - A slice of bytes to be scanned.
pub fn print_pubkeys_from_data(data: &[u8]) {
    println!("--- Scanning Data for Pubkeys (Sliding Window) ---");
    if data.len() < 32 {
        println!("  Data is too short to contain a valid Pubkey.");
        println!("--------------------------------------------------");
        return;
    }

    for i in 0..=(data.len() - 32) {
        let chunk = &data[i..i + 32];
        if let Ok(pubkey) = Pubkey::try_from(chunk) {
            // To avoid spamming with system program or other common keys,
            // you might add a filter here if needed.
            println!("  Found Pubkey at index {}: {}", i, pubkey.to_string());
        }
    }
    println!("--------------------------------------------------");
}
