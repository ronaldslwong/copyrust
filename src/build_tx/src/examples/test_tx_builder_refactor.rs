use crate::build_tx::tx_builder::build_optimized_transaction;
use crate::build_tx::tx_builder::default_instruction;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Test the refactored transaction builder
pub fn test_tx_builder_refactor() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== TESTING TX BUILDER REFACTOR ===");
    
    // Create a test instruction
    let test_instruction = default_instruction();
    
    // Create a test mint
    let test_mint = Pubkey::from_str("11111111111111111111111111111112")?;
    
    // Test the new function
    let result = build_optimized_transaction(
        test_instruction,
        test_mint,
        1000, // test amount
        "test_sig",
    );
    
    match result {
        Ok(tx) => {
            println!("✅ SUCCESS: Transaction built successfully");
            println!("   - Transaction signature: {}", tx.signatures[0]);
            println!("   - Transaction has {} instructions", tx.message.instructions.len());
        }
        Err(e) => {
            println!("❌ ERROR: Failed to build transaction: {}", e);
        }
    }
    
    println!("=== TEST COMPLETED ===");
    Ok(())
}

/// Test the future vendor-specific function (placeholder)
pub async fn test_vendor_specific_building() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("=== TESTING VENDOR-SPECIFIC BUILDING (PLACEHOLDER) ===");
    
    let test_instruction = default_instruction();
    let test_mint = Pubkey::from_str("11111111111111111111111111111112")?;
    
    // This will be implemented later for parallel vendor-specific transactions
    let result = crate::build_tx::tx_builder::build_vendor_specific_transactions(
        test_instruction,
        test_mint,
        1000,
        "test_sig",
    ).await?;
    
    println!("✅ SUCCESS: Vendor-specific building placeholder works");
    println!("   - Built {} transaction variants", result.len());
    
    for (vendor, tx) in result {
        println!("   - {}: {} instructions", vendor, tx.message.instructions.len());
    }
    
    println!("=== VENDOR-SPECIFIC TEST COMPLETED ===");
    Ok(())
} 