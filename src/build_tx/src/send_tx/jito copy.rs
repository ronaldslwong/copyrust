// src/send_tx/jito.rs
// Jito gRPC bundle sender for Solana, inspired by jitoGrpc.go and init.go (Go)
// NOTE: This is a skeleton. You must fill in the actual Jito gRPC client logic using the appropriate Rust crate or gRPC codegen.

use std::sync::{Mutex, OnceLock};
use solana_sdk::transaction::Transaction;
use solana_sdk::signature::{Keypair, read_keypair_file, Signer};
use tonic::transport::Channel;
use tonic::{Request, metadata::MetadataValue};
use std::path::Path;
use jito::searcher::{searcher_service_client::SearcherServiceClient, SendBundleRequest};
use jito::bundle::Bundle;
use jito::packet::Packet;
use jito::auth::{auth_service_client::AuthServiceClient, GenerateAuthChallengeRequest, GenerateAuthTokensRequest, Role};

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

static GLOBAL_KEYPAIR: OnceLock<Keypair> = OnceLock::new();
static JITO_GRPC_SENDER: OnceLock<Mutex<JitoGrpcSender>> = OnceLock::new();
static ACCESS_TOKEN: OnceLock<String> = OnceLock::new();

pub struct JitoGrpcClient {
    pub client: SearcherServiceClient<Channel>,
}

pub struct JitoGrpcSender {
    pub client: JitoGrpcClient,
}

/// Load the global keypair from ./jito_auth.json
pub fn load_global_keypair() -> &'static Keypair {
    GLOBAL_KEYPAIR.get_or_init(|| {
        read_keypair_file("./jito_auth.json").expect("Failed to read keypair file ./jito_auth.json")
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
    let mut to_sign = keypair.pubkey().to_bytes().to_vec();
    to_sign.extend_from_slice(challenge_resp.challenge.as_bytes());
    let signature = keypair.sign_message(&to_sign);
    // 3. Request tokens
    let tokens_resp = auth_client.generate_auth_tokens(
        GenerateAuthTokensRequest {
            challenge: challenge_resp.challenge,
            client_pubkey: keypair.pubkey().to_bytes().to_vec(),
            signed_challenge: signature.as_ref().to_vec(),
        }
    ).await?.into_inner();
    let access_token = tokens_resp.access_token.expect("No access token returned").value;
    Ok(access_token)
}

/// Initialize the Jito gRPC sender (like NewJitoBundleSender in Go)
pub async fn init_jito_grpc_sender(block_engine_url: &str) {
    let keypair = load_global_keypair();
    let url = block_engine_url.to_string();
    let access_token = authenticate(&url, keypair).await.expect("Failed to authenticate with Jito block engine");
    let channel = Channel::from_shared(url.clone()).unwrap().connect().await.unwrap();
    let client = SearcherServiceClient::new(channel);
    let grpc_client = JitoGrpcClient { client };
    let sender = JitoGrpcSender { client: grpc_client };
    JITO_GRPC_SENDER.set(Mutex::new(sender)).ok();
    ACCESS_TOKEN.set(access_token).ok();
}

/// Send a bundle via Jito gRPC (like SendGrpcBundle in Go)
pub async fn send_jito_bundle(tx: &Transaction) -> Result<(), Box<dyn std::error::Error>> {
    let sender = JITO_GRPC_SENDER.get().expect("JitoGrpcSender not initialized").lock().unwrap();
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
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut client = sender.client.client.clone();
        let _resp = client.send_bundle(request).await?;
        println!("[JITO] Sent bundle");
        Ok(())
    })
}

// Example usage:
// init_jito_grpc_sender("http://block-engine-url:port");
// send_jito_bundle(&tx)?; 