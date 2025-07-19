//! Global async event logger for fast, non-blocking event tracking and debugging.
//!
//! Usage:
//!   1. Call `setup_event_logger()` once at startup (e.g., in main.rs).
//!   2. Use `log_event(EventType::..., sig, reference_time)` from anywhere in your code.
//!
//! Example:
//!   use crate::utils::logger::{log_event, setup_event_logger, EventType};
//!   let t0 = std::time::Instant::now();
//!   setup_event_logger();
//!   log_event(EventType::Grpc_Detection_Processing, "mysig", t0);

use std::time::Instant;
use tokio::sync::mpsc;
use once_cell::sync::OnceCell;
use chrono::Utc;

#[derive(Debug)]
pub enum EventType {
    GrpcDetectionProcessing,
    ArpcDetectionProcessing,
    GrpcLanded,
    RaydiumLaunchpadBuy,



    // RaydiumBuy,
    RaydiumSell,
    SlotUpdate,
    Custom(String),
}

#[derive(Debug)]
pub struct Event {
    pub event_type: EventType,
    pub sig: Vec<u8>,
    pub reference_time: Instant, // Now tracks the reference point
    pub blocks_to_land: Option<i64>,
}

static EVENT_SENDER: OnceCell<mpsc::Sender<Event>> = OnceCell::new();

pub fn setup_event_logger() {
    let (tx, mut rx) = mpsc::channel::<Event>(1024);
    EVENT_SENDER.set(tx).unwrap();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let elapsed = event.reference_time.elapsed();
            let now = Utc::now();
            let now_str = now.format("%Y-%m-%d %H:%M:%S%.3f");
            match event.event_type {
                EventType::GrpcDetectionProcessing => {
                    let sig_detect = if event.sig.is_empty() {
                        String::new()
                    } else {
                        bs58::encode(&event.sig).into_string()
                    };
                    
                    println!(
                        "[{}] - [grpc] Detection event | elapsed: {:.2?} | sig: {}",
                        now_str, elapsed, sig_detect
                    );
                }
                EventType::GrpcLanded => {
                    let sig_detect = if event.sig.is_empty() {
                        String::new()
                    } else {
                        bs58::encode(&event.sig).into_string()
                    };
                    println!(
                        "[{}] - [grpc] Tranasction landed | sig: {} | blocks to land: {:?} | time to land: {:.2?} | Queueing sell tx, waiting 4 seconds",
                        now_str, sig_detect, event.blocks_to_land.unwrap_or(-1), elapsed
                    );
                }
                EventType::ArpcDetectionProcessing => {
                    let sig_detect = if event.sig.is_empty() {
                        String::new()
                    } else {
                        bs58::encode(&event.sig).into_string()
                    };
                    
                    println!(
                        "[{}] - [arpc] Detection event | elapsed: {:.2?} | sig: {}",
                        now_str, elapsed, sig_detect
                    );
                }






                EventType::RaydiumLaunchpadBuy => {
                    println!(
                        "[{}] - [arpc] Raydium Launchpad buy detected | elapsed: {:.2?} | sig: {}",
                        now_str, elapsed, bs58::encode(&event.sig).into_string()
                    );
                }
                EventType::RaydiumSell => {
                    println!(
                        "[{}] - [arpc] Raydium Launchpad sell detected | elapsed: {:.2?} | sig: {}",
                        now_str, elapsed, bs58::encode(&event.sig).into_string()
                    );
                }
                EventType::SlotUpdate => {
                    println!(
                        "[{}] - [arpc] Slot update | elapsed: {:.2?} | sig: {}",
                        now_str, elapsed, bs58::encode(&event.sig).into_string()
                    );
                }
                EventType::Custom(ref name) => {
                    println!(
                        "[{}] - [arpc] {} | elapsed: {:.2?} | sig: {}",
                        now_str, name, elapsed, bs58::encode(&event.sig).into_string()
                    );
                }
            }
        }
    });
}

pub fn log_event(event_type: EventType, sig: &[u8], reference_time: Instant, blocks_to_land: Option<i64>) {
    if let Some(sender) = EVENT_SENDER.get() {
        let event = Event {
            event_type,
            sig: sig.to_vec(),
            reference_time,
            blocks_to_land,
        };
        let _ = sender.try_send(event);
    }
} 