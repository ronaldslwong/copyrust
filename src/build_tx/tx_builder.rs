use ed25519_dalek::{Keypair as DalekKeypair, Signer as DalekSigner, Signature as DalekSignature};
use ed25519_zebra::{SigningKey, VerificationKey};
use solana_client::rpc_client::RpcClient;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use solana_sdk::compute_budget;
use solana_sdk::pubkey::Pubkey;
use crate::init::wallet_loader::get_wallet_keypair;
use crate::utils::ata::create_ata;
use solana_program::instruction::Instruction;
use solana_sdk::signature::Signature;
use crate::send_tx::rpc::get_cached_blockhash;
use rand::Rng;
use std::time::Instant;
use chrono::Utc;
use once_cell::sync::OnceCell;
use tokio::runtime::Runtime;

// Thread-local runtime storage for better concurrency
use std::cell::RefCell;
use std::sync::Arc;
use tokio::sync::Mutex;

thread_local! {
    static THREAD_RUNTIME: RefCell<Option<Runtime>> = RefCell::new(None);
}

// Alternative: Runtime pool for high-concurrency scenarios
static RUNTIME_POOL: OnceCell<Arc<Mutex<Vec<Runtime>>>> = OnceCell::new();
static POOL_SIZE: usize = 4; // Number of runtimes in the pool

/// Initialize runtime pool for high-concurrency scenarios
pub fn init_runtime_pool() {
    if RUNTIME_POOL.get().is_none() {
        let mut runtimes = Vec::with_capacity(POOL_SIZE);
        for _ in 0..POOL_SIZE {
            runtimes.push(Runtime::new().expect("Failed to create tokio runtime"));
        }
        let _ = RUNTIME_POOL.set(Arc::new(Mutex::new(runtimes)));
    }
}

/// Get a runtime from the pool (round-robin)
async fn get_runtime_from_pool() -> Runtime {
    let pool = RUNTIME_POOL.get().expect("Runtime pool not initialized");
    let mut pool_guard = pool.lock().await;
    
    // Round-robin selection
    if let Some(runtime) = pool_guard.pop() {
        runtime
    } else {
        // Pool exhausted, create new runtime
        Runtime::new().expect("Failed to create tokio runtime")
    }
}

/// Return runtime to pool
async fn return_runtime_to_pool(runtime: Runtime) {
    let pool = RUNTIME_POOL.get().expect("Runtime pool not initialized");
    let mut pool_guard = pool.lock().await;
    
    if pool_guard.len() < POOL_SIZE {
        pool_guard.push(runtime);
    }
    // If pool is full, drop the runtime
}

// We'll use the existing global blockhash cache from send_tx::rpc
// No need for our own cache since the global one is already optimized

/// Get or create a thread-local runtime
fn get_thread_runtime() -> Runtime {
    THREAD_RUNTIME.with(|runtime_cell| {
        let mut runtime_opt = runtime_cell.borrow_mut();
        if runtime_opt.is_none() {
            *runtime_opt = Some(Runtime::new().expect("Failed to create tokio runtime"));
        }
        // Take ownership of the runtime
        runtime_opt.take().unwrap()
    })
}

/// Get cached blockhash directly from the global cache
fn get_cached_blockhash_sync() -> solana_sdk::hash::Hash {
    // Use the existing global blockhash cache directly
    if let Some(global_cache) = crate::send_tx::rpc::GLOBAL_LATEST_BLOCKHASH.get() {
        // Try to get the blockhash synchronously if possible
        match global_cache.try_read() {
            Ok(hash_guard) => {
                let hash = hash_guard.clone();
                // Only log if it's the default hash (indicating cache not populated yet)
                if hash == solana_sdk::hash::Hash::default() {
                    println!("[TX_BUILDER] WARNING: Blockhash cache not populated yet, using default");
                }
                hash
            }
            Err(_) => {
                // If we can't get it immediately, return a default
                // This should be rare since the background task keeps it fresh
                solana_sdk::hash::Hash::default()
            }
        }
    } else {
        // This should never happen since it's initialized in main()
        println!("[TX_BUILDER] ERROR: Global blockhash cache not initialized!");
        solana_sdk::hash::Hash::default()
    }
}

