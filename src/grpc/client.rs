use super::arpc_parser::process_arpc_msg;
use crate::arpc::{
    arpc_service_client::ArpcServiceClient, SubscribeRequest as ArpcSubscribeRequest,
    SubscribeRequestFilterTransactions, SubscribeResponse,
};
use crate::config_load::Config;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio::time::{sleep, Duration};
use chrono::Utc;
use core_affinity;
use once_cell::sync::Lazy;

// Add global counters for monitoring
use std::sync::atomic::{AtomicUsize, Ordering};
static ARPC_MESSAGES_RECEIVED: AtomicUsize = AtomicUsize::new(0);
static ARPC_MESSAGES_PROCESSED: AtomicUsize = AtomicUsize::new(0);
static ARPC_MESSAGES_ERROR: AtomicUsize = AtomicUsize::new(0);

pub fn get_arpc_stats() -> (usize, usize, usize) {
    (
        ARPC_MESSAGES_RECEIVED.load(Ordering::Relaxed),
        ARPC_MESSAGES_PROCESSED.load(Ordering::Relaxed),
        ARPC_MESSAGES_ERROR.load(Ordering::Relaxed),
    )
}

// Add helper functions for GRPC client creation and subscription
async fn create_grpc_client(endpoint: &str) -> Result<ArpcServiceClient<tonic::transport::Channel>, Box<dyn std::error::Error + Send + Sync>> {
    let client = ArpcServiceClient::connect(endpoint.to_string()).await?;
    Ok(client)
}

async fn subscribe_to_accounts(
    client: &mut ArpcServiceClient<tonic::transport::Channel>,
    accounts_to_monitor: Vec<String>,
) -> Result<tonic::codec::Streaming<SubscribeResponse>, Box<dyn std::error::Error + Send + Sync>> {
    let mut filters: HashMap<String, SubscribeRequestFilterTransactions> = HashMap::new();
    if !accounts_to_monitor.is_empty() {
        filters.insert(
            "transactions".to_string(),
            SubscribeRequestFilterTransactions {
                account_include: accounts_to_monitor,
                account_exclude: vec![],
                account_required: vec![],
            },
        );
    }

    let (tx, rx) = mpsc::channel(128);
    let request_stream = ReceiverStream::new(rx);

    // Send the initial subscription request
    let initial_request = ArpcSubscribeRequest {
        transactions: filters,
        ping_id: None,
    };
    tx.send(initial_request).await?;

    let stream = client.subscribe(request_stream).await?.into_inner();
    println!("ARPC subscription established. Waiting for messages...");
    
    Ok(stream)
}

// CRITICAL FIX: Worker pool to prevent unbounded tokio::spawn accumulation
static ARPC_WORKER_POOL: once_cell::sync::Lazy<Arc<ArpcWorkerPool>> = once_cell::sync::Lazy::new(|| {
    Arc::new(ArpcWorkerPool::new(10)) // Limit to 10 concurrent workers
});

pub struct ArpcWorkerPool {
    sender: crossbeam::channel::Sender<ArpcTask>,
    workers: Vec<std::thread::JoinHandle<()>>,
}

struct ArpcTask {
    result: crate::arpc::SubscribeResponse,
    config: Arc<Config>,
}

