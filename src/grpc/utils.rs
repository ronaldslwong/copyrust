use solana_sdk::pubkey::Pubkey;
use crate::utils::token_balance::get_token_balance_change_for_mint;
use crate::grpc::arpc_worker::GLOBAL_TX_MAP;
use std::time::Instant;
use bs58;
use std::str::FromStr;
use solana_transaction_status;


pub fn parse_tx(
    account_key_list: &[Vec<u8>],
    account_list: &[u8],
    ac_x_pos: usize,
    mint_x_byte_pos: usize,
    mint_y_byte_pos: usize,
    data: &[u8],
) -> (Pubkey, u64, u64) {
    // Extract mint
    let mint = if account_list.len() > ac_x_pos {
        let idx = account_list[ac_x_pos] as usize;
        if account_key_list.len() > idx {
            Pubkey::try_from(account_key_list[idx].as_slice()).unwrap_or_default()
        } else {
            Pubkey::default()
        }
    } else {
        Pubkey::default()
    };
    // Extract u1
    let u1 = if data.len() >= mint_x_byte_pos + 8 {
        let slice = &data[mint_x_byte_pos..mint_x_byte_pos + 8];
        u64::from_le_bytes(slice.try_into().unwrap())
    } else {
        0
    };
    // Extract u2
    let u2 = if data.len() >= mint_y_byte_pos + 8 {
        let slice = &data[mint_y_byte_pos..mint_y_byte_pos + 8];
        u64::from_le_bytes(slice.try_into().unwrap())
    } else {
        0
    };
    (mint, u1, u2)
}

/// Calculate the token amount change after a swap based on ParsedTx data
/// Returns the token amount change (post_balance - pre_balance) for the specified mint
/// Returns None if the transaction is not found or the mint is not involved in the transaction
pub fn calculate_token_amount_change(
    parsed_tx: &crate::triton_grpc::crossbeam_worker::ParsedTx,
    target_mint: &Pubkey,
) -> Option<i64> {
    // Extract signature from ParsedTx
    let signature = if let Some(sig_bytes) = &parsed_tx.sig_bytes {
        bs58::encode(sig_bytes).into_string()
    } else {
        return None;
    };

    // Get pre and post balances for the target mint
    let balance_change = get_token_balance_change_for_mint(&signature, target_mint)?;
    let (pre_balance, post_balance) = balance_change;
    
    // Calculate the change (post - pre)
    let change = post_balance as i64 - pre_balance as i64;
    
    Some(change)
}

/// Calculate the token amount change for a transaction using the mint from GLOBAL_TX_MAP
/// This function looks up the transaction in GLOBAL_TX_MAP to get the mint information
pub fn calculate_token_amount_change_from_map(
    parsed_tx: &crate::triton_grpc::crossbeam_worker::ParsedTx,
) -> Option<i64> {
    // Extract signature from ParsedTx
    let sig_bytes = parsed_tx.sig_bytes.as_ref()?;
    
    // Look up the transaction in GLOBAL_TX_MAP to get the mint
    let tx_data = GLOBAL_TX_MAP.get(sig_bytes)?;
    let mint = &tx_data.mint;
    
    // Calculate the token amount change
    calculate_token_amount_change(parsed_tx, mint)
}

/// Get detailed token balance information for a transaction
/// Returns (pre_balance, post_balance, change) for the specified mint
pub fn get_detailed_token_balance_info(
    parsed_tx: &crate::triton_grpc::crossbeam_worker::ParsedTx,
    target_mint: &Pubkey,
) -> Option<(u64, u64, i64)> {
    // Extract signature from ParsedTx
    let signature = if let Some(sig_bytes) = &parsed_tx.sig_bytes {
        bs58::encode(sig_bytes).into_string()
    } else {
        return None;
    };

    // Get pre and post balances for the target mint
    let balance_change = get_token_balance_change_for_mint(&signature, target_mint)?;
    let (pre_balance, post_balance) = balance_change;
    
    // Calculate the change (post - pre)
    let change = post_balance as i64 - pre_balance as i64;
    
    Some((pre_balance, post_balance, change))
}

