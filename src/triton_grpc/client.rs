use crate::config_load::Config;
use crate::geyser::{
    geyser_client::GeyserClient, CommitmentLevel, SubscribeRequest, SubscribeRequestFilterBlocks,
    SubscribeRequestFilterTransactions,
};
use crate::triton_grpc::parser::process_triton_message;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tonic::transport::Endpoint;
use crate::init::wallet_loader::get_wallet_keypair;
use solana_sdk::signature::Signer;
use core_affinity;


// use chrono::Utc;

pub async fn subscribe_and_print_triton(
    endpoint: &str,
    config: Arc<Config>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Pin this async runtime thread to core 1 for lowest latency
    if let Some(cores) = core_affinity::get_core_ids() {
        if cores.len() > 1 {
            core_affinity::set_for_current(cores[0]);
            println!("[client.rs] Pinned to core 0");
        }
    }
    println!("[Triton] Connecting to endpoint: {}", endpoint);
    let channel_endpoint = Endpoint::from_shared(endpoint.to_string())?
        .http2_keep_alive_interval(Duration::from_secs(10))
        .keep_alive_timeout(Duration::from_secs(1))
        .keep_alive_while_idle(true);
    let channel = channel_endpoint.connect().await?;
    let mut client = GeyserClient::new(channel)
        .max_decoding_message_size(8 * 1024 * 1024) // 8 MB
        .max_encoding_message_size(8 * 1024 * 1024);
    println!("[Triton] Connection successful.");

    let mut accounts_to_monitor = config.accounts_monitor.clone();
    accounts_to_monitor.push(get_wallet_keypair().pubkey().to_string());
    println!(
        "[Triton] Subscribing to accounts: {:?}",
        accounts_to_monitor
    );

    let mut transactions = HashMap::new();
    transactions.insert(
        "transactions".to_string(),
        SubscribeRequestFilterTransactions {
            vote: Some(false),
            failed: Some(false),
            signature: None,
            account_include: accounts_to_monitor,
            account_exclude: vec![],
            account_required: vec![],
        },
    );

    let mut blocks = HashMap::new();
    blocks.insert(
        "blocks".to_string(),
        SubscribeRequestFilterBlocks {
            account_include: vec![],
            include_transactions: Some(true),
            include_accounts: Some(false),
            include_entries: Some(false),
        },
    );

    let initial_request = SubscribeRequest {
        transactions,
        slots: HashMap::new(),
        accounts: HashMap::new(),
        blocks,
        commitment: Some(CommitmentLevel::Processed as i32),
        accounts_data_slice: vec![],
        ping: None,
        transactions_status: HashMap::new(),
        from_slot: None,
        blocks_meta: HashMap::new(),
        entry: HashMap::new(),
    };

    println!(
        "[Triton] Sending subscription request: {:?}",
        initial_request
    );

    let mut stream = client
        .subscribe(tokio_stream::iter(vec![initial_request]))
        .await?
        .into_inner();
    println!("[Triton] Subscription stream established.");

    while let Some(message) = stream.message().await? {
        // println!("[Triton] Received raw message.");
        let message = message.clone();
        tokio::spawn(async move {
            // let now = Utc::now();
            // println!("[{}] - [Triton] Spawning message handler", now.format("%Y-%m-%d %H:%M:%S%.3f"));
            process_triton_message(&message);
        });
    }

    Ok(())
}

pub async fn subscribe_with_retry_triton(
    endpoint: &str,
    config: Arc<Config>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        println!("[Triton] Attempt {} to connect and subscribe...", attempt);
        let result = subscribe_and_print_triton(endpoint, config.clone()).await;
        match result {
            Ok(_) => {
                println!("[Triton] Subscription ended gracefully.");
                break;
            }
            Err(e) => {
                eprintln!("[Triton] Subscription error: {}. Retrying in 5 seconds...", e);
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
    Ok(())
}
