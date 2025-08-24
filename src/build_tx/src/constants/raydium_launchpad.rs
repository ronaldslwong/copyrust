use lazy_static::lazy_static;
use bs58;
use solana_program::pubkey;
use solana_program::pubkey::Pubkey;


pub const RAYDIUM_LAUNCHPAD_PROGRAM_ID: &str = "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj";
pub const RAY_LAUNCH_PROGRAM_ID: Pubkey = pubkey!("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj");
pub const RAY_LAUNCH_AUTHORITY: Pubkey = pubkey!("WLHv2UAZm6z4KyaaELi5pjdbJh6RESMva1Rnn8pJVVh");
// Add other relevant constants as needed
pub const RAY_LAUNCH_GLOBAL_CONFIG: Pubkey = pubkey!("6s1xP3hpbAfFoNtUNF8mfHsjr2Bd97JxFJRWLbL6aHuX");
pub const RAY_LAUNCH_PROGRAM_CONFIG: Pubkey = pubkey!("FfYek5vEz23cMkWsdJwG2oa6EphsvXSHrGpdALN4g6W1");

pub const RAY_LAUNCH_EVENT_AUTHORITY: Pubkey = pubkey!("2DPAtwB8L12vrMRExbLuyGnC7n2J5LNoZQSejeQGpwkr");

lazy_static! {
    pub static ref RAYDIUM_LAUNCHPAD_PROGRAM_ID_BYTES: [u8; 32] = {
        let decoded = bs58::decode(RAYDIUM_LAUNCHPAD_PROGRAM_ID).into_vec().unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        arr
    };
} 