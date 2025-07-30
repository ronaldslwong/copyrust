use crate::init::wallet_loader::{get_wallet_keypair, get_nonce_account};
use base64::{engine::general_purpose, Engine as _};
use once_cell::sync::OnceCell;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use std::str::FromStr;
use std::sync::Arc;
use tonic::codegen::InterceptedService;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, ClientTlsConfig};
use tonic::{Request, Status};
use solana_sdk::signer::keypair::Keypair;
use rand::Rng;
use solana_sdk::compute_budget;
use std::time::Instant;
use chrono::Utc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::VecDeque;
use std::sync::Mutex;

// BlockRazor proto definitions
pub mod blockrazor_proto {
    tonic::include_proto!("serverpb");
}

static BLOCKRAZOR_TIP_ACCOUNTS: &[&str] = &[
    "FjmZZrFvhnqqb9ThCuMVnENaM3JGVuGWNyCAxRJcFpg9",
    "6No2i3aawzHsjtThw81iq1EXPJN6rh8eSJCLaYZfKDTG",
    "A9cWowVAiHe9pJfKAj3TJiN9VpbzMUq6E4kEvf5mUT22",
    "Gywj98ophM7GmkDdaWs4isqZnDdFCW7B46TXmKfvyqSm",
    "68Pwb4jS7eZATjDfhmTXgRJjCiZmw1L7Huy4HNpnxJ3o",
    "4ABhJh5rZPjv63RBJBuyWzBK3g9gWMUQdTZP2kiW31V9",
    "B2M4NG5eyZp5SBQrSdtemzk5TqVuaWGQnowGaCBt8GyM",
    "5jA59cXMKQqZAVdtopv8q3yyw9SYfiE3vUCbt7p8MfVf",
    "5YktoWygr1Bp9wiS1xtMtUki1PeYuuzuCF98tqwYxf61",
    "295Avbam4qGShBYK7E9H5Ldew4B3WyJGmgmXfiWdeeyV",
    "EDi4rSy2LZgKJX74mbLTFk4mxoTgT6F7HxxzG2HBAFyK",
    "BnGKHAC386n4Qmv9xtpBVbRaUTKixjBe3oagkPFKtoy6",
    "Dd7K2Fp7AtoN8xCghKDRmyqr5U169t48Tw5fEd3wT9mq",
    "AP6qExwrbRgBAVaehg4b5xHENX815sMabtBzUzVB4v8S",
];

#[derive(Clone)]
pub struct ApiTokenInterceptor {
    pub token: String,
}

impl tonic::service::Interceptor for ApiTokenInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        let meta = MetadataValue::from_str(&self.token).unwrap();
        req.metadata_mut().insert("apikey", meta);
        Ok(req)
    }
}

// Global BlockRazor gRPC client (shared, persistent)
pub static BLOCKRAZOR_CLIENT: OnceCell<
    Arc<blockrazor_proto::server_client::ServerClient<InterceptedService<Channel, ApiTokenInterceptor>>>,
> = OnceCell::new();

/// Initialize the global BlockRazor gRPC client. Call this ONCE at startup.
pub async fn initialize_blockrazor_client(address: &str, token: &str, plaintext: bool) {
    let channel = connect_to_blockrazor(address, token, plaintext)
        .await
        .expect("Failed to connect to BlockRazor");
    let client = blockrazor_proto::server_client::ServerClient::with_interceptor(
        channel,
        ApiTokenInterceptor {
            token: token.to_string(),
        },
    );
    BLOCKRAZOR_CLIENT
        .set(Arc::new(client))
        .expect("BlockRazor client already set");
}

/// Get a clone of the global BlockRazor client (Arc)
pub fn get_blockrazor_client(
) -> Arc<blockrazor_proto::server_client::ServerClient<InterceptedService<Channel, ApiTokenInterceptor>>> {
    BLOCKRAZOR_CLIENT
        .get()
        .expect("BlockRazor client not initialized")
        .clone()
}

