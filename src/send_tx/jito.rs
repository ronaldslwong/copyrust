// src/send_tx/jito.rs
// Jito gRPC bundle sender for Solana, inspired by jitoGrpc.go and init.go (Go)
// NOTE: This is a skeleton. You must fill in the actual Jito gRPC client logic using the appropriate Rust crate or gRPC codegen.

use std::sync::{Mutex, OnceLock, Arc};
use solana_sdk::transaction::Transaction;
use solana_sdk::signature::{Keypair, read_keypair_file, Signer};
use tonic::transport::Channel;
use tonic::{Request, metadata::MetadataValue};
use tonic::service::Interceptor;
use std::path::Path;
use jito::{searcher::{searcher_service_client::SearcherServiceClient, SendBundleRequest},
// bundle::Bundle,
// packet::Packet,
auth::{auth_service_client::AuthServiceClient, GenerateAuthChallengeRequest, GenerateAuthTokensRequest, Role},};
use jito::bundle::Bundle;
use jito::packet::Packet;
// use jito::auth::{auth_service_client::AuthServiceClient, GenerateAuthChallengeRequest, GenerateAuthTokensRequest, Role};
use thiserror::Error;
use std::str::FromStr;
use crate::send_tx::jito_authenticator::ClientInterceptor;
use solana_sdk::{
    pubkey::Pubkey,
    system_instruction,
};
use solana_sdk::instruction::Instruction;
use rand::seq::SliceRandom;
use crate::init::wallet_loader::get_wallet_keypair;

use tonic::{
    codegen::{Body, Bytes, InterceptedService, StdError},
    transport,
    transport::{Endpoint},
    Response, Status, Streaming,
};

pub type BlockEngineConnectionResult<T> = Result<T, BlockEngineConnectionError>;

