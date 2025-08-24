use crate::grpc::arpc_worker::{TxWithPubkey, GLOBAL_TX_MAP, get_multiple_entries_stats};
use solana_sdk::transaction::Transaction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use std::str::FromStr;
use std::time::Instant;

/// Test the multiple signature storage functionality
pub fn test_multiple_signature_storage() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== TESTING MULTIPLE SIGNATURE STORAGE ===");
    
    // Clear the map first
    GLOBAL_TX_MAP.clear();
    
    // Create a test TxWithPubkey with vendor transactions
    let mut tx_with_pubkey = TxWithPubkey::default();
    
    // Create test transactions with different signatures
    let keypair1 = Keypair::new();
    let keypair2 = Keypair::new();
    let keypair3 = Keypair::new();
    
    let mut tx1 = Transaction::default();
    tx1.signatures = vec![keypair1.sign_message(&[1, 2, 3])];
    
    let mut tx2 = Transaction::default();
    tx2.signatures = vec![keypair2.sign_message(&[4, 5, 6])];
    
    let mut tx3 = Transaction::default();
    tx3.signatures = vec![keypair3.sign_message(&[7, 8, 9])];
    
    // Simulate vendor transactions with different signatures
    let vendor_transactions = vec![
        ("rpc".to_string(), tx1),
        ("zeroslot".to_string(), tx2),
        ("nextblock".to_string(), tx3),
    ];
    
    tx_with_pubkey.vendor_transactions = vendor_transactions.clone();
    tx_with_pubkey.mint = Pubkey::from_str("11111111111111111111111111111112")?;
    tx_with_pubkey.token_amount = 1000;
    tx_with_pubkey.tx_type = "test_multiple_sigs".to_string();
    tx_with_pubkey.created_at = Instant::now();
    
    // Simulate the new storage logic
    println!("📦 Storing transaction with multiple vendor signatures...");
    
    // Store with original signature (simulated)
    let original_sig = "original_signature_123";
    let original_key = original_sig.as_bytes().to_vec();
    GLOBAL_TX_MAP.insert(original_key, tx_with_pubkey.clone());
    
    // Store with each vendor signature
    for (vendor_name, transaction) in &vendor_transactions {
        if let Some(signature) = transaction.signatures.first() {
            let vendor_sig_bytes = signature.as_ref().to_vec();
            
            // Create a copy for this vendor signature
            let mut vendor_tx_with_pubkey = tx_with_pubkey.clone();
            vendor_tx_with_pubkey.send_sig = signature.to_string();
            
            GLOBAL_TX_MAP.insert(vendor_sig_bytes, vendor_tx_with_pubkey);
            
            println!("   ✅ Stored {} vendor signature: {}", vendor_name, signature);
        }
    }
    
    // Verify storage
    println!("\n🔍 Verifying storage...");
    println!("   Total entries in map: {}", GLOBAL_TX_MAP.len());
    
    // Should have 4 entries: 1 original + 3 vendor signatures
    let expected_entries = 1 + vendor_transactions.len();
    if GLOBAL_TX_MAP.len() == expected_entries {
        println!("   ✅ Correct number of entries stored: {}", expected_entries);
    } else {
        println!("   ❌ Expected {} entries, got {}", expected_entries, GLOBAL_TX_MAP.len());
    }
    
    // Test retrieval by different signatures
    println!("\n🔍 Testing retrieval by different signatures...");
    
    // Test original signature
    let original_key = original_sig.as_bytes().to_vec();
    if let Some(entry) = GLOBAL_TX_MAP.get(&original_key) {
        println!("   ✅ Found entry with original signature");
    } else {
        println!("   ❌ Could not find entry with original signature");
    }
    
    // Test vendor signatures
    for (vendor_name, transaction) in &vendor_transactions {
        if let Some(signature) = transaction.signatures.first() {
            let vendor_sig_bytes = signature.as_ref().to_vec();
            if let Some(entry) = GLOBAL_TX_MAP.get(&vendor_sig_bytes) {
                println!("   ✅ Found entry with {} signature: {}", vendor_name, signature);
            } else {
                println!("   ❌ Could not find entry with {} signature", vendor_name);
            }
        }
    }
    
    // Test multiple entries statistics
    println!("\n📊 Multiple entries statistics:");
    let (unique_signatures, signature_counts) = get_multiple_entries_stats();
    println!("   Unique signatures: {}", unique_signatures);
    println!("   Signature distribution: {:?}", signature_counts);
    
    // Clean up
    GLOBAL_TX_MAP.clear();
    println!("\n🧹 Cleaned up test data");
    
    println!("=== MULTIPLE SIGNATURE STORAGE TEST COMPLETED ===");
    Ok(())
}