/// Get a reference to the global BlockRazor client (for better performance)
pub fn get_blockrazor_client_ref(
) -> &'static blockrazor_proto::server_client::ServerClient<InterceptedService<Channel, ApiTokenInterceptor>> {
    BLOCKRAZOR_CLIENT
        .get()
        .expect("BlockRazor client not initialized")
}

pub async fn connect_to_blockrazor(
    address: &str,
    _token: &str,
    plaintext: bool,
) -> Result<Channel, Box<dyn std::error::Error>> {
    let channel = if plaintext {
        Channel::from_shared(address.to_string())?
            .connect_timeout(std::time::Duration::from_secs(2)) // Reduced from 5s
            .timeout(std::time::Duration::from_secs(5)) // Reduced from 10s
            .tcp_keepalive(Some(std::time::Duration::from_secs(10))) // Reduced from 30s
            .keep_alive_while_idle(true)
            .initial_stream_window_size(1024 * 1024) // 1MB initial window
            .initial_connection_window_size(1024 * 1024) // 1MB connection window
            .connect_lazy() // Use lazy connection for persistence
    } else {
        Channel::from_shared(address.to_string())?
            .tls_config(ClientTlsConfig::new())?
            .connect_timeout(std::time::Duration::from_secs(2)) // Reduced from 5s
            .timeout(std::time::Duration::from_secs(5)) // Reduced from 10s
            .tcp_keepalive(Some(std::time::Duration::from_secs(10))) // Reduced from 30s
            .keep_alive_while_idle(true)
            .initial_stream_window_size(1024 * 1024) // 1MB initial window
            .initial_connection_window_size(1024 * 1024) // 1MB connection window
            .connect_lazy() // Use lazy connection for persistence
    };
    Ok(channel)
}

/// Send a signed Solana transaction via BlockRazor gRPC with detailed profiling
pub async fn send_tx_blockrazor(
    tx: &Transaction,
    token: &str,
    mode: &str,
    safe_window: Option<i32>,
    revert_protection: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let total_start = Instant::now();
    let now = Utc::now();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_PROFILE] 🚀 Starting BlockRazor send transaction", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"));
    
    // Step 1: Get client (measure client acquisition time)
    let client_start = Instant::now();
    let client = get_blockrazor_client();
    let mut client = Arc::try_unwrap(client).unwrap_or_else(|arc| (*arc).clone());
    let client_time = client_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_PROFILE] 📡 Client acquisition: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), client_time);
    
    // Step 2: Serialize transaction (measure serialization time)
    let serialize_start = Instant::now();
    let tx_bytes = bincode::serialize(tx)?;
    let serialize_time = serialize_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_PROFILE] 📦 Transaction serialization: {:.2?} (size: {} bytes)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), serialize_time, tx_bytes.len());
    
    // Step 3: Base64 encoding (measure encoding time)
    let encode_start = Instant::now();
    let tx_b64 = general_purpose::STANDARD.encode(&tx_bytes);
    let encode_time = encode_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_PROFILE] 🔤 Base64 encoding: {:.2?} (encoded size: {} chars)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), encode_time, tx_b64.len());
    
    // Step 4: Build request (measure request building time)
    let request_start = Instant::now();
    let request = blockrazor_proto::SendRequest {
        transaction: tx_b64,
        mode: mode.to_string(),
        safe_window,
        revert_protection,
    };
    let request_time = request_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_PROFILE] 📝 Request building: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), request_time);
    
    // Step 5: Create gRPC request with metadata (measure request creation time)
    let grpc_request_start = Instant::now();
    let mut req = tonic::Request::new(request);
    req.metadata_mut()
        .insert("apikey", MetadataValue::from_str(token)?);
    let grpc_request_time = grpc_request_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_PROFILE] 🔗 gRPC request creation: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), grpc_request_time);
    
    // Step 6: Network call (measure network latency)
    let network_start = Instant::now();
    let response = client.send_transaction(req).await?;
    let network_time = network_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_PROFILE] 🌐 Network call: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), network_time);
    
    // Step 7: Extract response (measure response processing time)
    let response_start = Instant::now();
    let signature = response.into_inner().signature;
    let response_time = response_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_PROFILE] 📨 Response processing: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), response_time);
    
    // Calculate total time and breakdown
    let total_time = total_start.elapsed();
    let processing_time = total_time - network_time; // Time spent in our code
    
    #[cfg(feature = "verbose_logging")]
    {
        println!("[{}] - [BLOCKRAZOR_PROFILE] 📊 PERFORMANCE BREAKDOWN:", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
        println!("[{}] - [BLOCKRAZOR_PROFILE]   • Client acquisition: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), client_time, 
            (client_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_PROFILE]   • Serialization: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), serialize_time,
            (serialize_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_PROFILE]   • Base64 encoding: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), encode_time,
            (encode_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_PROFILE]   • Request building: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), request_time,
            (request_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_PROFILE]   • gRPC request creation: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), grpc_request_time,
            (grpc_request_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_PROFILE]   • Network call: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), network_time,
            (network_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_PROFILE]   • Response processing: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), response_time,
            (response_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_PROFILE]   • Processing overhead: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), processing_time,
            (processing_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_PROFILE]   • TOTAL TIME: {:.2?}", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), total_time);
        
        // Identify bottleneck
        let times = vec![
            ("Client acquisition", client_time),
            ("Serialization", serialize_time),
            ("Base64 encoding", encode_time),
            ("Request building", request_time),
            ("gRPC request creation", grpc_request_time),
            ("Network call", network_time),
            ("Response processing", response_time),
        ];
        
        let bottleneck = times.iter().max_by_key(|(_, time)| time.as_micros()).unwrap();
        println!("[{}] - [BLOCKRAZOR_PROFILE] 🎯 BOTTLENECK: {} ({:.2?})", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), bottleneck.0, bottleneck.1);
        
        println!("[{}] - [BLOCKRAZOR_PROFILE] ✅ SUCCESS - Signature: {}", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), signature);
    }
    
    // Record performance metrics
    record_blockrazor_performance(true, network_time.as_micros() as u64);
    
    Ok(signature)
}