// List of Jito tip accounts
static JITO_TIP_ACCOUNTS: &[&str] = &[
    "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
    "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
    "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
    "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
    "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
    "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
    "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
    "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];

// Import generated proto clients and types
pub mod jito {
    pub mod searcher {
        // tonic::include_proto!("jito.searcher");
        include!(concat!(env!("OUT_DIR"), "/searcher.rs"));

    }
    pub mod bundle {
        // tonic::include_proto!("jito.bundle");
        include!(concat!(env!("OUT_DIR"), "/bundle.rs"));

    }

    pub mod auth {
        // tonic::include_proto!("jito.bundle");
        include!(concat!(env!("OUT_DIR"), "/auth.rs"));

    }
    pub mod packet {
        // tonic::include_proto!("jito.packet");
        include!(concat!(env!("OUT_DIR"), "/packet.rs"));

    }
    pub mod shared {
        include!(concat!(env!("OUT_DIR"), "/shared.rs"));
    }
}

static GLOBAL_KEYPAIR: OnceLock<Arc<Keypair>> = OnceLock::new();
static JITO_GRPC_SENDER: OnceLock<Mutex<JitoGrpcSender<InterceptedService<Channel, ClientInterceptor>>>> = OnceLock::new();
static ACCESS_TOKEN: OnceLock<String> = OnceLock::new();

#[derive(Debug, Error)]
pub enum BlockEngineConnectionError {
    #[error("transport error {0}")]
    TransportError(#[from] transport::Error),
    #[error("client error {0}")]
    ClientError(#[from] Status),
}

pub struct JitoGrpcClient<T> {
    pub client: SearcherServiceClient<T>,
}

pub struct JitoGrpcSender<T> {
    pub client: JitoGrpcClient<T>,
}

/// Load the global keypair from ./jito_auth.json
pub fn load_global_keypair() -> &'static Arc<Keypair> {
    GLOBAL_KEYPAIR.get_or_init(|| {
        Arc::new(read_keypair_file("./jito_auth.json").expect("Failed to read keypair file ./jito_auth.json"))
    })
}

/// Authenticate with the Jito block engine using the challenge/response flow
async fn authenticate(block_engine_url: &str, keypair: &Keypair) -> Result<String, Box<dyn std::error::Error>> {
    let mut auth_client = AuthServiceClient::connect(block_engine_url.to_string()).await?;
    // 1. Request challenge
    let challenge_resp = auth_client.generate_auth_challenge(
        GenerateAuthChallengeRequest {
            role: Role::Searcher as i32,
            pubkey: keypair.pubkey().to_bytes().to_vec(),
        }
    ).await?.into_inner();
    // 2. Sign pubkey || challenge (concatenate pubkey bytes and challenge string)
    // let mut to_sign = keypair.pubkey().to_bytes().to_vec();
    // to_sign.extend_from_slice(challenge_resp.challenge.as_bytes());
    
    // 3. Request tokens
    let challenge = format!("{}-{}", keypair.pubkey(), challenge_resp.challenge);
    let signature = keypair.sign_message(challenge.as_bytes()).as_ref().to_vec();

    let tokens_resp = auth_client.generate_auth_tokens(
        GenerateAuthTokensRequest {
            challenge: challenge,
            client_pubkey: keypair.pubkey().to_bytes().to_vec(),
            signed_challenge: signature,
        }
    ).await?.into_inner();
    let access_token = tokens_resp.access_token.expect("No access token returned").value;
    Ok(access_token)
}

/// Initialize the Jito gRPC sender (like NewJitoBundleSender in Go)
pub async fn init_jito_grpc_sender(block_engine_url: &str) {
    let keypair: &'static Arc<Keypair> = load_global_keypair();
    let url = block_engine_url.to_string();

    let auth_channel = create_grpc_channel(block_engine_url).await.expect("Failed to create gRPC channel");

    // Now pass a reference to the Arc
    let client_interceptor = ClientInterceptor::new(
        AuthServiceClient::new(auth_channel),
        keypair, // <-- This is &Arc<Keypair>
        Role::Searcher,
    )
    .await.expect("Failed to create client interceptor");


    let searcher_channel = create_grpc_channel(block_engine_url).await.expect("Failed to create gRPC channel");
    let searcher_client =
        SearcherServiceClient::with_interceptor(searcher_channel, client_interceptor);

    let access_token = authenticate(&url, keypair).await.expect("Failed to authenticate with Jito block engine");
    // let searcher_channel = create_grpc_channel(block_engine_url).await.expect("Failed to create gRPC channel");

    // let interceptor = ApiTokenInterceptor {
    //     token: format!("Bearer {}", access_token),
    // };
    // let searcher_client =
    //     SearcherServiceClient::with_interceptor(searcher_channel, interceptor);

    let grpc_client = JitoGrpcClient { client: searcher_client };
    let sender = JitoGrpcSender { client: grpc_client };

    JITO_GRPC_SENDER.set(Mutex::new(sender)).ok();
    ACCESS_TOKEN.set(access_token).ok();
}


pub async fn create_grpc_channel(url: &str) -> BlockEngineConnectionResult<Channel> {

    let mut endpoint = Endpoint::from_shared(url.to_string()).expect("invalid url");
    if url.starts_with("https") {
        endpoint = endpoint.tls_config(tonic::transport::ClientTlsConfig::new().with_native_roots())?;
    }
    Ok(endpoint.connect().await?)
}

/// Send a bundle via Jito gRPC (like SendGrpcBundle in Go)
pub async fn send_jito_bundle(tx: &Transaction) -> Result<String, Box<dyn std::error::Error>> {
    let access_token = ACCESS_TOKEN.get().expect("Access token not initialized");
    let tx_bytes = bincode::serialize(tx)?;
    let packet = Packet {
        data: tx_bytes,
        ..Default::default()
    };
    let bundle = Bundle {
        packets: vec![packet],
        ..Default::default()
    };
    let mut request = Request::new(SendBundleRequest {
        bundle: Some(bundle),
        ..Default::default()
    });
    // Attach the access token as metadata (header)
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", access_token)).unwrap(),
    );
    
    // Clone the client before the async operation to avoid holding the lock
    let mut client = {
        let sender = JITO_GRPC_SENDER.get().expect("JitoGrpcSender not initialized").lock().unwrap();
        sender.client.client.clone()
    };
    
    let resp = client.send_bundle(request).await?;
    let bundle_id = resp.into_inner().uuid;  // Extract bundle ID
    let now = chrono::Utc::now();
    println!("[{}] - [JITO] Sent bundle with ID: {}", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"), bundle_id);
    Ok(bundle_id)
}

// Example usage:
// init_jito_grpc_sender("http://block-engine-url:port");
// send_jito_bundle(&tx)?; 

pub fn jito_tip(tip: u64, from_pubkey: &Pubkey) -> Instruction {
    // Randomly select a tip account from the list
    let tip_account = JITO_TIP_ACCOUNTS
        .choose(&mut rand::thread_rng())
        .expect("Failed to select random tip account");
    
    let tip_pubkey = Pubkey::from_str(tip_account).expect("Invalid pubkey");
    system_instruction::transfer(from_pubkey, &tip_pubkey, tip)
}

pub fn create_instruction_jito(
    instructions: Vec<Instruction>,
    tip: u64,
) -> Vec<Instruction> {

    let keypair: &'static Keypair = get_wallet_keypair();

    let tip_ix = jito_tip(
        tip,
        &keypair.pubkey(),
    );

    let mut result = vec![tip_ix];
    result.extend(instructions);
    result
}

#[derive(Clone)]
pub struct ApiTokenInterceptor {
    pub token: String,
}

impl Interceptor for ApiTokenInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        let meta = MetadataValue::from_str(&self.token).unwrap();
        req.metadata_mut().insert("authorization", meta);
        Ok(req)
    }
} 