use solana_sdk::pubkey::Pubkey;



pub fn parse_tx(
    account_key_list: &[Vec<u8>],
    account_list: &[u8],
    ac_x_pos: usize,
    mint_x_byte_pos: usize,
    mint_y_byte_pos: usize,
    data: &[u8],
) -> (Pubkey, u64, u64) {
    // Extract mint
    let mint = if account_list.len() > ac_x_pos {
        let idx = account_list[ac_x_pos] as usize;
        if account_key_list.len() > idx {
            Pubkey::try_from(account_key_list[idx].as_slice()).unwrap_or_default()
        } else {
            Pubkey::default()
        }
    } else {
        Pubkey::default()
    };
    // Extract u1
    let u1 = if data.len() >= mint_x_byte_pos + 8 {
        let slice = &data[mint_x_byte_pos..mint_x_byte_pos + 8];
        u64::from_le_bytes(slice.try_into().unwrap())
    } else {
        0
    };
    // Extract u2
    let u2 = if data.len() >= mint_y_byte_pos + 8 {
        let slice = &data[mint_y_byte_pos..mint_y_byte_pos + 8];
        u64::from_le_bytes(slice.try_into().unwrap())
    } else {
        0
    };
    (mint, u1, u2)
}