/// Get health status from BlockRazor
pub async fn get_blockrazor_health(
    token: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let client = get_blockrazor_client();
    let mut client = Arc::try_unwrap(client).unwrap_or_else(|arc| (*arc).clone());

    let request = blockrazor_proto::HealthRequest {};

    let mut req = tonic::Request::new(request);
    req.metadata_mut()
        .insert("apikey", MetadataValue::from_str(token)?);

    let response = client.get_health(req).await?;
    Ok(response.into_inner().status)
}

/// Build compute budget instructions for BlockRazor with detailed profiling
pub fn create_instruction_blockrazor(
    instructions: Vec<Instruction>,
    tip: u64,
    cu_price: u64,
    nonce_account: &Pubkey,
) -> Vec<Instruction> {
    let total_start = Instant::now();
    let now = Utc::now();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] 🔧 Starting BlockRazor instruction building", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"));
    
    // Step 1: Random number generation (measure RNG time)
    let rng_start = Instant::now();
    let mut rng = rand::thread_rng();
    let random_addition: u64 = rng.gen_range(1..=100);
    let adjusted_cu_price = cu_price + random_addition;
    let rng_time = rng_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] 🎲 RNG generation: {:.2?} (price: {} -> {})", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), rng_time, cu_price, adjusted_cu_price);
    
    // Step 2: Get wallet keypair (measure keypair access time)
    let keypair_start = Instant::now();
    let keypair: &'static Keypair = get_wallet_keypair();
    let keypair_time = keypair_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] 🔑 Keypair access: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), keypair_time);
    
    // Step 3: Create compute budget instruction (measure instruction creation time)
    let compute_start = Instant::now();
    let price_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(adjusted_cu_price);
    let compute_time = compute_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] 💰 Compute budget instruction: {:.2?}", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), compute_time);
    
    // Step 4: Select tip account (measure tip account selection time)
    let tip_select_start = Instant::now();
    let random_index = rng.gen_range(0..BLOCKRAZOR_TIP_ACCOUNTS.len());
    let tip_account = BLOCKRAZOR_TIP_ACCOUNTS[random_index];
    let tip_select_time = tip_select_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] 🎯 Tip account selection: {:.2?} (account: {})", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), tip_select_time, tip_account);
    
    // Step 5: Create tip instruction (measure tip instruction creation time)
    let tip_ix_start = Instant::now();
    let tip_ix = blockrazor_tip(tip_account, tip, &keypair.pubkey());
    let tip_ix_time = tip_ix_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] 💸 Tip instruction creation: {:.2?} (tip: {} lamports)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), tip_ix_time, tip);
    
    // Step 6: Create nonce instruction (measure nonce instruction creation time)
    let nonce_start = Instant::now();
    let advance_nonce_ix = system_instruction::advance_nonce_account(
        nonce_account,
        &keypair.pubkey(),
    );
    let nonce_time = nonce_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] 🔄 Nonce instruction creation: {:.2?} (nonce: {})", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), nonce_time, nonce_account);
    
    // Step 7: Combine all instructions (measure instruction combination time)
    let combine_start = Instant::now();
    let mut result = vec![advance_nonce_ix, tip_ix, price_ix];
    result.extend(instructions);
    let combine_time = combine_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] 🔗 Instruction combination: {:.2?} (total instructions: {})", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), combine_time, result.len());
    
    // Calculate total time and breakdown
    let total_time = total_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    {
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] 📊 INSTRUCTION BUILDING BREAKDOWN:", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE]   • RNG generation: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), rng_time,
            (rng_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE]   • Keypair access: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), keypair_time,
            (keypair_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE]   • Compute budget instruction: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), compute_time,
            (compute_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE]   • Tip account selection: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), tip_select_time,
            (tip_select_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE]   • Tip instruction creation: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), tip_ix_time,
            (tip_ix_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE]   • Nonce instruction creation: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), nonce_time,
            (nonce_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE]   • Instruction combination: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), combine_time,
            (combine_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE]   • TOTAL TIME: {:.2?}", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), total_time);
        
        // Identify bottleneck
        let times = vec![
            ("RNG generation", rng_time),
            ("Keypair access", keypair_time),
            ("Compute budget instruction", compute_time),
            ("Tip account selection", tip_select_time),
            ("Tip instruction creation", tip_ix_time),
            ("Nonce instruction creation", nonce_time),
            ("Instruction combination", combine_time),
        ];
        
        let bottleneck = times.iter().max_by_key(|(_, time)| time.as_micros()).unwrap();
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] 🎯 BOTTLENECK: {} ({:.2?})", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), bottleneck.0, bottleneck.1);
        
        println!("[{}] - [BLOCKRAZOR_INSTRUCTION_PROFILE] ✅ SUCCESS - Built {} instructions", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), result.len());
    }
    
    result
}

