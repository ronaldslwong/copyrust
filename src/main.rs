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

pub mod arpc {
    include!(concat!(env!("OUT_DIR"), "/arpc.rs"));
}

pub mod geyser {
    include!(concat!(env!("OUT_DIR"), "/geyser.rs"));
}

use crate::grpc::client::subscribe_with_retry;
use crate::init::initialize::initialize;
use crate::triton_grpc::client::subscribe_with_retry_triton;
// use futures::future::join_all;
use std::sync::Arc;
use tokio::task::JoinHandle;

#[tokio::main]
async fn main() {
    let (config, _) = initialize().await;
    let config_arc = Arc::new(config);

    let mut handles: Vec<JoinHandle<()>> = Vec::new();

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
