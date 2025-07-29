use crate::grpc::arpc_worker::TxWithPubkey;
use solana_sdk::transaction::Transaction;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Test the new vendor transaction functionality
pub fn test_vendor_transactions() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== TESTING VENDOR TRANSACTIONS ===");
    
    // Create a test TxWithPubkey with vendor transactions
    let mut tx_with_pubkey = TxWithPubkey::default();
    
    // Simulate vendor transactions (in real usage, these would come from build_vendor_specific_transactions_parallel)
    let vendor_transactions = vec![
        ("rpc".to_string(), Transaction::default()),
        ("zeroslot".to_string(), Transaction::default()),
        ("jito".to_string(), Transaction::default()),
        ("nextblock".to_string(), Transaction::default()),
    ];
    
    tx_with_pubkey.vendor_transactions = vendor_transactions;
    tx_with_pubkey.mint = Pubkey::from_str("11111111111111111111111111111112")?;
    tx_with_pubkey.token_amount = 1000;
    tx_with_pubkey.tx_type = "test".to_string();
    
    // Test the new helper methods
    println!("✅ Vendor names: {:?}", tx_with_pubkey.get_vendor_names());
    
    // Test getting specific vendor transactions
    if let Some(rpc_tx) = tx_with_pubkey.get_vendor_transaction("rpc") {
        println!("✅ RPC transaction found");
    } else {
        println!("❌ RPC transaction not found");
    }
    
    if let Some(zeroslot_tx) = tx_with_pubkey.get_vendor_transaction("zeroslot") {
        println!("✅ ZeroSlot transaction found");
    } else {
        println!("❌ ZeroSlot transaction not found");
    }
    
    // Test vendor existence checks
    println!("✅ Has RPC: {}", tx_with_pubkey.has_vendor("rpc"));
    println!("✅ Has ZeroSlot: {}", tx_with_pubkey.has_vendor("zeroslot"));
    println!("✅ Has Jito: {}", tx_with_pubkey.has_vendor("jito"));
    println!("✅ Has NextBlock: {}", tx_with_pubkey.has_vendor("nextblock"));
    println!("✅ Has Unknown: {}", tx_with_pubkey.has_vendor("unknown"));
    
    // Test getting first transaction (backward compatibility)
    if let Some(first_tx) = tx_with_pubkey.get_first_transaction() {
        println!("✅ First transaction found");
    } else {
        println!("❌ No transactions found");
    }
    
    // Test iteration over all vendor transactions
    println!("📋 All vendor transactions:");
    for (vendor_name, _transaction) in &tx_with_pubkey.vendor_transactions {
        println!("   - {}: Transaction available", vendor_name);
    }
    
    println!("=== VENDOR TRANSACTIONS TEST COMPLETED ===");
    Ok(())
}

/// Demonstrate how to use vendor transactions for parallel sending
pub fn demonstrate_parallel_sending() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== DEMONSTRATING PARALLEL SENDING ===");
    
    let mut tx_with_pubkey = TxWithPubkey::default();
    
    // Simulate vendor transactions from parallel building
    let vendor_transactions = vec![
        ("rpc".to_string(), Transaction::default()),
        ("zeroslot".to_string(), Transaction::default()),
        ("jito".to_string(), Transaction::default()),
        ("nextblock".to_string(), Transaction::default()),
    ];
    
    tx_with_pubkey.vendor_transactions = vendor_transactions;
    
    // Simulate parallel sending to vendors
    println!("🚀 Sending transactions to vendors in parallel:");
    
    for (vendor_name, transaction) in &tx_with_pubkey.vendor_transactions {
        println!("   📤 Sending {} transaction to {} vendor", 
            transaction.signatures.first().unwrap_or(&solana_sdk::signature::Signature::default()),
            vendor_name
        );
        
        // In real implementation, you would:
        // match vendor_name.as_str() {
        //     "rpc" => send_to_rpc(transaction),
        //     "zeroslot" => send_to_zeroslot(transaction),
        //     "jito" => send_to_jito(transaction),
        //     "nextblock" => send_to_nextblock(transaction),
        //     _ => println!("Unknown vendor: {}", vendor_name),
        // }
    }
    
    println!("✅ All vendor transactions sent in parallel!");
    println!("=== PARALLEL SENDING DEMONSTRATION COMPLETED ===");
    Ok(())
} 