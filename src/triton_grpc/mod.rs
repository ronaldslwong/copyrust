pub mod client;
pub mod parser;
pub mod crossbeam_worker;

// OPTIMIZATION: Re-export main functions for easier access
pub use client::{setup_multiple_triton_feeds, subscribe_with_retry_triton};
pub use parser::{process_triton_message, process_triton_message_legacy};
pub use crossbeam_worker::{setup_crossbeam_worker, send_parsed_tx, is_signature_processed_by_feed};