use crate::config_load::GLOBAL_CONFIG;
use crate::geyser::{subscribe_update::UpdateOneof, SubscribeUpdate};
use crate::init::initialize::GLOBAL_RPC_CLIENT; // or wherever you defined it
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signer;
use crate::init::wallet_loader::get_wallet_keypair;
use crate::triton_grpc::crossbeam_worker::{ParsedTx, send_parsed_tx, is_signature_processed_by_feed};
use chrono::Utc;
use solana_sdk::pubkey::Pubkey;
use bs58;

// Helper function to check if wallet or monitored accounts are signers/programs
fn check_transaction_permissions(
    tx: &crate::solana::storage::confirmed_block::Transaction,
    wallet_pubkey_bytes: &[u8],
    monitored_accounts: &[Vec<u8>],
) -> (bool, bool, bool) {
    if let Some(message) = &tx.message {
        if let Some(header) = &message.header {
            let num_signers = header.num_required_signatures as usize;
            let signer_accounts: Vec<&[u8]> = message.account_keys
                .iter()
                .take(num_signers)
                .map(|bytes| bytes.as_slice())
                .collect();
            
            // Check if our wallet is a signer
            let is_wallet_signer = signer_accounts.iter()
                .any(|&bytes| bytes == wallet_pubkey_bytes);
            
            // Check if any monitored account is a signer
            let is_monitored_signer = signer_accounts.iter()
                .any(|&signer_bytes| {
                    monitored_accounts.iter()
                        .any(|&ref monitored_bytes| signer_bytes == monitored_bytes)
                });
            
            // Check if any monitored account is used as a program ID in instructions
            let is_monitored_program = message.instructions.iter()
                .any(|instruction| {
                    if (instruction.program_id_index as usize) < message.account_keys.len() {
                        let program_id_bytes = &message.account_keys[instruction.program_id_index as usize];
                        monitored_accounts.iter()
                            .any(|&ref monitored_bytes| program_id_bytes.as_slice() == monitored_bytes)
                    } else {
                        false
                    }
                });
            
            (is_wallet_signer, is_monitored_signer, is_monitored_program)
        } else { 
            (false, false, false) 
        }
    } else { 
        (false, false, false) 
    }
}

// Helper function to extract SOL and mint token amounts from instruction data
fn extract_amounts_from_instructions(
    tx_info: &crate::geyser::SubscribeUpdateTransactionInfo,
) -> (Option<u64>, Option<u64>) {
    if let Some(meta) = &tx_info.meta {
        if !meta.inner_instructions.is_empty() {
            // Look for the first inner instruction that might contain buy data
            let first_inner_instr = meta.inner_instructions.iter()
                .flat_map(|group| &group.instructions)
                .next();
            
            if let Some(instr) = first_inner_instr {
                // Try to extract amounts from instruction data
                let sol_amount = if instr.data.len() >= 8 {
                    let mut amount_bytes = [0u8; 8];
                    amount_bytes.copy_from_slice(&instr.data[0..8]);
                    u64::from_le_bytes(amount_bytes)
                } else {
                    0
                };
                
                let mint_amount = if instr.data.len() >= 16 {
                    let mut amount_bytes = [0u8; 8];
                    amount_bytes.copy_from_slice(&instr.data[8..16]);
                    u64::from_le_bytes(amount_bytes)
                } else {
                    0
                };
                
                (Some(sol_amount), Some(mint_amount))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    }
}

// Helper function to get signer pubkey and SOL balance changes
fn get_signer_and_sol_changes(
    tx_info: &crate::geyser::SubscribeUpdateTransactionInfo,
) -> (Option<solana_sdk::pubkey::Pubkey>, Option<i64>) {
    if let Some(meta) = &tx_info.meta {
        if let Some(tx) = &tx_info.transaction {
            if let Some(msg) = &tx.message {
                let signer = if !msg.account_keys.is_empty() {
                    solana_sdk::pubkey::Pubkey::try_from(msg.account_keys[0].as_slice()).ok()
                } else {
                    None
                };
                
                let sol_change = if let Some(signer_pk) = &signer {
                    let signer_index = msg.account_keys.iter().position(|key| {
                        solana_sdk::pubkey::Pubkey::try_from(key.as_slice()).ok() == Some(*signer_pk)
                    });
                    
                    if let Some(idx) = signer_index {
                        let pre_sol = meta.pre_balances.get(idx).copied().unwrap_or(0);
                        let post_sol = meta.post_balances.get(idx).copied().unwrap_or(0);
                        let change = post_sol as i64 - pre_sol as i64;
                        
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON-PARSER] DEBUG - SOL balance details:");
                            println!("[TRITON-PARSER] DEBUG -   Signer index in account keys: {}", idx);
                            println!("[TRITON-PARSER] DEBUG -   Pre-balance: {} lamports", pre_sol);
                            println!("[TRITON-PARSER] DEBUG -   Post-balance: {} lamports", post_sol);
                            println!("[TRITON-PARSER] DEBUG -   Change: {} lamports ({})", 
                                change, if change > 0 { "gain" } else { "loss" });
                            
                            // Also show all pre/post balances for context
                            println!("[TRITON-PARSER] DEBUG - All SOL balances:");
                            println!("[TRITON-PARSER] DEBUG -   Pre-balances: {:?}", meta.pre_balances);
                            println!("[TRITON-PARSER] DEBUG -   Post-balances: {:?}", meta.post_balances);
                        }
                        
                        Some(change)
                    } else {
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON-PARSER] DEBUG - Could not find signer in account keys");
                            println!("[TRITON-PARSER] DEBUG - Available account keys:");
                            for (i, key) in msg.account_keys.iter().enumerate() {
                                let pubkey = solana_sdk::pubkey::Pubkey::try_from(key.as_slice()).unwrap_or_default();
                                println!("[TRITON-PARSER] DEBUG -   Index {}: {}", i, pubkey);
                            }
                        }
                        None
                    }
                } else {
                    None
                };
                
                (signer, sol_change)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    }
}