impl ArpcWorkerPool {
    fn new(num_workers: usize) -> Self {
        let (sender, receiver) = crossbeam::channel::bounded::<ArpcTask>(100); // Bounded queue
        
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
    
    fn worker_loop(worker_id: usize, receiver: crossbeam::channel::Receiver<ArpcTask>) {
        while let Ok(task) = receiver.recv() {
            let processing_start = std::time::Instant::now();
            
            // Process the task synchronously to avoid nested async
            let sig_str = task.result.transaction.as_ref()
                .and_then(|tx| tx.signatures.get(0))
                .map(|sig| bs58::encode(sig).into_string())
                .unwrap_or_else(|| "<no_sig>".to_string());
            
            let now = Utc::now();
            println!("[{}] - [ARPC-WORKER-{}] Processing message for sig: {}", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, sig_str);
            
            // Process the message (this will be handled by the existing crossbeam worker)
            if let Some(trade) = crate::grpc::arpc_parser::process_arpc_msg_sync(&task.result, &task.config) {
                ARPC_MESSAGES_PROCESSED.fetch_add(1, Ordering::Relaxed);
                let processing_time = processing_start.elapsed();
                let now = Utc::now();
                println!("[{}] - [ARPC-WORKER-{}] SUCCESS - Trade detected: {:?} (processing time: {:.2?})", 
                    now.format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, trade, processing_time);
            } else {
                let processing_time = processing_start.elapsed();
                let now = Utc::now();
                println!("[{}] - [ARPC-WORKER-{}] No trade detected (processing time: {:.2?})", 
                    now.format("%Y-%m-%d %H:%M:%S%.3f"), worker_id, processing_time);
            }
        }
    }
    
    pub fn submit(&self, result: crate::arpc::SubscribeResponse, config: Arc<Config>) -> Result<(), crossbeam::channel::SendError<ArpcTask>> {
        self.sender.send(ArpcTask { result, config })
    }
}

pub async fn subscribe_and_print(
    endpoint: &str,
    accounts_to_monitor: Vec<String>,
    config: Arc<Config>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut client = create_grpc_client(endpoint).await?;
    let mut stream = subscribe_to_accounts(&mut client, accounts_to_monitor).await?;

    // Start stats monitoring with proper cleanup
    let _stats_config = config.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let (received, processed, errors) = get_arpc_stats();
            let now = Utc::now();
            println!("[{}] - [ARPC STATS] Received: {}, Processed: {}, Errors: {}, Processing Rate: {:.2}%", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"),
                received, processed, errors,
                if received > 0 { (processed as f64 / received as f64) * 100.0 } else { 0.0 }
            );
            
            // Also log memory usage and map stats
            let stats = crate::grpc::arpc_worker::get_map_stats();
            println!("[{}] - [ARPC STATS] GLOBAL_TX_MAP size: {}, entries: {:?}", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"),
                stats.0,
                stats.1.iter().map(|(tx_type, age)| format!("{}:{:.2?}", tx_type, age)).collect::<Vec<_>>().join(", ")
            );
        }
    });

    // Pin the main ARPC processing thread to core 1 (once, outside the loop)
    if let Some(cores) = core_affinity::get_core_ids() {
        if cores.len() > 1 {
            core_affinity::set_for_current(cores[1]);
            println!("[ARPC] Main processing thread pinned to core 1");
        }
    }
    
    // Set high real-time priority for the main processing thread (once)
    if let Err(e) = crate::utils::rt_scheduler::set_realtime_priority(crate::utils::rt_scheduler::RealtimePriority::High) {
        eprintln!("[ARPC] Failed to set real-time priority: {}", e);
    }

    // Stream health monitoring
    let mut last_message_time = std::time::Instant::now();
    let mut message_count = 0u64;
    let mut consecutive_errors = 0u32;
    const MAX_CONSECUTIVE_ERRORS: u32 = 10;
    const STREAM_TIMEOUT_SECONDS: u64 = 60;

    while let Some(result) = stream.message().await? {
        last_message_time = std::time::Instant::now();
        message_count += 1;
        consecutive_errors = 0; // Reset error counter on successful message

        // Check for stale stream
        if last_message_time.elapsed() > Duration::from_secs(STREAM_TIMEOUT_SECONDS) {
            println!("[ARPC] WARNING: Stream appears stale (no messages for {}s), reconnecting...", 
                STREAM_TIMEOUT_SECONDS);
            break; // Exit to trigger reconnection
        }

        // Memory pressure check - force cleanup every 1000 messages
        if message_count % 1000 == 0 {
            // Force garbage collection and cleanup
            std::thread::sleep(Duration::from_millis(1));
            crate::grpc::arpc_parser::cleanup_old_signatures();
            println!("[ARPC] Memory pressure cleanup triggered after {} messages", message_count);
        }

        // Clone with size check to prevent memory leaks
        let result_size = result.transaction.as_ref()
            .map(|tx| tx.account_keys.len() + tx.instructions.len())
            .unwrap_or(0);
        
        if result_size > 1000 {
            println!("[ARPC] WARNING: Large message detected ({} items), skipping to prevent memory leak", result_size);
            continue;
        }

        let result = result.clone();
        let config = config.clone();
        
        // Increment received counter
        ARPC_MESSAGES_RECEIVED.fetch_add(1, Ordering::Relaxed);
        
        let sig_str = result.transaction.as_ref()
            .and_then(|tx| tx.signatures.get(0))
            .map(|sig| bs58::encode(sig).into_string())
            .unwrap_or_else(|| "<no_sig>".to_string());
        
        let now = Utc::now();
        println!("[{}] - [ARPC] Processing message for sig: {}", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"), sig_str);
        
        // CRITICAL FIX: Use worker pool instead of unbounded tokio::spawn
        if let Err(e) = ARPC_WORKER_POOL.submit(result.clone(), config.clone()) {
            eprintln!("[ARPC] Failed to submit task to worker pool: {}", e);
        }
    }

    println!("[ARPC] Stream ended gracefully after {} messages", message_count);
    Ok(())
}

pub async fn subscribe_with_retry(
    endpoint: &str,
    accounts_to_monitor: Vec<String>,
    config: Arc<Config>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        println!("[ARPC] Attempt {} to connect and subscribe...", attempt);
        let result = subscribe_and_print(endpoint, accounts_to_monitor.clone(), config.clone()).await;
        match result {
            Ok(_) => {
                println!("[ARPC] Subscription ended gracefully.");
                break;
            }
            Err(e) => {
                eprintln!("[ARPC] Subscription error: {}. Retrying in 5 seconds...", e);
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
    Ok(())
}