/// Create a system transfer instruction for BlockRazor tips with profiling
pub fn blockrazor_tip(tip_ac: &str, tip: u64, from_pubkey: &Pubkey) -> Instruction {
    let total_start = Instant::now();
    let now = Utc::now();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_TIP_PROFILE] 💸 Starting BlockRazor tip instruction creation", 
        now.format("%Y-%m-%d %H:%M:%S%.3f"));
    
    // Step 1: Parse tip account pubkey (measure parsing time)
    let parse_start = Instant::now();
    let tip_pubkey = Pubkey::from_str(tip_ac).expect("Invalid pubkey");
    let parse_time = parse_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_TIP_PROFILE] 🔍 Tip account parsing: {:.2?} (account: {})", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), parse_time, tip_ac);
    
    // Step 2: Create system transfer instruction (measure instruction creation time)
    let transfer_start = Instant::now();
    let instruction = system_instruction::transfer(from_pubkey, &tip_pubkey, tip);
    let transfer_time = transfer_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    println!("[{}] - [BLOCKRAZOR_TIP_PROFILE] 💰 Transfer instruction creation: {:.2?} (tip: {} lamports)", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), transfer_time, tip);
    
    // Calculate total time and breakdown
    let total_time = total_start.elapsed();
    
    #[cfg(feature = "verbose_logging")]
    {
        println!("[{}] - [BLOCKRAZOR_TIP_PROFILE] 📊 TIP INSTRUCTION BREAKDOWN:", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
        println!("[{}] - [BLOCKRAZOR_TIP_PROFILE]   • Tip account parsing: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), parse_time,
            (parse_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_TIP_PROFILE]   • Transfer instruction creation: {:.2?} ({:.1}%)", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), transfer_time,
            (transfer_time.as_micros() as f64 / total_time.as_micros() as f64) * 100.0);
        println!("[{}] - [BLOCKRAZOR_TIP_PROFILE]   • TOTAL TIME: {:.2?}", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), total_time);
        
        // Identify bottleneck
        let times = vec![
            ("Tip account parsing", parse_time),
            ("Transfer instruction creation", transfer_time),
        ];
        
        let bottleneck = times.iter().max_by_key(|(_, time)| time.as_micros()).unwrap();
        println!("[{}] - [BLOCKRAZOR_TIP_PROFILE] 🎯 BOTTLENECK: {} ({:.2?})", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"), bottleneck.0, bottleneck.1);
        
        println!("[{}] - [BLOCKRAZOR_TIP_PROFILE] ✅ SUCCESS - Tip instruction created", 
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
    }
    
    instruction
}

// Performance monitoring
static BLOCKRAZOR_TOTAL_CALLS: AtomicU64 = AtomicU64::new(0);
static BLOCKRAZOR_SUCCESSFUL_CALLS: AtomicU64 = AtomicU64::new(0);
static BLOCKRAZOR_FAILED_CALLS: AtomicU64 = AtomicU64::new(0);
static BLOCKRAZOR_LATENCY_HISTORY: OnceCell<Mutex<VecDeque<u64>>> = OnceCell::new();

/// Get BlockRazor performance statistics
pub fn get_blockrazor_stats() -> (u64, u64, u64, f64) {
    let total = BLOCKRAZOR_TOTAL_CALLS.load(Ordering::Relaxed);
    let successful = BLOCKRAZOR_SUCCESSFUL_CALLS.load(Ordering::Relaxed);
    let failed = BLOCKRAZOR_FAILED_CALLS.load(Ordering::Relaxed);
    
    let avg_latency = if let Some(history) = BLOCKRAZOR_LATENCY_HISTORY.get() {
        if let Ok(guard) = history.lock() {
            if !guard.is_empty() {
                let sum: u64 = guard.iter().sum();
                sum as f64 / guard.len() as f64
            } else {
                0.0
            }
        } else {
            0.0
        }
    } else {
        0.0
    };
    
    (total, successful, failed, avg_latency)
}

/// Record BlockRazor performance metrics
fn record_blockrazor_performance(success: bool, latency_micros: u64) {
    BLOCKRAZOR_TOTAL_CALLS.fetch_add(1, Ordering::Relaxed);
    
    if success {
        BLOCKRAZOR_SUCCESSFUL_CALLS.fetch_add(1, Ordering::Relaxed);
        
        // Record latency for successful calls
        if let Some(history) = BLOCKRAZOR_LATENCY_HISTORY.get() {
            if let Ok(mut guard) = history.lock() {
                guard.push_back(latency_micros);
                
                // Keep only last 1000 measurements
                if guard.len() > 1000 {
                    guard.pop_front();
                }
            }
        }
    } else {
        BLOCKRAZOR_FAILED_CALLS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Initialize performance monitoring
pub fn init_blockrazor_performance_monitoring() {
    let _ = BLOCKRAZOR_LATENCY_HISTORY.set(Mutex::new(VecDeque::new()));
    println!("[{}] - [BLOCKRAZOR] Performance monitoring initialized", 
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
}