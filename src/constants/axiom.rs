use lazy_static::lazy_static;
use bs58;

pub const AXIOM_PUMP_SWAP_PROGRAM_ID: &str = "AxiomxSitiyXyPjKgJ9XSrdhsydtZsskZTEDam3PxKcC";
pub const AXIOM_PUMP_FUN_PROGRAM_ID: &str = "AxiomfHaWDemCFBLBayqnEnNwE6b7B2Qz3UmzMpgbMG6";

lazy_static! {
    pub static ref AXIOM_PUMP_SWAP_PROGRAM_ID_BYTES: [u8; 32] = {
        let decoded = bs58::decode(AXIOM_PUMP_SWAP_PROGRAM_ID).into_vec().unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        arr
    };
} 

lazy_static! {
    pub static ref AXIOM_PUMP_FUN_PROGRAM_ID_BYTES: [u8; 32] = {
        let decoded = bs58::decode(AXIOM_PUMP_FUN_PROGRAM_ID).into_vec().unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        arr
    };
} 



