use crate::build_tx::tx_builder::build_vendor_specific_transactions_parallel;
use crate::build_tx::tx_builder::default_instruction;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Test the parallel vendor-specific transaction building
pub fn test_parallel_vendor_building() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== TESTING PARALLEL VENDOR BUILDING ===");
    
    // Create a test instruction
    let test_instruction = default_instruction();
    
    // Create a test mint
    let test_mint = Pubkey::from_str("11111111111111111111111111111112")?;
    
    // Test the parallel function
    let result = build_vendor_specific_transactions_parallel(
        test_instruction,
        test_mint,
        1000, // test amount
        "test_sig",
    );
    
    match result {
        Ok(vendor_transactions) => {
            println!("✅ SUCCESS: Built {} vendor versions in parallel", vendor_transactions.len());
            
            for (vendor_name, tx) in &vendor_transactions {
                println!("   - {}: {} instructions, sig: {}", 
                    vendor_name, 
                    tx.message.instructions.len(),
                    tx.signatures[0]
                );
            }
            
            // Verify we have the expected vendors
            let vendor_names: Vec<&String> = vendor_transactions.iter().map(|(v, _)| v).collect();
            println!("   - Vendors built: {:?}", vendor_names);
            
            // Expected vendors: rpc, zeroslot, jito, nextblock
            let expected_vendors = vec!["rpc", "zeroslot", "jito", "nextblock"];
            for expected in expected_vendors {
                if vendor_names.contains(&expected.to_string()) {
                    println!("   ✅ {} version built successfully", expected);
                } else {
                    println!("   ❌ {} version missing", expected);
                }
            }
        }
        Err(e) => {
            println!("❌ ERROR: Failed to build vendor transactions: {}", e);
        }
    }
    
    println!("=== PARALLEL VENDOR BUILDING TEST COMPLETED ===");
    Ok(())
}

/// Benchmark parallel vs sequential building
pub fn benchmark_parallel_vs_sequential() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== BENCHMARKING PARALLEL VS SEQUENTIAL ===");
    
    let test_instruction = default_instruction();
    let test_mint = Pubkey::from_str("11111111111111111111111111111112")?;
    
    // Benchmark parallel building
    let parallel_start = std::time::Instant::now();
    let parallel_result = build_vendor_specific_transactions_parallel(
        test_instruction.clone(),
        test_mint,
        1000,
        "benchmark_sig",
    );
    let parallel_time = parallel_start.elapsed();
    
    match parallel_result {
        Ok(vendor_transactions) => {
            println!("✅ PARALLEL: Built {} versions in {:.2?}", 
                vendor_transactions.len(), parallel_time);
        }
        Err(e) => {
            println!("❌ PARALLEL failed: {}", e);
        }
    }
    
    // TODO: Add sequential benchmark for comparison
    // This would build each vendor version one by one
    
    println!("=== BENCHMARK COMPLETED ===");
    Ok(())
} 