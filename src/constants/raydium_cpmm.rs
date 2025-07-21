use lazy_static::lazy_static;
use bs58;
use solana_sdk::pubkey::Pubkey;
use solana_program::pubkey;

pub const RAYDIUM_CPMM_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";
pub const RAYDIUM_CPMM_PROGRAM_ID_PUBKEY: Pubkey = pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");
pub const RAYDIUM_CPMM_AUTHORITY: Pubkey = pubkey!("GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL");
pub const RAYDIUM_CPMM_AMM_CONFIG: Pubkey = pubkey!("D4FPEruKEHrG5TenZ2mpDGEfu1iUvTiqBxvpU8HLBvC2");
pub const RAYDIUM_MIGRATION_LAUNCHPAD: [u8; 8] = [136, 92, 200, 103, 28, 218, 144, 140];

lazy_static! {
    pub static ref RAYDIUM_CPMM_PROGRAM_ID_BYTES: [u8; 32] = {
        let decoded = bs58::decode(RAYDIUM_CPMM_PROGRAM_ID).into_vec().unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        arr
    };
} 