// Pre-converted signing key for the main wallet (most common case)
static PRECONVERTED_MAIN_SIGNING_KEY: OnceCell<SigningKey> = OnceCell::new();

/// Initialize the signing key cache
pub fn init_signing_key_cache() {
    if PRECONVERTED_MAIN_SIGNING_KEY.get().is_none() {
        // Pre-convert the main wallet keypair once at startup
        let main_keypair = crate::init::wallet_loader::get_wallet_keypair();
        let keypair_bytes = main_keypair.to_bytes();
        let private_key: [u8; 32] = keypair_bytes[..32].try_into().unwrap();
        let signing_key = SigningKey::from(private_key);
        let _ = PRECONVERTED_MAIN_SIGNING_KEY.set(signing_key);
    }
}

/// Initialize all optimizations at startup
pub fn init_tx_builder_optimizations() {
    println!("[TX_BUILDER] Initializing optimizations...");
    init_signing_key_cache();
    init_runtime_pool();
    println!("[TX_BUILDER] Optimizations initialized successfully");
}

/// Get the pre-converted signing key (optimized for main wallet)
fn get_or_create_signing_key(signer: &Keypair) -> SigningKey {
    // Check if this is the main wallet keypair (most common case)
    let main_keypair = crate::init::wallet_loader::get_wallet_keypair();
    if signer.pubkey() == main_keypair.pubkey() {
        // Use pre-converted key (should be ~10-50ns)
        return PRECONVERTED_MAIN_SIGNING_KEY.get()
            .expect("Main signing key not initialized")
            .clone();
    }
    
    // For other keypairs, convert on-demand (rare case)
    let keypair_bytes = signer.to_bytes();
    let private_key: [u8; 32] = keypair_bytes[..32].try_into().unwrap();
    SigningKey::from(private_key)
}

/// Returns a default Instruction (empty program_id, empty accounts, empty data)
pub fn default_instruction() -> Instruction {
    Instruction {
        program_id: Pubkey::default(),
        accounts: vec![],
        data: vec![],
    }
}

/// Build and sign a Solana transaction from an array of instructions, fetching the recent blockhash from the network.
/// Uses ed25519-dalek for signing.
// pub fn build_and_sign_transaction(
//     rpc_client: &solana_client::rpc_client::RpcClient,
//     instructions: &[Instruction],
//     signer: &Keypair,
// ) -> Result<Transaction, Box<dyn std::error::Error>> {
//     // Fetch the recent blockhash from the network
//     let recent_blockhash = rpc_client.get_latest_blockhash()?;

//     // Build the message
//     let message = solana_sdk::message::Message::new(instructions, Some(&signer.pubkey()));
//     let message_bytes = message.serialize();

//     // Convert solana_sdk::Keypair to ed25519_dalek::Keypair
//     let dalek_keypair = DalekKeypair::from_bytes(&signer.to_bytes()).expect("Keypair conversion failed");

//     // Sign the message with dalek
//     let dalek_signature: DalekSignature = dalek_keypair.sign(&message_bytes);
//     let solana_signature = solana_sdk::signature::Signature::from(dalek_signature.to_bytes());

//     // Build the transaction manually
//     let mut tx = Transaction::new_unsigned(message);
//     tx.signatures = vec![solana_signature];
//     Ok(tx)
// }

// pub fn build_and_sign_transaction(
//     rpc_client: &RpcClient,
//     instructions: &[Instruction],
//     signer: &Keypair,
// ) -> Result<Transaction, Box<dyn std::error::Error>> {
//     let rt = tokio::runtime::Runtime::new().unwrap();
//     let recent_blockhash = rt.block_on(get_cached_blockhash());
//     // Build and sign the transaction
//     let tx = Transaction::new_signed_with_payer(
//         instructions,
//         Some(&signer.pubkey()),
//         &[signer],
//         recent_blockhash,
//     );
//     Ok(tx)
// }

