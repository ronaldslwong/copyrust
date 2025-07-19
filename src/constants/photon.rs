use lazy_static::lazy_static;
use bs58;

pub const PUMP_SWAP_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";

lazy_static! {
    pub static ref PUMP_SWAP_PROGRAM_ID_BYTES: [u8; 32] = {
        let decoded = bs58::decode(PUMP_SWAP_PROGRAM_ID).into_vec().unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        arr
    };
} 