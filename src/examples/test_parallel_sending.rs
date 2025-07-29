use crate::send_tx::generic_sender::send_all_vendors_parallel;
use crate::grpc::arpc_worker::TxWithPubkey;
use solana_sdk::transaction::Transaction;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::time::Instant;

/// Test the parallel sending functionality
pub async fn test_parallel_sending() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== TESTING PARALLEL SENDING ===");
    
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
    
    // Test parallel sending
    let detection_time = Instant::now();
    
    println!("🚀 Starting parallel send to {} vendors", tx_with_pubkey.vendor_transactions.len());
    
    let result = send_all_vendors_parallel(&tx_with_pubkey.vendor_transactions, detection_time).await;
    
    match result {
        Ok((winning_vendor, signature)) => {
            println!("✅ PARALLEL SUCCESS - {} won with signature: {}", winning_vendor, signature);
        }
        Err(e) => {
            println!("❌ PARALLEL SEND FAILED: {}", e);
            // This is expected in a test environment since we're using dummy transactions
        }
    }
    
    println!("=== PARALLEL SENDING TEST COMPLETED ===");
    Ok(())
}

/// Demonstrate the complete flow from building to sending
pub async fn demonstrate_complete_flow() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== DEMONSTRATING COMPLETE FLOW ===");
    
    // Step 1: Build vendor-specific transactions (simulated)
    println!("📦 Step 1: Building vendor-specific transactions...");
    let vendor_transactions = vec![
        ("rpc".to_string(), Transaction::default()),
        ("zeroslot".to_string(), Transaction::default()),
        ("jito".to_string(), Transaction::default()),
        ("nextblock".to_string(), Transaction::default()),
    ];
    
    // Step 2: Store in TxWithPubkey
    println!("💾 Step 2: Storing in TxWithPubkey...");
    let mut tx_with_pubkey = TxWithPubkey::default();
    tx_with_pubkey.vendor_transactions = vendor_transactions.clone();
    tx_with_pubkey.mint = Pubkey::from_str("11111111111111111111111111111112")?;
    tx_with_pubkey.token_amount = 1000;
    tx_with_pubkey.tx_type = "complete_flow_test".to_string();
    
    // Step 3: Send in parallel
    println!("🚀 Step 3: Sending to all vendors in parallel...");
    let detection_time = Instant::now();
    
    let result = send_all_vendors_parallel(&tx_with_pubkey.vendor_transactions, detection_time).await;
    
    match result {
        Ok((winning_vendor, signature)) => {
            println!("✅ Step 4: {} won the race with signature: {}", winning_vendor, signature);
            
            // Step 5: Update transaction info
            println!("📝 Step 5: Updating transaction info...");
            tx_with_pubkey.send_sig = signature;
            tx_with_pubkey.send_slot = 12345; // Example slot
            tx_with_pubkey.send_time = Instant::now();
            
            println!("✅ COMPLETE FLOW SUCCESSFUL!");
        }
        Err(e) => {
            println!("❌ Step 4: All vendors failed: {}", e);
            // This is expected in a test environment
        }
    }
    
    println!("=== COMPLETE FLOW DEMONSTRATION FINISHED ===");
    Ok(())
}

/// Benchmark parallel vs sequential sending (simulated)
pub async fn benchmark_sending_performance() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== BENCHMARKING SENDING PERFORMANCE ===");
    
    let vendor_transactions = vec![
        ("rpc".to_string(), Transaction::default()),
        ("zeroslot".to_string(), Transaction::default()),
        ("jito".to_string(), Transaction::default()),
        ("nextblock".to_string(), Transaction::default()),
    ];
    
    let detection_time = Instant::now();
    
    // Benchmark parallel sending
    let parallel_start = Instant::now();
    let parallel_result = send_all_vendors_parallel(&vendor_transactions, detection_time).await;
    let parallel_time = parallel_start.elapsed();
    
    match parallel_result {
        Ok((winning_vendor, _)) => {
            println!("✅ PARALLEL: {} won in {:.2?}", winning_vendor, parallel_time);
        }
        Err(e) => {
            println!("❌ PARALLEL failed: {} (took {:.2?})", e, parallel_time);
        }
    }
    
    // TODO: Add sequential benchmark for comparison
    // This would send to each vendor one by one
    
    println!("=== BENCHMARK COMPLETED ===");
    Ok(())
} 