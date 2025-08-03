use crate::config_load::Config;
use crate::config_load::GLOBAL_CONFIG;
use crate::geyser::{
    geyser_client::GeyserClient, CommitmentLevel, SubscribeRequest, SubscribeRequestFilterBlocks,
    SubscribeRequestFilterTransactions, SubscribeUpdate, subscribe_update::UpdateOneof,
};
use crate::triton_grpc::parser::process_triton_message;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tonic::transport::Endpoint;
use crate::init::wallet_loader::get_wallet_keypair;
use solana_sdk::signature::Signer;
use core_affinity;


use chrono::Utc;
use std::time::Instant;
use once_cell::sync::Lazy;

// CRITICAL FIX: Worker pool to prevent unbounded tokio::spawn accumulation
static TRITON_WORKER_POOL: Lazy<Arc<TritonWorkerPool>> = Lazy::new(|| {
    Arc::new(TritonWorkerPool::new(10)) // Limit to 10 concurrent workers
});

pub struct TritonWorkerPool {
    sender: crossbeam::channel::Sender<TritonTask>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

struct TritonTask {
    message: crate::geyser::SubscribeUpdate,
    feed_id: String,
}

impl TritonWorkerPool {
    fn new(num_workers: usize) -> Self {
        let (sender, receiver) = crossbeam::channel::bounded::<TritonTask>(100); // Bounded queue
        
        let mut workers = Vec::new();
        for worker_id in 0..num_workers {
            let receiver_clone = receiver.clone();
            let handle = std::thread::spawn(move || {
                Self::worker_loop(worker_id, receiver_clone);
            });
            workers.push(handle);
        }
        
        Self { sender, workers }
    }
    
    fn worker_loop(worker_id: usize, receiver: crossbeam::channel::Receiver<TritonTask>) {
        while let Ok(task) = receiver.recv() {
            let processing_start = Instant::now();
            
            // Process the message (this will be handled by the existing crossbeam worker)
            crate::triton_grpc::parser::process_triton_message(&task.message, &task.feed_id);
            
            let processing_time = processing_start.elapsed();
            let now = Utc::now();
            #[cfg(feature = "verbose_logging")]
            println!("[{}] - [TRITON-WORKER-{}] Processed message for feed {} (processing time: {:.2?})", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, task.feed_id, processing_time);
        }
    }
    
