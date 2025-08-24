use lazy_static::lazy_static;
use bs58;
use solana_sdk::pubkey::Pubkey;
use solana_program::pubkey;

pub const METEORA_BONDING_PROGRAM_ID: &str = "dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN";
pub const METEORA_BONDING_PROGRAM_ID_PUBKEY: Pubkey = pubkey!("dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN");
pub const METEORA_BONDING_POOL_AUTHORITY: Pubkey = pubkey!("FhVo3mqL8PW5pH5U2CN4XE33DokiyZnUwuGpH2hmHLuM");
pub const METEORA_BONDING_EVENT_AUTHORITY: Pubkey = pubkey!("8Ks12pbrD6PXxfty1hVQiE9sc289zgU1zHkvXhrSdriF");

// Meteora DBC Program Addresses (from official SDK)
// Mainnet-beta: dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN
// Devnet: dbcij3LWUppWqq96dh6gJWwBifmcGfLSB5D4DuSMaqN

// Migration fee option config keys for Meteora damm (v1)
pub const METEORA_DAMM_V1_MIGRATION_CONFIGS: [Pubkey; 6] = [
    pubkey!("8f848CEy8eY6PhJ3VcemtBDzPPSD4Vq7aJczLZ3o8MmX"), // 0.25%
    pubkey!("HBxB8Lf14Yj8pqeJ8C4qDb5ryHL7xwpuykz31BLNYr7S"), // 0.3%
    pubkey!("7v5vBdUQHTNeqk1HnduiXcgbvCyVEZ612HLmYkQoAkik"), // 1%
    pubkey!("EkvP7d5yKxovj884d2DwmBQbrHUWRLGK6bympzrkXGja"), // 2%
    pubkey!("9EZYAJrcqNWNQzP2trzZesP7XKMHA1jEomHzbRsdX8R2"), // 4%
    pubkey!("8cdKo87jZU2R12KY1BUjjRPwyjgdNjLGqSGQyrDshhud"), // 6%
];

// Migration fee option config keys for Damm v2
pub const METEORA_DAMM_V2_MIGRATION_CONFIGS: [Pubkey; 7] = [
    pubkey!("7F6dnUcRuyM2TwR8myT1dYypFXpPSxqwKNSFNkxyNESd"), // 0.25%
    pubkey!("2nHK1kju6XjphBLbNxpM5XRGFj7p9U8vvNzyZiha1z6k"), // 0.3%
    pubkey!("Hv8Lmzmnju6m7kcokVKvwqz7QPmdX9XfKjJsXz8RXcjp"), // 1%
    pubkey!("2c4cYd4reUYVRAB9kUUkrq55VPyy2FNQ3FDL4o12JXmq"), // 2%
    pubkey!("AkmQWebAwFvWk55wBoCr5D62C6VVDTzi84NJuD9H7cFD"), // 4%
    pubkey!("DbCRBj8McvPYHJG1ukj8RE15h2dCNUdTAESG49XpQ44u"), // 6%
    pubkey!("A8gMrEPJkacWkcb3DGwtJwTe16HktSEfvwtuDh2MCtck"), // Customizable
];

lazy_static! {
    pub static ref METEORA_BONDING_PROGRAM_ID_BYTES: [u8; 32] = {
        let decoded = bs58::decode(METEORA_BONDING_PROGRAM_ID).into_vec().unwrap();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&decoded);
        arr
    };
} 