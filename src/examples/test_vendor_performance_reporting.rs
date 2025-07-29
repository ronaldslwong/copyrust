use crate::send_tx::generic_sender::send_all_vendors_parallel;
use crate::grpc::arpc_worker::TxWithPubkey;
use solana_sdk::transaction::Transaction;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::time::Instant;

/// Test the comprehensive vendor performance reporting
pub async fn test_vendor_performance_reporting() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== TESTING VENDOR PERFORMANCE REPORTING ===");
    
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
    tx_with_pubkey.tx_type = "performance_test".to_string();
    
    // Test parallel sending with performance reporting
    let detection_time = Instant::now();
    
    println!("🚀 Starting parallel send with performance reporting to {} vendors", 
        tx_with_pubkey.vendor_transactions.len());
    
    let result = send_all_vendors_parallel(&tx_with_pubkey.vendor_transactions, detection_time).await;
    
    match result {
        Ok((winning_vendor, signature)) => {
            println!("✅ WINNER: {} with signature: {}", winning_vendor, signature);
        }
        Err(e) => {
            println!("❌ ALL VENDORS FAILED: {}", e);
            // This is expected in a test environment since we're using dummy transactions
        }
    }
    
    println!("=== VENDOR PERFORMANCE REPORTING TEST COMPLETED ===");
    Ok(())
}

/// Demonstrate the performance insights you can gain
pub async fn demonstrate_performance_insights() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== DEMONSTRATING PERFORMANCE INSIGHTS ===");
    
    println!("📊 The new performance reporting provides:");
    println!("   • Individual send times for each vendor");
    println!("   • Ranking of vendors by speed (🥇🥈🥉)");
    println!("   • Success/failure status for each vendor");
    println!("   • Total elapsed time from detection");
    println!("   • Average vendor send time");
    println!("   • Success rate percentage");
    println!("   • Comprehensive summary statistics");
    
    println!("\n🎯 Benefits:");
    println!("   • Identify fastest vendors for optimization");
    println!("   • Monitor vendor reliability");
    println!("   • Track performance trends over time");
    println!("   • Make data-driven vendor selection decisions");
    println!("   • Debug vendor-specific issues");
    
    println!("\n📈 Example output format:");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] ===== VENDOR PERFORMANCE REPORT =====");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] ✅ SUCCESSFUL VENDORS (3):");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] 🥇 zeroslot: 153.75ms | sig: abc123... | total elapsed: 200.50ms");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] 🥈 jito: 180.20ms | sig: def456... | total elapsed: 200.50ms");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] 🥉 rpc: 195.30ms | sig: ghi789... | total elapsed: 200.50ms");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] ❌ FAILED VENDORS (1):");
    println!("   [TIMESTAMP] - [GENERIC_SENDER]   nextblock: 250.00ms (FAILED)");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] 📊 SUMMARY: Total time: 200.50ms | Avg vendor time: 194.56ms | Success rate: 3/4");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] ================================================");
    
    println!("\n=== PERFORMANCE INSIGHTS DEMONSTRATION COMPLETED ===");
    Ok(())
}

/// Show how to use the performance data for optimization
pub async fn demonstrate_optimization_strategies() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== DEMONSTRATING OPTIMIZATION STRATEGIES ===");
    
    println!("🔧 Optimization strategies based on performance data:");
    println!("\n1. **Vendor Prioritization**:");
    println!("   • Use fastest vendors first in future builds");
    println!("   • Skip consistently slow vendors");
    println!("   • Implement vendor-specific retry logic");
    
    println!("\n2. **Load Balancing**:");
    println!("   • Distribute transactions across fastest vendors");
    println!("   • Avoid overloading single vendors");
    println!("   • Implement vendor health checks");
    
    println!("\n3. **Performance Monitoring**:");
    println!("   • Track vendor performance over time");
    println!("   • Set up alerts for vendor degradation");
    println!("   • A/B test different vendor configurations");
    
    println!("\n4. **Cost Optimization**:");
    println!("   • Compare vendor costs vs performance");
    println!("   • Optimize tip amounts based on success rates");
    println!("   • Implement dynamic tip adjustment");
    
    println!("\n5. **Reliability Improvements**:");
    println!("   • Identify and fix vendor-specific issues");
    println!("   • Implement fallback strategies");
    println!("   • Add vendor redundancy");
    
    println!("\n=== OPTIMIZATION STRATEGIES DEMONSTRATION COMPLETED ===");
    Ok(())
} 