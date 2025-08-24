use super::arpc_parser::process_arpc_msg;
use crate::arpc::{
    arpc_service_client::ArpcServiceClient, SubscribeRequest as ArpcSubscribeRequest,
    SubscribeRequestFilterTransactions,
};
use crate::config_load::Config;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio::time::{sleep, Duration};
use chrono::Utc;
use core_affinity;

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

pub async fn subscribe_and_print(
    endpoint: &str,
    accounts_to_monitor: Vec<String>,
    config: Arc<Config>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut client = ArpcServiceClient::connect(endpoint.to_string()).await?;
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

    let mut stream = client.subscribe(request_stream).await?.into_inner();

    println!("ARPC subscription established. Waiting for messages...");

    // Start a periodic stats reporting task
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

    while let Some(result) = stream.message().await? {
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
        
        tokio::spawn(async move {
            let processing_start = std::time::Instant::now();
            
            match process_arpc_msg(&result, &config).await {
                Some(trade) => {
                    ARPC_MESSAGES_PROCESSED.fetch_add(1, Ordering::Relaxed);
                    let processing_time = processing_start.elapsed();
                    let now = Utc::now();
                    println!("[{}] - [ARPC] SUCCESS - Trade detected: {:?} (processing time: {:.2?})", 
                        now.format("%Y-%m-%d %H:%M:%S%.3f"), trade, processing_time);
                }
                None => {
                    let processing_time = processing_start.elapsed();
                    let now = Utc::now();
                    println!("[{}] - [ARPC] No trade detected (processing time: {:.2?})", 
                        now.format("%Y-%m-%d %H:%M:%S%.3f"), processing_time);
                }
            }
        });
    }

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
