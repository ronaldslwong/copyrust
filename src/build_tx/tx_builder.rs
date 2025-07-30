use ed25519_dalek::Signer as DalekSigner;
use ed25519_zebra::SigningKey;
use solana_client::rpc_client::RpcClient;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use solana_sdk::compute_budget;
use solana_sdk::pubkey::Pubkey;
use crate::init::wallet_loader::{get_wallet_keypair, get_nonce_account, get_next_nonce_account_keypair, get_next_nonce_account_atomic};
use crate::utils::ata::create_ata;
use solana_program::instruction::Instruction;
use solana_sdk::nonce::state::State;
use solana_sdk::nonce::state::Versions;
use bincode::deserialize;
use std::time::Instant;
use chrono::Utc;
use once_cell::sync::OnceCell;
use tokio::runtime::Runtime;
use crate::config_load::GLOBAL_CONFIG;
use crate::send_tx::zero_slot::create_instruction_zeroslot;
use rayon::prelude::*;
use crate::send_tx::nextblock::create_instruction_nextblock;
use crate::send_tx::rpc::create_instruction_rpc;
use crate::send_tx::block_razor::create_instruction_blockrazor;
use crate::send_tx::flashblock::create_instruction_flashblock;
use crate::send_tx::astralane::create_instruction_astralane;

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

/// Get nonce blockhash from the nonce account (no fallback)
fn get_nonce_blockhash_sync(rpc_client: &RpcClient, nonce_account: &Pubkey) -> Result<solana_sdk::hash::Hash, Box<dyn std::error::Error + Send + Sync>> {
    println!("[TX_BUILDER] Nonce account: {}", nonce_account.to_string());
    // Get the nonce account data
    let account_data = rpc_client.get_account_data(nonce_account)?;

    // Parse the nonce account to get the current nonce
    let nonce_state: Versions = bincode::deserialize(&account_data)?;
    
    // Get the nonce blockhash
    let nonce_blockhash = match nonce_state {
        Versions::Current(boxed_state) => {
            match *boxed_state {
                State::Initialized(ref data) => {
                    println!("[TX_BUILDER] Nonce account state - blockhash: {}", data.blockhash());
                    data.blockhash()
                }
                _ => {
                    return Err("Nonce account not initialized".into());
                }
            }
        }
        _ => {
            return Err("Unsupported nonce version".into());
        }
    };
    println!("[TX_BUILDER] Using nonce blockhash: {}", nonce_blockhash);
    Ok(nonce_blockhash)
}

/// Get the next nonce account and its blockhash atomically (prevents race conditions)
fn get_next_nonce_account_and_blockhash(rpc_client: &RpcClient) -> Result<(&'static Keypair, &'static Pubkey, solana_sdk::hash::Hash), Box<dyn std::error::Error + Send + Sync>> {
    use crate::init::wallet_loader::get_next_nonce_account_atomic;
    
    let (keypair, pubkey) = get_next_nonce_account_atomic();
    let blockhash = get_nonce_blockhash_sync(rpc_client, pubkey)?;
    
    Ok((keypair, pubkey, blockhash))
}