/// Demonstrate the real-world scenario
pub fn demonstrate_real_world_scenario() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== DEMONSTRATING REAL-WORLD SCENARIO ===");
    
    // Clear the map first
    GLOBAL_TX_MAP.clear();
    
    println!("🎯 Scenario: GRPC detects a transaction, builds multiple vendor versions");
    println!("   Each vendor transaction has a different signature");
    println!("   We need to store all signatures so any can be detected later");
    
    // Simulate the detection and building process
    let mut tx_with_pubkey = TxWithPubkey::default();
    tx_with_pubkey.tx_type = "ray_launch".to_string();
    tx_with_pubkey.mint = Pubkey::from_str("11111111111111111111111111111112")?;
    tx_with_pubkey.token_amount = 1000;
    tx_with_pubkey.created_at = Instant::now();
    
    // Simulate vendor transactions with different signatures
    let keypair1 = Keypair::new();
    let keypair2 = Keypair::new();
    let keypair3 = Keypair::new();
    let keypair4 = Keypair::new();
    
    let mut tx1 = Transaction::default();
    tx1.signatures = vec![keypair1.sign_message(&[1, 2, 3])];
    
    let mut tx2 = Transaction::default();
    tx2.signatures = vec![keypair2.sign_message(&[4, 5, 6])];
    
    let mut tx3 = Transaction::default();
    tx3.signatures = vec![keypair3.sign_message(&[7, 8, 9])];
    
    let mut tx4 = Transaction::default();
    tx4.signatures = vec![keypair4.sign_message(&[10, 11, 12])];
    
    let vendor_transactions = vec![
        ("rpc".to_string(), tx1),
        ("zeroslot".to_string(), tx2),
        ("nextblock".to_string(), tx3),
        ("blockrazor".to_string(), tx4),
    ];
    
    tx_with_pubkey.vendor_transactions = vendor_transactions.clone();
    
    // Simulate the storage process
    println!("\n📦 Step 1: Storing with original detected signature");
    let original_sig = "detected_signature_456";
    let original_key = original_sig.as_bytes().to_vec();
    GLOBAL_TX_MAP.insert(original_key, tx_with_pubkey.clone());
    
    println!("📦 Step 2: Storing with each vendor signature");
    for (vendor_name, transaction) in &vendor_transactions {
        if let Some(signature) = transaction.signatures.first() {
            let vendor_sig_bytes = signature.as_ref().to_vec();
            
            let mut vendor_tx_with_pubkey = tx_with_pubkey.clone();
            vendor_tx_with_pubkey.send_sig = signature.to_string();
            
            GLOBAL_TX_MAP.insert(vendor_sig_bytes, vendor_tx_with_pubkey);
            println!("   ✅ Stored {} signature: {}", vendor_name, signature);
        }
    }
    
    println!("\n🔍 Step 3: Later, GRPC detects one of the vendor transactions");
    
    // Simulate GRPC detecting one of the vendor transactions
    let detected_sig = vendor_transactions[1].1.signatures.first().unwrap();
    let detected_key = detected_sig.as_ref().to_vec();
    
    if let Some(entry) = GLOBAL_TX_MAP.get(&detected_key) {
        println!("   ✅ SUCCESS: Found matching entry for detected signature: {}", detected_sig);
        println!("   📋 Transaction details:");
        println!("      - Type: {}", entry.tx_type);
        println!("      - Mint: {}", entry.mint);
        println!("      - Token amount: {}", entry.token_amount);
        println!("      - Vendor count: {}", entry.vendor_transactions.len());
        println!("      - Send signature: {}", entry.send_sig);
    } else {
        println!("   ❌ FAILED: Could not find matching entry for detected signature");
    }
    
    // Show final statistics
    println!("\n📊 Final Statistics:");
    println!("   Total entries in map: {}", GLOBAL_TX_MAP.len());
    let (unique_signatures, signature_counts) = get_multiple_entries_stats();
    println!("   Unique signatures: {}", unique_signatures);
    println!("   Signature distribution: {:?}", signature_counts);
    
    // Clean up
    GLOBAL_TX_MAP.clear();
    
    println!("\n✅ REAL-WORLD SCENARIO DEMONSTRATION COMPLETED");
    println!("=== REAL-WORLD SCENARIO COMPLETED ===");
    Ok(())
} 