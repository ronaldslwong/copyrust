use solana_sdk::signature::Signature;
use solana_sdk::pubkey::Pubkey;
use solana_transaction_status::UiTransactionEncoding;
use crate::init::initialize::GLOBAL_RPC_CLIENT;
use std::str::FromStr;

/// Returns (pre_balance, post_balance) for the given mint in the transaction, or None if not found.
pub fn get_token_balance_change_for_mint(
    signature: &str,
    mint: &Pubkey,
) -> Option<(u64, u64)> {
    let rpc = GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    let sig = Signature::from_str(signature).ok()?;
    let tx = rpc.get_transaction(&sig, UiTransactionEncoding::JsonParsed).ok()?;
    let meta = tx.transaction.meta.as_ref()?;

    // Helper to extract the balance for a mint from a token balance array
    fn find_balance(
        balances: &solana_transaction_status::option_serializer::OptionSerializer<
            Vec<solana_transaction_status::UiTransactionTokenBalance>,
        >,
        mint: &Pubkey,
    ) -> Option<u64> {
        match balances.as_ref() {
            solana_transaction_status::option_serializer::OptionSerializer::Some(vec) => {
                vec.iter()
                    .find(|b| Pubkey::from_str(&b.mint).ok().as_ref() == Some(mint))
                    .and_then(|b| b.ui_token_amount.amount.parse::<u64>().ok())
            }
            _ => None,
        }
    }

    let pre = find_balance(&meta.pre_token_balances, mint).unwrap_or(0);
    let post = find_balance(&meta.post_token_balances, mint).unwrap_or(0);

    // If both are zero, likely not present
    if pre == 0 && post == 0 {
        None
    } else {
        Some((pre, post))
    }
} 