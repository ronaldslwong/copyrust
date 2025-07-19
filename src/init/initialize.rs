use crate::config_load::{load_config, Config, GLOBAL_CONFIG};
use crate::init::bird_eye::load_birdeye_token_addresses;
use crate::init::dexscreener::{query_dexscreener, DexPairData};
use crate::init::wallet_loader::{get_wallet_keypair, load_wallet_keypair_global};
use crate::send_tx::nextblock::initialize_nextblock_client;
use futures::stream::{self, StreamExt};
use once_cell::sync::OnceCell;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signature::Signer;
use crate::send_tx::rpc::{initialize_send_rpc_clients};
use crate::utils::logger::setup_event_logger;
use crate::triton_grpc::crossbeam_worker::setup_crossbeam_worker;
use crate::grpc::arpc_parser::setup_arpc_crossbeam_worker;
use crate::send_tx::rpc::keep_blockhash_fresh;
use solana_sdk::hash::Hash;
use tokio::sync::RwLock;
use crate::send_tx::rpc::GLOBAL_LATEST_BLOCKHASH;
// use crate::send_tx::jito::init_jito_grpc_sender;
pub static GLOBAL_RPC_CLIENT: OnceCell<RpcClient> = OnceCell::new();


pub async fn initialize() -> (Config, Vec<DexPairData>) {
    println!("Initializing...");
    let config = load_config();
    GLOBAL_CONFIG
        .set(config.clone())
        .expect("Config already set");

    let mut mint_cache: Vec<DexPairData> = Vec::new();

    let _ = load_wallet_keypair_global("private_key.json.enc", "Metal@@2");

    let keypair = get_wallet_keypair();
    println!("Wallet loaded: {}", keypair.pubkey());

    initialize_nextblock_client(&config.nextblock_url, &config.nextblock_api, false).await;
    println!("Nextblock client initialized");

    initialize_rpc(&config);
    println!("RPC client initialized");

    initialize_send_rpc_clients(&config);
    println!("Send RPC clients initialized");
    // Spawn the keep-alive task in the background
    let _ = GLOBAL_LATEST_BLOCKHASH.set(RwLock::new(Hash::default()));

    tokio::spawn(async {
        keep_blockhash_fresh().await;
    });
    println!("Send RPC connections warmed up");

    setup_event_logger();
    println!("Event logger initialized");

    setup_crossbeam_worker();
    println!("GRPC Crossbeam worker initialized");

    setup_arpc_crossbeam_worker();
    println!("ARPC crossbeam worker initialized");

    // init_jito_grpc_sender(&config.jito_url).await;
    // println!("Jito gRPC sender initialized");

    if !config.birdeye_api.is_empty() {
        match load_birdeye_token_addresses(&config.birdeye_api, config.bird_eye_num_token as usize)
            .await
        {
            Ok(tokens) => {
                println!(
                    "Loaded {} token addresses from Birdeye. Querying DexScreener for each...",
                    tokens.len()
                );

                let responses = stream::iter(tokens)
                    .map(|token_ca: String| {
                        let config_ref = &config;
                        async move {
                            query_dexscreener(
                                &token_ca,
                                0.0, // pool_vol_filter - not in config, using 0.0
                                config_ref.pool_liq_filter as f64,
                                config_ref.total_volume_filter as f64,
                            )
                            .await
                        }
                    })
                    .buffer_unordered(10) // Process up to 10 requests concurrently
                    .collect::<Vec<_>>()
                    .await;

                for res in responses {
                    match res {
                        Ok(Some(dex_data)) => mint_cache.push(dex_data),
                        Ok(None) => (), // No data or filtered out
                        Err(e) => eprintln!("Failed to query dexscreener: {}", e),
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to load token addresses from Birdeye: {}", e);
            }
        }
    } else {
        println!("birdEyeApi is not configured in config.toml. Skipping Birdeye token list load.");
    }

    println!(
        "Initialization complete. Mint cache size: {}",
        mint_cache.len()
    );
    (config, mint_cache)
}

pub fn initialize_rpc(config: &Config) {
    println!("Initializing RPC client with endpoint: {}", config.rpc_endpoint);
    GLOBAL_RPC_CLIENT
        .set(RpcClient::new_with_commitment(config.rpc_endpoint.clone(), CommitmentConfig::processed()))
        .unwrap_or_else(|_| panic!("Failed to create RPC client"))
}