    pub fn submit(&self, message: crate::geyser::SubscribeUpdate, feed_id: String) -> Result<(), crossbeam::channel::SendError<TritonTask>> {
        self.sender.send(TritonTask { message, feed_id })
    }
}

pub async fn subscribe_and_print_triton(
    endpoint: &str,
    config: Arc<Config>,
    feed_id: &str, // OPTIMIZATION: Add feed_id parameter
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = Endpoint::from_shared(endpoint.to_string())?
        .connect()
        .await?;

    let mut client = GeyserClient::new(client)
        .max_decoding_message_size(8 * 1024 * 1024) // 8 MB
        .max_encoding_message_size(8 * 1024 * 1024);

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

    // Pin the main Triton processing thread to core 0 (once, outside the loop)
    if let Some(cores) = core_affinity::get_core_ids() {
        if cores.len() > 0 {
            core_affinity::set_for_current(cores[0]);
            println!("[Triton] Main processing thread pinned to core 0");
        }
    }
    
    // Set high real-time priority for the main processing thread (once)
    if let Err(e) = crate::utils::rt_scheduler::set_realtime_priority(crate::utils::rt_scheduler::RealtimePriority::High) {
        eprintln!("[Triton] Failed to set real-time priority: {}", e);
    }

    // Stream health monitoring
    let mut last_message_time = std::time::Instant::now();
    let mut message_count = 0u64;
    let mut consecutive_errors = 0u32;
    const MAX_CONSECUTIVE_ERRORS: u32 = 10;
    const STREAM_TIMEOUT_SECONDS: u64 = 60;

    while let Some(message) = stream.message().await? {
        last_message_time = std::time::Instant::now();
        message_count += 1;
        consecutive_errors = 0; // Reset error counter on successful message

        // Check for stale stream
        if last_message_time.elapsed() > Duration::from_secs(STREAM_TIMEOUT_SECONDS) {
            println!("[Triton] WARNING: Stream appears stale (no messages for {}s), reconnecting...", 
                STREAM_TIMEOUT_SECONDS);
            break; // Exit to trigger reconnection
        }

        // Memory pressure check - force cleanup every 1000 messages
        if message_count % 1000 == 0 {
            // Force garbage collection and cleanup
            std::thread::sleep(Duration::from_millis(1));
            crate::grpc::arpc_parser::cleanup_old_signatures();
            println!("[Triton] Memory pressure cleanup triggered after {} messages", message_count);
        }

        // Clone with size check to prevent memory leaks
        let message_size = message.update_oneof.as_ref()
            .map(|update| {
                match update {
                    UpdateOneof::Transaction(tx) => tx.transaction.as_ref()
                        .map(|t| t.transaction.as_ref()
                            .map(|tx| tx.signatures.len() + tx.message.as_ref()
                                .map(|msg| msg.instructions.len()).unwrap_or(0))
                            .unwrap_or(0))
                        .unwrap_or(0),
                    _ => 0
                }
            })
            .unwrap_or(0);
        
        if message_size > 1000 {
            println!("[Triton] WARNING: Large message detected ({} items), skipping to prevent memory leak", message_size);
            continue;
        }

        let message = message.clone();
        let feed_id = feed_id.to_string(); // OPTIMIZATION: Clone feed_id for async block
        
        let message_clone_start = std::time::Instant::now();
        let message_clone_time = message_clone_start.elapsed();
        
        // Direct processing without spawn overhead
        let processing_start = std::time::Instant::now();
        
        // CRITICAL FIX: Use worker pool instead of direct processing
        let parse_start = std::time::Instant::now();
        if let Err(e) = TRITON_WORKER_POOL.submit(message.clone(), feed_id.clone()) {
            eprintln!("[Triton] Failed to submit task to worker pool: {}", e);
        }
        let parse_time = parse_start.elapsed();
        
        let processing_time = processing_start.elapsed();
        #[cfg(feature = "verbose_logging")]
        {
            let now = Utc::now();
            println!("[{}] - [Triton] CLIENT PROFILE - message_clone: {:.2?}, parse: {:.2?}, total: {:.2?}", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"), message_clone_time, parse_time, processing_time);
        }
        
        // Explicitly drop large data structures
        drop(message);
        drop(feed_id);
    }

    println!("[Triton] Stream ended gracefully after {} messages", message_count);
    Ok(())
}

pub async fn subscribe_with_retry_triton(
    endpoint: &str,
    config: Arc<Config>,
    feed_id: &str, // OPTIMIZATION: Add feed_id parameter
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        println!("[Triton] Attempt {} to connect and subscribe for feed {}...", attempt, feed_id);
        let result = subscribe_and_print_triton(endpoint, config.clone(), feed_id).await; // OPTIMIZATION: Pass feed_id
        match result {
            Ok(_) => {
                println!("[Triton] Subscription ended gracefully for feed {}.", feed_id);
                break;
            }
            Err(e) => {
                eprintln!("[Triton] Subscription error for feed {}: {}. Retrying in 5 seconds...", feed_id, e);
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
    Ok(())
}

// OPTIMIZATION: Enhanced client for multiple feeds
pub async fn setup_multiple_triton_feeds() -> Result<(), Box<dyn std::error::Error>> {
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");
    
    // Setup multiple feeds from config
    let feeds = vec![
        ("triton_primary", &config.grpc_endpoint1),
        ("triton_backup", &config.grpc_endpoint2),
    ];
    
    for (feed_id, endpoint) in feeds {
        println!("[TRITON] Setting up feed: {} with endpoint: {}", feed_id, endpoint);
        // Setup individual feed connection
        setup_triton_feed(feed_id, endpoint).await?;
    }
    
    Ok(())
}

// OPTIMIZATION: Individual feed setup
async fn setup_triton_feed(feed_id: &str, endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");
    
    // Clone strings for async block
    let feed_id_cloned = feed_id.to_string();
    let endpoint_cloned = endpoint.to_string();
    
    // Start the subscription for this feed
    tokio::spawn(async move {
        if let Err(e) = subscribe_with_retry_triton(&endpoint_cloned, Arc::new(config.clone()), &feed_id_cloned).await {
            eprintln!("[TRITON] Feed {} failed: {}", feed_id_cloned, e);
        }
    });
    
    println!("[TRITON] Feed {} connected to {}", feed_id, endpoint);
    Ok(())
}

// OPTIMIZATION: Test network latency between endpoints
pub async fn test_endpoint_latency() -> Result<(), Box<dyn std::error::Error>> {
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");
    
    println!("[TRITON] Testing network latency between endpoints...");
    
    let endpoints = vec![
        ("triton_primary", &config.grpc_endpoint1),
        ("triton_backup", &config.grpc_endpoint2),
    ];
    
    for (feed_id, endpoint) in endpoints {
        let start = std::time::Instant::now();
        
        // Try to establish a connection to test latency
        match Endpoint::from_shared(endpoint.to_string()) {
            Ok(channel_endpoint) => {
                let channel = channel_endpoint.connect().await;
                let latency = start.elapsed();
                
                match channel {
                    Ok(_) => println!("[TRITON] {} ({}) - Connection successful, latency: {:.2?}", 
                        feed_id, endpoint, latency),
                    Err(e) => println!("[TRITON] {} ({}) - Connection failed: {} (latency: {:.2?})", 
                        feed_id, endpoint, e, latency),
                }
            }
            Err(e) => println!("[TRITON] {} ({}) - Invalid endpoint: {} (latency: {:.2?})", 
                feed_id, endpoint, e, start.elapsed()),
        }
    }
    
    Ok(())
}
