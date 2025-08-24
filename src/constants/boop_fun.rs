use lazy_static::lazy_static;
use bs58;
use solana_sdk::pubkey::Pubkey;
use solana_program::pubkey;


pub const BOOP_FUN_PROGRAM_ID: &str = "boop8hVGQGqehUK2iVEMEnMrL5RbjywRzHKBmBE7ry4";
pub const BOOP_FUN_PROGRAM_ID_PUBKEY: Pubkey = pubkey!("boop8hVGQGqehUK2iVEMEnMrL5RbjywRzHKBmBE7ry4");

lazy_static! {
    pub static ref BOOP_FUN_PROGRAM_ID_BYTES: [u8; 32] = {
        let decoded = bs58::decode(BOOP_FUN_PROGRAM_ID).into_vec().unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        arr
    };
} 