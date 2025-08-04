use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use url::Url;
use futures::{StreamExt, stream::SplitStream};

// Global variable to hold the latest tip data
pub static TIP_DATA: once_cell::sync::Lazy<Arc<Mutex<Option<TipData>>>> = 
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TipData {
    pub time: String,
    pub landed_tips_25th_percentile: f64,
    pub landed_tips_50th_percentile: f64,
    pub landed_tips_75th_percentile: f64,
    pub landed_tips_95th_percentile: f64,
    pub landed_tips_99th_percentile: f64,
}

impl TipData {
    pub fn new() -> Self {
        Self {
            time: String::new(),
            landed_tips_25th_percentile: 0.0,
            landed_tips_50th_percentile: 0.0,
            landed_tips_75th_percentile: 0.0,
            landed_tips_95th_percentile: 0.0,
            landed_tips_99th_percentile: 0.0,
        }
    }
}

pub async fn initialize_tip_stream() -> Result<(), Box<dyn std::error::Error>> {
    let config = crate::config_load::GLOBAL_CONFIG.get().expect("Config not loaded");
    let tip_stream_url = &config.tip_stream;
    
    println!("Initializing tip stream connection to: {}", tip_stream_url);
    
    // Parse the WebSocket URL
    let url = Url::parse(tip_stream_url)?;
    
    // Connect to the WebSocket
    let (ws_stream, _) = connect_async(url).await?;
    println!("WebSocket connection established");
    
    let (write, read) = ws_stream.split();
    
    // Spawn a task to handle incoming messages
    let tip_data_arc = Arc::clone(&TIP_DATA);
    tokio::spawn(async move {
        handle_tip_stream_messages(read, tip_data_arc).await;
    });
    
    Ok(())
}

async fn handle_tip_stream_messages(
    mut read: SplitStream<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
    tip_data_arc: Arc<Mutex<Option<TipData>>>,
) {
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                // Parse the JSON message
                match serde_json::from_str::<Vec<TipData>>(&text) {
                    Ok(tip_data_vec) => {
                        if let Some(latest_tip_data) = tip_data_vec.first() {
                            // Update the global tip data
                            if let Ok(mut guard) = tip_data_arc.lock() {
                                *guard = Some(latest_tip_data.clone());
                                println!("Updated tip data: {:?}", latest_tip_data);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to parse tip data JSON: {}", e);
                    }
                }
            }
            Ok(Message::Close(_)) => {
                println!("WebSocket connection closed");
                break;
            }
            Err(e) => {
                eprintln!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }
}

// Helper function to get the latest tip data
pub fn get_latest_tip_data() -> Option<TipData> {
    if let Ok(guard) = TIP_DATA.lock() {
        guard.clone()
    } else {
        None
    }
}

// Helper function to get a specific percentile tip
pub fn get_tip_percentile(percentile: u8) -> Option<f64> {
    if let Some(tip_data) = get_latest_tip_data() {
        match percentile {
            25 => Some(tip_data.landed_tips_25th_percentile),
            50 => Some(tip_data.landed_tips_50th_percentile),
            75 => Some(tip_data.landed_tips_75th_percentile),
            95 => Some(tip_data.landed_tips_95th_percentile),
            99 => Some(tip_data.landed_tips_99th_percentile),
            _ => None,
        }
    } else {
        None
    }
} 