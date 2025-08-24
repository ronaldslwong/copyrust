use crate::build_tx::tx_builder::build_vendor_specific_transactions_parallel;
use crate::send_tx::generic_sender::send_all_vendors_parallel;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::time::Instant;

/// Test the unified parallel transaction building (works for both buy and sell)
pub fn test_unified_parallel_building() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== TESTING UNIFIED PARALLEL BUILDING ===");
    
    // Create a dummy sell instruction (in real usage, this would come from the actual sell logic)
    let sell_instruction = Instruction::default();
    let mint = Pubkey::from_str("11111111111111111111111111111112")?;
    
    println!("🔨 Building vendor-specific transactions in parallel (unified function)...");
    
    let result = build_vendor_specific_transactions_parallel(
        sell_instruction,
        mint,
        0, // target_token_buy not used for sell transactions
        "test_sell_sig", // sig_str for logging
    );
    
    match result {
        Ok(vendor_transactions) => {
            println!("✅ SUCCESS - Built {} vendor transactions (unified function)", vendor_transactions.len());
            
            // Show the vendors that were built
            for (vendor_name, _transaction) in &vendor_transactions {
                println!("   📦 {}: Transaction built", vendor_name);
            }
            
            // Test parallel sending
            println!("\n🚀 Testing parallel sending of transactions...");
            let detection_time = Instant::now();
            
            // Note: This would fail in a test environment since we're using dummy transactions
            // In real usage, these would be properly signed transactions
            println!("   (Skipping actual send in test environment)");
        }
        Err(e) => {
            println!("❌ FAILED to build vendor transactions: {}", e);
        }
    }
    
    println!("=== UNIFIED PARALLEL BUILDING TEST COMPLETED ===");
    Ok(())
}

/// Demonstrate the complete sell flow
pub async fn demonstrate_complete_sell_flow() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== DEMONSTRATING COMPLETE SELL FLOW ===");
    
    println!("📋 Complete sell flow:");
    println!("1. 🔍 Detect sell opportunity");
    println!("2. 🔨 Build sell instruction");
    println!("3. 📦 Build vendor-specific sell transactions in parallel");
    println!("4. 🚀 Send all vendor transactions in parallel");
    println!("5. 🏆 First successful vendor wins");
    println!("6. 📊 Performance report for all vendors");
    
    println!("\n🎯 Benefits of parallel sell processing:");
    println!("   • Multiple vendor redundancy for sell transactions");
    println!("   • Faster sell execution through parallel sending");
    println!("   • Performance insights for sell optimization");
    println!("   • Consistent architecture with buy transactions");
    println!("   • Better reliability for time-sensitive sells");
    
    println!("\n📈 Example sell performance report:");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] ===== VENDOR PERFORMANCE REPORT =====");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] ✅ SUCCESSFUL VENDORS (3):");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] 🥇 zeroslot: 145.20ms | sig: sell123... | total elapsed: 180.50ms");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] 🥈 jito: 165.30ms | sig: sell456... | total elapsed: 180.50ms");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] 🥉 rpc: 175.40ms | sig: sell789... | total elapsed: 180.50ms");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] ❌ FAILED VENDORS (1):");
    println!("   [TIMESTAMP] - [GENERIC_SENDER]   nextblock: 220.00ms (FAILED)");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] 📊 SUMMARY: Total time: 180.50ms | Avg vendor time: 161.97ms | Success rate: 3/4");
    println!("   [TIMESTAMP] - [GENERIC_SENDER] ================================================");
    
    println!("\n=== COMPLETE SELL FLOW DEMONSTRATION FINISHED ===");
    Ok(())
}

/// Compare buy vs sell performance
pub fn compare_buy_vs_sell_performance() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== COMPARING BUY VS SELL PERFORMANCE ===");
    
    println!("🔄 Buy vs Sell Transaction Processing:");
    println!("\n📈 BUY TRANSACTIONS:");
    println!("   • Build vendor-specific buy transactions in parallel");
    println!("   • Store all vendor versions in TxWithPubkey");
    println!("   • Send all vendors in parallel when triggered");
    println!("   • Performance report shows all vendor times");
    
    println!("\n📉 SELL TRANSACTIONS:");
    println!("   • Build vendor-specific sell transactions in parallel");
    println!("   • Send all vendors immediately in parallel");
    println!("   • Performance report shows all vendor times");
    println!("   • Same parallel architecture as buy transactions");
    
    println!("\n🎯 Key Differences:");
    println!("   • Buy: Store first, send later (when triggered)");
    println!("   • Sell: Build and send immediately");
    println!("   • Both: Use same parallel vendor architecture");
    println!("   • Both: Provide comprehensive performance reporting");
    
    println!("\n⚡ Performance Benefits:");
    println!("   • Consistent parallel processing for both buy and sell");
    println!("   • Vendor redundancy for maximum reliability");
    println!("   • Performance insights for optimization");
    println!("   • Race condition safety for both operations");
    
    println!("\n=== BUY VS SELL COMPARISON COMPLETED ===");
    Ok(())
} 