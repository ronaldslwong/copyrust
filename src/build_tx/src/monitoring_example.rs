// Example of how to integrate the monitoring system into your main.rs
// This shows how to run the monitoring subscription alongside your trading pipes

use crate::grpc::monitoring_client::{
    start_arpc_monitoring_with_retry,
    get_monitoring_stats
};
use crate::constants::monitoring::{
    MONITORING_PROGRAMS, 
    get_monitoring_arpc_endpoint,
    MONITORING_FALLBACK_ENDPOINT
};
use crate::config_load::Config;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use chrono::Utc;

/// Example function showing how to start the monitoring system with ARPC
pub async fn start_arpc_monitoring_system(config: Arc<Config>) -> JoinHandle<()> {
    let monitoring_config = Arc::clone(&config);
    
    // Convert program IDs to Vec<String> for the monitoring client
    let programs_to_monitor: Vec<String> = MONITORING_PROGRAMS
        .iter()
        .map(|&s| s.to_string())
        .collect();
    
    println!("[Monitoring ARPC] Starting DEX activity monitoring for {} programs...", programs_to_monitor.len());
    
    let handle = tokio::spawn(async move {
        if let Err(e) = start_arpc_monitoring_with_retry(
            &get_monitoring_arpc_endpoint(), 
            programs_to_monitor, 
            monitoring_config
        ).await {
            eprintln!("[Monitoring ARPC] Monitoring system error: {}", e);
        }
    });
    
    handle
}



/// Example function showing how to query the monitoring data
pub async fn query_monitoring_data() {
    // Get monitoring statistics
    let (received, logged, errors) = get_monitoring_stats();
    println!("[Monitoring Query] Stats - Received: {}, Logged: {}, Errors: {}", 
        received, logged, errors);
    
    // REMOVED: DEX logs queries (performance optimization)
    // These functions were causing significant overhead due to:
    // - Expensive map iterations
    // - Memory allocation for result vectors
    // - String comparisons and filtering operations
}

/// Example of how to modify your main.rs to include monitoring
pub async fn example_main_with_monitoring() {
    // Your existing initialization code here...
    // let (config, _) = initialize().await;
    // let config_arc = Arc::new(config);
    
    // Example config for demonstration
    let config_arc = Arc::new(crate::config_load::load_config());
    
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    
    // Start your existing trading systems
    // handles.push(start_triton_system(config_arc.clone()));
    // handles.push(start_arpc_system(config_arc.clone()));
    
    // Start the monitoring system (separate from trading)
    let monitoring_handle = start_arpc_monitoring_system(config_arc.clone()).await;
    handles.push(monitoring_handle);
    
    // Start a periodic query task to demonstrate data access
    let query_config = Arc::clone(&config_arc);
    let query_handle = tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(120)); // Query every 2 minutes
        loop {
            interval.tick().await;
            query_monitoring_data().await;
        }
    });
    handles.push(query_handle);
    
    // Wait for all systems
    for handle in handles {
        handle.await.unwrap();
    }
}

/// Example of how to integrate monitoring stats into your existing stats monitoring
pub async fn enhanced_stats_monitoring() {
    let mut interval = interval(Duration::from_secs(60));
    
    loop {
        interval.tick().await;
        let now = Utc::now();
        
        // Your existing stats...
        // let (arpc_received, arpc_processed, arpc_errors) = crate::grpc::client::get_arpc_stats();
        // let (worker_received, worker_built, worker_inserted, worker_errors) = crate::grpc::arpc_worker::get_worker_stats();
        
        // Add monitoring stats
        let (monitoring_received, monitoring_logged, monitoring_errors) = get_monitoring_stats();
        // REMOVED: DEX logs count (performance optimization)
        let monitoring_logs_count = 0;
        
        println!("[{}] ========== ENHANCED SYSTEM STATS REPORT ==========", now.format("%Y-%m-%d %H:%M:%S%.3f"));
        
        // Your existing stats output...
        
        // Add monitoring stats
        println!("[{}] MONITORING: Received={}, Logged={}, Errors={}, Active Logs={}, Rate={:.2}%", 
            now.format("%Y-%m-%d %H:%M:%S%.3f"),
            monitoring_received, monitoring_logged, monitoring_errors, monitoring_logs_count,
            if monitoring_received > 0 { (monitoring_logged as f64 / monitoring_received as f64) * 100.0 } else { 0.0 }
        );
        
        // REMOVED: Program-specific activity queries (performance optimization)
        // These functions were causing significant overhead due to:
        // - Expensive map iterations
        // - Memory allocation for result vectors
        // - String comparisons and filtering operations
        
        println!("[{}] ==================================================", now.format("%Y-%m-%d %H:%M:%S%.3f"));
    }
} 