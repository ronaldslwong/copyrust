use lazy_static::lazy_static;
use bs58;
use solana_sdk::pubkey::Pubkey;
use solana_program::pubkey;


pub const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const PUMP_FUN_PROGRAM_ID_PUBKEY: Pubkey = pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const GLOBAL_ACCOUNT: Pubkey = pubkey!("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf");
pub const FEE_RECIPIENT: Pubkey = pubkey!("FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz");
pub const MINT_AUTHORITY: Pubkey = pubkey!("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");

lazy_static! {
    pub static ref PUMP_FUN_PROGRAM_ID_BYTES: [u8; 32] = {
        let decoded = bs58::decode(PUMP_FUN_PROGRAM_ID).into_vec().unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        arr
    };
} 