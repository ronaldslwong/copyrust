pub mod build_tx;
pub mod config_load;
pub mod grpc;
pub mod init;
pub mod proto;
pub mod send_tx;
#[path = "solana_storage_confirmed_block.rs"]
pub mod solana;
pub mod triton_grpc;
pub mod utils;
pub mod constants;

pub mod arpc {
    include!(concat!(env!("OUT_DIR"), "/arpc.rs"));
}

pub mod geyser {
    include!(concat!(env!("OUT_DIR"), "/geyser.rs"));
}

use crate::grpc::client::subscribe_with_retry;
use crate::init::initialize::initialize;
use crate::triton_grpc::client::subscribe_with_retry_triton;
use crate::utils::rt_scheduler::init_realtime_scheduling;
// use futures::future::join_all;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use chrono::Utc;

async fn start_stats_monitoring() {
    let mut interval = interval(Duration::from_secs(60)); // Report every minute
    
    loop {
        interval.tick().await;
        let now = Utc::now();
        
        // Get ARPC stats
        let (arpc_received, arpc_processed, arpc_errors) = crate::grpc::client::get_arpc_stats();
        
        // Get worker stats
        let (worker_received, worker_built, worker_inserted, worker_errors) = crate::grpc::arpc_worker::get_worker_stats();
        
        // Get triton stats
        let (triton_received, triton_sent, triton_found, triton_errors) = crate::triton_grpc::crossbeam_worker::get_triton_stats();
        
        // Get map stats
        let (map_size, map_entries) = crate::grpc::arpc_worker::get_map_stats();
        
        // Get memory usage
        let memory_info = crate::utils::get_memory_usage();
        
        println!("[{}] ========== SYSTEM STATS REPORT ==========", now.format("%Y-%m-%d %H:%M:%S%.3f"));
        println!("[{}] ARPC: Received={}, Processed={}, Errors={}, Rate={:.2}%", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"),
            arpc_received, arpc_processed, arpc_errors,
            if arpc_received > 0 { (arpc_processed as f64 / arpc_received as f64) * 100.0 } else { 0.0 }
        );
        println!("[{}] WORKER: Received={}, Built={}, Inserted={}, Errors={}", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"),
            worker_received, worker_built, worker_inserted, worker_errors
        );
        println!("[{}] TRITON: Received={}, Sent={}, Found={}, Errors={}", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"),
            triton_received, triton_sent, triton_found, triton_errors
        );
        println!("[{}] MAP: Size={}, Entries: {}", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"),
            map_size,
            map_entries.iter().map(|(tx_type, age)| format!("{}:{:.2?}", tx_type, age)).collect::<Vec<_>>().join(", ")
        );
        
        if let Some((rss, vm_size)) = memory_info {
            println!("[{}] MEMORY: RSS={}, Virtual={}", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"),
                crate::utils::format_bytes(rss),
                crate::utils::format_bytes(vm_size)
            );
        }
        
        // Calculate processing efficiency
        let total_processed = arpc_processed + worker_built + triton_sent;
        let total_errors = arpc_errors + worker_errors + triton_errors;
        let total_activity = arpc_received + worker_received + triton_received;
        
        println!("[{}] EFFICIENCY: Total Activity={}, Total Processed={}, Total Errors={}, Success Rate={:.2}%", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"),
            total_activity, total_processed, total_errors,
            if total_activity > 0 { (total_processed as f64 / total_activity as f64) * 100.0 } else { 0.0 }
        );
        
        // Memory leak detection and cleanup
        if map_size > 100 {
            println!("[{}] WARNING: Large map size detected ({}) - potential memory leak!", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"), map_size);
            
            // Trigger debug cleanup
            crate::grpc::arpc_worker::debug_and_cleanup();
        }
        
        // Check for processing bottlenecks
        if worker_received > 0 && worker_built < worker_received / 2 {
            println!("[{}] WARNING: Worker processing bottleneck detected! Received: {}, Built: {}", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"), worker_received, worker_built);
        }
        
        if triton_found > 0 && triton_sent < triton_found / 2 {
            println!("[{}] WARNING: Triton sending bottleneck detected! Found: {}, Sent: {}", 
                now.format("%Y-%m-%d %H:%M:%S%.3f"), triton_found, triton_sent);
        }
        
        println!("[{}] ================================================", now.format("%Y-%m-%d %H:%M:%S%.3f"));
    }
}

#[tokio::main]
async fn main() {
    // Initialize real-time scheduling early
    if let Err(e) = init_realtime_scheduling() {
        eprintln!("[Main] Real-time scheduling initialization failed: {}", e);
        eprintln!("[Main] Continuing without real-time scheduling...");
    }
    
    let (config, _) = initialize().await;
    let config_arc = Arc::new(config);

    let mut handles: Vec<JoinHandle<()>> = Vec::new();

    // Start stats monitoring
    let stats_handle = tokio::spawn(start_stats_monitoring());
    handles.push(stats_handle);

    let triton_config = Arc::clone(&config_arc);
    let handle = tokio::spawn(async move {
        let endpoint = triton_config.grpc_endpoint.clone();
        println!("[Main] Starting Triton gRPC client...");
        if let Err(e) = subscribe_with_retry_triton(&endpoint, triton_config).await {
            eprintln!("[Main] Triton gRPC client error: {}", e);
        }
    });
    handles.push(handle);

    let arpc_config = Arc::clone(&config_arc);
    let handle = tokio::spawn(async move {
        let endpoint = arpc_config.arpc_endpoint.clone();
        let accounts_to_monitor = arpc_config.accounts_monitor.clone();
        println!("[Main] Starting ARPC client...");
        if let Err(e) = subscribe_with_retry(&endpoint, accounts_to_monitor, arpc_config).await {
            eprintln!("[Main] ARPC client error: {}", e);
        }
    });
    handles.push(handle);

    println!("[Main] Waiting for all clients to complete...");
    for handle in handles {
        handle.await.unwrap();
    }
    println!("[Main] All clients have completed.");
}