// Helper function to extract token balances for the signer
fn extract_token_balances(
    tx_info: &crate::geyser::SubscribeUpdateTransactionInfo,
    signer_pubkey: &Option<solana_sdk::pubkey::Pubkey>,
) -> (Option<Vec<crate::triton_grpc::crossbeam_worker::SimpleTokenBalance>>, 
      Option<Vec<crate::triton_grpc::crossbeam_worker::SimpleTokenBalance>>) {
    if let Some(meta) = &tx_info.meta {
        // OPTIMIZATION: Reduce debug logging in critical path
        #[cfg(feature = "verbose_logging")]
        {
            println!("[TRITON-PARSER] DEBUG - extract_token_balances: Found {} pre and {} post token balances in meta", 
                meta.pre_token_balances.len(), meta.post_token_balances.len());
            
            // DEBUG: Show all token balances before filtering
            println!("[TRITON-PARSER] DEBUG - All pre-token balances:");
            for (i, balance) in meta.pre_token_balances.iter().enumerate() {
                println!("[TRITON-PARSER] DEBUG -   Pre[{}]: mint={}, owner={}, amount={}", 
                    i, balance.mint, balance.owner, 
                    balance.ui_token_amount.as_ref().map(|ui| &ui.amount).unwrap_or(&String::new()));
            }
            println!("[TRITON-PARSER] DEBUG - All post-token balances:");
            for (i, balance) in meta.post_token_balances.iter().enumerate() {
                println!("[TRITON-PARSER] DEBUG -   Post[{}]: mint={}, owner={}, amount={}", 
                    i, balance.mint, balance.owner, 
                    balance.ui_token_amount.as_ref().map(|ui| &ui.amount).unwrap_or(&String::new()));
            }
            
            if let Some(ref signer) = signer_pubkey {
                println!("[TRITON-PARSER] DEBUG - Looking for balances owned by signer: {}", signer);
            } else {
                println!("[TRITON-PARSER] DEBUG - No signer pubkey provided");
            }
        }
        
        let pre_balances: Vec<crate::triton_grpc::crossbeam_worker::SimpleTokenBalance> = meta.pre_token_balances.iter().filter_map(|balance| {
            // Only include token balances where the owner is the transaction signer
            if let Some(signer) = signer_pubkey {
                if balance.owner == signer.to_string() {
                    // OPTIMIZATION: Reduce debug logging in critical path
                    #[cfg(feature = "verbose_logging")]
                    {
                        println!("[TRITON-PARSER] DEBUG - Found matching pre-balance: mint={}, owner={}", balance.mint, balance.owner);
                    }
                    balance.ui_token_amount.as_ref().map(|ui_amount| {
                        crate::triton_grpc::crossbeam_worker::SimpleTokenBalance {
                            mint: balance.mint.clone(),
                            ui_amount: ui_amount.ui_amount,
                            decimals: ui_amount.decimals,
                            amount: ui_amount.amount.clone(),
                            owner: balance.owner.clone(),
                            program_id: balance.program_id.clone(),
                        }
                    })
                } else {
                    // OPTIMIZATION: Reduce debug logging in critical path
                    #[cfg(feature = "verbose_logging")]
                    {
                        println!("[TRITON-PARSER] DEBUG - Skipping pre-balance: owner {} != signer {}", balance.owner, signer);
                    }
                    None // Skip balances not owned by signer
                }
            } else {
                // OPTIMIZATION: Reduce debug logging in critical path
                #[cfg(feature = "verbose_logging")]
                {
                    println!("[TRITON-PARSER] DEBUG - Skipping if we can't determine signer");
                }
                None // Skip if we can't determine signer
            }
        }).collect();
        
        let post_balances: Vec<crate::triton_grpc::crossbeam_worker::SimpleTokenBalance> = meta.post_token_balances.iter().filter_map(|balance| {
            // Only include token balances where the owner is the transaction signer
            if let Some(signer) = signer_pubkey {
                if balance.owner == signer.to_string() {
                    // OPTIMIZATION: Reduce debug logging in critical path
                    #[cfg(feature = "verbose_logging")]
                    {
                        println!("[TRITON-PARSER] DEBUG - Found matching post-balance: mint={}, owner={}", balance.mint, balance.owner);
                    }
                    balance.ui_token_amount.as_ref().map(|ui_amount| {
                        crate::triton_grpc::crossbeam_worker::SimpleTokenBalance {
                            mint: balance.mint.clone(),
                            ui_amount: ui_amount.ui_amount,
                            decimals: ui_amount.decimals,
                            amount: ui_amount.amount.clone(),
                            owner: balance.owner.clone(),
                            program_id: balance.program_id.clone(),
                        }
                    })
                } else {
                    // OPTIMIZATION: Reduce debug logging in critical path
                    #[cfg(feature = "verbose_logging")]
                    {
                        println!("[TRITON-PARSER] DEBUG - Skipping post-balance: owner {} != signer {}", balance.owner, signer);
                    }
                    None // Skip balances not owned by signer
                }
            } else {
                // OPTIMIZATION: Reduce debug logging in critical path
                #[cfg(feature = "verbose_logging")]
                {
                    println!("[TRITON-PARSER] DEBUG - Skipping if we can't determine signer");
                }
                None // Skip if we can't determine signer
            }
        }).collect();
        
        // OPTIMIZATION: Reduce debug logging in critical path
        #[cfg(feature = "verbose_logging")]
        {
            println!("[TRITON-PARSER] DEBUG - After filtering: {} pre and {} post balances for signer", pre_balances.len(), post_balances.len());
        }
        
        (Some(pre_balances), Some(post_balances))
    } else {
        // OPTIMIZATION: Reduce debug logging in critical path
        #[cfg(feature = "verbose_logging")]
        {
            println!("[TRITON-PARSER] DEBUG - No meta found in transaction info");
        }
        (None, None)
    }
}

