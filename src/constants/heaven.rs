use lazy_static::lazy_static;
use bs58;
use solana_sdk::pubkey::Pubkey;
use solana_program::pubkey;


pub const HEAVEN_PROGRAM_ID: &str = "HEAVENoP2qxoeuF8Dj2oT1GHEnu49U5mJYkdeC8BAX2o";
pub const HEAVEN_PROGRAM_ID_PUBKEY: Pubkey = pubkey!("HEAVENoP2qxoeuF8Dj2oT1GHEnu49U5mJYkdeC8BAX2o");
pub const HEAVEN_PROTOCOL_CONFIG: Pubkey = pubkey!("42mepa9xLCtuerAEnnDY43KLRN5dgkrkKvoCT6nDZsyj");
pub const HEAVEN_15: Pubkey = pubkey!("HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny");
pub const HEAVEN_16: Pubkey = pubkey!("CH31Xns5z3M1cTAbKW34jcxPPciazARpijcHj9rxtemt");

lazy_static! {
    pub static ref HEAVEN_PROGRAM_ID_BYTES: [u8; 32] = {
        let decoded = bs58::decode(HEAVEN_PROGRAM_ID).into_vec().unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        arr
    };
} 