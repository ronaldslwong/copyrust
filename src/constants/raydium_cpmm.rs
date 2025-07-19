use lazy_static::lazy_static;
use bs58;

pub const RAYDIUM_CPMM_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";

lazy_static! {
    pub static ref RAYDIUM_CPMM_PROGRAM_ID_BYTES: [u8; 32] = {
        let decoded = bs58::decode(RAYDIUM_CPMM_PROGRAM_ID).into_vec().unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        arr
    };
} 