/// Automatically detect token mint and calculate amount change for simple SOL ↔ token swaps
/// Returns (mint, token_amount_change) for the non-SOL token involved in the swap
/// Returns None if no clear token swap is detected
/// Note: This function requires token balance data to be passed separately
pub fn calculate_token_amount_change_auto_detect(
    parsed_tx: &crate::triton_grpc::crossbeam_worker::ParsedTx,
    pre_token_balances: &[solana_transaction_status::UiTransactionTokenBalance],
    post_token_balances: &[solana_transaction_status::UiTransactionTokenBalance],
) -> Option<(Pubkey, i64)> {
    // Get token balances from parameters (no RPC call needed)
    let pre_balances = pre_token_balances;
    let post_balances = post_token_balances;

    // Find the token mint that had a balance change (excluding SOL/WSOL)
    for pre_balance in pre_balances {
        let mint_str = &pre_balance.mint;
        let mint = match Pubkey::from_str(mint_str) {
            Ok(pk) => pk,
            Err(_) => continue,
        };

        // Skip SOL and WSOL (common SOL representations)
        if mint_str == "So11111111111111111111111111111111111111112" || // SOL
           mint_str == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" || // USDC (often used as SOL proxy)
           mint_str == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" {  // USDT (often used as SOL proxy)
            continue;
        }

        // Find corresponding post balance
        let post_balance = post_balances.iter().find(|b| b.mint == *mint_str);
        
        if let Some(post) = post_balance {
            let pre_amount = pre_balance.ui_token_amount.amount.parse::<u64>().unwrap_or(0);
            let post_amount = post.ui_token_amount.amount.parse::<u64>().unwrap_or(0);
            
            let change = post_amount as i64 - pre_amount as i64;
            
            // If there's a significant change, this is likely our target token
            if change != 0 {
                return Some((mint, change));
            }
        }
    }

    // If no pre-balance found, check post-balances for new tokens
    for post_balance in post_balances {
        let mint_str = &post_balance.mint;
        let mint = match Pubkey::from_str(mint_str) {
            Ok(pk) => pk,
            Err(_) => continue,
        };

        // Skip SOL and WSOL
        if mint_str == "So11111111111111111111111111111111111111112" ||
           mint_str == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" ||
           mint_str == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" {
            continue;
        }

        // Check if this token didn't exist in pre-balances (new token)
        let pre_balance = pre_balances.iter().find(|b| b.mint == *mint_str);
        
        if pre_balance.is_none() {
            let post_amount = post_balance.ui_token_amount.amount.parse::<u64>().unwrap_or(0);
            if post_amount > 0 {
                return Some((mint, post_amount as i64));
            }
        }
    }

    None
}

