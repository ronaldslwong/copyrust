use super::arpc_parser::process_grpc_msg;
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

    while let Some(result) = stream.message().await? {
        let result = result.clone();
        let config = config.clone();
        tokio::spawn(async move {
            let now = Utc::now();
            if let Some(trade) = process_grpc_msg(&result, &config).await {
                println!("[{}] - ARPC feed trade: {:?}", now.format("%Y-%m-%d %H:%M:%S%.3f"), trade);
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