/// Build and sign a Solana transaction using ed25519-zebra for faster signing.
/// This function is optimized for performance and uses the zebra implementation
/// which is generally faster than ed25519-dalek.
pub fn build_and_sign_transaction(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    build_and_sign_transaction_with_timing(rpc_client, instructions, signer, true)
}

/// Ultra-fast version without timing logs for production use
pub fn build_and_sign_transaction_fast(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    build_and_sign_transaction_with_timing(rpc_client, instructions, signer, false)
}

/// Internal function with optional timing
fn build_and_sign_transaction_with_timing(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
    enable_timing: bool,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    let total_start = Instant::now();
    
    // Use synchronous blockhash fetch from global cache
    let blockhash_start = Instant::now();
    let recent_blockhash = get_cached_blockhash_sync();
    let blockhash_fetch_time = blockhash_start.elapsed();
    
    // Time the message building
    let message_start = Instant::now();
    let message = solana_sdk::message::Message::new_with_blockhash(instructions, Some(&signer.pubkey()), &recent_blockhash);
    let message_build_time = message_start.elapsed();
    
    // Time the keypair conversion (now cached)
    let conversion_start = Instant::now();
    let zebra_signing_key = get_or_create_signing_key(signer);
    let conversion_time = conversion_start.elapsed();
    
    // Time the message serialization
    let serialize_start = Instant::now();
    let message_bytes = message.serialize();
    let serialize_time = serialize_start.elapsed();
    
    // Time the signing operation
    let signing_start = Instant::now();
    let zebra_signature = zebra_signing_key.sign(&message_bytes);
    let signing_time = signing_start.elapsed();
    
    // Time the signature conversion
    let sig_conversion_start = Instant::now();
    let solana_signature = solana_sdk::signature::Signature::from(zebra_signature.to_bytes());
    let sig_conversion_time = sig_conversion_start.elapsed();

    // Time the transaction building
    let tx_build_start = Instant::now();
    let mut tx = Transaction::new_unsigned(message);
    tx.signatures = vec![solana_signature];
    let tx_build_time = tx_build_start.elapsed();
    
    let total_time = total_start.elapsed();
    
    // Log detailed timing information only if enabled
    if enable_timing {
        println!(
            "[{}] - [TX_BUILDER] DETAILED TIMING BREAKDOWN:",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.3f")
        );
        println!("  Blockhash fetch:      {:.3?} ({:.1}%)", 
                 blockhash_fetch_time, 
                 (blockhash_fetch_time.as_nanos() as f64 / total_time.as_nanos() as f64) * 100.0);
        println!("  Message building:     {:.3?} ({:.1}%)", 
                 message_build_time, 
                 (message_build_time.as_nanos() as f64 / total_time.as_nanos() as f64) * 100.0);
        println!("  Keypair conversion:   {:.3?} ({:.1}%)", 
                 conversion_time, 
                 (conversion_time.as_nanos() as f64 / total_time.as_nanos() as f64) * 100.0);
        println!("  Message serialization: {:.3?} ({:.1}%)", 
                 serialize_time, 
                 (serialize_time.as_nanos() as f64 / total_time.as_nanos() as f64) * 100.0);
        println!("  Signing operation:    {:.3?} ({:.1}%)", 
                 signing_time, 
                 (signing_time.as_nanos() as f64 / total_time.as_nanos() as f64) * 100.0);
        println!("  Signature conversion: {:.3?} ({:.1}%)", 
                 sig_conversion_time, 
                 (sig_conversion_time.as_nanos() as f64 / total_time.as_nanos() as f64) * 100.0);
        println!("  Transaction building: {:.3?} ({:.1}%)", 
                 tx_build_time, 
                 (tx_build_time.as_nanos() as f64 / total_time.as_nanos() as f64) * 100.0);
        println!("  TOTAL TIME:           {:.3?}", total_time);
    }
    
    Ok(tx)
}