/// Get detailed token balance info with auto-detected mint
/// Returns (mint, pre_balance, post_balance, change) for the detected token
/// Note: This function requires token balance data to be passed separately
pub fn get_detailed_token_balance_info_auto_detect(
    parsed_tx: &crate::triton_grpc::crossbeam_worker::ParsedTx,
    pre_token_balances: &[solana_transaction_status::UiTransactionTokenBalance],
    post_token_balances: &[solana_transaction_status::UiTransactionTokenBalance],
) -> Option<(Pubkey, u64, u64, i64)> {
    // Get token balances from parameters (no RPC call needed)
    let pre_balances = pre_token_balances;
    let post_balances = post_token_balances;

    // Find the token mint that had a balance change (excluding SOL/WSOL)
    for pre_balance in pre_balances {
        let mint_str = &pre_balance.mint;
        let mint = match Pubkey::from_str(mint_str) {
            Ok(pk) => pk,
            Err(_) => continue,
        };

        // Skip SOL and WSOL
        if mint_str == "So11111111111111111111111111111111111111112" ||
           mint_str == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" ||
           mint_str == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" {
            continue;
        }

        // Find corresponding post balance
        let post_balance = post_balances.iter().find(|b| b.mint == *mint_str);
        
        if let Some(post) = post_balance {
            let pre_amount = pre_balance.ui_token_amount.amount.parse::<u64>().unwrap_or(0);
            let post_amount = post.ui_token_amount.amount.parse::<u64>().unwrap_or(0);
            
            let change = post_amount as i64 - pre_amount as i64;
            
            // If there's a significant change, this is likely our target token
            if change != 0 {
                return Some((mint, pre_amount, post_amount, change));
            }
        }
    }

    // If no pre-balance found, check post-balances for new tokens
    for post_balance in post_balances {
        let mint_str = &post_balance.mint;
        let mint = match Pubkey::from_str(mint_str) {
            Ok(pk) => pk,
            Err(_) => continue,
        };

        // Skip SOL and WSOL
        if mint_str == "So11111111111111111111111111111111111111112" ||
           mint_str == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" ||
           mint_str == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" {
            continue;
        }

        // Check if this token didn't exist in pre-balances (new token)
        let pre_balance = pre_balances.iter().find(|b| b.mint == *mint_str);
        
        if pre_balance.is_none() {
            let post_amount = post_balance.ui_token_amount.amount.parse::<u64>().unwrap_or(0);
            if post_amount > 0 {
                return Some((mint, 0, post_amount, post_amount as i64));
            }
        }
    }

    None
}

/// Calculate token amount change using gRPC transaction data (no RPC call needed)
/// This function extracts token balances from the gRPC feed transaction data
pub fn calculate_token_amount_change_from_grpc_data(
    parsed_tx: &crate::triton_grpc::crossbeam_worker::ParsedTx,
    pre_token_balances: &[solana_transaction_status::UiTransactionTokenBalance],
    post_token_balances: &[solana_transaction_status::UiTransactionTokenBalance],
) -> Option<(Pubkey, i64)> {
    // Find the token mint that had a balance change (excluding SOL/WSOL)
    for pre_balance in pre_token_balances {
        let mint_str = &pre_balance.mint;
        let mint = match Pubkey::from_str(mint_str) {
            Ok(pk) => pk,
            Err(_) => continue,
        };

        // Skip SOL and WSOL (common SOL representations)
        if mint_str == "So11111111111111111111111111111111111111112" || // SOL
           mint_str == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" || // USDC (often used as SOL proxy)
           mint_str == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" {  // USDT (often used as SOL proxy)
            continue;
        }

        // Find corresponding post balance
        let post_balance = post_token_balances.iter().find(|b| b.mint == *mint_str);
        
        if let Some(post) = post_balance {
            let pre_amount = pre_balance.ui_token_amount.amount.parse::<u64>().unwrap_or(0);
            let post_amount = post.ui_token_amount.amount.parse::<u64>().unwrap_or(0);
            
            let change = post_amount as i64 - pre_amount as i64;
            
            // If there's a significant change, this is likely our target token
            if change != 0 {
                return Some((mint, change));
            }
        }
    }

    // If no pre-balance found, check post-balances for new tokens
    for post_balance in post_token_balances {
        let mint_str = &post_balance.mint;
        let mint = match Pubkey::from_str(mint_str) {
            Ok(pk) => pk,
            Err(_) => continue,
        };

        // Skip SOL and WSOL
        if mint_str == "So11111111111111111111111111111111111111112" ||
           mint_str == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" ||
           mint_str == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" {
            continue;
        }

        // Check if this token didn't exist in pre-balances (new token)
        let pre_balance = pre_token_balances.iter().find(|b| b.mint == *mint_str);
        
        if pre_balance.is_none() {
            let post_amount = post_balance.ui_token_amount.amount.parse::<u64>().unwrap_or(0);
            if post_amount > 0 {
                return Some((mint, post_amount as i64));
            }
        }
    }

    None
}