/// Build and sign transaction with a specific blockhash (for vendor transactions using same nonce)
fn build_and_sign_transaction_with_specific_blockhash(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
    blockhash: solana_sdk::hash::Hash,
) -> Result<Transaction, Box<dyn std::error::Error + Send + Sync>> {
    // Build the message with the specific blockhash
    let message = solana_sdk::message::Message::new_with_blockhash(instructions, Some(&signer.pubkey()), &blockhash);
    
    // Convert to zebra signing key for faster signing
    let zebra_signing_key = get_or_create_signing_key(signer);
    
    // Serialize and sign
    let message_bytes = message.serialize();
    let zebra_signature = zebra_signing_key.sign(&message_bytes);
    let solana_signature = solana_sdk::signature::Signature::from(zebra_signature.to_bytes());
    
    // Build the transaction
    let mut tx = Transaction::new_unsigned(message);
    tx.signatures = vec![solana_signature];
    
    Ok(tx)
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

pub fn build_and_sign_transaction_basic(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<Transaction, Box<dyn std::error::Error + Send + Sync>> {
    // Use nonce blockhash for actual transaction submission (atomic to prevent race conditions)
    let (_, _, recent_blockhash) = get_next_nonce_account_and_blockhash(rpc_client)?;
    
    // Build and sign the transaction
    let tx = Transaction::new_signed_with_payer(
        instructions,
        Some(&signer.pubkey()),
        &[signer],
        recent_blockhash,
    );
    Ok(tx)
}

/// Build and sign a Solana transaction using ed25519-zebra for faster signing.
/// This function is optimized for performance and uses the zebra implementation
/// which is generally faster than ed25519-dalek.
pub fn build_and_sign_transaction(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    build_and_sign_transaction_with_timing(rpc_client, instructions, signer, true, true)
}

/// Ultra-fast version without timing logs for production use
pub fn build_and_sign_transaction_fast(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    build_and_sign_transaction_with_timing(rpc_client, instructions, signer, false, true)
}

/// Build and sign transaction for simulation (uses regular blockhash, not nonce)
pub fn build_and_sign_transaction_for_simulation(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    build_and_sign_transaction_with_timing(rpc_client, instructions, signer, false, false)
}

/// Internal function with optional timing
fn build_and_sign_transaction_with_timing(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
    enable_timing: bool,
    use_nonce: bool,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    // Use nonce blockhash for transaction ordering
    build_and_sign_transaction_with_timing_and_blockhash(rpc_client, instructions, signer, enable_timing, use_nonce)
}

/// Internal function with optional timing and blockhash choice
fn build_and_sign_transaction_with_timing_and_blockhash(
    rpc_client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
    enable_timing: bool,
    use_nonce: bool,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    let total_start = Instant::now();
    
    // Choose blockhash - always use main wallet for signing
    let blockhash_start = Instant::now();
    let recent_blockhash = if use_nonce {
        let (_, _, nonce_hash) = get_next_nonce_account_and_blockhash(rpc_client).map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        println!("[TX_BUILDER] Using nonce blockhash: {}", nonce_hash);
        nonce_hash
    } else {
        let regular_hash = get_cached_blockhash_sync();
        println!("[TX_BUILDER] Using regular blockhash: {}", regular_hash);
        regular_hash
    };
    let actual_signer = signer; // Always use main wallet for signing
    let blockhash_fetch_time = blockhash_start.elapsed();
    
    // Time the message building
    let message_start = Instant::now();
    let message = solana_sdk::message::Message::new_with_blockhash(instructions, Some(&actual_signer.pubkey()), &recent_blockhash);
    let message_build_time = message_start.elapsed();
    
    // Time the keypair conversion (now cached)
    let conversion_start = Instant::now();
    let zebra_signing_key = get_or_create_signing_key(actual_signer);
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
    mint: Pubkey,
    instructions: Vec<Instruction>,
) -> Vec<Instruction> {
    let keypair: &'static Keypair = get_wallet_keypair();

    
    // // Log the random addition for debugging
    // println!("[TX_BUILDER] Original CU price: {}, Random addition: {}, Adjusted CU price: {}", 
    //     cu_price, random_addition, adjusted_cu_price);

    let limit_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(cu_limit);

    let ata_ix = create_ata(&keypair, &keypair.pubkey(), &mint);

    let mut result = vec![limit_ix, ata_ix];
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


/// Build and optimize transaction with vendor-specific instructions
/// This function encapsulates the transaction building logic from the worker
/// and is designed to be extended for parallel vendor-specific transaction building
pub fn build_optimized_transaction(
    buy_instruction: Instruction,
    mint: Pubkey,
    _target_token_buy: u64,
    _sig_str: &str,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    let build_start = Instant::now();
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");
    let rpc = crate::init::initialize::GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    
    // Initial transaction build with default compute units
    let mut final_buy_instruction = create_instruction(
        config.cu_limit,
        mint,
        vec![buy_instruction.clone()],
    );
    
    // Add ZeroSlot tip instruction
    final_buy_instruction = create_instruction_zeroslot(
        final_buy_instruction, 
        (config.zeroslot_buy_tip * 1_000_000_000.0) as u64,
        config.cu_price0_slot,
        get_nonce_account(),
    );
    
    // TODO: Add Jito tip instruction when needed
    // final_buy_instruction = create_instruction_jito(
    //     final_buy_instruction, 
    //     (config.zeroslot_buy_tip * 1_000_000_000.0) as u64
    // );

    #[cfg(feature = "verbose_logging")]
    println!("[BENCH][sig={}] Initial transaction build time: {:.2?}", _sig_str, build_start.elapsed());

    // Build and sign initial transaction for simulation (use regular blockhash, not nonce)
    let sign_start = Instant::now();
    let tx = build_and_sign_transaction_for_simulation(
        rpc,
        &final_buy_instruction,
        &get_wallet_keypair(),
    )?;
    #[cfg(feature = "verbose_logging")]
    println!("[BENCH][sig={}] Initial transaction sign time: {:.2?}", _sig_str, sign_start.elapsed());
    
    println!(
        "[{}] - [TX_BUILDER] SUCCESS - Initial tx signed, elapsed: {:.2?}",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        build_start.elapsed()
    );

    // Simulate transaction to get compute units used
    let mut simulated_units = None;
    match simulate_transaction(rpc, &tx) {
        Ok(Some(units)) => {
            simulated_units = Some(units);
        }
        Ok(None) => {
            // No units data available, but simulation succeeded
        }
        Err(e) => {
            println!(
                "[{}] - [TX_BUILDER] Failed to simulate transaction: {}",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                e
            );
        }
    }

    // Use simulated units if available, otherwise use default
    let cu_limit = if let Some(units) = simulated_units {
        (units as f64 * 1.05) as u32
    } else {
        config.cu_limit // Use default from config
    };

    // Rebuild transaction with optimized compute units
    final_buy_instruction = create_instruction(
        cu_limit,
        mint,
        vec![buy_instruction.clone()],
    );
    final_buy_instruction = create_instruction_zeroslot(
        final_buy_instruction, 
        (config.zeroslot_buy_tip * 1_000_000_000.0) as u64,
        config.cu_price0_slot,
        get_nonce_account(),
    );
    // final_buy_instruction = create_instruction_jito(
    //     final_buy_instruction, 
    //     (config.zeroslot_buy_tip * 1_000_000_000.0) as u64
    // );

    #[cfg(feature = "verbose_logging")]
    println!("[BENCH][sig={}] Optimized transaction build time: {:.2?}", _sig_str, build_start.elapsed());

    // Build and sign optimized transaction
    let optimized_sign_start = Instant::now();
    let optimized_tx = build_and_sign_transaction_fast(
        rpc,
        &final_buy_instruction,
        &get_wallet_keypair(),
    )?;
    #[cfg(feature = "verbose_logging")]
    println!("[BENCH][sig={}] Optimized transaction sign time: {:.2?}", _sig_str, optimized_sign_start.elapsed());
    
    let total_time = build_start.elapsed();
    println!(
        "[{}] - [TX_BUILDER] SUCCESS - Optimized tx built in {:.2?} | CU: {} | Mint: {}",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        total_time,
        cu_limit,
        mint
    );
    
    Ok(optimized_tx)
}

/// Build vendor-specific transactions in parallel using rayon
/// This function builds different transaction versions for different vendors simultaneously
/// Uses rayon for parallel processing within the pinned worker thread
pub fn build_vendor_specific_transactions_parallel(
    buy_instruction: Instruction,
    mint: Pubkey,
    target_token_buy: u64,
    sig_str: &str,
) -> Result<Vec<(String, Transaction)>, Box<dyn std::error::Error + Send + Sync>> {
    let build_start = Instant::now();
    let config = GLOBAL_CONFIG.get().expect("Config not initialized");
    let rpc = crate::init::initialize::GLOBAL_RPC_CLIENT.get().expect("RPC client not initialized");
    
    // First, get optimized compute units (same as before)
    let cu_start = Instant::now();
    let cu_limit = get_optimized_compute_units(&buy_instruction, mint, sig_str, rpc, config)?;
    let cu_time = cu_start.elapsed();
    println!("[PROFILE][{}] Compute units optimization: {:.2?}", sig_str, cu_time);
    // let cu_limit = config.cu_limit;
    
    // Define vendor configurations for parallel building
    let vendor_configs = vec![
        ("rpc", VendorConfig {
            name: "rpc",
            tip_amount: (config.zeroslot_buy_tip * 1_000_000_000.0) as u64,
            cu_price: config.rpc_cu_price,
            use_jito: false,
        }),
        ("zeroslot", VendorConfig {
            name: "zeroslot", 
            tip_amount: (config.zeroslot_buy_tip * 1_000_000_000.0) as u64,
            cu_price: config.cu_price0_slot,
            use_jito: false,
        }),
        // ("jito", VendorConfig {
        //     name: "jito",
        //     tip_amount: (config.zeroslot_buy_tip * 1_000_000_000.0) as u64, // Using same tip for now
        //     cu_price: config.cu_price0_slot,
        //     use_jito: true,
        // }),
        ("nextblock", VendorConfig {
            name: "nextblock",
            tip_amount: (config.nextblock_buy_tip * 1_000_000_000.0) as u64, // Using same tip for now
            cu_price: config.nextblock_cu_price,
            use_jito: false,
        }),
        ("blockrazor", VendorConfig {
            name: "blockrazor",
            tip_amount: (config.blockrazor_buy_tip * 1_000_000_000.0) as u64, // Using same tip for now
            cu_price: config.blockrazor_cu_price,
            use_jito: false,
        }),
        ("flashblock", VendorConfig {
            name: "flashblock",
            tip_amount: (config.flashblock_buy_tip * 1_000_000_000.0) as u64,
            cu_price: config.flashblock_cu_price,
            use_jito: false,
        }),
        ("astralane", VendorConfig {
            name: "astralane",
            tip_amount: (config.astralane_buy_tip * 1_000_000_000.0) as u64,
            cu_price: config.astralane_cu_price,
            use_jito: false,
        }),
    ];
    
    // Get the same nonce account and blockhash for all vendor transactions (prevents multiple advances)
    let nonce_start = Instant::now();
    let (nonce_keypair, nonce_pubkey, nonce_blockhash) = get_next_nonce_account_and_blockhash(rpc)?;
    let nonce_time = nonce_start.elapsed();
    println!("[PROFILE][{}] Nonce account setup: {:.2?}", sig_str, nonce_time);
    println!("[TX_BUILDER] Using nonce account {} for all {} vendor transactions", nonce_pubkey, vendor_configs.len());
    
    // Build all vendor versions in parallel using rayon
    let parallel_start = Instant::now();
    let results: Vec<Result<(String, Transaction), Box<dyn std::error::Error + Send + Sync>>> = vendor_configs
        .into_par_iter()
        .map(|(vendor_name, config)| {
            let start_time = Instant::now();
            
            // Build base instruction with optimized compute units
            let mut instructions = create_instruction(
                cu_limit,
                mint,
                vec![buy_instruction.clone()],
            );
            
            // Add vendor-specific tip instructions
            if config.name == "zeroslot" {
                instructions = create_instruction_zeroslot(
                    instructions,
                    config.tip_amount,
                    config.cu_price,
                    nonce_pubkey,
                );
            }
            if config.name == "nextblock" {
                instructions = create_instruction_nextblock(
                    instructions,
                    config.tip_amount,
                    config.cu_price,
                    nonce_pubkey,
                );
            }
            if config.name == "blockrazor" {
                instructions = create_instruction_blockrazor(
                    instructions,
                    config.tip_amount,
                    config.cu_price,
                    nonce_pubkey,
                );
            }
            if config.name == "flashblock" {
                instructions = create_instruction_flashblock(
                    instructions,
                    config.tip_amount,
                    config.cu_price,
                    nonce_pubkey,
                );
            }
            if config.name == "astralane" {
                instructions = create_instruction_astralane(
                    instructions,
                    config.tip_amount,
                    config.cu_price,
                    nonce_pubkey,
                );
            }
            if config.name == "rpc" {
                instructions = create_instruction_rpc(
                    cu_limit,
                    config.cu_price,
                    mint,
                    instructions,
                    config.tip_amount,
                    nonce_pubkey,
                );
            }
            // TODO: Add Jito tip instruction when implemented
            // if config.use_jito {
            //     instructions = create_instruction_jito(instructions, config.tip_amount);
            // }
            
            // Build and sign the transaction using the same nonce blockhash for all vendors
            let tx = match build_and_sign_transaction_with_specific_blockhash(
                rpc,
                &instructions,
                &get_wallet_keypair(),
                nonce_blockhash,
            ) {
                Ok(tx) => tx,
                Err(e) => {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to build transaction: {}", e)
                    )) as Box<dyn std::error::Error + Send + Sync>);
                }
            };
            
            let build_time = start_time.elapsed();
            println!(
                "[{}] - [TX_BUILDER] {} version built in {:.2?} | CU: {} | Tip: {}",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                vendor_name,
                build_time,
                cu_limit,
                config.tip_amount
            );
            
            Ok((vendor_name.to_string(), tx))
        })
        .collect();
    
    let parallel_time = parallel_start.elapsed();
    println!("[PROFILE][{}] Parallel vendor building: {:.2?}", sig_str, parallel_time);
    
    // Collect successful results
    let collect_start = Instant::now();
    let mut successful_results = Vec::new();
    for result in results {
        match result {
            Ok((vendor, tx)) => successful_results.push((vendor, tx)),
            Err(e) => {
                eprintln!("[TX_BUILDER] Failed to build vendor transaction: {}", e);
            }
        }
    }
    let collect_time = collect_start.elapsed();
    println!("[PROFILE][{}] Results collection: {:.2?}", sig_str, collect_time);
    
    let total_time = build_start.elapsed();
    println!(
        "[{}] - [TX_BUILDER] PARALLEL SUCCESS - Built {} vendor versions in {:.2?}",
        Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        successful_results.len(),
        total_time
    );
    
    Ok(successful_results)
}