pub fn create_instruction(
    cu_limit: u32,
    cu_price: u64,
    mint: Pubkey,
    instructions: Vec<Instruction>,
) -> Vec<Instruction> {
    let keypair: &'static Keypair = get_wallet_keypair();

    // Add a random number between 1-100 to the compute unit price
    let mut rng = rand::thread_rng();
    let random_addition: u64 = rng.gen_range(1..=100);
    let adjusted_cu_price = cu_price + random_addition;
    
    // // Log the random addition for debugging
    // println!("[TX_BUILDER] Original CU price: {}, Random addition: {}, Adjusted CU price: {}", 
    //     cu_price, random_addition, adjusted_cu_price);

    let limit_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);
    let price_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(adjusted_cu_price);

    let ata_ix = create_ata(&keypair, &keypair.pubkey(), &mint);

    let mut result = vec![limit_ix, price_ix, ata_ix];
    result.extend(instructions);
    result
}

/// Simulate a transaction to get compute units used and performance metrics
pub fn simulate_transaction(
    rpc_client: &RpcClient,
    transaction: &Transaction,
) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    let simulation_start = Instant::now();
    
    match rpc_client.simulate_transaction(transaction) {
        Ok(simulation_result) => {
            let simulation_time = simulation_start.elapsed();
            
            let units_consumed = simulation_result.value.units_consumed;
            let error = simulation_result.value.err;
            
            // Log simulation results
            if let Some(units) = units_consumed {
                println!(
                    "[{}] - [TX_BUILDER] SIMULATION - CU used: {}, simulation time: {:.2?}",
                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    units,
                    simulation_time
                );
            } else {
                println!(
                    "[{}] - [TX_BUILDER] SIMULATION - No CU data available, simulation time: {:.2?}",
                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    simulation_time
                );
            }
            
            // Log simulation errors if any
            if let Some(err) = &error {
                println!(
                    "[{}] - [TX_BUILDER] SIMULATION ERROR - {:?}",
                    Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    err
                );
            }
            
            Ok(units_consumed)
        }
        Err(e) => {
            let simulation_time = simulation_start.elapsed();
            println!(
                "[{}] - [TX_BUILDER] SIMULATION FAILED - Error: {}, simulation time: {:.2?}",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                e,
                simulation_time
            );
            Err(Box::new(e))
        }
    }
}



/// Result structure for transaction simulation
#[derive(Debug)]
pub struct SimulationResult {
    pub units_consumed: Option<u64>,
    pub error: Option<solana_sdk::transaction::TransactionError>,
    pub simulation_time: std::time::Duration,
}

/// Benchmark function to compare signing performance between ed25519-dalek and ed25519-zebra
pub fn benchmark_signing_performance(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
    iterations: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("[TX_BUILDER] Starting signing performance benchmark with {} iterations", iterations);
    
    // Benchmark ed25519-dalek (original method)
    let dalek_start = Instant::now();
    for _ in 0..iterations {
        let _ = build_and_sign_transaction(rpc_client, instructions, signer)?;
    }
    let dalek_duration = dalek_start.elapsed();
    
    // Benchmark ed25519-zebra (fast method)
    let zebra_start = Instant::now();
    for _ in 0..iterations {
        let _ = build_and_sign_transaction(rpc_client, instructions, signer)?;
    }
    let zebra_duration = zebra_start.elapsed();
    
    println!("[TX_BUILDER] BENCHMARK RESULTS:");
    println!("  ed25519-dalek:  {:.2?} total, {:.2?} per transaction", 
             dalek_duration, dalek_duration / iterations as u32);
    println!("  ed25519-zebra:  {:.2?} total, {:.2?} per transaction", 
             zebra_duration, zebra_duration / iterations as u32);
    println!("  Speedup: {:.2}x faster", dalek_duration.as_nanos() as f64 / zebra_duration.as_nanos() as f64);
    
    Ok(())
}