// Helper function to generate instructions from inner instructions
fn generate_instructions_from_inner(
    tx_info: &crate::geyser::SubscribeUpdateTransactionInfo,
) -> Option<Vec<solana_sdk::instruction::Instruction>> {
    if let Some(meta) = &tx_info.meta {
        if meta.inner_instructions.is_empty() {
            return None;
        }
        
        // Get the account keys from the transaction message
        let base_account_keys = if let Some(tx) = &tx_info.transaction {
            if let Some(msg) = &tx.message {
                msg.account_keys.clone()
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        
        // Get additional loaded addresses from meta
        let loaded_writable_addresses = meta.loaded_writable_addresses.clone();
        let loaded_readonly_addresses = meta.loaded_readonly_addresses.clone();
        
        // Combine all account keys: base + writable + readonly
        let mut all_account_keys = base_account_keys.clone();
        all_account_keys.extend(loaded_writable_addresses.clone());
        all_account_keys.extend(loaded_readonly_addresses.clone());
        
        // OPTIMIZATION: Reduce debug logging in critical path
        #[cfg(feature = "verbose_logging")]
        {
            println!("[TRITON-PARSER] DEBUG - Base account keys: {}", base_account_keys.len());
            println!("[TRITON-PARSER] DEBUG - Loaded writable addresses: {}", loaded_writable_addresses.len());
            println!("[TRITON-PARSER] DEBUG - Loaded readonly addresses: {}", loaded_readonly_addresses.len());
            println!("[TRITON-PARSER] DEBUG - Total combined account keys: {}", all_account_keys.len());
            
            // DEBUG: Print all account keys with their indices
            println!("[TRITON-PARSER] DEBUG - All account keys (base + loaded):");
            for (idx, account_key_bytes) in all_account_keys.iter().enumerate() {
                let pubkey = solana_sdk::pubkey::Pubkey::try_from(account_key_bytes.as_slice()).unwrap_or_default();
                let source = if idx < base_account_keys.len() {
                    "base"
                } else if idx < base_account_keys.len() + loaded_writable_addresses.len() {
                    "writable"
                } else {
                    "readonly"
                };
                println!("[TRITON-PARSER] DEBUG -   Index {}: {} ({} bytes) [{}]", idx, pubkey, account_key_bytes.len(), source);
            }
        }
        
        Some(meta.inner_instructions.iter().flat_map(|inner_instr_group| {
            // OPTIMIZATION: Reduce debug logging in critical path
            #[cfg(feature = "verbose_logging")]
            {
                println!("[TRITON-PARSER] DEBUG - Processing inner instruction group with index: {}", inner_instr_group.index);
            }
            inner_instr_group.instructions.iter().map(|inner_instr| {
                // DEBUG: Print the program_id_index and account keys for debugging
                // OPTIMIZATION: Reduce debug logging in critical path
                #[cfg(feature = "verbose_logging")]
                {
                    println!("[TRITON-PARSER] DEBUG - Inner instruction: program_id_index={}, accounts_indices={:?}, data_length={}, stack_height={:?}", 
                        inner_instr.program_id_index, inner_instr.accounts, inner_instr.data.len(), inner_instr.stack_height);
                }
                
                // Resolve the actual program ID from the account keys array
                let program_id = if (inner_instr.program_id_index as usize) < all_account_keys.len() {
                    let program_id_bytes = &all_account_keys[inner_instr.program_id_index as usize];
                    let resolved_program_id = solana_sdk::pubkey::Pubkey::try_from(program_id_bytes.as_slice()).unwrap_or_default();
                    // OPTIMIZATION: Reduce debug logging in critical path
                    #[cfg(feature = "verbose_logging")]
                    {
                        println!("[TRITON-PARSER] DEBUG - Resolved program_id_index {} -> program_id: {}", 
                            inner_instr.program_id_index, resolved_program_id);
                    }
                    resolved_program_id
                } else {
                    // OPTIMIZATION: Reduce debug logging in critical path
                    #[cfg(feature = "verbose_logging")]
                    {
                        println!("[TRITON-PARSER] WARNING - program_id_index {} out of bounds for all_account_keys length {}", 
                            inner_instr.program_id_index, all_account_keys.len());
                    }
                    solana_sdk::pubkey::Pubkey::default()
                };
                
                let accounts: Vec<solana_sdk::instruction::AccountMeta> = inner_instr.accounts.iter()
                    .map(|&account_idx| {
                        // For inner instructions, we need to determine signer/writable status differently
                        // since they're executed by programs, not by transaction signers
                        let is_signer = false; // Inner instructions are never signers
                        let is_writable = true; // Assume writable for inner instructions
                        
                        let pubkey = if (account_idx as usize) < all_account_keys.len() {
                            solana_sdk::pubkey::Pubkey::try_from(all_account_keys[account_idx as usize].as_slice()).unwrap_or_default()
                        } else {
                            solana_sdk::pubkey::Pubkey::default()
                        };
                        
                        solana_sdk::instruction::AccountMeta {
                            pubkey,
                            is_signer,
                            is_writable,
                        }
                    }).collect();
                
                solana_sdk::instruction::Instruction {
                    program_id,
                    accounts,
                    data: inner_instr.data.clone(),
                }
            })
        }).collect())
    } else {
        None
    }
}

// Helper function to generate instructions from outer (main) instructions
fn generate_instructions_from_outer(
    tx_info: &crate::geyser::SubscribeUpdateTransactionInfo,
) -> Option<Vec<solana_sdk::instruction::Instruction>> {
    if let Some(tx) = &tx_info.transaction {
        if let Some(msg) = &tx.message {
            if msg.instructions.is_empty() {
                return None;
            }
            
            // Get the account keys from the transaction message
            let base_account_keys = msg.account_keys.clone();
            
            // Get additional loaded addresses from meta if available
            let (loaded_writable_addresses, loaded_readonly_addresses) = if let Some(meta) = &tx_info.meta {
                (meta.loaded_writable_addresses.clone(), meta.loaded_readonly_addresses.clone())
            } else {
                (vec![], vec![])
            };
            
            // Combine all account keys: base + writable + readonly
            let mut all_account_keys = base_account_keys.clone();
            all_account_keys.extend(loaded_writable_addresses.clone());
            all_account_keys.extend(loaded_readonly_addresses.clone());
            
            // OPTIMIZATION: Reduce debug logging in critical path
            #[cfg(feature = "verbose_logging")]
            {
                println!("[TRITON-PARSER] DEBUG - Outer instructions: Base account keys: {}", base_account_keys.len());
                println!("[TRITON-PARSER] DEBUG - Outer instructions: Loaded writable addresses: {}", loaded_writable_addresses.len());
                println!("[TRITON-PARSER] DEBUG - Outer instructions: Loaded readonly addresses: {}", loaded_readonly_addresses.len());
                println!("[TRITON-PARSER] DEBUG - Outer instructions: Total combined account keys: {}", all_account_keys.len());
            }
            
            Some(msg.instructions.iter().map(|outer_instr| {
                // OPTIMIZATION: Reduce debug logging in critical path
                #[cfg(feature = "verbose_logging")]
                {
                    println!("[TRITON-PARSER] DEBUG - Outer instruction: program_id_index={}, accounts_indices={:?}, data_length={}", 
                        outer_instr.program_id_index, outer_instr.accounts, outer_instr.data.len());
                }
                
                // Resolve the actual program ID from the account keys array
                let program_id = if (outer_instr.program_id_index as usize) < all_account_keys.len() {
                    let program_id_bytes = &all_account_keys[outer_instr.program_id_index as usize];
                    let resolved_program_id = solana_sdk::pubkey::Pubkey::try_from(program_id_bytes.as_slice()).unwrap_or_default();
                    // OPTIMIZATION: Reduce debug logging in critical path
                    #[cfg(feature = "verbose_logging")]
                    {
                        println!("[TRITON-PARSER] DEBUG - Resolved outer program_id_index {} -> program_id: {}", 
                            outer_instr.program_id_index, resolved_program_id);
                    }
                    resolved_program_id
                } else {
                    // OPTIMIZATION: Reduce debug logging in critical path
                    #[cfg(feature = "verbose_logging")]
                    {
                        println!("[TRITON-PARSER] WARNING - Outer program_id_index {} out of bounds for all_account_keys length {}", 
                            outer_instr.program_id_index, all_account_keys.len());
                    }
                    solana_sdk::pubkey::Pubkey::default()
                };
                
                let accounts: Vec<solana_sdk::instruction::AccountMeta> = outer_instr.accounts.iter()
                    .enumerate()
                    .map(|(idx, &account_idx)| {
                        // For outer instructions, we need to determine signer/writable status from the message header
                        let header = msg.header.as_ref().unwrap_or(&crate::solana::storage::confirmed_block::MessageHeader {
                            num_required_signatures: 0,
                            num_readonly_signed_accounts: 0,
                            num_readonly_unsigned_accounts: 0,
                        });
                        
                        // Determine if account is a signer based on header
                        let is_signer = (account_idx as usize) < header.num_required_signatures as usize;
                        
                        // Determine if account is writable based on header and account index
                        let is_writable = if is_signer {
                            // Signed accounts are writable unless they're in the readonly signed section
                            (account_idx as usize) >= header.num_readonly_signed_accounts as usize
                        } else {
                            // Unsigned accounts are writable unless they're in the readonly unsigned section
                            let unsigned_account_idx = (account_idx as usize) - header.num_required_signatures as usize;
                            unsigned_account_idx < (base_account_keys.len() - header.num_required_signatures as usize - header.num_readonly_unsigned_accounts as usize)
                        };
                        
                        let pubkey = if (account_idx as usize) < all_account_keys.len() {
                            solana_sdk::pubkey::Pubkey::try_from(all_account_keys[account_idx as usize].as_slice()).unwrap_or_default()
                        } else {
                            solana_sdk::pubkey::Pubkey::default()
                        };
                        
                        solana_sdk::instruction::AccountMeta {
                            pubkey,
                            is_signer,
                            is_writable,
                        }
                    }).collect();
                
                solana_sdk::instruction::Instruction {
                    program_id,
                    accounts,
                    data: outer_instr.data.clone(),
                }
            }).collect())
        } else {
            None
        }
    } else {
        None
    }
}

// Helper function to concatenate inner and outer instructions
// Inner instructions come first (most common case), then outer instructions
fn generate_concat_instructions(
    inner_instructions: Option<Vec<solana_sdk::instruction::Instruction>>,
    outer_instructions: Option<Vec<solana_sdk::instruction::Instruction>>,
) -> Option<Vec<solana_sdk::instruction::Instruction>> {
    let inner = inner_instructions.unwrap_or_default();
    let outer = outer_instructions.unwrap_or_default();
    
    if inner.is_empty() && outer.is_empty() {
        return None;
    }
    
    // Concatenate: inner first, then outer
    let mut all_instructions = Vec::with_capacity(inner.len() + outer.len());
    all_instructions.extend(inner);
    all_instructions.extend(outer);
    
    // OPTIMIZATION: Reduce debug logging in critical path
    #[cfg(feature = "verbose_logging")]
    {
        println!("[TRITON-PARSER] DEBUG - Concatenated instructions: {} inner + {} outer = {} total", 
            inner.len(), outer.len(), all_instructions.len());
    }
    
    Some(all_instructions)
}

// Helper function to calculate final SOL amount based on balance changes and instruction data
fn calculate_final_sol_amount(
    sol_balance_change: Option<i64>,
    sol_buy_amount: Option<u64>,
) -> Option<u64> {
    if let Some(sol_change) = sol_balance_change {
        if sol_change < 0 {
            // If SOL decreased (spent), use the absolute value
            let abs_change = sol_change.abs() as u64;
            // OPTIMIZATION: Reduce debug logging in critical path
            #[cfg(feature = "verbose_logging")]
            {
                println!("[TRITON-PARSER] DEBUG - SOL decreased (spent): using absolute value {} lamports", abs_change);
            }
            Some(abs_change)
        } else {
            // If SOL increased (received), use instruction data or 0
            let fallback = sol_buy_amount.or(Some(0));
            // OPTIMIZATION: Reduce debug logging in critical path
            #[cfg(feature = "verbose_logging")]
            {
                println!("[TRITON-PARSER] DEBUG - SOL increased (received): using fallback value {} lamports", fallback.unwrap_or(0));
            }
            fallback
        }
    } else {
        // OPTIMIZATION: Reduce debug logging in critical path
        #[cfg(feature = "verbose_logging")]
        {
            println!("[TRITON-PARSER] DEBUG - No SOL balance change available: using instruction data {} lamports", sol_buy_amount.unwrap_or(0));
        }
        sol_buy_amount
    }
}

// OPTIMIZATION: Enhanced parser for multiple feeds
pub fn process_triton_message(resp: &SubscribeUpdate, feed_id: &str) {
    let start_time = std::time::Instant::now();
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");
    
    // Extract the GRPC creation timestamp for accurate performance tracking
    let grpc_creation_time = if let Some(created_at) = &resp.created_at {
        // Convert protobuf timestamp (Unix epoch time) to std::time::Instant
        let timestamp_seconds = created_at.seconds as u64;
        let timestamp_nanos = created_at.nanos as u32;
        
        // Get current time as SystemTime and convert to Unix timestamp
        let now_system = std::time::SystemTime::now();
        let now_unix = now_system.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0));
        
        // Calculate how long ago the GRPC message was created
        let grpc_unix_duration = std::time::Duration::new(timestamp_seconds, timestamp_nanos);
        
        if grpc_unix_duration <= now_unix {
            let time_diff = now_unix - grpc_unix_duration;
            // Subtract the time difference from current Instant to get the creation Instant
            std::time::Instant::now().checked_sub(time_diff).unwrap_or(std::time::Instant::now())
        } else {
            // If GRPC timestamp is in the future (clock skew), use current time
            std::time::Instant::now()
        }
    } else {
        // Fallback if no timestamp available
        std::time::Instant::now()
    };
    
    // PERFORMANCE TRACKING: Record when we first hit the parser AFTER expensive GRPC processing
    // This gives us a more accurate measure of actual parsing time
    let parser_first_hit = std::time::Instant::now();
    
    // OPTIMIZATION: Reduce logging in critical path - only log if verbose logging is enabled
    #[cfg(feature = "verbose_logging")]
    {
        println!("[TRITON-PARSER] GRPC message created_at: {:?}", resp.created_at);
        if let Some(created_at) = &resp.created_at {
            println!("[TRITON-PARSER] GRPC Unix timestamp: {}.{:09} seconds", created_at.seconds, created_at.nanos);
            let age = std::time::Instant::now().duration_since(grpc_creation_time);
            println!("[TRITON-PARSER] Message age: {:.3?}", age);
            
            // DEBUG: Show more detailed timing info
            let now_system = std::time::SystemTime::now();
            let now_unix = now_system.duration_since(std::time::UNIX_EPOCH).unwrap();
            println!("[TRITON-PARSER] Current Unix timestamp: {}.{:09} seconds", now_unix.as_secs(), now_unix.subsec_nanos());
            
            let grpc_unix = std::time::Duration::new(created_at.seconds as u64, created_at.nanos as u32);
            let time_diff_micros = if now_unix > grpc_unix {
                (now_unix - grpc_unix).as_micros()
            } else {
                0
            };
            println!("[TRITON-PARSER] Time difference: {}µs ({:.3}ms)", time_diff_micros, time_diff_micros as f64 / 1000.0);
        }
    }
    
    // OPTIMIZATION: Reduce logging in critical path
    #[cfg(feature = "verbose_logging")]
    {
        println!("[{}] - [TRITON-PARSER] Processing message from feed: {}", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), feed_id);
        
        // OPTIMIZATION: Log when message is received from GRPC stream
        println!("[{}] - [TRITON] GRPC message received from {} (processing started)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), feed_id);
    }
    
    let update_check_start = std::time::Instant::now();
    let update_check = resp.update_oneof.is_some();
    let update_check_time = update_check_start.elapsed();
    
    if let Some(update) = &resp.update_oneof {
        #[cfg(feature = "verbose_logging")]
        println!("[TRITON-PARSER] Found update_oneof, processing...");
        match update {
            UpdateOneof::Transaction(tx_update) => {
                #[cfg(feature = "verbose_logging")]
                println!("[TRITON-PARSER] Found transaction update");
                let tx_update_check_start = std::time::Instant::now();
                let tx_update_check = tx_update.transaction.is_some();
                let tx_update_check_time = tx_update_check_start.elapsed();
                
                if let Some(tx_info) = &tx_update.transaction {
                    #[cfg(feature = "verbose_logging")]
                    println!("[TRITON-PARSER] Found transaction info");
                    let tx_info_check_start = std::time::Instant::now();
                    let tx_info_check = tx_info.transaction.is_some();
                    let tx_info_check_time = tx_info_check_start.elapsed();
                    
                    if let Some(tx) = &tx_info.transaction {
                        #[cfg(feature = "verbose_logging")]
                        println!("[TRITON-PARSER] Found transaction, processing signature");
                        let sig_bytes = tx.signatures.get(0).map(|s| s.clone());
                        
                        // OPTIMIZATION: Extract signature string for deduplication
                        let sig_decode_start = std::time::Instant::now();
                        let sig_string = sig_bytes.as_ref()
                            .map(|s| bs58::encode(s).into_string())
                            .unwrap_or_default();
                        let sig_decode_time = sig_decode_start.elapsed();
                        
                        // NEW: Extract signature string early for nonce refresh
                        let sig_string_for_refresh = sig_string.clone();
                        
                        // OPTIMIZATION: Check if this signature was already processed by any feed
                        let dedup_start = std::time::Instant::now();
                        if is_signature_processed_by_feed(&sig_string, feed_id) {
                            // Skip processing - already handled by another feed
                            let dedup_time = dedup_start.elapsed();
                            #[cfg(feature = "verbose_logging")]
                            println!("[{}] - [TRITON] SKIPPED duplicate transaction for sig: {} (feed: {}) - already processed by another feed (dedup check took: {:.2?})", 
                                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_string, feed_id, dedup_time);
                            return;
                        }
                        let dedup_time = dedup_start.elapsed();
                        
                        // OPTIMIZATION: Log first detection of this transaction
                        #[cfg(feature = "verbose_logging")]
                        println!("[{}] - [TRITON] FIRST DETECTION of transaction for sig: {} (feed: {}) (sig_decode: {:.2?}, dedup: {:.2?})", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_string, feed_id, sig_decode_time, dedup_time);
                        
                        let wallet_check_start = std::time::Instant::now();
                        let wallet_pubkey = get_wallet_keypair().pubkey();
                        let wallet_pubkey_bytes = wallet_pubkey.to_bytes();
                        
                        // Get monitored accounts from config
                        let config = crate::config_load::GLOBAL_CONFIG.get().expect("Config not loaded");
                        let monitored_accounts: Vec<Vec<u8>> = config.accounts_monitor
                            .iter()
                            .filter_map(|account_str| {
                                account_str.parse::<Pubkey>().ok().map(|pk| pk.to_bytes().to_vec())
                            })
                            .collect();
                        
                        let (is_wallet_signer, is_monitored_signer, is_monitored_program) = check_transaction_permissions(tx, &wallet_pubkey_bytes, &monitored_accounts);
                        
                        // // Access inner instructions through the correct path
                        // if let Some(tx_info) = &tx_update.transaction {
                        //     if let Some(meta) = &tx_info.meta {
                        //         println!("inner instructions: {:?}", meta.inner_instructions);
                        //     }
                        // }
                        // Extract SOL buy amount and mint token amount from instruction data
                        let (sol_buy_amount, mint_token_amount) = extract_amounts_from_instructions(tx_info);
                        
                        // ADDED: Debug the instruction data extraction
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON-PARSER] DEBUG - === INSTRUCTION DATA EXTRACTION DEBUG ===");
                            println!("[TRITON-PARSER] DEBUG - sol_buy_amount (from instructions): {:?}", sol_buy_amount);
                            println!("[TRITON-PARSER] DEBUG - mint_token_amount (from instructions): {:?}", mint_token_amount);
                        }
                        
                        // Get the transaction signer's pubkey and SOL balance changes
                        let (signer_pubkey, sol_balance_change) = get_signer_and_sol_changes(tx_info);
                        
                        // Extract pre and post token balances from transaction metadata
                        let (pre_token_balances, post_token_balances) = extract_token_balances(tx_info, &signer_pubkey);
                        
                        // DEBUG: Show some token balance details
                        #[cfg(feature = "verbose_logging")]
                        {
                            if let Some(ref pre) = pre_token_balances {
                                // ADDED: Detailed debugging for mint multiplier issue
                                println!("[TRITON-PARSER] DEBUG - === PRE-BALANCE DETAILS ===");
                                for balance in pre.iter().take(3) { // Only show first 3 to avoid excessive logging
                                    println!("[TRITON-PARSER] DEBUG - Pre-balance: mint={}, ui_amount={}, raw_amount={}, decimals={}",
                                        balance.mint, balance.ui_amount, balance.amount, balance.decimals);
                                }
                                if pre.len() > 3 {
                                    println!("[TRITON-PARSER] DEBUG - ... and {} more pre-balances", pre.len() - 3);
                                }
                            }
                            
                            if let Some(ref post) = post_token_balances {
                                // ADDED: Detailed debugging for mint multiplier issue
                                println!("[TRITON-PARSER] DEBUG - === POST-BALANCE DETAILS ===");
                                for balance in post.iter().take(3) { // Only show first 3 to avoid excessive logging
                                    println!("[TRITON-PARSER] DEBUG - Post-balance: mint={}, ui_amount={}, raw_amount={}, decimals={}",
                                        balance.mint, balance.ui_amount, balance.amount, balance.decimals);
                                }
                                if post.len() > 3 {
                                    println!("[TRITON-PARSER] DEBUG - ... and {} more post-balances", post.len() - 3);
                                }
                            }
                        }
                        
                        // NEW: Calculate actual token amount change from balance differences
                        let actual_mint_token_amount = if let (Some(pre), Some(post)) = (&pre_token_balances, &post_token_balances) {
                            // ADDED: Debug the calculation step by step
                            #[cfg(feature = "verbose_logging")]
                            {
                                println!("[TRITON-PARSER] DEBUG - === TOKEN AMOUNT CALCULATION DEBUG ===");
                                println!("[TRITON-PARSER] DEBUG - Pre-balances count: {}, Post-balances count: {}", pre.len(), post.len());
                            }
                            
                            // Check if we actually have data in the vectors
                            if pre.is_empty() && !post.is_empty() {
                                // We bought tokens (no pre-balance, but post-balance exists)
                                // FIX: Use raw amount instead of ui_amount to preserve precision
                                let post_amount = post.iter().map(|b| b.amount.parse::<u64>().unwrap_or(0)).sum::<u64>();
                                // ADDED: Debug the calculation
                                #[cfg(feature = "verbose_logging")]
                                {
                                    println!("[TRITON-PARSER] DEBUG - No pre-balances but {} post-balances: bought {} raw tokens", 
                                        post.len(), post_amount);
                                    println!("[TRITON-PARSER] DEBUG - Final result: {} raw tokens", post_amount);
                                }
                                Some(post_amount)
                            } else if !pre.is_empty() && !post.is_empty() {
                                // We have both pre and post balances - calculate the difference
                                // FIX: Use raw amount instead of ui_amount to preserve precision
                                let pre_amount = pre.iter().map(|b| b.amount.parse::<u64>().unwrap_or(0)).sum::<u64>();
                                let post_amount = post.iter().map(|b| b.amount.parse::<u64>().unwrap_or(0)).sum::<u64>();
                                let change = if post_amount > pre_amount { post_amount - pre_amount } else { 0 };
                                
                                // ADDED: Debug the calculation step by step
                                #[cfg(feature = "verbose_logging")]
                                {
                                    println!("[TRITON-PARSER] DEBUG - Pre: {} raw tokens, Post: {} raw tokens, Change: {} raw tokens", 
                                        pre_amount, post_amount, change);
                                    println!("[TRITON-PARSER] DEBUG - Change as u64: {}", change);
                                }
                                
                                if change > 0 {
                                    Some(change)
                                } else {
                                    // OPTIMIZATION: Reduce debug logging in critical path
                                    #[cfg(feature = "verbose_logging")]
                                    {
                                        println!("[TRITON-PARSER] DEBUG - No positive token change detected");
                                    }
                                    None
                                }
                            } else {
                                // OPTIMIZATION: Reduce debug logging in critical path
                                #[cfg(feature = "verbose_logging")]
                                {
                                    println!("[TRITON-PARSER] DEBUG - No token balance data available");
                                }
                                None
                            }
                        } else {
                            // OPTIMIZATION: Reduce debug logging in critical path
                            #[cfg(feature = "verbose_logging")]
                            {
                                println!("[TRITON-PARSER] DEBUG - Missing token balance data");
                            }
                            None
                        };
                        
                        // Use actual token amount change if available, otherwise fall back to instruction data
                        let final_mint_token_amount = actual_mint_token_amount.or(mint_token_amount);
                        
                        // ADDED: Debug the final mint_token_amount calculation
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON-PARSER] DEBUG - === FINAL MINT TOKEN AMOUNT DEBUG ===");
                            println!("[TRITON-PARSER] DEBUG - actual_mint_token_amount: {:?}", actual_mint_token_amount);
                            println!("[TRITON-PARSER] DEBUG - mint_token_amount (from instructions): {:?}", mint_token_amount);
                            println!("[TRITON-PARSER] DEBUG - final_mint_token_amount: {:?}", final_mint_token_amount);
                        }
                        
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON-PARSER] DEBUG - Final mint_token_amount for ParsedTx: {:?}", final_mint_token_amount);
                        }
                        
                        // Use actual SOL balance change if available, otherwise fall back to instruction data
                        let final_sol_amount = calculate_final_sol_amount(sol_balance_change, sol_buy_amount);
                        
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON-PARSER] DEBUG - Final SOL amount calculation:");
                            println!("[TRITON-PARSER] DEBUG -   Balance change: {:?} lamports", sol_balance_change);
                            println!("[TRITON-PARSER] DEBUG -   Instruction data: {:?} lamports", sol_buy_amount);
                            println!("[TRITON-PARSER] DEBUG -   Final result: {} lamports", final_sol_amount.unwrap_or(0));
                        }
                        
                        // PERFORMANCE TRACKING: Calculate parse time
                        let parse_time = parser_first_hit.elapsed();
                        
                        // Create ParsedTx with all the information we need
                        let parsed = ParsedTx {
                            sig_bytes,
                            is_signer: is_wallet_signer, //|| is_monitored_signer || is_monitored_program,
                            detection_time: Some(grpc_creation_time), // Use the actual GRPC creation time
                            feed_id: feed_id.to_string(),
                            slot: Some(tx_update.slot),
                            pre_token_balances: pre_token_balances,
                            post_token_balances: post_token_balances,
                            instructions: generate_concat_instructions(
                                generate_instructions_from_inner(tx_info),
                                generate_instructions_from_outer(tx_info)
                            ),
                            account_keys: tx.message.as_ref().map(|msg| {
                                msg.account_keys.iter()
                                    .map(|bytes| solana_sdk::pubkey::Pubkey::try_from(bytes.as_slice()).unwrap_or_default())
                                    .collect()
                            }),
                            recent_blockhash: tx.message.as_ref().and_then(|msg| {
                                if msg.recent_blockhash.len() == 32 {
                                    let mut hash_bytes = [0u8; 32];
                                    hash_bytes.copy_from_slice(&msg.recent_blockhash);
                                    Some(solana_sdk::hash::Hash::from(hash_bytes))
                                } else {
                                    None
                                }
                            }),
                            fee_payer: tx.message.as_ref().and_then(|msg| {
                                if !msg.account_keys.is_empty() {
                                    solana_sdk::pubkey::Pubkey::try_from(msg.account_keys[0].as_slice()).ok()
                                } else {
                                    None
                                }
                            }),
                            token_amount_change: None, // Will be calculated in the worker
                            sol_buy_amount_lamports: final_sol_amount,
                            mint_token_amount: final_mint_token_amount,  // Use the final calculated amount
                            // PERFORMANCE TRACKING: Store timing information
                            parser_first_hit: Some(parser_first_hit),
                            parse_time: Some(parse_time),
                        };
                        
                        // OPTIMIZATION: Reduce debug logging in critical path
                        #[cfg(feature = "verbose_logging")]
                        {
                            println!("[TRITON-PARSER] DEBUG - Created ParsedTx with mint_token_amount: {:?}", parsed.mint_token_amount);
                        }
                        
                        // Log transaction detection with type
                        let tx_type = if is_wallet_signer { "WALLET" } else if is_monitored_signer { "SIGNER" } else if is_monitored_program { "PROGRAM" } else { "UNKNOWN" };
                        #[cfg(feature = "verbose_logging")]
                        println!("[{}] - [TRITON] Transaction detected: sig={}, type={}, feed={}",
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), sig_string, tx_type, feed_id);
                        
                        // Send to crossbeam worker for processing
                        let send_start = std::time::Instant::now();
                        
                        // Send to crossbeam worker for processing (including sell transaction building)
                        // ALL transactions that need sell processing should go to crossbeam worker
                        if is_wallet_signer || is_monitored_signer || is_monitored_program {
                            // OPTIMIZATION: Reduce debug logging in critical path
                            #[cfg(feature = "verbose_logging")]
                            {
                                println!("[TRITON-PARSER] Sending transaction to crossbeam worker for processing");
                            }
                            crate::triton_grpc::crossbeam_worker::send_parsed_tx(parsed.clone());
                            
                            // NEW: Trigger nonce refresh if this is our own transaction
                            if is_wallet_signer {
                                // Use channel-based approach instead of spawning async task
                                crate::triton_grpc::crossbeam_worker::trigger_nonce_refresh_for_transaction(&sig_string_for_refresh);
                            }
                        }
                        
                        let send_time = send_start.elapsed();
                        
                        // Log success message
                        #[cfg(feature = "verbose_logging")]
                        println!("[TRITON-PARSER] Successfully processed transaction, processing time: {:.2?}", send_time);
                        
                        // OPTIMIZATION: Log detailed profiling
                        let total_time = update_check_start.elapsed();
                        #[cfg(feature = "verbose_logging")]
                        println!("[{}] - [TRITON] PROFILE - update_check: {:.2?}, tx_update_check: {:.2?}, tx_info_check: {:.2?}, sig_decode: {:.2?}, dedup: {:.2?}, wallet_check: {:.2?}, send: {:.2?}, total: {:.2?}", 
                            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), 
                            update_check_time, tx_update_check_time, tx_info_check_time, sig_decode_time, dedup_time, wallet_check_start.elapsed(), send_time, total_time);
                    }
                }
            }
            _ => {
                // Handle other update types if needed
            }
        }
    }
}

// OPTIMIZATION: Backward compatibility function
pub fn process_triton_message_legacy(resp: &SubscribeUpdate) {
    process_triton_message(resp, "triton_legacy")
}

pub fn get_blockhash() -> Hash {
    let rpc = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    let blockhash = match rpc.get_latest_blockhash() {
        Ok(hash) => hash,
        Err(e) => {
            // handle error
            return Hash::default();
        }
    };
    blockhash
}