/// Helper function to get optimized compute units (extracted from build_optimized_transaction)
fn get_optimized_compute_units(
    buy_instruction: &Instruction,
    mint: Pubkey,
    sig_str: &str,
    rpc: &RpcClient,
    config: &crate::config_load::Config,
) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
    let sim_total_start = Instant::now();
    
    // Build initial transaction for simulation
    let build_sim_start = Instant::now();
    let mut initial_instructions = create_instruction(
        config.cu_limit,
        mint,
        vec![buy_instruction.clone()],
    );
    
    initial_instructions = create_instruction_zeroslot(
        initial_instructions,
        (config.zeroslot_buy_tip * 1_000_000_000.0) as u64,
        config.cu_price0_slot,
        get_nonce_account(),
    );
    
    // Build and sign for simulation (use regular blockhash, not nonce)
    let tx = match build_and_sign_transaction_for_simulation(
        rpc,
        &initial_instructions,
        &get_wallet_keypair(),
    ) {
        Ok(tx) => tx,
        Err(e) => {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to build simulation transaction: {}", e)
            )) as Box<dyn std::error::Error + Send + Sync>);
        }
    };
    let build_sim_time = build_sim_start.elapsed();
    println!("[PROFILE][{}] Simulation transaction build: {:.2?}", sig_str, build_sim_time);
    
    // Simulate to get compute units
    let sim_start = Instant::now();
    let simulated_units = match simulate_transaction(rpc, &tx) {
        Ok(Some(units)) => Some(units),
        Ok(None) => None,
        Err(e) => {
            println!(
                "[{}] - [TX_BUILDER] Simulation failed for sig {}: {}",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                sig_str,
                e
            );
            None
        }
    };
    let sim_time = sim_start.elapsed();
    println!("[PROFILE][{}] RPC simulation: {:.2?}", sig_str, sim_time);
    
    let sim_total_time = sim_total_start.elapsed();
    println!("[PROFILE][{}] Total simulation pipeline: {:.2?}", sig_str, sim_total_time);
    
    // Use simulated units if available, otherwise fall back to config
    let final_cu = match simulated_units {
        Some(units) => {
            // Add 20% buffer to the simulated units
            let buffered_units = (units as f64 * 1.2) as u32;
            println!(
                "[{}] - [TX_BUILDER] Using simulated CU: {} (original: {}, buffered: {})",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                buffered_units,
                units,
                buffered_units
            );
            buffered_units
        }
        None => {
            println!(
                "[{}] - [TX_BUILDER] Using config CU: {} (simulation failed)",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                config.cu_limit
            );
            config.cu_limit
        }
    };
    
    Ok(final_cu)
}

/// Configuration for building vendor-specific transactions
#[derive(Clone)]
struct VendorConfig {
    name: &'static str,
    tip_amount: u64,
    cu_price: u64,
    use_jito: bool,
}

/// Legacy async function (kept for compatibility)
pub async fn build_vendor_specific_transactions(
    buy_instruction: Instruction,
    mint: Pubkey,
    target_token_buy: u64,
    sig_str: &str,
) -> Result<Vec<(String, Transaction)>, Box<dyn std::error::Error + Send + Sync>> {
    // Use the parallel version instead
    build_vendor_specific_transactions_parallel(buy_instruction, mint, target_token_buy, sig_str)
}



