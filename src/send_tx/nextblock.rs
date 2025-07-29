use crate::init::wallet_loader::{get_wallet_keypair, get_nonce_account};
use base64::{engine::general_purpose, Engine as _};
use once_cell::sync::OnceCell;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use std::str::FromStr;
use std::sync::Arc;
use tonic::codegen::InterceptedService;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, ClientTlsConfig};
use tonic::{Request, Status};
use solana_sdk::signer::keypair::Keypair;
use rand::Rng;
use solana_sdk::compute_budget;

// You must have the generated gRPC client from NextBlock proto
// Example: use nextblock_proto::api_client::ApiClient;
pub mod nextblock_proto {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

#[derive(Clone)]
pub struct ApiTokenInterceptor {
    pub token: String,
}

impl tonic::service::Interceptor for ApiTokenInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        let meta = MetadataValue::from_str(&self.token).unwrap();
        req.metadata_mut().insert("authorization", meta);
        Ok(req)
    }
}

// Global NextBlock gRPC client (shared, persistent)
pub static NEXTBLOCK_CLIENT: OnceCell<
    Arc<nextblock_proto::api_client::ApiClient<InterceptedService<Channel, ApiTokenInterceptor>>>,
> = OnceCell::new();

/// Initialize the global NextBlock gRPC client. Call this ONCE at startup.
pub async fn initialize_nextblock_client(address: &str, token: &str, plaintext: bool) {
    let channel = connect_to_nextblock(address, token, plaintext)
        .await
        .expect("Failed to connect to NextBlock");
    let client = nextblock_proto::api_client::ApiClient::with_interceptor(
        channel,
        ApiTokenInterceptor {
            token: token.to_string(),
        },
    );
    NEXTBLOCK_CLIENT
        .set(Arc::new(client))
        .expect("NextBlock client already set");
}

/// Get a clone of the global NextBlock client (Arc)
pub fn get_nextblock_client(
) -> Arc<nextblock_proto::api_client::ApiClient<InterceptedService<Channel, ApiTokenInterceptor>>> {
    NEXTBLOCK_CLIENT
        .get()
        .expect("NextBlock client not initialized")
        .clone()
}

pub async fn connect_to_nextblock(
    address: &str,
    _token: &str,
    plaintext: bool,
) -> Result<Channel, Box<dyn std::error::Error>> {
    let channel = if plaintext {
        Channel::from_shared(address.to_string())?.connect().await?
    } else {
        Channel::from_shared(address.to_string())?
            .tls_config(ClientTlsConfig::new())?
            .connect()
            .await?
    };
    Ok(channel)
}

/// Send a signed Solana transaction via NextBlock gRPC
pub async fn send_tx_nextblock(
    tx: &Transaction,
    token: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let client = get_nextblock_client();
    let mut client = Arc::try_unwrap(client).unwrap_or_else(|arc| (*arc).clone());
    let tx_bytes = bincode::serialize(tx)?;
    let tx_b64 = general_purpose::STANDARD.encode(&tx_bytes);

    let request = nextblock_proto::PostSubmitRequest {
        transaction: Some(nextblock_proto::TransactionMessage {
            content: tx_b64,
            is_cleanup: false,
        }),
        skip_pre_flight: true,
        front_running_protection: Some(false),
        snipe_transaction: Some(true),
        disable_retries: Some(true),
        experimental_front_running_protection: Some(false),
        revert_on_fail: Some(false),
        // ... other fields as needed
    };

    let mut req = tonic::Request::new(request);
    req.metadata_mut()
        .insert("authorization", MetadataValue::from_str(token)?);

    let response = client.post_submit_v2(req).await?;
    Ok(response.into_inner().signature)
}

/// Build compute budget instructions for NextBlock
pub fn create_instruction_nextblock(
    instructions: Vec<Instruction>,
    tip: u64,
    cu_price: u64,
    nonce_account: &Pubkey,
) -> Vec<Instruction> {

    let mut rng = rand::thread_rng();
    let random_addition: u64 = rng.gen_range(1..=100);
    let adjusted_cu_price = cu_price + random_addition;
    let keypair: &'static Keypair = get_wallet_keypair();

    let price_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(adjusted_cu_price);

    let tip_ix = nextblock_tip(
        "NEXTbLoCkB51HpLBLojQfpyVAMorm3zzKg7w9NFdqid",
        tip,
        &keypair.pubkey(),
    );
    // Create advance nonce instruction using the provided nonce account
    let advance_nonce_ix = system_instruction::advance_nonce_account(
        nonce_account,
        &keypair.pubkey(),
    );

    let mut result = vec![advance_nonce_ix, tip_ix, price_ix];
    result.extend(instructions);
    result
}

/// Create a system transfer instruction for NextBlock tips
pub fn nextblock_tip(tip_ac: &str, tip: u64, from_pubkey: &Pubkey) -> Instruction {
    let tip_pubkey = Pubkey::from_str(tip_ac).expect("Invalid pubkey");
    system_instruction::transfer(from_pubkey, &tip_pubkey, tip)
}

// pub fn create_instruction_nextblock(
//     cu_limit: u32,
//     cu_price: u64,
//     mint: Pubkey,
//     instructions: Vec<Instruction>,
//     tip: u64,
// ) -> Vec<Instruction> {
//     let limit_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
//     let price_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(cu_price);

//     let keypair = get_wallet_keypair();

//     let tip_ix = nextblock_tip(
//         "NEXTbLoCkB51HpLBLojQfpyVAMorm3zzKg7w9NFdqid",
//         tip,
//         &keypair.pubkey(),
//     );
//     let ata_ix = create_ata(&keypair, &keypair.pubkey(), &mint);

//     let mut result = vec![limit_ix, price_ix, tip_ix, ata_ix];
//     result.extend(instructions);
//     result
// }